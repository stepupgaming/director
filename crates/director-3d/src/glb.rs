//! What a `.glb` holds, read from its JSON chunk without a GPU.

/// The parts of a glTF binary that change how its node paints over time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlbInfo {
    /// Animation clips. Every clip plays at once.
    pub clips: usize,
    /// The longest clip in seconds.
    pub duration: f64,
    /// Cameras. The first one frames the shot.
    pub cameras: usize,
}

/// Reads the glTF 2.0 binary header and JSON chunk.
pub fn inspect(bytes: &[u8]) -> Result<GlbInfo, String> {
    let word = |at: usize| -> Result<u32, String> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| "truncated glTF binary".to_string())
    };
    if bytes.get(..4) != Some(b"glTF") {
        return Err("model src must be a glTF binary (.glb)".into());
    }
    if word(4)? != 2 {
        return Err(format!("glTF binary version {} (2 is supported)", word(4)?));
    }
    let length = word(12)? as usize;
    if word(16)? != 0x4E4F_534A {
        return Err("glTF binary without a JSON chunk".into());
    }
    let json = bytes.get(20..20 + length).ok_or("truncated glTF JSON chunk")?;
    let json: serde_json::Value = serde_json::from_slice(json).map_err(|e| format!("glTF JSON: {e}"))?;
    let list = |key: &str| json.get(key).and_then(|v| v.as_array()).map_or(&[][..], |v| v.as_slice());
    let accessors = list("accessors");
    let mut duration = 0.0f64;
    for animation in list("animations") {
        for sampler in animation.get("samplers").and_then(|v| v.as_array()).into_iter().flatten() {
            let end = sampler
                .get("input")
                .and_then(|i| i.as_u64())
                .and_then(|i| accessors.get(i as usize))
                .and_then(|a| a.get("max"))
                .and_then(|m| m.get(0))
                .and_then(|m| m.as_f64())
                .ok_or("glTF animation input without a max")?;
            duration = duration.max(end);
        }
    }
    Ok(GlbInfo {
        clips: list("animations").len(),
        duration,
        cameras: list("cameras").len(),
    })
}

/// A glTF binary whose one scene holds one empty node, the base of a model
/// made only of splats.
pub(crate) fn empty() -> Vec<u8> {
    let mut json = br#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{}]}"#.to_vec();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() as u32;
    let mut glb = Vec::with_capacity(total as usize);
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&total.to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    glb.extend_from_slice(&json);
    glb
}
