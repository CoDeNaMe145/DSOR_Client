//! The leaderboards (leaderboards) and the PvP queue (matchmakingwindow).
//!
//! The leaderboards' PvP inlay lists one board: the overall board on its first
//! category tab (TemplateRowOverallRanking: rank, score, name, the place's
//! reward boxes), the honour board on the others (DefaultTemplateRow: rank,
//! honour, name, wins, draws, losses, kills, deaths), ten rows a page; our own
//! row is highlighted. Its headers sort the honour board, its check boxes
//! filter it to friends or guild members; the scroll buttons and the wheel page
//! it. The server records the lifetime honour board only: the PvE and event
//! boards say so. The matchmaking window shows the open tab's mode (team: best
//! of five 5 on 5; solo: the duel) with its artwork, rules and our queue state;
//! Join battle joins its queue, leaves it, or enters the match found.
//! SOURCE: DSOR_Server services/ranking.py (sort columns 0 honour, 1 wins, 2
//!   losses, 3 draws, 4 kills, 5 deaths; filters 0x02 friends, 0x04 guild),
//!   services/matchmaking.py; _Template_BattlegroundSettings.Artwork,
//!   _Template_OverallPVPRankingRewards.
use super::super::template::{clone_rows, has_part, icon_layers, image_art, is_template, leaf, selected, set_text, shift, without, wrap, Icon};
use super::Sources;
use crate::gameplay::systems::texture_path;
use crate::ui::Layout;

const PVP_INLAY: &str = "Canvas/WindowBackground/ContentFrame/PvP/PvP_Inlay";
const EMPTY: &str = "Canvas/WindowBackground/ContentFrame/EmptyMessage";
/// A board row spans 317 to 345.
const ROW_HEIGHT: f32 = 28.0;
const FRAME: &str = "Canvas/Skillwindow/BackGroundLabel/WindowTitleLabel/Frame/Frame2";
const STATUS: &str = "Canvas/Skillwindow/BackGroundLabel/WindowTitleLabel/Frame/Frame2/LabelArtwork2/LabelArtwork/TextList/TextLabel";
const STATUS_LINE: f32 = 13.0;
const STATUS_WIDTH: usize = 62;
const MOST_STATUS_LINES: usize = 17;
/// Sort events of the honour board's headers: its server columns.
pub(super) const SORTS: [(&str, u8); 6] = [("SortRank", 0), ("SortWins", 1), ("SortLosses", 2), ("SortDraws", 3), ("SortKills", 4), ("SortDeaths", 5)];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct RankRow {
    pub rank: u32,
    pub name: String,
    pub score: u32,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub kills: u32,
    pub deaths: u32,
    pub me: bool,
    pub rewards: Vec<Icon>,
}

/// The leaderboards' page and the PvP queue as shown.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Boards {
    pub rows: Vec<RankRow>,
    pub overall: bool,
    pub start: u32,
    pub total: u32,
    /// The honour board's sort column, the filters (0x02 friends, 0x04 guild).
    pub sort: u8,
    pub filters: u16,
    /// Our rank (1-based) and (score, kills, deaths, wins, losses, draws).
    pub own: Option<(u32, [u32; 6])>,
    pub honor: u64,
    /// The matchmaking window's tab: its title, artwork, rules; our queue state.
    pub mode: String,
    pub artwork: String,
    pub rules: String,
    pub status: String,
    pub queued: bool,
    /// The matchmaking window's open tab.
    pub tab: u8,
}

