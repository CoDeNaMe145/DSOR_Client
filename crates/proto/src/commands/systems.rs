//! The systems the 2018 crate had no layouts for: salvage, crafting, transmog and
//! the stickerbook; events, dailies, the Wanted Thief, titles; PvP matches and the
//! rankings; the premium shop's OCP commands. Every layout follows the release 261
//! server's codecs (tools/systems_vectors.py checks them byte for byte).
//!
//! SOURCE: dso-server-production protocol/player_init_protocol.py (the info
//!   commands), gameplay_protocol.py (stickerbook, titles, OCP), inventory_economy_
//!   protocol.py (salvage, crafting), item_customization_protocol.py, daily_
//!   challenge_protocol.py, wanted_thief_protocol.py, ranking_protocol.py,
//!   pvp_match_protocol.py, gem_protocol.py, reward_protocol.py, services/events.py.
use dsor_raknet::{BitReader, BitWriter};

use super::inventory::ItemInfo;
use super::wire::{counted, Body, ReadExt, WriteExt, MOST};
use super::DecodeError;

fn u32s<const N: usize>(r: &mut BitReader<'_>) -> Result<[u32; N], DecodeError> {
    let mut out = [0; N];
    for v in &mut out {
        *v = r.u32()?;
    }
    Ok(out)
}

fn write_u32s(w: &mut BitWriter, values: &[u32]) {
    for v in values {
        w.u32(*v);
    }
}

/// A string the server writes only when it has one: a bit, then the string.
fn optional_string(r: &mut BitReader<'_>) -> Result<Option<String>, DecodeError> {
    Ok(if r.bit()? { Some(r.string()?) } else { None })
}

fn write_optional_string(w: &mut BitWriter, s: &Option<String>) {
    w.bit(s.is_some());
    if let Some(s) = s {
        w.string(s);
    }
}

/// A command with no body.
macro_rules! empty_body {
    ($($(#[$m:meta])* $name:ident;)*) => {$(
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        pub struct $name;
        impl Body for $name {
            fn decode(_: &mut BitReader<'_>) -> Result<Self, DecodeError> {
                Ok(Self)
            }
            fn encode(&self, _: &mut BitWriter) {}
        }
    )*};
}

empty_body! {
    /// ActiveEventsCommand (222), client -> server: ask for the events (the server
    /// answers EventUpdate, then EventProgressInfo and EventParticipationInfo).
    /// SOURCE: services/events.py:401-433 (the header only is checked).
    ActiveEvents;
    /// GetWantedThiefInfoCommand (359), client -> server: the board (360 answers).
    WantedThiefInfoRequest;
    /// PVP::PVPMatchCountdownCommand (195), NOT actor-scoped: the round's countdown.
    MatchCountdown;
}

/// OCPOfferRequestCommand (267), client -> server: ask for the premium shop's
/// offers and payment methods (OcpOfferResponse 270 and OcpMethods 268). One bool,
/// false from the original ("8b0b0100"); the server fails the request without it.
/// SOURCE: protocol/gameplay_protocol.py decode_simple_gameplay_command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OcpOfferRequest {
    pub unknown_bool_0: bool,
}

impl Body for OcpOfferRequest {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unknown_bool_0: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.bit(self.unknown_bool_0);
    }
}

// ── items: gems, salvage, crafting, transmog ────────────────────────────────

/// GemRemovalResponseCommand (86), actor-scoped: the gem taken out (its new item
/// id) and the one it replaced in the stack (0 when none).
/// SOURCE: protocol/gem_protocol.py encode_gem_removal_response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GemRemovalResponse {
    pub gem: u32,
    pub replaced: u32,
}

impl Body for GemRemovalResponse {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { gem: r.u32()?, replaced: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.gem);
        w.u32(self.replaced);
    }
}

/// SalvageCommand (49), client -> server: the items to melt (up to 128) and a
/// mode (0). The server answers with the inventory, the wallet (glyphs) and the
/// inventory again.
/// SOURCE: protocol/inventory_economy_protocol.py decode_salvage_request (:115-125).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Salvage {
    pub items: Vec<u32>,
    pub mode: u32,
}

impl Body for Salvage {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let items = counted(r, 128, "salvage items", |r| r.u32())?;
        let mode = if r.remaining() >= 32 { r.u32()? } else { 0 };
        Ok(Self { items, mode })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.items.len());
        write_u32s(w, &self.items);
        w.u32(self.mode);
    }
}

/// CraftingCommand (92), client -> server: u8, the ingredient items (up to 32), u8
/// payment currency (0 gold, 1 andermant), u8 source slot, u32 times, the recipe,
/// u32 mode (2 for a *_dismantle0 recipe), u32, u32.
/// SOURCE: protocol/inventory_economy_protocol.py decode_crafting_request (:128-168).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Crafting {
    pub unknown_u8_0: u8,
    pub items: Vec<u32>,
    pub currency: u8,
    pub slot: u8,
    pub amount: u32,
    pub recipe: String,
    pub mode: u32,
    pub unknown_u32_1: u32,
    pub unknown_u32_2: u32,
}

