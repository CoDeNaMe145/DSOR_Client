//! Character creation against the release 261 server (tools/chargen_vectors.py):
//! the client's requests as its decoder reads them, its answers as it writes them.
use dsor_proto::commands::player::CharacterGeneration;
use dsor_proto::commands::{decode_message, ClientCommand, ServerCommand};
use serde_json::Value;

fn vectors() -> Vec<Value> {
    serde_json::from_str(include_str!("data/chargen_release261.json")).unwrap()
}

fn bytes(name: &str) -> (Vec<u8>, usize) {
    let v = vectors().into_iter().find(|v| v["name"] == name).unwrap();
    let hex = v["hex"].as_str().unwrap();
    let bytes = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
    (bytes, v["bits"].as_u64().unwrap() as usize)
}

fn identity() -> [f32; 16] {
    std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 })
}

#[test]
fn requests_encode_as_the_server_decodes_them() {
    for (name, command) in [
        ("open_editor", CharacterGeneration { operation: 1, matrix: identity(), ..Default::default() }),
        ("create", CharacterGeneration { operation: 3, player_class: 3, gender: 1, hair: 31, beard: 16, body: 2,
            name: "Coppergear".into(), matrix: identity(), ..Default::default() }),
    ] {
        let (want, want_bits) = bytes(name);
        assert_eq!(ClientCommand::CharacterGeneration(command).encode(), (want, want_bits), "{name}");
    }
}

#[test]
fn the_editor_a_refusal_and_a_creation_decode() {
    let decode = |name: &str| {
        let (data, bits) = bytes(name);
        match decode_message(&data, bits).unwrap().remove(0) {
            (ServerCommand::CharacterGeneration(c), _) => c,
            other => panic!("{name}: {other:?}"),
        }
    };
    let editor = decode("editor");
    assert_eq!((editor.operation, editor.slot, editor.hair, editor.beard, editor.words[3]), (2, 7, 18, 8, 1362));
    assert_eq!(editor.text, "en");
    let refused = decode("refused");
    assert_eq!((refused.operation, refused.deny_reason, refused.name.as_str()), (4, 1, "Coppergear"));
    let created = decode("created");
    assert_eq!((created.operation, created.character, created.player_class, created.body), (5, 1234, 3, 2));
}