pub(super) fn boards(sources: &Sources) -> Boards {
    let net = sources.net;
    let state = sources.state;
    let overall = state.rank_tabs.is_none_or(|(_, category, _)| category == 0);
    let me = net.local_name.clone().unwrap_or_default();
    let rewards = |rank: u32| -> Vec<Icon> {
        sources.catalog.pvp_rewards.iter().find(|r| (r.first..=r.last).contains(&(rank + 1)))
            .map(|r| r.items.iter().map(|i| sources.icon(i, None)).collect()).unwrap_or_default()
    };
    let (rows, start, total) = if overall {
        net.overall_ranking.as_ref().map(|p| (p.rows.iter().map(|(name, score, rank)| RankRow {
            rank: *rank, name: name.clone(), score: *score, me: *name == me, rewards: rewards(*rank), ..Default::default()
        }).collect(), p.start, p.total)).unwrap_or_default()
    } else {
        net.ranking.as_ref().map(|p| (p.rows.iter().map(|r| RankRow { rank: r.rank, name: r.name.clone(), score: r.score, wins: r.wins, losses: r.losses,
            draws: r.draws, kills: r.kills, deaths: r.deaths, me: r.name == me, rewards: Vec::new() }).collect(), p.start, p.total)).unwrap_or_default()
    };
    let own = net.local_rank.as_ref().filter(|l| l.active && l.rank != super::UNRANKED).map(|l| (l.rank, l.values));
    let tab = state.matchmaking_tab as usize;
    let mode = sources.catalog.pvp_modes.get(tab);
    let queued = net.matchmaking.as_ref().is_some_and(|m| m.state >= 0);
    let status = if let Some(found) = &net.battleground {
        format!("A match is ready on {}. Join battle to enter it.", found.map.replace('_', " "))
    } else if let Some(queue) = net.matchmaking.as_ref().filter(|m| m.state >= 0) {
        let name = sources.catalog.pvp_modes.iter().find(|m| m.mode == queue.mode).map_or_else(|| "this battle".to_owned(), |m| m.title.clone());
        if queue.message.is_empty() { format!("You are in the queue for {name}. Join battle again to leave it.") } else { queue.message.clone() }
    } else {
        String::new()
    };
    Boards { rows, overall, start, total, sort: state.rank_sort, filters: state.rank_filters, own, honor: net.wallet.map_or(0, |w| w[2]),
        mode: mode.map(|m| m.title.clone()).unwrap_or_default(), artwork: mode.map(|m| m.artwork.clone()).unwrap_or_default(),
        rules: mode.map(|m| format!("{}\n\n{}", m.description, m.reward)).unwrap_or_default(), status, queued, tab: state.matchmaking_tab }
}

/// The leaderboards: the PvP inlay's rows in place of its templates, our row
/// highlighted, the sort and filters shown.
pub(super) fn prepare_leaderboards(source: &Layout, boards: &Boards) -> Layout {
    let mut layout = without(source, |id| is_template(id) || id.contains("TemplateRow") || id.contains("/LoadingLabel")
        || (id == EMPTY && !boards.rows.is_empty() && id.starts_with(EMPTY)));
    // The dropdown names the board; there is no other season to pick.
    for w in &mut layout.widgets {
        if w.id.ends_with("/DropDownMenuButton") { w.kind = "Label".into(); w.event.clear(); }
        let sorted = SORTS.iter().find(|(e, _)| *e == w.event).is_some_and(|(_, c)| *c == boards.sort) && !boards.overall;
        let filter = (w.event.starts_with("FilterFriendsOnly") && boards.filters & 0x02 != 0) || (w.event.starts_with("FilterGuildMembersOnly") && boards.filters & 0x04 != 0);
        let owner = w.button.clone().unwrap_or_default();
        if sorted || filter { selected(w); }
        if let Some(button) = layout_button(source, &owner) {
            let sorted = SORTS.iter().find(|(e, _)| *e == button).is_some_and(|(_, c)| *c == boards.sort) && !boards.overall;
            let filter = (button.starts_with("FilterFriendsOnly") && boards.filters & 0x02 != 0) || (button.starts_with("FilterGuildMembersOnly") && boards.filters & 0x04 != 0);
            if sorted || filter { selected(w); }
        }
    }
    let template = if boards.overall { "TemplateRowOverallRanking" } else { "DefaultTemplateRow" };
    for (n, row) in boards.rows.iter().take(super::PAGE_ROWS as usize).enumerate() {
        for mut w in clone_rows(source, &format!("{PVP_INLAY}/{template}"), &format!("{PVP_INLAY}/Row{n}"), n as f32 * ROW_HEIGHT) {
            let rest = w.id.clone();
            if has_part(&rest, "PlayerBackground") && !row.me { continue; }
            if has_part(&rest, "Rewards") { continue; }
            if let Some(slot) = rest.split('/').find_map(|p| p.strip_prefix("ItemSlot")).and_then(|s| s.parse::<usize>().ok()) {
                // The place's reward boxes; empty slots keep their frame.
                if leaf(&rest) == "Icon0" {
                    match row.rewards.get(slot - 1) {
                        Some(icon) => { layout.widgets.extend(icon_layers(&mut w, icon)); set_text(&mut w, ""); }
                        None => continue,
                    }
                }
            }
            w.hidden = false;
            let text = match leaf(&w.id) {
                "Rank" => Some((row.rank + 1).to_string()),
                "Rankscore" => Some(row.score.to_string()),
                "CharacterNameGlobal" => Some(row.name.clone()),
                "Wins" => Some(row.wins.to_string()),
                "Losses" => Some(row.losses.to_string()),
                "Draws" => Some(row.draws.to_string()),
                "Kills" => Some(row.kills.to_string()),
                "Deaths" => Some(row.deaths.to_string()),
                _ => None,
            };
            if let Some(text) = text { set_text(&mut w, &text); }
            layout.widgets.push(w);
        }
    }
    layout
}

