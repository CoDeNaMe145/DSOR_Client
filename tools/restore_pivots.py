#!/usr/bin/env python3
"""Restore Nebula rotation/scale pivots omitted by DSO_Godot's glTF export.

Usage: restore_pivots.py <export_win32/models> <assets> <DSO_Godot> [model ...]

Run after the other model postprocessors. Static pivots are baked into the
translation; animated pivots use ordinary glTF nodes so interpolation stays
exact, including between keys. No mesh data or animation samples are changed.
The original node indices and names stay intact. Rerunning is a no-op.
"""
import argparse
from pathlib import Path
import sys

from fix_materials_io import read_glb, write_glb


def add(a, b):
    return [x + y for x, y in zip(a, b)]


def sub(a, b):
    return [x - y for x, y in zip(a, b)]


def rotate(q, v):
    def cross(a, b):
        return [a[1]*b[2] - a[2]*b[1], a[2]*b[0] - a[0]*b[2], a[0]*b[1] - a[1]*b[0]]
    uv = [2 * x for x in cross(q[:3], v)]
    return add(v, add([q[3] * x for x in uv], cross(q[:3], uv)))


def node_paths(nodes):
    """Authored paths, ignoring helpers from an earlier pivot restoration."""
    parents = {c: i for i, node in enumerate(nodes) for c in node.get('children', [])}
    paths = {}
    for i, node in enumerate(nodes):
        if node.get('extras', {}).get('dsor_pivot_helper'):
            continue
        parts, at = [], i
        while at is not None:
            n = nodes[at]
            if not n.get('extras', {}).get('dsor_pivot_helper'):
                parts.append(n.get('name', ''))
            at = parents.get(at)
        paths[tuple(reversed(parts))] = i
    return paths


def restore(doc, pivots):
    """Apply {authored node path: (rotation pivot, scale pivot)}; return count.

    Nebula transform44::getmatrix uses matrix44::transformation. In glTF's
    column convention this is T(t) T(rp) R T(sp-rp) S T(-sp).
    https://github.com/gscept/nebula-trifid/blob/master/code/foundation/math/transform44.h
    """
    nodes = doc.get('nodes', [])
    paths = node_paths(nodes)
    channels = [c for a in doc.get('animations', []) for c in a['channels']]
    animated = {c['target']['node'] for c in channels}
    changed = 0

    def helper(owner, role, **fields):
        i = len(nodes)
        nodes.append(dict(name=f'__dsor_pivot_{owner}_{role}',
                          extras={'dsor_pivot_helper': True}, **fields))
        return i

    for path, (rp, sp) in pivots.items():
        i = paths.get(path)
        if i is None or not any((*rp, *sp)):
            continue
        node = nodes[i]
        if node.get('extras', {}).get('dsor_pivots'):
            continue
        # Source pivots belong to TRFN groups, never skinned mesh/joint nodes.
        if 'mesh' in node or 'skin' in node or 'matrix' in node:
            raise ValueError(f'Unexpected pivot node layout: {"/".join(path)}')
        rp, sp = list(rp), list(sp)
        node.setdefault('extras', {})['dsor_pivots'] = {'rotate': rp, 'scale': sp}
        changed += 1
        if i not in animated and not node.get('extras', {}).get('dsor_sprite'):
            r = node.get('rotation', [0, 0, 0, 1])
            s = node.get('scale', [1, 1, 1])
            correction = add(rp, rotate(r, sub(sub(sp, rp), [a*b for a, b in zip(s, sp)])))
            node['translation'] = add(node.get('translation', [0, 0, 0]), correction)
            continue

        # Keep the named/animated node and its rotation in place (also used by
        # camera-facing sprites). A parent supplies the rotation center.
        if any(rp):
            parent = helper(i, 'rotate_origin', translation=rp, children=[i])
            for other in nodes[:parent]:
                if 'children' in other:
                    other['children'] = [parent if c == i else c for c in other['children']]
            for scene in doc.get('scenes', []):
                scene['nodes'] = [parent if c == i else c for c in scene.get('nodes', [])]

        children = node.pop('children', [])
        tail = helper(i, 'restore_origin', translation=[-x for x in sp], children=children)
        if rp != sp:
            # Distinct centers require scale below rotation. Only scale tracks
            # change target; sampler sharing, times and interpolation are intact.
            scale = helper(i, 'scale', translation=sub(sp, rp),
                           scale=node.pop('scale', [1, 1, 1]), children=[tail])
            for c in channels:
                if c['target'].get('node') == i and c['target']['path'] == 'scale':
                    c['target']['node'] = scale
            tail = scale
        node['children'] = [tail]
    return changed


def source_pivots(model):
    paths, pivots = [], {}
    for node in model.nodes:
        path = (paths[node.parent] if node.parent >= 0 else ()) + (node.name,)
        paths.append(path)
        rp, sp = node.rotate_pivot[:3], node.scale_pivot[:3]
        if node.type == 'TRFN' and any((*rp, *sp)):
            pivots[path] = (rp, sp)
    return pivots


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('models', type=Path)
    ap.add_argument('assets', type=Path)
    ap.add_argument('godot', type=Path)
    ap.add_argument('model', nargs='*', help='relative model paths without .n3/.glb; default: all')
    args = ap.parse_args()
    sys.path.insert(0, str(args.godot))
    from dsoexp import n3

    sources = ([args.models / (m + '.n3') for m in args.model]
               if args.model else sorted(args.models.rglob('*.n3')))
    models = total = 0
    for source in sources:
        dest = args.assets / source.relative_to(args.models).with_suffix('.glb')
        if not dest.is_file():
            continue
        raw = source.read_bytes()
        if b'VIPR' not in raw and b'VIPS' not in raw:
            continue
        pivots = source_pivots(n3.loads(raw, str(source)))
        if not pivots:
            continue
        doc, rest = read_glb(dest)
        count = restore(doc, pivots)
        if count:
            write_glb(dest, doc, rest)
            total += count
            models += 1
    print(f'restore_pivots: {total} nodes restored in {models} models')


if __name__ == '__main__':
    main()
