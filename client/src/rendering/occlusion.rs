//! Reveal the local character only when map geometry fully obscures the body.
//! Rays test the original meshes even while a hole is open. Only the obstructing
//! material instance gets a soft, dithered cutout; shared materials and whole
//! merged cells stay intact. The depth/normal prepass uses the same hole, while
//! shadow views retain the building's original silhouette.
//!
//! The map's visibility groups (crate::world::map::MapGroup, type 0) are the
//! original's way: a group standing between the camera and the player fades
//! out as a whole (fade_groups), the same dithered discard over all of it,
//! shadows kept. Their surfaces never get the hole, and the hole itself is now
//! only with DSOR_FLAGS=playerhole: the character shows through what is left
//! as the original's glow (crate::rendering::hidden_pixels).

use std::collections::HashSet;

use bevy::asset::uuid_handle;
use bevy::camera::visibility::VisibilitySystems;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::picking::prelude::{MeshRayCast, MeshRayCastSettings, RayCastBackfaces, RayCastVisibility};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{ShaderRef, Source};

use crate::rendering::surfaces::NebulaMaterial;

const STANDARD_SHADER: Handle<Shader> = uuid_handle!("44673d78-6357-49e4-9206-676b2e4bf1dc");
const SURFACE_SHADER: Handle<Shader> = uuid_handle!("b9a78bca-d433-49c4-8ffb-9849c297161b");
const PREPASS_SHADER: Handle<Shader> = uuid_handle!("f392c27b-985b-4487-82ef-727cad4f83f6");
const RADIUS: f32 = 1.65;
const HOLD_SECONDS: f32 = 0.12;
const FADE_IN: f32 = 0.16;
const FADE_OUT: f32 = 0.25;

#[derive(Clone, Copy, Default, ShaderType, Debug)]
pub struct CutoutParams {
    /// Camera position and opening radius at the character.
    camera: Vec4,
    /// Character's torso and fade amount.
    target: Vec4,
    /// Feet height, foreground depth margin, unused, unused.
    limits: Vec4,
}

macro_rules! extension {
    ($name:ident, $shader:ident) => {
        #[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
        pub struct $name {
            #[uniform(110)]
            params: CutoutParams,
        }
        impl MaterialExtension for $name {
            fn fragment_shader() -> ShaderRef { $shader.clone().into() }
            fn prepass_fragment_shader() -> ShaderRef { PREPASS_SHADER.clone().into() }
        }
    };
}
extension!(Cutout, STANDARD_SHADER);
extension!(SurfaceCutout, SURFACE_SHADER);

type CutoutMaterial = ExtendedMaterial<StandardMaterial, Cutout>;
type CutoutSurfaceMaterial = ExtendedMaterial<NebulaMaterial, SurfaceCutout>;

#[derive(Component)]
struct Candidate;

/// A surface of a map visibility group (crate::world::map::MapGroup type 0).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VisibilityGroup(pub u32);

/// Each visibility group's fade: amount (1 = gone) and hold time.
#[derive(Resource, Default)]
struct GroupFades(std::collections::HashMap<u32, (f32, f32)>);

enum MaterialPair {
    Standard(Handle<StandardMaterial>, Handle<CutoutMaterial>),
    Surface(Handle<NebulaMaterial>, Handle<CutoutSurfaceMaterial>),
}

#[derive(Component)]
struct Occluded {
    material: MaterialPair,
    amount: f32,
    hold: f32,
    params: CutoutParams,
}

#[derive(Resource, Default)]
struct ShadersReady(bool);

pub struct OcclusionPlugin;

impl Plugin for OcclusionPlugin {
    fn build(&self, app: &mut App) {
        bevy::shader::load_shader_library!(app, "shaders/occlusion.wgsl");
        app.add_plugins((MaterialPlugin::<CutoutMaterial>::default(), MaterialPlugin::<CutoutSurfaceMaterial>::default()))
            .init_resource::<ShadersReady>()
            .init_resource::<GroupFades>()
            .add_systems(Update, prepare_shaders)
            .add_systems(PostUpdate, (mark_candidates, reveal_player, fade_groups).chain()
                .after(TransformSystems::Propagate)
                .after(VisibilitySystems::VisibilityPropagate));
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(PostUpdate, replay::run.before(TransformSystems::Propagate));
    }
}

/// Derive cutout variants from the actual loaded Bevy shaders, preserving all
/// their alpha testing, normal mapping and prepass outputs. Never patch shared
/// shader assets. Fail closed if a future Bevy version changes these anchors.
fn cutout_shader(source: &str, anchor: &str, position: &str, path: &str) -> Option<Shader> {
    if !source.contains(anchor) { return None; }
    let call = format!("{anchor}\n    player_cutout({position}, in.position.xy);");
    Some(Shader::from_wgsl(format!("#import dsor::occlusion::player_cutout\n{}", source.replace(anchor, &call)), path.to_owned()))
}

