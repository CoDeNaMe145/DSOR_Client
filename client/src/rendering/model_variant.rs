//! Select a character set before Bevy loads meshes, materials, textures or scenes.
//!
//! Virtual filenames keep each (model, set) in its own asset cache entry. They
//! use the normal asset source on both native and web; no converted files are
//! required. The source GLB is read temporarily, but only the selected geometry
//! and its textures reach the renderer. Relative texture paths remain shared.

use std::{collections::HashSet, path::Path};

use bevy::asset::io::{AssetReader, AssetReaderError, AssetSource, AssetSourceBuilder, ErasedAssetReader, PathStream, Reader, VecReader};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use serde_json::Value;

const MARKER: &str = ".dsor-set-";
const STATE_MARKER: &str = ".dsor-state-";

pub fn prop_path(graphics: &str, state: &str) -> String {
    let model = crate::world::map::model_path(graphics);
    let encoded: String = state.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    format!("{}{STATE_MARKER}{encoded}.glb", model.trim_end_matches(".glb"))
}

/// Keep the file alive after animation setup, so another instance cannot reload
/// and replace the scene underneath already configured actors.
#[derive(Component)]
pub struct ResidentModel(#[allow(dead_code)] pub Handle<Gltf>);

pub fn path(graphics: &str, set: &str) -> String {
    if set.is_empty() {
        return format!("{graphics}.glb");
    }
    let encoded: String = set.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    format!("{graphics}{MARKER}{encoded}.glb")
}

fn request(path: &Path) -> Option<(String, String)> {
    request_kind(path, MARKER)
}

fn request_kind(path: &Path, marker: &str) -> Option<(String, String)> {
    let (model, encoded) = path.to_str()?.strip_suffix(".glb")?.rsplit_once(marker)?;
    if encoded.len() % 2 != 0 { return None; }
    let bytes: Option<Vec<u8>> = encoded.as_bytes().chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()).collect();
    Some((model.to_owned(), String::from_utf8(bytes?).ok()?))
}

pub fn source(root: String) -> AssetSourceBuilder {
    let mut reader = AssetSource::get_default_reader(root);
    AssetSourceBuilder::new(move || Box::new(VariantReader { inner: reader() }))
}

struct VariantReader {
    inner: Box<dyn ErasedAssetReader>,
}

impl AssetReader for VariantReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<Box<dyn Reader + 'a>, AssetReaderError> {
        if let Some((model, state)) = request_kind(path, STATE_MARKER) {
            let mut bytes = Vec::new();
            self.inner.read(Path::new(&format!("{model}.glb"))).await?.read_to_end(&mut bytes).await?;
            let bytes = rewrite_glb(&bytes, |doc| filter_prop_document(doc, &state)).map_err(std::io::Error::other)?;
            return Ok(Box::new(VecReader::new(bytes)));
        }
        let Some((model, set)) = request(path) else {
            if path.starts_with("characters") && path.extension().is_some_and(|e| e == "glb")
                && path.file_name().is_none_or(|n| n != "__animations.glb") {
                let mut bytes = Vec::new();
                self.inner.read(path).await?.read_to_end(&mut bytes).await?;
                let bytes = crate::rendering::model_compact::compact_glb(&bytes).map_err(std::io::Error::other)?;
                return Ok(Box::new(VecReader::new(bytes)));
            }
            if path.extension().is_some_and(|e| e == "glb") {
                let mut bytes = Vec::new();
                self.inner.read(path).await?.read_to_end(&mut bytes).await?;
                if json_mentions(&bytes, b"shd:envvertexanim") {
                    bytes = rewrite_glb(&bytes, |doc| { drop_wind_weights(doc); Ok(()) }).map_err(std::io::Error::other)?;
                }
                return Ok(Box::new(VecReader::new(bytes)));
            }
            return self.inner.read(path).await;
        };
        let model_path = format!("{model}.glb");
        let sets_path = format!("{model}.sets.json");
        let mut sets = Vec::new();
        self.inner.read(Path::new(&sets_path)).await?.read_to_end(&mut sets).await?;
        let sets: Value = serde_json::from_slice(&sets).map_err(std::io::Error::other)?;
        let skins = sets.get(&set).and_then(|s| s.get("skins")).and_then(Value::as_array)
            .ok_or_else(|| std::io::Error::other(format!("{model}: missing character set {set:?}")))?;
        let skins: HashSet<&str> = skins.iter().filter_map(Value::as_str).collect();
        let mut bytes = Vec::new();
        self.inner.read(Path::new(&model_path)).await?.read_to_end(&mut bytes).await?;
        let bytes = filter_glb(&bytes, &skins).map_err(std::io::Error::other)?;
        let bytes = crate::rendering::model_compact::compact_glb(&bytes).map_err(std::io::Error::other)?;
        Ok(Box::new(VecReader::new(bytes)))
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<Box<dyn Reader + 'a>, AssetReaderError> {
        self.inner.read_meta(path).await
    }

    async fn read_directory<'a>(&'a self, path: &'a Path) -> Result<Box<PathStream>, AssetReaderError> {
        self.inner.read_directory(path).await
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        self.inner.is_directory(path).await
    }
}

