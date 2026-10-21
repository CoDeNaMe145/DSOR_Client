//! The items, stickerbook, events, dailies, Wanted Thief, ranking, PvP and premium
//! shop commands against the release 261 server (tools/systems_vectors.py): its
//! answers as it writes them, the requests as its decoders read them.
use dsor_proto::commands::systems::*;
use dsor_proto::commands::{decode_message, encode_chain, encode_single, ClientCommand, ServerCommand};
use serde_json::Value;

fn vectors() -> Vec<Value> {
    serde_json::from_str(include_str!("data/systems_release261.json")).unwrap()
}

fn bytes(v: &Value) -> (Vec<u8>, usize) {
    let hex = v["hex"].as_str().unwrap();
    let bytes = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
    (bytes, v["bits"].as_u64().unwrap() as usize)
}

/// A server message decoded whole, every command known, and written back the same.
fn decoded(name: &str) -> Vec<(ServerCommand, Option<u32>)> {
    let v = vectors().into_iter().find(|v| v["name"] == name).unwrap_or_else(|| panic!("no vector {name}"));
    let (data, bits) = bytes(&v);
    let commands = decode_message(&data, bits).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(commands.iter().all(|(c, _)| !c.is_unknown()), "{name}: {commands:?}");
    let again = if data[0] == 0x84 {
        encode_single(&commands[0].0, commands[0].1.unwrap_or(0))
    } else {
        let chain: Vec<_> = commands.iter().map(|(c, actor)| (c.clone(), actor.unwrap_or(0))).collect();
        encode_chain(&chain)
    };
    assert_eq!(again, (data, bits), "{name}");
    commands
}

#[test]
fn item_system_answers_decode() {
    let (ServerCommand::GemRemovalResponse(g), actor) = &decoded("gem_removal")[0] else { panic!() };
    assert_eq!((g.gem, g.replaced, *actor), (5001, 400, Some(1)));
    let (ServerCommand::CraftingResult(c), _) = &decoded("crafting_result")[0] else { panic!() };
    assert_eq!((c.status, c.items.len(), c.items[0].template.as_str(), c.result_type, c.relations.as_slice(), c.unknown_u32_0), (0, 1, "gem_ruby_b", 3, &[(900, 801)][..], 7));
    let (ServerCommand::RecipeBookInfo(r), _) = &decoded("recipe_book")[0] else { panic!() };
    assert_eq!(r.recipes, [("recipe_a".to_owned(), None), ("recipe_b".to_owned(), Some("x".to_owned()))]);
    let (ServerCommand::ItemCustomizationInfo(i), _) = &decoded("customization_info")[0] else { panic!() };
    assert_eq!(i.items[1], ("item_b".to_owned(), Some("skin_b".to_owned())));
    let (ServerCommand::StickerbookInfo(s), _) = &decoded("stickerbook")[0] else { panic!() };
    assert_eq!(s.kind, 2);
    assert_eq!(s.entries.iter().map(|e| e.as_ref().map(|e| (e.state, e.id.as_str()))).collect::<Vec<_>>(), [Some((2, "pet_a")), None, Some((3, "mount_b"))]);
    let (ServerCommand::StickerbookItemInfo(s), _) = &decoded("stickerbook_item")[0] else { panic!() };
    assert_eq!((s.operation, s.entry.as_ref().unwrap().id.as_str(), s.related.as_str()), (3, "pet_b", "pet_a"));
}

#[test]
fn event_daily_wanted_and_ranking_answers_decode() {
    let (ServerCommand::EventProgressInfo(e), _) = &decoded("event_progress")[0] else { panic!() };
    assert_eq!(e.events, [(5, [1, 2, 3, 4])]);
    let (ServerCommand::EventParticipationInfo(e), _) = &decoded("event_participation")[0] else { panic!() };
    assert_eq!(e.events, [(5, true)]);
    let (ServerCommand::EventProgressChanged(e), _) = &decoded("event_changed")[0] else { panic!() };
    assert_eq!(e.values, [5, 1, 2, 3, 4]);
    let (ServerCommand::TimedChallengesInfo(c), _) = &decoded("challenges")[0] else { panic!() };
    assert_eq!((c.daily.len(), c.daily[0].id.as_str(), c.daily[0].state, c.daily[0].progress, c.daily[0].timestamp[0], c.bonuses.len()), (1, "dc_a", 2, 3, 2026, 0));
    assert_eq!((c.comeback.len(), c.comeback[0].id.as_str(), c.comeback[0].timestamp[2]), (1, "cb_a", 22));
    let (ServerCommand::DailyChallengeProgress(p), _) = &decoded("challenge_progress")[0] else { panic!() };
    assert_eq!((p.id.as_str(), p.state, p.progress, p.timestamp[2]), ("dc_a", 3, 5, 21));
    let (ServerCommand::WantedThiefInfo(w), actor) = &decoded("wanted_info")[0] else { panic!() };
    assert_eq!((w.remaining, w.special_remaining, w.missions.as_slice(), w.statuses.as_slice(), w.vip_expires[0], *actor), (2, 1, &[11, 12, 13, 14][..], &[0, 1, 0, 0][..], 2026, Some(0)));
    let (ServerCommand::HonorRankingPage(p), actor) = &decoded("ranking_page")[0] else { panic!() };
    assert_eq!((p.board, p.total, p.context, p.rows.len(), p.rows[0].name.as_str(), p.rows[0].score, p.rows[0].kills, p.class, p.flags, *actor), (4, 1, 5, 1, "Hero", 100, 9, 4, 1, Some(1)));
    let (ServerCommand::OverallRankingPage(p), _) = &decoded("overall_page")[0] else { panic!() };
    assert_eq!((p.total, p.rows[0].0.as_str(), p.rows[0].1, p.class), (1, "Hero", 100, 4));
    let (ServerCommand::LocalRanking(l), _) = &decoded("local_ranking")[0] else { panic!() };
    assert_eq!((l.context, l.board, l.values[0]), (5, 4, 100));
    let (ServerCommand::OverallLocalRanking(l), _) = &decoded("overall_local_ranking")[0] else { panic!() };
    assert_eq!(l.context, 5);
}

