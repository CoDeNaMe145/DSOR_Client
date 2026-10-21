//! The commands the client sends: `[u8 0x8B][u16 id][body]`, no actor, no terminator.
//!
//! SOURCE: the experimental server's decoders of client commands (server.py and the
//!   dsor/*.py `decode_*` / `parse_*` functions), checked byte for byte against the
//!   captured client payloads in tests/commands_client.rs.

use dsor_raknet::{BitReader, BitWriter};

use super::wire::{Body, Raw};
use super::{actors, combat, inventory, movement, player, social, systems, trade, DecodeError, CLIENT, PADDING_BITS};

macro_rules! client_commands {
    ($($id:literal => $var:ident($ty:ty),)*) => {
        /// Every command the client sends, as the server reads it.
        #[derive(Debug, Clone, PartialEq)]
        pub enum ClientCommand {
            $($var($ty),)*
            /// A command with no layout here: the id and the body verbatim.
            Unknown { id: u16, body: Raw },
        }

        impl ClientCommand {
            pub fn id(&self) -> u16 {
                match self {
                    $(Self::$var(_) => $id,)*
                    Self::Unknown { id, .. } => *id,
                }
            }

            pub fn is_known(id: u16) -> bool {
                matches!(id, $($id)|*)
            }

            fn decode_body(id: u16, r: &mut BitReader<'_>) -> Option<Result<Self, DecodeError>> {
                Some(match id {
                    $($id => <$ty as Body>::decode(r).map(Self::$var),)*
                    _ => return None,
                })
            }

            fn encode_body(&self, w: &mut BitWriter) {
                match self {
                    $(Self::$var(c) => c.encode(w),)*
                    Self::Unknown { body, .. } => body.write(w),
                }
            }
        }
    };
}