impl Body for Crafting {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            unknown_u8_0: r.u8()?,
            items: counted(r, 32, "crafting items", |r| r.u32())?,
            currency: r.u8()?,
            slot: r.u8()?,
            amount: r.u32()?,
            recipe: r.string()?,
            mode: r.u32()?,
            unknown_u32_1: r.u32()?,
            unknown_u32_2: r.u32()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.unknown_u8_0);
        w.count(self.items.len());
        write_u32s(w, &self.items);
        w.u8(self.currency);
        w.u8(self.slot);
        w.u32(self.amount);
        w.string(&self.recipe);
        w.u32(self.mode);
        w.u32(self.unknown_u32_1);
        w.u32(self.unknown_u32_2);
    }
}

/// CraftingResultCommand (93), actor-scoped: u8 status, the items made, u8 result
/// type (3), the (u32, u32) relations, u32.
/// SOURCE: protocol/player_init_protocol.py:201-213.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CraftingResult {
    pub status: u8,
    pub items: Vec<ItemInfo>,
    pub result_type: u8,
    pub relations: Vec<(u32, u32)>,
    pub unknown_u32_0: u32,
}

impl Body for CraftingResult {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            status: r.u8()?,
            items: counted(r, MOST, "crafting result items", ItemInfo::decode)?,
            result_type: r.u8()?,
            relations: counted(r, MOST, "crafting relations", |r| Ok((r.u32()?, r.u32()?)))?,
            unknown_u32_0: r.u32()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.status);
        w.count(self.items.len());
        for i in &self.items {
            i.encode(w);
        }
        w.u8(self.result_type);
        w.count(self.relations.len());
        for (a, b) in &self.relations {
            w.u32(*a);
            w.u32(*b);
        }
        w.u32(self.unknown_u32_0);
    }
}

/// RecipeBookInfoCommand (311), actor-scoped: the recipes known, each with a flag
/// and an optional string. Sent at login.
/// SOURCE: protocol/player_init_protocol.py:1055-1061.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecipeBookInfo {
    pub recipes: Vec<(String, Option<String>)>,
}

impl Body for RecipeBookInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { recipes: counted(r, MOST, "recipes", |r| Ok((r.string()?, optional_string(r)?)))? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.recipes.len());
        for (id, value) in &self.recipes {
            w.string(id);
            write_optional_string(w, value);
        }
    }
}

/// One equipment slot's transmog change: the slot type (0..6, 12 the cloak), then
/// the skin applied, the skin removed, the skin, the effect skin, the colour
/// applied, the colour removed (empty when unchanged).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CustomizationChange {
    pub slot: u32,
    pub apply_skin: String,
    pub remove_skin: String,
    pub skin: String,
    pub effect_skin: String,
    pub apply_color: String,
    pub remove_color: String,
}

/// ApplyItemCustomizationChangesCommand (322), client -> server: u32-counted slot
/// types, then the same count of changes, each a u32 6 and six strings. The server
/// answers with the wallet, the inventory and an ItemUpdate per changed item.
/// SOURCE: protocol/item_customization_protocol.py:34-53.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApplyItemCustomization {
    pub changes: Vec<CustomizationChange>,
}

impl Body for ApplyItemCustomization {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let slots = counted(r, 8, "customization slots", |r| r.u32())?;
        let n = r.count(8, "customization changes")?;
        if n != slots.len() {
            return Err(DecodeError::Invalid { what: "customization change count", value: n as i64 });
        }
        let mut changes = Vec::with_capacity(n);
        for slot in slots {
            let six = r.u32()?;
            if six != 6 {
                return Err(DecodeError::Invalid { what: "customization field count", value: six as i64 });
            }
            changes.push(CustomizationChange {
                slot,
                apply_skin: r.string()?,
                remove_skin: r.string()?,
                skin: r.string()?,
                effect_skin: r.string()?,
                apply_color: r.string()?,
                remove_color: r.string()?,
            });
        }
        Ok(Self { changes })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.changes.len());
        for c in &self.changes {
            w.u32(c.slot);
        }
        w.count(self.changes.len());
        for c in &self.changes {
            w.u32(6);
            for s in [&c.apply_skin, &c.remove_skin, &c.skin, &c.effect_skin, &c.apply_color, &c.remove_color] {
                w.string(s);
            }
        }
    }
}

/// ItemCustomizationInfoCommand (320), actor-scoped: the skins owned, each item
/// with a flag and an optional skin. Sent empty at login by this server.
/// SOURCE: protocol/player_init_protocol.py:974-982.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemCustomizationInfo {
    pub items: Vec<(String, Option<String>)>,
}

impl Body for ItemCustomizationInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { items: counted(r, MOST, "customizations", |r| Ok((r.string()?, optional_string(r)?)))? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.items.len());
        for (id, skin) in &self.items {
            w.string(id);
            write_optional_string(w, skin);
        }
    }
}

// ── the stickerbook (collector's album) ─────────────────────────────────────

/// One album entry: u32 state (2 owned, 3 a favourite), u32, the resource id.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StickerbookEntry {
    pub state: u32,
    pub unknown_u32_0: u32,
    pub id: String,
}

impl StickerbookEntry {
    pub const OWNED: u32 = 2;
    pub const FAVOURITE: u32 = 3;
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { state: r.u32()?, unknown_u32_0: r.u32()?, id: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.state);
        w.u32(self.unknown_u32_0);
        w.string(&self.id);
    }
}

