"""navmesh_tok reads a PathEngine ground mesh as world triangles; export_navmesh
writes them. A synthetic .tok: every value type the shipped maps use, absent
attributes, and binary partitioning data the reader must not walk into."""
import importlib.util
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

from navmesh_tok import read_mesh, read_triangles

ELEMENTS = ["mesh", "mesh3D", "verts", "vert", "tris", "tri", "mesh3DPartitioning"]
ATTRIBUTES = [("x", 3), ("y", 2), ("z", 1), ("edge0StartVert", 4), ("edge1StartVert", 7),
              ("edge2StartVert", 6), ("edge0Connection", 3)]
FORMATS = {2: "<i", 3: "<h", 4: "<b", 6: "<H", 7: "<B"}
SERVER = Path(__file__).resolve().parents[1] / ".local" / "dso_server"


def element(name, values, children=()):
    out = bytes([ELEMENTS.index(name) + 1])
    for attribute, value in values.items():
        index = [n for n, _ in ATTRIBUTES].index(attribute)
        kind = ATTRIBUTES[index][1]
        out += bytes([index + 1]) + (f"{value}\0".encode() if kind == 1 else struct.pack(FORMATS[kind], value))
    return out + b"\0" + b"".join(children) + b"\0"


def tok(corners=(2, 0, 1), elements=ELEMENTS):
    head = b"".join(f"{n}\0".encode() for n in elements) + b"\0"
    head += b"".join(bytes([kind]) + f"{n}\0".encode() for n, kind in ATTRIBUTES) + b"\0"
    verts = [element("vert", {"x": 150, "y": -250, "z": 100}),
             element("vert", {"x": -300, "y": 40000}),
             element("vert", {"y": 7, "z": -20})]
    edges = {f"edge{e}StartVert": corner for e, corner in enumerate(corners)}
    tris = [element("tri", {**edges, "edge0Connection": 5})]
    mesh3d = element("mesh3D", {}, [element("verts", {}, verts), element("tris", {}, tris)])
    # <mesh>, <mesh3D>, then the partitioning, whose binary children are not tokens.
    return head + bytes([1, 0]) + mesh3d + bytes([7]) + b"\xff\x08\x05\x8a\0\0\0\x01"


VERTICES = [(-2.5, 1.0, 1.5), (400.0, 0.0, -3.0), (0.07, -0.2, 0.0)]


class NavmeshTokTests(unittest.TestCase):
    def test_world_vertices_and_triangles(self):
        vertices, triangles = read_mesh(tok())
        self.assertEqual(vertices, VERTICES)
        self.assertEqual(triangles, [(2, 0, 1)])
        self.assertEqual(read_triangles(tok()), [(VERTICES[2], VERTICES[0], VERTICES[1])])

    def test_broken_files_are_errors(self):
        whole = tok()
        for data in (whole[:whole.index(b"\x07\xff")], tok(corners=(0, 1, 9)),
                     tok(elements=["mesh", "notAMesh"] + ELEMENTS[2:])):
            with self.assertRaises(ValueError):
                read_mesh(data)

    def test_export_writes_nav_bin(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "source/navigation/m").mkdir(parents=True)
            (root / "source/navigation/empty").mkdir()
            (root / "source/navigation/m/m.tok").write_bytes(tok())
            subprocess.run([sys.executable, str(Path(__file__).with_name("export_navmesh.py")),
                            str(root / "source"), str(root / "assets")], check=True, capture_output=True)
            corners = [VERTICES[2], VERTICES[0], VERTICES[1]]
            self.assertEqual((root / "assets/maps/m.nav.bin").read_bytes(),
                             struct.pack("<I9f", 1, *(c for corner in corners for c in corner)))
            self.assertEqual(sorted(p.name for p in (root / "assets/maps").iterdir()), ["m.nav.bin"])

    @unittest.skipUnless((SERVER / "gameplay/pathengine_navigation.py").exists(), "no dso_server checkout")
    def test_matches_the_server_reader(self):
        spec = importlib.util.spec_from_file_location("server_navigation", SERVER / "gameplay/pathengine_navigation.py")
        server = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = server
        spec.loader.exec_module(server)
        vertices, triangles = server._read_mesh3d(tok())
        self.assertEqual((list(vertices), [t.vertices for t in triangles]), read_mesh(tok()))


if __name__ == "__main__":
    unittest.main()
