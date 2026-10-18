use super::*;
use dsor_proto::commands::{combat::{StatusEffectElement, StatusEffectTail}, inventory::{ItemInfo, ItemStatistic}};

#[test]
fn full_loadouts_match_authoritative_server_vectors() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!("../../../tests/fixtures/character_stats.json")).unwrap();
    let data: StatsData = serde_json::from_value(fixture["data"].clone()).unwrap();
    let u = |v: &serde_json::Value, key: &str| v[key].as_u64().unwrap_or(0) as u32;
    let f = |v: &serde_json::Value, key: &str| v[key].as_f64().unwrap_or(0.0) as f32;
    let s = |v: &serde_json::Value, key: &str| v[key].as_str().unwrap_or("").to_owned();
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let mut inventory = crate::gameplay::inventory::InventoryState::default();
        for item in case["inventory"]["items"].as_array().unwrap() {
            let item = ItemInfo {
                id: u(item,"item_id"), template: s(item,"template_id"), third: u(item,"amount"),
                kind: u(item,"field_08") as u8, eighteenth: u(item,"field_18"), durability: u(item,"item_level"), tier: u(item,"field_38"),
                names: item["socketed_templates"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect(),
                statistics: item["enchantments"].as_array().unwrap().iter().map(|e| ItemStatistic {
                    name: s(e,"template_id"), value: f(e,"value"), kind: u(e,"kind") as i8, third: u(e,"field_08"), ..default()
                }).collect(), ..default()
            };
            inventory.items.insert(item.id,item);
        }
        inventory.allocation.slots = case["inventory"]["equipment_slots"].as_object().unwrap().iter()
            .map(|(id,slots)| (id.parse().unwrap(), slots.as_array().unwrap().iter().map(|n| n.as_i64().unwrap() as i8).collect())).collect();
        let effects = case["effects"].as_array().unwrap().iter().map(|e| StatusEffectElement {
            index: u(e,"effect_id") as u16, instance: u(e,"field_04"), spare: u(e,"field_18"),
            tail: Some(StatusEffectTail { parameters: e["values"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect(), ..default() }), ..default()
        });
        let actual = calculation::calculate(&data,case["cls"].as_str().unwrap(),u(case,"level"),&inventory,effects);
        for (key,expected) in case["expected"].as_object().unwrap() {
            let expected = expected.as_f64().unwrap();
            let actual = actual.get(key).copied().unwrap_or(f64::NAN);
            // Protocol floats are f32; the server fixture retains Python doubles.
            let tolerance = (expected.abs()*2e-6).max(0.002);
            if !actual.is_finite() || (actual-expected).abs() > tolerance {
                failures.push(format!("{} {key}: {actual} != {expected}",case["name"]));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn animation_rate_uses_the_server_breakpoints() {
    for (raw, expected) in [(0.7,0.7),(1.0,1.0),(1.0255,1.0),(1.0256,1.25),(2.1053,2.5),(3.6364,4.1667)] {
        let stats = CharacterStats([("SkillExecutionSpeed".into(),raw)].into_iter().collect());
        assert!((stats.animation_speed()-expected).abs() < 1e-6);
    }
}
