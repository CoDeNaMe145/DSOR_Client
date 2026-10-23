use super::*;
use bevy::gltf::Gltf;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;

use crate::actors::monsters::ModelSets;
use crate::network::net::start_offline;

const TABLE: &str = r#"{
    "effects": {"507": "pet_a", "508": "pet_b"},
    "pets": {
        "pet_a": {"graphics": "characters/pet_a", "set": "", "anim_set": "pet_default", "run_speed": 5.0,
                  "anim_run_speed": 2.5, "dye": [[1, 1, 1, 0], [1, 1, 1, 0]], "despawn_s": 3.0,
                  "death": "pet_a_end", "alt_death": "pet_a_revive"},
        "pet_b": {"graphics": "characters/pet_b", "set": "", "anim_set": "pet_default", "run_speed": 5.0,
                  "anim_run_speed": 5.0, "dye": [[1, 1, 1, 0], [1, 1, 1, 0]], "despawn_s": 2.0}
    }
}"#;

const MASTER: u32 = 0x77;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)))
        .init_asset::<PetTable>()
        .init_asset::<SequenceTable>()
        .init_asset::<NavMesh>()
        .init_asset::<Gltf>()
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_asset::<ModelSets>()
        .add_systems(Update, (apply_pets, follow_masters).chain());
    let table = app.world_mut().resource_mut::<Assets<PetTable>>().add(serde_json::from_str::<PetTable>(TABLE).unwrap());
    app.insert_resource(Data(table));
    start_offline(app.world_mut(), 1, 0);
    app
}

fn master(app: &mut App, at: Vec3) -> Entity {
    app.world_mut().spawn((RemotePlayer { actor: MASTER, ..default() }, Transform::from_translation(at), Visibility::Inherited)).id()
}

fn event(app: &mut App, index: u16, instance: u32, added: bool) {
    app.world_mut().non_send_mut::<Net>().pet_events.push((MASTER, index, instance, added));
}

fn pets(app: &mut App) -> Vec<(Entity, Vec3, bool)> {
    let mut q = app.world_mut().query::<(Entity, &Transform, Has<Leaving>, &Pet)>();
    q.iter(app.world()).map(|(e, tf, leaving, _)| (e, tf.translation, leaving)).collect()
}

/// Its rig in (setup_models in the game): the summon can play.
fn rig_in(app: &mut App, pet: Entity) {
    let player = app.world_mut().spawn_empty().id();
    app.world_mut().get_mut::<CharacterAnim>(pet).unwrap().bind_model(player, HashMap::new());
}

#[test]
fn a_pet_stands_behind_its_master_to_the_left_further_off_when_riding() {
    let d = 2.3 * std::f32::consts::FRAC_1_SQRT_2;
    // Facing +Z (its left is +X).
    let p = place(&Transform::IDENTITY, false);
    assert!(p.abs_diff_eq(Vec3::new(d, 0.0, -d), 1e-5), "{p}");
    // Facing +X (its left is -Z).
    let east = Transform::from_xyz(10.0, 1.0, 0.0).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
    let p = place(&east, false);
    assert!(p.abs_diff_eq(Vec3::new(10.0 - d, 1.0, -d), 1e-5), "{p}");
    assert!((place(&Transform::IDENTITY, true).length() - 4.3).abs() < 1e-5);
}

#[test]
fn a_pet_walks_past_its_follow_distance_and_stops_short_of_its_master() {
    assert_eq!(follow(2.3, false, false), Follow::Stand);
    assert_eq!(follow(2.5, false, false), Follow::Walk);
    assert_eq!(follow(2.0, true, false), Follow::Walk);
    assert_eq!(follow(1.7, true, false), Follow::Stop);
    assert_eq!(follow(4.3, false, true), Follow::Stand);
    assert_eq!(follow(4.5, false, true), Follow::Walk);
    assert_eq!(follow(4.2, true, true), Follow::Stop);
    assert_eq!(follow(20.5, false, false), Follow::Teleport);
    assert_eq!(follow(20.5, true, true), Follow::Teleport);
}

#[test]
fn idle_waits_stay_within_the_clients_range() {
    for seed in 0..1000u64 {
        let r = roll(seed.wrapping_mul(0x1234_5678_9ABC));
        assert!((0.0..1.0).contains(&r), "{r}");
    }
}

