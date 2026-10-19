use super::*;
use bevy::animation::AnimationPlugin;
use bevy::ecs::system::RunSystemOnce;
use std::time::Duration;

pub(crate) fn monster(actor: u32, health: u32) -> Monster {
    Monster { actor, template: "test".into(), health, max_health: 100, level: 1,
        radius: 0.5, height: 2.0, footprint: None, hits: default(), heavy: None }
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AnimationPlugin))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)))
        .init_resource::<Assets<MonsterTemplates>>()
        .init_resource::<Assets<SequenceTable>>()
        .init_resource::<Assets<PlayerHits>>()
        .init_resource::<OfflineMonster>()
        .init_resource::<DebugMonsters>()
        .init_resource::<Hovered>()
        .init_resource::<HoverInfo>()
        .init_resource::<ShaderColors>()
        .init_resource::<crate::skills::CameraShake>()
        .init_resource::<crate::ui::combat_text::CombatTexts>()
        .insert_resource(MonsterData { templates: default(), anims: default(), player_hits: default(), outfits: default(), variations: default() })
        .add_systems(Update, (monster_health, hits_and_kills, start_deaths, crate::skills::play_sequences,
            crate::actors::character::drive_animations).chain())
        .add_systems(PostUpdate, remove_corpses.after(bevy::app::AnimationSystems));
    app
}

fn spawn(app: &mut App, actor: u32) -> Entity {
    app.world_mut().spawn((monster(actor, 100), CharacterAnim::default(), Transform::default(),
        Visibility::Inherited, InheritedVisibility::VISIBLE, RemotePlayer { actor, ..default() })).id()
}

fn rig(app: &mut App, root: Entity, seconds: f32) -> (Entity, AnimationNodeIndex) {
    let mut clip = AnimationClip::default();
    clip.set_duration(seconds);
    let clip = app.world_mut().resource_mut::<Assets<AnimationClip>>().add(clip);
    let (graph, node) = AnimationGraph::from_clip(clip);
    let graph = app.world_mut().resource_mut::<Assets<AnimationGraph>>().add(graph);
    let player = app.world_mut().spawn((AnimationPlayer::default(), AnimationGraphHandle(graph), ChildOf(root))).id();
    // Even an exported death clip incorrectly marked as looping must play once.
    app.world_mut().get_mut::<CharacterAnim>(root).unwrap().bind_model(player, [("Death".into(), (node, true))].into_iter().collect());
    (player, node)
}

#[test]
fn zero_hp_update_disables_interaction_without_visual_assets_or_kill() {
    let mut app = app();
    crate::net::start_offline(app.world_mut(), 1, 0);
    let root = spawn(&mut app, 41);
    app.world_mut().entity_mut(root).insert((crate::picking::PickModel("test".into()), crate::picking::PickBoxes(Vec::new())));
    app.world_mut().get_mut::<CharacterAnim>(root).unwrap().speed = 2.5;
    app.world_mut().resource_mut::<Hovered>().0 = Some((root, 41));
    app.world_mut().resource_mut::<HoverInfo>().0 = Some(("test".into(), 1, 100, 100));
    app.world_mut().non_send_mut::<Net>().monster_health.extend([(41, 0, 100), (41, 100, 100)]);
    app.update();
    let anim = app.world().get::<CharacterAnim>(root).unwrap();
    assert!(anim.dead);
    assert_eq!(anim.speed, 1.0);
    assert_eq!(app.world().get::<Monster>(root).unwrap().health, 0);
    assert!(app.world().get::<crate::picking::PickBoxes>(root).is_none());
    assert!(app.world().get::<crate::picking::PickModel>(root).is_none());
    assert!(app.world().resource::<Hovered>().0.is_none());
    assert!(app.world().resource::<HoverInfo>().0.is_none());
    assert!(app.world().get::<MonsterDeath>(root).is_some());
}

#[test]
fn lethal_hit_starts_death_and_late_hits_cannot_heal_or_retrigger() {
    let mut app = app();
    crate::net::start_offline(app.world_mut(), 1, 0);
    let root = spawn(&mut app, 41);
    let attacker = app.world().non_send::<Net>().local_actor.unwrap();
    app.world_mut().non_send_mut::<Net>().hits.extend([
        (41, dsor_proto::commands::combat::Hit { attacker, victim_health: 0, victim_max_health: 100, damage: 100, ..default() }),
        (41, dsor_proto::commands::combat::Hit { attacker, victim_health: 75, victim_max_health: 100, damage: -75, ..default() }),
    ]);
    app.update();
    assert_eq!(app.world().get::<Monster>(root).unwrap().health, 0);
    assert!(app.world().get::<CharacterAnim>(root).unwrap().dead);
    assert_eq!(app.world().resource::<crate::ui::combat_text::CombatTexts>().0.len(), 1);
    let deadline = app.world().get::<MonsterDeath>(root).unwrap().fallback_at;
    app.world_mut().non_send_mut::<Net>().kills.push((41, dsor_proto::commands::combat::Kill { unknown_u32_0: 300, ..default() }));
    app.update();
    assert_eq!(app.world().get::<MonsterDeath>(root).unwrap().fallback_at, deadline);
}

