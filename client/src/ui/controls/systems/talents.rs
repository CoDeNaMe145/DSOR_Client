//! The Skills & Talents window (skilltalentswindow): the Experience tab (the
//! class's skills and skill talents), Wisdom (Knowledge), Group and Honor
//! (Fame), as release261's Talents::SkillTalentsWidget fills them from
//! Talents::ClientTalentManager; the reset confirmation
//! (confirmingamepaymentdialog) and the split dialog's talent mode
//! (unstackwindow, "Talent Level").
//!
//! What is picked comes from the server (TalentSelectionInfo: each tree's
//! active preset and the ranks in it; TalentCategoryPointsInfo: each tree's
//! XP), the rules from the talents catalog (crate::gameplay::talents). The
//! client sends one command a click and waits for the server's answer: nothing
//! changes here before it (the original updates nothing optimistically).
//!
//! SOURCE: release261 SkillTalentsWidget, ClientTalentManager and
//!   PlayerTalentModule, decompiled with annotations in the live server tree's
//!   data/diagnostics/skilltalents-widget.annotated.txt and
//!   talents-manager.annotated.txt (the addresses below are there);
//!   DSOR_Server services/talents.py (what the server accepts).
use bevy::prelude::*;
use std::collections::HashMap;

use crate::gameplay::talents::{Talent, TalentCatalog, Tree};
use super::super::template::{clip_y, clone_at, has_part, image_art, leaf, selected, set_text};
use super::Sources;
use crate::ui::{Layout, Widget};

pub(in crate::ui) const WINDOW: &str = "skilltalentswindow";
/// The reset's confirmation (the ConfirmVC dialog: its question and its gold).
pub(in crate::ui) const DIALOG: &str = "confirmingamepaymentdialog";
/// The split dialog, in its talent mode for a right click (UI::StackSplitWidget::SetupTalentMode 0x140540834).
pub(in crate::ui) const SPLIT: &str = "unstackwindow";

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 4] { [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0] }
/// A counter with bonus ranks from equipment (0x14036c93c).
const GREEN: [f32; 4] = rgb(143, 232, 143);
/// A locked talent's level, on its disabled face.
const LOCKED: [f32; 4] = rgb(189, 3, 3);
/// An Honor focus talent's counter: no point in it, some.
const FOCUS_IDLE: [f32; 4] = rgb(78, 78, 78);
const FOCUS_ACTIVE: [f32; 4] = rgb(255, 225, 165);
pub(in crate::ui) const WHITE: [f32; 4] = [1.0; 4];
pub(in crate::ui) const RED: [f32; 4] = rgb(255, 64, 64);
/// The cards' hints, and "(Left-click to level up)" (0x14060c554).
pub(in crate::ui) const YELLOW: [f32; 4] = rgb(255, 226, 38);
pub(in crate::ui) const GREY: [f32; 4] = rgb(205, 205, 205);
pub(in crate::ui) const BLUE: [f32; 4] = rgb(120, 170, 255);
/// The level bars' "missing" lines, by tree (0x140612278).
const BAR_COLOURS: [[f32; 4]; 4] = [rgb(87, 238, 255), rgb(140, 48, 201), rgb(237, 85, 47), rgb(237, 85, 47)];
/// The tabs' parts: label, level bar parent, available points.
const TABS: [&str; 4] = ["ExperienceLabel", "WisdomLabel", "GroupLabel", "FameLabel"];
const TAB_BUTTONS: [&str; 4] = ["ExperienceTab", "KnowledgeTab", "GroupTab", "FameTab"];
/// Below this level no skill talent opens, whatever its own (0x14036c93c).
const FIRST_TALENT_LEVEL: u32 = 4;
/// The Skill tree's presets from the fifth on are bought (UnlockPreselectionCost):
/// the most any membership gives (NumTalentSets, Deluxe).
const BOUGHT_PRESETS_FROM: u8 = 4;
const SHOP_TALENT: &str = "trait_wisdom_all_dungeon_shop";
const WORKBENCH_TALENT: &str = "trait_wisdom_all_dungeon_workbench";
pub(in crate::ui) const WISDOM_TRADER: &str = "skilltree_wisdom_trader";
pub(in crate::ui) const PVP_TRADER: &str = "skilltree_pvp_trader";

/// A hover card's lines (crate::ui::controls::tooltips draws them).
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Tip {
    pub title: String,
    pub icon: String,
    pub lines: Vec<(String, [f32; 4])>,
}

impl Tip {
    fn new(title: impl Into<String>) -> Self { Self { title: title.into(), ..default() } }
    fn line(&mut self, text: impl Into<String>, colour: [f32; 4]) { let t = text.into(); if !t.is_empty() { self.lines.push((t, colour)); } }
}

/// A talent as the window shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Entry {
    pub id: String,
    pub tree: Tree,
    /// A skill's id (the Experience grid's skill cells carry it), else empty.
    pub skill: String,
    /// A trait (Honor): on or off, a point each.
    pub toggle: bool,
    pub icon: String,
    pub rank: u32,
    /// Ranks from equipment (TalentLevelBonus, crate::gameplay::stats::TalentBonuses):
    /// the counters show them in green, their most raised by them (the
    /// Experience grid's), the purchase ignores them.
    pub bonus: u32,
    pub max: u32,
    /// The level it waits for, when too low: its disabled face shows it.
    pub locked: Option<u32>,
    /// Another talent of its exclusive group has ranks (the skill tree's
    /// GroupId: the elemental masters, the builds).
    pub blocked: bool,
    /// The next rank's price, the points left pay it.
    pub cost: u32,
    pub affordable: bool,
    /// Its category's ranks are at their most (Wisdom, Group).
    pub full: bool,
    /// The most ranks one purchase can add (the split dialog's slider).
    pub most: u32,
    /// The skill is on the action bar: its glow (Template_glow).
    pub on_bar: bool,
    pub tip: Tip,
}

