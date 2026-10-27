"""Writes motion.html, the Director showcase reel.

The dot grid, globe and orbit type are hundreds of elements, so they are generated here.
Everything else is motion.template.html. Run: python build.py
"""
import math
from pathlib import Path

BAR = 1.904  # 4 beats of assets/music.wav at ~126 BPM

# ---------- A. playhead ruler ----------
ticks = []
for i in range(57):
    major = i % 4 == 0
    h = 30 if major else 14
    ticks.append(f'<div class="tk" style="left:{120 + i * 30 - 1}px;top:{540 - h // 2}px;height:{h}px"></div>')

# ---------- D. dot grid ----------
COLS, ROWS, SP, X0, Y0 = 25, 11, 72, 96, 180
dots = [
    f'<div class="d" style="left:{X0 + c * SP - 6}px;top:{Y0 + r * SP - 6}px"></div>'
    for r in range(ROWS)
    for c in range(COLS)
]

# ---------- E. globe ----------
R = 300
globe = []
for k in range(6):
    globe.append(f'<div class="mer" style="transform:rotateY({k * 30}deg)"></div>')
for y in (-200, -100, 0, 100, 200):
    rr = math.sqrt(R * R - y * y)
    globe.append(
        f'<div class="{"lat eq" if y == 0 else "lat"}" style="left:{-rr:.1f}px;top:{-rr:.1f}px;width:{2 * rr:.1f}px;height:{2 * rr:.1f}px;'
        f'border-radius:{rr:.1f}px;transform:translateY({y}px) rotateX(90deg)"></div>'
    )
N = 96
golden = math.pi * (3 - math.sqrt(5))
for i in range(N):
    yy = 1 - 2 * (i + 0.5) / N
    lat = math.degrees(math.asin(yy))
    lon = math.degrees(i * golden) % 360
    cls = "sd hot" if i % 7 == 3 else "sd"
    globe.append(f'<div class="{cls}" style="transform:rotateY({lon:.1f}deg) rotateX({lat:.1f}deg) translateZ({R}px)"></div>')

slots = list("DIRECTOR") + ["", "·", ""] + list("NATIVE") + [""] + list("3D") + ["", "·", ""]
slots += [""] * (24 - len(slots))
orbit = []
for k, ch in enumerate(slots):
    if not ch:
        continue
    a = k * 360 / len(slots)
    orbit.append(f'<div class="ol" style="transform:rotateY({a:.1f}deg) translateZ(440px)">{ch}</div>')
    orbit.append(f'<div class="ol back" style="transform:rotateY({a:.1f}deg) translateZ(440px) rotateY(180deg)">{ch}</div>')

# ---------- F. ease curves, sampled from GSAP's own formulas ----------
def expo_in(p):
    return 2 ** (10 * (p - 1)) if p else 0.0

def expo_inout(p):
    return expo_in(2 * p) / 2 if p < 0.5 else 1 - expo_in(2 * (1 - p)) / 2

def back_out(p, s=2.0):
    q = 1 - p
    return 1 - q * q * ((s + 1) * q - s)

def elastic_out(p, amplitude=1.0, period=0.35):
    p1 = max(amplitude, 1.0)
    p2 = 2 * math.pi / period
    p3 = period / (2 * math.pi) * math.asin(1 / p1)
    return p1 * 2 ** (-10 * p) * math.sin((p - p3) * p2) + 1 if p else 0.0

CW, VY0, VSPAN = 520, 460, 340
curves = []
for k, f in enumerate((expo_inout, back_out, elastic_out)):
    pts = [(p * CW, VY0 - f(p) * VSPAN) for p in (j / 160 for j in range(161))]
    run = [0.0]
    for a, b in zip(pts, pts[1:]):
        run.append(run[-1] + math.dist(a, b))
    length = run[-1]
    d = "M " + " L ".join(f"{x:.1f} {y:.1f}" for x, y in pts)
    # The tracer dot moves linearly in x, so the drawn tip follows it when the
    # dash offset is sampled evenly in time at the arc length up to x.
    values = ";".join(f"{length - run[j * 4]:.1f}" for j in range(41))
    begin = (15.40, 16.66, 17.92)[k]
    fade = "" if k == 2 else f'<animate attributeName="opacity" from="1" to="0" begin="{begin + 1.1:.2f}s" dur="0.15s" fill="freeze"/>'
    curves.append(
        f'<g>{fade}<path d="{d}" fill="none" stroke="#141311" stroke-width="6" stroke-linecap="round" '
        f'stroke-dasharray="{length:.1f}" stroke-dashoffset="{length:.1f}">'
        f'<animate attributeName="stroke-dashoffset" values="{values}" begin="{begin:.2f}s" dur="0.55s" fill="freeze"/></path></g>'
    )

html = Path(__file__).with_name("motion.template.html").read_text(encoding="utf-8")
for key, value in {
    "TICKS": ticks,
    "DOTS": dots,
    "GLOBE": globe,
    "ORBIT": orbit,
    "CURVES": curves,
}.items():
    html = html.replace("{{" + key + "}}", "\n".join(value))
Path(__file__).with_name("motion.html").write_text(html, encoding="utf-8")
print("motion.html", len(html), "bytes")
