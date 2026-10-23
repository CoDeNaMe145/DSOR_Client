//! The interface's turning 3D views: the player's character on the
//! inventory's equipment and trinket pages, its Shadow Soul on the mercenary
//! page, and the collector's album's sticker -- a costume or an emote on the
//! player, the player on a mount, a pet, a Shadow Soul (`subjects`). The
//! character and the Shadow Soul are stages of their own: turning the pages
//! shows each at once, as it was left.
//!
//! Each view is a stage: an image its own camera draws, every frame while its
//! window shows it, of a subject standing far under the maps on a render layer
//! only that camera sees, lit by a light of its own -- the map's sun, fog and
//! colour grading never reach it (NoLevelLook; HighlightCamera, which every
//! scene camera query leaves out). The window draws the image as one of its
//! widgets, in its own drawing order: `prepare_inventory` and the album's
//! prepare give the preview area a widget of the stage's texture name
//! (`Stage::texture`), which spawn_windows binds to the image. Holding the
//! left button over the area's DragRotate buttons turns the subject.
//!
//! A new subject dresses hidden behind the one shown, which it replaces once
//! ready (crate::screens::selection's way): changing stickers never flashes an
//! empty box. Once posed, the camera frames each subject from what it draws
//! (`measure`): its feet on the area's floor mark, all of it in view whichever
//! way it turns.
//!
//! CONTRACT: a closed window's stage costs nothing: its camera is off and its
//!   subject parked (without crate::screens::selection::Preview a hidden actor's
//!   animation stops, crate::actors::character::drive_animations). A subject is
//!   rebuilt only when what it shows changes (UiWork::previews): `choose`
//!   compares a few revisions each frame and builds nothing unless one moved.
//! EVIDENCE: the inventory's `figur` widgets (exported hidden: a runtime
//!   render target) and the DragRotateAreaButton01/02 buttons (event DragRotate)
//!   of the inventory's and the album's layouts. The original (dro_client64.exe,
//!   read 2026-10-23) shows the player's own character in both, as the
//!   `inventorycharacter` entity: the inventory's plays the `Inventory`
//!   animation state, the album's `Idle_Hub`; a mount sticker shows the player
//!   riding it. Its cameras look from the front left (_Globals
//!   InventoryCamPosition (-2.49, 0, 8.42), a 10-degree field of view).
//! DEVIATION: the original frames each sticker by its own camera
//!   (_Template_StickerbookItem CameraPosition, CameraCharactorHeight...) and
//!   lights the stages with _Globals' InventoryLight colours; here every
//!   subject is framed from its posed bounds and lit alike.

use bevy::app::{HierarchyPropagatePlugin, Propagate, PropagateSet};
use bevy::camera::primitives::Aabb;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::controls::WindowControls;
use super::{Art, Layout, Screen, UiMaterial, UiWork, Window2018};
use crate::actors::appearance::Equipment;
use crate::actors::character::{AnimState, Character, CharacterAnim, CharacterDesc, ModelBones};
use crate::network::net::LocalPlayer;
use crate::screens::selection::Preview;

pub(super) mod subjects;
#[cfg(test)]
mod tests;

/// A turning view's place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Stage {
    /// The inventory's `figur` on its equipment and trinket pages: the character.
    Inventory,
    /// The inventory's `figur` on its mercenary page: the Shadow Soul.
    Mercenary,
    /// The album's preview box: the sticker picked.
    Album,
}

pub(crate) const STAGES: [Stage; 3] = [Stage::Inventory, Stage::Mercenary, Stage::Album];

/// The inventory's `figur` (oben/label_empty/deco_verlauf_character/figur).
const INVENTORY_AREA: [f32; 4] = [609.2, 60.2, 1009.9, 383.3];
/// The album's preview box under its headline: background_image (663.0, 195.8,
/// 958.4, 513.0) less background_headline (to 229.0).
const ALBUM_AREA: [f32; 4] = [663.0, 229.0, 958.4, 513.0];

