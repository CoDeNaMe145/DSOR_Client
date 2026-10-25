//! The source files contain mutually exclusive pages and data templates in one
//! tree. Keep page selection explicit instead of guessing from overlapping rects.
use super::{Action, Section, Widget};

const REGIONS: [&str; 9] = ["Duria", "Teganswall", "Northlands", "Dwarflands", "Parallelworlds", "Atlantis", "Lortac", "Qaizah", "LandOfImmortals"];
const QUEST_PAGES: [&str; 5] = ["QuestlogLabel", "CompendiumLabel", "AchievementsLabel", "DailyAchievementsLabel", "WantedMissionsLabel"];
const SOCIAL_TABS: [&str; 6] = ["Search", "Friends", "Group", "Guild", "Ignore", "Invite"];

pub(super) fn frame_suffix(window: &str) -> Option<&'static str> {
    Some(match window {
        "questlog" => "/BackGroundLabel/Frame2",
        "travelmap" => "/Travelmap/Frame/Frame/Frame",
        "collectorsalbum" => "/BackGroundLabel/Frame1",
        "socialscreen" => "/FrameLabel/BorderLabel",
        "leaderboards" => "/WindowBackground/Frame",
        "eventwindow" => "/BackGroundLabel/Frame",
        "shopwindow" => "/Frame/Frame/Frame1",
        "matchmakingwindow" => "/WindowTitleLabel/Frame/Frame2",
        _ => return None,
    })
}