#[test]
fn a_pet_effect_calls_the_pet_shown_once_its_model_is_in() {
    let mut app = app();
    let m = master(&mut app, Vec3::new(5.0, 0.0, 5.0));
    event(&mut app, 507, 9, true);
    app.update();
    let all = pets(&mut app);
    assert_eq!(all.len(), 1);
    let (pet, at, _) = all[0];
    assert!(at.abs_diff_eq(place(&Transform::from_xyz(5.0, 0.0, 5.0), false), 1e-5));
    assert_eq!(app.world().get::<HasPet>(m).map(|h| h.pet), Some(pet));
    assert_eq!(app.world().get::<Visibility>(pet), Some(&Visibility::Hidden));
    // Not a monster, an NPC nor a player: nothing picks it.
    assert!(app.world().get::<Monster>(pet).is_none() && app.world().get::<RemotePlayer>(pet).is_none());

    rig_in(&mut app, pet);
    app.update();
    assert!(app.world().get::<Pet>(pet).unwrap().summoned);
    assert_eq!(app.world().get::<Visibility>(pet), Some(&Visibility::Inherited));
    // The same effect again (a status snapshot) changes nothing.
    event(&mut app, 507, 9, true);
    app.update();
    assert_eq!(pets(&mut app).len(), 1);
    // Effects that put out no pet are not its business.
    event(&mut app, 3, 4, true);
    app.update();
    assert_eq!(pets(&mut app).len(), 1);
}

#[test]
fn a_pet_hides_with_its_master() {
    let mut app = app();
    let m = master(&mut app, Vec3::ZERO);
    event(&mut app, 507, 9, true);
    app.update();
    let pet = pets(&mut app)[0].0;
    rig_in(&mut app, pet);
    app.update();
    *app.world_mut().get_mut::<Visibility>(m).unwrap() = Visibility::Hidden;
    app.update();
    assert_eq!(app.world().get::<Visibility>(pet), Some(&Visibility::Hidden));
    *app.world_mut().get_mut::<Visibility>(m).unwrap() = Visibility::Inherited;
    app.update();
    assert_eq!(app.world().get::<Visibility>(pet), Some(&Visibility::Inherited));
}

#[test]
fn a_pet_follows_its_master_and_is_put_back_when_left_far_behind() {
    let mut app = app();
    let m = master(&mut app, Vec3::ZERO);
    event(&mut app, 507, 9, true);
    app.update();
    let pet = pets(&mut app)[0].0;
    rig_in(&mut app, pet);
    app.update();
    let start = pets(&mut app)[0].1;
    // Within its follow distance: it stands.
    app.update();
    assert_eq!(pets(&mut app)[0].1, start);

    app.world_mut().get_mut::<Transform>(m).unwrap().translation = Vec3::new(0.0, 0.0, 8.0);
    app.update();
    app.update();
    assert!(app.world().get::<Pet>(pet).unwrap().moving);
    let anim = app.world().get::<CharacterAnim>(pet).unwrap();
    assert_eq!(anim.state, AnimState::Run);
    // 5 m/s on a clip made for 2.5.
    assert!((anim.speed - 2.0).abs() < 1e-4, "{}", anim.speed);
    for _ in 0..40 {
        app.update();
    }
    let at = pets(&mut app)[0].1;
    let gap = at.distance(Vec3::new(0.0, 0.0, 8.0));
    assert!((1.6..=1.8).contains(&gap), "stopped {gap} from its master");
    assert!(!app.world().get::<Pet>(pet).unwrap().moving);
    assert_eq!(app.world().get::<CharacterAnim>(pet).unwrap().state, AnimState::Idle);
    // Facing the way it went (+Z).
    let ahead = app.world().get::<Transform>(pet).unwrap().rotation * Vec3::Z;
    assert!(ahead.dot((Vec3::new(0.0, 0.0, 8.0) - start).normalize()) > 0.9);

    let far = Transform::from_xyz(40.0, 0.0, 8.0).with_rotation(Quat::from_rotation_y(1.0));
    *app.world_mut().get_mut::<Transform>(m).unwrap() = far;
    app.update();
    let tf = *app.world().get::<Transform>(pet).unwrap();
    assert!(tf.translation.abs_diff_eq(place(&far, false), 1e-4));
    assert!(tf.rotation.abs_diff_eq(far.rotation, 1e-5));
}

