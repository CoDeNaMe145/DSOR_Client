//! Map placement manifests (`maps/<name>.map.json`, written by DSO_Godot's
//! `export_maps.py`) and the system that spawns one as a Bevy scene.
//!
//! The manifest is loaded through the `AssetServer`, never `std::fs`, so the same
//! code reads it from disk natively and over HTTP in the browser.
//!
//! Performance contract: each distinct model in the resident area is loaded ONCE
//! (`models[i]` -> one shared glTF), and every placement that uses it
//! spawns that same handle. The glTF scene therefore shares one `Handle<Mesh>`
//! and one `Handle<StandardMaterial>` per primitive across all of its instances,
//! which is what lets Bevy batch them, and no placement triggers a load of its own.
//! Frustum culling is Bevy's default (every mesh gets an `Aabb` and is tested
//! against the camera frustum); nothing here disables it.
//!
//! Map events change the scenery (crates/level-data events.rs writes them into
//! the manifest): a base placement with an `event_rules` entry ("christmas_2012=
//! False;": the .map's event mask) goes while one of its flags runs, and an
//! `event_instances` placement (the level's `<map>_<flag>.eve`: Christmas snow,
//! Kingshill's fairground, a purified well) stands only while its flag runs. The
//! running flags are the server's (EventUpdate, crate::world::props::NetworkProps;
//! none offline). A change rebuilds only the cells whose placements changed, when
//! the server's map state moves, never a frame without one.

use bevy::asset::{io::Reader, AssetLoader, LoadContext, LoadState, RecursiveDependencyLoadState};
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::camera::primitives::MeshAabb;
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use crate::rendering::merge::MapMaterial;

/// Game map position -> Bevy world position.
///
/// The game's map frame is the one the `.map` placements are written in, which is
/// also the server's "description frame" (`dsor/mapdata.py`: positions are
/// `(x, elevation, y)`): Nebula3's right-handed, Y-up frame in metres, with the
/// two ground axes on X and Z and the elevation on Y. glTF and Bevy are both
/// right-handed and Y-up in metres, and DSO_Godot writes the `.glb` models in
/// that frame unconverted (its docs: "Nebula3 and glTF are both right-handed Y-up,
/// so nothing is converted"). The mapping is therefore the identity; keep every
/// game -> Bevy conversion going through this one function so a correction, if
/// one is ever proven, lands in one place.
///
/// The same holds for rotations (quaternions are used as-is) and scales.
#[inline]
pub fn game_to_bevy(pos: [f32; 3]) -> Vec3 {
    Vec3::new(pos[0], pos[1], pos[2])
}

/// One placement: `models[m]` at position `p`, rotation `r` (quaternion x,y,z,w),
/// scale `s`.
#[derive(Debug, Deserialize)]
pub struct Placement {
    pub m: usize,
    pub p: [f32; 3],
    pub r: [f32; 4],
    pub s: [f32; 3],
    /// Its map group (`groups[g]`), -1 for none.
    #[serde(default = "no_group")]
    pub g: i32,
}

fn no_group() -> i32 {
    -1
}

/// A map group. Type 0 is the original's visibility group: walls, roofs and
/// tower tops that fade out as a whole while they stand between the camera and
/// the player (crate::rendering::occlusion::VisibilityGroup): Kingshill's church
/// walls and palace roofs, the crypt's upper walls. Type 1 only files
/// placements for the level editor.
/// EVIDENCE: dro_client64.exe Map::ClientMapManager::SetupVisibilityGroupCollides,
///   HideGroups, HandleGroupFade (AlphaBlendFactor), "no valid point of interest
///   set at time of OnFrame()"; the .map GROP section (DSO_Godot dsomap.py) has
///   only name, type, parent and members.
#[derive(Debug, Deserialize)]
pub struct MapGroup {
    #[serde(rename = "type")]
    pub kind: u32,
}

/// When a placement stands.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Always,
    /// A base placement's event rule ("flag=False;..."): while it holds.
    Rule(String),
    /// An event layer's placement: while its flag runs.
    Event(String),
}

impl Condition {
    pub fn holds(&self, events: &crate::world::props::NetworkProps) -> bool {
        match self {
            Condition::Always => true,
            Condition::Rule(rule) => events.rule_enabled(rule),
            Condition::Event(flag) => events.flag_active(flag),
        }
    }
}

/// An event layer's placement in the manifest (`event_instances`).
#[derive(Debug, Deserialize)]
struct EventPlacement {
    #[serde(flatten)]
    placement: Placement,
    event: String,
}

/// A whole `.map.json` manifest. Fields the renderer does not use yet (nav
/// blockers) are skipped by serde.
#[derive(Asset, TypePath, Debug, Deserialize)]
pub struct MapManifest {
    pub map: String,
    /// The map's bounding box centre and half-extents, game frame.
    pub center: [f32; 3],
    pub extents: [f32; 3],
    /// Model references, relative to the asset root without extension
    /// (`t001_hub/tile_wall_corner_02` -> `t001_hub/tile_wall_corner_02.glb`).
    pub models: Vec<String>,
    /// The base placements, then (once loaded) the event layers' placements.
    pub instances: Vec<Placement>,
    #[serde(default)]
    pub groups: Vec<MapGroup>,
    #[serde(default)]
    event_instances: Vec<EventPlacement>,
    /// Base placements' rules by level-data placement id ("placement:<sha256>:<n>").
    #[serde(default)]
    event_rules: HashMap<String, String>,
    /// When each of `instances` stands.
    #[serde(skip)]
    pub conditions: Vec<Condition>,
}

impl MapManifest {
    /// Parses a manifest and joins its event scenery to the placements. The ids
    /// of the base placements are level-data's (digests of each row), so the
    /// event rules need its code: the browser build keeps the base scenery as is
    /// and draws no event layer.
    pub fn parse(bytes: &[u8]) -> serde_json::Result<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        #[cfg(not(target_arch = "wasm32"))]
        let mut manifest: MapManifest = serde_json::from_value(value.clone())?;
        #[cfg(target_arch = "wasm32")]
        let mut manifest: MapManifest = serde_json::from_slice(bytes)?;
        manifest.conditions = vec![Condition::Always; manifest.instances.len()];
        #[cfg(not(target_arch = "wasm32"))]
        {
            if !manifest.event_rules.is_empty() {
                let base = manifest.instances.len();
                for (i, id) in dsor_level_data::placement_ids(&value).into_iter().take(base).enumerate() {
                    if let Some(rule) = manifest.event_rules.get(&id) {
                        manifest.conditions[i] = Condition::Rule(rule.clone());
                    }
                }
            }
            for event in std::mem::take(&mut manifest.event_instances) {
                manifest.instances.push(event.placement);
                manifest.conditions.push(Condition::Event(event.event));
            }
        }
        manifest.event_rules = HashMap::new();
        manifest.event_instances = Vec::new();
        Ok(manifest)
    }

    /// When placement `i` stands (always for a manifest not read by [`Self::parse`]).
    pub fn condition(&self, i: usize) -> &Condition {
        self.conditions.get(i).unwrap_or(&Condition::Always)
    }

    /// The visibility group (type 0) a placement belongs to.
    pub fn visibility_group(&self, placement: &Placement) -> Option<u32> {
        let index = usize::try_from(placement.g).ok()?;
        (self.groups.get(index)?.kind == 0).then_some(index as u32)
    }
}