impl Stage {
    fn index(self) -> usize { self as usize }
    /// Its render layer, only its camera's (the interface's is 7, the
    /// highlight's 5, the level editor's 30 and 31).
    fn layer(self) -> RenderLayers { RenderLayers::layer(26 + self as usize) }
    /// Where its subject stands: far under every map, the stages apart.
    fn origin(self) -> Vec3 { Vec3::new(60.0 * self as usize as f32, -10_000.0, 0.0) }
    /// The texture name of the widget drawing it (spawn_windows binds the image).
    pub(super) fn texture(self) -> &'static str {
        match self { Self::Inventory => "preview:inventory", Self::Mercenary => "preview:mercenary", Self::Album => "preview:album" }
    }
    pub(super) fn of_texture(texture: &str) -> Option<Self> { STAGES.into_iter().find(|s| s.texture() == texture) }
    /// The reference rect it fills.
    pub(super) fn area(self) -> [f32; 4] {
        match self { Self::Inventory | Self::Mercenary => INVENTORY_AREA, Self::Album => ALBUM_AREA }
    }
    /// Where the subject's top and its feet go, fractions of the image's
    /// height: in the inventory, from `deco_verlauf_character`'s top (95.5) to
    /// the middle of its `shadow` label (356.5..380.6); in the album, clear of
    /// the headline (to 229), down to the payment button's top (470.6), which
    /// an offer shows there.
    fn marks(self) -> (f32, f32) {
        let [_, t, _, b] = self.area();
        let (top, floor) = match self { Self::Inventory | Self::Mercenary => (95.5, 368.0), Self::Album => (252.0, 468.0) };
        ((top - t) / (b - t), (floor - t) / (b - t))
    }
    /// The window it is drawn in.
    fn window(self) -> &'static str {
        match self { Self::Inventory | Self::Mercenary => "inventory", Self::Album => super::controls::systems::ALBUM }
    }
}

/// A player character's look: what crate::actors::character dresses.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Look {
    pub class: u8,
    pub gender: u8,
    pub equipment: Vec<Equipment>,
    pub armament: i8,
    pub cosmetics: Vec<String>,
}

impl Look {
    pub(crate) fn of(desc: &CharacterDesc) -> Self {
        Self { class: desc.class, gender: desc.gender, equipment: desc.equipment.clone(), armament: desc.armament, cosmetics: desc.cosmetics.clone() }
    }
    fn desc(&self) -> CharacterDesc {
        CharacterDesc { class: self.class, gender: self.gender, equipment: self.equipment.clone(), armament: self.armament,
            cosmetics: self.cosmetics.clone(), look: None }
    }
}

/// A whole model with its character set and anim set: a mount's, a pet's, a
/// monster's (crate::actors::monsters::spawn_model).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Model {
    pub graphics: String,
    pub set: String,
    pub anim_set: String,
    pub dye: [[f32; 4]; 2],
}

/// What a stage shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Subject {
    /// The player's character, playing a state of its anim set (None: its
    /// standing idle) -- again and again after a rest, an emote -- or riding
    /// a mount.
    Character { look: Look, state: Option<String>, emote: bool, mount: Option<Model> },
    /// A whole model, playing a state (None: Idle).
    Model { model: Model, state: Option<String> },
}

/// The album's page group saying what its preview box shows: 1 the sticker
/// turning (its image, its DragRotate buttons), 0 its icon (a decoration, a
/// sticker of another class, a catalog without looks).
pub(super) const ALBUM_MODEL: &str = "sticker_model";

/// The stage's camera.
#[derive(Component)]
pub(crate) struct PreviewCamera;

/// What a stage's subjects stand on: DragRotate turns it.
#[derive(Component)]
pub(super) struct Turntable;

/// A window's mesh drawing a stage's image (spawn_windows): its material
/// follows the image when the image is remade at another size.
#[derive(Component)]
pub(super) struct Quad(pub Stage);

/// A rider on its mount (the model entity), seated on its seat joint.
#[derive(Component)]
pub(super) struct Seat { mount: Entity, anim_set: String }

/// An emote played again and again: its state, and while it rests, until when.
#[derive(Component)]
pub(super) struct Encore { state: String, rest_until: Option<f32> }

/// The rest between an emote's plays, seconds.
const ENCORE_REST: f32 = 1.2;
/// Frames a ready subject animates, hidden, before it is measured and shown.
const SETTLE_FRAMES: u8 = 3;

