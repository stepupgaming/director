//! Container provenance: camera tags, encoder, editor, renderer, generator and
//! platform signatures, AIGC labels and C2PA boxes. Ported from rea-video
//! `probe.py` (Apache-2.0, see NOTICE) with English output and global
//! platform, editor and tool signatures.
//!
//! Metadata is strong evidence when present and is often stripped by
//! platforms, so absence proves nothing.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::OnceLock;

use regex::{Regex, RegexBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::media::VideoInfo;

/// (pattern, category, label). Categories: generator, editor, renderer, screen, platform, tool.
const SIGNATURES: &[(&str, &str, &str)] = &[
    (r"kling|kuaishou.*ai|可灵", "generator", "Kling"),
    (r"jimeng|dreamina|即梦", "generator", "Jimeng/Dreamina"),
    (r"seedance|doubao.*video", "generator", "Seedance"),
    (r"hailuo|minimax", "generator", "MiniMax/Hailuo"),
    (r"\bveo\b|google.*(deepmind|veo)|synthid", "generator", "Google Veo"),
    (r"\bsora\b|openai", "generator", "OpenAI Sora"),
    (r"runway|gen-?4", "generator", "Runway"),
    (r"pika", "generator", "Pika"),
    (r"vidu|shengshu|生数", "generator", "Vidu"),
    (r"pixverse", "generator", "PixVerse"),
    (r"luma|dream ?machine", "generator", "Luma"),
    (r"wanx|tongyi|万相", "generator", "Wan/Tongyi"),
    (r"grok ?imagine|\bxai\b", "generator", "Grok Imagine"),
    (r"midjourney", "generator", "Midjourney"),
    (r"higgsfield", "generator", "Higgsfield"),
    (r"ltx-?video|lightricks", "generator", "LTX Video"),
    (r"hunyuan", "generator", "Hunyuan Video"),
    (r"stable ?video|stability ?ai", "generator", "Stable Video"),
    (r"firefly", "generator", "Adobe Firefly"),
    (r"capcut|剪映|jianying", "editor", "CapCut"),
    (r"premiere|adobe", "editor", "Adobe"),
    (r"final ?cut|imovie", "editor", "Apple editor"),
    (r"davinci|resolve", "editor", "DaVinci Resolve"),
    (r"shotcut", "editor", "Shotcut"),
    (r"kdenlive", "editor", "Kdenlive"),
    (r"openshot", "editor", "OpenShot"),
    (r"camtasia", "editor", "Camtasia"),
    (r"descript", "editor", "Descript"),
    (r"clipchamp", "editor", "Clipchamp"),
    (r"\bcanva\b", "editor", "Canva"),
    (r"inshot", "editor", "InShot"),
    (r"filmora|wondershare", "editor", "Filmora"),
    (r"vegas ?pro|magix", "editor", "Vegas"),
    (r"media ?composer|\bavid\b", "editor", "Avid"),
    (r"remotion", "renderer", "Remotion"),
    (r"hyperframes", "renderer", "HyperFrames"),
    (r"director-engine|director-render", "renderer", "Director"),
    (r"blender", "renderer", "Blender"),
    (r"after ?effects", "renderer", "After Effects"),
    (r"motion ?canvas", "renderer", "Motion Canvas"),
    (r"cinema ?4d|maxon", "renderer", "Cinema 4D"),
    (r"houdini|sidefx", "renderer", "Houdini"),
    (r"unreal ?engine", "renderer", "Unreal Engine"),
    (r"puppeteer|playwright|headless ?chrom", "renderer", "Headless browser"),
    (r"obs ?studio|\bobs\b", "screen", "OBS"),
    (r"screen ?record|屏幕录制|quicktime player", "screen", "Screen recorder"),
    (r"\bloom\b", "screen", "Loom"),
    (r"sharex", "screen", "ShareX"),
    (r"game ?bar|xbox", "screen", "Xbox Game Bar"),
    (r"shadowplay|nvidia share|geforce experience", "screen", "NVIDIA ShadowPlay"),
    (r"screenflow", "screen", "ScreenFlow"),
    (r"snagit", "screen", "Snagit"),
    (r"bandicam", "screen", "Bandicam"),
    (r"produced by google", "platform", "YouTube"),
    (r"\bvid:v[0-9a-z]{8,}|vid_md5=", "platform", "TikTok/Douyin"),
    (r"instagram", "platform", "Instagram"),
    (r"snapchat", "platform", "Snapchat"),
    (r"vimeo", "platform", "Vimeo"),
    (r"douyin|抖音|bytedance", "platform", "Douyin"),
    (r"Tencent CAPD MTS|\(T-tech\)", "platform", "WeChat transcode"),
    (r"Wxmm_", "platform", "WeChat Channels"),
    (r"kuaishou|快手", "platform", "Kuaishou"),
    (r"xiaohongshu|小红书", "platform", "Xiaohongshu"),
    (r"\blavf\d|\blavc\d|ffmpeg", "tool", "FFmpeg"),
    (r"handbrake", "tool", "HandBrake"),
    (r"yt-dlp|youtube-dl", "tool", "yt-dlp"),
];

const CAMERA_KEYS: &[&str] = &[
    "com.apple.quicktime.make",
    "com.apple.quicktime.model",
    "make",
    "model",
    "com.android.manufacturer",
    "com.android.model",
    "location",
    "com.apple.quicktime.location.iso6709",
];
const AIGC_KEYS: &[&str] = &["aigc", "ai_generated", "aigc_label", "contentproducer", "produceid"];
const SKIP_BOXES: &[&[u8; 4]] = &[b"mdat", b"free", b"skip", b"wide"];

/// An AI-generated content label found in the metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AigcLabel {
    /// `gb45438` (China's implicit label), `tiktok` (`aigc_label_type`) or `unknown`.
    pub scheme: String,
    pub label: String,
    pub meaning: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_producer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub produce_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_propagator: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub propagate_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

/// One piece of provenance evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    pub kind: String,
    pub detail: String,
    /// `strong`, `medium` or `weak`.
    pub weight: String,
}

