//! Paper Shader GLSL ES 300 to SKSL. Capture stores the evaluated fragment and the
//! uniforms the page uploaded. This module rebuilds the shared vertex varyings and
//! draws the fragment with Skia's runtime effect.
use crate::capture::{CapturedShader, CapturedShaderImage};
use base64::{engine::general_purpose::STANDARD, Engine};
use skia_safe::{
    images, runtime_effect::uniform::Type, runtime_effect::ChildPtr, AlphaType, ColorType, Data,
    FilterMode, ImageInfo, Matrix, MipmapMode, Paint, Rect, RuntimeEffect, SamplingOptions, Shader,
    TileMode,
};
use std::{cell::RefCell, collections::BTreeMap};

const PRELUDE: &str = r#"
uniform float2 u_paperSize;
uniform float2 u_resolution;
uniform float u_pixelRatio;
uniform float u_imageAspectRatio;
uniform float u_originX;
uniform float u_originY;
uniform float u_worldWidth;
uniform float u_worldHeight;
uniform float u_fit;
uniform float u_scale;
uniform float u_rotation;
uniform float u_offsetX;
uniform float u_offsetY;

float2 v_objectUV;
float2 v_objectBoxSize;
float2 v_responsiveUV;
float2 v_responsiveBoxGivenSize;
float2 v_patternUV;
float2 v_patternBoxSize;
float2 v_imageUV;
float2 paperFragCoord;
float4 fragColor;

float paperFwidth(float v) { return 1.5 / max(max(u_resolution.x, u_resolution.y), 1.0); }
float2 paperFwidth(float2 v) { float w = paperFwidth(v.x); return float2(w); }
float3 paperFwidth(float3 v) { float w = paperFwidth(v.x); return float3(w); }
float4 paperFwidth(float4 v) { float w = paperFwidth(v.x); return float4(w); }

int paperIMod(int x, int y) { return y == 0 ? 0 : x - y * (x / y); }
int paperClampInt(int x, int lo, int hi) { return x < lo ? lo : (x > hi ? hi : x); }
float paperRound(float x) { return x >= 0.0 ? floor(x + 0.5) : ceil(x - 0.5); }
float2 paperRound(float2 v) { return float2(paperRound(v.x), paperRound(v.y)); }
float3 paperRound(float3 v) { return float3(paperRound(v.x), paperRound(v.y), paperRound(v.z)); }
float4 paperRound(float4 v) { return float4(paperRound(v.x), paperRound(v.y), paperRound(v.z), paperRound(v.w)); }
float2x2 paperTranspose(float2x2 m) { return float2x2(m[0].x, m[1].x, m[0].y, m[1].y); }

float3 paperBoxSize(float boxRatio, float2 givenBoxSize) {
    float2 box = float2(0.0);
    box.x = boxRatio * min(givenBoxSize.x / boxRatio, givenBoxSize.y);
    float noFitBoxWidth = box.x;
    if (u_fit == 1.0) {
        box.x = boxRatio * min(u_resolution.x / boxRatio, u_resolution.y);
    } else if (u_fit == 2.0) {
        box.x = boxRatio * max(u_resolution.x / boxRatio, u_resolution.y);
    }
    box.y = box.x / boxRatio;
    return float3(box, noFitBoxWidth);
}

void paperPrepare(float2 paperCoord) {
    fragColor = float4(0.0);
    float2 uv = float2(paperCoord.x / u_paperSize.x - 0.5, 0.5 - paperCoord.y / u_paperSize.y);
    paperFragCoord = float2(
        paperCoord.x * u_resolution.x / u_paperSize.x,
        u_resolution.y - paperCoord.y * u_resolution.y / u_paperSize.y);
    float2 boxOrigin = float2(0.5 - u_originX, u_originY - 0.5);
    float2 givenBoxSize = max(float2(u_worldWidth, u_worldHeight), float2(1.0)) * u_pixelRatio;
    float r = u_rotation * 3.141592653589793 / 180.0;
    float2x2 graphicRotation = float2x2(cos(r), sin(r), -sin(r), cos(r));
    float2 graphicOffset = float2(-u_offsetX, u_offsetY);
    float safeScale = u_scale == 0.0 ? 1.0 : u_scale;
    float2 fixedBox = float2(
        u_worldWidth == 0.0 ? u_resolution.x : givenBoxSize.x,
        u_worldHeight == 0.0 ? u_resolution.y : givenBoxSize.y);
    v_objectBoxSize = paperBoxSize(1.0, fixedBox).xy;
    float2 objectWorldScale = u_resolution / v_objectBoxSize;
    v_objectUV = uv * objectWorldScale + boxOrigin * (objectWorldScale - 1.0) + graphicOffset;
    v_objectUV /= safeScale;
    v_objectUV = graphicRotation * v_objectUV;

    v_responsiveBoxGivenSize = fixedBox;
    float responsiveRatio = v_responsiveBoxGivenSize.x / v_responsiveBoxGivenSize.y;
    float2 responsiveBoxSize = paperBoxSize(responsiveRatio, v_responsiveBoxGivenSize).xy;
    float2 responsiveBoxScale = u_resolution / responsiveBoxSize;
    v_responsiveUV = uv * responsiveBoxScale + boxOrigin * (responsiveBoxScale - 1.0) + graphicOffset;
    v_responsiveUV /= safeScale;
    v_responsiveUV.x *= responsiveRatio;
    v_responsiveUV = graphicRotation * v_responsiveUV;
    v_responsiveUV.x /= responsiveRatio;

    float patternRatio = fixedBox.x / fixedBox.y;
    float3 patternData = paperBoxSize(patternRatio, fixedBox);
    v_patternBoxSize = patternData.xy;
    float2 patternBoxScale = u_resolution / v_patternBoxSize;
    v_patternUV = uv + graphicOffset / patternBoxScale + boxOrigin - boxOrigin / patternBoxScale;
    v_patternUV *= u_resolution;
    v_patternUV /= u_pixelRatio;
    if (u_fit > 0.0) v_patternUV *= patternData.z / v_patternBoxSize.x;
    v_patternUV /= safeScale;
    v_patternUV = graphicRotation * v_patternUV;
    v_patternUV += boxOrigin / patternBoxScale - boxOrigin;
    v_patternUV *= 0.01;

    float imageAspect = u_imageAspectRatio == 0.0 ? 1.0 : u_imageAspectRatio;
    float2 imageBoxSize = float2(0.0);
    if (u_fit == 1.0) {
        imageBoxSize.x = min(u_resolution.x / imageAspect, u_resolution.y) * imageAspect;
    } else if (u_fit == 2.0) {
        imageBoxSize.x = max(u_resolution.x / imageAspect, u_resolution.y) * imageAspect;
    } else {
        imageBoxSize.x = min(10.0, 10.0 / imageAspect * imageAspect);
    }
    imageBoxSize.y = imageBoxSize.x / imageAspect;
    float2 imageBoxScale = u_resolution / imageBoxSize;
    v_imageUV = uv * imageBoxScale + boxOrigin * (imageBoxScale - 1.0) + graphicOffset;
    v_imageUV /= safeScale;
    v_imageUV.x *= imageAspect;
    v_imageUV = graphicRotation * v_imageUV;
    v_imageUV.x /= imageAspect;
    v_imageUV += 0.5;
    v_imageUV.y = 1.0 - v_imageUV.y;
}
"#;

