//! DB4 scenery omitted from the binary .map placements. Keep GUIDs and event
//! declarations: the server binds scene props by GUID, not by template or row order.
use crate::*;

pub const TABLES: &[&str] = &[
    "_Instance_StateObject",
    "_Instance_TravelPoint",
    "_Instance_InfoObject",
    "_Instance_Destroyable",
    "_Instance_DestroyableProp",
    "_Instance_Destructible",
];

pub fn document(records: &Value) -> Result<Value> {
    let mut objects = Vec::new();
    for &kind in TABLES {
        for (id, row) in object(&records[kind]) {
            if text(&row["Graphics"]).is_empty() {
                continue;
            }
            let m = scene::matrix(kind, row)?;
            if !m.iter().all(|v| v.is_finite()) {
                return Err("Non-finite prop transform".into());
            }
            objects.push(json!({
                "key":format!("{kind}\t{id}"), "kind":kind,
                "id":text(&row["Id"]), "guid":text(&row["Guid"]["$blob"]).to_uppercase(),
                "graphics":row["Graphics"], "m":m,
                "map_icon":text(&row["LocationMapIcon"]).replace('\\', "/"),
                "state":if text(&row["State"]).is_empty(){"active"}else{text(&row["State"])},
                "event":number(&row["EventSetRow"],-1.) as i32,
                "quest_controlled":flag(&row["QuestControlled"]), "show_always":flag(&row["ShowAlways"])
            }));
        }
    }
    let events = events::MapEvents::new(records, &Value::Null);
    Ok(json!({"objects":objects,"event_sets":events.sets}))
}

/// Export all source maps, including maps with no authored overrides. Only the
/// prop sidecar is written; geometry, navigation and source databases stay intact.
pub fn export_all(project: &Project, maps: &[String], static_db: Option<&Path>) -> Result<usize> {
    let mut count = 0;
    for map in maps {
        map_name(map)?;
        let records = if project.patch_path(map).is_file() {
            let session = project.open(map, true)?;
            if !session.conflicts.is_empty() {
                return Err(format!("Resolve overrides for {map} before exporting props").into());
            }
            session.records
        } else {
            let path = project
                .db_root
                .as_ref()
                .ok_or("Prop export needs --db-root")?
                .join(format!("{map}.db4"));
            database::snapshot(&path)?.1
        };
        atomic_json(
            project
                .assets
                .join("maps")
                .join(format!("{map}.props.json")),
            &document(&records)?,
        )?;
        count += 1;
    }
    if let Some(path) = static_db {
        let db = database::connect(path)?;
        let mut query = db.prepare("SELECT Id, Graphics, LocationMapIcon FROM _Template_StateObject")?;
        let mut templates = serde_json::Map::new();
        let mut map_icons = serde_json::Map::new();
        for row in query.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?))
        })? {
            let (id, graphics, icon) = row?;
            if let Some(icon) = icon.filter(|s| !s.is_empty()) {
                map_icons.insert(id.clone(), json!(icon.replace('\\', "/")));
            }
            if let Some(g) = graphics.filter(|g| !g.is_empty()) {
                templates.insert(id, json!(g));
            }
        }
        atomic_json(
            project.assets.join("props/catalog.props.json"),
            &json!({"templates":templates,"map_icons":map_icons}),
        )?;
    }
    Ok(count)
}
