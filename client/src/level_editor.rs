//! Native Rust level editor. DB4 persistence, UI and rendering share one process.
mod desktop;
mod previews;
mod fly_camera;
mod camera_preview;
mod lighting_preview;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use bevy::asset::LoadState;
use bevy::camera::primitives::MeshAabb;
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings};
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Deserialize, PartialEq)]
struct Variation { bone: String, t: [f32; 3], s: [f32; 3] }
#[derive(Clone, Deserialize, PartialEq)]
struct Character { skeleton: String, parts: Vec<String>, anim_set: String, state: String, variation: Vec<Variation> }
#[derive(Clone, Deserialize, PartialEq)]
struct Decoration { model: String, matrix: [f32; 16], character: Option<Character> }
#[derive(Clone, Deserialize, PartialEq)]
struct Object {
    key: String,
    kind: String,
    label: String,
    matrix: [f32; 16],
    model: String,
    #[serde(default)] character: Option<Character>,
    #[serde(default)] light: Option<crate::lighting::LevelLight>,
    #[serde(default)] ambience: Option<crate::lighting::LocalBubble>,
    #[serde(default)] default_ambience: bool,
    #[serde(default = "always")] event: i32,
    #[serde(default)] sequence: Option<serde_json::Value>,
    #[serde(default)] decorations: Vec<Decoration>,
    #[serde(default)] camera: Option<crate::camera_view::Lens>,
}
fn always() -> i32 { -1 }
impl Object {
    fn transform(&self) -> Transform {
        let matrix = Mat4::from_cols_array(&self.matrix);
        if matrix.determinant().abs() < 1e-10 {
            return Transform::from_translation(matrix.w_axis.truncate());
        }
        Transform::from_matrix(matrix)
    }
    fn same_visual(&self, other: &Self) -> bool {
        self.model == other.model && self.character == other.character && self.light == other.light
            && self.sequence == other.sequence && self.decorations == other.decorations
    }
}
#[derive(Default, Deserialize)]
struct Scene {
    protocol: u32,
    revision: u64,
    map: String,
    center: [f32; 3],
    extents: [f32; 3],
    objects: Vec<Object>,
    #[serde(default)] ambience: Option<crate::lighting::Bubble>,
}
#[derive(Clone, Deserialize)]
struct Controls {
    revision: u64,
    selected: Option<String>,
    mode: String,
    snap: f32,
    focus_serial: u64,
    focus_target: String,
    pickable: HashSet<String>,
    #[serde(default)] active_events: HashSet<i32>,
    #[serde(default)] hidden_geometry: HashSet<String>,
    #[serde(default)] show_helpers: bool,
    #[serde(default)] show_grid: bool,
    #[serde(default)] show_lights: bool,
    #[serde(default)] show_bubbles: bool,
    #[serde(default)] preview_ambience: Option<String>,
    #[serde(default = "default_speed")] camera_speed: f32,
}
fn default_speed() -> f32 { 15. }
impl Controls {
    fn visible(&self, object: &Object) -> bool {
        !self.hidden_geometry.contains(&object.key) && (object.event == -1 || self.active_events.contains(&object.event))
    }
}
impl Default for Controls {
    fn default() -> Self {
        Self { revision: 0, selected: None, mode: "translate".into(), snap: 1., focus_serial: 0,
            focus_target: "map".into(), pickable: default(), active_events: default(), hidden_geometry: default(),
            show_helpers: false, show_grid: false, show_lights: false, show_bubbles: false,
            preview_ambience: None, camera_speed: default_speed() }
    }
}
#[derive(Component)] struct ObjectRoot(String);
#[derive(Component)] struct ViewCamera;
#[derive(Default, Reflect, GizmoConfigGroup)] struct Handles;

