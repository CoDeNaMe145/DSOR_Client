#!/usr/bin/env python3
"""Prepare event/sample bindings and every map's authored audio for the client.

Run after export_audio.py. No proprietary audio or metadata belongs in git.
"""
import argparse
import json
import sqlite3
import struct
from contextlib import closing
from pathlib import Path

from audio_events import chunks, flat, read_bank, resolve_events, strings
from export_audio import write_json
from audio_animation import animation_audio


def cue(value):
    return (value or '').strip().removeprefix('event:/').lstrip('/')


def metadata(path):
    # Sample payloads can be hundreds of MB; only RIFF metadata and each FSB's
    # 12-byte header are needed here. Do not reread all the compressed audio.
    with path.open('rb') as stream:
        header = stream.read(12)
        if header[:4] != b'RIFF': raise ValueError(f'not a RIFF bank: {path}')
        size = struct.unpack_from('<I', header, 4)[0] + 8
        data = header + stream.read(size - 12)
        counts = []
        for tag, payload in flat(chunks(data)):
            if tag == b'SNDH':
                for at in range(4, len(payload), 8):
                    stream.seek(struct.unpack_from('<I', payload, at)[0])
                    fsb = stream.read(12)
                    if fsb[:4] != b'FSB5': raise ValueError(f'invalid FSB reference: {path}')
                    counts.append(struct.unpack_from('<I', fsb, 8)[0])
        return data, counts


def map_audio(path):
    with closing(sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True)) as db:
        db.row_factory = sqlite3.Row
        tables = {r[0] for r in db.execute("select name from sqlite_master where type='table'")}
        def rows(table):
            return [dict(r) for r in db.execute(f'SELECT * FROM "{table}"')] if table in tables else []
        level = next(iter(rows('_Instance_Levels')), {})
        def placement(row):
            matrix = list(struct.unpack('<16f', row['Transform']))
            return {'event': int(row.get('EventSetRow', -1)), 'matrix': matrix,
                    'layers': row.get('_Layers') or ''}
        return {
            'music': cue(level.get('DefaultMusic')),
            'ambient': cue(level.get('Default2DAmbient')),
            'ambient_3d': cue(level.get('Default3DAmbient')),
            'reverb': level.get('ReverbPreset') or '',
            'layers': level.get('_Layers') or '',
            'event_sets': {int(r['Id']): r['EventFlags'] or '' for r in rows('_Instance_EventSets')},
            'emitters': [dict(placement(r), cue=cue(r.get('CueName')), active=bool(r.get('IsActive')))
                         for r in rows('_Instance_SoundObject') if r.get('CueName')],
            'areas': [dict(placement(r), cue=cue(r.get('MusicEventName')), shape=r.get('Shape') or 'box')
                      for r in rows('_Instance_MusicArea') if r.get('MusicEventName')],
        }


def export_runtime(root, assets):
    map_files = sorted((root / 'maps').glob('*.db4'))
    if not map_files:
        raise ValueError(f'no map databases found under {root / "maps"}')
    catalog = json.loads((assets / 'audio/catalog.json').read_text(encoding='utf-8'))
    graph, names = ({}, {}, {}, {}), {}
    for name, bank in catalog['banks'].items():
        if 'Desktop/' not in name:
            continue
        data, counts = metadata(assets / bank['source'])
        if name.endswith('.strings.bank'):
            end = struct.unpack_from('<I', data, 4)[0] + 8
            for tag, payload in flat(chunks(data, end=end)):
                if tag == b'STDT': names.update(strings(payload))
        else:
            read_bank(data, bank['samples'], graph, counts)
    events = resolve_events(graph, names)
    maps = {p.stem: map_audio(p) for p in map_files}
    samples = {s['path']: s['bytes'] for b in catalog['banks'].values() for s in b['samples']}
    missing = sorted({cue for m in maps.values() for cue in
                      [m['music'], m['ambient'], m['ambient_3d']] +
                      [r['cue'] for r in m['emitters'] + m['areas']] if cue and cue not in events})
    animations = animation_audio(root, assets)
    result = {'version': 1, 'events': events, 'samples': samples, 'maps': maps, 'animations': animations}
    write_json(assets / 'audio/runtime.audio.json', result)
    missing_animation = sorted({cue for skin in animations['skins'].values() for cues in skin.values()
                                for cue in cues if cue and cue not in events})
    write_json(assets / 'audio/runtime-report.json', {'unresolved_map_events': missing,
                                                    'unresolved_animation_events': missing_animation})
    print(f'[audio] runtime: {len(events)} events, {len(maps)} maps, {len(missing)} unresolved map cues', flush=True)
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    parser.add_argument('assets', type=Path)
    args = parser.parse_args()
    export_runtime(args.root, args.assets)
