//! Map exits: the glowing arrows on the ground that lead to another map.
//!
//! The 2018 client places them itself, from the level's `_Instance_InteractExit`
//! rows (tools/export_exits.py -> `maps/<map>.exits.json`): the server never
//! announces them. A click on one walks the player to it; once within its
//! PickingRange the client asks for the map with UnlockMapCommand (104), and the
//! server answers with a hand-off to the destination's map server
//! (dsor_proto Session::hand_off), which loads the new map (net.rs MapAssigned).
//! EVIDENCE: the real client's request, captured (session-maps1.jsonl frame 1074):
//!   8b 68 00 | 10 00 "a0302_wildforest" | 10 00 "a0302_wildforest" |
//!   0f 00 "exit_general_01" | 0 -- map id and exit URL are the row's ExitURL, the
//!   exit id its template Id. It came again every few ticks, between movement
//!   records, until the hand-off (frames 1084, 1090): the server ignores repeats.
//! Online the server says which exits are, and whether they lead anywhere:
//! ExitInfo (68) names every `_Instance_InteractExit` of the map by its instance
//! Guid with an active flag, those of map events not running too, inactive
//! (dso_server local_map_state.load_local_map_exits; kept in
//! crate::world::props::NetworkProps). Then an always-present exit is drawn
//! active or inactive (the model's state group; no request is sent for an
//! inactive one) and one of a map event (EventSetRow not -1, Kingshill stacks
//! several on one spot) only while announced active. Offline, or before ExitInfo, the always-present exits are
//! drawn active. UNVERIFIED: the click area (PICK_RADIUS) is not the client's
//! own picking shape.

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::world_serialization::WorldAssetRoot;
use dsor_proto::commands::{player::UnlockMap, ClientCommand};
use serde::Deserialize;

use crate::world::map::CurrentMap;
use crate::world::nav::{CurrentNav, NavMesh};
use crate::network::net::{LocalPlayer, Net, NetSystems};

#[derive(Deserialize, Debug, Clone)]
pub struct ExitRow {
    pub id: String,
    /// The instance Guid (hex), as ExitInfo names it; empty in older sidecars.
    #[serde(default)]
    pub guid: String,
    pub name: String,
    pub url: String,
    pub graphics: String,
    pub range: f32,
    pub event: i32,
    /// DirectX row-major 4x4 (the last row is the position): glam's column-major
    /// layout read as is, like the map's NPC placements.
    pub m: [f32; 16],
    /// Its location-map symbol (_Instance_InteractExit LocationMapIcon).
    #[serde(default)]
    pub icon: String,
}

#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct MapExits(pub Vec<ExitRow>);

#[derive(Default, TypePath)]
pub struct MapExitsLoader;

impl AssetLoader for MapExitsLoader {
    type Asset = MapExits;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<MapExits, Self::Error> {
        let mut b = Vec::new();
        r.read_to_end(&mut b).await?;
        serde_json::from_slice(&b).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] {
        &["exits.json"]
    }
}

/// One exit drawn on the map; an inactive one leads nowhere yet.
#[derive(Component, Clone)]
pub struct Exit {
    pub row: ExitRow,
    pub at: Vec3,
    pub active: bool,
}

/// A click on the cursor's ground point this close to an exit's centre is a click
/// on that exit.
const PICK_RADIUS: f32 = 2.0;
/// How often the request is repeated while the player stands in range.
const RESEND: f32 = 0.5;

#[derive(Resource, Default)]
struct ExitState {
    /// The map these exits belong to, and its exit file.
    map: Option<(String, Handle<MapExits>)>,
    spawned: bool,
    /// The server's map state revision last looked at, and the exits drawn
    /// then (row, active): redrawn only when that list changes.
    revision: Option<u64>,
    shown: Vec<(usize, bool)>,
    /// The exit the player clicked, until the map changes.
    chosen: Option<Exit>,
    /// Seconds until the request may go again.
    resend: f32,
}

pub struct ExitsPlugin;