/// StickerbookInfoCommand (303), actor-scoped: u32 (0 at login, 2 again once the
/// map is ready and after a favourite change), then the entries owned, each behind
/// a presence bit.
/// SOURCE: protocol/player_init_protocol.py:1001-1009, services/customization.py.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StickerbookInfo {
    pub kind: u32,
    pub entries: Vec<Option<StickerbookEntry>>,
}

impl Body for StickerbookInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: r.u32()?,
            entries: counted(r, MOST, "stickerbook entries", |r| Ok(if r.bit()? { Some(StickerbookEntry::decode(r)?) } else { None }))?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.kind);
        w.count(self.entries.len());
        for e in &self.entries {
            w.bit(e.is_some());
            if let Some(e) = e {
                e.encode(w);
            }
        }
    }
}

/// StickerbookItemInfoCommand (304), actor-scoped: one entry changed -- u32
/// operation (1 added, 3 upgraded), the entry behind a presence bit, then the
/// related resource id (the pet upgraded from; always written).
/// SOURCE: protocol/player_init_protocol.py:1030-1043.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StickerbookItemInfo {
    pub operation: u32,
    pub entry: Option<StickerbookEntry>,
    pub related: String,
}

impl Body for StickerbookItemInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let operation = r.u32()?;
        let entry = if r.bit()? { Some(StickerbookEntry::decode(r)?) } else { None };
        Ok(Self { operation, entry, related: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.operation);
        w.bit(self.entry.is_some());
        if let Some(e) = &self.entry {
            e.encode(w);
        }
        w.string(&self.related);
    }
}

/// UpdateStickerbookFavoriteCommand (305), client -> server: the entry and whether
/// it is a favourite (the album's star). A StickerbookInfo (kind 2) answers.
/// SOURCE: protocol/gameplay_protocol.py:1033-1047.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SetStickerbookFavorite {
    pub id: String,
    pub favorite: bool,
}

impl Body for SetStickerbookFavorite {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { id: r.string()?, favorite: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.id);
        w.bit(self.favorite);
    }
}

/// AddStickerbookFromInventoryCommand (307), client -> server: the inventory item
/// to add to the album (consumed; StickerbookItemInfo operation 1 answers).
/// SOURCE: protocol/gameplay_protocol.py:995-1011.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AddStickerbookFromInventory {
    pub item: u32,
}

impl Body for AddStickerbookFromInventory {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { item: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.item);
    }
}

/// UpgradeStickerbookItemCommand (308), client -> server: the pet to upgrade to
/// its next tier (costs currency_stickerbook_upgrade; GameCurrencyChanged and
/// StickerbookItemInfo operation 3 answer).
/// SOURCE: protocol/gameplay_protocol.py:1068-1078.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpgradeStickerbookItem {
    pub id: String,
}

impl Body for UpgradeStickerbookItem {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { id: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.id);
    }
}

// ── titles, events, dailies, the Wanted Thief ───────────────────────────────

/// SetTitleCommand (277), client -> server: u8 kind (0 none, 1 an achievement
/// title, 2 the PvP rank) and the achievement title. Nothing comes back; peers
/// see it in RemotePlayerInfo.
/// SOURCE: protocol/gameplay_protocol.py:787-806, services/presence_identity.py.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SetTitle {
    pub kind: u8,
    pub title: String,
}

impl Body for SetTitle {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { kind: r.u8()?, title: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.kind);
        w.string(&self.title);
    }
}

/// SetEventParticipationCommand (120), client -> server: take part in an active
/// event or not (EventParticipationInfo answers).
/// SOURCE: services/events.py:435-480.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SetEventParticipation {
    pub event: u32,
    pub participating: bool,
}

impl Body for SetEventParticipation {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { event: r.u32()?, participating: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.event);
        w.bit(self.participating);
    }
}

/// EventProgressInfoCommand (224), actor-scoped: per event its id and four values
/// (amount, level, currency level, group).
/// SOURCE: protocol/player_init_protocol.py:1570-1579, gameplay/event_system.py.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventProgressInfo {
    pub events: Vec<(u32, [u32; 4])>,
}

impl Body for EventProgressInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { events: counted(r, MOST, "event progress", |r| Ok((r.u32()?, u32s(r)?)))? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.events.len());
        for (id, values) in &self.events {
            w.u32(*id);
            write_u32s(w, values);
        }
    }
}

/// EventParticipationInfoCommand (223), actor-scoped: per event, taking part.
/// SOURCE: protocol/player_init_protocol.py:1593-1599.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventParticipationInfo {
    pub events: Vec<(u32, bool)>,
}

impl Body for EventParticipationInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { events: counted(r, MOST, "event participation", |r| Ok((r.u32()?, r.bit()?)))? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.events.len());
        for (id, on) in &self.events {
            w.u32(*id);
            w.bit(*on);
        }
    }
}

/// EventProgressChangedCommand (225), actor-scoped: event id, amount, level,
/// currency level, group.
/// SOURCE: protocol/player_init_protocol.py:822-828.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EventProgressChanged {
    pub values: [u32; 5],
}