struct Placed { entity: Entity, object: Object, ready: bool, placeholder: bool, bounds: (Vec3, Vec3) }
enum Visual { Flat(Vec<crate::map::FlatSurface>), Scene(Handle<bevy::world_serialization::WorldAsset>), Missing }
struct Model { handle: Option<Handle<Gltf>>, visual: Option<Visual>, bounds: (Vec3, Vec3) }
#[derive(Resource)]
struct Editor {
    desktop: bool,
    pending_scene: Option<Scene>,
    inbox: Vec<serde_json::Value>,
    pointer_blocked: bool,
    keyboard_blocked: bool,
    viewport: Option<Rect>,
    directory: PathBuf,
    revision: u64,
    map: String,
    center: Vec3,
    extents: Vec3,
    scene_stamp: Option<SystemTime>,
    controls_stamp: Option<SystemTime>,
    poll_at: f32,
    status_at: f32,
    controls: Controls,
    placed: HashMap<String, Placed>,
    models: HashMap<String, Model>,
    events: u64,
    drag: Option<Drag>,
    focus_serial: u64,
    needs_frame: bool,
    error: String,
    ambience: Option<crate::lighting::Bubble>,
}
impl Editor {
    fn send(&mut self, mut event: serde_json::Value) {
        self.events += 1;
        event["revision"] = self.revision.into();
        if self.desktop { self.inbox.push(event); return; }
        let path = self.directory.join("events").join(format!("{:016}.json", self.events));
        if let Err(error) = atomic_write(&path, &event) { self.error = error.to_string(); }
    }
    fn selected(&self) -> Option<&Placed> { self.controls.selected.as_ref().and_then(|key| self.placed.get(key)).filter(|p| self.controls.visible(&p.object)) }
}
#[derive(Clone)]
struct Drag {
    key: String,
    revision: u64,
    mode: String,
    axis: Option<usize>,
    original: Transform,
    start: Vec2,
    screen_axis: Vec2,
    length: f32,
    angle: f32,
    factors: Vec3,
    changed: bool,
    rotation_start: Option<Vec3>,
}

fn atomic_write(path: &std::path::Path, value: &impl Serialize) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec(value)?)?;
    // On Windows rename cannot replace an existing file. Status is informational;
    // event names are unique and must never overwrite another completed gesture.
    if path.exists() { std::fs::remove_file(path)?; }
    std::fs::rename(temporary, path)
}

pub fn run(directory: PathBuf) {
    run_app(directory, None);
}

pub fn desktop() {
    match desktop::State::from_args() {
        Ok(state) => { std::env::set_var("DSOR_ASSETS", &state.project.assets); let directory = state.project.cache.join("live").join(format!("rust-{}", std::process::id())); run_app(directory, Some(state)); }
        Err(error) => { eprintln!("Level editor: {error}"); std::process::exit(1); }
    }
}

fn run_app(directory: PathBuf, desktop: Option<desktop::State>) {
    let _ = std::fs::create_dir_all(directory.join("events"));
    let is_desktop = desktop.is_some();
    let title = format!("DSOR Level Viewport {}", directory.file_name().unwrap_or_default().to_string_lossy());
    let editor = Editor { directory, desktop: is_desktop, pending_scene: None, inbox: vec![], pointer_blocked: false, keyboard_blocked: false, viewport: None, revision: 0, map: String::new(), center: Vec3::ZERO, extents: Vec3::splat(30.),
        scene_stamp: None, controls_stamp: None, poll_at: -1., status_at: -1., controls: default(),
        placed: default(), models: default(), events: 0, drag: None, focus_serial: 0, needs_frame: true, error: String::new(), ambience: None };
    let mut app = App::new();
    app
        .add_plugins(DefaultPlugins.set(AssetPlugin { file_path: crate::asset_root(), meta_check: bevy::asset::AssetMetaCheck::Never, ..default() })
            .set(WindowPlugin { primary_window: Some({ let mut window = Window { title: if is_desktop { "DSOR • Level Editor".into() } else { title }, resolution: (1600, 960).into(),
                present_mode: bevy::window::PresentMode::AutoVsync, ..default() }; window.set_maximized(is_desktop); window }), close_when_requested: !is_desktop, ..default() }))
        .insert_resource(bevy::winit::WinitSettings { focused_mode: bevy::winit::UpdateMode::Continuous,
            unfocused_mode: bevy::winit::UpdateMode::Continuous })
        .add_plugins((crate::surfaces::SurfacesPlugin, crate::refraction::RefractionPlugin,
            crate::materials::MaterialsPlugin, crate::particles::ParticlePlugin, crate::decals::DecalPlugin,
            crate::character::CharacterPlugin, crate::skills::EditorSequencesPlugin))
        .add_plugins((crate::lighting::EditorLightingPlugin, crate::clamp::ClampPlugin, crate::nebula_bloom::NebulaBloomPlugin))
        .init_asset::<crate::npc::Variations>().init_asset_loader::<crate::npc::VariationsLoader>()
        .init_asset::<crate::map::MapManifest>()
        .insert_resource(crate::map::CurrentMap::new("editor".into(), Handle::default()))
        .insert_resource(editor)
        .init_resource::<fly_camera::FlyCamera>()
        .insert_resource(ClearColor(Color::srgb(0.055, 0.075, 0.10)))
        .insert_resource(GlobalAmbientLight { color: Color::srgb(0.75, 0.82, 0.93), brightness: 650., ..default() })
        .init_gizmo_group::<Handles>()
        .init_resource::<previews::Previews>()
        .init_resource::<camera_preview::Monitor>()
        .init_resource::<lighting_preview::State>()
        .add_systems(Startup, (setup, previews::setup, camera_preview::setup).chain())
        .add_systems(Update, (sync, stream, previews::update, fly_camera::navigate, interact,
            camera_preview::update, lighting_preview::update, draw_handles, camera_preview::draw, lighting_preview::draw, status).chain())
        .configure_sets(Update, crate::lighting::ApplyEditorLighting.after(lighting_preview::update))
        .add_systems(PostUpdate, previews::isolate.before(bevy::camera::visibility::VisibilitySystems::CheckVisibility))
        .add_systems(Update, capture_when_ready);
    if let Some(state) = desktop { desktop::install(&mut app, state); }
    app.run();
}

