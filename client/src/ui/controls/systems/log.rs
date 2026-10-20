//! The quest log's achievement, daily challenge and Wanted Thief pages, and the
//! character sheet's titles.
//!
//! The achievements page opens on its overview (each top category's progress,
//! the points earned, the latest achievements); a category lists its
//! achievements (icon, title, text, points, a check when done); a row opens
//! to show its progress bar. The daily page shows the four challenges of the
//! day as cards (their artwork and rank sign, objective, progress): Play
//! accepts one, Shuffle has it replaced, a finished one shows its plate. The
//! Wanted page shows the board's missions on their posters: Wanted accepts
//! one, the bottom button rolls the open ones. The titles tab lists the titles
//! our completed achievements unlock; a row shows it, the radio buttons show
//! none or the PvP rank's.
//! SOURCE: DSOR_Server services/achievements.py (AchievementInfo's values:
//!   progress, then the completion's date), daily_challenges.py (states 0
//!   available, 2 accepted, 3 completed), wanted_thief.py (slots 1..3 accept,
//!   0 refreshes the board), presence_identity.py (SetTitle kinds 0, 1, 2);
//!   _Template_DailyChallenges.Image, _Template_WantedMissionAll.BackgroundImage.
use super::super::template::{clip_x, clone_at, clone_rows, dim, grey, has_part, icon_layers, image_art, is_template, leaf, make_button, rename, selected, set_text, without, Icon};
use super::Sources;
use crate::gameplay::systems::texture_path;
use crate::ui::Layout;