client_commands! {
    // player.rs
    28 => ActorRequest(player::ActorRequest),
    35 => UpdateShowPlayerOverheadIcons(player::UpdateShowPlayerOverheadIcons),
    37 => SettingsInfo(player::SettingsInfo),
    96 => UnlockMap(player::UnlockMap),
    97 => Travel(player::Travel),
    99 => Logout(player::Logout),
    111 => StillAlive(player::StillAlive),
    134 => CharacterGeneration(player::CharacterGeneration),
    135 => CharacterSelection(player::CharacterSelection),
    180 => PlayerQuery(player::PlayerQuery),
    187 => GuildMessageOfTheDayRequest(player::GuildMessageOfTheDayRequest),
    226 => ChatClientLoggedIn(player::ChatClientLoggedIn),
    227 => ChatErrorReport(player::ChatErrorReport),
    232 => ClientAverageFrameInfoTracking(player::ClientAverageFrameInfoTracking),
    233 => ClientCurrentPositionFrameInfoTracking(player::ClientCurrentPositionFrameInfoTracking),
    234 => ClientLogEvent(player::ClientLogEvent),
    252 => OverallUserRankingInfo(player::OverallUserRankingInfo),
    254 => OverallLocalUserRankingInfo(player::OverallLocalUserRankingInfo),
    275 => StagingSignal(player::StagingSignal),
    299 => GauntletNotificationInfo(player::GauntletNotificationInfo),
    // actors.rs
    41 => NpcRequest(actors::NpcRequest),
    57 => PropInteraction(actors::PropInteraction),
    100 => PickupItem(actors::PickupItem),
    102 => Encounter(actors::Encounter),
    103 => Talk(actors::Talk),
    104 => Deliver(actors::Deliver),
    105 => Pay(actors::Pay),
    138 => Quest(actors::Quest),
    141 => QuestMonsterIndicationRequest(actors::QuestMonsterIndicationRequest),
    143 => QuestTriggerIndicationRequest(actors::QuestTriggerIndicationRequest),
    262 => DebugMonsterRequest(actors::DebugMonsterRequest),
    293 => QuestTriggerSignal(actors::QuestTriggerSignal),
    // movement.rs
    95 => Move(movement::Move),
    // combat.rs
    70 => Skill(combat::Skill),
    71 => TargetSkill(combat::TargetSkill),
    72 => BulletSkill(combat::BulletSkill),
    75 => TargetBulletSkill(combat::TargetBulletSkill),
    73 => TargetPointBulletSkill(combat::TargetPointBulletSkill),
    74 => ShiftedSkill(combat::ShiftedSkill),
    76 => SustainedSkill(combat::ClientSustainedSkill),
    98 => Respawn(combat::Respawn),
    110 => Resurrect(combat::Resurrect),
    291 => QuickResurrectGroupMember(combat::QuickResurrectGroupMember),
    // inventory.rs
    81 => QuickSlots(inventory::QuickSlots),
    85 => Inventory(inventory::Inventory),
    87 => CurrencyConversion(inventory::CurrencyConversion),
    89 => RequestOffer(inventory::RequestOffer),
    91 => Transaction(inventory::Transaction),
    121 => UseTravelItem(inventory::UseTravelItem),
    279 => SelectTalent(inventory::SelectTalent),
    280 => DeselectTalent(inventory::DeselectTalent),
    281 => TalentPreselection(inventory::TalentPreselection),
    286 => ResetTalents(inventory::ResetTalents),
    287 => ResetTalentCategory(inventory::ResetTalentCategory),
    309 => UseStickerBookItem(inventory::UseStickerBookItem),
    // social.rs
    136 => Notification(social::Notification),
    150 => GroupList(social::GroupList),
    154 => GroupInvite(social::GroupInvite),
    156 => GroupInvitationResponse(social::GroupInvitationResponse),
    159 => GroupKick(social::GroupKick),
    152 => GroupFoundation(social::Foundation),
    160 => GroupLeave(social::Leave),
    170 => GuildInvite(social::GuildInvite),
    171 => GuildInvitationResponse(social::GroupInvitationResponse),
    173 => GuildLeave(social::Leave),
    166 => GuildList(social::GuildList),
    168 => GuildFoundation(social::Foundation),
    176 => BuddyList(social::BuddyList),
    182 => GroupQuery(social::GroupQuery),
    192 => PvpFlag(social::PvpFlag),
    206 => JoinMatchQueue(social::JoinMatchQueue),
    // trade.rs
    // systems.rs: items, the stickerbook, titles, events, dailies, the Wanted
    // Thief, the rankings, PvP, the premium shop.
    49 => Salvage(systems::Salvage),
    92 => Crafting(systems::Crafting),
    322 => ApplyItemCustomization(systems::ApplyItemCustomization),
    305 => SetStickerbookFavorite(systems::SetStickerbookFavorite),
    307 => AddStickerbookFromInventory(systems::AddStickerbookFromInventory),
    308 => UpgradeStickerbookItem(systems::UpgradeStickerbookItem),
    277 => SetTitle(systems::SetTitle),
    222 => ActiveEvents(systems::ActiveEvents),
    120 => SetEventParticipation(systems::SetEventParticipation),
    148 => DailyChallengeSelection(systems::DailyChallengeSelection),
    359 => WantedThiefInfoRequest(systems::WantedThiefInfoRequest),
    361 => AcceptWantedThiefMission(systems::WantedThiefSlot),
    362 => RefreshWantedThiefMission(systems::WantedThiefSlot),
    235 => HonorRankingRequest(systems::HonorRankingRequest),
    240 => OverallRankingRequest(systems::OverallRankingRequest),
    242 => LocalRankingRequest(systems::LocalRankingRequest),
    244 => OverallLocalRankingRequest(systems::OverallLocalRankingRequest),
    267 => OcpOfferRequest(systems::OcpOfferRequest),
    271 => OcpBooking(systems::OcpBooking),
    313 => AcceptedTierSpender(systems::AcceptedTierSpender),
    209 => AbortMatchQueue(systems::AbortMatchQueue),
    211 => JoinBattlegroundResponse(systems::JoinBattlegroundResponse),
    199 => LeavePvp(systems::LeavePvp),
    0x0F00 => TradeHello(trade::TradeHello),
    0x0F01 => TradeRequest(trade::TradeRequest),
    0x0F02 => TradeAnswer(trade::TradeAnswer),
    0x0F03 => TradeOffer(trade::TradeOffer),
    0x0F04 => TradeReady(trade::TradeReady),
    0x0F05 => TradeCancel(trade::TradeCancel),
}

impl ClientCommand {
    /// The whole 0x8B message and its length in bits.
    pub fn encode(&self) -> (Vec<u8>, usize) {
        let mut w = BitWriter::new();
        w.write_u8(CLIENT);
        w.write_u16(self.id());
        self.encode_body(&mut w);
        w.finish()
    }

    /// Read a 0x8B message back (what the server does with it).
    /// CONTRACT: the body must end within PADDING_BITS of `bits`.
    pub fn decode(payload: &[u8], bits: usize) -> Result<Self, DecodeError> {
        let mut r = BitReader::with_bit_len(payload, bits);
        let envelope = r.read_u8().map_err(|_| DecodeError::Empty)?;
        if envelope != CLIENT {
            return Err(DecodeError::NotACommandMessage(envelope));
        }
        let id = r.read_u16()?;
        let at = r.position();
        let wrap = |error| DecodeError::InCommand { id, index: 0, at, error: Box::new(error) };
        let command = match Self::decode_body(id, &mut r) {
            Some(c) => c.map_err(wrap)?,
            None => {
                let n = r.remaining();
                Self::Unknown { id, body: Raw::read(&mut r, n)? }
            }
        };
        if r.remaining() > PADDING_BITS {
            return Err(wrap(DecodeError::Trailing { bits: r.remaining() }));
        }
        Ok(command)
    }

    /// The class name the 2018 client registers for this command.
    pub fn name(&self) -> &'static str {
        super::command_name(self.id()).unwrap_or("?")
    }
}
