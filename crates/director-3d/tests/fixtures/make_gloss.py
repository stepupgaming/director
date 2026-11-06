"""Writes gloss.glb, the screen-space reflection fixture, with the standard library.

The scene holds `Floor`, an 8 m square at y = 0 facing up in a dark grey
dielectric with a perceptual roughness of 0.05, and the camera of
`mirror.glb`: at (0, 1.2, 5) m pitched down to look at (0, 0.3, 0) with a 40
degree vertical field of view.

Run: python crates/director-3d/tests/fixtures/make_gloss.py
"""

import json
import math
import struct
from pathlib import Path

FLOOR = [(-4, 0, -4), (-4, 0, 4), (4, 0, 4), (4, 0, -4)]
FLOOR_INDICES = [0, 1, 2, 0, 2, 3]
UP = [(0, 1, 0)] * 4


def main():
    positions = b"".join(struct.pack("<3f", *p) for p in FLOOR)
    normals = b"".join(struct.pack("<3f", *n) for n in UP)
    indices = struct.pack(f"<{len(FLOOR_INDICES)}H", *FLOOR_INDICES)
    indices += b"\0" * (-len(indices) % 4)
    chunks = [positions, normals, indices]
    views, offset = [], 0
    for chunk in chunks:
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(chunk)})
        offset += len(chunk)
    binary = b"".join(chunks)
    pitch = -math.atan2(0.9, 5.0)
    gltf = {
        "asset": {"version": "2.0", "generator": "make_gloss.py"},
        "scene": 0,
        "scenes": [{"nodes": [0, 1]}],
        "nodes": [
            {"name": "Floor", "mesh": 0},
            {
                "name": "Camera",
                "camera": 0,
                "translation": [0, 1.2, 5],
                "rotation": [math.sin(pitch / 2), 0, 0, math.cos(pitch / 2)],
            },
        ],
        "materials": [
            {
                "name": "Gloss",
                "pbrMetallicRoughness": {"baseColorFactor": [0.05, 0.05, 0.05, 1], "metallicFactor": 0, "roughnessFactor": 0.05},
            }
        ],
        "meshes": [{"name": "Floor", "primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}, "indices": 2, "material": 0}]}],
        "cameras": [{"type": "perspective", "perspective": {"yfov": math.radians(40), "znear": 0.05, "zfar": 100}}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [-4, 0, -4], "max": [4, 0, 4]},
            {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5123, "count": len(FLOOR_INDICES), "type": "SCALAR"},
        ],
        "bufferViews": views,
        "buffers": [{"byteLength": len(binary)}],
    }
    text = json.dumps(gltf, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    body = struct.pack("<II", len(text), 0x4E4F534A) + text + struct.pack("<II", len(binary), 0x004E4942) + binary
    out = Path(__file__).with_name("gloss.glb")
    out.write_bytes(struct.pack("<III", 0x46546C67, 2, 12 + len(body)) + body)
    print(f"wrote {out} ({12 + len(body)} bytes)")


if __name__ == "__main__":
    main()