const PAGES: &str = "Canvas/BackGroundLabel";
const ACHIEVEMENTS: &str = "Canvas/BackGroundLabel/AchievementsLabel";
pub(super) const CATEGORIES: &str = "Canvas/BackGroundLabel/AchievementsLabel/CategoriesListBackground/CategoriesList";
pub(super) const ROWS: &str = "Canvas/BackGroundLabel/AchievementsLabel/ContentBackground/AchievementsListLabel/AchievementsList";
const OVERVIEW: &str = "Canvas/BackGroundLabel/AchievementsLabel/ContentBackground/OverviewListLabel";
const DAILIES: &str = "Canvas/BackGroundLabel/DailyAchievementsLabel";
const WANTED: &str = "Canvas/BackGroundLabel/WantedMissionsLabel";
const TITLES: &str = "Canvas/Label/AchievementTitles/BackgroundLabel";
/// The category list: entries from 207 to 627; the achievement list from 154 to 628.
const CATEGORY_TOP: f32 = 207.0;
const CATEGORY_BOTTOM: f32 = 627.0;
const ROW_TOP: f32 = 154.0;
const ROW_BOTTOM: f32 = 628.0;
const OVERVIEW_TOP: f32 = 151.0;
const OVERVIEW_BOTTOM: f32 = 519.0;
/// The overview's entries that fit (75 high each).
pub(super) const OVERVIEW_ROWS: usize = 4;
/// The four cards' slots (TemplateSlot0..3) start at these x; a card's
/// template (from x 178) is laid at its slot's left.
const SLOT_X: [f32; 4] = [136.0, 330.0, 526.0, 722.0];
const CARD_X: f32 = 178.0;
const CHECK: &str = "textures/ui/icon_checkmark.png";
/// The title rows: 21 high from 473 (253 for the PvP list); 8 fit.
const TITLE_HEIGHT: f32 = 21.0;
const MOST_TITLES: usize = 8;
const MOST_RECENT: usize = 7;
const AVAILABLE: u8 = 0;
const COMPLETED: u8 = 3;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Category {
    pub id: String,
    pub title: String,
    pub sub: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct AchievementRow {
    pub id: String,
    pub title: String,
    pub description: String,
    pub icon: String,
    pub points: u32,
    pub progress: (u32, u32),
    pub done: bool,
    pub hidden: bool,
    pub open: bool,
    pub title_reward: String,
}

/// A top category's progress on the overview: icon, title, (done, total).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct CategoryProgress {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub done: u32,
    pub total: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Daily {
    pub id: String,
    pub description: String,
    pub progress: (u32, u32),
    pub state: u8,
    pub tier: u32,
    /// The card's sign (Generic, Boss, PvP, Cooking).
    pub sign: String,
    pub image: String,
}

/// A bonus for the dailies solved today: at how many, its reward, reached.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Bonus {
    pub at: u32,
    pub icon: Icon,
    pub amount: String,
    pub done: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct WantedCard {
    pub slot: u32,
    pub title: String,
    pub rarity: String,
    pub reward: Icon,
    pub status: u32,
    pub image: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct WantedBoard {
    pub remaining: u32,
    /// The privileged missions left and a day's.
    pub special: (u32, u32),
    pub cards: Vec<WantedCard>,
    /// The time to the board's reset (midnight UTC).
    pub reset: String,
    /// Refreshing the whole board's Andermant.
    pub refresh_cost: u32,
}

/// The quest log's three pages.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Log {
    pub categories: Vec<Category>,
    /// None: the overview.
    pub category: Option<String>,
    pub achievements: Vec<AchievementRow>,
    pub overview: Vec<CategoryProgress>,
    pub points: u32,
    pub recent: Vec<String>,
    /// Rows scrolled off the category list and off the achievement list.
    pub scroll: (usize, usize),
    pub dailies: Vec<Daily>,
    /// The bonuses for the dailies solved today, and how many are.
    pub bonuses: Vec<Bonus>,
    pub solved: u32,
    /// A daily's reshuffle's Andermant.
    pub reshuffle: u32,
    pub wanted: Option<WantedBoard>,
}

pub(super) fn log(sources: &Sources) -> Log {
    let catalog = sources.catalog;
    let net = sources.net;
    let state = sources.state;
    let mut tops: Vec<_> = catalog.achievement_categories.iter().filter(|c| c.parent.is_empty()).collect();
    tops.sort_by_key(|c| (c.priority, c.id.clone()));
    let chosen = state.achievement_category.clone().filter(|c| catalog.achievement_categories.iter().any(|k| &k.id == c));
    let chosen_top = chosen.as_ref().and_then(|c| catalog.achievement_categories.iter().find(|k| &k.id == c))
        .map(|c| if c.parent.is_empty() { c.id.clone() } else { c.parent.clone() });
    let mut categories = Vec::new();
    for top in &tops {
        categories.push(Category { id: top.id.clone(), title: top.title.clone(), sub: false, selected: chosen.as_ref() == Some(&top.id) });
        if chosen_top.as_ref() != Some(&top.id) { continue; }
        let mut subs: Vec<_> = catalog.achievement_categories.iter().filter(|c| c.parent == top.id).collect();
        subs.sort_by_key(|c| (c.priority, c.id.clone()));
        categories.extend(subs.iter().map(|c| Category { id: c.id.clone(), title: c.title.clone(), sub: true, selected: chosen.as_ref() == Some(&c.id) }));
    }
    let done = |id: &str| net.achievements.get(id).is_some_and(|v| v[1] != 0);
    // A top category holds its own achievements and its sub-categories'.
    let under = |category: &str, top: &str| category == top || catalog.achievement_categories.iter().any(|c| c.id == category && c.parent == top);
    let mut achievements: Vec<AchievementRow> = chosen.as_ref().map(|chosen| {
        let whole = chosen_top.as_ref() == Some(chosen);
        catalog.achievements.iter().filter(|(_, a)| if whole { under(&a.category, chosen) } else { &a.category == chosen }).map(|(id, a)| {
            let values = net.achievements.get(id).copied().unwrap_or_default();
            AchievementRow { id: id.clone(), title: a.title.clone(), description: a.description.clone(), icon: a.icon.clone(), points: a.points,
                progress: (values[0].min(a.amount), a.amount), done: values[1] != 0, hidden: a.hidden,
                open: state.achievement_open.as_deref() == Some(id.as_str()), title_reward: a.title_text.clone() }
        }).collect()
    }).unwrap_or_default();
    // The ones under way first, then the completed, then the rest.
    achievements.sort_by_key(|a| (if !a.done && a.progress.0 > 0 { 0 } else if a.done { 1 } else { 2 }, a.hidden, a.title.clone(), a.id.clone()));
    let overview = tops.iter().map(|top| {
        let all: Vec<&String> = catalog.achievements.iter().filter(|(_, a)| under(&a.category, &top.id)).map(|(id, _)| id).collect();
        CategoryProgress { id: top.id.clone(), title: top.title.clone(), icon: top.icon.clone(), done: all.iter().filter(|id| done(id)).count() as u32, total: all.len() as u32 }
    }).filter(|c| c.total > 0).collect();
    let points = catalog.achievements.iter().filter(|(id, _)| done(id)).map(|(_, a)| a.points).sum();
    // The latest completed: by their completion date (year, month, day, hour, minute, second).
    let mut recent: Vec<(&String, [u32; 6])> = net.achievements.iter().filter(|(_, v)| v[1] != 0)
        .map(|(id, v)| (id, [v[1], v[8], v[2], v[3], v[4], v[5]])).collect();
    recent.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let recent = recent.into_iter().filter_map(|(id, _)| catalog.achievements.get(id).map(|a| a.icon.clone())).take(MOST_RECENT).collect();
    let dailies = net.challenges.as_ref().map(|c| c.daily.iter().take(4).map(|d| {
        let def = catalog.challenges.get(&d.id);
        let target = def.map_or(1, |d| d.value.max(1));
        let sign = match def.map_or("", |d| d.category.as_str()) { "Boss" => "Boss", "PvP" => "PvP", "Cooking" => "Cooking", _ => "Generic" };
        Daily { id: d.id.clone(), description: def.map(|d| d.description.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| crate::ui::dialogue::readable(&d.id)),
            progress: (d.progress.min(target), target), state: d.state, tier: def.map_or(1, |d| d.tier.clamp(1, 3)), sign: sign.into(),
            image: def.map(|d| d.image.clone()).unwrap_or_default() }
    }).collect()).unwrap_or_default();
    // The bonuses: the catalogue's in order, reached as the server says.
    let solved = net.challenges.as_ref().map_or(0, |c| {
        let daily = c.daily.iter().filter(|d| d.state == COMPLETED).count() as u32;
        c.bonuses.iter().map(|b| b.progress).max().unwrap_or(0).max(daily)
    });
    let mut bonus_defs: Vec<(&String, &crate::gameplay::systems::ChallengeDef)> = catalog.challenges.iter().filter(|(_, d)| d.category == "BonusReward").collect();
    bonus_defs.sort_by_key(|(id, d)| (d.value, d.tier, (*id).clone()));
    let bonuses = bonus_defs.into_iter().map(|(id, d)| {
        let (icon, amount) = reward(sources, &d.reward);
        let done = net.challenges.as_ref().is_some_and(|c| c.bonuses.iter().any(|b| &b.id == id && b.state == COMPLETED));
        Bonus { at: d.value, icon, amount, done }
    }).collect();
    let figure = |name: &str| catalog.figures.get(name).copied().unwrap_or(0);
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let left = 86_400 - seconds % 86_400;
    let wanted = net.wanted.as_ref().map(|w| WantedBoard {
        remaining: w.remaining,
        special: (w.special_remaining, figure("wanted_vip")),
        reset: format!("{} h {:02} min", left / 3600, left % 3600 / 60),
        refresh_cost: figure("wanted_all_refresh"),
        cards: w.missions.iter().enumerate().take(4).map(|(i, mission)| {
            let def = catalog.wanted.get(&mission.to_string());
            WantedCard {
                slot: i as u32 + 1,
                title: def.map(|d| d.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| format!("Mission {mission}")),
                rarity: def.map(|d| d.rarity.clone()).filter(|r| !r.is_empty()).unwrap_or_else(|| "Magic".to_owned()),
                reward: def.map(|d| sources.icon(&d.reward, None)).unwrap_or_default(),
                status: w.statuses.get(i).copied().unwrap_or(0),
                image: def.map(|d| d.image.clone()).unwrap_or_default(),
            }
        }).collect(),
    });
    let scroll = (state.scroll.get(CATEGORIES).copied().unwrap_or(0), state.scroll.get(ROWS).copied().unwrap_or(0));
    Log { categories, category: chosen, achievements, overview, points, recent, scroll, dailies, bonuses, solved, reshuffle: figure("daily_reshuffle"), wanted }
}

/// A challenge's reward ("Loot:<item>,<n>", "VC:<copper>", "RC:<n>") as an
/// icon and the amount written on it.
fn reward(sources: &Sources, reward: &str) -> (Icon, String) {
    let (kind, value) = reward.split_once(':').unwrap_or(("", reward));
    match kind {
        "VC" => {
            let copper: u64 = value.trim().parse().unwrap_or(0);
            let amount = if copper >= 10_000 { format!("{}g", copper / 10_000) } else if copper >= 100 { format!("{}s", copper / 100) } else { format!("{copper}c") };
            (Icon { image: texture_path("icons/all_coins_gold"), ..Default::default() }, amount)
        }
        "RC" => (Icon { image: texture_path("icons/money_rc_pile"), ..Default::default() }, value.trim().to_owned()),
        _ => {
            let (item, amount) = value.split_once(',').unwrap_or((value, "1"));
            let amount: u32 = amount.trim().parse().unwrap_or(1);
            (sources.icon(item.trim(), None), if amount > 1 { amount.to_string() } else { String::new() })
        }
    }
}

/// The dailies' bottom bar: the bonuses (their rewards, how many dailies
/// each needs, a check once reached) and the next one's.
fn bonus_bar(source: &Layout, log: &Log, layout: &mut Layout) {
    let bar = format!("{DAILIES}/BottomBar/");
    let next = log.bonuses.iter().find(|b| !b.done);
    for w in source.widgets.iter().filter(|w| w.id.starts_with(&bar)) {
        let mut w = w.clone();
        let rest = w.id[bar.len()..].to_owned();
        let parts: Vec<&str> = rest.split('/').collect();
        let bonus = match parts[0] {
            "NextReward" => match next { Some(b) => Some((b, false)), None => continue },
            slot => match slot.strip_prefix("Reward_").and_then(|n| n.parse::<usize>().ok()) {
                Some(n) => match log.bonuses.get(n.wrapping_sub(1)) { Some(b) => Some((b, b.done)), None => continue },
                None => None,
            },
        };
        if parts.contains(&"Check") {
            if !bonus.is_some_and(|(_, done)| done) { continue; }
            // The innermost check carries the mark (a runtime texture).
            if parts.len() == 4 { w.art = Some(image_art(w.rect, CHECK.into())); }
            w.hidden = false;
            layout.widgets.push(w);
            continue;
        }
        match (bonus, parts.len(), parts[0]) {
            (Some((b, _)), 1, _) => {
                layout.widgets.extend(icon_layers(&mut w, &b.icon));
                set_text(&mut w, &b.amount);
            }
            (Some((b, _)), _, _) => set_text(&mut w, &b.at.to_string()),
            (None, 1, "NextRewardText") => set_text(&mut w, if next.is_some() { "Next bonus" } else { "Every bonus received today" }),
            (None, 1, "Bonusreward") => set_text(&mut w, &match next { Some(b) => format!("Solved today: {} / {}", log.solved, b.at), None => format!("Solved today: {}", log.solved) }),
            (None, _, "MentorBonus") => continue,
            _ => {}
        }
        w.hidden = false;
        layout.widgets.push(w);
    }
}

/// How far a card's clone moves: from the template to the n-th slot.
fn slot_dx(source: &Layout, page: &str, n: usize) -> f32 {
    let x = |id: String| source.widgets.iter().find(|w| w.id == id).map(|w| w.rect[0]);
    match (x(format!("{page}/TemplateSlot{n}")), x(format!("{page}/Template"))) {
        (Some(slot), Some(template)) => slot - template,
        _ => SLOT_X[n] - CARD_X,
    }
}

/// Entries of a List stacked from `top`, as quests::Stack (each template's own
/// height), skipping `skip` and stopping at `bottom`.
fn stack<'a>(source: &'a Layout, entries: impl IntoIterator<Item = (String, String)>, top: f32, bottom: f32, skip: usize) -> Vec<(String, Vec<crate::ui::Widget>)> {
    let mut y = top;
    let mut out = Vec::new();
    for (n, (template, replacement)) in entries.into_iter().enumerate() {
        let Some(root) = source.widgets.iter().find(|w| w.id == template) else { continue };
        if n < skip { continue; }
        let height = root.rect[3] - root.rect[1];
        if y + height > bottom + 0.5 { break; }
        out.push((replacement.clone(), clone_rows(source, &template, &replacement, y - root.rect[1]).collect()));
        y += height;
    }
    out
}

