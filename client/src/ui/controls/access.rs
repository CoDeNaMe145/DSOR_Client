//! The map access window (mapaccesswindow): a travel item whose destination
//! has several difficulties (its _Template_MapAccessGroup rows sharing one
//! AccessGroup: pw003_05/07/09_grimmag_01_boss) asks for one on the
//! difficulty slider first. The slider's snap marks are the tiers; a click on
//! it picks the nearest; Enter travels: the item's use (0x13) arms it, then
//! UseTravelItem names the chosen row (DSOR_Server map_transition.py accepts
//! any row of the armed target's access group). Close drops it.
//! FAILURE: "j'ai pas le menu pour choisir quand j'utilise un voyage pour un
//!   boss, je devrais pouvoir choisir la difficulte avec le slider".
//! The row's BackgroundImage replaces the layout's default artwork; its
//! descriptions, possible rewards (RewardItems, the player's class variant),
//! the items its teleport takes (TeleportKeyId) and the group size
//! (_Template_MapTemplate.MaxPlayer) fill the panes.
//! FAILURE: "j'ai ni les boutons ni l'image du bon boss ni le texte".
//! UNVERIFIED: the difficulty modifications list the _Template_MapDifficulty
//!   values and the row's Resistances as text (the retail icons are not
//!   known); the leaderboards tab is left out; the items are not checked.
use super::*;
use dsor_proto::commands::ClientCommand;

pub(in crate::ui) const WINDOW: &str = "mapaccesswindow";
const ART: &str = "Canvas/Skillwindow/BackGroundLabel/WindowTitleLabel/Frame/Frame2/LabelArtwork2/LabelArtwork";
/// The layout's own artwork, swapped for the row's BackgroundImage.
const DEFAULT_ART: &str = "textures/ui/mapaccess_e_bos_02_mortis_01_crunch.png";
const CLASSES: [&str; 4] = ["Warrior", "Mage", "Ranger", "Dwarf"];
const RARITIES: [&str; 8] = ["Common", "Uncommon", "Magic", "Rare", "Epic", "Unique", "Set", "Mythic"];
/// The slider's track, reference pixels (k 0.5).
const TRACK: [f32; 4] = [327.69, 379.86, 692.5, 409.95];

