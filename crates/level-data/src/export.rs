use crate::*;
use scene::*;
pub fn client_documents(s: &Session) -> Result<Value> {
    let mut documents = json!({});
    if props::TABLES.iter().any(|kind| s.snapshot["schemas"].get(*kind).is_some()) {
        documents["props.json"] = props::document(&s.records)?;
    }
    let has = |name: &str| {
        s.snapshot["schemas"]
            .get(format!("_Instance_{name}"))
            .is_some()
    };
    if s.snapshot["manifest"].is_object() || object(&s.records[GEOMETRY]).next().is_some() {
        documents["map.json"] = s.manifest();
    }
    if has("NPC") {
        let templates =
            read_json(s.project.assets.join("characters/npc_templates.json"))?.unwrap_or(json!({}));
        let mut npcs = json!({});
        for (_, row) in object(&s.records["_Instance_NPC"]) {
            let Ok(m) = matrix("_Instance_NPC", row) else {
                continue;
            };
            let template = &templates[text(&row["Id"])];
            let mut look =
                json!({"loop":flag(&row["LoopStartAnimation"]),"title":text(&template["title"])});
            for (key, col, fallback) in [
                ("graphics", "Graphics", ""),
                ("outfit", "CharacterSet", ""),
                ("anim_set", "AnimSet", ""),
                ("state", "StartAnimationState", "Idle"),
            ] {
                let v = row.get(col).unwrap_or(&template[key]);
                look[key] = json!(if text(v).is_empty() {
                    fallback
                } else {
                    text(v)
                });
            }
            let guid = text(&row["Guid"]["$blob"]);
            let event = number(&row["EventSetRow"], -1.) as i32;
            let mut npc = json!({"id":row["Id"],"m":m,"look":look,"event":event});
            if let Some((_, set)) = object(&s.records["_Instance_EventSets"])
                .find(|(_, set)| text(&set["Id"]).parse::<i32>().ok() == Some(event)
                    || set["Id"].as_i64() == Some(event as i64)) {
                npc["event_flags"] = json!(text(&set["EventFlags"]));
            }
            npcs[guid.to_uppercase()] = npc;
        }
        documents["npcs.json"] = npcs;
    }
    if has("InteractExit") {
        let mut exits = vec![];
        for (_, row) in object(&s.records["_Instance_InteractExit"]) {
            let Ok(m) = matrix("_Instance_InteractExit", row) else {
                continue;
            };
            if text(&row["ExitURL"]).is_empty() || text(&row["Graphics"]).is_empty() {
                continue;
            }
            exits.push(json!({"id":row["Id"],"name":text(&row["Name"]),"url":row["ExitURL"],"entry":text(&row["EntryName"]),"graphics":row["Graphics"],"range":number(&row["PickingRange"],0.),"event":number(&row["EventSetRow"],-1.) as i32,"m":m}));
        }
        documents["exits.json"] = json!(exits);
    }
    if has("AmbienceBubble") || has("Light") {
        documents["ambience.json"] = lighting_document(s)?;
    }
    if has("Sequence") {
        let sequences = read_json(s.project.assets.join("skills/sequences.json"))?;
        let anims = read_json(s.project.assets.join("characters/anims.json"))?;
        let changed: std::collections::HashSet<_> = array(&s.patch["operations"])
            .filter(|op| op["kind"] == "_Instance_Sequence")
            .map(|op| text(&op["id"]))
            .collect();
        if let (Some(sequences), Some(anims)) = (sequences, anims) {
            let mut chars = vec![];
            let mut seqs = vec![];
            for (id, row) in object(&s.records["_Instance_Sequence"]) {
                let Ok(at) = matrix("_Instance_Sequence", row) else {
                    continue;
                };
                if number(&row["EventSetRow"], -1.) != -1. || !flag(&row["SequenceIsPlaying"]) {
                    continue;
                }
                let name = text(&row["SequencerFileName"])
                    .split_once('/')
                    .map(|(_, s)| s)
                    .unwrap_or(text(&row["SequencerFileName"]));
                let Some(sequence) = sequences.get(name) else {
                    if changed.contains(id.as_str()) {
                        return Err(format!("Missing sequence definition: {name}").into());
                    }
                    continue;
                };
                let mut effect = false;
                for track in array(&sequence["tracks"]) {
                    if track["kind"] != "character" {
                        effect |= matches!(text(&track["kind"]), "fx" | "light");
                        continue;
                    }
                    let parts: Vec<_> = text(&track["object"]).splitn(3, '/').collect();
                    if parts.len() != 3 {
                        continue;
                    }
                    let mut set = format!("deco:{}", parts[2]);
                    if anims.get(format!("{set}-loop")).is_some() {
                        set.push_str("-loop");
                    }
                    if anims.get(&set).is_none() {
                        if changed.contains(id.as_str()) {
                            return Err(format!(
                                "Animation set {set} unavailable; rebuild character assets"
                            )
                            .into());
                        }
                        continue;
                    }
                    let local = track_matrix(&track["at"])?;
                    let transform = (glam::Mat4::from_cols_array(&at)
                        * glam::Mat4::from_cols_array(&local))
                    .to_cols_array();
                    chars.push(json!({"g":format!("characters/{}",parts[0]),"outfit":parts[1],"set":set,"m":transform,"speed":number(&track["speed"],1.)}));
                }
                if effect {
                    seqs.push(json!({"seq":name,"m":at}));
                }
            }
            documents["deco.json"] = json!({"chars":chars,"seqs":seqs});
        } else if !changed.is_empty() {
            return Err("Sequence export needs skills/sequences.json and characters/anims.json; overrides can still be saved".into());
        }
    }
    Ok(documents)
}
pub fn selection_export(s: &Session) -> Result<Value> {
    let mut data = read_json(s.project.assets.join("characters/selection.json"))?.ok_or(
        "Restore characters/selection.json before building; camera edits can be saved offline",
    )?;
    let base = &s.snapshot["selection_camera"];
    let row = &s.records[CAMERA]["character-selection"];
    if base.is_null() || row.is_null() {
        return Err("Fixed camera baseline unavailable".into());
    }
    validate_special(CAMERA, row, None)?;
    let mut camera = base.clone();
    camera["matrix"] = json!(matrix(CAMERA, row)?);
    camera["fov"] = row["FieldOfView"].clone();
    camera["near"] = row["NearClip"].clone();
    camera["far"] = row["FarClip"].clone();
    camera["_level_override"] = json!({"base":base});
    data["camera"] = camera;
    Ok(data)
}
pub fn build(s: &Session, assets: Option<&Path>, db_out: Option<&Path>) -> Result<Value> {
    if !s.conflicts.is_empty() {
        return Err(format!(
            "Resolve {} override conflicts before building",
            s.conflicts.len()
        )
        .into());
    }
    for op in array(&s.patch["operations"]) {
        let k = text(&op["kind"]);
        let id = text(&op["id"]);
        if s.sources[source_key(k)] == "unavailable" {
            return Err("Complete baseline unavailable; save overrides and restore source or metadata cache before building".into());
        }
        if op["action"] != "delete" {
            s.validate(k, &s.records[k][id], s.snapshot["records"][k].get(id))?;
        }
    }
    let mut documents = client_documents(s)?;
    let camera = if s.records[CAMERA].is_object() {
        Some(selection_export(s)?)
    } else {
        None
    };
    if documents.get("map.json").is_some() {
        documents["map.json"]["_level_override"] = json!({"version":1,"base_sha256":digest(&s.snapshot["manifest"]),"patch_sha256":digest(&s.patch)});
    }
    let target = assets.unwrap_or(&s.project.assets);
    let default_db = s.project.cache.join("build/maps");
    let db = db_out
        .unwrap_or(&default_db)
        .join(format!("{}.db4", s.name));
    let database = if object(&s.snapshot["schemas"]).next().is_some() {
        database::export(s, &db)?;
        Some(db)
    } else {
        None
    };
    let mut files = vec![];
    for (suffix, doc) in object(&documents) {
        atomic_json(
            target.join("maps").join(format!("{}.{suffix}", s.name)),
            doc,
        )?;
        files.push(suffix.clone());
    }
    if let Some(camera) = camera {
        atomic_json(target.join("characters/selection.json"), &camera)?;
        files.push("characters/selection.json".into());
    }
    let geometry = array(&s.patch["operations"]).any(|o| o["kind"] == GEOMETRY);
    let report = json!({"map":s.name,"files":files,"database":database,"navigation_rebuild_required":geometry,"note":if geometry{"Geometry edits do not regenerate PathEngine navigation or .col collision files."}else{""}});
    atomic_json(
        s.project
            .cache
            .join("build")
            .join(format!("{}.report.json", s.name)),
        &report,
    )?;
    Ok(report)
}
