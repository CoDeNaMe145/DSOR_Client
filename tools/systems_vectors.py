"""Fixtures for the items, stickerbook, events, dailies, Wanted Thief, rankings,
PvP and premium shop commands, from the local server's codecs
(crates/proto/tests/systems_release261.rs).

Usage: python tools/systems_vectors.py <server> <output-directory>

Server -> client messages use the encoders it sends with; client -> server bytes
use its own encoders where it has them (stickerbook, PvP), else its BitWriter in
its decoders' order, checked by those decoders. Only pure protocol modules are
imported; no player database is opened.
"""
from pathlib import Path
import json
import sys
from prod_vectors import load_package

load_package(Path(sys.argv[1]).resolve())
from dso_server.protocol.character_selection import BitWriter
from dso_server.protocol.inventory_codec import ItemInfo
from dso_server.protocol import player_init_protocol as init
from dso_server.protocol import gameplay_protocol as gp
from dso_server.protocol import gem_protocol as gem
from dso_server.protocol import pvp_match_protocol as pvp
from dso_server.protocol import ranking_protocol as rk
from dso_server.protocol import wanted_thief_protocol as wt
from dso_server.protocol import daily_challenge_protocol as dc
from dso_server.protocol import reward_protocol as rw
from dso_server.protocol import inventory_economy_protocol as ie
from dso_server.protocol import item_customization_protocol as ic

out = Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
vectors = []
def record(name, packet):
    data, bits = packet; vectors.append(dict(name=name, hex=data.hex(), bits=bits))

def client(command_id, write):
    w = BitWriter(); w.write_uint8(0x8B); w.write_bytes(command_id.to_bytes(2, "little")); write(w); return w.finish()

def batch(name, command_id, value, actor=1):
    record(name, init.encode_server_command_batch((init.ServerCommand(command_id, value, actor),), include_terminator=True))

# ── items ───────────────────────────────────────────────────────────────────
record("gem_removal", gem.encode_gem_removal_response(5001, replaced_gem_item_id=400, actor_id=1))
batch("crafting_result", init.CRAFTING_RESULT_COMMAND,
      init.CraftingResult(0, (ItemInfo(900, amount=1, template_id="gem_ruby_b", item_level=12),), 3, ((900, 801),), 7))
batch("recipe_book", init.RECIPE_BOOK_INFO_COMMAND, init.RecipeBookInfo((init.RecipeBookEntry("recipe_a", None), init.RecipeBookEntry("recipe_b", "x"))))
batch("customization_info", init.ITEM_CUSTOMIZATION_INFO_COMMAND, init.ItemCustomizationInfo((init.ItemCustomization("item_a", None), init.ItemCustomization("item_b", "skin_b"))))
batch("stickerbook", init.STICKERBOOK_INFO_COMMAND, init.StickerbookInfo(2, (init.StickerbookEntry(2, 0, "pet_a"), None, init.StickerbookEntry(3, 0, "mount_b"))))
batch("stickerbook_item", init.STICKERBOOK_ITEM_INFO_COMMAND, init.StickerbookItemInfo(3, init.StickerbookEntry(2, 0, "pet_b"), "pet_a"))

packet = client(ie.SALVAGE_COMMAND, lambda w: (w.write_uint32_le(2), w.write_uint32_le(11), w.write_uint32_le(12), w.write_uint32_le(0)))
request = ie.decode_salvage_request(packet[0])
assert (tuple(request.item_ids), request.mode) == ((11, 12), 0), request
record("salvage", packet)
def crafting(w):
    w.write_uint8(0); w.write_uint32_le(2); w.write_uint32_le(11); w.write_uint32_le(12); w.write_uint8(0); w.write_uint8(1)
    w.write_uint32_le(1); w.write_string("recipe_a"); w.write_uint32_le(0); w.write_uint32_le(0); w.write_uint32_le(0)
packet = client(ie.CRAFTING_COMMAND, crafting)
request = ie.decode_crafting_request(packet[0])
assert (tuple(request.item_ids), request.recipe_id, request.amount, request.payment_currency, request.source_slot) == ((11, 12), "recipe_a", 1, 0, 1), request
record("crafting", packet)
def customize(w):
    w.write_uint32_le(1); w.write_uint32_le(3); w.write_uint32_le(1); w.write_uint32_le(6)
    for s in ("skin_x", "", "skin_x", "", "", ""): w.write_string(s)
packet = client(ic.APPLY_ITEM_CUSTOMIZATION_COMMAND, customize)
changes = ic.decode_item_customization_changes(packet[0])
assert len(changes) == 1 and changes[0].slot_type == 3 and changes[0].apply_skin == "skin_x", changes
record("customize", packet)
record("sticker_add", gp.encode_add_stickerbook_from_inventory(gp.AddStickerbookFromInventoryRequest(800)))
record("sticker_favorite", gp.encode_set_stickerbook_favorite(gp.SetStickerbookFavoriteRequest("pet_a", True)))
record("sticker_upgrade", gp.encode_upgrade_stickerbook_item(gp.UpgradeStickerbookItemRequest("pet_a")))