pub(in crate::ui) fn prepare(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    out.widgets.retain(|w| !w.id.contains("/TabButton") && !w.id.contains("/BottomLeaderboardsinfo") && !w.id.contains("/DifficultySnapLine")
        && !w.id.contains("/slider/Thumb") && !w.id.contains("/SmallMeanie") && !w.id.contains("/FatMeanie") && !w.id.contains("/TextList/Scroll")
        && !w.id.contains("/DifficultyModificationsRight/TemplateListEntry") && !w.id.contains("/DifficultyModificationsRight/ScrollButton")
        && !w.id.contains("/ExtraModificationsHeader") && !w.id.contains("/ItemsTop3/Item3") && !w.id.ends_with("/DeniedGroupMapInfo")
        && !w.id.ends_with("/ResistancesInfo"));
    for w in &mut out.widgets {
        if w.id.contains("/ButtonBuyEnter") { w.hidden = false; }
        let filled = ["/Headline/WindowTitle", "/TextList/TextLabel", "/DifficultyInfo/background2/TextLabel", "/GroupsizeText", "/ItemCount"];
        if filled.iter().any(|f| w.id.ends_with(f)) || w.id.contains("/DifficultyModificationsIcons/TextLabel") { w.text = None; }
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
    (data, catalogs, mut materials): (Res<items::Data>, Res<Assets<items::Catalog>>, ResMut<Assets<UiMaterial>>),
    (elements, players): (Query<(&Element, &MeshMaterial2d<UiMaterial>)>, Query<&crate::actors::character::Character, With<crate::network::net::LocalPlayer>>),
    primary: Query<&Window, With<bevy::window::PrimaryWindow>>,
    old: Query<Entity, With<AccessText>>,
    (mut asked, mut shown, mut art): (Local<Option<Asked>>, Local<Option<(usize, usize, u32, u32, bool)>>, Local<Option<Handle<Image>>>),
) {
    let Some(mut net) = net else { return };
    let names = names.get(&loaded.map_names);
    // DSOR_TEST_ACCESS=<access row>: offline, the window asks for that row (screenshots).
    if net.offline && asked.is_none() && shown.is_none() && names.is_some() {
        if let Ok(row) = std::env::var("DSOR_TEST_ACCESS") { if !row.is_empty() && net.access_ask.is_none() { net.access_ask = Some((0, row)); } }
    }
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
    let row = asked.as_ref().and_then(|a| a.tiers.get(a.picked)).map(|t| t.0.clone()).unwrap_or_default();
    // The row's artwork on every run of the layout's default one (rebuilt
    // windows come back with the default).
    if asked.is_some() {
        let bg = names.map(|n| n.access("bg", &row)).filter(|b| !b.is_empty()).map(|b| assets.load::<Image>(format!("textures/ui/{b}.png")));
        let default = assets.load::<Image>(DEFAULT_ART);
        let want = bg.unwrap_or_else(|| default.clone());
        for (element, m) in &elements {
            if element.root != root { continue }
            let Some(id) = materials.get(&m.0).and_then(|m| m.atlas.as_ref()).map(|h| h.id()) else { continue };
            if id != want.id() && (id == default.id() || art.as_ref().is_some_and(|a| a.id() == id)) {
                if let Some(mut mat) = materials.get_mut(&m.0) { mat.atlas = Some(want.clone()); }
            }
        }
        *art = Some(want);
    }
    let catalog = catalogs.get(&data.catalog);
    let key = asked.as_ref().map(|a| (a.tiers.len(), a.picked, screen.width.to_bits(), screen.scale.to_bits(), catalog.is_some()));
    if *shown == key { return; }
    *shown = key;
    for e in &old { commands.entity(e).despawn(); }
    let (Some(a), Some(layout)) = (asked.as_ref(), loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h)).find(|l| l.window == WINDOW)) else { return };
    let group = commands.spawn((AccessText, Transform::from_xyz(0.0, 0.0, WINDOW_ITEMS_DEPTH), Visibility::Inherited, ChildOf(root))).id();
    let font = |size: f32| TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(size * FONT_SCALE * screen.scale), ..default() };
    let put = |commands: &mut Commands, text: String, at: Vec2, width: f32, size: f32, color: Color, centered: bool| {
        let (justify, anchor) = if centered { (bevy::text::Justify::Center, Anchor::CENTER) } else { (bevy::text::Justify::Left, Anchor::TOP_LEFT) };
        commands.spawn((Text2d::new(text), font(size), TextColor(color), bevy::text::TextLayout::justify(justify), anchor,
            bevy::text::TextBounds::new_horizontal(width * screen.scale),
            Transform::from_translation(screen.world(0.5, at.x, at.y).extend(0.5)).with_scale(Vec3::splat(1.0 / screen.scale)),
            RenderLayers::layer(LAYER), ChildOf(group)));
    };
    let widget = |id: &str| layout.widgets.iter().find(|w| w.id.ends_with(id));
    let rect = |id: &str| widget(id).map(|w| w.rect);
    let (gold, pale) = (Color::srgb(1.0, 0.87, 0.6), Color::srgb(0.92, 0.87, 0.76));
    let (_, difficulty, _) = a.tiers.get(a.picked).cloned().unwrap_or_default();
    let text = |what: &str, row: &str| names.map(|n| n.access(what, row).to_owned()).unwrap_or_default();
    let group_id = names.map(|n| n.access_group(&row)).unwrap_or_default();
    let title = names.map_or_else(|| group_id.clone(), |n| n.caption(&group_id));
    let mods = names.map(|n| n.difficulty_mods(difficulty)).unwrap_or_default();
    let m = |i: usize| mods.get(i).copied().unwrap_or(0);
    if let Some([l, t, r, b]) = rect("/Headline/WindowTitle") { put(&mut commands, title, Vec2::new((l + r) * 0.5, (t + b) * 0.5), r - l, 12.0, gold, true); }
    // The description: the row's text, its boss note and the level.
    if let Some([l, t, r, _]) = rect(&format!("{ART}/TextList")) {
        let mut body = super::tooltips::card::plain(&text("desc", &row));
        let note = super::tooltips::card::plain(&text("desc2", &row));
        if !note.is_empty() { body.push_str(&format!("\n\n{note}")); }
        let level = text("level", &group_id).parse::<i64>().unwrap_or(0) + m(0);
        if level > 0 { body.push_str(&format!("\n\n{}: {level}", text("text", "recommendedLevel"))); }
        put(&mut commands, body, Vec2::new(l, t), r - l - 16.0, 10.0, pale, false);
    }
    let tier_name = names.map(|n| n.difficulty_name(difficulty)).unwrap_or_default();
    if let Some([l, t, r, b]) = rect("/DifficultyInfo/background2/TextLabel") { put(&mut commands, tier_name, Vec2::new((l + r) * 0.5, (t + b) * 0.5), r - l + 80.0, 11.0, gold, true); }
    // The snap marks and the thumb.
    let n = a.tiers.len().max(1);
    for i in 0..a.tiers.len() {
        let x = if n > 1 { TRACK[0] + (TRACK[2] - TRACK[0]) * i as f32 / (n - 1) as f32 } else { (TRACK[0] + TRACK[2]) * 0.5 };
        let y = (TRACK[1] + TRACK[3]) * 0.5;
        if i == a.picked {
            // The layout's thumb (slider/Thumb, mapaccess_sliderhandle) on the picked tier.
            commands.spawn((Sprite { image: assets.load("textures/ui/mapaccess_sliderhandle_crunch.png"), custom_size: Some(Vec2::new(14.0, 26.0)), ..default() },
                Anchor::CENTER, Transform::from_translation(screen.world(0.5, x, y).extend(0.6)), RenderLayers::layer(LAYER), ChildOf(group)));
        } else {
            put(&mut commands, "|".into(), Vec2::new(x, y), 20.0, 11.0, Color::srgb(0.75, 0.68, 0.55), true);
        }
    }
    // The difficulty modifications, two columns of five lines.
    let mut lines = vec![format!("Monster Level +{}", m(0)), format!("Magic items: {} %", m(1)), format!("Rare items: {} %", m(2)), format!("Epic items: {} %", m(3))];
    for part in text("res", &row).split(',') {
        let Some((element, value)) = part.split_once(':') else { continue };
        match value.trim().parse::<i32>().unwrap_or(0) { v if v > 0 => lines.push(format!("{element}: resistant")), v if v < 0 => lines.push(format!("{element}: vulnerable")), _ => {} }
    }
    for (i, line) in lines.into_iter().take(10).enumerate() {
        let (Some([l, _, _, _]), Some([_, t, r, b])) = (rect(&format!("/DifficultyModificationsIcons/Icon{i}")), rect(&format!("/DifficultyModificationsIcons/TextLabel{i}"))) else { continue };
        put(&mut commands, line, Vec2::new(l, t + (b - t) * 0.5 - 7.0), r - l, 9.5, pale, false);
    }
    if let Some([l, t, r, _]) = rect("/EntryRequirements/GroupsizeText") {
        let players = text("players", &group_id);
        if !players.is_empty() { put(&mut commands, text("text", "maxGroupSize").replace("{0:i}", &players), Vec2::new(l + 16.0, t + 18.0), r - l - 16.0, 10.0, pale, false); }
    }
    let Some(catalog) = catalog else { return };
    let rarity = |template: &str| catalog.items.get(template).and_then(|d| RARITIES.iter().position(|r| r.eq_ignore_ascii_case(&d.rarity))).unwrap_or(0) as u8;
    let class = players.iter().next().and_then(|c| CLASSES.get(c.desc.class as usize)).copied().unwrap_or("Warrior");
    // The possible rewards: one item, or one per class ("Warrior|x,Mage|y").
    let rewards: Vec<String> = text("rewards", &row).split(';').filter(|r| !r.is_empty()).filter_map(|entry| {
        if !entry.contains('|') { return Some(entry.to_owned()) }
        let options: Vec<(&str, &str)> = entry.split(',').filter_map(|o| o.split_once('|')).collect();
        options.iter().find(|(c, _)| c.eq_ignore_ascii_case(class)).or(options.first()).map(|(_, item)| (*item).to_owned())
    }).collect();
    for (i, template) in rewards.iter().take(12).enumerate() {
        let Some(slot) = widget(&format!("/PossibleRewardsIcons/Item{i}")) else { continue };
        social::item_icon(&mut commands, &assets, &screen, group, catalog, template, rarity(template), slot.k, slot.rect, None);
    }
    // What the teleport takes ("item:count;item:count").
    for (i, need) in text("needs", &row).split(';').filter(|n| !n.is_empty()).take(3).enumerate() {
        let (template, count) = need.split_once(':').unwrap_or((need, "1"));
        let Some(slot) = widget(&format!("/ItemsTop3/Item{i}/Icon0")) else { continue };
        let [l, t, r, b] = slot.rect;
        social::item_icon(&mut commands, &assets, &screen, group, catalog, template, rarity(template), slot.k, [l - 2.5, t - 2.5, r + 2.5, b + 2.5], None);
        if let Some([l, t, r, b]) = rect(&format!("/ItemsTop3/Item{i}/ItemCount")) { put(&mut commands, count.to_owned(), Vec2::new((l + r) * 0.5, (t + b) * 0.5), r - l, 9.5, Color::WHITE, true); }
    }
}
