use super::subjects::{for_class, resolve, COSTUME_SLOT};
use super::*;
use crate::gameplay::systems::{StickerLook, SystemsCatalog};
use bevy::ecs::system::RunSystemOnce;

/// Where a point lands in a stage's image, as fractions (0, 0 top left), for
/// a camera made by `framing`.
fn project(camera: &Transform, aspect: f32, point: Vec3) -> Vec2 {
    let view = camera.to_matrix().inverse().transform_point3(point);
    let t = (FOV * 0.5).tan();
    let depth = -view.z;
    Vec2::new(0.5 + view.x / depth / (t * aspect) * 0.5, 0.5 - view.y / depth / t * 0.5)
}

#[test]
fn the_camera_puts_the_feet_on_the_floor_mark_and_keeps_a_turning_subject_in_view() {
    for stage in STAGES {
        let size = image_size(stage, 1.0);
        let aspect = size.x as f32 / size.y as f32;
        let (top, floor) = stage.marks();
        let origin = stage.origin();
        // A player, a long mount, a pet hovering above the ground.
        for extent in [(0.0, 1.9, 0.45), (-0.1, 2.4, 2.6), (1.7, 2.6, 0.7)] {
            let camera = framing(extent, stage, aspect);
            // Its lowest point on the floor mark (a hoof below the root).
            let feet = project(&camera, aspect, origin + Vec3::Y * extent.0);
            assert!((feet.y - floor).abs() < 1e-3 && (feet.x - 0.5).abs() < 1e-3, "{stage:?} {extent:?}: feet at {feet}");
            // Its top under the top mark.
            let head = project(&camera, aspect, origin + Vec3::Y * extent.1);
            assert!(head.y >= top - 1e-3, "{stage:?} {extent:?}: head at {head}");
            // Whichever way it turns, its farthest joint stays in the image.
            for angle in 0..16 {
                let turn = Quat::from_rotation_y(angle as f32 * std::f32::consts::TAU / 16.0);
                for y in [extent.0, extent.1] {
                    let p = project(&camera, aspect, origin + turn * Vec3::new(extent.2, y, 0.0));
                    assert!((0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y), "{stage:?} {extent:?}: {p} out of the image");
                }
            }
        }
    }
    // A pet stands as low as a player does: framed to the same floor, bigger.
    let aspect = 1.0;
    let small = framing((0.0, 0.5, 0.3), Stage::Album, aspect);
    let tall = framing((0.0, 1.65, 0.45), Stage::Album, aspect);
    assert!(small.translation.distance(Stage::Album.origin()) < tall.translation.distance(Stage::Album.origin()));
}

fn look(fields: serde_json::Value) -> StickerLook {
    serde_json::from_value(fields).unwrap()
}

fn model(graphics: &str) -> Model {
    Model { graphics: graphics.into(), set: "s".into(), anim_set: "a".into(), dye: [[1.0; 4]; 2] }
}

#[test]
fn a_sticker_shows_its_costume_or_emote_on_the_player_rides_its_mount_or_shows_its_model() {
    let player = Look { class: 2, gender: 1, equipment: vec![Equipment { slot: 0, skin_parts: vec!["helm".into()], colour: None }], ..default() };
    // The costume dresses the player: one more skin part, its mask culls the rest.
    let Some(Subject::Character { look: dressed, state: None, emote: false, mount: None }) =
        resolve(&look(serde_json::json!({"item": "c", "skin": "costume_santa_01"})), Some(&player), None) else { panic!("costume") };
    assert_eq!(dressed.equipment.last(), Some(&Equipment { slot: COSTUME_SLOT, skin_parts: vec!["costume_santa_01".into()], colour: None }));
    assert_eq!(dressed.equipment[0], player.equipment[0]);
    // The emote, played by the player again and again.
    assert_eq!(resolve(&look(serde_json::json!({"item": "e", "animation": "vanity_dance"})), Some(&player), None),
        Some(Subject::Character { look: player.clone(), state: Some("vanity_dance".into()), emote: true, mount: None }));
    // The player on the mount; the pet; the Shadow Soul at rest.
    assert_eq!(resolve(&look(serde_json::json!({"item": "m", "mount": "mount_horse_vc_01"})), Some(&player), Some(model("characters/horse"))),
        Some(Subject::Character { look: player.clone(), state: None, emote: false, mount: Some(model("characters/horse")) }));
    assert_eq!(resolve(&look(serde_json::json!({"item": "p", "pet": "pet_rabbit"})), None, Some(model("characters/rabbit"))),
        Some(Subject::Model { model: model("characters/rabbit"), state: None }));
    assert_eq!(resolve(&look(serde_json::json!({"item": "s", "monster": "mercenary_ancient_watcher"})), None, Some(model("characters/retinue"))),
        Some(Subject::Model { model: model("characters/retinue"), state: Some("Idle_Hub".into()) }));
    // A decoration, a model not in the tables, a costume with no player: the icon.
    assert_eq!(resolve(&look(serde_json::json!({"item": "t", "status": 858})), Some(&player), None), None);
    assert_eq!(resolve(&look(serde_json::json!({"item": "p", "pet": "pet_x"})), Some(&player), None), None);
    assert_eq!(resolve(&look(serde_json::json!({"item": "c", "skin": "x"})), None, None), None);
    // The item for the player's class, else one for every class.
    let looks = [look(serde_json::json!({"item": "w", "class": "warrior", "skin": "a"})), look(serde_json::json!({"item": "r", "class": "ranger", "skin": "b"})),
        look(serde_json::json!({"item": "all", "skin": "c"}))];
    assert_eq!(for_class(&looks, Some(2)).map(|l| l.item.as_str()), Some("r"));
    assert_eq!(for_class(&looks, Some(1)).map(|l| l.item.as_str()), Some("all"));
    assert_eq!(for_class(&looks[..2], Some(3)), None);
}

