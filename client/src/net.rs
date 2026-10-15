//! The game server, inside Bevy: the session, the transport, and what the world
//! messages do to the scene.
//!
//! Natively the transport is UDP straight to the server; in the browser it is the
//! WebSocket relay (crates/relay). Either way the RakNet connection is the client's
//! own (crates/raknet), and `dsor_proto::session::Session` walks login -> character
//! service -> map.
//!
//! Frames: positions the server DESCRIBES (NewPlayer, NewMonster, ...) are in the
//! map's frame, the one the placement manifests use. Movement records are on the wire
//! frame: world units * 128 relative to the map header's centre, which is the
//! manifest's `center` (experimental dsor/mapheader.frame_origin reads the same
//! header field). SEE: wire_to_game, game_to_wire.

use std::f32::consts::TAU;
use std::collections::HashMap;

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use dsor_proto::commands::{movement, ClientCommand, ServerCommand};
use dsor_proto::identity::Credentials;
use dsor_proto::session::{Session, SessionEvent};
use dsor_raknet::Reliability;
use dsor_transport::Transport;

use crate::character::{redress, spawn_character, AnimState, Character, CharacterAnim, CharacterDesc, CharacterLibrary, ItemSkins};
use crate::appearance::{self, Equipment};
use crate::map::{CurrentMap, MapManifest, MapRoot};
use crate::nav::{CurrentNav, NavMesh};
use crate::nameplate::Nameplate;

/// How far up or down one step may go (stairs, slopes).
const MAX_STEP: f32 = 1.2;
/// Seconds ANOTHER player keeps running after they last moved: just over the 40 ms
/// between two of their records. SEE: move_remotes. The player's own run stops the
/// moment they stop ("elle devrait s'arreter quand j'arrete de marcher").
pub(crate) const RUN_GRACE: f32 = 0.12;
/// A click this close to the player is no destination: holding the button on
/// yourself must not make the run flicker.
const CLICK_DEAD_ZONE: f32 = 0.4;

/// Game tick, as the 2018 client counts it.
const TICK_SECONDS: f32 = 0.04;
/// _Template_Player.RunSpeed, the same 5.0 for every class (static.db4).
/// UNKNOWN: the unit -- tuned by eye against the map until confirmed.
pub const RUN_SPEED: f32 = 5.0;
/// The duration every captured client MoveCommand carries (session-walk4: 20).
const MOVE_DURATION: u16 = 20;
/// The game camera, from _Globals (static.db4): CamRotX -45, CamRotY -45,
/// DefCamFov 34, Min/Max/DefCamHeight 12/25/25, CamOffsetZ 0.5.
/// INFERRED: the height is the camera's altitude above the player and the FOV is
/// vertical -- the client's ObserverCameraProperty (0x4E9B84) was not traced to the
/// end; checked by eye against the 2018 client.
pub const CAM_PITCH_DEG: f32 = -45.0;
pub const CAM_YAW_DEG: f32 = -45.0;
pub const CAM_FOV_DEG: f32 = 34.0;
pub const CAM_HEIGHT_MIN: f32 = 12.0;
pub const CAM_HEIGHT_MAX: f32 = 25.0;
pub const CAM_HEIGHT_DEFAULT: f32 = 25.0;
pub const CAM_OFFSET_Z: f32 = 0.5;

/// Exponential response rates: zoom settles in about 250 ms, turns in 200 ms.
const ZOOM_RESPONSE: f32 = 14.0;
const TURN_RESPONSE: f32 = 18.0;

/// Rendered and requested distances along the camera's view direction.
#[derive(Resource)]
pub struct CameraZoom {
    current: f32,
    target: f32,
}

impl CameraZoom {
    fn new(distance: f32) -> Self {
        let distance = if distance.is_finite() { distance.clamp(CAM_HEIGHT_MIN, CAM_HEIGHT_MAX) } else { CAM_HEIGHT_DEFAULT };
        Self { current: distance, target: distance }
    }

    fn advance(&mut self, dt: f32) {
        self.current += (self.target - self.current) * response(ZOOM_RESPONSE, dt);
        if (self.target - self.current).abs() < 0.001 {
            self.current = self.target;
        }
    }
}

/// Equal elapsed time gives the same blend at low and high frame rates.
fn response(rate: f32, dt: f32) -> f32 {
    -(-rate * dt.max(0.0)).exp_m1()
}

/// Smooth the displayed heading only; movement and network aim use `facing`.
/// Quaternion interpolation takes the short arc across the +/-pi boundary.
fn turn_towards(rotation: Quat, facing: f32, dt: f32) -> Quat {
    let target = Quat::from_rotation_y(facing);
    if rotation.angle_between(target) < 0.001 {
        target
    } else {
        rotation.slerp(target, response(TURN_RESPONSE, dt)).normalize()
    }
}

/// Where and as whom to connect. Absent: the client is a map viewer.
#[derive(Resource, Clone, Debug)]
pub struct NetConfig {
    /// The login server, "host:port".
    pub login: String,
    pub account: String,
    pub session: String,
    /// Log in at the account service first (release261 requires it): its user
    /// name and password; `account` and `session` then come from its answer.
    pub user: Option<String>,
    pub password: Option<String>,
    /// The account service, "host:port" (default: the login server's host, 8080).
    pub account_api: Option<String>,
    /// The character to create when the account has none: name, class (0
    /// warrior, 1 mage, 2 ranger, 3 dwarf), gender. Default: the user name, a
    /// warrior.
    pub new_character: Option<(String, u8, u8)>,
    /// The WebSocket relay, required in the browser ("ws://host:2290").
    pub relay: Option<String>,
    /// Which character to highlight initially in the selection screen.
    pub character: Option<u32>,
    /// The chat service, "host:port" (crate::chat; default: the login server's
    /// host, 2191).
    pub chat: Option<String>,
}

/// The chat service of an account service's launch document.
/// EVIDENCE: dso-server-production services/accounts.py:464-470 (`launch`:
///   {"host", "gateway_port", "chat_port", "instance_id"}); the chat shares the
///   gateway's host (accounts.py:400).
pub fn chat_target(launch: &serde_json::Value, login: &str) -> String {
    let fallback = login.rsplit_once(':').map_or(login, |(h, _)| h);
    let host = launch["host"].as_str().filter(|h| !h.is_empty()).unwrap_or(fallback);
    let port = launch["chat_port"].as_u64().filter(|p| *p > 0 && *p <= 65535).unwrap_or(2191);
    format!("{host}:{port}")
}

/// CONTRACT: a NON-SEND resource: in the browser the transport holds a WebSocket
///   and JS closures, which cannot cross threads; natively it costs nothing.
pub struct Net {
    /// Playing without a server (crate::debug's playable character): nothing is
    /// sent or received; the local player, its skills and its moves work as online.
    pub offline: bool,
    /// Offline: the character to put on the map once its centre is known.
    pub offline_pending: Option<CharacterDesc>,
    session: Session,
    transport: Option<Transport>,
    clock_offset: Option<i64>,
    relay: Option<String>,
    wanted_character: Option<u32>,
    /// What to create if the server opens the character editor.
    new_character: (String, u8, u8),
    pub local_actor: Option<u32>,
    /// The map frame's centre (wire = (game - centre) * 128).
    pub centre: Option<Vec3>,
    /// (look, position, heading, name, admin).
    pending_local: Option<(CharacterDesc, Vec3, f32, String, bool)>,
    /// Actors already asked about (ActorRequest), so each is asked once.
    requested: std::collections::HashSet<u32>,
    /// Other players to spawn, despawn, or move, applied by `apply_remotes`.
    remote_spawns: Vec<(u32, CharacterDesc, Vec3, f32, String, bool)>,
    /// NPCs to place (NewNPCCommand) and actors gone, applied by crate::npc.
    pub npc_spawns: Vec<NpcSpawn>,
    pub npc_gone: Vec<u32>,
    pub props: crate::props::NetworkProps,
    pub map_markers: crate::ui::MapMarkers,
    /// Monsters to place (NewMonsterCommand 48), for crate::monsters; they leave
    /// with DiscardMonster (49) through `remote_gone`, as they move as remote actors.
    pub monster_spawns: Vec<MonsterSpawn>,
    /// Health of monsters (MonsterUpdate 50): actor, health, maximum.
    pub monster_health: Vec<(u32, u32, u32)>,
    /// Blows (HitCommand 115) and deaths (KillCommand 116), for crate::monsters.
    pub hits: Vec<(u32, dsor_proto::commands::combat::Hit)>,
    pub kills: Vec<(u32, dsor_proto::commands::combat::Kill)>,
    /// The local player's death: dead since KillCommand (116) until ReviveCommand
    /// (118), the death dialog asked for (ShowDeathDialogCommand 138), and the
    /// respawn choice to send (RespawnCommand 106, from crate::ui).
    pub death: DeathState,
    /// Other actors revived (ReviveCommand 118), for their death pose.
    pub revives: Vec<u32>,
    /// Every KillCommand's victim, for the players' death pose (monsters take
    /// theirs from `kills`, crate::monsters).
    player_deaths: Vec<u32>,
    /// Our character's name, to recognise our own stale session (below).
    pub local_name: Option<String>,
    /// Our current skill resource (rage, mana...), from ActorStatsUpdate (132).
    pub resource: Option<f32>,
    /// Our current health, from the same command.
    pub health: Option<f32>,
    /// Our maximum health, from a blow on us (HitCommand's victim maximum).
    pub max_health: Option<f32>,
    /// Level, and experience: (total, the level's floor, the next level's floor).
    /// NewPlayer +240.. [0, level, experience, floor, ceiling], then
    /// PlayerLevelUpdate (133) and XPChanged (134).
    pub level: Option<u32>,
    pub xp: Option<(u32, u32, u32)>,
    /// The wallet: slot 0 Andermant, slot 1 gold in copper (NewPlayer +356,
    /// CurrencyChanged 137). EVIDENCE: experimental dsor/mapinstance.py WALLET_RC /
    /// WALLET_VC (handler 0x513E39).
    pub wallet: Option<[u64; 5]>,
    /// The first quick slot bar (QuickSlotsInfo 0x50): skill ids by wire slot.
    pub bar: Vec<Option<String>>,
    /// Preserve all slot kinds and bars when saving an edited bar (0x51).
    pub quickbars: dsor_proto::commands::inventory::QuickSlotsInfo,
    pub inventory: crate::inventory::InventoryState,
    pub corrections: crate::corrections::Corrections,
    pub skill_book: Option<dsor_proto::commands::inventory::SkillBookInfo>,
    /// Skills other actors used, relayed by the server (73-77), for crate::skills.
    pub skill_events: Vec<SkillEvent>,
    /// Status effects the server started on actors (82), and ground effects it
    /// placed (64) or removed (65), for crate::skills.
    pub status_events: Vec<StatusEvent>,
    pub location_events: Vec<LocationEvent>,
    /// Offline play: status effects by name (holder, status id, seconds), for
    /// crate::skills (online they come by wire index in `status_events`).
    pub named_status_events: Vec<(u32, String, f32)>,
    pub location_gone: Vec<u32>,
    remote_gone: Vec<u32>,
    /// (actor, where, facing, the record's own start tick).
    remote_moves: Vec<(u32, Vec3, f32, u32)>,
    /// Re-resolve after full inventory or compact item updates; retained until
    /// the local entity, item records and skin table are available.
    local_appearance_dirty: bool,
    /// Another player re-dressed (RemotePlayerInfo): actor, skins, armament.
    remote_redress: HashMap<u32, (Vec<Equipment>, i8)>,
}

/// A status effect started on an actor (StatusEffectCommand 82): its
/// _Template_StatusEffect index, the instance the client keys on, how long.
pub struct StatusEvent {
    pub holder: u32,
    pub index: u16,
    pub instance: u32,
    pub seconds: f32,
}