fn setup(mut commands: Commands, mut config: ResMut<GizmoConfigStore>) {
    commands.spawn((ViewCamera, Camera3d::default(), Transform::from_xyz(20., 25., 25.).looking_at(Vec3::ZERO, Vec3::Y),
        bevy::camera::visibility::RenderLayers::from_layers(&[0, camera_preview::EDITOR_LAYER]),
        bevy::core_pipeline::tonemapping::Tonemapping::None, bevy::core_pipeline::prepass::DepthPrepass, Msaa::Off,
        bevy::anti_alias::fxaa::Fxaa::default()));
    commands.spawn((crate::lighting::Sun, DirectionalLight { illuminance: 8500., shadow_maps_enabled: true, ..default() },
        bevy::light::CascadeShadowConfigBuilder { num_cascades: 1, maximum_distance: 100., ..default() }.build(),
        Transform::default().looking_to(Vec3::new(-0.6, -1., -0.4), Vec3::Y)));
    let (handles, _) = config.config_mut::<Handles>();
    handles.depth_bias = -1.;
    handles.line.width = 3.;
    handles.render_layers = bevy::camera::visibility::RenderLayers::layer(camera_preview::EDITOR_LAYER);
    config.config_mut::<bevy::gizmos::config::DefaultGizmoConfigGroup>().0.render_layers =
        bevy::camera::visibility::RenderLayers::layer(camera_preview::EDITOR_LAYER);
}

