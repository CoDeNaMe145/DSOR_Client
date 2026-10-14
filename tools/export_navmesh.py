#!/usr/bin/env python3
"""Export every map's walkable ground (PathEngine .tok) for the client.

Usage: export_navmesh.py <export_win32> <assets> [--server <dso_server>]

Writes <assets>/maps/<map>.nav.bin: u32 triangle count, then per triangle 9 little-
endian f32 -- three corners (x, elevation, z) in the map frame, the frame of the
placement manifests. --server reads with the production server's PathEngine
parser. Without it, EXPERIMENTAL points to the legacy dsor/navmesh.py reader.
"""
import argparse
import importlib.util
import os, struct, sys
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source")
parser.add_argument("assets")
parser.add_argument("--server", type=Path, help="Production dso_server package directory")
args = parser.parse_args()

if args.server:
    # Load just the geometry reader, without starting/importing the server.
    path = args.server / "gameplay" / "pathengine_navigation.py"
    spec = importlib.util.spec_from_file_location("dsor_export_navigation", path)
    reader = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = reader
    spec.loader.exec_module(reader)

    def read_triangles(name, root):
        # World X/Y/Z, already axis-swapped by the production reader. Avoid
        # constructing server routing caches when exporting only geometry.
        vertices, triangles = reader._read_mesh3d((Path(root) / name / f"{name}.tok").read_bytes())
        return [tuple(vertices[i] for i in triangle.vertices) for triangle in triangles]
else:
    sys.path.insert(0, os.environ.get("EXPERIMENTAL", os.path.expanduser("~/Documents/experimental")))
    from dsor import navmesh

    def read_triangles(name, root):
        mesh = navmesh.load(name, root)
        if mesh is None:
            return None
        return [tuple((x, elevation, ground) for x, ground, elevation in corners)
                for corners, _face in mesh.triangles]

src, assets = args.source, args.assets
root = os.path.join(src, "navigation")
out_dir = os.path.join(assets, "maps")
os.makedirs(out_dir, exist_ok=True)
done = 0
for name in sorted(os.listdir(root)):
    if not os.path.exists(os.path.join(root, name, f"{name}.tok")):
        continue
    triangles = read_triangles(name, root)
    if triangles is None:
        continue
    with open(os.path.join(out_dir, f"{name}.nav.bin"), "wb") as f:
        f.write(struct.pack("<I", len(triangles)))
        for corners in triangles:
            for vertex in corners:
                f.write(struct.pack("<fff", *vertex))
    done += 1
print(f"{done} navigation meshes")
