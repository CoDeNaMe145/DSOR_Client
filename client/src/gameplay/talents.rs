//! The talent trees behind the Skills & Talents window (interface/
//! talents.catalog.json, tools/talent_catalog.py): the skills and skill
//! talents (Experience), the Wisdom and Group talents in their categories,
//! the Honor focus talents and traits, the levels that grant their points.
//! What the server says the player has picked is in crate::network::net
//! (TalentSelectionInfo, TalentCategoryPointsInfo); the window is
//! crate::ui::controls::systems::talents.
//!
//! The rules are the server's (DSOR_Server protocol/talent_protocol.py
//! TalentDatabase, services/talents.py), which refuses a pick that breaks
//! them: a tree's points are what its levels granted, a rank costs its
//! talent's price for that rank, a Wisdom or Group category caps the ranks of
//! its talents together.
use bevy::{asset::{io::Reader, AssetLoader, LoadContext}, prelude::*};
use serde::Deserialize;
use std::collections::HashMap;

/// A talent tree: TalentSelectionInfo's category index, TalentCategory in the
/// database.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Default)]
pub enum Tree {
    #[default]
    Skill,
    Knowledge,
    Group,
    Fame,
}

impl Tree {
    pub const ALL: [Tree; 4] = [Tree::Skill, Tree::Knowledge, Tree::Group, Tree::Fame];
    pub fn index(self) -> u8 { self as u8 }
    pub fn from_index(index: u8) -> Option<Tree> { Self::ALL.get(index as usize).copied() }
}

/// A talent's value in its texts: at rank r, `base + step * (r - 1)`, or a
/// fixed `value`.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Token {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub abs: bool,
    #[serde(default)]
    pub base: Option<f32>,
    #[serde(default)]
    pub step: Option<f32>,
    #[serde(default)]
    pub value: Option<f32>,
}

impl Token {
    /// The value at a rank (rank 0 reads as rank 1: what buying gives).
    pub fn at(&self, rank: u32) -> Option<f32> {
        if let Some(v) = self.value { return Some(v); }
        Some(self.base? + self.step.unwrap_or(0.0) * rank.max(1).saturating_sub(1) as f32)
    }

    /// As the client writes a locale token (ValueFormat: Percentage scales by
    /// 100; the first digit is the decimals; IsAbsoluteBonus drops the sign).
    pub fn format(&self, value: f32) -> String {
        let value = if self.abs { value.abs() } else { value };
        let value = if self.format.starts_with("Percentage") { value * 100.0 } else { value };
        let decimals = self.format.chars().find(|c| c.is_ascii_digit()).and_then(|c| c.to_digit(10)).unwrap_or(0) as usize;
        format!("{value:.decimals$}")
    }
}

/// A talent of any tree.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Talent {
    #[serde(default)]
    pub id: String,
    /// Its class (warrior, mage, ranger, dwarf), empty for every class.
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub tree: Tree,
    /// Its category (CategoryTemplate), or a skill tree's exclusive group
    /// (GroupId: dwarfTalentElemental, the masters).
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default = "one")]
    pub max: u32,
    #[serde(default)]
    pub unlock_cost: u32,
    #[serde(default)]
    pub cost: u32,
    #[serde(default)]
    pub mult: f32,
    /// The level it unlocks at (the character's, or its tree's).
    #[serde(default)]
    pub unlock: u32,
    /// A skill's own level (_Template_Skill.UnlockLevel): it is learnt then and
    /// upgraded from `unlock`.
    #[serde(default)]
    pub learn: u32,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub description2: String,
    /// The skill a skill-tree entry levels up (SkillTemplateId).
    #[serde(default)]
    pub skill: String,
    #[serde(default)]
    pub effect: String,
    #[serde(default)]
    pub tokens: HashMap<String, Token>,
    /// A focus talent's bonus per point (its effect's first modifier).
    #[serde(default)]
    pub per_point: f32,
}

fn one() -> u32 { 1 }

impl Talent {
    /// What one rank costs (talent_protocol.py point_cost_for_rank).
    pub fn rank_cost(&self, rank: u32) -> u32 {
        if rank == 0 || rank > self.max { return 0; }
        if rank == 1 { return self.unlock_cost; }
        (self.cost as f32 * (1.0 + self.mult * (rank - 2) as f32)).round().max(0.0) as u32
    }

    /// What ranks `from + 1 ..= to` cost together.
    pub fn cost_between(&self, from: u32, to: u32) -> u32 {
        (from + 1..=to.min(self.max)).map(|r| self.rank_cost(r)).sum()
    }

    pub fn for_class(&self, class: &str) -> bool { self.class.is_empty() || self.class == class }

