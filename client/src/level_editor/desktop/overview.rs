use super::*;
#[derive(Default)]
pub(super) struct Overview {
    edges: Vec<[f32; 4]>,
    center: Vec2,
    zoom: f32,
    drag: Option<(String, Value, egui::Pos2)>,
}
impl Overview {
    pub fn load(s: &Session) -> Self {
        let cache = s
            .project
            .cache
            .join("navigation")
            .join(format!("{}.json", s.name));
        let mut edges = data::read_json(&cache)
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let source = s
            .project
            .assets
            .join("maps")
            .join(format!("{}.nav.bin", s.name));
        if let Ok(raw) = std::fs::read(source) {
            if raw.len() >= 4 {
                let count = u32::from_le_bytes(raw[..4].try_into().unwrap()) as usize;
                if count.checked_mul(36).and_then(|n| n.checked_add(4)) == Some(raw.len()) {
                    let mut counts = std::collections::BTreeMap::new();
                    for tri in raw[4..].chunks_exact(36) {
                        let coords: Vec<_> = tri
                            .chunks_exact(4)
                            .map(|b| {
                                (f32::from_le_bytes(b.try_into().unwrap()) as f64 * 10000.).round()
                                    as i64
                            })
                            .collect();
                        let p = [
                            [coords[0], coords[1], coords[2]],
                            [coords[3], coords[4], coords[5]],
                            [coords[6], coords[7], coords[8]],
                        ];
                        for i in 0..3 {
                            let (a, b) = (p[i], p[(i + 1) % 3]);
                            *counts
                                .entry(if a < b { (a, b) } else { (b, a) })
                                .or_insert(0) += 1;
                        }
                    }
                    edges = counts
                        .into_iter()
                        .filter(|(_, n)| *n == 1)
                        .map(|((a, b), _)| {
                            [
                                a[0] as f32 / 10000.,
                                a[2] as f32 / 10000.,
                                b[0] as f32 / 10000.,
                                b[2] as f32 / 10000.,
                            ]
                        })
                        .collect();
                    let _ = data::atomic_json(cache, &json!(edges));
                }
            }
        }
        Self {
            edges,
            center: Vec2::new(
                data::number(&s.snapshot["manifest"]["center"][0], 0.) as f32,
                data::number(&s.snapshot["manifest"]["center"][2], 0.) as f32,
            ),
            zoom: 0.,
            drag: None,
        }
    }
}
pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut State,
    actions: &mut Vec<Action>,
    editor: &Editor,
) {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0., egui::Color32::from_rgb(18, 23, 29));
    let Some(session) = &mut state.session else {
        return;
    };
    let overview = &mut state.overview;
    if overview.zoom == 0. {
        overview.zoom = (rect.width() / ((editor.extents.x * 2.).max(1.)))
            .min(rect.height() / (editor.extents.z * 2.).max(1.))
            * 0.85;
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        overview.zoom = (overview.zoom * (scroll * 0.002).exp()).clamp(0.01, 100.);
    }
    if response.dragged_by(egui::PointerButton::Secondary) {
        let d = ui.input(|i| i.pointer.delta());
        overview.center += Vec2::new(-d.x, d.y) / overview.zoom;
    }
    let project = |x: f32, z: f32| {
        rect.center()
            + egui::vec2(
                (x - overview.center.x) * overview.zoom,
                -(z - overview.center.y) * overview.zoom,
            )
    };
    for [ax, az, bx, bz] in &overview.edges {
        painter.line_segment(
            [project(*ax, *az), project(*bx, *bz)],
            egui::Stroke::new(1., egui::Color32::from_rgb(54, 86, 103)),
        );
    }
    for poly in data::array(&session.snapshot["manifest"]["nav_blockers"]) {
        let points: Vec<_> = data::array(poly)
            .filter_map(|v| v.as_array())
            .filter(|a| a.len() >= 2)
            .map(|a| {
                project(
                    data::number(&a[0], 0.) as f32,
                    data::number(a.last().unwrap(), 0.) as f32,
                )
            })
            .collect();
        for i in 0..points.len() {
            painter.line_segment(
                [points[i], points[(i + 1) % points.len()]],
                egui::Stroke::new(1., egui::Color32::from_rgb(155, 88, 65)),
            );
        }
    }
    let pointer = response.interact_pointer_pos();
    let mut nearest = (10., None);
    for row in state.rows.iter().filter(|r| r.active) {
        let Some(p) = scene::position(&row.kind, &session.records[&row.kind][&row.id]) else {
            continue;
        };
        let at = project(p[0], p[2]);
        if !rect.contains(at) {
            continue;
        }
        let selected = state.selected.as_ref() == Some(&row.key);
        painter.circle_filled(
            at,
            if selected { 5. } else { 2. },
            if selected {
                egui::Color32::GOLD
            } else if row.kind == data::GEOMETRY {
                egui::Color32::from_rgb(94, 121, 137)
            } else {
                egui::Color32::from_rgb(101, 196, 201)
            },
        );
        if let Some(pointer) = pointer {
            let d = at.distance(pointer);
            if d < nearest.0 {
                nearest = (d, Some(row.key.clone()));
            }
        }
    }
    if response.clicked() || response.drag_started_by(egui::PointerButton::Primary) {
        if let Some(key) = nearest.1 {
            actions.push(Action::Select(key.clone()));
            if response.drag_started_by(egui::PointerButton::Primary) {
                let (k, id) = key.split_once('\t').unwrap();
                overview.drag = pointer.map(|p| (key.clone(), session.records[k][id].clone(), p));
            }
        }
    }
    if let Some((key, original, start)) = &overview.drag {
        if let Some(pointer) = pointer {
            let (k, _) = key.split_once('\t').unwrap();
            if let Some(mut p) = scene::position(k, original) {
                let d = pointer - *start;
                p[0] += d.x / overview.zoom;
                p[2] -= d.y / overview.zoom;
                if editor.controls.snap > 0. {
                    p[0] = (p[0] / editor.controls.snap).round() * editor.controls.snap;
                    p[2] = (p[2] / editor.controls.snap).round() * editor.controls.snap;
                }
                painter.circle_stroke(
                    project(p[0], p[2]),
                    7.,
                    egui::Stroke::new(2., egui::Color32::GOLD),
                );
                if response.drag_stopped() {
                    let (k, id) = key.split_once('\t').unwrap();
                    match scene::moved(k, original, p).and_then(|v| session.update(k, id, v)) {
                        Ok(()) => {
                            state.publish = true;
                            actions.push(Action::Select(key.clone()));
                        }
                        Err(e) => {
                            state.message = e.to_string();
                            state.error = true;
                        }
                    }
                }
            }
        }
    }
    if response.drag_stopped() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        overview.drag = None;
    }
}
