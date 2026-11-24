"""Writes physics.glb, the baked physics fixture, with the standard library.

The scene holds `Ground`, a 6 m square at y = 0 facing up; `Crate.0` to
`Crate.9`, 0.2 m cubes stacked at x = -1.5 from the ground up; `Domino.0` to
`Domino.7`, 0.05 x 0.4 x 0.2 m slabs standing in a row along +X from x = 0,
0.18 m apart; `Pusher`, a 0.1 m cube whose one clip slides it from x = -0.5 to
x = 0.05 at y = 0.3 over 1 s, tipping the first domino; `Coin`, a cylinder
0.1 m across and 0.012 m thick at (0, 3, -2), a spawner template; and a
camera at (0, 1.2, 4) m looking at (0, 0.4, 0) with a 45 degree vertical
field of view.

Run: python crates/director-3d/tests/fixtures/make_physics.py
"""

import json
import math
import struct
from pathlib import Path


def box(sx, sy, sz):
    """A box centred on the origin with flat normals: positions, normals, indices."""
    hx, hy, hz = sx / 2, sy / 2, sz / 2
    faces = [
        ((1, 0, 0), [(hx, -hy, -hz), (hx, hy, -hz), (hx, hy, hz), (hx, -hy, hz)]),
        ((-1, 0, 0), [(-hx, -hy, hz), (-hx, hy, hz), (-hx, hy, -hz), (-hx, -hy, -hz)]),
        ((0, 1, 0), [(-hx, hy, -hz), (-hx, hy, hz), (hx, hy, hz), (hx, hy, -hz)]),
        ((0, -1, 0), [(-hx, -hy, hz), (-hx, -hy, -hz), (hx, -hy, -hz), (hx, -hy, hz)]),
        ((0, 0, 1), [(-hx, -hy, hz), (hx, -hy, hz), (hx, hy, hz), (-hx, hy, hz)]),
        ((0, 0, -1), [(hx, -hy, -hz), (-hx, -hy, -hz), (-hx, hy, -hz), (hx, hy, -hz)]),
    ]
    positions, normals, indices = [], [], []
    for normal, corners in faces:
        base = len(positions)
        positions += corners
        normals += [normal] * 4
        indices += [base, base + 1, base + 2, base, base + 2, base + 3]
    return positions, normals, indices


def cylinder(radius, height, segments=32):
    """A closed cylinder along Y with flat caps and smooth sides."""
    h = height / 2
    positions, normals, indices = [], [], []
    ring = [(math.cos(2 * math.pi * k / segments), math.sin(2 * math.pi * k / segments)) for k in range(segments)]
    for k, (c, s) in enumerate(ring):
        positions += [(radius * c, -h, radius * s), (radius * c, h, radius * s)]
        normals += [(c, 0, s), (c, 0, s)]
    for k in range(segments):
        a, b = 2 * k, 2 * ((k + 1) % segments)
        indices += [a, a + 1, b + 1, a, b + 1, b]
    for y, ny in ((h, 1), (-h, -1)):
        centre = len(positions)
        positions.append((0, y, 0))
        normals.append((0, ny, 0))
        for c, s in ring:
            positions.append((radius * c, y, radius * s))
            normals.append((0, ny, 0))
        for k in range(segments):
            a, b = centre + 1 + k, centre + 1 + (k + 1) % segments
            indices += [centre, b, a] if ny > 0 else [centre, a, b]
    return positions, normals, indices


