//! World entities and quests: monsters, NPCs, destroyables, ground items, props, the
//! quest log, chapters and quest guidance; and the client's requests about them.
//!
//! SOURCE: experimental (multiplayer-2018 HEAD) dsor/describe.py, dsor/items.py,
//!   dsor/inventory.py (_write_item, encode_item_update, pickup_reply_2018),
//!   dsor/mapinstance.py (quest log / update, chapters, props, decode_interaction,
//!   decode_quest_command), server.py (client command handlers). Reader addresses are
//!   the 2018 dro_client.exe ones those builders cite.

use dsor_raknet::{BitReader, BitWriter};

use super::wire::{counted, Body, ReadExt, WriteExt, MOST};
use super::DecodeError;

// ── shared pieces ───────────────────────────────────────────────────────────

/// A u16 size then that many bytes (sub_C4A6DB, the Guid/blob reader).
pub(super) fn read_blob(r: &mut BitReader<'_>) -> Result<Vec<u8>, DecodeError> {
    let n = r.u16()? as usize;
    Ok(r.read_bytes(n)?)
}

pub(super) fn write_blob(w: &mut BitWriter, b: &[u8]) {
    w.u16(b.len() as u16);
    w.write_bytes(b);
}

fn write_counted<T>(w: &mut BitWriter, items: &[T], mut f: impl FnMut(&mut BitWriter, &T)) {
    w.count(items.len());
    for i in items {
        f(w, i);
    }
}

/// An int8 the client range-checks (refused outside lo..=hi).
pub(super) fn read_ranged_i8(r: &mut BitReader<'_>, lo: i8, hi: i8, what: &'static str) -> Result<i8, DecodeError> {
    let v = r.i8()?;
    if v < lo || v > hi {
        return Err(DecodeError::Invalid { what, value: v as i64 });
    }
    Ok(v)
}

/// Game::GameFaction as sub_A4C4E8 reads it: u32 FactionId, u32, FactionType (8 bits,
/// sub_A645AE), two bits. Shared by NewMonster (+0x98), NewNPC (+96) and
/// NewDestroyable (+128).
/// EVIDENCE: dsor/describe.py encode_new_monster_2018 (stance test sub_995A1C compares
///   FactionId), _write_middle_zeros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Faction {
    pub faction_id: u32,
    /// A hostile monster's own carries its actor here.
    pub unknown_u32_0: u32,
    pub faction_type: u8,
    pub unknown_flag_0: bool,
    pub unknown_flag_1: bool,
}

/// How one faction stands toward another (Faction::stance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stance {
    None,
    Neutral,
    Ally,
    Hostile,
}

impl Faction {
    /// What `self` (the viewer) makes of `target`: only Hostile may be attacked
    /// (PlayerInputProperty::IsEnemy 0x14018d290 asks for it).
    /// EVIDENCE: dro_client64.exe (July 2026) stance test 0x14091b3b8, in order:
    ///   either FactionType -1: none; target type 1, or 2 toward 3: neutral; 3
    ///   toward 2, the same FactionId, or one side's second flag with the other's
    ///   type 0: ally; then hostile when both first flags are set, the types
    ///   differ or the second flags differ; else neutral.
    /// NOT DONE: the world's modes (ally in modes 5/6, hostile under flag 4) and the
    ///   target's Damage blocker, which the test also reads.
    pub fn stance(&self, target: &Faction) -> Stance {
        let (f, t) = (self.faction_type as i8, target.faction_type as i8);
        if f == -1 || t == -1 {
            Stance::None
        } else if t == 1 || (f == 2 && t == 3) {
            Stance::Neutral
        } else if (f == 3 && t == 2) || self.faction_id == target.faction_id
            || (self.unknown_flag_1 && t == 0) || (target.unknown_flag_1 && f == 0) {
            Stance::Ally
        } else if (self.unknown_flag_0 && target.unknown_flag_0) || f != t || self.unknown_flag_1 != target.unknown_flag_1 {
            Stance::Hostile
        } else {
            Stance::Neutral
        }
    }
}

impl Body for Faction {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            faction_id: r.u32()?,
            unknown_u32_0: r.u32()?,
            faction_type: r.u8()?,
            unknown_flag_0: r.bit()?,
            unknown_flag_1: r.bit()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.faction_id);
        w.u32(self.unknown_u32_0);
        w.u8(self.faction_type);
        w.bit(self.unknown_flag_0);
        w.bit(self.unknown_flag_1);
    }
}

