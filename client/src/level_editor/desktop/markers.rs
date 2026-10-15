//! Screen-sized, asset-independent icons for gameplay placements. These are UI
//! overlays, so they never appear in the game camera or prefab thumbnails.
use super::*;

#[derive(Clone, Copy)]
enum Kind {
    Spawn(&'static str),
    Teleport,
}
impl Kind {
    fn of(table: &str) -> Option<Self> {
        match table {
            "_Instance_Entry" => Some(Self::Spawn("Player entry / spawn")),
            "_Instance_SpawnPoint" => Some(Self::Spawn("NPC spawn point")),
            "_Instance_StateObjectSpawnPoint" => Some(Self::Spawn("Object spawn point")),
            "_Instance_ItemSpawnPoint" => Some(Self::Spawn("Item spawn point")),
            "_Instance_SpawnArea" => Some(Self::Spawn("Spawn area")),
            "_Instance_TravelPoint" => Some(Self::Teleport),
            _ => None,
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Spawn(title) => title,
            Self::Teleport => "Teleport stone",
        }
    }
    fn color(self) -> egui::Color32 {
        match self {
            Self::Spawn(_) => egui::Color32::from_rgb(98, 225, 164),
            Self::Teleport => egui::Color32::from_rgb(118, 178, 255),
        }
    }
    fn paint(self, painter: &egui::Painter, center: egui::Pos2) {
        let p = |x, y| center + egui::vec2(x, y);
        let stroke = egui::Stroke::new(1.8, self.color());
        match self {
            Self::Spawn(_) => {
                painter.line_segment([p(-5., 9.), p(-5., -9.)], stroke);
                painter.add(egui::Shape::convex_polygon(
                    vec![p(-4., -8.), p(8., -6.), p(5., -1.), p(-4., -2.)],
                    self.color(),
                    egui::Stroke::NONE,
                ));
                painter.line_segment([p(-9., 9.), p(3., 9.)], stroke);
            }
            Self::Teleport => {
                // Obelisk and its portal ring, drawn without fonts or textures.
                let ring = (0..=32)
                    .map(|i| {
                        let a = i as f32 * std::f32::consts::TAU / 32.;
                        p(a.cos() * 10., 6. + a.sin() * 4.)
                    })
                    .collect();
                painter.add(egui::Shape::line(ring, stroke));
                painter.add(egui::Shape::convex_polygon(
                    vec![p(0., -10.), p(4., -5.), p(4., 6.), p(-4., 6.), p(-4., -5.)],
                    egui::Color32::from_rgb(31, 53, 78),
                    stroke,
                ));
                painter.line_segment([p(0., -4.), p(0., 3.)], stroke);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    editor: &mut Editor,
    camera: &Camera,
    camera_at: &Transform,
    roots: &Query<&Transform, With<ObjectRoot>>,
    logical_to_ui: f32,
    actions: &mut Vec<Action>,
) -> bool {
    let global = GlobalTransform::from(*camera_at);
    let mut markers: Vec<_> = editor
        .placed
        .values()
        .filter_map(|placed| {
            let kind = Kind::of(&placed.object.kind)?;
            if !editor.controls.visible(&placed.object) {
                return None;
            }
            // Use the live transform, including an in-progress gizmo movement.
            let at = roots.get(placed.entity).ok()?.translation;
            let point = camera.world_to_viewport(&global, at).ok()? * logical_to_ui;
            let anchor = egui::pos2(point.x, point.y);
            let center = anchor - egui::vec2(0., 34.);
            let bounds = egui::Rect::from_center_size(center, egui::vec2(30., 30.));
            if !viewport.contains(anchor) || !viewport.contains_rect(bounds) {
                return None;
            }
            let selected = editor.controls.selected.as_ref() == Some(&placed.object.key);
            Some((
                placed,
                kind,
                anchor,
                bounds,
                selected,
                at.distance_squared(camera_at.translation),
            ))
        })
        .collect();
    // Deterministic overlap: closer icons and the selection are drawn last.
    markers.sort_by(|a, b| {
        a.4.cmp(&b.4)
            .then_with(|| b.5.total_cmp(&a.5))
            .then_with(|| a.0.object.key.cmp(&b.0.object.key))
    });
    let can_interact = editor.drag.is_none()
        && !ui.input(|i| {
            i.pointer.button_down(egui::PointerButton::Secondary)
                || i.pointer.button_down(egui::PointerButton::Middle)
        });
    let painter = ui.painter_at(viewport);
    let mut hovered = false;
    let mut focus = false;
    for (placed, kind, anchor, bounds, selected, _) in markers {
        let response = ui.interact(
            bounds,
            egui::Id::new(("placement-icon", &placed.object.key)),
            if can_interact {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        );
        let active = can_interact && response.hovered();
        hovered |= active || (can_interact && response.is_pointer_button_down_on());
        let color = if selected {
            egui::Color32::GOLD
        } else {
            kind.color()
        };
        painter.line_segment(
            [bounds.center_bottom(), anchor],
            egui::Stroke::new(1.5, color),
        );
        painter.circle_filled(anchor, 2.5, color);
        painter.rect_filled(bounds.expand(2.), 9., egui::Color32::from_black_alpha(90));
        painter.rect_filled(
            bounds,
            7.,
            if active {
                egui::Color32::from_rgb(40, 53, 68)
            } else {
                egui::Color32::from_rgb(20, 28, 38)
            },
        );
        painter.rect_stroke(
            bounds,
            7.,
            egui::Stroke::new(if selected || active { 2. } else { 1. }, color),
            egui::StrokeKind::Inside,
        );
        kind.paint(&painter, bounds.center());
        if can_interact {
            if response.clicked() {
                actions.push(Action::Select(placed.object.key.clone()));
                ui.memory_mut(|m| {
                    if let Some(id) = m.focused() {
                        m.surrender_focus(id);
                    }
                });
            }
            if response.double_clicked() {
                focus = true;
            }
            response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_ui(|ui| {
                    ui.label(
                        egui::RichText::new(kind.title())
                            .color(kind.color())
                            .strong(),
                    );
                    ui.label(&placed.object.label);
                    ui.small("Click to inspect · Double-click to focus");
                });
        }
    }
    if focus {
        editor.controls.focus_serial += 1;
        editor.controls.focus_target = "selection".into();
    }
    hovered
}
