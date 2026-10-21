//! Groups, notifications, factions, the PvP flag, and the social commands the client
//! sends.
//!
//! SOURCE: experimental dsor/groups.py (encode_group_status, encode_member,
//!   encode_invitation_response, encode_invite_notification, encode_kick_notice and the
//!   client decoders), dsor/pvp.py (encode_pvp_flag, encode_faction_info,
//!   decode_pvp_flag), server.py Service._handle_group; rust-18 src/dsor/groups.rs and
//!   pvp.rs carry the same layouts.
//! CONTRACT: a command class has ONE reader and ONE writer (vtable slots 5 and 4), so
//!   an id both sides send shares one struct unless evidence shows otherwise.
//! UNKNOWN: client commands 175, 185 and 191 have no server decoder and no traced
//!   writer here; their bodies are kept verbatim (`unknown_body`).

use dsor_raknet::{BitReader, BitWriter};

use super::wire::{counted, Body, Raw, ReadExt, WriteExt, MOST};
use super::DecodeError;

/// A u16 length then raw bytes (the notification guid is not text).
fn read_blob(r: &mut BitReader<'_>) -> Result<Vec<u8>, DecodeError> {
    let n = r.u16()? as usize;
    Ok(r.read_bytes(n)?)
}

fn write_blob(w: &mut BitWriter, data: &[u8]) {
    w.u16(data.len() as u16);
    w.write_bytes(data);
}

/// Everything left in a client message, verbatim (0x8B bodies run to the end).
fn rest(r: &mut BitReader<'_>) -> Result<Raw, DecodeError> {
    let n = r.remaining();
    r.raw(n)
}

/// FactionInfoCommand (131): replace an actor's whole GameFaction.
/// EVIDENCE: reader 0x9A711D -> sub_A4C4E8 (u32, u32, s8 type, bool, bool); handler
///   0x515C24 copies it onto the actor named by `owner` (0x515CAA), not the trailer.
///   SOURCE: dsor/pvp.py encode_faction_info. 74 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FactionInfo {
    /// 0 while unflagged; a flagged player carries its own actor id.
    pub faction_id: u32,
    /// The actor this faction belongs to.
    pub owner: u32,
    pub kind: i8,
    /// Faction +0xC: both sides set and FactionIds differ -> hostile (sub_995A1C).
    pub pvp: bool,
    /// Faction +0xD, meaning not established.
    pub unknown_bool_0: bool,
}

impl Body for FactionInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { faction_id: r.u32()?, owner: r.u32()?, kind: r.i8()?, pvp: r.bit()?, unknown_bool_0: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.faction_id);
        w.u32(self.owner);
        w.i8(self.kind);
        w.bit(self.pvp);
        w.bit(self.unknown_bool_0);
    }
}

/// PVP::PVPFlagCommand (204), both directions: a bool then the world tick.
/// EVIDENCE: reader 0x9DD28C, writer 0x9DDBA6; handler 0x519244 -> SetPvPFlag 0x51DFC9
///   (tick stored at player +0x644 when on). The client's HandleSetPVPFlag 0x4E439F
///   sends NOT its current flag, then the tick. SOURCE: dsor/pvp.py encode_pvp_flag,
///   decode_pvp_flag. 33 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PvpFlag {
    pub flag: bool,
    pub tick: u32,
}

impl Body for PvpFlag {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { flag: r.bit()?, tick: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.bit(self.flag);
        w.u32(self.tick);
    }
}

/// One NotificationCommand entry.
/// EVIDENCE: entry reader 0x8FD574: u8 type, guid (u16 length + bytes), u32, u32
///   (echoed as the answer's b), u32 (echoed as its a), str JSON, str timestamp.
///   Type 1 is a group invite (0xA1EB67 "groupinvite"); its JSON keys are read by the
///   dialog 0x53E46E. SOURCE: dsor/groups.py encode_invite_notification.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotificationEntry {
    pub note_type: u8,
    /// 16 random bytes as the server sends it.
    pub guid: Vec<u8>,
    pub unknown_u32_0: u32,
    /// For a group invite: the group the invite names (the answer's b).
    pub group_id: u32,
    /// For a group invite: the inviter's player id (the answer's a).
    pub inviter_id: u32,
    /// JSON parameters (playerName, playerLevel, playerClass, honorPoints, groupName,
    /// playerGender, titleType, achievementTitle for an invite).
    pub params: String,
    /// "YYYY-MM-DD HH:MM:SS".
    pub timestamp: String,
}