// ── monsters ────────────────────────────────────────────────────────────────

/// The client refuses an attribute id at or above this (v < 0x39 at 0x8D8F52).
pub const MOST_ATTRIBUTES: u8 = 57;

/// The FactionIds the release261 server gives: the monsters', the players' and the
/// player-owned one of summons (type 0, as the players': not hostile to them) and
/// of fixed tower hazards (type 2: still enemies for Faction::stance, whatever
/// the server's comment says).
/// SOURCE: release261 server gameplay/local_monster_state.py (_load_monster:
///   0x00010004 when non_targetable, else 0x00010001, type 2;
///   load_player_summon_spawn: 0x00010004, type 0), protocol/gameplay_commands.py
///   (NewPlayer: 0x00010002, type 0, first flag set) and
///   services/presence_visuals.py _player_summon_group_for_viewer (a summon of a
///   PvP-hostile player goes to its viewer as 0x00010001 with type 2).
pub const MONSTER_FACTION: u32 = 0x0001_0001;
pub const PLAYER_FACTION: u32 = 0x0001_0002;
pub const PLAYER_OWNED_FACTION: u32 = 0x0001_0004;
/// FactionType of the monsters (and of a PvP enemy's summon).
pub const HOSTILE_FACTION_TYPE: u8 = 2;

/// NewMonsterCommand (release261: 42): a creature's description.
/// SOURCE: dso-server-production protocol/gameplay_protocol.py:2372-2394
///   (_write_new_monster_body). Against 2018: u64 current and u64 maximum health and
///   a u32 (0) before the level; a bool after the team id; no attribute id bound.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewMonster {
    /// The _Template_Monster blueprint.
    pub blueprint: String,
    /// (attribute id < 57, value): a u32 count (sub_999898: 0..=1000000), each u8 + f32.
    pub attributes: Vec<(u8, f32)>,
    pub health: u64,
    pub max_health: u64,
    /// production field_98, always 0 (meaning UNKNOWN).
    pub unknown_u32_98: u32,
    /// Copied to GameMonster+64, printed as the level by the info panel.
    pub level: u32,
    /// production field_a0: a summon's heading as it appears (its owner's yaw), 0
    /// for the others.
    pub heading: f32,
    /// World position (description frame), three float32.
    pub position: [f32; 3],
    /// production category (actor +0x134): 0 a monster, 1 a player's summon, 2 a
    /// map companion (an escort the map gives the player). 1 and 2 play their
    /// SummonSequence when fresh (see spawn_tick).
    pub category: u8,
    /// production field_c4 (actor +0x130): the server tick it appeared on, 0 for
    /// the map's monsters; sent again unchanged to whoever comes near it later.
    /// EVIDENCE: dro_client64.exe (July 2026) NewMonster handler 0x1401ff7b4;
    ///   ActorSequenceProperty::OnActivate 0x14012ea48 plays "spawn/" +
    ///   SummonSequence for categories 1 and 2 under 11 ticks after it.
    pub spawn_tick: u32,
    /// +0x98: the monster's faction, against the viewer's (Faction::stance).
    pub faction: Faction,
    /// +0xA8: the actor that summoned it, 0 for none (actor +0x164); the local
    /// player's own summons name it as actor 1. Its blows are the summoner's
    /// floating numbers (HitCommand's handler 0x1401c4050, July 2026 build).
    pub summoner: u32,
    pub unknown_flag_0: bool,
    /// The PvP team id, 0xFFFFFFFF for none.
    pub team: u32,
    /// production field_e4, always false (meaning UNKNOWN).
    pub unknown_flag_e4: bool,
    /// _Template_MonsterRanks id.
    pub rank: String,
    /// The rank's RMEType ("" or "guardian").
    pub rme_type: String,
}