/// A ground effect (NewLocationEffectCommand 64): it stays where it is placed.
pub struct LocationEvent {
    pub id: u32,
    pub status: String,
    pub position: Vec3,
    pub heading: f32,
    pub seconds: Option<f32>,
}

/// A skill another actor used: its wire index, its aim (the command's radians) and,
/// for 76/77, the points it aimed at (game frame).
pub struct SkillEvent {
    pub actor: u32,
    pub wire: u16,
    pub heading: f32,
    pub points: Vec<[f32; 3]>,
    /// The game tick the skill starts at (the command's own).
    pub start_tick: u32,
}

/// One NewNPCCommand: the template, the level Guid (hex) and where.
pub struct NpcSpawn {
    pub actor: u32,
    pub template: String,
    pub guid: String,
    pub position: Vec3,
    pub visible: bool,
}

/// The death dialog's button (crate::ui): (at the nearest exit, on the spot).
#[derive(Resource, Default)]
pub struct RespawnChoice(pub Option<(bool, bool)>);

/// The local player's death (Net::death).
#[derive(Default, Debug, Clone)]
pub struct DeathState {
    pub dead: bool,
    /// The death dialog is to be shown (until a respawn choice or a revive).
    pub dialog: bool,
    /// A respawn choice to send: (at the nearest exit, on the spot).
    pub respawn: Option<(bool, bool)>,
    /// A respawn was sent and not answered yet: the dialog stays closed.
    pub requested: bool,
    /// The last respawn sent, for its revival hold (player_deaths).
    pub chosen: Option<(bool, bool)>,
    /// Seconds (Time::elapsed) until which the revival holds the player: clicks
    /// do not walk (click_to_move), as the server locks its movement.
    pub held_until: f32,
}

/// A release261 u64 health, as the u32 the client keeps.
fn sat(v: u64) -> u32 {
    v.min(u32::MAX as u64) as u32
}

/// One NewMonsterCommand: its _Template_Monster blueprint and where (map frame).
pub struct MonsterSpawn {
    pub actor: u32,
    pub blueprint: String,
    pub position: Vec3,
    pub health: u32,
    pub level: u32,
}

/// Another player, drawn from NewRemotePlayer and moved by their MoveCommands.
#[derive(Component, Default)]
pub struct RemotePlayer {
    pub actor: u32,
    /// Where their last record says they are heading.
    pub target: Vec3,
    pub facing: f32,
    /// Seconds since this player last moved, so the run cycle is not cut between
    /// two of their records.
    pub still_for: f32,
    /// Seconds left of its own skill, during which movement records are ignored:
    /// it stands where and as it cast.
    /// EVIDENCE: HandleMoveCommand 0x518E80 applies a record whole only while the
    ///   actor's feature 0 is on (FUN_004abd74(0)); otherwise it keeps the record's
    ///   start and end ticks alone (0x9FB92E, 0x9FB8CA). The skill runner turns
    ///   Movement off until start + the unblock frames (0xA005A7, per the
    ///   experimental server's World.held_until notes).
    /// FAILURE: applied, a creature's standing records (facing byte 0) turned it
    ///   away while it struck ("ils tapent a 180 dans le mauvais sens").
    pub held: f32,
    /// The game tick its skill's hold ends at (the skill command's start tick plus
    /// its unblock frames): its movement records up to then are not applied.
    /// EVIDENCE: HandleMoveCommand 0x518E80 keeps only a record's ticks while the
    ///   actor's movement is off; recorded online (a0302 swamp thing,
    ///   ThingSwampStrike, unblock 41): the server's facing-0 records span the
    ///   skill's start - 2 .. start + 41 ticks exactly. Measured in the records'
    ///   own ticks: the client's clock (Net::server_tick) is on another scale.
    /// FAILURE: held by time from the command's arrival, the hold ended before
    ///   those records did, and the creature looked away before striking again.
    pub held_until_tick: Option<u32>,
    /// Its speed, from its last two records (distance over their ticks); 0 until
    /// known. It runs at that speed between records, so it is still moving when
    /// the next one arrives.
    /// FAILURE: run at the player's speed (6.25), a creature whose records come
    ///   every 4 ticks reached each one early and stood until the next: it moved
    ///   in jerks ("les mobs se deplacent frame par frame").
    pub speed: f32,
    /// Its last record's position and tick, for `speed`.
    pub last_record: Option<(Vec3, u32)>,
    /// Seconds between its records (monsters: every 4 ticks), 0 until known.
    pub interval: f32,
}

/// The player this client controls.
#[derive(Component)]
pub struct LocalPlayer {
    pub actor: u32,
    /// Where the player is walking to, game frame.
    pub target: Option<Vec3>,
    /// Requested facing around +Y; the displayed body eases toward it while walking.
    pub facing: f32,
    last_sent_tick: u32,
    /// Stood on the navigation mesh since arriving on this map.
    snapped: bool,
    /// Seconds the current skill still holds the player in place (its
    /// MotionUnblockFrame); walking and the run/idle animation wait for it.
    pub casting: f32,
}

#[cfg(test)]
mod pointer_tests {
    use super::*;
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
    use crate::ui_input::{BlocksWorldInput, UiInputPlugin, UiRegions};

    #[test]
    fn ui_and_debug_panels_block_actual_walk_targets_but_world_clicks_work() {
        let mut app = App::new();
        app.add_plugins(UiInputPlugin)
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Assets<NavMesh>>()
            .init_resource::<Time>()
            .add_systems(Update, click_to_move);
        let mut window = Window { resolution: (1024, 768).into(), ..default() };
        window.set_cursor_position(Some(Vec2::new(400.0, 400.0)));
        app.world_mut().spawn((window, PrimaryWindow));
        app.world_mut().spawn((Camera3d::default(), Camera {
            computed: ComputedCameraValues {
                clip_from_view: Mat4::perspective_infinite_reverse_rh(1.0, 1024.0 / 768.0, 0.1),
                target_info: Some(RenderTargetInfo { physical_size: UVec2::new(1024, 768), scale_factor: 1.0 }),
                ..default()
            },
            ..default()
        }, GlobalTransform::from(Transform::from_xyz(0.0, 10.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y))));
        let player = app.world_mut().spawn((Transform::default(), LocalPlayer {
            actor: 1, target: None, facing: 0.0, last_sent_tick: 0, snapped: true, casting: 0.0,
        })).id();
        let press = |app: &mut App| {
            let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            buttons.release(MouseButton::Left);
            buttons.clear();
            buttons.press(MouseButton::Left);
        };
        press(&mut app);
        app.update();
        assert!(app.world().get::<LocalPlayer>(player).unwrap().target.is_some(), "ground clicks must still move");

        // Mesh HUD/menu, native button, and empty F2/F3 panel background.
        for kind in 0..3 {
            app.world_mut().get_mut::<LocalPlayer>(player).unwrap().target = None;
            let blocker = match kind {
                0 => app.world_mut().spawn((UiRegions(vec![(0.0, [350.0, 300.0, 500.0, 500.0])]), Visibility::Inherited)).id(),
                1 => app.world_mut().spawn((Interaction::Pressed, InheritedVisibility::VISIBLE)).id(),
                _ => app.world_mut().spawn((BlocksWorldInput, Visibility::Inherited, InheritedVisibility::VISIBLE,
                    bevy::ui::RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::ZERO) })).id(),
            };
            // Continue a world-origin hold across each kind of UI. Clearing
            // the target proves this frame still generates a walk target.
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            app.update();
            assert!(app.world().get::<LocalPlayer>(player).unwrap().target.is_some(), "UI kind {kind} interrupted a world hold");
            app.world_mut().get_mut::<LocalPlayer>(player).unwrap().target = None;
            press(&mut app);
            app.update();
            assert!(app.world().get::<LocalPlayer>(player).unwrap().target.is_none(), "UI kind {kind} leaked a walk");
            app.world_mut().despawn(blocker);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            app.update();
            assert!(app.world().get::<LocalPlayer>(player).unwrap().target.is_none(), "dragging off UI leaked a walk");
            press(&mut app);
            app.update();
            assert!(app.world().get::<LocalPlayer>(player).unwrap().target.is_some(), "fresh world click stayed blocked");
        }
        app.world_mut().get_mut::<LocalPlayer>(player).unwrap().target = None;
        app.world_mut().spawn((UiRegions(vec![(0.0, [350.0, 300.0, 500.0, 500.0])]), Visibility::Hidden));
        press(&mut app);
        app.update();
        assert!(app.world().get::<LocalPlayer>(player).unwrap().target.is_some(), "hidden menus must not intercept input");
    }
}

#[cfg(test)]
mod smoothing_tests {
    use super::*;
    use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
    use std::time::Duration;

    fn local(target: Option<Vec3>) -> LocalPlayer {
        LocalPlayer { actor: 1, target, facing: 0.0, last_sent_tick: 0, snapped: true, casting: 0.0 }
    }

    #[test]
    fn forced_transform_replaces_position_and_stops_stale_movement() {
        let mut app = App::new();
        app.init_resource::<Time<Real>>().add_systems(Update, apply_corrections);
        start_offline(app.world_mut(), 1, 0);
        let mut animation = CharacterAnim::default();
        animation.state = AnimState::Run;
        let actor = app.world_mut().spawn((Transform::default(), local(Some(Vec3::splat(100.0))), animation)).id();
        let remote = app.world_mut().spawn((Transform::default(), CharacterAnim::default(), RemotePlayer {
            actor: 2, target: Vec3::splat(200.0), facing: 0.0, still_for: 0.0, held: 3.0, held_until_tick: Some(100), ..default()
        })).id();
        let at = Vec3::new(14.0, 0.0, 45.0);
        {
            let mut net = app.world_mut().non_send_mut::<Net>();
            net.local_actor = Some(1);
            net.centre = Some(Vec3::new(100.0, 200.0, 300.0));
            for id in [1, 2] {
                on_command(&mut net, ServerCommand::ForceTransform(dsor_proto::commands::movement::ForceTransform {
                    position: at.to_array(), heading: 1.0,
                }), Some(id));
            }
        }
        app.update();
        for e in [actor, remote] {
            let tf = app.world().get::<Transform>(e).unwrap();
            assert_eq!(tf.translation, at, "force positions are absolute, not relative to the map centre");
            assert!(tf.rotation.angle_between(Quat::from_rotation_y(1.0)) < 0.001);
        }
        assert!(app.world().get::<LocalPlayer>(actor).unwrap().target.is_none());
        assert_eq!(app.world().get::<RemotePlayer>(remote).unwrap().target, at);
        assert!(app.world().get::<RemotePlayer>(remote).unwrap().held_until_tick.is_none());
        assert!(app.world().non_send::<Net>().corrections.pending.is_empty());
    }

    fn step(app: &mut App, dt: f32) {
        app.world_mut().resource_mut::<Time>().advance_by(Duration::from_secs_f32(dt));
        app.update();
    }

    #[test]
    fn wheel_eases_camera_and_ui_discards_new_scroll_without_interrupting_settling() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(CameraZoom::new(20.0))
            .init_resource::<crate::ui_input::UiInput>()
            .add_message::<MouseWheel>()
            .add_systems(Update, (zoom_camera, follow_camera).chain());
        app.world_mut().spawn((Transform::default(), local(None)));
        let camera = app.world_mut().spawn((Camera3d::default(), Transform::default())).id();
        let wheel = |app: &mut App, unit, y| {
            app.world_mut().write_message(MouseWheel {
                unit, x: 0.0, y, window: Entity::PLACEHOLDER, phase: bevy::input::touch::TouchPhase::Moved,
            });
        };

        wheel(&mut app, MouseScrollUnit::Line, 5.0);
        step(&mut app, 1.0 / 60.0);
        let zoom = app.world().resource::<CameraZoom>();
        assert_eq!(zoom.target, 15.0);
        assert!(zoom.current > 15.0 && zoom.current < 20.0, "first frame must blend, not jump");
        let distance = app.world().get::<Transform>(camera).unwrap().translation.distance(Vec3::new(0.0, 0.0, CAM_OFFSET_Z));
        assert!((distance - zoom.current).abs() < 1e-5, "camera must use the blended distance");
        let previous = zoom.current;

