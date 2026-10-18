use director_core::systems::renderer::render_frame;
use director_html::{
    compile_capture, Capture, CapturedFrame, CapturedNode, CapturedShader, CapturedShaderImage,
    CAPTURE_FORMAT,
};
use std::collections::BTreeMap;

fn render(fragment: &str, uniforms: BTreeMap<String, Vec<f32>>, images: BTreeMap<String, CapturedShaderImage>) -> Vec<u8> {
    let root = CapturedNode {
        tag: "canvas".into(),
        id: "shader".into(),
        rect: [0.0, 0.0, 64.0, 64.0],
        styles: BTreeMap::new(),
        text: None,
        svg: None,
        src: None,
        shader: Some(CapturedShader {
            fragment: fragment.into(),
            uniforms,
            images,
        }),
        media: None,
        children: Vec::new(),
    };
    let capture = Capture {
        format: CAPTURE_FORMAT.into(),
        width: 64,
        height: 64,
        fps: 1.0,
        duration: 1.0,
        sample_rate: 1.0,
        fonts: Vec::new(),
        assets: BTreeMap::new(),
        frames: vec![CapturedFrame { time: 0.0, root }],
    };
    let mut compiled = compile_capture(capture, std::path::Path::new(".")).unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut compiled.director, 0.0, surface.canvas()).unwrap();
    let info = skia_safe::ImageInfo::new(
        (64, 64),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut pixels = vec![0u8; 64 * 64 * 4];
    assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (0, 0)));
    pixels
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
    let i = (y * 64 + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

#[test]
fn paper_shader_paints_a_uniform_color() {
    let pixels = render(
        "#version 300 es
         precision mediump float;
         uniform vec4 u_color;
         in vec2 v_objectUV;
         out vec4 fragColor;
         void main() { fragColor = u_color; }",
        BTreeMap::from([("u_color".into(), vec![1.0, 0.0, 0.0, 1.0])]),
        BTreeMap::new(),
    );
    assert_eq!(pixel(&pixels, 32, 32), [255, 0, 0, 255]);
}

#[test]
fn paper_shader_object_uv_uses_top_left_origin() {
    let pixels = render(
        "precision mediump float;
         in vec2 v_objectUV;
         out vec4 fragColor;
         void main() { fragColor = vec4(v_objectUV.x + 0.5, v_objectUV.y + 0.5, 0.0, 1.0); }",
        BTreeMap::new(),
        BTreeMap::new(),
    );
    assert!(pixel(&pixels, 0, 32)[0] < 40, "{:?}", pixel(&pixels, 0, 32));
    assert!(pixel(&pixels, 63, 32)[0] > 210, "{:?}", pixel(&pixels, 63, 32));
    assert!(pixel(&pixels, 32, 0)[1] > 210, "{:?}", pixel(&pixels, 32, 0));
    assert!(pixel(&pixels, 32, 63)[1] < 40, "{:?}", pixel(&pixels, 32, 63));
}

#[test]
fn paper_shader_indexes_color_array() {
    let mut colors = vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    assert_eq!(colors.len(), 16);
    let pixels = render(
        "precision mediump float;
         uniform vec4 u_colors[4];
         uniform float u_colorsCount;
         out vec4 fragColor;
         void main() {
           vec4 c = u_colors[0];
           for (int i = 1; i < 4; i++) {
             if (float(i) >= u_colorsCount) break;
             c = u_colors[i];
           }
           fragColor = c;
         }",
        BTreeMap::from([
            ("u_colors".into(), colors.split_off(0)),
            ("u_colorsCount".into(), vec![2.0]),
        ]),
        BTreeMap::new(),
    );
    assert_eq!(pixel(&pixels, 8, 8), [0, 255, 0, 255]);
}

#[test]
fn paper_shader_samples_child_image() {
    let pixels = render(
        "precision mediump float;
         uniform sampler2D u_image;
         out vec4 fragColor;
         void main() { fragColor = texture(u_image, vec2(0.5, 0.5)); }",
        BTreeMap::new(),
        BTreeMap::from([(
            "u_image".into(),
            CapturedShaderImage {
                width: 2,
                height: 2,
                rgba: base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    [0u8, 0, 255, 255].repeat(4),
                ),
                flip_y: false,
            },
        )]),
    );
    assert_eq!(pixel(&pixels, 16, 16), [0, 0, 255, 255]);
}

#[test]
fn paper_shader_approximates_fwidth() {
    let pixels = render(
        "precision mediump float;
         in vec2 v_objectUV;
         out vec4 fragColor;
         void main() { float aa = fwidth(v_objectUV.x); fragColor = vec4(aa * 40.0, 0.0, 0.0, 1.0); }",
        BTreeMap::new(),
        BTreeMap::new(),
    );
    assert_eq!(pixel(&pixels, 8, 8)[3], 255);
    assert!(pixel(&pixels, 8, 8)[0] > 0);
}

