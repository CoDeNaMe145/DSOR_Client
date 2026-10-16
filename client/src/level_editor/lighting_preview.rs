//! Effective DB4 lighting and editor-only light/volume handles.
use super::*;
use crate::rendering::lighting::{Bubble, EditorLook};

#[derive(Resource, Default)]
pub(super) struct State {
    pub source: String, pub source_key: Option<String>, pub previewing: bool,
    pub observer: Option<Vec3>, pub points: usize, pub spots: usize,
}

pub(super) fn update(editor: Res<Editor>, roots: Query<&Transform, With<ObjectRoot>>,
    cameras: Query<&Transform, With<ViewCamera>>, mut look: ResMut<EditorLook>, mut state: ResMut<State>) {
    let selected = editor.selected();
    // Match the free-camera client: only physical entry changes the look.
    // Use this frame's transform, after navigation, rather than last frame's
    // GlobalTransform. Selecting objects or rotating cannot move the observer.
    let position = cameras.single().ok().map(|at| at.translation);
    state.observer = position;
    let mut global = None;
    let mut local = None;
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
        if placed.object.default_ambience {
            if global.is_none_or(|previous: &Placed| placed.object.key < previous.object.key) { global = Some(placed); }
        }
        else {
            let mut volume = bubble.clone();
            volume.m = roots.get(placed.entity).ok().map(|t| t.to_matrix().to_cols_array());
            // HashMap traversal is unordered. Resolve equal priorities by the
            // stable object key so overlapping zones do not alternate looks.
            if position.is_some_and(|p| volume.contains(p)) && local.is_none_or(|previous: &Placed| {
                (volume.priority, &placed.object.key) > (previous.object.ambience.as_ref().unwrap().priority, &previous.object.key)
            }) { local = Some(placed); }
        }
    }
    let preview = selected.filter(|p| editor.controls.preview_ambience.as_ref() == Some(&p.object.key)
        && p.object.ambience.as_ref().is_some_and(|b| b.enabled));
    let chosen = preview.or(local).or(global);
    state.previewing = preview.is_some();
    state.source_key = chosen.map(|p| p.object.key.clone());
    let mut bubble = chosen.and_then(|p| p.object.ambience.as_ref()).map(|b| b.look.clone())
        .or_else(|| editor.ambience.clone()).unwrap_or_else(Bubble::default);
    if let Some(placed) = chosen {
        if let Ok(at) = roots.get(placed.entity) { bubble.dir = Some((at.rotation * Vec3::Z).to_array()); }
        state.source = placed.object.label.clone();
    } else { state.source = "Map ambience".into(); }
    // Local volumes never redirect the map's shadow projection. Legacy scenes
    // without a separate shadow record fall back to the default ambience.
    let shadow_dir = editor.shadow.or_else(|| global.and_then(|p| roots.get(p.entity).ok())
        .map(|at| (at.rotation * Vec3::Z).to_array()))
        .or_else(|| editor.ambience.as_ref().and_then(|b| b.dir));
    if look.shadow_dir != shadow_dir { look.shadow_dir = shadow_dir; }
    let shadow_reference = global.and_then(|p| p.object.ambience.as_ref()).map(|b| &b.look)
        .or(editor.ambience.as_ref()).cloned().unwrap_or_default().sun_luminance();
    if look.shadow_reference != shadow_reference { look.shadow_reference = shadow_reference; }
    if look.bubble != bubble { look.bubble = bubble; }
}

