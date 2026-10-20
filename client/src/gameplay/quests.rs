//! Quests: what the server tells us of our log, and what the quest data adds.
//!
//! The server keeps the log: QuestLogInfo (140) with every map, then QuestUpdate
//! (139) as things happen -- kind 2 a state change, kinds 0 and 1 a task's
//! progress (the task index, its count and its target) and its completion.
//! States: 0 accepted, 1 finished (ready to turn in), 2 closed, 3 cancelled.
//! SOURCE: dso-server-production protocol/quest_protocol.py (:57-68, :118-138),
//!   services/quests.py _quest_publish_result.
//! The quest data (interface/quests.catalog.json, tools/quest_catalog.py from the
//! server's quests.xml) names each quest's tasks in index order: a task's text
//! is the dialogue catalog's `quest.<quest>.s_<task>`.
use bevy::{asset::{io::Reader, AssetLoader, LoadContext}, prelude::*};
use serde::Deserialize;
use std::collections::HashMap;

/// A quest the server lets us drop (QuestCommand operation 2): quests.xml deletable.
pub const CANCEL: u8 = 2;

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct TaskDef {
    pub title: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default = "one")]
    pub target: u32,
}
fn one() -> u32 { 1 }

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Rewards {
    #[serde(default)]
    pub xp: u32,
    /// Gold in copper.
    #[serde(default)]
    pub vc: u32,
    /// Andermant.
    #[serde(default)]
    pub rc: u32,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct RewardItem {
    pub item: String,
    #[serde(default = "one")]
    pub amount: u32,
    /// The class it is for; empty for every class.
    #[serde(default)]
    pub class: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct QuestDef {
    #[serde(default)]
    pub level: u32,
    #[serde(default)]
    pub map: String,
    #[serde(default)]
    pub chapter: String,
    #[serde(default)]
    pub deletable: bool,
    /// quests.xml type: world (the main story), local (side quests), event,
    /// heroic, crafting, pvp, the repeatable variants.
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub rewards: Rewards,
    #[serde(default)]
    pub items: Vec<RewardItem>,
    #[serde(default)]
    pub tasks: Vec<TaskDef>,
}

/// The quest data, by lower-case quest id (the server's ids keep their case;
/// the log and the NPC states compare them lower-cased).
#[derive(Asset, TypePath, Deserialize, Default)]
pub struct QuestCatalog {
    #[serde(deserialize_with = "lower_keys")]
    pub quests: HashMap<String, QuestDef>,
    /// Travel items by template: the map they lead to (_Template_Item.TravelItemTarget).
    #[serde(default)]
    pub travel_items: HashMap<String, String>,
}

fn lower_keys<'de, D: serde::Deserializer<'de>>(d: D) -> Result<HashMap<String, QuestDef>, D::Error> {
    let map: HashMap<String, QuestDef> = Deserialize::deserialize(d)?;
    Ok(map.into_iter().map(|(k, v)| (k.to_lowercase(), v)).collect())
}

impl QuestCatalog {
    pub fn quest(&self, id: &str) -> Option<&QuestDef> { self.quests.get(&id.to_lowercase()) }
}

#[derive(Default, TypePath)]
pub struct QuestCatalogLoader;
impl AssetLoader for QuestCatalogLoader {
    type Asset = QuestCatalog;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<QuestCatalog, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] { &["quests.catalog.json"] }
}

/// One quest of our log, as the server last described it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestEntry {
    pub state: u8,
    /// Bit n: task n is done.
    pub finished: u16,
    /// Per task, how far it is (QuestLogInfo's counters, then QuestUpdate kinds 0/1).
    pub progress: Vec<u32>,
    /// Per task, the target QuestUpdate named (the quest data's otherwise).
    pub targets: HashMap<usize, u32>,
}

impl QuestEntry {
    pub fn task_done(&self, index: usize) -> bool { self.finished & (1u16 << index.min(15)) != 0 }
    pub fn task_progress(&self, index: usize) -> u32 { self.progress.get(index).copied().unwrap_or(0) }
    pub fn active(&self) -> bool { self.state <= 1 }
    /// A task's progress record (kind 0 or 1): keys 1 index, 3 progress, 4 target.
    pub fn apply(&mut self, update: &dsor_proto::commands::actors::QuestUpdate) {
        let int = |key: i8| update.ints.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
        let Some(index) = int(1).map(|i| i as usize) else { return };
        if index >= 16 { return; }
        if self.progress.len() <= index { self.progress.resize(index + 1, 0); }
        if let Some(progress) = int(3) { self.progress[index] = progress; }
        if let Some(target) = int(4) { self.targets.insert(index, target); }
        if update.kind == 1 { self.finished |= 1 << index; }
    }
}

/// The quest windows' state (crate::ui::controls::quests).
#[derive(Resource, Default)]
pub struct QuestUi {
    pub catalog: Handle<QuestCatalog>,
    /// The quest the log shows in its overview.
    pub selected: Option<String>,
    /// Which active quest the tracker follows.
    pub tracked: usize,
    /// A cancel asked in the log, sent by crate::ui::controls::quests::sync.
    pub cancel: Option<String>,
    /// The quest groups folded in the log (quests.xml types).
    pub folded: std::collections::HashSet<String>,
    /// The tracker folded to its compact button.
    pub compact: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsor_proto::commands::actors::QuestUpdate;

    #[test]
    fn progress_updates_fill_tasks_and_finish_them() {
        let mut entry = QuestEntry { state: 0, ..default() };
        entry.apply(&QuestUpdate { kind: 0, ints: vec![(1, 2), (3, 4), (4, 10)], strings: vec![(0, "q".into())] });
        assert_eq!((entry.task_progress(2), entry.targets[&2], entry.task_done(2)), (4, 10, false));
        entry.apply(&QuestUpdate { kind: 1, ints: vec![(1, 2), (3, 10), (4, 10)], strings: vec![] });
        assert!(entry.task_done(2) && entry.task_progress(2) == 10 && entry.active());
        entry.apply(&QuestUpdate { kind: 0, ints: vec![(1, 40)], strings: vec![] });
        assert_eq!(entry.progress.len(), 3);
    }

    #[test]
    fn the_catalog_reads_the_exported_shape() {
        let catalog: QuestCatalog = serde_json::from_str(r#"{"quests":{"Q-One":{"level":3,"map":"m","deletable":true,"type":"local","rewards":{"xp":5,"vc":79},"items":[{"item":"i","class":"mage"}],"tasks":[{"title":"t","kind":"KillTask","target":10},{"title":"u"}]}}}"#).unwrap();
        let q = catalog.quest("q-one").unwrap();
        assert_eq!((q.level, q.deletable, q.tasks.len(), q.tasks[0].target, q.tasks[1].target), (3, true, 2, 10, 1));
        assert_eq!((q.kind.as_str(), q.rewards.xp, q.rewards.vc, q.rewards.rc, q.items[0].amount, q.items[0].class.as_str()), ("local", 5, 79, 0, 1, "mage"));
        assert!(catalog.quest("Q-ONE").is_some());
    }
}
