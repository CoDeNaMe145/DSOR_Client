//! Authored DB4 props and travel points, plus server-owned prop lifecycle.
//! The .map contains terrain placements only. GUIDs bind PropInfo to these
//! authored entities; zero-GUID props are instantiated from the template catalog.
use crate::world::map::{CurrentMap, MapManifest};
use bevy::{
    asset::{io::Reader, AssetLoader, LoadContext, LoadState},
    gltf::Gltf,
    prelude::*,
    world_serialization::{WorldAssetRoot, WorldInstance, WorldInstanceSpawner},
};
use bevy::window::PrimaryWindow;
use dsor_proto::commands::{actors::{self, PropEntry}, ClientCommand, ServerCommand};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// A prop the server announced: its id and the operation string to echo when it
/// is used (PropInteractionCommand 57; the server checks the prop is live for us).
/// SOURCE: dso-server-production services/map_events.py _handle_prop_interaction,
///   docs/PROP_AND_TRIGGER_COMMANDS.md (:305-345).
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Interactive {
    pub prop_id: u32,
    pub operation: String,
}

/// The prop we walk to, to use it on arrival: its entity, where it stands, the
/// point we walk to.
#[derive(Resource, Default)]
pub(crate) struct PropTarget {
    pending: Option<(Entity, Vec3, Vec3)>,
}

/// How close a prop is used from (the NPC talk distance, with some slack).
const USE_DISTANCE: f32 = 2.4;
/// A prop's pick cylinder: levers, chests and shrines are about this wide.
const PICK_RADIUS: f32 = 1.2;

fn cylinder_hit(ray: Ray3d, base: Vec3) -> Option<f32> {
    let o = ray.origin - base;
    let d = *ray.direction;
    let a = d.x * d.x + d.z * d.z;
    if a < 1e-6 { return None; }
    let t = (-(o.x * d.x + o.z * d.z) / a).max(0.0);
    let p = o + d * t;
    (p.x * p.x + p.z * p.z <= PICK_RADIUS * PICK_RADIUS && (-0.5..=2.5).contains(&p.y)).then_some(t)
}

/// A left click on a live prop walks to it and uses it; another click, or
/// stopping short, forgets it. Runs after the NPC and ground clicks: an NPC or a
/// monster under the cursor wins.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn interact(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    ui: Res<crate::ui::input::UiInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    props: Query<(Entity, &Interactive, &GlobalTransform, &InheritedVisibility)>,
    mut players: Query<(&Transform, &mut crate::network::net::LocalPlayer)>,
    hovered: Option<Res<crate::actors::monsters::Hovered>>,
    item: Option<Res<crate::world::drops::ItemHover>>,
    mut dialogue: ResMut<crate::gameplay::dialogue::Dialogue>,
    mut target: ResMut<PropTarget>,
    net: Option<NonSendMut<crate::network::net::Net>>,
) {
    // The map viewer (--character) has no game session: no player controller
    // walks to a prop, no server takes the interaction.
    // FAILURE (2026-10-23): required, the parameter panicked the map viewer as
    //   soon as gameplay was ready ("Non-send data not found").
    let Some(mut net) = net else { target.pending = None; return };
    let Ok((tf, mut player)) = players.single_mut() else { target.pending = None; return };
    if let Some((entity, at, approach)) = target.pending {
        let Ok((_, prop, _, _)) = props.get(entity) else { target.pending = None; return };
        if tf.translation.xz().distance(at.xz()) <= USE_DISTANCE {
            player.target = None;
            if !net.offline {
                net.send(&ClientCommand::PropInteraction(actors::PropInteraction { prop_id: prop.prop_id, interaction: prop.operation.clone() }));
            }
            info!("prop: used {} ({:?})", prop.prop_id, prop.operation);
            target.pending = None;
        } else if player.target.is_none_or(|t| t.xz().distance(approach.xz()) > 0.5) {
            target.pending = None;
        }
    }
    if ui.blocks(MouseButton::Left) || !buttons.just_pressed(MouseButton::Left) || keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { return; }
    if dialogue.target.is_some() || net.death.dead || hovered.is_some_and(|h| h.0.is_some()) || item.is_some_and(|i| i.0.is_some()) { return; }
    let Ok(window) = windows.single() else { return };
    let ray = window.cursor_position().zip(cameras.single().ok()).and_then(|(p, (c, t))| c.viewport_to_world(t, p).ok());
    let Some(ray) = ray else { return };
    let hit = props.iter().filter(|(_, _, _, visible)| visible.get())
        .filter_map(|(e, _, t, _)| cylinder_hit(ray, t.translation()).map(|d| (d, e, t.translation())))
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let Some((_, entity, at)) = hit else { return };
    let approach = crate::gameplay::dialogue::approach_point(tf.translation, at);
    player.target = (tf.translation.xz().distance(at.xz()) > USE_DISTANCE).then_some(approach);
    // Holding the click must not resume ground walking (as an NPC click).
    dialogue.held = true;
    target.pending = Some((entity, at, approach));
}

