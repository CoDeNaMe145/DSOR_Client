"""Animation sound markers from NAX3 and the client's audio/hotspots.xml.

NAX events store the hotspot's index in `category`, NOT its name. Preserve that
index even when other hotspots are silent; XML order and key order differ.
The fixed record layout also used by DSO_Godot/dsoexp/nax.py is read without
loading animation curves or NAC key streams.
"""
import json
import struct
import xml.etree.ElementTree as ET
from collections import Counter


def clean(value):
    return (value or '').strip().removeprefix('event:/').lstrip('/')


def markers(path):
    result = {}
    with path.open('rb') as stream:
        header = stream.read(12)
        if len(header) != 12 or header[:4] not in (b'NAX3', b'3XAN', b'HAN0', b'0HAN', b'NA01', b'10AN'):
            raise ValueError(f'invalid animation header: {path}')
        count, _ = struct.unpack_from('<II', header, 4)
        for _ in range(count):
            record = stream.read(64)
            if len(record) != 64:
                raise ValueError(f'truncated animation: {path}')
            curves, start, keys, stride, tick, pre, post, count_events = struct.unpack_from('<5H2BH', record)
            name = record[14:].split(b'\0', 1)[0].decode('utf-8')
            events = []
            for _ in range(count_events):
                event = stream.read(64)
                if len(event) != 64:
                    raise ValueError(f'truncated animation event: {path}')
                slot = event[47:62].split(b'\0', 1)[0].decode('utf-8')
                if slot.isdecimal():
                    events.append({'at': round(struct.unpack_from('<H', event, 62)[0] * tick / 1000, 6),
                                   'slot': int(slot)})
            stream.seek(curves * 24, 1)
            if events:
                result[name] = sorted(events, key=lambda e: (e['at'], e['slot']))
    return result


def animation_audio(root, assets):
    hotspot_path = root / 'audio/hotspots.xml'
    skins = {}
    if hotspot_path.exists():
        for skin in ET.parse(hotspot_path).getroot().findall('Skin'):
            clips = skins.setdefault(skin.attrib['name'], {})
            for anim in skin.findall('Anim'):
                # Later entries override earlier entries, including empty cues.
                clips[anim.attrib['name']] = [clean(h.get('event')) for h in anim.findall('Hotspot')]
    models = {}
    for path in sorted((root / 'anims').rglob('*.nax3')):
        clips = markers(path)
        if not clips:
            continue
        model = path.relative_to(root / 'anims').with_suffix('').as_posix().removesuffix('_animations')
        models.setdefault(model, {}).update(clips)
    # Set names usually have their own bindings. Skins provide a fallback for
    # sets introduced later which reuse an existing creature's geometry.
    sets = {}
    for path in sorted((assets / 'characters').rglob('*.sets.json')):
        model = path.relative_to(assets).as_posix().removesuffix('.sets.json')
        sets[model] = {name: row.get('skins', []) for name, row in json.loads(path.read_text()).items()}
    # Cosmetic sets can postdate hotspots.xml. Reuse the most common authored
    # binding for that SAME model and clip; never use human steps for a mount.
    # An explicitly authored empty binding remains silent in the runtime.
    fallbacks = {}
    for model, profiles in sets.items():
        if model in ('characters/uniskel', 'characters/uniskel_dwarf'):
            continue
        for clip in models.get(model, {}):
            if not any(word in clip.lower() for word in ('run', 'walk')):
                continue  # Do not invent special-mount idles for ordinary decor.
            candidates = []
            for profile, parts in profiles.items():
                cues = skins.get(profile, {}).get(clip)
                if cues is None:
                    cues = next((skins[p][clip] for p in parts if clip in skins.get(p, {})), None)
                if cues and any(cues):
                    candidates.append(tuple(cues))
            if candidates:
                fallbacks.setdefault(model, {})[clip] = list(Counter(candidates).most_common(1)[0][0])
    return {'models': models, 'skins': skins, 'sets': sets, 'fallbacks': fallbacks}
