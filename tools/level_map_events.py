"""Read v5 DSOM event placements using their parent map's template/string tables.

The regular map exporter omits .eve files and the EMAL masks on base instances.
Keep these alongside the base manifest, rather than baking seasonal geometry
into its unconditional instances. No external converter installation required.
"""
import copy
from pathlib import Path
import struct


class Reader:
    def __init__(self, path):
        self.path, self.data, self.offset = path, Path(path).read_bytes(), 0

    def take(self, count):
        end = self.offset + count
        if end > len(self.data):
            raise ValueError(f"Truncated map event data: {self.path}")
        data, self.offset = self.data[self.offset:end], end
        return data

    def value(self, fmt):
        values = struct.unpack('<' + fmt, self.take(struct.calcsize('<' + fmt)))
        return values[0] if len(values) == 1 else values

    def tag(self, name):
        if self.take(4) != name[::-1].encode('ascii'):
            raise ValueError(f"Expected {name} in {self.path}")

    def header(self):
        self.tag('DSOM')
        if self.value('I') != 5:
            raise ValueError(f"Unsupported map event version: {self.path}")

    def instances(self, strings, templates):
        self.tag('INST')
        for _ in range(self.value('I')):
            pos, rot = self.value('4f'), self.value('4f')
            scale = self.value('4f') if self.value('?') else (1.0, 1.0, 1.0, 0.0)
            collide, _nav, template, group, name, mapping = self.value('??hhHH')
            if template == -1:
                continue
            if not 0 <= template < len(templates):
                raise ValueError(f"Invalid event template {template}: {self.path}")
            model = strings[templates[template]]
            if not model:
                continue
            yield {"model": model, "p": [round(v, 5) for v in pos[:3]],
                   "r": [round(v, 6) for v in rot], "s": [round(v, 5) for v in scale[:3]],
                   "n": strings[name] if name < len(strings) else "", "g": group, "col": collide}, mapping


def with_event_layers(manifest, map_path, cached=None):
    """Return None without a raw map, otherwise a complete enriched baseline."""
    from level_data import digest
    map_path = Path(map_path)
    if not map_path.is_file():
        return None
    r = Reader(map_path)
    r.header()
    r.tag('MAPI')
    r.take(54)  # three vec4s and three int16 grid dimensions
    r.tag('STRT')
    count, length = r.value('II')
    strings = r.take(length).decode('utf-8', 'replace').split('\0')[:count]
    r.tag('SETT')
    r.take(r.value('I') * 4)
    r.tag('EVET')
    flags = [strings[r.value('I')] for _ in range(r.value('I'))]
    r.tag('EMAL')
    masks = [r.take(r.value('H')).decode('ascii') for _ in range(r.value('I'))]
    r.tag('TMPL')
    templates = []
    for _ in range(r.value('I')):
        templates.append(r.value('H'))
        r.take(44)
    result = copy.deepcopy(manifest)
    result['event_instances'], result['event_rules'] = [], {}
    counts = {}
    for row, mapping in r.instances(strings, templates):
        fingerprint = digest(row)
        occurrence = counts.get(fingerprint, 0)
        counts[fingerprint] = occurrence + 1
        if mapping >= len(masks):
            raise ValueError(f"Invalid event mask {mapping}: {map_path}")
        mask = masks[mapping]
        if len(mask) != len(flags) or set(mask) - {'0', '1'}:
            raise ValueError(f"Invalid event mask in {map_path}")
        # EMAL 0 suppresses this base placement when that event is active.
        rule = ''.join(flag + '=False;' for flag, value in zip(flags, mask) if value == '0')
        if rule:
            result['event_rules'][f'placement:{fingerprint}:{occurrence}'] = rule
    indexes = {name: i for i, name in enumerate(result['models'])}
    for flag in flags:
        # Names originate in data but must never escape the map directory.
        if not flag or any(c in flag for c in '/\\:') or flag in ('.', '..'):
            raise ValueError(f"Invalid map event name: {flag}")
        path = map_path.with_name(map_path.stem + '_' + flag + '.eve')
        if not path.is_file():
            previous = manifest if 'event_instances' in manifest else (cached or {})
            for original in previous.get('event_instances', []):
                if original.get('event', '').casefold() != flag.casefold():
                    continue
                row = copy.deepcopy(original)
                model = previous['models'][row['m']]
                if model not in indexes:
                    indexes[model] = len(result['models'])
                    result['models'].append(model)
                row['m'] = indexes[model]
                result['event_instances'].append(row)
            continue
        event = Reader(path)
        event.header()
        for row, _mapping in event.instances(strings, templates):
            model = row.pop('model')
            if model not in indexes:
                indexes[model] = len(result['models'])
                result['models'].append(model)
            row.update(m=indexes[model], event=flag.casefold())
            result['event_instances'].append(row)
    return result


if __name__ == '__main__':
    import argparse
    from level_data import atomic_json, read_json
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('maps', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    count = 0
    for source in sorted(args.maps.glob('*.map')):
        target = args.output / (source.stem + '.map.json')
        manifest = read_json(target)
        if manifest is not None:
            if '_level_override' in manifest:
                raise ValueError(f"Run the base map exporter before event conversion: {target}")
            atomic_json(target, with_event_layers(manifest, source))
            count += 1
    print(f'[maps] Event layers and base visibility masks exported for {count} maps')