#[derive(Default, TypePath)]
pub struct MapManifestLoader;

impl AssetLoader for MapManifestLoader {
    type Asset = MapManifest;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _ctx: &mut LoadContext<'_>,
    ) -> Result<MapManifest, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        MapManifest::parse(&bytes).map_err(std::io::Error::other)
    }

    fn extensions(&self) -> &[&str] {
        &["map.json"]
    }
}

/// Where a model's geometry lies in its own space: every mesh's bounds through
/// its node chain, read from its glTF JSON alone (`<model>.bounds`,
/// crate::rendering::model_variant) before the model itself loads.
#[derive(Asset, TypePath, Debug, Clone, Copy, PartialEq)]
pub struct ModelBounds {
    pub min: Vec3,
    pub max: Vec3,
}

#[derive(Default, TypePath)]
pub struct ModelBoundsLoader;

impl AssetLoader for ModelBoundsLoader {
    type Asset = ModelBounds;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _ctx: &mut LoadContext<'_>,
    ) -> Result<ModelBounds, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let doc: serde_json::Value = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        document_bounds(&doc).ok_or_else(|| std::io::Error::other("no geometry"))
    }

    fn extensions(&self) -> &[&str] {
        &["bounds"]
    }
}

/// The bounds of a glTF document's meshes (their POSITION accessors' min and
/// max) through each node's chain of parents; None without geometry.
fn document_bounds(doc: &serde_json::Value) -> Option<ModelBounds> {
    let nodes = doc["nodes"].as_array()?;
    let mut parent = vec![None; nodes.len()];
    for (i, node) in nodes.iter().enumerate() {
        for child in node["children"].as_array().into_iter().flatten().filter_map(|c| c.as_u64()) {
            if let Some(p) = parent.get_mut(child as usize) {
                *p = Some(i);
            }
        }
    }
    let floats = |v: &serde_json::Value| -> Option<Vec<f32>> {
        v.as_array()?.iter().map(|x| x.as_f64().map(|x| x as f32)).collect()
    };
    let local = |node: &serde_json::Value| -> Mat4 {
        if let Some(m) = floats(&node["matrix"]).filter(|m| m.len() == 16) {
            return Mat4::from_cols_slice(&m);
        }
        let t = floats(&node["translation"]).filter(|v| v.len() == 3).map_or(Vec3::ZERO, |v| Vec3::from_slice(&v));
        let r = floats(&node["rotation"]).filter(|v| v.len() == 4).map_or(Quat::IDENTITY, |v| Quat::from_slice(&v));
        let s = floats(&node["scale"]).filter(|v| v.len() == 3).map_or(Vec3::ONE, |v| Vec3::from_slice(&v));
        Mat4::from_scale_rotation_translation(s, r, t)
    };
    let (mut min, mut max) = (Vec3::MAX, Vec3::MIN);
    for (i, node) in nodes.iter().enumerate() {
        let Some(mesh) = node["mesh"].as_u64() else { continue };
        let mut world = local(node);
        let mut at = i;
        // Bounded: a malformed file's cycle ends here.
        for _ in 0..nodes.len() {
            let Some(p) = parent[at] else { break };
            world = local(&nodes[p]) * world;
            at = p;
        }
        for primitive in doc["meshes"][mesh as usize]["primitives"].as_array().into_iter().flatten() {
            let Some(position) = primitive["attributes"]["POSITION"].as_u64() else { continue };
            let accessor = &doc["accessors"][position as usize];
            let (Some(lo), Some(hi)) = (floats(&accessor["min"]), floats(&accessor["max"])) else { continue };
            if lo.len() != 3 || hi.len() != 3 {
                continue;
            }
            let (lo, hi) = (Vec3::from_slice(&lo), Vec3::from_slice(&hi));
            for corner in 0..8 {
                let c = Vec3::select(BVec3::new(corner & 1 != 0, corner & 2 != 0, corner & 4 != 0), hi, lo);
                let w = world.transform_point3(c);
                min = min.min(w);
                max = max.max(w);
            }
        }
    }
    (min.x <= max.x).then_some(ModelBounds { min, max })
}

/// How many model files may be loading at once.
///
/// CONTRACT: keep this bounded. Bevy 0.19's glTF loader waits on a
/// `TaskPool::scope` for its images, and while it waits that thread runs other
/// queued load tasks *nested on the same stack*. Requesting all of a map's
/// models at once (433 for Kingshill) nested ~90 glTF loads deep and overflowed
/// the 2 MB IO-thread stack ("thread 'IO Task Pool (0)' has overflowed its
/// stack", gdb: 92 nested `GltfLoader::load_gltf` frames). The nesting depth is
/// bounded by the number of loads in flight, so streaming the models in keeps it
/// shallow, natively and in the single-threaded browser build alike.
pub const MAX_MODELS_IN_FLIGHT: usize = 24;

