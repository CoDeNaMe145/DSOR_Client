use dsor_proto::commands::{decode_message, encode_chain, ServerCommand};

#[test]
fn production_npc_and_quest_messages_preserve_markers_and_chain_alignment() {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!("data/map_markers_release261.json")).unwrap();
    for fixture in fixtures.as_array().unwrap() {
        let hex = fixture["hex"].as_str().unwrap();
        let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        let bits = fixture["bits"].as_u64().unwrap() as usize;
        let commands = decode_message(&bytes, bits).unwrap_or_else(|e| panic!("{}: {e}", fixture["name"]));
        for (command, actor) in &commands {
            match command {
                ServerCommand::NpcInfo(n) => {
                    assert_eq!(*actor, Some(77));
                    assert_eq!(n.interactions.len(), 2);
                    assert_eq!(n.string_sets[0].values, ["item1", "item2"]);
                    assert_eq!(n.links[0].value, 17);
                }
                ServerCommand::QuestLogInfo(q) => {
                    assert_eq!(q.records.len(), 2);
                    assert_eq!(q.records[0].tracking_id, "tracking");
                }
                ServerCommand::QuestUpdate(q) => assert_eq!(q.strings[1].0, 6),
                _ => panic!("unexpected command"),
            }
        }
        let owned: Vec<_> = commands.into_iter().map(|(c, a)| (c, a.unwrap_or_default())).collect();
        let encoded = encode_chain(&owned);
        assert_eq!(encoded, (bytes, bits));
    }
}
