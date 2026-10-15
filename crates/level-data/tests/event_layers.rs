use dsor_level_data::*;
use serde_json::json;
fn instance(template: i16, mapping: u16, name: u16, p: [f32; 3]) -> Vec<u8> {
    let mut b = vec![];
    for f in [p[0], p[1], p[2], 1., 0., 0., 0., 1.] {
        b.extend(f.to_le_bytes());
    }
    b.extend([0, 1, 1]);
    b.extend(template.to_le_bytes());
    b.extend((-1i16).to_le_bytes());
    b.extend(name.to_le_bytes());
    b.extend(mapping.to_le_bytes());
    b
}
fn files(root: &std::path::Path) -> (std::path::PathBuf, serde_json::Value) {
    let source = root.join("test.map");
    let strings = "\0props/tree\0effects/snow\0christmas_2012\0christmas_2012_silent\0tree\0";
    let mut b = b"MOSD".to_vec();
    b.extend(5u32.to_le_bytes());
    let header = b.clone();
    b.extend(b"IPAM");
    b.extend([0; 54]);
    b.extend(b"TRTS");
    b.extend(6u32.to_le_bytes());
    b.extend((strings.len() as u32).to_le_bytes());
    b.extend(strings.as_bytes());
    b.extend(b"TTES");
    b.extend(0u32.to_le_bytes());
    b.extend(b"TEVE");
    for n in [2u32, 3, 4] {
        b.extend(n.to_le_bytes());
    }
    b.extend(b"LAME");
    b.extend(2u32.to_le_bytes());
    for mask in ["11", "10"] {
        b.extend(2u16.to_le_bytes());
        b.extend(mask.as_bytes());
    }
    b.extend(b"LPMT");
    b.extend(2u32.to_le_bytes());
    for n in [1u16, 2] {
        b.extend(n.to_le_bytes());
        b.extend([0; 44]);
    }
    b.extend(b"TSNI");
    b.extend(1u32.to_le_bytes());
    b.extend(instance(0, 1, 5, [1., 2., 3.]));
    std::fs::write(&source, b).unwrap();
    let mut eve = header;
    eve.extend(b"TSNI");
    eve.extend(1u32.to_le_bytes());
    eve.extend(instance(1, 0, 0, [4., 5., 6.]));
    std::fs::write(root.join("test_christmas_2012_silent.eve"), eve).unwrap();
    let manifest = json!({"map":"test","models":["props/tree"],"instances":[{"m":0,"p":[1.,2.,3.],"r":[0.,0.,0.,1.],"s":[1.,1.,1.],"n":"tree","g":-1,"col":true}]});
    (source, manifest)
}
#[test]
fn parent_templates_winter_snow_and_base_masks() {
    let t = tempfile::tempdir().unwrap();
    let (path, manifest) = files(t.path());
    let enriched = events::with_event_layers(&manifest, &path, &serde_json::Value::Null).unwrap();
    let records = geometry_records(&enriched);
    let e = events::MapEvents::new(&json!({GEOMETRY:records}), &enriched["event_rules"]);
    let active = e.preview_flags("christmas_2012");
    for (id, row) in object(&records) {
        if row["model"] == "props/tree" {
            assert!(e.visible(GEOMETRY, id, row, &Default::default()));
            assert!(!e.visible(GEOMETRY, id, row, &active));
        } else {
            assert_eq!(row["p"], json!([4., 5., 6.]));
            assert!(!e.visible(GEOMETRY, id, row, &Default::default()));
            assert!(e.visible(GEOMETRY, id, row, &active));
        }
    }
}
#[test]
fn missing_eve_preserves_cached_layer_and_truncation_errors() {
    let t = tempfile::tempdir().unwrap();
    let (path, manifest) = files(t.path());
    let enriched = events::with_event_layers(&manifest, &path, &serde_json::Value::Null).unwrap();
    let eve = t.path().join("test_christmas_2012_silent.eve");
    std::fs::rename(&eve, eve.with_extension("missing")).unwrap();
    assert_eq!(
        events::with_event_layers(&manifest, &path, &enriched).unwrap(),
        enriched
    );
    std::fs::write(&eve, b"MOSD").unwrap();
    assert!(events::with_event_layers(&manifest, &path, &enriched).is_err());
}