fn page(group: &'static str, index: usize) -> Section { Section::Page(group, 1 << index) }

pub(super) fn section(window: &str, parts: &[&str]) -> Option<Section> {
    if !super::NAV_WINDOWS.contains(&window) && !matches!(window, "skilltalentswindow" | "collectorsalbum") { return None; }
    let has = |name: &str| parts.contains(&name);
    // These rows/cards are cloned when data arrives, not sample player data.
    // The recent achievements' slots (RecentAchievement0..6), not their panel.
    let recent_slot = |p: &str| p.strip_prefix("RecentAchievement").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if parts.iter().any(|p| p.contains("Template") || recent_slot(p) || *p == "LoadingLabel") {
        return Some(Section::Hidden);
    }
    match window {
        "collectorsalbum" => {
            // controls::systems::album shows the front page or a book's page.
            if parts.iter().any(|p| matches!(*p, "AnimUnlock" | "AnimUse" | "AnimIconGlow" | "FlagLabelCategory" | "FlagLabelSubcategory")) {
                return Some(Section::Hidden);
            }
        }
        "skilltalentswindow" => {
            if has("ExperienceLabel") && has("OptionsLabel") { return Some(Section::Hidden); }
            if let Some(class) = parts.iter().find_map(|p| p.strip_prefix("Class").and_then(|s| s.parse().ok())) {
                return Some(Section::SkillClass(class));
            }
            for (i, name) in ["ExperienceLabel", "WisdomLabel", "GroupLabel", "FameLabel"].iter().enumerate() {
                if has(name) { return Some(page("talents", i)); }
            }
        }
        "questlog" => {
            if has("ChapterOverview") { return Some(Section::Hidden); }
            for (i, name) in QUEST_PAGES.iter().enumerate() {
                if has(name) { return Some(page("quest", i)); }
            }
        }
        "travelmap" => {
            if parts.iter().any(|p| p.starts_with("mark_")) { return Some(Section::Hidden); }
            if has("OverviewMap") { return Some(page("region", 0)); }
            if has("RegionMaps") {
                for (i, name) in REGIONS.iter().enumerate() {
                    if has(name) { return Some(page("region", i + 1)); }
                }
            }
            if has("PrivilegeLabel") { return Some(Section::Hidden); }
        }
        "socialscreen" => {
            if has("InviteMultipleLabel") { return Some(page("social_overlay", 1)); }
            // The lists' rows and scroll bars (systems::rosters) show on their tab;
            // the List's own scroll bar on the tabs without a list.
            if let Some(tab) = parts.iter().find_map(|p| super::systems::rosters::list_tab(p)) { return Some(page("social", tab as usize)); }
            if has("List") && parts.iter().any(|p| matches!(*p, "ScrollbarBackground" | "ScrollUp" | "ScrollDown")) {
                return Some(Section::Page("social", super::systems::rosters::EMPTY_TABS));
            }
            for (name, index) in [("PanelSearch", 0), ("TableHeadSearch", 0), ("TableHeadFriends", 1),
                ("OptionsFriends", 1), ("TableHeadGroup", 2), ("OptionsGroup", 2), ("TableHeadGuild", 3),
                ("OptionsGuild", 3), ("LabelFoundGuildBackground", 3), ("TableHeadIgnore", 4),
                ("LabelInvitePlayerBackground", 5), ("TableHeadGroupFinder", 6), ("OptionsGroupFinder", 6)] {
                if has(name) { return Some(page("social", index)); }
            }
            // MenuBackground, the brown panels behind the column heads and the
            // bottom controls, shows on every tab (systems::rosters draws it first).
            if has("DummyScrollbarDown") || has("DummyScrollbarUp") { return Some(Section::Hidden); }
        }
        "leaderboards" => {
            // PlanarFissure: the October 2026 client's fourth board (no group
            // button, no server board); exported hidden.
            if parts.iter().any(|p| matches!(*p, "TimerBackgroundLabel" | "PlayerLabelTop" | "PlanarFissure")) { return Some(Section::Hidden); }
            for (i, name) in ["PvP", "PvE", "Event"].iter().enumerate() {
                if has(name) {
                    if has("HeaderOverallRanking") { return Some(Section::Ranking(i as u8, true)); }
                    if has("DefaultTableColumnHeading") || has("Header") { return Some(Section::Ranking(i as u8, false)); }
                    return Some(page("rank_group", i));
                }
            }
            if let Some(group) = parts.iter().find_map(|p| p.strip_prefix("RankGroup").and_then(|s| s.parse().ok())) {
                return Some(page("rank_group", group));
            }
        }
        "shopwindow" => {
            // controls::systems::shop shows the open category's page and banner.
            if parts.iter().any(|p| matches!(*p, "FlagLabelCategory" | "CinemaBackground" | "TieredSpenderLabel" | "PaymentMethodSettingsLabel"
                | "SteamPaymentButton" | "ExternalPaymentButton" | "HighlightSale" | "MoreOffersIndicator")) {
                return Some(Section::Hidden);
            }
        }
        "eventwindow" => {
            // controls::systems::boards shows the running events' parts; the
            // upcoming ones' (not scheduled by this server) and the banner stay out.
            if parts.iter().any(|p| matches!(*p, "BottomUpcomingEvent" | "TabBarLabel_UpcomingEvents")) { return Some(Section::Hidden); }
        }
        "matchmakingwindow" => {
            // No season and no mode rotation: their timers stay out, the join
            // button (controls::systems) stays.
            if has("LeaderboardSeason") || (has("NextBattlemode") && (has("TimerBackgroundLabel1") || has("TimeNextModeText"))) { return Some(Section::Hidden); }
        }
        "locationmap" => {
            if has("marker") || has("marker2") || has("markerdesc") || has("minimapcorner") { return Some(Section::Hidden); }
        }
        _ => {}
    }
    None
}

pub(super) fn action(window: &str, widget: &Widget) -> Option<Action> {
    let leaf = widget.id.rsplit('/').next().unwrap_or("");
    let event = widget.event.as_str();
    let tab = |group, index| Some(Action::Tab(group, index as u8));
    match window {
        "skilltalentswindow" => {
            if let Some(i) = ["ExperienceTab", "KnowledgeTab", "GroupTab", "FameTab"].iter().position(|s| *s == leaf) { return tab("talents", i); }
        }
        "questlog" => {
            if let Some(i) = ["QuestTabSelected", "CompendiumTabSelected", "AchievementsTabSelected", "DailyAchievementTabSelected", "WantedMissionsSelected"].iter().position(|e| *e == event) {
                return tab("quest", i);
            }
            // The rows super::quests::prepare_log clones.
            if let Some(id) = event.strip_prefix("QuestSelect:") { return Some(Action::QuestSelect(id.to_owned())); }
            if let Some(id) = event.strip_prefix("QuestCancel:") { return Some(Action::QuestCancel(id.to_owned())); }
            if let Some(kind) = event.strip_prefix("QuestGroup:") { return Some(Action::QuestGroup(kind.to_owned())); }
        }
        "activequestwindow" => {
            match event {
                "PreviousQuest" => return Some(Action::QuestTrack(-1)),
                "NextQuest" => return Some(Action::QuestTrack(1)),
                "SwitchToCompact" => return Some(Action::QuestCompact(true)),
                "SwitchToFull" => return Some(Action::QuestCompact(false)),
                "OpenQuestlog" => return Some(Action::Window("questlog")),
                _ => return None,
            }
        }
        super::party::WINDOW => {
            match event {
                "SwitchToCompact" => return Some(Action::PartyCompact(true)),
                "SwitchToFull" => return Some(Action::PartyCompact(false)),
                _ => return None,
            }
        }
        "socialscreen" => {
            // The invitation campaign (friend invites with rewards) has no server.
            if leaf == "TabButtonInvite" { return Some(Action::Unavailable); }
            if let Some(i) = leaf.strip_prefix("TabButton").and_then(|name| SOCIAL_TABS.iter().position(|s| *s == name)) { return tab("social", i); }
            // The group and guild buttons send their requests (controls::social::answer).
            let social = match event {
                "LeaveGroup" => Some(super::social::Social::LeaveGroup),
                "FoundGroup" => Some(super::social::Social::FoundGroup),
                "LeaveGuild" => Some(super::social::Social::LeaveGuild),
                "FoundGuild" => Some(super::social::Social::FoundGuild),
                _ => None,
            };
            if let Some(social) = social { return Some(Action::Social(social)); }
            match event {
                "ShowGroupFinder" => return tab("social", 6),
                "GroupInviteNearbyPlayers" | "GroupInviteFriends" => return tab("social_overlay", 1),
                "CloseMultipleInvite" => return tab("social_overlay", 0),
                _ => {}
            }
        }
        "travelmap" => {
            if event == "ReturnToOverviewMap" { return tab("region", 0); }
            // A destination's button is named by its map id (a0200_kingscity).
            if widget.id.contains("/map_buttons/") { return Some(Action::Travel(leaf.to_owned())); }
            if widget.id.contains("/RegionButtons/") {
                if let Some(i) = REGIONS.iter().position(|s| *s == leaf) { return tab("region", i + 1); }
            }
        }
        "leaderboards" => {
            if let Some(i) = event.strip_prefix("RankGroup:").and_then(|s| s.parse().ok()) { return tab("rank_group", i); }
            if let Some(i) = event.strip_prefix("RankCategory:").and_then(|s| s.parse().ok()) { return tab("rank_category", i); }
            if let Some(i) = leaf.strip_prefix("TabButton0").and_then(|s| s.parse::<usize>().ok()) { return tab("rank_class", i.saturating_sub(1)); }
        }
        "shopwindow" => {
            if let Some(i) = leaf.strip_prefix("ShopTab").and_then(|s| s.parse::<usize>().ok()) { return tab("shop", i.saturating_sub(1)); }
            if let Some(i) = leaf.strip_prefix("Counter").and_then(|s| s.parse::<usize>().ok()) { return tab("banner", i); }
        }
        "matchmakingwindow" => {
            if leaf == "TabButton01" { return tab("matchmaking", 0); }
            if leaf == "TabButton02" { return tab("matchmaking", 1); }
            if event == "openLeaderboard" { return Some(Action::Window("leaderboards")); }
        }
        "eventwindow" => {
            if let Some(i) = leaf.strip_prefix("TabEvent").and_then(|s| s.parse::<usize>().ok()) { return tab("event", i); }
            if matches!(event, "OpenShop" | "OpenEventShop" | "OpenRealMoneyShop") { return Some(Action::Window("shopwindow")); }
            if event == "OpenLeaderboards" { return Some(Action::Window("leaderboards")); }
            if event == "OpenAchievements" { return Some(Action::Window("questlog")); }
        }
        // The membership page's shop buttons.
        "charactersheet" => {
            if matches!(event, "OpenShop" | "OpenRealMoneyShop" | "ShowPremiumBenefits") { return Some(Action::Window("shopwindow")); }
            return None;
        }
        _ => return None,
    }
    if leaf.ends_with("CheckBox") || event.starts_with("Filter") || event.starts_with("Sort") {
        return Some(Action::Toggle(widget.id.clone()));
    }
    None
}