        app.world_mut().resource_mut::<crate::ui_input::UiInput>().over_ui = true;
        wheel(&mut app, MouseScrollUnit::Line, -8.0);
        step(&mut app, 1.0 / 60.0);
        let zoom = app.world().resource::<CameraZoom>();
        assert_eq!(zoom.target, 15.0, "UI scroll must be consumed without zooming");
        assert!(zoom.current < previous, "an accepted zoom must keep settling over UI");
        app.world_mut().resource_mut::<crate::ui_input::UiInput>().over_ui = false;
        wheel(&mut app, MouseScrollUnit::Pixel, -40.0);
        step(&mut app, 1.0 / 60.0);
        assert_eq!(app.world().resource::<CameraZoom>().target, 16.0);

        for (scroll, limit) in [(1000.0, CAM_HEIGHT_MIN), (-1000.0, CAM_HEIGHT_MAX)] {
            wheel(&mut app, MouseScrollUnit::Line, scroll);
            for _ in 0..60 { step(&mut app, 1.0 / 60.0); }
            let zoom = app.world().resource::<CameraZoom>();
            assert_eq!(zoom.target, limit);
            assert_eq!(zoom.current, limit, "scroll must settle exactly at either limit");
        }
    }

    #[test]
    fn zoom_and_turn_response_are_independent_of_frame_rate() {
        let mut results = Vec::new();
        for fps in [30, 60, 120] {
            let mut zoom = CameraZoom::new(25.0);
            zoom.target = 12.0;
            let mut rotation = Quat::IDENTITY;
            for _ in 0..fps / 10 {
                zoom.advance(1.0 / fps as f32);
                rotation = turn_towards(rotation, 2.8, 1.0 / fps as f32);
                assert!((12.0..25.0).contains(&zoom.current), "zoom must not overshoot");
            }
            results.push((zoom.current, rotation * Vec3::Z));
        }
        for (zoom, forward) in &results[1..] {
            assert!((*zoom - results[0].0).abs() < 1e-4);
            assert!(forward.distance(results[0].1) < 1e-4);
        }
    }

    #[test]
    fn heading_blend_crosses_the_angle_wrap_by_the_short_arc() {
        let start = Quat::from_rotation_y(179.0_f32.to_radians());
        let target = Quat::from_rotation_y(-179.0_f32.to_radians());
        let turned = turn_towards(start, -179.0_f32.to_radians(), 1.0 / 60.0);
        assert!(turned.angle_between(start) < 1.0_f32.to_radians());
        assert!(turned.angle_between(target) < start.angle_between(target));
        assert!((turned * Vec3::Z).z < -0.99, "must not rotate the long way through zero");
    }

    #[test]
    fn local_turn_blends_while_movement_and_aim_change_immediately() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<Assets<NavMesh>>()
            .add_systems(Update, walk_local);
        let player = app.world_mut().spawn((Transform::default(), local(Some(Vec3::new(0.0, 0.0, -2.0))), CharacterAnim::default())).id();
        step(&mut app, 1.0 / 60.0);
        let tf = app.world().get::<Transform>(player).unwrap();
        let facing = app.world().get::<LocalPlayer>(player).unwrap().facing;
        assert!((tf.translation.z + RUN_SPEED / 60.0).abs() < 1e-5);
        assert!((facing.abs() - std::f32::consts::PI).abs() < 1e-5, "logical aim must update immediately");
        let angle = Quat::IDENTITY.angle_between(tf.rotation);
        assert!(angle > 0.1 && angle < 1.0, "180-degree reversal must begin with a partial turn");
        for _ in 0..60 { step(&mut app, 1.0 / 60.0); }
        let tf = app.world().get::<Transform>(player).unwrap();
        assert!(tf.translation.distance(Vec3::new(0.0, 0.0, -2.0)) < 1e-5);
        assert!(tf.rotation.angle_between(Quat::from_rotation_y(facing)) < 0.001);

        let mut p = app.world_mut().get_mut::<LocalPlayer>(player).unwrap();
        p.facing = std::f32::consts::FRAC_PI_2;
        p.casting = 0.5;
        step(&mut app, 1.0 / 60.0);
        assert!((app.world().get::<Transform>(player).unwrap().rotation * Vec3::Z).distance(Vec3::X) < 1e-5,
            "cast effects must still align with their aim immediately");
    }

    #[test]
    fn remote_player_blends_record_heading_but_snaps_on_teleport_and_cast() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default()).add_systems(Update, move_remotes);
        let remote = app.world_mut().spawn((Transform::default(), CharacterAnim::default(), RemotePlayer {
            actor: 2, target: Vec3::X, facing: -std::f32::consts::FRAC_PI_2, still_for: 0.0, held: 0.0, held_until_tick: None, ..default()
        })).id();
        step(&mut app, 1.0 / 60.0);
        let tf = app.world().get::<Transform>(remote).unwrap();
        assert!(tf.translation.x > 0.0);
        assert!(Quat::IDENTITY.angle_between(tf.rotation) < 1.0);
        assert!((tf.rotation * Vec3::Z).x < 0.0, "turn toward the record's heading, not its travel direction");

        let mut r = app.world_mut().get_mut::<RemotePlayer>(remote).unwrap();
        r.target = Vec3::X * 20.0;
        r.facing = -std::f32::consts::FRAC_PI_2;
        step(&mut app, 1.0 / 60.0);
        let tf = app.world().get::<Transform>(remote).unwrap();
        assert_eq!(tf.translation, Vec3::X * 20.0);
        assert!((tf.rotation * Vec3::Z).distance(-Vec3::X) < 1e-5);
        let mut r = app.world_mut().get_mut::<RemotePlayer>(remote).unwrap();
        r.facing = 0.0;
        r.held = 0.5;
        step(&mut app, 1.0 / 60.0);
        assert!((app.world().get::<Transform>(remote).unwrap().rotation * Vec3::Z).distance(Vec3::Z) < 1e-5);
    }
}

/// The networking and local-player systems; crate::skills runs after them (its
/// camera shake on top of the follow camera).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetSystems;

pub struct NetPlugin;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetReceive;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RespawnChoice>();
        // DSOR_ZOOM=<height>: start at another zoom (debugging screenshots).
        let zoom = std::env::var("DSOR_ZOOM").ok().and_then(|z| z.parse().ok()).unwrap_or(CAM_HEIGHT_DEFAULT);
        app.insert_resource(CameraZoom::new(zoom))
            .add_systems(Startup, connect.run_if(resource_exists::<NetConfig>))
            .add_systems(PreUpdate, finish_login.run_if(resource_exists::<LoggingIn>))
            .add_systems(
                PreUpdate,
                pump.in_set(NetReceive).run_if(net_exists),
            )
            .add_systems(
                Update,
                (debug_hierarchy, spawn_local, apply_remotes, dress, move_remotes, player_deaths,
                    click_to_move.run_if(crate::loading::gameplay_ready), walk_local.run_if(crate::loading::gameplay_ready),
                    apply_corrections, zoom_camera.run_if(crate::loading::gameplay_ready), follow_camera, send_moves.run_if(crate::loading::gameplay_ready))
                    .chain()
                    .in_set(NetSystems)
                    .run_if(net_exists),
            );
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Update, replay_equipment.before(NetSystems).run_if(net_exists));
    }
}

pub fn now_ms(time: &Time<Real>) -> u64 {
    (time.elapsed_secs_f64() * 1000.0) as u64
}

/// The local actor of the offline character.
pub const OFFLINE_ACTOR: u32 = 0x5FFF_FFFF;

/// A class's starting quick bar for the offline character (left button, right
/// button, keys 1-5).
pub fn default_bar(class: u8) -> Vec<Option<String>> {
    let ids: &[&str] = match class {
        0 => &["angrystrike", "mightyswing", "mightybash", "battlecry", "enragingleap", "seismicslam", "bloody360"],
        2 => &["markshot", "multishot", "stab", "explosiveshot", "jump", "stunshot", "whirlwind"],
        3 => &["SimpleShot", "HeavyShot", "Grenade", "TurretCombat", "HoverJump", "ShrapnelShot", "SteamConductor"],
        _ => &["magicmissile", "fireball", "frostnova", "iceball", "teleport", "lightningstrike", "frostwind"],
    };
    ids.iter().map(|s| Some((*s).to_owned())).collect()
}

/// Offline play (no server): a Net that connects nowhere, and a character of
/// this class and gender. Switching later: crate::debug sets `offline_pending`.
pub fn start_offline(world: &mut World, class: u8, gender: u8) {
    let now = now_ms(world.resource::<Time<Real>>());
    let credentials = Credentials::parse("1", "00000000000000000000000000000000").expect("fixed credentials");
    let session = Session::new(std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 9), credentials, 1, now);
    world.insert_resource(NetActive);
    world.insert_non_send(Net {
        offline: true,
        offline_pending: Some(CharacterDesc { class, gender, ..default() }),
        session,
        transport: None,
        clock_offset: Some(0),
        relay: None,
        wanted_character: None,
        new_character: (String::new(), 0, 0),
        local_actor: Some(OFFLINE_ACTOR),
        centre: None,
        pending_local: None,
        requested: Default::default(),
        remote_spawns: Vec::new(),
        remote_gone: Vec::new(),
        remote_moves: Vec::new(),
        local_appearance_dirty: false,
        remote_redress: HashMap::new(),
        npc_spawns: Vec::new(),
        npc_gone: Vec::new(),
        props: default(),
        map_markers: default(),
        monster_spawns: Vec::new(),
        max_health: None,
        level: Some(1),
        xp: None,
        wallet: None,
        monster_health: Vec::new(),
        hits: Vec::new(),
        kills: Vec::new(),
        death: default(),
        revives: Vec::new(),
        player_deaths: Vec::new(),
        bar: default_bar(class),
        quickbars: default(),
        inventory: default(),
        corrections: default(),
        skill_book: None,
        skill_events: Vec::new(),
        status_events: Vec::new(),
        location_events: Vec::new(),
        named_status_events: Vec::new(),
        location_gone: Vec::new(),
        local_name: Some("Debug".into()),
        resource: None,
        health: None,
    });
    // Explicit offline packet replay for UI inspection, with no account mutation.
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(path) = std::env::var("DSOR_INVENTORY_PREVIEW") {
        match std::fs::read(path).ok().and_then(|b| dsor_proto::commands::decode_message(&b, b.len() * 8).ok()) {
            Some(commands) => {
                let mut net = world.non_send_mut::<Net>();
                for (command, actor) in commands {
                    if matches!(command, ServerCommand::InventoryInfo(_) | ServerCommand::MembershipUpdate(_) | ServerCommand::ItemUpdate(_)) {
                        on_command(&mut net, command, Some(OFFLINE_ACTOR));
                    } else if matches!(command, ServerCommand::NewRemotePlayer(_) | ServerCommand::RemotePlayerInfo(_)) {
                        on_command(&mut net, command, actor);
                    }
                }
            }
            None => warn!("DSOR_INVENTORY_PREVIEW could not read an inventory packet"),
        }
    }
}

