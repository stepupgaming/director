"""Build the demo video from the real session artifacts: session.jsonl, frames/*.png, export.mp4.
Every JSON line shown comes from session.jsonl (long values elided with "…"); every frame is a saved render."""
import json, os, re, subprocess, sys, textwrap
from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "director-agent.mp4")
# Sized for the page: the player is ~560 CSS px wide, so a 1120 px canvas is 2x and text lands at ~11 px.
W, H, FPS = 1120, 630, 30
FONTS = os.path.join(HERE, "..", "assets", "fonts")
CORE = os.path.join(HERE, "..", "..", "crates", "director-core", "assets", "fonts")
mono = ImageFont.truetype(os.path.join(FONTS, "JetBrainsMono-Regular.ttf"), 21)
sans_b = ImageFont.truetype(os.path.join(CORE, "Inter-Bold.ttf"), 28)
sans = ImageFont.truetype(os.path.join(CORE, "Inter-Regular.ttf"), 21)
BG, PANEL, FG, DIM = (10, 10, 15), (18, 17, 26), (230, 226, 215), (120, 117, 138)
OUTC, INC, ERR = (255, 179, 71), (42, 212, 200), (255, 90, 54)


def short(v, n=34):
    t = v if isinstance(v, str) else json.dumps(v, ensure_ascii=False)
    return t if len(t) <= n else t[: n - 1] + "…"


def call_text(name, a):
    """One compact line per tool call, built from the recorded arguments."""
    if name == "patch":
        ops = a.get("ops", [])
        o = ops[0]
        leaf = o.get("path", "").rstrip("/").split("/")[-1] if o.get("path") else ""
        first = f'{o["op"]} {o["node"]}' + (f".{leaf} = {short(o.get('value'), 26)}" if leaf else "")
        more = f" +{len(ops) - 1} ops" if len(ops) > 1 else ""
        return f"patch @rev {a.get('base_revision')}: {first}{more}"
    if name == "render_frame":
        return f"render_frame t={a.get('time', 0)}s -> {os.path.basename(a.get('output', ''))}"
    if name == "node":
        return f"node {a.get('node')}"
    if name in ("open",):
        return f"open {a.get('input')}"
    if name == "diff":
        return f"diff {a.get('before')} -> {a.get('after')}"
    if name in ("export",):
        return f"export -> {a.get('output')}"
    if name == "close":
        return f"close {a.get('session')}"
    return name + "".join(f" {k}={short(v, 16)}" for k, v in a.items() if k != "session")


def result_text(b, err):
    if err:
        e = b.get("error", {})
        return f"error {e.get('code')}: {e.get('message', '')}"
    if "tools" in b:
        return f"{len(b['tools'])} tools"
    if "capabilities" in b:
        return "initialized"
    if "revision" in b and "output" in b and str(b["output"]).endswith(".png"):
        return f"frame {b['width']}x{b['height']} at rev {b['revision']}"
    if "bytes" in b:
        return f"exported {b['duration']} s, {b['bytes'] // 1000} KB, rev {b['revision']}"
    if "pass" in b:
        return f"qa pass={str(b['pass']).lower()} score {b['score']}, {b['errors']} errors"
    if set(b) == {"revision"}:
        return f"ok, now revision {b['revision']}"
    if "kind" in b and "id" in b:
        return f"{b['kind']} node, {len(b.get('leaves', {}))} leaves"
    if "changed_nodes" in b:
        return f"{len(b['changed_nodes'])} nodes changed"
    if "closed" in b:
        return f"closed {b['closed']}"
    if "element_ids" in b:
        return f"{len(b['element_ids'])} elements"
    if "compositions" in b:
        return "session opened"
    if "fps" in b:
        return f"{b['duration']} s at {b['fps']:g} fps"
    return short(b, 40)


