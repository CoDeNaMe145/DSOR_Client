use super::*;
use bevy::window::{CursorGrabMode, CursorOptions};

/// Position is independent of orientation: looking never revolves around a pivot.
#[derive(Resource)]
pub(super) struct FlyCamera { position: Vec3, yaw: f32, pitch: f32 }
impl Default for FlyCamera {
    fn default() -> Self { Self { position: Vec3::new(20., 25., 25.), yaw: 0.7, pitch: 0.65 } }
}
impl FlyCamera {
    fn direction(&self) -> Vec3 {
        -Vec3::new(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), self.yaw.cos() * self.pitch.cos())
    }
    fn transform(&self) -> Transform { Transform::from_translation(self.position).looking_to(self.direction(), Vec3::Y) }
    fn look(&mut self, delta: Vec2) {
        self.yaw -= delta.x * 0.003;
        self.pitch = (self.pitch + delta.y * 0.003).clamp(-1.54, 1.54);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn navigate(mut editor: ResMut<Editor>, mut fly: ResMut<FlyCamera>,
    mut cameras: Query<&mut Transform, With<ViewCamera>>, mut windows: Query<(&Window, &mut CursorOptions)>,
    buttons: Res<ButtonInput<MouseButton>>, keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>, scroll: Res<AccumulatedMouseScroll>, time: Res<Time>) {
    let Ok(mut camera) = cameras.single_mut() else { return };
    let Ok((window, mut cursor)) = windows.single_mut() else { return };
    let over_preview = window.cursor_position().is_some_and(|p| camera_preview::contains(&editor, window, p));
    let flying = window.focused && !over_preview && buttons.pressed(MouseButton::Right) && editor.drag.is_none() && !keys.pressed(KeyCode::Escape);
    cursor.grab_mode = if flying { CursorGrabMode::Confined } else { CursorGrabMode::None };
    cursor.visible = !flying;
    let focus_key = window.focused && !flying && keys.just_pressed(KeyCode::KeyF);
    if editor.needs_frame || editor.controls.focus_serial != editor.focus_serial || focus_key {
        let selection = !editor.needs_frame && (editor.controls.focus_target != "map" || focus_key);
        let target = selection.then(|| editor.selected()).flatten().filter(|p| editor.controls.visible(&p.object));
        let (center, distance) = if let Some(placed) = target {
            let at = placed.object.transform();
            (at.transform_point(placed.bounds.0), (placed.bounds.1.length() * at.scale.abs().max_element() * 4.).clamp(5., 150.))
        } else {
            fly.yaw = 0.7;
            fly.pitch = 0.65;
            (editor.center, (editor.extents.xz().length() * 1.5).clamp(30., 450.))
        };
        fly.position = center - fly.direction() * distance;
        editor.focus_serial = editor.controls.focus_serial;
        editor.needs_frame = false;
    }
    if window.focused && !over_preview && editor.drag.is_none() {
        if flying { fly.look(motion.delta); }
        let orientation = fly.transform();
        let forward = orientation.forward().as_vec3();
        let right = orientation.right().as_vec3();
        let speed = editor.controls.camera_speed.clamp(0.5, 200.);
        if flying {
            let mut movement = Vec3::ZERO;
            for (key, direction) in [(KeyCode::KeyW, forward), (KeyCode::KeyS, -forward),
                (KeyCode::KeyA, -right), (KeyCode::KeyD, right), (KeyCode::KeyQ, -Vec3::Y), (KeyCode::KeyE, Vec3::Y)] {
                if keys.pressed(key) { movement += direction; }
            }
            let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 4. } else { 1. };
            fly.position += movement.normalize_or_zero() * speed * boost * time.delta_secs().min(0.1);
        }
        if buttons.pressed(MouseButton::Middle) && !flying {
            fly.position += (-right * motion.delta.x + orientation.up().as_vec3() * motion.delta.y) * speed * 0.003;
        }
        fly.position += forward * scroll.delta.y * speed * 0.15;
    }
    *camera = fly.transform();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn looking_rotates_in_place_even_far_from_the_map_center() {
        let mut camera = FlyCamera::default();
        camera.position = Vec3::new(320., 18., -100.);
        let before = camera.transform();
        camera.look(Vec2::new(130., -80.));
        let after = camera.transform();
        assert_eq!(before.translation, after.translation);
        assert!(before.forward().dot(*after.forward()) < 0.99);
    }
}
