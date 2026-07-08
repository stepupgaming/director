use anyhow::{Context, Result};
use director_core::animation::{Animated, EasingType, SpringConfig as CoreSpringConfig};
use director_core::audio::load_audio_bytes;
use director_core::element::TextSpan; // Added indirectly if needed, but keeping existing imports clean
use director_core::node::video_node::VideoSource;
use director_core::node::{
    effect::ShaderUniform, parse_srt, BoxNode, CaptionNode, CompositionNode, EffectNode,
    EffectType, ImageNode, LottieNode, TextNode, VectorNode, VideoNode,
};
use director_core::scene::AudioBinding;
use director_core::systems::transitions::{Transition, TransitionType as CoreTransitionType};
use director_core::types::{Color, NodeId, ObjectFit};
use director_core::video_wrapper::RenderMode;
use director_core::{AssetLoader, Director, Element};
use director_schema::{
    Animation, EffectConfig, MovieRequest, Node, NodeKind, StyleMap, TransformMap, TransitionType,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use taffy::geometry::{Line, Rect, Size};
use taffy::prelude::*;
use taffy::style::{
    AlignItems, AlignSelf, Dimension, Display, FlexDirection, FlexWrap, GridPlacement,
    GridTemplateComponent, JustifyContent, LengthPercentage, LengthPercentageAuto, Position, Style,
};

/// Converts a Schema Request into a runnable Director instance.
pub fn load_movie(request: MovieRequest, loader: Arc<dyn AssetLoader>) -> Result<Director> {
    let wrapped_loader: Arc<dyn AssetLoader> = Arc::new(PipelineAssetLoader {
        inner: loader,
        search_roots: request.asset_search_paths.clone(),
        default_font: request.default_font.clone(),
    });

    let mut director = Director::new(
        request.width as i32,
        request.height as i32,
        request.fps,
        wrapped_loader,
        RenderMode::Export,
        None,
    );

    // 0. Load Global Audio Tracks
    let mut track_id_map: HashMap<String, usize> = HashMap::new();
    for track in &request.audio_tracks {
        // Load bytes
        let bytes = director
            .assets
            .loader
            .load_bytes(&track.src)
            .with_context(|| format!("Failed to load audio track: {}", track.src))?;

        // Decode to samples
        let samples = load_audio_bytes(&bytes, director.audio_mixer.sample_rate)
            .with_context(|| format!("Failed to decode audio track: {}", track.src))?;

        // Add to mixer
        let idx = director.add_global_audio(samples);

        // Update track properties
        if let Some(core_track) = director.audio_mixer.get_track_mut(idx) {
            core_track.start_time = track.start_time;
            core_track.loop_audio = track.loop_audio;
            core_track.volume = Animated::new(track.volume);
        }

        track_id_map.insert(track.id.clone(), idx);
    }

    // Build transition list from scene configs
    let mut scene_end_times = Vec::new();
    let mut cumulative_time = 0.0;

    for scene_data in &request.scenes {
        // Build the scene graph for this scene
        let root_id = build_node_recursive(&mut director, &scene_data.root, &track_id_map)
            .with_context(|| format!("Failed to build scene graph for scene: {}", scene_data.id))?;

        // Calculate start time based on previous scenes
        let start_time = cumulative_time;
        let plays = scene_data.loop_count.unwrap_or(1).max(1) as usize;
        let hold_extra = scene_data
            .post_hold
            .as_ref()
            .map(|h| h.duration)
            .unwrap_or(0.0);
        let loop_span = if plays > 1 {
            scene_data.duration_secs + (plays as f64) * scene_data.duration_secs
        } else {
            scene_data.duration_secs
        };
        cumulative_time += loop_span + hold_extra;
        scene_end_times.push(start_time + loop_span + hold_extra);

        // Primary timeline item
        director
            .timeline
            .push(director_core::director::TimelineItem {
                scene_root: root_id,
                name: scene_data.name.clone(),
                start_time,
                duration: scene_data.duration_secs,
                z_index: scene_data.z_index,
                audio_tracks: vec![],
            });

        // Rhai `add_loop` — duplicate plays on the timeline (same spacing as lifecycle.rs)
        if plays > 1 {
            let scene_duration = scene_data.duration_secs;
            let repeat_start = start_time + scene_duration;
            for i in 1..plays {
                director
                    .timeline
                    .push(director_core::director::TimelineItem {
                        scene_root: root_id,
                        name: Some(format!("loop_{i}")),
                        start_time: repeat_start + i as f64 * scene_duration,
                        duration: scene_duration,
                        z_index: scene_data.z_index,
                        audio_tracks: vec![],
                    });
            }
        }

        // Rhai `add_freeze` — hold frame after scene ends
        if let Some(hold) = &scene_data.post_hold {
            let freeze_start = start_time + loop_span;
            director
                .timeline
                .push(director_core::director::TimelineItem {
                    scene_root: root_id,
                    name: Some("freeze".to_string()),
                    start_time: freeze_start,
                    duration: hold.duration,
                    z_index: scene_data.z_index,
                    audio_tracks: vec![],
                });
            let _ = hold.freeze_at;
        }
    }

    // Wire up transitions between scenes
    for (i, scene_data) in request.scenes.iter().enumerate() {
        if let Some(trans) = &scene_data.transition {
            if i + 1 < request.scenes.len() {
                let transition_start = scene_end_times[i] - trans.duration;
                director.transitions.push(Transition {
                    from_scene_idx: i,
                    to_scene_idx: i + 1,
                    start_time: transition_start,
                    duration: trans.duration,
                    kind: convert_transition_type(&trans.kind),
                    easing: trans.easing.clone(),
                });
            }
        }
    }

    Ok(director)
}

/// Converts schema TransitionType to core TransitionType
fn convert_transition_type(kind: &TransitionType) -> CoreTransitionType {
    match kind {
        TransitionType::Fade => CoreTransitionType::Fade,
        TransitionType::SlideLeft => CoreTransitionType::SlideLeft,
        TransitionType::SlideRight => CoreTransitionType::SlideRight,
        TransitionType::WipeLeft => CoreTransitionType::WipeLeft,
        TransitionType::WipeRight => CoreTransitionType::WipeRight,
        TransitionType::CircleOpen => CoreTransitionType::CircleOpen,
        TransitionType::Wave {
            amplitude,
            frequency,
        } => CoreTransitionType::Wave {
            amplitude: *amplitude,
            frequency: *frequency,
        },
        TransitionType::Glitch { intensity } => CoreTransitionType::Glitch {
            intensity: *intensity,
        },
        TransitionType::Iris {
            start_radius,
            end_radius,
        } => CoreTransitionType::Iris {
            start_radius: *start_radius,
            end_radius: *end_radius,
        },
        TransitionType::Spiral { rotations } => CoreTransitionType::Spiral {
            rotations: *rotations,
        },
        TransitionType::ZoomIn => CoreTransitionType::ZoomIn,
        TransitionType::RadialWipe => CoreTransitionType::RadialWipe,
        TransitionType::PixelateDissolve => CoreTransitionType::PixelateDissolve,
        TransitionType::CrossZoom => CoreTransitionType::CrossZoom,
        TransitionType::Wind => CoreTransitionType::Wind,
        TransitionType::Burn => CoreTransitionType::Burn,
        TransitionType::Dissolve => CoreTransitionType::Dissolve,
        TransitionType::Flip => CoreTransitionType::Flip,
        TransitionType::ClockWipe => CoreTransitionType::ClockWipe,
        TransitionType::Cube => CoreTransitionType::Cube,
    }
}

fn build_node_recursive(
    director: &mut Director,
    node_def: &Node,
    track_id_map: &HashMap<String, usize>,
) -> Result<NodeId> {
    // 1. Create Element based on NodeKind
    let mut element: Box<dyn Element> = match &node_def.kind {
        NodeKind::Box { border_radius } => {
            let mut b = BoxNode::new();
            b.border_radius = Animated::new(*border_radius);
            Box::new(b)
        }
        NodeKind::Text {
            content,
            font_size,
            animators,
        } => {
            // TextNode needs access to the AssetManager's font system
            let fc = director.assets.font_collection.clone();

            let span = TextSpan {
                text: content.clone(),
                font_size: Some(*font_size),
                color: Some(Color::WHITE), // Default to white, overridden by style later
                ..Default::default()
            };

            let mut t = TextNode::new(vec![span], fc);

            // Apply text animators for kinetic typography
            for anim in animators {
                let easing_str = easing_to_str(&anim.easing);
                t.add_text_animator(
                    anim.start_idx,
                    anim.end_idx,
                    anim.property.clone(),
                    anim.start_val,
                    anim.target_val,
                    anim.duration,
                    easing_str,
                );
            }

            Box::new(t)
        }
        NodeKind::Image { src, object_fit } => {
            // Load bytes immediately
            let bytes = director
                .assets
                .loader
                .load_bytes(src)
                .with_context(|| format!("Failed to load image asset: {}", src))?;
            let mut img = ImageNode::new(bytes);
            if let Some(fit) = object_fit {
                img.object_fit = parse_object_fit(fit);
            }
            Box::new(img)
        }
        NodeKind::Video { src, object_fit } => {
            // Load video bytes and create VideoNode
            let bytes = director
                .assets
                .loader
                .load_bytes(src)
                .with_context(|| format!("Failed to load video asset: {}", src))?;
            let source = VideoSource::Bytes(bytes);
            let mut vid = VideoNode::new(source, RenderMode::Export);
            if let Some(fit) = object_fit {
                vid.object_fit = parse_object_fit(fit);
            }
            Box::new(vid)
        }
        NodeKind::Vector { src } => {
            // Load SVG bytes and create VectorNode
            let bytes = director
                .assets
                .loader
                .load_bytes(src)
                .with_context(|| format!("Failed to load vector asset: {}", src))?;
            Box::new(VectorNode::new(&bytes))
        }
        NodeKind::Lottie {
            src,
            speed,
            loop_animation,
        } => {
            // Load Lottie JSON and create LottieNode
            let bytes = director
                .assets
                .loader
                .load_bytes(src)
                .with_context(|| format!("Failed to load Lottie asset: {}", src))?;
            match LottieNode::new(&bytes, HashMap::new(), &director.assets) {
                Ok(mut lottie) => {
                    lottie.speed = *speed;
                    lottie.loop_anim = *loop_animation;
                    Box::new(lottie)
                }
                Err(e) => return Err(anyhow::anyhow!("Failed to parse Lottie JSON: {}", e)),
            }
        }
        NodeKind::Effect { effect_type } => {
            // Color matrix presets
            const GRAYSCALE_MATRIX: [f32; 20] = [
                0.2126, 0.7152, 0.0722, 0.0, 0.0, 0.2126, 0.7152, 0.0722, 0.0, 0.0, 0.2126, 0.7152,
                0.0722, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
            ];
            const SEPIA_MATRIX: [f32; 20] = [
                0.393, 0.769, 0.189, 0.0, 0.0, 0.349, 0.686, 0.168, 0.0, 0.0, 0.272, 0.534, 0.131,
                0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
            ];

            // Create EffectNode based on effect configuration
            let effect = match effect_type {
                EffectConfig::Blur { sigma } => EffectType::Blur(Animated::new(*sigma)),
                EffectConfig::DropShadow {
                    blur,
                    offset_x,
                    offset_y,
                    color,
                } => EffectType::DropShadow {
                    blur: Animated::new(*blur),
                    offset_x: Animated::new(*offset_x),
                    offset_y: Animated::new(*offset_y),
                    color: Animated::new(color.unwrap_or(Color::BLACK)),
                },
                EffectConfig::ColorMatrix { matrix } => EffectType::ColorMatrix(matrix.clone()),
                EffectConfig::Grayscale => EffectType::ColorMatrix(GRAYSCALE_MATRIX.to_vec()),
                EffectConfig::Sepia => EffectType::ColorMatrix(SEPIA_MATRIX.to_vec()),
                EffectConfig::Invert => EffectType::ColorMatrix(vec![
                    -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 1.0,
                    0.0, 0.0, 0.0, 1.0, 0.0,
                ]),
                EffectConfig::Contrast { value } => {
                    let t = (1.0 - value) / 2.0;
                    EffectType::ColorMatrix(vec![
                        *value, 0.0, 0.0, 0.0, t, 0.0, *value, 0.0, 0.0, t, 0.0, 0.0, *value, 0.0,
                        t, 0.0, 0.0, 0.0, 1.0, 0.0,
                    ])
                }
                EffectConfig::Brightness { value } => EffectType::ColorMatrix(vec![
                    1.0, 0.0, 0.0, 0.0, *value, 0.0, 1.0, 0.0, 0.0, *value, 0.0, 0.0, 1.0, 0.0,
                    *value, 0.0, 0.0, 0.0, 1.0, 0.0,
                ]),
                EffectConfig::Saturation { value } => {
                    let s = *value;
                    EffectType::ColorMatrix(vec![
                        0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s, 0.0, 0.0,
                        0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s, 0.0, 0.0,
                        0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s, 0.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0,
                    ])
                }
                EffectConfig::Exposure { value } => {
                    let m = (2.0_f32).powf(*value);
                    EffectType::ColorMatrix(vec![
                        m, 0.0, 0.0, 0.0, 0.0, 0.0, m, 0.0, 0.0, 0.0, 0.0, 0.0, m, 0.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0,
                    ])
                }
                EffectConfig::HueRotate { angle } => {
                    let angle_rad = angle.to_radians();
                    let cos_a = angle_rad.cos();
                    let sin_a = angle_rad.sin();
                    let inv_cos = 1.0 - cos_a;
                    let isqrt3 = 0.577350269;
                    EffectType::ColorMatrix(vec![
                        cos_a + inv_cos / 3.0,
                        inv_cos / 3.0 - sin_a * isqrt3,
                        inv_cos / 3.0 + sin_a * isqrt3,
                        0.0,
                        0.0,
                        inv_cos / 3.0 + sin_a * isqrt3,
                        cos_a + inv_cos / 3.0,
                        inv_cos / 3.0 - sin_a * isqrt3,
                        0.0,
                        0.0,
                        inv_cos / 3.0 - sin_a * isqrt3,
                        inv_cos / 3.0 + sin_a * isqrt3,
                        cos_a + inv_cos / 3.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        1.0,
                        0.0,
                    ])
                }
                EffectConfig::Tint { color, amount } => {
                    let c = color.unwrap_or(Color::WHITE);
                    let a = amount.clamp(0.0, 1.0);
                    EffectType::ColorMatrix(vec![
                        1.0 - a + a * c.r, a * c.g, a * c.b, 0.0, 0.0, a * c.r, 1.0 - a + a * c.g,
                        a * c.b, 0.0, 0.0, a * c.r, a * c.g, 1.0 - a + a * c.b, 0.0, 0.0, 0.0, 0.0,
                        0.0, 1.0, 0.0,
                    ])
                }
                EffectConfig::DirectionalBlur {
                    strength,
                    angle,
                    samples,
                } => EffectType::DirectionalBlur {
                    strength: Animated::new(*strength),
                    angle: Animated::new(*angle),
                    samples: *samples,
                },
                EffectConfig::Grain { intensity, size } => EffectType::FilmGrain {
                    intensity: Animated::new(*intensity),
                    size: Animated::new(*size),
                },
                EffectConfig::Vignette {
                    intensity,
                    radius,
                    feather,
                } => EffectType::Vignette {
                    intensity: Animated::new(*intensity),
                    radius: Animated::new(*radius),
                    feather: Animated::new(*feather),
                },
                EffectConfig::ChromaticAberration { strength } => {
                    EffectType::ChromaticAberration(Animated::new(*strength))
                }
                EffectConfig::Pixelate { block_size } => {
                    EffectType::Pixelate(Animated::new(*block_size))
                }
                EffectConfig::Sharpen { strength } => EffectType::Sharpen(Animated::new(*strength)),
                EffectConfig::Glow {
                    intensity,
                    threshold,
                } => EffectType::Glow {
                    intensity: Animated::new(*intensity),
                    threshold: Animated::new(*threshold),
                },
                EffectConfig::Warp {
                    amplitude,
                    frequency,
                    direction,
                    speed,
                } => EffectType::Warp {
                    amplitude: Animated::new(*amplitude),
                    frequency: Animated::new(*frequency),
                    direction: Animated::new(*direction),
                    speed: Animated::new(*speed),
                },
                EffectConfig::Swirl { radius, angle } => EffectType::Swirl {
                    radius: Animated::new(*radius),
                    angle: Animated::new(*angle),
                },
                EffectConfig::Ripple {
                    amplitude,
                    frequency,
                    speed,
                } => EffectType::Ripple {
                    amplitude: Animated::new(*amplitude),
                    frequency: Animated::new(*frequency),
                    speed: Animated::new(*speed),
                },
                EffectConfig::Glass { scale, intensity } => EffectType::Glass {
                    scale: Animated::new(*scale),
                    intensity: Animated::new(*intensity),
                },
                EffectConfig::Shake { intensity, speed } => EffectType::Shake {
                    intensity: Animated::new(*intensity),
                    speed: Animated::new(*speed),
                },
                EffectConfig::Duotone { shadow, highlight } => EffectType::Duotone {
                    shadow: shadow.unwrap_or(Color::new(0.05, 0.05, 0.2, 1.0)),
                    highlight: highlight.unwrap_or(Color::new(1.0, 0.8, 0.4, 1.0)),
                },
                EffectConfig::Posterize { levels } => {
                    EffectType::Posterize(Animated::new(*levels))
                }
                EffectConfig::Threshold { level } => {
                    EffectType::Threshold(Animated::new(*level))
                }
                EffectConfig::Temperature { warmth } => {
                    let w = *warmth;
                    EffectType::ColorMatrix(vec![
                        1.0 + w * 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                        1.0 - w * 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                    ])
                }
                EffectConfig::Scanlines { intensity, count } => EffectType::Scanlines {
                    intensity: Animated::new(*intensity),
                    count: Animated::new(*count),
                },
                EffectConfig::Glitch { intensity, speed } => EffectType::Glitch {
                    intensity: Animated::new(*intensity),
                    speed: Animated::new(*speed),
                },
                EffectConfig::EdgeDetect { intensity } => {
                    EffectType::EdgeDetect(Animated::new(*intensity))
                }
                EffectConfig::ZoomBlur { strength } => {
                    EffectType::ZoomBlur(Animated::new(*strength))
                }
                EffectConfig::Kaleidoscope { segments } => {
                    EffectType::Kaleidoscope(Animated::new(*segments))
                }
                EffectConfig::Thermal => EffectType::Thermal,
                EffectConfig::Liquid {
                    frequency,
                    amplitude,
                } => EffectType::Liquid {
                    frequency: Animated::new(*frequency),
                    amplitude: Animated::new(*amplitude),
                },
                EffectConfig::Acid { color, speed } => EffectType::Acid {
                    color: color.unwrap_or(Color::new(0.0, 1.0, 0.5, 1.0)),
                    speed: Animated::new(*speed),
                },
                EffectConfig::Halftone { dot_size } => {
                    EffectType::Halftone(Animated::new(*dot_size))
                }
                EffectConfig::Ascii { scale, color } => EffectType::Ascii {
                    scale: Animated::new(*scale),
                    color: color.unwrap_or(Color::WHITE),
                },
                EffectConfig::Sketch { contrast } => {
                    EffectType::Sketch(Animated::new(*contrast))
                }
                EffectConfig::Mirror => EffectType::Mirror,
                EffectConfig::TriTone {
                    shadow,
                    mid,
                    highlight,
                    midpoint,
                } => EffectType::TriTone {
                    shadow: shadow.unwrap_or(Color::new(0.05, 0.05, 0.2, 1.0)),
                    mid: mid.unwrap_or(Color::new(0.5, 0.5, 0.5, 1.0)),
                    highlight: highlight.unwrap_or(Color::new(1.0, 0.8, 0.4, 1.0)),
                    midpoint: Animated::new(*midpoint),
                },
                EffectConfig::Vhs {
                    distortion,
                    scanline,
                } => EffectType::Vhs {
                    distortion: Animated::new(*distortion),
                    scanline: Animated::new(*scanline),
                },
                EffectConfig::NeonEdge {
                    glow_color,
                    threshold,
                } => EffectType::NeonEdge {
                    glow_color: glow_color.unwrap_or(Color::new(0.0, 1.0, 1.0, 1.0)),
                    threshold: Animated::new(*threshold),
                },
                EffectConfig::SkslShader { code, uniforms } => {
                    let mut uniform_map = HashMap::new();
                    for (k, v) in uniforms {
                        uniform_map.insert(
                            k.clone(),
                            ShaderUniform::Float(Animated::new(*v)),
                        );
                    }
                    EffectType::RuntimeShader {
                        sksl: code.clone(),
                        uniforms: uniform_map,
                    }
                }
            };
            Box::new(EffectNode {
                effects: vec![effect],
                style: Style::DEFAULT,
                shader_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
                current_time: 0.0,
            })
        }
        NodeKind::Caption { source, font_size } => {
            let srt_body = if source.contains("-->") {
                source.clone()
            } else {
                let bytes = director
                    .assets
                    .loader
                    .load_bytes(source)
                    .with_context(|| format!("Failed to load caption source: {}", source))?;
                String::from_utf8_lossy(&bytes).into_owned()
            };
            let lines = parse_srt(&srt_body);
            let fc = director.assets.font_collection.clone();
            let mut cap = CaptionNode::new(lines, fc);
            cap.style.font_size = *font_size;
            Box::new(cap)
        }
        NodeKind::Composition {
            width,
            height,
            fps,
            scenes,
            start_offset,
        } => {
            // Create internal Director for the sub-composition
            let mut internal_director = Director::new(
                *width as i32,
                *height as i32,
                *fps,
                director.assets.loader.clone(),
                RenderMode::Export,
                None,
            );

            // Build each scene in the sub-composition
            let mut cumulative_time = 0.0;
            for scene_data in scenes {
                let root_id =
                    build_node_recursive(&mut internal_director, &scene_data.root, track_id_map)
                        .with_context(|| {
                            format!("Failed to build sub-composition scene: {}", scene_data.id)
                        })?;
                internal_director
                    .timeline
                    .push(director_core::director::TimelineItem {
                        scene_root: root_id,
                        name: scene_data.name.clone(),
                        start_time: cumulative_time,
                        duration: scene_data.duration_secs,
                        z_index: scene_data.z_index,
                        audio_tracks: vec![],
                    });
                cumulative_time += scene_data.duration_secs;
            }

            let mut comp = CompositionNode::new(internal_director);
            comp.start_offset = *start_offset;
            Box::new(comp)
        }
    };

    // Apply opacity from StyleMap if specified
    if let Some(opacity) = node_def.style.opacity {
        element.animate_property("opacity", opacity, opacity, 0.0, "linear");
    }

    // 2. Add to Scene Graph
    let id = director.scene.add_node(element);

    // 3. Apply Common Properties (Style, Transform, Animation) via Node access
    if let Some(node) = director.scene.get_node_mut(id) {
        // Apply Style
        let mut style = node.element.layout_style();
        apply_style_map(&mut style, &node_def.style);
        node.element.set_layout_style(style);

        // Apply Background Color if it's a BoxNode
        if let Some(bg) = node_def.style.bg_color {
            if let Some(box_node) = node.element.as_any_mut().downcast_mut::<BoxNode>() {
                box_node.bg_color = Some(Animated::new(bg));
            }
        }

        // Apply BoxNode-specific properties (border, shadow, overflow)
        if let Some(box_node) = node.element.as_any_mut().downcast_mut::<BoxNode>() {
            // Border
            if let Some(bw) = node_def.style.border_width {
                box_node.border_width = Animated::new(bw);
            }
            if let Some(bc) = node_def.style.border_color {
                box_node.border_color = Some(Animated::new(bc));
            }
            if let Some(br) = node_def.style.border_radius {
                box_node.border_radius = Animated::new(br);
            }

            // Shadow
            if let Some(sc) = node_def.style.shadow_color {
                box_node.shadow_color = Some(Animated::new(sc));
            }
            if let Some(sb) = node_def.style.shadow_blur {
                box_node.shadow_blur = Animated::new(sb);
            }
            if let Some(sx) = node_def.style.shadow_x {
                box_node.shadow_offset_x = Animated::new(sx);
            }
            if let Some(sy) = node_def.style.shadow_y {
                box_node.shadow_offset_y = Animated::new(sy);
            }

            // Overflow
            if let Some(o) = &node_def.style.overflow {
                box_node.overflow = o.clone();
            }
        }

        // Apply Transform
        apply_transform_map(&mut node.transform, &node_def.transform);
        if let Some(z_index) = node_def.style.z_index {
            node.z_index = z_index;
        }

        // Apply Animations (Must be done after layout/transform setup as requested)
        apply_animations(&mut node.element, &node_def.animations);

        // Apply Spring Animations
        for spring in &node_def.spring_animations {
            let config = CoreSpringConfig {
                stiffness: spring.config.stiffness,
                damping: spring.config.damping,
                mass: spring.config.mass,
                velocity: spring.config.velocity,
            };

            match spring.property.as_str() {
                "x" => node.transform.translate_x.add_spring(spring.target, config),
                "y" => node.transform.translate_y.add_spring(spring.target, config),
                "scale" => {
                    node.transform.scale_x.add_spring(spring.target, config);
                    node.transform.scale_y.add_spring(spring.target, config);
                }
                "scale_x" => node.transform.scale_x.add_spring(spring.target, config),
                "scale_y" => node.transform.scale_y.add_spring(spring.target, config),
                "rotation" => node.transform.rotation.add_spring(spring.target, config),
                _ => {
                    node.element.animate_property_spring(
                        &spring.property,
                        None,
                        spring.target,
                        config,
                    );
                }
            }
        }

        // Apply Audio Bindings
        for binding in &node_def.audio_bindings {
            let track_idx = *track_id_map.get(&binding.audio_id).ok_or_else(|| {
                anyhow::anyhow!(
                    "Node '{}' references unknown audio track id '{}'",
                    node_def.id,
                    binding.audio_id
                )
            })?;

            if binding.max_value < binding.min_value {
                return Err(anyhow::anyhow!(
                    "Node '{}' has invalid audio binding range: min_value ({}) must be <= max_value ({})",
                    node_def.id,
                    binding.min_value,
                    binding.max_value
                ));
            }

            if !(0.0..=1.0).contains(&binding.smoothing) {
                return Err(anyhow::anyhow!(
                    "Node '{}' has invalid audio smoothing {}. Expected range: 0.0 to 1.0",
                    node_def.id,
                    binding.smoothing
                ));
            }

            let core_binding = AudioBinding {
                track_id: track_idx,
                band: binding.band.as_str().to_string(),
                property: binding.property.as_str().to_string(),
                min_value: binding.min_value,
                max_value: binding.max_value,
                smoothing: binding.smoothing,
                prev_value: binding.min_value,
            };
            node.audio_bindings.push(core_binding);
        }
    }

    // 4. Recurse Children
    // 4. Recurse Children
    for child_def in &node_def.children {
        let child_id = build_node_recursive(director, child_def, track_id_map)
            .with_context(|| format!("Failed to build child node: {}", child_def.id))?;
        director.scene.add_child(id, child_id);
    }

    Ok(id)
}

#[derive(Clone)]
struct PipelineAssetLoader {
    inner: Arc<dyn AssetLoader>,
    search_roots: Vec<String>,
    default_font: Option<String>,
}

impl AssetLoader for PipelineAssetLoader {
    fn load_bytes(&self, path: &str) -> Result<Vec<u8>> {
        if let Ok(bytes) = self.inner.load_bytes(path) {
            return Ok(bytes);
        }

        if !Path::new(path).is_absolute() {
            for root in &self.search_roots {
                let candidate = Path::new(root).join(path);
                let candidate_str = candidate.to_string_lossy();
                if let Ok(bytes) = self.inner.load_bytes(&candidate_str) {
                    return Ok(bytes);
                }
            }
        }

        Err(anyhow::anyhow!(
            "Asset not found: {} (searched default loader and {} configured asset search paths)",
            path,
            self.search_roots.len()
        ))
    }

    fn load_font_fallback(&self) -> Option<Vec<u8>> {
        if let Some(font_path) = &self.default_font {
            if let Ok(bytes) = self.load_bytes(font_path) {
                return Some(bytes);
            }
        }
        self.inner.load_font_fallback()
    }
}

fn apply_animations(element: &mut Box<dyn Element>, animations: &[Animation]) {
    for anim in animations {
        let easing_str = easing_to_str(&anim.easing);

        let start_val = anim.start.unwrap_or(0.0); // Fallback if start not provided

        element.animate_property(
            &anim.property,
            start_val,
            anim.end,
            anim.duration,
            easing_str,
        );
    }
}

fn apply_style_map(style: &mut Style, map: &StyleMap) {
    // Size
    if let Some(w) = &map.width {
        style.size.width = parse_dim(w);
    }
    if let Some(h) = &map.height {
        style.size.height = parse_dim(h);
    }

    // Size constraints
    if let Some(v) = &map.min_width {
        style.min_size.width = parse_dim(v);
    }
    if let Some(v) = &map.max_width {
        style.max_size.width = parse_dim(v);
    }
    if let Some(v) = &map.min_height {
        style.min_size.height = parse_dim(v);
    }
    if let Some(v) = &map.max_height {
        style.max_size.height = parse_dim(v);
    }

    // Aspect ratio
    if let Some(ar) = &map.aspect_ratio {
        if let Some(ratio) = parse_aspect_ratio(ar) {
            style.aspect_ratio = Some(ratio);
        }
    }

    // Display mode
    if let Some(d) = &map.display {
        style.display = match d.as_str() {
            "grid" => Display::Grid,
            "none" => Display::None,
            _ => Display::Flex,
        };
    }

    // Gap
    if let Some(g) = map.gap {
        let gap_val = LengthPercentage::length(g);
        style.gap = Size {
            width: gap_val,
            height: gap_val,
        };
    }

    // Flexbox (container)
    if let Some(d) = &map.flex_direction {
        style.flex_direction = match d.as_str() {
            "column" => FlexDirection::Column,
            "row" | _ => FlexDirection::Row,
        };
    }

    if let Some(j) = &map.justify_content {
        style.justify_content = match j.as_str() {
            "center" => Some(JustifyContent::Center),
            "space_between" => Some(JustifyContent::SpaceBetween),
            "flex_end" => Some(JustifyContent::FlexEnd),
            "flex_start" | _ => Some(JustifyContent::FlexStart),
        };
    }

    if let Some(a) = &map.align_items {
        style.align_items = match a.as_str() {
            "center" => Some(AlignItems::Center),
            "stretch" => Some(AlignItems::Stretch),
            "flex_end" => Some(AlignItems::FlexEnd),
            "flex_start" | _ => Some(AlignItems::FlexStart),
        };
    }

    if let Some(w) = &map.flex_wrap {
        style.flex_wrap = match w.as_str() {
            "wrap" => FlexWrap::Wrap,
            _ => FlexWrap::NoWrap,
        };
    }

    // Flexbox (item)
    if let Some(g) = map.flex_grow {
        style.flex_grow = g;
    }
    if let Some(s) = map.flex_shrink {
        style.flex_shrink = s;
    }
    if let Some(a) = &map.align_self {
        style.align_self = match a.as_str() {
            "center" => Some(AlignSelf::Center),
            "stretch" => Some(AlignSelf::Stretch),
            "flex_end" => Some(AlignSelf::FlexEnd),
            "flex_start" => Some(AlignSelf::FlexStart),
            _ => None,
        };
    }

    // Grid templates
    if let Some(cols) = &map.grid_template_columns {
        style.grid_template_columns = cols.iter().map(|s| parse_track_sizing_str(s)).collect();
    }
    if let Some(rows) = &map.grid_template_rows {
        style.grid_template_rows = rows.iter().map(|s| parse_track_sizing_str(s)).collect();
    }

    // Grid placement
    if let Some(s) = &map.grid_row {
        style.grid_row = parse_grid_line_str(s);
    }
    if let Some(s) = &map.grid_column {
        style.grid_column = parse_grid_line_str(s);
    }

    // Padding (uniform)
    if let Some(p) = map.padding {
        let d = LengthPercentage::length(p);
        style.padding = Rect {
            left: d,
            right: d,
            top: d,
            bottom: d,
        };
    }

    // Padding (per-side overrides)
    if let Some(v) = map.padding_top {
        style.padding.top = LengthPercentage::length(v);
    }
    if let Some(v) = map.padding_right {
        style.padding.right = LengthPercentage::length(v);
    }
    if let Some(v) = map.padding_bottom {
        style.padding.bottom = LengthPercentage::length(v);
    }
    if let Some(v) = map.padding_left {
        style.padding.left = LengthPercentage::length(v);
    }

    // Margin (uniform)
    if let Some(m) = map.margin {
        let d = LengthPercentageAuto::length(m);
        style.margin = Rect {
            left: d,
            right: d,
            top: d,
            bottom: d,
        };
    }

    // Margin (per-side overrides)
    if let Some(v) = map.margin_top {
        style.margin.top = LengthPercentageAuto::length(v);
    }
    if let Some(v) = map.margin_right {
        style.margin.right = LengthPercentageAuto::length(v);
    }
    if let Some(v) = map.margin_bottom {
        style.margin.bottom = LengthPercentageAuto::length(v);
    }
    if let Some(v) = map.margin_left {
        style.margin.left = LengthPercentageAuto::length(v);
    }

    // Position mode (absolute/relative)
    if let Some(pos) = &map.position {
        if pos == "absolute" {
            style.position = Position::Absolute;
        }
    }

    // Insets for absolute positioning
    if let Some(top) = map.top {
        style.inset.top = LengthPercentageAuto::length(top);
    }
    if let Some(left) = map.left {
        style.inset.left = LengthPercentageAuto::length(left);
    }
    if let Some(right) = map.right {
        style.inset.right = LengthPercentageAuto::length(right);
    }
    if let Some(bottom) = map.bottom {
        style.inset.bottom = LengthPercentageAuto::length(bottom);
    }
}

/// Parses aspect ratio string like "16:9" into a float
fn parse_aspect_ratio(s: &str) -> Option<f32> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 2 {
        let w = parts[0].trim().parse::<f32>().ok()?;
        let h = parts[1].trim().parse::<f32>().ok()?;
        if h > 0.0 {
            return Some(w / h);
        }
    }
    None
}

