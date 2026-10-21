//! Encoded by the release 261 server's protocol/shop_protocol.py, including
//! a real jewelry_hub_all response and nonempty metadata/payment lists.
use dsor_proto::commands::{decode_message, encode_single, ServerCommand};
use serde_json::Value;

#[test]
fn vendor_offers_decode_and_round_trip_the_server_packets() {
    let vectors: Vec<Value> = serde_json::from_str(include_str!("data/shop_release261.json")).unwrap();
    for v in vectors {
        let hex = v["hex"].as_str().unwrap();
        let bytes: Vec<_> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        let bits = v["bits"].as_u64().unwrap() as usize;
        let commands = decode_message(&bytes, bits).unwrap();
        let (command, actor) = commands.last().unwrap();
        let ServerCommand::Offer(offer) = command else { panic!("{}: expected decoded offer", v["name"]) };
        if v["name"] == "batch" {
            assert_eq!(commands.len(), 2);
            assert_eq!(offer.shop, "jewelry_hub_all");
        } else {
            let (encoded, encoded_bits) = encode_single(command, actor.unwrap());
            assert_eq!(encoded_bits, bits, "{}", v["name"]);
            assert_eq!(encoded, bytes, "{}", v["name"]);
        }
        if v["name"] == "metadata" {
            let o = offer.offers[0].as_ref().unwrap();
            assert_eq!((o.price_rc, o.price_vc, o.price_arena, o.price_gc, o.price_ec, o.price_uc, o.price_ic), (10800, 12345, 17, 23, 29, 31, 37));
            assert_eq!((o.category, o.rarity), (46, 7));
            assert_eq!(o.icon_config, "unique");
            assert!(!o.item.is_empty());
            assert!(o.is_recommended && o.is_new && o.once_per_character && o.event_timed && o.timed_offer);
            // release261_192's ninth flag, between event_timed and the rarity.
            assert!(o.directly_buy && !o.hide_not_suitable);
            assert_eq!(o.veteran_discounts, [("Veteran3".into(), 0.05)]);
            assert_eq!(offer.payment_items[0].as_ref().unwrap().payment_item, 51);
            assert!(offer.offers[1].is_none() && offer.payment_items[1].is_none());
            assert_eq!(offer.categories, [46, 43, 7]);
        } else if v["name"] == "buyback" {
            assert!(offer.keep_shop && offer.offers.is_empty());
        } else {
            // The server's jewelry_hub_all: its static offers plus the max-tier
            // trinket gems and fully socketed trinkets it adds (2026-10-21).
            assert_eq!(offer.categories, [46, 4, 30]);
            assert_eq!(offer.offers.len(), 137);
            assert!(offer.offers.iter().flatten().all(|o| !o.item.is_empty()));
        }
    }
}