def main():
    meshes = {
        "Ground": (box(6, 0.0, 6), [0.45, 0.45, 0.45, 1], 0.0, 0.8),
        "Crate": (box(0.2, 0.2, 0.2), [0.55, 0.35, 0.18, 1], 0.0, 0.7),
        "Domino": (box(0.05, 0.4, 0.2), [0.92, 0.92, 0.9, 1], 0.0, 0.4),
        "Pusher": (box(0.1, 0.1, 0.1), [0.85, 0.1, 0.08, 1], 0.0, 0.5),
        "Coin": (cylinder(0.05, 0.012), [1.0, 0.76, 0.3, 1], 1.0, 0.3),
    }
    # The ground box has no thickness: keep only its upward face.
    ground_positions, ground_normals, _ = meshes["Ground"][0]
    meshes["Ground"] = (
        (ground_positions[8:12], ground_normals[8:12], [0, 1, 2, 0, 2, 3]),
        *meshes["Ground"][1:],
    )

    chunks, accessors = [], []

    def add(data, accessor):
        data += b"\0" * (-len(data) % 4)
        offset = sum(len(c) for c in chunks)
        chunks.append(data)
        accessor["bufferView"] = len(chunks) - 1
        accessors.append(accessor)
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(data)})
        return len(accessors) - 1

    views = []

    def vec(points, kind):
        width = len(points[0])
        lo = [min(p[k] for p in points) for k in range(width)]
        hi = [max(p[k] for p in points) for k in range(width)]
        data = b"".join(struct.pack(f"<{width}f", *p) for p in points)
        return add(data, {"componentType": 5126, "count": len(points), "type": kind, "min": lo, "max": hi})

    def scalars(values):
        data = struct.pack(f"<{len(values)}f", *values)
        return add(data, {"componentType": 5126, "count": len(values), "type": "SCALAR", "min": [min(values)], "max": [max(values)]})

    def index(values):
        return add(struct.pack(f"<{len(values)}H", *values), {"componentType": 5123, "count": len(values), "type": "SCALAR"})

    gltf_meshes, materials, mesh_ids = [], [], {}
    for name, ((positions, normals, indices), color, metallic, roughness) in meshes.items():
        materials.append(
            {"name": name, "pbrMetallicRoughness": {"baseColorFactor": color, "metallicFactor": metallic, "roughnessFactor": roughness}}
        )
        primitive = {
            "attributes": {"POSITION": vec(positions, "VEC3"), "NORMAL": vec(normals, "VEC3")},
            "indices": index(indices),
            "material": len(materials) - 1,
        }
        mesh_ids[name] = len(gltf_meshes)
        gltf_meshes.append({"name": name, "primitives": [primitive]})

    nodes = [{"name": "Ground", "mesh": mesh_ids["Ground"]}]
    for i in range(10):
        nodes.append({"name": f"Crate.{i}", "mesh": mesh_ids["Crate"], "translation": [-1.5, 0.1 + 0.2 * i, 0]})
    for i in range(8):
        nodes.append({"name": f"Domino.{i}", "mesh": mesh_ids["Domino"], "translation": [0.18 * i, 0.2, 0]})
    pusher = len(nodes)
    nodes.append({"name": "Pusher", "mesh": mesh_ids["Pusher"], "translation": [-0.5, 0.3, 0]})
    nodes.append({"name": "Coin", "mesh": mesh_ids["Coin"], "translation": [0, 3, -2]})
    pitch = -math.atan2(0.8, 4.0)
    nodes.append({
        "name": "Camera",
        "camera": 0,
        "translation": [0, 1.2, 4],
        "rotation": [math.sin(pitch / 2), 0, 0, math.cos(pitch / 2)],
    })

    times = scalars([0.0, 1.0])
    slide = vec([(-0.5, 0.3, 0), (0.05, 0.3, 0)], "VEC3")
    animation = {
        "name": "Push",
        "samplers": [{"input": times, "output": slide, "interpolation": "LINEAR"}],
        "channels": [{"sampler": 0, "target": {"node": pusher, "path": "translation"}}],
    }

    binary = b"".join(chunks)
    gltf = {
        "asset": {"version": "2.0", "generator": "make_physics.py"},
        "scene": 0,
        "scenes": [{"nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "materials": materials,
        "animations": [animation],
        "cameras": [{"type": "perspective", "perspective": {"yfov": math.radians(45), "znear": 0.05, "zfar": 100}}],
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": len(binary)}],
    }
    text = json.dumps(gltf, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    body = struct.pack("<II", len(text), 0x4E4F534A) + text + struct.pack("<II", len(binary), 0x004E4942) + binary
    out = Path(__file__).with_name("physics.glb")
    out.write_bytes(struct.pack("<III", 0x46546C67, 2, 12 + len(body)) + body)
    print(f"wrote {out} ({12 + len(body)} bytes)")


if __name__ == "__main__":
    main()