/// One stage.
struct Shown {
    image: Handle<Image>,
    size: UVec2,
    camera: Entity,
    root: Entity,
    /// What it shows and the entities showing it (a rider and its mount); a
    /// newer subject dressing behind it.
    subject: Option<Subject>,
    shown: Vec<Entity>,
    pending: Option<(Subject, Vec<Entity>)>,
    /// Frames the pending subject has been ready (posed) for.
    settled: u8,
    /// Its window shows it: its camera draws.
    open: bool,
    angle: f32,
}

/// The stages (indexed by Stage) and the inputs their subjects came from.
#[derive(Resource, Default)]
pub(super) struct Previews {
    stages: Vec<Shown>,
    inputs: Option<Inputs>,
    /// The player's look, and how many times it changed.
    look: Option<Look>,
    looks: u32,
    /// How many times the systems' views changed (the album's sticker).
    views: u32,
}

impl Previews {
    /// The image a stage draws into (spawn_windows).
    pub(super) fn image(&self, stage: Stage) -> Option<Handle<Image>> {
        self.stages.get(stage.index()).map(|s| s.image.clone())
    }
}

/// What the subjects are chosen from: compared each frame, nothing built
/// unless one moved.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
struct Inputs {
    /// The inventory open, its page; the album open.
    inventory: Option<u8>,
    album: bool,
    /// Revisions: the player's look, the views (the album's sticker), the
    /// player's running effects (Net::stat_revision: its Shadow Soul).
    looks: u32,
    views: u32,
    effects: u64,
    /// Which tables a subject is resolved from have loaded.
    tables: u8,
}

/// The stages' studio light: an ambient and a key light from the front left,
/// above the camera (the level editor's thumbnails', crate::level_editor::previews).
const AMBIENT: f32 = 650.0;
const KEY_LUX: f32 = 11_000.0;
/// The cameras' vertical field of view: narrow, as the original's (10
/// degrees), so the figure is hardly distorted.
const FOV: f32 = 10.0 * std::f32::consts::PI / 180.0;
/// Where the cameras look from, about the subject (InventoryCamPosition's
/// direction): from the front, a little to the left.
const VIEW: Vec2 = Vec2::new(-2.49, 8.42);
/// The turn per reference pixel the cursor moves while DragRotate is held.
const TURN_PER_PIXEL: f32 = 0.012;

/// The image's resolution at a scale (physical pixels per reference pixel).
fn image_size(stage: Stage, scale: f32) -> UVec2 {
    let [l, t, r, b] = stage.area();
    let s = scale.clamp(0.5, 4.0);
    UVec2::new(((r - l) * s).ceil().max(16.0) as u32, ((b - t) * s).ceil().max(16.0) as u32)
}

pub(super) struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn build(&self, app: &mut App) {
        // A subject's parts arrive asynchronously (scenes, outfits): its stage's
        // render layer reaches each as it is parented.
        app.add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate))
            .configure_sets(PostUpdate, PropagateSet::<RenderLayers>::default().before(VisibilitySystems::CheckVisibility))
            .init_resource::<Previews>()
            .init_asset::<subjects::PetModels>()
            .register_asset_loader(subjects::PetModelsLoader)
            .add_systems(Startup, (setup, subjects::load))
            .add_systems(PostUpdate, seat.after(TransformSystems::Propagate));
    }
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut previews: ResMut<Previews>, screen: Res<Screen>) {
    for stage in STAGES {
        let size = image_size(stage, screen.scale);
        let image = images.add(Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None));
        let layer = stage.layer();
        let mut camera = commands.spawn((
            Name::new(format!("preview camera {stage:?}")),
            PreviewCamera, crate::rendering::highlight::HighlightCamera, crate::rendering::lighting::NoLevelLook,
            Camera3d::default(),
            // Drawn before the interface, whose window samples it this frame.
            Camera { order: -30 + stage.index() as isize, is_active: false, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
            RenderTarget::Image(image.clone().into()),
            layer.clone(), Tonemapping::None,
            // Smooth silhouettes over the window's art: the image's alpha.
            if crate::WEBGL2 && crate::depth_prepass() { Msaa::Off } else { Msaa::Sample4 },
            AmbientLight { color: Color::srgb(0.78, 0.82, 0.92), brightness: AMBIENT, ..default() },
            Projection::Perspective(PerspectiveProjection { fov: FOV, aspect_ratio: size.x as f32 / size.y as f32, near: 0.5, far: 500.0, ..default() }),
            framing((0.0, 1.8, 0.5), stage, size.x as f32 / size.y as f32),
        ));
        // The shaders' soft edges read the depth prepass (main.rs's camera).
        if crate::depth_prepass() { camera.insert(bevy::core_pipeline::prepass::DepthPrepass); }
        let camera = camera.id();
        let toward = Vec3::new(-VIEW.x, 0.0, -VIEW.y).normalize();
        commands.spawn((DirectionalLight { illuminance: KEY_LUX, shadow_maps_enabled: false, ..default() }, layer.clone(),
            Transform::default().looking_to(Quat::from_rotation_y(-0.6) * (toward - Vec3::Y * 0.9), Vec3::Y)));
        let root = commands.spawn((Name::new(format!("preview {stage:?}")), Turntable, Transform::from_translation(stage.origin()),
            Visibility::Hidden, Propagate(layer))).id();
        previews.stages.push(Shown { image, size, camera, root, subject: None, shown: Vec::new(), pending: None, settled: 0, open: false, angle: 0.0 });
    }
}

