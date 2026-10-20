//! Detach the gameplay camera without moving or replacing the character.
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraState};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::network::net::LocalPlayer;
use crate::ui::input::UiInput;

#[derive(Component)]
pub(super) struct Toggle;
#[derive(Component)]
pub(super) struct Caption;
#[derive(Component)]
pub(crate) struct DetachedCamera;
/// Rendering follows the camera instead of this player while detached.
#[derive(Component)]
pub(crate) struct DetachedPlayer;
pub(crate) type FollowedPlayer = (With<LocalPlayer>, Without<DetachedPlayer>);

pub(super) fn button(
    mut input: ResMut<UiInput>,
    players: Query<(), With<LocalPlayer>>,
    presses: Query<(&Interaction, &InheritedVisibility), (With<Toggle>, Changed<Interaction>)>,
    mut buttons: Query<&mut BackgroundColor, With<Toggle>>,
    mut captions: Query<&mut Text, With<Caption>>,
) {
    let available = !players.is_empty();
    for (interaction, visible) in &presses {
        if available && visible.get() && *interaction == Interaction::Pressed {
            input.free_look = !input.free_look;
        }
    }
    let caption = if !available { "Free look: spawn a character first" }
        else if input.free_look { "Free look: on (click to follow character)" }
        else { "Free look: off (click to detach camera)" };
    for mut text in &mut captions {
        if text.0 != caption { text.0 = caption.into(); }
    }
    for mut color in &mut buttons {
        color.0 = if !available { Color::srgb(0.12, 0.12, 0.14) }
            else if input.free_look { Color::srgb(0.25, 0.4, 0.2) }
            else { Color::srgb(0.22, 0.22, 0.28) };
    }
}