impl Body for EventProgressChanged {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { values: u32s(r)? })
    }
    fn encode(&self, w: &mut BitWriter) {
        write_u32s(w, &self.values);
    }
}

/// One daily challenge: its id, its schedule id, u8 state (0 open, 2 taken, 3
/// done), u32 progress, a six-part time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimedChallenge {
    pub id: String,
    pub schedule: String,
    pub state: u8,
    pub progress: u32,
    pub timestamp: [u32; 6],
}

impl TimedChallenge {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { id: r.string()?, schedule: r.string()?, state: r.u8()?, progress: r.u32()?, timestamp: u32s(r)? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.id);
        w.string(&self.schedule);
        w.u8(self.state);
        w.u32(self.progress);
        write_u32s(w, &self.timestamp);
    }
}

/// TimedChallengesInfoCommand (131), actor-scoped: u64 server time, then the daily
/// board, the bonuses, the spender challenges and the comeback dailies, each
/// counted.
/// SOURCE: protocol/player_init_protocol.py _read_timed_challenges_info_body,
///   gameplay/daily_challenges.py. The fourth map is what the release261_192 client
///   reads (the server always writes it since 2026-10-21, empty so far).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimedChallengesInfo {
    pub server_time: u64,
    pub daily: Vec<TimedChallenge>,
    pub bonuses: Vec<TimedChallenge>,
    pub spender: Vec<TimedChallenge>,
    pub comeback: Vec<TimedChallenge>,
}

impl Body for TimedChallengesInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            server_time: r.u64()?,
            daily: counted(r, MOST, "daily challenges", TimedChallenge::decode)?,
            bonuses: counted(r, MOST, "bonus challenges", TimedChallenge::decode)?,
            spender: counted(r, MOST, "spender challenges", TimedChallenge::decode)?,
            comeback: counted(r, MOST, "comeback challenges", TimedChallenge::decode)?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u64(self.server_time);
        for list in [&self.daily, &self.bonuses, &self.spender, &self.comeback] {
            w.count(list.len());
            for c in list {
                c.encode(w);
            }
        }
    }
}

/// DailyChallengeProgressCommand (149), actor-scoped: one challenge's new state.
/// SOURCE: protocol/player_init_protocol.py:1363-1369.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DailyChallengeProgress {
    pub id: String,
    pub state: u8,
    pub progress: u32,
    pub timestamp: [u32; 6],
}

impl Body for DailyChallengeProgress {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { id: r.string()?, state: r.u8()?, progress: r.u32()?, timestamp: u32s(r)? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.id);
        w.u8(self.state);
        w.u32(self.progress);
        write_u32s(w, &self.timestamp);
    }
}

/// DailyChallengeSelectionCommand (148), client -> server: take a challenge, or
/// reroll it (refresh, for andermant). TimedChallengesInfo answers.
/// SOURCE: protocol/daily_challenge_protocol.py:9-31.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DailyChallengeSelection {
    pub id: String,
    pub refresh: bool,
}

impl Body for DailyChallengeSelection {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { id: r.string()?, refresh: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.id);
        w.bit(self.refresh);
    }
}

/// GetWantedThiefInfoResultCommand (360), actor-scoped: missions left today, the
/// special ones left, the four missions and their statuses (0 open, 1 taken), and
/// the VIP expiry (year, day, hour, minute, second, ms, month, weekday; zeros when
/// none).
/// SOURCE: protocol/wanted_thief_protocol.py:59-94.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WantedThiefInfo {
    pub remaining: u32,
    pub special_remaining: u32,
    pub missions: Vec<u32>,
    pub statuses: Vec<u32>,
    pub vip_expires: [u32; 8],
}

impl Body for WantedThiefInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            remaining: r.u32()?,
            special_remaining: r.u32()?,
            missions: counted(r, 64, "wanted missions", |r| r.u32())?,
            statuses: counted(r, 64, "wanted statuses", |r| r.u32())?,
            vip_expires: u32s(r)?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.remaining);
        w.u32(self.special_remaining);
        w.count(self.missions.len());
        write_u32s(w, &self.missions);
        w.count(self.statuses.len());
        write_u32s(w, &self.statuses);
        write_u32s(w, &self.vip_expires);
    }
}

/// AcceptWantedThiefMissionCommand (361) and RefreshWantedThiefMissionCommand
/// (362), client -> server: the board slot (1..3 to accept; 0 rerolls the whole
/// board, 1..4 one mission, for andermant). The board (360) answers.
/// SOURCE: protocol/wanted_thief_protocol.py:44-56, services/wanted_thief.py.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WantedThiefSlot {
    pub slot: u32,
}

impl Body for WantedThiefSlot {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { slot: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.slot);
    }
}

// ── the premium shop (OCP) ──────────────────────────────────────────────────

/// OCPBookingCommand (271), client -> server: buy an OCP offer (its product key)
/// with a payment method ("external" is the only one this server takes); a kind
/// 16 Notification follows, whose answer settles the booking.
/// SOURCE: protocol/reward_protocol.py:117-143, services/commerce.py:315-339.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OcpBooking {
    pub offer: String,
    pub method: String,
}

impl Body for OcpBooking {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { offer: r.string()?, method: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.offer);
        w.string(&self.method);
    }
}