/// The stages' subjects: each open window's, chosen again when what it is
/// chosen from moves; a closed window's stage stops.
#[allow(clippy::too_many_arguments)]
pub(super) fn choose(
    mut commands: Commands,
    mut previews: ResMut<Previews>,
    mut windows: Query<(&Window2018, &Visibility, &mut WindowControls)>,
    player: Query<Ref<Character>, With<LocalPlayer>>,
    net: Option<NonSend<crate::network::net::Net>>,
    views: Res<super::controls::systems::Views>,
    tables: subjects::Tables,
    assets: Res<AssetServer>,
    mut cameras: Query<&mut Camera, With<PreviewCamera>>,
    mut work: ResMut<UiWork>,
) {
    if previews.stages.is_empty() { return; }
    // The look is read again only when the player's Character changed.
    if let Some(character) = player.iter().next().filter(|c| c.is_changed() || previews.look.is_none()) {
        let look = Look::of(&character.desc);
        if previews.look.as_ref() != Some(&look) {
            previews.look = Some(look);
            previews.looks = previews.looks.wrapping_add(1);
        }
    }
    if views.is_changed() { previews.views = previews.views.wrapping_add(1); }
    let shown = |name: &str| windows.iter().find(|(w, v, _)| w.0 == name && **v != Visibility::Hidden).map(|(_, _, c)| c);
    let inputs = Inputs {
        inventory: shown(Stage::Inventory.window()).map(|c| c.inventory_page()),
        album: shown(Stage::Album.window()).is_some(),
        looks: previews.looks,
        views: previews.views,
        effects: net.as_ref().map_or(0, |n| n.stat_revision),
        tables: tables.ready(),
    };
    if previews.inputs == Some(inputs) { return; }
    previews.inputs = Some(inputs);
    let look = previews.look.as_ref();
    let wanted = [
        look.filter(|_| matches!(inputs.inventory, Some(0 | 2)))
            .map(|look| Subject::Character { look: look.clone(), state: Some("Inventory".into()), emote: false, mount: None }),
        net.as_ref().filter(|_| inputs.inventory == Some(3)).and_then(|n| tables.shadow_soul(n)),
        views.album.preview.as_ref().filter(|_| inputs.album).and_then(|sticker| tables.sticker(&sticker.id, look)),
    ];
    // The mercenary page shows its Shadow Soul's figure, or its empty
    // placeholder without one (controls::Section::Mercenary); the album, its
    // sticker turning or its icon (controls::systems::album).
    let pages = [("inventory", "shadow_soul", u8::from(wanted[Stage::Mercenary.index()].is_some())),
        (Stage::Album.window(), ALBUM_MODEL, u8::from(wanted[Stage::Album.index()].is_some()))];
    for (window, _, mut controls) in &mut windows {
        for (name, group, page) in pages {
            if window.0 == name && controls.page(group) != page { controls.set_page(group, page); }
        }
    }
    for (stage, wanted) in STAGES.into_iter().zip(wanted) {
        let shown = &mut previews.stages[stage.index()];
        let open = wanted.is_some();
        if shown.open != open {
            shown.open = open;
            if let Ok(mut camera) = cameras.get_mut(shown.camera) { camera.is_active = open; }
            commands.entity(shown.root).insert(if open { Visibility::Inherited } else { Visibility::Hidden });
            // Parked while closed: its animation stops (drive_animations).
            for &e in shown.shown.iter().chain(shown.pending.iter().flat_map(|p| p.1.iter())) {
                if open { commands.entity(e).try_insert(Preview); } else { commands.entity(e).try_remove::<Preview>(); }
            }
        }
        let Some(subject) = wanted else { continue };
        if shown.pending.as_ref().map(|p| &p.0).or(shown.subject.as_ref()) == Some(&subject) { continue; }
        if let Some((_, old)) = shown.pending.take() { for e in old { commands.entity(e).try_despawn(); } }
        if shown.subject.as_ref() == Some(&subject) { continue; }
        let entities = spawn(&mut commands, &assets, &subject, shown.root);
        shown.pending = Some((subject, entities));
        shown.settled = 0;
        work.previews += 1;
    }
}