fn filter_glb(bytes: &[u8], skins: &HashSet<&str>) -> Result<Vec<u8>, String> {
    rewrite_glb(bytes, |doc| filter_document(doc, skins))
}

/// Whether a GLB's JSON chunk contains `needle` (cheap pre-check before a rewrite).
fn json_mentions(bytes: &[u8], needle: &[u8]) -> bool {
    let len = bytes.get(12..16).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).unwrap_or(0) as usize;
    bytes.get(20..20 + len).is_some_and(|json| json.windows(needle.len()).any(|w| w == needle))
}

/// Nebula's envvertexanim (grass, leaves, bushes) reads its vertex colour only
/// as the wind's wave weight (`v3.y`) and never passes it on: the colour is not
/// tinted. Bevy multiplies the base colour by COLOR_0, which darkened the blades
/// where the weight is low (Kingshill's market grass, weights 0 to 1).
/// EVIDENCE: shaders_sm30 envvertexanim vertex shaders (`dcl_color v3`, only
///   `v3.y` used, no colour output).
fn drop_wind_weights(doc: &mut Value) {
    let wind: Vec<bool> = doc["materials"].as_array().map(|m| m.iter()
        .map(|m| m["extras"]["nebula_shader"].as_str() == Some("shd:envvertexanim")).collect()).unwrap_or_default();
    for mesh in doc["meshes"].as_array_mut().into_iter().flatten() {
        for primitive in mesh["primitives"].as_array_mut().into_iter().flatten() {
            let material = primitive["material"].as_u64().map(|v| v as usize);
            if material.is_some_and(|m| wind.get(m).copied().unwrap_or(false)) {
                if let Some(attributes) = primitive["attributes"].as_object_mut() { attributes.remove("COLOR_0"); }
            }
        }
    }
}

fn rewrite_glb(bytes: &[u8], filter: impl FnOnce(&mut Value) -> Result<(), String>) -> Result<Vec<u8>, String> {
    let word = |at: usize| bytes.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes);
    if word(0) != Some(0x46546c67) || word(4) != Some(2) || word(16) != Some(0x4e4f534a) {
        return Err("expected a GLB 2.0 JSON chunk".into());
    }
    let end = 20 + word(12).ok_or("missing JSON chunk length")? as usize;
    let json = bytes.get(20..end).ok_or("truncated GLB JSON")?;
    let mut doc: Value = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    filter(&mut doc)?;
    let mut json = serde_json::to_vec(&doc).map_err(|e| e.to_string())?;
    while json.len() % 4 != 0 { json.push(b' '); }
    // Keep accessor offsets and the embedded binary chunk intact. Unreferenced
    // accessors do not create mesh/image assets in Bevy's glTF loader.
    let mut out = Vec::with_capacity(20 + json.len() + bytes.len() - end);
    out.extend_from_slice(&bytes[..12]);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4e4f534au32.to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&bytes[end..]);
    let len = out.len() as u32;
    out[8..12].copy_from_slice(&len.to_le_bytes());
    Ok(out)
}

