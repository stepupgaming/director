//! `mcp`: a Model Context Protocol server on stdio. Each line is one JSON-RPC
//! 2.0 message; tools open, outline, diff, patch, inspect, paint and export
//! host sessions. Stdout carries protocol messages only.

use crate::input::Encoder;
use base64::Engine;
use clap::Parser;
use director_host::{ErrorCode, ExportSettings, HostError, PatchOp, QaOptions, Session, Source};
use schemars::gen::SchemaSettings;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// Supported protocol revisions, newest first.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub const INSTRUCTIONS: &str = "Director renders video from HTML/CSS, Rhai and native projects. \
`open` an input to get a session id and its outline, then `patch` nodes by authored name against \
the session revision, `inspect` layout and timeline, `render_frame` to look at a frame, `qa` to \
score it, and `export` the video. Failed tools return `{\"error\":{\"code\",\"message\"}}`.";

#[derive(Parser, Debug)]
pub struct McpArgs {
    /// Paint frames and exports on a required Vulkan context
    #[arg(long, default_value_t = false)]
    gpu: bool,
}

/// Serve stdin until it closes.
pub fn run(args: McpArgs) -> i32 {
    let mut server = Server::new(args.gpu);
    let mut stdout = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else {
            return 1;
        };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str(&line) {
            Ok(message) => server.handle(message),
            Err(error) => Some(rpc_error(Value::Null, -32700, format!("parse error: {error}"))),
        };
        if let Some(reply) = reply {
            if writeln!(stdout, "{reply}").and_then(|()| stdout.flush()).is_err() {
                return 1;
            }
        }
    }
    0
}

/// Open sessions by id.
pub struct Server {
    gpu: bool,
    sessions: BTreeMap<String, Session>,
    opened: u64,
}

impl Server {
    pub fn new(gpu: bool) -> Self {
        Self {
            gpu,
            sessions: BTreeMap::new(),
            opened: 0,
        }
    }