/// A subject, dressing hidden on its stage: its entities (a rider first).
fn spawn(commands: &mut Commands, assets: &AssetServer, subject: &Subject, root: Entity) -> Vec<Entity> {
    let model = |commands: &mut Commands, m: &Model| crate::actors::monsters::spawn_model(commands, assets, &m.graphics, &m.set, &m.anim_set, m.dye);
    let (first, state, more) = match subject {
        Subject::Character { look, state, emote, mount } => {
            let rider = crate::actors::character::spawn_character(commands, look.desc(), Transform::IDENTITY);
            if let (true, Some(state)) = (*emote, state) { commands.entity(rider).insert(Encore { state: state.clone(), rest_until: None }); }
            let mount = mount.as_ref().map(|m| {
                let e = model(commands, m);
                commands.entity(rider).insert(Seat { mount: e, anim_set: m.anim_set.clone() });
                e
            });
            (rider, state, mount)
        }
        Subject::Model { model: m, state } => (model(commands, m), state, None),
    };
    let mut anim = CharacterAnim::default();
    if let Some(state) = state { anim.state = AnimState::Named(state.clone()); }
    commands.entity(first).insert(anim);
    let entities: Vec<Entity> = std::iter::once(first).chain(more).collect();
    for &e in &entities { commands.entity(e).insert((Preview, Visibility::Hidden, ChildOf(root))); }
    entities
}

/// A dressed subject replaces the one shown, and the camera frames it.
#[allow(clippy::too_many_arguments)]
pub(super) fn reveal(
    mut commands: Commands,
    mut previews: ResMut<Previews>,
    subjects: Query<(&CharacterAnim, Option<&Character>, Has<ModelBones>)>,
    children: Query<&Children>,
    globals: Query<&GlobalTransform>,
    shapes: Shapes,
    mut cameras: Query<&mut Transform, With<PreviewCamera>>,
    mut turntables: Query<&mut Transform, (With<Turntable>, Without<PreviewCamera>)>,
    exists: Query<()>,
) {
    let mut lost = false;
    for stage in STAGES {
        let Some(shown) = previews.stages.get_mut(stage.index()) else { continue };
        // A subject despawned under the stage is built again (`choose`).
        if shown.shown.iter().any(|e| !exists.contains(*e)) {
            for e in shown.shown.drain(..) { commands.entity(e).try_despawn(); }
            shown.subject = None;
            lost = true;
        }
        let Some((_, entities)) = &shown.pending else { continue };
        let ready = entities.iter().all(|e| subjects.get(*e).is_ok_and(|(anim, character, model)|
            anim.is_ready() && character.map_or(model, |c| c.outfit_ready())));
        if !ready { continue; }
        // Measured once its animation has posed it (its skinned bounds follow).
        // FAILURE (2026-10-23): measured in its bind pose the moment it was
        //   ready, a hovering Dragonspawn (joints 0..0.8, drawn 1.7..2.6) was
        //   framed under its body and culled: an empty box.
        shown.settled = shown.settled.saturating_add(1);
        if shown.settled < SETTLE_FRAMES { continue; }
        let Some(extent) = entities.iter()
            .filter_map(|e| measure(*e, stage.origin(), &children, &globals, &shapes))
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1), a.2.max(b.2))) else { continue };
        debug!("preview {stage:?}: extent {extent:?}");
        let (subject, entities) = shown.pending.take().unwrap();
        // An emote leaps and swings far past the pose it is measured in.
        let extent = if matches!(subject, Subject::Character { emote: true, .. }) {
            (extent.0, extent.1 + (extent.1 - extent.0) * 0.12, extent.2 * 1.2)
        } else { extent };
        for old in std::mem::replace(&mut shown.shown, entities) { commands.entity(old).try_despawn(); }
        for &e in &shown.shown { commands.entity(e).insert(Visibility::Inherited); }
        shown.subject = Some(subject);
        shown.angle = 0.0;
        if let Ok(mut tf) = turntables.get_mut(shown.root) { tf.rotation = Quat::IDENTITY; }
        if let Ok(mut tf) = cameras.get_mut(shown.camera) { *tf = framing(extent, stage, shown.size.x as f32 / shown.size.y as f32); }
    }
    if lost { previews.inputs = None; }
}