impl NotificationEntry {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            note_type: r.u8()?,
            guid: read_blob(r)?,
            unknown_u32_0: r.u32()?,
            group_id: r.u32()?,
            inviter_id: r.u32()?,
            params: r.string()?,
            timestamp: r.string()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.note_type);
        write_blob(w, &self.guid);
        w.u32(self.unknown_u32_0);
        w.u32(self.group_id);
        w.u32(self.inviter_id);
        w.string(&self.params);
        w.string(&self.timestamp);
    }
}

/// NotificationCommand (145; release261 136), both directions.
/// Type 1, server -> client: the notifications to show (a u16 count, then entries).
/// Type 2, client -> server: the answer to one of them -- a u8 response, then the
///   entry echoed whole.
/// EVIDENCE: reader 0xA1FB21: u8 type (1 carries notifications), then entries.
///   Writer 0xA1FC71 (Commands::NotificationCommand::Encode): type 1 writes the u16
///   count and the entries; type 2 the u8 at +0x14 then the one entry at +0x18.
///   The response is Game::NotificationResponse::Code, passed through unchanged
///   (0x564FC6 stores it at +0x14): the group-invite dialog 0x53E46E builds its
///   confirm message with 1 and its cancel message with 2 (release261 0x14020C4F4
///   the same, and 5 when already grouped).
///   SOURCE: dsor/groups.py encode_invite_notification; DSOR_Server
///   protocol/player_init_protocol.py _write_notification_body and
///   protocol/reward_protocol.py decode_notification_action.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Notification {
    pub kind: u8,
    /// Type 2 only: the answer (1 confirm, 2 cancel).
    pub response: u8,
    pub entries: Vec<NotificationEntry>,
}

impl Notification {
    /// The answer to one notification (type 2).
    pub fn response(response: u8, entry: NotificationEntry) -> Self {
        Self { kind: 2, response, entries: vec![entry] }
    }
}

impl Body for Notification {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let kind = r.u8()?;
        if kind == 2 {
            let response = r.u8()?;
            return Ok(Self { kind, response, entries: vec![NotificationEntry::decode(r)?] });
        }
        let n = r.u16()? as usize;
        let mut entries = Vec::with_capacity(n.min(64));
        for _ in 0..n {
            entries.push(NotificationEntry::decode(r)?);
        }
        Ok(Self { kind, response: 0, entries })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.kind);
        if self.kind == 2 {
            w.u8(self.response);
            if let Some(e) = self.entries.first() {
                e.encode(w);
            }
            return;
        }
        w.u16(self.entries.len() as u16);
        for e in &self.entries {
            e.encode(w);
        }
    }
}

/// One Game::MessageParam of a MessageCommand: u8 kind, then its value.
/// SOURCE: DSOR_Server protocol/gameplay_protocol.py _read_message_argument (kinds
///   1 integer, 2 string, 4 item instance id, the only ones retail was seen sending).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageParam {
    Int(u32),
    Str(String),
    Item(u32),
}

impl MessageParam {
    pub fn int(&self) -> Option<u32> {
        match self {
            Self::Int(v) | Self::Item(v) => Some(*v),
            Self::Str(_) => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// MessageCommand (137): a typed event with parameters. Release 261's handler
/// (ClientGameWorld::HandleMessageCommand, dro_client64 0x1401d0848) switches on the
/// type: 0 maintenance countdown, 0x19 loot received (currency type, amount, item,
/// template), 0x1d sudden death...
/// SOURCE: DSOR_Server protocol/gameplay_protocol.py encode_message: u8 type, u32
///   count, the params.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Message {
    pub message_type: u8,
    pub params: Vec<MessageParam>,
}

impl Body for Message {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let message_type = r.u8()?;
        let n = r.count(64, "message params")?;
        let mut params = Vec::with_capacity(n);
        for _ in 0..n {
            params.push(match r.u8()? {
                1 => MessageParam::Int(r.u32()?),
                2 => MessageParam::Str(r.string()?),
                4 => MessageParam::Item(r.u32()?),
                k => return Err(DecodeError::Invalid { what: "message param kind", value: k as i64 }),
            });
        }
        Ok(Self { message_type, params })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.message_type);
        w.u32(self.params.len() as u32);
        for p in &self.params {
            match p {
                MessageParam::Int(v) => {
                    w.u8(1);
                    w.u32(*v);
                }
                MessageParam::Str(s) => {
                    w.u8(2);
                    w.string(s);
                }
                MessageParam::Item(v) => {
                    w.u8(4);
                    w.u32(*v);
                }
            }
        }
    }
}

