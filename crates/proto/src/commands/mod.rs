//! Game commands (the 0x84 single and 0x85 chained server commands, the 0x8B client
//! commands). Each command is decoded field by field: a 0x85 batch carries no lengths,
//! so the next command can only be found by reading the previous one entirely.
//!
//! WIRE (server -> client):
//! - 0x84: `[u8 0x84][u16 id][body]` then `[u32 actor]` only when the command is
//!   actor-scoped (names::actor_scoped). No terminator.
//! - 0x85: `[u8 0x85]` then, until the stream ends, `{[u16 id][body][u32 actor if
//!   scoped][u8 0xFF]}`. Up to seven padding bits may follow the last 0xFF.
//!
//! EVIDENCE: DrasaClientHandler::DecodeCommand (sub_A67DF6) calls slot 5 (the body)
//!   then slot 9 on every command; slot 9 is sub_A06021 (32-bit actor) for the
//!   actor-scoped commands and sub_A4F84F (reads nothing) for the rest
//!   (experimental dsor/mapentry2018.py guild_status, dsor/groups.py).
//!
//! WIRE (client -> server):
//! - 0x8B: `[u8 0x8B][u16 id][body]`, NO actor and NO terminator.
//!   EVIDENCE: every captured client payload (e.g. `8b 2b 00 00`, 25 bits: id 43 and a
//!   one-bit body; MoveCommand is 144 bits = 8 + 16 + 120); experimental
//!   dsor/chain.py `single`.

pub mod actors;
pub mod client;
pub mod combat;
pub mod inventory;
pub mod movement;
pub mod names;
pub mod player;
pub mod props;
pub mod social;
pub mod systems;
pub mod trade;
pub mod wire;

use dsor_raknet::{BitReader, BitWriter, Overrun};

pub use client::ClientCommand;
pub use names::{actor_scoped, command_name, MOST_COMMANDS};
pub use wire::{Body, Raw, ReadExt, WriteExt};

/// The single-command envelope the server sends.
pub const SINGLE: u8 = 0x84;
/// The chained-command envelope the server sends.
pub const CHAIN: u8 = 0x85;
/// The envelope the client sends its commands in.
pub const CLIENT: u8 = 0x8B;
/// What closes every command in a 0x85 chain.
pub const TERMINATOR: u8 = 0xFF;
/// How many padding bits may follow the last command (RakNet rounds up to a byte).
pub const PADDING_BITS: usize = 7;
/// Chat's builtin command, the one id above MOST_COMMANDS the client accepts.
pub const BUILTIN_CHAT: u16 = 1234;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// A field ran past the end of the message.
    Overrun(Overrun),
    Empty,
    /// The first byte is not an envelope this decoder reads.
    NotACommandMessage(u8),
    /// A value the client's own reader refuses (count above its bound, enum out of
    /// range), or a value that proves the layout is being misread.
    Invalid { what: &'static str, value: i64 },
    /// A 0x85 command did not end on 0xFF.
    BadTerminator { got: u8 },
    /// Bits left over past the last command (beyond the allowed padding).
    Trailing { bits: usize },
    /// The failure, with the command it happened in.
    InCommand { id: u16, index: usize, at: usize, error: Box<DecodeError> },
}

