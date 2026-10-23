"""A player's summon (a Shadow Soul) as the local server publishes it (crates/proto/tests/summon_release261.rs).

Usage: python tools/summon_vectors.py <server> <output-directory> [<static.db4>]

The summon's lifecycle from each viewer's side, built by the server's own code:
its vicinity and NewMonster batch (gameplay/local_monster_state.py
load_player_summon_spawn) as the owner, a peer and a PvP-hostile peer receive it
(services/presence_visuals.py _player_summon_group_for_viewer), a map companion,
a map monster and a tower hazard (_load_monster), its blow on a monster
(services/combat_resolution.py: attacker the summon, source its owner) and its
DiscardMonster. The static database is read for the monster templates only; no
player database is opened.
"""
from pathlib import Path
import json
import sys
from prod_vectors import load_package

server = Path(sys.argv[1]).resolve()
load_package(server)
from dso_server.protocol import gameplay_protocol as gp
from dso_server.protocol.combat_protocol import CombatBatch, encode_combat_batch
from dso_server.gameplay.local_monster_state import load_player_summon_spawn, _load_monster, _spawn_group
from dso_server.gameplay.local_combat_state import DamageResolution
from dso_server.services.presence_visuals import PresenceVisualService
from dso_server.services.combat_resolution import CombatResolutionService

out = Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
database = Path(sys.argv[3]) if len(sys.argv) > 3 else server / "data" / "db" / "static.db4"
vectors = []
def record(name, packet):
    data, bits = packet; vectors.append(dict(name=name, hex=data.hex(), bits=bits))

SOUL, OWNER, LOCAL, TICK = 0x10600, 0x2000001, 1, 123456
group = load_player_summon_spawn(database, "mercenary_ancient_watcher", SOUL, OWNER, (10.0, 1.0, -5.0),
                                 level=60, server_tick=TICK, yaw=0.5)
record("soul_vicinity", gp.encode_actors_enter_vicinity_batch(group.vicinity))
# The owner sees itself as the local player (1); a peer, the owner's actor; a
# PvP-hostile peer, the monster faction with the hostile type.
for name, viewer, hostile in (("soul_owner", LOCAL, False), ("soul_peer", OWNER, False), ("soul_hostile", OWNER, True)):
    remapped = PresenceVisualService._player_summon_group_for_viewer(
        group, source_owner_actor_id=OWNER, viewer_owner_actor_id=viewer, hostile=hostile, health=(5000, 6000))
    record(name, gp.encode_monster_spawn_batch(remapped.spawn_batch))
companion = load_player_summon_spawn(database, "a0005_companion_spellweaver_01", SOUL + 1, OWNER, (12.0, 1.0, -5.0),
                                     level=10, server_tick=TICK, map_companion=True)
record("map_companion", gp.encode_monster_spawn_batch(companion.spawn_batch))
# The map's own: a skeleton, and a fixed tower hazard the server calls non-targetable.
for name, hazard in (("map_monster", False), ("tower_hazard", True)):
    monster = _load_monster(database, "a0202_normal_g_gen_skeleton_warrior", 0x30001, (14.0, 1.0, -5.0),
                            monster_level=60, non_targetable=hazard)
    record(name, gp.encode_monster_spawn_batch(_spawn_group(database, (monster,)).spawn_batch))
# Its blow on a monster: the summon strikes, the floating number is its owner's.
hit = DamageResolution(target_actor_id=0x30001, attacker_actor_id=SOUL, current_health=900, maximum_health=1000,
                       damage=100, killed=False, position=(11.0, 1.0, -5.0), server_tick=TICK + 30, hit_tags=(0,),
                       skill_index=1967, source_actor_id=LOCAL)
record("soul_hit", encode_combat_batch(CombatBatch(CombatResolutionService._combat_commands_for_resolution(hit))))
record("soul_discard", gp.encode_discard_monster_batch((SOUL,)))

(out / "summon_release261.json").write_text(json.dumps(vectors, indent=1) + "\n", encoding="utf-8")
print(f"{len(vectors)} vectors -> {out / 'summon_release261.json'}")