/// The quest log's achievement, daily and Wanted pages, in place of their
/// templates (the quest page is super::super::quests::prepare_log's).
pub(super) fn prepare_questlog(source: &Layout, log: &Log) -> Layout {
    let page = |id: &str, name: &str| id.starts_with(&format!("{PAGES}/{name}/"));
    let overview = log.category.is_none();
    let mut layout = without(source, |id| {
        ((page(id, "AchievementsLabel") || page(id, "DailyAchievementsLabel") || page(id, "WantedMissionsLabel")) && is_template(id))
            || (id.starts_with(&format!("{DAILIES}/BottomBar/")) && id != format!("{DAILIES}/BottomBar/TextLabel"))
            || (log.wanted.is_some() && id == format!("{WANTED}/LoadingLabel"))
            || (overview && id.starts_with(&format!("{ACHIEVEMENTS}/ContentBackground/AchievementsListLabel")))
            || (!overview && id.starts_with(OVERVIEW))
            || id.contains("/RecentAchievementsLabel/RecentAchievement")
    });
    if overview {
        for w in &mut layout.widgets {
            if w.id.ends_with("/CategoriesOverviewtBackground/OverviewButton") { selected(w); }
            if w.button.as_deref().is_some_and(|b| b.ends_with("/CategoriesOverviewtBackground/OverviewButton")) { selected(w); }
        }
    }
    // The categories, the open one's sub-categories under it.
    let entries = log.categories.iter().enumerate().map(|(n, c)| (format!("{CATEGORIES}/{}", if c.sub { "TemplateSubCategory" } else { "TemplateCategory" }), format!("{CATEGORIES}/Cat{n}")));
    for (replacement, rows) in stack(source, entries, CATEGORY_TOP, CATEGORY_BOTTOM, log.scroll.0) {
        let n: usize = replacement.rsplit("Cat").next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let category = &log.categories[n];
        for mut w in rows {
            if w.id == replacement { w.event = format!("Sys:Achievements:{}", category.id); }
            if w.text.is_some() { set_text(&mut w, &category.title); }
            if category.selected { selected(&mut w); }
            layout.widgets.push(w);
        }
    }
    super::super::quests::place_handle(&mut layout, &format!("{CATEGORIES}/ScrollBarBackground/ScrollBarBoundaries"), log.scroll.0, log.categories.len());
    if overview { overview_page(source, log, &mut layout); } else { achievement_rows(source, log, &mut layout); }
    dailies(source, log, &mut layout);
    bonus_bar(source, log, &mut layout);
    wanted(source, log, &mut layout);
    layout
}

/// The overview's entries: the category pairs, then the points.
pub(super) fn overview_entries(log: &Log) -> usize { log.overview.len().div_ceil(2) + 1 }