/// Runs before Bevy's free camera controller, which runs between PreUpdate and
/// Update. Keep its disabled state present so it also releases cursor capture.
pub(super) fn sync_controls(
    mut commands: Commands,
    mut input: ResMut<UiInput>,
    players: Query<(Entity, Has<DetachedPlayer>), With<LocalPlayer>>,
    panels: Query<&Visibility, With<super::Panel>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(Entity, Option<&FreeCamera>, Option<&mut FreeCameraState>, Has<DetachedCamera>),
        (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
) {
    let has_player = !players.is_empty();
    if !has_player { input.free_look = false; }
    for (entity, detached) in &players {
        if input.free_look && !detached { commands.entity(entity).insert(DetachedPlayer); }
        if !input.free_look && detached { commands.entity(entity).remove::<DetachedPlayer>(); }
    }
    let controls_enabled = input.free_look && !input.typing && !input.hold_keys && !input.over_ui
        && !panels.iter().any(|v| *v != Visibility::Hidden)
        && windows.iter().any(|w| w.focused);
    for (entity, config, state, detached) in &mut cameras {
        // An ordinary map viewer keeps its existing free camera controls.
        if !has_player && !detached { continue; }
        if input.free_look && !detached {
            let mut state = FreeCameraState::default();
            state.enabled = controls_enabled;
            commands.entity(entity).insert((
                DetachedCamera,
                FreeCamera { walk_speed: 15.0, run_speed: 60.0, key_up: KeyCode::KeyE,
                    key_down: KeyCode::KeyQ, ..default() },
                // Reinitialize yaw/pitch from the current transform every time.
                // Reusing the viewer's old state would jump to its old heading.
                state,
            ));
        } else {
            if detached && !input.free_look { commands.entity(entity).remove::<DetachedCamera>(); }
            if config.is_some() {
                if let Some(mut state) = state {
                    state.enabled = controls_enabled;
                    if !controls_enabled { state.velocity = Vec3::ZERO; state.rotation_curve = None; }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera_controller::free_camera::run_freecamera_controller;
    use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
    use bevy::input::touch::Touches;
    use bevy::window::{CursorGrabMode, CursorOptions};
    use std::time::Duration;

    fn app() -> (App, Entity, Entity, Entity) {
        let mut app = App::new();
        app.init_resource::<UiInput>().init_resource::<Time<Real>>()
            .init_resource::<ButtonInput<KeyCode>>().init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>().init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Touches>()
            .add_systems(Update, (button, sync_controls, run_freecamera_controller).chain());
        app.world_mut().resource_mut::<Time<Real>>().advance_by(Duration::from_millis(100));
        let window = app.world_mut().spawn((Window { focused: true, ..default() }, PrimaryWindow, CursorOptions::default())).id();
        let player = app.world_mut().spawn((LocalPlayer::default(), Transform::from_xyz(12., 0., 30.))).id();
        let camera = app.world_mut().spawn((Camera3d::default(), Transform::from_xyz(20., 15., 50.).looking_at(Vec3::ZERO, Vec3::Y))).id();
        (app, player, camera, window)
    }

    #[test]
    fn free_look_flies_without_moving_player_and_releases_controls_on_disable() {
        let (mut app, player, camera, window) = app();
        let original = *app.world().get::<Transform>(camera).unwrap();
        let player_at = *app.world().get::<Transform>(player).unwrap();
        app.world_mut().resource_mut::<UiInput>().free_look = true;
        app.update();
        assert_eq!(*app.world().get::<Transform>(camera).unwrap(), original, "detach starts from the current view");
        assert!(app.world().get::<DetachedPlayer>(player).is_some());
        assert!(app.world().resource::<UiInput>().blocks(MouseButton::Right));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        app.world_mut().resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(20., -5.);
        app.update();
        assert_ne!(*app.world().get::<Transform>(camera).unwrap(), original);
        assert_eq!(*app.world().get::<Transform>(player).unwrap(), player_at);
        assert_eq!(app.world().get::<CursorOptions>(window).unwrap().grab_mode, CursorGrabMode::Locked);
        app.world_mut().resource_mut::<UiInput>().free_look = false;
        let detached_view = *app.world().get::<Transform>(camera).unwrap();
        app.update();
        assert_eq!(*app.world().get::<Transform>(camera).unwrap(), detached_view);
        assert!(app.world().get::<DetachedPlayer>(player).is_none());
        assert!(app.world().get::<DetachedCamera>(camera).is_none());
        assert!(!app.world().get::<FreeCameraState>(camera).unwrap().enabled);
        let cursor = app.world().get::<CursorOptions>(window).unwrap();
        assert!(cursor.visible);
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
        assert!(!app.world().resource::<UiInput>().blocks(MouseButton::Right));
    }

    #[test]
    fn free_look_pauses_for_panels_text_input_and_focus_loss() {
        let (mut app, _, camera, window) = app();
        app.world_mut().resource_mut::<UiInput>().free_look = true;
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        let panel = app.world_mut().spawn((super::super::Panel, Visibility::Inherited)).id();
        let before = *app.world().get::<Transform>(camera).unwrap();
        app.update();
        assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
        *app.world_mut().get_mut::<Visibility>(panel).unwrap() = Visibility::Hidden;
        for gate in 0..3 {
            app.world_mut().resource_mut::<UiInput>().typing = gate == 0;
            app.world_mut().resource_mut::<UiInput>().over_ui = gate == 1;
            app.world_mut().get_mut::<Window>(window).unwrap().focused = gate != 2;
            app.update();
            assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
        }
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert_ne!(*app.world().get::<Transform>(camera).unwrap(), before);
    }

    #[test]
    fn free_look_button_requires_player_and_resets_if_player_disappears() {
        let (mut app, player, camera, _) = app();
        app.world_mut().despawn(player);
        let toggle = app.world_mut().spawn((Toggle, Interaction::Pressed, InheritedVisibility::VISIBLE, BackgroundColor::default())).id();
        app.update();
        assert!(!app.world().resource::<UiInput>().free_look);
        let player = app.world_mut().spawn(LocalPlayer::default()).id();
        *app.world_mut().get_mut::<Interaction>(toggle).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(toggle).unwrap() = Interaction::Pressed;
        app.update();
        assert!(app.world().resource::<UiInput>().free_look);
        app.world_mut().despawn(player);
        app.update();
        assert!(!app.world().resource::<UiInput>().free_look);
        assert!(app.world().get::<DetachedCamera>(camera).is_none());
        assert!(!app.world().get::<FreeCameraState>(camera).unwrap().enabled);
    }
}