/// AcceptedTierSpenderChallengesCommand (313), client -> server: the spender
/// rewards claimed, by id.
/// SOURCE: protocol/reward_protocol.py:74-82.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AcceptedTierSpender {
    pub rewards: Vec<String>,
}

impl Body for AcceptedTierSpender {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { rewards: counted(r, MOST, "spender rewards", |r| r.string())? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.rewards.len());
        for s in &self.rewards {
            w.string(s);
        }
    }
}

// ── the rankings ────────────────────────────────────────────────────────────

/// Ranking::HonorRankingInfoCommand (235), client -> server: one board's page.
/// u8 board (0..4, 4 the lifetime honour; 0xFF none), u32 start (0xFFFFFFFF: from
/// the top), u32 count (10 at most), u32 context (echoed), u16 flags (1 all, 2
/// friends, 4 guild, 0x10 the player's page), u8 class (0..3, 4 all), u8 sort
/// column (0 honour, 1 wins, 2 losses, 3 draws, 4 kills, 5 deaths).
/// SOURCE: protocol/ranking_protocol.py:15-92.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HonorRankingRequest {
    pub board: u8,
    pub start: u32,
    pub count: u32,
    pub context: u32,
    pub flags: u16,
    pub class: u8,
    pub sort: u8,
}

impl Body for HonorRankingRequest {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { board: r.u8()?, start: r.u32()?, count: r.u32()?, context: r.u32()?, flags: r.u16()?, class: r.u8()?, sort: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.board);
        w.u32(self.start);
        w.u32(self.count);
        w.u32(self.context);
        w.u16(self.flags);
        w.u8(self.class);
        w.u8(self.sort);
    }
}

/// Ranking::HonorOverallUserRankingInfoCommand (240), client -> server: the overall
/// board's page. u32 start, u32 count, u32 context, u16 flags, u8 class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OverallRankingRequest {
    pub start: u32,
    pub count: u32,
    pub context: u32,
    pub flags: u16,
    pub class: u8,
}

impl Body for OverallRankingRequest {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { start: r.u32()?, count: r.u32()?, context: r.u32()?, flags: r.u16()?, class: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.start);
        w.u32(self.count);
        w.u32(self.context);
        w.u16(self.flags);
        w.u8(self.class);
    }
}

/// Ranking::HonorLocalUserRankingInfoCommand (242), client -> server: our own
/// place on a board. u32 context, u8 board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalRankingRequest {
    pub context: u32,
    pub board: u8,
}

impl Body for LocalRankingRequest {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { context: r.u32()?, board: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.context);
        w.u8(self.board);
    }
}

/// Ranking::HonorOverallLocalUserRankingInfoCommand (244), client -> server: our
/// own overall place. u32 context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OverallLocalRankingRequest {
    pub context: u32,
}

impl Body for OverallLocalRankingRequest {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { context: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.context);
    }
}

/// One row of a board: the player, its 0-based rank, its score, wins, losses,
/// draws, kills, deaths.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RankingRow {
    pub name: String,
    pub rank: u32,
    pub score: u32,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub kills: u32,
    pub deaths: u32,
}

/// Ranking::HonorRankingResponseCommand (236), actor-scoped: a board's page --
/// u8 board, u32 total, u32 context, u8 status, the rows, u32 start, u8 class,
/// u8 sort column, u16 flags.
/// SOURCE: protocol/ranking_protocol.py encode_ranking_page (:102-137).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HonorRankingPage {
    pub board: u8,
    pub total: u32,
    pub context: u32,
    pub status: u8,
    pub rows: Vec<RankingRow>,
    pub start: u32,
    pub class: u8,
    pub sort: u8,
    pub flags: u16,
}

impl Body for HonorRankingPage {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            board: r.u8()?,
            total: r.u32()?,
            context: r.u32()?,
            status: r.u8()?,
            rows: counted(r, MOST, "ranking rows", |r| Ok(RankingRow {
                name: r.string()?, rank: r.u32()?, score: r.u32()?, wins: r.u32()?, losses: r.u32()?, draws: r.u32()?, kills: r.u32()?, deaths: r.u32()?,
            }))?,
            start: r.u32()?,
            class: r.u8()?,
            sort: r.u8()?,
            flags: r.u16()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.board);
        w.u32(self.total);
        w.u32(self.context);
        w.u8(self.status);
        w.count(self.rows.len());
        for row in &self.rows {
            w.string(&row.name);
            write_u32s(w, &[row.rank, row.score, row.wins, row.losses, row.draws, row.kills, row.deaths]);
        }
        w.u32(self.start);
        w.u8(self.class);
        w.u8(self.sort);
        w.u16(self.flags);
    }
}

/// Ranking::HonorOverallUserRankingResponseCommand (241), actor-scoped: the overall
/// board's page -- u32 total, u32 context, u8 status, rows of (name, score, rank),
/// u32 start, u16 flags, u8 class.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OverallRankingPage {
    pub total: u32,
    pub context: u32,
    pub status: u8,
    pub rows: Vec<(String, u32, u32)>,
    pub start: u32,
    pub flags: u16,
    pub class: u8,
}