/// What the container says about where the video came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    pub container: String,
    pub encoder: String,
    pub handlers: Vec<String>,
    pub creation_time: String,
    pub aigc: Option<AigcLabel>,
    /// Category to labels: generator, generator_weak, editor, renderer, camera, screen, platform, tool.
    /// A trailing `?` marks a name read only from raw container strings.
    pub hints: BTreeMap<String, Vec<String>>,
    pub evidence: Vec<Evidence>,
    /// Every format and stream tag, keyed `format.<key>` and `stream<i>.<key>`.
    pub tags: BTreeMap<String, String>,
}

struct Signature {
    tags: Regex,
    bytes: Regex,
    category: &'static str,
    label: &'static str,
}

fn signatures() -> &'static [Signature] {
    static SIGS: OnceLock<Vec<Signature>> = OnceLock::new();
    SIGS.get_or_init(|| {
        let build = |p: &str| RegexBuilder::new(p).case_insensitive(true).build().expect("signature pattern");
        SIGNATURES
            .iter()
            .map(|&(p, category, label)| Signature { tags: build(p), bytes: build(&format!(r"\b(?:{p})\b")), category, label })
            .collect()
    })
}

fn tag_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Format and stream tags in probe order.
fn flatten_tags(raw: &Value) -> Vec<(String, String)> {
    let mut tags = Vec::new();
    if let Some(map) = raw.pointer("/format/tags").and_then(Value::as_object) {
        tags.extend(map.iter().map(|(k, v)| (format!("format.{k}"), tag_text(v))));
    }
    if let Some(streams) = raw.get("streams").and_then(Value::as_array) {
        for (i, s) in streams.iter().enumerate() {
            if let Some(map) = s.get("tags").and_then(Value::as_object) {
                tags.extend(map.iter().map(|(k, v)| (format!("stream{i}.{k}"), tag_text(v))));
            }
        }
    }
    tags
}

/// Metadata bytes of an MP4 or MOV: every top-level box except media data and padding, up to `limit`.
/// Other containers give their first and last 4 MB.
pub fn scan_bytes(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();
    let mut out = Vec::new();
    let (mut pos, mut iso) = (0u64, false);
    while pos + 8 <= size && (out.len() as u64) < limit {
        f.seek(SeekFrom::Start(pos))?;
        let mut hdr = [0u8; 16];
        let got = read_up_to(&mut f, &mut hdr)?;
        let kind = &hdr[4..8];
        if !kind.iter().all(|&c| c.is_ascii_alphanumeric() || c == b' ' || c == 0xa9) {
            break;
        }
        let (mut box_size, mut head) = (u32::from_be_bytes(hdr[..4].try_into().unwrap()) as u64, 8u64);
        if box_size == 1 && got >= 16 {
            box_size = u64::from_be_bytes(hdr[8..16].try_into().unwrap());
            head = 16;
        } else if box_size == 0 {
            box_size = size - pos;
        }
        if box_size < head {
            break;
        }
        iso |= kind == b"ftyp";
        if !SKIP_BOXES.iter().any(|s| s.as_slice() == kind) {
            f.seek(SeekFrom::Start(pos))?;
            let take = box_size.min(limit - out.len() as u64);
            (&mut f).take(take).read_to_end(&mut out)?;
        }
        pos = pos.saturating_add(box_size);
    }
    if iso {
        return Ok(out);
    }
    const EDGE: u64 = 4 << 20;
    let mut bytes = Vec::new();
    f.seek(SeekFrom::Start(0))?;
    (&mut f).take(EDGE.min(size)).read_to_end(&mut bytes)?;
    if size > EDGE {
        f.seek(SeekFrom::Start(size - EDGE))?;
        (&mut f).take(EDGE).read_to_end(&mut bytes)?;
    }
    Ok(bytes)
}

