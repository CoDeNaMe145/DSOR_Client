//! A player's summon (a Shadow Soul) as the release 261 server publishes it
//! (tools/summon_vectors.py): its NewMonster batch as its owner, a peer and a
//! PvP-hostile peer receive it, a map companion, the map's own monsters, its blow
//! on a monster and its discard; and how the player's faction stands toward each.
use dsor_proto::commands::actors::{Faction, NewMonster, Stance, HOSTILE_FACTION_TYPE, MONSTER_FACTION, PLAYER_FACTION, PLAYER_OWNED_FACTION};
use dsor_proto::commands::{decode_message, encode_single, ServerCommand};
use serde_json::Value;

const SOUL: u32 = 0x10600;
const OWNER: u32 = 0x2000001;
/// The faction every player gets in NewPlayer (protocol/gameplay_commands.py).
const PLAYER: Faction = Faction { faction_id: PLAYER_FACTION, unknown_u32_0: 1, faction_type: 0, unknown_flag_0: true, unknown_flag_1: false };

fn decoded(name: &str) -> Vec<(ServerCommand, Option<u32>)> {
    let vectors: Vec<Value> = serde_json::from_str(include_str!("data/summon_release261.json")).unwrap();
    let v = vectors.into_iter().find(|v| v["name"] == name).unwrap_or_else(|| panic!("no vector {name}"));
    let hex = v["hex"].as_str().unwrap();
    let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
    decode_message(&bytes, v["bits"].as_u64().unwrap() as usize).unwrap()
}

fn new_monster(name: &str) -> (NewMonster, Option<u32>) {
    let commands = decoded(name);
    match commands.into_iter().next() {
        Some((ServerCommand::NewMonster(m), actor)) => (m, actor),
        other => panic!("{name}: {other:?}"),
    }
}

#[test]
fn the_owner_receives_its_soul_as_a_player_owned_summon_it_cannot_attack() {
    let (m, actor) = new_monster("soul_owner");
    assert_eq!((m.blueprint.as_str(), actor), ("mercenary_ancient_watcher", Some(SOUL)));
    // Owned by the local player (1), stamped with the tick it appeared on.
    assert_eq!((m.summoner, m.category, m.spawn_tick, m.team), (1, 1, 123456, u32::MAX));
    assert_eq!((m.faction.faction_id, m.faction.faction_type), (PLAYER_OWNED_FACTION, 0));
    assert_eq!((m.health, m.max_health, m.level, m.heading), (5000, 6000, 60, 0.5));
    assert_eq!(PLAYER.stance(&m.faction), Stance::Neutral);
    // Its permanent effects (its aura) ride in the same batch.
    assert!(decoded("soul_owner").iter().any(|(c, a)| matches!(c, ServerCommand::StatusEffect(_)) && *a == Some(SOUL)));
    let (bytes, bits) = encode_single(&ServerCommand::NewMonster(m.clone()), SOUL);
    assert_eq!(decode_message(&bytes, bits).unwrap()[0].0, ServerCommand::NewMonster(m));
}

#[test]
fn peers_see_its_owner_and_only_a_pvp_enemy_may_attack_it() {
    let (peer, _) = new_monster("soul_peer");
    assert_eq!((peer.summoner, peer.faction.faction_id, peer.faction.faction_type), (OWNER, PLAYER_OWNED_FACTION, 0));
    assert_eq!(PLAYER.stance(&peer.faction), Stance::Neutral);
    let (hostile, _) = new_monster("soul_hostile");
    assert_eq!((hostile.summoner, hostile.faction.faction_id, hostile.faction.faction_type), (OWNER, MONSTER_FACTION, HOSTILE_FACTION_TYPE));
    assert_eq!(PLAYER.stance(&hostile.faction), Stance::Hostile);
    // A map's escort: the monster faction, but type 0 as ours -- not an enemy.
    let (companion, _) = new_monster("map_companion");
    assert_eq!((companion.category, companion.faction.faction_id, companion.faction.faction_type), (2, MONSTER_FACTION, 0));
    assert_eq!(PLAYER.stance(&companion.faction), Stance::Neutral);
}

#[test]
fn the_maps_monsters_are_enemies_tower_hazards_included() {
    let (monster, _) = new_monster("map_monster");
    assert_eq!((monster.summoner, monster.category, monster.spawn_tick), (0, 0, 0));
    assert_eq!(PLAYER.stance(&monster.faction), Stance::Hostile);
    // The server's "non-targetable" hazard keeps the hostile type: the client's
    // stance test has no case for its FactionId.
    let (hazard, _) = new_monster("tower_hazard");
    assert_eq!((hazard.faction.faction_id, hazard.faction.faction_type), (PLAYER_OWNED_FACTION, HOSTILE_FACTION_TYPE));
    assert_eq!(PLAYER.stance(&hazard.faction), Stance::Hostile);
}

#[test]
fn its_blows_name_it_and_it_leaves_with_a_discard() {
    let commands = decoded("soul_hit");
    let Some((ServerCommand::Hit(h), victim)) = commands.first() else { panic!("{commands:?}") };
    assert_eq!((h.attacker, h.combat_value_owner, h.damage, *victim), (SOUL, 1, 100, Some(0x30001)));
    let vicinity = decoded("soul_vicinity");
    let Some((ServerCommand::ActorsEnterVicinity(v), _)) = vicinity.first() else { panic!("{vicinity:?}") };
    assert_eq!(v.actors, vec![SOUL]);
    let discard = decoded("soul_discard");
    assert!(matches!(discard.first(), Some((ServerCommand::DiscardMonster(_), Some(SOUL)))), "{discard:?}");
}

#[test]
fn the_stance_follows_the_clients_test() {
    let f = |faction_type: u8, first: bool, second: bool| Faction { faction_id: 7, unknown_u32_0: 0, faction_type, unknown_flag_0: first, unknown_flag_1: second };
    assert_eq!(f(0, false, false).stance(&f(255, false, false)), Stance::None);
    assert_eq!(f(0, false, false).stance(&f(1, false, false)), Stance::Neutral);
    // The same FactionId: allies whatever the types.
    assert_eq!(f(0, false, false).stance(&f(2, false, false)), Stance::Ally);
    let other = |faction_type: u8, first: bool, second: bool| Faction { faction_id: 8, ..f(faction_type, first, second) };
    assert_eq!(f(3, false, false).stance(&other(2, false, false)), Stance::Ally);
    assert_eq!(f(0, false, true).stance(&other(0, false, false)), Stance::Ally);
    assert_eq!(f(0, true, false).stance(&other(0, true, false)), Stance::Hostile, "both first flags: PvP players");
    assert_eq!(f(0, false, false).stance(&other(2, false, false)), Stance::Hostile);
    assert_eq!(f(2, false, false).stance(&other(2, false, false)), Stance::Neutral);
}
