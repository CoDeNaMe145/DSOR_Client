"""Navigation footprints (tables/navshapes.bin): the radius the release261 client
subtracts from a target's centre distance before a targeted melee cast
(Skills::ClientSkillValidator::Validate 0x14032bed0, range check 0x140a1b750),
instead of the monster's CapsuleRadius. The same parse as DSOR_Server's
gameplay/navigation_shapes.py (docs/MELEE_TARGETING.md there): width and depth
truncated to centimetres, vertex zero (width // 3, depth) of the native octagon,
its length / 100. small 0.525, medium 1.053, large 1.581, giant 4.215.
"""
import math
import os
import struct


def shape_radii(export):
    """{shape name: radius} from <export_win32>/tables/navshapes.bin; "default" is "small"."""
    data = open(os.path.join(export, "tables", "navshapes.bin"), "rb").read()
    if data[:4] != b"HSAN":
        raise ValueError("navshapes.bin: not a navigation shape table")
    version, count = struct.unpack_from("<II", data, 4)
    if version != 1:
        raise ValueError(f"navshapes.bin: version {version}")
    offset, radii = 12, {}
    for _ in range(count):
        (length,) = struct.unpack_from("<H", data, offset)
        offset += 2
        name = data[offset:offset + length].decode("utf-8")
        offset += length
        width, depth, _height = struct.unpack_from("<fff", data, offset)
        offset += 12
        x, z = int(width * 100), int(depth * 100)
        radii[name] = math.hypot(x // 3, z) / 100.0
    radii["default"] = radii["small"]
    return radii
