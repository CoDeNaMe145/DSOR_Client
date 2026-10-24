"""PathEngine ground meshes (<export_win32>/navigation/<map>/<map>.tok) as world
triangles, with no game server.

A .tok is PathEngine's tokenised XML. Two tables open it: the element names
(NUL-terminated UTF-8, ended by an empty name), then the attributes (a type byte
and a NUL-terminated name, ended by type 0). The tree follows as one-based
tokens into them: an element's token, its attributes (token and value, ended by
0), its children (ended by 0). Value types, little-endian: 1 a NUL-terminated
string, 2/3/4 i32/i16/i8, 5/6/7 u32/u16/u8. Each file declares one type per
attribute name: i8 vertex indices in small maps, z as decimal strings in some.

The ground is <mesh><mesh3D><verts><vert x y z/>..</verts><tris><tri
edgeNStartVert edgeNConnection edgeNStartZ/>..</tris></mesh3D>, then
<mesh3DPartitioning> and the 2D mapping, whose children are raw binary blocks
(token 0xff): reading stops at the partitioning. Absent attributes are 0 (a fifth
of the vertices have no z). Units are centimetres, z up, the horizontal plane
swapped: world (x, y, z) = PathEngine (y, z, x) / 100. edgeNStartZ, on 6% of the
corners and always 1 to 42 cm off its vertex's z (the corner's own height?), is
ignored, as by the server.

Checked against DSOR_Server's reader (gameplay/pathengine_navigation.py,
_read_mesh3d): export_navmesh.py writes byte-identical maps/<map>.nav.bin with
either.
"""
import struct

_NUMBERS = {kind: struct.Struct(f) for kind, f in
            {2: "<i", 3: "<h", 4: "<b", 5: "<I", 6: "<H", 7: "<B"}.items()}
_STRING = 1
_UNITS_PER_METRE = 100.0


def _string(data, at):
    end = data.find(0, at)
    if end < 0:
        raise ValueError(".tok: unterminated string")
    return data[at:end].decode("utf-8"), end + 1


def read_mesh(data):
    """(vertices, triangles) of .tok bytes: world (x, y, z) metres, y up, and
    each triangle's three vertex indices (its edges' start vertices)."""
    try:
        return _read_mesh(data)
    except (IndexError, struct.error):
        raise ValueError(".tok: truncated") from None


def read_triangles(data):
    """Each ground triangle of .tok bytes as three world (x, y, z) corners."""
    vertices, triangles = read_mesh(data)
    return [tuple(vertices[i] for i in triangle) for triangle in triangles]


def _read_mesh(data):
    elements, at = [], 0
    while True:
        name, at = _string(data, at)
        if not name:
            break
        elements.append(name)
    attributes = []
    while True:
        kind = data[at]
        at += 1
        if not kind:
            break
        name, at = _string(data, at)
        attributes.append((name, kind))
    if "mesh3D" not in elements:
        raise ValueError(".tok: not a PathEngine ground mesh")
    stop = elements.index("mesh3DPartitioning") + 1 if "mesh3DPartitioning" in elements else -1

    vertices, triangles, depth = [], [], 0
    while True:
        token = data[at]
        at += 1
        if not token and depth:  # the open element's children end
            depth -= 1
            if not depth:
                break
            continue
        if token == stop:
            break
        if not 0 < token <= len(elements):
            raise ValueError(f".tok: element token {token} at {at - 1}")
        element, values = elements[token - 1], {}
        while True:
            token = data[at]
            at += 1
            if not token:
                break
            if token > len(attributes):
                raise ValueError(f".tok: attribute token {token} at {at - 1}")
            name, kind = attributes[token - 1]
            if kind == _STRING:
                values[name], at = _string(data, at)
            elif kind in _NUMBERS:
                number = _NUMBERS[kind]
                values[name] = number.unpack_from(data, at)[0]
                at += number.size
            else:
                raise ValueError(f".tok: attribute type {kind}")
        if element == "vert":
            vertices.append(tuple(float(values.get(axis, 0)) / _UNITS_PER_METRE for axis in "yzx"))
        elif element == "tri":
            corners = [values.get(f"edge{edge}StartVert") for edge in range(3)]
            if None in corners:
                raise ValueError(f".tok: triangle {len(triangles)} without its corners")
            triangles.append(tuple(int(corner) for corner in corners))
        depth += 1
    if triangles and not (min(map(min, triangles)) >= 0 and max(map(max, triangles)) < len(vertices)):
        raise ValueError(".tok: a triangle's vertex is out of range")
    return vertices, triangles
