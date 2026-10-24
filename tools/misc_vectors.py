"""Travel, talent, logout and PvP queue fixtures from the local server's codecs (crates/proto/tests/misc_release261.rs).

Usage: python tools/misc_vectors.py <server> <output-directory>

Client -> server bytes come from the server's own encoders where it has one
(talents), else from its BitWriter in its decoder's order, and are checked by
that decoder. Server -> client messages use the writers it sends with. Only pure
protocol modules are imported; no player database is opened.
"""
from pathlib import Path
import json
import sys
from prod_vectors import load_package

load_package(Path(sys.argv[1]).resolve())
from dso_server.protocol.character_selection import BitWriter
from dso_server.protocol import gameplay_protocol as gp
from dso_server.protocol import talent_protocol as tp
from dso_server.protocol import pvp_match_protocol as pvp
from dso_server.protocol import player_init_protocol as init

out = Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
vectors = []
def record(name, packet):
    data, bits = packet; vectors.append(dict(name=name, hex=data.hex(), bits=bits))

def client(command_id, write):
    w = BitWriter(); w.write_uint8(0x8B); w.write_bytes(command_id.to_bytes(2, "little")); write(w); return w.finish()

# Travel: the travel map's selection (empty source and entry) and an event's Start.
def travel(source, map_id, entry):
    def write(w): w.write_string(source); w.write_string(map_id); w.write_string(entry); w.write_bool(False)
    return client(gp.MAP_TRAVEL_COMMAND, write)
packet = travel("", "a0200_kingscity", "")
request = gp.decode_map_travel_request(packet[0], packet[1])
assert request.map_id == "a0200_kingscity" and not getattr(request, "entry_name", None) and not getattr(request, "travel_point_id", ""), request
record("travel_map", packet)
packet = travel("", "a0200_kingscity", "a0200_fairground")
request = gp.decode_map_travel_request(packet[0], packet[1])
assert request.map_id == "a0200_kingscity" and request.entry_name == "a0200_fairground", request
record("travel_event", packet)

packet = client(gp.USE_TRAVEL_ITEM_COMMAND, lambda w: (w.write_uint32_le(800), w.write_string("a0203_kingscrypt02")))
request = gp.decode_use_travel_item_request(packet[0])
assert (request.item_id, request.map_id) == (800, "a0203_kingscrypt02"), request
record("use_travel_item", packet)

packet = client(gp.LOGOUT_COMMAND, lambda w: (w.write_bool(False), w.write_bool(False)))
assert gp.decode_logout(packet[0], packet[1]) == gp.Logout(False, False)
record("logout", packet)
record("logout_info", gp.encode_player_logout_info(10))

# Talents: the server encodes these client commands itself.
record("select_talent", tp.encode_select_talent(tp.SelectTalent("skill_mine_lvl", 1)))
assert tp.decode_select_talent(*tp.encode_select_talent(tp.SelectTalent("skill_mine_lvl", 1))).rank_delta == 1
record("preselection", tp.encode_select_talent_preselection(tp.SelectTalentPreselection(2, 0)))
record("reset_talents", tp.encode_reset_talent_preselection(tp.ResetTalentPreselection(1, 2)))
record("reset_branch", tp.encode_reset_talent_category(tp.ResetTalentCategory(1, "wisdom_branch_a", 0)))
points = next(getattr(init, n) for n in dir(init) if n.startswith("TALENT_CATEGORY_POINTS_INFO") and n.endswith("COMMAND"))
record("talent_points", init.encode_server_command_batch((
    init.ServerCommand(points, init.TalentCategoryPointsInfo(0xFF, (10, 20, 30, 40)), 1),
    init.ServerCommand(init.UNLOCK_SKILL_TALENT_PRESELECTION_INFO_COMMAND, init.UnlockSkillTalentPreselectionInfo(3), 1),
), include_terminator=True))

# Items received (MessageCommand type 2): a reward's item, then a currency the
# reward made no item of (its template added), as services send them.
from dso_server.protocol import item_receipt_protocol as receipts
record("item_received", init.encode_server_command_batch((
    init.ServerCommand(init.MESSAGE_COMMAND, receipts.item_received_message(receipts.ItemReceipt(4012, "dailylogin_lootbox_day_01", 1)), 1),
    init.ServerCommand(init.MESSAGE_COMMAND, receipts.item_received_message(receipts.ItemReceipt(4013, "money_hc_no_auto_use", 50, True)), 1),
), include_terminator=True))

packet = client(pvp.JOIN_MATCH_QUEUE_COMMAND, lambda w: (w.write_uint32_le(7), w.write_uint8(0), w.write_uint8(1)))
request = pvp.decode_join_match_queue(packet[0], packet[1])
assert (request.server_tick, request.match_mode, request.queue_kind) == (7, 0, 1), request
record("join_queue", packet)

(out / "misc_release261.json").write_text(json.dumps(vectors, indent=2) + "\n", encoding="utf-8")
print(f"wrote {len(vectors)} vectors")