# ── titles, events, dailies, the Wanted Thief ───────────────────────────────
packet = client(gp.SET_TITLE_COMMAND, lambda w: (w.write_uint8(1), w.write_string("title_x")))
request = gp.decode_set_title_request(packet[0], packet[1])
assert (request.title_type, request.achievement_title) == (1, "title_x"), request
record("set_title", packet)
record("active_events", client(222, lambda w: None))
record("participation", client(120, lambda w: (w.write_uint32_le(5), w.write_bool(True))))
batch("event_progress", init.EVENT_PROGRESS_INFO_COMMAND, init.EventProgressInfo((init.EventProgressInfoEntry(5, (1, 2, 3, 4)),)))
batch("event_participation", init.EVENT_PARTICIPATION_INFO_COMMAND, init.EventParticipationInfo((init.EventParticipation(5, True),)))
batch("event_changed", init.EVENT_PROGRESS_CHANGED_COMMAND, init.EventProgressChanged((5, 1, 2, 3, 4)))
challenge = init.TimedChallenge("dc_a", "dc_a", 2, 3, (2026, 10, 21, 0, 0, 0))
comeback = init.TimedChallenge("cb_a", "cb_a", 0, 1, (2026, 10, 22, 0, 0, 0))
batch("challenges", init.TIMED_CHALLENGES_INFO_COMMAND, init.TimedChallengesInfo(0, (challenge,), (), (), (comeback,)))
batch("challenge_progress", init.DAILY_CHALLENGE_PROGRESS_COMMAND, init.TimedChallenge("dc_a", "dc_a", 3, 5, (2026, 10, 21, 0, 0, 0)))
packet = client(dc.DAILY_CHALLENGE_SELECTION_COMMAND, lambda w: (w.write_string("dc_a"), w.write_bool(True)))
request = dc.decode_daily_challenge_selection(packet[0], packet[1])
assert (request.challenge_id, request.refresh) == ("dc_a", True), request
record("challenge_selection", packet)
import datetime
vip = datetime.datetime(2026, 10, 20, 1, 2, 3, 4000, tzinfo=datetime.timezone.utc)
record("wanted_info", wt.encode_wanted_thief_info(wt.WantedThiefInfo(2, 1, (11, 12, 13, 14), (0, 1, 0, 0), vip), actor_id=0))
for name, command_id, write in [("wanted_get", wt.GET_WANTED_THIEF_INFO_COMMAND, lambda w: None),
                                ("wanted_accept", wt.ACCEPT_WANTED_THIEF_MISSION_COMMAND, lambda w: w.write_uint32_le(2)),
                                ("wanted_refresh", wt.REFRESH_WANTED_THIEF_MISSION_COMMAND, lambda w: w.write_uint32_le(0))]:
    packet = client(command_id, write)
    request = wt.decode_wanted_thief_request(packet[0], packet[1])
    assert request.command_id == command_id, request
    record(name, packet)

# ── the rankings ────────────────────────────────────────────────────────────
rows = (rk.RankingRow("Hero", 100, 0, 3, 1, 0, 9, 2),)
page = rk.RankingRequest(command_id=rk.HONOR_RANKING_INFO_COMMAND, start=0, count=10, context=5, flags=1, class_id=4, board=4, sort_column=0)
record("ranking_page", rk.encode_ranking_page(page, rows, start=0, total=1))
overall = rk.RankingRequest(command_id=rk.HONOR_OVERALL_USER_RANKING_INFO_COMMAND, start=0, count=10, context=5, flags=1, class_id=4, board=0xFF, sort_column=0)
record("overall_page", rk.encode_ranking_page(overall, rows, start=0, total=1))
local = rk.RankingRequest(command_id=rk.HONOR_LOCAL_USER_RANKING_INFO_COMMAND, start=0xFFFFFFFF, count=0, context=5, flags=0, class_id=4, board=4, sort_column=0)
record("local_ranking", rk.encode_local_ranking(local, rows[0]))
overall_local = rk.RankingRequest(command_id=rk.HONOR_OVERALL_LOCAL_USER_RANKING_INFO_COMMAND, start=0xFFFFFFFF, count=0, context=5, flags=0, class_id=4, board=0xFF, sort_column=0)
record("overall_local_ranking", rk.encode_local_ranking(overall_local, None))
for name, command_id, write in [
    ("rank_request", rk.HONOR_RANKING_INFO_COMMAND, lambda w: (w.write_uint8(4), w.write_uint32_le(0xFFFFFFFF), w.write_uint32_le(10), w.write_uint32_le(5), w.write_bytes((1).to_bytes(2, "little")), w.write_uint8(4), w.write_uint8(0))),
    ("overall_request", rk.HONOR_OVERALL_USER_RANKING_INFO_COMMAND, lambda w: (w.write_uint32_le(0), w.write_uint32_le(10), w.write_uint32_le(5), w.write_bytes((1).to_bytes(2, "little")), w.write_uint8(4))),
    ("local_request", rk.HONOR_LOCAL_USER_RANKING_INFO_COMMAND, lambda w: (w.write_uint32_le(5), w.write_uint8(4))),
    ("overall_local_request", rk.HONOR_OVERALL_LOCAL_USER_RANKING_INFO_COMMAND, lambda w: w.write_uint32_le(5)),
]:
    packet = client(command_id, write)
    request = rk.decode_ranking_request(packet[0], packet[1])
    assert request.command_id == command_id and request.context == 5, request
    record(name, packet)

