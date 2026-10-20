//! Initial NPC/decor scheduling readiness. Visual assets and poses are checked
//! by the shared loading barrier, including hidden actors in the preload ring.
use super::*;
use crate::screens::loading::{asset_failed, asset_settled, LoadingScreen};

#[derive(Resource, Default)]
pub(crate) struct NpcLoading { pub(crate) map: String, pub(crate) ready: bool }

#[allow(clippy::too_many_arguments)]
pub(super) fn check(loading: Option<Res<LoadingScreen>>, current: Option<Res<CurrentMap>>,
    data: Res<NpcData>, deco: Res<Deco>, offline: Option<Res<OfflineNpcs>>, assets: Res<AssetServer>,
    mut result: ResMut<NpcLoading>, files: Res<Assets<DecoFile>>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    players: Query<&GlobalTransform, crate::diagnostics::debug::FollowedPlayer>,
    manifests: Res<Assets<crate::world::map::MapManifest>>,
    visuals: Query<(&Transform, &OnlineNpcVisual)>) {
    if !loading.is_some_and(|l| l.map_active()) { return; }
    let Some(current) = current else { return };
    result.map = current.name.clone();
    result.ready = false;
    let Some((map, placements)) = &data.placements else { return };
    if *map != current.name || !asset_settled(&assets, placements.id().untyped()) { return; }
    if !asset_settled(&assets, data.templates.id().untyped())
        || !data.outfits.values().all(|h| asset_settled(&assets, h.id().untyped()))
        || !data.variations.values().all(|h| asset_settled(&assets, h.id().untyped())) { return; }
    if !asset_failed(&assets, data.templates.id().untyped())
        && offline.is_some_and(|o| o.pending || !o.queued.is_empty()) { return; }
    if std::env::var("DSOR_NO_DECO").is_err() {
        let Some((map, file)) = &deco.file else { return };
        if *map != current.name || !asset_settled(&assets, file.id().untyped()) { return; }
        if files.get(file).is_some() && deco.pending { return; }
    }
    let Ok(camera) = cameras.single() else { return };
    let ground = manifests.get(&current.manifest).map_or(0., |m| m.center[1]);
    let focus = crate::world::map::camera_focus(camera, players.iter().next().map(|p| p.translation()), ground);
    result.ready = visuals.iter().all(|(at, v)| v.resident
        || at.translation.xz().distance(focus.xz()) > crate::world::map::LOAD_DISTANCE);
}