/// What a subject draws: its meshes, their bounds and skins.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Shapes<'w, 's> {
    meshes: Query<'w, 's, (&'static GlobalTransform, &'static Aabb, &'static Mesh3d, Option<&'static SkinnedMesh>)>,
    assets: Res<'w, Assets<Mesh>>,
    bindposes: Res<'w, Assets<SkinnedMeshInverseBindposes>>,
}

/// A subject's extent about its stage's origin, from what it draws: (lowest,
/// highest, farthest from the vertical axis it turns about). A skinned mesh's
/// vertices are skinned here as the GPU draws them, once, when the subject
/// appears. Neither a character's mesh bounds (a fixed box around any
/// character once bound to the shared skeleton, crate::actors::character::bind)
/// nor its joints (a body variation scales the skin through helper joints,
/// crate::actors::character::VariedJoint; some helpers stand over the head)
/// say where it is drawn; bevy's skinned bounds, its fallback, grow as the
/// joints turn. A rigid mesh spans its bounds. Without meshes, its joints.
fn measure(entity: Entity, origin: Vec3, children: &Query<&Children>, globals: &Query<&GlobalTransform>, shapes: &Shapes) -> Option<(f32, f32, f32)> {
    let mut extent = None;
    for e in children.iter_descendants(entity) {
        let Ok((at, aabb, mesh, skin)) = shapes.meshes.get(e) else { continue };
        let Some(skin) = skin else {
            add_box(&mut extent, (aabb.center - aabb.half_extents).into(), (aabb.center + aabb.half_extents).into(), |p| at.transform_point(p) - origin);
            continue;
        };
        let (Some(mesh), Some(bindposes)) = (shapes.assets.get(&mesh.0), shapes.bindposes.get(&skin.inverse_bindposes)) else { continue };
        if skin_vertices(&mut extent, mesh, skin, bindposes, globals, origin) { continue; }
        if let Ok(posed) = bevy::mesh::skinning::entity_aabb_from_skinned_mesh_bounds(globals, mesh, skin, bindposes, None) {
            add_box(&mut extent, posed.min.into(), posed.max.into(), |p| p - origin);
        }
    }
    if extent.is_some() { return extent; }
    for e in children.iter_descendants(entity) {
        if let Ok(at) = globals.get(e) { add(&mut extent, at.translation() - origin); }
    }
    extent
}

fn add(extent: &mut Option<(f32, f32, f32)>, p: Vec3) {
    if !p.is_finite() || p.length() > 100.0 { return; }
    let (low, high, radius) = extent.unwrap_or((p.y, p.y, 0.0));
    *extent = Some((low.min(p.y), high.max(p.y), radius.max(Vec2::new(p.x, p.z).length())));
}