impl Body for OverallRankingPage {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            total: r.u32()?,
            context: r.u32()?,
            status: r.u8()?,
            rows: counted(r, MOST, "overall ranking rows", |r| Ok((r.string()?, r.u32()?, r.u32()?)))?,
            start: r.u32()?,
            flags: r.u16()?,
            class: r.u8()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.total);
        w.u32(self.context);
        w.u8(self.status);
        w.count(self.rows.len());
        for (name, score, rank) in &self.rows {
            w.string(name);
            w.u32(*score);
            w.u32(*rank);
        }
        w.u32(self.start);
        w.u16(self.flags);
        w.u8(self.class);
    }
}

/// Ranking::HonorLocalUserRankingInfoResponseCommand (243), actor-scoped: our place
/// -- u32 context, u32 rank (1-based; 0xFFFFFFFF unranked), score, kills, deaths,
/// wins, losses, draws, u8 board, bool active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalRanking {
    pub context: u32,
    pub rank: u32,
    pub values: [u32; 6],
    pub board: u8,
    pub active: bool,
}

impl Body for LocalRanking {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { context: r.u32()?, rank: r.u32()?, values: u32s(r)?, board: r.u8()?, active: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.context);
        w.u32(self.rank);
        write_u32s(w, &self.values);
        w.u8(self.board);
        w.bit(self.active);
    }
}

/// Ranking::HonorOverallLocalUserRankingInfoResponseCommand (245), actor-scoped:
/// our overall place -- u32 context, u32 rank, u8 status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OverallLocalRanking {
    pub context: u32,
    pub rank: u32,
    pub status: u8,
}

impl Body for OverallLocalRanking {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { context: r.u32()?, rank: r.u32()?, status: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.context);
        w.u32(self.rank);
        w.u8(self.status);
    }
}

// ── PvP ─────────────────────────────────────────────────────────────────────

/// PVP::HonorPointsChangedCommand (127), actor-scoped: the honour total.
/// SOURCE: protocol/pvp_match_protocol.py encode_honor_points_changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HonorPointsChanged {
    pub honor: u32,
}

impl Body for HonorPointsChanged {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { honor: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.honor);
    }
}

/// PVP::PVPMatchOverCommand (193), NOT actor-scoped: u32 tick, u32 winning team,
/// bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MatchOver {
    pub tick: u32,
    pub winning_team: u32,
    pub flag: bool,
}

impl Body for MatchOver {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { tick: r.u32()?, winning_team: r.u32()?, flag: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.tick);
        w.u32(self.winning_team);
        w.bit(self.flag);
    }
}

/// PVP::PVPMatchStartCommand (194), NOT actor-scoped: the round's start tick, its
/// duration in ticks (3000), bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MatchStart {
    pub round_start: u32,
    pub duration: u32,
    pub flag: bool,
}

impl Body for MatchStart {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { round_start: r.u32()?, duration: r.u32()?, flag: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.round_start);
        w.u32(self.duration);
        w.bit(self.flag);
    }
}

/// One player of a match's team (PVP::PVPMatchInfoCommand).
/// SOURCE: protocol/pvp_match_protocol.py MatchPlayer (labels from
///   ScoreboardWidget::FillTable, 2026-10-21; the wire shape is unchanged).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatchPlayer {
    /// actor, honour total, honour for rounds won, honour for taking part, honour
    /// bonus, rounds won, kills, deaths, damage dealt, score.
    pub values: [u32; 10],
    pub support: f32,
    pub rating: u32,
    pub level: u16,
    pub enemy: bool,
    pub name: String,
    /// The scoreboard's class emblem (0 warrior, 1 mage, 2 ranger, 3 dwarf).
    pub class_id: u32,
    pub stats: Vec<u32>,
    pub rounds_lost: u32,
}

/// One team of a match: id, rounds won, score, win chance, its players.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatchTeam {
    pub id: u32,
    pub rounds_won: u32,
    pub score: u32,
    pub win_chance: f32,
    pub players: Vec<MatchPlayer>,
}

/// PVP::PVPMatchInfoCommand (196), NOT actor-scoped: the scoreboard -- the round's
/// start and duration, the teams, the battleground id, u32 stage.
/// SOURCE: protocol/pvp_match_protocol.py encode_match_info (:884-949).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatchInfo {
    pub round_start: u32,
    pub duration: u32,
    pub teams: Vec<MatchTeam>,
    pub battleground: String,
    pub stage: u32,
}

impl Body for MatchInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let (round_start, duration) = (r.u32()?, r.u32()?);
        let teams = counted(r, 16, "match teams", |r| {
            let (id, rounds_won, score, win_chance) = (r.u32()?, r.u32()?, r.u32()?, r.f32()?);
            let players = counted(r, 64, "match players", |r| {
                let values = u32s(r)?;
                let (support, rating, level, enemy, name, class_id) = (r.f32()?, r.u32()?, r.u16()?, r.bit()?, r.string()?, r.u32()?);
                let stats = counted(r, 64, "match player stats", |r| r.u32())?;
                Ok(MatchPlayer { values, support, rating, level, enemy, name, class_id, stats, rounds_lost: r.u32()? })
            })?;
            Ok(MatchTeam { id, rounds_won, score, win_chance, players })
        })?;
        Ok(Self { round_start, duration, teams, battleground: r.string()?, stage: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.round_start);
        w.u32(self.duration);
        w.count(self.teams.len());
        for t in &self.teams {
            w.u32(t.id);
            w.u32(t.rounds_won);
            w.u32(t.score);
            w.f32(t.win_chance);
            w.count(t.players.len());
            for p in &t.players {
                write_u32s(w, &p.values);
                w.f32(p.support);
                w.u32(p.rating);
                w.u16(p.level);
                w.bit(p.enemy);
                w.string(&p.name);
                w.u32(p.class_id);
                w.count(p.stats.len());
                write_u32s(w, &p.stats);
                w.u32(p.rounds_lost);
            }
        }
        w.string(&self.battleground);
        w.u32(self.stage);
    }
}