fn index(v: &Value) -> Result<usize, String> {
    v.as_u64().map(|n| n as usize).ok_or_else(|| "invalid glTF index".into())
}

fn remap(v: &mut Value, map: &[Option<usize>]) -> Result<(), String> {
    let old = index(v)?;
    *v = map.get(old).copied().flatten().ok_or_else(|| format!("missing glTF dependency {old}"))?.into();
    Ok(())
}

fn compact(doc: &mut Value, key: &str, used: &HashSet<usize>) -> Vec<Option<usize>> {
    let Some(items) = doc.get_mut(key).and_then(Value::as_array_mut) else { return Vec::new() };
    let mut map = vec![None; items.len()];
    let mut next = 0;
    *items = std::mem::take(items).into_iter().enumerate().filter_map(|(i, item)| {
        if !used.contains(&i) { return None; }
        map[i] = Some(next);
        next += 1;
        Some(item)
    }).collect();
    map
}

fn array_mut<'a>(doc: &'a mut Value, key: &str) -> impl Iterator<Item = &'a mut Value> {
    doc.get_mut(key).and_then(Value::as_array_mut).into_iter().flatten()
}

fn prune_indices(value: &mut Value, key: &str, map: &[Option<usize>]) {
    if let Some(list) = value.get_mut(key).and_then(Value::as_array_mut) {
        *list = list.iter().filter_map(|v| v.as_u64().and_then(|i| map.get(i as usize).copied().flatten()).map(Value::from)).collect();
    }
}

/// Visit standard/extension texture-info indices and our converted material
/// extras. Texture indices in Nebula dye masks/layers must follow the same remap.
fn texture_indices(value: &mut Value, visit: &mut impl FnMut(&mut Value) -> Result<(), String>) -> Result<(), String> {
    if let Some(object) = value.as_object_mut() {
        for (key, value) in object {
            if key.ends_with("Texture") {
                if let Some(i) = value.get_mut("index") { visit(i)?; }
            } else if matches!(key.as_str(), "dsor_dye" | "dsor_layer" | "dsor_decal" | "dsor_decal_effect") {
                for field in ["color", "mask", "glow"] {
                    if let Some(i) = value.get_mut(field) { visit(i)?; }
                }
            } else {
                texture_indices(value, visit)?;
            }
        }
    } else if let Some(array) = value.as_array_mut() {
        for value in array { texture_indices(value, visit)?; }
    }
    Ok(())
}

fn filter_document(doc: &mut Value, requested: &HashSet<&str>) -> Result<(), String> {
    filter_scene(doc, |node| node.pointer("/extras/dsor_skin").and_then(Value::as_str).is_none_or(|s| requested.contains(s)))
}

fn filter_prop_document(doc: &mut Value, wanted: &str) -> Result<(), String> {
    // Keep unusual skinned props on the existing state-visibility path: state
    // branches could contain bones referenced by a different branch.
    if doc["skins"].as_array().is_some_and(|s| !s.is_empty()) { return Ok(()); }
    let available: HashSet<String> = doc["nodes"].as_array().into_iter().flatten()
        .filter_map(|n| n["name"].as_str()).map(str::to_lowercase)
        .filter(|n| crate::world::props::STATES.contains(&n.as_str()) || n == wanted).collect();
    let selected = crate::world::props::selected_state(wanted, &available);
    filter_scene(doc, |node| node["name"].as_str().is_none_or(|n| {
        let name = n.to_lowercase();
        !available.contains(&name) || name == selected
    }))
}