/// The overview: each top category's progress (two a row; the element's
/// button is its face, drawn under its icon and bar), the points, the latest
/// achievements; it scrolls with the achievement list's scroll.
fn overview_page(source: &Layout, log: &Log, layout: &mut Layout) {
    let list = format!("{OVERVIEW}/OverviewList");
    let pairs = log.overview.chunks(2).count();
    let mut entries: Vec<(String, String)> = (0..pairs).map(|n| (format!("{list}/TemplateOverviewElements"), format!("{list}/Row{n}"))).collect();
    entries.push((format!("{list}/TemplateOverviewAchievementPoints"), format!("{list}/Points")));
    for (replacement, rows) in stack(source, entries, OVERVIEW_TOP, OVERVIEW_BOTTOM, log.scroll.1) {
        if replacement.ends_with("/Points") {
            for mut w in rows {
                if w.id.ends_with("/FrameLabel/TextLabel") { set_text(&mut w, &log.points.to_string()); }
                layout.widgets.push(w);
            }
            continue;
        }
        let row: usize = replacement.rsplit("Row").next().and_then(|s| s.parse().ok()).unwrap_or(0);
        // The row's and the elements' backgrounds, then the faces (with their
        // titles), then the icons and bars.
        let (mut backs, mut faces, mut content) = (Vec::new(), Vec::new(), Vec::new());
        for mut w in rows {
            rename(&mut w, "TemplateOverviewElement0", "Element0");
            rename(&mut w, "TemplateOverviewElement1", "Element1");
            let side = if has_part(&w.id, "Element1") { 1 } else { 0 };
            let Some(category) = log.overview.get(row * 2 + side) else { continue };
            let fraction = category.done as f32 / category.total.max(1) as f32;
            let face = has_part(&w.id, "TitleButton");
            match leaf(&w.id) {
                "IconLabel" if !face => {
                    let icon = Icon { image: texture_path(&category.icon), ..Default::default() };
                    content.extend(icon_layers(&mut w, &icon));
                }
                "progress" => clip_x(&mut w, fraction),
                "progresspoints" => {
                    set_text(&mut w, &format!("{} / {}", category.done, category.total));
                    if let Some(t) = &mut w.text { t.color[3] = 1.0; }
                }
                "TextLabel" if face => set_text(&mut w, &category.title),
                "TitleButton" => make_button(&mut w, &format!("Sys:Achievements:{}", category.id)),
                _ => {}
            }
            let back = w.id == replacement || w.id == format!("{replacement}/Element0") || w.id == format!("{replacement}/Element1");
            if back { backs.push(w); } else if face { faces.push(w); } else { content.push(w); }
        }
        layout.widgets.extend(backs);
        layout.widgets.extend(faces);
        layout.widgets.extend(content);
    }
    super::super::quests::place_handle(layout, &format!("{list}/ScrollBarBackground/ScrollBarBoundaries"), log.scroll.1, overview_entries(log));
    // The latest achievements, as icons in their slots.
    for (n, icon) in log.recent.iter().enumerate() {
        let Some(slot) = source.widgets.iter().find(|w| w.id == format!("{OVERVIEW}/RecentAchievementsLabel/RecentAchievement{n}")) else { continue };
        let mut w = slot.clone();
        w.id = format!("{OVERVIEW}/RecentAchievementsLabel/Recent{n}");
        w.kind = "Label".into();
        w.event.clear();
        w.art = Some(image_art(w.rect, texture_path(icon)));
        layout.widgets.push(w);
    }
}

/// A category's achievements: title rows; an open row adds its progress.
fn achievement_rows(source: &Layout, log: &Log, layout: &mut Layout) {
    let mut entries = Vec::new();
    for (n, a) in log.achievements.iter().enumerate() {
        let template = if a.hidden && !a.done { "TemplateHidden" } else { "TemplateTitle" };
        entries.push((format!("{ROWS}/{template}"), format!("{ROWS}/Row{n}")));
        if a.open && a.progress.1 > 1 && !a.done { entries.push((format!("{ROWS}/TemplateProgress"), format!("{ROWS}/Progress{n}"))); }
    }
    for (replacement, rows) in stack(source, entries, ROW_TOP, ROW_BOTTOM, log.scroll.1) {
        let progress = replacement.contains("/Progress");
        let n: usize = replacement.rsplit(if progress { "Progress" } else { "Row" }).next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let a = &log.achievements[n];
        let extended = a.open && a.progress.1 > 1 && !a.done;
        // The row's backgrounds come after its content in the layout; the
        // game draws them under it.
        let (backgrounds, rows): (Vec<_>, Vec<_>) = rows.into_iter().partition(|w| {
            let rest = &w.id[replacement.len()..];
            rest == "/BackgroundLabel" || rest == "/BackgroundLabelBottom" || rest.starts_with("/BackgroundLabel/") || rest.starts_with("/BackgroundLabelBottom/")
        });
        for mut w in backgrounds.into_iter().chain(rows) {
            let name = leaf(&w.id).to_owned();
            let rest = w.id[replacement.len()..].to_owned();
            if progress {
                // The progress part closes the row.
                if rest.ends_with("/BackgroundLabel") || rest.contains("/AchievementPoints") { continue; }
                match name.as_str() {
                    "progress" => clip_x(&mut w, a.progress.0 as f32 / a.progress.1.max(1) as f32),
                    "ProgressTextLabel" => set_text(&mut w, &format!("{} / {}", a.progress.0, a.progress.1)),
                    "progresspoints" => set_text(&mut w, ""),
                    _ => {}
                }
                w.hidden = false;
                layout.widgets.push(w);
                continue;
            }
            // The row's background: open at its bottom when its progress follows.
            if rest == "/BackgroundLabel" { if extended { w.hidden = false; } else { continue; } }
            if rest == "/BackgroundLabelBottom" { if extended { continue; } else { w.hidden = false; } }
            if rest.contains("/HiddenAchievementLabel") && !(a.hidden && a.done) { continue; }
            if rest.contains("/CompleteLabel/CompleteLabel") && !a.done { continue; }
            if rest.contains("/AchievementPointsCompletedLabel") && !a.done { continue; }
            if (rest.ends_with("/AchievementPointsLabel") || rest.contains("/AchievementPointsLabel/")) && a.done { continue; }
            if rest.contains("/RewardParentLabel") && a.title_reward.is_empty() { continue; }
            if w.id == replacement { make_button(&mut w, &format!("Sys:Achievement:{}", a.id)); }
            match name.as_str() {
                "TitleLabel" if !(a.hidden && !a.done) => set_text(&mut w, &a.title),
                "DescriptionLabel" if !(a.hidden && !a.done) => set_text(&mut w, &if a.done || a.progress.1 <= 1 || a.open { a.description.clone() } else { format!("{} ({}/{})", a.description, a.progress.0, a.progress.1) }),
                "TextLabel" => set_text(&mut w, &a.points.to_string()),
                "IconLabel" if !(a.hidden && !a.done) => {
                    let mut layers = icon_layers(&mut w, &Icon { image: texture_path(&a.icon), ..Default::default() });
                    if !a.done { for l in &mut layers { dim(l, 0.55); } }
                    layout.widgets.extend(layers);
                }
                _ => {}
            }
            layout.widgets.push(w);
        }
    }
    let entries = log.achievements.len();
    super::super::quests::place_handle(layout, &format!("{ROWS}/ScrollBarBackground/ScrollBarBoundaries"), log.scroll.1, entries);
}

