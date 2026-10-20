//! Draw bounded batches of inert mesh/material samples through the real game
//! camera while the opaque loading overlay is up. No scenes, emitters, sounds,
//! animation drivers, or gameplay sequences are instantiated. The render world
//! acknowledges uploaded meshes/materials and compiled pipelines before removal.
use std::{collections::VecDeque, sync::{Arc, atomic::{AtomicU64, Ordering}}};
use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::camera::{primitives::MeshAabb, visibility::NoFrustumCulling};
use bevy::mesh::{Indices, PrimitiveTopology, skinning::SkinnedMesh};
use bevy::pbr::{PreparedMaterial, RenderMaterialInstances, SpecializedMaterialPipelineCache};
use bevy::render::{Render, RenderApp, RenderSystems,
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    erased_render_asset::ErasedRenderAssets,
    mesh::RenderMesh, render_asset::RenderAssets,
    render_resource::{CachedPipelineState, PipelineCache}, view::RetainedViewEntity};
use bevy::world_serialization::WorldAsset;
use crate::{gameplay::skills::preload::FxPreload, screens::loading::{LoadingScreen, readiness::SceneReadiness}, world::map::CurrentMap};
use super::{merge::MapMaterial, surfaces::NebulaMaterial};

const BATCH: usize = 16;

#[derive(Clone)]
struct Sample { mesh: Handle<Mesh>, material: MapMaterial, skin: Option<SkinnedMesh>, shadows: bool }

#[derive(Resource, Default)]
pub(crate) struct Warmup {
    key: Option<(AssetId<crate::world::map::MapManifest>, Option<u8>)>,
    models: VecDeque<Handle<bevy::gltf::Gltf>>,
    pending: VecDeque<Sample>,
    active: Option<Entity>,
    complete: bool,
    token: u64,
    started: f64,
    count: usize,
    // Keep generated particle materials alive while their batches are prepared.
    particles: Vec<Handle<NebulaMaterial>>,
}

impl Warmup { pub(crate) fn ready(&self) -> bool { self.complete } }

#[derive(Resource, Clone, Default, ExtractResource)]
struct Request { token: u64, camera: Option<Entity>, meshes: Vec<(Entity, AssetId<Mesh>)> }

#[derive(Default)]
struct Receipt { complete: AtomicU64, failed: AtomicU64 }
#[derive(Resource, Clone, Default)]
struct Shared(Arc<Receipt>);

pub(crate) struct WarmupPlugin;
impl Plugin for WarmupPlugin {
    fn build(&self, app: &mut App) {
        let shared = Shared::default();
        app.init_resource::<Warmup>().init_resource::<Request>().insert_resource(shared.clone())
            .add_plugins(ExtractResourcePlugin::<Request>::default())
            .add_systems(PostUpdate, update.after(crate::screens::loading::readiness::check)
                .before(crate::screens::loading::update_progress));
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.insert_resource(shared).add_systems(Render,
                acknowledge.after(RenderSystems::Render).before(RenderSystems::Cleanup));
        }
    }
}

fn acknowledge(request: Res<Request>, shared: Res<Shared>, pipelines: Res<PipelineCache>,
    specialized: Res<SpecializedMaterialPipelineCache>, materials: Res<RenderMaterialInstances>,
    prepared: Res<ErasedRenderAssets<PreparedMaterial>>, meshes: Res<RenderAssets<RenderMesh>>,
    mut observed: Local<(u64, u8)>) {
    if request.token == 0 || request.meshes.is_empty() { return; }
    let Some(camera) = request.camera else { return };
    let view = RetainedViewEntity::new(camera.into(), None, 0);
    let Some(cache) = specialized.get(&view) else { return };
    for &(entity, mesh) in &request.meshes {
        let entity: bevy::render::sync_world::MainEntity = entity.into();
        if meshes.get(mesh).is_none() { return; }
        let Some(material) = materials.instances.get(&entity) else { return };
        if prepared.get(material.asset_id).is_none() { return; }
        let Some(pipeline) = cache.get(&entity) else { return };
        match pipelines.get_render_pipeline_state(*pipeline) {
            CachedPipelineState::Ok(_) => {},
            CachedPipelineState::Err(bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
                | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable) => return,
            CachedPipelineState::Err(error) => {
                if shared.0.failed.swap(request.token, Ordering::Release) != request.token {
                    error!("render warm-up: {error}");
                }
                return;
            }
            _ => return,
        }
    }
    // Includes prepass, shadow and post-processing variants queued by this draw.
    if pipelines.waiting_pipelines().next().is_some() { return; }
    if observed.0 != request.token { *observed = (request.token, 0); }
    observed.1 = observed.1.saturating_add(1);
    if observed.1 >= 2 { shared.0.complete.store(request.token, Ordering::Release); }
}

fn quad(color: bool) -> Mesh {
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-0.5, -0.5, 0.], [0.5, -0.5, 0.], [0.5, 0.5, 0.], [-0.5, 0.5, 0.]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 1.], [1., 1.], [1., 0.], [0., 0.]])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    if color { mesh.with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1., 1., 1., 1.]; 4]) } else { mesh }
}

