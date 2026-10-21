//! The social window (socialscreen): the player search, our group and our guild
//! as the server last described them.
//!
//! Each list is its authored row template (ListTemplateSearch, ListTemplateGroup
//! and the leader's ListTemplateGroupLeader, ListTemplateGuild) cloned per player
//! down the List, 19 rows at a time; a row's name puts it on its tab (`LISTS`,
//! crate::ui::controls::panels). A row shows what UI::SocialWidget's element
//! builders set: the name and level (greyed while offline), the online or offline
//! status icon, the class icon, the group status icon, the group's location and
//! the guild's rank. The window asks the server when it opens (GroupList,
//! GuildList, and a PlayerQuery of the players on our map: systems::send); Enter
//! searches the name typed, Update asks again, the column heads sort the lists
//! and a row opens the player menu (crate::ui::controls::social).
//! SOURCE: DSOR_Server services/social.py (_handle_player_query: kind 1 the map's
//!   players, any other kind every character, never the asker; _group_status_for;
//!   _guild_status_for), data/diagnostics/social-online-ui.annotated.txt:
//!   CreatePlayerListElement 0x14052c55c, CreateGroupListElement 0x14052ae50,
//!   CreateGuildListElement 0x14052b928 (release261).
//! UNVERIFIED: the group status icon. CreatePlayerListElement shows it by the
//!   row's third flag, which this server sets to "online" for everyone; the icon
//!   is a group and its column "Group Status" (gui.window.social_groupColumn), so
//!   it marks the players known to be grouped: our group's members and rows with
//!   a group role (0 leader, 1 member).
use super::super::quests::place_handle;
use super::super::template::{clone_at, clone_rows, grey, image_art, leaf, make_button, set_text, without};
use super::Sources;
use crate::ui::Layout;
use dsor_proto::commands::player::GuildMember;

pub(in crate::ui) const WINDOW: &str = "socialscreen";
const RAHMEN: &str = "Canvas/Label_texture/MainWindow/rahmen";
const LIST: &str = "Canvas/Label_texture/MainWindow/rahmen/List";
/// The window's backdrop (its struktur texture), its transparent frame holder, and
/// the brown panels behind the column heads and the bottom controls.
const MAIN: &str = "Canvas/Label_texture/MainWindow";
const PANELS: &str = "Canvas/Label_texture/MenuBackground";
const OVERLAY: &str = "Canvas/Label_texture/InviteMultipleLabel";
const OVERLAY_PANEL: &str = "Canvas/Label_texture/InviteMultipleLabel/MenuBackground";
/// A row of the List: 21 reference pixels (its scroll arrows' height); 19 fit
/// between the column heads (168) and the bottom panel's border (568).
const ROW_STEP: f32 = 21.0;
pub(in crate::ui) const VISIBLE_ROWS: usize = 19;
/// The lists: their name (rows `<name><n>` beside the List, the scroll bar under
/// `List/<name>`), their tab (the window's "social" page), their scroll key
/// (systems::State::scroll).
pub(in crate::ui) const LISTS: [(&str, u8, &str); 3] = [("SearchList", 0, "social_search"), ("GroupList", 2, "social_group"), ("GuildList", 3, "social_guild")];
/// The tabs without a list of their own: friends, ignore, invite, group finder.
pub(in crate::ui) const EMPTY_TABS: u32 = 1 << 1 | 1 << 4 | 1 << 5 | 1 << 6;

/// The tab of a path part naming a list (`SearchList`) or one of its rows
/// (`SearchList3`).
pub(in crate::ui) fn list_tab(part: &str) -> Option<u8> {
    LISTS.iter().find(|(name, _, _)| part.strip_prefix(name).is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))).map(|(_, tab, _)| *tab)
}

/// A column head's sort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Column { Online, Name, Class, Level, Group, Rank }

/// The column heads' events, their sort and caption (gui.window.social_*Column).
pub(in crate::ui) const COLUMNS: [(&str, Column, &str); 6] = [("SortOnline", Column::Online, "Online Status"), ("SortName", Column::Name, "User Name"),
    ("SortClass", Column::Class, "Class"), ("SortLevel", Column::Level, "Level"), ("SortGroup", Column::Group, "Group Status"), ("SortRank", Column::Rank, "Rank")];