/// ServerToClientMsgCommand (release261_192: 381), actor-scoped (actor 0): a text the
/// server puts on screen. i32 type (0 the nonmodal MessageWidget, 1 and 2 dialogs),
/// the text, a second string (empty from production).
/// SOURCE: DSOR_Server protocol/gameplay_protocol.py encode_screen_message (decoder
///   0x14093e6f4, ClientMessageManager 0x1401d9978); the map events' wave notices.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenMessage {
    pub kind: i32,
    pub text: String,
    pub extra: String,
}

impl Body for ScreenMessage {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { kind: r.i32()?, text: r.string()?, extra: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.i32(self.kind);
        w.string(&self.text);
        w.string(&self.extra);
    }
}

/// One PlayerSocialInfo row of a GroupStatus (and of a GroupMemberQueryResult).
/// SOURCE: dso-server-production protocol/social_protocol.py PlayerSummary and
///   _write_player_summary (:216-262): u32, u32, u32 character, u32 actor (on the
///   RECIPIENT's map; 0 elsewhere), str name, str map, u8 class, u16 level, u16 (200),
///   u32, u16, bool online, bool available, bool invitable, u8 role (0 the leader,
///   1 a member), str timestamp, u8 relation (0xFF), str note, u32 (0xFFFFFFFF).
///   Against the 2018 reader (0xA617CB): the role byte and the note string are new.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupMember {
    pub unknown_u32_0: u32,
    pub unknown_u32_1: u32,
    pub player_id: u32,
    /// The member's actor on the RECIPIENT's map, 0 when elsewhere.
    pub actor: u32,
    pub name: String,
    pub map_name: String,
    pub character_class: u8,
    pub level: u16,
    pub unknown_u16_2: u16,
    pub unknown_u32_3: u32,
    pub unknown_u16_4: u16,
    pub online: bool,
    pub available: bool,
    pub invitable: bool,
    /// 0 the leader, 1 another member (0xFF when not grouped).
    pub role: u8,
    pub timestamp: String,
    pub relation: u8,
    pub note: String,
    pub unknown_u32_5: u32,
}

impl GroupMember {
    pub const LEADER: u8 = 0;

    pub fn leads(&self) -> bool {
        self.role == Self::LEADER
    }
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            unknown_u32_0: r.u32()?,
            unknown_u32_1: r.u32()?,
            player_id: r.u32()?,
            actor: r.u32()?,
            name: r.string()?,
            map_name: r.string()?,
            character_class: r.u8()?,
            level: r.u16()?,
            unknown_u16_2: r.u16()?,
            unknown_u32_3: r.u32()?,
            unknown_u16_4: r.u16()?,
            online: r.bit()?,
            available: r.bit()?,
            invitable: r.bit()?,
            role: r.u8()?,
            timestamp: r.string()?,
            relation: r.u8()?,
            note: r.string()?,
            unknown_u32_5: r.u32()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_0);
        w.u32(self.unknown_u32_1);
        w.u32(self.player_id);
        w.u32(self.actor);
        w.string(&self.name);
        w.string(&self.map_name);
        w.u8(self.character_class);
        w.u16(self.level);
        w.u16(self.unknown_u16_2);
        w.u32(self.unknown_u32_3);
        w.u16(self.unknown_u16_4);
        w.bit(self.online);
        w.bit(self.available);
        w.bit(self.invitable);
        w.u8(self.role);
        w.string(&self.timestamp);
        w.u8(self.relation);
        w.string(&self.note);
        w.u32(self.unknown_u32_5);
    }
}