fn prepare_shaders(mut shaders: ResMut<Assets<Shader>>, mut ready: ResMut<ShadersReady>) {
    if ready.0 { return; }
    let source = |suffix: &str| shaders.iter().find_map(|(_, s)| {
        if s.path.ends_with(suffix) { if let Source::Wgsl(text) = &s.source { return Some(text.to_string()); } }
        None
    });
    let (Some(standard), Some(prepass)) = (source("bevy_pbr/render/pbr.wgsl"), source("bevy_pbr/render/pbr_prepass.wgsl")) else { return };
    let variants = [
        (STANDARD_SHADER, cutout_shader(&standard, "var in = vertex_output;", "in.world_position.xyz", "dsor/occlusion_standard.wgsl")),
        (SURFACE_SHADER, cutout_shader(include_str!("shaders/surface.wgsl"), "var pbr_input = pbr_input_from_standard_material(moved, is_front);", "in.world_position.xyz", "dsor/occlusion_surface.wgsl")),
        (PREPASS_SHADER, cutout_shader(&prepass, "pbr_prepass_functions::prepass_alpha_discard(in);", "in.world_position.xyz", "dsor/occlusion_prepass.wgsl")),
    ];
    if variants.iter().any(|(_, shader)| shader.is_none()) {
        error_once!("Player occlusion: shader entry points changed; cutouts disabled");
        return;
    }
    for (id, shader) in variants { shaders.insert(id.id(), shader.unwrap()).unwrap(); }
    ready.0 = true;
}

fn mark_candidates(mut commands: Commands,
    meshes: Query<Entity, (Added<Mesh3d>, Without<SkinnedMesh>)>, parents: Query<&ChildOf>,
    maps: Query<(), With<crate::world::map::MapRoot>>, characters: Query<(), With<crate::actors::character::Character>>) {
    for entity in &meshes {
        let mut map = false;
        let mut actor = characters.contains(entity);
        for parent in parents.iter_ancestors(entity) {
            map |= maps.contains(parent);
            actor |= characters.contains(parent);
        }
        if map && !actor { commands.entity(entity).insert((Candidate, RayCastBackfaces)); }
    }
}

fn opaque(material: &StandardMaterial) -> bool {
    matches!(material.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_))
}

fn sight_targets(feet: Vec3, right: Vec3) -> [Vec3; 5] {
    let torso = feet + Vec3::Y;
    [torso, feet + Vec3::Y * 1.75, feet + Vec3::Y * 0.35, torso + right * 0.55, torso - right * 0.55]
}

fn blocks_player(distance: f32, length: f32, point: Vec3, feet: Vec3) -> bool {
    distance > 0.05 && distance < length - 0.15 && point.y > feet.y + 0.2
}

/// Every body sample must be blocked before opening any cutout. A low prop or
/// narrow pillar must remain intact while part of the character is visible.
/// Different meshes can jointly hide the body; retain all layers on each ray.
fn full_occlusion(raycast: &mut MeshRayCast, settings: &MeshRayCastSettings,
    feet: Vec3, origin: Vec3, right: Vec3) -> HashSet<Entity> {
    let mut hits = HashSet::new();
    for target in sight_targets(feet, right) {
        let delta = target - origin;
        let Ok(direction) = Dir3::new(delta) else { return HashSet::new() };
        let mut blocked = false;
        for (entity, hit) in raycast.cast_ray(Ray3d::new(origin, direction), settings) {
            if blocks_player(hit.distance, delta.length(), hit.point, feet) {
                blocked = true;
                hits.insert(*entity);
            }
        }
        if !blocked { return HashSet::new(); }
    }
    hits
}

