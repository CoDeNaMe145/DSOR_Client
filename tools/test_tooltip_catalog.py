"""Locale-token regression tests independent of proprietary game assets."""
import unittest
from tooltip_catalog import SkillPreview


class SkillPreviewTests(unittest.TestCase):
    def setUp(self):
        self.preview = SkillPreview.__new__(SkillPreview)
        self.preview.skills = {'HeavyShot': {'DamageModifier': 2.5, 'LocationStatusEffects': 'explosion,D:0,$0:-1'}}
        self.preview.effects = {'explosion': {'AuraRadius': 2.5}}
        self.preview.variables = {'growth': {'InitialValue': -0.05, 'LowerLimit': -0.05, 'UpperLimit': -0.05}}
        def token(kind, name, attr='', second='', prop='', fmt='Percentage1NoSign'):
            return dict(TokenType=kind, Param1Id=name, Param1Attr=attr, Param2Id=second,
                        Param2Attr=prop, ValueFormat=fmt, IsAbsoluteBonus=True)
        self.preview.tokens = {
            'damage': token('Skill', '*', 'DamageModifier'),
            'explosion': token('SkillEffect', 'HeavyShot', 'LocationEffect', 'explosion', 'ValueReplacement0'),
            'radius': token('SkillEffect', 'HeavyShot', 'LocationEffect', 'explosion', 'AuraRadius ', 'Float1'),
            'growth': token('ActorVariable', 'growth'),
        }

    def test_spell_and_per_level_values_keep_authored_formatting(self):
        self.assertEqual(self.preview.resolve('Deals {0:t:damage}% as <i>physical</> damage', 'HeavyShot'),
                         'Deals 250.0% as <i>physical</> damage')
        self.assertEqual(self.preview.resolve('{0:t:explosion}% in {0:t:radius} meters; + {0:t:growth}%', 'HeavyShot'),
                         '100.0% in 2.5 meters; + 5.0%')

    def test_actor_dependent_and_unknown_values_are_not_fabricated(self):
        text = 'Deals {0:t:damage_range} damage'
        self.assertEqual(self.preview.resolve(text, 'HeavyShot'), text)
        self.preview.variables['growth']['UpperLimit'] = 1
        self.assertEqual(self.preview.resolve('{0:t:growth}', 'HeavyShot'), '{0:t:growth}')


if __name__ == '__main__':
    unittest.main()
