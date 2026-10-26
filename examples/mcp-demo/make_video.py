"""Build the demo video from the real session artifacts: session.jsonl, frames/*.png, export.mp4.
Every JSON line shown comes from session.jsonl (long values elided with "…"); every frame is a saved render."""
import json, os, re, subprocess, sys, textwrap
from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "director-agent.mp4")
W, H, FPS = 1600, 900, 30
FONTS = os.path.join(HERE, "..", "assets", "fonts")
CORE = os.path.join(HERE, "..", "..", "crates", "director-core", "assets", "fonts")
mono = ImageFont.truetype(os.path.join(FONTS, "JetBrainsMono-Regular.ttf"), 15)
sans_b = ImageFont.truetype(os.path.join(CORE, "Inter-Bold.ttf"), 24)
sans = ImageFont.truetype(os.path.join(CORE, "Inter-Regular.ttf"), 17)
BG, PANEL, FG, DIM = (10, 10, 15), (18, 17, 26), (230, 226, 215), (120, 117, 138)
OUTC, INC, ERR = (255, 179, 71), (42, 212, 200), (255, 90, 54)
SKIP = ("element_ids", "manifest", "changed_nodes", "audio_tracks", "inputSchema", "capabilities", "serverInfo", "outputSchema")


def elide(v):
    if isinstance(v, dict):
        if "roots" in v:  # outline: keep authored names only
            names = []

            def walk(n):
                names.append(n.get("name"))
                for c in n["children"]:
                    walk(c)
            for r in v["roots"]:
                walk(r)
            v = {k: x for k, x in v.items() if k not in ("roots", "clips")}
            v["roots"] = "[" + ", ".join(map(str, names)) + "]"
        out = {}
        for k, x in v.items():
            if k in SKIP:
                out[k] = "…"
            elif k == "patch" and isinstance(x, list):
                out[k] = [elide(o) for o in x[:2]] + ([f"…{len(x) - 2} more ops"] if len(x) > 2 else [])
            else:
                out[k] = elide(x)
        return out
    if isinstance(v, list):
        if v and isinstance(v[0], dict) and "description" in v[0]:
            return [t["name"] for t in v]
        return [elide(x) for x in v]
    if isinstance(v, float):
        return round(v, 3)
    return v


recs = [json.loads(l) for l in open(os.path.join(HERE, "session.jsonl"), encoding="utf-8")]
events = []  # (kind, text, frame_path)
for r in recs:
    m = r["msg"]
    if r["dir"] == "send":
        if m.get("method") == "notifications/initialized":
            continue
        if m.get("method") == "tools/call":
            txt = m["params"]["name"] + " " + json.dumps(elide(m["params"]["arguments"]), ensure_ascii=False)
        else:
            txt = m["method"] + " " + json.dumps(elide(m["params"]), ensure_ascii=False)
        events.append(("out", f'[{r["t"]:6.3f}s] >> {txt}', None))
    else:
        res = m.get("result", {})
        body = res.get("structuredContent", res)
        frame = None
        if isinstance(body, dict) and str(body.get("output", "")).endswith(".png"):
            frame = os.path.join(HERE, body["output"])
        kind = "err" if res.get("isError") else "in"
        events.append((kind, f'[{r["t"]:6.3f}s] << {json.dumps(elide(body), ensure_ascii=False)}  ({r["ms"]} ms)', frame))

LW, COLS, LINE_H, TOP = 820, 84, 20, 70
MAXL = (H - TOP - 40) // LINE_H


def wrap(t):
    return textwrap.wrap(t, COLS, subsequent_indent="    ", break_long_words=True) or [""]


CPS = 200
tl = []
t = 0.6
for k, txt, fr in events:
    lines = wrap(txt)
    n = sum(len(l) for l in lines)
    dur = max(0.3, min(1.6, n / CPS)) if k == "out" else max(0.2, min(1.2, n / (CPS * 1.6)))
    tl.append((t, dur, k, lines, fr))
    t += dur + (0.9 if fr else 0.3)
SPLIT_END = t + 1.5

FRAME_BOX = (LW + 40, 110, W - 30, 110 + int((W - 30 - LW - 40) * 9 / 16))
cache = {}


def img(p, size):
    key = (p, size)
    if key not in cache:
        cache[key] = Image.open(p).convert("RGB").resize(size, Image.LANCZOS)
    return cache[key]


def rev_of(p):
    return re.search(r"rev(\d+)", os.path.basename(p)).group(1)


def label_of(p):
    return os.path.basename(p).split("_", 2)[2][:-4]


