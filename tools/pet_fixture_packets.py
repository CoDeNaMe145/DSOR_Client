"""Pets around the offline player, as the server sends them, for screenshots (DSOR_PACKET_REPLAY).

Usage: python tools/pet_fixture_packets.py <server> <assets> <out.json> <shots-dir> [<map> [<x,y,z>]] [<static.db4>]

Writes [{frame, name, hex, bits}] plus capture/quit records: our frogman comes
out (its summon), we jump a few metres (a ForceTransform) and it walks after
us; we swap it for a dragon (the new effect, then the old one's end, in one
packet: the old pet's despawn and the new one's summon); a player arrives with
a phoenix, walks, and is revived with a cooldown (ReviveCommand's u32), then
loses the phoenix's effect (its revival despawn); the player leaves
(DiscardPlayer) and we send the dragon away. Every packet comes from the local
server's own builders and encoders (load_cosmetic_definition and
sticker_status_entry as _toggle_switchable_sticker uses them, its removal entry,
the remote player's spawn and status snapshot, encode_revive), as the server
sends them. Offline play stands the player at the map's centre (in Kingscity,
under a wagon): a ForceTransform first puts it on open ground (<x,y,z>,
default Kingscity's square). Frames are paced for about 170 frames a second.
Nothing here touches a player database.

Run: target/debug/dsor-client.exe <map> --play 1 0 with DSOR_PACKET_REPLAY=<out.json>
(DSOR_ASSETS=<assets>); the run ends after the last capture.
"""
from dataclasses import replace
from pathlib import Path
import json
import math
import sys
from prod_vectors import load_package

server, assets, out, shots = Path(sys.argv[1]).resolve(), Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4])
map_id = sys.argv[5] if len(sys.argv) > 5 else "a0200_kingscity"
base = tuple(float(v) for v in sys.argv[6].split(",")) if len(sys.argv) > 6 else (87.0, 1.0, -63.0)
database = Path(sys.argv[7]) if len(sys.argv) > 7 else server / "data" / "db" / "static.db4"
load_package(server)
from dso_server.protocol import gameplay_protocol as gp
from dso_server.protocol import social_protocol as sp
from dso_server.protocol.respawn_protocol import encode_revive
from dso_server.gameplay.stickerbook_state import load_cosmetic_definition, sticker_status_entry
from dso_server.services.presence_visuals import PresenceVisualService

centre = json.loads((assets / "maps" / f"{map_id}.map.json").read_text(encoding="utf-8"))["center"]
def at(dx, dz):
    return (base[0] + dx, base[1], base[2] + dz)
def frames(seconds):
    return round(seconds * 170)
def wire(position):
    return tuple(p - c for p, c in zip(position, centre))

records = []
def record(frame, name, packet):
    data, bits = packet
    records.append(dict(frame=frame, name=name, hex=data.hex(), bits=bits))
def capture(frame, name):
    records.append(dict(frame=frame, capture=str(shots / f"{name}.png")))

LOCAL, WALKER = 1, 0x103C1
tick = 1000

def pet_entry(item, instance, actor):
    definition = load_cosmetic_definition(database, item)
    assert definition is not None and definition.kind == "pet", item
    return sticker_status_entry(definition, instance_id=instance, start_tick=tick, actor_id=actor, active=True)

def removed(entry):
    # customization.py _toggle_switchable_sticker's end of the current effect.
    return replace(entry, has_details=False, values=(), field_38=False, field_3c=0xFF, shape_values=())

def transitions(*effects):
    # One packet, as _toggle_switchable_sticker sends its transitions.
    return gp.encode_monster_spawn_batch(gp.MonsterSpawnBatch(tuple(
        gp.StatusEffectServerCommand(gp.StatusEffect(False, (entry,)), LOCAL) for entry in effects)))

def moves(frame, actor, path, every=6):
    """Walk `actor` along `path`, a record per 4 ticks, then a stop."""
    global tick
    for (a, b) in zip(path, path[1:]):
        steps = max(1, round(math.dist(a, b) / 0.4))
        for i in range(1, steps + 1):
            p = tuple(a[k] + (b[k] - a[k]) * i / steps for k in range(3))
            yaw = math.atan2(-(b[0] - a[0]), -(b[2] - a[2]))
            tick += 4
            moving = not (b == path[-1] and i == steps)
            move = gp.Move.from_values(wire(p), 1.0 if moving else 0.0, yaw, yaw, tick, 4 if moving else 0)
            record(frame, f"move_{actor:x}_{tick}", gp.encode_move_batch(gp.MoveBatch((gp.MoveServerCommand(move, actor),))))
            frame += every
    return frame

