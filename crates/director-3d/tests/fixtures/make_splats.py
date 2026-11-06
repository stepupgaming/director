"""Writes splats.ply: two Gaussian splat balls in the 3DGS PLY layout.

A red ball of radius 0.5 m sits at x = -0.6 and a green one at x = +0.6, each
768 isotropic gaussians 3 cm across on a Fibonacci spiral through the ball,
nearly opaque. Run with any Python 3: python make_splats.py
"""

import math
import struct
from pathlib import Path

SH_C0 = 0.28209479177387814
COUNT = 768
FIELDS = ["x", "y", "z", "nx", "ny", "nz", "f_dc_0", "f_dc_1", "f_dc_2", "opacity",
          "scale_0", "scale_1", "scale_2", "rot_0", "rot_1", "rot_2", "rot_3"]


def ball(cx, color):
    golden = math.pi * (3.0 - math.sqrt(5.0))
    for i in range(COUNT):
        # Shells by the cube root keep the ball evenly filled.
        r = 0.5 * ((i + 0.5) / COUNT) ** (1.0 / 3.0)
        y = 1.0 - 2.0 * (i + 0.5) / COUNT
        ring = math.sqrt(max(0.0, 1.0 - y * y))
        a = golden * i
        p = (cx + r * ring * math.cos(a), r * y, r * ring * math.sin(a))
        dc = [(c - 0.5) / SH_C0 for c in color]
        scale = math.log(0.03)
        yield [*p, 0.0, 0.0, 0.0, *dc, 4.0, scale, scale, scale, 1.0, 0.0, 0.0, 0.0]


def main():
    rows = [*ball(-0.6, (0.9, 0.1, 0.1)), *ball(0.6, (0.1, 0.8, 0.2))]
    header = "ply\nformat binary_little_endian 1.0\nelement vertex %d\n" % len(rows)
    header += "".join("property float %s\n" % f for f in FIELDS) + "end_header\n"
    body = b"".join(struct.pack("<%df" % len(FIELDS), *row) for row in rows)
    out = Path(__file__).with_name("splats.ply")
    out.write_bytes(header.encode("ascii") + body)
    print(out, len(rows), "gaussians")


if __name__ == "__main__":
    main()