const PRELUDE_UNIFORMS: &[&str] = &[
    "u_paperSize",
    "u_resolution",
    "u_pixelRatio",
    "u_imageAspectRatio",
    "u_originX",
    "u_originY",
    "u_worldWidth",
    "u_worldHeight",
    "u_fit",
    "u_scale",
    "u_rotation",
    "u_offsetX",
    "u_offsetY",
];

#[derive(Clone)]
pub struct PaperPaint {
    sksl: String,
    uniforms: BTreeMap<String, Vec<f32>>,
    images: BTreeMap<String, PaperImage>,
}

#[derive(Clone)]
struct PaperImage {
    width: i32,
    height: i32,
    rgba: std::sync::Arc<Vec<u8>>,
    /// WebGL uploaded this image with UNPACK_FLIP_Y, so v=0 is the visual top.
    flip_y: bool,
}

impl std::fmt::Debug for PaperPaint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaperPaint")
            .field("uniforms", &self.uniforms.keys().collect::<Vec<_>>())
            .field("images", &self.images.keys().collect::<Vec<_>>())
            .field("sksl_len", &self.sksl.len())
            .finish()
    }
}

impl PaperPaint {
    pub fn draw(&self, canvas: &skia_safe::Canvas, rect: Rect) -> Result<(), director_core::RenderError> {
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return Ok(());
        }
        let effect = effect(&self.sksl).map_err(director_core::RenderError::SkiaError)?;
        let data = uniform_data(&effect, &self.uniforms, &self.images, rect.width(), rect.height())
            .map_err(director_core::RenderError::SkiaError)?;
        let mut children = Vec::new();
        for child in effect.children() {
            let image = self.images.get(child.name()).ok_or_else(|| {
                director_core::RenderError::SkiaError(format!(
                    "shader child {} has no captured image",
                    child.name()
                ))
            })?;
            let shader = image_shader(image).map_err(director_core::RenderError::SkiaError)?;
            children.push(ChildPtr::Shader(shader));
        }
        let shader = effect
            .make_shader(data, &children, None)
            .ok_or_else(|| director_core::RenderError::SkiaError("runtime shader rejected uniforms".into()))?;
        let mut paint = Paint::default();
        paint.set_shader(shader);
        canvas.save();
        canvas.translate((rect.left(), rect.top()));
        canvas.draw_rect(Rect::from_wh(rect.width(), rect.height()), &paint);
        canvas.restore();
        Ok(())
    }
}

pub fn prepare(shader: &CapturedShader) -> Result<PaperPaint, String> {
    if shader.fragment.len() > 200_000 {
        return Err("shader source exceeds 200000 characters".into());
    }
    if shader.fragment.contains("${") {
        return Err("shader source still contains template placeholders".into());
    }
    let sksl = translate(&shader.fragment)?;
    let effect = effect(&sksl)?;
    let mut images = BTreeMap::new();
    for (name, image) in &shader.images {
        images.insert(name.clone(), decode_image(image)?);
    }
    for child in effect.children() {
        if !images.contains_key(child.name()) {
            return Err(format!(
                "shader child {} has no captured image",
                child.name()
            ));
        }
    }
    for (name, values) in &shader.uniforms {
        if values.len() > 4096 {
            return Err(format!("uniform {name} exceeds 4096 values"));
        }
    }
    Ok(PaperPaint {
        sksl,
        uniforms: shader.uniforms.clone(),
        images,
    })
}

fn effect(sksl: &str) -> Result<RuntimeEffect, String> {
    thread_local! {
        static CACHE: RefCell<BTreeMap<String, RuntimeEffect>> = const { RefCell::new(BTreeMap::new()) };
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(effect) = cache.get(sksl) {
            return Ok(effect.clone());
        }
        let options = skia_safe::runtime_effect::Options {
            force_unoptimized: true,
            name: "paper",
        };
        let compiled = RuntimeEffect::make_for_shader(sksl, Some(&options))?;
        cache.insert(sksl.to_string(), compiled.clone());
        Ok(compiled)
    })
}

fn decode_image(image: &CapturedShaderImage) -> Result<PaperImage, String> {
    if image.width <= 0 || image.height <= 0 || image.width > 4096 || image.height > 4096 {
        return Err(format!(
            "shader image {}x{} is outside 1..4096",
            image.width, image.height
        ));
    }
    let rgba = STANDARD
        .decode(image.rgba.trim())
        .map_err(|e| format!("shader image base64: {e}"))?;
    let expected = image.width as usize * image.height as usize * 4;
    if rgba.len() != expected {
        return Err(format!(
            "shader image is {} bytes, expected {expected}",
            rgba.len()
        ));
    }
    Ok(PaperImage {
        width: image.width,
        height: image.height,
        rgba: std::sync::Arc::new(rgba),
        flip_y: image.flip_y,
    })
}

fn image_shader(image: &PaperImage) -> Result<Shader, String> {
    let info = ImageInfo::new(
        (image.width, image.height),
        ColorType::RGBA8888,
        AlphaType::Unpremul,
        None,
    );
    let bitmap = images::raster_from_data(&info, Data::new_copy(&image.rgba), image.width as usize * 4)
        .ok_or_else(|| "shader image pixels were rejected".to_string())?;
    let mut matrix = Matrix::new_identity();
    // Default WebGL upload keeps v=0 at the visual bottom. UNPACK_FLIP_Y puts v=0 at the top.
    if image.flip_y {
        matrix.set_scale((image.width as f32, image.height as f32), None);
    } else {
        matrix.set_scale_translate(
            (image.width as f32, -(image.height as f32)),
            (0.0, image.height as f32),
        );
    }
    bitmap
        .to_shader(
            (TileMode::Clamp, TileMode::Clamp),
            SamplingOptions::new(FilterMode::Linear, MipmapMode::None),
            &matrix,
        )
        .ok_or_else(|| "shader image could not become a child shader".to_string())
}

fn uniform_data(
    effect: &RuntimeEffect,
    supplied: &BTreeMap<String, Vec<f32>>,
    images: &BTreeMap<String, PaperImage>,
    width: f32,
    height: f32,
) -> Result<Data, String> {
    let mut bytes = vec![0u8; effect.uniform_size()];
    for uniform in effect.uniforms() {
        let slots = uniform.size_in_bytes() / 4;
        if slots == 0 || uniform.offset() + uniform.size_in_bytes() > bytes.len() {
            return Err(format!("uniform {} has an unsupported layout", uniform.name()));
        }
        let values = padded(uniform.name(), slots, supplied, images, width, height);
        let int = matches!(
            uniform.ty(),
            Type::Int | Type::Int2 | Type::Int3 | Type::Int4
        );
        for (i, value) in values.iter().enumerate() {
            let offset = uniform.offset() + i * 4;
            let chunk = if int {
                (*value as i32).to_le_bytes()
            } else {
                value.to_le_bytes()
            };
            bytes[offset..offset + 4].copy_from_slice(&chunk);
        }
    }
    Ok(Data::new_copy(&bytes))
}

