//! Clicking an NPC owns that mouse hold, approaches it, then requests its options.
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use dsor_proto::commands::{actors, ClientCommand, ServerCommand};
use std::collections::{HashMap, VecDeque};

use crate::{actors::character::{AnimState, CharacterAnim}, world::nav::{CurrentNav, NavMesh}, network::net::{LocalPlayer, Net}, actors::npc::{Npc, NpcTemplate}};

pub(crate) const TALK_DISTANCE: f32 = 1.8;
const HEIGHT_TOLERANCE: f32 = 1.5;

#[derive(Default)]
pub(crate) struct NetworkDialogue {
    pub infos: HashMap<u32, actors::NpcInfo>,
    pub quests: HashMap<String, u8>,
    /// The log in full: states, finished tasks and their counters
    /// (crate::gameplay::quests).
    pub log: HashMap<String, crate::gameplay::quests::QuestEntry>,
    pub offers: HashMap<String, dsor_proto::commands::inventory::Offer>,
}

impl NetworkDialogue {
    pub fn reset_map(&mut self) { self.infos.clear(); self.offers.clear(); }
    pub fn receive(&mut self, command: &ServerCommand, actor: Option<u32>) {
        match command {
            ServerCommand::Offer(offer) => {
                if offer.keep_shop {
                    if let Some(shop) = self.offers.get_mut(&offer.shop) { shop.items.clone_from(&offer.items); }
                } else { self.offers.insert(offer.shop.clone(), offer.clone()); }
            }
            ServerCommand::NpcInfo(info) => {
                if let Some(actor) = actor { self.infos.insert(actor, info.clone()); }
            }
            ServerCommand::ActorsLeftVicinity(gone) => {
                for actor in &gone.actors { self.infos.remove(actor); }
            }
            ServerCommand::QuestLogInfo(log) => {
                self.quests = log.records.iter().map(|q| (q.id.to_lowercase(), q.state)).collect();
                self.log = log.records.iter().map(|q| (q.id.to_lowercase(), crate::gameplay::quests::QuestEntry {
                    state: q.state, finished: q.finished, progress: q.progress.clone(), targets: HashMap::new(),
                })).collect();
            }
            // Kind 2: the quest's state; kinds 0 and 1: a task's count and its target
            // (1: the task is done). The quest is named by string key 0.
            ServerCommand::QuestUpdate(q) if q.kind <= 2 => {
                let Some((_, id)) = q.strings.iter().find(|(k, _)| *k == 0) else { return };
                let id = id.to_lowercase();
                let entry = self.log.entry(id.clone()).or_default();
                if q.kind == 2 {
                    if let Some(state) = q.ints.iter().find(|(k, _)| *k == 2).and_then(|(_, s)| u8::try_from(*s).ok()) {
                        entry.state = state;
                        self.quests.insert(id, state);
                    }
                } else {
                    entry.apply(q);
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
pub(crate) struct Target {
    pub entity: Entity,
    pub actor: u32,
    pub template: String,
    pub profile: NpcTemplate,
}

#[derive(Resource, Default)]
pub(crate) struct Dialogue {
    pub target: Option<Target>,
    pub open: bool,
    /// Retained through release so holding an NPC click cannot resume ground walking.
    pub held: bool,
    pub choice: Option<usize>,
    pub generation: u64,
    route: VecDeque<Vec3>,
    waypoint: Option<Vec3>,
}

impl Dialogue {
    pub fn close(&mut self) {
        if self.target.take().is_some() { self.generation += 1; }
        self.open = false;
        self.choice = None;
        self.route.clear();
        self.waypoint = None;
    }
}

pub(crate) fn approach_point(player: Vec3, npc: Vec3) -> Vec3 {
    let delta = Vec3::new(player.x - npc.x, 0.0, player.z - npc.z);
    if delta.length() <= TALK_DISTANCE { player }
    else { npc + delta.normalize() * TALK_DISTANCE }
}

fn in_range(player: Vec3, npc: Vec3) -> bool {
    Vec2::new(player.x - npc.x, player.z - npc.z).length() <= TALK_DISTANCE + 0.08
        && (player.y - npc.y).abs() <= HEIGHT_TOLERANCE
}

fn clear_to_npc(mesh: &NavMesh, from: Vec3, npc: Vec3) -> bool {
    // NPC feet may be outside the navigation mesh, inside their collision shape.
    mesh.walkable_segment(from, npc.lerp(from, 0.3))
}

fn approach_route(mesh: &NavMesh, from: Vec3, npc: Vec3) -> Option<Vec<Vec3>> {
    if in_range(from, npc) && clear_to_npc(mesh, from, npc) { return Some(Vec::new()); }
    let preferred = approach_point(from, npc);
    let angle = (preferred.x - npc.x).atan2(preferred.z - npc.z);
    // Try the facing side first, then alternatives when the NPC stands beside a
    // wall/stall and that side has no walkable stopping point.
    for offset in [0, 1, -1, 2, -2, 3, -3, 4] {
        let a = angle + offset as f32 * std::f32::consts::FRAC_PI_4;
        let candidate = npc + Vec3::new(a.sin(), 0.0, a.cos()) * TALK_DISTANCE;
        let Some(height) = mesh.ground(candidate.x, candidate.z, npc.y, HEIGHT_TOLERANCE) else { continue };
        let stop = Vec3::new(candidate.x, height, candidate.z);
        if !clear_to_npc(mesh, stop, npc) { continue; }
        if let Some(route) = mesh.path(from, stop) { return Some(route); }
    }
    None
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn click(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    ui: Res<crate::ui::input::UiInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    npcs: Query<(Entity, &Npc, &GlobalTransform, &InheritedVisibility, Option<&crate::actors::picking::PickBoxes>)>,
    joints: Query<&GlobalTransform>,
    mut players: Query<(&Transform, &mut LocalPlayer)>,
    hovered: Option<Res<crate::actors::monsters::Hovered>>,
    net: NonSend<Net>,
    nav: Option<Res<CurrentNav>>,
    meshes: Res<Assets<NavMesh>>,
    time: Res<Time>,
    mut dialogue: ResMut<Dialogue>,
) {
    if !buttons.pressed(MouseButton::Left) && !buttons.just_released(MouseButton::Left) { dialogue.held = false; }
    let Ok(window) = windows.single() else { return };
    let Ok((tf, mut player)) = players.single_mut() else { dialogue.close(); return };
    if !window.focused || net.death.dead || time.elapsed_secs() < net.death.held_until || player.casting > 0.0 {
        if dialogue.target.is_some() { player.target = None; dialogue.close(); }
        return;
    }
    if !ui.typing && keys.just_pressed(KeyCode::Escape) {
        if dialogue.target.is_some() { player.target = None; dialogue.close(); }
        return;
    }
    let attack = (!ui.blocks(MouseButton::Right) && buttons.just_pressed(MouseButton::Right))
        || (!ui.typing && [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5].into_iter().any(|k| keys.just_pressed(k)));
    if attack && dialogue.target.is_some() { player.target = None; dialogue.close(); }
    if ui.blocks(MouseButton::Left) || !buttons.just_pressed(MouseButton::Left) { return; }
    dialogue.held = false;
    if dialogue.target.is_some() { player.target = None; dialogue.close(); }
    if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) || hovered.is_some_and(|h| h.0.is_some()) { return; }
    let ray = window.cursor_position().zip(cameras.single().ok())
        .and_then(|(p, (c, t))| c.viewport_to_world(t, p).ok());
    let Some(ray) = ray else { return };
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    let hit = npcs.iter().filter(|(_, n, _, visible, _)| visible.get() && !n.profile.no_talk
        && net.npc_dialogue.infos.get(&n.actor).is_none_or(|i| i.enabled))
        .filter_map(|(e, n, t, _, boxes)| {
            let distance = crate::actors::picking::friendly_hit(ray, t.translation(), boxes, &joints)?;
            Some((distance, e, n, t.translation()))
        }).min_by(|a, b| a.0.total_cmp(&b.0));
    let Some((_, entity, npc, position)) = hit else { return };
    dialogue.held = true;
    player.target = None;
    let route = match mesh {
        Some(mesh) => approach_route(mesh, tf.translation, position),
        None => Some(if in_range(tf.translation, position) { Vec::new() } else { vec![approach_point(tf.translation, position)] }),
    };
    let Some(route) = route else { info!("dialogue: no reachable approach to {}", npc.template); return };
    dialogue.generation += 1;
    dialogue.target = Some(Target { entity, actor: npc.actor, template: npc.template.clone(), profile: npc.profile.clone() });
    dialogue.route = route.into();
    dialogue.waypoint = dialogue.route.pop_front();
    player.target = dialogue.waypoint;
    info!("dialogue: approaching {} via {} waypoints", npc.template, dialogue.route.len() + usize::from(dialogue.waypoint.is_some()));
}

/// Runs after movement/corrections so range is checked against the actual
/// stopped position, with the current tick's movement sent before the request.
pub(crate) fn arrive(
    mut dialogue: ResMut<Dialogue>,
    npcs: Query<(&Npc, &Transform, &Visibility), Without<LocalPlayer>>,
    mut players: Query<(&mut Transform, &mut LocalPlayer, &mut CharacterAnim)>,
    mut net: NonSendMut<Net>,
    nav: Option<Res<CurrentNav>>,
    meshes: Res<Assets<NavMesh>>,
) {
    let Some(target) = dialogue.target.clone() else { return };
    let Ok((mut tf, mut player, mut anim)) = players.single_mut() else { dialogue.close(); return };
    let Ok((npc, at, visibility)) = npcs.get(target.entity) else { player.target = None; dialogue.close(); return };
    if npc.actor != target.actor || *visibility == Visibility::Hidden || net.death.dead || player.casting > 0.0
        || net.npc_dialogue.infos.get(&npc.actor).is_some_and(|i| !i.enabled) {
        player.target = None;
        dialogue.close();
        return;
    }
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    let close = in_range(tf.translation, at.translation) && mesh.is_none_or(|m| clear_to_npc(m, tf.translation, at.translation));
    if dialogue.open {
        if !close || player.target.is_some() { dialogue.close(); }
        return;
    }
    if !close {
        if player.target.is_none() {
            if dialogue.waypoint.is_some_and(|p| p.xz().distance(tf.translation.xz()) <= 0.1) {
                dialogue.waypoint = dialogue.route.pop_front();
                player.target = dialogue.waypoint;
            }
            if player.target.is_none() { dialogue.close(); }
        }
        return;
    }
    player.target = None;
    let delta = at.translation - tf.translation;
    if delta.x * delta.x + delta.z * delta.z > 1e-6 { player.facing = delta.x.atan2(delta.z); }
    tf.rotation = Quat::from_rotation_y(player.facing);
    anim.state = AnimState::Idle;
    dialogue.open = true;
    if !net.offline {
        net.send(&ClientCommand::Encounter(actors::Encounter { actor: target.actor }));
        net.send(&ClientCommand::NpcRequest(actors::NpcRequest { actor: target.actor }));
    }
    info!("dialogue: opened {} ({:#x})", target.template, target.actor);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn player(target: Option<Vec3>) -> LocalPlayer {
        let mut player = LocalPlayer::default();
        player.actor = 1;
        player.target = target;
        player
    }
    #[test]
    fn approach_stops_short_and_nearby_click_does_not_back_up() {
        let npc = Vec3::new(10.0, 3.0, 5.0);
        let player = Vec3::new(0.0, 3.0, 5.0);
        let stop = approach_point(player, npc);
        assert!((stop.distance(npc) - TALK_DISTANCE).abs() < 1e-5);
        assert!(stop.x > player.x && stop.x < npc.x);
        let near = npc - Vec3::X;
        assert_eq!(approach_point(near, npc), near);
        assert!(in_range(near, npc));
        assert!(!in_range(near + Vec3::Y * 5.0, npc));
    }
    #[test]
    fn close_retains_click_ownership_and_invalidates_choices() {
        let mut dialogue = Dialogue { held: true, open: true, choice: Some(1), ..default() };
        dialogue.close();
        assert!(dialogue.held);
        assert!(!dialogue.open);
        assert_eq!(dialogue.choice, None);
    }
    #[test]
    fn vicinity_removal_and_map_change_discard_npc_options() {
        let mut state = NetworkDialogue::default();
        state.receive(&ServerCommand::NpcInfo(actors::NpcInfo::default()), Some(12));
        assert!(state.infos.contains_key(&12));
        state.reset_map();
        assert!(state.infos.is_empty());
    }
    #[test]
    fn arrival_waits_for_range_then_stops_faces_and_closes_on_despawn() {
        let mut app = App::new();
        app.init_resource::<Time<Real>>().init_resource::<Assets<NavMesh>>().init_resource::<Dialogue>()
            .add_systems(Update, arrive);
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        let npc = app.world_mut().spawn((Npc { actor: 42, ..default() }, Transform::from_xyz(8.0, 0.0, 0.0), Visibility::Inherited)).id();
        let mut animation = CharacterAnim::default();
        animation.state = AnimState::Run;
        let player = app.world_mut().spawn((Transform::default(), animation, player(Some(Vec3::new(6.2, 0.0, 0.0))))).id();
        app.world_mut().resource_mut::<Dialogue>().target = Some(Target { entity: npc, actor: 42, template: "test".into(), profile: default() });
        app.update();
        assert!(!app.world().resource::<Dialogue>().open);
        app.world_mut().get_mut::<Transform>(player).unwrap().translation.x = 6.2;
        app.update();
        assert!(app.world().resource::<Dialogue>().open);
        assert!(app.world().get::<LocalPlayer>(player).unwrap().target.is_none());
        assert_eq!(app.world().get::<CharacterAnim>(player).unwrap().state, AnimState::Idle);
        assert!((app.world().get::<LocalPlayer>(player).unwrap().facing - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        app.update();
        assert!(app.world().resource::<Dialogue>().open);
        app.world_mut().entity_mut(npc).despawn();
        app.update();
        assert!(app.world().resource::<Dialogue>().target.is_none());
        assert!(!app.world().resource::<Dialogue>().open);
    }

    #[test]
    fn blocked_approach_cancels_without_opening_dialogue() {
        let mut app = App::new();
        app.init_resource::<Time<Real>>().init_resource::<Assets<NavMesh>>().init_resource::<Dialogue>()
            .add_systems(Update, arrive);
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        let npc = app.world_mut().spawn((Npc { actor: 42, ..default() }, Transform::from_xyz(8.0, 0.0, 0.0), Visibility::Inherited)).id();
        app.world_mut().spawn((Transform::default(), CharacterAnim::default(), player(None)));
        app.world_mut().resource_mut::<Dialogue>().target = Some(Target { entity: npc, actor: 42, template: "test".into(), profile: default() });
        app.update();
        assert!(app.world().resource::<Dialogue>().target.is_none());
        assert!(!app.world().resource::<Dialogue>().open);
    }

    #[test]
    fn click_picks_npc_and_ui_press_cannot_start_an_approach() {
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
        let mut app = App::new();
        app.add_plugins(crate::ui::input::UiInputPlugin)
            .init_resource::<Time<Real>>().init_resource::<Time>()
            .init_resource::<Assets<NavMesh>>().init_resource::<Dialogue>()
            .init_resource::<ButtonInput<MouseButton>>().init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, click);
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        let mut window = Window { resolution: (1024, 768).into(), focused: true, ..default() };
        window.set_cursor_position(Some(Vec2::new(512.0, 384.0)));
        app.world_mut().spawn((window, PrimaryWindow));
        app.world_mut().spawn((Camera3d::default(), Camera {
            computed: ComputedCameraValues {
                clip_from_view: Mat4::perspective_infinite_reverse_rh(1.0, 1024.0 / 768.0, 0.1),
                target_info: Some(RenderTargetInfo { physical_size: UVec2::new(1024, 768), scale_factor: 1.0 }), ..default()
            }, ..default()
        }, GlobalTransform::from(Transform::from_xyz(0.0, 10.0, 10.0).looking_at(Vec3::Y, Vec3::Y))));
        let npc = app.world_mut().spawn((Npc { actor: 42, ..default() }, GlobalTransform::IDENTITY, InheritedVisibility::VISIBLE)).id();
        // An authored joint box can miss the visible body (Petyax's outfit).
        // Clicking the torso must still interact, just as the hover outline does.
        app.world_mut().entity_mut(npc).insert(crate::actors::picking::PickBoxes(vec![
            (npc, Transform::from_xyz(100.0, 0.0, 0.0), Vec3::splat(0.5)),
        ]));
        let player = app.world_mut().spawn((Transform::from_xyz(-8.0, 0.0, 0.0), player(None))).id();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert_eq!(app.world().resource::<Dialogue>().target.as_ref().map(|t| t.actor), Some(42));
        assert!(app.world().resource::<Dialogue>().held);
        let stop = app.world().get::<LocalPlayer>(player).unwrap().target.unwrap();
        assert!((stop.x + TALK_DISTANCE).abs() < 1e-5);
        app.world_mut().resource_mut::<Dialogue>().close();
        app.world_mut().get_mut::<LocalPlayer>(player).unwrap().target = None;
        app.world_mut().spawn((crate::ui::input::UiRegions(vec![(0.0, [400.0, 300.0, 600.0, 450.0])]), Visibility::Inherited));
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        buttons.release(MouseButton::Left); buttons.clear(); buttons.press(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<Dialogue>().target.is_none());
        assert!(app.world().get::<LocalPlayer>(player).unwrap().target.is_none());
    }

    #[test]
    fn npc_route_goes_around_a_stall_and_rejects_disconnected_ground() {
        let mut triangles = Vec::new();
        for x in 0..5 {
            for z in 0..5 {
                if x == 2 && (1..4).contains(&z) { continue; }
                let a = Vec3::new(x as f32, 0.0, z as f32);
                triangles.extend([[a, a + Vec3::X, a + Vec3::X + Vec3::Z], [a, a + Vec3::X + Vec3::Z, a + Vec3::Z]]);
            }
        }
        let isolated = Vec3::new(10.0, 0.0, 0.0);
        triangles.push([isolated, isolated + Vec3::X, isolated + Vec3::Z]);
        let nav = NavMesh::new(triangles);
        let start = Vec3::new(0.5, 0.0, 2.5);
        let goal = Vec3::new(4.5, 0.0, 2.5);
        assert!(!nav.walkable_segment(start, goal));
        let path = nav.path(start, goal).expect("stall has an open route around it");
        assert!(path.len() > 1);
        assert_eq!(path.last(), Some(&goal));
        let mut at = start;
        for next in path { assert!(nav.walkable_segment(at, next)); at = next; }
        assert!(nav.path(start, isolated + Vec3::splat(0.1)).is_none());
    }
}