/// Which map to show, and its loading progress.
#[derive(Resource)]
pub struct CurrentMap {
    pub name: String,
    pub manifest: Handle<MapManifest>,
    /// Only models used by the resident cells. Dropping the last handle releases
    /// their meshes and textures after the cell's entities are removed.
    pub models: HashMap<usize, Handle<Gltf>>,
    /// Requested models whose file has not finished parsing yet.
    pub in_flight: Vec<usize>,
    /// Placements per model, built once the manifest arrives.
    pub by_model: Vec<Vec<usize>>,
    by_cell: HashMap<(i32, i32), Vec<usize>>,
    /// Each model's bounds, requested with the manifest (natively), until all
    /// are read: then `measured`.
    bounds: Vec<Handle<ModelBounds>>,
    /// The models' bounds by manifest index (None: unknown), once read.
    measured: Option<Vec<Option<ModelBounds>>>,
    /// How far each cell's placements reach (its square at least), once
    /// measured. A placement is filed under the cell of its origin, but its
    /// geometry can lie far beyond: g0003_tribe_01_hub's whole ground is one
    /// placement, 160 units across, whose origin's cell is at the map's north
    /// end. Measured by the square, the ground vanished from under the player
    /// in the south ("ground textures disappearing"). SEE: CurrentMap::distance.
    reach: HashMap<(i32, i32), Rect>,
    wanted_cells: HashSet<(i32, i32)>,
    wanted_models: HashSet<usize>,
    /// Geometry roots, separate from the lightweight cells used by level lights.
    contents: HashMap<(i32, i32), Entity>,
    stream_focus: Option<Vec3>,
    pub root: Option<Entity>,
    /// Culling cells (CELL units square) by grid key; every placement is a child
    /// of its cell. SEE: cull_cells.
    pub cells: std::collections::HashMap<(i32, i32), Entity>,
    /// Static surfaces being merged for cells completed in this frame.
    pub batches: crate::rendering::merge::Batches,
    /// Materials by what they look like (textures, colours, states): the many
    /// identical materials of different models share one, so their surfaces merge.
    pub canonical: std::collections::HashMap<String, Handle<StandardMaterial>>,
    /// The same for the Nebula surfaces and refractions (crate::rendering::surfaces,
    /// crate::rendering::refraction): one per look across all the map's models.
    pub canonical_surfaces: std::collections::HashMap<String, Handle<crate::rendering::surfaces::NebulaMaterial>>,
    pub canonical_refractions: std::collections::HashMap<String, Handle<crate::rendering::refraction::RefractionMaterial>>,
    /// True once the requested nearby cells have been built.
    pub spawned: bool,
    pub instances: usize,
    /// The placements standing only under some events, and whether each does now.
    conditional: Vec<(usize, bool)>,
    /// The server's map state revision the scenery follows (None offline).
    events_revision: Option<Option<u64>>,
    /// Cells taken down for an event change are being built again: streaming
    /// goes on, `spawned` (the map is ready, for everyone else) stays.
    rebuild: bool,
}

impl CurrentMap {
    pub fn new(name: String, manifest: Handle<MapManifest>) -> Self {
        Self {
            name,
            manifest,
            models: Default::default(),
            in_flight: Vec::new(),
            by_model: Vec::new(),
            by_cell: Default::default(),
            bounds: Vec::new(),
            measured: None,
            reach: Default::default(),
            wanted_cells: Default::default(),
            wanted_models: Default::default(),
            contents: Default::default(),
            stream_focus: None,
            root: None,
            cells: Default::default(),
            batches: Default::default(),
            canonical: Default::default(),
            canonical_surfaces: Default::default(),
            canonical_refractions: Default::default(),
            spawned: false,
            instances: 0,
            conditional: Vec::new(),
            events_revision: None,
            rebuild: false,
        }
    }

    /// (models fully loaded including textures, or failed; models needed nearby)
    pub fn progress(&self, asset_server: &AssetServer) -> (usize, usize) {
        let done = self
            .models
            .values()
            .filter(|h| {
                matches!(asset_server.get_load_state(*h), Some(LoadState::Failed(_))) || matches!(
                    asset_server.get_recursive_dependency_load_state(*h),
                    Some(RecursiveDependencyLoadState::Loaded | RecursiveDependencyLoadState::Failed(_))
                )
            })
            .count();
        if std::env::var("DSOR_DEBUG_LOAD").is_ok() {
            for h in self.models.values() {
                let rec = asset_server.get_recursive_dependency_load_state(h);
                if !matches!(rec, Some(RecursiveDependencyLoadState::Loaded | RecursiveDependencyLoadState::Failed(_)))
                    && !matches!(asset_server.get_load_state(h), Some(LoadState::Failed(_)))
                {
                    warn!("map load: {:?} {:?} {:?}", h.path(), asset_server.get_load_state(h), rec);
                }
            }
        }
        (done, self.wanted_models.len())
    }

    /// How far the focus is from what a cell holds: as far as its placements
    /// reach once measured, else its square. Streaming and culling both go by it.
    fn distance(&self, key: (i32, i32), focus: Vec3) -> f32 {
        let reach = self.reach.get(&key).copied().unwrap_or_else(|| cell_square(key));
        focus.xz().distance(focus.xz().clamp(reach.min, reach.max))
    }

    /// Measures again where a cell's placements reach (they changed, or the
    /// models' bounds came in).
    fn measure(&mut self, manifest: &MapManifest, key: (i32, i32)) {
        let Some(measured) = &self.measured else { return };
        let placements = self.by_cell.get(&key).map(Vec::as_slice).unwrap_or_default();
        let reach = cell_reach(key, placements.iter().filter_map(|&i| {
            let inst = manifest.instances.get(i)?;
            Some((inst, (*measured.get(inst.m)?)?))
        }));
        self.reach.insert(key, reach);
    }
}

fn cell_square((x, z): (i32, i32)) -> Rect {
    Rect::new(x as f32 * CELL, z as f32 * CELL, (x + 1) as f32 * CELL, (z + 1) as f32 * CELL)
}

/// A cell's square grown by the ground each placement's model covers.
fn cell_reach<'a>(key: (i32, i32), placements: impl Iterator<Item = (&'a Placement, ModelBounds)>) -> Rect {
    placements.fold(cell_square(key), |reach, (inst, bounds)| {
        let at = Transform {
            translation: game_to_bevy(inst.p),
            rotation: Quat::from_xyzw(inst.r[0], inst.r[1], inst.r[2], inst.r[3]).normalize(),
            scale: Vec3::from_array(inst.s),
        }
        .compute_affine();
        (0..8).fold(reach, |reach, corner| {
            let c = Vec3::select(BVec3::new(corner & 1 != 0, corner & 2 != 0, corner & 4 != 0), bounds.max, bounds.min);
            let w = at.transform_point3(c);
            reach.union_point(w.xz())
        })
    })
}

/// Root entity of the spawned map; every placement is its child.
#[derive(Component)]
pub struct MapRoot;

/// Streamed geometry and its projected decals share this lifetime.
#[derive(Component)]
pub struct MapChunk;

/// Asset path of a model reference from a manifest.
pub fn model_path(reference: &str) -> String {
    // Older manifests may keep a resource prefix such as `mdl:`.
    let rel = reference.rsplit(':').next().unwrap_or(reference);
    format!("{rel}.glb")
}

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<MapManifest>()
            .init_asset_loader::<MapManifestLoader>()
            .init_asset::<ModelBounds>()
            .init_asset_loader::<ModelBoundsLoader>()
            .add_systems(Update, (start_map, measure_cells, follow_events, stream_models, cull_cells).chain().run_if(resource_exists::<CurrentMap>));
    }
}

