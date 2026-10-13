//! Every layout against the release261 production server's own encoders.
//!
//! tests/data/release261.json is written by tools/prod_vectors.py from
//! dso-server-production's protocol modules (the server this client talks to):
//! each vector is a whole message (0x84 single, 0x85 chain or 0x8B client) with
//! distinctive values in every field. Each must decode with no unknown command and
//! no bit left over, name the commands the encoder put in it, and encode back to
//! the same bits.

use dsor_proto::commands::{decode_message, encode_chain, encode_single, ClientCommand, ServerCommand};
use serde_json::Value;

fn vectors() -> Vec<Value> {
    let text = include_str!("data/release261.json");
    match serde_json::from_str(text).unwrap() {
        Value::Array(v) => v,
        _ => panic!("release261.json is not a list"),
    }
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// The class name, as the vectors spell it ("HitCommand"), of a decoded command.
fn class(c: &ServerCommand) -> String {
    let name = dsor_proto::commands::command_name(c.id()).unwrap_or("?");
    name.rsplit("::").next().unwrap().to_owned()
}

#[test]
fn every_server_vector_decodes_and_encodes_back() {
    let mut failures = Vec::new();
    for v in vectors() {
        let name = v["name"].as_str().unwrap();
        let envelope = v["envelope"].as_str().unwrap();
        if envelope == "0x8B" {
            continue;
        }
        let data = hex(v["hex"].as_str().unwrap());
        let bits = v["bits"].as_u64().unwrap() as usize;
        let got = match decode_message(&data, bits) {
            Ok(g) => g,
            Err(e) => {
                failures.push(format!("{name} ({envelope}): {e}"));
                continue;
            }
        };
        if let Some(c) = got.iter().find(|(c, _)| c.is_unknown()) {
            failures.push(format!("{name} ({envelope}): unknown command {}", c.0.id()));
            continue;
        }
        let names: Vec<String> = got.iter().map(|(c, _)| class(c)).collect();
        let expected: Vec<String> = match v.get("batch").and_then(|b| b.as_array()) {
            Some(b) => b.iter().map(|n| n.as_str().unwrap().replace("XPChanged", "XPChanged")).collect(),
            None => vec![name.to_owned()],
        };
        if names != expected {
            failures.push(format!("{name} ({envelope}): decoded {names:?}, encoder wrote {expected:?}"));
            continue;
        }
        let (again, again_bits) = if envelope == "0x84" {
            let (c, a) = &got[0];
            encode_single(c, a.unwrap_or(0))
        } else {
            let list: Vec<(ServerCommand, u32)> = got.iter().map(|(c, a)| (c.clone(), a.unwrap_or(0))).collect();
            encode_chain(&list)
        };
        // A 0x85 chain may end on up to seven padding bits; compare the bits both carry.
        let same = again_bits.min(bits);
        let bit = |d: &[u8], i: usize| (d[i / 8] >> (7 - i % 8)) & 1;
        if bits.abs_diff(again_bits) > 7 || (0..same).any(|i| bit(&data, i) != bit(&again, i)) {
            failures.push(format!("{name} ({envelope}): re-encoded bits differ ({bits} vs {again_bits})"));
        }
    }
    assert!(failures.is_empty(), "{} vector(s) failed:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn every_client_vector_decodes_and_encodes_back() {
    let mut failures = Vec::new();
    for v in vectors() {
        if v["envelope"].as_str() != Some("0x8B") {
            continue;
        }
        let name = v["name"].as_str().unwrap();
        let data = hex(v["hex"].as_str().unwrap());
        let bits = v["bits"].as_u64().unwrap() as usize;
        match ClientCommand::decode(&data, bits) {
            Ok(c) => {
                let (again, again_bits) = c.encode();
                if again_bits != bits || again[..] != data[..] {
                    failures.push(format!("{name}: re-encoded {again_bits} bits, expected {bits}"));
                }
            }
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    assert!(failures.is_empty(), "{} vector(s) failed:\n{}", failures.len(), failures.join("\n"));
}

/// The first command named `name` in the first vector whose encoder wrote it.
fn first(name: &str) -> (ServerCommand, Option<u32>) {
    for v in vectors() {
        if v["envelope"].as_str() == Some("0x8B") {
            continue;
        }
        let got = decode_message(&hex(v["hex"].as_str().unwrap()), v["bits"].as_u64().unwrap() as usize).unwrap();
        if let Some(c) = got.into_iter().find(|(c, _)| class(c) == name) {
            return c;
        }
    }
    panic!("no vector carries {name}");
}

#[test]
fn the_fields_the_client_reads_land_where_the_server_put_them() {
    // Values: tools/prod_vectors.py's inputs, as recorded in each vector's "fields".
    let (ServerCommand::NewMonster(m), actor) = first("NewMonsterCommand") else { panic!() };
    assert_eq!((m.blueprint.as_str(), m.health, m.max_health, m.level), ("MonsterTemplateA", 4886718345, 9468016794, 2627509404));
    assert_eq!((m.position, m.rank.as_str(), actor), ([101.5, 202.25, -3.125], "Champion", Some(197637)));

    let (ServerCommand::MonsterUpdate(u), actor) = first("MonsterUpdateCommand") else { panic!() };
    assert_eq!((u.health, u.max_health, actor), (4294975520, 8589944872, Some(262145)));

    let (ServerCommand::Hit(h), actor) = first("HitCommand") else { panic!() };
    assert_eq!((h.tick, h.damage_types.as_slice(), h.blocked, h.critical), (287454020, &[10u8, 11, 12][..], true, false));
    assert_eq!((h.victim_health, h.victim_max_health, h.damage, h.kind), (4328719365, 8691058697, 202116108, 13));
    assert_eq!((h.attacker, h.combat_value_owner, h.combat_text.as_str(), actor), (921102, 986895, "DamageDonePoison", Some(589825)));

    let (ServerCommand::ActorStatsUpdate(s), _) = first("ActorStatsUpdateCommand") else { panic!() };
    assert_eq!((s.health, s.resource), (4582421316, 77.5));

    let (ServerCommand::CurrencyChanged(c), _) = first("CurrencyChangedCommand") else { panic!() };
    assert_eq!((c.wallet, c.unknown_u64_5, c.notify), ([273, 4294967842, 819, 1092, 1365], 8589936230, true));

    let (ServerCommand::XpChanged(x), _) = first("XPChangedCommand") else { panic!() };
    assert_eq!((x.total, x.level, x.floor, x.ceiling, x.levelled), (74565, 33, 65536, 131072, true));

    let (ServerCommand::NewRemotePlayer(p), actor) = first("NewRemotePlayerCommand") else { panic!() };
    assert_eq!((p.name.as_str(), p.parts[0], p.parts[1], p.character_id, p.account_id), ("RemoteHero", 2, 1, 4294970044, 8589938159));
    assert_eq!((p.position, p.heading, p.equipment.len(), actor), ([15.5, 16.25, 17.125], 1.5, 2, Some(983297)));
    assert_eq!(p.equipment[0].skin_parts, ["skinA", "skinB"]);

    let (ServerCommand::CharacterSelection(sel), _) = first("CharacterSelectionCommand") else { panic!() };
    assert_eq!(sel.characters.len(), 2);
    assert_eq!((sel.characters[0].name.as_str(), sel.characters[0].unknown_u32_1, sel.characters[0].level), ("Alpha", 4886718345, 21));
}