/// Explicit offline regression replay, scheduled after the initial outfits load.
/// Records are {"frame": N, "hex": server_packet, "bits": exact_bit_count}.
#[cfg(not(target_arch = "wasm32"))]
fn replay_equipment(
    mut net: NonSendMut<Net>,
    characters: Query<&Character>,
    loading: Res<crate::loading::LoadingScreen>,
    mut loaded: Local<bool>,
    mut records: Local<std::collections::VecDeque<(u32, Vec<(ServerCommand, Option<u32>)>)>>,
    mut frame: Local<u32>,
) {
    if !net.offline { return; }
    if !*loaded {
        *loaded = true;
        if let Ok(path) = std::env::var("DSOR_EQUIPMENT_REPLAY") {
            let values = std::fs::read(path).ok().and_then(|data| serde_json::from_slice::<Vec<serde_json::Value>>(&data).ok());
            for record in values.into_iter().flatten() {
                let Some(hex) = record["hex"].as_str() else { continue };
                let data: Option<Vec<u8>> = hex.as_bytes().chunks(2).map(|b| std::str::from_utf8(b).ok().and_then(|s| u8::from_str_radix(s, 16).ok())).collect();
                if let Some(data) = data {
                    if let Ok(commands) = dsor_proto::commands::decode_message(&data, record["bits"].as_u64().unwrap_or((data.len() * 8) as u64) as usize) {
                        records.push_back((record["frame"].as_u64().unwrap_or(0) as u32, commands));
                    }
                }
            }
            records.make_contiguous().sort_by_key(|r| r.0);
        }
    }
    if records.is_empty() { return; }
    if *frame == 0 && (loading.active() || characters.is_empty() || !characters.iter().all(Character::outfit_ready)) { return; }
    *frame += 1;
    while records.front().is_some_and(|r| r.0 <= *frame) {
        let (_, commands) = records.pop_front().unwrap();
        info!("equipment replay frame {}", *frame);
        for (command, actor) in commands {
            if matches!(command, ServerCommand::InventoryInfo(_) | ServerCommand::ItemUpdate(_)) {
                on_command(&mut net, command, Some(OFFLINE_ACTOR));
            } else if matches!(command, ServerCommand::RemotePlayerInfo(_)) {
                on_command(&mut net, command, actor);
            }
        }
    }
}

/// The announcements of one batch, the last one per actor.
/// FAILURE: the server announces each actor several times ("npc vicinity ...
///   repeat 6 of 6"), and a batch held until the outfits loaded spawned every
///   copy (the despawn of the existing one sees no entity spawned in the same
///   loop): 246 dressed NPCs for Kingshill's 41, 1 FPS online against 60 offline.
pub fn last_per_actor<T>(batch: Vec<T>, actor: impl Fn(&T) -> u32) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<T> = batch.into_iter().rev().filter(|x| seen.insert(actor(x))).collect();
    out.reverse();
    out
}

/// Marks that `Net` exists, for run conditions.
/// CONTRACT: run conditions must not read the NonSend `Net` itself: bevy 0.19 runs
///   them on worker threads, and the game aborted at random ("Attempted to access
///   or drop non-send resource ... from thread", segfaults).
#[derive(Resource)]
pub struct NetActive;

/// Lets the in-client login screen recover from a refused game connection.
#[derive(Resource)]
pub struct ConnectionFailure(pub String);

fn net_exists(active: Option<Res<NetActive>>) -> bool {
    active.is_some()
}

/// A login at the account service in flight (crate::net::connect).
#[derive(Resource)]
struct LoggingIn;

/// Exclusive: a non-send resource can only be inserted through the World.
/// With a user name, log in at the account service first (finish_login connects).
fn connect(world: &mut World) {
    let Some(config) = world.get_resource::<NetConfig>().cloned() else { return };
    if let Some(user) = config.user.clone() {
        let api = config.account_api.clone().unwrap_or_else(|| {
            let host = config.login.rsplit_once(':').map(|(h, _)| h).unwrap_or(&config.login);
            format!("{host}:8080")
        });
        match dsor_transport::AccountLogin::start(&api, config.relay.as_deref(), &user, config.password.as_deref().unwrap_or("")) {
            Ok(login) => {
                info!("logging in as {user} at {api}");
                world.insert_non_send(login);
                world.insert_resource(LoggingIn);
            }
            Err(e) => error!("cannot log in at {api}: {e}"),
        }
        return;
    }
    start_session(world, config);
}

/// The account service's answer: the session to play with, or why not.
/// EVIDENCE: dso-server-production services/accounts.py:254-284 (`POST /api/login`
///   -> {"session": {"session_id", "account_id", ...}, "launch": {...}}; errors
///   {"error": ...} with 401 / 403 / 429).
fn finish_login(world: &mut World) {
    let Some(mut login) = world.get_non_send_mut::<dsor_transport::AccountLogin>() else { return };
    let Some(answer) = login.poll() else { return };
    world.remove_non_send::<dsor_transport::AccountLogin>();
    world.remove_resource::<LoggingIn>();
    let parsed = answer.and_then(|(status, body)| {
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("HTTP {status}: {e}: {body}"))?;
        if status != 200 {
            return Err(format!("HTTP {status}: {}", v["error"].as_str().unwrap_or(&body)));
        }
        let s = &v["session"];
        match (s["account_id"].as_u64(), s["session_id"].as_str()) {
            (Some(a), Some(sid)) => Ok((a.to_string(), sid.to_string(), v["launch"].clone())),
            _ => Err(format!("no session in {body}")),
        }
    });
    match parsed {
        Ok((account, session, launch)) => {
            info!("logged in: account {account}");
            let Some(mut config) = world.get_resource::<NetConfig>().cloned() else { return };
            config.account = account;
            config.session = session;
            config.chat = Some(chat_target(&launch, &config.login));
            world.insert_resource(config.clone());
            start_session(world, config);
        }
        Err(e) => error!("login refused: {e}"),
    }
}

pub(crate) fn start_session(world: &mut World, config: NetConfig) {
    world.resource_mut::<crate::loading::LoadingScreen>().connecting();
    world.remove_resource::<ConnectionFailure>();
    let now = now_ms(world.resource::<Time<Real>>());
    let Some(credentials) = Credentials::parse(&config.account, &config.session) else {
        error!("invalid account or session received from the account service");
        return;
    };
    let Some(login) = dsor_proto::session::resolve(&config.login, 2190) else {
        error!("cannot resolve the login server {}", config.login);
        world.insert_resource(ConnectionFailure("Cannot resolve the game server.".into()));
        return;
    };
    // One RakNet GUID per run, as the real client keeps one for all its connections.
    let guid = 0x0660_0000_0000_0000 | (credentials.account as u64) << 8 | 0x42;
    let session = Session::new(login, credentials, guid, now);
    world.insert_resource(NetActive);
    world.insert_non_send(Net {
        offline: false,
        offline_pending: None,
        session,
        transport: None,
        clock_offset: None,
        relay: config.relay.clone(),
        wanted_character: config.character,
        new_character: config
            .new_character
            .clone()
            .unwrap_or_else(|| (config.user.clone().unwrap_or_else(|| "Hero".into()), 0, 0)),
        local_actor: None,
        centre: None,
        pending_local: None,
        requested: Default::default(),
        remote_spawns: Vec::new(),
        remote_gone: Vec::new(),
        remote_moves: Vec::new(),
        local_appearance_dirty: false,
        remote_redress: HashMap::new(),
        npc_spawns: Vec::new(),
        npc_gone: Vec::new(),
        props: crate::props::NetworkProps::online(),
        map_markers: default(),
        monster_spawns: Vec::new(),
        max_health: None,
        level: None,
        xp: None,
        wallet: None,
        monster_health: Vec::new(),
        hits: Vec::new(),
        kills: Vec::new(),
        death: default(),
        revives: Vec::new(),
        player_deaths: Vec::new(),
        bar: Vec::new(),
        quickbars: default(),
        inventory: default(),
        corrections: default(),
        skill_book: None,
        skill_events: Vec::new(),
        status_events: Vec::new(),
        location_events: Vec::new(),
        named_status_events: Vec::new(),
        location_gone: Vec::new(),
        local_name: None,
        resource: None,
        health: None,
    });
}

/// CONTRACT: leaving says goodbye. FAILURE: closing the window left the player in
///   the world for the server's 60 s silence timeout, so the next login found a
///   double of itself standing on the arrival point (2026-10-06).
impl Drop for Net {
    fn drop(&mut self) {
        // Any time will do: the goodbye is flushed at once.
        self.session.disconnect(u64::MAX / 2);
        let mut out = Vec::new();
        self.session.drain_outgoing(&mut out);
        if let Some(t) = self.transport.as_mut() {
            for d in &out {
                let _ = t.send(d);
            }
        }
    }
}

impl Net {
    /// An actor leaves (a DiscardMonster, or a corpse whose time is over:
    /// crate::monsters::remove_corpses).
    pub(crate) fn discard_actor(&mut self, actor: u32) {
        self.corrections.pending.remove(&actor);
        self.requested.remove(&actor);
        self.remote_gone.push(actor);
    }

    /// The server's game tick now.
    pub fn server_tick(&self, now: u64) -> u32 {
        let local = self.session.local_tick(now) as i64;
        (local + self.clock_offset.unwrap_or(0)).max(0) as u32
    }

    pub fn wire_to_game(&self, x: i16, elevation: i16, y: i16) -> Vec3 {
        let c = self.centre.unwrap_or(Vec3::ZERO);
        Vec3::new(x as f32 / 128.0, elevation as f32 / 128.0, y as f32 / 128.0) + c
    }

    pub fn game_to_wire(&self, p: Vec3) -> (i16, i16, i16) {
        let c = self.centre.unwrap_or(Vec3::ZERO);
        let w = (p - c) * 128.0;
        let clamp = |v: f32| v.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        (clamp(w.x), clamp(w.y), clamp(w.z))
    }

    /// A game command to the map server, reliable ordered.
    fn skill_event(&mut self, actor: Option<u32>, base: &dsor_proto::commands::combat::SkillBase, points: Vec<[f32; 3]>) {
        let Some(actor) = actor else { return };
        if Some(actor) == self.local_actor {
            return;
        }
        self.skill_events.push(SkillEvent { actor, wire: base.skill_id, heading: base.heading, points, start_tick: base.start_tick });
    }

    /// Put the local player (back) on the map: spawned by spawn_local, the old one
    /// removed (crate::debug's character switch).
    pub fn queue_local(&mut self, desc: CharacterDesc, at: Vec3, facing: f32) {
        let name = self.local_name.clone().unwrap_or_else(|| "Debug".into());
        self.pending_local = Some((desc, at, facing, name, false));
    }

    /// Who to log in to the chat as, once a character plays on a map: the account,
    /// the character and the login session (crate::chat).
    pub fn chat_identity(&self) -> Option<(u32, u32, [u8; 16])> {
        if self.offline || self.session.stage() != dsor_proto::session::Stage::Map {
            return None;
        }
        let credentials = self.session.credentials();
        Some((credentials.account, self.session.character()?, credentials.session))
    }

    /// The browser's WebSocket relay, for the other connections (crate::chat).
    pub fn relay(&self) -> Option<&str> {
        self.relay.as_deref()
    }

    pub fn send(&mut self, command: &ClientCommand) {
        if self.offline {
            return;
        }
        let (bytes, bits) = command.encode();
        self.session.send_command(&bytes, bits);
    }
}