#[derive(Clone, Debug, Deserialize)]
struct Row {
    key: String,
    #[serde(default)]
    guid: String,
    graphics: String,
    #[serde(default)]
    map_icon: String,
    m: [f32; 16],
    state: String,
    event: i32,
    #[serde(default)]
    quest_controlled: bool,
    #[serde(default)]
    show_always: bool,
}

#[derive(Asset, TypePath, Debug, Default, Deserialize)]
#[serde(default)]
struct MapProps {
    objects: Vec<Row>,
    event_sets: BTreeMap<i32, String>,
    templates: HashMap<String, String>,
    map_icons: HashMap<String, String>,
}
#[derive(Default, TypePath)]
struct Loader;
impl AssetLoader for Loader {
    type Asset = MapProps;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<MapProps, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] {
        &["props.json"]
    }
}

#[derive(Clone)]
struct RemoteProp {
    entry: PropEntry,
    visible: bool,
    visual: String,
}

/// Retain announcements that arrive before the map's assets. Discard tombstones
/// prevent the authored scene from immediately recreating a removed prop.
#[derive(Default)]
pub struct NetworkProps {
    online: bool,
    entries: HashMap<u32, RemoteProp>,
    global_events: HashSet<String>,
    local_events: HashSet<String>,
    revision: u64,
}
fn visual_name(state: i8, variant: i32) -> &'static str {
    match variant {
        0 => "blue",
        1 => "red",
        2 => "yellow",
        3 => "green",
        _ => match state {
            0 | 3 => "inactive",
            1 => "active",
            2 => "cooldown",
            _ => "invalidstate",
        },
    }
}
fn guid(entry: &PropEntry) -> Option<String> {
    (entry.guid.len() == 16 && entry.guid.iter().any(|b| *b != 0))
        .then(|| entry.guid.iter().map(|b| format!("{b:02X}")).collect())
}
impl NetworkProps {
    pub fn online() -> Self {
        Self {
            online: true,
            ..default()
        }
    }
    pub fn reset_map(&mut self) {
        self.entries.clear();
        self.local_events.clear();
        self.revision += 1;
    }
    fn insert(&mut self, entry: &PropEntry) {
        if let Some(new_guid) = guid(entry) {
            self.entries
                .retain(|id, p| *id == entry.prop_id || guid(&p.entry).as_ref() != Some(&new_guid));
        }
        self.entries.insert(
            entry.prop_id,
            RemoteProp {
                visual: visual_name(entry.state, entry.team).into(),
                entry: entry.clone(),
                visible: true,
            },
        );
    }
    pub fn receive(&mut self, command: &ServerCommand) {
        match command {
            ServerCommand::PropInfo(info) => {
                for e in &info.entries {
                    self.insert(e);
                }
            }
            ServerCommand::NewProp(new) => {
                if !self.entries.get(&new.0.prop_id).is_some_and(|p| p.visible) {
                    self.insert(&new.0);
                }
            }
            ServerCommand::DiscardProp(discard) => {
                if let Some(p) = self.entries.get_mut(&discard.prop_id) {
                    p.visible = false;
                }
            }
            ServerCommand::PropUpdate(update) => {
                if let Some(p) = self.entries.get_mut(&update.prop_id) {
                    p.entry.state = update.state;
                    p.entry.team = update.variant;
                    p.entry.unknown_u32_0 = update.counter;
                    p.entry.interaction = update.operation.clone();
                    p.entry.unknown_string_0 = update.quest_id.clone();
                    p.entry.task_index = update.task_index;
                    p.entry.quest_type = update.quest_indication;
                    if !update.keep_visual_state {
                        p.visual = visual_name(update.state, update.variant).into();
                    }
                }
            }
            ServerCommand::EventUpdate(update) => {
                for e in &update.entries {
                    let set = if e.map_local {
                        &mut self.local_events
                    } else {
                        &mut self.global_events
                    };
                    let flag = e.key.trim().to_lowercase();
                    if e.flag {
                        set.insert(flag);
                    } else {
                        set.remove(&flag);
                    }
                }
            }
            _ => return,
        }
        self.revision += 1;
    }
    /// Bumped by every prop or event change.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn event_enabled(&self, event: i32, sets: &BTreeMap<i32, String>) -> bool {
        if event == -1 {
            return true;
        }
        let Some(rule) = sets.get(&event) else {
            return false;
        };
        self.rule_enabled(rule)
    }

    /// An EventSet's `EventFlags` ("flag=True;other=False;") against the running
    /// events: on when one positive flag runs (or none is asked) and no negative one does.
    pub(crate) fn rule_enabled(&self, rule: &str) -> bool {
        let mut positive = false;
        let mut matched = false;
        for declaration in rule.split(';') {
            let Some((key, value)) = declaration.split_once('=') else {
                continue;
            };
            let key = key.trim().to_lowercase();
            if key.is_empty() {
                continue;
            }
            let active = self.global_events.contains(&key) || self.local_events.contains(&key);
            if value.trim().eq_ignore_ascii_case("true") {
                positive = true;
                matched |= active;
            } else if active {
                return false;
            }
        }
        !positive || matched
    }
}

