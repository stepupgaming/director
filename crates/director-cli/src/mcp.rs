//! `mcp`: a Model Context Protocol server on stdio. Each line is one JSON-RPC
//! 2.0 message; tools open, outline, diff, patch, inspect, paint and export
//! host sessions. Stdout carries protocol messages only. Output paths stay
//! inside the allowed roots, and long tools report MCP progress.

use crate::input::Encoder;
use base64::Engine;
use clap::Parser;
use director_host::{ErrorCode, ExportSettings, HostError, NodeSelector, PatchOp, QaOptions, Session, Source};
use schemars::gen::SchemaSettings;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Supported protocol revisions, newest first.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub const INSTRUCTIONS: &str = "Director renders video from HTML/CSS, Rhai and native projects. \
`open` an input to get a session id and its outline, then `patch` nodes by authored name against \
the session revision (`node` reads a record and lists the JSON pointers `patch` takes), `inspect` layout and timeline, `render_frame` to look at a frame, `qa` to \
score it, and `export` the video. Failed tools return `{\"error\":{\"code\",\"message\"}}`.";

#[derive(Parser, Debug)]
pub struct McpArgs {
    /// Paint frames and exports on a required Vulkan context
    #[arg(long, default_value_t = false)]
    gpu: bool,
    /// Another directory `render_frame` and `export` may write into, besides
    /// the server directory and each session's input directory. Repeatable.
    #[arg(long = "allow-dir", alias = "output-root", value_name = "DIR")]
    allow_dirs: Vec<PathBuf>,
}

/// Writes one JSON-RPC message to the client, from any thread.
pub type Notifier = Arc<dyn Fn(Value) + Send + Sync>;

/// Serve stdin until it closes.
pub fn run(args: McpArgs) -> i32 {
    let notifier: Notifier = Arc::new(|message: Value| {
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "{message}").and_then(|()| stdout.flush());
    });
    let mut server = Server::new(args.gpu).with_roots(&args.allow_dirs).with_notifier(notifier);
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
            let mut stdout = std::io::stdout().lock();
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
    /// The server directory that relative paths resolve against.
    cwd: PathBuf,
    /// Directories outputs may land in besides `cwd` and session input directories.
    roots: Vec<PathBuf>,
    notifier: Notifier,
}

impl Server {
    pub fn new(gpu: bool) -> Self {
        Self {
            gpu,
            sessions: BTreeMap::new(),
            opened: 0,
            cwd: std::env::current_dir().unwrap_or_default(),
            roots: Vec::new(),
            notifier: Arc::new(|_| {}),
        }
    }

    /// Also allow outputs inside `dirs`, relative to the server directory.
    pub fn with_roots(mut self, dirs: &[PathBuf]) -> Self {
        self.roots.extend(dirs.iter().map(|dir| normalize(&self.cwd.join(dir))));
        self
    }

    /// Send progress notifications through `notifier`.
    pub fn with_notifier(mut self, notifier: Notifier) -> Self {
        self.notifier = notifier;
        self
    }

