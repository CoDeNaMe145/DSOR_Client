"""A Shadow Soul's life around the offline player, for screenshots (DSOR_PACKET_REPLAY).

Usage: python tools/summon_fixture_packets.py <server> <assets> <out.json> <shots-dir> [<map> [<x,y,z>]] [<static.db4>]

Writes [{frame, name, hex, bits}] plus capture/quit records: our own soul appears
beside the player (its rise), follows a path, runs to a skeleton, strikes it
(the owner's floating number), kills it; a PvP enemy's Ember Knight appears;
then ours is discarded (its despawn). Every packet comes from the local
server's own builders and encoders (load_player_summon_spawn and the viewer
remap, _load_monster, encode_monster_attack_batch, the combat resolution
batches), as the server sends them to the soul's owner. Offline play stands the
player at the map's centre (in Kingscity, under a wagon): a ForceTransform first
puts it on open ground (<x,y,z>, default Kingscity's square), and the scene
plays around it. Frames are paced for about 170 frames a second. Nothing here
touches a player database.

Run: target/debug/dsor-client.exe <map> --play 1 0 with DSOR_PACKET_REPLAY=<out.json>
(DSOR_ASSETS=<assets>); the run ends after the last capture.
"""
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
from dso_server.protocol import combat_protocol as cp
from dso_server.gameplay import local_monster_state as lms
from dso_server.gameplay.local_combat_state import DamageResolution
from dso_server.services.presence_visuals import PresenceVisualService
from dso_server.services.combat_resolution import CombatResolutionService

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

SOUL, ENEMY_SOUL, SKELETON, OWNER, LOCAL = 0x10600, 0x10601, 0x30001, 0x2000001, 1
WATCHER_ATTACK = 1967   # mercenaryskill_ancient_watcher_attack_01 (assets/skills/skills.json)
tick = 1000

def summon(frame, name, template, actor, position, yaw, viewer_owner, hostile=False):
    group = lms.load_player_summon_spawn(database, template, actor, OWNER, position, level=60, server_tick=tick, yaw=yaw)
    group = PresenceVisualService._player_summon_group_for_viewer(
        group, source_owner_actor_id=OWNER, viewer_owner_actor_id=viewer_owner, hostile=hostile)
    record(frame, f"{name}_vicinity", gp.encode_actors_enter_vicinity_batch(group.vicinity))
    record(frame, name, gp.encode_monster_spawn_batch(group.spawn_batch))

def moves(frame, actor, path, every=6):
    """Walk `actor` along `path` as the server's movement publisher does: a record
    per 4 ticks, then a stop."""
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

# We stand on open ground; our soul appears beside us, facing as we do, and rises.
record(5, "player_placed", gp.encode_force_transform_batch(gp.ForceTransform(base, 0.0), actor_id=LOCAL))
start = frames(1.0)
summon(start, "soul_owner", "mercenary_ancient_watcher", SOUL, at(2.0, 1.0), 0.0, LOCAL)
capture(start + frames(0.15), "01_rise")
capture(start + frames(1.0), "02_risen")
# It follows us around (the server walks it after its owner).
frame = moves(start + frames(2.5), SOUL, [at(2.0, 1.0), at(-2.0, 3.0), at(-3.0, -1.0)], every=12)
capture(frame - 40, "03_following")
# A skeleton comes; the soul runs at it and strikes.
skeleton = lms._load_monster(database, "a0202_normal_g_gen_skeleton_warrior", SKELETON, at(4.0, -3.0), monster_level=60)
group = lms._spawn_group(database, (skeleton,))
record(frame + 20, "skeleton_vicinity", gp.encode_actors_enter_vicinity_batch(group.vicinity))
record(frame + 20, "skeleton", gp.encode_monster_spawn_batch(group.spawn_batch))
frame = moves(frame + frames(0.5), SOUL, [at(-3.0, -1.0), at(2.6, -2.6)], every=12)
tick += 4
aim = math.atan2(-(4.0 - 2.6), -(-3.0 + 2.6))
context = cp.SkillExecutionContext(impact_tick=tick + 18, hit_frame=18, motion_unblock_frame=70, skill_unblock_frame=70,
                                   reserved=0, position=(*at(2.6, -2.6), 0.0), predicted=False)
attack = cp.TargetSkillServerCommand(cp.TargetSkill(skill_level=1, skill_index=WATCHER_ATTACK, aim_angle=aim, server_tick=tick,
                                                    cast_id=tick, target_actor_id=SKELETON, context=context), SOUL)
stop = gp.MoveServerCommand(gp.Move.from_values(wire(at(2.6, -2.6)), 0.0, aim, aim, tick - 1, 0), SOUL)
frame += frames(0.5)
record(frame, "soul_attack", cp.encode_monster_attack_batch(cp.MonsterAttackBatch((stop,), (attack,))))
capture(frame + frames(0.4), "04_attack")
def blow(health, killed):
    resolution = DamageResolution(target_actor_id=SKELETON, attacker_actor_id=SOUL, current_health=health,
                                  maximum_health=int(skeleton.field_90), damage=int(skeleton.field_90) - health, killed=killed,
                                  position=at(4.0, -3.0), server_tick=tick + 18, hit_tags=(0,), skill_index=WATCHER_ATTACK,
                                  source_actor_id=LOCAL)
    return cp.encode_combat_batch(cp.CombatBatch(CombatResolutionService._combat_commands_for_resolution(resolution)))
record(frame + frames(0.72), "soul_hit", blow(int(skeleton.field_90) // 2, False))
capture(frame + frames(0.85), "05_hit")
record(frame + frames(2.2), "soul_kill", blow(0, True))
frame += frames(3.5)
# A PvP enemy's soul: the monster faction, a target for us.
summon(frame, "enemy_soul", "mercenary_ember_knight_kain", ENEMY_SOUL, at(-4.0, -3.0), 1.0, OWNER, hostile=True)
capture(frame + frames(1.5), "06_enemy_soul")
# Ours leaves (another soul summoned, or we left): its despawn.
frame += frames(2.0)
record(frame, "soul_discard", gp.encode_discard_monster_batch((SOUL,)))
capture(frame + frames(0.12), "07_despawn")
capture(frame + frames(0.6), "08_despawn_late")
capture(frame + frames(1.6), "09_gone")
records.append(dict(frame=frame + frames(1.8), quit=True))

shots.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps(records, indent=1) + "\n", encoding="utf-8")
print(f"{len(records)} records -> {out} (captures in {shots})")
