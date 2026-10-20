//! The event window (eventwindow, UI::EventProgressWidget).
//!
//! A tab per running event (EventUpdate) with the event's icon; the open one
//! shows its artwork (EventLabel) behind its title, its text and hints (the
//! bullet points with their artwork), the time left, the items it advertises,
//! its useful offers (the Andermant shop's, bought from their slots), the
//! buttons it offers (AdditionalEventElements: travel to its area, its
//! achievements, the leaderboards, the shops) and its progress bar -- or its
//! banner when it has no bar. The bar shows a page of rewards at a time (their
//! page in EventRewardItems; more than eight split), evenly spaced, the fill
//! rising between their thresholds; the Andermant rewards mark the bar; the
//! page buttons turn it. Nothing running: the authored empty state.
//! SOURCE: _Template_Events (IconBrush, EventLabel, BannerIcon,
//!   BulletPointLocaIds/Artworks, AdvertisedItems, AdditionalEventElements,
//!   EventProgressBars, EventOffer), _Template_EventProgressBar
//!   (EventRewardItems "[page/]threshold:item:amount", EventRewardCurrency,
//!   EventCollectibles).
use super::super::template::{clip_y, clone_at, has_part, icon_layers, image_art, is_template, leaf, make_button, selected, set_text, shift, without, wrap, Icon};
use super::Sources;
use crate::gameplay::systems::texture_path;
use crate::ui::Layout;

