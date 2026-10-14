"""Generate timed equipment swaps for the native offline flicker regression.

Usage: python tools/equipment_transition_preview.py <server> <output-directory> [--clothing]
Replay with --play 0 0 on the debug map, DSOR_INVENTORY_PREVIEW=initial.bin and
DSOR_EQUIPMENT_REPLAY=sequence.json. No account is opened or changed.
"""
from dataclasses import replace
from pathlib import Path
import json
import sys
from prod_vectors import load_package

load_package(Path(sys.argv[1]).resolve())
from dso_server.protocol.character_selection import BitReader, BitWriter
from dso_server.protocol.inventory_codec import InventoryInfo, ItemInfo, INVENTORY_INFO_COMMAND
from dso_server.protocol.player_init_protocol import ServerCommand, encode_server_command_batch
from dso_server.protocol.social_protocol import RemotePlayer, RemoteEquipmentVisual, encode_new_remote_player, encode_remote_player_info

out = Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
items = [ItemInfo(101, template_id='warrior_base_rh_sword_speedAttack_damage'),
         ItemInfo(102, template_id='warrior_base_lh_shield_armor_block_healthpoints'),
         ItemInfo(103, template_id='warrior_base_2h_sword_speedAttack_damage')]
skins = {101: 'warrior_rh_sword_01', 102: 'warrior_lh_shield_01', 103: 'warrior_2h_sword2h_01'}
allocations = [{101: (5,), 102: (6,)}, {103: (5, 6)}, {}, {101: (5,)}]
sets = ['warrior_female_1h_weapon_shield', 'warrior_female_2h_weapon', 'warrior_female_1h_weapon', 'warrior_female_1h_weapon']
if '--clothing' in sys.argv[3:]:
    items = [ItemInfo(101, template_id='warrior_base_torso_armor_block_critical'),
             ItemInfo(102, template_id='warrior_base_boots_armor_block_critical'),
             ItemInfo(103, template_id='warrior_base_gloves_armor_speedAttack_block'),
             ItemInfo(104, template_id='warrior_base_torso_armor_block_critical', field_f8='warrior_basic_torso_02')]
    skins = {101: 'warrior_torso_01', 102: 'warrior_boots_01', 103: 'warrior_gloves_01', 104: 'warrior_torso_02'}
    allocations = [{101: (2,), 102: (4,), 103: (3,)}, {104: (2,)}, {}, {101: (2,)}]
    sets = ['warrior_female_1h_weapon'] * 4
remote = RemotePlayer(actor_id=0x103C1, name='Remote equipment', guild_name='', class_id=0, gender=1,
    hair_style=0, beard_style=0, body_style=0, body_stature=0, template_row=0,
    position=(24.2, 0.0, 22.0), heading=0.0, character_id=2, account_id=2, level=10)


def packet(mode, spawn=False):
    slots = allocations[mode]
    inventory = encode_server_command_batch((ServerCommand(INVENTORY_INFO_COMMAND,
        InventoryInfo(items=items, equipment_slots=slots, storage_size=224), 1),))
    equipment = tuple(RemoteEquipmentVisual(slot, (skins[item],)) for item, cells in slots.items() for slot in cells)
    presence = replace(remote, equipment=equipment, animation_set=sets[mode])
    packets = [inventory]
    if spawn:
        packets.append(encode_new_remote_player(presence))
    packets.append(encode_remote_player_info(presence))
    writer = BitWriter(); writer.write_uint8(0x85)
    for data, bits in packets:
        reader = BitReader(data); assert reader.read_uint8() == 0x85
        for _ in range(bits - 8):
            writer.write_bits(reader.read_bits(1), 1)
    return writer.finish()


(out / 'initial.bin').write_bytes(packet(0, spawn=True)[0])
records = []
for frame, mode in [(70, 1), (100, 0), (105, 1), (130, 2), (160, 3), (190, 0)]:
    data, bits = packet(mode)
    records.append(dict(frame=frame, hex=data.hex(), bits=bits))
(out / 'sequence.json').write_text(json.dumps(records, indent=2)+'\n', encoding='utf8')
print(f'Initial outfit and {len(records)} local/remote swaps -> {out}')
