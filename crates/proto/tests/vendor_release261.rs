//! Vendor transactions against the release 261 server (tools/vendor_vectors.py):
//! TransactionCommand bytes as its protocol/commerce_protocol.py decode_transaction
//! reads them, and a purchase's reply as services/commerce.py chains it.
use dsor_proto::commands::{decode_message, inventory::Transaction, ClientCommand, ServerCommand};
use serde_json::Value;

fn vectors() -> Vec<Value> {
    serde_json::from_str(include_str!("data/vendor_release261.json")).unwrap()
}

fn bytes(v: &Value) -> (Vec<u8>, usize) {
    let hex = v["hex"].as_str().unwrap();
    let bytes = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
    (bytes, v["bits"].as_u64().unwrap() as usize)
}

#[test]
fn transactions_encode_as_the_server_decodes_them() {
    let mut seen = 0;
    for v in vectors() {
        let command = match v["name"].as_str().unwrap() {
            "buy" => Transaction::buy("jewelry_hub_all", "jewelry_ring_01", 2),
            "sell" => Transaction::sell("smith_hub_a0200", 800),
            "restore" => Transaction::restore("smith_hub_a0200", 800),
            _ => continue,
        };
        seen += 1;
        let want = bytes(&v);
        assert_eq!(ClientCommand::Transaction(command.clone()).encode(), want, "{}", v["name"]);
        // The server's own decoder order, read back.
        let entry = command.entries[0].as_ref().unwrap();
        assert_eq!(entry.extra, 0);
        assert!(!command.mode);
    }
    assert_eq!(seen, 3);
}

#[test]
fn a_purchase_reply_decodes_every_command_in_order() {
    let v = vectors().into_iter().find(|v| v["name"] == "purchase_reply").unwrap();
    let (data, bits) = bytes(&v);
    let commands = decode_message(&data, bits).unwrap();
    let names: Vec<_> = commands.iter().map(|(c, _)| c.name()).collect();
    assert_eq!(names, [
        "UpdateProgressTierSpenderChallengesCommand", "TimedOffersInfoCommand", "InventoryInfoCommand",
        "CurrencyChangedCommand", "GameCurrencyChangedCommand", "InventoryInfoCommand", "QuickbarInfoCommand",
        "CurrencyChangedCommand",
    ]);
    assert!(commands.iter().all(|(c, actor)| !c.is_unknown() && *actor == Some(1)));
    let ServerCommand::TierSpenderProgress(t) = &commands[0].0 else { panic!() };
    assert_eq!(t.amount, 40);
    let ServerCommand::TimedOffersInfo(t) = &commands[1].0 else { panic!() };
    assert_eq!((t.server_time, t.offers.len()), (0, 0));
    let ServerCommand::InventoryInfo(delta) = &commands[2].0 else { panic!() };
    assert_eq!(delta.items.len(), 1);
    assert_eq!(delta.items[0].template, "jewelry_ring_01");
    let ServerCommand::CurrencyChanged(c) = &commands[3].0 else { panic!() };
    assert_eq!(c.wallet, [1234, 56789, 11, 22, 33]);
    let ServerCommand::GameCurrencyChanged(g) = &commands[4].0 else { panic!() };
    assert_eq!((g.currency.as_str(), g.amount), ("materi_fragment_01", 5));
    let ServerCommand::QuickbarInfo(q) = &commands[6].0 else { panic!() };
    assert_eq!((q.quickbars, q.locked), (1, 2));
}

#[test]
fn timed_offers_round_trip() {
    let v = vectors().into_iter().find(|v| v["name"] == "timed_offer").unwrap();
    let (data, bits) = bytes(&v);
    let commands = decode_message(&data, bits).unwrap();
    let ServerCommand::TimedOffersInfo(t) = &commands[0].0 else { panic!() };
    assert_eq!(t.server_time, 1700000000);
    assert_eq!(t.offers[0].offer, "daily_ring");
    assert_eq!((t.offers[0].unknown_u32_0, t.offers[0].unknown_u32_1), (7, 9));
    assert_eq!(t.offers[0].ends_at, [2026, 10, 21, 12, 30, 0]);
    let chain: Vec<_> = commands.iter().map(|(c, actor)| (c.clone(), actor.unwrap())).collect();
    assert_eq!(dsor_proto::commands::encode_chain(&chain), (data, bits));
}