impl Plugin for ExitsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<MapExits>()
            .register_asset_loader(MapExitsLoader)
            .init_resource::<ExitState>()
            .add_systems(Update, (load_exits, spawn_exits).chain().run_if(resource_exists::<CurrentMap>))
            .add_systems(Update, (click_exit, use_exit).chain().after(NetSystems).run_if(crate::screens::loading::gameplay_ready));
    }
}

fn load_exits(mut state: ResMut<ExitState>, current: Res<CurrentMap>, assets: Res<AssetServer>) {
    if state.map.as_ref().is_some_and(|(m, _)| *m == current.name) {
        return;
    }
    state.map = Some((current.name.clone(), assets.load(format!("maps/{}.exits.json", current.name))));
    state.spawned = false;
    state.revision = None;
    state.shown.clear();
    state.chosen = None;
}

/// Which exits to draw, and active or not: the server's word when it gave one
/// (a map event's exit only while active: off, its scene entity is not there),
/// else the always-present exits, active.
fn wanted(rows: &[ExitRow], props: Option<&crate::world::props::NetworkProps>) -> Vec<(usize, bool)> {
    let announced = props.filter(|p| p.exits_announced());
    rows.iter().enumerate().filter_map(|(i, row)| {
        match announced.and_then(|p| p.exit_active(&row.name, &row.guid)) {
            Some(active) => (row.event == -1 || active).then_some((i, active)),
            None => (row.event == -1).then_some((i, true)),
        }
    }).collect()
}

#[allow(clippy::too_many_arguments)]
fn spawn_exits(
    mut commands: Commands,
    mut state: ResMut<ExitState>,
    current: Res<CurrentMap>,
    exits: Res<Assets<MapExits>>,
    assets: Res<AssetServer>,
    net: Option<NonSend<Net>>,
    drawn: Query<Entity, With<Exit>>,
) {
    // Children of the map root: they go with the map.
    let Some(root) = current.root else { return };
    let Some((_, handle)) = &state.map else { return };
    let props = net.as_deref().filter(|n| !n.offline).map(|n| &n.props);
    let revision = props.map(|p| p.revision());
    if state.spawned && state.revision == revision {
        return;
    }
    let Some(list) = exits.get(handle) else {
        // A map without exits has no file.
        if assets.load_state(handle).is_failed() {
            state.spawned = true;
            state.revision = revision;
        }
        return;
    };
    state.revision = revision;
    let shown = wanted(&list.0, props);
    if state.spawned && shown == state.shown {
        return;
    }
    for entity in &drawn {
        commands.entity(entity).despawn();
    }
    for &(i, active) in &shown {
        let row = &list.0[i];
        let tf = Transform::from_matrix(Mat4::from_cols_array(&row.m));
        let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(
            crate::rendering::model_variant::prop_path(&row.graphics, if active { "active" } else { "inactive" })));
        let mut exit = commands.spawn((
            Name::new(format!("exit {} -> {}", row.name, row.url)),
            Exit { row: row.clone(), at: tf.translation, active },
            WorldAssetRoot(scene),
            tf,
            ChildOf(root),
        ));
        if active {
            // On the location map: its door, named after where it leads.
            let icon = if row.icon.is_empty() { "ui/icon_map_exit".to_owned() } else { row.icon.clone() };
            exit.insert((crate::ui::MapIcon(icon), crate::ui::MapLabel(row.url.clone())));
        }
    }
    info!("{} exits on {} ({} drawn, {} active)", list.0.len(), current.name, shown.len(), shown.iter().filter(|(_, a)| *a).count());
    state.shown = shown;
    state.spawned = true;
}