#[test]
fn the_inventory_draws_its_figures_with_the_character_and_shadow_soul_stages() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface/inventory.ui.json");
    let Ok(raw) = std::fs::read(&dir) else { return };
    let source: Layout = serde_json::from_slice(&raw).unwrap();
    let prepared = prepare_inventory(&source);
    let figures: Vec<_> = prepared.widgets.iter().filter(|w| w.id.ends_with("/figur")).collect();
    assert_eq!(figures.len(), 3, "the equipment, trinket and mercenary pages' figures");
    for w in figures {
        assert!(!w.hidden);
        let art = w.art.as_ref().unwrap();
        let stage = if w.id.contains("/mercenary/") { Stage::Mercenary } else { Stage::Inventory };
        assert_eq!(art.tex.as_deref(), Some(stage.texture()));
        assert_eq!(&art.pos[..2], &w.rect[..2]);
    }
    assert_eq!(Stage::of_texture("preview:album"), Some(Stage::Album));
    assert_eq!(Stage::of_texture("textures/ui/inventory.png"), None);
}

/// An app with the stages, the album's tables (empty) and `choose`.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<Image>().init_asset::<SystemsCatalog>().init_asset::<crate::actors::mounts::MountTable>()
        .init_asset::<subjects::PetModels>().init_asset::<crate::actors::monsters::MonsterTemplates>()
        .init_resource::<super::super::controls::systems::State>().init_resource::<super::super::controls::systems::Views>()
        .init_resource::<UiWork>().init_resource::<Screen>().init_resource::<Previews>()
        .add_systems(Startup, (setup, subjects::load))
        .add_systems(Update, choose);
    app
}

#[test]
fn subjects_are_built_when_what_they_show_changes_and_never_on_idle_frames() {
    let mut app = app();
    let layout = Layout { window: "inventory".into(), widgets: Vec::new() };
    let inventory = app.world_mut().spawn((Window2018("inventory".into()), Visibility::Hidden, WindowControls::new(&layout))).id();
    let built = |app: &App| app.world().resource::<UiWork>().previews;
    let active = |app: &mut App| app.world_mut().query::<&Camera>().iter(app.world()).filter(|c| c.is_active).count();
    let show = |app: &mut App, visibility| *app.world_mut().get_mut::<Visibility>(inventory).unwrap() = visibility;
    for _ in 0..3 { app.update(); }
    let player = app.world_mut().run_system_once(|mut commands: Commands| {
        let desc = CharacterDesc { class: 1, ..default() };
        let e = crate::actors::character::spawn_character(&mut commands, desc, Transform::IDENTITY);
        commands.entity(e).insert(LocalPlayer::default());
        e
    }).unwrap();
    for _ in 0..3 { app.update(); }
    assert_eq!((built(&app), active(&mut app)), (0, 0), "a closed window's stage builds and draws nothing");
    show(&mut app, Visibility::Inherited);
    app.update();
    assert_eq!((built(&app), active(&mut app)), (1, 1), "opened: the character dresses, the camera draws");
    for _ in 0..10 { app.update(); }
    assert_eq!(built(&app), 1, "idle frames build nothing");
    show(&mut app, Visibility::Hidden);
    app.update();
    assert_eq!((built(&app), active(&mut app)), (1, 0), "closed: the camera stops");
    show(&mut app, Visibility::Inherited);
    app.update();
    assert_eq!((built(&app), active(&mut app)), (1, 1), "reopened: the same character, not dressed again");
    app.world_mut().get_mut::<Character>(player).unwrap().desc.equipment = vec![Equipment { slot: 0, skin_parts: vec!["helm".into()], colour: None }];
    app.update();
    assert_eq!(built(&app), 2, "the player's look changed");
    for _ in 0..5 { app.update(); }
    assert_eq!(built(&app), 2, "idle again");
    let subjects = app.world_mut().query_filtered::<(), (With<Preview>, With<Character>, Without<LocalPlayer>)>().iter(app.world()).count();
    assert_eq!(subjects, 1, "a subject still dressing gives way to the newer one");
}