fn padded(
    name: &str,
    slots: usize,
    supplied: &BTreeMap<String, Vec<f32>>,
    images: &BTreeMap<String, PaperImage>,
    width: f32,
    height: f32,
) -> Vec<f32> {
    let mut out = vec![0.0; slots];
    if let Some(values) = supplied.get(name) {
        for (slot, value) in values.iter().take(slots).enumerate() {
            out[slot] = *value;
        }
    }
    match name {
        "u_paperSize" if slots >= 2 => {
            out[0] = width;
            out[1] = height;
        }
        "u_resolution" if slots >= 2 && out[0] == 0.0 && out[1] == 0.0 => {
            out[0] = width;
            out[1] = height;
        }
        "u_pixelRatio" if slots >= 1 && out[0] == 0.0 => out[0] = 1.0,
        "u_imageAspectRatio" if slots >= 1 && out[0] == 0.0 => out[0] = 1.0,
        "u_originX" | "u_originY" if slots >= 1 && !supplied.contains_key(name) => out[0] = 0.5,
        "u_scale" if slots >= 1 && out[0] == 0.0 => out[0] = 1.0,
        "u_fit" if slots >= 1 && !supplied.contains_key(name) => out[0] = 1.0,
        _ => {}
    }
    if let Some(image_name) = name.strip_prefix("paperTexSize_") {
        if let Some(image) = images.get(image_name) {
            if slots >= 2 {
                out[0] = image.width as f32;
                out[1] = image.height as f32;
            }
        }
    }
    out
}

pub fn translate(fragment: &str) -> Result<String, String> {
    let mut src = strip_comments(fragment);
    src = expand_defines(&src)?;
    src = replace_idents(&src, PRECISION);
    src = strip_headers(&src);
    src = rewrite_bool_uniforms(&src)?;
    src = replace_idents(&src, IDENT_MAP);
    src = strip_declarations(&src);
    reject_unsupported(&src)?;
    src = hoist_shader_arguments(src)?;
    src = replace_builtin_calls(&src)?;
    src = declare_texture_sizes(src)?;
    src = rewrite_const_arrays(&src)?;
    src = rewrite_dynamic_indexes(&src)?;
    src = rewrite_integer_mod(&src)?;
    src = rewrite_counted_loops(&src)?;
    src = rewrite_main(&src)?;
    Ok(format!("{PRELUDE}\n{src}"))
}

const PRECISION: &[(&str, &str)] = &[("highp", ""), ("mediump", ""), ("lowp", "")];

const IDENT_MAP: &[(&str, &str)] = &[
    ("sampler2D", "shader"),
    ("ivec2", "int2"),
    ("ivec3", "int3"),
    ("ivec4", "int4"),
    ("bvec2", "bool2"),
    ("bvec3", "bool3"),
    ("bvec4", "bool4"),
    ("uvec2", "int2"),
    ("uvec3", "int3"),
    ("uvec4", "int4"),
    ("vec2", "float2"),
    ("vec3", "float3"),
    ("vec4", "float4"),
    ("mat2", "float2x2"),
    ("mat3", "float3x3"),
    ("mat4", "float4x4"),
    ("gl_FragCoord", "paperFragCoord"),
    ("gl_FragColor", "fragColor"),
];

const FORBIDDEN: &[&str] = &[
    "samplerCube",
    "sampler3D",
    "isampler2D",
    "usampler2D",
    "texelFetch",
    "imageLoad",
    "imageStore",
    "atomicAdd",
    "barrier",
];

fn reject_unsupported(src: &str) -> Result<(), String> {
    for name in FORBIDDEN {
        if has_ident(src, name) {
            return Err(format!("shader uses unsupported {name}"));
        }
    }
    Ok(())
}

fn has_ident(src: &str, name: &str) -> bool {
    let chars: Vec<char> = src.chars().collect();
    let target: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i + target.len() <= chars.len() {
        if chars[i..i + target.len()] == target[..]
            && (i == 0 || !is_ident(chars[i - 1]))
            && (i + target.len() == chars.len() || !is_ident(chars[i + target.len()]))
        {
            return true;
        }
        i += 1;
    }
    false
}