# ── the premium shop ────────────────────────────────────────────────────────
packet = client(gp.OCP_OFFER_REQUEST_COMMAND, lambda w: w.write_bool(False))
assert gp.decode_simple_gameplay_command(packet[0]).value is False
record("ocp_request", packet)
packet = client(rw.OCP_BOOKING_COMMAND, lambda w: (w.write_string("1_realCurrency_100_0_NONE"), w.write_string("external")))
request = rw.decode_ocp_booking_request(packet[0])
assert (request.offer_id, request.payment_method) == ("1_realCurrency_100_0_NONE", "external"), request
record("ocp_booking", packet)
packet = client(rw.ACCEPTED_TIER_SPENDER_COMMAND, lambda w: (w.write_uint32_le(2), w.write_string("spender_a"), w.write_string("spender_b")))
assert rw.decode_tier_spender_accept(packet[0]) == ("spender_a", "spender_b")
record("tier_spender", packet)

# ── PvP ─────────────────────────────────────────────────────────────────────
record("match_over", pvp.encode_match_over(pvp.MatchOver(100, 2, True)))
record("match_start", pvp.encode_match_start(pvp.MatchStart(100, 3000, False)))
record("match_countdown", pvp.encode_match_countdown())
player = pvp.MatchPlayer(actor_id=7, honor_total=10, honor_rounds_won=11, honor_participation=12, honor_bonus=13,
                         rounds_won=14, kills=2, deaths=1, damage_dealt=500, score=3, support=1.5, rating=1200,
                         level=40, enemy=False, name="Hero", class_id=2, stats=(1, 2, 3), rounds_lost=1)
record("match_info", pvp.encode_match_info(pvp.MatchInfo(100, 3000, (pvp.MatchTeam(1, 2, 30, 0.5, (player,)),), "bg_1", 2)))
record("match_description", pvp.encode_match_description(pvp.MatchDescription(True, 4)))
record("battleground_id", pvp.encode_set_battleground_id(pvp.SetBattlegroundId("bg_1")))
record("queue_response", pvp.encode_join_match_queue_response(pvp.JoinMatchQueueResponse(0, True, 1)))
record("queue_info", pvp.encode_join_match_queue_info(pvp.JoinMatchQueueInfo(100, 12.5, 1, 3, pvp.CalendarTime(2026, 293, 1, 2, 3, 4, 2, 10))))
record("join_battleground", pvp.encode_join_battleground(pvp.JoinBattleground(100, 9, 3, "pvp_map", 0)))
record("matchmaking_info", pvp.encode_matchmaking_info(pvp.MatchMakingInfo(1, 100, 0, "queued")))
record("honor_points", pvp.encode_honor_points_changed(pvp.HonorPointsChanged(150, 1)))
record("pvp_death", pvp.encode_show_pvp_death_dialog(pvp.ShowPVPDeathDialog(125, 1)))
record("abort_queue", pvp.encode_abort_match_queue(pvp.AbortMatchQueue(100, True, (0, 1), False)))
record("battleground_response", pvp.encode_join_battleground_response(pvp.JoinBattlegroundResponse(100, 3, True, 0, 0)))
record("leave_pvp", pvp.encode_leave_pvp(pvp.LeavePVP(True)))

# ── the Season Pass, the Endless Tower, screen messages (release261_192) ────
batch("season_pass_rewards", init.SEASON_PASS_REWARDS_INFO_RESULT_COMMAND, init.SeasonPassRewardsInfoResult(3, 4, (1, 2), (7,)), 0)
batch("season_pass_missions", init.SEASON_PASS_MISSIONS_INFO_RESULT_COMMAND, init.SeasonPassMissionsInfoResult(((5, 9), (6, 0))), 0)
batch("kick_out_time_left", init.KICK_OUT_TO_HUB_LIMIT_TIME_LEFT_COMMAND, init.KickOutToHubLimitTimeLeft(90))
assert init.command_available(init.KILL_MONSTER_GET_TOTAL_SCORES_COMMAND)
record("kill_score", init.encode_kill_monster_get_total_scores(1234, actor_id=1))
record("endless_tower_progress", init.encode_endless_tower_progress(12, 10, actor_id=1))
record("endless_tower_buffs", init.encode_endless_tower_buffs(("buff_a", "buff_b"), actor_id=1))
record("screen_message", gp.encode_screen_message("Wave 2"))

(out / "systems_release261.json").write_text(json.dumps(vectors, indent=2) + "\n", encoding="utf-8")
print(f"wrote {len(vectors)} vectors")
