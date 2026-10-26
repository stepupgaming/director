"""Real MCP session against `director-engine mcp` over stdio (JSON-RPC, newline-delimited).
Logs every request/response with timestamps to session.jsonl and saves every rendered frame."""
import json, subprocess, time, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ENGINE = os.path.join(HERE, "..", "..", "target", "release", "director-engine.exe")
os.makedirs(os.path.join(HERE, "frames"), exist_ok=True)
log = open(os.path.join(HERE, "session.jsonl"), "w", encoding="utf-8")
p = subprocess.Popen([ENGINE, "mcp"], cwd=HERE, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8")
T0 = time.perf_counter(); nid = 0

def rpc(method, params, notify=False):
    global nid
    msg = {"jsonrpc": "2.0", "method": method, "params": params}
    if not notify: nid += 1; msg["id"] = nid
    t = time.perf_counter()
    p.stdin.write(json.dumps(msg) + "\n"); p.stdin.flush()
    if notify:
        log.write(json.dumps({"t": round(t - T0, 4), "dir": "send", "msg": msg}) + "\n"); return None
    line = p.stdout.readline(); t2 = time.perf_counter()
    reply = json.loads(line)
    log.write(json.dumps({"t": round(t - T0, 4), "dir": "send", "msg": msg}) + "\n")
    log.write(json.dumps({"t": round(t2 - T0, 4), "dir": "recv", "ms": round((t2 - t) * 1000, 1), "msg": reply}) + "\n"); log.flush()
    print(f"{method:12} {params.get('name','') if isinstance(params, dict) else '':13} {(t2-t)*1000:8.1f} ms", file=sys.stderr)
    return reply

def call(name, **args):
    r = rpc("tools/call", {"name": name, "arguments": args})
    return r.get("result", {}).get("structuredContent", r)

FT = 2.5
rev = 0; step = 0
def frame(label):
    global step
    out = f"frames/{step:02d}_rev{rev}_{label}.png"; step += 1
    return call("render_frame", session=S, time=FT, output=out)

def patch(label, ops):
    global rev
    r = call("patch", session=S, base_revision=rev, ops=ops)
    if "error" in r: print("PATCH FAILED", r, file=sys.stderr); return r
    rev = r["revision"]; frame(label); return r

rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "mcp-demo", "version": "1"}})
rpc("notifications/initialized", {}, notify=True)
rpc("tools/list", {})
S = call("open", input="scene.html")["session"]
call("outline", session=S)
frame("open")
patch("title", [{"op": "set", "node": "title", "path": "/body/content/body/spans/0/text", "value": "Edited live by an agent."}])
teal = {"r": 0.165, "g": 0.831, "b": 0.784, "a": 1.0}
patch("colors", [
    {"op": "set", "node": "bar", "path": "/body/paint/box_paint/background/keyframes/0/value", "value": teal},
    {"op": "set", "node": "glow", "path": "/body/paint/box_paint/background/keyframes/0/value", "value": {"r": 1.0, "g": 0.353, "b": 0.212, "a": 1.0}},
])
patch("layout", [
    {"op": "set", "node": "kicker", "path": "/layout/inset/top/px", "value": 240.0},
    {"op": "set", "node": "title", "path": "/layout/inset/top/px", "value": 310.0},
    {"op": "set", "node": "bar", "path": "/layout/inset/top/px", "value": 660.0},
    {"op": "set", "node": "sub", "path": "/layout/inset/top/px", "value": 720.0},
])
patch("timing", [
    {"op": "set", "node": "sub", "path": "/body/animations/0/delay", "value": 2.6},
    {"op": "set", "node": "bar", "path": "/body/animations/0/duration", "value": 2.2},
])
# stale write on purpose: base_revision 0 against the current revision
call("patch", session=S, base_revision=0, ops=[{"op": "remove", "node": "glow"}])
call("inspect", session=S, time=FT)
call("qa", session=S)
B = call("open", input="scene.html")["session"]
call("diff", before=B, after=S)
call("export", session=S, output="export.mp4")
call("close", session=B); call("close", session=S)
p.stdin.close(); p.wait(); log.close()