fn apply_transform_map(transform: &mut director_core::types::Transform, map: &TransformMap) {
    if let Some(v) = map.x {
        transform.translate_x = Animated::new(v);
    }
    if let Some(v) = map.y {
        transform.translate_y = Animated::new(v);
    }
    if let Some(v) = map.rotation {
        transform.rotation = Animated::new(v);
    }
    if let Some(v) = map.scale {
        transform.scale_x = Animated::new(v);
        transform.scale_y = Animated::new(v);
    }
    if let Some(v) = map.pivot_x {
        transform.pivot_x = v;
    }
    if let Some(v) = map.pivot_y {
        transform.pivot_y = v;
    }
}

fn parse_dim(val: &str) -> Dimension {
    if val == "auto" {
        Dimension::auto()
    } else if val.ends_with("%") {
        let f = val.trim_end_matches("%").parse::<f32>().unwrap_or(0.0);
        Dimension::percent(f / 100.0)
    } else {
        let f = val.parse::<f32>().unwrap_or(0.0);
        Dimension::length(f)
    }
}

fn parse_object_fit(val: &str) -> ObjectFit {
    match val {
        "contain" => ObjectFit::Contain,
        "fill" => ObjectFit::Fill,
        "cover" | _ => ObjectFit::Cover,
    }
}