/// Once the manifest is in: create the map root, index placements by model and
/// frame the camera on the map.
fn start_map(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    manifests: Res<Assets<MapManifest>>,
    mut current: ResMut<CurrentMap>,
    mut cameras: Query<&mut Transform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    net: Option<NonSend<crate::network::net::Net>>,
) {
    if current.root.is_some() || current.spawned {
        return;
    }
    let Some(manifest) = manifests.get(&current.manifest) else {
        if let Some(LoadState::Failed(err)) = asset_server.get_load_state(&current.manifest) {
            error!("map manifest {} failed to load: {err}", current.name);
            current.spawned = true;
        }
        return;
    };

    let none = crate::world::props::NetworkProps::default();
    let events = net.as_deref().map_or(&none, |n| &n.props);
    let mut by_model = vec![Vec::new(); manifest.models.len()];
    for (i, inst) in manifest.instances.iter().enumerate() {
        // The level editor's aids (helpers/ruler_200x200...: a black and white
        // checkerboard under 58 release 261 levels, Kingshill among them) are not
        // drawn by the game.
        if manifest.models.get(inst.m).is_some_and(|m| m.starts_with("helpers/")) {
            continue;
        }
        let condition = manifest.condition(i);
        let stands = condition.holds(events);
        if *condition != Condition::Always {
            current.conditional.push((i, stands));
        }
        if !stands {
            continue;
        }
        if let Some(list) = by_model.get_mut(inst.m) {
            list.push(i);
            current.by_cell.entry(cell_of(game_to_bevy(inst.p))).or_default().push(i);
        }
    }
    current.by_model = by_model;
    // Natively, every model's bounds (event scenery's included), read from the
    // head of its file. The browser would download whole files for them: its
    // cells keep to their squares.
    if cfg!(not(target_arch = "wasm32")) {
        current.bounds = manifest.models.iter().map(|m| asset_server.load(format!("{}{}",
            model_path(m).trim_end_matches(".glb"), crate::rendering::model_variant::BOUNDS))).collect();
    }
    current.events_revision = Some(net.as_deref().map(|n| n.props.revision()));
    current.root = Some(
        commands
            .spawn((MapRoot, Name::new(manifest.map.clone()), Transform::default(), Visibility::default()))
            .id(),
    );

    // Frame the map: look down on its centre from the south, roughly the game's
    // own three-quarter view, far enough out to see most of it.
    let center = game_to_bevy(manifest.center);
    let reach = Vec3::from_array(manifest.extents).xz().length().clamp(20.0, 110.0);
    for mut t in &mut cameras {
        if t.translation == Vec3::ZERO {
            *t = Transform::from_translation(center + Vec3::new(0.0, reach * 0.55, reach * 0.55))
                .looking_at(center, Vec3::Y);
        }
    }
    info!(
        "map {}: {} placements of {} distinct models, {} under map events",
        manifest.map,
        manifest.instances.len(),
        manifest.models.len(),
        current.conditional.len()
    );
}

/// Once every model's bounds are read (or failed): how far each cell's
/// placements reach. Streaming waits for it (stream_models' `ready`).
fn measure_cells(
    asset_server: Res<AssetServer>,
    manifests: Res<Assets<MapManifest>>,
    bounds: Res<Assets<ModelBounds>>,
    mut current: ResMut<CurrentMap>,
) {
    if current.measured.is_some() || current.root.is_none() {
        return;
    }
    if !current.bounds.iter().all(|h| matches!(asset_server.load_state(h), LoadState::Loaded | LoadState::Failed(_))) {
        return;
    }
    let Some(manifest) = manifests.get(&current.manifest) else { return };
    let measured: Vec<_> = std::mem::take(&mut current.bounds).iter().map(|h| bounds.get(h).copied()).collect();
    let known = measured.iter().flatten().count();
    current.measured = Some(measured);
    let keys: Vec<_> = current.by_cell.keys().copied().collect();
    for &key in &keys {
        current.measure(manifest, key);
    }
    let overhang = |key: (i32, i32)| {
        let (reach, square) = (current.reach[&key], cell_square(key));
        (square.min - reach.min).max(reach.max - square.max).max_element()
    };
    let wide: Vec<_> = keys.iter().copied().filter(|&k| overhang(k) > 1.0).collect();
    info!("map {}: {known} of {} models measured; {} cells reach beyond their square, up to {:.0} units",
        current.name, manifest.models.len(), wide.len(), wide.iter().map(|&k| overhang(k)).fold(0.0, f32::max));
}

/// The server's map events moved (EventUpdate, a map change, an event's end):
/// the placements whose condition changed come or go, and only their cells are
/// built again. An integer compare on other frames.
fn follow_events(
    mut commands: Commands,
    manifests: Res<Assets<MapManifest>>,
    mut current: ResMut<CurrentMap>,
    net: Option<NonSend<crate::network::net::Net>>,
) {
    let Some(revision) = current.events_revision else { return };
    let now = net.as_deref().map(|n| n.props.revision());
    if revision == now {
        return;
    }
    current.events_revision = Some(now);
    let Some(manifest) = manifests.get(&current.manifest) else { return };
    let none = crate::world::props::NetworkProps::default();
    let events = net.as_deref().map_or(&none, |n| &n.props);
    let mut changed = HashSet::new();
    let mut conditional = std::mem::take(&mut current.conditional);
    for (i, stands) in &mut conditional {
        let now = manifest.condition(*i).holds(events);
        if now == *stands {
            continue;
        }
        *stands = now;
        let inst = &manifest.instances[*i];
        let cell = cell_of(game_to_bevy(inst.p));
        if now {
            current.by_cell.entry(cell).or_default().push(*i);
            if let Some(list) = current.by_model.get_mut(inst.m) { list.push(*i); }
        } else {
            if let Some(list) = current.by_cell.get_mut(&cell) { list.retain(|j| j != i); }
            if let Some(list) = current.by_model.get_mut(inst.m) { list.retain(|j| j != i); }
        }
        changed.insert(cell);
    }
    current.conditional = conditional;
    if changed.is_empty() {
        return;
    }
    // Emptied cells stay listed (streaming indexes them): they build empty.
    for cell in &changed {
        if let Some(content) = current.contents.remove(cell) {
            commands.entity(content).try_despawn();
        }
        current.measure(manifest, *cell);
    }
    // The models those cells now want are loaded with them.
    let wanted_models: HashSet<usize> = current.wanted_cells.iter().filter_map(|k| current.by_cell.get(k))
        .flat_map(|list| list.iter().map(|&i| manifest.instances[i].m)).collect();
    current.wanted_models = wanted_models;
    current.rebuild = true;
    info!("map {}: map events changed {} cells", current.name, changed.len());
}

