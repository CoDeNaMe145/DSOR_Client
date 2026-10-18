"""Export the modifier inputs consumed by gameplay/player_combat_stats.py.

No precomputed character values: every loadout is evaluated by the client.
"""
import re


def export_combat(db):
    def rows(table):
        cursor = db.execute('SELECT * FROM '+table)
        names = [c[0] for c in cursor.description]
        return [dict(zip(names, row)) for row in cursor]

    def program(raw):
        result = []
        for line in (raw or '').split(';'):
            kind, sep, arguments = line.partition(':')
            if sep:
                result.append([kind.strip().lower(), [v.split('#')[0].split('<')[0].strip().lower()
                                                      for v in arguments.split(',')]])
        return result

    def ids(raw):
        return [s.strip() for s in (raw or '').split(';') if s.strip() and '|' not in s]

    items = {r['Id']: dict(category=r['ItemCategory'] or '', damage=[r['MinDamage'] or 0, r['MaxDamage'] or 0],
                          scaled=r['ItemLevelScalingType'] != 'NoScaling',
                          modifiers=program(r['UniqueModifier'])+program(r['BaseModifier']),
                          base=ids(r['BaseEnchantments']), enchantments=ids(r['Enchantments']))
             for r in rows('_Template_Item')}
    enchantments = {r['Id']: dict(modifiers=program(r['Modifiers']), scaled=r['ScalingMode'] in ('Absolute','Relative'),
                                tier=bool(r['UseTierFactor']), minimum=max(1,r['MinItemLevel'] or 1),
                                maximum=max(0,r['MaxItemLevel'] or 0), reference=max(1,r['ScaleReferenceLevel'] or 1))
                    for r in rows('_Template_Enchantment')}
    def tiers(table):
        return {str(r['Tier']): [[r['MinPowerFactor'+rarity],r['MaxPowerFactor'+rarity]]
                                for rarity in ['Common','Uncommon','Magic','Rare','Epic','Unique','Set','Mythic']]
                for r in rows(table)}

    sets, seen = [], set()
    for r in rows('_Template_ItemSet'):
        signature = (r['SetParts'],r['Modifiers'])
        if signature in seen:
            continue
        seen.add(signature)
        tiers_ = []
        # A range inside a modifier also uses '|'; only N) opens a tier.
        for part in re.split(r'\|(?=\s*\d+\))', r['Modifiers'] or ''):
            match = re.match(r'\s*(\d+)\)(.*)', part, re.S)
            if match:
                tiers_.append([int(match[1]), program(match[2])])
        if tiers_:
            sets.append(dict(parts=list(dict.fromkeys(ids(r['SetParts']))), tiers=tiers_))
    cursor = db.execute('SELECT rowid-1, Id, MaxStackSize, StartModifiers FROM _Template_StatusEffect')
    statuses = {str(wire): dict(id=name.lower(), stacks=max(1,stacks or 1), modifiers=program(raw))
                for wire,name,stacks,raw in cursor}
    return dict(items=items, enchantments=enchantments, statuses=statuses, sets=sets,
                variables={r['Id'].lower():r['InitialValue'] for r in rows('_Template_ActorVariables')},
                progressions={str(r['Level']): {k.lower():v for k,v in r.items() if isinstance(v,(int,float))}
                              for r in rows('_Template_Progressions')},
                tiers=tiers('_Template_EnchantmentTiers'), base_tiers=tiers('_Template_EnchantmentBaseTiers'),
                globals={k:v for k,v in rows('_Globals')[0].items() if isinstance(v,(int,float))})