const INLAY: &str = "Canvas/Eventwindow/BackGroundLabel/Inlay";
const TEXT: &str = "Canvas/Eventwindow/BackGroundLabel/Inlay/LabelArtwork/TextList/TextLabel";
const BULLET: &str = "Canvas/Eventwindow/BackGroundLabel/Inlay/LabelArtwork/TextList/TemplateBulletpoint";
const PANEL: &str = "Canvas/Eventwindow/BackGroundLabel/Inlay/ProgressbarLabel";
const BAR: &str = "Canvas/Eventwindow/BackGroundLabel/Inlay/ProgressbarLabel/ProgressBar";
const HELPFUL: &str = "Canvas/Eventwindow/BackGroundLabel/Inlay/BottomActiveEvent/HelpfulItemsArea";
/// The text list: lines of 16 from 180; bullets of 51; the list ends at 424.
const LINE: f32 = 16.0;
const TEXT_WIDTH: usize = 58;
const TEXT_BOTTOM: f32 = 420.0;
const MOST_TABS: usize = 8;
/// The bar's span (its fill rises from 544 to 127); a page's rewards.
const BAR_TOP: f32 = 127.0;
const BAR_BOTTOM: f32 = 544.0;
const PER_PAGE: usize = 8;
const MOST_ADVERTISED: usize = 12;
const MOST_OFFERS: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Event {
    pub id: u32,
    pub key: String,
    pub title: String,
    pub description: String,
    pub icon: String,
    pub label: String,
    pub banner: String,
    pub hints: Vec<(String, Icon)>,
    pub advertised: Vec<Icon>,
    pub elements: Vec<String>,
    /// The end as (year, month, day, hour, minute, second); None when unknown.
    pub ends: Option<[u32; 6]>,
    /// (progress, the next reward's threshold); None without a bar.
    pub progress: Option<(u32, u32)>,
    /// The bar's page shown, of how many; its span's start (the previous
    /// page's last threshold), its rewards (threshold, icon) and Andermant
    /// marks (thresholds).
    pub page: usize,
    pub pages: usize,
    pub start: u32,
    pub rewards: Vec<(u32, Icon)>,
    pub currency: Vec<u32>,
    /// What the progress counts.
    pub collectible: Icon,
    /// Its useful offers: (offer id, icon).
    pub offers: Vec<(String, Icon)>,
    pub participating: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Events {
    pub list: Vec<Event>,
    pub open: usize,
    /// Minutes since the epoch when built, for the time left.
    pub minute: u64,
}

/// A bar's pages: its rewards by their page, by threshold, at most PER_PAGE a page.
fn pages<T: Clone>(rewards: impl Iterator<Item = (u32, u32, T)>) -> Vec<Vec<(u32, T)>> {
    let mut all: Vec<(u32, u32, T)> = rewards.collect();
    all.sort_by_key(|(page, at, _)| (*page, *at));
    let mut out: Vec<Vec<(u32, T)>> = Vec::new();
    let mut last_page = None;
    for (page, at, item) in all {
        if last_page != Some(page) || out.last().is_some_and(|p| p.len() >= PER_PAGE) { out.push(Vec::new()); }
        last_page = Some(page);
        out.last_mut().unwrap().push((at, item));
    }
    out
}

pub(super) fn events(sources: &Sources) -> Events {
    let net = sources.net;
    let open = sources.state.event.min(net.events.len().min(MOST_TABS).saturating_sub(1));
    let shop = net.npc_dialogue.offers.get(super::shop::ITEM_SHOP);
    let list: Vec<Event> = net.events.iter().take(MOST_TABS).enumerate().map(|(n, e)| {
        let def = sources.catalog.events.get(&e.key);
        let reached = net.event_progress.get(&e.identifier).map(|p| p[0]);
        let pages = pages(def.map(|d| d.rewards.iter().map(|r| (r.page, r.at, r.item.clone())).collect::<Vec<_>>()).unwrap_or_default().into_iter());
        let thresholds: Vec<u32> = pages.iter().flatten().map(|(at, _)| *at).collect();
        let progress = (!pages.is_empty()).then(|| {
            let p = reached.unwrap_or(0);
            (p, thresholds.iter().copied().find(|at| *at > p).or(thresholds.last().copied()).unwrap_or(p.max(1)))
        });
        // The page shown: the one chosen, else the one with the next reward.
        let reached_page = pages.iter().position(|page| page.last().is_some_and(|(at, _)| *at > reached.unwrap_or(0))).unwrap_or(pages.len().saturating_sub(1));
        let page = sources.state.event_page.filter(|_| n == open).unwrap_or(reached_page).min(pages.len().saturating_sub(1));
        let start = if page == 0 { 0 } else { pages.get(page - 1).and_then(|p| p.last()).map_or(0, |(at, _)| *at) };
        let end = pages.get(page).and_then(|p| p.last()).map_or(0, |(at, _)| *at);
        let rewards = pages.get(page).map(|p| p.iter().map(|(at, item)| (*at, sources.icon(item, None))).collect()).unwrap_or_default();
        let currency = def.map(|d| d.currency.iter().map(|c| c.at).filter(|at| *at > start && *at <= end).collect()).unwrap_or_default();
        // The offers the Andermant shop sells (a premium product is not one).
        let offers = def.map(|d| d.offers.iter().filter_map(|key| {
            let offer = shop?.offers.iter().flatten().find(|o| &o.id == key)?;
            Some((key.clone(), sources.icon(&offer.item, None)))
        }).filter(|(_, icon)| !icon.image.is_empty()).take(MOST_OFFERS).collect()).unwrap_or_default();
        Event {
            id: e.identifier, key: e.key.clone(),
            title: def.map(|d| d.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| crate::ui::dialogue::readable(&e.key)),
            description: def.map(|d| d.description.clone()).unwrap_or_default(),
            icon: def.map(|d| d.icon.clone()).unwrap_or_default(),
            label: def.map(|d| d.label.clone()).unwrap_or_default(),
            banner: def.map(|d| d.banner.clone()).unwrap_or_default(),
            hints: def.map(|d| d.hints.iter().map(|h| (h.text.clone(), Icon { image: texture_path(&h.icon), ..Default::default() })).collect()).unwrap_or_default(),
            advertised: def.map(|d| d.advertised.iter().take(MOST_ADVERTISED).map(|i| sources.icon(i, None)).filter(|i| !i.image.is_empty()).collect()).unwrap_or_default(),
            elements: def.map(|d| d.elements.clone()).unwrap_or_default(),
            ends: Some(e.date).filter(|d| !e.map_local && d[0] > 0),
            progress, page, pages: pages.len(), start, rewards, currency,
            collectible: def.map(|d| sources.icon(&d.collectible, None)).unwrap_or_default(),
            offers,
            participating: net.event_participation.get(&e.identifier).copied(),
        }
    }).collect();
    Events { list, open, minute: now() / 60 }
}

fn now() -> u64 {
    super::unix_seconds()
}

/// Seconds since the epoch of a civil date (UTC).
pub(in crate::ui) fn epoch(date: [u32; 6]) -> u64 {
    let (y, m, d) = (date[0] as i64, date[1] as i64, date[2] as i64);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (days * 86_400 + date[3] as i64 * 3600 + date[4] as i64 * 60 + date[5] as i64).max(0) as u64
}

/// The buttons an event offers, by the element names of AdditionalEventElements.
fn offers(event: &Event, id: &str) -> bool {
    let wants = |name: &str| event.elements.iter().any(|e| e == name);
    if has_part(id, "ButtonTeleport") { return wants("ButtonTeleport"); }
    if has_part(id, "ButtonAchievements") { return wants("ButtonAchievements"); }
    if has_part(id, "EventShopLabel") { return wants("EventShopLabel"); }
    if has_part(id, "ButtonRealmoneyShop") { return wants("ButtonRealmoneyShop"); }
    if has_part(id, "ButtonShop") { return wants("ButtonShop"); }
    if has_part(id, "ButtonLeaderboards") { return wants("ButtonLeaderboards") || event.elements.is_empty(); }
    true
}

impl Event {
    /// Where a threshold lies along the page's bar (0 its foot, 1 its top):
    /// the rewards evenly spaced, linear between them.
    fn along(&self, value: u32) -> f32 {
        let n = self.rewards.len();
        if n == 0 { return 0.0; }
        let mut below = (self.start, 0.0f32);
        for (i, (at, _)) in self.rewards.iter().enumerate() {
            let here = (i + 1) as f32 / n as f32;
            if value < *at {
                let span = at.saturating_sub(below.0).max(1) as f32;
                return below.1 + (here - below.1) * (value.saturating_sub(below.0) as f32 / span).clamp(0.0, 1.0);
            }
            below = (*at, here);
        }
        1.0
    }
}

/// The event window: a tab per event, the open one's artwork, text, hints,
/// timer, advertised items, offers, buttons and progress (or banner).
pub(super) fn prepare_events(source: &Layout, events: &Events) -> Layout {
    let open = events.list.get(events.open);
    let has_bar = open.is_some_and(|e| e.progress.is_some());
    let offer_slot = |id: &str| id.split('/').find_map(|p| p.strip_prefix("Backgound_Item")).and_then(|n| n.parse::<usize>().ok());
    // The useful offers' area: with offers or the shop buttons.
    let helpful = open.is_some_and(|e| !e.offers.is_empty() || e.elements.iter().any(|x| x == "ButtonShop" || x == "ButtonRealmoneyShop"));
    let mut layout = without(source, |id| {
        is_template(id) || id == TEXT || id.contains("TabBarLabel_UpcomingEvents") || id.contains("/BottomUpcomingEvent")
            || (!helpful && id.starts_with(HELPFUL))
            || offer_slot(id).is_some_and(|n| open.is_none_or(|e| n >= e.offers.len()))
            || (open.is_some_and(|e| e.offers.is_empty()) && id.starts_with(&format!("{HELPFUL}/HelpfulItemsText")))
            || (open.is_none() && (id.contains("/LabelArtwork") || id.contains("/ProgressbarLabel") || id.contains("/BottomActiveEvent") || id.contains("/TabBarLabel") || id.contains("/BannerLabel")))
            || (open.is_some() && id.contains("/BottomNoEvent"))
            || id.split('/').find_map(|p| p.strip_prefix("TabEvent")).and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| n >= events.list.len())
            || open.is_some_and(|e| e.ends.is_none()) && id.contains("/LabelArtwork/TimerBackgroundLabel")
            || open.is_some_and(|e| !offers(e, id))
            || (has_bar && id.contains("/BannerLabel")) || (!has_bar && id.contains("/ProgressbarLabel"))
            // The on-screen counter is not drawn; the page buttons turn existing pages.
            || id.contains("/ProgressbarLabel/") && (id.contains("/ItemTemplates") || id.contains("ToggleOnScreenProgressbar"))
            || id.starts_with(&format!("{PANEL}/PrevPageBtn")) && open.is_none_or(|e| e.page == 0)
            || id.starts_with(&format!("{PANEL}/NextPageBtn")) && open.is_none_or(|e| e.page + 1 >= e.pages)
            || id.starts_with(&format!("{PANEL}/ItemSlot")) && open.is_none_or(|e| e.collectible.image.is_empty())
            || id.split('/').find_map(|p| p.strip_prefix("ItemSlot")).and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| id.contains("/AdvertisedItems/") && open.is_some_and(|e| n >= e.advertised.len()))
    });
    let Some(event) = open else { return layout };
    let left = event.ends.map(|e| epoch(e).saturating_sub(events.minute * 60)).unwrap_or(0);
    let mut extra = Vec::new();
    for w in &mut layout.widgets {
        let id = w.id.clone();
        // The tabs: each event's icon; the open one pressed.
        if let Some(n) = id.split('/').find_map(|p| p.strip_prefix("TabEvent")).and_then(|n| n.split('/').next().and_then(|n| n.parse::<usize>().ok())) {
            if n == events.open { selected(w); }
            if leaf(&id).starts_with("TabEvent") && !w.button.is_some() {
                if let Some(tab) = events.list.get(n).filter(|t| !t.icon.is_empty()) {
                    let rect = [w.rect[0] + 22.0, w.rect[1] + 7.0, w.rect[2] - 22.0, w.rect[3] - 7.0];
                    let mut icon = w.clone();
                    icon.id = format!("{id}/Icon");
                    icon.kind = "Label".into();
                    icon.event.clear();
                    icon.text = None;
                    icon.art = Some(image_art(rect, texture_path(&tab.icon)));
                    icon.rect = rect;
                    extra.push(icon);
                }
            }
        }
        if id.ends_with("/Inlay/LabelArtwork") && !event.label.is_empty() { w.art = Some(image_art(w.rect, texture_path(&event.label))); }
        if id.ends_with("/Banner_Glyph") && !event.banner.is_empty() { w.art = Some(image_art(w.rect, texture_path(&event.banner))); }
        if let Some(n) = id.split('/').find_map(|p| p.strip_prefix("ItemSlot")).and_then(|n| n.parse::<usize>().ok()) {
            if id.contains("/AdvertisedItems/") && leaf(&id) == "Icon0" {
                if let Some(icon) = event.advertised.get(n) { extra.extend(icon_layers(w, icon)); set_text(w, ""); w.hidden = false; }
            }
        }
        // The useful offers: their items in the slots, a click buys one.
        if let Some(n) = offer_slot(&id) {
            if leaf(&id) == "button_Item0" {
                if let Some((key, icon)) = event.offers.get(n) {
                    make_button(w, &format!("Sys:Buy:{key}"));
                    extra.extend(icon_layers(w, icon));
                }
            }
        }
        if id == format!("{PANEL}/ItemSlot/Icon0") { extra.extend(icon_layers(w, &event.collectible)); set_text(w, ""); w.hidden = false; }
        match leaf(&id) {
            "HeadlineLabel" => set_text(w, &event.title),
            "TimerDaysLabel" => set_text(w, &(left / 86_400).to_string()),
            "TimerHoursLabel" => set_text(w, &(left / 3600 % 24).to_string()),
            "TimerMinutesLabel" => set_text(w, &(left / 60 % 60).to_string()),
            "TimerSecondsLabel" => set_text(w, "0"),
            "ProgressNumbers" => set_text(w, &event.progress.map(|(a, b)| format!("{a} / {b}")).unwrap_or_default()),
            "DifficultyLevelText" => set_text(w, &if event.pages > 1 { format!("Page {} / {}", event.page + 1, event.pages) } else {
                event.participating.map_or(String::new(), |p| if p { "Taking part".into() } else { "Not taking part".into() }) }),
            "progress" if id.starts_with(BAR) => {
                let (reached, _) = event.progress.unwrap_or((0, 1));
                clip_y(w, event.along(reached));
            }
            _ => {}
        }
    }
    layout.widgets.extend(extra);
    // The page's rewards along the bar, alternately left and right of it;
    // its Andermant rewards' marks on it.
    if let Some((reached, _)) = event.progress {
        let y = |at: u32| BAR_BOTTOM - (BAR_BOTTOM - BAR_TOP) * event.along(at);
        for (n, (at, icon)) in event.rewards.iter().enumerate() {
            let side = if n % 2 == 0 { "ItemTemplateLeft" } else { "ItemTemplateRight" };
            let template = format!("{BAR}/ItemTemplates/{side}");
            let Some(root) = source.widgets.iter().find(|w| w.id == template) else { continue };
            for mut w in clone_at(source, &template, &format!("{BAR}/Reward{n}"), 0.0, y(*at) - root.rect[1]) {
                if w.id.ends_with("/IconHighlight") && *at > reached { continue; }
                if leaf(&w.id) == "Icon" { layout.widgets.extend(icon_layers(&mut w, icon)); set_text(&mut w, ""); }
                w.hidden = false;
                layout.widgets.push(w);
            }
        }
        let template = format!("{BAR}/ItemTemplates/CurrencyTemplate");
        if let Some(root) = source.widgets.iter().find(|w| w.id == template) {
            let middle = (root.rect[1] + root.rect[3]) / 2.0;
            for (n, at) in event.currency.iter().enumerate() {
                for mut w in clone_at(source, &template, &format!("{BAR}/Andermant{n}"), 0.0, y(*at) - middle) {
                    if w.id.ends_with("/IconHighlight") && *at > reached { continue; }
                    w.hidden = false;
                    layout.widgets.push(w);
                }
            }
        }
    }
    // The text, then the hints, down the text list.
    let mut y = 180.0;
    if let Some(template) = source.widgets.iter().find(|w| w.id == TEXT) {
        for (n, line) in event.description.split('\n').flat_map(|p| wrap(p, TEXT_WIDTH, 30)).enumerate() {
            if y + LINE > TEXT_BOTTOM { break; }
            let mut w = template.clone();
            w.id = format!("{TEXT}/Line{n}");
            shift(&mut w, 0.0, y - template.rect[1]);
            set_text(&mut w, &line);
            layout.widgets.push(w);
            y += LINE;
        }
    }
    if let Some(root) = source.widgets.iter().find(|w| w.id == BULLET) {
        let height = root.rect[3] - root.rect[1];
        y += 4.0;
        for (n, (text, icon)) in event.hints.iter().enumerate() {
            if y + height > TEXT_BOTTOM + 4.0 { break; }
            for mut w in clone_at(source, BULLET, &format!("{BULLET}{n}").replace("TemplateBulletpoint", "Bulletpoint"), 0.0, y - root.rect[1]) {
                match leaf(&w.id) {
                    "TextLabel" => set_text(&mut w, &wrap(text, 44, 3).join("\n")),
                    "Icon0" if !icon.image.is_empty() => { layout.widgets.extend(icon_layers(&mut w, icon)); set_text(&mut w, ""); }
                    _ => {}
                }
                layout.widgets.push(w);
            }
            y += height;
        }
    }
    let _ = INLAY;
    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::template::fixture::{find, layout, text_of};
    use super::super::EVENTS;

    #[test]
    fn the_event_window_shows_the_open_event() {
        let tab = "Canvas/Eventwindow/TabBarLabel/TabEvent";
        let ids = [(format!("{tab}0"), "Button", false), (format!("{tab}0/pressed"), "Label", false), (format!("{tab}1"), "Button", false), (format!("{tab}2"), "Button", false),
            (format!("{INLAY}/LabelArtwork"), "Label", false), (format!("{INLAY}/LabelArtwork/HeadlineLabel"), "TextLabel", true), (TEXT.into(), "TextLabel", true),
            (BULLET.into(), "Label", false), (format!("{BULLET}/TextLabel"), "TextLabel", true), (format!("{BULLET}/ItemSlot0/Icon0"), "Icon", true),
            (format!("{INLAY}/LabelArtwork/TimerBackgroundLabel/TimerDaysBackgroundLabel/TimerDaysLabel"), "TextLabel", true),
            (format!("{INLAY}/BottomNoEvent/EmptyMessage"), "TextLabel", true), (format!("{INLAY}/BottomActiveEvent/ButtonArea/ButtonTeleport"), "Button", false),
            (format!("{INLAY}/BottomActiveEvent/ButtonArea/ButtonLeaderboards"), "Button", false),
            (format!("{INLAY}/BottomActiveEvent/AdvertisedItemArea/AdvertisedItems/ItemSlot0/Icon0"), "Icon", true), (format!("{INLAY}/BottomActiveEvent/AdvertisedItemArea/AdvertisedItems/ItemSlot1/Icon0"), "Icon", true),
            (format!("{INLAY}/ProgressbarLabel/ProgressNumbers"), "TextLabel", true), (format!("{BAR}/progress"), "Label", false), (format!("{INLAY}/BannerLabel/BannerLabel1/Banner1/Banner2/Banner_Glyph"), "Label", false),
            (format!("{PANEL}/NextPageBtn"), "Button", false), (format!("{PANEL}/PrevPageBtn"), "Button", false), (format!("{PANEL}/DifficultyLevelText"), "TextLabel", true),
            (format!("{HELPFUL}/HelpfulItemsText"), "TextLabel", true), (format!("{HELPFUL}/Helpfultems/Backgound_Item0"), "Label", false), (format!("{HELPFUL}/Helpfultems/Backgound_Item0/button_Item0"), "Button", false),
            (format!("{HELPFUL}/Helpfultems/Backgound_Item1"), "Label", false), (format!("{HELPFUL}/Helpfultems/Backgound_Item1/button_Item0"), "Button", false)];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let source = layout(EVENTS, &refs);
        let ghost = Event { title: "Ghost Festival".into(), description: "one two three".into(), icon: "ui_event/tabicon_season_halloween".into(), label: "ui_event/event_halloween_crunch".into(),
            hints: vec![("Collect souls".into(), Icon { image: "souls.png".into(), ..Default::default() })], advertised: vec![Icon { image: "staff.png".into(), ..Default::default() }],
            elements: vec!["ButtonTeleport".into()], ends: Some([2026, 10, 22, 12, 0, 0]), progress: Some((250, 500)), rewards: vec![(100, Icon::default()), (500, Icon::default())],
            page: 0, pages: 2, offers: vec![("global_e_hal_01_progress_RC_100".into(), Icon { image: "souls.png".into(), ..Default::default() })], ..Default::default() };
        let other = Event { title: "Other".into(), banner: "ui_event/banner_glyph_halloween".into(), ..Default::default() };
        let events = Events { list: vec![ghost, other], open: 0, minute: 1_792_497_600 / 60 };
        let prepared = prepare_events(&source, &events);
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.clone()).collect();
        assert!(ids.iter().any(|id| id.ends_with("/TabEvent1")) && !ids.iter().any(|id| id.ends_with("/TabEvent2") || id.contains("BottomNoEvent")));
        assert_eq!(find(&prepared, &format!("{tab}0/Icon")).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui_event/tabicon_season_halloween.png"));
        assert_eq!(find(&prepared, &format!("{tab}0/pressed")).state.as_deref(), Some("normal"));
        assert_eq!(find(&prepared, &format!("{INLAY}/LabelArtwork")).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui_event/event_halloween_crunch.png"));
        assert_eq!(text_of(&prepared, &format!("{INLAY}/LabelArtwork/HeadlineLabel")), "Ghost Festival");
        assert_eq!(text_of(&prepared, &format!("{TEXT}/Line0")), "one two three");
        assert_eq!(text_of(&prepared, "Canvas/Eventwindow/BackGroundLabel/Inlay/LabelArtwork/TextList/Bulletpoint0/TextLabel"), "Collect souls");
        assert_eq!(text_of(&prepared, &format!("{INLAY}/LabelArtwork/TimerBackgroundLabel/TimerDaysBackgroundLabel/TimerDaysLabel")), "2");
        assert_eq!(text_of(&prepared, &format!("{INLAY}/ProgressbarLabel/ProgressNumbers")), "250 / 500");
        assert!(ids.iter().any(|id| id.contains("ButtonTeleport")) && !ids.iter().any(|id| id.contains("ButtonLeaderboards")));
        assert!(ids.iter().any(|id| id.ends_with("/AdvertisedItems/ItemSlot0/Icon0/Image")) && !ids.iter().any(|id| id.contains("/AdvertisedItems/ItemSlot1")));
        assert!(!ids.iter().any(|id| id.contains("BannerLabel")));
        // The rewards evenly spaced: 250 lies halfway from the first (100, at 1/2) to the second (500, at 1).
        assert!((events.list[0].along(250) - 0.6875).abs() < 1e-4 && events.list[0].along(0) == 0.0 && events.list[0].along(900) == 1.0);
        assert!(ids.iter().any(|id| id.ends_with("/NextPageBtn")) && !ids.iter().any(|id| id.ends_with("/PrevPageBtn")));
        assert_eq!(text_of(&prepared, &format!("{PANEL}/DifficultyLevelText")), "Page 1 / 2");
        // The useful offer in its slot; the empty slot gone.
        assert_eq!(find(&prepared, &format!("{HELPFUL}/Helpfultems/Backgound_Item0/button_Item0")).event, "Sys:Buy:global_e_hal_01_progress_RC_100");
        assert!(ids.iter().any(|id| id.ends_with("/Backgound_Item0/button_Item0/Image")) && !ids.iter().any(|id| id.contains("/Backgound_Item1")));
        let second = prepare_events(&source, &Events { open: 1, ..events.clone() });
        assert!(second.widgets.iter().any(|w| w.id.contains("BannerLabel")) && !second.widgets.iter().any(|w| w.id.contains("ProgressbarLabel")));
        // No offers and no shop buttons: no offers' area.
        assert!(!second.widgets.iter().any(|w| w.id.starts_with(HELPFUL)));
        assert_eq!(find(&second, &format!("{INLAY}/BannerLabel/BannerLabel1/Banner1/Banner2/Banner_Glyph")).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui_event/banner_glyph_halloween.png"));
        let none = prepare_events(&source, &Events::default());
        assert!(none.widgets.iter().any(|w| w.id.contains("BottomNoEvent")) && !none.widgets.iter().any(|w| w.id.contains("TabEvent") || w.id.contains("HeadlineLabel")));
        assert_eq!(epoch([1970, 1, 2, 0, 0, 0]), 86_400);
        assert_eq!(epoch([2026, 10, 20, 12, 0, 0]), 1_792_497_600);
    }
}