impl Body for NewMonster {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let blueprint = r.string()?;
        let attributes = counted(r, MOST, "monster attribute count", |r| Ok((r.u8()?, r.f32()?)))?;
        Ok(Self {
            blueprint,
            attributes,
            health: r.u64()?,
            max_health: r.u64()?,
            unknown_u32_98: r.u32()?,
            level: r.u32()?,
            heading: r.f32()?,
            position: r.vec3()?,
            category: r.u8()?,
            spawn_tick: r.u32()?,
            faction: Faction::decode(r)?,
            summoner: r.u32()?,
            unknown_flag_0: r.bit()?,
            team: r.u32()?,
            unknown_flag_e4: r.bit()?,
            rank: r.string()?,
            rme_type: r.string()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.blueprint);
        write_counted(w, &self.attributes, |w, (id, v)| {
            w.u8(*id);
            w.f32(*v);
        });
        w.u64(self.health);
        w.u64(self.max_health);
        w.u32(self.unknown_u32_98);
        w.u32(self.level);
        w.f32(self.heading);
        w.vec3(self.position);
        w.u8(self.category);
        w.u32(self.spawn_tick);
        self.faction.encode(w);
        w.u32(self.summoner);
        w.bit(self.unknown_flag_0);
        w.u32(self.team);
        w.bit(self.unknown_flag_e4);
        w.string(&self.rank);
        w.string(&self.rme_type);
    }
}

/// MonsterUpdateCommand (release261: 44): what moves a monster's health bar --
/// u64 current health, u64 maximum, then three u32 production sends as 0, 0, 100
/// (meanings UNKNOWN; the first mirrors NewMonster's unknown_u32_98).
/// SOURCE: dso-server-production protocol/gameplay_protocol.py:1464-1486.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MonsterUpdate {
    pub health: u64,
    pub max_health: u64,
    pub unknown_u32_30: u32,
    pub unknown_u32_34: u32,
    pub unknown_u32_38: u32,
}

impl Body for MonsterUpdate {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { health: r.u64()?, max_health: r.u64()?, unknown_u32_30: r.u32()?, unknown_u32_34: r.u32()?, unknown_u32_38: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u64(self.health);
        w.u64(self.max_health);
        w.u32(self.unknown_u32_30);
        w.u32(self.unknown_u32_34);
        w.u32(self.unknown_u32_38);
    }
}

/// DebugMonsterInfoCommand (266): the client's debug panel for one monster.
/// SOURCE: dsor/describe.py encode_debug_monster_info; serializer 0x9AB061.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DebugMonsterInfo {
    /// Printed after "Skill: ".
    pub skill_text: String,
    /// "Attack Targets (sorted by Priority)".
    pub targets: Vec<u32>,
    pub priorities: Vec<(u32, u32)>,
}

impl Body for DebugMonsterInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            skill_text: r.string()?,
            targets: counted(r, MOST, "debug target count", |r| r.u32())?,
            priorities: counted(r, MOST, "debug priority count", |r| Ok((r.u32()?, r.u32()?)))?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.skill_text);
        write_counted(w, &self.targets, |w, t| w.u32(*t));
        write_counted(w, &self.priorities, |w, (a, b)| {
            w.u32(*a);
            w.u32(*b);
        });
    }
}

// ── NPCs and destroyables ───────────────────────────────────────────────────

/// NewNPCCommand (44): an NPC placed on the map.
/// SOURCE: dsor/describe.py encode_new_npc; reader 0x9A8455 (vtable 0xFEE428 slot 5):
///   string +16, guid blob +80 (sub_C4A6DB), faction +96 (sub_A4C4E8), f32 +112,
///   float3 +128 (sub_C4A80D), bit +144.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewNpc {
    /// The template name (the placed row's template).
    pub name: String,
    /// The level entity's Guid (16 bytes when present).
    pub guid: Vec<u8>,
    pub faction: Faction,
    pub unknown_f32_0: f32,
    pub position: [f32; 3],
    /// Visibility: false draws nothing.
    pub visible: bool,
}

impl Body for NewNpc {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            name: r.string()?,
            guid: read_blob(r)?,
            faction: Faction::decode(r)?,
            unknown_f32_0: r.f32()?,
            position: r.vec3()?,
            visible: r.bit()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.name);
        write_blob(w, &self.guid);
        self.faction.encode(w);
        w.f32(self.unknown_f32_0);
        w.vec3(self.position);
        w.bit(self.visible);
    }
}

