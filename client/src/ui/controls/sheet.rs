//! The character page's statistics list (release261 charactersheet): rows made
//! from the layout's own TemplateHeadline / TemplateSubHeadline /
//! TemplateAttributes, filled from crate::gameplay::stats::SheetRows, scrolled
//! with the wheel over the list.
//! UNVERIFIED: the gap between rows (1 reference pixel) and one row per wheel
//!   notch; the templates' own spacing in the layout is 0.6 to 1.6.

use super::*;
use crate::gameplay::stats::{RowKind, SheetRows};

const WINDOW: &str = "charactersheet";
const AREA: &str = "/StatsCharacter/CharacterStatsArea";
const GAP: f32 = 1.0;
/// Inside the window's art band (crate::ui::WINDOW_ART_DEPTH), over the area.
const ART_Z: f32 = 7.5;
const TEXT_Z: f32 = 7.6;

#[derive(Component)]
pub(in crate::ui) struct SheetGroup;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::ui) fn rows(
    mut commands: Commands,
    rows: Res<SheetRows>,
    loaded: Res<crate::ui::Loaded>,
    layouts: Res<Assets<Layout>>,
    windows: Query<(Entity, &Window2018, &WindowControls, &Visibility, &Transform)>,
    groups: Query<Entity, With<SheetGroup>>,
    screen: Res<Screen>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<UiMaterial>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    primary: Query<&Window, With<PrimaryWindow>>,
    mut state: Local<(Option<Entity>, usize, Option<usize>)>,
    mut kept: Local<Option<(Entity, Entity)>>,
    alive: Query<(), With<SheetGroup>>,
) {
    let notches: f32 = wheel.read().map(|w| w.y.signum()).sum();
    let Some((root, _, controls, visibility, tf)) = windows.iter().find(|(_, w, _, _, _)| w.0 == WINDOW) else { return };
    let Some(layout) = loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h)).find(|l| l.window == WINDOW) else { return };
    let find = |suffix: &str| layout.widgets.iter().find(|w| w.id.ends_with(&format!("{AREA}{suffix}")));
    let (Some(list), Some(area), Some(head), Some(attr), Some(sub)) = (find("/OverviewList"), find(""), find("/TemplateHeadline"), find("/TemplateAttributes"), find("/TemplateSubHeadline")) else { return };
    let height = |w: &Widget| w.rect[3] - w.rect[1];
    let (top, bottom) = (head.rect[1], list.rect[3].min(area.rect[3]));
    // How many rows fit from a first one on.
    let fitting = |first: usize| {
        let mut y = top;
        let mut n = 0;
        for r in rows.0.iter().skip(first) {
            let h = height(match r.kind { RowKind::Headline => head, RowKind::SubHeadline => sub, RowKind::Attribute => attr });
            if y + h > bottom { break; }
            y += h + GAP;
            n += 1;
        }
        n
    };
    let max_scroll = (0..rows.0.len()).find(|&f| f + fitting(f) >= rows.0.len()).unwrap_or(0);
    let mut scroll = state.1.min(max_scroll);
    // The wheel over the list scrolls it, a row a notch.
    if notches != 0.0 && *visibility != Visibility::Hidden {
        if let Some(cursor) = primary.single().ok().and_then(|w| w.cursor_position().map(|c| (c * (BASE_H / w.height().max(1.0)), BASE_H * w.width() / w.height().max(1.0)))) {
            let point = cursor.0 - window_offset(tf);
            if contains(list.k, list.rect, point, cursor.1) {
                scroll = (scroll as i64 - notches as i64).clamp(0, max_scroll as i64) as usize;
            }
        }
    }
    let rebuild = rows.is_changed() || screen.is_changed() || state.0 != Some(root) || state.1 != scroll || state.2.is_none();
    if !rebuild { return; }
    // The two groups stay; only their rows are made again. Spawned anew, they
    // were hidden a frame until paint showed them: the rows flickered as they
    // scrolled ("les epees flickers quand je scroll").
    let k = list.k;
    let (art_parent, text_parent) = match *kept {
        Some((a, t)) if alive.contains(a) && alive.contains(t) && state.0 == Some(root) => {
            commands.entity(a).despawn_related::<Children>();
            commands.entity(t).despawn_related::<Children>();
            (a, t)
        }
        _ => {
            for g in &groups { commands.entity(g).despawn(); }
            let gate = controls.gate(WINDOW, area);
            let group = |commands: &mut Commands, flip: bool| commands.spawn((SheetGroup, Follows { k, flip }, follow_transform(&screen, k, flip), ChildOf(root),
                Element { root, gate: gate.clone() }, Visibility::Hidden)).id();
            (group(&mut commands, true), group(&mut commands, false))
        }
    };
    *kept = Some((art_parent, text_parent));
    *state = (Some(root), scroll, Some(rows.0.len()));
    let mut runs: HashMap<Option<String>, Run> = HashMap::new();
    let mut y = top;
    for r in rows.0.iter().skip(scroll) {
        let template = match r.kind { RowKind::Headline => head, RowKind::SubHeadline => sub, RowKind::Attribute => attr };
        let h = height(template);
        if y + h > bottom { break; }
        let dy = y - template.rect[1];
        let prefix = format!("{}/", template.id);
        for w in layout.widgets.iter().filter(|w| w.id == template.id || w.id.starts_with(&prefix)) {
            // The headline's Icon is not shown (the official page: none).
            if w.id.ends_with("/Icon") { continue; }
            if let Some(art) = &w.art {
                let mut a = art.clone();
                for p in a.pos.chunks_exact_mut(2) { p[1] += dy; }
                runs.entry(a.tex.clone()).or_insert_with(|| Run { k, tex: a.tex.clone(), ..default() }).push(&a, ART_Z);
            }
            let Some(label) = &w.text else { continue };
            let text = match (r.kind, w.id.rsplit('/').next().unwrap_or("")) {
                (RowKind::Headline, "Headline") | (RowKind::SubHeadline, "Text") | (RowKind::Attribute, "Stat") => r.label.clone(),
                // A single figure stands in the right column, a pair in both
                // (the official page: "Base Damage: 100" right, "Critical
                // Value: 2 (0.09 %)").
                (RowKind::Attribute, "Value") => if r.right.is_empty() { String::new() } else { r.value.clone() },
                (RowKind::Attribute, "ValueRight") => if r.right.is_empty() { r.value.clone() } else { r.right.clone() },
                _ => continue,
            };
            let [l, t, rr, b] = w.rect;
            let (t, b) = (t + dy, b + dy);
            let x = match label.align[0] { 0 => l, 2 => rr, _ => (l + rr) * 0.5 };
            let yy = match label.align[1] { 0 => t, 2 => b, _ => (t + b) * 0.5 };
            commands.spawn((
                Text2d::new(text),
                TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(label.size * FONT_SCALE * screen.scale), ..default() },
                TextColor(Color::srgba(label.color[0], label.color[1], label.color[2], label.color[3])),
                Anchor(Vec2::new(match label.align[0] { 0 => -0.5, 2 => 0.5, _ => 0.0 }, match label.align[1] { 0 => 0.5, 2 => -0.5, _ => 0.0 })),
                Transform::from_xyz(x, -yy, TEXT_Z).with_scale(Vec3::splat(1.0 / screen.scale)),
                RenderLayers::layer(LAYER),
                ChildOf(text_parent),
            ));
        }
        y += h + GAP;
    }
    // The scroll handle, along its boundaries as the list scrolls (the static
    // one is hidden, crate::ui::controls::section).
    if let (Some(bounds), Some(handle)) = (find("/OverviewList/ScrollBarBackground/ScrollBarBoundaries"), find("/OverviewList/ScrollBarBackground/ScrollBarBoundaries/ScrollHandle/normal")) {
        if let Some(art) = &handle.art {
            let h = height(handle);
            let frac = if max_scroll > 0 { scroll as f32 / max_scroll as f32 } else { 0.0 };
            let dy = bounds.rect[1] + frac * (bounds.rect[3] - bounds.rect[1] - h) - handle.rect[1];
            let mut a = art.clone();
            for p in a.pos.chunks_exact_mut(2) { p[1] += dy; }
            runs.entry(a.tex.clone()).or_insert_with(|| Run { k, tex: a.tex.clone(), ..default() }).push(&a, ART_Z);
        }
    }
    for (tex, run) in runs {
        if run.idx.is_empty() { continue; }
        let m = materials.add(material(tex.as_ref().map(|t| assets.load::<Image>(t.clone()))));
        commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(m), Transform::from_xyz(0.0, 0.0, ART_Z), RenderLayers::layer(LAYER), ChildOf(art_parent)));
    }
}