fn sync(mut commands: Commands, mut editor: ResMut<Editor>, time: Res<Time>, mut exit: MessageWriter<AppExit>, assets: Res<AssetServer>) {
    if time.elapsed_secs() < editor.poll_at { return; }
    editor.poll_at = time.elapsed_secs() + 0.08;
    if editor.directory.join("stop.json").exists() { exit.write(AppExit::Success); return; }
    let path = editor.directory.join("scene.json");
    let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    if editor.pending_scene.is_some() || (stamp.is_some() && stamp != editor.scene_stamp) {
        let scene = editor.pending_scene.take().or_else(|| std::fs::read(&path).ok().and_then(|raw| serde_json::from_slice::<Scene>(&raw).ok()));
        match scene {
            Some(scene) if scene.protocol == 1 => {
                editor.scene_stamp = stamp;
                editor.drag = None;
                editor.needs_frame |= scene.map != editor.map;
                if scene.map != editor.map { editor.controls.preview_ambience = None; }
                editor.map = scene.map;
                editor.center = Vec3::from(scene.center);
                editor.extents = Vec3::from(scene.extents);
                editor.revision = scene.revision;
                editor.ambience = scene.ambience;
                let keep: HashSet<_> = scene.objects.iter().map(|o| o.key.clone()).collect();
                editor.placed.retain(|key, placed| {
                    if keep.contains(key) { true } else { commands.entity(placed.entity).despawn(); false }
                });
                for object in scene.objects {
                    if !object.matrix.iter().all(|n| n.is_finite()) { continue; }
                    if let Some(placed) = editor.placed.get_mut(&object.key) {
                        if placed.object.same_visual(&object) {
                            commands.entity(placed.entity).insert(object.transform());
                            placed.object = object;
                            continue;
                        }
                        commands.entity(placed.entity).despawn();
                    }
                    let entity = commands.spawn((ObjectRoot(object.key.clone()), Name::new(object.label.clone()),
                        object.transform(), Visibility::Inherited)).id();
                    let helper = object.model.starts_with("helpers/");
                    let mut ready = helper;
                    let mut placeholder = helper || object.model.is_empty();
                    if let Some(character) = &object.character {
                        let desc = crate::character::CharacterDesc { look: Some(crate::character::NpcLook {
                            skeleton: character.skeleton.clone(), parts: character.parts.clone(), anim_set: character.anim_set.clone(),
                            variation: character.variation.iter().map(|v| (v.bone.clone(), Vec3::from(v.t), Vec3::from(v.s))).collect(),
                        }), ..default() };
                        let child = crate::character::spawn_character(&mut commands, desc, Transform::IDENTITY);
                        let mut animation = crate::character::CharacterAnim::default();
                        animation.state = crate::character::AnimState::Named(character.state.clone());
                        commands.entity(child).insert((ChildOf(entity), animation));
                        ready = true;
                        placeholder = false;
                    }
                    if let Some(light) = &object.light {
                        let phase = object.key.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32)) as f32 * 0.001;
                        crate::lighting::spawn_level_light(&mut commands, light, entity, Transform::IDENTITY, phase, 1.);
                    }
                    if let Some(sequence) = object.sequence.as_ref().and_then(|value| serde_json::from_value::<crate::skills::Sequence>(value.clone()).ok()) {
                        crate::skills::play(&mut commands, sequence, None, entity, true);
                    }
                    for decoration in &object.decorations {
                        let mut at = Transform::from_matrix(Mat4::from_cols_array(&decoration.matrix));
                        at.rotation *= Quat::from_rotation_y(std::f32::consts::PI);
                        if let Some(character) = &decoration.character {
                            let desc = crate::character::CharacterDesc { look: Some(crate::character::NpcLook {
                                skeleton: character.skeleton.clone(), parts: character.parts.clone(), anim_set: character.anim_set.clone(),
                                variation: character.variation.iter().map(|v| (v.bone.clone(), Vec3::from(v.t), Vec3::from(v.s))).collect(),
                            }), ..default() };
                            let child = crate::character::spawn_character(&mut commands, desc, at);
                            commands.entity(child).insert(ChildOf(entity));
                        } else {
                            commands.spawn((WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(crate::map::model_path(&decoration.model)))), at, ChildOf(entity), Visibility::Inherited));
                        }
                    }
                    if !ready && !object.model.is_empty() {
                        editor.models.entry(object.model.clone()).or_insert(Model { handle: None, visual: None, bounds: (Vec3::ZERO, Vec3::ONE) });
                    }
                    let bounds = if object.light.is_some() || object.ambience.is_some() { (Vec3::ZERO, Vec3::splat(0.5)) }
                        else { (Vec3::Y * 0.7, Vec3::splat(0.7)) };
                    editor.placed.insert(object.key.clone(), Placed { entity, object, ready, placeholder, bounds });
                }
            }
            _ => { editor.error = "Could not read the live scene; retaining the last valid scene".into(); }
        }
    }
    let path = editor.directory.join("controls.json");
    let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    if stamp.is_some() && stamp != editor.controls_stamp {
        if let Some(controls) = std::fs::read(path).ok().and_then(|raw| serde_json::from_slice::<Controls>(&raw).ok()) {
            if controls.revision == editor.revision {
                editor.controls = controls;
                editor.controls_stamp = stamp;
            }
        }
    }
    for placed in editor.placed.values() {
        commands.entity(placed.entity).insert(if editor.controls.visible(&placed.object) { Visibility::Inherited } else { Visibility::Hidden });
    }
}

#[allow(clippy::too_many_arguments)]
fn stream(mut commands: Commands, mut editor: ResMut<Editor>, assets: Res<AssetServer>, gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>, meshes: Res<Assets<GltfMesh>>, materials: Res<Assets<GltfMaterial>>, mesh_assets: Res<Assets<Mesh>>) {
    let mut in_flight = editor.models.values().filter(|m| m.handle.is_some() && m.visual.is_none()).count();
    for (reference, model) in &mut editor.models {
        if model.visual.is_some() { continue; }
        if model.handle.is_none() {
            if in_flight >= crate::map::MAX_MODELS_IN_FLIGHT { continue; }
            let path = crate::map::model_path(reference);
            if !PathBuf::from(crate::asset_root()).join(&path).is_file() {
                model.visual = Some(Visual::Missing);
                continue;
            }
            model.handle = Some(assets.load(path));
            in_flight += 1;
        }
        let handle = model.handle.as_ref().unwrap();
        match assets.get_load_state(handle) {
            Some(LoadState::Failed(_)) => model.visual = Some(Visual::Missing),
            Some(LoadState::Loaded) => {
                let Some(gltf) = gltfs.get(handle) else { continue };
                if let Some(surfaces) = crate::map::flat_surfaces(gltf, &nodes, &meshes, &materials, &mesh_assets, &assets) {
                    let (mut low, mut high) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
                    for surface in &surfaces {
                        if let Some(bounds) = mesh_assets.get(&surface.mesh).and_then(|m| m.compute_aabb()) {
                            for x in [-1., 1.] { for y in [-1., 1.] { for z in [-1., 1.] {
                                let local = Vec3::from(bounds.center) + Vec3::from(bounds.half_extents) * Vec3::new(x, y, z);
                                let point = surface.local.transform_point3(local);
                                low = low.min(point); high = high.max(point);
                            } } }
                        }
                    }
                    if low.is_finite() && high.is_finite() { model.bounds = ((low + high) * 0.5, ((high - low) * 0.5).max(Vec3::splat(0.05))); }
                    model.visual = Some(Visual::Flat(surfaces));
                } else {
                    model.visual = gltf.scenes.first().cloned().map(Visual::Scene).or(Some(Visual::Missing));
                }
            }
            _ => {}
        }
    }
    let editor = &mut *editor;
    for placed in editor.placed.values_mut().filter(|p| !p.ready) {
        if placed.object.model.is_empty() { placed.ready = true; continue; }
        let Some(model) = editor.models.get(&placed.object.model) else { continue };
        let Some(visual) = &model.visual else { continue };
        match visual {
            Visual::Flat(surfaces) => {
                for surface in surfaces {
                    let mut entity = commands.spawn((Mesh3d(surface.mesh.clone()), Transform::from_matrix(Mat4::from(surface.local)), ChildOf(placed.entity)));
                    surface.material.insert(&mut entity);
                    if surface.no_shadow { entity.insert(bevy::light::NotShadowCaster); }
                }
            }
            Visual::Scene(scene) => { commands.spawn((WorldAssetRoot(scene.clone()), Transform::IDENTITY, ChildOf(placed.entity))); }
            Visual::Missing => { placed.placeholder = true; }
        }
        placed.bounds = model.bounds;
        placed.ready = true;
    }
}

