//! Effective DB4 lighting and editor-only light/volume handles.
use super::*;
use crate::lighting::{Bubble, EditorLook};

#[derive(Resource, Default)]
pub(super) struct State { pub source: String, pub points: usize, pub spots: usize }

pub(super) fn update(editor: Res<Editor>, roots: Query<&Transform, With<ObjectRoot>>,
    cameras: Query<&GlobalTransform, With<ViewCamera>>, mut look: ResMut<EditorLook>, mut state: ResMut<State>) {
    let selected = editor.selected();
    let focus = selected.filter(|p| p.object.camera.is_some()).and_then(|p| roots.get(p.entity).ok())
        .map(|at| crate::map::camera_focus(&GlobalTransform::from(*at), None, editor.center.y))
        .or_else(|| cameras.single().ok().map(|at| crate::map::camera_focus(at, None, editor.center.y)))
        .unwrap_or(editor.center);
    let mut global = None;
    let mut local = None;
    let mut priority = i32::MIN;
    state.points = 0; state.spots = 0;
    for placed in editor.placed.values().filter(|p| editor.controls.visible(&p.object)) {
        if let Some(light) = &placed.object.light {
            if light.i > 0. && light.r > 0. {
                if light.t == 0 { state.points += 1; }
                else if light.t == 1 && light.cone > 0. { state.spots += 1; }
            }
        }
        let Some(bubble) = &placed.object.ambience else { continue };
        if !bubble.enabled { continue; }
        if placed.object.default_ambience { global = Some(placed); }
        else {
            let mut volume = bubble.clone();
            volume.m = roots.get(placed.entity).ok().map(|t| t.to_matrix().to_cols_array());
            if volume.priority >= priority && volume.contains(focus) { local = Some(placed); priority = volume.priority; }
        }
    }
    // Selecting a volume previews that volume directly, even while outside it.
    let chosen = selected.filter(|p| p.object.ambience.as_ref().is_some_and(|b| b.enabled)).or(local).or(global);
    let mut bubble = chosen.and_then(|p| p.object.ambience.as_ref()).map(|b| b.look.clone())
        .or_else(|| editor.ambience.clone()).unwrap_or_else(Bubble::default);
    if let Some(placed) = chosen {
        if let Ok(at) = roots.get(placed.entity) { bubble.dir = Some((at.rotation * Vec3::Z).to_array()); }
        state.source = placed.object.label.clone();
    } else { state.source = "Map ambience".into(); }
    if look.bubble != bubble { look.bubble = bubble; }
}

fn ring(gizmos: &mut Gizmos<Handles>, at: &Transform, axis: usize, radius: f32, color: Color) {
    gizmos.linestrip((0..=64).map(|i| at.transform_point(ring_point(axis, i as f32 * std::f32::consts::TAU / 64.) * radius)), color);
}

pub(super) fn draw(editor: Res<Editor>, roots: Query<&Transform, With<ObjectRoot>>, mut gizmos: Gizmos<Handles>) {
    for placed in editor.placed.values().filter(|p| editor.controls.visible(&p.object)) {
        let Ok(at) = roots.get(placed.entity) else { continue };
        let selected = editor.controls.selected.as_ref() == Some(&placed.object.key);
        let color = Color::srgb(1., 0.78, 0.28);
        let frame = Transform::from_translation(at.translation).with_rotation(at.rotation);
        if let Some(light) = &placed.object.light {
            if !selected && !editor.controls.show_helpers && !editor.controls.show_lights { continue; }
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] { gizmos.line(at.translation - axis * 0.3, at.translation + axis * 0.3, color); }
            if selected {
                if light.t == 0 {
                    for axis in 0..3 { ring(&mut gizmos, &frame, axis, light.r.max(0.01), color); }
                } else if light.cone > 0. {
                    let distance = light.r.max(0.01);
                    let radius = (light.cone.to_radians() * 0.5).tan() * distance;
                    let end = frame * Transform::from_xyz(0., 0., -distance);
                    ring(&mut gizmos, &end, 2, radius, color);
                    for axis in [Vec3::X, Vec3::Y, Vec3::NEG_X, Vec3::NEG_Y] { gizmos.line(at.translation, end.transform_point(axis * radius), color); }
                }
            }
        }
        if let Some(bubble) = &placed.object.ambience {
            if !selected && !editor.controls.show_helpers { continue; }
            if placed.object.default_ambience {
                gizmos.arrow(at.translation, at.translation - at.rotation * Vec3::Z * 4., color);
            } else if bubble.shape == "Sphere" {
                for axis in 0..3 { ring(&mut gizmos, at, axis, 0.5, color); }
            } else { gizmos.cube(*at, color); }
        }
    }
}