#[derive(Clone, PartialEq)]
struct Desired {
    key: String,
    graphics: String,
    transform: Transform,
    state: String,
    map_icon: String,
    /// The server's prop id and interaction, when it announced this one.
    interactive: Option<(u32, String)>,
}
fn desired(map: &MapProps, templates: &MapProps, net: &NetworkProps) -> Vec<Desired> {
    let by_guid: HashMap<_, _> = net
        .entries
        .values()
        .filter_map(|p| guid(&p.entry).map(|g| (g, p)))
        .collect();
    let mut result = Vec::new();
    for row in &map.objects {
        if !net.event_enabled(row.event, &map.event_sets) {
            continue;
        }
        let remote = by_guid.get(&row.guid.to_uppercase());
        if remote.is_some_and(|p| !p.visible)
            || (net.online && remote.is_none() && row.quest_controlled && !row.show_always)
        {
            continue;
        }
        let mut transform = Transform::from_matrix(Mat4::from_cols_array(&row.m));
        if let Some(p) = remote {
            // PropInfo is absolute; preserve authored scale. It is not a wire
            // movement coordinate and must not have the map origin added to it.
            transform.translation = Vec3::from_array(p.entry.position);
            transform.rotation = Quat::from_rotation_y(p.entry.rotation);
        }
        result.push(Desired {
            key: row.key.clone(),
            graphics: row.graphics.clone(),
            transform,
            state: remote.map_or_else(|| row.state.to_lowercase(), |p| p.visual.clone()),
            map_icon: row.map_icon.clone(),
            interactive: remote.map(|p| (p.entry.prop_id, p.entry.interaction.clone())),
        });
    }
    for p in net
        .entries
        .values()
        .filter(|p| p.visible && p.entry.guid.iter().all(|b| *b == 0))
    {
        let Some(graphics) = templates.templates.get(&p.entry.template) else {
            continue;
        };
        result.push(Desired {
            key: format!("network:{}", p.entry.prop_id),
            graphics: graphics.clone(),
            transform: Transform::from_translation(Vec3::from_array(p.entry.position))
                .with_rotation(Quat::from_rotation_y(p.entry.rotation)),
            state: p.visual.clone(),
            map_icon: templates.map_icons.get(&p.entry.template).cloned().unwrap_or_default(),
            interactive: Some((p.entry.prop_id, p.entry.interaction.clone())),
        });
    }
    result.retain(|p| {
        p.transform.translation.is_finite()
            && p.transform.rotation.is_finite()
            && p.transform.scale.is_finite()
    });
    result
}

