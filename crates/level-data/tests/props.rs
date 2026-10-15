use dsor_level_data::*;
use serde_json::json;

#[test]
fn props_export_unedited_maps_and_preserve_guid_transform_event_and_quest_rules() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let db_root = root.join("db4");
    std::fs::create_dir_all(&db_root).unwrap();
    let db = rusqlite::Connection::open(db_root.join("town.db4")).unwrap();
    db.execute_batch("CREATE TABLE _Instance_StateObject(Guid BLOB PRIMARY KEY,Id TEXT,Graphics TEXT,Transform BLOB,State TEXT,EventSetRow INTEGER,QuestControlled INTEGER,ShowAlways INTEGER);
        CREATE TABLE _Instance_TravelPoint(Guid BLOB PRIMARY KEY,Id TEXT,Graphics TEXT,Transform BLOB,State TEXT,EventSetRow INTEGER);
        CREATE TABLE _Instance_EventSets(Id TEXT PRIMARY KEY,EventFlags TEXT);
        INSERT INTO _Instance_EventSets VALUES('119','craft=True;moon=True;'),('120','craft=False;moon=False;');").unwrap();
    let transform = bytes(&float_blob(
        glam::Mat4::from_translation(glam::Vec3::new(46., 1., -45.)).to_cols_array(),
    ))
    .unwrap();
    db.execute(
        "INSERT INTO _Instance_TravelPoint VALUES(?,'stone','props/stone',?,'active',-1)",
        rusqlite::params![[1u8; 16].as_slice(), &transform],
    )
    .unwrap();
    db.execute(
        "INSERT INTO _Instance_StateObject VALUES(?,'bench','props/bench',?,'active',120,1,1)",
        rusqlite::params![[2u8; 16].as_slice(), &transform],
    )
    .unwrap();
    drop(db);
    let static_path = root.join("static.db4");
    let static_db = rusqlite::Connection::open(&static_path).unwrap();
    static_db.execute_batch("CREATE TABLE _Template_StateObject(Id TEXT,Graphics TEXT,LocationMapIcon TEXT); INSERT INTO _Template_StateObject VALUES('chest','props/chest','ui/icon_map_chest');").unwrap();
    drop(static_db);
    let project = Project {
        root: root.into(),
        assets: root.join("assets"),
        overrides: root.join("overrides"),
        cache: root.join("cache"),
        db_root: Some(db_root.clone()),
    }
    .checked()
    .unwrap();
    let original = std::fs::read(db_root.join("town.db4")).unwrap();
    assert_eq!(
        props::export_all(&project, &["town".into()], Some(&static_path)).unwrap(),
        1
    );
    let doc = read_json(project.assets.join("maps/town.props.json"))
        .unwrap()
        .unwrap();
    let rows = doc["objects"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let stone = rows.iter().find(|r| r["id"] == "stone").unwrap();
    assert_eq!(stone["guid"], "01010101010101010101010101010101");
    assert_eq!(stone["m"][12], 46.);
    assert_eq!(stone["event"], -1);
    let bench = rows.iter().find(|r| r["id"] == "bench").unwrap();
    assert_eq!(bench["event"], 120);
    assert_eq!(bench["quest_controlled"], true);
    assert_eq!(bench["show_always"], true);
    assert_eq!(doc["event_sets"]["120"], "craft=False;moon=False;");
    assert_eq!(
        read_json(project.assets.join("props/catalog.props.json"))
            .unwrap()
            .unwrap()["templates"]["chest"],
        "props/chest"
    );
    assert_eq!(original, std::fs::read(db_root.join("town.db4")).unwrap());

    // The editor's Build and the standalone exporter use the same composed rows.
    let mut s = project.open("town", true).unwrap();
    let (id, row) = object(&s.records["_Instance_TravelPoint"])
        .next()
        .map(|(k, v)| (k.clone(), v.clone()))
        .unwrap();
    let moved = scene::moved("_Instance_TravelPoint", &row, [90., 2., 30.]).unwrap();
    s.update("_Instance_TravelPoint", &id, moved).unwrap();
    s.save().unwrap();
    let expected = export::client_documents(&s).unwrap()["props.json"].clone();
    props::export_all(&project, &["town".into()], None).unwrap();
    assert_eq!(
        read_json(project.assets.join("maps/town.props.json"))
            .unwrap()
            .unwrap(),
        expected
    );
    s.delete("_Instance_TravelPoint", &id).unwrap();
    s.save().unwrap();
    export::build(&s, None, None).unwrap();
    assert_eq!(
        read_json(project.assets.join("maps/town.props.json"))
            .unwrap()
            .unwrap()["objects"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(original, std::fs::read(db_root.join("town.db4")).unwrap());
}

#[test]
fn malformed_visual_transform_is_reported_instead_of_silently_omitting_props() {
    assert!(props::document(&json!({"_Instance_StateObject":{"x":{"Graphics":"props/bench","Transform":{"$blob":"00"}}}})).is_err());
}