/// One player of a list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Person {
    pub name: String,
    pub level: u16,
    /// 0 warrior, 1 mage, 2 ranger, 3 dwarf, 4 niwalk.
    pub class: u8,
    /// Its name, level and location in full colour (PlayerSocialInfo +0x7c,
    /// GuildMember +0x50); greyed otherwise.
    pub online: bool,
    /// The status icon: online, else offline (+0x7d; a guild member's +0x51).
    pub active: bool,
    /// The group status icon.
    pub grouped: bool,
    /// The group's leader (its template, a star); the guild's (rank 2).
    pub leader: bool,
    pub me: bool,
    /// A guild member's rank (0 member, 1 officer, 2 leader).
    pub rank: u8,
    /// The group's location (the member's map); the guild's rank name.
    pub detail: String,
}

/// The social window's lists as shown.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Rosters {
    /// The search results, the group, the guild (LISTS' order), sorted.
    pub lists: [Vec<Person>; 3],
    /// Each list's first row shown.
    pub scroll: [usize; 3],
    pub grouped: bool,
    pub in_guild: bool,
    /// The column the lists are sorted by, descending or not.
    pub sort: Option<(Column, bool)>,
}

/// gui.window.grouping_Member / _Officer / _Leader.
fn rank_name(rank: u8) -> &'static str {
    match rank { GuildMember::LEADER => "Guild master", GuildMember::OFFICER => "Officer", _ => "Member" }
}

/// The class icons (CreatePlayerListElement); the niwalk's is not in the game's
/// textures.
fn class_icon(class: u8) -> Option<&'static str> {
    Some(match class {
        0 => "textures/ui/icon_member_warrior.png",
        1 => "textures/ui/icon_member_wizard.png",
        2 => "textures/ui/icon_member_ranger.png",
        3 => "textures/ui/icon_member_dwarf.png",
        _ => return None,
    })
}

fn status_icon(active: bool) -> String {
    format!("textures/ui/icon_status_{}.png", if active { "online" } else { "offline" })
}

