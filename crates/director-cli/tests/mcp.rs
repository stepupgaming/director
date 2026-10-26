use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    id: u64,
}

impl Client {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_director-engine"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
            id: 0,
        }
    }

    /// Send a request, then a notification that must get no reply.
    fn send(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        writeln!(self.stdin, "{}", json!({ "jsonrpc": "2.0", "id": self.id, "method": method, "params": params })).unwrap();
        writeln!(self.stdin, "{}", json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap_or_else(|error| panic!("{error}: {line}"));
        assert_eq!(reply["id"], self.id, "{reply}");
        reply["result"].clone()
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let result = self.send("tools/call", json!({ "name": name, "arguments": arguments }));
        assert_eq!(result["isError"], Value::Null, "{name}: {result}");
        result["structuredContent"].clone()
    }
}

/// An agent drives a project through the stdio MCP server alone.
#[test]
fn an_agent_opens_patches_inspects_and_exports_over_stdio() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("scene.html");
    std::fs::write(
        &input,
        "<style>body{margin:0}#box{width:32px;height:32px;background:red}</style><main data-composition-id='main' data-width='32' data-height='32' data-duration='.4'><div id='box'></div></main>",
    )
    .unwrap();
    let (frame, output) = (dir.path().join("frame.png"), dir.path().join("out.mp4"));

    let mut client = Client::spawn();
    let initialized = client.send(
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }),
    );
    assert_eq!(initialized["protocolVersion"], "2025-06-18");
    assert!(initialized["capabilities"]["tools"].is_object());
    assert_eq!(client.send("tools/list", json!({}))["tools"].as_array().unwrap().len(), 8);

    let session = client.call("open", json!({ "input": input }))["session"].clone();
    let ops = json!([{ "op": "set", "node": "box", "path": "/body/hidden", "value": true }]);
    assert_eq!(client.call("patch", json!({ "session": session, "base_revision": 0, "ops": ops }))["revision"], 1);
    assert!(client.call("inspect", json!({ "session": session, "time": 0.2 })).is_object());
    let painted = client.call("render_frame", json!({ "session": session, "time": 0.2, "output": frame }));
    assert_eq!(painted["revision"], 1);
    assert!(std::fs::metadata(&frame).unwrap().len() > 0);
    let exported = client.call("export", json!({ "session": session, "output": output, "draft": true }));
    assert_eq!(exported["revision"], 1);
    assert!(exported["bytes"].as_u64().unwrap() > 0);

    drop(client.stdin);
    assert!(client.child.wait().unwrap().success());
}
