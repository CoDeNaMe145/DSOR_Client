//! Travel, talents, logout and the PvP queue against the release 261 server
//! (tools/misc_vectors.py): the requests as its decoders read them (its own
//! encoders for the talent commands), its answers as it writes them.
use dsor_proto::commands::inventory::{ResetTalentCategory, ResetTalents, SelectTalent, TalentPreselection, UseTravelItem};
use dsor_proto::commands::player::{Logout, Travel};
use dsor_proto::commands::social::JoinMatchQueue;
use dsor_proto::commands::{decode_message, ClientCommand, ServerCommand};
use serde_json::Value;

fn vectors() -> Vec<Value> {
    serde_json::from_str(include_str!("data/misc_release261.json")).unwrap()
}

fn bytes(v: &Value) -> (Vec<u8>, usize) {
    let hex = v["hex"].as_str().unwrap();
    let bytes = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
    (bytes, v["bits"].as_u64().unwrap() as usize)
}

#[test]
fn requests_encode_as_the_server_decodes_them() {
    let mut seen = 0;
    for v in vectors() {
        let command = match v["name"].as_str().unwrap() {
            "travel_map" => ClientCommand::Travel(Travel::to_map("a0200_kingscity")),
            "travel_event" => ClientCommand::Travel(Travel { source: String::new(), map: "a0200_kingscity".into(), entry: "a0200_fairground".into(), force: false }),
            "use_travel_item" => ClientCommand::UseTravelItem(UseTravelItem { item: 800, map: "a0203_kingscrypt02".into() }),
            "logout" => ClientCommand::Logout(Logout { cancel: false, plain: false }),
            "select_talent" => ClientCommand::SelectTalent(SelectTalent { talent: "skill_mine_lvl".into(), ranks: 1 }),
            "preselection" => ClientCommand::TalentPreselection(TalentPreselection { preselection: 2, category: 0 }),
            "reset_talents" => ClientCommand::ResetTalents(ResetTalents { category: 1, preselection: 2 }),
            "reset_branch" => ClientCommand::ResetTalentCategory(ResetTalentCategory { category: 1, branch: "wisdom_branch_a".into(), preselection: 0 }),
            "join_queue" => ClientCommand::JoinMatchQueue(JoinMatchQueue { tick: 7, mode: 0, kind: 1 }),
            _ => continue,
        };
        seen += 1;
        let (want, want_bits) = bytes(&v);
        let (got, got_bits) = command.encode();
        // The server's own encoders round their bit counts up to the byte.
        assert_eq!(got, want, "{}", v["name"]);
        assert!(got_bits == want_bits || got_bits.div_ceil(8) == want_bits.div_ceil(8), "{}: {got_bits} vs {want_bits} bits", v["name"]);
    }
    assert_eq!(seen, 9);
}

#[test]
fn the_logout_countdown_and_the_talent_points_decode() {
    let v = vectors().into_iter().find(|v| v["name"] == "logout_info").unwrap();
    let (data, bits) = bytes(&v);
    let commands = decode_message(&data, bits).unwrap();
    let (ServerCommand::PlayerLogoutInfo(l), actor) = &commands[0] else { panic!("{commands:?}") };
    assert_eq!((l.seconds, *actor), (10, None));
    let v = vectors().into_iter().find(|v| v["name"] == "talent_points").unwrap();
    let (data, bits) = bytes(&v);
    let commands = decode_message(&data, bits).unwrap();
    let (ServerCommand::TalentCategoryPointsInfo(p), actor) = &commands[0] else { panic!("{commands:?}") };
    // The server's values are per category (skill, wisdom, group, fame); the wire
    // carries them as skill, fame, wisdom, group.
    assert_eq!((p.category, p.points, *actor), (0xFF, [10, 40, 20, 30], Some(1)));
    assert_eq!((p.of(0), p.of(1), p.of(2), p.of(3)), (10, 20, 30, 40));
    let (ServerCommand::UnlockSkillTalentPreselectionInfo(u), _) = &commands[1] else { panic!() };
    assert_eq!(u.unlocked, 3);
}

/// Items received (MessageCommand type 2) as DSOR_Server's item_receipt_protocol
/// writes them: source 7, the amount, the item's id as an item param, and the
/// template for a currency the reward made no item of.
#[test]
fn items_received_decode() {
    use dsor_proto::commands::social::MessageParam::{Int, Item, Str};
    let v = vectors().into_iter().find(|v| v["name"] == "item_received").unwrap();
    let (data, bits) = bytes(&v);
    let commands = decode_message(&data, bits).unwrap();
    let (ServerCommand::Message(chest), actor) = &commands[0] else { panic!("{commands:?}") };
    assert_eq!((chest.message_type, chest.params.as_slice(), *actor), (2, &[Int(7), Int(1), Item(4012)][..], Some(1)));
    let (ServerCommand::Message(honor), _) = &commands[1] else { panic!("{commands:?}") };
    assert_eq!(honor.params, [Int(7), Int(50), Item(4013), Str("money_hc_no_auto_use".into())]);
}
