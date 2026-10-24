#!/usr/bin/env python3
"""Export every map's walkable ground (PathEngine .tok) for the client.

Usage: export_navmesh.py <export_win32> <assets> [--server <dso_server>]

Writes <assets>/maps/<map>.nav.bin: u32 triangle count, then per triangle 9 little-
endian f32 -- three corners (x, elevation, z) in the map frame, the frame of the
placement manifests. The meshes are the game's own
<export_win32>/navigation/<map>/<map>.tok, read by navmesh_tok.py. --server
reads them with a dso_server checkout's PathEngine parser instead
(gameplay/pathengine_navigation.py), to compare: the files are byte-identical.
"""
import argparse
import importlib.util
import os, struct, sys
from pathlib import Path

sys.path.insert(0, os.path.dirname(__file__))
import navmesh_tok  # noqa: E402

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source")
parser.add_argument("assets")
parser.add_argument("--server", type=Path, help="dso_server package directory: read with its parser instead")
args = parser.parse_args()

read_triangles = navmesh_tok.read_triangles
if args.server:
    # Load just the geometry reader, without starting/importing the server.
    path = args.server / "gameplay" / "pathengine_navigation.py"
    spec = importlib.util.spec_from_file_location("dsor_export_navigation", path)
    reader = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = reader
    spec.loader.exec_module(reader)

    def read_triangles(data):
        # World X/Y/Z, already axis-swapped by the production reader. Avoid
        # constructing server routing caches when exporting only geometry.
        vertices, triangles = reader._read_mesh3d(data)
        return [tuple(vertices[i] for i in triangle.vertices) for triangle in triangles]

src, assets = args.source, args.assets
root = os.path.join(src, "navigation")
out_dir = os.path.join(assets, "maps")
os.makedirs(out_dir, exist_ok=True)
done = 0
for name in sorted(os.listdir(root)):
    tok = os.path.join(root, name, f"{name}.tok")
    if not os.path.exists(tok):
        continue
    with open(tok, "rb") as f:
        triangles = read_triangles(f.read())
    with open(os.path.join(out_dir, f"{name}.nav.bin"), "wb") as f:
        f.write(struct.pack("<I", len(triangles)))
        for corners in triangles:
            for vertex in corners:
                f.write(struct.pack("<fff", *vertex))
    done += 1
print(f"{done} navigation meshes")