/// A left click on an exit: walk to it, and remember it for use_exit.
#[allow(clippy::too_many_arguments)]
fn click_exit(
    buttons: Res<ButtonInput<MouseButton>>,
    ui: Res<crate::ui::input::UiInput>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    // The 3D view's camera: the UI has its own (crate::ui::UiCamera), and with
    // two, single() failed and no click walked ("je ne peux pas me déplacer").
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    nav: Option<Res<CurrentNav>>,
    meshes: Res<Assets<NavMesh>>,
    exits: Query<&Exit>,
    mut players: Query<(&Transform, &mut LocalPlayer)>,
    mut state: ResMut<ExitState>,
) {
    if ui.blocks(MouseButton::Left) || !buttons.just_pressed(MouseButton::Left) || keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        return;
    }
    let Ok((tf, mut player)) = players.single_mut() else { return };
    let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), cameras.single()) else { return };
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    let Some(point) = crate::network::net::cursor_ground(window, camera, cam_tf, mesh, tf.translation) else { return };
    let flat = |a: Vec3, b: Vec3| Vec2::new(a.x - b.x, a.z - b.z).length();
    let hit = exits
        .iter()
        .filter(|e| e.active && flat(e.at, point) <= PICK_RADIUS && (e.at.y - point.y).abs() < 3.0)
        .min_by(|a, b| flat(a.at, point).total_cmp(&flat(b.at, point)));
    state.chosen = hit.cloned();
    if let Some(exit) = hit {
        let target = mesh.and_then(|m| m.nearest(exit.at, 3.0)).unwrap_or(exit.at);
        player.target = Some(target);
        state.resend = 0.0;
    }
}

/// The clicked exit, once the player is within its PickingRange: ask the server
/// for its map, again every RESEND seconds until it moves us.
fn use_exit(
    time: Res<Time>,
    net: Option<NonSendMut<Net>>,
    players: Query<&Transform, With<LocalPlayer>>,
    exits: Query<&Exit>,
    mut state: ResMut<ExitState>,
) {
    // DSOR_TEST_EXIT=<map>: use this map's exit at once, wherever the player is
    // (testing the map switch without walking there).
    let test = std::env::var("DSOR_TEST_EXIT").ok();
    if state.chosen.is_none() && players.single().is_ok() {
        if let Some(url) = &test {
            state.chosen = exits.iter().find(|e| e.row.url == *url).cloned();
        }
    }
    let Some(exit) = state.chosen.clone() else { return };
    let Some(mut net) = net else { return };
    let Ok(tf) = players.single() else { return };
    state.resend -= time.delta_secs();
    let reach = if test.is_some() { f32::INFINITY } else { exit.row.range.max(1.0) };
    if Vec2::new(tf.translation.x - exit.at.x, tf.translation.z - exit.at.z).length() > reach || state.resend > 0.0 {
        return;
    }
    state.resend = RESEND;
    info!("exit {} -> {} ({})", exit.row.id, exit.row.url, exit.row.name);
    net.send(&ClientCommand::UnlockMap(UnlockMap {
        map_id: exit.row.url.clone(),
        exit_url: exit.row.url.clone(),
        exit_id: exit.row.id.clone(),
        unknown_flag_3: false,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsor_proto::commands::{player::{Exit as ServerExit, ExitInfo}, ServerCommand};

    fn row(name: &str, guid: &str, event: i32) -> ExitRow {
        ExitRow { id: "exit_general_01".into(), guid: guid.into(), name: name.into(), url: "a0302_wildforest".into(),
            graphics: "gate".into(), range: 2.0, event, m: [0.0; 16], icon: String::new() }
    }

    /// Offline the always-present exits; online the server's list, inactive ones
    /// too, and a map event's exits only when announced.
    #[test]
    fn the_server_says_which_exits_stand_and_lead_somewhere() {
        let rows = [row("north", "AA", -1), row("pumpkin", "BB", 90), row("sewers", "CC", 61), row("east", "DD", -1)];
        assert_eq!(wanted(&rows, None), vec![(0, true), (3, true)]);
        let mut props = crate::world::props::NetworkProps::online();
        assert_eq!(wanted(&rows, Some(&props)), vec![(0, true), (3, true)], "nothing announced yet");
        props.receive(&ServerCommand::ExitInfo(ExitInfo { exits: vec![
            ServerExit { name: "north".into(), destination: String::new(), value: 0, guid: vec![0xAA] },
            ServerExit { name: "pumpkin".into(), destination: String::new(), value: 1, guid: vec![0xBB] },
            ServerExit { name: "sewers".into(), destination: String::new(), value: 0, guid: vec![0xCC] },
        ] }));
        assert_eq!(wanted(&rows, Some(&props)), vec![(0, false), (1, true), (3, true)]);
    }
}
