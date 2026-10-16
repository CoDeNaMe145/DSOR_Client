//! Remove conversion-only branches and unused skin palette entries at load time.
//! Bone transforms/animation tracks stay intact; only vertex palette indices and
//! their matching inverse bind matrices change.

use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};

pub fn compact_glb(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let mut doc: Value =
        serde_json::from_slice(&bytes[20..20 + json_len]).map_err(|e| e.to_string())?;
    restore_character_culling(&mut doc);
    let mut binary = gltf.blob.clone().unwrap_or_default();
    let buffer = |b: gltf::Buffer<'_>| (b.index() == 0).then_some(gltf.blob.as_deref()).flatten();
    // An external/multiple buffer asset is outside this conversion contract.
    if gltf
        .buffers()
        .any(|b| b.index() != 0 || !matches!(b.source(), gltf::buffer::Source::Bin))
    {
        return Ok(bytes.to_vec());
    }
    let mut palettes = HashMap::new();
    for mesh in gltf.meshes() {
        let skins: Vec<_> = gltf
            .nodes()
            .filter(|n| n.mesh().is_some_and(|m| m.index() == mesh.index()))
            .filter_map(|n| n.skin())
            .collect();
        if skins.is_empty() {
            continue;
        }
        let mut used = BTreeSet::new();
        let mut primitives = Vec::new();
        for primitive in mesh.primitives() {
            // Keep unsupported extra influence sets untouched rather than lose weights.
            if primitive.attributes().any(|(s, _)| matches!(s, gltf::Semantic::Joints(n) | gltf::Semantic::Weights(n) if n != 0)) {
                primitives.clear();
                break;
            }
            let reader = primitive.reader(buffer);
            let (Some(joints), Some(weights)) = (reader.read_joints(0), reader.read_weights(0))
            else {
                primitives.clear();
                break;
            };
            let joints: Vec<_> = joints.into_u16().collect();
            let weights: Vec<_> = weights.into_f32().collect();
            if joints.len() != weights.len() {
                return Err("skin joint/weight count mismatch".into());
            }
            for (js, ws) in joints.iter().zip(&weights) {
                for (&j, &w) in js.iter().zip(ws) {
                    if w != 0.0 {
                        used.insert(j as usize);
                    }
                }
            }
            primitives.push((primitive.index(), joints, weights));
        }
        if primitives.is_empty() || used.is_empty() {
            continue;
        }
        if skins
            .iter()
            .any(|s| used.last().copied().unwrap() >= s.joints().len())
        {
            return Err("weighted joint outside skin palette".into());
        }
        if skins.iter().all(|s| used.len() == s.joints().len()) {
            continue;
        }
        let palette: Vec<_> = used.into_iter().collect();
        let remap: HashMap<_, _> = palette
            .iter()
            .enumerate()
            .map(|(new, &old)| (old, new as u16))
            .collect();
        for (i, joints, weights) in primitives {
            let mut data = Vec::with_capacity(joints.len() * 8);
            for (js, ws) in joints.iter().zip(weights) {
                for (&j, w) in js.iter().zip(ws) {
                    // Zero-weight lanes must still contain an in-range index: the
                    // vertex shader fetches all four matrices before multiplying.
                    let index = if w == 0.0 { 0 } else { remap[&(j as usize)] };
                    data.extend_from_slice(&index.to_le_bytes());
                }
            }
            let accessor = append_accessor(
                &mut doc,
                &mut binary,
                &data,
                5123,
                "VEC4",
                joints.len(),
                Some(34962),
            );
            doc["meshes"][mesh.index()]["primitives"][i]["attributes"]["JOINTS_0"] =
                accessor.into();
        }
        palettes.insert(mesh.index(), palette);
    }
    let mut skins = HashMap::new();
    for node in gltf.nodes() {
        let (Some(mesh), Some(skin)) = (node.mesh(), node.skin()) else {
            continue;
        };
        let Some(palette) = palettes.get(&mesh.index()) else {
            continue;
        };
        let key = (skin.index(), palette.clone());
        let new_skin = *skins.entry(key).or_insert_with(|| {
            let joints: Vec<_> = skin.joints().map(|j| j.index()).collect();
            let matrices: Vec<_> = skin
                .reader(buffer)
                .read_inverse_bind_matrices()
                .map(|m| m.collect())
                .unwrap_or_else(|| {
                    vec![
                        [
                            [1., 0., 0., 0.],
                            [0., 1., 0., 0.],
                            [0., 0., 1., 0.],
                            [0., 0., 0., 1.]
                        ];
                        joints.len()
                    ]
                });
            let data: Vec<_> = palette
                .iter()
                .flat_map(|&i| matrices[i].iter().flatten().flat_map(|f| f.to_le_bytes()))
                .collect();
            let accessor = append_accessor(
                &mut doc,
                &mut binary,
                &data,
                5126,
                "MAT4",
                palette.len(),
                None,
            );
            let mut compact = doc["skins"][skin.index()].clone();
            compact["joints"] = palette.iter().map(|&i| Value::from(joints[i])).collect();
            compact["inverseBindMatrices"] = accessor.into();
            let all = doc["skins"].as_array_mut().unwrap();
            let index = all.len();
            all.push(compact);
            index
        });
        doc["nodes"][node.index()]["skin"] = new_skin.into();
    }
    prune_empty_model_branches(&mut doc);
    // The original skin entries remain valid metadata. Only node-referenced
    // palettes are instantiated; keeping indices stable preserves animation rigs.
    if !binary.is_empty() {
        doc["buffers"][0]["byteLength"] = binary.len().into();
    }
    let mut json = serde_json::to_vec(&doc).map_err(|e| e.to_string())?;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let mut out = Vec::new();
    for word in [
        0x46546c67u32,
        2,
        (20 + json.len()
            + if binary.is_empty() {
                0
            } else {
                8 + binary.len()
            }) as u32,
        json.len() as u32,
        0x4e4f534a,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&json);
    if !binary.is_empty() {
        out.extend_from_slice(&(binary.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004e4942u32.to_le_bytes());
        out.extend_from_slice(&binary);
    }
    Ok(out)
}

/// Compatibility for converted assets written before character render states
/// were restored. DSO_Godot defaulted every material to double-sided. The shipped
/// character/characterfalloff/monster effects instead default CullMode to 2
/// (D3DCULL_CW: back faces for the exported counter-clockwise front winding).
/// See tools/character_render_states.py; explicit per-node overrides win.
fn restore_character_culling(doc: &mut Value) {
    let mut modes = vec![HashSet::new(); doc["materials"].as_array().map_or(0, Vec::len)];
    for node in doc["nodes"].as_array().into_iter().flatten() {
        let Some(mesh) = node["mesh"].as_u64() else { continue };
        for primitive in doc["meshes"][mesh as usize]["primitives"].as_array().into_iter().flatten() {
            let Some(material) = primitive["material"].as_u64().map(|v| v as usize) else { continue };
            let Some(entries) = modes.get_mut(material) else { continue };
            let extras = &doc["materials"][material]["extras"];
            if !matches!(extras["nebula_shader"].as_str(), Some("shd:character" | "shd:characterfalloff" | "shd:monster")) {
                continue;
            }
            let mode = node.pointer("/extras/dsor_ints/CullMode").and_then(Value::as_i64)
                .or_else(|| extras["dsor_cull_mode"].as_i64()).unwrap_or(2);
            entries.insert(mode);
        }
    }
    for (index, modes) in modes.into_iter().enumerate() {
        // Front-face/unknown modes cannot be represented by doubleSided alone.
        // Keep them unchanged. A shared material with any two-sided user must
        // remain two-sided, retaining all authored surfaces.
        if modes.is_empty() || modes.iter().any(|v| !matches!(v, 1 | 2)) { continue; }
        let double_sided = modes.contains(&1);
        doc["materials"][index]["doubleSided"] = double_sided.into();
        doc["materials"][index]["extras"]["dsor_cull_mode"] = Value::from(if double_sided { 1 } else { 2 });
    }
}

fn append_accessor(
    doc: &mut Value,
    binary: &mut Vec<u8>,
    data: &[u8],
    component: u32,
    kind: &str,
    count: usize,
    target: Option<u32>,
) -> usize {
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let mut view = json!({"buffer":0,"byteOffset":binary.len(),"byteLength":data.len()});
    if let Some(target) = target {
        view["target"] = target.into();
    }
    binary.extend_from_slice(data);
    let views = doc["bufferViews"].as_array_mut().unwrap();
    let index = views.len();
    views.push(view);
    let accessors = doc["accessors"].as_array_mut().unwrap();
    let index_accessor = accessors.len();
    accessors.push(json!({"bufferView":index,"componentType":component,"count":count,"type":kind}));
    index_accessor
}

fn prune_empty_model_branches(doc: &mut Value) {
    let mut protected = HashSet::new();
    for skin in doc["skins"].as_array().into_iter().flatten() {
        for joint in skin["joints"].as_array().into_iter().flatten() {
            if let Some(i) = joint.as_u64() {
                protected.insert(i as usize);
            }
        }
        if let Some(i) = skin["skeleton"].as_u64() {
            protected.insert(i as usize);
        }
    }
    for animation in doc["animations"].as_array().into_iter().flatten() {
        for channel in animation["channels"].as_array().into_iter().flatten() {
            if let Some(i) = channel.pointer("/target/node").and_then(Value::as_u64) {
                protected.insert(i as usize);
            }
        }
    }
    let Some(nodes) = doc["nodes"].as_array_mut() else {
        return;
    };
    let mut removed = HashSet::new();
    loop {
        let previous = removed.len();
        for (i, node) in nodes.iter().enumerate() {
            if protected.contains(&i) || removed.contains(&i) {
                continue;
            }
            let Some(fields) = node.as_object() else {
                continue;
            };
            if fields.keys().any(|k| {
                !matches!(
                    k.as_str(),
                    "name" | "children" | "translation" | "rotation" | "scale" | "matrix"
                )
            }) {
                continue;
            }
            let children: Vec<_> = node["children"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .collect();
            // Converted skinned meshes were lifted out of their original model
            // branches. Prune those dead ends only, never arbitrary named sockets.
            if (node["name"] == "model" && children.is_empty())
                || (!children.is_empty()
                    && children.iter().all(|c| removed.contains(&(*c as usize))))
            {
                removed.insert(i);
            }
        }
        if removed.len() == previous {
            break;
        }
    }
    for node in nodes {
        if let Some(children) = node["children"].as_array_mut() {
            children.retain(|c| !removed.contains(&(c.as_u64().unwrap() as usize)));
        }
    }
    for scene in doc["scenes"].as_array_mut().into_iter().flatten() {
        if let Some(nodes) = scene["nodes"].as_array_mut() {
            nodes.retain(|n| !removed.contains(&(n.as_u64().unwrap() as usize)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_authored_character_culling_preserves_overrides_and_other_shaders() {
        let mut doc = json!({
            "materials":[
                {"doubleSided":true,"extras":{"nebula_shader":"shd:character"}},
                {"doubleSided":true,"extras":{"nebula_shader":"shd:characterfalloff"}},
                {"doubleSided":true,"extras":{"nebula_shader":"shd:monster"}},
                {"doubleSided":true,"extras":{"nebula_shader":"shd:standard"}}],
            "meshes":[{"primitives":[{"material":0}]},{"primitives":[{"material":1}]},
                {"primitives":[{"material":2}]},{"primitives":[{"material":3}]}],
            "nodes":[{"mesh":0},{"mesh":1,"extras":{"dsor_ints":{"CullMode":1}}},
                {"mesh":2,"extras":{"dsor_ints":{"CullMode":3}}},{"mesh":3}]});
        restore_character_culling(&mut doc);
        assert_eq!(doc["materials"][0]["doubleSided"], false);
        for i in 1..4 { assert_eq!(doc["materials"][i]["doubleSided"], true); }
        let once = doc.clone();
        restore_character_culling(&mut doc);
        assert_eq!(doc, once);
        doc["nodes"].as_array_mut().unwrap().push(json!({"mesh":0,"extras":{"dsor_ints":{"CullMode":1}}}));
        restore_character_culling(&mut doc);
        assert_eq!(doc["materials"][0]["doubleSided"], true);
    }

    fn fixture() -> Vec<u8> {
        let mut doc = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":0}],"bufferViews":[],"accessors":[],
            "scenes":[{"nodes":[0,4,5,7]}],"scene":0,
            "nodes":[{"name":"rig","children":[1,2,3]}, {"name":"unused"}, {"name":"head"}, {"name":"hand"},
                {"mesh":0,"skin":0},{"name":"old_branch","children":[6]},{"name":"model"},{"name":"fx_socket"}],
            "skins":[{"joints":[0,1,2,3],"skeleton":0}],
            "meshes":[{"primitives":[{"attributes":{}}]}]});
        let mut bin = Vec::new();
        let positions: Vec<u8> = [0f32, 0., 0., 1., 0., 0., 0., 1., 0.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let position =
            append_accessor(&mut doc, &mut bin, &positions, 5126, "VEC3", 3, Some(34962));
        doc["accessors"][position]["min"] = json!([0, 0, 0]);
        doc["accessors"][position]["max"] = json!([1, 1, 0]);
        let joints: Vec<u8> = [2u16, 3, 1, 0, 3, 2, 1, 0, 2, 3, 0, 1]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        let joints = append_accessor(&mut doc, &mut bin, &joints, 5123, "VEC4", 3, Some(34962));
        let weights: Vec<u8> = [0.25f32, 0.75, 0., 0., 0.5, 0.5, 0., 0., 1., 0., 0., 0.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let weights = append_accessor(&mut doc, &mut bin, &weights, 5126, "VEC4", 3, Some(34962));
        doc["meshes"][0]["primitives"][0]["attributes"] =
            json!({"POSITION":position,"JOINTS_0":joints,"WEIGHTS_0":weights});
        doc["buffers"][0]["byteLength"] = bin.len().into();
        let mut json = serde_json::to_vec(&doc).unwrap();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let mut result = Vec::new();
        for n in [
            0x46546c67u32,
            2,
            (28 + json.len() + bin.len()) as u32,
            json.len() as u32,
            0x4e4f534a,
        ] {
            result.extend(n.to_le_bytes());
        }
        result.extend(json);
        result.extend((bin.len() as u32).to_le_bytes());
        result.extend(0x004e4942u32.to_le_bytes());
        result.extend(bin);
        result
    }

    // Equality of each nonzero influence's joint and inverse bind matrix proves
    // the skinned position is unchanged for any animated pose, including blends.
    fn assert_same_influences(before: &gltf::Gltf, after: &gltf::Gltf) -> (usize, usize) {
        let mut counts = (0, 0);
        for (a, b) in before.nodes().zip(after.nodes()) {
            let (Some(old), Some(new), Some(mesh)) = (a.skin(), b.skin(), a.mesh()) else {
                continue;
            };
            let old_joints: Vec<_> = old.joints().map(|n| n.index()).collect();
            let new_joints: Vec<_> = new.joints().map(|n| n.index()).collect();
            counts.0 += old_joints.len();
            counts.1 += new_joints.len();
            let matrices = |skin: gltf::Skin<'_>, blob: &[u8]| {
                skin.reader(|_| Some(blob))
                    .read_inverse_bind_matrices()
                    .map(|m| m.collect::<Vec<_>>())
                    .unwrap_or_else(|| {
                        vec![
                            [
                                [1., 0., 0., 0.],
                                [0., 1., 0., 0.],
                                [0., 0., 1., 0.],
                                [0., 0., 0., 1.]
                            ];
                            skin.joints().len()
                        ]
                    })
            };
            let old_ibm = matrices(old, before.blob.as_deref().unwrap());
            let new_ibm = matrices(new, after.blob.as_deref().unwrap());
            for (p, q) in mesh.primitives().zip(b.mesh().unwrap().primitives()) {
                let p = p.reader(|_| before.blob.as_deref());
                let q = q.reader(|_| after.blob.as_deref());
                for ((js, ks), ws) in p
                    .read_joints(0)
                    .unwrap()
                    .into_u16()
                    .zip(q.read_joints(0).unwrap().into_u16())
                    .zip(p.read_weights(0).unwrap().into_f32())
                {
                    for ((j, k), w) in js.into_iter().zip(ks).zip(ws) {
                        assert!((k as usize) < new_joints.len());
                        if w == 0. {
                            continue;
                        }
                        assert_eq!(old_joints[j as usize], new_joints[k as usize]);
                        assert_eq!(old_ibm[j as usize], new_ibm[k as usize]);
                    }
                }
            }
        }
        counts
    }

    #[test]
    fn compact_palette_preserves_every_weighted_influence_and_named_socket() {
        let bytes = fixture();
        let compact = compact_glb(&bytes).unwrap();
        let before = gltf::Gltf::from_slice(&bytes).unwrap();
        let after = gltf::Gltf::from_slice(&compact).unwrap();
        assert_eq!(assert_same_influences(&before, &after), (4, 2));
        assert_eq!(
            after
                .scenes()
                .next()
                .unwrap()
                .nodes()
                .map(|n| n.index())
                .collect::<Vec<_>>(),
            vec![0, 4, 7]
        );
        assert_eq!(after.nodes().nth(0).unwrap().children().len(), 3);
    }

    #[test]
    fn animated_or_metadata_model_nodes_are_not_pruned() {
        let mut doc = json!({"nodes":[{"name":"model"},{"name":"model","extras":{"socket":true}},{"name":"model"}],
            "scenes":[{"nodes":[0,1,2]}],"skins":[{"joints":[0]}],"animations":[{"channels":[{"target":{"node":2}}]}]});
        prune_empty_model_branches(&mut doc);
        assert_eq!(doc["scenes"][0]["nodes"], json!([0, 1, 2]));
    }

    #[test]
    fn converted_characters_keep_identical_skinning() {
        let Ok(root) = std::env::var("DSOR_VARIANT_TEST_ASSETS") else {
            return;
        };
        let mut paths = Vec::new();
        for skeleton in ["uniskel", "uniskel_dwarf"] {
            let mut parts: Vec<_> =
                std::fs::read_dir(format!("{root}/characters/{skeleton}/parts"))
                    .unwrap()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "glb"))
                    .collect();
            parts.sort();
            paths.extend(parts.into_iter().take(40));
        }
        for model in ["horse", "troll", "skeleton_archer"] {
            paths.push(format!("{root}/characters/{model}.glb").into());
        }
        let mut totals = (0, 0);
        for path in &paths {
            let bytes = std::fs::read(path).unwrap();
            let compact = compact_glb(&bytes).unwrap();
            let before = gltf::Gltf::from_slice(&bytes).unwrap();
            let after = gltf::Gltf::from_slice(&compact).unwrap();
            let counts = assert_same_influences(&before, &after);
            totals.0 += counts.0;
            totals.1 += counts.1;
        }
        assert!(totals.1 < totals.0 / 2, "{totals:?}");
        println!(
            "{} converted models: {} -> {} skin palette entries, identical weighted influences",
            paths.len(),
            totals.0,
            totals.1
        );
    }
}