/// Load models needed by nearby cells, with bounded IO and cell creation.
/// Cells prefetch beyond visibility range and unload beyond a wider radius.
///
/// CONTRACT: a static model (no animation, skin, particle emitter or decal) is
///   spawned FLAT: one entity per visible surface, its node chain folded into its
///   transform, under MapRoot. As a scene it cost every node of its hierarchy
///   too: Kingshill was 37 534 node entities for 16 311 visible surfaces, and the
///   browser (one thread) paid for each entity every frame (20-30 FPS).
///   Effect and helper surfaces (dsor_state Hidden) are not spawned at all.
/// SEE: crate::rendering::materials for the states a scene gets from its extension handler,
///   which flat surfaces replicate here.
#[allow(clippy::too_many_arguments)]
fn stream_models(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    manifests: Res<Assets<MapManifest>>,
    mut current: ResMut<CurrentMap>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    meshes: Res<Assets<GltfMesh>>,
    materials: Res<Assets<GltfMaterial>>,
    mut mesh_assets_mut: ResMut<Assets<Mesh>>,
    std_materials: Res<Assets<StandardMaterial>>,
    surface_materials: Res<Assets<crate::rendering::surfaces::NebulaMaterial>>,
    refraction_materials: Res<Assets<crate::rendering::refraction::RefractionMaterial>>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    players: Query<&GlobalTransform, crate::diagnostics::debug::FollowedPlayer>,
) {
    let Some(root) = current.root else { return };
    let Some(manifest) = manifests.get(&current.manifest) else { return };
    let Ok(cam) = cameras.single() else { return };
    let focus = camera_focus(cam, players.iter().next().map(|p| p.translation()), manifest.center[1]);
    if current.spawned && !current.rebuild && current.stream_focus.is_some_and(|p| p.xz().distance_squared(focus.xz()) < 1.0) { return; }
    current.stream_focus = Some(focus);
    let wanted: HashSet<_> = current.by_cell.keys().copied().filter(|key| {
        let radius = if current.wanted_cells.contains(key) { UNLOAD_DISTANCE } else { LOAD_DISTANCE };
        current.distance(*key, focus) <= radius
    }).collect();
    if current.spawned && !current.rebuild && wanted == current.wanted_cells { return; }
    if wanted != current.wanted_cells {
        let leaving: Vec<_> = current.contents.keys().copied().filter(|k| !wanted.contains(k)).collect();
        for key in leaving {
            if let Some(entity) = current.contents.remove(&key) { commands.entity(entity).try_despawn(); }
        }
        current.wanted_models = wanted.iter().flat_map(|k| current.by_cell[k].iter().map(|&i| manifest.instances[i].m)).collect();
        let wanted_models = current.wanted_models.clone();
        current.models.retain(|m, _| wanted_models.contains(m));
        current.in_flight.retain(|m| wanted_models.contains(m));
        // These are only a merge-time deduplication cache. Keeping them forever
        // would pin the textures of every cell the player had ever visited.
        current.canonical.clear();
        current.canonical_surfaces.clear();
        current.canonical_refractions.clear();
        current.wanted_cells = wanted;
    }
    let active: Vec<_> = current.in_flight.iter().copied().filter(|m| {
        !matches!(asset_server.get_load_state(&current.models[m]), Some(LoadState::Loaded | LoadState::Failed(_)))
    }).collect();
    current.in_flight = active;
    // Nearest cells first; only a bounded number of model loads may run at once.
    let mut cells: Vec<_> = current.wanted_cells.iter().copied().collect();
    cells.sort_by(|a, b| current.distance(*a, focus).total_cmp(&current.distance(*b, focus)));
    for key in &cells {
        let placements = current.by_cell[key].clone();
        for i in placements {
            let m = manifest.instances[i].m;
            if current.in_flight.len() >= MAX_MODELS_IN_FLIGHT { break; }
            if current.models.contains_key(&m) { continue; }
            current.models.insert(m, asset_server.load(model_path(&manifest.models[m])));
            current.in_flight.push(m);
        }
    }
    // Complete cells atomically, with a small per-frame budget. No partial
    // static batches are rebuilt when another model arrives.
    let mut built = 0;
    for key in cells {
        if current.contents.contains_key(&key) { continue; }
        let placements = current.by_cell[&key].clone();
        if !placements.iter().all(|&i| current.models.get(&manifest.instances[i].m).is_some_and(|h| {
            gltfs.get(h).is_some() || asset_server.load_state(h).is_failed()
        })) { continue; }
        let cell = *current.cells.entry(key).or_insert_with(|| {
            commands.spawn((Name::new(format!("cell {key:?}")), Transform::default(), Visibility::default(), ChildOf(root))).id()
        });
        let content = commands.spawn((MapChunk, Name::new("streamed geometry"), Transform::default(), Visibility::default(), ChildOf(cell))).id();
        current.contents.insert(key, content);
        let cell = content;
        let mut flat_cache = HashMap::new();
        for &i in &placements {
            let inst = &manifest.instances[i];
            let group = manifest.visibility_group(inst);
            let Some(gltf) = gltfs.get(&current.models[&inst.m]) else { continue };
            let flat = flat_cache.entry(inst.m).or_insert_with(|| flat_surfaces(gltf, &nodes, &meshes, &materials, &mesh_assets_mut, &asset_server));
            let mesh_assets = &*mesh_assets_mut;
            let at = Transform {
                translation: game_to_bevy(inst.p),
                rotation: Quat::from_xyzw(inst.r[0], inst.r[1], inst.r[2], inst.r[3]).normalize(),
                scale: Vec3::from_array(inst.s),
            };
            match flat {
                Some(surfaces) => {
                    let inst_affine = at.compute_affine();
                    for s in surfaces {
                        let world = inst_affine * s.local;
                        let size = s.size * at.scale.abs().max_element();
                        let casts_shadow = !(s.no_shadow || size < MIN_SHADOW_CASTER);
                        // Merged with its cell's like surfaces (crate::rendering::merge);
                        // alone when its layout cannot be.
                        let material = match &s.material {
                            MapMaterial::Standard(h) => match std_materials.get(h) {
                                Some(m) => {
                                    let sig = material_signature(m);
                                    MapMaterial::Standard(current.canonical.entry(sig).or_insert_with(|| h.clone()).clone())
                                }
                                None => s.material.clone(),
                            },
                            MapMaterial::Surface(h) => match surface_materials.get(h) {
                                Some(m) => {
                                    let e = &m.extension;
                                    let t = |h: &Option<Handle<Image>>| h.as_ref().map(|h| format!("{:?}", h.id())).unwrap_or_default();
                                    let sig = format!("{}|{:?}|{}|{}|{}", material_signature(&m.base), e.params, t(&e.cube), t(&e.layer), t(&e.mask));
                                    MapMaterial::Surface(current.canonical_surfaces.entry(sig).or_insert_with(|| h.clone()).clone())
                                }
                                None => s.material.clone(),
                            },
                            MapMaterial::Refraction(h) => match refraction_materials.get(h) {
                                Some(m) => {
                                    let dudv = m.extension.dudv.as_ref().map(|h| format!("{:?}", h.id())).unwrap_or_default();
                                    let sig = format!("{}|{:?}|{dudv}", material_signature(&m.base), m.extension.params);
                                    MapMaterial::Refraction(current.canonical_refractions.entry(sig).or_insert_with(|| h.clone()).clone())
                                }
                                None => s.material.clone(),
                            },
                        };
                        let merged = mesh_assets.get(&s.mesh).and_then(|mesh| {
                            let layout = crate::rendering::merge::layout_of(mesh)?;
                            let key = crate::rendering::merge::BatchKey { cell: key, material: material.id(), casts_shadow, decal_receiver: s.decal_receiver, group, layout };
                            current.batches.batch(key, &material).push(mesh, &world, layout).then_some(())
                        });
                        if merged.is_none() {
                            let tf = Transform::from_matrix(Mat4::from(world));
                            let mut e = commands.spawn((Mesh3d(s.mesh.clone()), tf, ChildOf(cell)));
                            s.material.insert(&mut e);
                            if !casts_shadow {
                                e.insert(bevy::light::NotShadowCaster);
                            }
                            if s.decal_receiver {
                                e.insert(crate::rendering::decals::DecalReceiver);
                            }
                            if let Some(g) = group {
                                e.insert(crate::rendering::occlusion::VisibilityGroup(g));
                            }
                        }
                    }
                }
                None => {
                    let scene = gltf.scenes.first().cloned().unwrap_or_default();
                    commands.spawn((WorldAssetRoot(scene), at, ChildOf(cell)));
                }
            }
        }
        built += 1;
        if built >= 2 { break; }
    }
    let batches = std::mem::take(&mut current.batches);
    for (key, batch) in batches.open.into_iter().chain(batches.ready) {
        let Some(&cell) = current.contents.get(&key.cell) else { continue };
        let material = batch.material.clone();
        let Some(mesh) = batch.build() else { continue };
        let mut e = commands.spawn((Mesh3d(mesh_assets_mut.add(mesh)), Transform::IDENTITY, ChildOf(cell)));
        material.insert(&mut e);
        if !key.casts_shadow { e.insert(bevy::light::NotShadowCaster); }
        if key.decal_receiver { e.insert(crate::rendering::decals::DecalReceiver); }
        if let Some(g) = key.group { e.insert(crate::rendering::occlusion::VisibilityGroup(g)); }
    }
    // Not before the cells are measured: a far placement reaching here (a
    // ground) would come after the loading screen.
    let ready = current.measured.is_some() && current.wanted_cells.iter().all(|k| current.contents.contains_key(k));
    if ready && !current.spawned {
        info!("map {}: {} of {} cells resident, {} of {} models loaded", current.name, current.contents.len(), current.by_cell.len(), current.models.len(), manifest.models.len());
    }
    // An event change's cells: the map stays ready while they come back.
    if ready {
        current.rebuild = false;
    }
    current.spawned = ready || (current.spawned && current.rebuild);
    current.instances = current.contents.keys().map(|k| current.by_cell[k].len()).sum();
}

