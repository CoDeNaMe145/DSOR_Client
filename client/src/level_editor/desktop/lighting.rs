//! Lighting-zone labels and an explicit, selection-scoped look preview.
use super::*;

fn color(rgb: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(
        (rgb[0].clamp(0., 1.) * 255.).round() as u8,
        (rgb[1].clamp(0., 1.) * 255.).round() as u8,
        (rgb[2].clamp(0., 1.) * 255.).round() as u8,
    )
}

fn swatches(ui: &mut egui::Ui, look: &crate::lighting::Bubble) {
    ui.horizontal(|ui| {
        for (label, rgba) in [
            ("Sun", look.light_color),
            ("Ambient", look.light_ambient),
            ("Fog", look.fog_color),
        ] {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(18., 18.), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 3., color([rgba[0], rgba[1], rgba[2]]));
            response.on_hover_text(format!(
                "{label}: {:.3}, {:.3}, {:.3}",
                rgba[0], rgba[1], rgba[2]
            ));
            ui.small(label);
        }
    });
    ui.small(format!(
        "Sun intensity {:.2}  |  Fog {:.1} - {:.1}  |  Fade {:.2}s",
        look.light_intensity, look.fog_near_dist, look.fog_far_dist, look.fade_time
    ));
}

pub(super) fn selection(ui: &mut egui::Ui, editor: &mut Editor) {
    let Some(placed) = editor.selected() else {
        return;
    };
    let Some(bubble) = &placed.object.ambience else {
        return;
    };
    let key = placed.object.key.clone();
    let mut preview = editor.controls.preview_ambience.as_ref() == Some(&key);
    ui.heading("Lighting preview");
    let changed = ui.add_enabled(bubble.enabled, egui::Checkbox::new(&mut preview, "Preview selected lighting"))
        .on_hover_text("Apply this look to the scene and camera monitor, even outside its zone. Turn off to follow the editor camera position.")
        .changed();
    swatches(ui, &bubble.look);
    if !bubble.enabled {
        ui.small("Disabled zone: enable it in Volume to preview.");
    }
    ui.separator();
    if changed {
        editor.controls.preview_ambience = preview.then_some(key);
    }
}

pub(super) fn source(ui: &mut egui::Ui, state: &lighting_preview::State) {
    if state.previewing {
        ui.colored_label(
            egui::Color32::GOLD,
            format!("Preview override: {}", state.source),
        );
    } else {
        ui.label(format!("Current lighting: {}", state.source));
    }
    ui.small(
        "Zones activate at the camera position. Select a zone to inspect or preview its lighting.",
    );
    ui.separator();
}

#[allow(clippy::too_many_arguments)]
pub(super) fn labels(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    editor: &Editor,
    state: &lighting_preview::State,
    camera: &Camera,
    camera_at: &Transform,
    roots: &Query<&Transform, With<ObjectRoot>>,
    logical_to_ui: f32,
    actions: &mut Vec<Action>,
) -> bool {
    let global = GlobalTransform::from(*camera_at);
    let can_interact = editor.drag.is_none()
        && !ui.input(|i| {
            i.pointer.button_down(egui::PointerButton::Secondary)
                || i.pointer.button_down(egui::PointerButton::Middle)
        });
    let painter = ui.painter_at(viewport);
    let mut zones: Vec<_> = editor
        .placed
        .values()
        .filter(|p| {
            p.object.ambience.is_some()
                && !p.object.default_ambience
                && editor.controls.visible(&p.object)
        })
        .collect();
    zones.sort_by(|a, b| a.object.key.cmp(&b.object.key));
    let mut hovered = false;
    for placed in zones {
        let selected = editor.controls.selected.as_ref() == Some(&placed.object.key);
        if !selected
            && !editor.controls.show_bubbles
            && !editor.controls.show_lights
            && !editor.controls.show_helpers
        {
            continue;
        }
        let Ok(at) = roots.get(placed.entity) else {
            continue;
        };
        let Ok(point) = camera.world_to_viewport(&global, at.translation) else {
            continue;
        };
        let anchor = egui::pos2(point.x * logical_to_ui, point.y * logical_to_ui);
        if !viewport.contains(anchor) {
            continue;
        }
        let bubble = placed.object.ambience.as_ref().unwrap();
        let tint = lighting_preview::zone_color(&placed.object.key, bubble.enabled).to_srgba();
        let tint = color([tint.red, tint.green, tint.blue]);
        let active = state.source_key.as_ref() == Some(&placed.object.key);
        let suffix = if !bubble.enabled {
            " (disabled)"
        } else if active && state.previewing {
            " (preview)"
        } else if active {
            " (active)"
        } else {
            ""
        };
        let text = format!("{}{}", placed.object.label, suffix);
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.), tint);
        let bounds = egui::Rect::from_center_size(
            anchor - egui::vec2(0., 16.),
            galley.size() + egui::vec2(12., 8.),
        );
        if !viewport.contains_rect(bounds) {
            continue;
        }
        let response = ui.interact(
            bounds,
            egui::Id::new(("lighting-zone", &placed.object.key)),
            if can_interact {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        );
        hovered |= can_interact && (response.hovered() || response.is_pointer_button_down_on());
        painter.rect_filled(bounds, 4., egui::Color32::from_black_alpha(210));
        painter.rect_stroke(
            bounds,
            4.,
            egui::Stroke::new(if active || selected { 2. } else { 1. }, tint),
            egui::StrokeKind::Inside,
        );
        painter.galley(bounds.min + egui::vec2(6., 4.), galley, tint);
        if can_interact {
            if response.clicked() {
                actions.push(Action::Select(placed.object.key.clone()));
            }
            response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_ui(|ui| {
                    ui.label(&placed.object.label);
                    ui.small(format!("{}  |  Priority {}", bubble.shape, bubble.priority));
                    swatches(ui, &bubble.look);
                    ui.small("Click to inspect. Preview selected lighting applies the full look.");
                });
        }
    }
    hovered
}