recs = [json.loads(l) for l in open(os.path.join(HERE, "session.jsonl"), encoding="utf-8")]
events = []  # (kind, text, frame_path)
for r in recs:
    m = r["msg"]
    if r["dir"] == "send":
        if m.get("method") == "notifications/initialized":
            continue
        if m.get("method") == "tools/call":
            txt = call_text(m["params"]["name"], m["params"]["arguments"])
        else:
            txt = m["method"]
        events.append(("out", f">> {txt}", None))
    else:
        if r.get("ms") is None:  # progress notifications
            continue
        res = m.get("result", {})
        body = res.get("structuredContent", res)
        frame = None
        if isinstance(body, dict) and str(body.get("output", "")).endswith(".png"):
            frame = os.path.join(HERE, body["output"])
        kind = "err" if res.get("isError") else "in"
        events.append((kind, f"<< {result_text(body, kind == 'err')}  {r['ms']:g} ms", frame))

LW, COLS, LINE_H, TOP = 560, 40, 27, 64
MAXL = (H - TOP - 26) // LINE_H


def wrap(t):
    return textwrap.wrap(t, COLS, subsequent_indent="   ", break_long_words=True) or [""]


CPS = 70
tl = []
t = 0.6
for k, txt, fr in events:
    lines = wrap(txt)
    n = sum(len(l) for l in lines)
    dur = max(0.25, min(1.0, n / CPS)) if k == "out" else 0.2
    tl.append((t, dur, k, lines, fr))
    t += dur + (0.9 if fr else 0.25)
SPLIT_END = t + 1.5

FRAME_BOX = (LW + 30, 100, W - 20, 100 + int((W - 20 - LW - 30) * 9 / 16))
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
    d.rounded_rectangle((14, 14, LW + 6, H - 14), 12, fill=PANEL)
    for i, c in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]):
        d.ellipse((32 + i * 22, 30, 44 + i * 22, 42), fill=c)
    d.text((106, 24), "agent <-> director mcp", font=mono, fill=DIM)
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
        d.text((32, TOP + i * LINE_H), l, font=mono, fill=c)
    if vis and int(now * 2) % 2 == 0:
        y = TOP + (len(vis) - 1) * LINE_H
        x = 32 + int(d.textlength(vis[-1][0], font=mono))
        d.rectangle((x + 2, y + 2, x + 10, y + 18), fill=FG)
    d.text((LW + 30, 24), "An agent edits a video over MCP", font=sans_b, fill=FG)
    d.text((LW + 30, 62), "real session log, each rendered frame", font=sans, fill=DIM)
    size = (FRAME_BOX[2] - FRAME_BOX[0], FRAME_BOX[3] - FRAME_BOX[1])
    if cur:
        im.paste(img(cur, size), FRAME_BOX[:2])
        tag = f"rev {rev_of(cur)}  ·  {label_of(cur)}"
        tw = d.textlength(tag, font=sans_b)
        x0, y0 = FRAME_BOX[0] + 14, FRAME_BOX[1] + 14
        d.rounded_rectangle((x0, y0, x0 + tw + 20, y0 + 42), 8, fill=BG)
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
                       "-c:v", "libx264", "-preset", "slow", "-crf", "18", "-pix_fmt", "yuv420p", "-movflags", "+faststart", OUT],
                      stdin=subprocess.PIPE)
nf = int(SPLIT_END * FPS)
for f in range(nf):
    im = draw_frame(f / FPS)
    ff.stdin.write(im.tobytes())
    if f == nf - 1:
        im.save(os.path.join(HERE, "poster_src.png"))

# Outro: the exported clip itself, decoded frame by frame, untouched apart from scaling.
fw = W - 160
fh = fw * 9 // 16
dec = subprocess.run(["ffmpeg", "-v", "error", "-i", os.path.join(HERE, "export.mp4"), "-vf", f"scale={fw}:{fh}",
                      "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], capture_output=True).stdout
sz = fw * fh * 3
cap = "export.mp4, revision 4, exactly as the agent exported it"
for i in range(len(dec) // sz):
    im = Image.new("RGB", (W, H), BG)
    im.paste(Image.frombytes("RGB", (fw, fh), dec[i * sz:(i + 1) * sz]), (80, 24))
    ImageDraw.Draw(im).text((80, 24 + fh + 16), cap, font=mono, fill=INC)
    ff.stdin.write(im.tobytes())
ff.stdin.close()
ff.wait()
print(f"split screen {SPLIT_END:.1f}s, {len(events)} log lines, outro {len(dec) // sz} frames")