fn segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    point.distance(a + ab * ((point - a).dot(ab) / ab.length_squared().max(0.001)).clamp(0., 1.))
}
fn axis(index: usize) -> Vec3 { [Vec3::X, Vec3::Y, Vec3::Z][index] }
fn handle_axis(index: usize, mode: &str, at: &Transform) -> Vec3 {
    if mode == "scale" { at.rotation * axis(index) } else { axis(index) }
}
fn ring_point(axis_index: usize, angle: f32) -> Vec3 {
    let (s, c) = angle.sin_cos();
    [Vec3::new(0., c, s), Vec3::new(s, 0., c), Vec3::new(c, s, 0.)][axis_index]
}
fn plane_vector(ray: Ray3d, center: Vec3, normal: Vec3) -> Option<Vec3> {
    let denominator = normal.dot(*ray.direction);
    if denominator.abs() < 0.0001 { return None; }
    let t = normal.dot(center - ray.origin) / denominator;
    (t > 0.).then(|| (ray.get_point(t) - center).normalize_or_zero()).filter(|v| v.length_squared() > 0.1)
}
fn handle_length(camera: Vec3, point: Vec3) -> f32 { (camera.distance(point) * 0.12).clamp(0.6, 35.) }

#[allow(clippy::too_many_arguments)]
fn interact(mut editor: ResMut<Editor>, windows: Query<&Window>, cameras: Query<(&Camera, &GlobalTransform), With<ViewCamera>>,
    buttons: Res<ButtonInput<MouseButton>>, keys: Res<ButtonInput<KeyCode>>, mut raycast: MeshRayCast,
    parents: Query<&ChildOf>, roots: Query<&ObjectRoot>, mut transforms: Query<&mut Transform, (With<ObjectRoot>, Without<ViewCamera>)>) {
    let Ok(window) = windows.single() else { return };
    let Ok((camera, camera_at)) = cameras.single() else { return };
    if let Some(drag) = editor.drag.clone() {
        if keys.just_pressed(KeyCode::Escape) || !window.focused || drag.revision != editor.revision {
            if let Some(placed) = editor.placed.get(&drag.key) {
                if let Ok(mut at) = transforms.get_mut(placed.entity) { *at = drag.original; }
            }
            editor.drag = None;
            return;
        }
    }
    if !window.focused || (editor.keyboard_blocked && editor.drag.is_none()) { return; }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if control {
        for (key, action) in [(KeyCode::KeyS, "save"), (KeyCode::KeyZ, "undo"), (KeyCode::KeyY, "redo"), (KeyCode::KeyD, "duplicate")] {
            if keys.just_pressed(key) { editor.send(json!({"action": action})); }
        }
    } else if !buttons.pressed(MouseButton::Right) && !buttons.pressed(MouseButton::Middle)
        && !keys.pressed(KeyCode::AltLeft) && !keys.pressed(KeyCode::AltRight) {
        for (key, mode) in [(KeyCode::KeyQ, "select"), (KeyCode::KeyW, "translate"), (KeyCode::KeyE, "rotate"),
                            (KeyCode::KeyR, "scale"), (KeyCode::KeyG, "translate"), (KeyCode::KeyS, "scale")] {
            if keys.just_pressed(key) && editor.drag.is_none() && editor.selected().is_some()
                && !(mode == "scale" && editor.selected().is_some_and(|p| p.object.camera.is_some())) {
                editor.controls.mode = mode.into();
                editor.send(json!({"action": "mode", "mode": mode}));
            }
        }
        if keys.just_pressed(KeyCode::Delete) && editor.drag.is_none() { editor.send(json!({"action": "delete"})); }
    }
    let Some(cursor) = window.cursor_position() else { return };
    if editor.pointer_blocked && editor.drag.is_none() { return; }
    let Ok(ray) = camera.viewport_to_world(camera_at, cursor) else { return };
    if let Some(mut drag) = editor.drag.take() {
        let Some(placed) = editor.placed.get(&drag.key) else { return };
        let Ok(mut at) = transforms.get_mut(placed.entity) else { return };
        let pixels = cursor - drag.start;
        if pixels.length() > 2. || drag.changed {
            drag.changed = true;
            *at = drag.original;
            let along = if drag.screen_axis.length_squared() > 1. { pixels.dot(drag.screen_axis) / drag.screen_axis.length_squared() } else { -pixels.y / 90. };
            match drag.mode.as_str() {
                "rotate" => {
                    let index = drag.axis.unwrap_or(1);
                    let mut angle = match (drag.rotation_start, plane_vector(ray, drag.original.translation, axis(index))) {
                        (Some(start), Some(current)) => axis(index).dot(start.cross(current)).atan2(start.dot(current)),
                        _ => (pixels.x - pixels.y) * 0.01,
                    };
                    if editor.controls.snap > 0. && !control { let step = 15_f32.to_radians(); angle = (angle / step).round() * step; }
                    drag.angle = angle;
                    at.rotation = Quat::from_axis_angle(axis(index), angle) * drag.original.rotation;
                }
                "scale" => {
                    let mut factor = (1. + if drag.axis.is_some() { along } else { (pixels.x - pixels.y) / 100. }).max(0.01);
                    if editor.controls.snap > 0. && !control { factor = (factor * 10.).round().max(1.) / 10.; }
                    drag.factors = Vec3::ONE;
                    if let Some(index) = drag.axis { drag.factors[index] = factor; } else { drag.factors = Vec3::splat(factor); }
                    at.scale = drag.original.scale * drag.factors;
                }
                _ => {
                    let index = drag.axis.unwrap_or(0);
                    let mut target = drag.original.translation[index] + along * drag.length;
                    if editor.controls.snap > 0. && !control { target = (target / editor.controls.snap).round() * editor.controls.snap; }
                    at.translation[index] = target;
                }
            }
        }
        if buttons.just_released(MouseButton::Left) || !buttons.pressed(MouseButton::Left) {
            if drag.changed {
                editor.send(json!({"action": "transform", "key": drag.key, "mode": drag.mode,
                    "position": at.translation.to_array(), "rotation": at.rotation.normalize().to_array(), "scale": at.scale.to_array(),
                    "axis": drag.axis, "angle": drag.angle, "factors": drag.factors.to_array()}));
            }
        } else { editor.drag = Some(drag); }
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) { return; }
    if camera_preview::contains(&editor, window, cursor) { return; }
    if let Some(placed) = editor.selected().filter(|_| editor.controls.mode != "select") {
        let Ok(at) = transforms.get(placed.entity) else { return };
        let length = handle_length(camera_at.translation(), at.translation);
        if let Ok(origin) = camera.world_to_viewport(camera_at, at.translation) {
            let mut best = (11., None);
            for index in 0..3 {
                let distance = if editor.controls.mode == "rotate" {
                    (0..64).filter_map(|i| {
                        let a = at.translation + ring_point(index, i as f32 * std::f32::consts::TAU / 64.) * length;
                        let b = at.translation + ring_point(index, (i + 1) as f32 * std::f32::consts::TAU / 64.) * length;
                        Some(segment_distance(cursor, camera.world_to_viewport(camera_at, a).ok()?, camera.world_to_viewport(camera_at, b).ok()?))
                    }).fold(f32::INFINITY, f32::min)
                } else {
                    camera.world_to_viewport(camera_at, at.translation + handle_axis(index, &editor.controls.mode, at) * length).ok()
                        .filter(|end| end.distance(origin) > 12.)
                        .map(|end| segment_distance(cursor, origin.lerp(end, 0.18), end)).unwrap_or(f32::INFINITY)
                };
                if distance < best.0 { best = (distance, Some(index)); }
            }
            let uniform = editor.controls.mode == "scale" && cursor.distance(origin) < 10.;
            if best.1.is_some() || uniform {
                let selected_axis = if uniform { None } else { best.1 };
                let screen_axis = selected_axis.and_then(|index| camera.world_to_viewport(camera_at, at.translation + handle_axis(index, &editor.controls.mode, at) * length).ok()).unwrap_or(origin) - origin;
                editor.drag = Some(Drag { key: placed.object.key.clone(), revision: editor.revision, mode: editor.controls.mode.clone(),
                    axis: selected_axis, original: *at, start: cursor, screen_axis, length, angle: 0., factors: Vec3::ONE, changed: false,
                    rotation_start: selected_axis.and_then(|index| plane_vector(ray, at.translation, axis(index))) });
                return;
            }
        }
    }
    let owner = |entity: Entity| -> Option<String> {
        roots.get(entity).ok().map(|root| root.0.clone()).or_else(|| parents.iter_ancestors(entity).find_map(|parent| roots.get(parent).ok().map(|root| root.0.clone())))
    };
    let filter = |entity| owner(entity).is_some_and(|key| editor.controls.pickable.contains(&key));
    let settings = MeshRayCastSettings::default().with_filter(&filter);
    let mut hit = raycast.cast_ray(ray, &settings).first().and_then(|(entity, hit)| owner(*entity).map(|key| (hit.distance, key)));
    // Metadata-only records and missing models are still selectable in 3D.
    for (key, placed) in &editor.placed {
        if !editor.controls.pickable.contains(key) || !editor.controls.visible(&placed.object) { continue; }
        if !(placed.placeholder || !placed.ready || placed.object.kind != "geometry") { continue; }
        let Ok(at) = transforms.get(placed.entity) else { continue };
        let center = at.translation + if placed.object.camera.is_some() || placed.object.light.is_some() || placed.object.ambience.is_some() { Vec3::ZERO } else { Vec3::Y * 0.5 };
        if camera.world_to_viewport(camera_at, center).is_ok_and(|point| point.distance(cursor) < 13.) {
            let distance = center.distance(ray.origin);
            if hit.as_ref().is_none_or(|(near, _)| distance <= *near + 1.) { hit = Some((distance, key.clone())); }
        }
    }
    if let Some((_, key)) = hit {
        editor.controls.selected = Some(key.clone());
        editor.send(json!({"action": "select", "key": key}));
    }
}