/// Map surfaces smaller than this (largest extent, world units) cast no shadow.
/// 0 since the static surfaces are merged (crate::rendering::merge): every object casts one
/// again, as in the game, for a few hundred shadow draws.
/// Before the merge:
/// the shadow pass drew ~1 900 surfaces in Kingshill, 5 ms a frame in the
/// browser, mostly barrels, crates and clutter whose shadows hardly show.
const MIN_SHADOW_CASTER: f32 = 0.0;

/// Culling cell size, world units.
pub const CELL: f32 = 24.0;
/// What lies farther than this from the player is not drawn nor animated: map
/// cells (from the view and the shadow pass alike), actors and their animations.
/// A zone around the player, as the 2018 client loads it, never the camera's view
/// (the user's account of the original; 50 is their figure, UNVERIFIED).
/// FAILURE: tied to the camera (its distance, what is on screen), models and
///   animations went off and on as a free camera looked around ("ca fait
///   clignoter les choses quand on regarde en camera libre").
pub const CULL_DISTANCE: f32 = 50.0;
/// Prefetch outside the drawn area; release farther out to avoid boundary churn.
pub const LOAD_DISTANCE: f32 = 70.0;
pub const UNLOAD_DISTANCE: f32 = 90.0;

fn cell_of(p: Vec3) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32)
}

/// Where the camera looks: the local player when there is one, else where the
/// camera's axis meets the ground plane of the map (or a point ahead of it).
pub fn camera_focus(cam: &GlobalTransform, player: Option<Vec3>, ground: f32) -> Vec3 {
    if let Some(p) = player {
        return p;
    }
    let (o, d) = (cam.translation(), cam.forward().as_vec3());
    if d.y < -0.05 {
        let t = (ground - o.y) / d.y;
        if t > 0.0 {
            return o + d * t.min(200.0);
        }
    }
    o + d * 30.0
}

/// Culling: hide whole cells away from the camera's focus, measured to where
/// their placements reach (CurrentMap::distance), not to their square.
/// CONTRACT: hidden for every view, the shadow cascade included -- frustum culling
///   alone left the shadow pass ~12 700 surfaces in Kingshill (the camera drew
///   1 840), each prepared on the CPU every frame: 30 FPS in the browser.
pub fn cull_cells(
    current: Res<CurrentMap>,
    manifests: Res<Assets<MapManifest>>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    players: Query<&GlobalTransform, crate::diagnostics::debug::FollowedPlayer>,
    mut cells: Query<&mut Visibility>,
) {
    let Ok(cam) = cameras.single() else { return };
    let ground = manifests.get(&current.manifest).map(|m| m.center[1]).unwrap_or(0.0);
    let focus = camera_focus(cam, players.iter().next().map(|p| p.translation()), ground);
    for (&key, &e) in &current.cells {
        let near = current.distance(key, focus) <= CULL_DISTANCE;
        let Ok(mut v) = cells.get_mut(e) else { continue };
        let wanted = if near { Visibility::Inherited } else { Visibility::Hidden };
        if *v != wanted {
            *v = wanted;
        }
    }
}

