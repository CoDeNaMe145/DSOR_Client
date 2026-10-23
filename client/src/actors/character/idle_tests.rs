use super::*;
use bevy::ecs::system::RunSystemOnce;
use crate::network::net::{LocalPlayer, Net, RemotePlayer};
use dsor_proto::commands::combat::{Hit, Kill};

fn actor(world: &mut World, desc: CharacterDesc) -> Entity {
    let root = world.run_system_once(move |mut commands: Commands|
        spawn_character(&mut commands, desc.clone(), Transform::default())).unwrap();
    world.entity_mut(root).insert(InheritedVisibility::VISIBLE);
    root
}

fn step(world: &mut World, seconds: f32) {
    world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(seconds));
    world.run_system_once(drive_animations).unwrap();
}

#[test]
fn standing_players_blend_between_relaxed_and_combat_clips_with_or_without_weapons() {
    for class in 0..4 {
        for gender in 0..2 {
            for armament in [0, 3, 4, 5] {
                let mut world = World::new();
                world.init_resource::<Time>();
                let root = actor(&mut world, CharacterDesc { class, gender, armament, ..default() });
                let (_, nodes) = AnimationGraph::from_clips([Handle::default(), Handle::default()]);
                let player = world.spawn(AnimationPlayer::default()).id();
                world.get_mut::<CharacterAnim>(root).unwrap().bind_model(player,
                    [("Idle".into(), (nodes[0], true)), ("Idle_Hub".into(), (nodes[1], true))].into_iter().collect());
                step(&mut world, 0.0);
                assert_eq!(world.get::<CharacterAnim>(root).unwrap().current, Some(nodes[1]));
                world.get_mut::<CharacterAnim>(root).unwrap().enter_combat();
                step(&mut world, 0.0);
                let anim = world.get::<CharacterAnim>(root).unwrap();
                assert_eq!(anim.state, AnimState::Idle);
                assert_eq!(anim.current, Some(nodes[0]));
                assert_eq!(anim.fading, [(nodes[1], 1.0)], "enter combat with a blend");
                step(&mut world, COMBAT_IDLE_SECONDS - 1.0);
                world.get_mut::<CharacterAnim>(root).unwrap().enter_combat();
                step(&mut world, 2.0);
                assert_eq!(world.get::<CharacterAnim>(root).unwrap().current, Some(nodes[0]), "activity refreshes the stance");
                step(&mut world, COMBAT_IDLE_SECONDS);
                let anim = world.get::<CharacterAnim>(root).unwrap();
                assert_eq!(anim.state, AnimState::IdleHub);
                assert_eq!(anim.current, Some(nodes[1]));
                assert_eq!(anim.fading, [(nodes[0], 1.0)], "leave combat with a blend");
            }
        }
    }
}

#[test]
fn hostile_hits_and_kills_engage_local_and_remote_players_but_healing_does_not() {
    let mut world = World::new();
    world.init_resource::<Time<Real>>();
    crate::network::net::start_offline(&mut world, 1, 0);
    let local = world.spawn((LocalPlayer::default(), CharacterAnim::default())).id();
    let remote = world.spawn((RemotePlayer { actor: 1, ..default() }, CharacterAnim::default())).id();
    for (damage, text) in [(-10, ""), (10, "HealingOther")] {
        world.non_send_mut::<Net>().hits = vec![(0, Hit { attacker: 1, damage, combat_text: text.into(), ..default() })];
        world.run_system_once(track_player_combat).unwrap();
        for root in [local, remote] { assert_eq!(world.get::<CharacterAnim>(root).unwrap().combat_remaining, 0.0); }
    }
    for hit in [Hit { damage: 10, ..default() }, Hit { blocked: true, ..default() }, Hit { immune: true, ..default() }] {
        for root in [local, remote] { world.get_mut::<CharacterAnim>(root).unwrap().combat_remaining = 0.0; }
        world.non_send_mut::<Net>().hits = vec![(0, Hit { attacker: 1, ..hit })];
        world.run_system_once(track_player_combat).unwrap();
        for root in [local, remote] { assert_eq!(world.get::<CharacterAnim>(root).unwrap().combat_remaining, COMBAT_IDLE_SECONDS); }
    }
    world.non_send_mut::<Net>().hits.clear();
    world.get_mut::<CharacterAnim>(remote).unwrap().combat_remaining = 0.0;
    world.non_send_mut::<Net>().kills.push((42, Kill { killer: 1, ..default() }));
    world.run_system_once(track_player_combat).unwrap();
    assert_eq!(world.get::<CharacterAnim>(remote).unwrap().combat_remaining, COMBAT_IDLE_SECONDS);
}