/// NewDestroyableCommand (63): a destroyable map entity.
/// SOURCE: dsor/describe.py encode_new_destroyable; reader 0x9A81E5, writer 0x9AC213:
///   string +16, guid blob +80, f32 +96, float3 +112, faction +128, bit +144.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewDestroyable {
    pub name: String,
    pub guid: Vec<u8>,
    pub unknown_f32_0: f32,
    pub position: [f32; 3],
    pub faction: Faction,
    /// Visibility, as for NewNPC.
    pub visible: bool,
}

impl Body for NewDestroyable {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            name: r.string()?,
            guid: read_blob(r)?,
            unknown_f32_0: r.f32()?,
            position: r.vec3()?,
            faction: Faction::decode(r)?,
            visible: r.bit()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.name);
        write_blob(w, &self.guid);
        w.f32(self.unknown_f32_0);
        w.vec3(self.position);
        self.faction.encode(w);
        w.bit(self.visible);
    }
}

/// One Game::NpcInteractionInfo (sub_A5ABCC): string, kind byte (0 quest, 2 dialog),
/// string, state/quest-type byte.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NpcInteraction {
    /// The quest (kind 0) or dialog (kind 2) id.
    pub name: String,
    pub kind: u8,
    /// The quest id again (kind 0) or the dialog's quest (kind 2).
    pub quest: String,
    /// Kind 0: the quest's type code (QUEST_TYPE_2018), picking the marker colour.
    pub state: u8,
}

/// NPCInfoCommand (46): what an NPC offers.
/// SOURCE: dsor/describe.py encode_npc_info_2018; decoder 0x9A80F5 (vtable 0xFEE4A8):
///   string +16, four u32-counted lists +80 (sub_9980C6), +88 (sub_997DA6), +96
///   (sub_997C07), +104 (sub_9981D6), bit +112, byte -1..24 +116 (sub_9E12D8).
/// release261 string sets and links: production protocol/gameplay_protocol.py
/// _write_npc_info_body. These may be populated for quest deliveries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NpcInfo {
    pub name: String,
    /// The NPC's options (quests it gives, dialogs).
    pub interactions: Vec<NpcInteraction>,
    /// (dialog id, seen) pairs (sub_A5AEB8: string and one bit).
    pub dialogs: Vec<(String, bool)>,
    pub string_sets: Vec<NpcStringSet>,
    pub links: Vec<NpcLink>,
    /// The enabled state (feeds SetGameEntityVisible).
    pub enabled: bool,
    /// -1..24.
    pub kind: i8,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NpcStringSet {
    pub name: String,
    pub values: Vec<String>,
    pub amount: u16,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NpcLink {
    pub source: String,
    pub target: String,
    pub value: u32,
    pub enabled: bool,
}

impl Body for NpcInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let name = r.string()?;
        let interactions = counted(r, MOST, "npc interaction count", |r| {
            Ok(NpcInteraction { name: r.string()?, kind: r.u8()?, quest: r.string()?, state: r.u8()? })
        })?;
        let dialogs = counted(r, MOST, "npc dialog count", |r| Ok((r.string()?, r.bit()?)))?;
        let string_sets = counted(r, MOST, "npc string sets", |r| {
            let name = r.string()?;
            let count = r.u16()?;
            let values = (0..count).map(|_| r.string()).collect::<Result<_, _>>()?;
            Ok(NpcStringSet { name, values, amount: r.u16()?, enabled: r.bit()? })
        })?;
        let links = counted(r, MOST, "npc links", |r| {
            Ok(NpcLink { source: r.string()?, target: r.string()?, value: r.u32()?, enabled: r.bit()? })
        })?;
        let enabled = r.bit()?;
        let kind = read_ranged_i8(r, -1, 24, "npc info kind")?;
        Ok(Self { name, interactions, dialogs, string_sets, links, enabled, kind })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.name);
        write_counted(w, &self.interactions, |w, i| {
            w.string(&i.name);
            w.u8(i.kind);
            w.string(&i.quest);
            w.u8(i.state);
        });
        write_counted(w, &self.dialogs, |w, (d, seen)| {
            w.string(d);
            w.bit(*seen);
        });
        write_counted(w, &self.string_sets, |w, s| {
            w.string(&s.name);
            w.u16(s.values.len().try_into().expect("too many NPC string values"));
            for value in &s.values { w.string(value); }
            w.u16(s.amount);
            w.bit(s.enabled);
        });
        write_counted(w, &self.links, |w, link| {
            w.string(&link.source);
            w.string(&link.target);
            w.u32(link.value);
            w.bit(link.enabled);
        });
        w.bit(self.enabled);
        w.i8(self.kind);
    }
}