#[test]
fn online_and_offline_deaths_dispose_hierarchy_at_animation_completion() {
    for online in [false, true] {
        let mut app = app();
        if online { crate::net::start_offline(app.world_mut(), 1, 0); }
        let root = spawn(&mut app, 41);
        let (player, node) = rig(&mut app, root, 1.0);
        let child = app.world_mut().spawn(ChildOf(player)).id();
        let effect = app.world_mut().spawn(ChildOf(child)).id();
        if online {
            // A zero server lifetime must not truncate the animation.
            app.world_mut().non_send_mut::<Net>().kills.push((41, default()));
        } else {
            app.world_mut().resource_mut::<DebugMonsters>().blows.push((41, 2));
        }
        for _ in 0..5 { app.update(); }
        assert!(app.world().get_entity(root).is_ok());
        let elapsed = app.world().get::<AnimationPlayer>(player).unwrap().animation(node).unwrap().seek_time();
        assert!(elapsed > 0.0 && elapsed < 1.0);
        // Duplicate kills cannot restart the death animation.
        app.world_mut().resource_mut::<DebugMonsters>().blows.push((41, 2));
        app.update();
        assert!(app.world().get::<AnimationPlayer>(player).unwrap().animation(node).unwrap().seek_time() > elapsed);
        for _ in 0..10 { app.update(); }
        for entity in [root, player, child, effect] {
            assert!(app.world().get_entity(entity).is_err(), "death must dispose the entire hierarchy");
        }
    }
}

#[test]
fn missing_or_culled_animation_has_bounded_cleanup() {
    for culled in [false, true] {
        let mut app = app();
        let root = spawn(&mut app, 41);
        if culled {
            rig(&mut app, root, 1.0);
            app.world_mut().entity_mut(root).insert(InheritedVisibility::HIDDEN);
        }
        app.world_mut().get_mut::<Monster>(root).unwrap().health = 0;
        app.update();
        assert!(app.world().get_entity(root).is_ok());
        for _ in 0..55 { app.update(); }
        assert!(app.world().get_entity(root).is_err());
    }
}

#[test]
fn death_waits_for_delayed_sequence_animation_and_cleans_sequence() {
    use crate::skills::{Sequence, Track};
    let mut app = app();
    let templates = app.world_mut().resource_mut::<Assets<MonsterTemplates>>().add(serde_json::from_str::<MonsterTemplates>(
        r#"{"test":{"graphics":"","set":"","anim_set":"","state":"","loop":false,"death":"delayed"}}"#).unwrap());
    app.world_mut().resource_mut::<MonsterData>().templates = templates;
    let sequences = app.world_mut().resource_mut::<Assets<SequenceTable>>().add(SequenceTable([("delayed".into(), Sequence {
        length: 200, repeat: false, tracks: vec![Track::Anim { name: "Death".into(), speed: 0.5, start: 25, end: 200 }],
    })].into_iter().collect()));
    app.insert_resource(SkillData::for_test(sequences));
    let root = spawn(&mut app, 41);
    rig(&mut app, root, 0.5);
    app.world_mut().get_mut::<Monster>(root).unwrap().health = 0;
    for _ in 0..9 { app.update(); }
    assert!(app.world().get_entity(root).is_ok(), "initial Death clip must not bypass a pending track");
    let sequence = app.world().get::<MonsterDeath>(root).unwrap().sequence.unwrap();
    for _ in 0..7 { app.update(); }
    assert!(app.world().get_entity(root).is_ok(), "sequence playback speed must be respected");
    for _ in 0..10 { app.update(); }
    assert!(app.world().get_entity(root).is_err());
    assert!(app.world().get_entity(sequence).is_err(), "do not leave a sequence for the old corpse lifetime");
}

#[test]
fn cleanup_removes_attached_sequence_players() {
    let mut app = app();
    let root = spawn(&mut app, 41);
    let sequence = app.world_mut().run_system_once(move |mut commands: Commands| {
        crate::skills::play_attached(&mut commands, crate::skills::Sequence { length: 1000, repeat: false, tracks: Vec::new() }, root)
    }).unwrap();
    app.world_mut().entity_mut(root).insert(MonsterDeath { sequence: None, fallback_at: 0.0 });
    app.world_mut().run_system_once(remove_corpses).unwrap();
    assert!(app.world().get_entity(root).is_err());
    assert!(app.world().get_entity(sequence).is_err());
}