#[test]
fn an_ended_pet_effect_leaves_after_its_despawn_time() {
    let mut app = app();
    let m = master(&mut app, Vec3::ZERO);
    event(&mut app, 507, 9, true);
    app.update();
    let pet = pets(&mut app)[0].0;
    rig_in(&mut app, pet);
    app.update();
    // Another instance's end is not this pet's.
    event(&mut app, 507, 8, false);
    app.update();
    assert!(!pets(&mut app)[0].2);

    event(&mut app, 507, 9, false);
    app.update();
    assert!(pets(&mut app)[0].2, "leaving");
    assert!(app.world().get::<HasPet>(m).is_none());
    // Its movement lives on while it leaves: it still follows its master.
    let before = pets(&mut app)[0].1;
    app.world_mut().get_mut::<Transform>(m).unwrap().translation = Vec3::new(0.0, 0.0, 6.0);
    for _ in 0..5 {
        app.update();
    }
    assert!(pets(&mut app)[0].1.distance(Vec3::new(0.0, 0.0, 6.0)) < before.distance(Vec3::new(0.0, 0.0, 6.0)) - 1.0);
    // Still there through its DespawnTime (3 s), gone after.
    for _ in 0..20 {
        app.update();
    }
    assert_eq!(pets(&mut app).len(), 1);
    for _ in 0..10 {
        app.update();
    }
    assert!(pets(&mut app).is_empty());
}

#[test]
fn a_new_pet_replaces_the_old_one_in_the_same_batch() {
    let mut app = app();
    let m = master(&mut app, Vec3::ZERO);
    event(&mut app, 507, 9, true);
    app.update();
    let old = pets(&mut app)[0].0;
    rig_in(&mut app, old);
    app.update();
    // The server starts the new pet's effect, then ends the old one's.
    event(&mut app, 508, 10, true);
    event(&mut app, 507, 9, false);
    app.update();
    let all = pets(&mut app);
    assert_eq!(all.len(), 2);
    let new = app.world().get::<HasPet>(m).unwrap().pet;
    assert_ne!(new, old);
    assert!(all.iter().any(|(e, _, leaving)| *e == old && *leaving));
    assert!(all.iter().any(|(e, _, leaving)| *e == new && !*leaving));
    assert_eq!(app.world().get::<Pet>(new).unwrap().template, "pet_b");
}

#[test]
fn a_pet_that_revived_its_master_leaves_with_its_alternative_despawn() {
    let mut app = app();
    master(&mut app, Vec3::ZERO);
    event(&mut app, 507, 9, true);
    app.update();
    let pet = pets(&mut app)[0].0;
    rig_in(&mut app, pet);
    app.update();
    assert!(!app.world().get::<Pet>(pet).unwrap().revived);
    // A revive without a cooldown is not the pet's.
    for cooldown in [0u32, 250] {
        {
            let mut net = app.world_mut().non_send_mut::<Net>();
            crate::network::net::on_command(&mut net, dsor_proto::commands::ServerCommand::Revive(
                dsor_proto::commands::combat::Revive { cooldown }), Some(MASTER));
        }
        app.update();
        assert_eq!(app.world().get::<Pet>(pet).unwrap().revived, cooldown != 0);
    }
    let table: PetTable = serde_json::from_str(TABLE).unwrap();
    let def = &table.pets["pet_a"];
    assert_eq!(despawn_sequence(def, true), "pet_a_revive");
    assert_eq!(despawn_sequence(def, false), "pet_a_end");
    // Without an alternative, its own despawn.
    assert_eq!(despawn_sequence(&table.pets["pet_b"], true), "");
}

#[test]
fn a_leaving_pet_goes_at_once_with_its_master() {
    let mut app = app();
    let m = master(&mut app, Vec3::ZERO);
    event(&mut app, 507, 9, true);
    app.update();
    let pet = pets(&mut app)[0].0;
    rig_in(&mut app, pet);
    app.update();
    event(&mut app, 507, 9, false);
    app.update();
    assert!(pets(&mut app)[0].2, "leaving");
    app.world_mut().despawn(m);
    app.update();
    assert!(pets(&mut app).is_empty());
}

#[test]
fn a_pet_goes_at_once_with_its_master() {
    let mut app = app();
    let m = master(&mut app, Vec3::ZERO);
    event(&mut app, 507, 9, true);
    app.update();
    assert_eq!(pets(&mut app).len(), 1);
    app.world_mut().despawn(m);
    app.update();
    assert!(pets(&mut app).is_empty());
}

#[test]
fn a_pet_waits_for_a_master_not_in_yet() {
    let mut app = app();
    event(&mut app, 507, 9, true);
    app.update();
    assert!(pets(&mut app).is_empty());
    master(&mut app, Vec3::ZERO);
    app.update();
    assert_eq!(pets(&mut app).len(), 1);
}