fn draw_handles(editor: Res<Editor>, cameras: Query<&GlobalTransform, With<ViewCamera>>,
    transforms: Query<&Transform, With<ObjectRoot>>, mut grid: Gizmos, mut gizmos: Gizmos<Handles>) {
    let Ok(camera) = cameras.single() else { return };
    let center = (camera.translation() / 5.).floor() * 5.;
    let gray = Color::srgba(0.3, 0.4, 0.45, 0.35);
    for i in (-20..=20).filter(|_| editor.controls.show_grid) {
        let v = i as f32 * 5.;
        grid.line(Vec3::new(center.x + v, editor.center.y, center.z - 100.), Vec3::new(center.x + v, editor.center.y, center.z + 100.), gray);
        grid.line(Vec3::new(center.x - 100., editor.center.y, center.z + v), Vec3::new(center.x + 100., editor.center.y, center.z + v), gray);
    }
    for (key, placed) in &editor.placed {
        if !editor.controls.visible(&placed.object) { continue; }
        if placed.object.camera.is_some() || placed.object.light.is_some() || placed.object.ambience.is_some() { continue; }
        let missing_model = placed.placeholder && !placed.object.model.is_empty() && !placed.object.model.starts_with("helpers/");
        if !editor.controls.show_helpers && !missing_model && editor.controls.selected.as_ref() != Some(key) { continue; }
        if placed.object.model.starts_with("helpers/") && editor.controls.selected.as_ref() != Some(key) { continue; }
        let Ok(at) = transforms.get(placed.entity) else { continue };
        if placed.placeholder || !placed.ready || placed.object.kind != "geometry" {
            let color = if placed.object.kind == "_Instance_Light" { Color::srgb(1., 0.8, 0.2) }
                else if placed.object.kind == "geometry" { Color::srgb(1., 0.25, 0.7) } else { Color::srgb(0.2, 0.85, 0.9) };
            grid.cube(Transform::from_translation(at.translation + Vec3::Y * 0.5).with_scale(Vec3::splat(0.65)), color);
        }
    }
    let Some(placed) = editor.selected() else { return };
    if !editor.controls.visible(&placed.object) { return; }
    let Ok(at) = transforms.get(placed.entity) else { return };
    let length = handle_length(camera.translation(), at.translation);
    if placed.object.camera.is_none() && placed.object.light.is_none() && placed.object.ambience.is_none() {
        gizmos.cube(*at * Transform::from_translation(placed.bounds.0).with_scale(placed.bounds.1 * 2.), Color::srgb(1., 0.85, 0.3));
    }
    if editor.controls.mode == "select" { return; }
    for (index, color) in [Color::srgb(1., 0.25, 0.22), Color::srgb(0.3, 1., 0.4), Color::srgb(0.3, 0.55, 1.)].into_iter().enumerate() {
        if editor.controls.mode == "rotate" {
            gizmos.linestrip((0..=64).map(|i| at.translation + ring_point(index, i as f32 * std::f32::consts::TAU / 64.) * length), color);
        } else {
            let end = at.translation + handle_axis(index, &editor.controls.mode, at) * length;
            gizmos.arrow(at.translation, end, color);
            if editor.controls.mode == "scale" { gizmos.cube(Transform::from_translation(end).with_scale(Vec3::splat(length * 0.09)), color); }
        }
    }
    if editor.controls.mode == "scale" { gizmos.cube(Transform::from_translation(at.translation).with_scale(Vec3::splat(length * 0.12)), Color::WHITE); }
}

