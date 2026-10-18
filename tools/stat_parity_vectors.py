"""Generate regression vectors against the server's live stat implementation.

Usage: stat_parity_vectors.py <server checkout> <assets> <output.json>
The export contains only the DB rows referenced by the test loadouts.
"""
import dataclasses
import json
from pathlib import Path
import sys

server, assets, output = map(Path, sys.argv[1:])
sys.path.insert(0, str(server.resolve().parent))
from dso_server.tools.audit_character_stats import character_sources
from dso_server.gameplay.player_combat_stats import prime_player_combat_database, _calculate_player_combat_stats
from dso_server.protocol.inventory_codec import InventoryInfo
from dso_server.protocol.item_set_protocol import ItemSetDatabase
from dso_server.protocol.socket_effect_protocol import SocketEffectDatabase

dbpath = server.resolve()/'data/db/static.db4'
database = prime_player_combat_database(dbpath)
document = json.loads((server/'tests/fixtures/player_stats_test_acc_2.json').read_text())
inventory, sources, _ = character_sources(document, database_path=dbpath)
data = json.loads((assets/'characters/stats.json').read_text())
data = {k:data[k] for k in ['attrs','levels','player','curves','chance','globals','combat']}
data.update(skills={}, texts={})
cases = []
all_sources = list(sources)
mapping = {'minimum_damage':'MinDamage','maximum_damage':'MaxDamage','maximum_health':'MaxHealthPoints',
           'maximum_resource':'MaxSkillResource','critical_rating':'CriticalRating','critical_hit_chance':'CriticalChance',
           'attack_speed':'SkillExecutionSpeed','movement_speed':'MovementSpeed','movement_bonus_speed':'MovementBonus',
           'armor_rating':'Armor','critical_damage_multiplier':'CriticalDamage','block_chance':'BlockChance',
           'block_damage_reduction':'BlockDamageReduction'}


def case(name, inv, effects, cls=document['class_name'], level=document['level']):
    all_sources.extend(effects)
    result = _calculate_player_combat_stats(database, class_name=cls, level=level, inventory=inv, modifier_sources=effects)
    expected = {target:getattr(result, field) for field,target in mapping.items()}
    expected.update({{'fire':'Fire','ice':'Ice','lightning':'Lightning','darkmagic':'DarkMagic','poison':'Poison'}[k]+'Resistance':v
                     for k,v in result.resistance_ratings})
    expected['HealthRegeneration'] = (result.maximum_health*result.health_regeneration_fraction+result.health_regeneration_absolute)/result.health_regeneration_interval
    expected['ResourceRegeneration'] = (result.maximum_resource*result.resource_regeneration_fraction+result.resource_regeneration_absolute)/result.resource_regeneration_interval
    cases.append(dict(name=name, cls=cls, level=level, inventory=dataclasses.asdict(inv),
                      effects=[dataclasses.asdict(e) for e in effects if hasattr(e,'effect_id')], expected=expected))


case('equipped_with_talents_sets_sockets_and_pet',inventory,sources)
pet = database.status_effect_ids['pet_jabbax']
case('equipped_pet_removed',inventory,tuple(e for e in sources if getattr(e,'effect_id',None)!=pet))
case('unequipped_no_buffs',InventoryInfo(),())

def equipment_case(name, inv):
    sets_db = ItemSetDatabase(dbpath)
    sets = sets_db.evaluate_inventory(inv, class_name=document['class_name'])
    effects = tuple(sets_db.reconcile_effects(sets, {}, actor_id=1, next_instance_id=300000, server_tick=0).effects.values())
    socket_db = SocketEffectDatabase(dbpath)
    sockets = socket_db.evaluate_inventory(inv)
    effects += tuple(socket_db.reconcile_effects(sockets, {}, actor_id=1, next_instance_id=400000, server_tick=0).effects.values())
    case(name, inv, effects + tuple(sets.modifiers))

equipment_case('equipment_without_talents_or_buffs', inventory)
equipment_case('sockets_removed', dataclasses.replace(inventory,
    items=tuple(dataclasses.replace(i, socketed_templates=()) for i in inventory.items)))
equipment_case('weapon_removed', dataclasses.replace(inventory,
    equipment_slots={i:s for i,s in inventory.equipment_slots.items() if 5 not in s}))
equipment_case('different_base_roll_seeds', dataclasses.replace(inventory,
    items=tuple(dataclasses.replace(i, field_18=123456789) for i in inventory.items)))
for cls in ['mage','warrior','ranger','dwarf']:
    case(cls+'_base',InventoryInfo(),(),cls,100)

# Retain referenced item/enchantment definitions and reachable status children.
catalog=data['combat']
item_ids={i.template_id for i in inventory.items}
item_ids.update(s for i in inventory.items for s in i.socketed_templates if s)
catalog['items']={k:v for k,v in catalog['items'].items() if k in item_ids}
enchants={e.template_id for i in inventory.items for e in i.enchantments}
enchants.update(e for item in catalog['items'].values() for e in item['base']+item['enchantments'])
catalog['enchantments']={k:v for k,v in catalog['enchantments'].items() if k in enchants}
catalog['sets']=[s for s in catalog['sets'] if any(p in item_ids for p in s['parts'])]
status_ids={e.effect_id for e in all_sources if hasattr(e,'effect_id')}
programs=[i['modifiers'] for i in catalog['items'].values()]+[e['modifiers'] for e in catalog['enchantments'].values()]
programs += [p for s in catalog['sets'] for _,p in s['tiers']]
names={v['id']:int(k) for k,v in catalog['statuses'].items()}
while True:
    before=set(status_ids)
    for program in programs+[catalog['statuses'][str(i)]['modifiers'] for i in status_ids]:
        for kind,args in program:
            if kind=='actorstatuseffect' and len(args)>1 and args[1] in names:
                status_ids.add(names[args[1]])
    if status_ids==before:break
catalog['statuses']={k:v for k,v in catalog['statuses'].items() if int(k) in status_ids}
output.parent.mkdir(parents=True,exist_ok=True)
output.write_text(json.dumps(dict(data=data,cases=cases),separators=(',',':')),encoding='utf-8')
print(f'{len(cases)} server parity cases -> {output} ({output.stat().st_size} bytes)')