fn samples(world: &mut World, model: &Handle<bevy::gltf::Gltf>, warmup: &mut Warmup) {
    let Some(scene) = world.resource::<Assets<bevy::gltf::Gltf>>().get(model).and_then(|g| g.scenes.first()).cloned() else { return };
    scene_samples(world, &scene, warmup);
}

fn scene_samples(world: &mut World, scene: &Handle<WorldAsset>, warmup: &mut Warmup) {
    let mut regular = Vec::new();
    let mut particles = Vec::new();
    {
        let scenes = world.resource::<Assets<WorldAsset>>();
        let Some(scene) = scenes.get(scene) else { return };
        #[allow(deprecated)]
        for entity in scene.world.iter_entities() {
            if let Some(emitter) = entity.get::<super::particles::Emitter>() {
                let texture = entity.get::<Children>().into_iter().flatten().find_map(|child| {
                    let mat = scene.world.get::<MeshMaterial3d<StandardMaterial>>(*child)?;
                    world.resource::<Assets<StandardMaterial>>().get(&mat.0)?.base_color_texture.clone()
                });
                particles.push((emitter.clone(), texture));
            }
            let Some(mesh) = entity.get::<Mesh3d>() else { continue };
            let decal = entity.contains::<super::materials::DecalVolume>();
            if !decal && entity.get::<Visibility>() == Some(&Visibility::Hidden) { continue; }
            let material = entity.get::<MeshMaterial3d<StandardMaterial>>().map(|m| MapMaterial::Standard(m.0.clone()))
                .or_else(|| entity.get::<MeshMaterial3d<NebulaMaterial>>().map(|m| MapMaterial::Surface(m.0.clone())))
                .or_else(|| entity.get::<MeshMaterial3d<super::refraction::RefractionMaterial>>().map(|m| MapMaterial::Refraction(m.0.clone())));
            if let Some(material) = material {
                regular.push((Sample { mesh: mesh.0.clone(), material, skin: entity.get::<SkinnedMesh>().cloned(),
                    shadows: !decal && !entity.contains::<bevy::light::NotShadowCaster>() }, decal));
            }
        }
    }
    for (mut sample, decal) in regular {
        if decal { sample.mesh = world.resource_mut::<Assets<Mesh>>().add(quad(false)); sample.skin = None; }
        warmup.pending.push_back(sample);
    }
    for (emitter, texture) in particles {
        let mat = world.resource_mut::<Assets<NebulaMaterial>>().add(super::particles::particle_material(&emitter, texture));
        warmup.particles.push(mat.clone());
        let mesh = world.resource_mut::<Assets<Mesh>>().add(quad(true));
        warmup.pending.push_back(Sample { mesh, material: MapMaterial::Surface(mat), skin: None, shadows: false });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmup_copies_render_inputs_without_instantiating_emitters_or_scenes() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<NebulaMaterial>>();
        world.init_resource::<Assets<WorldAsset>>();
        let mesh = world.resource_mut::<Assets<Mesh>>().add(quad(false));
        let mat = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
        let mut source = World::new();
        let emitter = source.spawn(super::super::particles::Emitter::default()).id();
        source.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Visibility::Hidden, ChildOf(emitter)));
        source.spawn((Mesh3d(mesh), MeshMaterial3d(mat.clone()), Visibility::Inherited));
        let scene = world.resource_mut::<Assets<WorldAsset>>().add(WorldAsset::new(source));
        let mut warmup = Warmup::default();
        let before = world.entities().len(); // Bevy also represents resources as entities.
        scene_samples(&mut world, &scene, &mut warmup);
        assert_eq!(world.entities().len(), before, "source components must never become gameplay entities");
        assert!(world.query::<&super::super::particles::Emitter>().iter(&world).next().is_none());
        assert_eq!(warmup.pending.len(), 2, "one surface and one runtime particle sample");
        assert!(warmup.pending.iter().any(|s| s.material.id() == mat.id().untyped()));
        let particle = warmup.pending.iter().find(|s| matches!(s.material, MapMaterial::Surface(_))).unwrap();
        let mesh = world.resource::<Assets<Mesh>>().get(&particle.mesh).unwrap();
        assert!(mesh.contains_attribute(Mesh::ATTRIBUTE_COLOR), "particle pipeline needs its runtime vertex layout");
        assert!(!particle.shadows);
    }

    #[test]
    fn cancelling_loading_removes_proxies_joints_and_queued_asset_handles() {
        let mut world = World::new();
        world.init_resource::<LoadingScreen>();
        world.insert_resource(Request { token: 7, ..default() });
        let root = world.spawn(Transform::default()).id();
        let joint = world.spawn((Transform::default(), ChildOf(root))).id();
        world.insert_resource(Warmup { active: Some(root), token: 7,
            models: VecDeque::from([Handle::default()]), ..default() });
        update(&mut world);
        assert!(world.get_entity(root).is_err() && world.get_entity(joint).is_err());
        let warmup = world.resource::<Warmup>();
        assert!(warmup.models.is_empty() && warmup.pending.is_empty() && warmup.active.is_none());
        assert_eq!(warmup.token, 7, "old render receipts must not acknowledge a future batch");
        assert_eq!(world.resource::<Request>().token, 0);
    }
}