/// What a material looks like, as a key: two materials with the same signature
/// draw the same.
fn material_signature(m: &StandardMaterial) -> String {
    let t = |h: &Option<Handle<Image>>| h.as_ref().map(|h| format!("{:?}", h.id())).unwrap_or_default();
    let c = m.base_color.to_linear();
    format!(
        "{}|{}|{}|{}|{}|{:?}|{}|{}|{:?}|{:.3},{:.3},{:.3},{:.3}|{:.3},{:.3},{:.3}|{:.3}|{:.3}|{:?}|{:?}",
        t(&m.base_color_texture),
        t(&m.normal_map_texture),
        t(&m.metallic_roughness_texture),
        t(&m.emissive_texture),
        t(&m.occlusion_texture),
        m.alpha_mode,
        m.unlit,
        m.double_sided,
        m.cull_mode,
        c.red, c.green, c.blue, c.alpha,
        m.emissive.red, m.emissive.green, m.emissive.blue,
        m.perceptual_roughness,
        m.metallic,
        m.uv_transform,
        m.depth_bias,
    )
}

/// One visible surface of a static model, in the model's space.
#[derive(Clone)]
pub(crate) struct FlatSurface {
    pub(crate) mesh: Handle<Mesh>,
    pub(crate) material: MapMaterial,
    pub(crate) local: bevy::math::Affine3A,
    pub(crate) no_shadow: bool,
    /// The node's pass is DecalReceiveSolid: map decals land on it
    /// (crate::rendering::decals::DecalReceiver).
    pub(crate) decal_receiver: bool,
    /// Largest extent of the surface in the model's space (its mesh bounds
    /// through `local`).
    size: f32,
}