fn easing_to_str(easing: &EasingType) -> &'static str {
    match easing {
        EasingType::Linear => "linear",
        EasingType::EaseIn => "ease_in",
        EasingType::EaseOut => "ease_out",
        EasingType::EaseInOut => "ease_in_out",
        EasingType::BounceOut => "bounce_out",
        EasingType::BounceIn => "bounce_in",
        EasingType::BounceInOut => "bounce_in_out",
        EasingType::ElasticOut => "elastic_out",
        EasingType::ElasticIn => "elastic_in",
        EasingType::ElasticInOut => "elastic_in_out",
        EasingType::BackOut => "back_out",
        EasingType::BackIn => "back_in",
        EasingType::BackInOut => "back_in_out",
    }
}

fn parse_track_sizing_str(s: &str) -> GridTemplateComponent<String> {
    if s == "auto" {
        GridTemplateComponent::Single(auto())
    } else if s == "min-content" || s == "min_content" {
        GridTemplateComponent::Single(min_content())
    } else if s == "max-content" || s == "max_content" {
        GridTemplateComponent::Single(max_content())
    } else if s.ends_with("fr") {
        let val = s.trim_end_matches("fr").parse::<f32>().unwrap_or(1.0);
        GridTemplateComponent::Single(fr(val))
    } else if s.ends_with("%") {
        let val = s.trim_end_matches("%").parse::<f32>().unwrap_or(0.0);
        GridTemplateComponent::Single(percent(val / 100.0))
    } else {
        let val = s.parse::<f32>().unwrap_or(0.0);
        GridTemplateComponent::Single(length(val))
    }
}

fn parse_grid_placement_str(s: &str) -> GridPlacement<String> {
    let s = s.trim();
    if s.starts_with("span") {
        let val = s
            .trim_start_matches("span")
            .trim()
            .parse::<u16>()
            .unwrap_or(1);
        GridPlacement::Span(val)
    } else if let Ok(val) = s.parse::<i16>() {
        GridPlacement::Line(val.into())
    } else {
        GridPlacement::Auto
    }
}

fn parse_grid_line_str(s: &str) -> Line<GridPlacement<String>> {
    if s.contains('/') {
        let parts: Vec<&str> = s.split('/').collect();
        Line {
            start: parse_grid_placement_str(parts[0]),
            end: if parts.len() > 1 {
                parse_grid_placement_str(parts[1])
            } else {
                GridPlacement::Auto
            },
        }
    } else {
        Line {
            start: parse_grid_placement_str(s),
            end: GridPlacement::Auto,
        }
    }
}