fn cleanup(world: &mut World, warmup: &mut Warmup) {
    if let Some(root) = warmup.active.take() { world.despawn(root); }
    if world.resource::<Request>().token != 0 { *world.resource_mut::<Request>() = Request::default(); }
}

fn update(world: &mut World) {
    let active = world.resource::<LoadingScreen>().map_active();
    world.resource_scope(|world, mut warmup: Mut<Warmup>| {
        if !active {
            cleanup(world, &mut warmup);
            if !warmup.complete {
                let token = warmup.token;
                *warmup = Warmup { token, ..default() };
            }
            return;
        }
        let Some(map) = world.get_resource::<CurrentMap>() else { return };
        let class = world.get_resource::<FxPreload>().and_then(|p| p.class);
        let key = (map.manifest.id(), class);
        if warmup.key != Some(key) {
            cleanup(world, &mut warmup);
            let token = warmup.token;
            *warmup = Warmup { key: Some(key), token, ..default() };
        }
        if warmup.complete || !world.resource::<SceneReadiness>().ready { return; }
        let now = world.resource::<Time<Real>>().elapsed_secs_f64();
        if warmup.active.is_some() {
            let receipt = world.resource::<Shared>().clone();
            if receipt.0.failed.load(Ordering::Acquire) == warmup.token || now - warmup.started > 90. {
                world.resource_mut::<LoadingScreen>().fail("Unable to prepare graphics for this map. See the client log for details.".into());
                cleanup(world, &mut warmup);
                return;
            }
            if receipt.0.complete.load(Ordering::Acquire) != warmup.token { return; }
            cleanup(world, &mut warmup);
        } else if warmup.count == 0 && warmup.models.is_empty() && warmup.pending.is_empty() {
            warmup.models = world.get_resource::<FxPreload>().map(|p| p.models.clone().into()).unwrap_or_default();
        }
        // At most two GLB scene descriptions and 16 proxy draws per batch.
        for _ in 0..2 {
            if warmup.pending.len() >= BATCH { break; }
            let Some(model) = warmup.models.pop_front() else { break };
            samples(world, &model, &mut warmup);
        }
        if warmup.pending.is_empty() {
            if warmup.models.is_empty() {
                warmup.complete = true;
                warmup.particles.clear();
                info!("render warm-up: {} mesh/material samples complete", warmup.count);
            }
            return;
        }
        let mut cameras = world.query_filtered::<(Entity, &Camera, &GlobalTransform), (With<Camera3d>, Without<super::highlight::HighlightCamera>)>();
        let Some((camera, _, at)) = cameras.iter(world).find(|(_, c, _)| c.is_active) else { return };
        let (camera, at) = (camera, *at);
        let origin = at.translation() + at.forward() * 2.;
        let root = world.spawn((Name::new("Loading render warm-up"), Transform::default(), Visibility::Inherited)).id();
        warmup.active = Some(root);
        warmup.started = now;
        warmup.token += 1;
        let mut request = Request { token: warmup.token, camera: Some(camera), meshes: Vec::new() };
        for _ in 0..BATCH {
            let Some(sample) = warmup.pending.pop_front() else { break };
            let bounds = world.resource::<Assets<Mesh>>().get(&sample.mesh).and_then(MeshAabb::compute_aabb);
            let scale = bounds.map_or(0.1, |a| 0.1 / a.half_extents.max_element().max(0.01));
            let center = bounds.map_or(Vec3::ZERO, |a| Vec3::from(a.center));
            let transform = Transform::from_translation(origin - at.rotation() * center * scale)
                .with_rotation(at.rotation()).with_scale(Vec3::splat(scale));
            let mesh_id = sample.mesh.id();
            let entity = world.spawn((Mesh3d(sample.mesh), transform, Visibility::Inherited, NoFrustumCulling,
                Pickable::IGNORE, ChildOf(root))).id();
            match sample.material {
                MapMaterial::Standard(h) => { world.entity_mut(entity).insert(MeshMaterial3d(h)); }
                MapMaterial::Surface(h) => { world.entity_mut(entity).insert(MeshMaterial3d(h)); }
                MapMaterial::Refraction(h) => { world.entity_mut(entity).insert(MeshMaterial3d(h)); }
            }
            if !sample.shadows { world.entity_mut(entity).insert(bevy::light::NotShadowCaster); }
            if let Some(skin) = sample.skin {
                let joints = skin.joints.iter().map(|_| world.spawn((transform, GlobalTransform::from(transform), ChildOf(root))).id()).collect();
                world.entity_mut(entity).insert(SkinnedMesh { joints, inverse_bindposes: skin.inverse_bindposes });
            }
            // We're after transform propagation. Supply the current transform
            // so visibility/extraction need not wait for a later main frame.
            world.entity_mut(entity).insert(GlobalTransform::from(transform));
            request.meshes.push((entity, mesh_id));
            warmup.count += 1;
        }
        *world.resource_mut::<Request>() = request;
    });
}