struct Placed {
    entity: Entity,
    desired: Desired,
    visual: Option<Entity>,
}
#[derive(Resource, Default)]
pub struct PropScene {
    map: Option<(String, AssetId<MapManifest>)>,
    data: Handle<MapProps>,
    catalog: Handle<MapProps>,
    models: HashMap<String, Handle<Gltf>>,
    placed: HashMap<String, Placed>,
    revision: Option<(u64, bool)>,
    complete: bool,
    waiting_states: bool,
    focus: Option<Vec3>,
}
impl PropScene {
    pub(crate) fn residency(&self) -> (usize, usize, usize) {
        (self.placed.len(), self.placed.values().filter(|p| p.visual.is_some()).count(), self.models.len())
    }

    pub fn ready(&self, current: &CurrentMap, assets: &AssetServer) -> bool {
        self.map
            .as_ref()
            .is_some_and(|(name, id)| name == &current.name && *id == current.manifest.id())
            && self.complete
            && !self.waiting_states
            && self.models.values().all(|h| {
                assets.is_loaded_with_dependencies(h.id())
                    || assets.load_state(h).is_failed()
                    || matches!(
                        assets.get_recursive_dependency_load_state(h.id()),
                        Some(bevy::asset::RecursiveDependencyLoadState::Failed(_))
                    )
            })
    }
}
#[derive(Component)]
struct PendingState {
    state: String,
    model: Handle<Gltf>,
    animate: bool,
}

