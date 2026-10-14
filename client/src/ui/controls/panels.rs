//! The source files contain mutually exclusive pages and data templates in one
//! tree. Keep page selection explicit instead of guessing from overlapping rects.
use super::{Action, Section, Widget};

const REGIONS: [&str; 9] = ["Duria", "Teganswall", "Northlands", "Dwarflands", "Parallelworlds", "Atlantis", "Lortac", "Qaizah", "LandOfImmortals"];
const QUEST_PAGES: [&str; 5] = ["QuestlogLabel", "CompendiumLabel", "AchievementsLabel", "DailyAchievementsLabel", "WantedMissionsLabel"];
const SOCIAL_TABS: [&str; 6] = ["Search", "Friends", "Group", "Guild", "Ignore", "Invite"];
const LEADERBOARD_TABS: [&str; 7] = ["Tab1v1", "Tab3v3", "Tab5v5", "Tab6v6", "TabHP", "TabHPDay", "TabHPWeek"];

pub(super) fn frame_suffix(window: &str) -> Option<&'static str> {
    Some(match window {
        "questlog" => "/BackGroundLabel/Frame2",
        "travelmap" => "/Travelmap/Frame/Frame/Frame",
        "locationmap" => "Canvas/map",
        "socialscreen" => "/FrameLabel/BorderLabel",
        "pvpleaderboards" => "/WindowTitleBar/Frame",
        "eventwindow" => "/BackGroundLabel/Frame",
        "shopwindow" => "/Frame/Frame/Frame1",
        "matchmakingwindow" => "/WindowTitleLabel/Frame/Frame2",
        _ => return None,
    })
}

fn page(group: &'static str, index: usize) -> Section { Section::Page(group, 1 << index) }

pub(super) fn section(window: &str, parts: &[&str]) -> Option<Section> {
    if !super::NAV_WINDOWS.contains(&window) && window != "skilltalentswindow" { return None; }
    let has = |name: &str| parts.contains(&name);
    // These rows/cards are cloned when data arrives, not sample player data.
    if parts.iter().any(|p| p.contains("Template") || p.starts_with("RecentAchievement") || *p == "LoadingLabel") {
        return Some(Section::Hidden);
    }
    match window {
        "skilltalentswindow" => {
            if has("ExperienceLabel") && has("OptionsLabel") { return Some(Section::Hidden); }
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
            for (name, index) in [("PanelSearch", 0), ("TableHeadSearch", 0), ("TableHeadFriends", 1),
                ("OptionsFriends", 1), ("TableHeadGroup", 2), ("OptionsGroup", 2), ("TableHeadGuild", 3),
                ("OptionsGuild", 3), ("LabelFoundGuildBackground", 3), ("TableHeadIgnore", 4),
                ("LabelInvitePlayerBackground", 5), ("TableHeadGroupFinder", 6), ("OptionsGroupFinder", 6)] {
                if has(name) { return Some(page("social", index)); }
            }
            if has("MenuBackground") || has("DummyScrollbarDown") || has("DummyScrollbarUp") { return Some(Section::Hidden); }
        }
        "pvpleaderboards" => {
            if has("TableBackgroundPvE") { return Some(Section::Hidden); }
            if has("DefaultTableColumnHeading") { return Some(Section::Page("leaderboard", 0b0001011)); }
            if has("CTFTableColumnHeading") { return Some(page("leaderboard", 2)); }
            if has("HonorPointsTableColumnHeading") { return Some(Section::Page("leaderboard", 0b1110000)); }
        }
        "shopwindow" => {
            if has("FlagLabelCategory") || has("AdvertisementArtwork") { return Some(Section::Hidden); }
            for (name, index) in [("DealsPage", 0), ("OffersPage", 1), ("VoucherLabel", 2)] {
                if has(name) { return Some(page("shop", index)); }
            }
            if parts.iter().any(|p| matches!(*p, "CinemaBackground" | "TieredSpenderLabel" | "PaymentMethodSettingsLabel"
                | "SteamPaymentButton" | "ExternalPaymentButton" | "HighlightSale" | "MoreOffersIndicator")) {
                return Some(Section::Hidden);
            }
        }
        "eventwindow" => {
            // No event payload has arrived. The authored no-event panel is the
            // correct empty state; active/upcoming art must not stack over it.
            if parts.iter().any(|p| matches!(*p, "BottomActiveEvent" | "BottomUpcomingEvent" | "TabBarLabel" | "TabBarLabel_UpcomingEvents"
                | "ProgressbarLabel" | "BannerLabel" | "LabelArtwork")) { return Some(Section::Hidden); }
        }
        "matchmakingwindow" => {
            if has("LeaderboardSeason") || has("NextBattlemode") { return Some(Section::Hidden); }
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
            if let Some(i) = leaf.strip_prefix("PresetButton").and_then(|s| s.parse::<usize>().ok()) { return tab("preset", i.saturating_sub(1)); }
            if event == "Shop" || event == "pvpShop" { return Some(Action::Window("shopwindow")); }
        }
        "questlog" => {
            if let Some(i) = ["QuestTabSelected", "CompendiumTabSelected", "AchievementsTabSelected", "DailyAchievementTabSelected", "WantedMissionsSelected"].iter().position(|e| *e == event) {
                return tab("quest", i);
            }
        }
        "socialscreen" => {
            if let Some(i) = leaf.strip_prefix("TabButton").and_then(|name| SOCIAL_TABS.iter().position(|s| *s == name)) { return tab("social", i); }
            match event {
                "ShowGroupFinder" => return tab("social", 6),
                "GroupInviteNearbyPlayers" | "GroupInviteFriends" => return tab("social_overlay", 1),
                "CloseMultipleInvite" => return tab("social_overlay", 0),
                _ => {}
            }
        }
        "travelmap" => {
            if event == "ReturnToOverviewMap" { return tab("region", 0); }
            if widget.id.contains("/map_buttons/") { return Some(Action::Choose("destination", widget.id.clone())); }
            if widget.id.contains("/RegionButtons/") {
                if let Some(i) = REGIONS.iter().position(|s| *s == leaf) { return tab("region", i + 1); }
            }
        }
        "pvpleaderboards" => {
            if let Some(i) = LEADERBOARD_TABS.iter().position(|e| *e == event) { return tab("leaderboard", i); }
        }
        "shopwindow" => {
            if let Some(i) = leaf.strip_prefix("ShopTab").and_then(|s| s.parse::<usize>().ok()) { return tab("shop", i.saturating_sub(1)); }
            if let Some(i) = leaf.strip_prefix("Counter").and_then(|s| s.parse::<usize>().ok()) { return tab("banner", i); }
        }
        "matchmakingwindow" => {
            if leaf == "TabButton01" { return tab("matchmaking", 0); }
            if leaf == "TabButton02" { return tab("matchmaking", 1); }
            if event == "openLeaderboard" { return Some(Action::Window("pvpleaderboards")); }
        }
        "eventwindow" => {
            if let Some(i) = leaf.strip_prefix("TabEvent").and_then(|s| s.parse::<usize>().ok()) { return tab("event", i); }
            if matches!(event, "OpenShop" | "OpenEventShop" | "OpenRealMoneyShop") { return Some(Action::Window("shopwindow")); }
            if event == "OpenLeaderboards" { return Some(Action::Window("pvpleaderboards")); }
            if event == "OpenAchievements" { return Some(Action::Window("questlog")); }
        }
        _ => return None,
    }
    if leaf.ends_with("CheckBox") || event.starts_with("Filter") || event.starts_with("Sort") {
        return Some(Action::Toggle(widget.id.clone()));
    }
    None
}