#[test]
fn pvp_answers_decode() {
    let (ServerCommand::MatchOver(m), actor) = &decoded("match_over")[0] else { panic!() };
    assert_eq!((m.tick, m.winning_team, m.flag, *actor), (100, 2, true, None));
    let (ServerCommand::MatchStart(m), _) = &decoded("match_start")[0] else { panic!() };
    assert_eq!((m.round_start, m.duration, m.flag), (100, 3000, false));
    assert!(matches!(decoded("match_countdown")[0].0, ServerCommand::MatchCountdown(_)));
    let (ServerCommand::MatchInfo(m), _) = &decoded("match_info")[0] else { panic!() };
    let p = &m.teams[0].players[0];
    assert_eq!((m.round_start, m.teams.len(), m.teams[0].id, m.teams[0].win_chance, m.battleground.as_str(), m.stage), (100, 1, 1, 0.5, "bg_1", 2));
    assert_eq!((p.values[0], p.values[6], p.values[7], p.values[9], p.support, p.rating, p.level, p.enemy), (7, 2, 1, 3, 1.5, 1200, 40, false));
    assert_eq!((p.name.as_str(), p.class_id, p.stats.as_slice(), p.rounds_lost), ("Hero", 2, &[1, 2, 3][..], 1));
    let (ServerCommand::MatchDescription(m), _) = &decoded("match_description")[0] else { panic!() };
    assert_eq!((m.active, m.value), (true, 4));
    let (ServerCommand::SetBattlegroundId(b), _) = &decoded("battleground_id")[0] else { panic!() };
    assert_eq!(b.id, "bg_1");
    let (ServerCommand::JoinMatchQueueResponse(q), actor) = &decoded("queue_response")[0] else { panic!() };
    assert_eq!((q.result, q.accepted, *actor), (0, true, Some(1)));
    let (ServerCommand::JoinMatchQueueInfo(q), _) = &decoded("queue_info")[0] else { panic!() };
    assert_eq!((q.tick, q.estimated_wait, q.state, q.queued, q.queued_at[0]), (100, 12.5, 1, 3, 2026));
    let (ServerCommand::JoinBattleground(j), _) = &decoded("join_battleground")[0] else { panic!() };
    assert_eq!((j.tick, j.dialog, j.battleground, j.map.as_str(), j.mode), (100, 9, 3, "pvp_map", 0));
    let (ServerCommand::MatchMakingInfo(m), _) = &decoded("matchmaking_info")[0] else { panic!() };
    assert_eq!((m.state, m.tick, m.mode, m.message.as_str(), m.queued()), (1, 100, 0, "queued", true));
    let (ServerCommand::HonorPointsChanged(h), actor) = &decoded("honor_points")[0] else { panic!() };
    assert_eq!((h.honor, *actor), (150, Some(1)));
    let (ServerCommand::ShowPvpDeathDialog(d), actor) = &decoded("pvp_death")[0] else { panic!() };
    assert_eq!((d.respawn_delay, *actor), (125, Some(1)));
}

