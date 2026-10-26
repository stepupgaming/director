"""Real MCP session against `director-engine mcp` over stdio (JSON-RPC, newline-delimited).
Logs every request/response with timestamps to session.jsonl and saves every rendered frame."""
import json, re, subprocess, time, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ENGINE = os.path.join(HERE, "..", "..", "target", "release", "director-engine.exe")
os.makedirs(os.path.join(HERE, "frames"), exist_ok=True)
log = open(os.path.join(HERE, "session.jsonl"), "w", encoding="utf-8")
stderr = open(os.path.join(HERE, "stderr.log"), "w", encoding="utf-8")
p = subprocess.Popen([ENGINE, "mcp"], cwd=HERE, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, text=True, encoding="utf-8")
T0 = time.perf_counter(); nid = 0

def rpc(method, params, notify=False):
    global nid
    msg = {"jsonrpc": "2.0", "method": method, "params": params}
    if not notify: nid += 1; msg["id"] = nid
    t = time.perf_counter()
    p.stdin.write(json.dumps(msg) + "\n"); p.stdin.flush()
    if notify:
        log.write(json.dumps({"t": round(t - T0, 4), "dir": "send", "msg": msg}) + "\n"); return None
    log.write(json.dumps({"t": round(t - T0, 4), "dir": "send", "msg": msg}) + "\n")
    while True:  # progress notifications arrive before the reply
        line = p.stdout.readline(); t2 = time.perf_counter()
        reply = json.loads(line)
        if "id" in reply: break
        log.write(json.dumps({"t": round(t2 - T0, 4), "dir": "recv", "msg": reply}) + "\n")
    log.write(json.dumps({"t": round(t2 - T0, 4), "dir": "recv", "ms": round((t2 - t) * 1000, 1), "msg": reply}) + "\n"); log.flush()
    print(f"{method:12} {params.get('name','') if isinstance(params, dict) else '':13} {(t2-t)*1000:8.1f} ms", file=sys.stderr)
    return reply

def call(name, _progress=None, **args):
    params = {"name": name, "arguments": args}
    if _progress: params["_meta"] = {"progressToken": _progress}
    r = rpc("tools/call", params)
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

def path(node, pattern):
    """The one settable pointer of `node` matching `pattern`, discovered with the `node` tool."""
    leaves = call("node", session=S, node=node, leaves=True)["leaves"]
    hits = [p for p in leaves if re.search(pattern, p)]
    assert len(hits) >= 1, (node, pattern, list(leaves)[:40])
    print(f"  {node}: {hits[0]} = {json.dumps(leaves[hits[0]])[:60]}", file=sys.stderr)
    return hits[0]

def top(node):
    p = path(node, r"^/layout/inset/top(/px)?$")
    return (p, 0) if p.endswith("/px") else (p, 1)

def top_value(node, px):
    p, wrapped = top(node)
    return {"op": "set", "node": node, "path": p, "value": {"px": px} if wrapped else px}

patch("title", [{"op": "set", "node": "title", "path": path("title", r"/spans/0/text$"), "value": "Edited live by an agent."}])
teal = {"r": 0.165, "g": 0.831, "b": 0.784, "a": 1.0}
patch("colors", [
    {"op": "set", "node": "bar", "path": path("bar", r"/background/keyframes/0/value$"), "value": teal},
    {"op": "set", "node": "glow", "path": path("glow", r"/background/keyframes/0/value$"), "value": {"r": 1.0, "g": 0.353, "b": 0.212, "a": 1.0}},
])
patch("layout", [top_value("kicker", 240.0), top_value("title", 310.0), top_value("bar", 660.0), top_value("sub", 720.0)])
patch("timing", [
    {"op": "set", "node": "sub", "path": path("sub", r"^/body/animations/0/delay$"), "value": 2.6},
    {"op": "set", "node": "bar", "path": path("bar", r"^/body/animations/0/duration$"), "value": 2.2},
])
# stale write on purpose: base_revision 0 against the current revision
call("patch", session=S, base_revision=0, ops=[{"op": "remove", "node": "glow"}])
call("inspect", session=S, time=FT)
call("qa", _progress="qa", session=S)
B = call("open", input="scene.html")["session"]
d = call("diff", before=B, after=S)
print(f"diff: {len(d['patch'])} ops, patchable={d['patchable']}", file=sys.stderr)
call("export", _progress="export", session=S, output="export.mp4")
print("outside root:", call("render_frame", session=S, output="../../escape.png").get("error"), file=sys.stderr)
call("close", session=B); call("close", session=S)
p.stdin.close(); p.wait(); log.close(); stderr.close()