#[allow(clippy::too_many_arguments)]
fn pump(
    mut commands: Commands,
    mut net: NonSendMut<Net>,
    time: Res<Time<Real>>,
    current: Option<Res<CurrentMap>>,
    roots: Query<Entity, With<MapRoot>>,
    assets: Res<AssetServer>,
    manifests: Res<Assets<MapManifest>>,
    left_behind: Query<Entity, Or<(With<crate::npc::Npc>, With<RemotePlayer>, With<LocalPlayer>)>>,
    mut loading: ResMut<crate::loading::LoadingScreen>,
    mut selection: ResMut<crate::selection::Selection>,
) {
    let now = now_ms(&time);
    let mut changed_map = false;
    let net = &mut *net;
    for request in std::mem::take(&mut selection.requests) {
        use crate::selection::Request;
        match request {
            Request::Preview(id) => net.session.character_operation(8, id, now),
            Request::Play(id) => { net.session.select_character(id, now); loading.connecting(); }
            Request::OpenEditor => net.session.open_character_editor(now),
            Request::Back => net.session.character_operation(2, 0, now),
            Request::Create(name, class, gender) => net.session.create_character(&name, class, gender, now),
            Request::Delete(id) => net.session.character_operation(6, id, now),
        }
    }
    if net.offline {
        if net.centre.is_none() {
            if let Some(manifest) = current.as_ref().and_then(|c| manifests.get(&c.manifest)) {
                net.centre = Some(Vec3::from(manifest.center));
            }
        }
        // CONTRACT: taken only once the centre is known (taken before, it was lost).
        if let Some(c) = net.centre.filter(|_| net.offline_pending.is_some()) {
            let desc = net.offline_pending.take().expect("checked");
            info!("offline: playing class {} at {c:?}", desc.class);
            net.pending_local = Some((desc, c, 0.0, "Debug".into(), false));
        }
        return;
    }
    if let Some(t) = net.transport.as_mut() {
        let mut inbox = Vec::new();
        t.poll(&mut inbox);
        for d in inbox {
            net.session.receive(&d, now);
        }
    }
    net.session.update(now);
    while let Some(event) = net.session.poll_event() {
        match event {
            SessionEvent::Connect { target, farewell } => {
                loading.connecting();
                if let Some(t) = net.transport.as_mut() {
                    for d in &farewell {
                        let _ = t.send(d);
                    }
                }
                match Transport::connect(&target.to_string(), net.relay.as_deref()) {
                    Ok(t) => net.transport = Some(t),
                    Err(e) => {
                        error!("cannot reach {target}: {e}");
                        commands.insert_resource(ConnectionFailure("Cannot reach the game server. Sign in again to retry.".into()));
                    }
                }
                info!("connecting to {target}");
            }
            SessionEvent::Service(name) => info!("service: {name}"),
            SessionEvent::Roster(roster) => {
                info!("characters: {:?}", roster.iter().map(|e| (&e.name, e.character, e.level)).collect::<Vec<_>>());
            }
            SessionEvent::CharacterSelection(reply) => {
                if reply.operation == 4 { loading.cancel(); }
                selection.receive(reply, net.wanted_character);
            }
            SessionEvent::CharacterEditor => {
                selection.editor(&net.new_character);
            }
            SessionEvent::CreationRefused(reason) => {
                selection.creation_refused(reason);
            }
            SessionEvent::MapAssigned { name, rule_set } => {
                net.props.reset_map();
                net.map_markers.reset_map();
                selection.active = name == crate::selection::MAP;
                if !selection.active { selection.entering = false; selection.dialog = crate::selection::Dialog::None; }
                net.corrections = default();
                loading.map(name.clone());
                info!("map {name} (rule set {rule_set})");
                if current.as_ref().is_some_and(|c| c.name == name) {
                    continue;
                }
                for root in &roots {
                    commands.entity(root).despawn();
                }
                // The old map's NPCs and players go with it: the new map server has
                // its own actors, and nobody would ever say these had left.
                for e in &left_behind {
                    commands.entity(e).despawn();
                }
                net.requested.clear();
                net.remote_spawns.clear();
                net.remote_moves.clear();
                net.remote_gone.clear();
                net.remote_redress.clear();
                net.npc_spawns.clear();
                net.npc_gone.clear();
                net.monster_spawns.clear();
                net.monster_health.clear();
                net.hits.clear();
                net.kills.clear();
                net.revives.clear();
                net.player_deaths.clear();
                net.death = default();
                // Nothing may stand on the old map's ground (the new player snaps to
                // the first mesh it sees); crate::nav loads the new one.
                commands.remove_resource::<CurrentNav>();
                net.centre = None;
                changed_map = true;
                commands.insert_resource(CurrentMap::new(name.clone(), assets.load(format!("maps/{name}.map.json"))));
            }
            SessionEvent::Clock(tick) => {
                let local = net.session.local_tick(now) as i64;
                net.clock_offset = Some(tick as i64 - local);
            }
            SessionEvent::Commands { data, bits } => {
                // What a chain held before a command we cannot read still counts
                // (the player initialization batch starts with NewPlayer).
                let (list, error) = dsor_proto::commands::decode_message_partial(&data, bits);
                for (command, actor) in list {
                    on_command(net, command, actor);
                }
                if let Some(e) = error {
                    warn!("undecodable {:#04x} message ({} bytes, {bits} bits): {e}", data.first().copied().unwrap_or(0), data.len());
                    // Natively, keep it to study: dsor-undecodable-<bits>.bin in the working directory.
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let path = format!("dsor-undecodable-{bits}.bin");
                        if std::fs::write(&path, &data).is_ok() {
                            warn!("  saved to {path}");
                        }
                    }
                }
            }
            SessionEvent::Disconnected(reason) => {
                warn!("disconnected: {reason:?}");
                commands.insert_resource(ConnectionFailure("The game server disconnected. Sign in again to retry.".into()));
            }
        }
    }
    // CONTRACT: not in the pass that changed map: `current` is still the OLD map
    //   (the new CurrentMap is inserted at the end of this system).
    // FAILURE: the old map's centre was taken at once and kept: every wire position
    //   on the new map was off by the difference of the two centres (Kingshill to
    //   a0302_wildforest: monsters 16 units up in the air, out of view).
    if net.centre.is_none() && !changed_map {
        if let Some(manifest) = current.as_ref().and_then(|c| manifests.get(&c.manifest)) {
            net.centre = Some(Vec3::from(manifest.center));
        }
    }
    let mut out = Vec::new();
    net.session.drain_outgoing(&mut out);
    if let Some(t) = net.transport.as_mut() {
        for d in &out {
            let _ = t.send(d);
        }
    }
}

fn on_command(net: &mut Net, command: ServerCommand, actor: Option<u32>) {
    net.props.receive(&command);
    net.map_markers.receive(&command, actor);
    match command {
        ServerCommand::NewPlayer(p) => {
            net.inventory = default();
            net.local_appearance_dirty = false;
            net.corrections = default();
            let desc = CharacterDesc {
                class: p.parts[0],
                gender: p.parts[1],
                equipment: Vec::new(),
                // The client tracks its own armament from the inventory; 0 is what
                // the server sends (dsor/newplayer.py DEFAULT_ARMAMENT).
                armament: p.rank.max(0),
                look: None,
                cosmetics: Vec::new(),
            };
            info!("I am {} ({}), actor {:?}, at {:?}", p.name, desc.animation_set(), actor, p.position);
            net.local_actor = actor;
            // release261: NewPlayer carries no health or resource; InventoryInfo
            // ends with both (production inventory_codec.py:476-477).
            net.level = Some(p.words_240[1]);
            net.xp = Some((p.words_240[2], p.words_240[3], p.words_240[4]));
            net.wallet = Some(p.currencies);
            net.local_name = Some(p.name.clone());
            // The third leading bool is the admin byte (OverheadAdminColor name).
            let admin = p.leading_flags[2];
            net.pending_local = Some((desc, Vec3::from(p.position), p.heading, p.name.clone(), admin));
        }
        // The real client asks about every actor the vicinity names
        // (session-walk4: a burst of 8b/34 after each 0x85/125).
        ServerCommand::ActorsEnterVicinity(v) => {
            debug!("vicinity +{:x?}", v.actors);
            for a in v.actors {
                if Some(a) != net.local_actor && net.requested.insert(a) {
                    net.send(&ClientCommand::ActorRequest(dsor_proto::commands::player::ActorRequest { actor: a }));
                }
            }
        }
        ServerCommand::ActorsLeftVicinity(v) => {
            for a in v.actors {
                net.remote_redress.remove(&a);
                net.remote_spawns.retain(|p| p.0 != a);
                net.requested.remove(&a);
                net.remote_gone.push(a);
                net.npc_gone.push(a);
            }
        }
        ServerCommand::NewRemotePlayer(p) => {
            let Some(actor) = actor else { return };
            // Our own character from a previous session the server has not timed
            // out yet (a page reload): it stood frozen where we had been.
            // FAILURE (2026-10-06): "l'ancien corps reste stuck avec l'animation".
            if net.local_name.as_deref() == Some(p.name.as_str()) {
                info!("{} (actor {actor:#x}) is our own previous session; not drawn", p.name);
                return;
            }
            let equipment = appearance::normalize(p.equipment);
            let armament = appearance::remote_armament(&equipment, "");
            let desc = CharacterDesc {
                class: p.parts[0],
                gender: p.parts[1],
                equipment,
                armament,
                look: None,
                cosmetics: Vec::new(),
            };
            // A spawn is a newer complete snapshot than any pending update.
            net.remote_redress.remove(&actor);
            info!("{} ({}) is here, actor {actor:#x}, {} worn slot(s)", p.name, desc.animation_set(), desc.equipment.len());
            net.remote_spawns.push((actor, desc, Vec3::from(p.position), p.heading, p.name, p.flags[2]));
        }
        ServerCommand::ActorStatsUpdate(v) => {
            if actor.is_some() && actor == net.local_actor {
                net.resource = Some(v.resource);
                net.health = Some(v.health as f32);
            }
        }
        ServerCommand::QuickSlotsInfo(q) => {
            net.quickbars = q.clone();
            if let Some(first) = q.bars.first() {
                net.bar = first.iter().map(|s| if s.kind == 0 { s.id.clone() } else { None }).collect();
                info!("quick slots: {:?}", net.bar.iter().flatten().collect::<Vec<_>>());
            }
        }
        ServerCommand::SkillBookInfo(book) => net.skill_book = Some(book.clone()),
        // Skills the server relays from other actors (our own are played as cast).
        ServerCommand::StatusEffect(c) => {
            for e in &c.elements {
                if actor.is_some() && actor == net.local_actor {
                    info!("status effect {} on us for {:.2} s (instance {})", e.index, e.duration as f32 / 25.0, e.instance);
                }
                net.status_events.push(StatusEvent {
                    // release261: the holder is the command's actor (e.source is
                    // who caused it).
                    holder: actor.unwrap_or(e.source),
                    index: e.index,
                    instance: e.instance,
                    seconds: e.duration as f32 / 25.0,
                });
            }
        }
        ServerCommand::NewLocationEffect(c) => {
            net.location_events.push(LocationEvent {
                id: c.effect_id,
                status: c.status_effect.clone(),
                position: Vec3::from(c.position),
                heading: c.heading,
                seconds: if c.endless { None } else { Some(c.end_tick.saturating_sub(c.start_tick) as f32 / 25.0) },
            });
        }
        ServerCommand::DiscardLocationEffect(c) => net.location_gone.push(c.effect_id),
        ServerCommand::Skill(c) => net.skill_event(actor, &c.base, Vec::new()),
        ServerCommand::TargetSkill(c) => net.skill_event(actor, &c.base, Vec::new()),
        ServerCommand::BulletSkill(c) => net.skill_event(actor, &c.base, Vec::new()),
        // Monsters' aimed projectiles (release261: the server sends them too).
        ServerCommand::TargetBulletSkill(c) => net.skill_event(actor, &c.base, c.points.clone()),
        ServerCommand::TargetPointBulletSkill(c) => net.skill_event(actor, &c.base, c.points.clone()),
        ServerCommand::ShiftedSkill(c) => net.skill_event(actor, &c.base, c.points.clone()),
        ServerCommand::NewNpc(n) => {
            let Some(actor) = actor else { return };
            let guid: String = n.guid.iter().map(|b| format!("{b:02X}")).collect();
            debug!("npc {} ({guid}) actor {actor:#x} at {:?}", n.name, n.position);
            net.npc_spawns.push(NpcSpawn {
                actor,
                template: n.name,
                guid,
                position: Vec3::from(n.position),
                visible: n.visible,
            });
        }
        ServerCommand::NewMonster(m) => {
            let Some(actor) = actor else { return };
            debug!("monster {} actor {actor:#x} at {:?}", m.blueprint, m.position);
            net.monster_spawns.push(MonsterSpawn {
                actor,
                blueprint: m.blueprint,
                position: Vec3::from(m.position),
                health: sat(m.health),
                level: m.level,
            });
        }
        ServerCommand::DiscardMonster(_) => {
            if let Some(a) = actor {
                net.discard_actor(a);
            }
        }
        ServerCommand::MonsterUpdate(u) => {
            if let Some(a) = actor {
                net.monster_health.push((a, sat(u.health), sat(u.max_health)));
            }
        }
        ServerCommand::PlayerLevelUpdate(u) => {
            net.level = Some(u.level);
        }
        ServerCommand::XpChanged(x) => {
            net.level = Some(x.level);
            net.xp = Some((x.total, x.floor, x.ceiling));
        }
        ServerCommand::CurrencyChanged(c) => {
            net.wallet = Some(c.wallet);
        }
        ServerCommand::Hit(h) => {
            if actor.is_some() && actor == net.local_actor && h.victim_max_health > 0 {
                net.max_health = Some(h.victim_max_health as f32);
                net.health = Some(h.victim_health.max(0) as f32);
            }
            if let Some(a) = actor {
                net.hits.push((a, h));
            }
        }
        ServerCommand::Kill(k) => {
            if let Some(a) = actor {
                if Some(a) == net.local_actor {
                    net.death = DeathState { dead: true, ..default() };
                }
                net.player_deaths.push(a);
                net.kills.push((a, k));
            }
        }
        ServerCommand::ShowDeathDialog(_) => {
            if net.death.dead {
                net.death.dialog = true;
            }
        }
        ServerCommand::Revive(_) => {
            if let Some(a) = actor {
                if Some(a) == net.local_actor {
                    let chosen = net.death.chosen;
                    net.death = DeathState { chosen, ..default() };
                }
                net.revives.push(a);
            }
        }
        ServerCommand::ItemUpdate(update) => {
            if actor.is_some() && actor != net.local_actor { return; }
            if let Some(item) = net.inventory.items.get_mut(&update.item) {
                item.third = update.third;
                item.tenth = update.tenth.min(255) as u8;
                item.durability = update.level;
                item.fifty_second = update.unknown_u32_0;
                item.flag_0 = update.flags[0]; item.flag_1 = update.flags[1];
                item.twentieth = update.twentieth; item.twenty_eighth = update.twenty_eighth;
                item.names = update.unknown_strings_0;
                item.fourth_name = update.appearance; item.flag_2 = update.flag_2;
                net.inventory.revision = net.inventory.revision.wrapping_add(1);
                net.local_appearance_dirty = true;
            }
        }
        ServerCommand::MembershipUpdate(m) => {
            if actor.is_none() || actor == net.local_actor { net.inventory.membership(m.tier, m.active); }
        }
        ServerCommand::InventoryInfo(inv) => {
            // release261: the health and skill resource come here, not in
            // NewPlayer, so a skill the character cannot pay for is never cast.
            if actor.is_none() || actor == net.local_actor {
                net.health = Some(inv.health);
                net.resource = Some(inv.resource);
            }
            if actor.is_some() && actor != net.local_actor { return; }
            net.inventory.apply(inv);
            net.local_appearance_dirty = true;
        }
        ServerCommand::RemotePlayerInfo(p) => {
            if let Some(a) = actor {
                let equipment = appearance::normalize(p.equipment);
                let armament = appearance::remote_armament(&equipment, &p.animation_set);
                net.remote_redress.insert(a, (equipment, armament));
            }
        }
        ServerCommand::DiscardPlayer(_) => {
            if let Some(a) = actor {
                net.remote_redress.remove(&a);
                net.remote_spawns.retain(|p| p.0 != a);
                net.corrections.pending.remove(&a);
                net.requested.remove(&a);
                net.remote_gone.push(a);
            }
        }
        ServerCommand::ForceTransform(t) => {
            if let Some(a) = actor {
                net.corrections.force(a, Vec3::from(t.position), t.heading, Some(a) == net.local_actor);
            }
        }
        ServerCommand::Move(m) => {
            if let Some(a) = actor {
                if Some(a) != net.local_actor {
                    let at = net.wire_to_game(m.x, m.elevation, m.y);
                    let facing = m.facing as f32 / 256.0 * TAU;
                    if std::env::var("DSOR_DEBUG_MOVES").is_ok() {
                        info!("move record {a} tick {} at ({:.2}, {:.2}) heading {} facing {} speed {} duration {}",
                            m.start_tick, at.x, at.z, m.heading, m.facing, m.speed, m.duration);
                    }
                    net.remote_moves.push((a, at, facing, m.start_tick));
                } else if net.corrections.corrected_echo(&m) {
                    let at = net.wire_to_game(m.x, m.elevation, m.y);
                    let facing = m.facing as f32 / 256.0 * TAU;
                    net.corrections.force(a, at, facing, true);
                }
            }
        }
        _ => {}
    }
}