/// A Wisdom or Group category: its box, in its column.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Branch {
    pub id: String,
    pub tree: Tree,
    pub column: u32,
    pub title: String,
    /// One of its talents has bonus ranks: the title is green.
    pub green: bool,
    pub talents: Vec<Entry>,
    /// The points its talents cost: what a reset gives back, and its price.
    pub points: u32,
    pub cost: u64,
    pub reset_tip: Tip,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) enum PresetState {
    Active,
    #[default]
    Open,
    /// Bought with Andermant (the Skill tree's fifth to eighth).
    Locked,
    /// A membership's (NumTalentSets).
    Privilege,
    /// A preset cooldown effect runs (the server's SkillPresetCooldown...).
    Cooldown,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Preset {
    pub state: PresetState,
    pub tip: Tip,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct TreeView {
    pub level: u32,
    pub max_level: u32,
    /// The bar: level / most levels (UVProgressBar::SetValues(level, max)).
    pub fraction: f32,
    pub available: i64,
    pub spent: u32,
    pub preset: u8,
    /// The tab opens at the level its first talent needs (0x140a23060).
    pub unlocked: bool,
    pub points_text: String,
    pub presets: Vec<Preset>,
    /// The tree's preset cooldown effect ends at this game tick (0: never),
    /// while it runs: the waiting presets' cards count it down.
    pub cooldown_end: Option<u32>,
    /// Their line, "{0:i}" the seconds left (preSelectionActiveCooldown).
    pub cooldown_text: String,
    pub reset_cost: u64,
    pub bar_tip: Tip,
    pub points_tip: Tip,
    pub reset_tip: Tip,
    pub tab_tip: Tip,
}

/// The client's messages for a refused pick (ClientTalentManager::SelectTalent 0x14035e5e4).
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Messages {
    pub points: String,
    pub level: String,
    pub category: String,
    pub talent: String,
    pub skill: String,
    pub group: String,
    pub money: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Talents {
    /// The catalog and our class are known.
    pub ready: bool,
    pub class: u8,
    pub level: u32,
    /// UI::SkillTalentsWidget::SetEditable: off, nothing can be picked.
    pub editable: bool,
    pub trees: Vec<TreeView>,
    pub skills: Vec<Entry>,
    pub branches: Vec<Branch>,
    pub focus: Vec<Entry>,
    pub traits: Vec<Entry>,
    /// The Wisdom tab's portable shop and workbench (their talents bought).
    pub shop: bool,
    pub workbench: bool,
    /// The money a reset is paid with (copper) and the Andermant a preset is.
    pub gold: u64,
    pub andermant: u64,
    /// The next Skill preset's price, and how many were bought.
    pub unlock_price: Option<u32>,
    pub unlocked_presets: u32,
    pub messages: Messages,
    pub texts: HashMap<&'static str, String>,
}

impl Talents {
    pub fn tree(&self, tree: Tree) -> Option<&TreeView> { self.trees.get(tree as usize) }
    /// Any talent shown, by id.
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.skills.iter().chain(self.branches.iter().flat_map(|b| &b.talents)).chain(&self.focus).chain(&self.traits).find(|e| e.id == id)
    }
    /// The Experience grid's entry of a skill (its skill id).
    pub fn skill(&self, skill: &str) -> Option<&Entry> { self.skills.iter().find(|e| !e.skill.is_empty() && e.skill == skill) }
    pub fn branch(&self, id: &str) -> Option<&Branch> { self.branches.iter().find(|b| b.id == id) }
    fn text(&self, key: &'static str) -> &str { self.texts.get(key).map(String::as_str).unwrap_or("") }
}

/// The reset or unlock waiting for its confirmation (DIALOG).
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Ask {
    pub question: String,
    pub price: String,
    pub action: Confirmed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) enum Confirmed {
    #[default]
    Nothing,
    /// ResetTalentsCommand (286): the tree, its active preset.
    Reset(u8, u8),
    /// ResetTalentCategoryCommand (287): the tree, the category, its preset.
    ResetBranch(u8, String, u8),
    /// UnlockSkillTalentPreselectionCommand (284): the presets bought then.
    Unlock(u32),
}

/// A gui.window text with its arguments: "{0:i}", "{1:s:name}"... in order.
pub(in crate::ui) fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 8);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else { out.push_str(&rest[start..]); return out };
        let inner = &rest[start + 1..start + end];
        match inner.split(':').next().and_then(|n| n.parse::<usize>().ok()).and_then(|n| args.get(n)) {
            Some(arg) => out.push_str(arg),
            None => out.push_str(&rest[start..=start + end]),
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}

/// A whole or one-decimal number, as the client writes a percentage.
fn number(value: f32) -> String {
    if (value - value.round()).abs() < 0.005 { format!("{}", value.round() as i64) } else { format!("{value:.1}") }
}

/// Gold, silver and copper of an amount of copper.
pub(in crate::ui) fn money(copper: u64) -> String {
    let (gold, silver, copper) = (copper / 10_000, copper / 100 % 100, copper % 100);
    let mut parts = Vec::new();
    if gold > 0 { parts.push(format!("{gold} Gold")); }
    if silver > 0 { parts.push(format!("{silver} Silver")); }
    if copper > 0 || parts.is_empty() { parts.push(format!("{copper} Copper")); }
    parts.join(" ")
}

/// The gold a reset of `spent` points costs (PlayerTalentModule 0x1409d0960):
/// VCRespecCostsPerPoint x RespecVCCostMultiplier[level] x points x the
/// TalentRespecCostFactor, rounded; past 999999, to the gold (10000 copper).
pub(in crate::ui) fn reset_cost(catalog: &TalentCatalog, level: u32, spent: u32, factor: f32) -> u64 {
    let per_point = catalog.global_f32("VCRespecCostsPerPoint").unwrap_or(1.0);
    let multiplier = catalog.respec.get(level.max(1) as usize - 1).or(catalog.respec.last()).copied().unwrap_or(0.0);
    let cost = (per_point * multiplier * spent as f32 * factor + 0.5).floor().max(0.0) as u64;
    if cost > 999_999 { cost / 10_000 * 10_000 } else { cost }
}

fn class_name(class: u8) -> &'static str { ["warrior", "mage", "ranger", "dwarf", "niwalk"].get(class as usize).copied().unwrap_or("warrior") }

/// The view (systems::refresh): built when the window is open and what it shows changed.
pub(in crate::ui) fn talents(s: &Sources) -> Talents {
    let (Some(catalog), Some(class)) = (s.talents, s.class) else { return Talents::default() };
    let net = s.net;
    let cls = class_name(class);
    let level = net.level.unwrap_or(1).max(1);
    let text = |key: &str, fallback: &'static str| -> String { catalog.text(&format!("gui.window.{key}"), fallback).to_owned() };
    let mut view = Talents { ready: true, class, level, editable: true, ..default() };
    for (key, name, fallback) in [
        ("cost", "skilltalentswindow_talentUnlockCosts", "Cost: {0:i} Talent Points"),
        ("level_skill", "skilltalentswindow_experienceUnlockLevel", "Requires level: {0:i}"),
        ("level_wisdom", "skilltalentswindow_wisdomUnlockLevel", "Required Wisdom Level {0:i}"),
        ("level_honor", "skilltalentswindow_honorUnlockLevel", "Required Honor Level {0:i}"),
        ("upgrade", "skilltalentswindow_skillUnlockLevel", "Requires level for upgrade: {0:i}"),
        ("unlock", "talentwindow_grindTreeHowToUnlockTooltip", "(Left-click to unlock)"),
        ("level_up", "skilltalentswindow_howToLevelSkillTalents", "(Left-click to level up)"),
        ("toggle", "skilltalentswindow_howToUseActivateTalent", "(Left-click to activate/deactivate)"),
        ("focus", "skilltalentswindow_howToUseActivateFocusTalent", "(Left-click to add points/right-click to delete)"),
        ("blocked", "skilltalentswindow_talentGroupBlocked", "You can only activate one talent of this group."),
        ("not_editable", "skilltalentswindow_talentModificationDisabled", "You cannot modify Talents in this mode."),
        ("headline", "skilltalentswindow_unstackwindowTalentModeHeadline", "Talent Level"),
        ("level_button", "skilltalentswindow_splitButtonTalentModeText", "Level Up"),
        ("respec_question", "general_confirmVCwidgetTalentRespecQuestion", "You must pay to reset your Talent Points! This will return {0:i:grantedTalentPoints} Talent Points to you. \\n Are you sure?"),
        ("unlock_question", "skilltalentswindow_unlockpreselectiondesc", "Activating the next one requires {0:i} andermants"),
    ] { view.texts.insert(key, text(name, fallback)); }
    view.messages = Messages {
        points: text("skilltalentswindow_notEnoughTalentPointsMessage", "Not enough Talent Points to unlock this Talent."),
        level: text("skilltalentswindow_unlockLevelMessage", "Your level is too low to unlock this Talent."),
        category: text("skilltalentswindow_maxCategoryLevelReached", "You reached the max. level of this category."),
        talent: text("skilltalentswindow_maxTalentLevelReached", "You reached the max. level of this talent."),
        skill: text("skilltalentswindow_maxSkillTalentLevelReached", "You reached the max. level of this skill."),
        group: text("skilltalentswindow_talentGroupBlocked", "You can only activate one talent of this group."),
        money: text("messages_skillTalentRespecNotEnoughCurrency", "You don't have enough money to reset your Talent Points."),
    };
    let snapshot = net.talents.as_ref().filter(|t| t.context == dsor_proto::commands::inventory::TalentSelectionInfo::ALL_CONTEXT);
    let picks = |tree: Tree| -> Vec<(String, u32)> {
        snapshot.and_then(|t| t.categories.get(tree as usize)).map(|c| c.picks.iter().map(|e| (e.id.clone(), e.rank as u32)).collect()).unwrap_or_default()
    };
    let preset = |tree: Tree| snapshot.and_then(|t| t.categories.get(tree as usize)).map_or(0, |c| c.preset);
    let rank = |picks: &[(String, u32)], id: &str| picks.iter().find(|(p, _)| p == id).map_or(0, |(_, r)| *r);
    let bonus = |id: &str| s.bonuses.and_then(|b| b.get(&id.to_lowercase())).copied().unwrap_or(0);
    // The player's effects: a preset's cooldown, the reset price's factor
    // (TalentRespecCost:$0,relative on talent_reset_costs_buff).
    let mut cooling: [Option<u32>; 4] = [None; 4];
    let mut factor = 1.0f32;
    for e in net.stat_effects.values() {
        if let Some(tree) = catalog.preset_effects.get(&e.index.to_string()) { cooling[*tree as usize] = Some(e.end); }
        if catalog.reset_effects.contains(&(e.index as u32)) {
            factor += e.tail.as_ref().and_then(|t| t.parameters.first()).copied().unwrap_or(0.0);
        }
    }
    let factor = factor.max(0.0);
    let wallet = super::super::currency::Wallet::of(net);
    view.gold = wallet.copper;
    view.andermant = wallet.andermant;
    // Presets: NumTalentSets of the membership (_Template_Privileges: none 2,
    // Premium 3, Deluxe 4); the Skill tree's 5th to 8th bought, one after the
    // other (UnlockPreselectionCost "10000;30000;50000;100000").
    let privilege: u8 = match net.inventory.membership { 2 => 4, 1 => 3, _ => 2 };
    view.unlocked_presets = net.skill_presets_unlocked.unwrap_or(0);
    let prices: Vec<u32> = catalog.globals.get("UnlockPreselectionCost").and_then(|v| v.as_str())
        .map(|s| s.split(';').filter_map(|p| p.trim().parse().ok()).collect()).unwrap_or_default();
    view.unlock_price = prices.get(view.unlocked_presets as usize).copied();

    for tree in Tree::ALL {
        let picked = picks(tree);
        let value = if tree == Tree::Skill { level } else { net.talent_points.map_or(0, |p| p.of(tree.index())) };
        let progress = catalog.progress(tree, value);
        let max_level = if tree == Tree::Skill { catalog.skill_levels.iter().map(|(l, _)| *l).max().unwrap_or(100) }
            else { catalog.levels.get(&tree).and_then(|l| l.iter().map(|(l, _, _)| *l).max()).unwrap_or(0) };
        let spent = catalog.spent(&picked);
        let available = progress.granted as i64 - spent as i64;
        let mut t = TreeView { level: progress.level, max_level, fraction: if max_level > 0 { (progress.level as f32 / max_level as f32).min(1.0) } else { 0.0 },
            available, spent, preset: preset(tree), ..default() };
        // A tree's tab opens at the lowest level among its talents of our class.
        let lowest = match tree {
            Tree::Skill => Some(0),
            Tree::Knowledge | Tree::Group => catalog.talents.iter().filter(|x| x.tree == tree && x.for_class(cls)).map(|x| x.unlock).min(),
            Tree::Fame => catalog.focus.iter().map(|x| x.unlock).chain(catalog.traits.iter().filter(|x| x.for_class(cls)).map(|x| x.unlock)).min(),
        };
        t.unlocked = lowest.is_some_and(|l| level >= l);
        let (points_key, points_fallback) = match tree {
            Tree::Skill => ("skilltalentswindow_xpAvailableTalentPoints", "Available Experience Talent Points: {0:i}"),
            Tree::Knowledge => ("skilltalentswindow_knowledgeAvailableTalentPoints", "Available Wisdom Talent Points: {0:i}"),
            Tree::Group => ("skilltalentswindow_groupAvailableTalentPoints", "Available Group Talent Points: {0:i}"),
            Tree::Fame => ("skilltalentswindow_fameAvailableTalentPoints", "Available Honor Talent Points: {0:i}"),
        };
        t.points_text = fill(&text(points_key, points_fallback), &[&available.max(0).to_string()]);
        t.points_tip = Tip::new(fill(&text("skilltalentswindow_xpSpentTalentPoints", "Spent points: {0:i}"), &[&spent.to_string()]));
        // The bar's card (0x140612278): the level and the XP, then what the next
        // level still needs; at the top, only that it is reached.
        let (bar_key, missing_key, bar_fallback, missing_fallback) = match tree {
            Tree::Skill => ("xpBarPoints", "xpBarMissingPoints", "Level - {0:i} | {1:i} Experience Points", "Experience Points needed for the next Talent Point: {0:s}"),
            Tree::Knowledge => ("knowledgeBarPoints", "knowledgeBarMissingPoints", "Wisdom Level - {0:i} | {1:i} Ancient Wisdom", "Ancient Wisdom needed for next Wisdom Level: {0:s}"),
            Tree::Group => ("groupBarPoints", "groupBarMissingPoints", "Group Level - {0:i} | {1:i} Group Experience", "Group experience needed for next Group level: {0:s}"),
            Tree::Fame => ("fameBarPoints", "fameBarMissingPoints", "Honor Level - {0:i} | {1:i} Honor Points", "Honor Points needed for next Talent Point: {0:s}"),
        };
        let (xp, next) = if tree == Tree::Skill { net.xp.map_or((0, None), |(total, _, ceiling)| (total, (progress.level < max_level).then_some(ceiling))) }
            else { (value, progress.next) };
        if progress.level >= max_level || next.is_none() {
            t.bar_tip = Tip::new(text("skilltalentswindow_barMaximumLevelReached", "Maximum level reached"));
        } else {
            t.bar_tip = Tip::new(fill(&text(&format!("skilltalentswindow_{bar_key}"), bar_fallback), &[&progress.level.to_string(), &xp.to_string()]));
            t.bar_tip.line(fill(&text(&format!("skilltalentswindow_{missing_key}"), missing_fallback), &[&next.unwrap_or(xp).saturating_sub(xp).to_string()]), BAR_COLOURS[tree as usize]);
        }
        let unlocked_key = match tree { Tree::Knowledge => "general_wisdomUnlockedInfo", Tree::Group => "general_groupUnlockedInfo", _ => "general_honorUnlockedInfo" };
        if !t.unlocked { t.tab_tip = Tip::new(text(unlocked_key, "")); }
        // Presets: the active one pressed; the others open, or waiting.
        let count = match tree { Tree::Skill => 8, Tree::Knowledge | Tree::Group => 4, Tree::Fame => 0 };
        let cooldown = catalog.global_f32(match tree { Tree::Skill => "SkillTalentCooldown", Tree::Knowledge => "WisdomTalentCooldown", _ => "GroupTalentCooldown" }).unwrap_or(300.0);
        t.cooldown_end = cooling[tree as usize];
        t.cooldown_text = text("skilltalentswindow_preSelectionActiveCooldown", "You must wait for {0:i} seconds until you are allowed to change your preset again or you must enter an urban area.");
        for i in 0..count as u8 {
            let open = i < privilege || (tree == Tree::Skill && (BOUGHT_PRESETS_FROM..BOUGHT_PRESETS_FROM + view.unlocked_presets as u8).contains(&i));
            let state = if i == t.preset { PresetState::Active }
                else if !open && tree == Tree::Skill && i >= BOUGHT_PRESETS_FROM { PresetState::Locked }
                else if !open { PresetState::Privilege }
                else if t.cooldown_end.is_some() { PresetState::Cooldown }
                else { PresetState::Open };
            let mut tip = Tip::new(text(&format!("skilltalentswindow_talentPreselection{}Desc", i + 1), "Preset"));
            match state {
                PresetState::Locked => tip.line(fill(&text("skilltalentswindow_talentPreselectionInactive_new", "Use {0:i} andermants to obtain the current position!"),
                    &[&view.unlock_price.unwrap_or(0).to_string()]), YELLOW),
                PresetState::Privilege => tip.line(text(if privilege < 3 { "skilltalentswindow_talentPreselectionInactive_Premium" } else { "skilltalentswindow_talentPreselectionInactive_Deluxe" }, ""), YELLOW),
                // Its seconds left are filled in when the card shows (card).
                PresetState::Cooldown => {}
                _ => tip.line(fill(&text("skilltalentswindow_preSelectionNoActiveCooldown", "Preset can be toggled every {0:i} seconds outside of urban areas."), &[&number(cooldown)]), WHITE),
            }
            t.presets.push(Preset { state, tip });
        }
        if tree == Tree::Skill {
            t.reset_cost = reset_cost(catalog, level, spent, factor);
            t.reset_tip = Tip::new(text("skilltalentswindow_talentpointsResetButtonDesc", "Reset points"));
            t.reset_tip.line(fill(&text("skilltalentswindow_respecCosts", "Reset costs: {0:i:respecCosts}"), &[&money(t.reset_cost)]), WHITE);
            t.reset_tip.line(fill(&text("skilltalentswindow_usedTalentPoints", "Used Talent Points: {0:i}"), &[&spent.to_string()]), WHITE);
        } else if tree == Tree::Fame {
            t.reset_tip = Tip::new(text("skilltalentswindow_famepointsResetButtonDesc", "Reset points"));
            t.reset_tip.line(fill(&text("skilltalentswindow_usedTalentPoints", "Used Talent Points: {0:i}"), &[&spent.to_string()]), WHITE);
        }
        view.trees.push(t);
    }

    let available = |tree: Tree| view.trees[tree as usize].available;
    let affordable_ranks = |t: &Talent, rank: u32, available: i64| -> u32 {
        let mut n = 0;
        let mut paid = 0i64;
        while rank + n < t.max {
            paid += t.rank_cost(rank + n + 1) as i64;
            if paid > available { break; }
            n += 1;
        }
        n
    };
    // Experience: the class's skills and skill talents.
    let skill_picks = picks(Tree::Skill);
    let mut skill_ids: Vec<&String> = catalog.skills.iter().filter(|(_, t)| t.class == cls).map(|(id, _)| id).collect();
    skill_ids.sort();
    for id in skill_ids {
        let t = &catalog.skills[id];
        let r = rank(&skill_picks, id);
        let is_skill = t.kind == "Skill";
        let locked = if is_skill { (level < t.learn).then_some(t.learn) } else { (level < t.unlock.max(FIRST_TALENT_LEVEL)).then_some(t.unlock.max(FIRST_TALENT_LEVEL)) };
        let blocked = !is_skill && !t.category.is_empty() && r == 0 && catalog.skills.iter()
            .any(|(other, o)| other != id && o.category == t.category && o.class == cls && rank(&skill_picks, other) > 0);
        let avail = available(Tree::Skill);
        let cost = t.rank_cost(r + 1);
        let mut e = Entry { id: id.clone(), tree: Tree::Skill, skill: if is_skill { t.skill.clone() } else { String::new() }, icon: t.icon.clone(),
            rank: r, bonus: bonus(id), max: t.max, locked, blocked, cost, affordable: avail >= cost as i64 && r < t.max,
            most: affordable_ranks(t, r, avail).min(t.max.saturating_sub(r)), on_bar: is_skill && net.bar.iter().flatten().any(|s| *s == t.skill), ..default() };
        e.tip = tip(&view, t, &e, level);
        view.skills.push(e);
    }
    // Wisdom and Group: each category's box, its talents of our class in table order.
    for tree in [Tree::Knowledge, Tree::Group] {
        let picked = picks(tree);
        let avail = available(tree);
        for c in catalog.categories.iter().filter(|c| c.tree == tree) {
            let talents: Vec<&Talent> = catalog.talents.iter().filter(|t| t.category == c.id && t.for_class(cls)).collect();
            let ranks: u32 = talents.iter().map(|t| rank(&picked, &t.id)).sum();
            let bonuses: u32 = talents.iter().map(|t| bonus(&t.id)).sum();
            let points: u32 = talents.iter().map(|t| t.cost_between(0, rank(&picked, &t.id))).sum();
            let full = ranks >= c.max;
            let mut branch = Branch { id: c.id.clone(), tree, column: c.column, title: fill(&c.title, &[&(ranks + bonuses).to_string(), &c.max.to_string()]),
                green: bonuses > 0, points, cost: reset_cost(catalog, level, points, factor), ..default() };
            branch.reset_tip = Tip::new(text("skilltalentswindow_usedTalentPointsHeadline", "Used Talent Points"));
            for t in &talents {
                let r = rank(&picked, &t.id);
                let unlock = t.unlock.max(FIRST_TALENT_LEVEL);
                let cost = t.rank_cost(r + 1);
                let mut e = Entry { id: t.id.clone(), tree, icon: t.icon.clone(), rank: r, bonus: bonus(&t.id), max: t.max, locked: (level < unlock).then_some(t.unlock), cost,
                    affordable: avail >= cost as i64 && r < t.max, full,
                    most: affordable_ranks(t, r, avail).min(t.max.saturating_sub(r)).min(c.max.saturating_sub(ranks)), ..default() };
                e.tip = tip(&view, t, &e, level);
                branch.reset_tip.line(fill(&text("skilltalentswindow_usedTalentPointsLine", "{0:s:talentid}: {0:i:usedTalentPoints}").replacen("{0:i", "{1:i", 1),
                    &[&t.name, &t.cost_between(0, r).to_string()]), WHITE);
                branch.talents.push(e);
            }
            branch.reset_tip.line(fill(&text("skilltalentswindow_respecCosts", "Reset costs: {0:i:respecCosts}"), &[&money(branch.cost)]), WHITE);
            view.branches.push(branch);
        }
    }
    // Honor: the focus talents (a point a rank, right click takes one back)
    // and the class's traits (on or off).
    let fame = picks(Tree::Fame);
    let fame_level = view.trees[Tree::Fame as usize].level;
    let avail = available(Tree::Fame);
    for t in &catalog.focus {
        let r = rank(&fame, &t.id);
        let mut e = Entry { id: t.id.clone(), tree: Tree::Fame, icon: t.icon.clone(), rank: r, bonus: bonus(&t.id), max: t.max, cost: 1, affordable: avail >= 1 && r < t.max,
            locked: (fame_level < t.unlock).then_some(t.unlock), ..default() };
        e.tip = tip(&view, t, &e, fame_level);
        view.focus.push(e);
    }
    for t in catalog.traits.iter().filter(|t| t.for_class(cls)) {
        let r = rank(&fame, &t.id);
        let mut e = Entry { id: t.id.clone(), tree: Tree::Fame, toggle: true, icon: t.icon.clone(), rank: r, max: 1, cost: t.unlock_cost,
            affordable: avail >= t.unlock_cost as i64, locked: (fame_level < t.unlock).then_some(t.unlock), ..default() };
        e.tip = tip(&view, t, &e, fame_level);
        view.traits.push(e);
    }
    // The portable shop and workbench: their Wisdom talents in the active preset.
    let wisdom = picks(Tree::Knowledge);
    // Bonus ranks count (HasUnlockedWisdomShopTalent 0x14035c1f0).
    view.shop = rank(&wisdom, SHOP_TALENT) + bonus(SHOP_TALENT) > 0;
    view.workbench = rank(&wisdom, WORKBENCH_TALENT) + bonus(WORKBENCH_TALENT) > 0;
    view
}

/// A talent's card (0x14060c554): its name with its ranks while partly bought,
/// its texts at its rank, the price, the requirement, how to pick it.
fn tip(view: &Talents, t: &Talent, e: &Entry, level: u32) -> Tip {
    let total = e.rank + e.bonus;
    let suffix = if total > 0 && total < e.max + e.bonus { if e.bonus > 0 { format!(" ({}+{})", e.rank, e.bonus) } else { format!(" ({})", e.rank) } } else { String::new() };
    let mut tip = Tip { title: format!("{}{suffix}", t.name), icon: t.icon.clone(), lines: Vec::new() };
    if !view.editable {
        tip.line(view.text("not_editable"), RED);
        return tip;
    }
    // A skill's card is the spell card (crate::ui::controls::tooltips): its
    // texts are there; these lines follow them.
    if t.kind != "Skill" {
        let at = e.rank.max(1);
        let description = if t.per_point > 0.0 {
            // Focus talents: the bonus a point, the bonus in total.
            fill(&t.description, &[&number(t.per_point * 100.0), &number(t.per_point * 100.0 * e.rank as f32)])
        } else { t.resolve(&t.description, at) };
        tip.line(description, WHITE);
        if e.rank > 0 && e.max > 1 && !t.description2.is_empty() { tip.line(t.resolve(&t.description2, e.rank), BLUE); }
    }
    if e.rank < e.max && !(e.toggle && e.rank > 0) {
        tip.line(fill(view.text("cost"), &[&e.cost.to_string()]), if !e.affordable && e.rank == 0 { RED } else { WHITE });
    }
    let unlock = if e.tree == Tree::Skill && t.kind != "Skill" { t.unlock.max(FIRST_TALENT_LEVEL) } else { t.unlock };
    if unlock > 0 {
        let key = match (e.tree, t.kind.as_str()) {
            (Tree::Skill, "Skill") => "upgrade",
            (Tree::Knowledge, _) => "level_wisdom",
            (Tree::Fame, _) => "level_honor",
            _ => "level_skill",
        };
        tip.line(fill(view.text(key), &[&unlock.to_string()]), if level < unlock { RED } else { WHITE });
    }
    let hint = if e.toggle { "toggle" } else if e.tree == Tree::Fame { "focus" } else if e.rank == 0 { "unlock" } else if e.rank < e.max { "level_up" } else { "" };
    if !hint.is_empty() { tip.line(view.text(hint), if hint == "level_up" { GREY } else { YELLOW }); }
    if e.blocked { tip.line(view.text("blocked"), RED); }
    tip
}

// -- the layout -----------------------------------------------------------------

/// The Experience grid's class part ("Class3").
fn class_part(id: &str) -> Option<u8> { id.split('/').find_map(|p| p.strip_prefix("Class").and_then(|n| n.parse().ok())) }

fn state_of(w: &Widget) -> Option<&str> { w.state.as_deref() }

fn set_colour(w: &mut Widget, colour: [f32; 4]) { if let Some(t) = &mut w.text { t.color = colour; } }

fn saturate(w: &mut Widget, saturation: f32) { if let Some(a) = &mut w.art { a.saturation = saturation; } }

fn alpha(w: &mut Widget, alpha: f32) { if let Some(a) = &mut w.art { for c in a.color.chunks_exact_mut(4) { c[3] = alpha; } } }

/// A talent button's faces (`button` its id): the icon on each state (a state
/// with an Icon child keeps its own art, the frame, and its Icon gets it),
/// greyscale until bought, full colour on hover when affordable, the pressed
/// face (its gold frame) once bought, the disabled one with the level it waits for.
/// EVIDENCE: 0x14036e514 (states and Saturation through Button::SetSaturation
///   0x14072cf28, which writes the five state labels, not their children).
fn faces(widgets: &mut [Widget], button: &str, e: &Entry, editable: bool, event: Option<String>) {
    let bought = e.rank + e.bonus > 0;
    let enabled = editable && e.locked.is_none() && !e.blocked;
    let with_icon: Vec<String> = widgets.iter().filter(|w| w.button.as_deref() == Some(button) && leaf(&w.id) == "Icon")
        .filter_map(|w| w.id.rsplit_once('/').map(|(p, _)| p.to_owned())).collect();
    for w in widgets.iter_mut() {
        if w.id == button {
            if let Some(event) = &event { w.event = event.clone(); w.spell = None; }
            w.enabled = enabled;
            w.hidden = false;
            continue;
        }
        if w.button.as_deref() != Some(button) { continue; }
        let state = state_of(w).unwrap_or("").to_owned();
        let own = leaf(&w.id) == state.as_str();
        let icon_here = (own && !with_icon.contains(&w.id)) || leaf(&w.id) == "Icon";
        if icon_here && !e.icon.is_empty() {
            w.art = Some(image_art(w.rect, e.icon.clone()));
            w.hidden = false;
        }
        if leaf(&w.id) == "UnlockInfo" {
            set_text(w, &e.locked.map(|l| l.to_string()).unwrap_or_default());
            set_colour(w, LOCKED);
        }
        let saturation = if state == "disabled" { 0.0 } else if bought { 1.0 } else if state == "mouseover" && e.affordable { 1.0 } else { 0.0 };
        saturate(w, saturation);
        if state == "disabled" && icon_here { alpha(w, 0.5); }
        if bought && enabled { selected(w); }
    }
}

fn counter(w: &mut Widget, e: &Entry, max_with_bonus: bool) {
    let max = if max_with_bonus { e.max + e.bonus } else { e.max };
    set_text(w, &format!("{}/{}", e.rank + e.bonus, max));
    w.hidden = false;
    if e.bonus > 0 { set_colour(w, GREEN); }
}

/// A preset button: its event, pressed when active, off when it cannot be used.
fn preset_button(widgets: &mut [Widget], button: &str, preset: &Preset, event: String) {
    for w in widgets.iter_mut() {
        if w.id == button {
            w.event = event.clone();
            w.enabled = matches!(preset.state, PresetState::Open | PresetState::Active);
        } else if w.button.as_deref() == Some(button) && preset.state == PresetState::Active {
            selected(w);
        }
    }
}

/// The window from its view (systems::prepare): the class's grid, each
/// tab's level, bar and points, the categories' boxes cloned into their
/// columns, the Honor talents and traits, the presets and resets.
pub(in crate::ui) fn prepare_window(source: &Layout, view: &Talents) -> Layout {
    let class = view.class;
    let mut widgets: Vec<Widget> = source.widgets.iter().filter(|w| {
        // Only our class's grid and backdrop; the templates are cloned below.
        if class_part(&w.id).is_some_and(|c| c != class) { return false; }
        if has_part(&w.id, "TemplateSkill") || has_part(&w.id, "TemplateTalent") || has_part(&w.id, "TemplateWisdomTalents") { return false; }
        // The Honor tab's preset buttons (it has one preset).
        if has_part(&w.id, "FameLabel") && w.id.contains("/OptionsLabel/PresetButton") { return false; }
        // BuildOption3: the mage's fourth column only.
        if has_part(&w.id, "BuildOption3") && class != 1 { return false; }
        true
    }).cloned().collect();
    if !view.ready { return Layout { window: source.window.clone(), widgets } }

    // Each tab's level, its bar (level / most), its points.
    for (i, tab) in TABS.iter().enumerate() {
        let Some(t) = view.trees.get(i) else { continue };
        for w in widgets.iter_mut().filter(|w| has_part(&w.id, tab)) {
            match leaf(&w.id) {
                "LevelLabel" => set_text(w, &t.level.to_string()),
                "AvailablePoints" => { set_text(w, &t.points_text); w.hidden = !view.editable; }
                "progress" if w.id.contains("/LevelBar/") => { w.hidden = false; clip_y(w, t.fraction); }
                _ => {}
            }
        }
    }
    // The tabs open at their level.
    for (i, name) in TAB_BUTTONS.iter().enumerate() {
        let unlocked = view.trees.get(i).is_some_and(|t| t.unlocked);
        for w in widgets.iter_mut().filter(|w| leaf(&w.id) == *name && w.kind.contains("Button")) { w.enabled = unlocked; }
    }

    // Experience: the grid's cells.
    // The cells are the class part's children: Spell<wire> (a skill: its
    // SlotButton carries the skill) and Talent<id>.
    let class_dir = format!("/Skillgrid/Class{class}/");
    let cells: Vec<(String, String)> = widgets.iter().filter_map(|w| {
        let at = w.id.find(&class_dir)? + class_dir.len();
        let rest = &w.id[at..];
        if let Some(id) = rest.strip_prefix("Talent").filter(|r| !r.contains('/')) { return Some((w.id.clone(), id.to_owned())); }
        let (cell, button) = rest.split_once('/')?;
        (cell.starts_with("Spell") && button == "SlotButton").then_some(())?;
        w.spell.as_ref().map(|s| (w.id[..at + cell.len()].to_owned(), format!("skill:{}", s.id)))
    }).collect();
    for (cell, key) in cells {
        let entry = match key.strip_prefix("skill:") { Some(skill) => view.skill(skill), None => view.entry(&key) };
        let Some(e) = entry else { continue };
        let prefix = format!("{cell}/");
        let span: Vec<usize> = widgets.iter().enumerate().filter(|(_, w)| w.id.starts_with(&prefix)).map(|(i, _)| i).collect();
        let (lo, hi) = (span.first().copied().unwrap_or(0), span.last().map_or(0, |i| i + 1));
        let part = &mut widgets[lo..hi];
        if e.skill.is_empty() {
            faces(part, &format!("{cell}/TalentButton"), e, view.editable, Some(format!("Sys:Talent:{}", e.id)));
        } else {
            // A skill: dragged to the bar, clicked to level up (the window's
            // Spell action); locked below its level, its face greyed.
            let button = format!("{cell}/SlotButton");
            for w in part.iter_mut() {
                if w.id == button { w.enabled = e.locked.is_none(); }
                if w.button.as_deref() == Some(button.as_str()) && state_of(w) == Some("disabled") {
                    if leaf(&w.id) == "SkillIcon" { saturate(w, 0.0); alpha(w, 0.5); }
                    if leaf(&w.id) == "UnlockInfo" { set_text(w, &e.locked.map(|l| l.to_string()).unwrap_or_default()); set_colour(w, LOCKED); }
                }
                // Its glow while it is on the action bar (Template_glow: renamed,
                // a Template part never shows).
                if w.id.contains("/Template_glow") {
                    w.id = w.id.split('/').map(|p| p.replacen("Template_glow", "BarGlow", 1)).collect::<Vec<_>>().join("/");
                    w.hidden = !e.on_bar;
                }
            }
        }
        for w in part.iter_mut().filter(|w| leaf(&w.id) == "UpgradeCounter") { counter(w, e, true); }
    }
    // Experience's presets, their locks and the reset.
    let skill_tree = &view.trees[Tree::Skill as usize];
    let options = "ExperienceLabel/artworkExtension/Inlay/OptionsLabel_new/";
    for (i, preset) in skill_tree.presets.iter().enumerate() {
        let button = widgets.iter().find(|w| w.id.contains(options) && leaf(&w.id) == format!("PresetButton{:02}", i + 1) && w.button.is_none()).map(|w| w.id.clone());
        if let Some(button) = button { preset_button(&mut widgets, &button, preset, format!("Sys:Preset:0:{i}")); }
        let lock = format!("Lock{:02}", i + 1);
        let locked = preset.state == PresetState::Locked;
        widgets.retain(|w| !(w.id.contains(options) && has_part(&w.id, &lock)) || locked);
        for w in widgets.iter_mut().filter(|w| w.id.contains(options) && has_part(&w.id, &lock)) {
            // Its one face (authored as its disabled state) shown while it waits.
            if leaf(&w.id) == lock { w.event = format!("Sys:UnlockPreset:{i}"); w.enabled = view.editable; }
            if state_of(w) == Some("disabled") { w.state = Some("normal".into()); }
        }
    }
    for w in widgets.iter_mut().filter(|w| w.id.contains(options) && leaf(&w.id) == "RespecButton") { w.event = "Sys:Reset:0".into(); w.enabled = view.editable; }

    // Wisdom and Group: presets, the categories' boxes.
    for (tree, tab, counter_name, third) in [(Tree::Knowledge, "WisdomLabel", "WisdomCounter", "TalentListCombatEffects"), (Tree::Group, "GroupLabel", "GroupCounter", "TalentListAura")] {
        let t = &view.trees[tree as usize];
        for (i, preset) in t.presets.iter().enumerate() {
            let button = widgets.iter().find(|w| has_part(&w.id, tab) && w.id.contains("/OptionsLabel/") && leaf(&w.id) == format!("PresetButton{:02}", i + 1) && w.button.is_none()).map(|w| w.id.clone());
            if let Some(button) = button { preset_button(&mut widgets, &button, preset, format!("Sys:Preset:{}:{i}", tree as u8)); }
        }
        let Some(template) = source.widgets.iter().find(|w| has_part(&w.id, tab) && leaf(&w.id) == "TemplateWisdomTalents") else { continue };
        let [tl, tt, _, tb] = template.rect;
        let inlay = template.id.rsplit_once('/').map_or("", |(p, _)| p).to_owned();
        let mut rows = [0u32; 3];
        for (c, branch) in view.branches.iter().enumerate().filter(|(_, b)| b.tree == tree) {
            let column = (branch.column.clamp(1, 3) - 1) as usize;
            let list_name = ["TalentListOffense", "TalentListDefense", third][column];
            let Some(list) = source.widgets.iter().find(|w| w.id == format!("{inlay}/{list_name}")) else { continue };
            let target = format!("{inlay}/{list_name}/Category{c}");
            let dy = list.rect[1] - tt + rows[column] as f32 * (tb - tt);
            rows[column] += 1;
            let mut part: Vec<Widget> = clone_at(source, &template.id, &target, list.rect[0] - tl, dy).collect();
            for w in part.iter_mut() {
                w.hidden = false;
                match leaf(&w.id) {
                    "category_title" => { set_text(w, &branch.title); if branch.green { set_colour(w, GREEN); } }
                    "prize_trait" => w.hidden = true,
                    "RespecButton" => { w.event = format!("Sys:ResetBranch:{}:{}", tree as u8, branch.id); w.enabled = view.editable; }
                    name => if let Some(j) = name.strip_prefix(counter_name).and_then(|n| n.parse::<usize>().ok()) {
                        match branch.talents.get(j) { Some(e) => counter(w, e, false), None => w.hidden = true }
                    }
                }
            }
            for j in 0..3 {
                let button = format!("{target}/Inlay/Knowledge_Talents/FocusTalent{j}");
                match branch.talents.get(j) {
                    Some(e) => faces(&mut part, &button, e, view.editable, Some(format!("Sys:Talent:{}", e.id))),
                    None => part.retain(|w| w.id != button && !w.id.starts_with(&format!("{button}/"))),
                }
            }
            widgets.extend(part);
        }
    }
    // The Wisdom tab's portable workbench and shop, the Honor tab's merchant.
    for w in widgets.iter_mut() {
        match leaf(&w.id) {
            "Button_Workbench" => { w.event = "Sys:Workbench".into(); w.enabled = view.workbench; }
            "Button_Shop" => { w.event = format!("Sys:TalentShop:{WISDOM_TRADER}"); w.enabled = view.shop; }
            "Button_pvpShop" => w.event = format!("Sys:TalentShop:{PVP_TRADER}"),
            _ => {}
        }
    }

    // Honor: the focus talents and the class's traits; their names stay hidden.
    let fame = "FameLabel/artworkExtension/Inlay/";
    for (i, e) in view.focus.iter().enumerate().take(4) {
        if let Some(button) = widgets.iter().find(|w| w.id.contains(fame) && leaf(&w.id) == format!("FocusTalent{i}")).map(|w| w.id.clone()) {
            faces(&mut widgets, &button, e, view.editable, Some(format!("Sys:Talent:{}", e.id)));
        }
        for w in widgets.iter_mut().filter(|w| w.id.contains(fame) && leaf(&w.id) == format!("counter_talent{i}")) {
            counter(w, e, false);
            set_colour(w, if e.bonus > 0 { GREEN } else if e.rank > 0 { FOCUS_ACTIVE } else { FOCUS_IDLE });
        }
    }
    for i in 0..8 {
        let e = view.traits.get(i);
        let button = widgets.iter().find(|w| w.id.contains(fame) && leaf(&w.id) == format!("Trait{i}")).map(|w| w.id.clone());
        match (e, button) {
            (Some(e), Some(button)) => {
                faces(&mut widgets, &button, e, view.editable, Some(format!("Sys:Talent:{}", e.id)));
                for w in widgets.iter_mut().filter(|w| w.id.contains(fame) && leaf(&w.id) == format!("prize_trait{i}")) { set_text(w, &e.cost.to_string()); }
            }
            (None, Some(button)) => {
                let prize = format!("prize_trait{i}");
                widgets.retain(|w| w.id != button && !w.id.starts_with(&format!("{button}/")) && leaf(&w.id) != prize);
            }
            _ => {}
        }
    }
    for w in widgets.iter_mut().filter(|w| w.id.contains(fame)) {
        let name = leaf(&w.id);
        if name.starts_with("name_talent") || name.starts_with("name_trait") { w.hidden = true; }
        if name.starts_with("counter_talent") && name[14..].parse::<usize>().is_ok_and(|i| i >= view.focus.len()) { w.hidden = true; }
        if name == "RespecButton" { w.event = "Sys:Reset:3".into(); w.enabled = view.editable; }
    }
    Layout { window: source.window.clone(), widgets }
}

/// The reset's confirmation: its question and price; nothing without one.
pub(in crate::ui) fn prepare_dialog(source: &Layout, ask: Option<&Ask>) -> Layout {
    let Some(ask) = ask else { return Layout { window: source.window.clone(), widgets: Vec::new() } };
    let mut layout = source.clone();
    for w in &mut layout.widgets {
        match leaf(&w.id) {
            "PackageTextLabel" => { set_text(w, &ask.question.replace("\\n", "\n")); if let Some(t) = &mut w.text { t.word_break = true; } }
            "PackagePriceLabel" => set_text(w, &ask.price),
            "PackageIconLabel" => w.hidden = true,
            _ => {}
        }
    }
    layout
}

/// The split dialog in its talent mode: "Talent Level", "Level Up".
pub(in crate::ui) fn prepare_split(source: &Layout, split: Option<&(String, u32)>, view: &Talents) -> Layout {
    let mut layout = source.clone();
    let Some((_, most)) = split else { return layout };
    for w in &mut layout.widgets {
        match leaf(&w.id) {
            "PackageDescriptionTitle" => set_text(w, view.text("headline")),
            // Beside the amount, the most ranks there are (the item's stack in its item mode).
            "TextLabel_Stack" => set_text(w, &most.to_string()),
            "TextLabel" if w.id.contains("/SplitButton/") => set_text(w, view.text("level_button")),
            _ => {}
        }
    }
    layout
}

/// The hover card of a talent window request (crate::ui::controls::tooltips);
/// `now`, the game tick, counts a preset cooldown down.
pub(in crate::ui) fn card(request: &super::Request, view: &Talents, now: Option<u32>) -> Option<Tip> {
    use super::Request;
    match request {
        Request::Talent(id) => view.entry(id).map(|e| e.tip.clone()),
        Request::Preset(tree, i) => Tree::from_index(*tree).and_then(|t| view.tree(t)).and_then(|t| {
            let p = t.presets.get(*i as usize)?;
            let mut tip = p.tip.clone();
            if p.state == PresetState::Cooldown { tip.line(fill(&t.cooldown_text, &[&cooldown_left(t.cooldown_end, now).to_string()]), RED); }
            Some(tip)
        }),
        Request::UnlockPreset(i) => view.tree(Tree::Skill).and_then(|t| t.presets.get(*i as usize)).map(|p| p.tip.clone()),
        Request::Reset(tree) => Tree::from_index(*tree).and_then(|t| view.tree(t)).map(|t| t.reset_tip.clone()),
        Request::ResetBranch(_, id) => view.branch(id).map(|b| b.reset_tip.clone()),
        _ => None,
    }
}

/// Whole seconds until a preset cooldown ends ((endTick - nowTick) * 0.04,
/// the last second reading 1); 0 when it never ends or the clock is unknown.
fn cooldown_left(end: Option<u32>, now: Option<u32>) -> u32 {
    match (end.filter(|e| *e != 0), now) {
        (Some(end), Some(now)) => (end.wrapping_sub(now) as i32).max(0).unsigned_abs().div_ceil(25),
        _ => 0,
    }
}

/// The card of a label: a tab's level bar, its points, a locked tab.
pub(in crate::ui) fn label_card(id: &str, view: &Talents) -> Option<Tip> {
    let tree = TABS.iter().position(|t| has_part(id, t))?;
    let t = view.trees.get(tree)?;
    if id.contains("/ProgressBackground") { return Some(t.bar_tip.clone()); }
    if id.contains("/AvailablePointsLabel") { return Some(t.points_tip.clone()); }
    None
}

/// A locked tab's card (its tree opens at a level).
pub(in crate::ui) fn tab_card(index: u8, view: &Talents) -> Option<Tip> {
    view.trees.get(index as usize).filter(|t| !t.unlocked && !t.tab_tip.title.is_empty()).map(|t| t.tab_tip.clone())
}

/// What a click asks for, as ClientTalentManager checks it before sending
/// (0x14035e5e4): the points, the level, the category's and the talent's
/// most, an exclusive group; else its message. Honor's focus talents and
/// traits always go (their messages are warnings, 0x14035e16c, 0x14035f7b8).
pub(in crate::ui) fn pick(view: &Talents, id: &str, ranks: u32) -> Result<dsor_proto::commands::ClientCommand, String> {
    use dsor_proto::commands::{inventory, ClientCommand};
    let e = view.entry(id).ok_or_else(String::new)?;
    let m = &view.messages;
    if !view.editable { return Err(String::new()); }
    if e.toggle && e.rank > 0 { return Ok(ClientCommand::DeselectTalent(inventory::DeselectTalent { talent: id.to_owned() })); }
    let select = ClientCommand::SelectTalent(inventory::SelectTalent { talent: id.to_owned(), ranks: ranks.max(1) });
    if e.tree == Tree::Fame { return Ok(select); }
    let available = view.tree(e.tree).map_or(0, |t| t.available);
    let price = if e.rank == 0 { e.cost } else {
        // The ranks' prices, from the next one on (ComputeUpgradeCosts).
        e.cost.saturating_mul(ranks.max(1))
    };
    if available < price as i64 { return Err(m.points.clone()); }
    if e.locked.is_some() { return Err(m.level.clone()); }
    if e.full { return Err(m.category.clone()); }
    if e.rank + ranks.max(1) > e.max { return Err(if e.skill.is_empty() { m.talent.clone() } else { m.skill.clone() }); }
    if e.blocked { return Err(m.group.clone()); }
    Ok(select)
}

/// A right click: a focus talent loses a point; another talent opens the
/// split dialog when more than one rank can be bought (Honor never).
pub(in crate::ui) fn more(view: &Talents, id: &str) -> Option<Result<dsor_proto::commands::ClientCommand, (String, u32)>> {
    use dsor_proto::commands::{inventory, ClientCommand};
    let e = view.entry(id)?;
    if !view.editable { return None; }
    if e.tree == Tree::Fame {
        return (!e.toggle && e.rank > 0).then(|| Ok(ClientCommand::DeselectTalent(inventory::DeselectTalent { talent: id.to_owned() })));
    }
    (e.most > 1 && e.locked.is_none() && !e.blocked && !e.full).then(|| Err((id.to_owned(), e.most)))
}

/// A reset button: Honor's at once and free; the Experience tree's and a
/// category's for their gold (ConfirmVC dialog 4), at once when free, refused
/// without the money (AskForTalentReset 0x140363b30).
pub(in crate::ui) fn reset(view: &Talents, tree: Tree, branch: Option<&str>) -> Result<Option<Ask>, Option<String>> {
    let Some(t) = view.tree(tree) else { return Err(None) };
    if !view.editable { return Err(None); }
    let (cost, points, action) = match (tree, branch) {
        (Tree::Fame, _) => (0, t.spent, Confirmed::Reset(3, t.preset)),
        (Tree::Skill, _) => (t.reset_cost, t.spent, Confirmed::Reset(0, t.preset)),
        (_, Some(id)) => {
            let b = view.branch(id).ok_or(None)?;
            (b.cost, b.points, Confirmed::ResetBranch(tree as u8, id.to_owned(), t.preset))
        }
        _ => return Err(None),
    };
    let ask = Ask { question: fill(view.text("respec_question"), &[&points.to_string()]), price: money(cost), action };
    if cost == 0 { return Ok(Some(Ask { question: String::new(), ..ask })); }
    if view.gold < cost { return Err(Some(view.messages.money.clone())); }
    Ok(Some(ask))
}

/// A locked Skill preset: its price in Andermant, confirmed first.
pub(in crate::ui) fn unlock(view: &Talents) -> Result<Ask, Option<String>> {
    let price = view.unlock_price.ok_or(None)?;
    if view.andermant < price as u64 { return Err(Some(format!("You need {price} Andermant to unlock the next preset."))); }
    Ok(Ask { question: fill(view.text("unlock_question"), &[&price.to_string()]), price: format!("{price} Andermant"), action: Confirmed::Unlock(view.unlocked_presets + 1) })
}

/// The command a confirmed dialog sends.
pub(in crate::ui) fn confirmed(action: &Confirmed) -> Option<dsor_proto::commands::ClientCommand> {
    use dsor_proto::commands::{inventory, ClientCommand};
    Some(match action {
        Confirmed::Reset(tree, preset) => ClientCommand::ResetTalents(inventory::ResetTalents { category: *tree, preselection: *preset }),
        Confirmed::ResetBranch(tree, branch, preset) => ClientCommand::ResetTalentCategory(inventory::ResetTalentCategory { category: *tree, branch: branch.clone(), preselection: *preset }),
        Confirmed::Unlock(count) => ClientCommand::UnlockSkillTalentPreselection(inventory::UnlockSkillTalentPreselection { unlocked: *count }),
        Confirmed::Nothing => return None,
    })
}

/// Carries out a request of the window (systems::send): the commands to send.
/// A refused pick says why in the chat, as the client's messages did.
pub(in crate::ui) fn request(request: super::Request, view: &Talents, state: &mut super::State, net: &mut crate::network::net::Net) -> Vec<dsor_proto::commands::ClientCommand> {
    use dsor_proto::commands::{inventory, ClientCommand};
    use super::Request;
    let mut out = Vec::new();
    let mut note = |message: String| if !message.is_empty() { net.social_notes.push(message); };
    match request {
        Request::Talent(id) => match pick(view, &id, 1) { Ok(c) => out.push(c), Err(m) => note(m) },
        Request::Skill(skill) => if let Some(e) = view.skill(&skill) {
            match pick(view, &e.id, 1) { Ok(c) => out.push(c), Err(m) => note(m) }
        },
        Request::TalentMore(id) => {
            let id = match id.strip_prefix("skill:") { Some(skill) => view.skill(skill).map(|e| e.id.clone()), None => Some(id) };
            match id.and_then(|id| more(view, &id)) {
                Some(Ok(c)) => out.push(c),
                Some(Err(split)) => state.talent_split = Some(split),
                None => {}
            }
        }
        Request::TalentLevels(id, ranks) => {
            state.talent_split = None;
            match pick(view, &id, ranks) { Ok(c) => out.push(c), Err(m) => note(m) }
        }
        // EnableTalentPreselectionCommand (281): u8 preset, u8 tree.
        Request::Preset(tree, preset) => {
            if view.tree(Tree::from_index(tree).unwrap_or_default()).and_then(|t| t.presets.get(preset as usize)).is_some_and(|p| p.state == PresetState::Open) {
                out.push(ClientCommand::TalentPreselection(inventory::TalentPreselection { preselection: preset, category: tree }));
            }
        }
        Request::UnlockPreset(_) => match unlock(view) { Ok(ask) => state.talent_dialog = Some(ask), Err(m) => note(m.unwrap_or_default()) },
        Request::Reset(tree) | Request::ResetBranch(tree, _) if Tree::from_index(tree).is_some() => {
            let branch = match &request { Request::ResetBranch(_, id) => Some(id.as_str()), _ => None };
            match reset(view, Tree::from_index(tree).unwrap_or_default(), branch) {
                Ok(Some(ask)) if ask.question.is_empty() => out.extend(confirmed(&ask.action)),
                Ok(Some(ask)) => state.talent_dialog = Some(ask),
                Ok(None) => {}
                Err(m) => note(m.unwrap_or_default()),
            }
        }
        Request::TalentAnswer(yes) => if let Some(ask) = state.talent_dialog.take() { if yes { out.extend(confirmed(&ask.action)); } },
        Request::Workbench => if view.workbench { state.talent_open = Some(super::WORKBENCH); },
        // The trader's offers, then its window (crate::ui::dialogue, a trader
        // without an NPC).
        Request::TalentShop(shop) => {
            out.push(ClientCommand::RequestOffer(inventory::RequestOffer { by_name: true, actor: 0, shop: shop.clone() }));
            state.talent_trader = Some(shop);
        }
        _ => {}
    }
    out
}

/// Shows the dialogs while they wait (State::talent_dialog, talent_split);
/// closed by their own buttons, Escape or the close button, they are dropped.
/// Also opens what the window asked for: the portable workbench, a trader.
pub(in crate::ui) fn dialogs(mut state: ResMut<super::State>, mut windows: Query<(&crate::ui::Window2018, &mut Visibility)>,
    mut dialogue: ResMut<crate::gameplay::dialogue::Dialogue>, mut shown: Local<[bool; 2]>) {
    if state.talent_open.is_some() {
        let name = state.talent_open.take();
        for (w, mut visibility) in &mut windows { if Some(w.0.as_str()) == name { visibility.set_if_neq(Visibility::Inherited); } }
    }
    if state.talent_trader.is_some() {
        if let Some(shop) = state.talent_trader.take() { dialogue.trade(shop); }
    }
    for (index, name) in [(0, DIALOG), (1, SPLIT)] {
        let wanted = if index == 0 { state.talent_dialog.is_some() } else { state.talent_split.is_some() };
        let Some((_, mut visibility)) = windows.iter_mut().find(|(w, _)| w.0 == name) else { continue };
        let visible = *visibility != Visibility::Hidden;
        if wanted && !shown[index] {
            visibility.set_if_neq(Visibility::Inherited);
            shown[index] = true;
        } else if shown[index] && (!visible || !wanted) {
            if visible { visibility.set_if_neq(Visibility::Hidden); }
            if index == 0 && state.talent_dialog.is_some() { state.talent_dialog = None; }
            if index == 1 && state.talent_split.is_some() { state.talent_split = None; }
            shown[index] = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_take_their_arguments_in_order() {
        assert_eq!(fill("Attack ({0:i}/{1:i})", &["80", "200"]), "Attack (80/200)");
        assert_eq!(fill("{0:s:talentid}: {1:i:usedTalentPoints}", &["Rising Power", "240"]), "Rising Power: 240");
        assert_eq!(fill("no {2:i} argument", &["a"]), "no {2:i} argument");
        assert_eq!(money(20_744_800), "2074 Gold 48 Silver");
        assert_eq!(money(0), "0 Copper");
        assert_eq!(number(1.0), "1");
        assert_eq!(number(2.5), "2.5");
    }

    fn catalog() -> TalentCatalog {
        serde_json::from_str(r#"{
            "skill_levels": [[1,1],[2,1],[3,1],[4,1],[5,1],[6,1],[7,1],[8,1],[9,1],[10,1],[11,1],[12,1],[13,1],[14,1],[15,1],[16,1],[17,1],[18,1],[19,1],[20,1],
                [21,1],[22,1],[23,1],[24,1],[25,1],[26,1],[27,1],[28,1],[29,1],[30,1],[31,1],[32,1],[33,1],[34,1],[35,1],[36,1],[37,1],[38,1],[39,1],[40,1]],
            "levels": {"Knowledge": [[0, 0, 9], [1, 100, 5], [2, 500, 5]], "Group": [[1, 0, 18]], "Fame": [[1, 50, 1], [2, 150, 1], [3, 300, 1]]},
            "skills": {
                "dwarf_HeavyShot_talent": {"class": "dwarf", "tree": "Skill", "kind": "Skill", "skill": "HeavyShot", "max": 10, "unlock_cost": 1, "cost": 1, "unlock": 10, "learn": 9},
                "dwarf_talent_fire_master": {"class": "dwarf", "tree": "Skill", "kind": "Talent", "category": "dwarfTalentElemental", "max": 10, "unlock_cost": 5, "cost": 2, "unlock": 40, "name": "Master of Fire"},
                "dwarf_talent_poison_master": {"class": "dwarf", "tree": "Skill", "kind": "Talent", "category": "dwarfTalentElemental", "max": 10, "unlock_cost": 5, "cost": 2, "unlock": 40},
                "mage_talent_fire_master": {"class": "mage", "tree": "Skill", "kind": "Talent", "category": "mageTalentElemental", "max": 10, "unlock_cost": 5, "cost": 2, "unlock": 40}
            },
            "categories": [{"id": "trait_wisdom_attack", "tree": "Knowledge", "column": 1, "max": 200, "title": "Attack ({0:i}/{1:i})"},
                {"id": "trait_wisdom_combat", "tree": "Knowledge", "column": 2, "max": 1, "title": "Combat  ({0:i}/{1:i})"},
                {"id": "trait_group_aura", "tree": "Group", "column": 1, "max": 75, "title": "Group Aura  ({0:i}/{1:i})"}],
            "talents": [
                {"id": "trait_wisdom_all_damage", "tree": "Knowledge", "category": "trait_wisdom_attack", "max": 80, "unlock_cost": 3, "cost": 3, "unlock": 30},
                {"id": "trait_wisdom_dwarf_towerInRange_Trigger", "class": "dwarf", "tree": "Knowledge", "category": "trait_wisdom_combat", "max": 1, "unlock_cost": 300, "cost": 300, "unlock": 55},
                {"id": "trait_wisdom_mage_hitCritical_Trigger", "class": "mage", "tree": "Knowledge", "category": "trait_wisdom_combat", "max": 1, "unlock_cost": 300, "cost": 300, "unlock": 55},
                {"id": "trait_groupaura_dwarf_lightning_resistance", "class": "dwarf", "tree": "Group", "category": "trait_group_aura", "max": 50, "unlock_cost": 3, "cost": 3, "unlock": 20}],
            "traits": [{"id": "dwarf_fame_trait_01_nova", "class": "dwarf", "tree": "Fame", "max": 1, "unlock_cost": 10, "cost": 10, "unlock": 40},
                {"id": "dwarf_fame_trait_04_supportaura", "class": "dwarf", "tree": "Fame", "max": 1, "unlock_cost": 10, "cost": 10, "unlock": 2}],
            "focus": [{"id": "fame_focus_talent_01_honorbonus", "tree": "Fame", "max": 25, "unlock_cost": 1, "cost": 1, "unlock": 1, "per_point": 0.01,
                "description": "Each point: {0:s}%. Total: {1:s}%."}],
            "globals": {"VCRespecCostsPerPoint": 1, "UnlockPreselectionCost": "10000;30000;50000;100000", "SkillTalentCooldown": 300.0},
            "respec": [400.0],
            "preset_effects": {"5930": "Skill"},
            "texts": {}
        }"#).unwrap()
    }

    fn picks(entries: &[(&str, u16)]) -> Vec<dsor_proto::commands::inventory::TalentEntry> {
        entries.iter().map(|(id, rank)| dsor_proto::commands::inventory::TalentEntry { rank: *rank, bonus: 0, id: (*id).into() }).collect()
    }

    /// A dwarf at level 40, Wisdom level 1 (100 XP), Honor level 3.
    fn net(world: &mut World) {
        use dsor_proto::commands::inventory::{TalentCategory, TalentCategoryPointsInfo, TalentSelectionInfo};
        world.init_resource::<Time<Real>>();
        crate::network::net::start_offline(world, 3, 0);
        let mut net = world.non_send_mut::<crate::network::net::Net>();
        net.level = Some(40);
        // Wire order: Skill, Fame, Knowledge, Group.
        net.talent_points = Some(TalentCategoryPointsInfo { category: 0xFF, points: [0, 300, 100, 0] });
        net.talents = Some(TalentSelectionInfo { context: 0, categories: vec![
            TalentCategory { preset: 0, picks: picks(&[("dwarf_HeavyShot_talent", 3), ("dwarf_talent_fire_master", 2)]), branches: Vec::new() },
            TalentCategory { preset: 1, picks: picks(&[("trait_wisdom_all_damage", 4)]), branches: Vec::new() },
            TalentCategory::default(),
            TalentCategory { preset: 0, picks: picks(&[("fame_focus_talent_01_honorbonus", 3)]), branches: Vec::new() },
        ], ..Default::default() });
        net.wallet = Some([0, 50_000, 0, 0, 0]);
    }

    fn view(world: &World, catalog: &TalentCatalog) -> Talents {
        let net = world.non_send::<crate::network::net::Net>();
        let state = super::super::State::default();
        let systems = crate::gameplay::systems::SystemsCatalog::default();
        talents(&Sources { net: &net, catalog: &systems, items: None, names: None, state: &state, class: Some(3), talents: Some(catalog), bonuses: None })
    }

    #[test]
    fn the_view_follows_the_server_s_picks_and_the_catalog_s_rules() {
        let catalog = catalog();
        let mut world = World::new();
        net(&mut world);
        let v = view(&world, &catalog);
        // Experience: 40 points; HeavyShot 3 ranks, the fire master 5 + 2.
        let skill = v.tree(Tree::Skill).unwrap();
        assert_eq!((skill.level, skill.spent, skill.available), (40, 10, 30));
        assert_eq!(v.skills.len(), 3, "only our class's skill tree");
        let heavy = v.skill("HeavyShot").unwrap();
        assert_eq!((heavy.rank, heavy.max, heavy.locked, heavy.cost), (3, 10, None, 1));
        assert!(v.entry("dwarf_talent_poison_master").unwrap().blocked, "one master of the group has ranks");
        assert!(!v.entry("dwarf_talent_fire_master").unwrap().blocked);
        // Wisdom: level 1 (9 + 5 points), 4 ranks of 3 spent; Group locked below 20? open at 40.
        let wisdom = v.tree(Tree::Knowledge).unwrap();
        assert_eq!((wisdom.level, wisdom.spent, wisdom.available, wisdom.preset), (1, 12, 2, 1));
        assert!(wisdom.unlocked && v.tree(Tree::Group).unwrap().unlocked);
        let attack = v.branch("trait_wisdom_attack").unwrap();
        assert_eq!((attack.title.as_str(), attack.points), ("Attack (4/200)", 12));
        let combat = v.branch("trait_wisdom_combat").unwrap();
        assert_eq!(combat.talents.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["trait_wisdom_dwarf_towerInRange_Trigger"]);
        assert_eq!(combat.talents[0].locked, Some(55), "Wisdom talents wait for the character's level");
        // Honor: level 3, three focus points, a trait locked below Honor level 40.
        let fame = v.tree(Tree::Fame).unwrap();
        assert_eq!((fame.level, fame.available), (3, 0));
        assert_eq!(v.traits.iter().map(|e| e.locked).collect::<Vec<_>>(), [Some(40), None]);
        assert!(v.focus[0].tip.lines[0].0.contains("Each point: 1%. Total: 3%."));
        // Presets: the active one, the membership's two, the bought ones locked.
        let states: Vec<_> = skill.presets.iter().map(|p| p.state).collect();
        assert_eq!(states, [PresetState::Active, PresetState::Open, PresetState::Privilege, PresetState::Privilege,
            PresetState::Locked, PresetState::Locked, PresetState::Locked, PresetState::Locked]);
        assert_eq!(v.unlock_price, Some(10_000));
        // 400 copper a point (past the table's levels, its last multiplier).
        assert_eq!(skill.reset_cost, 4000);
    }

    #[test]
    fn clicks_are_checked_as_the_client_checks_them() {
        use dsor_proto::commands::ClientCommand;
        let catalog = catalog();
        let mut world = World::new();
        net(&mut world);
        let v = view(&world, &catalog);
        assert!(matches!(pick(&v, "dwarf_HeavyShot_talent", 1), Ok(ClientCommand::SelectTalent(s)) if s.ranks == 1));
        assert_eq!(pick(&v, "dwarf_talent_poison_master", 1).unwrap_err(), v.messages.group);
        assert_eq!(pick(&v, "trait_wisdom_dwarf_towerInRange_Trigger", 1).unwrap_err(), v.messages.points);
        // Honor's focus talents always go; a trait toggles.
        assert!(matches!(pick(&v, "fame_focus_talent_01_honorbonus", 1), Ok(ClientCommand::SelectTalent(_))));
        assert!(matches!(more(&v, "fame_focus_talent_01_honorbonus"), Some(Ok(ClientCommand::DeselectTalent(_)))));
        // A right click offers the ranks the points pay for.
        assert_eq!(more(&v, "dwarf_HeavyShot_talent"), Some(Err(("dwarf_HeavyShot_talent".into(), 7))));
        // Resets: the Experience tree's for gold (4000 copper; 50000 carried).
        let ask = reset(&v, Tree::Skill, None).unwrap().unwrap();
        assert_eq!((ask.price.as_str(), ask.action.clone()), ("40 Silver", Confirmed::Reset(0, 0)));
        assert!(matches!(confirmed(&ask.action), Some(ClientCommand::ResetTalents(r)) if r.category == 0));
        let honor = reset(&v, Tree::Fame, None).unwrap().unwrap();
        assert!(honor.question.is_empty(), "Honor resets at once, for nothing");
        let branch = reset(&v, Tree::Knowledge, Some("trait_wisdom_attack")).unwrap().unwrap();
        assert!(matches!(confirmed(&branch.action), Some(ClientCommand::ResetTalentCategory(r)) if r.category == 1 && r.branch == "trait_wisdom_attack" && r.preselection == 1));
        assert_eq!(unlock(&v).unwrap_err(), Some("You need 10000 Andermant to unlock the next preset.".into()));
    }

    #[test]
    fn a_preset_cooldown_holds_the_other_presets() {
        let catalog = catalog();
        let mut world = World::new();
        net(&mut world);
        // The server's effect: 300 s from tick 1000 (field 1 = its end tick).
        world.non_send_mut::<crate::network::net::Net>().stat_effects.insert(9, dsor_proto::commands::combat::StatusEffectElement { index: 5930, instance: 9, end: 8500, start: 1000, duration: 7500, ..Default::default() });
        let v = view(&world, &catalog);
        assert_eq!(v.tree(Tree::Skill).unwrap().presets[1].state, PresetState::Cooldown);
        assert_eq!(v.tree(Tree::Knowledge).unwrap().presets[0].state, PresetState::Open, "another tree's presets stay");
        // Its card counts the seconds left down, the last one reading 1.
        let left = |now: Option<u32>| card(&super::super::Request::Preset(0, 1), &v, now).unwrap().lines.last().unwrap().0.clone();
        assert!(left(Some(1000)).contains(" 300 "), "{}", left(Some(1000)));
        assert!(left(Some(8499)).contains(" 1 "), "{}", left(Some(8499)));
        assert!(left(Some(9000)).contains(" 0 "), "{}", left(Some(9000)));
        assert!(card(&super::super::Request::Preset(1, 1), &v, Some(1000)).unwrap().lines.iter().all(|(l, _)| !l.contains("wait")));
    }

    /// The window's view is built when it opens and when what it shows moves,
    /// never on idle frames; status effects count only when they are a preset's.
    #[test]
    fn the_view_is_rebuilt_only_when_what_it_shows_changes() {
        use crate::ui::{Layout, Loaded, Window2018};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<crate::gameplay::systems::SystemsCatalog>().init_asset::<super::super::items::Catalog>().init_asset::<Layout>()
            .init_asset::<crate::ui::local_map::MapNames>().init_asset::<TalentCatalog>()
            .init_resource::<super::super::State>().init_resource::<super::super::Views>().init_resource::<Loaded>().init_resource::<crate::ui::UiWork>()
            .add_systems(Update, super::super::sync);
        let handle = app.world_mut().resource_mut::<Assets<TalentCatalog>>().add(catalog());
        app.world_mut().resource_mut::<super::super::State>().talents = handle;
        net(app.world_mut());
        app.world_mut().spawn((Window2018(WINDOW.into()), Visibility::Inherited));
        let built = |app: &App| app.world().resource::<crate::ui::UiWork>().system_views;
        app.update();
        let first = built(&app);
        for _ in 0..10 { app.update(); }
        assert_eq!(built(&app), first, "idle frames rebuild nothing");
        let effect = |index| dsor_proto::commands::combat::StatusEffectElement { index, instance: index as u32, ..Default::default() };
        app.world_mut().non_send_mut::<crate::network::net::Net>().stat_effects.insert(1, effect(17));
        for _ in 0..3 { app.update(); }
        assert_eq!(built(&app), first, "an effect the window does not show moves nothing");
        app.world_mut().non_send_mut::<crate::network::net::Net>().stat_effects.insert(2, effect(5930));
        app.update();
        assert_eq!(built(&app), first + 1, "a preset's cooldown");
        app.world_mut().non_send_mut::<crate::network::net::Net>().bar[3] = Some("HeavyShot".into());
        app.update();
        assert_eq!(built(&app), first + 2, "the action bar's skills (their glow)");
        for _ in 0..5 { app.update(); }
        assert_eq!(built(&app), first + 2);
    }

    /// The real window and catalog (needs the converted assets): a dwarf at
    /// level 100 with the user's 2026-10-25 reference shots' Wisdom picks.
    /// `cargo test -p dsor-client the_real_window -- --ignored`
    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn the_real_window_fills_its_tabs_from_the_view() {
        use dsor_proto::commands::inventory::{TalentCategory, TalentCategoryPointsInfo, TalentSelectionInfo};
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface");
        let layout: Layout = serde_json::from_slice(&std::fs::read(dir.join("skilltalentswindow.ui.json")).unwrap()).unwrap();
        let catalog: TalentCatalog = serde_json::from_slice(&std::fs::read(dir.join("talents.catalog.json")).unwrap()).unwrap();
        let mut world = World::new();
        world.init_resource::<Time<Real>>();
        crate::network::net::start_offline(&mut world, 3, 0);
        {
            let mut net = world.non_send_mut::<crate::network::net::Net>();
            net.level = Some(100);
            net.talent_points = Some(TalentCategoryPointsInfo { category: 0xFF, points: [0, 3_750_000, 3_622_960, 1_924_643] });
            net.talents = Some(TalentSelectionInfo { context: 0, categories: vec![
                TalentCategory { preset: 0, picks: picks(&[("dwarf_HeavyShot_talent", 10), ("dwarf_talent_fire_master", 10)]), branches: Vec::new() },
                TalentCategory { preset: 1, picks: picks(&[("trait_wisdom_all_damage", 80), ("trait_wisdom_all_critical", 80), ("trait_wisdom_all_hpPerKill", 40),
                    ("trait_wisdom_dwarf_towerInRange_Trigger", 1), ("trait_wisdom_all_dungeon_shop", 1)]), branches: Vec::new() },
                TalentCategory::default(),
                TalentCategory { preset: 0, picks: picks(&[("fame_focus_talent_01_honorbonus", 25)]), branches: Vec::new() },
            ], ..Default::default() });
        }
        let v = view(&world, &catalog);
        let out = prepare_window(&layout, &v);
        let text = |suffix: &str| out.widgets.iter().filter(|w| w.id.ends_with(suffix)).filter_map(|w| w.text.as_ref().map(|t| t.text.clone())).collect::<Vec<_>>();
        assert!(!out.widgets.iter().any(|w| class_part(&w.id).is_some_and(|c| c != 3)), "only the dwarf's grid");
        assert!(!out.widgets.iter().any(|w| super::super::super::template::is_template(&w.id)), "templates are cloned, not drawn");
        // Wisdom: eight boxes in three columns, Group three.
        let boxes = |tab: &str| out.widgets.iter().filter(|w| has_part(&w.id, tab) && leaf(&w.id).starts_with("Category")).count();
        assert_eq!((boxes("WisdomLabel"), boxes("GroupLabel")), (8, 3));
        assert!(text("/category_title").contains(&"Attack (200/200)".to_owned()));
        assert!(text("/category_title").iter().any(|t| t.starts_with("Combat") && t.ends_with("(1/1)")));
        assert!(text("/WisdomCounter2").contains(&"40/80".to_owned()));
        // Experience: the counters, the masters' group.
        let cell = |id: &str, part: &str| out.widgets.iter().find(|w| w.id.ends_with(&format!("/Talent{id}/{part}"))).cloned();
        assert_eq!(cell("dwarf_talent_fire_master", "UpgradeCounter").and_then(|w| w.text).map(|t| t.text), Some("10/10".into()));
        assert!(!cell("dwarf_talent_poison_master", "TalentButton").unwrap().enabled, "a master's group is taken");
        assert_eq!(cell("dwarf_talent_fire_master", "TalentButton").map(|w| w.event), Some("Sys:Talent:dwarf_talent_fire_master".into()));
        // Honor: the focus counter, the class's four traits (Trait4-7 dropped).
        assert_eq!(text("/counter_talent0"), ["25/25"]);
        assert_eq!(out.widgets.iter().filter(|w| leaf(&w.id).starts_with("Trait") && w.kind == "Button").count(), 4);
        // The portable shop's talent is bought; the workbench's is not.
        let button = |name: &str| out.widgets.iter().find(|w| leaf(&w.id) == name).unwrap().clone();
        assert!(button("Button_Shop").enabled && !button("Button_Workbench").enabled);
        assert!(text("/AvailablePoints").iter().any(|t| t.starts_with("Available Wisdom Talent Points:")));
    }

    #[test]
    fn resets_cost_gold_by_level_and_points() {
        let catalog: TalentCatalog = serde_json::from_str(r#"{"respec": [400.0, 2760.0], "globals": {"VCRespecCostsPerPoint": 1}}"#).unwrap();
        assert_eq!(reset_cost(&catalog, 1, 10, 1.0), 4000);
        // Past 999999 copper, whole gold (10000 copper).
        assert_eq!(reset_cost(&catalog, 2, 1000, 1.0), 2_760_000);
        assert_eq!(reset_cost(&catalog, 2, 999, 1.0), 2_750_000);
        assert_eq!(reset_cost(&catalog, 2, 10, 0.5), 13_800);
        assert_eq!(reset_cost(&catalog, 99, 1, 1.0), 2760);
    }
}