#[test]
fn idle_selection_preserves_actions_mounts_npcs_and_death_and_expires_without_a_rig() {
    let mut world = World::new();
    world.init_resource::<Time>();
    let root = actor(&mut world, CharacterDesc::default());
    for state in [AnimState::Run, AnimState::Named("Skill01_execute".into()), AnimState::Named("Resurrect".into()),
        AnimState::Named("CharSelectIdle".into())] {
        let mut anim = world.get_mut::<CharacterAnim>(root).unwrap();
        anim.state = state.clone();
        anim.enter_combat();
        step(&mut world, COMBAT_IDLE_SECONDS + 1.0);
        assert_eq!(world.get::<CharacterAnim>(root).unwrap().state, state);
    }
    world.get_mut::<CharacterAnim>(root).unwrap().state = AnimState::Idle;
    step(&mut world, 0.0);
    assert_eq!(world.get::<CharacterAnim>(root).unwrap().state, AnimState::IdleHub);
    world.get_mut::<Character>(root).unwrap().riding = Some("mount_horse_01".into());
    step(&mut world, 0.0);
    assert_eq!(world.get::<CharacterAnim>(root).unwrap().state, AnimState::Idle);
    world.get_mut::<Character>(root).unwrap().riding = None;
    let mut anim = world.get_mut::<CharacterAnim>(root).unwrap();
    anim.enter_combat();
    anim.dead = true;
    anim.state = AnimState::Death;
    step(&mut world, 0.0);
    assert_eq!(world.get::<CharacterAnim>(root).unwrap().state, AnimState::Death);
    assert_eq!(world.get::<CharacterAnim>(root).unwrap().combat_remaining, 0.0);
    let npc = actor(&mut world, CharacterDesc { look: Some(NpcLook::default()), ..default() });
    step(&mut world, 0.0);
    assert_eq!(world.get::<CharacterAnim>(npc).unwrap().state, AnimState::Idle);
}

#[test]
fn a_shadow_soul_rests_relaxed_and_takes_its_battle_stance_when_it_fights() {
    let mut world = World::new();
    world.init_resource::<Time>();
    world.init_resource::<Time<Real>>();
    crate::network::net::start_offline(&mut world, 1, 0);
    let soul =world.spawn((RemotePlayer { actor: 0x10600, ..default() }, crate::actors::monsters::tests::monster(0x10600, 100),
        RelaxedIdle, CharacterAnim::default(), GlobalTransform::default(), InheritedVisibility::VISIBLE)).id();
    let monster = world.spawn((RemotePlayer { actor: 41, ..default() }, crate::actors::monsters::tests::monster(41, 100),
        CharacterAnim::default(), GlobalTransform::default(), InheritedVisibility::VISIBLE)).id();
    step(&mut world, 0.0);
    assert_eq!(world.get::<CharacterAnim>(soul).unwrap().state, AnimState::IdleHub);
    assert_eq!(world.get::<CharacterAnim>(monster).unwrap().state, AnimState::Idle);
    world.non_send_mut::<Net>().hits = vec![(41, Hit { attacker: 0x10600, damage: 20, ..default() })];
    world.run_system_once(track_player_combat).unwrap();
    step(&mut world, 0.0);
    assert_eq!(world.get::<CharacterAnim>(soul).unwrap().state, AnimState::Idle);
    assert_eq!(world.get::<CharacterAnim>(monster).unwrap().combat_remaining, 0.0, "a monster of the map stands as it is");
    step(&mut world, COMBAT_IDLE_SECONDS + 0.5);
    assert_eq!(world.get::<CharacterAnim>(soul).unwrap().state, AnimState::IdleHub);
}