/// PVP::PVPMatchDescriptionCommand (202), NOT actor-scoped: bool active, u32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MatchDescription {
    pub active: bool,
    pub value: u32,
}

impl Body for MatchDescription {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { active: r.bit()?, value: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.bit(self.active);
        w.u32(self.value);
    }
}

/// PVP::SetBattlegroundIdCommand (203), NOT actor-scoped: the battleground's id.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SetBattlegroundId {
    pub id: String,
}

impl Body for SetBattlegroundId {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { id: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.id);
    }
}

/// PVP::ShowPVPDeathDialogCommand (204), actor-scoped: the respawn delay in ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShowPvpDeathDialog {
    pub respawn_delay: u32,
}

impl Body for ShowPvpDeathDialog {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { respawn_delay: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.respawn_delay);
    }
}

/// PVP::JoinMatchQueueResponseCommand (207), actor-scoped: i8 result, bool accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JoinMatchQueueResponse {
    pub result: i8,
    pub accepted: bool,
}

impl Body for JoinMatchQueueResponse {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { result: r.i8()?, accepted: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.i8(self.result);
        w.bit(self.accepted);
    }
}

/// PVP::JoinMatchQueueInfoCommand (208), NOT actor-scoped: u32 tick, f64 estimated
/// wait, i8 state, u16 players queued, the queueing time (year, day, hour, minute,
/// second, ms, weekday, month).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct JoinMatchQueueInfo {
    pub tick: u32,
    pub estimated_wait: f64,
    pub state: i8,
    pub queued: u16,
    pub queued_at: [u32; 8],
}

impl Body for JoinMatchQueueInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { tick: r.u32()?, estimated_wait: f64::from_bits(r.u64()?), state: r.i8()?, queued: r.u16()?, queued_at: u32s(r)? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.tick);
        w.u64(self.estimated_wait.to_bits());
        w.i8(self.state);
        w.u16(self.queued);
        write_u32s(w, &self.queued_at);
    }
}

/// PVP::JoinBattleGroundCommand (210), NOT actor-scoped: a match found -- u32 tick,
/// u32 dialog, u32 battleground, the map, i8 mode. JoinBattlegroundResponse (211)
/// answers it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JoinBattleground {
    pub tick: u32,
    pub dialog: u32,
    pub battleground: u32,
    pub map: String,
    pub mode: i8,
}

impl Body for JoinBattleground {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { tick: r.u32()?, dialog: r.u32()?, battleground: r.u32()?, map: r.string()?, mode: r.i8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.tick);
        w.u32(self.dialog);
        w.u32(self.battleground);
        w.string(&self.map);
        w.i8(self.mode);
    }
}

/// PVP::MatchMakingInfoCommand (215), NOT actor-scoped: i8 event, u32 tick, i8 mode,
/// a message. The event is what happened to the queue, not a state: 0 joined, 1
/// joined by the group leader, 2 left, 3 left by the group leader, 4 aborted by the
/// server (a match that never started; a requeue follows as a new 0), 5 group too
/// big, 6 group level range, -1 invalid.
/// SOURCE: protocol/pvp_match_protocol.py MATCHMAKING_EVENT_* (2026-10-21; before,
///   production sent 0 to join and -1 to leave), services/matchmaking.py.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MatchMakingInfo {
    pub state: i8,
    pub tick: u32,
    pub mode: i8,
    pub message: String,
}

impl MatchMakingInfo {
    /// Whether this event leaves us in the queue.
    pub fn queued(&self) -> bool {
        matches!(self.state, 0 | 1)
    }
}

impl Body for MatchMakingInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { state: r.i8()?, tick: r.u32()?, mode: r.i8()?, message: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.i8(self.state);
        w.u32(self.tick);
        w.i8(self.mode);
        w.string(&self.message);
    }
}

/// PVP::AbortMatchQueueCommand (209), client -> server: u32 tick, bool, the modes
/// left (i8 each), bool every queue.
/// SOURCE: protocol/pvp_match_protocol.py:378-417.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AbortMatchQueue {
    pub tick: u32,
    pub flag: bool,
    pub modes: Vec<i8>,
    pub all: bool,
}

impl Body for AbortMatchQueue {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { tick: r.u32()?, flag: r.bit()?, modes: counted(r, 16, "queue modes", |r| r.i8())?, all: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.tick);
        w.bit(self.flag);
        w.count(self.modes.len());
        for m in &self.modes {
            w.i8(*m);
        }
        w.bit(self.all);
    }
}