def draw_frame(now):
    im = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(im)
    d.rounded_rectangle((20, 20, LW + 10, H - 20), 14, fill=PANEL)
    for i, c in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]):
        d.ellipse((40 + i * 22, 36, 52 + i * 22, 48), fill=c)
    d.text((120, 33), "agent <-> director-engine mcp   (JSON-RPC over stdio)", font=mono, fill=DIM)
    shown, cur = [], None
    for (t0, dur, k, lines, fr) in tl:
        if now < t0:
            break
        frac = min(1.0, (now - t0) / dur)
        n = int(sum(len(l) for l in lines) * frac)
        col = {"out": OUTC, "in": INC, "err": ERR}[k]
        for l in lines:
            if n <= 0:
                break
            shown.append((l[:n], col))
            n -= len(l)
        if frac >= 1 and fr:
            cur = fr
    vis = shown[-MAXL:]
    for i, (l, c) in enumerate(vis):
        d.text((40, TOP + i * LINE_H), l, font=mono, fill=c)
    if vis and int(now * 2) % 2 == 0:
        y = TOP + (len(vis) - 1) * LINE_H
        x = 40 + int(d.textlength(vis[-1][0], font=mono))
        d.rectangle((x + 2, y + 2, x + 10, y + 18), fill=FG)
    d.text((LW + 40, 36), "An AI agent edits a Director video over MCP", font=sans_b, fill=FG)
    d.text((LW + 40, 72), "left: the real session log  ·  right: each render_frame PNG", font=sans, fill=DIM)
    size = (FRAME_BOX[2] - FRAME_BOX[0], FRAME_BOX[3] - FRAME_BOX[1])
    if cur:
        im.paste(img(cur, size), FRAME_BOX[:2])
        tag = f"revision {rev_of(cur)}  ·  {label_of(cur)}  ·  t = 2.5 s"
        tw = d.textlength(tag, font=sans_b)
        x0, y0 = FRAME_BOX[0] + 14, FRAME_BOX[1] + 14
        d.rounded_rectangle((x0, y0, x0 + tw + 20, y0 + 38), 8, fill=BG)
        d.text((x0 + 10, y0 + 6), tag, font=sans_b, fill=INC)
    else:
        d.rectangle(FRAME_BOX, outline=(50, 48, 66))
        d.text((FRAME_BOX[0] + 20, FRAME_BOX[1] + 20), "waiting for render_frame…", font=sans, fill=DIM)
    thumbs = [fr for (t0, dur, k, l, fr) in tl if fr and now >= t0 + dur]
    tw_ = (size[0] - 4 * 12) // 5
    th = tw_ * 9 // 16
    ty = FRAME_BOX[3] + 34
    d.text((FRAME_BOX[0], FRAME_BOX[3] + 8), "revisions", font=sans, fill=DIM)
    for i, fr in enumerate(thumbs[:5]):
        x = FRAME_BOX[0] + i * (tw_ + 12)
        im.paste(img(fr, (tw_, th)), (x, ty))
        if fr == cur:
            d.rectangle((x - 2, ty - 2, x + tw_ + 1, ty + th + 1), outline=INC, width=2)
        d.text((x, ty + th + 8), f"rev {rev_of(fr)}", font=sans, fill=FG if fr == cur else DIM)
    return im


ff = subprocess.Popen(["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-",
                       "-c:v", "libx264", "-preset", "slow", "-crf", "23", "-pix_fmt", "yuv420p", "-movflags", "+faststart", OUT],
                      stdin=subprocess.PIPE)
nf = int(SPLIT_END * FPS)
for f in range(nf):
    im = draw_frame(f / FPS)
    ff.stdin.write(im.tobytes())
    if f == nf - 1:
        im.save(os.path.join(HERE, "poster_src.png"))

# Outro: the exported clip itself, decoded frame by frame, untouched apart from scaling.
fw = W - 80
fh = fw * 9 // 16
dec = subprocess.run(["ffmpeg", "-v", "error", "-i", os.path.join(HERE, "export.mp4"), "-vf", f"scale={fw}:{fh}",
                      "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], capture_output=True).stdout
sz = fw * fh * 3
cap = 'export {"session":"s1","output":"export.mp4"}  ->  revision 4, as exported'
for i in range(len(dec) // sz):
    im = Image.new("RGB", (W, H), BG)
    im.paste(Image.frombytes("RGB", (fw, fh), dec[i * sz:(i + 1) * sz]), (40, 24))
    ImageDraw.Draw(im).text((40, 24 + fh + 12), cap, font=mono, fill=INC)
    ff.stdin.write(im.tobytes())
ff.stdin.close()
ff.wait()
print(f"split screen {SPLIT_END:.1f}s, {len(events)} log lines, outro {len(dec) // sz} frames")