/// The release261_192 additions: the Season Pass, the Endless Tower and the screen
/// message, at their new ids.
#[test]
fn season_pass_and_endless_tower_answers_decode() {
    let (ServerCommand::SeasonPassRewardsInfo(s), actor) = &decoded("season_pass_rewards")[0] else { panic!() };
    assert_eq!((s.unknown_u32_20, s.unknown_u32_24, s.pass_types.as_slice(), s.claimed_awards.as_slice(), *actor), (3, 4, &[1, 2][..], &[7][..], Some(0)));
    let (ServerCommand::SeasonPassMissionsInfo(s), _) = &decoded("season_pass_missions")[0] else { panic!() };
    assert_eq!(s.missions, [(5, 9), (6, 0)]);
    let (ServerCommand::KickOutToHubTimeLeft(k), actor) = &decoded("kick_out_time_left")[0] else { panic!() };
    assert_eq!((k.seconds, *actor), (90, Some(1)));
    let (ServerCommand::KillScore(k), _) = &decoded("kill_score")[0] else { panic!() };
    assert_eq!(k.score, 1234);
    let (ServerCommand::EndlessTowerProgress(p), _) = &decoded("endless_tower_progress")[0] else { panic!() };
    assert_eq!((p.cleared_floor, p.score, p.claimed_floor), (12, 0, 10));
    let (ServerCommand::EndlessTowerBuffs(b), _) = &decoded("endless_tower_buffs")[0] else { panic!() };
    assert_eq!(b.buffs, ["buff_a", "buff_b"]);
    let (ServerCommand::ScreenMessage(m), actor) = &decoded("screen_message")[0] else { panic!() };
    assert_eq!((m.kind, m.text.as_str(), m.extra.as_str(), *actor), (0, "Wave 2", "", Some(0)));
    for (name, id) in [("season_pass_rewards", 354), ("kill_score", 364), ("endless_tower_buffs", 369), ("screen_message", 381)] {
        assert_eq!(decoded(name)[0].0.id(), id, "{name}");
    }
}

#[test]
fn requests_encode_as_the_server_decodes_them() {
    let mut seen = 0;
    for v in vectors() {
        let command = match v["name"].as_str().unwrap() {
            "salvage" => ClientCommand::Salvage(Salvage { items: vec![11, 12], mode: 0 }),
            "crafting" => ClientCommand::Crafting(Crafting { unknown_u8_0: 0, items: vec![11, 12], currency: 0, slot: 1, amount: 1, recipe: "recipe_a".into(), mode: 0, unknown_u32_1: 0, unknown_u32_2: 0 }),
            "customize" => ClientCommand::ApplyItemCustomization(ApplyItemCustomization { changes: vec![CustomizationChange { slot: 3, apply_skin: "skin_x".into(), skin: "skin_x".into(), ..Default::default() }] }),
            "sticker_add" => ClientCommand::AddStickerbookFromInventory(AddStickerbookFromInventory { item: 800 }),
            "sticker_favorite" => ClientCommand::SetStickerbookFavorite(SetStickerbookFavorite { id: "pet_a".into(), favorite: true }),
            "sticker_upgrade" => ClientCommand::UpgradeStickerbookItem(UpgradeStickerbookItem { id: "pet_a".into() }),
            "set_title" => ClientCommand::SetTitle(SetTitle { kind: 1, title: "title_x".into() }),
            "active_events" => ClientCommand::ActiveEvents(ActiveEvents),
            "participation" => ClientCommand::SetEventParticipation(SetEventParticipation { event: 5, participating: true }),
            "challenge_selection" => ClientCommand::DailyChallengeSelection(DailyChallengeSelection { id: "dc_a".into(), refresh: true }),
            "wanted_get" => ClientCommand::WantedThiefInfoRequest(WantedThiefInfoRequest),
            "wanted_accept" => ClientCommand::AcceptWantedThiefMission(WantedThiefSlot { slot: 2 }),
            "wanted_refresh" => ClientCommand::RefreshWantedThiefMission(WantedThiefSlot { slot: 0 }),
            "rank_request" => ClientCommand::HonorRankingRequest(HonorRankingRequest { board: 4, start: 0xFFFFFFFF, count: 10, context: 5, flags: 1, class: 4, sort: 0 }),
            "overall_request" => ClientCommand::OverallRankingRequest(OverallRankingRequest { start: 0, count: 10, context: 5, flags: 1, class: 4 }),
            "local_request" => ClientCommand::LocalRankingRequest(LocalRankingRequest { context: 5, board: 4 }),
            "overall_local_request" => ClientCommand::OverallLocalRankingRequest(OverallLocalRankingRequest { context: 5 }),
            "ocp_request" => ClientCommand::OcpOfferRequest(OcpOfferRequest),
            "ocp_booking" => ClientCommand::OcpBooking(OcpBooking { offer: "1_realCurrency_100_0_NONE".into(), method: "external".into() }),
            "tier_spender" => ClientCommand::AcceptedTierSpender(AcceptedTierSpender { rewards: vec!["spender_a".into(), "spender_b".into()] }),
            "abort_queue" => ClientCommand::AbortMatchQueue(AbortMatchQueue { tick: 100, flag: true, modes: vec![0, 1], all: false }),
            "battleground_response" => ClientCommand::JoinBattlegroundResponse(JoinBattlegroundResponse { tick: 100, battleground: 3, accepted: true, unknown_u32_0: 0, mode: 0 }),
            "leave_pvp" => ClientCommand::LeavePvp(LeavePvp { leave: true }),
            _ => continue,
        };
        seen += 1;
        let (want, want_bits) = bytes(&v);
        let (got, got_bits) = command.encode();
        assert_eq!(got, want, "{}", v["name"]);
        assert!(got_bits == want_bits || got_bits.div_ceil(8) == want_bits.div_ceil(8), "{}: {got_bits} vs {want_bits} bits", v["name"]);
    }
    assert_eq!(seen, 23);
}