// ── items on the ground ─────────────────────────────────────────────────────

/// The item record NewItem carries is `inventory::ItemInfo` (reader 0x980AFC), the
/// same record InventoryInfo (87) and Offer (93) carry.
pub use super::inventory::{ItemInfo as ItemRecord, ItemStatistic, MOST_RARITIES};

/// NewItemCommand (release261: 45): an item lying on the ground. The body is one item
/// record (its position: ItemInfo.position, map coordinates; its rarity: `kind`).
/// SOURCE (release261): protocol/world_loot_protocol.py encode_new_item_batch.
/// SOURCE: dsor/items.py encode_new_item_2018; slot 5 0x9A8292 hands reader 0x980AFC
///   the item at command+16.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewItem {
    pub item: ItemRecord,
}

impl Body for NewItem {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { item: ItemRecord::decode(r)? })
    }
    fn encode(&self, w: &mut BitWriter) {
        self.item.encode(w)
    }
}

/// DiscardItemCommand (release261: 46): take an item off the ground -- the item,
/// then a bool (production's `notify`); the server writes actor 0 in the trailer.
/// SOURCE: dso-server-production protocol/world_loot_protocol.py encode_discard_item_batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiscardItem {
    pub item: u32,
    pub notify: bool,
}

impl Body for DiscardItem {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { item: r.u32()?, notify: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.item);
        w.bit(self.notify);
    }
}

/// ItemUpdateCommand (54): overwrite an owned item's fields (durability mostly).
/// SOURCE: dsor/inventory.py encode_item_update; reader 0x9A7CD4 (vtable 0xFEE6A8):
///   u32 item +16, seven 32-bit ints +20..+44, two bits +48/+49, two u32 +52/+56, two
///   lists (u32 count, strings) +60/+80. Handler HandleItemUpdateCommand 0x5354F8.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemUpdate {
    pub item: u32,
    /// +20 -> GameItem+48 (the server's Item.third).
    pub third: u32,
    /// +24 -> SetSockets (Item.tenth).
    pub tenth: u32,
    /// +28 -> +72 (Item.eleventh).
    pub eleventh: u32,
    /// +32 -> +68.
    pub unknown_u32_0: u32,
    /// +36 -> +116 (Item.twelfth).
    pub twelfth: u32,
    /// +40 -> SetBaseLevel.
    pub level: u32,
    pub flags: [bool; 2],
    /// +52 -> +108.
    pub twentieth: u32,
    /// +56 -> +104.
    pub twenty_eighth: u32,
    pub unknown_strings_0: Vec<String>,
    pub appearance: String,
    pub flag_2: bool,
}

impl Body for ItemUpdate {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            item: r.u32()?,
            third: r.u32()?,
            tenth: r.u32()?,
            eleventh: r.u32()?,
            unknown_u32_0: r.u32()?,
            twelfth: r.u32()?,
            level: r.u32()?,
            flags: [r.bit()?, r.bit()?],
            twentieth: r.u32()?,
            twenty_eighth: r.u32()?,
            unknown_strings_0: counted(r, MOST, "item update list", |r| r.string())?,
            appearance: r.string()?,
            flag_2: r.bit()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.item);
        w.u32(self.third);
        w.u32(self.tenth);
        w.u32(self.eleventh);
        w.u32(self.unknown_u32_0);
        w.u32(self.twelfth);
        w.u32(self.level);
        w.bit(self.flags[0]);
        w.bit(self.flags[1]);
        w.u32(self.twentieth);
        w.u32(self.twenty_eighth);
        write_counted(w, &self.unknown_strings_0, |w, s| w.string(s));
        w.string(&self.appearance);
        w.bit(self.flag_2);
    }
}

// ── props ───────────────────────────────────────────────────────────────────