    /// The reply to one message. Notifications and client responses get none.
    pub fn handle(&mut self, message: Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            if message.get("result").is_some() || message.get("error").is_some() {
                return None;
            }
            return Some(rpc_error(id.unwrap_or(Value::Null), -32600, "invalid request".into()));
        };
        let id = id?;
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let result = match method {
            "initialize" => Ok(initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => self.call(&params),
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => rpc_error(id, code, message),
        })
    }

    fn call(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "tools/call needs a tool name".to_string()))?;
        let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let outcome = match name {
            "open" => parse(arguments).and_then(|args| self.open(args)),
            "close" => parse(arguments).and_then(|args| self.close(args)),
            "outline" => parse(arguments).and_then(|args: SessionArgs| Ok(structured(json!(self.session(&args.session)?.outline())))),
            "diff" => parse(arguments).and_then(|args| self.diff(args)),
            "patch" => parse(arguments).and_then(|args| self.patch(args)),
            "inspect" => parse(arguments).and_then(|args: InspectArgs| Ok(structured(self.session(&args.session)?.inspect(args.time)?))),
            "qa" => parse(arguments).and_then(|args: QaArgs| {
                let options = QaOptions {
                    every: args.every,
                    safe_zone: args.safe_zone,
                    loudness_target: args.loudness_target,
                };
                Ok(structured(json!(self.session(&args.session)?.qa(&options)?)))
            }),
            "render_frame" => parse(arguments).and_then(|args| self.render_frame(args)),
            "export" => parse(arguments).and_then(|args| self.export(args)),
            _ => return Err((-32602, format!("unknown tool: {name}"))),
        };
        Ok(outcome.unwrap_or_else(|error| {
            let envelope = error.envelope();
            json!({ "content": [text(&envelope)], "structuredContent": envelope, "isError": true })
        }))
    }

    fn session(&mut self, id: &str) -> Result<&mut Session, HostError> {
        self.sessions.get_mut(id).ok_or_else(|| not_open(id))
    }

    fn open(&mut self, args: OpenArgs) -> Result<Value, HostError> {
        let mut source = Source::new(args.input);
        source.width = args.width.unwrap_or(source.width);
        source.height = args.height.unwrap_or(source.height);
        source.fps = args.fps.unwrap_or(source.fps);
        source.duration = args.duration.unwrap_or(source.duration);
        source.composition = args.composition;
        source.props = args.props.map(|props| match props {
            Value::String(props) => props,
            props => props.to_string(),
        });
        let mut session = Session::open(&source)?;
        crate::renderer::use_gpu(&mut session, self.gpu).map_err(|error| HostError::new(ErrorCode::Render, error))?;
        self.opened += 1;
        let id = format!("s{}", self.opened);
        let opened = json!({
            "session": id,
            "source_kind": session.source_kind(),
            "missing_fonts": session.missing_fonts(),
            "compositions": session.compositions(),
            "outline": session.outline(),
        });
        self.sessions.insert(id, session);
        Ok(structured(opened))
    }

    fn close(&mut self, args: SessionArgs) -> Result<Value, HostError> {
        self.sessions.remove(&args.session).ok_or_else(|| not_open(&args.session))?;
        Ok(structured(json!({ "closed": args.session })))
    }

    fn diff(&self, args: DiffArgs) -> Result<Value, HostError> {
        let session = |id: &str| self.sessions.get(id).ok_or_else(|| not_open(id));
        Ok(structured(json!(director_host::diff(session(&args.before)?, session(&args.after)?))))
    }

    fn patch(&mut self, args: PatchArgs) -> Result<Value, HostError> {
        let session = self.session(&args.session)?;
        let base_revision = args.base_revision.unwrap_or(session.revision());
        Ok(structured(json!({ "revision": session.patch(base_revision, &args.ops)? })))
    }

    fn render_frame(&mut self, args: FrameArgs) -> Result<Value, HostError> {
        let session = self.session(&args.session)?;
        let document = session.document();
        crate::renderer::frame_within_limit(document.width, document.height).map_err(|error| HostError::new(ErrorCode::InvalidArgument, error))?;
        let frame = session.render_frame(args.time)?;
        let png = crate::renderer::frame_png(&frame).map_err(|error| HostError::new(ErrorCode::Render, error))?;
        let mut info = json!({
            "revision": session.revision(),
            "time": args.time,
            "width": frame.width,
            "height": frame.height,
        });
        let Some(output) = args.output else {
            let image = json!({ "type": "image", "data": base64::engine::general_purpose::STANDARD.encode(&png), "mimeType": "image/png" });
            return Ok(json!({ "content": [image, text(&info)], "structuredContent": info }));
        };
        std::fs::write(&output, &png).map_err(|error| HostError::new(ErrorCode::Render, format!("{}: {error}", output.display())))?;
        info["output"] = json!(output);
        Ok(structured(info))
    }

    fn export(&mut self, args: ExportArgs) -> Result<Value, HostError> {
        let mut context = crate::gpu::required_context(self.gpu).map_err(|error| HostError::new(ErrorCode::Export, error))?;
        let session = self.session(&args.session)?;
        let settings = ExportSettings {
            scale: args.scale.unwrap_or(1.0),
            alpha: args.alpha,
            options: director_render::ExportOptions {
                draft: args.draft,
                ..director_render::ExportOptions::new(args.encoder.mode())
            },
        };
        session.export(&args.output, &settings, context.as_mut())?;
        Ok(structured(json!({
            "output": args.output,
            "revision": session.revision(),
            "duration": session.duration(),
            "bytes": std::fs::metadata(&args.output).map(|metadata| metadata.len()).ok(),
        })))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct OpenArgs {
    /// Rhai, HTML, capture JSON, native .dirproj or core project JSON path, relative to the server directory
    input: PathBuf,
    /// Canvas width for inputs that do not set one (default 1920)
    width: Option<i32>,
    /// Canvas height for inputs that do not set one (default 1080)
    height: Option<i32>,
    /// Frame rate for inputs that do not set one (default 30)
    fps: Option<f64>,
    /// Seconds for inputs that do not set a duration (default 5)
    duration: Option<f64>,
    /// Script composition to open
    composition: Option<String>,
    /// Script composition props as JSON or a JSON file path
    props: Option<Value>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SessionArgs {
    /// Session id from `open`
    session: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DiffArgs {
    /// Session id to diff from
    before: String,
    /// Session id to diff to
    after: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PatchArgs {
    /// Session id from `open`
    session: String,
    /// Revision the ops were written against; a stale revision fails with `conflict`. Defaults to the current revision.
    base_revision: Option<u64>,
    /// Ops applied in order; any failure rejects them all
    ops: Vec<PatchOp>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct InspectArgs {
    /// Session id from `open`
    session: String,
    /// Seconds into the timeline
    #[serde(default)]
    time: f64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QaArgs {
    /// Session id from `open`
    session: String,
    /// Seconds between layout samples
    #[serde(default = "default_every")]
    every: f64,
    /// Safe-zone preset for text: `tiktok`, `youtube_shorts`, `instagram_reel`, `instagram_story`, `linkedin`, `twitter`, `mobile` or `desktop`
    safe_zone: Option<String>,
    /// Integrated loudness target of the audio mix in LUFS
    #[serde(default = "default_loudness_target")]
    loudness_target: f64,
}

fn default_every() -> f64 {
    QaOptions::default().every
}

fn default_loudness_target() -> f64 {
    QaOptions::default().loudness_target
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FrameArgs {
    /// Session id from `open`
    session: String,
    /// Seconds into the timeline
    #[serde(default)]
    time: f64,
    /// Write the PNG here and return its path; without it the PNG returns as image content
    output: Option<PathBuf>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExportArgs {
    /// Session id from `open`
    session: String,
    /// Video path
    output: PathBuf,
    /// Raster scale in (0, 1]
    scale: Option<f32>,
    /// Leave uncovered pixels clear and write VP9 with alpha
    alpha: Option<bool>,
    /// Video encoder
    #[serde(default)]
    encoder: EncoderArg,
    /// Software x264 ultrafast at CRF 30
    #[serde(default)]
    draft: bool,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum EncoderArg {
    #[default]
    Software,
    Nvenc,
}

impl EncoderArg {
    fn mode(self) -> director_core::video_wrapper::EncoderMode {
        match self {
            Self::Software => Encoder::Software.mode(),
            Self::Nvenc => Encoder::Nvenc.mode(),
        }
    }
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested.filter(|version| PROTOCOL_VERSIONS.contains(version)).unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "director-engine", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

pub fn tools() -> Vec<Value> {
    vec![
        tool::<OpenArgs>("open", "Open an input as a session at revision 0. Returns the session id, source kind, missing fonts, compositions and outline.", false),
        tool::<SessionArgs>("close", "Close a session and free its renderers.", false),
        tool::<SessionArgs>("outline", "The node tree with authored names, the clips and the current revision.", true),
        tool::<DiffArgs>("diff", "Document, added, removed, moved and changed nodes between two sessions as JSON pointers, with `patch`: the ops that turn `before` into `after` when `patchable`.", true),
        tool::<PatchArgs>("patch", "Apply patch ops atomically and return the new revision. `set` replaces the value at a JSON pointer in a node record (`{\"op\":\"set\",\"node\":\"box\",\"path\":\"/opacity\",\"value\":0.5}`), `set_document` one in the document, `remove` drops a subtree.", false),
        tool::<InspectArgs>("inspect", "Source ids, layout bounds, timeline and audio of the current revision at a time.", true),
        tool::<QaArgs>("qa", "Score the current revision 0-100 for publishing. Errors cost 20: missing fonts, nodes never on the canvas, clipped or non-finite audio. Warnings cost 5: text clipped by the canvas edge or inside the safe-zone inset for a second or more, captions over faces, reframed subjects out of frame, true peak above -1 dBTP, a silent mix, loudness more than 2 LU off target. Static stretches are info. `pass` holds with no errors. Each issue carries the authored node name, time range and a fix.", true),
        tool::<FrameArgs>("render_frame", "Paint one frame of the current revision as PNG.", false),
        tool::<ExportArgs>("export", "Export the current revision as a video file. Fails with `missing_font` when required fonts are absent.", false),
    ]
}

/// A tool whose input schema is `T`.
fn tool<T: JsonSchema>(name: &str, description: &str, read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema::<T>(),
        "annotations": { "readOnlyHint": read_only },
    })
}

/// The JSON schema of `T` with every subschema inline and no title.
pub fn schema<T: JsonSchema>() -> Value {
    let generator = SchemaSettings::draft07()
        .with(|settings| {
            settings.inline_subschemas = true;
            settings.meta_schema = None;
        })
        .into_generator();
    let mut schema = json!(generator.into_root_schema_for::<T>());
    if let Some(schema) = schema.as_object_mut() {
        schema.remove("title");
    }
    schema
}

fn parse<T: DeserializeOwned>(arguments: Value) -> Result<T, HostError> {
    serde_json::from_value(arguments).map_err(|error| HostError::new(ErrorCode::InvalidArgument, error.to_string()))
}

fn not_open(id: &str) -> HostError {
    HostError::new(ErrorCode::NotFound, format!("no open session `{id}`"))
}

fn text(value: &Value) -> Value {
    json!({ "type": "text", "text": value.to_string() })
}

/// A tool result carrying `value` as text and as structured content.
fn structured(value: Value) -> Value {
    json!({ "content": [text(&value)], "structuredContent": value })
}

fn rpc_error(id: Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::Server;
    use serde_json::{json, Value};

    fn request(server: &mut Server, method: &str, params: Value) -> Value {
        server.handle(json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params })).unwrap()
    }

    fn call(server: &mut Server, name: &str, arguments: Value) -> Value {
        request(server, "tools/call", json!({ "name": name, "arguments": arguments }))["result"].clone()
    }

    #[test]
    fn initialize_negotiates_the_protocol_and_lists_tools() {
        let mut server = Server::new(false);
        let reply = request(&mut server, "initialize", json!({ "protocolVersion": "2025-03-26", "capabilities": {} }));
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(reply["result"]["serverInfo"]["name"], "director-engine");
        let reply = request(&mut server, "initialize", json!({ "protocolVersion": "1999-01-01" }));
        assert_eq!(reply["result"]["protocolVersion"], super::PROTOCOL_VERSIONS[0]);
        assert!(server.handle(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        assert_eq!(request(&mut server, "ping", json!({}))["result"], json!({}));

        let tools = request(&mut server, "tools/list", json!({}))["result"]["tools"].clone();
        let names: Vec<&str> = tools.as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["open", "close", "outline", "diff", "patch", "inspect", "qa", "render_frame", "export"]);
        let patch = &tools[4]["inputSchema"];
        assert_eq!(patch["type"], "object");
        assert_eq!(patch["required"], json!(["ops", "session"]));
        assert!(patch.to_string().contains("set_document"), "{patch}");
        assert!(!patch.to_string().contains("$ref"), "{patch}");
    }

    #[test]
    fn protocol_failures_are_json_rpc_errors() {
        let mut server = Server::new(false);
        assert_eq!(request(&mut server, "resources/list", json!({}))["error"]["code"], -32601);
        assert_eq!(request(&mut server, "tools/call", json!({ "name": "nope" }))["error"]["code"], -32602);
        assert_eq!(server.handle(json!({ "jsonrpc": "2.0", "id": 1 })).unwrap()["error"]["code"], -32600);
        assert!(server.handle(json!({ "jsonrpc": "2.0", "id": 1, "result": {} })).is_none());
    }

    #[test]
    fn tool_failures_carry_the_error_envelope() {
        let mut server = Server::new(false);
        let missing = call(&mut server, "outline", json!({ "session": "s9" }));
        assert_eq!(missing["isError"], true);
        assert_eq!(missing["structuredContent"]["error"]["code"], "not_found");
        let text: Value = serde_json::from_str(missing["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, missing["structuredContent"]);
        let unknown = call(&mut server, "inspect", json!({ "session": "s1", "times": 1 }));
        assert_eq!(unknown["structuredContent"]["error"]["code"], "invalid_argument");
        let load = call(&mut server, "open", json!({ "input": "no/such/input.html" }));
        assert_eq!(load["structuredContent"]["error"]["code"], "load");
    }

    #[test]
    fn tools_open_patch_inspect_diff_and_paint_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("scene.html");
        std::fs::write(
            &input,
            "<style>body{margin:0}#box{width:32px;height:32px;background:red}</style><main data-composition-id='main' data-width='32' data-height='32' data-duration='.6'><div id='box'></div></main>",
        )
        .unwrap();
        let mut server = Server::new(false);
        let opened = call(&mut server, "open", json!({ "input": input }))["structuredContent"].clone();
        assert_eq!(opened["session"], "s1");
        assert_eq!(opened["outline"]["revision"], 0);
        assert!(opened["outline"].to_string().contains(r#""name":"box""#));

        let before = call(&mut server, "render_frame", json!({ "session": "s1", "time": 0.1 }));
        assert_eq!(before["content"][0]["type"], "image");
        assert_eq!(before["content"][0]["mimeType"], "image/png");
        assert_eq!(before["structuredContent"]["width"], 32);

        let ops = json!([{ "op": "set", "node": "box", "path": "/body/hidden", "value": true }]);
        let patched = call(&mut server, "patch", json!({ "session": "s1", "base_revision": 0, "ops": ops }));
        assert_eq!(patched["structuredContent"]["revision"], 1, "{patched}");
        let stale = call(&mut server, "patch", json!({ "session": "s1", "base_revision": 0, "ops": ops }));
        assert_eq!(stale["structuredContent"]["error"]["code"], "conflict");
        let after = call(&mut server, "render_frame", json!({ "session": "s1", "time": 0.1 }));
        assert_ne!(after["content"][0]["data"], before["content"][0]["data"]);

        let inspected = call(&mut server, "inspect", json!({ "session": "s1", "time": 0.1 }));
        assert!(inspected["structuredContent"].is_object(), "{inspected}");
        assert_eq!(call(&mut server, "outline", json!({ "session": "s1" }))["structuredContent"]["revision"], 1);
        let qa = call(&mut server, "qa", json!({ "session": "s1" }))["structuredContent"].clone();
        assert_eq!((qa["revision"].as_u64(), qa["pass"].as_bool()), (Some(1), Some(true)), "{qa}");
        let zone = call(&mut server, "qa", json!({ "session": "s1", "safe_zone": "nope" }));
        assert_eq!(zone["structuredContent"]["error"]["code"], "invalid_argument");

        call(&mut server, "open", json!({ "input": input }));
        let diff = call(&mut server, "diff", json!({ "before": "s2", "after": "s1" }))["structuredContent"].clone();
        assert_eq!(diff["patchable"], true);
        assert_eq!(diff["changed_nodes"][0]["name"], "box");
        assert_eq!(call(&mut server, "close", json!({ "session": "s2" }))["structuredContent"]["closed"], "s2");
        assert_eq!(call(&mut server, "outline", json!({ "session": "s2" }))["isError"], true);
    }
}
