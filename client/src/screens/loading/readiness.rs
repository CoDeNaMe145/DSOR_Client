use super::*;
use crate::actors::character::{Character, OutfitReadiness};

/// How long entry waits for other players' appearances once everything else is
/// ready. FAILURE: one remote outfit or rig that never settled (without failing)
/// held the loading screen at 90-98% for good in a busy Kingshill.
const REMOTE_GRACE_SECS: f64 = 4.0;

#[derive(Resource, Default)]
pub(crate) struct SceneReadiness {
    pub(crate) ready: bool,
    pub(crate) fraction: f32,
    pub(crate) stage: String,
    /// Since when only other players hold entry: the map and the real time.
    remote_wait: Option<(String, f64)>,
    /// The map whose entry went ahead without some players (logged once).
    remote_released: Option<String>,
}

/// Whether the wait for other players on `map` is over, starting it at `now`.
fn remote_wait_over(wait: &mut Option<(String, f64)>, map: &str, now: f64) -> bool {
    match wait {
        Some((m, since)) if m == map => now - *since >= REMOTE_GRACE_SECS,
        _ => {
            *wait = Some((map.to_string(), now));
            false
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn check(mut loading: ResMut<LoadingScreen>, mut result: ResMut<SceneReadiness>,
    current: Option<Res<CurrentMap>>, assets: Res<AssetServer>, time: Res<Time<Real>>,
    nav: Option<Res<crate::world::nav::CurrentNav>>, level: Option<Res<crate::rendering::lighting::Level>>,
    npcs: Option<Res<crate::actors::npc::NpcLoading>>, preload: Option<Res<crate::gameplay::skills::preload::FxPreload>>,
    characters: Query<(Entity, &Character, &CharacterAnim, Has<LocalPlayer>, Option<&crate::actors::nameplate::Nameplate>)>,
    roots: Query<(Entity, &WorldAssetRoot), Without<crate::gameplay::skills::FxModel>>,
    children: Query<&Children>, parents: Query<&ChildOf>, render_inputs: OutfitReadiness,
    first_poses: Query<Entity, With<crate::rendering::anim_cull::PendingFirstPose>>) {
    if !loading.map_active() {
        result.remote_wait = None;
        result.remote_released = None;
        return;
    }
    result.ready = false;
    result.fraction = 0.;
    result.stage = "map and class assets".into();
    let Some(current) = current else { return };
    let class = characters.iter().find(|(_, _, _, local, _)| *local).or_else(|| characters.iter().next()).map(|(_, c, _, _, _)| c.desc.class);
    let preload_ready = preload.as_ref().is_none_or(|p| p.ready(class, &assets));
    if let Some(preload) = &preload {
        let (done, total) = preload.progress(&assets);
        result.fraction = if total > 0 { done as f32 / total as f32 } else { preload_ready as u8 as f32 };
    }
    if !preload_ready || !current.spawned { return; }
    result.stage = "navigation".into();
    if nav.is_none_or(|n| !asset_settled(&assets, n.0.id().untyped())) { return; }
    result.stage = "ambience".into();
    if level.is_none_or(|l| l.map != current.name || !asset_settled(&assets, l.ambience.id().untyped())) { return; }
    result.stage = "nearby NPC/decor scheduling".into();
    if npcs.is_some_and(|n| n.map != current.name || !n.ready) { return; }
    for (entity, scene) in &roots {
        if asset_failed(&assets, scene.id().untyped()) || render_inputs.failed(entity) {
            if parents.iter_ancestors(entity).any(|p| characters.get(p).is_ok_and(|(_, _, _, local, _)| local)) {
                loading.fail("Unable to load your character's appearance. Check the installed assets.".into());
                return;
            }
            continue; // Optional scenery failures already have AssetServer diagnostics.
        }
        // Labeled glTF scenes can retain a stale recursive Loading state.
        // Use the same live mesh/material/texture check as character outfits.
        if !render_inputs.ready(entity) {
            result.stage = format!("scene {:?}", scene.0.path());
            return;
        }
    }
    // Our own character must be complete. Other players are waited for too (no
    // pop-in), but only for REMOTE_GRACE_SECS: whoever is still pending then
    // finishes loading in the world.
    let mut others: Vec<(Entity, &'static str)> = Vec::new();
    for (entity, character, anim, local, _) in &characters {
        let failed = render_inputs.failed(entity) || children.iter_descendants(entity)
            .any(|e| roots.get(e).is_ok_and(|(_, scene)| asset_failed(&assets, scene.id().untyped())));
        if failed && !local { continue; }
        let wait = if !anim.is_ready() { Some("no animation player") } else { character.outfit_wait() };
        let Some(wait) = wait else { continue };
        if local {
            result.stage = format!("appearance {entity:?} (local, {wait})");
            return;
        }
        others.push((entity, wait));
    }
    // An animation player's first pose belongs to its character; other players'
    // join the same bounded wait.
    for pose in &first_poses {
        let owner = std::iter::once(pose).chain(parents.iter_ancestors(pose))
            .find_map(|e| characters.get(e).ok().map(|(c, _, _, local, _)| (c, local)));
        match owner {
            Some((c, false)) => if !others.iter().any(|(e, _)| *e == c) { others.push((c, "first animation pose")); },
            _ => {
                result.stage = "first animation poses".into();
                return;
            }
        }
    }
    if let Some(&(first, wait)) = others.first() {
        let name = |e: Entity| characters.get(e).ok().and_then(|(.., plate)| plate.map(|p| p.text.clone())).unwrap_or_default();
        if !remote_wait_over(&mut result.remote_wait, &current.name, time.elapsed_secs_f64()) {
            result.stage = format!("appearance of {} other player(s), first {first:?} {:?} ({wait})", others.len(), name(first));
            return;
        }
        if result.remote_released.as_deref() != Some(current.name.as_str()) {
            result.remote_released = Some(current.name.clone());
            let list: Vec<String> = others.iter().map(|&(e, w)| format!("{:?} ({w})", name(e))).collect();
            warn!("entering {} without waiting longer for {} other player(s): {}", current.name, others.len(), list.join(", "));
        }
    }
    result.ready = true;
    result.fraction = 1.;
    result.stage = "render warm-up".into();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{ecs::system::SystemState, world_serialization::{WorldAsset, WorldSerializationPlugin}};

    #[test]
    fn other_players_hold_entry_only_for_the_grace_period_of_each_map() {
        let mut wait = None;
        assert!(!remote_wait_over(&mut wait, "a0200_kingscity", 10.0), "the first pending player starts the wait");
        assert!(!remote_wait_over(&mut wait, "a0200_kingscity", 10.0 + REMOTE_GRACE_SECS - 0.1));
        assert!(remote_wait_over(&mut wait, "a0200_kingscity", 10.0 + REMOTE_GRACE_SECS), "a stuck outfit must not block forever");
        assert!(!remote_wait_over(&mut wait, "a0302_wildforest", 30.0), "a new map starts its own wait");
    }

    #[test]
    fn scene_readiness_uses_live_render_assets_without_waiting_for_scene_dependency_metadata() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), WorldSerializationPlugin))
            .init_asset::<Mesh>().init_asset::<Image>().init_asset::<StandardMaterial>()
            .init_asset::<crate::rendering::surfaces::NebulaMaterial>()
            .init_asset::<crate::rendering::refraction::RefractionMaterial>()
            .register_asset_loader(bevy::image::ImageLoader::new(bevy::image::CompressedImageFormats::NONE));
        let scene = app.world_mut().resource_mut::<Assets<WorldAsset>>().add(WorldAsset::new(World::new()));
        let root = app.world_mut().spawn(WorldAssetRoot(scene.clone())).id();
        app.update();
        // This scene has no AssetServer dependency state. Readiness must come
        // from its instantiated meshes, materials and textures instead.
        assert!(!asset_settled(app.world().resource::<AssetServer>(), scene.id().untyped()));
        let texture = app.world().resource::<Assets<Image>>().reserve_handle();
        let material = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color_texture: Some(texture.clone()), ..default()
        });
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        app.world_mut().spawn((Mesh3d(mesh), MeshMaterial3d(material.clone()), ChildOf(root)));
        let mut inputs = SystemState::<OutfitReadiness>::new(app.world_mut());
        assert!(!inputs.get(app.world()).unwrap().ready(root), "a pending texture still blocks the scene");
        app.world_mut().resource_mut::<Assets<Image>>().insert(texture.id(), Image::default()).unwrap();
        assert!(inputs.get(app.world()).unwrap().ready(root), "available render inputs must release the barrier");
        let missing = app.world().resource::<AssetServer>().load::<Image>("__loading_test_missing_texture__.png");
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&material).unwrap()
            .base_color_texture = Some(missing.clone());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !asset_failed(app.world().resource::<AssetServer>(), missing.id().untyped()) {
            assert!(std::time::Instant::now() < deadline, "missing image must resolve to a load failure");
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(inputs.get(app.world()).unwrap().failed(root), "a failed texture must not look like an endless pending load");
    }
}