/// GroupStatusCommand (151), NOT actor-scoped: the whole group as one recipient sees
/// it; group 0 with no members is "no group". The server sends it to the asker of
/// a GroupList, a GroupFoundation or a GroupLeave, to every member when one joins
/// or changes map, and at login when grouped (services/social.py).
/// SOURCE: dso-server-production protocol/social_protocol.py encode_group_status
///   (:443-459): u8 status, u32 group, u32 leader, u32 (the recipient's character),
///   bool listed, u32 count + members, str name, bool private, str description.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupStatus {
    pub kind: i8,
    pub group_id: u32,
    pub leader_id: u32,
    /// The receiving character's id (what the server writes as the tick).
    pub recipient_id: u32,
    /// Listed in the group finder.
    pub listed: bool,
    pub members: Vec<GroupMember>,
    pub name: String,
    pub private: bool,
    pub description: String,
}

impl GroupStatus {
    /// Whether the recipient is in a group at all.
    pub fn grouped(&self) -> bool {
        self.group_id != 0 && !self.members.is_empty()
    }
}

impl Body for GroupStatus {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: r.i8()?,
            group_id: r.u32()?,
            leader_id: r.u32()?,
            recipient_id: r.u32()?,
            listed: r.bit()?,
            members: counted(r, MOST, "group member count", GroupMember::decode)?,
            name: r.string()?,
            private: r.bit()?,
            description: r.string()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.i8(self.kind);
        w.u32(self.group_id);
        w.u32(self.leader_id);
        w.u32(self.recipient_id);
        w.bit(self.listed);
        w.count(self.members.len());
        for m in &self.members {
            m.encode(w);
        }
        w.string(&self.name);
        w.bit(self.private);
        w.string(&self.description);
    }
}

/// PlayerQueryResultCommand (181), NOT actor-scoped: the query id echoed, then the
/// players found as GroupStatus's rows (PlayerSocialInfo). The server fills a
/// row's map field with the player's guild, its three flags with whether the
/// player is online, its role with 0xFF.
/// SOURCE: dso-server-production protocol/social_protocol.py
///   encode_player_query_result (:344-355); services/social.py _handle_player_query.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerQueryResult {
    pub query_id: u32,
    pub players: Vec<GroupMember>,
}

impl Body for PlayerQueryResult {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { query_id: r.u32()?, players: counted(r, MOST, "player query result count", GroupMember::decode)? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.query_id);
        w.count(self.players.len());
        for p in &self.players {
            p.encode(w);
        }
    }
}

/// GroupFoundationResponseCommand (153), NOT actor-scoped: u8 status (0 made, 1
/// refused), u32 tick, str message (the refusal's text). A GroupStatus follows.
/// SOURCE: dso-server-production protocol/social_protocol.py
///   encode_create_group_result (:489-499).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupFoundationResponse {
    pub status: u8,
    pub tick: u32,
    pub message: String,
}

impl Body for GroupFoundationResponse {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { status: r.u8()?, tick: r.u32()?, message: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.status);
        w.u32(self.tick);
        w.string(&self.message);
    }
}

/// GroupingFailedCommand (163), NOT actor-scoped, to the player whose acceptance
/// failed: u32 tick, str player, u8 reason (0 the group is full, 1 already in a
/// group, 2 the invitation expired, 3 no such group).
/// SOURCE: dso-server-production protocol/social_protocol.py encode_grouping_failed
///   (:535-567).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupingFailed {
    pub tick: u32,
    pub name: String,
    pub reason: u8,
}

impl GroupingFailed {
    pub fn text(&self) -> &'static str {
        match self.reason {
            0 => "The group is full.",
            1 => "You are already in a group.",
            2 => "The invitation has expired.",
            _ => "That group no longer exists.",
        }
    }
}

impl Body for GroupingFailed {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { tick: r.u32()?, name: r.string()?, reason: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.tick);
        w.string(&self.name);
        w.u8(self.reason);
    }
}

/// GuildFoundationResponseCommand (169), NOT actor-scoped: u8 status (0 founded, 1
/// already in a guild, 3 the name is taken, 4 a bad name), u32 tick, str message,
/// str guild name, bool. A GuildStatus follows a success.
/// SOURCE: dso-server-production protocol/social_protocol.py
///   encode_guild_foundation_result (:679-702).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuildFoundationResponse {
    pub status: u8,
    pub tick: u32,
    pub message: String,
    pub guild: String,
    pub unknown_bool_0: bool,
}

