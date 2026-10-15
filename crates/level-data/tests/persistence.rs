use dsor_level_data::*;
use serde_json::{json, Value};
struct Fixture {
    _temp: tempfile::TempDir,
    p: Project,
    manifest: Value,
    db: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().to_owned();
        let db_root = root.join("originals");
        std::fs::create_dir_all(&db_root).unwrap();
        let db = db_root.join("test.db4");
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch("CREATE TABLE _Attributes(Name TEXT,Data BLOB); INSERT INTO _Attributes VALUES('original',X'0000FF00'); CREATE TABLE _Instance_NPC(Guid BLOB PRIMARY KEY,Id TEXT,Transform BLOB,PickingRange REAL,EventSetRow INTEGER); CREATE INDEX npc_id ON _Instance_NPC(Id); CREATE TABLE _Instance_Light(Guid BLOB PRIMARY KEY,Transform BLOB,EventSetRow INTEGER,LightType INTEGER,LightColor BLOB,LightIntensity REAL,LightRange REAL,LightConeAngle REAL,LightCastShadows INTEGER,LightFlickerEnable INTEGER,LightFlickerFrequency REAL,LightFlickerIntensity REAL);").unwrap();
        let m = bytes(&float_blob(glam::Mat4::IDENTITY.to_cols_array())).unwrap();
        c.execute(
            "INSERT INTO _Instance_NPC VALUES(?,'guard',?,2.0,-1)",
            rusqlite::params![&[1u8; 16], &m],
        )
        .unwrap();
        c.execute(
            "INSERT INTO _Instance_Light VALUES(?,?, -1,0,?,1.,5.,0.,0,0,2.,0.3)",
            rusqlite::params![
                &[2u8; 16],
                m,
                bytes(&float_blob([1., 0.5, 0.2, 0.7])).unwrap()
            ],
        )
        .unwrap();
        drop(c);
        let p = Project {
            assets: root.join("assets"),
            overrides: root.join("overrides"),
            cache: root.join("cache"),
            db_root: Some(db_root),
            root,
        }
        .checked()
        .unwrap();
        let manifest = json!({"map":"test","center":[0,0,0],"extents":[20,10,20],"models":["props/tree","props/rock"],"groups":[{"name":"original"}],"instances":[{"m":0,"p":[1.,2.,3.],"r":[0.,0.,0.,1.],"s":[1.,1.,1.],"n":"tree","g":-1,"col":true},{"m":1,"p":[5.,6.,7.],"r":[0.,0.,0.,1.],"s":[1.,1.,1.],"n":"rock","g":-1,"col":true}]});
        atomic_json(p.assets.join("maps/test.map.json"), &manifest).unwrap();
        Self {
            _temp: t,
            p,
            manifest,
            db,
        }
    }
    fn open(&self) -> Session {
        self.p.open("test", true).unwrap()
    }
}
fn first(s: &Session, kind: &str) -> (String, Value) {
    object(&s.records[kind])
        .next()
        .map(|(k, v)| (k.clone(), v.clone()))
        .unwrap()
}
#[test]
fn python_identity_encoding() {
    assert_eq!(canonical(&json!({"z":1e16,"a":1e-7,"tiny":0.0001,"decimal":1e15,"negative":-0.0,"unicode":"snow ☃"})),"{\"a\":1e-07,\"decimal\":1000000000000000.0,\"negative\":-0.0,\"tiny\":0.0001,\"unicode\":\"snow ☃\",\"z\":1e+16}");
    assert_eq!(
        digest(&json!({})),
        "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    );
}
#[test]
fn rebuild_and_repeat_build_preserve_identity() {
    let f = Fixture::new();
    let mut s = f.open();
    let (id, v) = first(&s, GEOMETRY);
    s.update(
        GEOMETRY,
        &id,
        scene::moved(GEOMETRY, &v, [90., 2., 40.]).unwrap(),
    )
    .unwrap();
    s.save().unwrap();
    export::build(&s, None, None).unwrap();
    assert_eq!(f.open().records, s.records);
    let mut rebuilt = f.manifest.clone();
    rebuilt["models"].as_array_mut().unwrap().reverse();
    for row in rebuilt["instances"].as_array_mut().unwrap() {
        row["m"] = json!(1 - row["m"].as_i64().unwrap());
    }
    rebuilt["instances"].as_array_mut().unwrap().reverse();
    atomic_json(f.p.assets.join("maps/test.map.json"), &rebuilt).unwrap();
    let s = f.open();
    assert!(s.conflicts.is_empty());
    assert_eq!(s.records[GEOMETRY][&id]["p"], json!([90., 2., 40.]));
    assert_eq!(s.manifest()["groups"], f.manifest["groups"]);
}
#[test]
fn three_way_disjoint_merge_and_conflict() {
    let base = json!({GEOMETRY:{"a":{"p":1,"color":2}}});
    let patch = json!({"operations":[{"kind":GEOMETRY,"id":"a","action":"update","base":{"p":1,"color":1},"values":{"p":3}}]});
    let (r, c) = compose(&base, &patch);
    assert!(c.is_empty());
    assert_eq!(r[GEOMETRY]["a"], json!({"p":3,"color":2}));
    let mut changed = base;
    changed[GEOMETRY]["a"]["p"] = json!(4);
    assert_eq!(compose(&changed, &patch).1.len(), 1);
}
#[test]
fn offline_cache_and_patch_only_edits() {
    let f = Fixture::new();
    let mut s = f.open();
    let (id, v) = first(&s, GEOMETRY);
    s.update(
        GEOMETRY,
        &id,
        scene::moved(GEOMETRY, &v, [8., 9., 10.]).unwrap(),
    )
    .unwrap();
    s.save().unwrap();
    let mut offline = f.p.clone();
    offline.assets = f.p.root.join("missing");
    offline.db_root = None;
    let s = offline.open("test", true).unwrap();
    assert_eq!(s.sources[GEOMETRY], "cached");
    assert!(s.conflicts.is_empty());
    offline.cache = f.p.root.join("empty-cache");
    let mut s = offline.open("test", true).unwrap();
    assert_eq!(s.records[GEOMETRY][&id]["p"], json!([8., 9., 10.]));
    let v = scene::moved(GEOMETRY, &s.records[GEOMETRY][&id], [2., 3., 4.]).unwrap();
    s.update(GEOMETRY, &id, v).unwrap();
    s.save().unwrap();
    assert!(export::build(&s, None, None).is_err());
}
#[test]
fn db_export_preserves_bytes_indexes_and_original() {
    let f = Fixture::new();
    let original = std::fs::read(&f.db).unwrap();
    let mut s = f.open();
    let (id, mut v) = first(&s, "_Instance_NPC");
    v["PickingRange"] = json!(12.);
    s.update("_Instance_NPC", &id, v).unwrap();
    let dest = f.p.cache.join("derived/test.db4");
    database::export(&s, &dest).unwrap();
    let c = database::connect(&dest).unwrap();
    assert_eq!(
        c.query_row("SELECT PickingRange FROM _Instance_NPC", [], |r| r
            .get::<_, f64>(0))
            .unwrap(),
        12.
    );
    assert_eq!(
        c.query_row("SELECT Data FROM _Attributes", [], |r| r
            .get::<_, Vec<u8>>(0))
            .unwrap(),
        vec![0, 0, 255, 0]
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='npc_id'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(std::fs::read(&f.db).unwrap(), original);
    assert!(database::export(&s, &f.db).is_err());
}
#[test]
fn db_backup_includes_wal() {
    let f = Fixture::new();
    let c = rusqlite::Connection::open(&f.db).unwrap();
    c.execute_batch("PRAGMA journal_mode=WAL; UPDATE _Instance_NPC SET PickingRange=15;")
        .unwrap();
    let s = f.open();
    assert_eq!(first(&s, "_Instance_NPC").1["PickingRange"], 15.);
}
#[test]
fn concurrent_saves_are_rejected() {
    let f = Fixture::new();
    let mut a = f.open();
    let mut b = f.open();
    let (id, v) = first(&a, GEOMETRY);
    a.update(
        GEOMETRY,
        &id,
        scene::moved(GEOMETRY, &v, [8., 9., 10.]).unwrap(),
    )
    .unwrap();
    a.save().unwrap();
    b.update(
        GEOMETRY,
        &id,
        scene::moved(GEOMETRY, &v, [1., 9., 10.]).unwrap(),
    )
    .unwrap();
    assert!(b.save().is_err());
}
#[test]
fn history_duplicate_delete_revert() {
    let f = Fixture::new();
    let mut s = f.open();
    let (id, _) = first(&s, "_Instance_NPC");
    let clone = s.duplicate("_Instance_NPC", &id).unwrap();
    assert_ne!(
        s.records["_Instance_NPC"][&id]["Guid"],
        s.records["_Instance_NPC"][&clone]["Guid"]
    );
    s.undo();
    assert!(s.records["_Instance_NPC"].get(&clone).is_none());
    s.redo();
    s.delete("_Instance_NPC", &clone).unwrap();
    assert_eq!(array(&s.patch["operations"]).count(), 0);
    s.delete("_Instance_NPC", &id).unwrap();
    s.revert("_Instance_NPC", &id);
    assert!(s.records["_Instance_NPC"].get(&id).is_some());
}
#[test]
fn composed_output_never_becomes_baseline() {
    let f = Fixture::new();
    let mut s = f.open();
    let (id, v) = first(&s, GEOMETRY);
    s.update(
        GEOMETRY,
        &id,
        scene::moved(GEOMETRY, &v, [8., 9., 10.]).unwrap(),
    )
    .unwrap();
    s.save().unwrap();
    export::build(&s, None, None).unwrap();
    let mut p = f.p.clone();
    p.cache = f.p.root.join("no-cache");
    let s = p.open("test", true).unwrap();
    assert_eq!(s.sources[GEOMETRY], "unavailable");
    assert!(!s.conflicts.is_empty());
}
#[test]
fn event_semantics_and_winter_scenery() {
    let records = json!({"_Instance_EventSets":{"a":{"Id":7,"EventFlags":"winter=True;anniversary=True;disabled=False;"}},"_Instance_NPC":{"b":{"EventSetRow":7}},GEOMETRY:{"g":{"event":"christmas_2012_silent"}}});
    let e = events::MapEvents::new(&records, &json!({"masked":"winter=False;"}));
    let active = ["winter".into()].into_iter().collect();
    assert!(e.visible(
        "_Instance_NPC",
        "b",
        &records["_Instance_NPC"]["b"],
        &active
    ));
    assert!(!e.visible(GEOMETRY, "masked", &json!({}), &active));
    assert!(e
        .preview_flags("christmas_2012")
        .contains("christmas_2012_silent"));
    assert!(!events::enabled(
        "winter=True;disabled=False;",
        &["winter".into(), "disabled".into()].into_iter().collect()
    ));
}
#[test]
fn camera_pose_lens_and_singleton() {
    let f = Fixture::new();
    atomic_json(f.p.assets.join("characters/selection.json"),&json!({"camera":{"pitch":-3.,"yaw":-0.2,"offset":[0.,2.,-14.5],"fov":20.},"slots":[1,2,3]})).unwrap();
    let mut s = f.p.open("a0000_char", true).unwrap();
    let (id, mut row) = first(&s, CAMERA);
    assert_eq!(scene::position(CAMERA, &row), Some([0., 2., 14.5]));
    row["FieldOfView"] = json!(35.);
    s.update(CAMERA, &id, row.clone()).unwrap();
    s.save().unwrap();
    let catalog = export::selection_export(&s).unwrap();
    assert_eq!(catalog["slots"], json!([1, 2, 3]));
    assert_eq!(catalog["camera"]["fov"], 35.);
    assert!(s.delete(CAMERA, &id).is_err());
    assert!(s.add(CAMERA, row.clone()).is_err());
    row["NearClip"] = json!(-1.);
    assert!(s.update(CAMERA, &id, row).is_err());
}
#[test]
fn lighting_color_alpha_export_and_validation() {
    let f = Fixture::new();
    let mut s = f.open();
    let (id, mut row) = first(&s, LIGHT);
    row["LightColor"] = float_blob([0.1, 0.2, 0.9, 0.7]);
    row["LightIntensity"] = json!(4.);
    s.update(LIGHT, &id, row.clone()).unwrap();
    let new = scene::new_light(&s, [1., 2., 3.], true).unwrap();
    s.add(LIGHT, new).unwrap();
    let doc = scene::lighting_document(&s).unwrap();
    assert_eq!(array(&doc["lights"]).count(), 2);
    assert_eq!(
        floats(&s.records[LIGHT][&id]["LightColor"]).unwrap()[3],
        0.7
    );
    row["LightRange"] = json!(-1.);
    assert!(s.update(LIGHT, &id, row).is_err());
}
#[test]
fn transform_gesture_preserves_untouched_precision_and_rejects_stale() {
    let f = Fixture::new();
    let mut s = f.open();
    let (id, mut v) = first(&s, GEOMETRY);
    v["p"] = json!([1.123456789, 2.987654321, 3.123456789]);
    s.update(GEOMETRY, &id, v.clone()).unwrap();
    let event = json!({"revision":4,"key":format!("geometry\t{id}"),"mode":"translate","axis":0,"position":[9.,3.,4.]});
    assert!(scene::apply_transform(&mut s, &event, 5).is_err());
    scene::apply_transform(&mut s, &event, 4).unwrap();
    assert_eq!(s.records[GEOMETRY][&id]["p"][1], v["p"][1]);
    s.undo();
    assert_eq!(s.records[GEOMETRY][&id], v);
}
#[test]
fn source_guard_and_schema_identity_validation() {
    let f = Fixture::new();
    let mut p = f.p.clone();
    p.overrides = p.assets.join("overrides");
    assert!(p.checked().is_err());
    let mut s = f.open();
    let (id, mut v) = first(&s, "_Instance_NPC");
    v["Guid"] = blob(&[9; 16]);
    assert!(s.update("_Instance_NPC", &id, v).is_err());
}
