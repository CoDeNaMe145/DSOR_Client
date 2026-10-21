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
        .add_systems(Last, remove_corpses);
    app
}

fn spawn(app: &mut App, actor: u32) -> Entity {
    app.world_mut().spawn((monster(actor, 100), CharacterAnim::default(), Transform::default(),
        Visibility::Inherited, InheritedVisibility::VISIBLE, RemotePlayer { actor, ..default() })).id()
}

fn spawn_assets(app: &mut App) -> Handle<MonsterTemplates> {
    app.init_resource::<Assets<Outfits>>().init_resource::<Assets<Variations>>()
        .init_resource::<Assets<crate::world::nav::NavMesh>>().init_asset::<Gltf>()
        .init_asset::<bevy::world_serialization::WorldAsset>().init_asset::<ModelSets>();
    let templates = app.world_mut().resource_mut::<Assets<MonsterTemplates>>().add(serde_json::from_str::<MonsterTemplates>(
        r#"{"test":{"graphics":"characters/test","set":"","anim_set":"","state":"","loop":false}}"#).unwrap());
    app.world_mut().resource_mut::<MonsterData>().templates = templates.clone();
    templates
}

#[test]
fn monsters_spawn_on_the_movement_plane_before_the_first_move() {
    use crate::world::nav::{CurrentNav, NavMesh};
    let mut app = app();
    spawn_assets(&mut app);
    let mesh = app.world_mut().resource_mut::<Assets<NavMesh>>().add(NavMesh::new(vec![
        [Vec3::new(-10., 0., -10.), Vec3::new(10., 0., -10.), Vec3::new(0., 0., 10.)],
    ]));
    app.insert_resource(CurrentNav(mesh));
    app.world_mut().resource_mut::<DebugMonsters>().spawns.push(MonsterSpawn {
        actor: 41, blueprint: "test".into(), position: Vec3::new(0., -2.58, 0.), health: 100, level: 1,
    });
    app.world_mut().run_system_once(spawn_monsters).unwrap();
    let mut query = app.world_mut().query::<(&Transform, &RemotePlayer)>();
    let (tf, remote) = query.single(app.world()).unwrap();
    assert_eq!(tf.translation, Vec3::ZERO);
    assert_eq!(remote.target, tf.translation);
}

#[test]
fn death_received_while_templates_load_cannot_spawn_a_ghost_later() {
    let mut app = app();
    crate::net::start_offline(app.world_mut(), 1, 0);
    let handle = spawn_assets(&mut app);
    let templates = app.world_mut().resource_mut::<Assets<MonsterTemplates>>().remove(handle.id()).unwrap();
    app.world_mut().non_send_mut::<Net>().monster_spawns.push(MonsterSpawn {
        actor: 41, blueprint: "test".into(), position: Vec3::ZERO, health: 100, level: 1,
    });
    app.world_mut().run_system_once(spawn_monsters).unwrap();
    assert_eq!(app.world().non_send::<Net>().monster_spawns.len(), 1);
    {
        let mut net = app.world_mut().non_send_mut::<Net>();
        // The packet decoder retains death even before an ECS actor exists.
        net.remember_monster_health(41, 0, 100);
        net.kills.push((41, default()));
    }
    app.update();
    assert!(app.world().non_send::<Net>().kills.is_empty());
    app.world_mut().resource_mut::<Assets<MonsterTemplates>>().insert(handle.id(), templates).unwrap();
    app.world_mut().run_system_once(spawn_monsters).unwrap();
    assert!(app.world_mut().query::<&Monster>().iter(app.world()).next().is_none());
    assert!(app.world().non_send::<Net>().monster_spawns.is_empty());
}

#[test]
fn hover_info_rejects_dead_and_disposed_targets_even_if_selection_is_stale() {
    let mut app = app();
    let root = spawn(&mut app, 41);
    app.world_mut().resource_mut::<Hovered>().0 = Some((root, 41));
    app.world_mut().run_system_once(hover_info).unwrap();
    assert!(app.world().resource::<HoverInfo>().0.is_some());
    app.world_mut().get_mut::<Monster>(root).unwrap().health = 0;
    app.world_mut().run_system_once(hover_info).unwrap();
    assert!(app.world().resource::<HoverInfo>().0.is_none());
    app.world_mut().get_mut::<Monster>(root).unwrap().health = 100;
    app.world_mut().get_mut::<CharacterAnim>(root).unwrap().dead = true;
    app.world_mut().run_system_once(hover_info).unwrap();
    assert!(app.world().resource::<HoverInfo>().0.is_none());
    app.world_mut().despawn(root);
    app.world_mut().run_system_once(hover_info).unwrap();
    assert!(app.world().resource::<HoverInfo>().0.is_none());
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

#[test]
fn corpse_effect_lights_survive_post_update_and_are_removed_before_the_next_frame() {
    use bevy::camera::primitives::Sphere;
    use bevy::light::{LightPlugin, SimulationLightSystems};

    let mut app = app();
    app.init_asset::<Image>()
        .init_asset::<bevy::gizmos::GizmoAsset>()
        .add_plugins((TransformPlugin, LightPlugin));
    crate::net::start_offline(app.world_mut(), 1, 0);
    let root = spawn(&mut app, 41);
    app.world_mut().entity_mut(root).insert(MonsterDeath { sequence: None, fallback_at: 0.0 });
    let point = app.world_mut().spawn((PointLight::default(), Transform::default(), ChildOf(root))).id();
    let spot = app.world_mut().spawn((SpotLight::default(), Transform::default(), ChildOf(root))).id();
    let survivor = app.world_mut().spawn((PointLight::default(), Transform::default())).id();
    app.add_systems(PostUpdate, (move |world: &World| {
        // Exercise Bevy's actual deferred Sphere insertions. With cleanup in
        // PostUpdate, its lights could disappear before these commands flushed.
        for entity in [point, spot, survivor] {
            assert!(world.get::<Sphere>(entity).is_some(), "light must survive all PostUpdate work");
        }
    }).after(SimulationLightSystems::UpdateBounds).after(remove_corpses).run_if(run_once));
    app.update();
    for entity in [root, point, spot] {
        assert!(app.world().get_entity(entity).is_err(), "corpse and effects must be disposed in the same frame");
    }
    assert!(app.world().get::<Sphere>(survivor).is_some());
    // A subsequent lighting pass must not retain the disposed hierarchy.
    app.world_mut().get_mut::<Transform>(survivor).unwrap().translation.x = 3.0;
    app.update();
    assert_eq!(app.world().get::<Sphere>(survivor).unwrap().center.x, 3.0);
}