fn add_box(extent: &mut Option<(f32, f32, f32)>, min: Vec3, max: Vec3, to_stage: impl Fn(Vec3) -> Vec3) {
    for corner in 0..8 {
        add(extent, to_stage(Vec3::new([min.x, max.x][corner & 1], [min.y, max.y][corner >> 1 & 1], [min.z, max.z][corner >> 2 & 1])));
    }
}

/// A skinned mesh's vertices where they are drawn (linear blend skinning by
/// its joints' poses); false without its vertex data in the main world.
fn skin_vertices(extent: &mut Option<(f32, f32, f32)>, mesh: &Mesh, skin: &SkinnedMesh, bindposes: &SkinnedMeshInverseBindposes,
    globals: &Query<&GlobalTransform>, origin: Vec3) -> bool {
    use bevy::mesh::VertexAttributeValues as Values;
    let Some(positions) = mesh.try_attribute(Mesh::ATTRIBUTE_POSITION).ok().and_then(|p| p.as_float3()) else { return false };
    let (Ok(Values::Uint16x4(joints)), Ok(Values::Float32x4(weights))) =
        (mesh.try_attribute(Mesh::ATTRIBUTE_JOINT_INDEX), mesh.try_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)) else { return false };
    let Some(matrices) = skin.joints.iter().zip(bindposes.iter())
        .map(|(joint, bindpose)| globals.get(*joint).ok().map(|g| g.affine() * bevy::math::Affine3A::from_mat4(*bindpose)))
        .collect::<Option<Vec<_>>>() else { return false };
    for ((p, j), w) in positions.iter().zip(joints).zip(weights) {
        let v = Vec3::from(*p);
        let (mut drawn, mut total) = (Vec3::ZERO, 0.0);
        for k in 0..4 {
            let Some(m) = matrices.get(j[k] as usize).filter(|_| w[k] > 0.0) else { continue };
            drawn += m.transform_point3(v) * w[k];
            total += w[k];
        }
        if total > 0.0 { add(extent, drawn / total - origin); }
    }
    true
}

/// The camera that shows a subject of that extent whole, its lowest point
/// (its feet; a hovering pet's belly) on the stage's floor mark, turning about
/// its axis without leaving the image. A little room above its head: an
/// mount's idle moves past the pose it was measured in, and the perspective
/// raises the near side.
fn framing((low, high, radius): (f32, f32, f32), stage: Stage, aspect: f32) -> Transform {
    let (top_mark, floor_mark) = stage.marks();
    let high = (high + (high - low) * 0.05).max(low + 0.3);
    let radius = radius * 1.05 + 0.05;
    let t = (FOV * 0.5).tan();
    let tall = (high - low) / (2.0 * t * (floor_mark - top_mark));
    let wide = radius / (t * aspect * 0.92);
    let distance = tall.max(wide) + radius * 0.5;
    let eye = low + (floor_mark - 0.5) * 2.0 * distance * t;
    let origin = stage.origin();
    let from = VIEW.normalize() * distance;
    Transform::from_translation(origin + Vec3::new(from.x, eye, from.y)).looking_at(origin + Vec3::new(0.0, eye, 0.0), Vec3::Y)
}

/// DragRotate held: the stage of that window turns with the cursor.
pub(super) fn turn(
    mut pointer: ResMut<super::controls::Pointer>,
    mut previews: ResMut<Previews>,
    windows: Query<&Window2018>,
    mut turntables: Query<&mut Transform, With<Turntable>>,
) {
    let Some((window, dx)) = pointer.take_turn() else { return };
    let Ok(window) = windows.get(window) else { return };
    // The window's stage shown (the inventory's character or Shadow Soul).
    let Some(stage) = STAGES.into_iter().find(|s| s.window() == window.0 && previews.stages.get(s.index()).is_some_and(|p| p.open)) else { return };
    let Some(shown) = previews.stages.get_mut(stage.index()) else { return };
    shown.angle = (shown.angle + dx * TURN_PER_PIXEL).rem_euclid(std::f32::consts::TAU);
    if let Ok(mut tf) = turntables.get_mut(shown.root) { tf.rotation = Quat::from_rotation_y(shown.angle); }
}

