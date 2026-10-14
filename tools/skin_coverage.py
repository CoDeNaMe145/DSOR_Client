"""Export authored clothing coverage from tables/gfxskintags.bin.

Usage: python tools/skin_coverage.py <export_win32> <assets>
The version-2 table contains a u32 region mask and u16 layer per mesh name.
Higher layers cover matching regions on lower layers (body 0, armor 1,
special capes 2, costumes/cloaks 3, transformations 4).
"""
import json
from pathlib import Path
import struct


def read_skin_tags(data):
    if len(data) < 12 or data[:4] != b'GTSG':
        raise ValueError('invalid gfxskintags header')
    version, count = struct.unpack_from('<II', data, 4)
    if version != 2:
        raise ValueError(f'unsupported gfxskintags version {version}')
    offset = 12
    tags = {}
    try:
        for _ in range(count):
            length, = struct.unpack_from('<H', data, offset)
            offset += 2
            name = data[offset:offset + length].decode('utf8')
            offset += length
            mask, layer = struct.unpack_from('<IH', data, offset)
            offset += 6
            if not name or name in tags:
                raise ValueError(f'empty or duplicate skin tag: {name!r}')
            tags[name] = dict(mask=mask, layer=layer)
    except (struct.error, UnicodeDecodeError) as error:
        raise ValueError(f'truncated or invalid skin tag at byte {offset}') from error
    if offset != len(data):
        raise ValueError(f'unexpected trailing skin-tag data at byte {offset}')
    return tags


def export_part_lists(source, assets):
    tags = read_skin_tags((Path(source) / 'tables/gfxskintags.bin').read_bytes())
    for skeleton in ('uniskel', 'uniskel_dwarf'):
        directory = Path(assets) / 'characters' / skeleton
        if not (directory / 'parts').is_dir():
            continue
        parts = sorted(path.stem for path in (directory / 'parts').glob('*.glb'))
        coverage = {name: tags[name] for name in parts if name in tags}
        (directory / 'parts.json').write_text(json.dumps(dict(parts=parts, tags=coverage), separators=(',', ':')) + '\n', encoding='utf8')
        print(f'{skeleton}/parts.json: {len(parts)} parts, {len(coverage)} coverage rules')


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source')
    parser.add_argument('assets')
    args = parser.parse_args()
    export_part_lists(args.source, args.assets)