/// Stable, readable identity colors shared by zone outlines and their labels.
pub(super) fn zone_color(key: &str, enabled: bool) -> Color {
    if !enabled { return Color::srgb(0.5, 0.5, 0.5); }
    let palette = [[0.35, 0.8, 1.], [1., 0.65, 0.3], [0.6, 0.9, 0.35], [0.85, 0.5, 1.],
        [1., 0.45, 0.65], [0.3, 1., 0.8], [1., 0.9, 0.35], [0.55, 0.65, 1.]];
    let hash = key.bytes().fold(0_u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let [r, g, b] = palette[hash as usize % palette.len()];
    Color::srgb(r, g, b)
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
            if !selected && !editor.controls.show_helpers && !editor.controls.show_lights && !editor.controls.show_bubbles { continue; }
            let color = zone_color(&placed.object.key, bubble.enabled);
            if placed.object.default_ambience {
                gizmos.arrow(at.translation, at.translation - at.rotation * Vec3::Z * 4., color);
            } else if bubble.shape == "Sphere" {
                for axis in 0..3 { ring(&mut gizmos, at, axis, 0.5, color); }
            } else { gizmos.cube(*at, color); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<EditorLook>().init_resource::<State>().add_systems(Update, update);
        let mut editor = Editor {
            desktop: false, pending_scene: None, inbox: vec![], pointer_blocked: false, keyboard_blocked: false,
            viewport: None, directory: default(), revision: 0, map: "test".into(), center: Vec3::ZERO,
            extents: Vec3::splat(10.), scene_stamp: None, controls_stamp: None, poll_at: 0., status_at: 0.,
            controls: default(), placed: default(), models: default(), events: 0, drag: None,
            focus_serial: 0, needs_frame: false, error: String::new(), ambience: None, shadow: None,
        };
        for (key, intensity, global) in [("global", 1., true), ("local", 4., false)] {
            let at = Transform::from_scale(Vec3::splat(10.));
            let object: Object = serde_json::from_value(json!({
                "key": key, "kind": "_Instance_AmbienceBubble", "label": key, "model": "",
                "matrix": at.to_matrix().to_cols_array(), "default_ambience": global,
                "ambience": {"m": at.to_matrix().to_cols_array(), "shape": "Box", "event": -1, "LightIntensity": intensity}
            })).unwrap();
            let entity = app.world_mut().spawn((ObjectRoot(key.into()), at)).id();
            editor.placed.insert(key.into(), Placed { entity, object, ready: true, placeholder: false, bounds: (Vec3::ZERO, Vec3::splat(0.5)) });
        }
        app.insert_resource(editor);
        let camera = app.world_mut().spawn((ViewCamera, Transform::from_xyz(0., 10., 20.).looking_at(Vec3::ZERO, Vec3::Y))).id();
        (app, camera)
    }

    #[test]
    fn local_preview_retains_the_authored_map_shadow_direction() {
        let (mut app, camera) = scene();
        let shadow = Some([-0.2747, 0.7545, -0.5961]);
        app.world_mut().resource_mut::<Editor>().shadow = shadow;
        for position in [Vec3::new(0., 10., 20.), Vec3::ZERO, Vec3::new(0., 10., 20.)] {
            app.world_mut().entity_mut(camera).insert(Transform::from_translation(position));
            app.update();
            assert_eq!(app.world().resource::<EditorLook>().shadow_dir, shadow);
        }
    }

    #[test]
    fn turning_towards_and_selecting_a_zone_outside_does_not_change_lighting() {
        let (mut app, camera) = scene();
        for target in [Vec3::ZERO, Vec3::new(0., 0., 40.), Vec3::ZERO] {
            app.world_mut().entity_mut(camera).insert(Transform::from_xyz(0., 10., 20.).looking_at(target, Vec3::Y));
            app.world_mut().resource_mut::<Editor>().controls.selected = Some("local".into());
            app.update();
            assert_eq!(app.world().resource::<State>().source, "global");
            assert_eq!(app.world().resource::<EditorLook>().bubble.light_intensity, 1.);
        }
        app.world_mut().entity_mut(camera).insert(Transform::from_xyz(0., 1., 0.));
        app.update();
        assert_eq!(app.world().resource::<State>().source, "local");
        assert_eq!(app.world().resource::<EditorLook>().bubble.light_intensity, 4.);
        app.world_mut().entity_mut(camera).insert(Transform::from_xyz(0., 5.1, 0.));
        app.update();
        assert_eq!(app.world().resource::<State>().source, "global", "height also participates in containment");
    }

    #[test]
    fn preview_is_explicit_and_cannot_activate_hidden_or_disabled_zones() {
        let (mut app, _) = scene();
        {
            let mut editor = app.world_mut().resource_mut::<Editor>();
            editor.controls.selected = Some("local".into());
            editor.controls.preview_ambience = Some("local".into());
        }
        app.update();
        assert!(app.world().resource::<State>().previewing);
        assert_eq!(app.world().resource::<EditorLook>().bubble.light_intensity, 4.);
        app.world_mut().resource_mut::<Editor>().controls.preview_ambience = None;
        app.update();
        assert!(!app.world().resource::<State>().previewing);
        assert_eq!(app.world().resource::<State>().source, "global");
        for case in 0..3 {
            {
                let mut editor = app.world_mut().resource_mut::<Editor>();
                editor.controls.preview_ambience = Some("local".into());
                editor.controls.selected = Some(if case == 0 { "global" } else { "local" }.into());
                editor.placed.get_mut("local").unwrap().object.ambience.as_mut().unwrap().enabled = case != 1;
                if case == 2 { editor.controls.hidden_geometry.insert("local".into()); }
            }
            app.update();
            assert!(!app.world().resource::<State>().previewing);
            assert_eq!(app.world().resource::<State>().source, "global");
        }
    }

    #[test]
    fn live_volume_transform_priority_and_event_visibility_control_activation() {
        let (mut app, camera) = scene();
        app.world_mut().entity_mut(camera).insert(Transform::from_xyz(0., 1., 0.));
        let mut object = app.world().resource::<Editor>().placed["local"].object.clone();
        object.key = "second".into(); object.label = "second".into(); object.event = 7;
        object.ambience.as_mut().unwrap().priority = 5;
        let entity = app.world_mut().spawn((ObjectRoot(object.key.clone()), object.transform())).id();
        app.world_mut().resource_mut::<Editor>().placed.insert(object.key.clone(),
            Placed { entity, object, ready: true, placeholder: false, bounds: (Vec3::ZERO, Vec3::splat(0.5)) });
        app.update();
        assert_eq!(app.world().resource::<State>().source, "local");
        app.world_mut().resource_mut::<Editor>().controls.active_events.insert(7);
        app.update();
        assert_eq!(app.world().resource::<State>().source, "second");
        // A gizmo edit takes effect before scene serialization/transform propagation.
        app.world_mut().entity_mut(entity).insert(Transform::from_xyz(50., 0., 0.));
        app.update();
        assert_eq!(app.world().resource::<State>().source, "local");
    }
}