fn status(mut editor: ResMut<Editor>, time: Res<Time>,
    windows: Query<&Window>, cameras: Query<(&Camera, &GlobalTransform), With<ViewCamera>>,
    buttons: Res<ButtonInput<MouseButton>>, monitor: Res<camera_preview::Monitor>, lighting: Res<lighting_preview::State>, look: Res<crate::lighting::EditorLook>) {
    if time.elapsed_secs() < editor.status_at { return; }
    editor.status_at = time.elapsed_secs() + 0.5;
    let ready = editor.placed.values().filter(|p| p.ready).count();
    let missing = editor.placed.values().filter(|p| p.placeholder && !p.object.model.is_empty() && !p.object.model.starts_with("helpers/")).count();
    let selected = editor.selected().map(|p| p.object.label.as_str()).unwrap_or("Click an object to select");
    let mut message = format!("{} | {ready}/{} objects | {missing} missing models | {} | {selected}", editor.map, editor.placed.len(), editor.controls.mode);
    if !editor.error.is_empty() { message.push_str(&format!(" | {}", editor.error)); }
    let window = windows.single().ok();
    let handles = editor.selected().and_then(|placed| {
        let (camera, camera_at) = cameras.single().ok()?;
        let at = placed.object.transform();
        let length = handle_length(camera_at.translation(), at.translation);
        Some((0..3).map(|index| camera.world_to_viewport(camera_at,
            at.translation + handle_axis(index, &editor.controls.mode, &at) * length).ok().map(|point| point.to_array())).collect::<Vec<_>>())
    });
    let _ = atomic_write(&editor.directory.join("status.json"), &json!({"revision": editor.revision, "message": message,
        "ready": ready, "total": editor.placed.len(), "missing": missing,
        "visible": editor.placed.values().filter(|p| editor.controls.visible(&p.object)).count(),
        "camera_position": cameras.single().ok().map(|(_, t)| t.translation().to_array()),
        "camera_forward": cameras.single().ok().map(|(_, t)| t.forward().to_array()),
        "focused": window.is_some_and(|w| w.focused), "cursor": window.and_then(|w| w.cursor_position()).map(|p| p.to_array()),
        "camera_preview": monitor.state,
        "lighting": {"source": lighting.source, "source_key": lighting.source_key, "previewing": lighting.previewing,
            "observer": lighting.observer.map(|p| p.to_array()), "points": lighting.points, "spots": lighting.spots,
            "sun_intensity": look.bubble.light_intensity, "ambient": look.bubble.light_ambient},
        "mode": editor.controls.mode, "dragging": editor.drag.is_some(), "left_pressed": buttons.pressed(MouseButton::Left), "handles": handles}));
}

