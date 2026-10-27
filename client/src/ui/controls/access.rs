//! The map access window (mapaccesswindow): a travel item whose destination
//! has several difficulties (its _Template_MapAccessGroup rows sharing one
//! AccessGroup: pw003_05/07/09_grimmag_01_boss) asks for one on the
//! difficulty slider first. The slider's snap marks are the tiers; a click on
//! it picks the nearest; Enter travels: the item's use (0x13) arms it, then
//! UseTravelItem names the chosen row (DSOR_Server map_transition.py accepts
//! any row of the armed target's access group). Close drops it.
//! FAILURE: "j'ai pas le menu pour choisir quand j'utilise un voyage pour un
//!   boss, je devrais pouvoir choisir la difficulte avec le slider".
//! UNVERIFIED: the rewards, difficulty modifications and leaderboards panes
//!   are left out; the key a tier needs is named, not checked.
use super::*;
use dsor_proto::commands::ClientCommand;

pub(in crate::ui) const WINDOW: &str = "mapaccesswindow";
const ART: &str = "Canvas/Skillwindow/BackGroundLabel/WindowTitleLabel/Frame/Frame2/LabelArtwork2/LabelArtwork";
/// The slider's track, reference pixels (k 0.5).
const TRACK: [f32; 4] = [327.69, 379.86, 692.5, 409.95];

pub(in crate::ui) fn prepare(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    out.widgets.retain(|w| !w.id.contains("/TabButton") && !w.id.contains("/BottomDungeoninfo") && !w.id.contains("/BottomLeaderboardsinfo")
        && !w.id.contains("/DifficultySnapLine") && !w.id.contains("/slider/Thumb") && !w.id.contains("/SmallMeanie") && !w.id.contains("/FatMeanie")
        && !(w.id.contains("/EntryRequirements/") && !w.id.contains("/ButtonBuyEnter")) && !w.id.contains("/TextList/Scroll"));
    for w in &mut out.widgets {
        if w.id.ends_with("/Headline/WindowTitle") || w.id.ends_with("/TextList/TextLabel") || w.id.ends_with("/DifficultyInfo/background2/TextLabel") { w.text = None; }
    }
    out
}

pub(in crate::ui) fn action(w: &Widget) -> Action {
    match w.event.as_str() { "Buy" => Action::AccessAnswer(true), "Close" => Action::AccessAnswer(false), _ => Action::Unavailable }
}

#[derive(Component)] pub struct AccessText;

