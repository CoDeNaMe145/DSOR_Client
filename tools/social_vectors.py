"""Group and guild fixtures from the local server's real codecs (crates/proto/tests/social_release261.rs).

Usage: python tools/social_vectors.py <server> <output-directory>

Server -> client messages are encoded with the writers services/social.py sends
(protocol/social_protocol.py). Client -> server bytes are built with the server's
BitWriter in its decoders' order and checked by those decoders. Only pure
protocol modules are imported; no player database is opened.
"""
from pathlib import Path
import json
import sys
from prod_vectors import load_package

load_package(Path(sys.argv[1]).resolve())
from dso_server.protocol.character_selection import BitWriter
from dso_server.protocol import social_protocol as sp

out = Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
vectors = []
def record(name, packet):
    data, bits = packet; vectors.append(dict(name=name, hex=data.hex(), bits=bits))

leader = sp.PlayerSummary(0, 0, 11, 1, "Leader", "a0200_kingscity", 1, 45, role=0)
member = sp.PlayerSummary(0, 0, 22, 0, "Member", "a0100_map", 2, 30, online=False, available=False,
                          group_invitable=False, role=1, timestamp="2026-10-20 01:02:03")
record("group_status", sp.encode_group_status(sp.GroupStatus(0, 7, 11, 11, True, (leader, member), "Leader's group", False, "desc")))
record("no_group", sp.encode_group_status(sp.GroupStatus(0, 0, 0, 11, False, (), "", False, "")))
record("group_made", sp.encode_create_group_result(1, 5, "name taken"))
record("group_answer", sp.encode_invitation_response(sp.GROUP_INVITATION_RESPONSE_COMMAND, 0, "Joiner", 22, 7, actor_id=1))
record("guild_answer", sp.encode_invitation_response(sp.GUILD_INVITATION_RESPONSE_COMMAND, 2, "Joiner", 22, 3, actor_id=1))
record("grouping_failed", sp.encode_grouping_failed(2, "Joiner", 9))
record("guild_made", sp.encode_guild_foundation_result(3, 5, "name in use", "Knights", False))
knight = sp.GuildMember(11, 1, "Leader", 1, 45, True, True, False, 2, (2026, 1, 2, 3, 4, 5))
officer = sp.GuildMember(22, 0, "Officer", 3, 40, False, False, False, 1, (2025, 12, 31, 23, 59, 58))
record("guild_status", sp.encode_guild_status(sp.GuildStatus(0, 3, "Knights", 11, True, 11, 2, (knight, officer))))
record("no_guild", sp.encode_guild_status(sp.GuildStatus(0, 0, "", 11, False, 0, 0, ())))

def client(command_id, write):
    w = BitWriter(); w.write_uint8(0x8B); w.write_bytes(command_id.to_bytes(2, "little")); write(w); return w.finish()

def found(w): w.write_uint32_le(0); w.write_string("Leader"); w.write_string("Knights"); w.write_bool(True)
for name, command_id in [("found_group", sp.CREATE_GROUP_COMMAND), ("found_guild", sp.CREATE_GUILD_COMMAND)]:
    packet = client(command_id, found)
    request = sp.decode_foundation_request(packet[0], command_id, packet[1])
    assert (request.source_name, request.name, request.field_a8) == ("Leader", "Knights", True), request
    record(name, packet)
def invite(w): w.write_string("Leader"); w.write_string("Friend"); w.write_uint32_le(0); w.write_uint32_le(0); w.write_string("come")
packet = client(sp.GUILD_INVITE_COMMAND, invite)
request = sp.decode_guild_invite(packet[0], packet[1])
assert (request.source_name, request.target_name, request.message) == ("Leader", "Friend", "come"), request
record("guild_invite", packet)
def answer(w): w.write_uint8(0); w.write_uint32_le(11); w.write_uint32_le(3); w.write_string("Joiner")
packet = client(sp.GUILD_INVITATION_RESPONSE_COMMAND, answer)
request = sp.decode_invitation_response(packet[0], sp.GUILD_INVITATION_RESPONSE_COMMAND, packet[1])
assert (request.status, request.inviter_character_id, request.collective_id, request.player_name) == (0, 11, 3, "Joiner"), request
record("guild_accept", packet)
for name, command_id in [("leave_group", sp.GROUP_LEAVE_COMMAND), ("leave_guild", sp.GUILD_LEAVE_COMMAND)]:
    packet = client(command_id, lambda w: w.write_uint32_le(0))
    assert sp.decode_uint32_request(packet[0], command_id, packet[1]) == 0
    record(name, packet)

(out / "social_release261.json").write_text(json.dumps(vectors, indent=2) + "\n", encoding="utf-8")
print(f"wrote {len(vectors)} vectors")