pub struct PropsPlugin;
impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<MapProps>()
            .init_asset_loader::<Loader>()
            .init_resource::<PropScene>()
            .init_resource::<PropTarget>()
            .add_systems(
                Update,
                (load, reconcile, apply_states)
                    .chain()
                    .after(crate::network::net::NetSystems)
                    .run_if(resource_exists::<CurrentMap>),
            )
            .add_systems(Update, interact.after(crate::network::net::NetSystems).run_if(crate::screens::loading::gameplay_ready));
    }
}
fn load(
    mut commands: Commands,
    mut scene: ResMut<PropScene>,
    current: Res<CurrentMap>,
    assets: Res<AssetServer>,
) {
    let key = (current.name.clone(), current.manifest.id());
    if scene.map.as_ref() == Some(&key) {
        return;
    }
    // Usually already removed with MapRoot. try_despawn also handles manual map reloads.
    for p in scene.placed.values() {
        commands.entity(p.entity).try_despawn();
    }
    *scene = PropScene {
        map: Some(key),
        data: assets.load(format!("maps/{}.props.json", current.name)),
        catalog: assets.load("props/catalog.props.json"),
        ..default()
    };
}
#[allow(clippy::too_many_arguments)]
fn reconcile(
    mut commands: Commands,
    mut scene: ResMut<PropScene>,
    current: Res<CurrentMap>,
    assets: Res<AssetServer>,
    maps: Res<Assets<MapProps>>,
    gltfs: Res<Assets<Gltf>>,
    net: Option<NonSend<crate::network::net::Net>>,
    manifests: Res<Assets<MapManifest>>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    players: Query<&GlobalTransform, crate::diagnostics::debug::FollowedPlayer>,
) {
    let Some(root) = current.root else { return };
    let Some(map) = maps.get(&scene.data) else {
        if assets.load_state(&scene.data).is_failed() && !scene.complete {
            warn!("{}: prop placements unavailable; run dsor-levels props", current.name);
            scene.complete = true;
        }
        return;
    };
    let Ok(camera) = cameras.single() else { return };
    let ground = manifests.get(&current.manifest).map_or(0.0, |m| m.center[1]);
    let focus = crate::world::map::camera_focus(camera, players.iter().next().map(|p| p.translation()), ground);
    let empty = NetworkProps::default();
    let net = net.as_ref().map(|n| &n.props).unwrap_or(&empty);
    let catalog = maps.get(&scene.catalog);
    let revision = (net.revision, catalog.is_some());
    if scene.complete && scene.revision == Some(revision)
        && scene.focus.is_some_and(|p| p.xz().distance_squared(focus.xz()) < 1.0) { return; }
    scene.focus = Some(focus);
    let mut list = desired(map, catalog.unwrap_or(map), net);
    let keys: HashSet<_> = list.iter().map(|p| p.key.as_str()).collect();
    scene.placed.retain(|key, placed| {
        if keys.contains(key.as_str()) { true }
        else { commands.entity(placed.entity).try_despawn(); false }
    });
    list.sort_by(|a, b| a.transform.translation.xz().distance_squared(focus.xz())
        .total_cmp(&b.transform.translation.xz().distance_squared(focus.xz())));
    let mut needed_models = HashSet::new();
    let mut in_flight = scene.models.values().filter(|h|
        !matches!(assets.load_state(*h), LoadState::Loaded | LoadState::Failed(_))).count();
    let budget = crate::world::map::MAX_MODELS_IN_FLIGHT.saturating_sub(current.in_flight.len());
    let mut complete = true;
    let mut added = 0;
    for wanted in list {
        // Keep authored/network identity and map markers even while geometry is
        // unloaded. A distant server update is applied to this record immediately.
        if let Some(placed) = scene.placed.get_mut(&wanted.key) {
            if placed.desired != wanted {
                commands.entity(placed.entity).insert((wanted.transform, crate::ui::MapIcon(wanted.map_icon.clone())));
                if placed.desired.interactive != wanted.interactive {
                    match &wanted.interactive {
                        Some((prop_id, operation)) => { commands.entity(placed.entity).insert(Interactive { prop_id: *prop_id, operation: operation.clone() }); }
                        None => { commands.entity(placed.entity).remove::<Interactive>(); }
                    }
                }
                if placed.desired.graphics != wanted.graphics || placed.desired.state != wanted.state {
                    if let Some(visual) = placed.visual.take() { commands.entity(visual).try_despawn(); }
                }
                placed.desired = wanted.clone();
            }
        } else {
            let entity = commands.spawn((Name::new(format!("prop {}", wanted.key)),
                crate::ui::MapIcon(wanted.map_icon.clone()), wanted.transform,
                Visibility::Inherited, ChildOf(root))).id();
            if let Some((prop_id, operation)) = &wanted.interactive {
                commands.entity(entity).insert(Interactive { prop_id: *prop_id, operation: operation.clone() });
            }
            scene.placed.insert(wanted.key.clone(), Placed { entity, desired: wanted.clone(), visual: None });
        }
        let placed = &scene.placed[&wanted.key];
        let resident = prop_near(wanted.transform.translation, focus, placed.visual.is_some());
        if !resident {
            if let Some(visual) = scene.placed.get_mut(&wanted.key).unwrap().visual.take() {
                commands.entity(visual).try_despawn();
            }
            continue;
        }
        let path = crate::rendering::model_variant::prop_path(&wanted.graphics, &wanted.state);
        needed_models.insert(path.clone());
        if placed.visual.is_some() { continue; }
        if !scene.models.contains_key(&path) {
            if in_flight >= budget { complete = false; continue; }
            scene.models.insert(path.clone(), assets.load(path.clone()));
            in_flight += 1;
        }
        let handle = scene.models[&path].clone();
        let Some(gltf) = gltfs.get(&handle) else {
            if !assets.load_state(&handle).is_failed() { complete = false; }
            continue;
        };
        let Some(world) = gltf.scenes.first() else { continue };
        if added >= 8 { complete = false; continue; }
        added += 1;
        let visual = commands.spawn((WorldAssetRoot(world.clone()), Transform::default(), Visibility::Hidden,
            ChildOf(scene.placed[&wanted.key].entity),
            PendingState { state: wanted.state.clone(), model: handle, animate: true })).id();
        scene.placed.get_mut(&wanted.key).unwrap().visual = Some(visual);
    }
    scene.models.retain(|path, _| needed_models.contains(path));
    if complete && (!scene.complete || scene.revision != Some(revision)) {
        info!("{}: {} of {} DB4/network prop visuals resident", current.name,
            scene.placed.values().filter(|p| p.visual.is_some()).count(), scene.placed.len());
    }
    scene.complete = complete;
    scene.revision = Some(revision);
}

