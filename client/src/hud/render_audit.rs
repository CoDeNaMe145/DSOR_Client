//! Opt-in scene census: `DSOR_FLAGS=renderaudit` / `?flag=renderaudit`.
//! Reports CPU visibility candidates, never claims they are GPU draw calls.
//! Native callers may save the latest census with `DSOR_RENDER_AUDIT=<path>`.

use std::collections::{BTreeMap, HashMap, HashSet};

use bevy::camera::{
    primitives::{Aabb, Frustum, Sphere},
    visibility::{CascadesVisibleEntities, CubemapVisibleEntities, NoCpuCulling, NoFrustumCulling, RenderLayers, VisibleEntities, VisibleMeshEntities},
};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use serde::Serialize;

#[derive(Default, Serialize)]
struct Counts {
    entities: usize,
    meshes: usize,
    skinned_meshes: usize,
    skin_palette_entries: usize,
    visible_skin_palette_entries: usize,
    mesh2d: usize,
    animation_targets: usize,
    active_animation_targets: usize,
    hidden_meshes: usize,
    any_view_candidates: usize,
    shadow_candidates: usize,
    main_frustum_candidates: usize,
    outside_main_but_in_other_view: usize,
    missing_bounds: usize,
    no_frustum_culling: usize,
    no_cpu_culling: usize,
    meshes_without_cpu_geometry: usize,
    triangles: usize,
    main_frustum_triangles: usize,
}

/// Mirror Bevy's CPU bounding-volume test without changing visibility.
fn in_frustum(frustum: &Frustum, bounds: Option<&Aabb>, at: &GlobalTransform) -> bool {
    let Some(bounds) = bounds else { return true };
    let transform = at.affine();
    let sphere = Sphere {
        center: transform.transform_point3a(bounds.center),
        radius: at.radius_vec3a(bounds.half_extents),
    };
    frustum.intersects_sphere(&sphere, false)
        && frustum.intersects_obb(bounds, &transform, true, false)
}