/// What is asked: the travel item, its tiers (access row, difficulty, key) and
/// the one picked.
#[derive(Default)]
pub(in crate::ui) struct Asked { item: u32, tiers: Vec<(String, u32, Option<String>)>, picked: usize }

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::ui) fn dialog(
    mut commands: Commands,
    net: Option<NonSendMut<crate::network::net::Net>>,
    mut windows: Windows,
    mut pointer: ResMut<Pointer>,
    (layouts, loaded, names): (Res<Assets<Layout>>, Res<crate::ui::Loaded>, Res<Assets<crate::ui::local_map::MapNames>>),
    (assets, screen, mouse): (Res<AssetServer>, Res<Screen>, Res<ButtonInput<MouseButton>>),
    primary: Query<&Window, With<bevy::window::PrimaryWindow>>,
    old: Query<Entity, With<AccessText>>,
    (mut asked, mut shown): (Local<Option<Asked>>, Local<Option<(usize, usize, u32, u32)>>),
) {
    let Some(mut net) = net else { return };
    let names = names.get(&loaded.map_names);
    if let Some((item, row)) = net.access_ask.take() {
        let tiers = names.map(|n| n.access_tiers(&row)).unwrap_or_default();
        let picked = tiers.iter().position(|t| t.0 == row).unwrap_or(0);
        *asked = Some(Asked { item, tiers, picked });
    }
    if let Some(yes) = pointer.access_answer.take() {
        if let Some(a) = asked.take().filter(|_| yes) {
            if let Some((row, _, _)) = a.tiers.get(a.picked) {
                net.send(&crate::gameplay::inventory::command(a.item, dsor_proto::commands::inventory::Inventory::USE, vec![]));
                net.send(&ClientCommand::UseTravelItem(dsor_proto::commands::inventory::UseTravelItem { item: a.item, map: row.clone() }));
            }
        }
        *asked = None;
    }
    let Some((root, _, _, mut v, tf)) = windows.iter_mut().find(|(_, n, ..)| n.0 == WINDOW) else { return };
    if asked.is_some() && shown.is_some() && *v == Visibility::Hidden { *asked = None; }
    v.set_if_neq(if asked.is_some() { Visibility::Inherited } else { Visibility::Hidden });
    // A click on the slider: the nearest tier.
    if let (Some(a), true, Ok(window)) = (asked.as_mut(), mouse.just_pressed(MouseButton::Left), primary.single()) {
        if let Some(cursor) = window.cursor_position() {
            let height = window.height().max(1.0);
            let point = cursor * (BASE_H / height);
            let width = BASE_H * window.width() / height;
            let x = point.x - 0.5 * (width - BASE_W) - tf.translation.x;
            let y = point.y + tf.translation.y;
            let n = a.tiers.len();
            if n > 1 && (TRACK[1] - 8.0..=TRACK[3] + 8.0).contains(&y) && (TRACK[0] - 10.0..=TRACK[2] + 10.0).contains(&x) {
                let at = ((x - TRACK[0]) / (TRACK[2] - TRACK[0])).clamp(0.0, 1.0);
                a.picked = (at * (n - 1) as f32).round() as usize;
            }
        }
    }
    let key = asked.as_ref().map(|a| (a.tiers.len(), a.picked, screen.width.to_bits(), screen.scale.to_bits()));
    if *shown == key { return; }
    *shown = key;
    for e in &old { commands.entity(e).despawn(); }
    let (Some(a), Some(layout)) = (asked.as_ref(), loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h)).find(|l| l.window == WINDOW)) else { return };
    let font = |size: f32| TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(size * FONT_SCALE * screen.scale), ..default() };
    let mut put = |text: String, at: Vec2, width: f32, size: f32, color: Color| {
        commands.spawn((Text2d::new(text), font(size), TextColor(color), bevy::text::TextLayout::justify(bevy::text::Justify::Center), Anchor::CENTER,
            bevy::text::TextBounds::new_horizontal(width * screen.scale),
            Transform::from_translation(screen.world(0.5, at.x, at.y).extend(WINDOW_ITEMS_DEPTH + 0.2)).with_scale(Vec3::splat(1.0 / screen.scale)),
            RenderLayers::layer(LAYER), AccessText, ChildOf(root)));
    };
    let rect = |id: &str| layout.widgets.iter().find(|w| w.id.ends_with(id)).map(|w| w.rect);
    let (row, difficulty, needs) = a.tiers.get(a.picked).cloned().unwrap_or_default();
    let group = names.map(|n| n.access_group(&row)).unwrap_or_default();
    let title = names.map_or_else(|| group.clone(), |n| n.caption(&group));
    if let Some([l, t, r, b]) = rect("/Headline/WindowTitle") { put(title, Vec2::new((l + r) * 0.5, (t + b) * 0.5), r - l, 12.0, Color::srgb(1.0, 0.87, 0.6)); }
    if let Some([l, t, r, _]) = rect(&format!("{ART}/TextList/TextLabel")) {
        put("Choose the difficulty on the slider, then Enter.".into(), Vec2::new((l + r) * 0.5, t + 12.0), r - l, 10.0, Color::srgb(0.92, 0.87, 0.76));
    }
    let tier_name = names.map(|n| n.difficulty_name(difficulty)).unwrap_or_default();
    let line = match &needs { Some(k) => format!("{tier_name}\n(needs {})", k.replace(':', " x ")), None => tier_name };
    if let Some([l, t, r, b]) = rect("/DifficultyInfo/background2/TextLabel") { put(line, Vec2::new((l + r) * 0.5, (t + b) * 0.5), r - l + 80.0, 11.0, Color::srgb(1.0, 0.87, 0.6)); }
    // The snap marks and the thumb.
    let n = a.tiers.len().max(1);
    for (i, (_, level, _)) in a.tiers.iter().enumerate() {
        let x = if n > 1 { TRACK[0] + (TRACK[2] - TRACK[0]) * i as f32 / (n - 1) as f32 } else { (TRACK[0] + TRACK[2]) * 0.5 };
        let picked = i == a.picked;
        put(if picked { "\u{25C6}".into() } else { "|".into() }, Vec2::new(x, (TRACK[1] + TRACK[3]) * 0.5), 20.0, if picked { 16.0 } else { 11.0 },
            if picked { Color::srgb(1.0, 0.8, 0.3) } else { Color::srgb(0.75, 0.68, 0.55) });
        put(level.to_string(), Vec2::new(x, TRACK[3] + 8.0), 30.0, 9.0, Color::srgb(0.85, 0.78, 0.6));
    }
}