fn apply_corrections(
    mut net: NonSendMut<Net>,
    mut actors: Query<(&mut Transform, Option<&mut LocalPlayer>, Option<&mut RemotePlayer>, &mut CharacterAnim), Or<(With<LocalPlayer>, With<RemotePlayer>)>>,
) {
    for (mut transform, local, remote, mut animation) in &mut actors {
        let actor = local.as_ref().map(|p| p.actor).or_else(|| remote.as_ref().map(|p| p.actor));
        let Some((position, heading)) = actor.and_then(|a| net.corrections.pending.remove(&a)) else { continue };
        transform.translation = position;
        transform.rotation = Quat::from_rotation_y(heading);
        // A revival's ForceTransform lands while its Resurrect plays and holds
        // the player (crate::skills::hold_for_status): kept, not cut to Idle
        // ("mon perso retourne instant en idle").
        let held = local.as_ref().is_some_and(|p| p.casting > 0.0) && matches!(animation.state, AnimState::Named(_));
        if let Some(mut p) = local { p.target = None; p.facing = heading; if !held { p.casting = 0.0; } p.snapped = true; }
        if let Some(mut p) = remote { p.target = position; p.facing = heading; p.held = 0.0; p.held_until_tick = None; p.still_for = 1.0; }
        if !animation.dead && !held { animation.state = AnimState::Idle; }
    }
}

fn apply_remotes(
    mut commands: Commands,
    mut net: NonSendMut<Net>,
    mut remotes: Query<(Entity, &mut RemotePlayer)>,
    nav: Option<Res<CurrentNav>>,
    meshes: Res<Assets<NavMesh>>,
) {
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    for actor in std::mem::take(&mut net.remote_gone) {
        for (e, r) in &remotes {
            if r.actor == actor {
                commands.entity(e).despawn();
            }
        }
    }
    for (actor, desc, at, heading, name, admin) in last_per_actor(std::mem::take(&mut net.remote_spawns), |x| x.0) {
        for (e, r) in &remotes {
            if r.actor == actor {
                commands.entity(e).despawn();
            }
        }
        let at = mesh.and_then(|m| m.nearest(at, 4.0)).unwrap_or(at);
        let e = spawn_character(&mut commands, desc, Transform::from_translation(at).with_rotation(Quat::from_rotation_y(heading)));
        commands.entity(e).insert((
            Nameplate::player(name, admin),
            RemotePlayer { actor, target: at, facing: heading, still_for: 1.0, held: 0.0, held_until_tick: None, ..default() },
        ));
    }
    for (actor, at, facing, tick) in std::mem::take(&mut net.remote_moves) {
        for (_, mut r) in &mut remotes {
            // A record from before the end of its skill's hold: its ticks only
            // (RemotePlayer::held_until_tick).
            let held = r.held_until_tick.is_some_and(|end| end.wrapping_sub(tick) as i32 >= 0);
            if r.actor == actor && !held {
                let at = mesh.and_then(|m| m.ground(at.x, at.z, at.y, 3.0).map(|h| Vec3::new(at.x, h, at.z))).unwrap_or(at);
                if let Some((before, then)) = r.last_record {
                    let ticks = tick.wrapping_sub(then) as i32;
                    if ticks > 0 {
                        let moved = Vec2::new(at.x - before.x, at.z - before.z).length();
                        let speed = moved / (ticks as f32 * TICK_SECONDS);
                        // A record that moved: its speed (a stop keeps the last one,
                        // for the next start); a teleport is no speed.
                        if moved > 0.02 && speed < 25.0 {
                            r.speed = if r.speed > 0.0 { r.speed * 0.5 + speed * 0.5 } else { speed };
                            r.interval = (ticks as f32 * TICK_SECONDS).min(0.5);
                        }
                    }
                }
                r.last_record = Some((at, tick));
                r.target = at;
                r.facing = facing;
            }
        }
    }
}

/// Other players run toward their last reported position at the run speed, and
/// snap when they are far off (a teleport, a map entry).
fn move_remotes(time: Res<Time>, mut remotes: Query<(&mut Transform, &mut RemotePlayer, &mut CharacterAnim, Has<crate::monsters::Monster>)>) {
    for (mut tf, mut r, mut anim, monster) in &mut remotes {
        if anim.dead {
            continue;
        }
        if r.held > 0.0 {
            r.held -= time.delta_secs();
            tf.rotation = Quat::from_rotation_y(r.facing);
            continue;
        }
        let to = r.target - tf.translation;
        let flat = Vec2::new(to.x, to.z).length();
        // The gap to its record covered in one record interval: as fast as it
        // moves when a record has just arrived, a little slower as it closes in,
        // so it is still walking when the next one lands; faster when late.
        let step = if r.speed > 0.0 && r.interval > 0.0 {
            (flat / r.interval).clamp(r.speed * 0.8, r.speed * 3.0) * time.delta_secs()
        } else {
            RUN_SPEED * 1.25 * time.delta_secs()
        };
        let moving = flat > 0.05;
        if flat > 8.0 {
            tf.translation = r.target;
        } else if moving {
            tf.translation += to * (step / flat).min(1.0);
        }
        // The body faces its record's facing byte, walking or not: the client's
        // orientation is the record's (GameActor::GetOrientation 0x8F5F6F reads
        // +0x28; the MoveCommand reader 0x9CE976 decodes both angle bytes the same
        // way). FAILURE: turned toward each step while walking and to the facing
        // between steps, a chasing creature (walking to its ring slot, facing the
        // player) flickered between the two ("ils recalculent en boucle l'angle").
        // Turned toward the record's facing over a few frames, monsters too: set
        // at once, the 1/256-turn facing bytes of 4-tick records turned them in
        // visible notches ("le heading semble crante"). A teleport snaps.
        tf.rotation = if flat > 8.0 {
            Quat::from_rotation_y(r.facing)
        } else {
            turn_towards(tf.rotation, r.facing, time.delta_secs() * if monster { 1.5 } else { 1.0 })
        };
        // CONTRACT: a short grace before standing: records arrive 25 a second and a
        //   remote player reaches each one a little early, so without it the run
        //   restarted at every record ("les animations ne se jouent pas entierement").
        r.still_for = if moving { 0.0 } else { r.still_for + time.delta_secs() };
        let wanted = if r.still_for < RUN_GRACE {
            AnimState::Run
        } else if matches!(anim.state, AnimState::Named(_)) {
            anim.state.clone()
        } else {
            AnimState::Idle
        };
        if anim.state != wanted {
            anim.state = wanted;
        }
    }
}

