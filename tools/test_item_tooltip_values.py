"""Item tooltip regressions using authored DB4 programs, without game assets."""
import unittest
from item_tooltip_values import program, unique_effects


class ItemTooltipTests(unittest.TestCase):
    def test_ring_of_death_keeps_all_three_character_bonuses(self):
        values = program('Speed:5.0,absolute,Skills;Speed:5.0,absolute,Movement;DamageFactorPvP:0.66,relative')
        self.assertEqual([v['name'] for v in values], ['Attack speed', 'Movement speed', 'Damage against players'])
        self.assertEqual([v['high'] for v in values], [5, 5, .66])
        self.assertTrue(all(not v['item_local'] for v in values))

    def test_scopes_and_level_scaling_survive_export(self):
        attack, movement, resistance, resource = program(
            'ItemSpeed:0.002389,absolute,Skills;ItemSpeed:0.005,absolute,Movement;'
            'ItemResistance:4.661349,absolute,Lightning;MaxSkillResource:1|5,absolute,noScaling')
        self.assertEqual(attack['factor'], 'AttacksPerSecondFactorAbsolute')
        self.assertEqual(movement['factor'], 'MovementSpeedFactorAbsolute')
        self.assertEqual(movement['name'], 'Movement speed')
        self.assertEqual(resistance['name'], 'Lightning resistance')
        self.assertEqual((resource['low'], resource['high'], resource['factor']), (1, 5, ''))

    def test_negative_bonuses_and_authored_hidden_companions(self):
        values = program('StatusEffectGroupDuration:-0.33,relative,debuff_cc_frost#unique_item_frost_reduction;'
                         'StatusEffectGroupDuration:-0.33,relative,debuff_cc_freeze#~;'
                         'DropChance:1.03|1.1')
        self.assertEqual(values[0]['name'], 'Frost duration')
        self.assertEqual(values[0]['high'], -.33)
        self.assertEqual(len(values), 2)
        self.assertAlmostEqual(values[1]['low'], .03)
        self.assertTrue(values[1]['relative'])

    def test_scripted_effects_are_kept_alongside_numeric_values(self):
        text = {'gui.statuseffects.modifier_proc': 'Gain a buff whenever you defeat an enemy.',
                'gui.statuseffects.modifier_jewel': '+ {0:s}% additional critical value per stack'}
        raw = ('ItemDamage:0.1,relative;ActorStatusEffect:addAE,buff,C:1,On:Kill#proc;'
               'ActorVariable:0.0005,absolute,critical,forceShowAsPercentage#jewel;'
               'ItemStatusEffectCleanse:unequip,buff#~')
        self.assertEqual(unique_effects(raw, lambda key, fallback='': text.get(key, fallback)),
                         ['Gain a buff whenever you defeat an enemy.', '+ 0.05% additional critical value per stack'])


if __name__ == '__main__':
    unittest.main()
