use dsor_proto::commands::{decode_message, encode_chain, encode_single, ServerCommand};
use serde_json::Value;

#[test]
fn server_generated_props_and_events_decode_and_preserve_every_bit() {
    let vectors: Vec<Value> =
        serde_json::from_str(include_str!("data/props_release261.json")).unwrap();
    for vector in vectors {
        let hex = vector["hex"].as_str().unwrap();
        let data: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let bits = vector["bits"].as_u64().unwrap() as usize;
        let commands =
            decode_message(&data, bits).unwrap_or_else(|e| panic!("{}: {e}", vector["name"]));
        assert_eq!(
            commands
                .iter()
                .map(|(c, _)| u64::from(c.id()))
                .collect::<Vec<_>>(),
            vector["ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_u64().unwrap())
                .collect::<Vec<_>>()
        );
        for (c, actor) in &commands {
            match c {
                ServerCommand::PropInfo(info) => {
                    assert_eq!(*actor, Some(12345));
                    let last = info.entries.last().unwrap();
                    assert_eq!(last.task_index, 5);
                    assert_eq!(last.quest_type, 13);
                    assert_eq!(last.position, [46., 1., -45.]);
                }
                ServerCommand::EventUpdate(events) => {
                    assert_eq!(*actor, None);
                    assert_eq!(events.entries.len(), 3);
                    assert!(events.entries.iter().all(|e| e.unknown_bit_3));
                    assert!(events.entries[1].map_local);
                    assert_eq!(events.entries[1].key, "a0200_crafting_npc_exchange");
                }
                ServerCommand::NewProp(p) => {
                    assert_eq!(p.0.task_index, 5);
                    assert_eq!(p.0.quest_type, 13);
                }
                ServerCommand::PropUpdate(p) => {
                    assert_eq!(
                        (p.state, p.variant, p.unknown_cc, p.unknown_d0),
                        (3, 2, -7, 23)
                    );
                    assert!(p.state_changed && p.operation_changed && !p.keep_visual_state);
                }
                ServerCommand::DiscardProp(_) => {}
                _ => panic!("unexpected command: {c:?}"),
            }
        }
        let (again, again_bits) = if data[0] == 0x84 {
            encode_single(&commands[0].0, commands[0].1.unwrap_or(0))
        } else {
            encode_chain(
                &commands
                    .into_iter()
                    .map(|(c, a)| (c, a.unwrap_or(0)))
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(again_bits, bits, "{}", vector["name"]);
        assert_eq!(again, data, "{}", vector["name"]);
    }
}