/// One PropInfo entry (sub_A5A17A).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PropEntry {
    pub prop_id: u32,
    pub template: String,
    /// The level entity's Guid; a valid one links the entry to that entity.
    pub guid: Vec<u8>,
    /// Visual variant: -1 uses the state; 0..3 select blue/red/yellow/green.
    pub team: i32,
    /// -1..3: 0 inactive, 1 active, 2 cooldown (sub_A590ED).
    pub state: i8,
    pub unknown_u32_0: u32,
    pub position: [f32; 3],
    pub rotation: f32,
    /// Sent back in PropInteractionCommand when the prop is clicked.
    pub interaction: String,
    pub unknown_string_0: String,
    pub unknown_u8_0: u8,
    /// release261 quest indication, -1..13.
    pub quest_type: i8,
    /// release261 field_60, after the indication byte. -1 means no quest task.
    pub task_index: i32,
}

/// PropInfoCommand (54): announce or update map props (release261).
/// SOURCE: dso_server/protocol/gameplay_protocol.py::encode_prop_info.
/// Each release261 entry ends with field_60 (i32 task index).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PropInfo {
    pub entries: Vec<PropEntry>,
}

impl Body for PropInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let entries = counted(r, MOST, "prop count", |r| {
            Ok(PropEntry {
                prop_id: r.u32()?,
                template: r.string()?,
                guid: read_blob(r)?,
                team: r.i32()?,
                state: read_ranged_i8(r, -1, 3, "prop state")?,
                unknown_u32_0: r.u32()?,
                position: r.vec3()?,
                rotation: r.f32()?,
                interaction: r.string()?,
                unknown_string_0: r.string()?,
                unknown_u8_0: r.u8()?,
                quest_type: read_ranged_i8(r, -1, 13, "prop quest type")?,
                task_index: r.i32()?,
            })
        })?;
        Ok(Self { entries })
    }
    fn encode(&self, w: &mut BitWriter) {
        write_counted(w, &self.entries, |w, e| {
            w.u32(e.prop_id);
            w.string(&e.template);
            write_blob(w, &e.guid);
            w.i32(e.team);
            w.i8(e.state);
            w.u32(e.unknown_u32_0);
            w.vec3(e.position);
            w.f32(e.rotation);
            w.string(&e.interaction);
            w.string(&e.unknown_string_0);
            w.u8(e.unknown_u8_0);
            w.i8(e.quest_type);
            w.i32(e.task_index);
        });
    }
}

// ── quests and chapters ─────────────────────────────────────────────────────

/// One QuestLogInfo record.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestRecord {
    pub id: String,
    pub first: u32,
    pub state: u8,
    pub finished: u16,
    /// Per-task progress (a u32 count, 16 from this server, then that many u32).
    pub progress: Vec<u32>,
    pub last: u32,
    /// release261 trailing tracking id (production player_init_protocol.py).
    pub tracking_id: String,
}

/// QuestLogInfoCommand (149): the whole quest log.
/// SOURCE: dsor/mapinstance.py encode_quest_log_info (u32 count; per record string,
///   u32, u8 state, u16, u32 task count + u32 each, u32).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestLogInfo {
    pub records: Vec<QuestRecord>,
}

impl Body for QuestLogInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let records = counted(r, MOST, "quest log count", |r| {
            Ok(QuestRecord {
                id: r.string()?,
                first: r.u32()?,
                state: r.u8()?,
                finished: r.u16()?,
                progress: counted(r, MOST, "quest task count", |r| r.u32())?,
                last: r.u32()?,
                tracking_id: r.string()?,
            })
        })?;
        Ok(Self { records })
    }
    fn encode(&self, w: &mut BitWriter) {
        write_counted(w, &self.records, |w, q| {
            w.string(&q.id);
            w.u32(q.first);
            w.u8(q.state);
            w.u16(q.finished);
            write_counted(w, &q.progress, |w, v| w.u32(*v));
            w.u32(q.last);
            w.string(&q.tracking_id);
        });
    }
}

/// QuestUpdateCommand (148): one change to a quest.
/// SOURCE: dsor/mapinstance.py encode_quest_update_2018; decoder 0x9A98CB: int8 kind
///   -1..4 (sub_A5AA53), Dictionary<Param,int> (u32 count, int8 key -1..5 sub_A5AA22,
///   u32 value), Dictionary<Param,String> (same, 16-bit-length strings).
///   Kinds: 0 progress, 1 task finished, 2 quest state, 3 item provided, 4 clear region.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestUpdate {
    pub kind: i8,
    pub ints: Vec<(i8, u32)>,
    pub strings: Vec<(i8, String)>,
}

