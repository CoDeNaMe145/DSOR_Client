"""Generate prop/event regression vectors using the server's encoders.

Usage: python tools/prop_vectors.py <server-root> <output.json>
Imports protocol modules only; does not start a server or access account data.
"""
import json
import os
from pathlib import Path
import sys
from prod_vectors import load_package


def main():
    os.environ.pop('DSO_COMMAND_IDS', None)
    os.environ.pop('DSO_SELECTION_WIRE', None)
    load_package(Path(sys.argv[1]).resolve())
    from dso_server.protocol import gameplay_protocol as gp
    from dso_server.protocol import player_init_protocol as pi
    from dso_server.protocol import prop_protocol as pp
    entries = gp.PropInfo((
        gp.PropInfoEntry(65540, 'workbench_smith_01', bytes(range(16)), 0xFFFFFFFF, 1, 350,
                         (68.9764, 1.0, -61.9107), 0.7, '', '', 0, 0xFF, 0xFFFFFFFF),
        gp.PropInfoEntry(65541, 'chest', bytes(16), 2, 3, 400,
                         (46.0, 1.0, -45.0), -0.3, 'quest_switch', 'quest', 7, 13, 5),
    ))
    events = pi.EventUpdate((
        pi.EventUpdateEntry(17, 'christmas_2012', (True, False, True, True),
                            (2026, 10, 15, 1, 2, 3), (2026, 12, 30, 3, 2, 1), ('tag',)),
        pi.EventUpdateEntry(0, 'a0200_crafting_npc_exchange', (True, True, False, True), None, None, ()),
        pi.EventUpdateEntry(0, 'BOSS_dead', (False, True, True, True), None, None, ()),
    ))
    vectors = []
    def add(name, packet, ids):
        data, bits = packet
        vectors.append(dict(name=name, hex=data.hex(), bits=bits, ids=ids))
    add('PropInfo_two_records', gp.encode_prop_info(entries, actor_id=12345), [54])
    add('EventUpdate_four_flags', pi.encode_event_update(events), [221])
    add('unaligned_event_and_prop_chain', pi.encode_server_command_batch((
        pi.ServerCommand(54, entries, 12345), pi.ServerCommand(221, events, None),
        pi.ServerCommand(54, gp.PropInfo((entries.entries[1],)), 12345),
    )), [54, 221, 54])
    add('NewProp', pp.encode_new_prop_batch((pp.NewProp(65541, 'chest', bytes(16),
        (46., 1., -45.), 0.7, state=1, variant=-1, counter=350,
        operation='quest_switch', quest_id='quest', task_index=5, quest_indication=13),), actor_id=12345), [52])
    add('PropUpdate_unaligned', pp.encode_prop_update_batch((pp.PropUpdate(65541, 3,
        actor_id=6789, variant=2, counter=400, state_changed=True, operation='quest_switch',
        quest_id='quest', task_index=5, quest_indication=13, unknown_c0=11, unknown_c4=0.25,
        operation_changed=True, unknown_cc=-7, unknown_d0=23, keep_visual_state=False),), actor_id=12345), [55])
    add('DiscardProp', pp.encode_discard_prop_batch((65540, 65541), actor_id=12345), [53, 53])
    Path(sys.argv[2]).write_text(json.dumps(vectors, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