impl From<Overrun> for DecodeError {
    fn from(e: Overrun) -> Self {
        Self::Overrun(e)
    }
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Overrun(e) => write!(f, "overrun: {e}"),
            Self::Empty => write!(f, "empty message"),
            Self::NotACommandMessage(b) => write!(f, "not a command message: {b:#04x}"),
            Self::Invalid { what, value } => write!(f, "invalid {what}: {value}"),
            Self::BadTerminator { got } => write!(f, "terminator is {got:#04x}, not 0xFF"),
            Self::Trailing { bits } => write!(f, "{bits} bits left over"),
            Self::InCommand { id, index, at, error } => write!(
                f,
                "command #{index} id {id} ({}) at bit {at}: {error}",
                command_name(*id).unwrap_or("?")
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

macro_rules! server_commands {
    ($($id:literal => $var:ident($ty:ty),)*) => {
        /// Every command the server sends, decoded.
        #[derive(Debug, Clone, PartialEq)]
        pub enum ServerCommand {
            $($var($ty),)*
            /// A command this crate has no layout for. In a 0x85 chain its end was
            /// found by searching for the actor + 0xFF tail, so treat it as a guess.
            Unknown { id: u16, body: Raw },
        }

        impl ServerCommand {
            /// The command id on the wire.
            pub fn id(&self) -> u16 {
                match self {
                    $(Self::$var(_) => $id,)*
                    Self::Unknown { id, .. } => *id,
                }
            }

            /// Whether this crate has a layout for server command `id`.
            pub fn is_known(id: u16) -> bool {
                matches!(id, $($id)|*)
            }

            /// The body of server command `id`, or None when the id has no layout.
            pub fn decode_body(id: u16, r: &mut BitReader<'_>) -> Option<Result<Self, DecodeError>> {
                Some(match id {
                    $($id => <$ty as Body>::decode_server(r).map(Self::$var),)*
                    _ => return None,
                })
            }

            /// Write the body (no id, no actor, no terminator).
            pub fn encode_body(&self, w: &mut BitWriter) {
                match self {
                    $(Self::$var(c) => c.encode_server(w),)*
                    Self::Unknown { body, .. } => body.write(w),
                }
            }
        }
    };
}

server_commands! {
    // player.rs: the local player, other players, map entry and hand-offs.
    27 => InstanceConfigClientInfo(player::InstanceConfigClientInfo),
    29 => NewPlayer(player::NewPlayer),
    33 => NewRemotePlayer(player::NewRemotePlayer),
    34 => RemotePlayerInfo(player::RemotePlayerInfo),
    68 => ExitInfo(player::ExitInfo),
    112 => SwitchMap(player::SwitchMap),
    124 => PlayerLevelUpdate(player::PlayerLevelUpdate),
    125 => XpChanged(player::XpChanged),
    128 => CurrencyChanged(player::CurrencyChanged),
    132 => SpecialOffer(player::SpecialOffer),
    134 => CharacterGeneration(player::CharacterGeneration),
    135 => CharacterSelection(player::CharacterSelection),
    167 => GuildStatus(player::GuildStatus),
    221 => EventUpdate(player::EventUpdate),
    276 => AchievementInfo(player::AchievementInfo),
    327 => PlayerLogoutInfo(player::PlayerLogoutInfo),
    // actors.rs: world entities, items on the ground, props, quests.
    38 => NewNpc(actors::NewNpc),
    40 => NpcInfo(actors::NpcInfo),
    42 => NewMonster(actors::NewMonster),
    44 => MonsterUpdate(actors::MonsterUpdate),
    45 => NewItem(actors::NewItem),
    46 => DiscardItem(actors::DiscardItem),
    48 => ItemUpdate(actors::ItemUpdate),
    54 => PropInfo(actors::PropInfo),
    52 => NewProp(props::NewProp),
    53 => DiscardProp(props::DiscardProp),
    55 => PropUpdate(props::PropUpdate),
    58 => NewDestroyable(actors::NewDestroyable),
    139 => QuestUpdate(actors::QuestUpdate),
    140 => QuestLogInfo(actors::QuestLogInfo),
    142 => QuestMonsterIndicationInfo(actors::QuestMonsterIndicationInfo),
    144 => QuestTriggerIndicationInfo(actors::QuestTriggerIndicationInfo),
    146 => ChapterUnlocked(actors::ChapterUnlocked),
    147 => ChapterProgress(actors::ChapterProgress),
    261 => DebugMonsterInfo(actors::DebugMonsterInfo),
    // movement.rs: positions, vicinity, discards, stats.
    30 => DiscardPlayer(movement::DiscardPlayer),
    43 => DiscardMonster(movement::DiscardMonster),
    95 => Move(movement::Move),
    36 => MembershipUpdate(player::MembershipUpdate),
    113 => ForceTransform(movement::ForceTransform),
    115 => ActorsLeftVicinity(movement::ActorsLeftVicinity),
    116 => ActorsEnterVicinity(movement::ActorsEnterVicinity),
    123 => ActorStatsUpdate(movement::ActorStatsUpdate),
    // combat.rs: skills, hits, deaths, status and ground effects, traps.
    60 => NewLocationEffect(combat::NewLocationEffect),
    61 => DiscardLocationEffect(combat::DiscardLocationEffect),
    63 => NewTrap(combat::NewTrap),
    64 => DiscardTrap(combat::DiscardTrap),
    66 => TrapTripped(combat::TrapTripped),
    70 => Skill(combat::Skill),
    71 => TargetSkill(combat::TargetSkill),
    72 => BulletSkill(combat::BulletSkill),
    75 => TargetBulletSkill(combat::TargetBulletSkill),
    73 => TargetPointBulletSkill(combat::TargetPointBulletSkill),
    74 => ShiftedSkill(combat::ShiftedSkill),
    77 => SkillStop(combat::SkillStop),
    79 => StatusEffect(combat::StatusEffect),
    107 => Hit(combat::Hit),
    108 => Kill(combat::Kill),
    109 => Revive(combat::Revive),
    110 => Resurrect(combat::Resurrect),
    129 => ShowDeathDialog(combat::ShowDeathDialog),
    // inventory.rs: bags, quick slots, skill book, shops, talents.
    80 => QuickSlotsInfo(inventory::QuickSlotsInfo),
    84 => InventoryInfo(inventory::InventoryInfo),
    88 => SkillBookInfo(inventory::SkillBookInfo),
    90 => Offer(inventory::Offer),
    268 => OcpMethods(inventory::OcpMethods),
    270 => OcpOfferResponse(inventory::OcpOfferResponse),
    283 => TalentSelectionInfo(inventory::TalentSelectionInfo),
    82 => QuickbarInfo(inventory::QuickbarInfo),
    130 => TimedOffersInfo(inventory::TimedOffersInfo),
    310 => GameCurrencyChanged(inventory::GameCurrencyChanged),
    314 => TierSpenderProgress(inventory::TierSpenderProgress),
    282 => TalentCategoryPointsInfo(inventory::TalentCategoryPointsInfo),
    285 => UnlockSkillTalentPreselectionInfo(inventory::UnlockSkillTalentPreselectionInfo),
    // social.rs: factions, groups, notifications, PvP.
    122 => FactionInfo(social::FactionInfo),
    136 => Notification(social::Notification),
    137 => Message(social::Message),
    151 => GroupStatus(social::GroupStatus),
    156 => GroupInvitationResponse(social::GroupInvitationResponse),
    159 => GroupKick(social::GroupKick),
    153 => GroupFoundationResponse(social::GroupFoundationResponse),
    163 => GroupingFailed(social::GroupingFailed),
    169 => GuildFoundationResponse(social::GuildFoundationResponse),
    171 => GuildInvitationResponse(social::GroupInvitationResponse),
    179 => IgnoredStatus(social::IgnoredStatus),
    181 => PlayerQueryResult(social::PlayerQueryResult),
    192 => PvpFlag(social::PvpFlag),
    381 => ScreenMessage(social::ScreenMessage),
    // systems.rs: items, the stickerbook, events, dailies, the Wanted Thief, the
    // rankings, PvP, the premium shop, the Season Pass, the Endless Tower.
    86 => GemRemovalResponse(systems::GemRemovalResponse),
    93 => CraftingResult(systems::CraftingResult),
    311 => RecipeBookInfo(systems::RecipeBookInfo),
    320 => ItemCustomizationInfo(systems::ItemCustomizationInfo),
    303 => StickerbookInfo(systems::StickerbookInfo),
    304 => StickerbookItemInfo(systems::StickerbookItemInfo),
    224 => EventProgressInfo(systems::EventProgressInfo),
    223 => EventParticipationInfo(systems::EventParticipationInfo),
    225 => EventProgressChanged(systems::EventProgressChanged),
    131 => TimedChallengesInfo(systems::TimedChallengesInfo),
    149 => DailyChallengeProgress(systems::DailyChallengeProgress),
    360 => WantedThiefInfo(systems::WantedThiefInfo),
    354 => SeasonPassRewardsInfo(systems::SeasonPassRewardsInfo),
    358 => SeasonPassMissionsInfo(systems::SeasonPassMissionsInfo),
    363 => KickOutToHubTimeLeft(systems::KickOutToHubTimeLeft),
    364 => KillScore(systems::KillScore),
    367 => EndlessTowerProgress(systems::EndlessTowerProgress),
    369 => EndlessTowerBuffs(systems::EndlessTowerBuffs),
    236 => HonorRankingPage(systems::HonorRankingPage),
    241 => OverallRankingPage(systems::OverallRankingPage),
    243 => LocalRanking(systems::LocalRanking),
    245 => OverallLocalRanking(systems::OverallLocalRanking),
    127 => HonorPointsChanged(systems::HonorPointsChanged),
    193 => MatchOver(systems::MatchOver),
    194 => MatchStart(systems::MatchStart),
    195 => MatchCountdown(systems::MatchCountdown),
    196 => MatchInfo(systems::MatchInfo),
    202 => MatchDescription(systems::MatchDescription),
    203 => SetBattlegroundId(systems::SetBattlegroundId),
    204 => ShowPvpDeathDialog(systems::ShowPvpDeathDialog),
    207 => JoinMatchQueueResponse(systems::JoinMatchQueueResponse),
    208 => JoinMatchQueueInfo(systems::JoinMatchQueueInfo),
    210 => JoinBattleground(systems::JoinBattleground),
    215 => MatchMakingInfo(systems::MatchMakingInfo),
    // trade.rs: a DSOR addition (no game client has it).
    0x0F10 => TradeState(trade::TradeState),
}

impl ServerCommand {
    /// The class name the 2018 client registers for this command.
    pub fn name(&self) -> &'static str {
        command_name(self.id()).unwrap_or("?")
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }
}

/// Decode a whole server game message (0x84 or 0x85) of `bits` bits.
///
/// Returns each command with its actor (None for commands that carry none).
/// CONTRACT: the stream is read to its end; more than PADDING_BITS left over is an
///   error, never silently dropped.
pub fn decode_message(payload: &[u8], bits: usize) -> Result<Vec<(ServerCommand, Option<u32>)>, DecodeError> {
    let mut r = BitReader::with_bit_len(payload, bits);
    let envelope = r.read_u8().map_err(|_| DecodeError::Empty)?;
    match envelope {
        SINGLE => {
            let id = r.read_u16()?;
            let at = r.position();
            let wrap = |error| DecodeError::InCommand { id, index: 0, at, error: Box::new(error) };
            let scoped = actor_scoped(id);
            let command = match ServerCommand::decode_body(id, &mut r) {
                Some(c) => c.map_err(wrap)?,
                None => {
                    let tail = if scoped { 32 } else { 0 };
                    let n = r.remaining().checked_sub(tail).ok_or_else(|| {
                        wrap(DecodeError::Overrun(Overrun { wanted: tail, at, length: bits }))
                    })?;
                    ServerCommand::Unknown { id, body: Raw::read(&mut r, n)? }
                }
            };
            let mut command = command;
            let actor = if scoped { Some(r.read_u32().map_err(|e| wrap(e.into()))?) } else { None };
            command.decode_actor_slot_tail(&mut r).map_err(wrap)?;
            if r.remaining() > PADDING_BITS {
                return Err(wrap(DecodeError::Trailing { bits: r.remaining() }));
            }
            Ok(vec![(command, actor)])
        }
        CHAIN => {
            let mut out = Vec::new();
            decode_chain(&mut r, &mut out)?;
            Ok(out)
        }
        other => Err(DecodeError::NotACommandMessage(other)),
    }
}

/// `decode_message`, keeping what a 0x85 chain held before the command that failed:
/// the commands read so far, and the error if one stopped it.
/// CONTRACT: the first commands of a chain stand on their own (a player
///   initialization batch starts with NewPlayer); losing the whole chain to one
///   later command without a layout left the player unspawned.
pub fn decode_message_partial(payload: &[u8], bits: usize) -> (Vec<(ServerCommand, Option<u32>)>, Option<DecodeError>) {
    if payload.first() != Some(&CHAIN) {
        return match decode_message(payload, bits) {
            Ok(v) => (v, None),
            Err(e) => (Vec::new(), Some(e)),
        };
    }
    let mut r = BitReader::with_bit_len(payload, bits);
    let _ = r.read_u8();
    let mut out = Vec::new();
    let error = decode_chain(&mut r, &mut out).err();
    (out, error)
}

/// The commands of a 0x85 chain after its envelope byte, appended to `out` as read.
fn decode_chain(r: &mut BitReader<'_>, out: &mut Vec<(ServerCommand, Option<u32>)>) -> Result<(), DecodeError> {
    while out.is_empty() || r.remaining() > PADDING_BITS {
        let id = r.read_u16()?;
        let at = r.position();
        let index = out.len();
        let wrap = |error| DecodeError::InCommand { id, index, at, error: Box::new(error) };
        let scoped = actor_scoped(id);
        // A layout we have, read to its terminator; if it does not fit (a release261
        // shape we have not ported: offers, challenges...), the command is kept as
        // Unknown up to its tail like one we have no layout for, and the chain
        // goes on. CONTRACT: only the chain's own framing ([actor] 0xFF) is trusted.
        let mut try_known = r.clone();
        let known = ServerCommand::decode_body(id, &mut try_known).map(|c| {
            c.and_then(|mut command| {
                let actor = if scoped { Some(try_known.read_u32()?) } else { None };
                command.decode_actor_slot_tail(&mut try_known)?;
                let end = try_known.read_u8()?;
                if end != TERMINATOR {
                    return Err(DecodeError::BadTerminator { got: end });
                }
                Ok((command, actor))
            })
        });
        match known {
            Some(Ok(done)) => {
                *r = try_known;
                out.push(done);
                continue;
            }
            Some(Err(e)) => log::debug!("command {id} at bit {at}: {e}; kept as unknown"),
            None => {}
        }
        let n = find_tail(r, scoped).ok_or_else(|| wrap(DecodeError::Trailing { bits: r.remaining() }))?;
        let command = ServerCommand::Unknown { id, body: Raw::read(r, n)? };
        let actor = if scoped { Some(r.read_u32().map_err(|e| wrap(e.into()))?) } else { None };
        let end = r.read_u8().map_err(|e| wrap(e.into()))?;
        if end != TERMINATOR {
            return Err(wrap(DecodeError::BadTerminator { got: end }));
        }
        out.push((command, actor));
    }
    Ok(())
}

/// How many body bits precede an unknown command's tail: the first position where
/// `[u32 actor if scoped][0xFF]` is followed by the end of the stream or by a command
/// id the client would accept. A heuristic; only used for ids with no layout.
fn find_tail(r: &BitReader<'_>, scoped: bool) -> Option<usize> {
    let tail = if scoped { 40 } else { 8 };
    let total = r.remaining();
    for n in 0..=total.checked_sub(tail)? {
        let mut probe = r.clone();
        probe.seek(r.position() + n + tail - 8);
        if probe.read_u8().ok()? != TERMINATOR {
            continue;
        }
        let left = probe.remaining();
        if left <= PADDING_BITS {
            return Some(n);
        }
        if left < 16 {
            continue;
        }
        let next = probe.read_u16().ok()?;
        if next < MOST_COMMANDS || next == BUILTIN_CHAT {
            return Some(n);
        }
    }
    None
}

/// A 0x84 message: one command and its actor (written only when scoped).
pub fn encode_single(command: &ServerCommand, actor: u32) -> (Vec<u8>, usize) {
    let mut w = BitWriter::new();
    w.write_u8(SINGLE);
    w.write_u16(command.id());
    command.encode_body(&mut w);
    if actor_scoped(command.id()) {
        w.write_u32(actor);
    }
    command.encode_actor_slot_tail(&mut w);
    w.finish()
}

/// A 0x85 message: each command, its actor when scoped, and 0xFF.
pub fn encode_chain(commands: &[(ServerCommand, u32)]) -> (Vec<u8>, usize) {
    let mut w = BitWriter::new();
    w.write_u8(CHAIN);
    for (command, actor) in commands {
        w.write_u16(command.id());
        command.encode_body(&mut w);
        if actor_scoped(command.id()) {
            w.write_u32(*actor);
        }
        command.encode_actor_slot_tail(&mut w);
        w.write_u8(TERMINATOR);
    }
    w.finish()
}