/// PVP::JoinBattlegroundResponseCommand (211), client -> server: u32 tick, u32
/// battleground, bool accepted, u32, i8 mode.
/// SOURCE: protocol/pvp_match_protocol.py:447-497.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JoinBattlegroundResponse {
    pub tick: u32,
    pub battleground: u32,
    pub accepted: bool,
    pub unknown_u32_0: u32,
    pub mode: i8,
}

impl Body for JoinBattlegroundResponse {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { tick: r.u32()?, battleground: r.u32()?, accepted: r.bit()?, unknown_u32_0: r.u32()?, mode: r.i8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.tick);
        w.u32(self.battleground);
        w.bit(self.accepted);
        w.u32(self.unknown_u32_0);
        w.i8(self.mode);
    }
}

/// PVP::LeavePVPCommand (199), client -> server: leave the battleground (a forfeit,
/// back to the hub). One bool.
/// SOURCE: protocol/pvp_match_protocol.py:420-445.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LeavePvp {
    pub leave: bool,
}

impl Body for LeavePvp {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { leave: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.bit(self.leave);
    }
}

// ── the Season Pass and the Endless Tower (release261_192) ──────────────────
//
// Answers the server sends since 2026-10-21; the client has no window for them
// yet, so they are decoded only to keep their 0x85 chains exact.

fn u32_array(r: &mut BitReader<'_>, what: &'static str) -> Result<Vec<u32>, DecodeError> {
    counted(r, MOST, what, |r| r.u32())
}

fn write_u32_array(w: &mut BitWriter, values: &[u32]) {
    w.count(values.len());
    write_u32s(w, values);
}

/// GetSeasonPassRewardsInfoResultCommand (354), actor-scoped (actor 0): two u32,
/// the passes owned and the awards claimed (u32 arrays). Sent at login and after a
/// pass ticket is used.
/// SOURCE: protocol/player_init_protocol.py _write_season_pass_rewards_info_result_body,
///   services/season_pass.py.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeasonPassRewardsInfo {
    pub unknown_u32_20: u32,
    pub unknown_u32_24: u32,
    pub pass_types: Vec<u32>,
    pub claimed_awards: Vec<u32>,
}

impl Body for SeasonPassRewardsInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            unknown_u32_20: r.u32()?,
            unknown_u32_24: r.u32()?,
            pass_types: u32_array(r, "season pass types")?,
            claimed_awards: u32_array(r, "season pass awards")?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_20);
        w.u32(self.unknown_u32_24);
        write_u32_array(w, &self.pass_types);
        write_u32_array(w, &self.claimed_awards);
    }
}

/// GetSeasonPassMissionsInfoResultCommand (358), actor-scoped (actor 0): counted
/// (mission, progress) pairs.
/// SOURCE: protocol/player_init_protocol.py _write_season_pass_missions_info_result_body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeasonPassMissionsInfo {
    pub missions: Vec<(u32, u32)>,
}

impl Body for SeasonPassMissionsInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { missions: counted(r, MOST, "season pass missions", |r| Ok((r.u32()?, r.u32()?)))? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.missions.len());
        for &(mission, progress) in &self.missions {
            w.u32(mission);
            w.u32(progress);
        }
    }
}

/// KickOutToHubLimitTimeLeftCommand (363), actor-scoped: the Fractured Planes floor
/// timer in seconds; 0 marks the floor completed (PlanarFissureInfoWidget::SetTimeLeft).
/// SOURCE: protocol/player_init_protocol.py KickOutToHubLimitTimeLeft, services/map_events.py.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KickOutToHubTimeLeft {
    pub seconds: u32,
}

impl Body for KickOutToHubTimeLeft {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { seconds: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.seconds);
    }
}

/// KillMonsterGetTotalScoresCommand (364), actor-scoped: the running Endless Tower /
/// Fractured Planes kill score (the PlanarFissureInfoWidget bar).
/// SOURCE: protocol/player_init_protocol.py encode_kill_monster_get_total_scores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KillScore {
    pub score: u32,
}

impl Body for KillScore {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { score: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.score);
    }
}

/// GetSelfEndlessTowerDataCommand (367), actor-scoped: the highest floor cleared, the
/// ranking score, the highest floor rewarded.
/// SOURCE: protocol/player_init_protocol.py encode_endless_tower_progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EndlessTowerProgress {
    pub cleared_floor: u32,
    pub score: u32,
    pub claimed_floor: u32,
}

impl Body for EndlessTowerProgress {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { cleared_floor: r.u32()?, score: r.u32()?, claimed_floor: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        write_u32s(w, &[self.cleared_floor, self.score, self.claimed_floor]);
    }
}

/// GetEndlessTowerAllBuffsResultCommand (369), actor-scoped: the buff choices (status
/// effect resources), a u32 count then strings.
/// SOURCE: protocol/player_init_protocol.py encode_endless_tower_buffs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EndlessTowerBuffs {
    pub buffs: Vec<String>,
}

impl Body for EndlessTowerBuffs {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { buffs: counted(r, MOST, "endless tower buffs", |r| r.string())? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.buffs.len());
        for b in &self.buffs {
            w.string(b);
        }
    }
}