    /// The texts with their tokens' values at a rank; tokens without one
    /// stay for the card to dash.
    pub fn resolve(&self, text: &str, rank: u32) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let Some(end) = rest[start..].find('}') else { out.push_str(&rest[start..]); return out; };
            let inner = &rest[start + 1..start + end];
            let value = inner.split_once(":t:").and_then(|(_, name)| self.tokens.get(name))
                .and_then(|t| t.at(rank).map(|v| t.format(v)));
            match value {
                Some(v) => out.push_str(&v),
                None => out.push_str(&rest[start..=start + end]),
            }
            rest = &rest[start + end + 1..];
        }
        out.push_str(rest);
        out
    }
}

/// A Wisdom or Group category: its talents' ranks together reach `max`.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Category {
    pub id: String,
    pub tree: Tree,
    /// The window's column (1 to 3).
    #[serde(default)]
    pub column: u32,
    #[serde(default)]
    pub max: u32,
    /// Its title, "Attack ({0:i}/{1:i})": the ranks spent and the most.
    #[serde(default)]
    pub title: String,
}

#[derive(Asset, TypePath, Deserialize, Default, Debug)]
pub struct TalentCatalog {
    /// (level, points granted) of the character's levels.
    #[serde(default)]
    pub skill_levels: Vec<(u32, u32)>,
    /// (level, XP from, points granted) of the Knowledge, Group and Fame levels.
    #[serde(default)]
    pub levels: HashMap<Tree, Vec<(u32, u32, u32)>>,
    #[serde(default)]
    pub skills: HashMap<String, Talent>,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub talents: Vec<Talent>,
    /// The Honor traits: one rank each, bought with their cost.
    #[serde(default)]
    pub traits: Vec<Talent>,
    /// The Honor focus talents: a point a rank.
    #[serde(default)]
    pub focus: Vec<Talent>,
    #[serde(default)]
    pub globals: HashMap<String, serde_json::Value>,
    /// RespecVCCostMultiplier by character level - 1 (_Template_DynamicLevelCosts).
    #[serde(default)]
    pub respec: Vec<f32>,
    /// The status effects that hold a tree's presets (Groups SkillPreset,
    /// KnowledgePreset, GroupPreset), by wire index.
    #[serde(default)]
    pub preset_effects: HashMap<String, Tree>,
    /// The status effects that change reset prices (TalentRespecCost), by wire index.
    #[serde(default)]
    pub reset_effects: Vec<u32>,
    #[serde(default)]
    pub texts: HashMap<String, String>,
}

/// Where a tree stands: its level, the progress to the next one, the points
/// its levels granted.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
    pub level: u32,
    /// The XP (or the character's level for the Skill tree) it is at.
    pub value: u32,
    /// The XP the level starts at and the next one starts at (None at the top).
    pub floor: u32,
    pub next: Option<u32>,
    pub granted: u32,
}

impl Progress {
    /// How far into the level (0 to 1); 1 at the top level.
    pub fn fraction(&self) -> f32 {
        match self.next {
            Some(next) if next > self.floor => (self.value.saturating_sub(self.floor)) as f32 / (next - self.floor) as f32,
            Some(_) => 0.0,
            None => 1.0,
        }
    }
}

impl TalentCatalog {
    /// A gui.window text (talents.catalog.json's texts), else the fallback.
    pub fn text<'a>(&'a self, key: &str, fallback: &'a str) -> &'a str {
        self.texts.get(key).map(String::as_str).filter(|t| !t.is_empty()).unwrap_or(fallback)
    }

    pub fn global_f32(&self, name: &str) -> Option<f32> {
        self.globals.get(name).and_then(|v| v.as_f64()).map(|v| v as f32)
    }

    /// A talent of a tree by id (talent_protocol.py TalentDatabase.definition:
    /// skill tree, then the categories' talents, the traits, the focus talents).
    pub fn talent(&self, id: &str) -> Option<&Talent> {
        self.skills.get(id)
            .or_else(|| self.talents.iter().find(|t| t.id == id))
            .or_else(|| self.traits.iter().find(|t| t.id == id))
            .or_else(|| self.focus.iter().find(|t| t.id == id))
    }

    /// A tree's level and points: the Skill tree's from the character's level
    /// (_Template_TalentXPLevelSkill), the others' from their XP
    /// (_Template_TalentXPLevel: the highest level whose XP is reached).
    pub fn progress(&self, tree: Tree, value: u32) -> Progress {
        if tree == Tree::Skill {
            let granted = self.skill_levels.iter().filter(|(level, _)| *level <= value.max(1)).map(|(_, p)| p).sum();
            let top = self.skill_levels.iter().map(|(l, _)| *l).max().unwrap_or(0);
            return Progress { level: value, value, floor: value, next: (value < top).then_some(value + 1), granted };
        }
        let levels = self.levels.get(&tree).map(Vec::as_slice).unwrap_or_default();
        let reached = levels.iter().filter(|(_, xp, _)| *xp <= value);
        let (level, floor) = reached.clone().max_by_key(|(level, _, _)| *level).map(|(l, xp, _)| (*l, *xp)).unwrap_or((0, 0));
        let granted = reached.map(|(_, _, p)| p).sum();
        let next = levels.iter().filter(|(l, _, _)| *l > level).min_by_key(|(l, _, _)| *l).map(|(_, xp, _)| *xp);
        Progress { level, value, floor, next, granted }
    }