impl Body for GuildFoundationResponse {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { status: r.u8()?, tick: r.u32()?, message: r.string()?, guild: r.string()?, unknown_bool_0: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.status);
        w.u32(self.tick);
        w.string(&self.message);
        w.string(&self.guild);
        w.bit(self.unknown_bool_0);
    }
}

/// IgnoredStatusCommand (179), NOT actor-scoped: the ignore list, which this server
/// always sends empty right after a GuildStatus (the client reads the pair).
/// u8 status, u32 (the recipient's character id), u32 count (0).
/// SOURCE: dso-server-production protocol/social_protocol.py encode_guild_status
///   (:775-781).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IgnoredStatus {
    pub status: u8,
    pub recipient_id: u32,
    pub count: u32,
}

impl Body for IgnoredStatus {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let v = Self { status: r.u8()?, recipient_id: r.u32()?, count: r.u32()? };
        if v.count != 0 {
            return Err(DecodeError::Invalid { what: "ignored players (entry layout unknown)", value: v.count as i64 });
        }
        Ok(v)
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.status);
        w.u32(self.recipient_id);
        w.u32(0);
    }
}

/// GroupInvitationResponseCommand (156) and GuildInvitationResponseCommand (171),
/// both directions, one layout.
/// Server -> inviter (actor-scoped): code 0 joined, 1 declined, 2 already in a group
///   or guild, 4 no such player or group, 5 no right to invite, 6 no guild (guilds),
///   with the other player's name; a = the target's id, b = the group's or guild's.
/// Client (invitee): code 0 accepts, 1 declines, a = the notification's inviter id,
///   b = its group or guild id, name = OUR name (the server checks it).
/// SOURCE: dso-server-production protocol/social_protocol.py
///   decode_invitation_response (:185-203), encode_invitation_response (:502-532);
///   services/social.py _handle_invitation_response (0 joins, 1 declines, anything
///   else is refused).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupInvitationResponse {
    pub code: i8,
    /// The inviter's player id when the client answers; 0 from the server.
    pub a: u32,
    /// The group id when the client answers; 0 from the server.
    pub b: u32,
    pub name: String,
}

impl Body for GroupInvitationResponse {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { code: r.i8()?, a: r.u32()?, b: r.u32()?, name: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.i8(self.code);
        w.u32(self.a);
        w.u32(self.b);
        w.string(&self.name);
    }
}

/// GroupKickCommand (168), both directions: the kicked member's name and two u32.
/// Leader -> server: decode_kick reads the name (0x56EEAB). Server -> kicked player:
///   handler 0x5711E7 shows youWereKicked and ignores the fields.
/// SOURCE: dsor/groups.py encode_kick_notice (str, u32 0, u32 0), decode_kick.
/// UNKNOWN: whether the client's own writer also emits the two u32 (no capture).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupKick {
    pub name: String,
    pub unknown_u32_0: u32,
    pub unknown_u32_1: u32,
}

impl Body for GroupKick {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { name: r.string()?, unknown_u32_0: r.u32()?, unknown_u32_1: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.name);
        w.u32(self.unknown_u32_0);
        w.u32(self.unknown_u32_1);
    }
}

/// GroupListCommand (159), client -> server: type 0 asks for the GroupStatus, type 2
/// promotes the player id in the THIRD u32.
/// EVIDENCE: writer 0xA234A1; senders 0x57566E (type 0), 0x56D09B (type 2).
///   SOURCE: dsor/groups.py decode_group_list. 104 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GroupList {
    pub kind: u8,
    pub unknown_u32_0: u32,
    pub unknown_u32_1: u32,
    /// The new leader's player id for type 2.
    pub target: u32,
}

impl Body for GroupList {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { kind: r.u8()?, unknown_u32_0: r.u32()?, unknown_u32_1: r.u32()?, target: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.kind);
        w.u32(self.unknown_u32_0);
        w.u32(self.unknown_u32_1);
        w.u32(self.target);
    }
}

/// GroupInviteCommand (163), client -> server: own name, invitee name, then a u32.
/// EVIDENCE: writer 0xA232F5 (dsor/groups.py decode_invite reads the two strings);
///   the captured payloads (session-merged*) carry 32 more bits, always 0:
///   `8b a300 1100 "DSOReconstruction" 0400 "simo" 00000000`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupInvite {
    pub inviter_name: String,
    pub invitee_name: String,
    pub unknown_u32_0: u32,
}