    /// A `(done, total)` reporter for the request's `progressToken`, if it sent one.
    fn progress(&self, params: &Value) -> Option<Arc<dyn Fn(usize, usize) + Send + Sync>> {
        let token = params.get("_meta")?.get("progressToken")?.clone();
        let notifier = self.notifier.clone();
        let last = std::sync::Mutex::new(None);
        Some(Arc::new(move |done: usize, total: usize| {
            let mut last = last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if *last == Some(done) {
                return;
            }
            *last = Some(done);
            notifier(json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": { "progressToken": token, "progress": done, "total": total },
            }));
        }))
    }

    /// `output` resolved against the server directory, once it lands inside
    /// the server directory, the session's input directory or an allowed root.
    fn output_path(&self, session: &str, output: &Path) -> Result<PathBuf, HostError> {
        let path = normalize(&self.cwd.join(output));
        let input_dir = self
            .sessions
            .get(session)
            .and_then(Session::source)
            .and_then(|source| self.cwd.join(&source.input).parent().map(normalize));
        let mut roots: Vec<PathBuf> = std::iter::once(normalize(&self.cwd)).chain(input_dir).chain(self.roots.iter().cloned()).collect();
        roots.dedup();
        // Resolve links in the existing parent, so a link out of a root does not count as inside.
        let real = |path: &Path| -> PathBuf {
            match (path.parent().and_then(|parent| parent.canonicalize().ok()), path.file_name()) {
                (Some(parent), Some(name)) => parent.join(name),
                _ => path.to_path_buf(),
            }
        };
        let real_roots: Vec<PathBuf> = roots.iter().map(|root| root.canonicalize().unwrap_or_else(|_| root.clone())).collect();
        let checked = real(&path);
        let inside = roots.iter().any(|root| path.starts_with(root)) && real_roots.iter().any(|root| checked.starts_with(root));
        if inside && path.file_name().is_some() {
            return Ok(path);
        }
        let allowed: Vec<String> = roots.iter().map(|root| root.display().to_string()).collect();
        Err(HostError::new(
            ErrorCode::OutputOutsideRoot,
            format!("`{}` resolves outside the allowed output roots ({}); pass `--allow-dir` to the server to add one", output.display(), allowed.join(", ")),
        ))
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
        let progress = self.progress(params);
        let outcome = match name {
            "open" => parse(arguments).and_then(|args| self.open(args)),
            "close" => parse(arguments).and_then(|args| self.close(args)),
            "outline" => parse(arguments).and_then(|args: SessionArgs| Ok(structured(json!(self.session(&args.session)?.outline())))),
            "node" => parse(arguments).and_then(|args: NodeArgs| {
                let view = self.session(&args.session)?.node(&args.node, args.path.as_deref(), args.leaves)?;
                Ok(structured(json!(view)))
            }),
            "diff" => parse(arguments).and_then(|args| self.diff(args)),
            "patch" => parse(arguments).and_then(|args| self.patch(args)),
            "inspect" => parse(arguments).and_then(|args: InspectArgs| Ok(structured(self.session(&args.session)?.inspect(args.time)?))),
            "qa" => parse(arguments).and_then(|args: QaArgs| {
                let options = QaOptions {
                    every: args.every,
                    safe_zone: args.safe_zone,
                    loudness_target: args.loudness_target,
                };
                let session = self.session(&args.session)?;
                let report = match &progress {
                    Some(progress) => session.qa_with_progress(&options, &mut |done, total| progress(done, total))?,
                    None => session.qa(&options)?,
                };
                Ok(structured(json!(report)))
            }),
            "render_frame" => parse(arguments).and_then(|args| self.render_frame(args)),
            "export" => parse(arguments).and_then(|args| self.export(args, progress)),
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
        let output = args.output.as_deref().map(|output| self.output_path(&args.session, output)).transpose()?;
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
        let Some(output) = output else {
            let image = json!({ "type": "image", "data": base64::engine::general_purpose::STANDARD.encode(&png), "mimeType": "image/png" });
            return Ok(json!({ "content": [image, text(&info)], "structuredContent": info }));
        };
        std::fs::write(&output, &png).map_err(|error| HostError::new(ErrorCode::Render, format!("{}: {error}", output.display())))?;
        info["output"] = json!(output);
        Ok(structured(info))
    }

    fn export(&mut self, args: ExportArgs, progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>) -> Result<Value, HostError> {
        let output = self.output_path(&args.session, &args.output)?;
        let mut context = crate::gpu::required_context(self.gpu).map_err(|error| HostError::new(ErrorCode::Export, error))?;
        let session = self.session(&args.session)?;
        let settings = ExportSettings {
            scale: args.scale.unwrap_or(1.0),
            alpha: args.alpha,
            options: director_render::ExportOptions {
                draft: args.draft,
                gpu_contexts: crate::gpu::export_contexts(),
                progress,
                ..director_render::ExportOptions::new(args.encoder.mode())
            },
        };
        session.export(&output, &settings, context.as_mut())?;
        Ok(structured(json!({
            "output": output,
            "revision": session.revision(),
            "duration": session.duration(),
            "bytes": std::fs::metadata(&output).map(|metadata| metadata.len()).ok(),
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
struct NodeArgs {
    /// Session id from `open`
    session: String,
    /// Authored name (an HTML `id`) or `{"index","generation"}` node id, as `patch` takes it
    node: NodeSelector,
    /// JSON pointer inside the node record, such as `/body`; the whole record when absent
    path: Option<String>,
    /// Also list every settable leaf below `path` as `{pointer: value}`; colors and other objects of numbers are one leaf
    #[serde(default)]
    leaves: bool,
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
    /// Write the PNG here and return its path; without it the PNG returns as image content. It must resolve inside the server directory, the session's input directory or an `--allow-dir` root
    output: Option<PathBuf>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExportArgs {
    /// Session id from `open`
    session: String,
    /// Video path inside the server directory, the session's input directory or an `--allow-dir` root
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
        tool::<NodeArgs>("node", "A node's document record, or the value at a JSON pointer inside it, by authored name or node id. `leaves` lists every settable pointer below it, each one a `path` that `patch` `set` takes.", true),
        tool::<DiffArgs>("diff", "Document, added, removed, moved and changed nodes between two sessions as JSON pointers, with `patch`: the ops that turn `before` into `after` when `patchable`.", true),
        tool::<PatchArgs>("patch", "Apply patch ops atomically and return the new revision. `set` replaces the value at a JSON pointer in a node record (`{\"op\":\"set\",\"node\":\"box\",\"path\":\"/opacity\",\"value\":0.5}`), `set_document` one in the document, `remove` drops a subtree.", false),
        tool::<InspectArgs>("inspect", "Source ids, layout bounds, timeline and audio of the current revision at a time.", true),
        tool::<QaArgs>("qa", "Score the current revision 0-100 for publishing. Errors cost 20: missing fonts, nodes never on the canvas, clipped or non-finite audio. Warnings cost 5: text clipped by the canvas edge or inside the safe-zone inset for a second or more, captions over faces, reframed subjects out of frame, true peak above -1 dBTP, a silent mix, loudness more than 2 LU off target. Static stretches are info. `pass` holds with no errors. Each issue carries the authored node name, time range and a fix. Reports progress when the request carries a `progressToken`.", true),
        tool::<FrameArgs>("render_frame", "Paint one frame of the current revision as PNG.", false),
        tool::<ExportArgs>("export", "Export the current revision as a video file, with an audio stream only when the scene has audio tracks. Reports encoded frames as progress when the request carries a `progressToken`. Fails with `missing_font` when required fonts are absent and `output_outside_root` for a path outside the allowed roots.", false),
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

/// `path` with `.` and `..` folded away lexically.
fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(normal.components().next_back(), Some(Component::Normal(_))) {
                    continue;
                }
                normal.pop();
            }
            component => normal.push(component),
        }
    }
    normal
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
        assert_eq!(names, ["open", "close", "outline", "node", "diff", "patch", "inspect", "qa", "render_frame", "export"]);
        let patch = &tools[5]["inputSchema"];
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

    fn scene(dir: &std::path::Path) -> std::path::PathBuf {
        let input = dir.join("scene.html");
        std::fs::write(
            &input,
            "<style>body{margin:0}#box{width:32px;height:32px;background:red}</style><main data-composition-id='main' data-width='32' data-height='32' data-duration='.6'><div id='box'></div></main>",
        )
        .unwrap();
        input
    }

    #[test]
    fn node_leaves_round_trip_into_patch() {
        let dir = tempfile::tempdir().unwrap();
        let mut server = Server::new(false);
        call(&mut server, "open", json!({ "input": scene(dir.path()) }));
        let node = call(&mut server, "node", json!({ "session": "s1", "node": "box", "leaves": true }))["structuredContent"].clone();
        assert_eq!(node["name"], "box", "{node}");
        let leaves = node["leaves"].as_object().unwrap();
        let (path, value) = leaves.iter().find(|(path, value)| path.contains("background") && value.get("r").is_some()).expect("a background color leaf");
        let read = call(&mut server, "node", json!({ "session": "s1", "node": node["id"], "path": path }))["structuredContent"].clone();
        assert_eq!(&read["value"], value);
        let blue = json!({ "r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0 });
        let ops = json!([{ "op": "set", "node": "box", "path": path, "value": blue }]);
        assert_eq!(call(&mut server, "patch", json!({ "session": "s1", "ops": ops }))["structuredContent"]["revision"], 1);
        let read = call(&mut server, "node", json!({ "session": "s1", "node": "box", "path": path }))["structuredContent"].clone();
        assert_eq!(read["value"], blue);
        let missing = call(&mut server, "node", json!({ "session": "s1", "node": "box", "path": "/nope" }));
        assert_eq!(missing["structuredContent"]["error"]["code"], "not_found");
    }

    #[test]
    fn outputs_outside_the_allowed_roots_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let input = scene(dir.path());
        let mut server = Server::new(false);
        call(&mut server, "open", json!({ "input": input }));
        let outside = tempfile::tempdir().unwrap();
        let escape = dir.path().join("..").join("..").join("escape.png");
        for output in [outside.path().join("frame.png"), escape] {
            let frame = call(&mut server, "render_frame", json!({ "session": "s1", "output": output }));
            assert_eq!(frame["structuredContent"]["error"]["code"], "output_outside_root", "{frame}");
            let export = call(&mut server, "export", json!({ "session": "s1", "output": output.with_extension("mp4") }));
            assert_eq!(export["structuredContent"]["error"]["code"], "output_outside_root", "{export}");
            assert!(!output.exists());
        }
        let inside = dir.path().join("frames").join("..").join("frame.png");
        let frame = call(&mut server, "render_frame", json!({ "session": "s1", "output": inside }));
        assert_eq!(frame["isError"], Value::Null, "{frame}");
        assert!(dir.path().join("frame.png").exists());

        let mut server = Server::new(false).with_roots(&[outside.path().to_path_buf()]);
        call(&mut server, "open", json!({ "input": input }));
        let frame = call(&mut server, "render_frame", json!({ "session": "s1", "output": outside.path().join("frame.png") }));
        assert!(outside.path().join("frame.png").exists(), "{frame}");
    }

    #[test]
    fn qa_and_export_report_progress_for_a_progress_token() {
        let dir = tempfile::tempdir().unwrap();
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let sink = sent.clone();
        let mut server = Server::new(false).with_notifier(std::sync::Arc::new(move |message| sink.lock().unwrap().push(message)));
        call(&mut server, "open", json!({ "input": scene(dir.path()) }));
        call(&mut server, "qa", json!({ "session": "s1" }));
        assert!(sent.lock().unwrap().is_empty());
        let meta = |token: &str| json!({ "progressToken": token });
        let qa = request(&mut server, "tools/call", json!({ "name": "qa", "arguments": { "session": "s1" }, "_meta": meta("q") }));
        assert_eq!(qa["result"]["structuredContent"]["pass"], true);
        let output = dir.path().join("out.mp4");
        let export = request(&mut server, "tools/call", json!({ "name": "export", "arguments": { "session": "s1", "output": output, "draft": true }, "_meta": meta("e") }));
        assert!(export["result"]["structuredContent"]["bytes"].as_u64().is_some_and(|bytes| bytes > 0), "{export}");
        let sent = sent.lock().unwrap();
        for token in ["q", "e"] {
            let progress: Vec<&Value> = sent.iter().map(|message| &message["params"]).filter(|params| params["progressToken"] == token).collect();
            assert!(!progress.is_empty(), "{token}: {sent:?}");
            assert!(sent.iter().all(|message| message["method"] == "notifications/progress"));
            let last = progress.last().unwrap();
            assert_eq!(last["progress"], last["total"], "{token}: {last}");
        }
    }
}