fn spawn_local(
    mut commands: Commands,
    mut net: NonSendMut<Net>,
    existing: Query<Entity, With<LocalPlayer>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some((desc, at, heading, name, admin)) = net.pending_local.take() else { return };
    for e in &existing {
        commands.entity(e).despawn();
    }
    let entity = spawn_character(&mut commands, desc, Transform::from_translation(at).with_rotation(Quat::from_rotation_y(heading)));
    net.local_appearance_dirty |= net.inventory.ready;
    // DSOR_MARKER=1: a bright pillar on the player, to find them while debugging.
    if std::env::var("DSOR_MARKER").is_ok() {
        let marker = commands
            .spawn((
                Mesh3d(meshes.add(Cylinder::new(0.3, 30.0))),
                MeshMaterial3d(materials.add(StandardMaterial { emissive: LinearRgba::rgb(20.0, 0.0, 0.0), ..default() })),
                Transform::from_xyz(0.0, 15.0, 0.0),
            ))
            .id();
        commands.entity(entity).add_child(marker);
    }
    commands.entity(entity).insert(Nameplate::player(name, admin));
    commands.entity(entity).insert(LocalPlayer {
        actor: net.local_actor.unwrap_or(0),
        target: None,
        facing: heading,
        last_sent_tick: 0,
        snapped: false,
        casting: 0.0,
    });
}

/// Where the cursor points on the ground: the navigation mesh, or the plane at
/// `fallback_height`.
pub fn cursor_ground(
    window: &Window,
    camera: &Camera,
    cam_tf: &GlobalTransform,
    nav: Option<&NavMesh>,
    fallback: Vec3,
) -> Option<Vec3> {
    let cursor = window.cursor_position()?;
    let ray = camera.viewport_to_world(cam_tf, cursor).ok()?;
    nav.and_then(|m| m.raycast(ray))
        .or_else(|| ray.intersect_plane(fallback, InfinitePlane3d::new(Vec3::Y)).map(|d| ray.get_point(d)))
}

/// Left click on the ground: walk there. The ground is the navigation mesh -- stairs
/// and upper floors included -- or, before it has loaded, the plane at the player's feet.
#[allow(clippy::too_many_arguments)]
fn click_to_move(
    buttons: Res<ButtonInput<MouseButton>>,
    ui: Res<crate::ui_input::UiInput>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    // The 3D view's camera: the UI has its own (crate::ui::UiCamera), and with
    // two, single() failed and no click walked ("je ne peux pas me déplacer").
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::highlight::HighlightCamera>)>,
    nav: Option<Res<CurrentNav>>,
    meshes: Res<Assets<NavMesh>>,
    mut players: Query<(&Transform, &mut LocalPlayer)>,
    hovered: Option<Res<crate::monsters::Hovered>>,
    net: Option<NonSend<Net>>,
    time: Res<Time>,
) {
    if ui.blocks(MouseButton::Left) || !buttons.pressed(MouseButton::Left) || keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        return;
    }
    // Dead, or held by the revival: no walking (the server refuses it anyway).
    if net.as_ref().is_some_and(|n| n.death.dead || time.elapsed_secs() < n.death.held_until) {
        return;
    }
    // A monster under the cursor: the click attacks it (crate::skills), it is no walk.
    if hovered.is_some_and(|h| h.0.is_some()) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok((camera, cam_tf)) = cameras.single() else { return };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else { return };
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    for (tf, mut player) in &mut players {
        let hit = mesh.and_then(|m| m.raycast(ray)).or_else(|| {
            ray.intersect_plane(tf.translation, InfinitePlane3d::new(Vec3::Y)).map(|d| ray.get_point(d))
        });
        if let Some(point) = hit {
            let flat = Vec2::new(point.x - tf.translation.x, point.z - tf.translation.z).length();
            if flat > CLICK_DEAD_ZONE {
                player.target = Some(point);
            }
        }
    }
}

/// Walk toward the target on the navigation mesh: every step takes the ground's
/// height, and a step that would leave the mesh slides along its edge or stops.
fn walk_local(
    time: Res<Time>,
    nav: Option<Res<CurrentNav>>,
    meshes: Res<Assets<NavMesh>>,
    mut players: Query<(&mut Transform, &mut LocalPlayer, &mut CharacterAnim)>,
    mut autowalked: Local<bool>,
) {
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    for (mut tf, mut player, mut anim) in &mut players {
        // On arrival, stand on the ground nearest where the server put us.
        if let (Some(mesh), false) = (mesh, player.snapped) {
            if let Some(p) = mesh.nearest(tf.translation, 4.0) {
                tf.translation = p;
            }
            player.snapped = true;
        }
        // DSOR_AUTOWALK=dx,dz: walk that far once, on arrival (testing without a mouse).
        if player.snapped && player.target.is_none() && player.last_sent_tick > 0 {
            if let Some((dx, dz)) = std::env::var("DSOR_AUTOWALK").ok().and_then(|v| {
                let mut it = v.split(',').filter_map(|x| x.trim().parse::<f32>().ok());
                Some((it.next()?, it.next()?))
            }) {
                if !*autowalked {
                    *autowalked = true;
                    player.target = Some(tf.translation + Vec3::new(dx, 0.0, dz));
                }
            }
        }
        if player.casting > 0.0 {
            player.casting -= time.delta_secs();
            tf.rotation = Quat::from_rotation_y(player.facing);
            continue;
        }
        let mut moving = false;
        if let Some(target) = player.target {
            let here = tf.translation;
            let mut to = target - here;
            to.y = 0.0;
            let step = RUN_SPEED * time.delta_secs();
            let dir = to.normalize_or_zero();
            let wish = if to.length() <= step { Vec3::new(target.x, here.y, target.z) } else { here + dir * step };
            let next = match mesh {
                None => Some(wish),
                Some(m) => {
                    let on = |p: Vec3| m.ground(p.x, p.z, here.y, MAX_STEP).map(|h| Vec3::new(p.x, h, p.z));
                    on(wish)
                        .or_else(|| on(Vec3::new(wish.x, here.y, here.z)))
                        .or_else(|| on(Vec3::new(here.x, here.y, wish.z)))
                }
            };
            match next {
                Some(p) if (p - here).length_squared() > 1e-8 => {
                    tf.translation = p;
                    if dir != Vec3::ZERO {
                        player.facing = dir.x.atan2(dir.z);
                    }
                    moving = true;
                    if Vec2::new(target.x - p.x, target.z - p.z).length() < 0.05 {
                        player.target = None;
                    }
                }
                // Blocked: a wall, a cliff, the edge of the world.
                _ => player.target = None,
            }
        }
        tf.rotation = turn_towards(tf.rotation, player.facing, time.delta_secs());
        // A skill's animation plays to its end unless the player walks off; forcing
        // Idle as soon as the movement block lifted cut every cast short.
        let wanted = if moving {
            AnimState::Run
        } else if matches!(anim.state, AnimState::Named(_)) {
            anim.state.clone()
        } else {
            AnimState::Idle
        };
        if anim.state != wanted {
            anim.state = wanted;
        }
    }
}

fn zoom_camera(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, mut zoom: ResMut<CameraZoom>, ui: Res<crate::ui_input::UiInput>, time: Res<Time>) {
    for w in wheel.read() {
        if ui.over_ui { continue; }
        let step = if w.unit == bevy::input::mouse::MouseScrollUnit::Line { w.y } else { w.y / 40.0 };
        zoom.target = (zoom.target - step).clamp(CAM_HEIGHT_MIN, CAM_HEIGHT_MAX);
    }
    // Previously accepted input keeps settling even after the cursor enters UI.
    zoom.advance(time.delta_secs());
}

/// The DSO camera: fixed pitch and yaw, looking at the player from `zoom` above.
fn follow_camera(
    zoom: Res<CameraZoom>,
    opts: Option<Res<crate::Options>>,
    players: Query<&Transform, With<LocalPlayer>>,
    mut cameras: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<crate::highlight::HighlightCamera>, Without<LocalPlayer>)>,
) {
    if opts.is_some_and(|o| o.cam.is_some()) { return; }
    let Ok(player) = players.single() else { return };
    let rotation = Quat::from_euler(EulerRot::YXZ, CAM_YAW_DEG.to_radians(), CAM_PITCH_DEG.to_radians(), 0.0);
    let back = rotation * Vec3::Z;
    let target = player.translation + Vec3::new(0.0, 0.0, CAM_OFFSET_Z);
    // The zoom is the camera's distance to the player along its view.
    // FAILURE (2026-10-06): read as an altitude, the camera sat 35 units away and the
    // player was a speck ("je vois pas le perso que je joue").
    let distance = zoom.current;
    for (mut cam, mut projection) in &mut cameras {
        *cam = Transform::from_translation(target + back * distance).with_rotation(rotation);
        if let Projection::Perspective(p) = &mut *projection {
            p.fov = CAM_FOV_DEG.to_radians();
        }
    }
}

/// One MoveCommand per game tick, as the 2018 client sends them: where the player
/// is, speed 0 (the 2018 client never fills it), heading and facing in 256ths of a
/// turn, the tick, and a 20-tick duration.
fn send_moves(mut net: NonSendMut<Net>, time: Res<Time<Real>>, mut players: Query<(&Transform, &mut LocalPlayer)>) {
    let Ok((tf, mut player)) = players.single_mut() else { return };
    if net.centre.is_none() {
        return;
    }
    let tick = net.server_tick(now_ms(&time));
    if tick == player.last_sent_tick {
        return;
    }
    player.last_sent_tick = tick;
    let (x, elevation, y) = net.game_to_wire(tf.translation);
    let turn = |r: f32| ((r.rem_euclid(TAU) / TAU) * 256.0).round() as u32 as u8;
    let command = ClientCommand::Move(movement::Move {
        x,
        elevation,
        y,
        speed: 0,
        heading: turn(player.facing),
        facing: turn(player.facing),
        start_tick: tick,
        duration: MOVE_DURATION,
    });
    if net.offline {
        return;
    }
    if let ClientCommand::Move(m) = &command { net.corrections.sent(*m); }
    let (bytes, bits) = command.encode();
    net.session.send_command_with(&bytes, bits, Reliability::UnreliableSequenced);
    let _ = TICK_SECONDS;
}

/// Dress the player from the inventory, and other players from RemotePlayerInfo.
#[allow(clippy::type_complexity)]
fn dress(
    mut net: NonSendMut<Net>,
    library: Option<Res<CharacterLibrary>>,
    skins: Res<Assets<ItemSkins>>,
    mut locals: Query<&mut Character, (With<LocalPlayer>, Without<RemotePlayer>)>,
    mut remotes: Query<(&RemotePlayer, &mut Character), Without<LocalPlayer>>,
) {
    for (r, mut c) in &mut remotes {
        if let Some((equipment, armament)) = net.remote_redress.remove(&r.actor) {
            redress(&mut c, equipment, armament);
        }
    }
    if !net.local_appearance_dirty { return; }
    let Some(table) = library.as_ref().and_then(|l| skins.get(&l.item_skins)) else { return };
    let Ok(mut c) = locals.single_mut() else { return };
    let Some((equipment, armament)) = appearance::local(&net.inventory, table) else { return };
    net.local_appearance_dirty = false;
    redress(&mut c, equipment, armament);
}