pub(super) fn snapshot(world: &mut World, mut last: Local<f64>) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    if now - *last < 5.0 || world.resource::<crate::loading::LoadingScreen>().active() {
        return;
    }
    let Some(map) = world.get_resource::<crate::map::CurrentMap>().filter(|m| m.spawned) else { return };
    let map_name = map.name.clone();
    let resident_models = map.models.len();
    let total_models = map.by_model.len();
    let cells: HashSet<_> = map.cells.values().copied().collect();
    *last = now;

    let cameras: Vec<_> = world.query::<(Entity, &Camera, &GlobalTransform, &Frustum, Option<&RenderLayers>, Option<&VisibleEntities>)>()
        .iter(world).filter(|(_, camera, ..)| camera.is_active)
        .map(|(entity, camera, at, frustum, layers, visible)| {
            (entity, camera.order, *at, *frustum, layers.cloned().unwrap_or_default(),
                visible.map(|v| v.get(std::any::TypeId::of::<Mesh3d>()).len()).unwrap_or(0))
        }).collect();
    let Some(main) = cameras.iter().find(|(entity, ..)| world.get::<Camera3d>(*entity).is_some()
        && world.get::<crate::highlight::HighlightCamera>(*entity).is_none()
        && world.get::<crate::lighting::NoLevelLook>(*entity).is_none()) else { return };
    let entities: Vec<_> = world.query::<Entity>().iter(world).collect();
    let mut shadow_candidates = HashSet::new();
    let mut directional_shadow_views = Vec::new();
    for (light, cascades) in world.query::<(Entity, &CascadesVisibleEntities)>().iter(world) {
        for (camera, views) in &cascades.entities {
            for view in views { shadow_candidates.extend(view.entities.iter().copied()); }
            directional_shadow_views.push(serde_json::json!({
                "light": format!("{light:?}"), "camera": format!("{camera:?}"),
                "camera_3d": world.get::<Camera3d>(*camera).is_some(),
                "camera_active": world.get::<Camera>(*camera).is_some_and(|c| c.is_active),
                "cascade_candidates": views.iter().map(|view| view.entities.len()).collect::<Vec<_>>(),
            }));
        }
    }
    for cube in world.query::<&CubemapVisibleEntities>().iter(world) {
        for view in cube.iter() { shadow_candidates.extend(view.entities.iter().copied()); }
    }
    for view in world.query::<&VisibleMeshEntities>().iter(world) {
        shadow_candidates.extend(view.entities.iter().copied());
    }
    let mut groups: BTreeMap<String, Counts> = BTreeMap::new();
    let mut scenes: BTreeMap<String, Counts> = BTreeMap::new();
    let mut labels = HashMap::new();
    let mut components: BTreeMap<String, usize> = BTreeMap::new();
    let asset_server = world.resource::<AssetServer>();
    let meshes = world.resource::<Assets<Mesh>>();
    let mut bypass = Vec::new();

    for &entity in &entities {
        let mut category = "other";
        let mut scene = None;
        let mut parent = Some(entity);
        while let Some(e) = parent {
            let node = world.entity(e);
            if category == "other" {
                category = if node.contains::<crate::net::LocalPlayer>() { "local_player" }
                    else if node.contains::<crate::npc::Npc>() { "npc" }
                    else if node.contains::<crate::monsters::Monster>() { "monster" }
                    else if node.contains::<crate::net::RemotePlayer>() { "remote_player" }
                    else if node.contains::<crate::character::Character>() { "character" }
                    else if node.contains::<crate::map::MapRoot>() { "map" }
                    else if node.contains::<Node>() || node.contains::<crate::ui::Window2018>() { "ui" }
                    else { "other" };
            }
            if scene.is_none() {
                if let Some(root) = node.get::<WorldAssetRoot>() {
                    scene = asset_server.get_path(root.0.id()).map(|p| p.to_string());
                }
            }
            parent = node.get::<ChildOf>().map(ChildOf::parent);
        }
        labels.insert(entity, (category, scene.unwrap_or_else(|| "<no scene hierarchy>".into())));
    }

    for entity in entities.iter().copied() {
        let node = world.entity(entity);
        for component in node.archetype().components() {
            if let Some(info) = world.components().get_info(*component) {
                *components.entry(info.name().to_string()).or_default() += 1;
            }
        }
        let mesh = node.get::<Mesh3d>();
        let bounds = node.get::<Aabb>();
        let shown = node.get::<InheritedVisibility>().is_some_and(|v| v.get());
        let any_view = node.get::<ViewVisibility>().is_some_and(|v| v.get());
        let no_frustum = node.contains::<NoFrustumCulling>();
        let no_cpu = node.contains::<NoCpuCulling>();
        let layers = node.get::<RenderLayers>().cloned().unwrap_or_default();
        let in_main = shown && main.4.intersects(&layers) && node.get::<GlobalTransform>()
            .is_some_and(|at| no_frustum || in_frustum(&main.3, bounds, at));
        // GPU-only meshes retain a handle after their CPU buffers are discarded.
        // The infallible Mesh accessors panic for these assets.
        let triangle_count = mesh.and_then(|m| meshes.get(&m.0)).and_then(|m|
            m.try_indices().map(|i| i.len())
                .or_else(|_| m.try_attribute(Mesh::ATTRIBUTE_POSITION).map(|a| a.len()))
                .ok().map(|vertices| vertices / 3));
        let triangles = triangle_count.unwrap_or(0);
        let (category, scene) = &labels[&entity];
        for counts in [groups.entry((*category).into()).or_default(), scenes.entry(scene.clone()).or_default()] {
            counts.entities += 1;
            counts.animation_targets += usize::from(node.contains::<bevy::animation::AnimationTargetId>());
            counts.active_animation_targets += usize::from(node.contains::<bevy::animation::AnimatedBy>());
            counts.mesh2d += usize::from(node.contains::<Mesh2d>());
            if let Some(skin) = node.get::<SkinnedMesh>() {
                counts.skin_palette_entries += skin.joints.len();
                if any_view { counts.visible_skin_palette_entries += skin.joints.len(); }
            }
            if mesh.is_none() { continue; }
            counts.meshes += 1;
            counts.skinned_meshes += usize::from(node.contains::<SkinnedMesh>());
            counts.hidden_meshes += usize::from(!shown);
            counts.any_view_candidates += usize::from(any_view);
            counts.shadow_candidates += usize::from(shadow_candidates.contains(&entity));
            counts.main_frustum_candidates += usize::from(in_main);
            counts.outside_main_but_in_other_view += usize::from(any_view && !in_main);
            counts.missing_bounds += usize::from(bounds.is_none());
            counts.no_frustum_culling += usize::from(no_frustum);
            counts.no_cpu_culling += usize::from(no_cpu);
            counts.meshes_without_cpu_geometry += usize::from(triangle_count.is_none());
            counts.triangles += triangles;
            counts.main_frustum_triangles += if in_main { triangles } else { 0 };
        }
        if mesh.is_some() && (bounds.is_none() || no_frustum || no_cpu) {
            bypass.push(serde_json::json!({"entity": format!("{entity:?}"), "name": node.get::<Name>().map(Name::as_str),
                "scene": scene, "triangles": triangles, "missing_bounds": bounds.is_none(), "no_frustum": no_frustum, "no_cpu": no_cpu}));
        }
    }
    let hidden_cells = cells.iter().filter(|&&e| world.get::<InheritedVisibility>(e).is_some_and(|v| !v.get())).count();
    let resident_chunks = world.query_filtered::<Entity, With<crate::map::MapChunk>>().iter(world).count();
    let npc_roots: Vec<_> = world.query::<(Entity, &crate::npc::Npc, &GlobalTransform)>()
        .iter(world).filter(|(_, npc, _)| npc.actor != 0)
        .map(|(entity, npc, at)| serde_json::json!({
            "entity": format!("{entity:?}"), "actor": npc.actor, "template": npc.template,
            "position": at.translation().to_array(),
            "character_resident": world.get::<crate::character::Character>(entity).is_some(),
            "model_bones": world.get::<crate::character::ModelBones>(entity).is_some(),
            "pending_visual": world.get::<crate::npc::NpcVisualPending>(entity).is_some(),
            "pick_boxes": world.get::<crate::picking::PickBoxes>(entity).is_some(),
            "visual_children": world.get::<Children>(entity).into_iter().flatten()
                .filter(|child| world.get::<crate::character::CharacterVisual>(**child).is_some()
                    || world.get::<WorldAssetRoot>(**child).is_some())
                .map(|child| format!("{child:?}")).collect::<Vec<_>>(),
        })).collect();
    let report = serde_json::json!({
        "map": map_name, "uptime_s": now, "entities": entities.len(),
        "directional_shadow_views": directional_shadow_views,
        "npc_roots": npc_roots,
        "note": "CPU candidates, not GPU draw counts or pixel visibility. Main frustum ignores occlusion; all views include shadows. Triangle counts omit GPU-only geometry.",
        "cells": {"total": cells.len(), "hidden": hidden_cells},
        "residency": {"chunks": resident_chunks, "models": resident_models, "map_models": total_models,
            "props_records_visuals_models": world.get_resource::<crate::props::PropScene>().map(|p| p.residency()),
            "mesh_assets": world.resource::<Assets<Mesh>>().len(), "image_assets": world.resource::<Assets<Image>>().len()},
        "cameras": cameras.iter().map(|(e, order, at, _, layers, candidates)| serde_json::json!({
            "entity": format!("{e:?}"), "order": order, "position": at.translation().to_array(),
            "forward": at.forward().to_array(), "layers": format!("{layers:?}"), "cpu_list_meshes": candidates,
            "kind": if world.get::<Camera3d>(*e).is_some() { "3d" } else { "2d" },
        })).collect::<Vec<_>>(),
        "groups": groups, "scene_hierarchies": scenes, "components": components, "culling_bypasses": bypass,
    });
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(path) = std::env::var("DSOR_RENDER_AUDIT") {
        if let Err(error) = std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap()) {
            warn!("render audit: cannot write {path}: {error}");
        }
    }
    info!("render audit: {}", serde_json::json!({"map": map_name, "groups": report["groups"], "cells": report["cells"], "cameras": report["cameras"], "directional_shadow_views": report["directional_shadow_views"]}));
}