fn expand_defines(src: &str) -> Result<String, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = String::with_capacity(src.len());
    let mut macros: Vec<(String, String)> = Vec::new();
    while i < chars.len() {
        if chars[i] == '#' && directive_at_line_start(&chars, i) && ident_at(&chars, i + 1, "define") {
            i += "#define".len();
            i = skip_ws(&chars, i);
            if i >= chars.len() || !is_ident_start(chars[i]) {
                return Err("shader #define is missing a name".into());
            }
            let name_at = i;
            i += 1;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let name: String = chars[name_at..i].iter().collect();
            if i < chars.len() && chars[i] == '(' {
                return Err(format!("shader #define {name}() is not a supported macro"));
            }
            i = skip_ws(&chars, i);
            let mut body = String::new();
            while i < chars.len() && chars[i] != '\n' {
                if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '\n' {
                    body.push(' ');
                    i += 2;
                    continue;
                }
                body.push(chars[i]);
                i += 1;
            }
            macros.push((name, body.trim().to_string()));
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    macros.sort_by_key(|item| std::cmp::Reverse(item.0.len()));
    for _ in 0..32 {
        let mut next = out.clone();
        for (name, body) in &macros {
            next = replace_idents(&next, &[(name.as_str(), body.as_str())]);
        }
        if next == out {
            return Ok(out);
        }
        out = next;
    }
    Err("shader #define expansion did not finish".into())
}

fn directive_at_line_start(chars: &[char], hash: usize) -> bool {
    let mut i = hash;
    while i > 0 {
        i -= 1;
        if chars[i] == '\n' {
            return true;
        }
        if !chars[i].is_whitespace() {
            return false;
        }
    }
    true
}

fn rewrite_bool_uniforms(src: &str) -> Result<String, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = String::with_capacity(src.len());
    let mut names = Vec::new();
    while i < chars.len() {
        if ident_at(&chars, i, "uniform") {
            let mut j = skip_ws(&chars, i + "uniform".len());
            if ident_at(&chars, j, "bool") {
                j = skip_ws(&chars, j + "bool".len());
                if j < chars.len() && is_ident_start(chars[j]) {
                    let name_at = j;
                    j += 1;
                    while j < chars.len() && is_ident(chars[j]) {
                        j += 1;
                    }
                    let name: String = chars[name_at..j].iter().collect();
                    j = skip_ws(&chars, j);
                    if j < chars.len() && chars[j] == ';' {
                        out.push_str(&format!("uniform int paperBoolKeep_{name};"));
                        names.push(name);
                        i = j + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    for name in names {
        out = replace_idents(&out, &[(name.as_str(), &format!("({name} != 0)"))]);
        out = out.replace(&format!("paperBoolKeep_{name}"), &name);
    }
    Ok(out)
}

struct ShaderFn {
    name: String,
    paren: usize,
    paren_end: usize,
    body_start: usize,
    body_end: usize,
    shader_params: Vec<(usize, String)>,
}

fn hoist_shader_arguments(mut src: String) -> Result<String, String> {
    for _ in 0..16 {
        let chars: Vec<char> = src.chars().collect();
        let Some(function) = shader_function(&chars) else {
            return Ok(src);
        };
        let mut images = Vec::new();
        let mut calls = Vec::new();
        let mut search = 0;
        while search < chars.len() {
            if ident_at(&chars, search, &function.name) {
                let paren = skip_ws(&chars, search + function.name.len());
                if paren < chars.len() && chars[paren] == '(' && paren != function.paren {
                    if paren > function.body_start && paren < function.body_end {
                        return Err(format!("shader function {} calls itself", function.name));
                    }
                    let args = split_args(&chars, paren)?;
                    let mut call_images = Vec::new();
                    for (index, name) in &function.shader_params {
                        let arg = args.get(*index).map(|arg| arg.trim()).unwrap_or("");
                        if arg.is_empty() || !arg.chars().all(is_ident) {
                            return Err(format!(
                                "shader parameter {name} in {} must be a captured image",
                                function.name
                            ));
                        }
                        call_images.push(arg.to_string());
                    }
                    if images.is_empty() {
                        images = call_images;
                    } else if images != call_images {
                        return Err(format!(
                            "shader function {} is passed more than one image",
                            function.name
                        ));
                    }
                    if let Some(end) = matching_paren(&chars, paren) {
                        calls.push((paren + 1, end));
                    }
                }
            }
            search += 1;
        }
        if images.is_empty() {
            return Err(format!(
                "shader parameter {} in {} is never called",
                function.shader_params[0].1, function.name
            ));
        }
        let params: String = chars[function.paren + 1..function.paren_end].iter().collect();
        let new_params = drop_indexed(&split_top_level(&params), &function.shader_params);
        let body: String = chars[function.body_start..=function.body_end].iter().collect();
        let mut new_body = body;
        for ((_, name), image) in function.shader_params.iter().zip(&images) {
            new_body = replace_idents(&new_body, &[(name.as_str(), image.as_str())]);
        }
        let mut edits = vec![
            (function.paren + 1, function.paren_end, new_params),
            (function.body_start, function.body_end + 1, new_body),
        ];
        for (start, end) in calls {
            let args: String = chars[start..end].iter().collect();
            edits.push((start, end, drop_indexed(&split_top_level(&args), &function.shader_params)));
        }
        edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
        for (start, end, text) in edits {
            src.replace_range(start..end, &text);
        }
    }
    Err("shader parameter rewrite did not finish".into())
}

fn drop_indexed(parts: &[String], shader_params: &[(usize, String)]) -> String {
    parts
        .iter()
        .enumerate()
        .filter(|(index, _)| !shader_params.iter().any(|(shader_index, _)| shader_index == index))
        .map(|(_, part)| part.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn shader_function(chars: &[char]) -> Option<ShaderFn> {
    let mut depth = 0;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            depth += 1;
        } else if chars[i] == '}' {
            depth -= 1;
        } else if depth == 0 && is_ident_start(chars[i]) {
            let start = i;
            i += 1;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            let paren = skip_ws(chars, i);
            if paren < chars.len() && chars[paren] == '(' {
                if let Some(paren_end) = matching_paren(chars, paren) {
                    let after = skip_ws(chars, paren_end + 1);
                    if after < chars.len() && chars[after] == '{' {
                        if let Some(body_end) = matching_brace(chars, after) {
                            let params: String = chars[paren + 1..paren_end].iter().collect();
                            let shader_params = shader_param_indexes(&params);
                            if !shader_params.is_empty() {
                                return Some(ShaderFn {
                                    name,
                                    paren,
                                    paren_end,
                                    body_start: after,
                                    body_end,
                                    shader_params,
                                });
                            }
                            i = after;
                            continue;
                        }
                    }
                }
            }
            continue;
        }
        i += 1;
    }
    None
}

fn shader_param_indexes(params: &str) -> Vec<(usize, String)> {
    split_top_level(params)
        .into_iter()
        .enumerate()
        .filter_map(|(index, param)| {
            let words: Vec<&str> = param.split_whitespace().collect();
            if words.first() == Some(&"shader") {
                words.last().filter(|name| **name != "shader").map(|name| (index, (*name).to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn declare_texture_sizes(src: String) -> Result<String, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut sizes = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i..].starts_with(&['p', 'a', 'p', 'e', 'r', 'T', 'e', 'x', 'S', 'i', 'z', 'e', '_'])
            && (i == 0 || !is_ident(chars[i - 1]))
        {
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            if !sizes.contains(&name) {
                sizes.push(name);
            }
            continue;
        }
        i += 1;
    }
    if sizes.is_empty() {
        return Ok(src);
    }
    let mut header = String::new();
    for name in sizes {
        header.push_str(&format!("uniform float2 {name};\n"));
    }
    Ok(format!("{header}{src}"))
}

fn split_top_level(src: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    for ch in src.chars() {
        if ch == '(' || ch == '[' || ch == '{' {
            depth += 1;
        } else if ch == ')' || ch == ']' || ch == '}' {
            depth -= 1;
        }
        if ch == ',' && depth == 0 {
            parts.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn matching_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut i = open;
    while i < chars.len() {
        if chars[i] == '(' {
            depth += 1;
        } else if chars[i] == ')' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn rewrite_const_arrays(src: &str) -> Result<String, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = String::with_capacity(src.len());
    while i < chars.len() {
        if let Some((name, values, next)) = const_array_at(&chars, i)? {
            out.push_str(&array_lookup_fn(&name, &values));
            let mut rest: String = chars[next..].iter().collect();
            rest = replace_array_reads(&rest, &name, &format!("{name}_at"));
            rest = rewrite_const_arrays(&rest)?;
            out.push_str(&rest);
            return Ok(out);
        }
        out.push(chars[i]);
        i += 1;
    }
    Ok(out)
}

fn const_array_at(chars: &[char], i: usize) -> Result<Option<(String, Vec<i32>, usize)>, String> {
    let mut j = i;
    if ident_at(chars, j, "const") {
        j = skip_ws(chars, j + "const".len());
    }
    if !ident_at(chars, j, "int") {
        return Ok(None);
    }
    j = skip_ws(chars, j + "int".len());
    if j >= chars.len() || !is_ident_start(chars[j]) {
        return Ok(None);
    }
    let name_at = j;
    j += 1;
    while j < chars.len() && is_ident(chars[j]) {
        j += 1;
    }
    let name: String = chars[name_at..j].iter().collect();
    j = skip_ws(chars, j);
    if j >= chars.len() || chars[j] != '[' {
        return Ok(None);
    }
    let count = take_int_after(chars, j + 1);
    let Some((count, after_count)) = count else {
        return Ok(None);
    };
    j = skip_ws(chars, after_count);
    if j >= chars.len() || chars[j] != ']' {
        return Ok(None);
    }
    j = skip_ws(chars, j + 1);
    if j >= chars.len() || chars[j] != '=' {
        return Ok(None);
    }
    j = skip_ws(chars, j + 1);
    if !ident_at(chars, j, "int") {
        return Err(format!("shader array {name} needs an int constructor"));
    }
    j = skip_ws(chars, j + "int".len());
    if j >= chars.len() || chars[j] != '[' {
        return Err(format!("shader array {name} needs an int constructor"));
    }
    j = skip_ws(chars, j + 1);
    if j < chars.len() && chars[j] != ']' {
        if let Some((_, after)) = take_int_after(chars, j) {
            j = skip_ws(chars, after);
        }
    }
    if j >= chars.len() || chars[j] != ']' {
        return Err(format!("shader array {name} needs an int constructor"));
    }
    j = skip_ws(chars, j + 1);
    if j >= chars.len() || chars[j] != '(' {
        return Err(format!("shader array {name} needs an int constructor"));
    }
    let values_end = matching_paren(chars, j).ok_or_else(|| format!("shader array {name} is unbalanced"))?;
    let body: String = chars[j + 1..values_end].iter().collect();
    let mut values = Vec::new();
    for piece in body.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        let value = piece.parse::<i32>().map_err(|_| format!("shader array {name} has a non-integer value"))?;
        values.push(value);
    }
    if values.len() != count as usize {
        return Err(format!("shader array {name} has {} values, expected {count}", values.len()));
    }
    let mut next = values_end + 1;
    next = skip_ws(chars, next);
    if next < chars.len() && chars[next] == ';' {
        next += 1;
    }
    Ok(Some((name, values, next)))
}

fn take_int_after(chars: &[char], mut i: usize) -> Option<(i32, usize)> {
    i = skip_ws(chars, i);
    let start = i;
    if i < chars.len() && chars[i] == '-' {
        i += 1;
    }
    let digits = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits {
        return None;
    }
    chars[start..i].iter().collect::<String>().parse().ok().map(|value| (value, i))
}

fn array_lookup_fn(name: &str, values: &[i32]) -> String {
    let mut out = format!("int {name}_at(int paperIndex) {{\n int paperValue = 0;\n");
    for (index, value) in values.iter().enumerate() {
        out.push_str(&format!(" if (paperIndex == {index}) paperValue = {value};\n"));
    }
    out.push_str(" return paperValue;\n}\n");
    out
}

fn replace_array_reads(src: &str, name: &str, callee: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if ident_at(&chars, i, name) {
            let bracket = skip_ws(&chars, i + name.len());
            if bracket < chars.len() && chars[bracket] == '[' {
                if let Some(end) = matching_bracket(&chars, bracket) {
                    let index: String = chars[bracket + 1..end].iter().collect();
                    out.push_str(&format!("{callee}({index})"));
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn matching_bracket(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut i = open;
    while i < chars.len() {
        if chars[i] == '[' {
            depth += 1;
        } else if chars[i] == ']' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

struct ArrayDecl {
    ty: String,
    name: String,
    count: i32,
    start: usize,
    end: usize,
    local: bool,
}

fn rewrite_dynamic_indexes(src: &str) -> Result<String, String> {
    let chars: Vec<char> = src.chars().collect();
    let arrays = array_decls(&chars);
    let mut src = src.to_string();
    let mut lifted = String::new();
    for array in arrays.iter().rev().filter(|array| array.local) {
        lifted.insert_str(0, &src[array.start..array.end]);
        src.replace_range(array.start..array.end, "");
    }
    let chars: Vec<char> = src.chars().collect();
    let loops = for_loops(&chars);
    let mut helpers = String::new();
    let mut used = Vec::new();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some(array) = arrays.iter().find(|array| ident_at(&chars, i, &array.name)) {
            let bracket = skip_ws(&chars, i + array.name.len());
            if bracket < chars.len() && chars[bracket] == '[' {
                if let Some(end) = matching_bracket(&chars, bracket) {
                    let index: String = chars[bracket + 1..end].iter().collect();
                    let after = skip_ws(&chars, end + 1);
                    let assignment = after < chars.len() && chars[after] == '=' && (after + 1 == chars.len() || chars[after + 1] != '=');
                    if assignment || index_is_loop_safe(&index, loop_var_at(&loops, bracket)) {
                        out.push_str(&array.name);
                        i += array.name.len();
                        continue;
                    }
                    if !used.contains(&array.name) {
                        helpers.push_str(&index_helper(array));
                        used.push(array.name.clone());
                    }
                    out.push_str(&format!("paperLoad_{}({index})", array.name));
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    if !lifted.is_empty() || !helpers.is_empty() {
        if let Some(main_at) = find_void_main(&out) {
            out.insert_str(main_at, &format!("{lifted}{helpers}"));
        } else {
            out.insert_str(0, &format!("{lifted}{helpers}"));
        }
    }
    Ok(out)
}

fn array_decls(chars: &[char]) -> Vec<ArrayDecl> {
    let mut decls = Vec::new();
    let mut depth = 0;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            depth += 1;
            i += 1;
            continue;
        }
        if chars[i] == '}' {
            depth -= 1;
            i += 1;
            continue;
        }
        if is_ident_start(chars[i]) {
            let mut j = i;
            if ident_at(chars, j, "const") {
                i += 1;
                continue;
            }
            let uniform = ident_at(chars, j, "uniform");
            if uniform {
                j = skip_ws(chars, j + "uniform".len());
            }
            if j < chars.len() && is_ident_start(chars[j]) {
                let type_at = j;
                j += 1;
                while j < chars.len() && is_ident(chars[j]) {
                    j += 1;
                }
                let ty: String = chars[type_at..j].iter().collect();
                j = skip_ws(chars, j);
                if j < chars.len() && is_ident_start(chars[j]) {
                    let name_at = j;
                    j += 1;
                    while j < chars.len() && is_ident(chars[j]) {
                        j += 1;
                    }
                    let name: String = chars[name_at..j].iter().collect();
                    j = skip_ws(chars, j);
                    if j < chars.len() && chars[j] == '[' {
                        if let Some((count, after)) = take_int_after(chars, j + 1) {
                            let mut k = skip_ws(chars, after);
                            if k < chars.len() && chars[k] == ']' {
                                k = skip_ws(chars, k + 1);
                                if k < chars.len() && chars[k] == ';' {
                                    decls.push(ArrayDecl {
                                        ty,
                                        name,
                                        count,
                                        start: i,
                                        end: k + 1,
                                        local: depth > 0 && !uniform,
                                    });
                                    i = k + 1;
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
    decls
}

struct ForLoop {
    var: String,
    body_start: usize,
    body_end: usize,
}

fn for_loops(chars: &[char]) -> Vec<ForLoop> {
    let mut loops = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if ident_at(chars, i, "for") {
            let paren = skip_ws(chars, i + "for".len());
            if paren < chars.len() && chars[paren] == '(' {
                if let Some(header_end) = matching_paren(chars, paren) {
                    let header: String = chars[paren + 1..header_end].iter().collect();
                    if let Some(var) = for_var(&header) {
                        let brace = skip_ws(chars, header_end + 1);
                        if brace < chars.len() && chars[brace] == '{' {
                            if let Some(body_end) = matching_brace(chars, brace) {
                                loops.push(ForLoop {
                                    var,
                                    body_start: brace,
                                    body_end,
                                });
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
    loops
}

fn for_var(header: &str) -> Option<String> {
    let mut words = header.split(|ch: char| ch == ';' || ch.is_whitespace()).filter(|word| !word.is_empty());
    let first = words.next()?;
    if first == "int" {
        words.next().map(str::to_string)
    } else {
        None
    }
}

fn loop_var_at(loops: &[ForLoop], pos: usize) -> Option<&str> {
    loops
        .iter()
        .filter(|loop_| loop_.body_start < pos && pos < loop_.body_end)
        .max_by_key(|loop_| loop_.body_start)
        .map(|loop_| loop_.var.as_str())
}

fn index_is_loop_safe(index: &str, loop_var: Option<&str>) -> bool {
    let index = index.trim();
    if is_int_literal(index) {
        return true;
    }
    let Some(var) = loop_var else {
        return false;
    };
    if index == var {
        return true;
    }
    let Some(rest) = index.strip_prefix(var) else {
        return false;
    };
    let rest = rest.trim();
    (rest.starts_with('+') || rest.starts_with('-')) && is_int_literal(rest[1..].trim())
}

fn is_int_literal(text: &str) -> bool {
    let mut chars = text.chars();
    if let Some(first) = chars.next() {
        if first == '-' {
            return text.len() > 1 && text[1..].chars().all(|ch| ch.is_ascii_digit());
        }
        return text.chars().all(|ch| ch.is_ascii_digit()) && !text.is_empty();
    }
    false
}

fn index_helper(array: &ArrayDecl) -> String {
    let zero = if array.ty.starts_with("int") || array.ty == "bool" {
        format!("{}(0)", array.ty)
    } else if array.ty == "float" {
        "0.0".to_string()
    } else {
        format!("{}(0.0)", array.ty)
    };
    format!(
        "{ty} paperLoad_{name}(int paperIndex) {{\n {ty} paperValue = {zero};\n for (int paperSlot = 0; paperSlot < {count}; paperSlot++) {{ if (paperSlot == paperIndex) paperValue = {name}[paperSlot]; }}\n return paperValue;\n}}\n",
        ty = array.ty,
        name = array.name,
        count = array.count
    )
}

fn find_void_main(src: &str) -> Option<usize> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if ident_at(&chars, i, "void") {
            let j = skip_ws(&chars, i + "void".len());
            if ident_at(&chars, j, "main") {
                return Some(chars[..i].iter().collect::<String>().len());
            }
        }
        i += 1;
    }
    None
}

fn rewrite_integer_mod(src: &str) -> Result<String, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && (i + 1 == chars.len() || chars[i + 1] != '=') {
            let left = pop_multiplicative(&mut out).ok_or("shader % is missing a left operand")?;
            i = skip_ws(&chars, i + 1);
            let (right, next) = take_unary(&chars, i).ok_or("shader % is missing a right operand")?;
            out.push_str(&format!("paperIMod({left}, {right})"));
            i = next;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    Ok(out)
}

fn pop_multiplicative(out: &mut String) -> Option<String> {
    let mut expr = pop_unary(out)?;
    loop {
        trim_end_ws(out);
        let Some(op) = out.chars().next_back() else {
            break;
        };
        if op != '*' && op != '/' {
            break;
        }
        out.pop();
        trim_end_ws(out);
        let left = pop_unary(out)?;
        expr = format!("{left} {op} {expr}");
    }
    Some(expr)
}

fn pop_unary(out: &mut String) -> Option<String> {
    trim_end_ws(out);
    let mut expr = pop_primary(out)?;
    trim_end_ws(out);
    if let Some(op) = out.chars().next_back() {
        if op == '+' || op == '-' || op == '!' {
            let binary = out.chars().rev().nth(1).is_some_and(|ch| is_ident(ch) || ch == ')' || ch == ']');
            if !binary {
                out.pop();
                expr = format!("{op}{expr}");
            }
        }
    }
    Some(expr)
}

fn pop_primary(out: &mut String) -> Option<String> {
    trim_end_ws(out);
    let end = out.chars().next_back()?;
    if end == ')' || end == ']' {
        let open = if end == ')' { '(' } else { '[' };
        let mut wrapped = pop_balanced(out, open, end)?;
        trim_end_ws(out);
        if end == ')' {
            if let Some(name) = pop_ident(out) {
                wrapped.insert_str(0, &name);
            }
        } else if let Some(name) = pop_primary(out) {
            wrapped.insert_str(0, &name);
        }
        return Some(wrapped);
    }
    if end.is_ascii_digit() || end == '.' {
        return pop_number(out);
    }
    pop_ident(out)
}

fn pop_balanced(out: &mut String, open: char, close: char) -> Option<String> {
    let mut depth = 0;
    let mut got = Vec::new();
    loop {
        let ch = out.pop()?;
        got.push(ch);
        if ch == close {
            depth += 1;
        } else if ch == open {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
    }
    got.reverse();
    Some(got.into_iter().collect())
}

fn pop_ident(out: &mut String) -> Option<String> {
    trim_end_ws(out);
    let mut got = Vec::new();
    while out.chars().next_back().is_some_and(is_ident) {
        got.push(out.pop()?);
    }
    if got.is_empty() || !is_ident_start(*got.last()?) {
        for ch in got.into_iter().rev() {
            out.push(ch);
        }
        return None;
    }
    got.reverse();
    Some(got.into_iter().collect())
}

fn pop_number(out: &mut String) -> Option<String> {
    let mut got = Vec::new();
    while out.chars().next_back().is_some_and(|ch| ch.is_ascii_digit() || ch == '.' || ch == 'e' || ch == 'E' || ch == '+' || ch == '-') {
        let ch = out.pop()?;
        if (ch == '+' || ch == '-') && !got.last().is_some_and(|prev| *prev == 'e' || *prev == 'E') {
            out.push(ch);
            break;
        }
        got.push(ch);
    }
    if got.is_empty() {
        return None;
    }
    got.reverse();
    Some(got.into_iter().collect())
}

fn trim_end_ws(out: &mut String) {
    while out.chars().next_back().is_some_and(|ch| ch.is_whitespace()) {
        out.pop();
    }
}

fn take_unary(chars: &[char], mut i: usize) -> Option<(String, usize)> {
    i = skip_ws(chars, i);
    let mut prefix = String::new();
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-' || chars[i] == '!') {
        let binary = i > 0 && (is_ident(chars[i - 1]) || chars[i - 1] == ')' || chars[i - 1] == ']');
        if !binary {
            prefix.push(chars[i]);
            i += 1;
            i = skip_ws(chars, i);
        }
    }
    let (primary, next) = take_primary(chars, i)?;
    Some((format!("{prefix}{primary}"), next))
}

fn take_primary(chars: &[char], i: usize) -> Option<(String, usize)> {
    if i >= chars.len() {
        return None;
    }
    if chars[i] == '(' {
        let end = matching_paren(chars, i)?;
        let text: String = chars[i..=end].iter().collect();
        return Some((text, end + 1));
    }
    if is_ident_start(chars[i]) {
        let mut j = i + 1;
        while j < chars.len() && is_ident(chars[j]) {
            j += 1;
        }
        let mut text: String = chars[i..j].iter().collect();
        let paren = skip_ws(chars, j);
        if paren < chars.len() && chars[paren] == '(' {
            let end = matching_paren(chars, paren)?;
            text.push_str(&chars[j..=end].iter().collect::<String>());
            return Some((text, end + 1));
        }
        return Some((text, j));
    }
    if chars[i].is_ascii_digit() || chars[i] == '.' {
        let mut j = i + 1;
        while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.' || chars[j] == 'e' || chars[j] == 'E') {
            j += 1;
        }
        if j < chars.len() && (chars[j] == '+' || chars[j] == '-') && (chars[j - 1] == 'e' || chars[j - 1] == 'E') {
            j += 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
        }
        return Some((chars[i..j].iter().collect(), j));
    }
    None
}

fn strip_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                if chars[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(chars.len());
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn strip_headers(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' {
            let rest: String = chars[i..chars.len().min(i + 16)].iter().collect();
            if rest.starts_with("#version") {
                i += "#version".len();
                i = skip_ws(&chars, i);
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                i = skip_ws(&chars, i);
                if ident_at(&chars, i, "es") || ident_at(&chars, i, "core") {
                    while i < chars.len() && is_ident(chars[i]) {
                        i += 1;
                    }
                }
                continue;
            }
            if rest.starts_with("#extension") {
                while i < chars.len() && chars[i] != '\n' && chars[i] != ';' {
                    i += 1;
                }
                continue;
            }
        }
        if ident_at(&chars, i, "precision") {
            i += "precision".len();
            while i < chars.len() && chars[i] != ';' {
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            continue;
        }
        if ident_at(&chars, i, "layout") {
            i += "layout".len();
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '(' {
                let mut depth = 0;
                while i < chars.len() {
                    if chars[i] == '(' {
                        depth += 1;
                    } else if chars[i] == ')' {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    i += 1;
                }
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn ident_at(chars: &[char], i: usize, name: &str) -> bool {
    let target: Vec<char> = name.chars().collect();
    if i + target.len() > chars.len() || chars[i..i + target.len()] != target[..] {
        return false;
    }
    (i == 0 || !is_ident(chars[i - 1]))
        && (i + target.len() == chars.len() || !is_ident(chars[i + target.len()]))
}

fn replace_idents(src: &str, map: &[(&str, &str)]) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if is_ident_start(chars[i]) {
            let start = i;
            i += 1;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match map.iter().find(|(key, _)| *key == word) {
                Some((_, replacement)) => out.push_str(replacement),
                None => out.push_str(&word),
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn strip_declarations(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if ident_at(&chars, i, "uniform") {
            if let Some(end) = uniform_to_skip(&chars, i) {
                i = end;
                continue;
            }
        }
        if ident_at(&chars, i, "in") {
            if let Some(end) = varying_to_skip(&chars, i) {
                i = end;
                continue;
            }
        }
        if ident_at(&chars, i, "out") {
            if let Some(end) = frag_out_to_skip(&chars, i) {
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn uniform_to_skip(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start + "uniform".len();
    i = skip_ws(chars, i);
    if !(i < chars.len() && is_ident_start(chars[i])) {
        return None;
    }
    i += 1;
    while i < chars.len() && is_ident(chars[i]) {
        i += 1;
    }
    i = skip_ws(chars, i);
    let name_at = i;
    if !(i < chars.len() && is_ident_start(chars[i])) {
        return None;
    }
    i += 1;
    while i < chars.len() && is_ident(chars[i]) {
        i += 1;
    }
    let name: String = chars[name_at..i].iter().collect();
    if !PRELUDE_UNIFORMS.contains(&name.as_str()) {
        return None;
    }
    i = skip_ws(chars, i);
    if i < chars.len() && chars[i] == ';' {
        Some(i + 1)
    } else {
        None
    }
}

fn varying_to_skip(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start + "in".len();
    i = skip_ws(chars, i);
    if ident_at(chars, i, "float2") || ident_at(chars, i, "float3") || ident_at(chars, i, "float4") {
        while i < chars.len() && is_ident(chars[i]) {
            i += 1;
        }
        i = skip_ws(chars, i);
        let name_at = i;
        if !(i < chars.len() && chars[i] == 'v') {
            return None;
        }
        while i < chars.len() && is_ident(chars[i]) {
            i += 1;
        }
        if !chars[name_at..i].starts_with(&['v', '_']) {
            return None;
        }
        i = skip_ws(chars, i);
        if i < chars.len() && chars[i] == ';' {
            return Some(i + 1);
        }
    }
    None
}

fn frag_out_to_skip(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start + "out".len();
    i = skip_ws(chars, i);
    if !ident_at(chars, i, "float4") {
        return None;
    }
    while i < chars.len() && is_ident(chars[i]) {
        i += 1;
    }
    i = skip_ws(chars, i);
    if !ident_at(chars, i, "fragColor") {
        return None;
    }
    i += "fragColor".len();
    i = skip_ws(chars, i);
    if i < chars.len() && chars[i] == ';' {
        Some(i + 1)
    } else {
        None
    }
}

fn declared_ints(chars: &[char]) -> Vec<String> {
    let mut names = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if ident_at(chars, i, "int") {
            let mut j = skip_ws(chars, i + "int".len());
            if j < chars.len() && is_ident_start(chars[j]) {
                let start = j;
                j += 1;
                while j < chars.len() && is_ident(chars[j]) {
                    j += 1;
                }
                names.push(chars[start..j].iter().collect());
            }
        }
        i += 1;
    }
    names
}

fn replace_builtin_calls(src: &str) -> Result<String, String> {
    let chars: Vec<char> = src.chars().collect();
    let int_names = declared_ints(&chars);
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some(name) = call_name(
            &chars,
            i,
            &[
                "fwidth",
                "dFdx",
                "dFdy",
                "textureSize",
                "textureLod",
                "textureGrad",
                "texture",
                "transpose",
                "round",
                "clamp",
            ],
        ) {
            let after = i + name.len();
            let paren = skip_ws(chars.as_slice(), after);
            if paren < chars.len() && chars[paren] == '(' {
                let args = split_args(&chars, paren)?;
                let replacement = match name {
                    "fwidth" | "dFdx" | "dFdy" | "round" | "transpose" => {
                        if args.len() != 1 {
                            return Err(format!("{name} expects one argument"));
                        }
                        let callee = match name {
                            "round" => "paperRound",
                            "transpose" => "paperTranspose",
                            _ => "paperFwidth",
                        };
                        format!("{callee}({})", args[0])
                    }
                    "texture" | "textureLod" | "textureGrad" => {
                        if args.len() < 2 {
                            return Err(format!("{name} expects a sampler and coordinates"));
                        }
                        let sampler = args[0].trim();
                        if !sampler.chars().all(is_ident) || sampler.is_empty() {
                            return Err(format!("{name} sampler must be a uniform name"));
                        }
                        format!("{sampler}.eval({})", args[1])
                    }
                    "textureSize" => {
                        if args.len() != 2 {
                            return Err("textureSize expects a sampler and a lod".into());
                        }
                        let sampler = args[0].trim();
                        if !sampler.chars().all(is_ident) || sampler.is_empty() {
                            return Err("textureSize sampler must be a name".into());
                        }
                        format!("paperTexSize_{sampler}")
                    }
                    "clamp" => {
                        if args.len() != 3 {
                            return Err("clamp expects three arguments".into());
                        }
                        let value = args[0].trim();
                        let lo = args[1].trim();
                        let hi = args[2].trim();
                        if int_names.iter().any(|name| name == value) && is_int_literal(lo) && is_int_literal(hi) {
                            format!("paperClampInt({value}, {lo}, {hi})")
                        } else {
                            format!("clamp({value}, {lo}, {hi})")
                        }
                    }
                    _ => unreachable!(),
                };
                out.push_str(&replacement);
                i = end_of_call(&chars, paren);
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    Ok(out)
}

fn call_name<'a>(chars: &[char], i: usize, names: &[&'a str]) -> Option<&'a str> {
    for name in names {
        if ident_at(chars, i, name) {
            return Some(*name);
        }
    }
    None
}

fn split_args(chars: &[char], paren: usize) -> Result<Vec<String>, String> {
    let mut depth = 0;
    let mut args = Vec::new();
    let mut current = String::new();
    let mut i = paren;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '(' {
            depth += 1;
            if depth > 1 {
                current.push(ch);
            }
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 {
                args.push(current);
                return Ok(args);
            }
            current.push(ch);
        } else if ch == ',' && depth == 1 {
            args.push(current);
            current = String::new();
        } else {
            current.push(ch);
        }
        i += 1;
    }
    Err("unbalanced shader call".into())
}

fn end_of_call(chars: &[char], paren: usize) -> usize {
    let mut depth = 0;
    let mut i = paren;
    while i < chars.len() {
        if chars[i] == '(' {
            depth += 1;
        } else if chars[i] == ')' {
            depth -= 1;
            if depth == 0 {
                return i + 1;
            }
        }
        i += 1;
    }
    chars.len()
}

fn rewrite_counted_loops(src: &str) -> Result<String, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut edits = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if ident_at(&chars, i, "for") {
            let paren = skip_ws(&chars, i + "for".len());
            if paren < chars.len() && chars[paren] == '(' {
                if let Some(header_end) = matching_paren(&chars, paren) {
                    let header: String = chars[paren + 1..header_end].iter().collect();
                    let parts = split_semis(&header);
                    if parts.len() == 3 {
                        if let Some(var) = loop_var_from_init(&parts[0]) {
                            if let Some((op, bound)) = condition_bound(&parts[1], &var) {
                                if !is_const_int_expr(bound) {
                                    if !bound.chars().all(is_ident) || bound.is_empty() {
                                        return Err(format!("shader loop bound {bound} is not a constant expression"));
                                    }
                                    if op != "<" && op != "<=" {
                                        return Err(format!("shader loop bound {bound} is not a rising constant cap"));
                                    }
                                    if !is_increment(&parts[2], &var) {
                                        return Err(format!("shader loop bound {bound} is not a rising constant cap"));
                                    }
                                    let hi = clamped_int_hi(&chars, i, bound)?;
                                    let brace = skip_ws(&chars, header_end + 1);
                                    if brace >= chars.len() || chars[brace] != '{' {
                                        return Err(format!("shader loop bound {bound} needs a braced body"));
                                    }
                                    let new_header = format!(
                                        "{}; {} {op} {hi}; {}",
                                        parts[0].trim(),
                                        var,
                                        parts[2].trim()
                                    );
                                    let guard = if op == "<" {
                                        format!(" if ({var} >= {bound}) break;")
                                    } else {
                                        format!(" if ({var} > {bound}) break;")
                                    };
                                    edits.push((paren + 1, header_end, new_header));
                                    edits.push((brace + 1, brace + 1, guard));
                                }
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
    if edits.is_empty() {
        return Ok(src.to_string());
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut src = src.to_string();
    for (start, end, text) in edits {
        src.replace_range(start..end, &text);
    }
    Ok(src)
}

fn split_semis(src: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    for ch in src.chars() {
        if ch == '(' || ch == '[' || ch == '{' {
            depth += 1;
        } else if ch == ')' || ch == ']' || ch == '}' {
            depth -= 1;
        }
        if ch == ';' && depth == 0 {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }
    parts.push(current);
    parts
}

fn loop_var_from_init(init: &str) -> Option<String> {
    let rest = init.trim().strip_prefix("int")?;
    if rest.starts_with(|ch: char| is_ident(ch)) {
        return None;
    }
    let rest = rest.trim_start();
    if !rest.starts_with(is_ident_start) {
        return None;
    }
    Some(rest.chars().take_while(|ch| is_ident(*ch)).collect())
}

fn condition_bound<'a>(cond: &'a str, var: &str) -> Option<(&'a str, &'a str)> {
    let cond = cond.trim();
    let rest = cond.strip_prefix(var)?;
    if rest.starts_with(|ch: char| is_ident(ch)) {
        return None;
    }
    let rest = rest.trim_start();
    let op = if rest.starts_with("<=") {
        "<="
    } else if rest.starts_with(">=") {
        ">="
    } else if rest.starts_with('<') {
        "<"
    } else if rest.starts_with('>') {
        ">"
    } else {
        return None;
    };
    let bound = rest[op.len()..].trim();
    if bound.is_empty() || bound.contains('&') || bound.contains('|') {
        None
    } else {
        Some((op, bound))
    }
}

fn is_const_int_expr(text: &str) -> bool {
    let text = text.trim();
    if is_int_literal(text) {
        return true;
    }
    for op in ['+', '-'] {
        if let Some((left, right)) = text.split_once(op) {
            if is_int_literal(left.trim()) && is_int_literal(right.trim()) && !left.trim().is_empty() {
                return true;
            }
        }
    }
    false
}

fn is_increment(step: &str, var: &str) -> bool {
    let step: String = step.chars().filter(|ch| !ch.is_whitespace()).collect();
    step == format!("{var}++") || step == format!("++{var}")
}

fn clamped_int_hi(chars: &[char], end: usize, name: &str) -> Result<i32, String> {
    let mut i = 0;
    let mut found: Option<Result<i32, ()>> = None;
    while i < end {
        if ident_at(chars, i, name) {
            let mut j = skip_ws(chars, i + name.len());
            if j < end {
                let compound = matches!(chars[j], '+' | '-' | '*' | '/' | '%') && chars.get(j + 1) == Some(&'=');
                let plain = chars[j] == '=' && chars.get(j + 1) != Some(&'=');
                if compound || plain {
                    if chars[j] == '=' {
                        j = skip_ws(chars, j + 1);
                        if ident_at(chars, j, "paperClampInt") {
                            let paren = skip_ws(chars, j + "paperClampInt".len());
                            if paren < chars.len() && chars[paren] == '(' {
                                let args = split_args(chars, paren)?;
                                if args.len() == 3 {
                                    if let Ok(hi) = args[2].trim().parse::<i32>() {
                                        found = Some(Ok(hi));
                                        i += 1;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    found = Some(Err(()));
                }
            }
        }
        i += 1;
    }
    match found {
        Some(Ok(hi)) => Ok(hi),
        _ => Err(format!("shader loop bound {name} is not clamped to a constant")),
    }
}

fn rewrite_main(src: &str) -> Result<String, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if ident_at(&chars, i, "void") {
            let mut j = skip_ws(&chars, i + "void".len());
            if ident_at(&chars, j, "main") {
                j = skip_ws(&chars, j + "main".len());
                if j < chars.len() && chars[j] == '(' {
                    let mut k = j;
                    while k < chars.len() && chars[k] != '{' {
                        k += 1;
                    }
                    if k >= chars.len() {
                        return Err("shader main has no body".into());
                    }
                    let body_end = matching_brace(&chars, k).ok_or("shader main body is unbalanced")?;
                    let body: String = chars[k + 1..body_end].iter().collect();
                    let mut out: String = chars[..i].iter().collect();
                    out.push_str("half4 main(float2 paperCoord) {\n paperPrepare(paperCoord);\n");
                    out.push_str(&body);
                    out.push_str("\n return half4(fragColor.r, fragColor.g, fragColor.b, fragColor.a);\n}\n");
                    out.extend(chars[body_end + 1..].iter());
                    return Ok(out);
                }
            }
        }
        i += 1;
    }
    Err("fragment shader has no main".into())
}

fn matching_brace(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut i = open;
    while i < chars.len() {
        if chars[i] == '{' {
            depth += 1;
        } else if chars[i] == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn skip_ws(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

impl PaperPaint {
    /// The scene document form of this shader, with each image embedded as raw RGBA8.
    pub(crate) fn to_doc(&self, cx: &mut director_core::scene_export::ExportContext) -> director_scene::html::PaperShader {
        director_scene::html::PaperShader {
            sksl: self.sksl.clone(),
            uniforms: self.uniforms.clone(),
            images: self
                .images
                .iter()
                .map(|(name, image)| {
                    (
                        name.clone(),
                        director_scene::html::PaperImage {
                            width: image.width,
                            height: image.height,
                            asset: cx.embedded_asset("paper", "application/x-rgba8", &image.rgba),
                            flip_y: image.flip_y,
                        },
                    )
                })
                .collect(),
        }
    }
}
