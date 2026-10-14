//! Fixtures produced by the Python server, including its two-packet mutation.
use dsor_proto::commands::{decode_message, encode_chain, encode_single, ClientCommand, ServerCommand};
use serde_json::Value;

fn vectors() -> Vec<Value> {
    serde_json::from_str(include_str!("data/inventory_release261.json")).unwrap()
}
fn bytes(v: &Value) -> Vec<u8> {
    let s = v["hex"].as_str().unwrap();
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn inventory_mutations_and_corrections_match_the_server() {
    for v in vectors().into_iter().filter(|v| !v["name"].as_str().unwrap().ends_with("request")) {
        let data = bytes(&v);
        let bits = v["bits"].as_u64().unwrap() as usize;
        let commands = decode_message(&data, bits).unwrap();
        assert!(commands.iter().all(|(c, a)| !c.is_unknown() && *a == Some(1)), "{}", v["name"]);
        let (encoded, encoded_bits) = if data[0] == 0x84 {
            encode_single(&commands[0].0, 1)
        } else {
            encode_chain(&commands.iter().map(|(c, _)| (c.clone(), 1)).collect::<Vec<_>>())
        };
        assert!(bits.abs_diff(encoded_bits) <= 7);
        for bit in 0..bits.min(encoded_bits) {
            assert_eq!((encoded[bit / 8] >> (7 - bit % 8)) & 1, (data[bit / 8] >> (7 - bit % 8)) & 1, "{} bit {bit}", v["name"]);
        }
        match v["name"].as_str().unwrap() {
            "initial" => {
                let ServerCommand::InventoryInfo(i) = &commands[0].0 else { panic!() };
                assert_eq!(i.items.iter().map(|i| (i.id, i.third, i.durability)).collect::<Vec<_>>(), [(101, 10, 12), (102, 8, 12)]);
                assert_eq!(i.storage_sizes[2], 224);
            }
            "split" => {
                let ServerCommand::InventoryInfo(delta) = &commands[0].0 else { panic!() };
                let ServerCommand::InventoryInfo(final_state) = &commands[1].0 else { panic!() };
                assert_eq!(delta.map_158, [(200, 101)]);
                assert_eq!(delta.items.iter().map(|i| (i.id, i.third)).collect::<Vec<_>>(), [(200, 7), (201, 3)]);
                assert!(final_state.items.is_empty());
                assert_eq!(final_state.placements, delta.placements);
            }
            "force" => {
                let ServerCommand::ForceTransform(t) = &commands[0].0 else { panic!() };
                assert_eq!(t.position, [14.0, 0.0, 45.0]);
                assert!((t.heading + 1.6433599).abs() < 0.00001);
            }
            "item_update" => {
                let ServerCommand::ItemUpdate(i) = &commands[0].0 else { panic!() };
                assert_eq!((i.item, i.third, i.tenth, i.level), (101, 7, 2, 19));
                assert_eq!(i.unknown_strings_0, ["gem_a", "gem_b"]);
                assert_eq!(i.appearance, "skin_a");
                assert!(i.flag_2);
            }
            "premium" | "expired" => {
                let ServerCommand::MembershipUpdate(m) = &commands[0].0 else { panic!() };
                assert_eq!(m.tier, 1);
                assert_eq!(m.active, v["name"] == "premium");
                if m.active { assert_eq!(&m.expires[..6], &[2099, 7, 8, 9, 10, 0]); assert_eq!(m.expires[7], 6); }
            }
            "overflow" => {
                let ServerCommand::InventoryInfo(i) = &commands[0].0 else { panic!() };
                assert_eq!(i.overflow, [301]);
                assert_eq!((i.items[0].id, i.items[0].third), (301, 9));
                assert!(i.placements.is_empty() && i.extra.is_empty());
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn split_and_merge_requests_match_server_decoding() {
    for v in vectors().into_iter().filter(|v| v["name"].as_str().unwrap().ends_with("request")) {
        let data = bytes(&v);
        let bits = v["bits"].as_u64().unwrap() as usize;
        let c = ClientCommand::decode(&data, bits).unwrap();
        let ClientCommand::Inventory(i) = &c else { panic!() };
        assert_eq!(i.item, 101);
        if v["name"] == "split_request" {
            assert_eq!((i.operation, i.arguments.as_slice()), (0x1e, &[7, 2][..]));
        } else {
            assert_eq!((i.operation, i.arguments.as_slice()), (0x0d, &[102][..]));
        }
        assert_eq!(c.encode(), (data, bits));
    }
}
