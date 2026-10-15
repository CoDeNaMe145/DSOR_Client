//! Conversion to the client's native scene, camera and lighting formats.
use crate::*;
use glam::{Mat4, Quat, Vec3};
pub fn matrix(kind: &str, row: &Value) -> Result<[f32; 16]> {
    if kind != GEOMETRY {
        return floats(&row["Transform"])?
            .try_into()
            .map_err(|_| "Transform needs 16 values".into());
    }
    let vec = |key: &str| -> Result<Vec<f32>> {
        array(&row[key])
            .map(|v| {
                v.as_f64()
                    .map(|n| n as f32)
                    .ok_or_else(|| "Invalid transform number".into())
            })
            .collect()
    };
    let p: [f32; 3] = vec("p")?.try_into().map_err(|_| "Invalid position")?;
    let r: [f32; 4] = vec("r")?.try_into().map_err(|_| "Invalid rotation")?;
    let s: [f32; 3] = vec("s")?.try_into().map_err(|_| "Invalid scale")?;
    Ok(
        Mat4::from_scale_rotation_translation(s.into(), Quat::from_array(r), p.into())
            .to_cols_array(),
    )
}
pub fn position(kind: &str, row: &Value) -> Option<[f32; 3]> {
    let m = matrix(kind, row).ok()?;
    Some([m[12], m[13], m[14]])
}
pub fn moved(kind: &str, row: &Value, p: [f32; 3]) -> Result<Value> {
    let mut v = row.clone();
    if kind == GEOMETRY {
        v["p"] = json!(p);
    } else {
        let mut m = matrix(kind, row)?;
        m[12..15].copy_from_slice(&p);
        v["Transform"] = float_blob(m);
    }
    Ok(v)
}
pub fn is_camera(kind: &str) -> bool {
    matches!(
        kind,
        CAMERA | "_Instance_Camera" | "_Instance_CutsceneCamera" | "_Instance_SequencerCamera"
    )
}
pub fn selection_record(c: &Value) -> Result<Value> {
    let m = if let Some(v) = c["matrix"].as_array() {
        v.iter().map(|v| number(v, 0.) as f32).collect::<Vec<_>>()
    } else {
        let p = number(&c["pitch"], 0.).to_radians();
        let y = number(&c["yaw"], 0.).to_radians();
        let (sx, cx, sy, cy) = (p.sin(), p.cos(), y.sin(), y.cos());
        vec![
            cy,
            0.,
            -sy,
            0.,
            sy * sx,
            cx,
            cy * sx,
            0.,
            sy * cx,
            -sx,
            cy * cx,
            0.,
            number(&c["offset"][0], 0.),
            number(&c["offset"][1], 0.),
            -number(&c["offset"][2], 0.),
            1.,
        ]
        .into_iter()
        .map(|n| n as f32)
        .collect()
    };
    Ok(
        json!({"Id":"Character selection camera","Transform":float_blob(m),"FieldOfView":c["fov"],"PreviewAspect":16./9.,"NearClip":number(&c["near"],0.1),"FarClip":number(&c["far"],1000.)}),
    )
}
pub fn lens(kind: &str, row: &Value) -> Value {
    if kind == CAMERA {
        json!({"fov":row["FieldOfView"],"aspect":row["PreviewAspect"],"near":row["NearClip"],"far":row["FarClip"]})
    } else {
        let mut fov = number(&row["CameraVertFieldOfView"], 0.);
        if fov > 0. && fov <= std::f64::consts::PI {
            fov = fov.to_degrees();
        }
        json!({"fov":if fov==0.{34.}else{fov},"aspect":number(&row["CameraAspectRatio"],16./9.),"near":0.1,"far":1000.})
    }
}
pub fn light(row: &Value) -> Result<Value> {
    let m = matrix(LIGHT, row)?;
    let color = floats(&row["LightColor"])?;
    if color.len() < 3 {
        return Err("Invalid light color".into());
    }
    Ok(
        json!({"t":number(&row["LightType"],0.) as i32,"p":&m[12..15],"dir":&m[8..11],"color":&color[..3],"i":number(&row["LightIntensity"],0.),"r":number(&row["LightRange"],0.),"cone":number(&row["LightConeAngle"],0.),"shadows":flag(&row["LightCastShadows"]),"event":number(&row["EventSetRow"],-1.) as i32,"flicker":if flag(&row["LightFlickerEnable"]){json!([number(&row["LightFlickerFrequency"],0.),number(&row["LightFlickerIntensity"],0.)])}else{Value::Null}}),
    )
}
pub fn bubble(row: &Value) -> Result<Value> {
    let m = matrix(AMBIENCE, row).ok();
    let mut out = json!({"dir":m.map(|m|m[8..11].to_vec()),"m":m,"priority":number(&row["AmbienceBubblePriority"],0.) as i32,"event":number(&row["EventSetRow"],-1.) as i32,"shape":row.get("PEShapeType").unwrap_or(&json!("Box")),"enabled":row.get("AmbienceBubbleEnabled").is_none_or(flag)});
    for key in "LightColor LightOppositeColor LightIntensity BackLightFactor LightAmbient LightShadowIntensity LightSpecularIntensity Saturation Balance BloomScale BloomColor BrightPassThreshold FogColor HeightFogColor FogIntensity FogNearHeight FogFarHeight FogNearDist FogFarDist LocalLightColor LocalLightIntensity RimLightColor RimLightIntensity VignetteIntensity VignetteSize VignetteColor PEFadeTime".split_whitespace(){if let Some(v)=row.get(key){out[key]=if v.is_object(){json!(floats(v)?)}else{v.clone()};}}
    Ok(out)
}
pub fn lighting_document(session: &Session) -> Result<Value> {
    let mut result = json!({"map":session.name,"global":null,"bubbles":[],"lights":[]});
    for (_, row) in object(&session.records[AMBIENCE]) {
        let b = bubble(row)?;
        if flag(&row["PEDefaultEntity"]) && result["global"].is_null() {
            result["global"] = b;
        } else {
            result["bubbles"].as_array_mut().unwrap().push(b);
        }
    }
    for (_, row) in object(&session.records["_Instance_GlobalShadow"]) {
        if let Ok(m) = matrix("_Instance_GlobalShadow", row) {
            result["shadow"] = json!(&m[8..11]);
            break;
        }
    }
    for (_, row) in object(&session.records[LIGHT]) {
        if !row["Transform"].is_null() {
            result["lights"].as_array_mut().unwrap().push(light(row)?);
        }
    }
    Ok(result)
}
pub fn validate_special(kind: &str, row: &Value, original: Option<&Value>) -> Result<()> {
    if kind == CAMERA {
        let l = lens(kind, row);
        let (f, a, n, z) = (
            number(&l["fov"], 0.),
            number(&l["aspect"], 0.),
            number(&l["near"], 0.),
            number(&l["far"], 0.),
        );
        if !(1. ..=170.).contains(&f) || !(0.25..=4.).contains(&a) || n <= 0. || z <= n {
            return Err("Camera needs FOV 1–170°, aspect 0.25–4 and 0 < near < far".into());
        }
        let m = matrix(kind, row)?;
        let axes = [
            Vec3::from_slice(&m[0..3]),
            Vec3::from_slice(&m[4..7]),
            Vec3::from_slice(&m[8..11]),
        ];
        if axes.iter().any(|a| (a.length_squared() - 1.).abs() > 0.002)
            || [(0, 1), (0, 2), (1, 2)]
                .iter()
                .any(|(i, j)| axes[*i].dot(axes[*j]).abs() > 0.002)
        {
            return Err("Cameras can move and rotate; scale and shear are not allowed".into());
        }
    }
    if !matches!(kind, LIGHT | AMBIENCE) {
        return Ok(());
    }
    let changed = |k: &str| row.get(k).is_some() && original.is_none_or(|o| o[k] != row[k]);
    for k in [
        "LightColor",
        "LightOppositeColor",
        "LightAmbient",
        "FogColor",
        "BloomColor",
        "LocalLightColor",
        "VignetteColor",
    ] {
        if changed(k)
            && floats(&row[k])?
                .iter()
                .any(|n| !n.is_finite() || !(0. ..=32.).contains(n))
        {
            return Err(format!("{k} channels must be between 0 and 32").into());
        }
    }
    for k in [
        "LightIntensity",
        "LightRange",
        "LightFlickerFrequency",
        "FogIntensity",
        "BloomScale",
        "BrightPassThreshold",
        "Saturation",
        "LocalLightIntensity",
        "PEFadeTime",
        "VignetteIntensity",
        "VignetteSize",
    ] {
        if changed(k) && row[k].as_f64().is_none_or(|n| n < 0. || !n.is_finite()) {
            return Err(format!("{k} must be a finite nonnegative number").into());
        }
    }
    for (k, lo, hi) in [
        ("LightConeAngle", 0., 179.),
        ("LightFlickerIntensity", 0., 1.),
    ] {
        if changed(k) && !(lo..=hi).contains(&number(&row[k], -1.)) {
            return Err(format!("{k} must be between {lo} and {hi}").into());
        }
    }
    for k in [
        "LightType",
        "LightCastShadows",
        "LightFlickerEnable",
        "AmbienceBubbleEnabled",
    ] {
        if changed(k) && !matches!(row[k].as_i64(), Some(0 | 1)) && !row[k].is_boolean() {
            return Err(format!("{k} must be 0 or 1").into());
        }
    }
    if (changed("FogNearDist") || changed("FogFarDist"))
        && number(&row["FogFarDist"], 0.) <= number(&row["FogNearDist"], 0.)
    {
        return Err("Fog far distance must exceed near distance".into());
    }
    Ok(())
}
pub fn new_record(session: &Session, kind: &str) -> Result<Value> {
    let columns = &session.snapshot["schemas"][kind]["columns"];
    if !columns.is_array() {
        return Err("DB4 schema unavailable; restore the map DB4 or metadata cache".into());
    }
    let mut row = object(&session.records[kind])
        .next()
        .map(|(_, r)| r.clone())
        .unwrap_or(json!({}));
    for c in array(columns) {
        let k = text(&c["name"]);
        if row.get(k).is_none() {
            let ty = text(&c["type"]).to_uppercase();
            let default = text(&c["dflt_value"]);
            let v = serde_json::from_str(default).unwrap_or_else(|_| {
                if !default.is_empty() {
                    json!(default.trim_matches('\''))
                } else if ty.contains("BLOB") {
                    blob(&[])
                } else if ty.contains("INT") {
                    json!(0)
                } else if ty.contains("REAL") {
                    json!(0.0)
                } else {
                    json!("")
                }
            });
            row[k] = v;
        }
    }
    for (k, v) in [
        ("Guid", blob(uuid::Uuid::new_v4().as_bytes())),
        ("_ID", json!(uuid::Uuid::new_v4().to_string())),
        ("_Level", json!(session.name)),
        ("Transform", float_blob(Mat4::IDENTITY.to_cols_array())),
    ] {
        if row.get(k).is_some() {
            row[k] = v;
        }
    }
    Ok(row)
}
pub fn new_light(session: &Session, position: [f32; 3], spot: bool) -> Result<Value> {
    let mut row = new_record(session, LIGHT)?;
    let mut defaults = json!({"_Layers":"","EventSetRow":-1,"ParentNodeIndex":-1,"LightType":if spot{1}else{0},"LightIntensity":1.0,"LightRange":8.0,"LightCastShadows":0,"LightShadowIntensity":1.0,"LightTexture":"","AnimPath":"","LightFlickerEnable":0,"LightFlickerFrequency":2.0,"LightFlickerIntensity":0.3,"LightConeAngle":if spot{45.0}else{0.0}});
    defaults["Transform"] = float_blob([
        1.,
        0.,
        0.,
        0.,
        0.,
        0.,
        -1.,
        0.,
        0.,
        1.,
        0.,
        0.,
        position[0],
        position[1],
        position[2],
        1.,
    ]);
    defaults["LightColor"] = float_blob([1., 0.86, 0.65, 1.]);
    defaults["LightOppositeColor"] = float_blob([0., 0., 0., 1.]);
    defaults["LightAmbient"] = float_blob([0., 0., 0., 1.]);
    for (k, v) in object(&defaults) {
        if row.get(k).is_some() {
            row[k] = v.clone();
        }
    }
    Ok(row)
}
pub fn track_matrix(at: &Value) -> Result<[f32; 16]> {
    let a: Vec<_> = array(at).map(|n| number(n, 0.)).collect();
    if a.len() != 9 {
        return Err("Invalid sequence transform".into());
    }
    let (cx, cy, cz) = (
        a[3].to_radians().cos(),
        a[4].to_radians().cos(),
        a[5].to_radians().cos(),
    );
    let (ax, ay, az) = (
        a[3].to_radians().sin(),
        a[4].to_radians().sin(),
        a[5].to_radians().sin(),
    );
    let (sx, sy, sz) = (a[6], a[7], a[8]);
    Ok([
        cy * cz * sx,
        cy * az * sx,
        -ay * sx,
        0.,
        (ax * ay * cz - cx * az) * sy,
        (ax * ay * az + cx * cz) * sy,
        ax * cy * sy,
        0.,
        (cx * ay * cz + ax * az) * sz,
        (cx * ay * az - ax * cz) * sz,
        cx * cy * sz,
        0.,
        a[0],
        a[1],
        a[2],
        1.,
    ]
    .map(|n| n as f32))
}
#[derive(Default)]
pub struct Catalogs {
    cache: std::collections::HashMap<String, Value>,
}
impl Catalogs {
    pub fn get(&mut self, assets: &Path, relative: &str) -> Value {
        self.cache
            .entry(assets.join(relative).to_string_lossy().into_owned())
            .or_insert_with(|| {
                read_json(assets.join(relative))
                    .ok()
                    .flatten()
                    .unwrap_or(json!({}))
            })
            .clone()
    }
    pub fn character(
        &mut self,
        assets: &Path,
        model: &str,
        outfit: &str,
        set: &str,
        state: &str,
    ) -> Option<Value> {
        let skeleton = model.strip_prefix("characters/")?;
        if !matches!(skeleton, "uniskel" | "uniskel_dwarf") {
            return None;
        }
        let outfits = self.get(assets, &format!("characters/{skeleton}/outfits.json"));
        let outfit = array(&outfits["outfits"]).find(|r| r["name"] == outfit)?;
        let variants = self.get(assets, &format!("characters/{skeleton}/variations.json"));
        Some(
            json!({"skeleton":skeleton,"parts":outfit["parts"],"anim_set":set,"state":if state.is_empty(){"Idle"}else{state},"variation":variants["variations"].get(text(&outfit["variation"])).cloned().unwrap_or(json!([]))}),
        )
    }
}
pub fn document(session: &Session, catalogs: &mut Catalogs, revision: u64) -> Result<Value> {
    let mut objects = vec![];
    let assets = &session.project.assets;
    for (kind, rows) in object(&session.records) {
        for (id, row) in object(rows) {
            let Ok(m) = matrix(kind, row) else { continue };
            let model = if kind == GEOMETRY {
                text(&row["model"])
            } else {
                text(&row["Graphics"])
            };
            let label = ["n", "Name", "Id"]
                .iter()
                .find_map(|k| row[*k].as_str().filter(|s| !s.is_empty()))
                .unwrap_or(if model.is_empty() { kind } else { model });
            let mut item = json!({"key":format!("{kind}\t{id}"),"kind":kind,"label":label,"matrix":m,"model":model,"event":if kind==GEOMETRY{-1}else{number(&row["EventSetRow"],-1.) as i32}});
            if is_camera(kind) {
                item["camera"] = lens(kind, row);
                item["model"] = json!("");
            }
            if let Some(c) = catalogs.character(
                assets,
                model,
                text(&row["CharacterSet"]),
                text(&row["AnimSet"]),
                text(&row["StartAnimationState"]),
            ) {
                item["character"] = c;
            }
            if kind == LIGHT {
                item["light"] = light(row)?;
                item["model"] = json!("");
                item["label"] = json!(format!(
                    "{} light · {}",
                    if row["LightType"] == 1 {
                        "Spot"
                    } else {
                        "Point"
                    },
                    &id[id.len().saturating_sub(8)..]
                ));
            }
            if kind == AMBIENCE {
                item["ambience"] = bubble(row)?;
                item["default_ambience"] = json!(flag(&row["PEDefaultEntity"]));
                item["model"] = json!("");
            }
            if kind == "_Instance_Sequence" && flag(&row["SequenceIsPlaying"]) {
                let sequences = catalogs.get(assets, "skills/sequences.json");
                let name = text(&row["SequencerFileName"])
                    .split_once('/')
                    .map(|(_, b)| b)
                    .unwrap_or(text(&row["SequencerFileName"]));
                if let Some(sequence) = sequences.get(name) {
                    item["sequence"] = sequence.clone();
                    let mut decorations = vec![];
                    for track in array(&sequence["tracks"]).filter(|t| t["kind"] == "character") {
                        let parts: Vec<_> = text(&track["object"]).splitn(3, '/').collect();
                        if parts.len() != 3 {
                            continue;
                        }
                        let model = format!("characters/{}", parts[0]);
                        let anims = catalogs.get(assets, "characters/anims.json");
                        let mut set = format!("deco:{}", parts[2]);
                        if anims.get(format!("{set}-loop")).is_some() {
                            set.push_str("-loop");
                        }
                        decorations.push(json!({"model":model,"matrix":track_matrix(&track["at"])?,"character":catalogs.character(assets,&model,parts[1],&set,"Idle")}));
                    }
                    item["decorations"] = json!(decorations);
                }
            }
            objects.push(item);
        }
    }
    let manifest = &session.snapshot["manifest"];
    Ok(
        json!({"protocol":1,"revision":revision,"map":session.name,"center":manifest.get("center").cloned().unwrap_or(json!([0,0,0])),"extents":manifest.get("extents").cloned().unwrap_or(json!([30,20,30])),"objects":objects,"ambience":catalogs.get(assets,&format!("maps/{}.ambience.json",session.name))["global"]}),
    )
}
pub fn apply_transform(session: &mut Session, event: &Value, revision: u64) -> Result<()> {
    if event["revision"] != revision {
        return Err("Scene changed during gesture; edit cancelled".into());
    }
    let (kind, id) = text(&event["key"])
        .split_once('\t')
        .ok_or("Invalid object key")?;
    let mut row = session.records[kind][id].clone();
    match text(&event["mode"]) {
        "translate" => {
            if kind == GEOMETRY {
                if let Some(axis) = event["axis"].as_u64().filter(|a| *a < 3) {
                    row["p"][axis as usize] = event["position"][axis as usize].clone();
                } else {
                    row["p"] = event["position"].clone();
                }
            } else {
                let mut m = matrix(kind, &row)?;
                for i in 0..3 {
                    if event["axis"].as_u64().is_none_or(|a| a == i as u64) {
                        m[12 + i] = number(&event["position"][i], f64::NAN) as f32;
                    }
                }
                row["Transform"] = float_blob(m);
            }
        }
        "rotate" | "scale" if kind == GEOMETRY => {
            let key = if event["mode"] == "rotate" { "r" } else { "s" };
            row[key] = event[if key == "r" { "rotation" } else { "scale" }].clone();
        }
        mode @ ("rotate" | "scale") => {
            let mut m = matrix(kind, &row)?;
            if mode == "rotate" {
                let axis = event["axis"]
                    .as_u64()
                    .filter(|a| *a < 3)
                    .ok_or("Invalid rotation axis")?;
                let angle = number(&event["angle"], f64::NAN);
                if !angle.is_finite() {
                    return Err("Invalid rotation angle".into());
                }
                let (c, s) = (angle.cos(), angle.sin());
                for offset in [0, 4, 8] {
                    let (x, y, z) = (m[offset] as f64, m[offset + 1] as f64, m[offset + 2] as f64);
                    let v = match axis {
                        0 => [x, c * y - s * z, s * y + c * z],
                        1 => [c * x + s * z, y, -s * x + c * z],
                        _ => [c * x - s * y, s * x + c * y, z],
                    };
                    for i in 0..3 {
                        m[offset + i] = v[i] as f32;
                    }
                }
            } else {
                for axis in 0..3 {
                    let f = number(&event["factors"][axis], f64::NAN);
                    if !f.is_finite() || f <= 0. {
                        return Err("Invalid scale factor".into());
                    }
                    for i in 0..3 {
                        m[axis * 4 + i] = (m[axis * 4 + i] as f64 * f) as f32;
                    }
                }
            }
            row["Transform"] = float_blob(m);
        }
        _ => return Err("Unknown transform mode".into()),
    }
    session.update(kind, id, row)
}
pub fn friendly(v: &Value) -> Value {
    let mut out = v.clone();
    for (k, val) in object(v) {
        if k == "Transform"
            || k.contains("Color")
            || matches!(k.as_str(), "LightAmbient" | "Balance")
        {
            if let Ok(f) = floats(val) {
                out[k] = json!({"$f32":f});
            }
        }
    }
    out
}
pub fn from_friendly(v: &Value) -> Result<Value> {
    let mut out = v.clone();
    for (k, val) in object(v) {
        if let Some(a) = val["$f32"].as_array() {
            let f: Vec<_> = a
                .iter()
                .map(|v| {
                    v.as_f64()
                        .filter(|n| n.is_finite() && (*n as f32).is_finite())
                        .map(|n| n as f32)
                        .ok_or_else(|| "Expected finite float32 value".into())
                })
                .collect::<Result<_>>()?;
            out[k] = float_blob(f);
        }
    }
    Ok(out)
}