/// An emote plays, rests a moment in the standing idle, plays again.
pub(super) fn encore(
    time: Res<Time>,
    mut actors: Query<(&mut CharacterAnim, &mut Encore), With<Preview>>,
    players: Query<&AnimationPlayer>,
) {
    let now = time.elapsed_secs();
    for (mut anim, mut encore) in &mut actors {
        match encore.rest_until {
            Some(until) if now >= until => {
                encore.rest_until = None;
                anim.state = AnimState::Named(encore.state.clone());
                anim.replay();
            }
            Some(_) => {}
            None => if anim.playback_finished(&players) == Some(true) {
                encore.rest_until = Some(now + ENCORE_REST);
                anim.state = AnimState::IdleHub;
            }
        }
    }
}

/// A rider sits on its mount's seat joint, the mount's anim set its own
/// (crate::actors::mounts::seat_riders, for the riders of the world).
#[allow(clippy::type_complexity)]
fn seat(
    mut riders: Query<(&GlobalTransform, &mut Character, &Seat)>,
    mounts: Query<&ModelBones>,
    globals: Query<&GlobalTransform>,
    mut transforms: Query<&mut Transform>,
) {
    for (root, mut character, seat) in &mut riders {
        if character.riding.as_ref() != Some(&seat.anim_set) { character.riding = Some(seat.anim_set.clone()); }
        let Ok(bones) = mounts.get(seat.mount) else { continue };
        let saddle = bones.0.get("mount").or_else(|| bones.0.get("Saddle")).and_then(|s| globals.get(*s).ok());
        let (Some(saddle), Some(skeleton)) = (saddle, character.skeleton_root()) else { continue };
        let on = Transform::from_matrix((root.affine().inverse() * saddle.affine()).into());
        if let Ok(mut tf) = transforms.get_mut(skeleton) {
            if *tf != on { *tf = on; }
        }
    }
}

/// The images follow the interface's resolution (a resize): remade at the new
/// size, their cameras and the windows' quads given the new one (a resized
/// image would leave the material on the old texture, crate::rendering::highlight).
pub(super) fn resize(
    screen: Res<Screen>,
    mut previews: ResMut<Previews>,
    mut images: ResMut<Assets<Image>>,
    mut targets: Query<(&mut RenderTarget, &mut Projection), With<PreviewCamera>>,
    quads: Query<(&Quad, &MeshMaterial2d<UiMaterial>)>,
    mut materials: ResMut<Assets<UiMaterial>>,
) {
    if !screen.is_changed() { return; }
    for stage in STAGES {
        let Some(shown) = previews.stages.get_mut(stage.index()) else { continue };
        let size = image_size(stage, screen.scale);
        if size == shown.size { continue; }
        shown.size = size;
        let old = std::mem::replace(&mut shown.image, images.add(Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None)));
        images.remove(&old);
        if let Ok((mut target, mut projection)) = targets.get_mut(shown.camera) {
            *target = RenderTarget::Image(shown.image.clone().into());
            if let Projection::Perspective(p) = &mut *projection { p.aspect_ratio = size.x as f32 / size.y as f32; }
        }
        for (quad, material) in &quads {
            if quad.0 != stage { continue; }
            if let Some(mut m) = materials.get_mut(&material.0) { m.atlas = Some(shown.image.clone()); }
        }
    }
}

/// A quad over a widget's rect, drawing a stage's image (spawn_windows binds
/// the texture name).
pub(super) fn quad_art(rect: [f32; 4], stage: Stage) -> Art {
    let [l, t, r, b] = rect;
    Art { tex: Some(stage.texture().to_owned()), pos: vec![l, t, r, t, r, b, l, b], uv: vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
        color: vec![1.0; 16], idx: vec![0, 1, 2, 0, 2, 3] }
}

/// The inventory with its `figur`s drawn: the character's image on the
/// equipment and trinket pages, the Shadow Soul's on the mercenary page.
pub(super) fn prepare_inventory(source: &Layout) -> Layout {
    let mut layout = source.clone();
    for w in layout.widgets.iter_mut().filter(|w| w.id.contains("/label_empty/") && w.id.ends_with("/figur")) {
        let stage = if w.id.contains("/mercenary/") { Stage::Mercenary } else { Stage::Inventory };
        w.hidden = false;
        w.art = Some(quad_art(w.rect, stage));
    }
    layout
}
