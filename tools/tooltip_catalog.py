"""Resolve static locale tokens for skill previews; never guess actor-dependent values.

The UI still retains the original text. Unknown tokens are displayed as an em
dash at runtime, instead of dropping the entire effect from the description.
"""
import math
import re


class SkillPreview:
    def __init__(self, db):
        def table(name):
            cursor = db.execute('SELECT * FROM ' + name)
            keys = [c[0] for c in cursor.description]
            return {r['Id']: r for r in (dict(zip(keys, values)) for values in cursor)}
        self.skills = table('_Template_Skill')
        self.effects = table('_Template_StatusEffect')
        self.variables = table('_Template_ActorVariables')
        self.tokens = table('_Template_LocaleToken')
        self.talents = table('_Template_SkillTalent')

    def value(self, token, skill):
        kind = token['TokenType']
        name = token['Param1Id']
        attr = token['Param1Attr'].strip()
        if kind == 'ActorVariable':
            row = self.variables.get(name, {})
            # Mutable variables may depend on talents, equipment or buffs.
            if row.get('LowerLimit') == row.get('UpperLimit'):
                return row.get('InitialValue')
        elif kind == 'Skill':
            row = self.skills.get(skill if name == '*' else name, {})
            return row.get({'Range': 'AttackRange'}.get(attr, attr))
        elif kind == 'StatusEffect':
            return self.effects.get(name, {}).get({'Duration': 'StatusEffectDuration', 'DurationPvP': 'StatusEffectDurationPvP'}.get(attr, attr))
        elif kind == 'SkillEffect':
            row = self.skills.get(skill if name == '*' else name, {})
            field = {'LocationEffect': 'LocationStatusEffects', 'VictimEffect': 'VictimStatusEffects', 'UserEffect': 'UserStatusEffects'}.get(attr)
            effect = token['Param2Id']
            prop = token['Param2Attr'].strip()
            entries = [part.split(',') for part in (row.get(field, '') or '').split(';')]
            entry = next((e for e in entries if e[0] == effect), [])
            if not entry:
                return None
            params = dict(p.split(':', 1) for p in entry[1:] if ':' in p)
            key = {'Duration': 'D', 'DurationPvP': 'DP'}.get(prop, prop.replace('ValueReplacement', '$'))
            if key in params:
                try:
                    return float(params[key])
                except ValueError:
                    return None
            return self.effects.get(effect, {}).get(prop)
        return None

    def resolve(self, text, skill):
        def replace(match):
            token = self.tokens.get(match[1])
            if not token or token.get('LevelModifier') or token.get('BonusActorVariable'):
                return match[0]
            value = self.value(token, skill)
            if not isinstance(value, (float, int)) or not math.isfinite(value):
                return match[0]
            if token.get('IsAbsoluteBonus'):
                value = abs(value)
            fmt = token.get('ValueFormat') or ''
            if fmt.startswith('Percentage'):
                value *= 100
            decimals = re.search(r'\d', fmt)
            return f'{value:.{int(decimals[0]) if decimals else 0}f}'
        return re.sub(r'\{\d+:t:([^}]+)\}', replace, text)