fn read_up_to(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// Printable ASCII runs of at least `min_len` bytes, joined by spaces.
fn printable_strings(raw: &[u8], min_len: usize) -> String {
    let mut out = String::new();
    for run in raw.split(|&c| !(0x20..=0x7e).contains(&c)) {
        if run.len() >= min_len {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(std::str::from_utf8(run).unwrap_or_default());
        }
    }
    out
}

/// Parses an AIGC label: China's GB 45438-2025 implicit label (`{"Label": "1", "ContentProducer": ...}`)
/// or TikTok's `aigc_info` (`{"aigc_label_type": 0}`).
pub fn parse_aigc(value: &str) -> Option<AigcLabel> {
    let data: Value = serde_json::from_str(value).ok().or_else(|| {
        let (a, b) = (value.find('{')?, value.rfind('}')?);
        (a < b).then(|| serde_json::from_str(&value[a..=b]).ok()).flatten()
    })?;
    let obj = data.as_object()?;
    let text = |k: &str| obj.get(k).map(tag_text).unwrap_or_default();
    if let Some(kind) = obj.get("aigc_label_type") {
        let label = tag_text(kind);
        let meaning = if label == "0" { "no AI label".to_string() } else { format!("TikTok AI-generated label type {label}") };
        return Some(AigcLabel {
            scheme: "tiktok".into(),
            label,
            meaning,
            content_producer: String::new(),
            produce_id: String::new(),
            content_propagator: String::new(),
            propagate_id: String::new(),
            raw: None,
        });
    }
    let label = obj.get("Label").or_else(|| obj.get("label")).map(tag_text).unwrap_or_default().trim().to_string();
    let meaning = match label.as_str() {
        "1" => "labelled as AI-generated synthetic content",
        "2" => "possibly AI-generated synthetic content",
        "3" => "suspected AI-generated synthetic content",
        _ => "unknown value",
    };
    Some(AigcLabel {
        scheme: "gb45438".into(),
        meaning: meaning.into(),
        label,
        content_producer: text("ContentProducer"),
        produce_id: text("ProduceID"),
        content_propagator: text("ContentPropagator"),
        propagate_id: text("PropagateID"),
        raw: None,
    })
}

/// Reads the container's provenance.
pub fn provenance(info: &VideoInfo) -> Provenance {
    let tags = flatten_tags(&info.raw);
    let mut evidence = Vec::new();
    let mut hints: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut hint = |cat: &str, label: String| hints.entry(cat.to_string()).or_default().push(label);
    let lower: Vec<(String, &str)> = tags.iter().map(|(k, v)| (k.to_lowercase(), v.as_str())).collect();
    let aigc_word = RegexBuilder::new(r"\baigc\b").case_insensitive(true).build().unwrap();

    let mut aigc = None;
    for (key, value) in &lower {
        let short = key.split_once('.').map_or(key.as_str(), |(_, s)| s);
        if CAMERA_KEYS.contains(&short) {
            hint("camera", format!("{short}={value}"));
        }
        if AIGC_KEYS.iter().any(|a| short.contains(a)) || aigc_word.is_match(value) {
            let label = parse_aigc(value).unwrap_or_else(|| AigcLabel {
                scheme: "unknown".into(),
                label: String::new(),
                meaning: "could not be parsed".into(),
                content_producer: String::new(),
                produce_id: String::new(),
                content_propagator: String::new(),
                propagate_id: String::new(),
                raw: Some(value.chars().take(200).collect()),
            });
            let declared = match label.scheme.as_str() {
                "gb45438" => matches!(label.label.as_str(), "1" | "2" | "3"),
                "tiktok" => label.label != "0" && !label.label.is_empty(),
                _ => false,
            };
            let detail = match label.scheme.as_str() {
                "tiktok" => format!("TikTok AIGC field: aigc_label_type={} ({})", label.label, label.meaning),
                _ => format!(
                    "Implicit AIGC label Label={} ({}), content producer code {}",
                    label.label,
                    label.meaning,
                    if label.content_producer.is_empty() { "none" } else { &label.content_producer }
                ),
            };
            evidence.push(Evidence { kind: "aigc_label".into(), detail, weight: if declared { "strong" } else { "weak" }.into() });
            if (label.scheme == "gb45438" && label.label == "1") || (label.scheme == "tiktok" && declared) {
                hint("generator", format!("AIGC label ({})", label.meaning));
            }
            aigc = Some(label);
        }
    }

    let blob = tags.iter().filter(|(k, _)| !k.to_lowercase().contains("aigc")).map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ");
    let raw_bytes = scan_bytes(&info.path, 8 << 20).unwrap_or_default();
    let strings = printable_strings(&raw_bytes, 6);
    for sig in signatures() {
        if sig.tags.is_match(&blob) {
            hint(sig.category, sig.label.to_string());
            evidence.push(Evidence {
                kind: format!("{}_signature", sig.category),
                detail: format!("Metadata contains {}", sig.label),
                weight: "medium".into(),
            });
        } else if matches!(sig.category, "generator" | "renderer" | "editor") && sig.bytes.is_match(&strings) {
            // A readable string inside the container boxes is only a hint: short names also occur by chance.
            let cat = if sig.category == "generator" { "generator_weak" } else { sig.category };
            hint(cat, format!("{}?", sig.label));
            evidence.push(Evidence {
                kind: format!("{}_bytes", sig.category),
                detail: format!("Readable string {} found inside the container", sig.label),
                weight: "weak".into(),
            });
        }
    }

    let has = |needle: &[u8]| raw_bytes.windows(needle.len()).any(|w| w == needle);
    if has(b"c2pa") || has(b"jumb") {
        // Cameras sign C2PA too: only a generator named beside the manifest counts as AI.
        let named: Vec<String> = hints.get("generator_weak").cloned().unwrap_or_default();
        let detail = if named.is_empty() {
            "C2PA / JUMBF content-credential box found".to_string()
        } else {
            format!("C2PA / JUMBF content-credential box found ({})", named.join(", "))
        };
        evidence.push(Evidence { kind: "c2pa".into(), detail, weight: if named.is_empty() { "medium" } else { "strong" }.into() });
        if !named.is_empty() {
            let names: Vec<&str> = named.iter().map(|n| n.trim_end_matches('?')).collect();
            hints.entry("generator".into()).or_default().push(format!("C2PA credential: {}", names.join(", ")));
        }
    }
    if let Some(camera) = hints.get("camera") {
        evidence.push(Evidence { kind: "camera_tags".into(), detail: camera.iter().take(4).cloned().collect::<Vec<_>>().join("; "), weight: "strong".into() });
    }
    for list in hints.values_mut() {
        list.sort();
        list.dedup();
    }

    let encoder = lower
        .iter()
        .find(|(k, _)| k == "format.encoder")
        .or_else(|| lower.iter().find(|(k, _)| k.ends_with(".encoder")))
        .map(|(_, v)| v.to_string())
        .unwrap_or_default();
    let mut handlers: Vec<String> = lower.iter().filter(|(k, _)| k.ends_with("handler_name")).map(|(_, v)| v.to_string()).collect();
    handlers.sort();
    handlers.dedup();
    let creation_time = lower.iter().find(|(k, _)| k == "format.creation_time").map(|(_, v)| v.to_string()).unwrap_or_default();
    Provenance {
        container: info.raw.pointer("/format/format_name").and_then(Value::as_str).unwrap_or("").to_string(),
        encoder,
        handlers,
        creation_time,
        aigc,
        hints,
        evidence,
        tags: tags.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_aigc_schemes() {
        let gb = parse_aigc(r#"{"Label":"1","ContentProducer":"001191110108MA01KP2T5U00000"}"#).unwrap();
        assert_eq!((gb.scheme.as_str(), gb.label.as_str()), ("gb45438", "1"));
        let tt = parse_aigc(r#"{"aigc_label_type":0}"#).unwrap();
        assert_eq!((tt.scheme.as_str(), tt.label.as_str(), tt.meaning.as_str()), ("tiktok", "0", "no AI label"));
        assert!(parse_aigc("plain text").is_none());
    }

    #[test]
    fn printable_runs_need_six_bytes() {
        assert_eq!(printable_strings(b"ab\0Remotion\x01xyz12345", 6), "Remotion xyz12345");
    }

    #[test]
    fn every_signature_compiles() {
        assert_eq!(signatures().len(), SIGNATURES.len());
    }
}