fn advance_fade(amount: f32, hold: &mut f32, hit: bool, dt: f32) -> f32 {
    if hit { *hold = HOLD_SECONDS; } else { *hold = (*hold - dt).max(0.0); }
    if hit || *hold > 0.0 { (amount + dt / FADE_IN).min(1.0) }
    else { (amount - dt / FADE_OUT).max(0.0) }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn reveal_player(mut commands: Commands, ready: Res<ShadersReady>, time: Res<Time>,
    players: Query<&GlobalTransform, With<crate::network::net::LocalPlayer>>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>, Without<crate::rendering::lighting::NoLevelLook>)>,
    candidates: Query<(Entity, Option<&MeshMaterial3d<StandardMaterial>>, Option<&MeshMaterial3d<NebulaMaterial>>, Has<Occluded>), (With<Candidate>, Without<VisibilityGroup>)>,
    mut occluded: Query<(Entity, &mut Occluded), Without<VisibilityGroup>>, mut raycast: MeshRayCast,
    standard: Res<Assets<StandardMaterial>>, surfaces: Res<Assets<NebulaMaterial>>,
    mut cutouts: ResMut<Assets<CutoutMaterial>>, mut surface_cutouts: ResMut<Assets<CutoutSurfaceMaterial>>) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("occlusion::reveal_player");
    // The hole is no longer the default: the original fades its visibility groups
    // (fade_groups) and shows the character through the rest
    // (crate::rendering::hidden_pixels).
    if !ready.0 || crate::flag("noocclusion") { return; }
    // Without it nothing opens; what is open still closes and is restored.
    let hole = crate::flag("playerhole");
    let view = players.single().ok().zip(cameras.single().ok().filter(|(cam, _)| cam.is_active))
        .filter(|_| hole)
        .map(|(player, (_, camera))| (player.translation(), camera.translation(), camera.right().as_vec3()));
    let mut hits = HashSet::new();
    let mut params = None;
    if let Some((feet, origin, right)) = view {
        let filter = |entity| candidates.get(entity).is_ok_and(|(_, std, neb, active)| active
            || std.and_then(|m| standard.get(&m.0)).is_some_and(opaque)
            || neb.and_then(|m| surfaces.get(&m.0)).is_some_and(|m| opaque(&m.base)));
        let settings = MeshRayCastSettings::default().with_filter(&filter)
            .with_visibility(RayCastVisibility::Visible).never_early_exit();
        hits = full_occlusion(&mut raycast, &settings, feet, origin, right);
        params = Some(CutoutParams { camera: origin.extend(RADIUS), target: (feet + Vec3::Y).extend(0.0),
            limits: Vec4::new(feet.y + 0.2, 0.15, 0.0, 0.0) });
    }
    let dt = time.delta_secs();
    for (entity, mut state) in &mut occluded {
        let amount = state.amount;
        state.amount = advance_fade(amount, &mut state.hold, hits.contains(&entity), dt);
        if state.amount == 0.0 {
            let mut entity = commands.entity(entity);
            match &state.material {
                MaterialPair::Standard(original, _) => { entity.remove::<MeshMaterial3d<CutoutMaterial>>().insert(MeshMaterial3d(original.clone())); }
                MaterialPair::Surface(original, _) => { entity.remove::<MeshMaterial3d<CutoutSurfaceMaterial>>().insert(MeshMaterial3d(original.clone())); }
            }
            entity.remove::<Occluded>();
            continue;
        }
        if let Some(params) = params { state.params = params; }
        state.params.target.w = state.amount;
        match &state.material {
            MaterialPair::Standard(_, handle) => { if let Some(mut m) = cutouts.get_mut(handle) { m.extension.params = state.params; } }
            MaterialPair::Surface(_, handle) => { if let Some(mut m) = surface_cutouts.get_mut(handle) { m.extension.params = state.params; } }
        }
    }
    let Some(mut params) = params else { return };
    params.target.w = (dt / FADE_IN).min(1.0);
    for entity in hits {
        let Ok((_, std, neb, active)) = candidates.get(entity) else { continue };
        if active { continue; }
        let mut entity = commands.entity(entity);
        let material = if let Some((handle, base)) = std.and_then(|h| standard.get(&h.0).map(|m| (h.0.clone(), m.clone()))) {
            let cutout = cutouts.add(CutoutMaterial { base, extension: Cutout { params } });
            entity.remove::<MeshMaterial3d<StandardMaterial>>().insert(MeshMaterial3d(cutout.clone()));
            MaterialPair::Standard(handle, cutout)
        } else if let Some((handle, base)) = neb.and_then(|h| surfaces.get(&h.0).map(|m| (h.0.clone(), m.clone()))) {
            let cutout = surface_cutouts.add(CutoutSurfaceMaterial { base, extension: SurfaceCutout { params } });
            entity.remove::<MeshMaterial3d<NebulaMaterial>>().insert(MeshMaterial3d(cutout.clone()));
            MaterialPair::Surface(handle, cutout)
        } else { continue };
        entity.insert(Occluded { material, amount: params.target.w, hold: HOLD_SECONDS, params });
    }
}