/// The daily cards: artwork, sign, objective, progress, buttons or the plate.
fn dailies(source: &Layout, log: &Log, layout: &mut Layout) {
    for (n, daily) in log.dailies.iter().enumerate() {
        let replacement = format!("{DAILIES}/Slot{n}");
        let mut texts = Vec::new();
        for mut w in clone_at(source, &format!("{DAILIES}/Template"), &replacement, slot_dx(source, DAILIES, n), 0.0) {
            rename(&mut w, "TemplateText", "Text");
            let rest = w.id[replacement.len()..].to_owned();
            let parts: Vec<&str> = rest.split('/').collect();
            if rest == "/Card/Label" || rest.contains("/ArtworkSwitchAnimation") || rest.contains("/Rewards") || rest.contains("/AnimationNextTier") || rest.contains("/PrivilegeLabel") { continue; }
            if parts.iter().find_map(|p| p.strip_prefix("SignChallenge")).is_some_and(|sign| sign != format!("{}Tier{}", daily.sign, daily.tier)) { continue; }
            if parts.iter().find_map(|p| p.strip_prefix("FinishedChallenge")).is_some_and(|fin| daily.state != COMPLETED || fin != format!("Tier{}NoAnim", daily.tier)) { continue; }
            if parts.iter().any(|p| matches!(*p, "PlayButton" | "ButtonShuffle")) && daily.state != AVAILABLE { continue; }
            if parts.contains(&"SignChallenge") { continue; }
            w.hidden = w.hidden && !rest.contains("NoAnim") && !rest.contains("SignChallenge");
            // The challenge's artwork in place of the template's (the same
            // layout: the card shows the texture's upper part).
            if rest == "/Card" && !daily.image.is_empty() {
                if let Some(art) = &mut w.art { art.tex = Some(texture_path(&daily.image)); }
            }
            match leaf(&w.id) {
                "Value" => set_text(&mut w, &daily.description),
                "ButtonLabel" => set_text(&mut w, &log.reshuffle.to_string()),
                "ProgressTextLabel" => set_text(&mut w, &format!("{} / {}", daily.progress.0, daily.progress.1)),
                "progresspoints" => set_text(&mut w, ""),
                "progress" => clip_x(&mut w, daily.progress.0 as f32 / daily.progress.1.max(1) as f32),
                "PlayButton" => w.event = format!("Sys:Daily:{}:0", daily.id),
                "ButtonShuffle" => w.event = format!("Sys:Daily:{}:1", daily.id),
                _ => {}
            }
            // The objective's text lies over the card, not under its artwork.
            if rest.starts_with("/Text") { texts.push(w); } else { layout.widgets.push(w); }
        }
        layout.widgets.extend(texts);
    }
}

/// The Wanted posters: artwork, rarity, name, reward, accept or its state.
fn wanted(source: &Layout, log: &Log, layout: &mut Layout) {
    let Some(board) = &log.wanted else { return };
    for w in &mut layout.widgets {
        match leaf(&w.id) {
            "TaskCount" if w.id.starts_with(WANTED) => set_text(w, &format!("Missions left today: {}", board.remaining)),
            "VipTime" if w.id.starts_with(WANTED) => set_text(w, &board.reset),
            "price" if w.id.starts_with(&format!("{WANTED}/BottomBar/Confirm/")) => set_text(w, &board.refresh_cost.to_string()),
            _ => {}
        }
    }
    for (n, card) in board.cards.iter().enumerate() {
        let replacement = format!("{WANTED}/Slot{n}/Card");
        for mut w in clone_at(source, &format!("{WANTED}/Template/Card_new"), &replacement, slot_dx(source, WANTED, n), 0.0) {
            let rest = w.id[replacement.len()..].to_owned();
            let parts: Vec<&str> = rest.split('/').collect();
            // The fourth poster is the privileged one (a membership's); the
            // single-card refresh lies over the accept button (the bottom
            // button refreshes the board).
            if parts.iter().any(|p| matches!(*p, "Reward_2" | "Reward_3" | "help" | "help1" | "Slot3" | "durationTxt" | "RefreshButton")) { continue; }
            if parts.contains(&"VIPButton") && card.slot <= 3 { continue; }
            if rest.starts_with("/Slot/rarity/") && parts.get(3).is_some_and(|r| *r != card.rarity) { continue; }
            if parts.contains(&"ProgressTextLabel") && card.status != 1 { continue; }
            if parts.contains(&"WantedButton") && (card.status != 0 || card.slot > 3) { continue; }
            if rest == "/card" && !card.image.is_empty() { w.art = Some(image_art(w.rect, texture_path(&card.image))); }
            match leaf(&w.id) {
                "Brief" => set_text(&mut w, &card.title),
                "rarityTxt" => set_text(&mut w, &card.rarity),
                "WantedButton" => w.event = format!("Sys:Wanted:{}", card.slot),
                "Reward_1" => { layout.widgets.extend(icon_layers(&mut w, &card.reward)); set_text(&mut w, ""); }
                // The privileged poster's count: "Secret 5/5".
                "TextLabel" if parts.contains(&"VIPButton") => {
                    let label = w.text.as_ref().map(|t| t.text.split("{0").next().unwrap_or("").trim().to_owned()).unwrap_or_default();
                    set_text(&mut w, &format!("{label} {}/{}", board.special.0, board.special.1));
                }
                _ => {}
            }
            if rest.starts_with("/Slot/rarity/") { w.hidden = false; }
            layout.widgets.push(w);
        }
    }
}

/// The character sheet's titles tab.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Titles {
    /// The title shown: SetTitle's (kind, achievement).
    pub current: (u8, String),
    pub titles: Vec<(String, String)>,
    /// The PvP rank's title, if any.
    pub rank: String,
}

pub(super) fn titles(sources: &Sources) -> Titles {
    let mut titles: Vec<(String, String)> = sources.catalog.achievements.iter()
        .filter(|(id, a)| !a.title_text.is_empty() && sources.net.achievements.get(*id).is_some_and(|v| v[1] != 0))
        .map(|(id, a)| (id.clone(), a.title_text.clone())).collect();
    titles.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    // The honour levels' titles: one per 1000 badges of honour, as far as known.
    let honor = sources.net.wallet.map_or(0, |w| w[2]);
    let level = ((honor / 1000) as usize + usize::from(honor > 0)).min(sources.catalog.pvp_ranks.len().saturating_sub(1));
    let rank = if level > 0 { sources.catalog.pvp_ranks.get(level).cloned().unwrap_or_default() } else { String::new() };
    Titles { current: sources.state.title.clone().unwrap_or_default(), titles, rank }
}