fn sort(list: &mut [Person], column: Column, descending: bool) {
    list.sort_by(|a, b| {
        let order = match column {
            Column::Online => b.active.cmp(&a.active),
            Column::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Column::Class => a.class.cmp(&b.class),
            Column::Level => a.level.cmp(&b.level),
            Column::Group => b.grouped.cmp(&a.grouped),
            Column::Rank => b.rank.cmp(&a.rank),
        };
        let order = if descending { order.reverse() } else { order };
        order.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

pub(super) fn rosters(sources: &Sources) -> Rosters {
    let net = sources.net;
    let state = sources.state;
    let group = net.group.as_ref().filter(|g| g.grouped());
    let in_group = |name: &str| group.is_some_and(|g| g.members.iter().any(|m| m.name.eq_ignore_ascii_case(name)));
    let location = |map: &str| if map.is_empty() { String::new() } else { sources.names.map_or_else(|| crate::ui::dialogue::readable(map), |n| n.caption(map)) };
    let search = net.player_query.as_ref().map_or_else(Vec::new, |q| q.players.iter().map(|p| Person {
        name: p.name.clone(), level: p.level, class: p.character_class, online: p.online, active: p.available,
        grouped: p.role <= 1 || in_group(&p.name), leader: p.leads(), ..Default::default()
    }).collect());
    let members = group.map_or_else(Vec::new, |g| g.members.iter().map(|m| Person {
        name: m.name.clone(), level: m.level, class: m.character_class, online: m.online, active: m.available,
        grouped: true, leader: m.leads(), me: m.player_id == g.recipient_id, rank: 0, detail: location(&m.map_name),
    }).collect());
    let guild = net.guild.as_ref().filter(|g| g.in_guild);
    let guild_members = guild.map_or_else(Vec::new, |g| g.members.iter().map(|m| Person {
        name: m.name.clone(), level: m.level, class: m.character_class, online: m.online, active: m.active,
        grouped: m.unknown_bool_0 || in_group(&m.name), leader: m.rank == GuildMember::LEADER, me: m.player_id == g.recipient_id,
        rank: m.rank, detail: rank_name(m.rank).to_owned(),
    }).collect());
    let mut lists = [search, members, guild_members];
    if let Some((column, descending)) = state.social_sort {
        for list in &mut lists { sort(list, column, descending); }
    }
    let scroll = std::array::from_fn(|i| state.scroll.get(LISTS[i].2).copied().unwrap_or(0).min(lists[i].len().saturating_sub(VISIBLE_ROWS)));
    Rosters { lists, scroll, grouped: group.is_some(), in_guild: guild.is_some(), sort: state.social_sort }
}

/// How far a list scrolls (systems::scroll_limit).
pub(super) fn scroll_limit(rosters: &Rosters, key: &str) -> usize {
    LISTS.iter().position(|(_, _, k)| *k == key).map_or(0, |i| rosters.lists[i].len().saturating_sub(VISIBLE_ROWS))
}

/// The window: its panels under its controls, each list's rows and scroll bar,
/// the buttons with nothing to do disabled, the sorted column's arrow.
pub(super) fn prepare_social(source: &Layout, rosters: &Rosters) -> Layout {
    let found_guild = format!("{RAHMEN}/LabelFoundGuildBackground");
    let mut layout = without(source, |id| rosters.in_guild && (id == found_guild || id.starts_with(&format!("{found_guild}/"))));
    // The brown panels behind the column heads and the bottom controls come
    // after the window's contents in the layout; the game draws them under them
    // (their border line stays over them).
    // FAILURE (2026-10-21): left out instead, the bottom of the window had no
    // background ("the bottom part background of the window is not shown").
    for (panel, after) in [(PANELS, MAIN), (OVERLAY_PANEL, OVERLAY)] {
        let from = layout.widgets.iter().position(|w| w.id == panel);
        let to = layout.widgets.iter().position(|w| w.id == after);
        if let (Some(from), Some(to)) = (from, to) {
            if to < from { let w = layout.widgets.remove(from); layout.widgets.insert(to + 1, w); }
        }
    }
    for w in &mut layout.widgets {
        let id = w.id.as_str();
        // Nothing to leave, nothing to found twice.
        if id == format!("{RAHMEN}/OptionsGroup/ButtonLeaveGroup") { w.enabled = rosters.grouped; }
        if id == format!("{RAHMEN}/OptionsGroupFinder/ButtonFoundGroup") { w.enabled = !rosters.grouped; }
        if id == format!("{RAHMEN}/OptionsGuild/ButtonLeaveGuild") { w.enabled = rosters.in_guild; }
    }
    // A column head shows the sorted column's arrow; the others, none.
    layout.widgets.retain(|w| {
        let parts: Vec<&str> = w.id.split('/').collect();
        let Some(at) = parts.iter().position(|p| *p == "SortArrows") else { return true };
        let Some(arrow) = parts.get(at + 1) else { return true };
        let column = parts.get(at.wrapping_sub(1)).and_then(|button| COLUMNS.iter().find(|(event, _, _)| button.strip_suffix("Button") == Some(*event)).map(|c| c.1));
        let shown = match rosters.sort { Some((sorted, descending)) if Some(sorted) == column => if descending { "SortArrowsDown" } else { "SortArrowsUp" }, _ => "SortArrowsNone" };
        *arrow == shown
    });
    // The rows go where the List draws its contents: after it and its scroll bar.
    let list_prefix = format!("{LIST}/");
    let at = layout.widgets.iter().rposition(|w| w.id == LIST || w.id.starts_with(&list_prefix)).map_or(layout.widgets.len(), |i| i + 1);
    let mut rows = Vec::new();
    for (i, (name, _, key)) in LISTS.iter().enumerate() {
        let list = &rosters.lists[i];
        let scroll = rosters.scroll[i];
        let limit = list.len().saturating_sub(VISIBLE_ROWS);
        // The list's own scroll bar: its arrows turn it, its handle follows.
        for (part, event, enabled) in [("ScrollUp", -1, scroll > 0), ("ScrollDown", 1, scroll < limit)] {
            for mut w in clone_at(source, &format!("{LIST}/{part}"), &format!("{LIST}/{name}/{part}"), 0.0, 0.0) {
                if w.id == format!("{LIST}/{name}/{part}") {
                    w.event = format!("Sys:Scroll:{key}:{event}");
                    w.enabled = enabled;
                }
                rows.push(w);
            }
        }
        let bar = format!("{LIST}/{name}/ScrollbarBackground");
        let mut handle = Layout { window: source.window.clone(), widgets: clone_at(source, &format!("{LIST}/ScrollbarBackground"), &bar, 0.0, 0.0).collect() };
        place_handle(&mut handle, &bar, scroll, limit + 1);
        rows.extend(handle.widgets);
        for (n, person) in list.iter().skip(scroll).take(VISIBLE_ROWS).enumerate() {
            let template = match i {
                0 => "ListTemplateSearch",
                1 if person.leader => "ListTemplateGroupLeader",
                1 => "ListTemplateGroup",
                _ => "ListTemplateGuild",
            };
            let row = format!("{RAHMEN}/{name}{n}");
            for mut w in clone_rows(source, &format!("{RAHMEN}/{template}"), &row, n as f32 * ROW_STEP) {
                w.hidden = false;
                if w.id == row {
                    // Another player's row opens the player menu.
                    if !person.me { make_button(&mut w, &format!("Sys:Player:{}", person.name)); }
                    rows.push(w);
                    continue;
                }
                let part = leaf(&w.id).to_owned();
                match part.as_str() {
                    "NameLabel" => set_text(&mut w, &person.name),
                    "LevelLabel" => set_text(&mut w, &person.level.to_string()),
                    "LocationLabel" | "RankLabel" => set_text(&mut w, &person.detail),
                    "OnlineLabel" => w.art = Some(image_art(w.rect, status_icon(person.active))),
                    "ClassLabel" => w.art = class_icon(person.class).map(|t| image_art(w.rect, t.to_owned())),
                    "GroupLabel" if !person.grouped => continue,
                    _ => {}
                }
                if !person.online && matches!(part.as_str(), "NameLabel" | "LevelLabel" | "LocationLabel" | "RankLabel") { grey(&mut w); }
                rows.push(w);
            }
        }
    }
    layout.widgets.splice(at..at, rows);
    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::template::fixture::{find, layout, text_of};

    fn person(name: &str, level: u16) -> Person {
        Person { name: name.into(), level, class: 1, online: true, active: true, ..Default::default() }
    }

    fn source() -> Layout {
        let ids: Vec<(String, &str, bool)> = vec![
            ("Canvas/Label_texture".into(), "Label", false), (MAIN.into(), "Label", false), (format!("{RAHMEN}/TableHeadSearch"), "Label", false),
            (format!("{RAHMEN}/TableHeadSearch/SortNameButton"), "Button", false),
            (format!("{RAHMEN}/TableHeadSearch/SortNameButton/SortArrows"), "Label", false),
            (format!("{RAHMEN}/TableHeadSearch/SortNameButton/SortArrows/SortArrowsUp"), "Label", false),
            (format!("{RAHMEN}/TableHeadSearch/SortNameButton/SortArrows/SortArrowsNone"), "Label", false),
            (format!("{RAHMEN}/TableHeadSearch/SortNameButton/SortArrows/SortArrowsDown"), "Label", false),
            (LIST.into(), "List", false), (format!("{LIST}/ScrollbarBackground"), "Label", false), (format!("{LIST}/ScrollbarBackground/ScrollHandle"), "Button", false),
            (format!("{LIST}/ScrollUp"), "Button", false), (format!("{LIST}/ScrollUp/disabled"), "Label", false), (format!("{LIST}/ScrollDown"), "Button", false),
            (format!("{RAHMEN}/ListTemplateSearch"), "Label", false), (format!("{RAHMEN}/ListTemplateSearch/OnlineLabel"), "Label", false),
            (format!("{RAHMEN}/ListTemplateSearch/NameLabel"), "TextLabel", true), (format!("{RAHMEN}/ListTemplateSearch/ClassLabel"), "Label", false),
            (format!("{RAHMEN}/ListTemplateSearch/LevelLabel"), "TextLabel", true), (format!("{RAHMEN}/ListTemplateSearch/GroupLabel"), "Label", false),
            (format!("{RAHMEN}/ListTemplateGroup"), "Label", false), (format!("{RAHMEN}/ListTemplateGroup/NameLabel"), "TextLabel", true),
            (format!("{RAHMEN}/ListTemplateGroup/LocationLabel"), "TextLabel", true),
            (format!("{RAHMEN}/ListTemplateGroupLeader"), "Label", false), (format!("{RAHMEN}/ListTemplateGroupLeader/NameLabel"), "TextLabel", true),
            (format!("{RAHMEN}/ListTemplateGroupLeader/Star"), "Label", false),
            (format!("{RAHMEN}/ListTemplateGuild"), "Label", false), (format!("{RAHMEN}/ListTemplateGuild/NameLabel"), "TextLabel", true),
            (format!("{RAHMEN}/ListTemplateGuild/RankLabel"), "TextLabel", true),
            (format!("{RAHMEN}/OptionsGroup/ButtonLeaveGroup"), "Button", false), (format!("{RAHMEN}/OptionsGuild/ButtonLeaveGuild"), "Button", false),
            (format!("{RAHMEN}/LabelFoundGuildBackground"), "Label", false), (format!("{RAHMEN}/LabelFoundGuildBackground/ButtonFoundGuild"), "Button", false),
            (PANELS.into(), "Label", false), (format!("{PANELS}/Border1"), "Label", false),
        ];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let mut source = layout(WINDOW, &refs);
        for w in &mut source.widgets { if w.id.contains("/ListTemplate") { w.hidden = true; } }
        source
    }

    #[test]
    fn the_lists_fill_their_tabs_with_the_players_as_the_server_described_them() {
        let mut rosters = Rosters::default();
        rosters.lists[0] = vec![Person { grouped: true, ..person("Ann", 30) }, Person { online: false, active: false, class: 3, ..person("Bob", 12) }];
        rosters.lists[1] = vec![Person { leader: true, me: true, detail: "Kingshill".into(), ..person("Me", 40) }, Person { detail: "Wildforest".into(), ..person("Ann", 30) }];
        rosters.lists[2] = vec![Person { rank: 2, detail: "Guild master".into(), ..person("Cid", 50) }];
        rosters.grouped = true;
        rosters.in_guild = true;
        let prepared = prepare_social(&source(), &rosters);
        let ids: Vec<&str> = prepared.widgets.iter().map(|w| w.id.as_str()).collect();
        // The search: name, level, status and class icons, the group icon if grouped.
        assert_eq!(text_of(&prepared, &format!("{RAHMEN}/SearchList0/NameLabel")), "Ann");
        assert_eq!(text_of(&prepared, &format!("{RAHMEN}/SearchList1/LevelLabel")), "12");
        assert_eq!(find(&prepared, &format!("{RAHMEN}/SearchList1")).rect[1] - find(&prepared, &format!("{RAHMEN}/SearchList0")).rect[1], ROW_STEP);
        let art = |id: &str| find(&prepared, id).art.as_ref().and_then(|a| a.tex.clone()).unwrap_or_default();
        assert_eq!(art(&format!("{RAHMEN}/SearchList0/OnlineLabel")), "textures/ui/icon_status_online.png");
        assert_eq!(art(&format!("{RAHMEN}/SearchList1/OnlineLabel")), "textures/ui/icon_status_offline.png");
        assert_eq!(art(&format!("{RAHMEN}/SearchList1/ClassLabel")), "textures/ui/icon_member_dwarf.png");
        assert!(ids.contains(&format!("{RAHMEN}/SearchList0/GroupLabel").as_str()) && !ids.contains(&format!("{RAHMEN}/SearchList1/GroupLabel").as_str()));
        assert_eq!(find(&prepared, &format!("{RAHMEN}/SearchList1/NameLabel")).text.as_ref().unwrap().color, [0.55, 0.55, 0.55, 1.0]);
        assert!(!find(&prepared, &format!("{RAHMEN}/SearchList0")).hidden);
        // Another player's row opens the menu; ours does not.
        assert_eq!(find(&prepared, &format!("{RAHMEN}/SearchList0")).event, "Sys:Player:Ann");
        assert_eq!(find(&prepared, &format!("{RAHMEN}/GroupList0")).event, "");
        // The group: the leader's template (its star), the location.
        assert!(ids.contains(&format!("{RAHMEN}/GroupList0/Star").as_str()) && !ids.contains(&format!("{RAHMEN}/GroupList1/Star").as_str()));
        assert_eq!(text_of(&prepared, &format!("{RAHMEN}/GroupList1/LocationLabel")), "Wildforest");
        assert_eq!(text_of(&prepared, &format!("{RAHMEN}/GuildList0/RankLabel")), "Guild master");
        // In a guild: no foundation panel; grouped: leaving is possible.
        assert!(!ids.iter().any(|id| id.contains("LabelFoundGuildBackground")));
        assert!(find(&prepared, &format!("{RAHMEN}/OptionsGroup/ButtonLeaveGroup")).enabled);
        // The panels go under the contents, their border stays over them.
        let at = |id: &str| ids.iter().position(|i| *i == id).unwrap();
        assert!(at(PANELS) < at(&format!("{RAHMEN}/TableHeadSearch")));
        assert!(at(&format!("{PANELS}/Border1")) > at(&format!("{RAHMEN}/SearchList0")));
        // The rows lie where the List draws, after its scroll bars.
        assert!(at(&format!("{RAHMEN}/SearchList0")) > at(LIST) && at(&format!("{RAHMEN}/SearchList0")) < at(&format!("{RAHMEN}/OptionsGroup/ButtonLeaveGroup")));
        // Every list has its own scroll bar, arrows disabled with nothing to scroll.
        for (name, _, key) in LISTS {
            let up = find(&prepared, &format!("{LIST}/{name}/ScrollUp"));
            assert_eq!((up.event.clone(), up.enabled), (format!("Sys:Scroll:{key}:-1"), false));
            assert!(ids.contains(&format!("{LIST}/{name}/ScrollUp/disabled").as_str()));
            assert!(ids.contains(&format!("{LIST}/{name}/ScrollbarBackground/ScrollHandle").as_str()));
        }
        // Unsorted: only the "none" arrows.
        assert!(ids.iter().any(|id| id.ends_with("/SortArrowsNone")) && !ids.iter().any(|id| id.ends_with("/SortArrowsUp") || id.ends_with("/SortArrowsDown")));
    }

    #[test]
    fn long_lists_scroll_and_sort() {
        let mut rosters = Rosters::default();
        rosters.lists[0] = (0..25).map(|n| person(&format!("P{n:02}"), n)).collect();
        rosters.scroll[0] = 6;
        rosters.sort = Some((Column::Name, true));
        let prepared = prepare_social(&source(), &rosters);
        let ids: Vec<&str> = prepared.widgets.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(text_of(&prepared, &format!("{RAHMEN}/SearchList0/NameLabel")), "P06");
        assert!(ids.contains(&format!("{RAHMEN}/SearchList18").as_str()) && !ids.contains(&format!("{RAHMEN}/SearchList19").as_str()));
        assert!(find(&prepared, &format!("{LIST}/SearchList/ScrollUp")).enabled && !find(&prepared, &format!("{LIST}/SearchList/ScrollDown")).enabled);
        assert!(!find(&prepared, &format!("{RAHMEN}/OptionsGroup/ButtonLeaveGroup")).enabled);
        assert!(ids.iter().any(|id| id.ends_with("/SortNameButton/SortArrows/SortArrowsDown")) && !ids.iter().any(|id| id.ends_with("/SortNameButton/SortArrows/SortArrowsNone")));
        assert!(ids.iter().any(|id| id.contains("LabelFoundGuildBackground")), "not in a guild: the foundation panel stays");
        // Sorting: by name descending, then by the other columns.
        let mut list = vec![person("ann", 5), person("Bob", 9), Person { active: false, ..person("Cid", 1) }];
        sort(&mut list, Column::Name, true);
        assert_eq!(list.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Cid", "Bob", "ann"]);
        sort(&mut list, Column::Level, false);
        assert_eq!(list[0].name, "Cid");
        sort(&mut list, Column::Online, false);
        assert_eq!(list[2].name, "Cid");
        assert_eq!((list_tab("SearchList"), list_tab("GuildList12"), list_tab("GroupListX"), list_tab("List")), (Some(0), Some(3), None, None));
    }
}