fn prop_near(position: Vec3, focus: Vec3, resident: bool) -> bool {
    position.xz().distance(focus.xz()) <= if resident { crate::world::map::UNLOAD_DISTANCE } else { crate::world::map::LOAD_DISTANCE }
}

pub(crate) const STATES: &[&str] = &[
    "active",
    "inactive",
    "cooldown",
    "invalidstate",
    "blue",
    "red",
    "yellow",
    "green",
    "goingactive",
    "goinginactive",
    "goininactive", // Misspelled transition group in the original shrine models.
    "goingcooldown",
];
pub(crate) fn selected_state<'a>(wanted: &'a str, available: &HashSet<String>) -> &'a str {
    if available.contains(wanted) {
        wanted
    } else if wanted == "cooldown" && available.contains("inactive") {
        "inactive"
    } else if wanted != "invalidstate" && available.contains("active") {
        "active"
    } else {
        wanted
    }
}
#[allow(clippy::too_many_arguments)]
fn apply_states(
    mut commands: Commands,
    mut scene: ResMut<PropScene>,
    pending: Query<(Entity, &PendingState, Option<&WorldInstance>)>,
    instances: Res<WorldInstanceSpawner>,
    children: Query<&Children>,
    mut names: Query<(&Name, &mut Visibility)>,
    players: Query<(), With<AnimationPlayer>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    scene.waiting_states = false;
    for (entity, state, instance) in &pending {
        if !instance.is_some_and(|i| instances.instance_is_ready(**i)) {
            scene.waiting_states = true;
            continue;
        }
        let parts: Vec<_> = children
            .iter_descendants(entity)
            .filter_map(|e| {
                let (name, _) = names.get(e).ok()?;
                let name = name.as_str().to_lowercase();
                (STATES.contains(&name.as_str()) || name == state.state).then_some((e, name))
            })
            .collect();
        let available = parts.iter().map(|(_, n)| n.clone()).collect();
        let selected = selected_state(&state.state, &available);
        for (e, name) in parts {
            if let Ok((_, mut visibility)) = names.get_mut(e) {
                *visibility = if name == selected {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
        if state.animate {
            if let (Some(gltf), Some(player)) = (
                gltfs.get(&state.model),
                children
                    .iter_descendants(entity)
                    .find(|e| players.contains(*e)),
            ) {
                if let Some(clip) = gltf.animations.first() {
                    let (graph, node) = AnimationGraph::from_clip(clip.clone());
                    let sound_clips = gltf.named_animations.iter().find(|(_, h)| *h == clip)
                        .map(|(name, _)| (node, name.to_string())).into_iter().collect();
                    let mut animation = AnimationPlayer::default();
                    animation.play(node).repeat();
                    commands
                        .entity(player)
                        .insert((AnimationGraphHandle(graphs.add(graph)), animation,
                            crate::audio::AnimationAudio::new(entity, &state.model.path().unwrap().path().to_string_lossy(), "", sound_clips)));
                }
            }
        }
        commands
            .entity(entity)
            .insert(Visibility::Inherited)
            .remove::<PendingState>();
    }
}

#[cfg(test)]
mod tests;
