"""DB4 lighting values shared by the editor, live renderer and client export."""
import copy
import json
import math
import re
import uuid

LIGHT = '_Instance_Light'
AMBIENCE = '_Instance_AmbienceBubble'
KINDS = (LIGHT, AMBIENCE)
BUBBLE = '''LightColor LightOppositeColor LightIntensity BackLightFactor LightAmbient
LightShadowIntensity LightSpecularIntensity Saturation Balance BloomScale BloomColor
BrightPassThreshold FogColor HeightFogColor FogIntensity FogNearHeight FogFarHeight
FogNearDist FogFarDist LocalLightColor LocalLightIntensity RimLightColor RimLightIntensity
VignetteIntensity VignetteSize VignetteColor PEFadeTime'''.split()
COLORS = set('LightColor LightOppositeColor LightAmbient FogColor BloomColor LocalLightColor VignetteColor'.split())
LIGHT_TYPES = {'Point': 0, 'Spot / projected': 1}
BOOLEANS = {'LightCastShadows', 'LightFlickerEnable', 'AmbienceBubbleEnabled'}


def light(row):
    from level_data import floats
    at = floats(row['Transform'])
    return {'t': int(row.get('LightType') or 0), 'p': at[12:15], 'dir': at[8:11],
            'color': floats(row['LightColor'])[:3], 'i': float(row.get('LightIntensity') or 0),
            'r': float(row.get('LightRange') or 0), 'cone': float(row.get('LightConeAngle') or 0),
            'shadows': bool(row.get('LightCastShadows')),
            'flicker': [float(row.get('LightFlickerFrequency') or 0), float(row.get('LightFlickerIntensity') or 0)] if row.get('LightFlickerEnable') else None,
            'event': int(row.get('EventSetRow', -1))}


def bubble(row):
    from level_data import floats
    at = floats(row['Transform']) if row.get('Transform') else None
    result = {key: floats(row[key]) if isinstance(row[key], dict) else row[key] for key in BUBBLE if key in row}
    result.update(dir=at[8:11] if at else None, m=at, priority=row.get('AmbienceBubblePriority', 0),
                  event=row.get('EventSetRow', -1), shape=row.get('PEShapeType', 'Box'),
                  enabled=bool(row.get('AmbienceBubbleEnabled', 1)))
    return result


def document(session):
    result = {'map': session.name, 'global': None, 'bubbles': [], 'lights': []}
    for row in session.records.get(AMBIENCE, {}).values():
        value = bubble(row)
        if row.get('PEDefaultEntity') and result['global'] is None:
            result['global'] = value
        else:
            result['bubbles'].append(value)
    for row in session.records.get('_Instance_GlobalShadow', {}).values():
        if row.get('Transform'):
            from level_data import floats
            result['shadow'] = floats(row['Transform'])[8:11]
            break
    result['lights'] = [light(row) for row in session.records.get(LIGHT, {}).values() if row.get('Transform')]
    return result


def property_value(key, text, old):
    from level_data import float_blob, floats
    if key in COLORS:
        channels = [float(v) for v in re.split(r'[,\s]+', text.strip().strip('[]')) if v]
        if len(channels) == 3:
            channels.append(floats(old)[3])
        if len(channels) != 4:
            raise ValueError('Colors need R, G, B values and an optional alpha')
        return float_blob(channels)
    if key == 'LightType':
        return LIGHT_TYPES[text] if text in LIGHT_TYPES else int(text)
    return (text.casefold() in ('true', '1')) if isinstance(old, bool) else int(text) if isinstance(old, int) else float(text) if isinstance(old, float) else text


def validate(kind, row, original=None):
    """Validate changed lighting fields without rejecting untouched legacy data."""
    from level_data import floats
    if kind not in KINDS:
        return
    changed = {key for key in row if original is None or row[key] != original.get(key)}
    for key in changed & COLORS:
        channels = floats(row[key])
        if len(channels) != 4 or not all(math.isfinite(v) and 0 <= v <= 32 for v in channels):
            raise ValueError(f'{key} needs four finite color channels between 0 and 32')
    nonnegative = {'LightIntensity', 'LightRange', 'LightFlickerFrequency', 'FogIntensity', 'BloomScale',
                   'BrightPassThreshold', 'Saturation', 'LocalLightIntensity', 'PEFadeTime', 'VignetteIntensity', 'VignetteSize'}
    for key in changed & nonnegative:
        if not isinstance(row[key], (int, float)) or not math.isfinite(row[key]) or row[key] < 0:
            raise ValueError(f'{key} must be a finite number at least zero')
    for key, low, high in [('LightConeAngle', 0, 179), ('LightFlickerIntensity', 0, 1)]:
        if key in changed and not low <= row[key] <= high:
            raise ValueError(f'{key} must be between {low} and {high}')
    if 'LightType' in changed and row['LightType'] not in (0, 1):
        raise ValueError('Choose a point light (0) or spot light (1)')
    for key in changed & BOOLEANS:
        if row[key] not in (0, 1):
            raise ValueError(f'{key} must be enabled (1) or disabled (0)')
    if changed & {'FogNearDist', 'FogFarDist'} and row.get('FogFarDist', 0) <= row.get('FogNearDist', 0):
        raise ValueError('Fog far distance must be greater than fog near distance')


def new_light(session, location, spot=False):
    """Keep the exact source schema, including opaque/unknown DB4 fields."""
    from level_data import encode, float_blob
    schema = session.snapshot['schemas'].get(LIGHT)
    if not schema:
        raise ValueError('The light DB4 schema is unavailable. Restore the map DB4 or its metadata cache.')
    template = next(iter(session.records.get(LIGHT, {}).values()), None)
    row = copy.deepcopy(template) if template else {}
    for column in schema['columns']:
        if column['name'] in row:
            continue
        affinity = column['type'].upper()
        default = column['dflt_value']
        try:
            value = json.loads(default) if default is not None else None
        except (ValueError, TypeError):
            value = default.strip("'")
        if value is None:
            value = encode(b'') if 'BLOB' in affinity else 0 if 'INT' in affinity else 0.0 if 'REAL' in affinity else ''
        row[column['name']] = value
    x, y, z = location
    defaults = {'Guid': encode(uuid.uuid4().bytes), '_ID': str(uuid.uuid4()), '_Level': session.name, '_Layers': '',
                'Transform': float_blob([1,0,0,0, 0,0,-1,0, 0,1,0,0, x,y,z,1]),
                'EventSetRow': -1, 'ParentNodeIndex': -1, 'LightType': int(spot),
                'LightColor': float_blob([1, 0.86, 0.65, 1]), 'LightOppositeColor': float_blob([0,0,0,1]),
                'LightAmbient': float_blob([0,0,0,1]), 'LightIntensity': 1.0, 'LightRange': 8.0,
                'LightCastShadows': 0, 'LightShadowIntensity': 1.0, 'LightTexture': '', 'AnimPath': '',
                'LightFlickerEnable': 0, 'LightFlickerFrequency': 2.0, 'LightFlickerIntensity': 0.3,
                'LightConeAngle': 45.0 if spot else 0.0}
    row.update({k: v for k, v in defaults.items() if k in row})
    return row
