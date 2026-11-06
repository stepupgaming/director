"""Writes mirror.glb, the plate reflection fixture, with the standard library.

The scene holds `Floor`, an 8 m square at y = 0 facing up; `Ridge`, two
triangles folded along an edge at x = 20 m, out of the camera's view, that lie
in no single plane; and a camera at (0, 1.2, 5) m pitched down to look at
(0, 0.3, 0) with a 40 degree vertical field of view.

Run: python crates/director-3d/tests/fixtures/make_mirror.py
"""

import json
import math
import struct
from pathlib import Path

FLOOR = [(-4, 0, -4), (-4, 0, 4), (4, 0, 4), (4, 0, -4)]
FLOOR_INDICES = [0, 1, 2, 0, 2, 3]
RIDGE = [(0, 0, 0), (0, 0, 1), (1, 0, 0), (1, 1, 1)]
RIDGE_INDICES = [0, 1, 2, 2, 1, 3]
UP = [(0, 1, 0)] * 4


def floats(points):
    return b"".join(struct.pack("<3f", *p) for p in points)


def shorts(indices):
    data = struct.pack(f"<{len(indices)}H", *indices)
    return data + b"\0" * (-len(data) % 4)


def main():
    chunks = [floats(FLOOR), floats(UP), shorts(FLOOR_INDICES), floats(RIDGE), floats(UP), shorts(RIDGE_INDICES)]
    views, offset = [], 0
    for chunk in chunks:
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(chunk)})
        offset += len(chunk)
    binary = b"".join(chunks)

    def vec3(view, points):
        lo = [min(p[k] for p in points) for k in range(3)]
        hi = [max(p[k] for p in points) for k in range(3)]
        return {"bufferView": view, "componentType": 5126, "count": len(points), "type": "VEC3", "min": lo, "max": hi}

    def index(view, indices):
        return {"bufferView": view, "componentType": 5123, "count": len(indices), "type": "SCALAR"}

    accessors = [
        vec3(0, FLOOR), vec3(1, UP), index(2, FLOOR_INDICES),
        vec3(3, RIDGE), vec3(4, UP), index(5, RIDGE_INDICES),
    ]
    pitch = -math.atan2(0.9, 5.0)
    gltf = {
        "asset": {"version": "2.0", "generator": "make_mirror.py"},
        "scene": 0,
        "scenes": [{"nodes": [0, 1, 2]}],
        "nodes": [
            {"name": "Floor", "mesh": 0},
            {"name": "Ridge", "mesh": 1, "translation": [20, 0, 0]},
            {
                "name": "Camera",
                "camera": 0,
                "translation": [0, 1.2, 5],
                "rotation": [math.sin(pitch / 2), 0, 0, math.cos(pitch / 2)],
            },
        ],
        "meshes": [
            {"name": "Floor", "primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}, "indices": 2}]},
            {"name": "Ridge", "primitives": [{"attributes": {"POSITION": 3, "NORMAL": 4}, "indices": 5}]},
        ],
        "cameras": [{"type": "perspective", "perspective": {"yfov": math.radians(40), "znear": 0.05, "zfar": 100}}],
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": len(binary)}],
    }
    text = json.dumps(gltf, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    body = struct.pack("<II", len(text), 0x4E4F534A) + text + struct.pack("<II", len(binary), 0x004E4942) + binary
    out = Path(__file__).with_name("mirror.glb")
    out.write_bytes(struct.pack("<III", 0x46546C67, 2, 12 + len(body)) + body)
    print(f"wrote {out} ({12 + len(body)} bytes)")


if __name__ == "__main__":
    main()