impl Body for QuestUpdate {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: read_ranged_i8(r, -1, 4, "quest update kind")?,
            ints: counted(r, MOST, "quest int params", |r| {
                Ok((read_ranged_i8(r, -1, 6, "quest param key")?, r.u32()?))
            })?,
            strings: counted(r, MOST, "quest string params", |r| {
                Ok((read_ranged_i8(r, -1, 6, "quest param key")?, r.string()?))
            })?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.i8(self.kind);
        write_counted(w, &self.ints, |w, (k, v)| {
            w.i8(*k);
            w.u32(*v);
        });
        write_counted(w, &self.strings, |w, (k, v)| {
            w.i8(*k);
            w.string(v);
        });
    }
}

/// ChapterUnlockedCommand (155): the "chapter unlocked" notice.
/// SOURCE: dsor/mapinstance.py encode_chapter_unlocked; reader sub_9A6A4D (one string).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChapterUnlocked {
    pub chapter_id: String,
}

impl Body for ChapterUnlocked {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { chapter_id: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.chapter_id)
    }
}

/// ChapterProgressCommand (156): chapter and act progress.
/// SOURCE: dsor/mapinstance.py encode_chapter_progress; reader sub_9A6A03: two lists
///   (u32 count 0..=1000000; string, u16, u16 each). Chapters are (id, total,
///   finished); acts are (id, finished, total).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChapterProgress {
    pub chapters: Vec<(String, u16, u16)>,
    pub acts: Vec<(String, u16, u16)>,
}

impl Body for ChapterProgress {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let entry = |r: &mut BitReader<'_>| Ok((r.string()?, r.u16()?, r.u16()?));
        Ok(Self {
            chapters: counted(r, MOST, "chapter count", entry)?,
            acts: counted(r, MOST, "act count", entry)?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        for entries in [&self.chapters, &self.acts] {
            write_counted(w, entries, |w, (n, a, b)| {
                w.string(n);
                w.u16(*a);
                w.u16(*b);
            });
        }
    }
}

/// One guidance point of QuestMonsterIndicationInfo: u32 id, string, float3.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MonsterMarker {
    pub id: u32,
    pub name: String,
    pub position: [f32; 3],
}

/// QuestMonsterIndicationInfoCommand (151): points the guidance trail runs through.
/// The quest and task echo the request, or the client drops the answer.
/// SOURCE: dsor/describe.py encode_quest_monster_indication; decoder sub_9A97A8 ->
///   sub_A5A61B; handler 0x54ECA2.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuestMonsterIndicationInfo {
    /// Read and never used by the handler.
    pub first: u32,
    pub quest: String,
    pub task: u32,
    pub markers: Vec<MonsterMarker>,
}

impl Body for QuestMonsterIndicationInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            first: r.u32()?,
            quest: r.string()?,
            task: r.u32()?,
            markers: counted(r, MOST, "monster marker count", |r| {
                Ok(MonsterMarker { id: r.u32()?, name: r.string()?, position: r.vec3()? })
            })?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.first);
        w.string(&self.quest);
        w.u32(self.task);
        write_counted(w, &self.markers, |w, m| {
            w.u32(m.id);
            w.string(&m.name);
            w.vec3(m.position);
        });
    }
}

/// QuestTriggerIndicationInfoCommand (153): the trigger points of the guided task.
/// SOURCE: dsor/describe.py encode_quest_trigger_indication; decoder sub_9A9822 ->
///   sub_A5A8FC (string, u32, then u32 count of {string, float3}); handler 0x54EDB5.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuestTriggerIndicationInfo {
    pub quest: String,
    pub task: u32,
    pub markers: Vec<(String, [f32; 3])>,
}

impl Body for QuestTriggerIndicationInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            quest: r.string()?,
            task: r.u32()?,
            markers: counted(r, MOST, "trigger marker count", |r| Ok((r.string()?, r.vec3()?)))?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.quest);
        w.u32(self.task);
        write_counted(w, &self.markers, |w, (n, p)| {
            w.string(n);
            w.vec3(*p);
        });
    }
}

// ── client -> server ────────────────────────────────────────────────────────

/// A client command whose whole body is one u32 actor/id.
macro_rules! actor_body {
    ($($(#[$m:meta])* $name:ident { $field:ident };)*) => {$(
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        pub struct $name {
            pub $field: u32,
        }
        impl Body for $name {
            fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
                Ok(Self { $field: r.u32()? })
            }
            fn encode(&self, w: &mut BitWriter) {
                w.u32(self.$field)
            }
        }
    )*};
}

