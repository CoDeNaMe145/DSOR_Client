"""Build a five-character roster with the real server's packet encoder.

Usage: python tools/character_selection_preview.py <server> <output.json> [--editor] [--empty] [--refuse N]
Run the client with a0000_char and DSOR_SELECTION_PREVIEW=<output.json>.
No account is accessed or changed.

--editor   the server then opens the character creation screen (CharacterGeneration
           operation 2, as after the roster's new-character button)
--empty    an account without characters: no roster, only the editor
--refuse N then refuses a creation with deny reason N (operation 4)
"""
from pathlib import Path
import argparse
import json
from prod_vectors import load_package

parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
parser.add_argument('server')
parser.add_argument('output')
parser.add_argument('--editor', action='store_true')
parser.add_argument('--empty', action='store_true')
parser.add_argument('--refuse', type=int)
args = parser.parse_args()

load_package(Path(args.server).resolve())
from dso_server.protocol.character_selection import CharacterRecord, CharacterSelection, EquipmentRecord, encode_character_selection
from dso_server.protocol import character_generation as cg

characters = []
gear = []
for i, (name, cls, gender, weapon) in enumerate([
        ('Balenciagas', 'warrior', 0, 'rh_sword_speedAttack_damage'),
        ('Aurelia', 'mage', 1, '2h_staff_speedAttack_damage'),
        ('Sylvan', 'ranger', 0, 'rh_shortbow_speedAttack_damage'),
        ('Coppergear', 'dwarf', 1, 'rh_pistol_speedAttack_damage'),
        ('Valeria', 'warrior', 1, '2h_sword_speedAttack_damage')]):
    class_id = ['warrior', 'mage', 'ranger', 'dwarf'].index(cls)
    characters.append(CharacterRecord(name, 'a0200_kingscity', class_id, gender, 3 if gender else 0, 255,
        0, 0, i + 1, 0, 9999999, 0, 0, 100 - i * 12))
    gear.append([EquipmentRecord(f'{cls}_base_{weapon}'), EquipmentRecord(f'{cls}_base_torso_armor_block_critical')])
gear[0] = [EquipmentRecord('warrior_base_rh_sword_speedAttack_damage'), EquipmentRecord('warrior_base_lh_shield_armor_block_healthpoints')]
gear[4].append(EquipmentRecord('warrior_base_boots_armor_block_critical'))
packets = []
if not args.empty:
    packets.append(encode_character_selection(CharacterSelection(active_character_id=1, character_slots=5,
        header_field_b=200000003, characters=characters, equipment=gear[0])))
    packets += [encode_character_selection(CharacterSelection(operation=9, active_character_id=i + 1, equipment=items)) for i, items in enumerate(gear)]
if args.editor or args.empty:
    packets.append(cg.encode_character_generation(cg.character_generation_editor_response(locale='en', preview_id=6)))
if args.refuse is not None:
    request = cg.CharacterGeneration(operation=3, name='Balenciagas')
    packets.append(cg.encode_character_generation(cg.character_generation_result(request, operation=4, deny_reason=args.refuse)))
output = Path(args.output); output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps([(data.hex(), bits) for data, bits in packets]), encoding='utf-8')
print(f'{0 if args.empty else len(characters)} characters and {len(packets)} packets -> {output}')