/// The titles tab: the titles earned as rows, the one shown named and its
/// radio button set.
pub(super) fn prepare_sheet(source: &Layout, titles: &Titles) -> Layout {
    let mut layout = without(source, |id| id.starts_with(TITLES) && is_template(id));
    let current = match titles.current.0 {
        1 => titles.titles.iter().find(|(id, _)| *id == titles.current.1).map(|(_, t)| t.clone()).unwrap_or_else(|| titles.current.1.clone()),
        2 if !titles.rank.is_empty() => titles.rank.clone(),
        2 => "PvP rank".to_owned(),
        _ => "None".to_owned(),
    };
    let radio = match titles.current.0 { 1 => "RadioButtonShowAchievementTitle", 2 => "RadioButtonShowPVPTitle", _ => "RadioButtonDoNotShowTitle" };
    for w in &mut layout.widgets {
        if w.id == format!("{TITLES}/Headline/TextLabelTitle") { set_text(w, &current); }
        if w.button.as_deref().is_some_and(|b| b.ends_with(radio)) { selected(w); }
        // Its authored event repeats the PvP one.
        if w.id.ends_with("/RadioButtonShowAchievementTitle") {
            w.event = titles.titles.first().map_or(String::new(), |(id, _)| format!("Sys:Title:{id}"));
        }
    }
    let lists = [(format!("{TITLES}/ShowPVETitle/ListAchievementTitle"), titles.titles.clone()),
        (format!("{TITLES}/ShowPVPTitle/ListPVPTitle"), if titles.rank.is_empty() { Vec::new() } else { vec![(String::new(), titles.rank.clone())] })];
    for (list, rows) in lists {
        let pvp = list.ends_with("ListPVPTitle");
        for (n, (id, text)) in rows.iter().take(MOST_TITLES).enumerate() {
            for mut w in clone_rows(source, &format!("{list}/TemplateButton"), &format!("{list}/Title{n}"), n as f32 * TITLE_HEIGHT) {
                if w.id == format!("{list}/Title{n}") { w.event = if pvp { "ShowPVPTitle".into() } else { format!("Sys:Title:{id}") }; }
                if w.text.is_some() { set_text(&mut w, text); }
                let shown = if pvp { titles.current.0 == 2 } else { titles.current == (1, id.clone()) };
                let list_on = titles.current.0 == if pvp { 2 } else { 1 };
                if shown { selected(&mut w); } else if !list_on { grey(&mut w); }
                layout.widgets.push(w);
            }
        }
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::template::fixture::{find, layout, text_of};

    #[test]
    fn the_achievement_page_shows_its_overview_then_a_category_s_rows() {
        let list = format!("{OVERVIEW}/OverviewList");
        let ids: Vec<(String, &str, bool)> = vec![(format!("{CATEGORIES}/TemplateCategory"), "Button", false), (format!("{CATEGORIES}/TemplateCategory/TextLabel"), "TextLabel", true),
            (format!("{CATEGORIES}/TemplateSubCategory"), "Button", false), (format!("{CATEGORIES}/TemplateSubCategory/TextLabel"), "TextLabel", true),
            (format!("{ROWS}/TemplateTitle"), "Label", false), (format!("{ROWS}/TemplateTitle/ContentLabel/TitleLabel"), "TextLabel", true), (format!("{ROWS}/TemplateTitle/ContentLabel/DescriptionLabel"), "TextLabel", true),
            (format!("{ROWS}/TemplateTitle/ContentLabel/IconLabel"), "Icon", false), (format!("{ROWS}/TemplateTitle/ContentLabel/AchievementPointsLabel/TextLabel"), "TextLabel", true),
            (format!("{ROWS}/TemplateTitle/ContentLabel/AchievementPointsCompletedLabel/TextLabel"), "TextLabel", true), (format!("{ROWS}/TemplateTitle/BackgroundLabel"), "Label", false),
            (format!("{ROWS}/TemplateTitle/BackgroundLabelBottom"), "Label", false), (format!("{ROWS}/TemplateProgress"), "Label", false),
            (format!("{ROWS}/TemplateProgress/ContentLabel/ProgressBarLabel/ProgressTextLabel"), "TextLabel", true), (format!("{ROWS}/TemplateProgress/ContentLabel/ProgressBarLabel/ProgressBar/progress"), "Label", false),
            (format!("{list}/TemplateOverviewElements"), "Label", false), (format!("{list}/TemplateOverviewElements/TemplateOverviewElement0"), "Label", false),
            (format!("{list}/TemplateOverviewElements/TemplateOverviewElement0/ContentLabel/ProgressBar/progress"), "Label", false),
            (format!("{list}/TemplateOverviewElements/TemplateOverviewElement0/ContentLabel/ProgressBar/progress/progresspoints"), "TextLabel", true),
            (format!("{list}/TemplateOverviewElements/TemplateOverviewElement0/TitleButton"), "Button", false), (format!("{list}/TemplateOverviewElements/TemplateOverviewElement0/TitleButton/normal"), "Label", false),
            (format!("{list}/TemplateOverviewElements/TemplateOverviewElement0/TitleButton/normal/TextLabel"), "TextLabel", true),
            (format!("{list}/TemplateOverviewElements/TemplateOverviewElement1/ContentLabel/ProgressBar/progress/progresspoints"), "TextLabel", true),
            (format!("{list}/TemplateOverviewAchievementPoints"), "Label", false), (format!("{list}/TemplateOverviewAchievementPoints/FrameLabel/TextLabel"), "TextLabel", true)];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let source = layout("questlog", &refs);
        let mut log = Log {
            categories: vec![Category { id: "General".into(), title: "General".into(), ..Default::default() }, Category { id: "Lore".into(), title: "Lore".into(), ..Default::default() }],
            overview: vec![CategoryProgress { id: "General".into(), title: "General".into(), done: 3, total: 10, ..Default::default() }, CategoryProgress { id: "Lore".into(), done: 1, total: 4, ..Default::default() }, CategoryProgress { id: "PvP".into(), done: 0, total: 2, ..Default::default() }],
            points: 120, ..Default::default()
        };
        let prepared = prepare_questlog(&source, &log);
        assert_eq!(find(&prepared, &format!("{CATEGORIES}/Cat1")).event, "Sys:Achievements:Lore");
        assert_eq!(text_of(&prepared, &format!("{list}/Row0/Element1/ContentLabel/ProgressBar/progress/progresspoints")), "1 / 4");
        assert_eq!(text_of(&prepared, &format!("{list}/Row1/Element0/ContentLabel/ProgressBar/progress/progresspoints")), "0 / 2");
        assert!(!prepared.widgets.iter().any(|w| w.id.starts_with(&format!("{list}/Row1/Element1"))));
        assert_eq!(text_of(&prepared, &format!("{list}/Points/FrameLabel/TextLabel")), "120");
        assert_eq!(find(&prepared, &format!("{list}/Row0/Element0/ContentLabel/ProgressBar/progress/progresspoints")).text.as_ref().unwrap().color[3], 1.0);
        let at = |id: &str| prepared.widgets.iter().position(|w| w.id == id).unwrap_or_else(|| panic!("no {id}"));
        assert!(at(&format!("{list}/Row0/Element0/TitleButton")) < at(&format!("{list}/Row0/Element0/ContentLabel/ProgressBar/progress")));
        // The element's own background goes under its face and title.
        assert!(at(&format!("{list}/Row0/Element0")) < at(&format!("{list}/Row0/Element0/TitleButton/normal/TextLabel")));
        assert_eq!(text_of(&prepared, &format!("{list}/Row0/Element0/TitleButton/normal/TextLabel")), "General");
        log.category = Some("Lore".into());
        log.achievements = vec![AchievementRow { id: "a".into(), title: "Regicide".into(), description: "Defeat the king.".into(), points: 10, progress: (1, 3), open: true, ..Default::default() },
            AchievementRow { id: "b".into(), title: "Done".into(), points: 5, progress: (1, 1), done: true, ..Default::default() }];
        let prepared = prepare_questlog(&source, &log);
        assert!(!prepared.widgets.iter().any(|w| w.id.starts_with(OVERVIEW)));
        assert_eq!(find(&prepared, &format!("{ROWS}/Row0")).event, "Sys:Achievement:a");
        assert_eq!(text_of(&prepared, &format!("{ROWS}/Progress0/ContentLabel/ProgressBarLabel/ProgressTextLabel")), "1 / 3");
        assert!(prepared.widgets.iter().any(|w| w.id == format!("{ROWS}/Row0/BackgroundLabel")) && !prepared.widgets.iter().any(|w| w.id == format!("{ROWS}/Row0/BackgroundLabelBottom")));
        assert!(prepared.widgets.iter().any(|w| w.id == format!("{ROWS}/Row1/BackgroundLabelBottom")));
        let at = |id: &str| prepared.widgets.iter().position(|w| w.id == id).unwrap_or_else(|| panic!("no {id}"));
        assert!(at(&format!("{ROWS}/Row1/BackgroundLabelBottom")) < at(&format!("{ROWS}/Row1/ContentLabel/TitleLabel")));
        assert_eq!(text_of(&prepared, &format!("{ROWS}/Row1/ContentLabel/AchievementPointsCompletedLabel/TextLabel")), "5");
        assert!(!prepared.widgets.iter().any(|w| w.id == format!("{ROWS}/Row1/ContentLabel/AchievementPointsLabel/TextLabel")));
        assert!(!prepared.widgets.iter().any(|w| is_template(&w.id)));
    }

    #[test]
    fn the_daily_and_wanted_cards_fill_their_slots() {
        let card = format!("{DAILIES}/Template/Card/Label/Cardframe");
        let wanted_card = format!("{WANTED}/Template/Card_new");
        let ids: Vec<(String, &str, bool)> = vec![(format!("{DAILIES}/Template"), "Label", false), (format!("{DAILIES}/Template/TemplateText/Value"), "TextLabel", true), (format!("{DAILIES}/Template/Card"), "Label", false),
            (format!("{card}/SignChallengeGenericTier1"), "Label", false), (format!("{card}/SignChallengeBossTier2"), "Label", false),
            (format!("{card}/FinishedChallengeTier1NoAnim/Headline"), "TextLabel", true), (format!("{card}/ProgressBar/ProgressTextLabel"), "TextLabel", true), (format!("{card}/ProgressBar/progress"), "Label", false),
            (format!("{card}/Buttons/PlayButton"), "Button", false), (format!("{card}/Buttons/ButtonShuffle"), "TextButton", false),
            (wanted_card.clone(), "Label", false), (format!("{wanted_card}/card"), "Label", false), (format!("{wanted_card}/Brief"), "TextLabel", true), (format!("{wanted_card}/Slot/rarity/Epic"), "Label", false), (format!("{wanted_card}/Slot/rarity/Magic"), "Label", false),
            (format!("{wanted_card}/Rewards/Reward_1"), "Icon", true), (format!("{wanted_card}/ProgressTextLabel"), "TextLabel", true), (format!("{wanted_card}/Buttons/WantedButton"), "Button", false),
            (format!("{wanted_card}/Buttons/RefreshButton"), "TextButton", false), (format!("{WANTED}/BottomBar/TaskCount"), "TextLabel", true), (format!("{WANTED}/LoadingLabel"), "TextLabel", true),
            (format!("{DAILIES}/BottomBar/Reward_1"), "Icon", true), (format!("{DAILIES}/BottomBar/Reward_1/TextLabel"), "TextLabel", true), (format!("{DAILIES}/BottomBar/Reward_1/Check"), "Label", false),
            (format!("{DAILIES}/BottomBar/Reward_1/Check/Check2"), "Label", false), (format!("{DAILIES}/BottomBar/Reward_1/Check/Check2/Check3"), "Label", false),
            (format!("{DAILIES}/BottomBar/Reward_2"), "Icon", true), (format!("{DAILIES}/BottomBar/Reward_2/Check"), "Label", false), (format!("{DAILIES}/BottomBar/Reward_3"), "Icon", true),
            (format!("{DAILIES}/BottomBar/NextReward"), "Icon", true), (format!("{DAILIES}/BottomBar/Bonusreward"), "TextLabel", true), (format!("{DAILIES}/BottomBar/MentorBonus"), "TextLabel", true)];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let source = layout("questlog", &refs);
        let log = Log {
            dailies: vec![Daily { id: "d1".into(), description: "Kill ten rats.".into(), progress: (2, 10), state: AVAILABLE, tier: 2, sign: "Boss".into(), image: "ui/artwork_dailyquest_boss_02_crunch".into() },
                Daily { id: "d2".into(), description: "Done.".into(), progress: (1, 1), state: COMPLETED, tier: 1, sign: "Generic".into(), image: String::new() }],
            wanted: Some(WantedBoard { remaining: 2, cards: vec![WantedCard { slot: 1, title: "Ranulf".into(), rarity: "Epic".into(), reward: Icon { image: "box.png".into(), ..Default::default() }, status: 0, image: "ui/daomu_blue_01_crunch".into() },
                WantedCard { slot: 2, title: "Gorm".into(), rarity: "Magic".into(), status: 1, ..Default::default() }], reset: "5 h 03 min".into(), ..Default::default() }),
            bonuses: vec![Bonus { at: 2, icon: Icon { image: "a.png".into(), ..Default::default() }, amount: "2".into(), done: true }, Bonus { at: 3, amount: "500g".into(), ..Default::default() }],
            solved: 2,
            ..Default::default()
        };
        let prepared = prepare_questlog(&source, &log);
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.clone()).collect();
        let frame = |n: usize| format!("{DAILIES}/Slot{n}/Card/Label/Cardframe");
        assert_eq!(text_of(&prepared, &format!("{DAILIES}/Slot0/Text/Value")), "Kill ten rats.");
        // The text is drawn after the card's artwork, which shows the challenge's image.
        let text_at = ids.iter().position(|id| *id == format!("{DAILIES}/Slot0/Text/Value")).unwrap();
        let card_at = ids.iter().position(|id| *id == format!("{DAILIES}/Slot0/Card")).unwrap();
        assert!(text_at > card_at);
        assert_eq!(find(&prepared, &format!("{DAILIES}/Slot0/Card")).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui/artwork_dailyquest_boss_02_crunch.png"));
        // The authored artwork's crop stays (the texture's upper part).
        assert_eq!(find(&prepared, &format!("{DAILIES}/Slot0/Card")).art.as_ref().unwrap().uv, find(&source, &format!("{DAILIES}/Template/Card")).art.as_ref().unwrap().uv);
        assert!(ids.contains(&format!("{}/SignChallengeBossTier2", frame(0))) && !ids.contains(&format!("{}/SignChallengeGenericTier1", frame(0))));
        assert_eq!(find(&prepared, &format!("{}/Buttons/PlayButton", frame(0))).event, "Sys:Daily:d1:0");
        assert_eq!(text_of(&prepared, &format!("{}/ProgressBar/ProgressTextLabel", frame(0))), "2 / 10");
        assert!(!ids.contains(&format!("{}/Buttons/PlayButton", frame(1))) && ids.contains(&format!("{}/FinishedChallengeTier1NoAnim/Headline", frame(1))));
        let w = |n: usize| format!("{WANTED}/Slot{n}/Card");
        assert_eq!(text_of(&prepared, &format!("{}/Brief", w(0))), "Ranulf");
        assert_eq!(find(&prepared, &format!("{}/card", w(0))).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui/daomu_blue_01_crunch.png"));
        assert!(ids.contains(&format!("{}/Slot/rarity/Epic", w(0))) && !ids.contains(&format!("{}/Slot/rarity/Magic", w(0))));
        assert_eq!(find(&prepared, &format!("{}/Buttons/WantedButton", w(0))).event, "Sys:Wanted:1");
        assert!(!ids.iter().any(|id| id.contains("RefreshButton")));
        assert!(!ids.contains(&format!("{}/Buttons/WantedButton", w(1))) && ids.contains(&format!("{}/ProgressTextLabel", w(1))));
        assert_eq!(text_of(&prepared, &format!("{WANTED}/BottomBar/TaskCount")), "Missions left today: 2");
        let bar = format!("{DAILIES}/BottomBar");
        assert_eq!((text_of(&prepared, &format!("{bar}/Reward_1")), text_of(&prepared, &format!("{bar}/Reward_1/TextLabel"))), ("2".into(), "2".into()));
        assert_eq!(find(&prepared, &format!("{bar}/Reward_1/Check/Check2/Check3")).art.as_ref().unwrap().tex.as_deref(), Some(CHECK));
        assert!(!ids.iter().any(|id| id.starts_with(&format!("{bar}/Reward_2/Check"))) && !ids.iter().any(|id| id.starts_with(&format!("{bar}/Reward_3"))));
        assert_eq!(text_of(&prepared, &format!("{bar}/NextReward")), "500g");
        assert_eq!(text_of(&prepared, &format!("{bar}/Bonusreward")), "Solved today: 2 / 3");
        assert!(!ids.contains(&format!("{WANTED}/LoadingLabel")));
    }

    #[test]
    fn the_titles_tab_lists_earned_titles_and_sets_its_radio() {
        let list = format!("{TITLES}/ShowPVETitle/ListAchievementTitle");
        let ids: Vec<(String, &str, bool)> = vec![(format!("{TITLES}/Headline/TextLabelTitle"), "TextLabel", true), (format!("{list}/TemplateButton"), "Button", true), (format!("{list}/TemplateButton/pressed"), "Label", false),
            (format!("{TITLES}/ShowPVPTitle/ListPVPTitle/TemplateButton"), "Button", true), (format!("{TITLES}/ShowPVETitle/RadioButtonShowAchievementTitle"), "Button", false),
            (format!("{TITLES}/ShowPVETitle/RadioButtonShowAchievementTitle/pressed"), "Label", false), (format!("{TITLES}/DoNotShowTitle/RadioButtonDoNotShowTitle"), "Button", false),
            (format!("{TITLES}/DoNotShowTitle/RadioButtonDoNotShowTitle/pressed"), "Label", false)];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let source = layout("charactersheet", &refs);
        let titles = Titles { current: (1, "b".into()), titles: vec![("a".into(), "Regicide".into()), ("b".into(), "Wanderer".into())], rank: "Recruit".into() };
        let prepared = prepare_sheet(&source, &titles);
        assert_eq!(text_of(&prepared, &format!("{TITLES}/Headline/TextLabelTitle")), "Wanderer");
        assert_eq!((find(&prepared, &format!("{list}/Title0")).event.as_str(), text_of(&prepared, &format!("{list}/Title1")), find(&prepared, &format!("{list}/Title1")).rect[1]), ("Sys:Title:a", "Wanderer".into(), 192.0 + TITLE_HEIGHT));
        assert_eq!(find(&prepared, &format!("{list}/Title1/pressed")).state.as_deref(), Some("normal"));
        assert_eq!(find(&prepared, &format!("{TITLES}/ShowPVETitle/RadioButtonShowAchievementTitle/pressed")).state.as_deref(), Some("normal"));
        assert_eq!(find(&prepared, &format!("{TITLES}/ShowPVETitle/RadioButtonShowAchievementTitle")).event, "Sys:Title:a");
        assert_eq!(text_of(&prepared, &format!("{TITLES}/ShowPVPTitle/ListPVPTitle/Title0")), "Recruit");
        assert!(!prepared.widgets.iter().any(|w| is_template(&w.id)));
        let none = prepare_sheet(&source, &Titles { current: (0, String::new()), titles: titles.titles.clone(), rank: String::new() });
        assert_eq!(text_of(&none, &format!("{TITLES}/Headline/TextLabelTitle")), "None");
        assert_eq!(find(&none, &format!("{TITLES}/DoNotShowTitle/RadioButtonDoNotShowTitle/pressed")).state.as_deref(), Some("normal"));
    }
}