actor_body! {
    /// NPCRequestCommand (47), client: the clicked NPC's actor; answered with NPCInfo.
    /// SOURCE: server.py _handle_npc_request (mapinstance.decode_interaction, no text);
    ///   sender sub_4E3D69. Captured: `8b 2f 00 | 13 01 01 00`.
    NpcRequest { actor };
    /// EncounterCommand (110), client: clicking an NPC hands in its quests.
    /// SOURCE: mapinstance.decode_interaction; serializer 0x9AB1B1 writes one 32-bit
    ///   field. Captured: `8b 6e 00 | 13 01 01 00`.
    Encounter { actor };
    /// PickupItemCommand (108), client: the item's actor id, four bytes and nothing else.
    /// SOURCE: server.py PICKUP_ITEM_OPCODE handler, World.pick_up.
    PickupItem { item };
    /// DebugMonsterRequestCommand (267), client: the monster whose debug panel to send.
    /// SOURCE: server.py _debug_monster (body[:4] is the actor).
    DebugMonsterRequest { actor };
}

/// A client command of a u32 actor/id then a string.
macro_rules! actor_text_body {
    ($($(#[$m:meta])* $name:ident { $field:ident, $text:ident };)*) => {$(
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, Default)]
        pub struct $name {
            pub $field: u32,
            pub $text: String,
        }
        impl Body for $name {
            fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
                Ok(Self { $field: r.u32()?, $text: r.string()? })
            }
            fn encode(&self, w: &mut BitWriter) {
                w.u32(self.$field);
                w.string(&self.$text);
            }
        }
    )*};
}

actor_text_body! {
    /// TalkCommand (111), client: an NPC's actor and the dialog/quest value picked.
    /// SOURCE: mapinstance.decode_interaction (has_text); serializer 0x9ADBD9.
    Talk { actor, value };
    /// DeliverCommand (112), client: same shape as Talk.
    /// SOURCE: server.py _handle_quest_interaction, mapinstance.decode_interaction.
    Deliver { actor, value };
    /// PayCommand (113), client: same shape as Talk; the value is the quest cost.
    /// SOURCE: server.py _handle_quest_interaction.
    Pay { actor, value };
    /// PropInteractionCommand (62), client: the prop id and its interaction string.
    /// SOURCE: server.py _handle_prop_interaction; writer 0x9AD1C8.
    PropInteraction { prop_id, interaction };
}

/// QuestCommand (release261: 138), client: accept (0), turn in (1), cancel (2).
/// SOURCE: mapinstance.decode_quest_command; serializer 0x9AD37E: string, two 8-bit
///   fields (sub_A5AA04, sub_999CD0).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Quest {
    pub quest_id: String,
    pub state: u8,
    pub parameter: u8,
}

impl Body for Quest {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { quest_id: r.string()?, state: r.u8()?, parameter: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.quest_id);
        w.u8(self.state);
        w.u8(self.parameter);
    }
}

/// QuestMonsterIndicationRequestCommand (150), client: the guided quest and task.
/// SOURCE: describe.decode_quest_monster_indication_request; decoder sub_9A97D8
///   (string, u32).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestMonsterIndicationRequest {
    pub quest: String,
    pub task: u32,
}

impl Body for QuestMonsterIndicationRequest {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { quest: r.string()?, task: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.quest);
        w.u32(self.task);
    }
}

/// QuestTriggerIndicationRequestCommand (152), client: the guided quest and task.
/// SOURCE: describe.decode_quest_trigger_indication_request; decoder sub_9A9852
///   (string, u32), sender sub_551490.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestTriggerIndicationRequest {
    pub quest: String,
    pub task: u32,
}

impl Body for QuestTriggerIndicationRequest {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { quest: r.string()?, task: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.quest);
        w.u32(self.task);
    }
}

/// QuestTriggerSignalCommand (299), client: one string (the trigger volume's event).
/// SOURCE: describe.decode_quest_trigger_signal; decoder sub_9A989C, sender sub_55134F.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestTriggerSignal {
    pub name: String,
}

impl Body for QuestTriggerSignal {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { name: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.name)
    }
}
