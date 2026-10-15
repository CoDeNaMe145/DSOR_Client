"""Export release-matched item metadata and authored ingredient collection cells."""
import json
import os
import struct
import re

SLOTS = {'HelmetSlot':[0], 'ShoulderSlot':[1], 'TorsoSlot':[2], 'GlovesSlot':[3],
         'BootsSlot':[4], 'RightHandSlot':[5], 'LeftHandSlot':[6],
         'RightHandSlot,LeftHandSlot':[5,6], 'CloakSlot':[7], 'RingSlot':[8,9],
         'AmuletSlot':[10], 'BeltSlot':[11], 'WeaponModSlot':[12],
         'RuneTrinketSlot':list(range(13,20)), 'JewelTrinketSlot':[20,21,22],
         'AncientTrinketSlot':[24], 'MercenaryAmuletSlot':[25],
         'MercenaryCloakSlot':[26], 'MercenaryBeltSlot':[27], 'ConsumableSlot':[37],
         'AmmoSlot':[35,34]}


def export_inventory(db, textures, loca, assets):
    table = loca.get('en', {})
    short = {k.split('_', 1)[-1]: v for k, v in table.items() if k.startswith('db.')}
    def local(key, fallback=''):
        if not key:
            return fallback
        for candidate in (key, 'db.item.table_' + key, 'db.item2.table_' + key):
            if candidate in table:
                return re.sub(r'<[^>]*>', '', table[candidate])
        if key in short:
            return re.sub(r'<[^>]*>', '', short[key])
        return fallback

    result = dict(items={}, ingredients=[], colors={})
    cursor = db.execute('SELECT * FROM _Template_Item')
    columns = [c[0] for c in cursor.description]
    for values in cursor:
        r = dict(zip(columns, values))
        brush = r.get('IconBrush') or ''
        icon = 'textures/' + brush + '.png' if brush else ''
        if brush and not os.path.isfile(os.path.join(assets, icon)):
            if not textures.get(brush):
                icon = ''
        result['items'][r['Id']] = dict(name=local(r.get('LocaleIdTitle') or r['Id']+'Title', r.get('Name') or r['Id']),
            description=local(r.get('LocaleIdDesc') or r['Id']+'Desc'),
            category=local(r.get('LocaleIdCategory') or '', r.get('ItemCategory') or ''),
            icon=icon, max_stack=max(1, int(r.get('MaxStackSize') or 1)),
            required_level=max(0, int(r.get('RequiredLevel') or 0)),
            slots=SLOTS.get(r.get('SlotType'), []), class_name=(r.get('CharClass') or '').lower(),
            rarity=r.get('DefaultRarity') or 'Common',
            # The model it lies on the ground as (crate::drops), and the sequence
            # that replaces its rarity glow, if any.
            graphics=r.get('Graphics') or '', drop_sequence=r.get('DropSequenceOverride') or '')
    for row in db.execute('SELECT ItemTemplateId, IngredientBagTab, IngredientBagRarity, IngredientBagSlot, CharClass, UnlockLevel FROM _Template_IngredientBag WHERE IsLive=1 ORDER BY IngredientBagTab, IngredientBagRarity, IngredientBagSlot, Id'):
        item, tab, rarity, slot, cls, level = row
        if item not in result['items']:
            continue
        result['ingredients'].append(dict(template=item, tab=tab, rarity=int(rarity[-2:]) if rarity.startswith('Rarity_') else 0,
            slot=int(slot)-1, class_name=(cls or 'any').lower(), unlock=int(level or 0)))
    for color, rarity, color_id in db.execute('SELECT PrimaryColor,Rarity,ColorId FROM _Template_ItemRarityColors'):
        rgba = list(struct.unpack('<4f', color)) if isinstance(color, bytes) and len(color)==16 else [1,1,1,1]
        rgba[3] = 1.0
        result['colors'][str(color_id)] = rgba
    path = os.path.join(assets, 'interface', 'inventory.items.json')
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(result, f, ensure_ascii=False, separators=(',', ':'))
    return len(result['items']), len(result['ingredients'])