#[test]
fn paper_shader_rewrites_sksl_gaps() {
    let pixels = render(
        "precision mediump float;
         uniform mediump float u_scale;
         uniform bool u_edges;
         uniform vec4 u_colors[4];
         uniform float u_colorsCount;
         out vec4 fragColor;
         const int bayer[4] = int[4](0, 2, 3, 1);
         void main() {
           float4 local[4];
           for (int i = 0; i < 4; i++) local[i] = u_colors[i];
           float4 picked = local[int(u_colorsCount - 1.0)];
           mat2 spin = mat2(0.0, 1.0, 1.0, 0.0);
           mat2 turned = transpose(spin);
           float band = float(5 % 3) / 2.0;
           float stepped = round(0.6);
           float gate = u_edges == true ? u_scale : 0.0;
           float table = float(bayer[1]) / 2.0;
           fragColor = vec4(picked.g * gate * stepped * band * table * turned[0][1], 0.0, 0.0, 1.0);
         }",
        BTreeMap::from([
            ("u_scale".into(), vec![1.0]),
            ("u_edges".into(), vec![1.0]),
            ("u_colors".into(), vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
            ("u_colorsCount".into(), vec![2.0]),
        ]),
        BTreeMap::new(),
    );
    assert_eq!(pixel(&pixels, 8, 8), [255, 0, 0, 255], "{:?}", pixel(&pixels, 8, 8));
}

#[test]
fn paper_shader_clamps_an_octave_loop() {
    let pixels = render(
        "precision mediump float;
         out vec4 fragColor;
         void main() {
           int steps = 1;
           steps = clamp(steps, 1, 4);
           int seen = 0;
           for (int i = 0; i < steps; i++) { seen += 1; }
           float kept = clamp(0.25, 0.0, 1.0);
           fragColor = vec4(float(seen) / 4.0, kept, 0.0, 1.0);
         }",
        BTreeMap::new(),
        BTreeMap::new(),
    );
    let color = pixel(&pixels, 8, 8);
    assert!(color[0] > 32 && color[0] < 96, "{color:?}");
    assert!(color[1] > 32 && color[1] < 96, "{color:?}");
    assert_eq!(color[2], 0);
    assert_eq!(color[3], 255);
}

#[test]
fn paper_shader_uses_captured_texture_size() {
    let pixels = render(
        "precision mediump float;
         uniform sampler2D u_image;
         out vec4 fragColor;
         float edge(sampler2D tex) { return textureSize(tex, 0).x / 2.0; }
         void main() { fragColor = vec4(edge(u_image), 0.0, 0.0, 1.0); }",
        BTreeMap::new(),
        BTreeMap::from([(
            "u_image".into(),
            CapturedShaderImage {
                width: 2,
                height: 2,
                rgba: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0u8, 0, 255, 255].repeat(4)),
                flip_y: false,
            },
        )]),
    );
    assert_eq!(pixel(&pixels, 4, 4), [255, 0, 0, 255], "{:?}", pixel(&pixels, 4, 4));
}

#[test]
fn paper_shader_expands_object_defines() {
    let pixels = render(
        "precision mediump float;
         out vec4 fragColor;
         #define TWO_PI 6.28318530718
         #define PI 3.141592653589793
         void main() { fragColor = vec4(TWO_PI / (2.0 * PI), 0.0, 0.0, 1.0); }",
        BTreeMap::new(),
        BTreeMap::new(),
    );
    assert_eq!(pixel(&pixels, 8, 8), [255, 0, 0, 255]);
}

#[test]
fn canvas_without_a_shader_stays_refused() {
    let root = CapturedNode {
        tag: "canvas".into(),
        id: "empty".into(),
        rect: [0.0, 0.0, 8.0, 8.0],
        styles: BTreeMap::new(),
        text: None,
        svg: None,
        src: None,
        shader: None,
        media: None,
        children: Vec::new(),
    };
    let error = match compile_capture(
        Capture {
            format: CAPTURE_FORMAT.into(),
            width: 8,
            height: 8,
            fps: 1.0,
            duration: 1.0,
            sample_rate: 1.0,
            fonts: Vec::new(),
            assets: BTreeMap::new(),
            frames: vec![CapturedFrame { time: 0.0, root }],
        },
        std::path::Path::new("."),
    ) {
        Err(error) => error,
        Ok(_) => panic!("canvas without a shader compiled"),
    };
    let message = error.to_string();
    assert!(message.contains("canvas"), "{message}");
}