fn capture_when_ready(mut commands: Commands, editor: Res<Editor>, time: Res<Time>, mut ready_since: Local<Option<f32>>, mut done: Local<bool>) {
    if *done { return; }
    let Ok(path) = std::env::var("DSOR_EDITOR_SCREENSHOT") else { return };
    if editor.placed.is_empty() || editor.placed.values().any(|placed| !placed.ready) { return; }
    let start = *ready_since.get_or_insert(time.elapsed_secs());
    if time.elapsed_secs() - start < 5. { return; }
    *done = true;
    commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window())
        .observe(bevy::render::view::screenshot::save_to_disk(path));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gizmo_hit_distance_clamps_to_segment() {
        assert_eq!(segment_distance(Vec2::new(5., 3.), Vec2::ZERO, Vec2::new(10., 0.)), 3.);
        assert_eq!(segment_distance(Vec2::new(13., 4.), Vec2::ZERO, Vec2::new(10., 0.)), 5.);
    }
    #[test]
    fn rotation_rings_stay_in_their_axis_plane() {
        for a in 0..3 { for step in 0..20 {
            let point = ring_point(a, step as f32 * 0.2);
            assert!(point.dot(axis(a)).abs() < 1e-6);
            assert!((point.length() - 1.).abs() < 1e-6);
        } }
    }
}