    /// The points a tree's selection spent.
    pub fn spent(&self, selection: &[(String, u32)]) -> u32 {
        selection.iter().filter_map(|(id, rank)| self.talent(id).map(|t| t.cost_between(0, *rank))).sum()
    }

    /// The ranks a category's talents hold together.
    pub fn category_ranks(&self, category: &str, selection: &[(String, u32)]) -> u32 {
        selection.iter().filter(|(id, _)| self.talents.iter().any(|t| t.id == *id && t.category == category)).map(|(_, r)| r).sum()
    }
}

#[derive(Default, TypePath)]
pub struct TalentCatalogLoader;
impl AssetLoader for TalentCatalogLoader {
    type Asset = TalentCatalog;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<TalentCatalog, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] { &["talents.catalog.json"] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> TalentCatalog {
        serde_json::from_str(r#"{
            "skill_levels": [[1, 1], [2, 1], [3, 1]],
            "levels": {"Fame": [[1, 50, 1], [2, 150, 1], [3, 300, 1]], "Knowledge": [[0, 0, 0], [1, 100, 5]]},
            "skills": {"dwarf_talent_fire_master": {"class": "dwarf", "tree": "Skill", "category": "dwarfTalentElemental",
                "max": 10, "unlock_cost": 5, "cost": 2, "mult": 0.0, "unlock": 40}},
            "categories": [{"id": "trait_wisdom_attack", "tree": "Knowledge", "column": 1, "max": 200, "title": "Attack ({0:i}/{1:i})"}],
            "talents": [{"id": "trait_wisdom_all_damage", "tree": "Knowledge", "category": "trait_wisdom_attack", "max": 80,
                "unlock_cost": 3, "cost": 3, "unlock": 30, "description": "+ {0:t:WT_all_damage} damage",
                "tokens": {"WT_all_damage": {"format": "Float0NoSign", "abs": true, "base": 75.0, "step": 75.0}}}],
            "traits": [{"id": "dwarf_fame_trait_01_nova", "class": "dwarf", "tree": "Fame", "cost": 10, "unlock": 40}],
            "focus": [{"id": "fame_focus_talent_01_honorbonus", "tree": "Fame", "max": 25, "unlock_cost": 1, "cost": 1, "per_point": 0.01}]
        }"#).unwrap()
    }

    #[test]
    fn ranks_cost_as_the_server_charges_them() {
        let c = catalog();
        let master = c.talent("dwarf_talent_fire_master").unwrap();
        // Rank 1 is the unlock cost, every further rank CostsPerLevel.
        assert_eq!((master.rank_cost(1), master.rank_cost(2), master.rank_cost(10), master.rank_cost(11)), (5, 2, 2, 0));
        assert_eq!(master.cost_between(0, 10), 5 + 9 * 2);
        let multiplied = Talent { max: 5, unlock_cost: 10, cost: 5, mult: 0.5, ..default() };
        assert_eq!((2..=5).map(|r| multiplied.rank_cost(r)).collect::<Vec<_>>(), [5, 8, 10, 13]);
        assert_eq!(c.spent(&[("dwarf_talent_fire_master".into(), 10), ("trait_wisdom_all_damage".into(), 2), ("unknown".into(), 3)]), 23 + 6);
    }

    #[test]
    fn levels_and_points_follow_the_xp_tables() {
        let c = catalog();
        let fame = c.progress(Tree::Fame, 200);
        assert_eq!((fame.level, fame.floor, fame.next, fame.granted), (2, 150, Some(300), 2));
        assert!((fame.fraction() - 50.0 / 150.0).abs() < 1e-6);
        let top = c.progress(Tree::Fame, 10_000);
        assert_eq!((top.level, top.next, top.granted, top.fraction()), (3, None, 3, 1.0));
        assert_eq!(c.progress(Tree::Fame, 10).granted, 0);
        assert_eq!(c.progress(Tree::Knowledge, 99).level, 0);
        assert_eq!(c.progress(Tree::Skill, 2).granted, 2);
    }

    #[test]
    fn talent_texts_show_their_values_at_a_rank() {
        let c = catalog();
        let t = c.talent("trait_wisdom_all_damage").unwrap();
        assert_eq!(t.resolve(&t.description, 80), "+ 6000 damage");
        assert_eq!(t.resolve(&t.description, 0), "+ 75 damage");
        assert_eq!(t.resolve("{0:t:unknown} and {1:s}", 3), "{0:t:unknown} and {1:s}");
        let percent = Token { format: "Percentage1NoSign".into(), abs: true, base: Some(0.055), step: Some(0.005), value: None };
        assert_eq!(percent.format(percent.at(50).unwrap()), "30.0");
        assert_eq!(c.category_ranks("trait_wisdom_attack", &[("trait_wisdom_all_damage".into(), 7)]), 7);
    }
}