/// DSOR_DEBUG_HIER=1: once, four seconds in, where the player's body really is.
pub fn debug_hierarchy(
    time: Res<Time>,
    mut done: Local<bool>,
    players: Query<(Entity, &GlobalTransform), With<LocalPlayer>>,
    children: Query<&Children>,
    names: Query<&Name>,
    globals: Query<&GlobalTransform>,
    skinned: Query<&bevy::mesh::skinning::SkinnedMesh>,
    parents: Query<&ChildOf>,
) {
    if *done || std::env::var("DSOR_DEBUG_HIER").is_err() || time.elapsed_secs() < 6.0 {
        return;
    }
    *done = true;
    let Ok((player, at)) = players.single() else { return };
    info!("HIER player {player:?} at {:?}", at.translation());
    for c in children.get(player).into_iter().flatten() {
        let n = names.get(*c).map(|n| n.as_str().to_owned()).unwrap_or_default();
        info!("HIER  child {c:?} {n:?} global {:?} has_global {}", globals.get(*c).map(|g| g.translation()).ok(), globals.contains(*c));
    }
    let mut shown = 0;
    for e in children.iter_descendants(player) {
        if let Ok(s) = skinned.get(e) {
            let j0 = s.joints.first().copied();
            let jp = j0.and_then(|j| parents.get(j).ok().map(|p| p.parent()));
            info!(
                "HIER  skinned {e:?} global {:?}; joint0 {:?} {:?} at {:?}; joint0 under player: {}",
                globals.get(e).map(|g| g.translation()).ok(),
                j0,
                j0.and_then(|j| names.get(j).ok().map(|n| n.as_str().to_owned())),
                j0.and_then(|j| globals.get(j).ok().map(|g| g.translation())),
                j0.map(|j| children.iter_descendants(player).any(|d| d == j)).unwrap_or(false),
            );
            let _ = jp;
            shown += 1;
            if shown > 4 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod appearance_tests {
    use super::*;
    use crate::character::CharacterVisual;
    use bevy::ecs::system::RunSystemOnce;
    use dsor_proto::commands::{inventory::{InventoryInfo, ItemInfo}, actors::ItemUpdate, player::{NewRemotePlayer, RemotePlayerInfo}};

    fn app() -> App {
        let mut app = App::new();
        app.insert_resource(Time::<Real>::default())
            .init_resource::<Assets<ItemSkins>>()
            .init_resource::<Assets<NavMesh>>()
            .add_systems(Update, (apply_remotes, dress).chain());
        start_offline(app.world_mut(), 1, 0);
        let table = app.world_mut().resource_mut::<Assets<ItemSkins>>().add(serde_json::from_str::<ItemSkins>(
            r#"{"items":{"chest":["mage_torso_01"]},"appearances":{"changed":["mage_torso_02"]}}"#).unwrap());
        app.insert_resource(CharacterLibrary::for_test(table));
        app
    }

    fn receive(app: &mut App, command: ServerCommand, actor: u32) {
        on_command(&mut app.world_mut().non_send_mut::<Net>(), command, Some(actor));
    }

    #[test]
    fn local_inventory_and_compact_updates_redress_only_changed_visuals() {
        let mut app = app();
        let packet = InventoryInfo { items: vec![ItemInfo { id: 10, template: "chest".into(), ..default() }], slots: vec![(10, vec![2])], ..default() };
        receive(&mut app, ServerCommand::InventoryInfo(packet.clone()), OFFLINE_ACTOR);
        app.update();
        assert!(app.world().non_send::<Net>().local_appearance_dirty, "wait for local spawn");
        let root = app.world_mut().run_system_once(|mut commands: Commands| {
            let root = spawn_character(&mut commands, CharacterDesc { class: 1, ..default() }, Transform::default());
            commands.entity(root).insert(LocalPlayer { actor: OFFLINE_ACTOR, target: None, facing: 0.0, last_sent_tick: 0, snapped: true, casting: 0.0 });
            root
        }).unwrap();
        app.update();
        assert_eq!(app.world().get::<Character>(root).unwrap().desc.equipment[0].skin_parts, ["mage_torso_01"]);
        let visual = app.world_mut().spawn((CharacterVisual, ChildOf(root))).id();
        let attachment = app.world_mut().spawn(ChildOf(root)).id();
        app.world_mut().get_mut::<CharacterAnim>(root).unwrap().state = AnimState::Run;
        receive(&mut app, ServerCommand::InventoryInfo(packet), OFFLINE_ACTOR);
        app.update();
        assert!(app.world().get_entity(visual).is_ok(), "unchanged equipment must preserve the current skeleton");
        receive(&mut app, ServerCommand::ItemUpdate(ItemUpdate { item: 10, appearance: "changed".into(), twentieth: 0xFFFF0000, ..default() }), OFFLINE_ACTOR);
        app.update();
        let desc = &app.world().get::<Character>(root).unwrap().desc;
        assert_eq!(desc.equipment[0].skin_parts, ["mage_torso_02"]);
        assert_eq!(desc.equipment[0].colour, Some([1.0, 0.0, 0.0, 1.0]));
        assert!(app.world().get_entity(visual).is_ok(), "keep old visuals until their replacement is loaded");
        assert!(app.world().get_entity(attachment).is_ok(), "non-outfit attachments survive");
        assert_eq!(app.world().get::<CharacterAnim>(root).unwrap().state, AnimState::Run);
        receive(&mut app, ServerCommand::InventoryInfo(InventoryInfo::default()), OFFLINE_ACTOR);
        app.update();
        assert!(app.world().get::<Character>(root).unwrap().desc.equipment.is_empty());
    }

    #[test]
    fn remote_spawn_then_update_in_same_batch_uses_latest_outfit_and_pose() {
        let mut app = app();
        let equipment = vec![Equipment { slot: 5, skin_parts: vec!["staff".into()], colour: None }];
        receive(&mut app, ServerCommand::NewRemotePlayer(NewRemotePlayer { name: "Other".into(), equipment: equipment.clone(), armament: 5, parts: [1, 0, 0, 0, 0, 0], ..default() }), 100);
        let equipment = vec![Equipment { slot: 2, skin_parts: vec!["robe; robe_fx".into()], colour: Some([0.0, 0.0, 1.0, 1.0]) }];
        receive(&mut app, ServerCommand::RemotePlayerInfo(RemotePlayerInfo { equipment, animation_set: "mage_1h_weapon".into(), armament: 5, ..default() }), 100);
        app.update();
        let mut query = app.world_mut().query::<(Entity, &RemotePlayer, &Character)>();
        let (entity, _, character) = query.single(app.world()).unwrap();
        assert_eq!(character.desc.armament, 3, "player kind 5 is not two-hand armament");
        assert_eq!(character.desc.equipment[0].skin_parts, ["robe", "robe_fx"]);
        assert_eq!(character.desc.equipment[0].colour, Some([0.0, 0.0, 1.0, 1.0]));
        let visual = app.world_mut().spawn((CharacterVisual, ChildOf(entity))).id();
        for set in ["mage_2h_weapon", "mage_1h_weapon_shield"] {
            receive(&mut app, ServerCommand::RemotePlayerInfo(RemotePlayerInfo { animation_set: set.into(), ..default() }), 100);
        }
        app.update();
        assert_eq!(app.world().get::<Character>(entity).unwrap().desc.armament, 4);
        assert!(app.world().get_entity(visual).is_ok(), "the outfit loader publishes the replacement later");
        assert!(app.world().non_send::<Net>().remote_redress.is_empty());
    }

    #[test]
    fn server_encoded_appearance_batches_drive_both_player_types() {
        let vectors: Vec<serde_json::Value> = serde_json::from_str(include_str!("../../crates/proto/tests/data/appearance_release261.json")).unwrap();
        for vector in vectors {
            let mut app = app();
            let table = app.world_mut().resource_mut::<Assets<ItemSkins>>().add(serde_json::from_str::<ItemSkins>(
                r#"{"items":{"mage_base_torso_armor_block_critical":["mage_torso_01"]},"appearances":{"mage_basic_torso_06":["mage_torso_06"]}}"#).unwrap());
            app.insert_resource(CharacterLibrary::for_test(table));
            let root = app.world_mut().run_system_once(|mut commands: Commands| {
                let root = spawn_character(&mut commands, CharacterDesc { class: 1, ..default() }, Transform::default());
                commands.entity(root).insert(LocalPlayer { actor: OFFLINE_ACTOR, target: None, facing: 0.0, last_sent_tick: 0, snapped: true, casting: 0.0 });
                root
            }).unwrap();
            let hex = vector["hex"].as_str().unwrap();
            let data: Vec<_> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i+2], 16).unwrap()).collect();
            let commands = dsor_proto::commands::decode_message(&data, vector["bits"].as_u64().unwrap() as usize).unwrap();
            for (command, actor) in commands {
                receive(&mut app, command, if actor == Some(1) { OFFLINE_ACTOR } else { actor.unwrap() });
            }
            app.update();
            let local = &app.world().get::<Character>(root).unwrap().desc;
            let expected = match vector["name"].as_str().unwrap() {
                "appearance_initial" => Some("mage_torso_01"),
                "appearance_changed" => Some("mage_torso_06"),
                "appearance_unequipped" => None,
                name => panic!("unexpected fixture {name}"),
            };
            assert_eq!(local.equipment.first().map(|e| e.skin_parts[0].as_str()), expected);
            let mut query = app.world_mut().query_filtered::<&Character, With<RemotePlayer>>();
            let remote = &query.single(app.world()).unwrap().desc;
            assert_eq!(remote.equipment.first().map(|e| e.skin_parts[0].as_str()), expected);
            assert_ne!(remote.armament, 5);
            if expected.is_some() { assert_eq!(remote.equipment[0].colour, Some([0.1, 0.9, 0.2, 1.0])); }
        }
    }
}


/// Players' deaths and revivals: the death pose (the anim set's Death row, held),
/// the local player stopped where it fell; the respawn choice from the death
/// dialog sent as RespawnCommand (106).
/// SEE: the server's docs/PLAYER_RESPAWN.md (Kill, then ShowDeathDialog 58 ticks
///   later; Revive after a respawn; a town respawn is a map change).
fn player_deaths(
    mut net: NonSendMut<Net>,
    mut choice: ResMut<RespawnChoice>,
    mut locals: Query<(Entity, &mut CharacterAnim, &mut LocalPlayer)>,
    mut remotes: Query<(&RemotePlayer, &mut CharacterAnim), (Without<LocalPlayer>, Without<crate::monsters::Monster>)>,
    mut holds: ResMut<crate::skills::StatusHolds>,
    time: Res<Time>,
) {
    let local = net.local_actor;
    for actor in std::mem::take(&mut net.player_deaths) {
        if Some(actor) == local {
            for (_, mut anim, mut player) in &mut locals {
                player.target = None;
                player.casting = 0.0;
                if !anim.dead {
                    anim.dead = true;
                    anim.state = AnimState::Named("Death".into());
                    anim.replay();
                }
            }
        } else if let Some((_, mut anim)) = remotes.iter_mut().find(|(r, _)| r.actor == actor) {
            if !anim.dead {
                anim.dead = true;
                anim.state = AnimState::Named("Death".into());
                anim.replay();
            }
        }
    }
    for actor in std::mem::take(&mut net.revives) {
        if Some(actor) == local {
            // The revival the server plays and holds the player for (its
            // services/player_respawn.py: resurrect_self, StartAnimation
            // Resurrect, 4.32 s, on the spot; map_respawn 2 s at an exit): held
            // here too, whether or not its status effects reach crate::skills
            // (the player walked off mid-revival).
            let chosen = net.death.chosen.take();
            for (e, mut anim, mut player) in &mut locals {
                anim.dead = false;
                anim.state = AnimState::Idle;
                player.target = None;
                // crate::skills::hold_for_status plays it, holds, then idles.
                let hold = match chosen {
                    Some((false, true)) => 4.32,
                    Some((true, false)) => 2.0,
                    _ => 0.0,
                };
                if hold > 0.0 {
                    holds.0.push((e, if hold > 3.0 { "Resurrect".into() } else { String::new() }, hold));
                }
                net.death.held_until = time.elapsed_secs() + hold;
            }
        } else if let Some((_, mut anim)) = remotes.iter_mut().find(|(r, _)| r.actor == actor) {
            anim.dead = false;
            anim.state = AnimState::Idle;
        }
    }
    if let Some(c) = choice.0.take() {
        if net.death.dead {
            net.death.respawn = Some(c);
        }
    }
    if let Some((at_respawn_point, instant)) = net.death.respawn.take() {
        info!("respawn requested: nearest exit {at_respawn_point}, on the spot {instant}");
        net.send(&ClientCommand::Respawn(dsor_proto::commands::combat::Respawn { at_respawn_point, instant }));
        net.death.requested = true;
        net.death.dialog = false;
        net.death.chosen = Some((at_respawn_point, instant));
    }
}