fn filter_scene(doc: &mut Value, keep: impl Fn(&Value) -> bool) -> Result<(), String> {
    let nodes = doc.get("nodes").and_then(Value::as_array).ok_or("missing glTF nodes")?;
    let mut pending: Vec<usize> = doc.get("scenes").and_then(Value::as_array).into_iter().flatten()
        .flat_map(|s| s.get("nodes").and_then(Value::as_array).into_iter().flatten())
        .map(index).collect::<Result<_, _>>()?;
    let mut used_nodes = HashSet::new();
    while let Some(i) = pending.pop() {
        let node = nodes.get(i).ok_or("invalid glTF node reference")?;
        if !keep(node) {
            continue;
        }
        if !used_nodes.insert(i) { continue; }
        for child in node.get("children").and_then(Value::as_array).into_iter().flatten() {
            pending.push(index(child)?);
        }
    }
    let mut used_meshes = HashSet::new();
    let mut used_skins = HashSet::new();
    for &i in &used_nodes {
        if let Some(m) = nodes[i].get("mesh") { used_meshes.insert(index(m)?); }
        if let Some(s) = nodes[i].get("skin") { used_skins.insert(index(s)?); }
    }
    // Skeletons and attachment nodes are retained by traversal above. Reject a
    // malformed set which would remove a joint rather than silently breaking it.
    let node_map = compact(doc, "nodes", &used_nodes);
    for node in array_mut(doc, "nodes") { prune_indices(node, "children", &node_map); }
    for scene in array_mut(doc, "scenes") { prune_indices(scene, "nodes", &node_map); }
    let mesh_map = compact(doc, "meshes", &used_meshes);
    let skin_map = compact(doc, "skins", &used_skins);
    for node in array_mut(doc, "nodes") {
        if let Some(i) = node.get_mut("mesh") { remap(i, &mesh_map)?; }
        if let Some(i) = node.get_mut("skin") { remap(i, &skin_map)?; }
    }
    for skin in array_mut(doc, "skins") {
        for joint in array_mut(skin, "joints") { remap(joint, &node_map)?; }
        if let Some(i) = skin.get_mut("skeleton") { remap(i, &node_map)?; }
    }
    for animation in array_mut(doc, "animations") {
        if let Some(channels) = animation.get_mut("channels").and_then(Value::as_array_mut) {
            channels.retain(|c| c.pointer("/target/node").and_then(Value::as_u64)
                .is_none_or(|i| node_map.get(i as usize).copied().flatten().is_some()));
            for channel in channels {
                if let Some(i) = channel.pointer_mut("/target/node") { remap(i, &node_map)?; }
            }
        }
    }
    if let Some(animations) = doc.get_mut("animations").and_then(Value::as_array_mut) {
        animations.retain(|a| a["channels"].as_array().is_some_and(|c| !c.is_empty()));
    }
    // The loader loads even unreferenced entries in these arrays: deleting only
    // scene nodes would still load every variant's GPU meshes and textures.
    let mut used_materials = HashSet::new();
    for mesh in array_mut(doc, "meshes") {
        for primitive in array_mut(mesh, "primitives") {
            if let Some(i) = primitive.get("material") { used_materials.insert(index(i)?); }
        }
    }
    let material_map = compact(doc, "materials", &used_materials);
    for mesh in array_mut(doc, "meshes") {
        for primitive in array_mut(mesh, "primitives") {
            if let Some(i) = primitive.get_mut("material") { remap(i, &material_map)?; }
        }
    }
    let mut used_textures = HashSet::new();
    for material in array_mut(doc, "materials") {
        texture_indices(material, &mut |i| { used_textures.insert(index(i)?); Ok(()) })?;
    }
    let texture_map = compact(doc, "textures", &used_textures);
    for material in array_mut(doc, "materials") {
        texture_indices(material, &mut |i| remap(i, &texture_map))?;
    }
    let mut used_images = HashSet::new();
    for texture in array_mut(doc, "textures") {
        if let Some(i) = texture.get("source") { used_images.insert(index(i)?); }
        for ext in texture.get("extensions").and_then(Value::as_object).into_iter().flat_map(|o| o.values()) {
            if let Some(i) = ext.get("source") { used_images.insert(index(i)?); }
        }
    }
    let image_map = compact(doc, "images", &used_images);
    for texture in array_mut(doc, "textures") {
        if let Some(i) = texture.get_mut("source") { remap(i, &image_map)?; }
        for ext in texture.get_mut("extensions").and_then(Value::as_object_mut).into_iter().flat_map(|o| o.values_mut()) {
            if let Some(i) = ext.get_mut("source") { remap(i, &image_map)?; }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envvertexanim_vertex_colours_are_wind_weights_not_tints() {
        let mut doc = serde_json::json!({
            "materials": [{"extras": {"nebula_shader": "shd:envvertexanim"}}, {"extras": {"nebula_shader": "shd:standard"}}],
            "meshes": [{"primitives": [
                {"material": 0, "attributes": {"POSITION": 0, "COLOR_0": 1}},
                {"material": 1, "attributes": {"POSITION": 2, "COLOR_0": 3}}]}]});
        drop_wind_weights(&mut doc);
        assert!(doc["meshes"][0]["primitives"][0]["attributes"].get("COLOR_0").is_none());
        assert_eq!(doc["meshes"][0]["primitives"][1]["attributes"]["COLOR_0"], 3);
        assert_eq!(doc["meshes"][0]["primitives"][0]["attributes"]["POSITION"], 0);
    }
    use serde_json::json;

    #[test]
    fn prop_state_selection_preserves_common_nodes_and_fallback_but_discards_other_assets() {
        let original = json!({"scenes":[{"nodes":[0]}], "nodes":[{"name":"root","children":[1,2,3,4]},
            {"name":"active","mesh":0},{"name":"inactive","mesh":1}, {"name":"goingactive","mesh":2},
            {"name":"common","mesh":3}], "meshes":[{"primitives":[{"material":0}]},{"primitives":[{"material":1}]},
            {"primitives":[{"material":2}]},{"primitives":[{"material":3}]}],"materials":[{},{},{},{}],
            "animations":[{"channels":[{"target":{"node":3}}]}]});
        for (wanted, selected) in [("active","active"),("inactive","inactive"),("cooldown","inactive"),("blue","active")] {
            let mut doc = original.clone();
            filter_prop_document(&mut doc, wanted).unwrap();
            assert_eq!(doc["nodes"].as_array().unwrap().len(), 3);
            assert_eq!(doc["nodes"][1]["name"], selected);
            assert_eq!(doc["nodes"][2]["name"], "common");
            assert_eq!(doc["meshes"].as_array().unwrap().len(), 2);
            assert_eq!(doc["materials"].as_array().unwrap().len(), 2);
            assert!(doc["animations"].as_array().unwrap().is_empty());
        }
        assert_ne!(prop_path("t003_outdoor/wall", "active"), prop_path("t003_outdoor/wall", "inactive"));
    }

    #[test]
    fn converted_prop_states_validate() {
        let Ok(root) = std::env::var("DSOR_VARIANT_TEST_ASSETS") else { return };
        let mut models = HashSet::new();
        for map in ["a0200_kingscity", "a0007_pastures_pve"] {
            let doc: Value = serde_json::from_slice(&std::fs::read(format!("{root}/maps/{map}.props.json")).unwrap()).unwrap();
            for row in doc["objects"].as_array().unwrap() { models.insert(row["graphics"].as_str().unwrap().to_owned()); }
        }
        let mut checked = 0;
        for model in &models {
            let path = crate::world::map::model_path(model);
            let bytes = std::fs::read(format!("{root}/{path}")).unwrap();
            for state in crate::world::props::STATES {
                let filtered = rewrite_glb(&bytes, |doc| filter_prop_document(doc, state))
                    .unwrap_or_else(|e| panic!("{model}/{state}: {e}"));
                gltf::Gltf::from_slice(&filtered).unwrap_or_else(|e| panic!("{model}/{state}: {e}"));
                checked += 1;
            }
        }
        eprintln!("validated {checked} prop states across {} models", models.len());
    }

    #[test]
    fn request_paths_are_unique_and_keep_relative_texture_directory() {
        for set in ["horse/black", "horse#white", "été"] {
            let path = path("characters/horse", set);
            assert_eq!(Path::new(&path).parent(), Some(Path::new("characters")));
            assert_eq!(request(Path::new(&path)), Some(("characters/horse".into(), set.into())));
        }
        assert_ne!(path("characters/horse", "black"), path("characters/horse", "white"));
    }

    #[test]
    fn selection_preserves_rig_animation_attachments_and_custom_texture_indices() {
        let mut doc = json!({
            "scenes": [{"nodes": [0, 2, 3]}],
            "nodes": [{"name":"rig", "children":[1]}, {"name":"fx_hand"},
                {"mesh":0,"skin":0,"extras":{"dsor_skin":"unused"}},
                {"mesh":1,"skin":0,"extras":{"dsor_skin":"wanted"}}],
            "skins": [{"joints":[0,1],"skeleton":0}],
            "animations": [{"channels":[{"target":{"node":1}}, {"target":{"node":3}}, {"target":{"node":2}}]}],
            "meshes": [{"primitives":[{"material":0}]}, {"primitives":[{"material":1}]}],
            "materials": [{"normalTexture":{"index":0}},
                {"pbrMetallicRoughness":{"baseColorTexture":{"index":2}}, "extras":{"dsor_dye":{"mask":1}}}],
            "textures": [{"source":0},{"source":2},{"source":1}],
            "images": [{"uri":"unused.png"},{"uri":"wanted.png"},{"uri":"mask.png"}]
        });
        filter_document(&mut doc, &HashSet::from(["wanted"])).unwrap();
        assert_eq!(doc["scenes"][0]["nodes"], json!([0,2]));
        assert_eq!(doc["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(doc["nodes"][2]["mesh"], 0);
        assert_eq!(doc["skins"][0]["joints"], json!([0,1]));
        assert_eq!(doc["animations"][0]["channels"], json!([{"target":{"node":1}}, {"target":{"node":2}}]));
        assert_eq!(doc["meshes"].as_array().unwrap().len(), 1);
        assert_eq!(doc["materials"][0]["extras"]["dsor_dye"]["mask"], 0);
        assert_eq!(doc["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"], 1);
        assert_eq!(doc["images"], json!([{"uri":"wanted.png"},{"uri":"mask.png"}]));
        assert_eq!(doc["textures"], json!([{"source":1},{"source":0}]));
    }

    /// Optional integration check against the local proprietary conversion.
    #[test]
    fn converted_model_sets_validate() {
        let Ok(root) = std::env::var("DSOR_VARIANT_TEST_ASSETS") else { return };
        let mut models: HashSet<String> = ["horse", "troll", "skeleton_archer"].into_iter().map(str::to_owned).collect();
        let read = |path: String| -> Value { serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap() };
        let templates = read(format!("{root}/characters/npc_templates.json"));
        for map in ["a0200_kingscity", "a0007_pastures_pve"] {
            let deco = read(format!("{root}/maps/{map}.deco.json"));
            let npcs = read(format!("{root}/maps/{map}.npcs.json"));
            for graphics in deco["chars"].as_array().unwrap().iter().filter_map(|c| c["g"].as_str())
                .chain(npcs.as_object().unwrap().values().filter_map(|p| {
                    let t = p.get("look").filter(|v| !v.is_null()).or_else(|| templates.get(p["id"].as_str()?))?;
                    t["graphics"].as_str()
                })) {
                if let Some(model) = graphics.strip_prefix("characters/").filter(|m| !matches!(*m, "uniskel" | "uniskel_dwarf")) {
                    models.insert(model.to_owned());
                }
            }
        }
        let mut checked = 0;
        for model in &models {
            let bytes = std::fs::read(format!("{root}/characters/{model}.glb")).unwrap();
            let sets: Value = serde_json::from_slice(&std::fs::read(format!("{root}/characters/{model}.sets.json")).unwrap()).unwrap();
            for (name, set) in sets.as_object().unwrap() {
                let skins = set["skins"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
                let filtered = filter_glb(&bytes, &skins).unwrap_or_else(|e| panic!("{model}/{name}: {e}"));
                let gltf = gltf::Gltf::from_slice(&filtered).unwrap_or_else(|e| panic!("{model}/{name}: {e}"));
                assert!(gltf.nodes().all(|n| n.extras().as_ref().is_none_or(|e| {
                    let v: Value = serde_json::from_str(e.get()).unwrap();
                    v["dsor_skin"].as_str().is_none_or(|s| skins.contains(s))
                })));
                checked += 1;
            }
        }
        eprintln!("validated {checked} character sets across {} models", models.len());
    }
}