/// A ray from the camera to the player's torso or head blocked by a group
/// surface hides that whole group, as the original's point-of-interest test
/// against its visibility-group collides. The group keeps fading while hit and
/// HOLD_SECONDS after, then comes back.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn fade_groups(mut commands: Commands, ready: Res<ShadersReady>, time: Res<Time>, mut fades: ResMut<GroupFades>,
    players: Query<&GlobalTransform, With<crate::network::net::LocalPlayer>>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>, Without<crate::rendering::lighting::NoLevelLook>)>,
    mut members: Query<(Entity, &VisibilityGroup, Option<&MeshMaterial3d<StandardMaterial>>, Option<&MeshMaterial3d<NebulaMaterial>>, Option<&mut Occluded>)>,
    mut raycast: MeshRayCast,
    standard: Res<Assets<StandardMaterial>>, surfaces: Res<Assets<NebulaMaterial>>,
    mut cutouts: ResMut<Assets<CutoutMaterial>>, mut surface_cutouts: ResMut<Assets<CutoutSurfaceMaterial>>) {
    if !ready.0 || crate::flag("noocclusion") { return; }
    let view = players.single().ok().zip(cameras.single().ok().filter(|(cam, _)| cam.is_active))
        .map(|(player, (_, camera))| (player.translation(), camera.translation()));
    let mut hit = HashSet::new();
    if let Some((feet, origin)) = view {
        let groups: std::collections::HashMap<Entity, u32> = members.iter().map(|(e, g, ..)| (e, g.0)).collect();
        let filter = |entity| groups.contains_key(&entity);
        let settings = MeshRayCastSettings::default().with_filter(&filter)
            .with_visibility(RayCastVisibility::Any).never_early_exit();
        for target in [feet + Vec3::Y, feet + Vec3::Y * 1.75] {
            let delta = target - origin;
            let Ok(direction) = Dir3::new(delta) else { continue };
            for (entity, h) in raycast.cast_ray(Ray3d::new(origin, direction), &settings) {
                if blocks_player(h.distance, delta.length(), h.point, feet) {
                    if let Some(g) = groups.get(entity) { hit.insert(*g); }
                }
            }
        }
    }
    let dt = time.delta_secs();
    for g in &hit { fades.0.entry(*g).or_insert((0.0, 0.0)); }
    for (g, (amount, hold)) in fades.0.iter_mut() {
        *amount = advance_fade(*amount, hold, hit.contains(g), dt);
    }
    let origin = view.map(|(_, o)| o).unwrap_or(Vec3::ZERO);
    for (entity, group, std, neb, state) in &mut members {
        let amount = fades.0.get(&group.0).map(|f| f.0).unwrap_or(0.0);
        // limits.z 1: the whole surface (shaders/occlusion.wgsl); the camera
        // position keeps shadow views out of it.
        let params = CutoutParams { camera: origin.extend(RADIUS), target: Vec3::ZERO.extend(amount), limits: Vec4::new(f32::MIN, 0.0, 1.0, 0.0) };
        match state {
            Some(state) if amount == 0.0 => {
                let mut entity = commands.entity(entity);
                match &state.material {
                    MaterialPair::Standard(original, _) => { entity.remove::<MeshMaterial3d<CutoutMaterial>>().insert(MeshMaterial3d(original.clone())); }
                    MaterialPair::Surface(original, _) => { entity.remove::<MeshMaterial3d<CutoutSurfaceMaterial>>().insert(MeshMaterial3d(original.clone())); }
                }
                entity.remove::<Occluded>();
            }
            Some(mut state) => {
                state.amount = amount;
                state.params = params;
                match &state.material {
                    MaterialPair::Standard(_, handle) => { if let Some(mut m) = cutouts.get_mut(handle) { m.extension.params = params; } }
                    MaterialPair::Surface(_, handle) => { if let Some(mut m) = surface_cutouts.get_mut(handle) { m.extension.params = params; } }
                }
            }
            None if amount > 0.0 => {
                let mut entity = commands.entity(entity);
                let material = if let Some((handle, base)) = std.and_then(|h| standard.get(&h.0).map(|m| (h.0.clone(), m.clone()))) {
                    let cutout = cutouts.add(CutoutMaterial { base, extension: Cutout { params } });
                    entity.remove::<MeshMaterial3d<StandardMaterial>>().insert(MeshMaterial3d(cutout.clone()));
                    MaterialPair::Standard(handle, cutout)
                } else if let Some((handle, base)) = neb.and_then(|h| surfaces.get(&h.0).map(|m| (h.0.clone(), m.clone()))) {
                    let cutout = surface_cutouts.add(CutoutSurfaceMaterial { base, extension: SurfaceCutout { params } });
                    entity.remove::<MeshMaterial3d<NebulaMaterial>>().insert(MeshMaterial3d(cutout.clone()));
                    MaterialPair::Surface(handle, cutout)
                } else { continue };
                entity.insert(Occluded { material, amount, hold: 0.0, params });
            }
            None => {}
        }
    }
    fades.0.retain(|_, (amount, _)| *amount > 0.0);
}

#[cfg(not(target_arch = "wasm32"))]
mod replay;

#[cfg(test)]
mod tests;