# We stand on open ground; our frogman comes out behind us, to our left.
record(5, "player_placed", gp.encode_force_transform_batch(gp.ForceTransform(base, 0.0), actor_id=LOCAL))
frogman = pet_entry("pet_frogman", 0x500, LOCAL)
start = frames(1.0)
record(start, "frogman_out", transitions(frogman))
capture(start + frames(0.25), "01_frogman_summon")
capture(start + frames(1.5), "02_frogman_out")
# We jump 7 m on (a ForceTransform): it walks after us and stops short.
frame = start + frames(2.5)
record(frame, "player_jumps", gp.encode_force_transform_batch(gp.ForceTransform(at(5.0, 5.0), 0.8), actor_id=LOCAL))
capture(frame + frames(0.5), "03_frogman_following")
capture(frame + frames(2.5), "04_frogman_arrived")
# The dragon for the frogman: its effect first, then the frogman's end.
frame += frames(3.5)
dragon = pet_entry("pet_dragon", 0x501, LOCAL)
record(frame, "swap_to_dragon", transitions(dragon, removed(frogman)))
capture(frame + frames(0.3), "05_swap")
capture(frame + frames(2.0), "06_dragon_out")
# A player arrives with a phoenix (vicinity, NewRemotePlayer, its info, then its
# status snapshot as presence.py sends it) and walks past us. One who has not
# moved yet: presence.py's _remote_player_for sends its spawn position (world);
# after a move it sends last_move.relative_position (map-relative), which this
# client reads as world -- a separate question, not the pets'.
frame += frames(3.0)
walker = sp.RemotePlayer(actor_id=WALKER, name="Pet walker", guild_name="", class_id=1, gender=1,
    hair_style=0, beard_style=0, body_style=0, body_stature=0, template_row=0,
    position=at(-3.0, -2.0), heading=0.0, character_id=7, account_id=7, level=40,
    animation_set="mage_female_1h_weapon")
record(frame, "walker_vicinity", gp.encode_actors_enter_vicinity_batch(
    gp.ActorsEnterVicinityBatch(gp.ActorsEnterVicinity((WALKER,)), actor_id=0, commands=())))
record(frame, "walker", sp.encode_new_remote_player(walker))
record(frame, "walker_info", sp.encode_remote_player_info(walker))
phoenix = pet_entry("pet_phoenix", 0x600, LOCAL)
snapshot = PresenceVisualService._remap_player_status_effect(gp.StatusEffect(False, (phoenix,)), WALKER)
record(frame + 1, "walker_status", gp.encode_status_effect_batch(snapshot, actor_id=WALKER))
capture(frame + frames(1.6), "07_walker_phoenix")
frame = moves(frame + frames(2.0), WALKER, [at(-3.0, -2.0), at(-3.0, 6.0), at(2.0, 9.0)], every=12)
capture(frame - 40, "08_walker_walking")
capture(frame + frames(1.5), "09_walker_stopped")
# Revived with a cooldown: the phoenix leaves with its revival when its effect ends.
frame += frames(2.0)
record(frame, "walker_revived", encode_revive(actor_id=WALKER, duration_ticks=750))
record(frame + frames(0.5), "walker_phoenix_gone", gp.encode_status_effect_batch(
    PresenceVisualService._remap_player_status_effect(gp.StatusEffect(False, (removed(phoenix),)), WALKER), actor_id=WALKER))
capture(frame + frames(0.5) + frames(0.6), "10_phoenix_revival")
capture(frame + frames(0.5) + frames(1.8), "11_phoenix_revival_late")
# The player leaves; we send the dragon away.
frame += frames(5.0)
record(frame, "walker_leaves", sp.encode_discard_player(WALKER))
record(frame + frames(1.0), "dragon_away", transitions(removed(dragon)))
capture(frame + frames(1.0) + frames(0.4), "12_dragon_leaving")
capture(frame + frames(1.0) + frames(3.0), "13_alone")
records.append(dict(frame=frame + frames(4.5), quit=True))

shots.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps(records, indent=1) + "\n", encoding="utf-8")
print(f"{len(records)} records -> {out} (captures in {shots})")