/// The visible surfaces of a model that can be spawned flat, or None when it needs
/// its scene (animations, skins, emitters, decal boxes).
pub(crate) fn flat_surfaces(
    gltf: &Gltf,
    nodes: &Assets<GltfNode>,
    meshes: &Assets<GltfMesh>,
    materials: &Assets<GltfMaterial>,
    mesh_assets: &Assets<Mesh>,
    asset_server: &AssetServer,
) -> Option<Vec<FlatSurface>> {
    if !gltf.animations.is_empty() || !gltf.skins.is_empty() {
        return None;
    }
    let all: Vec<&GltfNode> = gltf.nodes.iter().map(|h| nodes.get(h)).collect::<Option<_>>()?;
    if all.iter().any(|n| n.extras.as_ref().is_some_and(|e| e.value.contains("dsor_emitter"))) {
        return None;
    }
    let children: std::collections::HashSet<usize> =
        all.iter().flat_map(|n| n.children.iter().filter_map(|c| nodes.get(c).map(|c| c.index))).collect();
    let mut out = Vec::new();
    let mut stack: Vec<(usize, bevy::math::Affine3A)> = all
        .iter()
        .filter(|n| !children.contains(&n.index))
        .map(|n| (n.index, bevy::math::Affine3A::IDENTITY))
        .collect();
    let by_index: std::collections::HashMap<usize, &GltfNode> = all.iter().map(|n| (n.index, *n)).collect();
    while let Some((i, parent)) = stack.pop() {
        let node = by_index.get(&i)?;
        let local = parent * node.transform.compute_affine();
        for c in &node.children {
            stack.push((nodes.get(c)?.index, local));
        }
        let Some(mesh) = node.mesh.as_ref().and_then(|h| meshes.get(h)) else { continue };
        for prim in &mesh.primitives {
            let state = prim.material_extras.as_ref().map(|e| e.value.as_str()).unwrap_or("");
            // Nebula shaders of ours come first: refraction and volume fog are
            // marked Hidden for the scene path's StandardMaterial.
            let ours = if state.contains("shd:refraction") {
                Some("refr")
            } else {
                crate::rendering::surfaces::kind_of(state).map(|_| "neb")
            };
            if ours.is_none() && state.contains("\"dsor_state\":\"Hidden\"") {
                continue;
            }
            if state.contains("\"dsor_state\":\"Decal\"") {
                return None;
            }
            let additive = state.contains("\"dsor_state\":\"Additive\"");
            let Some(gm) = prim.material.as_ref() else { return None };
            let Some(path) = gm.path() else { return None };
            let label = path.label()?.to_owned();
            // Mirroring is undone in the merged mesh itself (crate::rendering::merge reverses
            // its winding), so the plain material, never the "(inverted)" one.
            let label = label.trim_end_matches(" (inverted)").to_owned();
            // Ours (crate::rendering::materials) for glows, unlit surfaces, surfaces with a
            // static alpha factor, and lit surfaces with an emissive map, whose
            // emission is scaled to the node's intensity.
            let emissive = materials.get(gm).is_some_and(|m| m.emissive_texture.is_some());
            // The node's MatSpecularIntensity other than 1 also has a material of
            // ours (crate::rendering::materials::specular_scale).
            let specular = node.extras.as_ref()
                .and_then(|e| serde_json::from_str::<serde_json::Value>(&e.value).ok())
                .and_then(|v| v["dsor_shader"]["MatSpecularIntensity"].as_f64())
                .and_then(|v| crate::rendering::materials::specular_scale(Some(v as f32)));
            // Only when the loader made it: it pairs materials with nodes by name,
            // which some models share or name differently, and a missing label
            // never loads (deco_banner_mound_01 held Kingshill at 346/347 models).
            let specular = specular.is_some()
                && asset_server.get_handle::<StandardMaterial>(path.clone().with_label(format!("{label}/dsor"))).is_some();
            let adjusted = state.contains("\"dsor_unlit\":true") || state.contains("\"dsor_alpha\"") || specular;
            let material = match ours {
                Some("refr") => MapMaterial::Refraction(asset_server.load(path.clone().with_label(format!("{label}/refr")))),
                Some(_) => MapMaterial::Surface(asset_server.load(path.clone().with_label(format!("{label}/neb")))),
                None => {
                    let suffix = if additive || emissive || adjusted { "dsor" } else { "std" };
                    MapMaterial::Standard(asset_server.load(path.clone().with_label(format!("{label}/{suffix}"))))
                }
            };
            let opaque = materials.get(gm).is_some_and(|m| matches!(m.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)));
            let size = mesh_assets
                .get(&prim.mesh)
                .and_then(|m| m.compute_aabb())
                .map(|b| (local.matrix3 * Vec3A::from(b.half_extents * 2.0)).abs().max_element())
                .unwrap_or(f32::MAX);
            let decal_receiver = node.extras.as_ref()
                .and_then(|e| serde_json::from_str::<serde_json::Value>(&e.value).ok())
                .is_some_and(|v| v["dsor_pass"].as_str() == Some("DecalReceiveSolid"));
            out.push(FlatSurface { mesh: prim.mesh.clone(), material, local, no_shadow: additive || !opaque, decal_receiver, size });
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// crates/level-data events.rs: a base placement goes while a flag of its
    /// rule runs, an event layer's placement stands only while its flag runs.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn map_events_take_base_scenery_away_and_bring_their_own() {
        let mut value = serde_json::json!({
            "map": "test", "center": [0, 0, 0], "extents": [10, 10, 10], "models": ["props/tree", "props/snowman"],
            "instances": [
                {"m": 0, "p": [1.0, 0.0, 1.0], "r": [0.0, 0.0, 0.0, 1.0], "s": [1.0, 1.0, 1.0], "n": "", "g": -1, "col": false},
                {"m": 0, "p": [5.0, 0.0, 5.0], "r": [0.0, 0.0, 0.0, 1.0], "s": [1.0, 1.0, 1.0], "n": "", "g": -1, "col": false}
            ],
            "event_instances": [
                {"m": 1, "p": [2.0, 0.0, 2.0], "r": [0.0, 0.0, 0.0, 1.0], "s": [1.0, 1.0, 1.0], "n": "", "g": -1, "col": false, "event": "christmas_2012"}
            ],
            "event_rules": {}
        });
        let second = dsor_level_data::placement_ids(&value)[1].clone();
        value["event_rules"][second] = serde_json::json!("christmas_2012=False;");
        let manifest = MapManifest::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(manifest.instances.len(), 3, "the event layer joins the placements");
        assert_eq!(manifest.conditions, vec![Condition::Always, Condition::Rule("christmas_2012=False;".into()),
            Condition::Event("christmas_2012".into())]);
        let mut events = crate::world::props::NetworkProps::online();
        let stand = |events: &crate::world::props::NetworkProps| (0..3).map(|i| manifest.condition(i).holds(events)).collect::<Vec<_>>();
        assert_eq!(stand(&events), [true, true, false], "no event runs");
        events.receive(&dsor_proto::commands::ServerCommand::EventUpdate(dsor_proto::commands::player::EventUpdate {
            entries: vec![dsor_proto::commands::player::ScheduleEntry { key: "christmas_2012".into(), flag: true, ..default() }] }));
        assert_eq!(stand(&events), [true, false, true], "winter: the tree gives way to the snowman");
        // A manifest read without parse (tests elsewhere) stands as written.
        let plain: MapManifest = serde_json::from_value(value).unwrap();
        assert_eq!(*plain.condition(1), Condition::Always);
    }

    #[test]
    fn visibility_groups_are_the_type_0_groups() {
        // a0202_kingscrypt01: group 207 (type 0) holds the upper walls over its
        // corridors; type 1 groups only file placements.
        let manifest: MapManifest = serde_json::from_str(r#"{
            "map": "m", "center": [0, 0, 0], "extents": [1, 1, 1], "models": ["a"],
            "groups": [{"name": "x", "parent": -1, "type": 1}, {"name": "y", "parent": -1, "type": 0}],
            "instances": [
                {"m": 0, "p": [0, 0, 0], "r": [0, 0, 0, 1], "s": [1, 1, 1], "g": 1},
                {"m": 0, "p": [0, 0, 0], "r": [0, 0, 0, 1], "s": [1, 1, 1], "g": 0},
                {"m": 0, "p": [0, 0, 0], "r": [0, 0, 0, 1], "s": [1, 1, 1], "g": -1},
                {"m": 0, "p": [0, 0, 0], "r": [0, 0, 0, 1], "s": [1, 1, 1]}
            ]}"#).unwrap();
        let groups: Vec<_> = manifest.instances.iter().map(|p| manifest.visibility_group(p)).collect();
        assert_eq!(groups, [Some(1), None, None, None]);
    }

    #[test]
    fn model_bounds_follow_each_mesh_node_chain() {
        let doc = serde_json::json!({
            "nodes": [
                {"name": "model", "children": [1], "translation": [10.0, 0.0, 0.0]},
                {"name": "static_0_0", "mesh": 0, "rotation": [0.0, 1.0, 0.0, 0.0], "scale": [2.0, 2.0, 2.0]},
                {"name": "locator"}
            ],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
            "accessors": [{"min": [-1.0, 0.0, 0.0], "max": [1.0, 1.0, 3.0]}]
        });
        let b = document_bounds(&doc).unwrap();
        // Doubled, turned about Y (z to -z), moved 10 along x.
        assert!(b.min.abs_diff_eq(Vec3::new(8.0, 0.0, -6.0), 1e-5) && b.max.abs_diff_eq(Vec3::new(12.0, 2.0, 0.0), 1e-5), "{b:?}");
        assert_eq!(document_bounds(&serde_json::json!({"nodes": [{"name": "locator"}]})), None);
    }

    #[test]
    fn a_cell_reaches_as_far_as_its_placements_models() {
        // g0003_tribe_01_hub: the whole ground is one placement, its origin's
        // cell at the north end of the map.
        let manifest: MapManifest = serde_json::from_str(r#"{
            "map": "m", "center": [0, 0, 0], "extents": [1, 1, 1], "models": ["village_ground_uniqe"],
            "instances": [{"m": 0, "p": [-22, 4, -38], "r": [0, 1, 0, 0], "s": [1, 1, 1]}]}"#).unwrap();
        let mut map = CurrentMap::new("m".into(), Handle::default());
        let key = cell_of(game_to_bevy(manifest.instances[0].p));
        map.by_cell.insert(key, vec![0]);
        // Where one arrives from g0002_jungle_01_dun, in the south.
        let arrival = Vec3::new(-20.4, -4.0, 74.4);
        assert!(map.distance(key, arrival) > UNLOAD_DISTANCE, "by its square, the ground is gone there");
        map.measured = Some(vec![None]);
        map.measure(&manifest, key);
        assert!(map.distance(key, arrival) > UNLOAD_DISTANCE, "an unknown model keeps the square");
        map.measured = Some(vec![Some(ModelBounds { min: Vec3::new(-60.1, -50.7, -132.8), max: Vec3::new(86.4, 20.7, 26.3) })]);
        map.measure(&manifest, key);
        assert_eq!(map.distance(key, arrival), 0.0, "the ground lies under the player");
        assert!(map.distance(key, Vec3::new(60.0, 0.0, 0.0)) > 20.0, "and ends at its edge (x 38)");
    }
}