impl Body for GroupInvite {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { inviter_name: r.string()?, invitee_name: r.string()?, unknown_u32_0: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.inviter_name);
        w.string(&self.invitee_name);
        w.u32(self.unknown_u32_0);
    }
}

/// PVP::JoinMatchQueueCommand (206), client -> server: u32 tick, i8 match mode
/// (0 BestOf3 1v1, 1 BestOf5 5v5, 2 CTF 5v5, 3 AssaultFortress 5v5, 4
/// LastManStanding 8ffa), i8 queue kind (0 alone, 1 as a group).
/// SOURCE: dso-server-production protocol/pvp_match_protocol.py:342-375 (the
///   2018 capture carried the mode as a string).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JoinMatchQueue {
    pub tick: u32,
    pub mode: i8,
    pub kind: i8,
}

impl Body for JoinMatchQueue {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { tick: r.u32()?, mode: r.i8()?, kind: r.i8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.tick);
        w.i8(self.mode);
        w.i8(self.kind);
    }
}

/// GroupFoundationCommand (152) and GuildFoundationCommand (168), client -> server:
/// u32, str our name, str the group's or guild's name, bool.
/// SOURCE: dso-server-production protocol/social_protocol.py decode_foundation_request
///   (:101-112); the captured `8b b100 000000000000 0600 "qsdqsd"` + 1 bit is the
///   same bytes with an empty first string (the 2018 client wrote none).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Foundation {
    pub unknown_u32_0: u32,
    pub source_name: String,
    pub name: String,
    pub unknown_bool_1: bool,
}

impl Body for Foundation {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unknown_u32_0: r.u32()?, source_name: r.string()?, name: r.string()?, unknown_bool_1: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_0);
        w.string(&self.source_name);
        w.string(&self.name);
        w.bit(self.unknown_bool_1);
    }
}

/// GroupLeaveCommand (160) and GuildLeaveCommand (173), client -> server: one u32
/// the server ignores.
/// SOURCE: dso-server-production protocol/social_protocol.py decode_uint32_request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Leave {
    pub unknown_u32_0: u32,
}

impl Body for Leave {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unknown_u32_0: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_0);
    }
}

/// GuildInviteCommand (170), client -> server: our name, the invitee's, two u32 and
/// a message. The server answers the inviter with a GuildInvitationResponse (171)
/// on failure (6 no guild, 5 no right to invite, 2 already in a guild, 4 offline) and
/// sends the invitee a Notification of kind 3 on success.
/// SOURCE: dso-server-production protocol/social_protocol.py decode_guild_invite
///   (:155-172).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuildInvite {
    pub inviter_name: String,
    pub invitee_name: String,
    pub unknown_u32_0: u32,
    pub unknown_u32_1: u32,
    pub message: String,
}

impl Body for GuildInvite {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { inviter_name: r.string()?, invitee_name: r.string()?, unknown_u32_0: r.u32()?, unknown_u32_1: r.u32()?, message: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.inviter_name);
        w.string(&self.invitee_name);
        w.u32(self.unknown_u32_0);
        w.u32(self.unknown_u32_1);
        w.string(&self.message);
    }
}

macro_rules! verbatim_body {
    ($($(#[$m:meta])* $name:ident;)*) => {$(
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, Default)]
        pub struct $name {
            /// The body to the end of the 0x8B message; layout not established.
            pub unknown_body: Raw,
        }
        impl Body for $name {
            fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
                Ok(Self { unknown_body: rest(r)? })
            }
            fn encode(&self, w: &mut BitWriter) {
                self.unknown_body.write(w)
            }
        }
    )*};
}

verbatim_body! {
    /// GuildListCommand (175), client -> server. Captured: 144 body bits, seventeen
    /// zero bytes then 0xFF. No server decoder; layout NOT established.
    GuildList;
    /// BuddyListCommand (185), client -> server. Captured: 57 zero body bits. No
    /// server decoder; layout NOT established.
    BuddyList;
    /// GroupQueryCommand (191), client -> server, sent while ungrouped and left
    /// unanswered (server.py _handle_group). Captured: 184 body bits, 0x10 at body
    /// byte 6, otherwise zero. Layout NOT established.
    GroupQuery;
}