/// The event of the button a state widget belongs to.
fn layout_button(source: &Layout, owner: &str) -> Option<String> {
    if owner.is_empty() { return None; }
    source.widgets.iter().find(|w| w.id == owner).map(|w| w.event.clone())
}

/// The matchmaking window: the tab's mode (artwork, rules, our queue state)
/// and our standing.
pub(super) fn prepare_matchmaking(source: &Layout, boards: &Boards) -> Layout {
    let mut layout = without(source, |id| id == STATUS || id.contains("/DeniedGroupMapInfo") || id.contains("/CurrentReward")
        || (id.contains("/TextList/Scroll")));
    let values = boards.own.map(|(_, v)| v).unwrap_or_default();
    let dash = |n: u32| if boards.own.is_some() { n.to_string() } else { "-".to_owned() };
    for w in &mut layout.widgets {
        if w.id.ends_with("/LabelArtwork2/LabelArtwork") && !boards.artwork.is_empty() { w.art = Some(image_art(w.rect, texture_path(&boards.artwork))); }
        if boards.queued && (w.id.ends_with("/ButtonJoinBattle") || w.button.as_deref().is_some_and(|b| b.ends_with("/ButtonJoinBattle"))) { selected(w); }
        let tab = format!("/TabButton0{}", boards.tab + 1);
        if w.button.as_deref().is_some_and(|b| b.ends_with(&tab)) { selected(w); }
        match leaf(&w.id) {
            "LeaderboardRankNumber" => set_text(w, &boards.own.map_or("unranked".to_owned(), |(rank, _)| format!("#{rank}"))),
            "LeaderboardScoreValue" => set_text(w, &boards.honor.to_string()),
            "MatchesPlayedValue" => set_text(w, &dash(values[3] + values[4] + values[5])),
            "MatchesWonValue" => set_text(w, &dash(values[3])),
            "MatchesDrawValue" => set_text(w, &dash(values[5])),
            "PlayersDefeatedValue" => set_text(w, &dash(values[1])),
            _ => {}
        }
    }
    // The rules, then our queue state, down the text list.
    if let Some(template) = source.widgets.iter().find(|w| w.id == STATUS) {
        let mut lines = vec![boards.mode.to_uppercase(), String::new()];
        for paragraph in boards.rules.split('\n') { lines.extend(if paragraph.is_empty() { vec![String::new()] } else { wrap(paragraph, STATUS_WIDTH, 40) }); }
        if !boards.status.is_empty() { lines.push(String::new()); lines.extend(wrap(&boards.status, STATUS_WIDTH, 4)); }
        for (n, line) in lines.iter().take(MOST_STATUS_LINES).enumerate() {
            let mut w = template.clone();
            w.id = format!("{STATUS}/Line{n}");
            shift(&mut w, 0.0, n as f32 * STATUS_LINE);
            set_text(&mut w, line);
            layout.widgets.push(w);
        }
    }
    let _ = FRAME;
    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::template::fixture::{find, layout, text_of};

    #[test]
    fn the_boards_rows_fill_the_pvp_inlay_and_mark_our_row() {
        let rows = format!("{PVP_INLAY}/DefaultTemplateRow");
        let overall = format!("{PVP_INLAY}/TemplateRowOverallRanking");
        let ids: Vec<(String, &str, bool)> = vec![(rows.clone(), "Label", false), (format!("{rows}/Rank"), "TextLabel", true), (format!("{rows}/CharacterName/CharacterNameGlobal"), "TextLabel", true),
            (format!("{rows}/PlayerBackground"), "Label", false), (format!("{rows}/Rewards/Rewards1"), "Label", false),
            (overall.clone(), "Label", false), (format!("{overall}/Rankscore"), "TextLabel", true), (format!("{overall}/Label/ItemSlot1"), "Slot", false), (format!("{overall}/Label/ItemSlot1/Icon0"), "Icon", false),
            (format!("{overall}/Label/ItemSlot2/Icon0"), "Icon", false), (format!("{PVP_INLAY}/LoadingLabel/LoadingLabelTextLabel"), "TextLabel", true), (EMPTY.into(), "TextLabel", true)];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let source = layout("leaderboards", &refs);
        let boards = Boards { rows: vec![RankRow { rank: 0, name: "Ann".into(), score: 50, ..Default::default() }, RankRow { rank: 1, name: "Bob".into(), me: true, ..Default::default() }], overall: false, ..Default::default() };
        let prepared = prepare_leaderboards(&source, &boards);
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.clone()).collect();
        assert_eq!(text_of(&prepared, &format!("{PVP_INLAY}/Row0/Rank")), "1");
        assert_eq!(text_of(&prepared, &format!("{PVP_INLAY}/Row1/CharacterName/CharacterNameGlobal")), "Bob");
        assert_eq!(find(&prepared, &format!("{PVP_INLAY}/Row1")).rect[1], 192.0 + ROW_HEIGHT);
        assert!(ids.contains(&format!("{PVP_INLAY}/Row1/PlayerBackground")) && !ids.contains(&format!("{PVP_INLAY}/Row0/PlayerBackground")));
        assert!(!ids.iter().any(|id| id.contains("Template") || id.contains("LoadingLabel") || id.contains("Rewards") || *id == EMPTY));
        let overall = prepare_leaderboards(&source, &Boards { rows: vec![RankRow { score: 50, rewards: vec![Icon { image: "box.png".into(), ..Default::default() }], ..Default::default() }], overall: true, ..Default::default() });
        assert_eq!(text_of(&overall, &format!("{PVP_INLAY}/Row0/Rankscore")), "50");
        assert!(overall.widgets.iter().any(|w| w.id == format!("{PVP_INLAY}/Row0/Label/ItemSlot1/Icon0/Image")));
        assert!(!overall.widgets.iter().any(|w| w.id.contains("/Row0/Label/ItemSlot2/Icon0")));
        let empty = prepare_leaderboards(&source, &Boards::default());
        assert!(empty.widgets.iter().any(|w| w.id == EMPTY));
    }

    #[test]
    fn the_matchmaking_window_shows_the_mode_and_our_standing() {
        let ids: Vec<(String, &str, bool)> = vec![(STATUS.into(), "TextLabel", true), (format!("{FRAME}/LabelArtwork2/LabelArtwork"), "Label", false),
            (format!("{FRAME}/LabelArtwork2/LabelArtwork/TextList/ScrollUp"), "Button", false), (format!("{FRAME}/Label/LeaderboardRankNumber"), "TextLabel", true),
            (format!("{FRAME}/NextBattlemode/ButtonJoinBattle"), "Button", false), (format!("{FRAME}/NextBattlemode/ButtonJoinBattle/pressed"), "Label", false)];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let source = layout("matchmakingwindow", &refs);
        let boards = Boards { own: Some((7, [870, 3, 1, 2, 1, 0])), mode: "Duel".into(), artwork: "ui/matchmaking_background_1v1_crunch".into(),
            rules: "Duel Rules:\n- Win two points".into(), status: "You are in the queue for Duel.".into(), queued: true, ..Default::default() };
        let prepared = prepare_matchmaking(&source, &boards);
        assert_eq!(text_of(&prepared, &format!("{FRAME}/Label/LeaderboardRankNumber")), "#7");
        assert_eq!(text_of(&prepared, &format!("{STATUS}/Line0")), "DUEL");
        assert_eq!(text_of(&prepared, &format!("{STATUS}/Line2")), "Duel Rules:");
        assert!(prepared.widgets.iter().any(|w| w.text.as_ref().is_some_and(|t| t.text == "You are in the queue for Duel.")));
        assert_eq!(find(&prepared, &format!("{FRAME}/LabelArtwork2/LabelArtwork")).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui/matchmaking_background_1v1_crunch.png"));
        assert_eq!(find(&prepared, &format!("{FRAME}/NextBattlemode/ButtonJoinBattle/pressed")).state.as_deref(), Some("normal"));
        assert!(!prepared.widgets.iter().any(|w| w.id.contains("/TextList/Scroll")));
    }
}
