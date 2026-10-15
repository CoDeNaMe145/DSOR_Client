"""Generate map-marker protocol fixtures from the production server codecs.

Usage: python tools/map_marker_vectors.py <server-root> <output.json>
Only imports codecs; never starts the server or reads account data.
"""
import json
from pathlib import Path
import sys
from prod_vectors import load_package

load_package(Path(sys.argv[1]).resolve())
from dso_server.protocol import gameplay_protocol as gp
from dso_server.protocol import player_init_protocol as pi
from dso_server.protocol import quest_protocol as qp

npc = gp.NPCInfo('smith', (gp.NPCInteraction('quest', 0, 'quest', 6),
    gp.NPCInteraction('delivery', 3, 'quest', 6)), (gp.NPCFlag('dialog', False),),
    (gp.NPCStringSet('delivery', ('item1', 'item2'), 3, True),),
    (gp.NPCLink('from', 'to', 17, True),), True, 2, 77)
log = pi.QuestLogInfo((pi.QuestLogEntry('quest', 0, 1, 3, (1, 2), 0, 'tracking'),
    pi.QuestLogEntry('second', 0, 0, 0, (), 0, '')))
packets = [
    ('npc_quest_delivery', gp.encode_npc_batch(gp.NPCBatch((npc,)))),
    ('quest_log_and_timestamp', pi.encode_server_command_batch((
        pi.ServerCommand(qp.QUEST_LOG_INFO_COMMAND, log, 123),
        pi.ServerCommand(qp.QUEST_UPDATE_COMMAND,
            qp.QuestUpdate.quest_state('quest', 2, '2026-10-15 12:00:00'), 123),
    ))),
]
Path(sys.argv[2]).write_text(json.dumps([
    dict(name=name, hex=data.hex(), bits=bits) for name, (data, bits) in packets
], indent=2) + '\n', encoding='utf-8')
