"""Export character-selection camera, equipment slots and cosmetic choices.

Usage: export_character_selection.py <static.db4> <assets>
The scene itself is maps/a0000_char and the UI is charselectionwindow.bxml.
"""
import json
import sqlite3
import struct
from pathlib import Path


def selection_data(db):
    db.row_factory = sqlite3.Row
    globals_ = db.execute('SELECT * FROM _Globals').fetchone()
    slot_names = ['HelmetSlot', 'ShoulderSlot', 'TorsoSlot', 'GlovesSlot',
                  'BootsSlot', 'RightHandSlot', 'LeftHandSlot', 'CloakSlot']
    slots = {}
    for row in db.execute('SELECT Id, SlotType FROM _Template_Item'):
        indices = [slot_names.index(s.strip()) for s in (row['SlotType'] or '').split(',')
                   if s.strip() in slot_names]
        if indices:
            slots[row['Id']] = indices
    cosmetics = {}
    for kind, table in [('hair', '_Template_CharacterHair'), ('beard', '_Template_CharacterBeard')]:
        choices = list(db.execute(f'SELECT * FROM {table}'))
        cosmetics[kind] = {
            f'{cls.lower()}_{gender.lower()}': {
                str(r['Index']): r['Name'] for r in choices if r[cls + gender]
            }
            for cls in ['Warrior', 'Mage', 'Ranger', 'Dwarf'] for gender in ['Male', 'Female']
        }
    return dict(camera=dict(pitch=globals_['CharGenAngleX'], yaw=globals_['CharGenAngleY'],
                            offset=struct.unpack('<4f', globals_['CharGenOffsetVector'])[:3],
                            fov=globals_['CharGenFov']), slots=slots, **cosmetics)


def export_selection(db, assets):
    path = Path(assets) / 'characters' / 'selection.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    data = selection_data(db)
    path.write_text(json.dumps(data, separators=(',', ':')), encoding='utf-8')
    print(f'selection.json: camera, cosmetics and {len(data["slots"])} equipment slot mappings')


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('database')
    parser.add_argument('assets')
    args = parser.parse_args()
    with sqlite3.connect(Path(args.database).resolve().as_uri() + '?mode=ro', uri=True) as db:
        export_selection(db, args.assets)
