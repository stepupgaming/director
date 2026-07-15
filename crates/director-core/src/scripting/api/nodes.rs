//! # Nodes API
//!
//! Node creation functions for Rhai scripts.
//!
//! ## Responsibilities
//! - **Box Nodes**: `add_box` for container nodes
//! - **Text Nodes**: `add_text` for rich text
//! - **Image Nodes**: `add_image` for static images
//! - **Video Nodes**: `add_video` for video playback
//! - **Lottie Nodes**: `add_lottie` for Lottie animations
//! - **SVG Nodes**: `add_svg` for vector graphics
//! - **Composition Nodes**: `add_composition` for nested compositions
//! - **Effect Nodes**: `add_effect_node` for effect containers
//! - **Node Destruction**: `destroy` to remove nodes
//! - **Safety Guards**: stale-handle checks and parent/child attachment validation

use crate::animation::Animated;
use crate::compiler::DependencyInput;
use crate::element::TextFit;
use crate::node::smart_caption::SmartCaptionConfig;
use crate::node::{ActiveSpeakerCamera, ActiveSpeakerCameraConfig, BoxNode, BoxShadow, CaptionLine, CaptionNode, CaptionPosition, CaptionWord, CompositionNode, ContinuityGroup, EffectNode, Framing, ImageNode, LottieNode, PathCommand, ShapeNode, ShapeType, SmartCaptionNode, TextNode, VectorNode, VideoNode, VideoSource};
use crate::types::NodeId;
use crate::semantic::SemanticManifest;
use crate::types::FilterConfig;
use crate::AssetLoader;
use rhai::{Engine, Map};
use std::collections::HashMap;
use std::sync::Arc;
use taffy::style::{Dimension, Style};

use tracing::error;

use super::super::types::{MovieHandle, NodeHandle, SceneHandle};
use super::super::utils::{
    parse_gradient_config, parse_hex_color, parse_layout_style, parse_object_fit,
    parse_spans_from_dynamic, parse_text_shadow,
};

/// Register node creation Rhai functions.
pub fn register(engine: &mut Engine, _loader: Arc<dyn AssetLoader>) {
    engine.register_type_with_name::<NodeHandle>("Node");

    engine.register_fn(
        "destroy",
        |node: &mut NodeHandle| -> Result<(), Box<rhai::EvalAltResult>> {
            let mut d = node.lock_director()?;
            node.ensure_alive(&d)?;
            let parent_id = d.scene.get_node(node.id).and_then(|n| n.parent);
            d.scene.destroy_node(node.id);
            if let Some(parent) = parent_id {
                d.invalidate(vec![DependencyInput::Children(parent)]);
            }
            Ok(())
        },
    );

    engine.register_fn(
        "remove_child",
        |parent: &mut NodeHandle, child: NodeHandle| -> Result<(), Box<rhai::EvalAltResult>> {
            if !Arc::ptr_eq(&parent.director, &child.director) {
                return Err("Parent and child belong to different directors".into());
            }
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            child.ensure_alive(&d)?;
            d.remove_child(parent.id, child.id);
            Ok(())
        },
    );

    engine.register_fn(
        "set_filter",
        |node: &mut NodeHandle,
         blur: f64,
         brightness: f64,
         contrast: f64,
         saturation: f64,
         hue_rotate: f64|
         -> Result<(), Box<rhai::EvalAltResult>> {
            let mut d = node.lock_director()?;
            node.ensure_alive(&d)?;
            if let Some(n) = d.scene.get_node_mut(node.id) {
                n.filter = FilterConfig {
                    blur: blur as f32,
                    brightness: brightness as f32,
                    contrast: contrast as f32,
                    saturation: saturation as f32,
                    hue_rotate: hue_rotate as f32,
                };
            }
            Ok(())
        },
    );

    // ========== ADD_BOX ==========
    engine.register_fn(
        "add_box",
        |parent: &mut NodeHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let mut box_node = BoxNode::new();
            apply_box_props(&mut box_node, &props);
            parse_layout_style(&props, &mut box_node.style);

            let id = d.scene.add_node(Box::new(box_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(format!("Failed to attach node {} to parent {}", id, parent.id).into());
            }

            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_box",
        |scene: &mut SceneHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let mut box_node = BoxNode::new();
            apply_box_props(&mut box_node, &props);
            parse_layout_style(&props, &mut box_node.style);

            let id = d.scene.add_node(Box::new(box_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }

            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    // ========== ADD_IMAGE ==========

    engine.register_fn(
        "add_image",
        move |parent: &mut NodeHandle,
              path: &str|
              -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let image = d.assets.load_image(path);

            let img_node = ImageNode::from_image_with_path(image, path);
            let id = d.scene.add_node(Box::new(img_node));
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach image node {} to parent {}", id, parent.id).into(),
                );
            }
            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_image",
        |scene: &mut SceneHandle, path: &str| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let image = d.assets.load_image(path);

            let img_node = ImageNode::from_image_with_path(image, path);
            let id = d.scene.add_node(Box::new(img_node));
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach image node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }
            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_image",
        |scene: &mut SceneHandle,
         path: &str,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let image = d.assets.load_image(path);

            let mut img_node = ImageNode::from_image_with_path(image, path);
            parse_layout_style(&props, &mut img_node.style);

            if let Some(fit_str) = props
                .get("object_fit")
                .and_then(|v| v.clone().into_string().ok())
            {
                if let Some(fit) = parse_object_fit(&fit_str) {
                    img_node.object_fit = fit;
                }
            }
            if let Some(v) = props.get("border_radius").and_then(|v| v.as_float().ok()) {
                img_node.border_radius = Animated::new(v as f32);
            }

            let id = d.scene.add_node(Box::new(img_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach image node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }
            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_image",
        |parent: &mut NodeHandle,
         path: &str,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let image = d.assets.load_image(path);

            let mut img_node = ImageNode::from_image_with_path(image, path);
            parse_layout_style(&props, &mut img_node.style);

            if let Some(fit_str) = props
                .get("object_fit")
                .and_then(|v| v.clone().into_string().ok())
            {
                if let Some(fit) = parse_object_fit(&fit_str) {
                    img_node.object_fit = fit;
                }
            }
            if let Some(v) = props.get("border_radius").and_then(|v| v.as_float().ok()) {
                img_node.border_radius = Animated::new(v as f32);
            }

            let id = d.scene.add_node(Box::new(img_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach image node {} to parent {}", id, parent.id).into(),
                );
            }
            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    // ========== ADD_LOTTIE ==========
    engine.register_fn(
        "add_lottie",
        |parent: &mut NodeHandle, path: &str| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let blob = d.assets.load_blob(path).map_err(|e| e.to_string())?;

            match LottieNode::new(&blob, HashMap::new(), &d.assets) {
                Ok(lottie_node) => {
                    let id = d.scene.add_node(Box::new(lottie_node));
                    if let Some(n) = d.scene.get_node_mut(id) {
                        n.filter = FilterConfig::default();
                    }
                    if !d.add_child(parent.id, id) {
                        d.scene.destroy_node(id);
                        return Err(format!(
                            "Failed to attach lottie node {} to parent {}",
                            id, parent.id
                        )
                        .into());
                    }
                    Ok(NodeHandle {
                        director: parent.director.clone(),
                        id,
                    })
                }
                Err(e) => Err(format!("Failed to parse lottie: {}", e).into()),
            }
        },
    );

    engine.register_fn(
        "add_lottie",
        |parent: &mut NodeHandle,
         path: &str,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let blob = d.assets.load_blob(path).map_err(|e| e.to_string())?;

            let mut assets_map = HashMap::new();
            if let Some(assets_prop) = props
                .get("assets")
                .and_then(|v| v.clone().try_cast::<Map>())
            {
                for (key, val) in assets_prop {
                    if let Ok(asset_path) = val.into_string() {
                        if let Some(image) = d.assets.load_image(&asset_path) {
                            assets_map.insert(key.to_string(), image);
                        }
                    }
                }
            }

            match LottieNode::new(&blob, assets_map, &d.assets) {
                Ok(mut lottie_node) => {
                    parse_layout_style(&props, &mut lottie_node.style);
                    if let Some(v) = props.get("speed").and_then(|v| v.as_float().ok()) {
                        lottie_node.speed = v as f32;
                    }
                    if let Some(v) = props.get("loop").and_then(|v| v.as_bool().ok()) {
                        lottie_node.loop_anim = v;
                    }

                    let id = d.scene.add_node(Box::new(lottie_node));
                    if let Some(n) = d.scene.get_node_mut(id) {
                        n.filter = parse_filter_config(&props);
                    }
                    if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                        if let Some(n) = d.scene.get_node_mut(id) {
                            n.z_index = z as i32;
                        }
                    }
                    if !d.add_child(parent.id, id) {
                        d.scene.destroy_node(id);
                        return Err(format!(
                            "Failed to attach lottie node {} to parent {}",
                            id, parent.id
                        )
                        .into());
                    }
                    Ok(NodeHandle {
                        director: parent.director.clone(),
                        id,
                    })
                }
                Err(e) => Err(format!("Failed to parse lottie: {}", e).into()),
            }
        },
    );

    engine.register_fn(
        "add_lottie",
        |scene: &mut SceneHandle,
         path: &str,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let blob = d.assets.load_blob(path).map_err(|e| e.to_string())?;

            let mut assets_map = HashMap::new();
            if let Some(assets_prop) = props
                .get("assets")
                .and_then(|v| v.clone().try_cast::<Map>())
            {
                for (key, val) in assets_prop {
                    if let Ok(asset_path) = val.into_string() {
                        if let Some(image) = d.assets.load_image(&asset_path) {
                            assets_map.insert(key.to_string(), image);
                        }
                    }
                }
            }

            match LottieNode::new(&blob, assets_map, &d.assets) {
                Ok(mut lottie_node) => {
                    parse_layout_style(&props, &mut lottie_node.style);
                    if let Some(v) = props.get("speed").and_then(|v| v.as_float().ok()) {
                        lottie_node.speed = v as f32;
                    }
                    if let Some(v) = props.get("loop").and_then(|v| v.as_bool().ok()) {
                        lottie_node.loop_anim = v;
                    }

                    let id = d.scene.add_node(Box::new(lottie_node));
                    if let Some(n) = d.scene.get_node_mut(id) {
                        n.filter = parse_filter_config(&props);
                    }
                    if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                        if let Some(n) = d.scene.get_node_mut(id) {
                            n.z_index = z as i32;
                        }
                    }
                    if !d.add_child(scene.root_id, id) {
                        d.scene.destroy_node(id);
                        return Err(format!(
                            "Failed to attach lottie node {} to scene root {}",
                            id, scene.root_id
                        )
                        .into());
                    }
                    Ok(NodeHandle {
                        director: scene.director.clone(),
                        id,
                    })
                }
                Err(e) => Err(format!("Failed to parse lottie: {}", e).into()),
            }
        },
    );

    // ========== ADD_SVG ==========
    engine.register_fn(
        "add_svg",
        |scene: &mut SceneHandle, path: &str| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let blob = d.assets.load_blob(path).unwrap_or(Arc::new(Vec::new()));

            let vec_node = VectorNode::new(&blob);
            let id = d.scene.add_node(Box::new(vec_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = FilterConfig::default();
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach svg node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }
            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_svg",
        |scene: &mut SceneHandle,
         path: &str,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let blob = d.assets.load_blob(path).unwrap_or(Arc::new(Vec::new()));

            let mut vec_node = VectorNode::new(&blob);
            parse_layout_style(&props, &mut vec_node.style);

            let id = d.scene.add_node(Box::new(vec_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach svg node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }
            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_svg",
        |parent: &mut NodeHandle, path: &str| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let blob = d.assets.load_blob(path).unwrap_or(Arc::new(Vec::new()));

            let vec_node = VectorNode::new(&blob);
            let id = d.scene.add_node(Box::new(vec_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = FilterConfig::default();
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach svg node {} to parent {}", id, parent.id).into(),
                );
            }
            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_svg",
        |parent: &mut NodeHandle,
         path: &str,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let blob = d.assets.load_blob(path).unwrap_or(Arc::new(Vec::new()));

            let mut vec_node = VectorNode::new(&blob);
            parse_layout_style(&props, &mut vec_node.style);

            let id = d.scene.add_node(Box::new(vec_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach svg node {} to parent {}", id, parent.id).into(),
                );
            }
            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    // ========== ADD_VIDEO ==========
    engine.register_fn(
        "add_video",
        |parent: &mut NodeHandle, path: &str| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let mode = d.render_mode;
            let source = build_video_source(&d, path);

            let vid_node = VideoNode::new(source, mode).with_source_path(path);
            let id = d.scene.add_node(Box::new(vid_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = FilterConfig::default();
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach video node {} to parent {}", id, parent.id).into(),
                );
            }
            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_video",
        |parent: &mut NodeHandle,
         path: &str,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let mode = d.render_mode;
            let source = build_video_source(&d, path);

            let mut vid_node = VideoNode::new(source, mode).with_source_path(path);
            parse_layout_style(&props, &mut vid_node.style);

            if let Some(fit_str) = props
                .get("object_fit")
                .and_then(|v| v.clone().into_string().ok())
            {
                if let Some(fit) = parse_object_fit(&fit_str) {
                    vid_node.object_fit = fit;
                }
            }
            if let Some(v) = props.get("border_radius").and_then(|v| v.as_float().ok()) {
                vid_node.border_radius = Animated::new(v as f32);
            }

            let id = d.scene.add_node(Box::new(vid_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach video node {} to parent {}", id, parent.id).into(),
                );
            }
            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_video",
        |scene: &mut SceneHandle, path: &str| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let mode = d.render_mode;
            let source = build_video_source(&d, path);

            let vid_node = VideoNode::new(source, mode).with_source_path(path);
            let id = d.scene.add_node(Box::new(vid_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = FilterConfig::default();
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach video node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }
            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_video",
        |scene: &mut SceneHandle,
         path: &str,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let mode = d.render_mode;
            let source = build_video_source(&d, path);

            let mut vid_node = VideoNode::new(source, mode).with_source_path(path);
            parse_layout_style(&props, &mut vid_node.style);

            if let Some(fit_str) = props
                .get("object_fit")
                .and_then(|v| v.clone().into_string().ok())
            {
                if let Some(fit) = parse_object_fit(&fit_str) {
                    vid_node.object_fit = fit;
                }
            }
            if let Some(v) = props.get("border_radius").and_then(|v| v.as_float().ok()) {
                vid_node.border_radius = Animated::new(v as f32);
            }

            let id = d.scene.add_node(Box::new(vid_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach video node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }
            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    // ========== ADD_ACTIVE_SPEAKER_CAMERA ==========
    engine.register_fn(
        "add_active_speaker_camera",
        |parent: &mut NodeHandle,
         path: &str,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let (camera, entity_refs) = build_active_speaker_camera(&mut d, path, &props)?;
            attach_active_speaker_camera_node(
                &mut d,
                camera,
                entity_refs,
                &props,
                parent.id,
                parent.director.clone(),
                "parent",
            )
        },
    );

    engine.register_fn(
        "add_active_speaker_camera",
        |scene: &mut SceneHandle,
         path: &str,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let (camera, entity_refs) = build_active_speaker_camera(&mut d, path, &props)?;
            attach_active_speaker_camera_node(
                &mut d,
                camera,
                entity_refs,
                &props,
                scene.root_id,
                scene.director.clone(),
                "scene root",
            )
        },
    );

    // ========== ADD_TEXT ==========
    engine.register_fn(
        "add_text",
        |parent: &mut NodeHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let font_collection = d.assets.font_collection.clone();

            let spans = if let Some(c) = props.get("content") {
                parse_spans_from_dynamic(c.clone())
            } else {
                Vec::new()
            };

            let mut text_node = TextNode::new(spans, font_collection);
            parse_layout_style(&props, &mut text_node.style);
            apply_text_props(&mut text_node, &props);
            text_node.shadow = parse_text_shadow(&props);
            text_node.init_paragraph();

            let id = d.scene.add_node(Box::new(text_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach text node {} to parent {}", id, parent.id).into(),
                );
            }

            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_text",
        |scene: &mut SceneHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let font_collection = d.assets.font_collection.clone();

            let spans = if let Some(c) = props.get("content") {
                parse_spans_from_dynamic(c.clone())
            } else {
                Vec::new()
            };

            let mut text_node = TextNode::new(spans, font_collection);
            parse_layout_style(&props, &mut text_node.style);
            apply_text_props(&mut text_node, &props);
            text_node.shadow = parse_text_shadow(&props);
            text_node.init_paragraph();

            let id = d.scene.add_node(Box::new(text_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach text node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }

            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    // ========== ADD_CAPTION ==========
    engine.register_fn(
        "add_caption",
        |parent: &mut NodeHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let font_collection = d.assets.font_collection.clone();
            let lines = parse_caption_lines(&d, &props);
            let mut caption_node = CaptionNode::new(lines, font_collection);
            parse_layout_style(&props, &mut caption_node.style_cache);
            apply_caption_props(&mut caption_node, &props);

            let id = d.scene.add_node(Box::new(caption_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach caption node {} to parent {}", id, parent.id).into(),
                );
            }

            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_caption",
        |scene: &mut SceneHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let font_collection = d.assets.font_collection.clone();
            let lines = parse_caption_lines(&d, &props);
            let mut caption_node = CaptionNode::new(lines, font_collection);
            parse_layout_style(&props, &mut caption_node.style_cache);
            apply_caption_props(&mut caption_node, &props);

            let id = d.scene.add_node(Box::new(caption_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach caption node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }

            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );    // ========== ADD_SMART_CAPTION ==========
    engine.register_fn(
        "add_smart_caption",
        |parent: &mut NodeHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let font_collection = d.assets.font_collection.clone();
            let lines = parse_caption_lines(&d, &props);
            let mut caption_node = CaptionNode::new(lines, font_collection);
            parse_layout_style(&props, &mut caption_node.style_cache);
            apply_caption_props(&mut caption_node, &props);

            let (trajectory, entity_refs) = build_smart_caption_trajectory(&mut d, &caption_node, &props)?;
            let smart_node = SmartCaptionNode::new(caption_node, trajectory);


            let id = d.scene.add_node(Box::new(smart_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.entity_refs = entity_refs;
            }
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach smart caption node {} to parent {}", id, parent.id).into(),
                );
            }

            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_smart_caption",
        |scene: &mut SceneHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let font_collection = d.assets.font_collection.clone();
            let lines = parse_caption_lines(&d, &props);
            let mut caption_node = CaptionNode::new(lines, font_collection);
            parse_layout_style(&props, &mut caption_node.style_cache);
            apply_caption_props(&mut caption_node, &props);

            let (trajectory, entity_refs) = build_smart_caption_trajectory(&mut d, &caption_node, &props)?;
            let smart_node = SmartCaptionNode::new(caption_node, trajectory);


            let id = d.scene.add_node(Box::new(smart_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.entity_refs = entity_refs;
            }
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach smart caption node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }

            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    // ========== ADD_SMART_CAPTION FROM MANIFEST/CONFIG ==========
    engine.register_fn(
        "add_smart_caption",
        |parent: &mut NodeHandle,
         manifest: SemanticManifest,
         config: SmartCaptionConfig,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let id = add_smart_caption_from_manifest(&mut d, &manifest, config, &props, false)?;
            attach_smart_caption_node(&mut d, id, &props, parent.id, parent.director.clone(), "parent")
        },
    );

    engine.register_fn(
        "add_smart_caption",
        |scene: &mut SceneHandle,
         manifest: SemanticManifest,
         config: SmartCaptionConfig,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let id = add_smart_caption_from_manifest(&mut d, &manifest, config, &props, false)?;
            attach_smart_caption_node(&mut d, id, &props, scene.root_id, scene.director.clone(), "scene root")
        },
    );

    // ========== ADD_SMART_CAPTION_CONTINUOUS FROM MANIFEST/CONFIG ==========
    engine.register_fn(
        "add_smart_caption_continuous",
        |parent: &mut NodeHandle,
         manifest: SemanticManifest,
         config: SmartCaptionConfig,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let id = add_smart_caption_from_manifest(&mut d, &manifest, config, &props, true)?;
            attach_smart_caption_node(&mut d, id, &props, parent.id, parent.director.clone(), "parent")
        },
    );

    engine.register_fn(
        "add_smart_caption_continuous",
        |scene: &mut SceneHandle,
         manifest: SemanticManifest,
         config: SmartCaptionConfig,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let id = add_smart_caption_from_manifest(&mut d, &manifest, config, &props, true)?;
            attach_smart_caption_node(&mut d, id, &props, scene.root_id, scene.director.clone(), "scene root")
        },
    );

    // Standalone SRT parsing for use in Rhai scripts
    engine.register_fn("parse_srt", |content: &str| -> Vec<CaptionLine> {
        crate::node::caption::parse_srt(content)
    });

    // ========== ADD_COMPOSITION ==========
    engine.register_fn(
        "add_composition",
        |scene: &mut SceneHandle,
         comp_def: MovieHandle|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            // Cycle Detection
            if Arc::ptr_eq(&scene.director, &comp_def.director) {
                error!("Cycle detected. A composition cannot contain itself.");
                return Err("A composition cannot contain itself".into());
            }

            let mut inner_director = comp_def.lock_director()?.clone();

            // Share resources from parent
            {
                let parent = scene.lock_director()?;
                inner_director.assets = parent.assets.clone();
            }

            let comp_node = CompositionNode::new(inner_director);

            let mut d = scene.lock_director()?;
            let id = d.scene.add_node(Box::new(comp_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = FilterConfig::default();
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach composition node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }

            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_composition",
        |scene: &mut SceneHandle,
         comp_def: MovieHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            // Cycle Detection
            if Arc::ptr_eq(&scene.director, &comp_def.director) {
                error!("Cycle detected. A composition cannot contain itself.");
                return Err("A composition cannot contain itself".into());
            }

            let mut inner_director = comp_def.lock_director()?.clone();

            // Share resources from parent
            {
                let parent = scene.lock_director()?;
                inner_director.assets = parent.assets.clone();
            }

            let mut comp_node = CompositionNode::new(inner_director);
            parse_layout_style(&props, &mut comp_node.style);

            let mut d = scene.lock_director()?;
            let id = d.scene.add_node(Box::new(comp_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach composition node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }

            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    // ========== ADD_EFFECT_NODE ==========
    engine.register_fn(
        "add_effect_node",
        |parent: &mut NodeHandle|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let effect_node = EffectNode {
                effects: Vec::new(),
                style: Style::default(),
                shader_cache: d.assets.shader_cache.clone(),
                current_time: 0.0,
            };
            let id = d.scene.add_node(Box::new(effect_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = FilterConfig::default();
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach effect node {} to parent {}", id, parent.id).into(),
                );
            }
            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_effect_node",
        |parent: &mut NodeHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let mut effect_node = EffectNode {
                effects: Vec::new(),
                style: Style::default(),
                shader_cache: d.assets.shader_cache.clone(),
                current_time: 0.0,
            };
            parse_layout_style(&props, &mut effect_node.style);
            let id = d.scene.add_node(Box::new(effect_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach effect node {} to parent {}", id, parent.id).into(),
                );
            }
            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_effect_node",
        |scene: &mut SceneHandle|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let effect_node = EffectNode {
                effects: Vec::new(),
                style: Style::default(),
                shader_cache: d.assets.shader_cache.clone(),
                current_time: 0.0,
            };
            let id = d.scene.add_node(Box::new(effect_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = FilterConfig::default();
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach effect node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }
            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_effect_node",
        |scene: &mut SceneHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let mut effect_node = EffectNode {
                effects: Vec::new(),
                style: Style::default(),
                shader_cache: d.assets.shader_cache.clone(),
                current_time: 0.0,
            };
            parse_layout_style(&props, &mut effect_node.style);
            let id = d.scene.add_node(Box::new(effect_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach effect node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }
            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    // ========== ADD_SHAPE (circle / ellipse / path) ==========
    engine.register_fn(
        "add_circle",
        |parent: &mut NodeHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let cx = props.get("cx").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
            let cy = props.get("cy").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
            let r = props.get("r").and_then(|v| v.as_float().ok()).unwrap_or(10.0) as f32;
            let mut shape = ShapeNode::new(ShapeType::Circle { cx, cy, r });
            apply_shape_props(&mut shape, &props);
            parse_layout_style(&props, &mut shape.style);

            let id = d.scene.add_node(Box::new(shape));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach shape node {} to parent {}", id, parent.id).into(),
                );
            }
            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_circle",
        |scene: &mut SceneHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let cx = props.get("cx").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
            let cy = props.get("cy").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
            let r = props.get("r").and_then(|v| v.as_float().ok()).unwrap_or(10.0) as f32;
            let mut shape = ShapeNode::new(ShapeType::Circle { cx, cy, r });
            apply_shape_props(&mut shape, &props);
            parse_layout_style(&props, &mut shape.style);

            let id = d.scene.add_node(Box::new(shape));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach shape node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }
            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_ellipse",
        |parent: &mut NodeHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let cx = props.get("cx").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
            let cy = props.get("cy").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
            let rx = props.get("rx").and_then(|v| v.as_float().ok()).unwrap_or(10.0) as f32;
            let ry = props.get("ry").and_then(|v| v.as_float().ok()).unwrap_or(10.0) as f32;
            let mut shape = ShapeNode::new(ShapeType::Ellipse { cx, cy, rx, ry });
            apply_shape_props(&mut shape, &props);
            parse_layout_style(&props, &mut shape.style);

            let id = d.scene.add_node(Box::new(shape));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach shape node {} to parent {}", id, parent.id).into(),
                );
            }
            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_ellipse",
        |scene: &mut SceneHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let cx = props.get("cx").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
            let cy = props.get("cy").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
            let rx = props.get("rx").and_then(|v| v.as_float().ok()).unwrap_or(10.0) as f32;
            let ry = props.get("ry").and_then(|v| v.as_float().ok()).unwrap_or(10.0) as f32;
            let mut shape = ShapeNode::new(ShapeType::Ellipse { cx, cy, rx, ry });
            apply_shape_props(&mut shape, &props);
            parse_layout_style(&props, &mut shape.style);

            let id = d.scene.add_node(Box::new(shape));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach shape node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }
            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_path",
        |parent: &mut NodeHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = parent.lock_director()?;
            parent.ensure_alive(&d)?;
            let commands = parse_path_commands(&props);
            let mut shape = ShapeNode::new(ShapeType::Path { commands });
            apply_shape_props(&mut shape, &props);
            parse_layout_style(&props, &mut shape.style);

            let id = d.scene.add_node(Box::new(shape));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(parent.id, id) {
                d.scene.destroy_node(id);
                return Err(
                    format!("Failed to attach shape node {} to parent {}", id, parent.id).into(),
                );
            }
            Ok(NodeHandle {
                director: parent.director.clone(),
                id,
            })
        },
    );

    engine.register_fn(
        "add_path",
        |scene: &mut SceneHandle,
         props: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = scene.lock_director()?;
            let commands = parse_path_commands(&props);
            let mut shape = ShapeNode::new(ShapeType::Path { commands });
            apply_shape_props(&mut shape, &props);
            parse_layout_style(&props, &mut shape.style);

            let id = d.scene.add_node(Box::new(shape));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = parse_filter_config(&props);
            }
            if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
                if let Some(n) = d.scene.get_node_mut(id) {
                    n.z_index = z as i32;
                }
            }
            if !d.add_child(scene.root_id, id) {
                d.scene.destroy_node(id);
                return Err(format!(
                    "Failed to attach shape node {} to scene root {}",
                    id, scene.root_id
                )
                .into());
            }
            Ok(NodeHandle {
                director: scene.director.clone(),
                id,
            })
        },
    );

    // ========== WGPU NODE (feature-gated) ==========
    #[cfg(feature = "wgpu")]
    {
        engine.register_fn(
            "add_wgpu_node",
            |parent: &mut NodeHandle,
             vertex_src: &str,
             fragment_src: &str|
             -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
                let mut d = parent.lock_director()?;
                parent.ensure_alive(&d)?;
                let node = crate::node::WgpuNode::new(vertex_src.to_string(), fragment_src.to_string());
                let id = d.scene.add_node(Box::new(node));
                if !d.add_child(parent.id, id) {
                    d.scene.destroy_node(id);
                    return Err(
                        format!("Failed to attach wgpu node {} to parent {}", id, parent.id)
                            .into(),
                    );
                }
                Ok(NodeHandle {
                    director: parent.director.clone(),
                    id,
                })
            },
        );
        engine.register_fn(
            "add_wgpu_node",
            |parent: &mut NodeHandle| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
                let mut d = parent.lock_director()?;
                parent.ensure_alive(&d)?;
                let node = crate::node::WgpuNode::new_default();
                let id = d.scene.add_node(Box::new(node));
                if !d.add_child(parent.id, id) {
                    d.scene.destroy_node(id);
                    return Err(
                        format!("Failed to attach wgpu node {} to parent {}", id, parent.id).into(),
                    );
                }
                Ok(NodeHandle {
                    director: parent.director.clone(),
                    id,
                })
            },
        );
    }

    // Static-subtree caching controls
    engine.register_fn(
        "set_static",
        |node: &mut NodeHandle, enabled: bool| -> Result<bool, Box<rhai::EvalAltResult>> {
            let mut d = node.lock_director()?;
            node.ensure_alive(&d)?;
            if let Some(n) = d.scene.get_node_mut(node.id) {
                n.is_static = enabled;
            }
            Ok(enabled)
        },
    );

    engine.register_fn(
        "is_static",
        |node: &mut NodeHandle| -> Result<bool, Box<rhai::EvalAltResult>> {
            let d = node.lock_director()?;
            node.ensure_alive(&d)?;
            Ok(d.scene.get_node(node.id).map(|n| n.is_static).unwrap_or(false))
        },
    );

    engine.register_fn(
        "clear_static_cache",
        |node: &mut NodeHandle| -> Result<(), Box<rhai::EvalAltResult>> {
            let mut d = node.lock_director()?;
            node.ensure_alive(&d)?;
            d.static_cache.remove(&node.id);
            Ok(())
        },
    );
}

/// Parse CSS-like filter properties from a Rhai map.
fn parse_filter_config(props: &rhai::Map) -> FilterConfig {
    FilterConfig {
        blur: props.get("filter_blur").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32,
        brightness: props
            .get("filter_brightness")
            .and_then(|v| v.as_float().ok())
            .unwrap_or(1.0) as f32,
        contrast: props
            .get("filter_contrast")
            .and_then(|v| v.as_float().ok())
            .unwrap_or(1.0) as f32,
        saturation: props
            .get("filter_saturation")
            .and_then(|v| v.as_float().ok())
            .unwrap_or(1.0) as f32,
        hue_rotate: props
            .get("filter_hue_rotate")
            .and_then(|v| v.as_float().ok())
            .unwrap_or(0.0) as f32,
    }
}

/// Apply box-specific properties from a Rhai map
fn apply_box_props(box_node: &mut BoxNode, props: &rhai::Map) {
    if let Some(c) = props.get("bg_color") {
        if let Ok(s) = c.clone().into_string() {
            if let Some(color) = parse_hex_color(&s) {
                box_node.bg_color = Some(Animated::new(color));
            }
        }
    }
    if let Some(g) = props.get("bg_gradient") {
        box_node.bg_gradient = parse_gradient_config(g);
    }
    if let Some(c) = props.get("shadow_color") {
        if let Ok(s) = c.clone().into_string() {
            if let Some(color) = parse_hex_color(&s) {
                box_node.shadow_color = Some(Animated::new(color));
            }
        }
    }
    if let Some(v) = props.get("shadow_blur").and_then(|v| v.as_float().ok()) {
        box_node.shadow_blur = Animated::new(v as f32);
    }
    if let Some(v) = props.get("shadow_x").and_then(|v| v.as_float().ok()) {
        box_node.shadow_offset_x = Animated::new(v as f32);
    }
    if let Some(v) = props.get("shadow_y").and_then(|v| v.as_float().ok()) {
        box_node.shadow_offset_y = Animated::new(v as f32);
    }
    if let Some(v) = props.get("border_radius").and_then(|v| v.as_float().ok()) {
        box_node.border_radius = Animated::new(v as f32);
    }
    if let Some(v) = props.get("border_width").and_then(|v| v.as_float().ok()) {
        box_node.border_width = Animated::new(v as f32);
    }
    if let Some(c) = props.get("border_color") {
        if let Ok(s) = c.clone().into_string() {
            if let Some(color) = parse_hex_color(&s) {
                box_node.border_color = Some(Animated::new(color));
            }
        }
    }
    if let Some(s) = props
        .get("overflow")
        .and_then(|v| v.clone().into_string().ok())
    {
        box_node.overflow = s;
    }
    // Backdrop blur (glassmorphism)
    if let Some(v) = props.get("backdrop_blur").and_then(|v| v.as_float().ok()) {
        box_node.backdrop_blur = Animated::new(v as f32);
    }
    // Clip-path polygon: array of [x, y] normalized points
    if let Some(arr) = props.get("clip_path").and_then(|v| v.clone().into_array().ok()) {
        let mut points = Vec::new();
        for item in arr {
            if let Some(map) = item.clone().try_cast::<rhai::Map>() {
                let x = map.get("x").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                let y = map.get("y").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                points.push((x, y));
            } else if let Ok(pair) = item.clone().into_array() {
                if pair.len() >= 2 {
                    let x = pair[0].as_float().unwrap_or(0.0) as f32;
                    let y = pair[1].as_float().unwrap_or(0.0) as f32;
                    points.push((x, y));
                }
            }
        }
        if points.len() >= 3 {
            box_node.clip_path = Some(points);
        }
    }
    // Extra stacked shadows: array of #{ color, blur, x, y }
    if let Some(arr) = props.get("box_shadows").and_then(|v| v.clone().into_array().ok()) {
        for item in arr {
            if let Some(map) = item.clone().try_cast::<rhai::Map>() {
                let color = map
                    .get("color")
                    .and_then(|v| v.clone().into_string().ok())
                    .and_then(|s| parse_hex_color(&s))
                    .unwrap_or(crate::types::Color::BLACK);
                let blur = map.get("blur").and_then(|v| v.as_float().ok()).unwrap_or(10.0) as f32;
                let x = map.get("x").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                let y = map.get("y").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                box_node.extra_shadows.push(BoxShadow {
                    color: Animated::new(color),
                    blur: Animated::new(blur),
                    offset_x: Animated::new(x),
                    offset_y: Animated::new(y),
                });
            }
        }
    }
}

/// Apply shape-specific properties from a Rhai map.
fn apply_shape_props(shape: &mut ShapeNode, props: &rhai::Map) {
    if let Some(c) = props.get("fill") {
        if let Ok(s) = c.clone().into_string() {
            if let Some(color) = parse_hex_color(&s) {
                shape.fill_color = Some(Animated::new(color));
            }
        }
    }
    if let Some(g) = props.get("fill_gradient") {
        shape.fill_gradient = parse_gradient_config(g);
    }
    if let Some(c) = props.get("stroke") {
        if let Ok(s) = c.clone().into_string() {
            if let Some(color) = parse_hex_color(&s) {
                shape.stroke_color = Some(Animated::new(color));
            }
        }
    }
    if let Some(v) = props.get("stroke_width").and_then(|v| v.as_float().ok()) {
        shape.stroke_width = Animated::new(v as f32);
    }
}

/// Parse path commands from a Rhai map's `commands` array.
fn parse_path_commands(props: &rhai::Map) -> Vec<PathCommand> {
    let mut commands = Vec::new();
    if let Some(arr) = props.get("commands").and_then(|v| v.clone().into_array().ok()) {
        for item in arr {
            if let Some(map) = item.clone().try_cast::<rhai::Map>() {
                let cmd_type = map
                    .get("type")
                    .and_then(|v| v.clone().into_string().ok())
                    .unwrap_or_default();
                match cmd_type.as_str() {
                    "move" | "move_to" | "M" => {
                        let x = map.get("x").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let y = map.get("y").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        commands.push(PathCommand::MoveTo { x, y });
                    }
                    "line" | "line_to" | "L" => {
                        let x = map.get("x").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let y = map.get("y").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        commands.push(PathCommand::LineTo { x, y });
                    }
                    "quad" | "quad_to" | "Q" => {
                        let cx = map.get("cx").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let cy = map.get("cy").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let x = map.get("x").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let y = map.get("y").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        commands.push(PathCommand::QuadTo { cx, cy, x, y });
                    }
                    "cubic" | "cubic_to" | "C" => {
                        let c1x = map.get("c1x").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let c1y = map.get("c1y").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let c2x = map.get("c2x").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let c2y = map.get("c2y").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let x = map.get("x").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let y = map.get("y").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        commands.push(PathCommand::CubicTo { c1x, c1y, c2x, c2y, x, y });
                    }
                    "arc" | "arc_to" | "A" => {
                        let cx = map.get("cx").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let cy = map.get("cy").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let rx = map.get("rx").and_then(|v| v.as_float().ok()).unwrap_or(10.0) as f32;
                        let ry = map.get("ry").and_then(|v| v.as_float().ok()).unwrap_or(10.0) as f32;
                        let start_angle = map.get("start_angle").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                        let sweep_angle = map.get("sweep_angle").and_then(|v| v.as_float().ok()).unwrap_or(360.0) as f32;
                        commands.push(PathCommand::ArcTo { cx, cy, rx, ry, start_angle, sweep_angle });
                    }
                    "close" | "Z" => {
                        commands.push(PathCommand::Close);
                    }
                    _ => {}
                }
            }
        }
    }
    commands
}

/// Apply text-specific properties from a Rhai map
/// Parse caption lines from Rhai props.
fn parse_caption_lines(d: &crate::director::Director, props: &rhai::Map) -> Vec<CaptionLine> {
    // Priority: file path > raw SRT source > inline content array
    if let Some(file_path) = props.get("file").and_then(|v| v.clone().into_string().ok()) {
        if let Ok(blob) = d.assets.load_blob(&file_path) {
            let content = String::from_utf8_lossy(&blob);
            return crate::node::caption::parse_srt(&content);
        }
    }

    if let Some(source) = props.get("source").and_then(|v| v.clone().into_string().ok()) {
        return crate::node::caption::parse_srt(&source);
    }

    if let Some(arr) = props.get("content").and_then(|v| v.clone().into_array().ok()) {
        let mut lines = Vec::new();
        for item in arr {
            if let Some(map) = item.clone().try_cast::<rhai::Map>() {
                let text = map.get("text").map(|v| v.to_string()).unwrap_or_default();
                let start_sec = map.get("start").and_then(|v| v.as_float().ok()).unwrap_or(0.0);
                let end_sec = map.get("end").and_then(|v| v.as_float().ok()).unwrap_or(0.0);

                let mut words = Vec::new();
                if let Some(w) = map.get("words").and_then(|v| v.clone().into_array().ok()) {
                    for w_item in w {
                        if let Some(w_map) = w_item.clone().try_cast::<rhai::Map>() {
                            let word_text = w_map.get("word").map(|v| v.to_string()).unwrap_or_default();
                            let w_start = w_map.get("start").and_then(|v| v.as_float().ok()).unwrap_or(start_sec);
                            let w_end = w_map.get("end").and_then(|v| v.as_float().ok()).unwrap_or(end_sec);
                            words.push(CaptionWord {
                                text: word_text,
                                start_sec: w_start,
                                end_sec: w_end,
                                display_line_idx: words.len(),
                            });
                        }
                    }
                }

                let mut line = CaptionLine {
                    text,
                    words,
                    start_sec,
                    end_sec,
                };
                crate::node::caption::auto_time_words(&mut line);
                lines.push(line);
            }
        }
        return lines;
    }

    Vec::new()
}

/// Apply caption style properties from a Rhai map.
fn apply_caption_props(node: &mut CaptionNode, props: &rhai::Map) {
    if let Some(s) = props.get("font_size").and_then(|v| v.as_float().ok()) {
        node.style.font_size = s as f32;
    }
    if let Some(c) = props.get("color").and_then(|v| v.clone().into_string().ok()) {
        if let Some(col) = parse_hex_color(&c) {
            node.style.color = col;
        }
    }
    if let Some(c) = props.get("highlight_color").and_then(|v| v.clone().into_string().ok()) {
        if let Some(col) = parse_hex_color(&c) {
            node.style.highlight_color = col;
        }
    }
    if let Some(c) = props.get("spoken_color").and_then(|v| v.clone().into_string().ok()) {
        if let Some(col) = parse_hex_color(&c) {
            node.style.spoken_color = col;
        }
    }
    if let Some(p) = props.get("caption_position").and_then(|v| v.clone().into_string().ok()) {
        let position = CaptionPosition::from_str(&p);
        node.set_position(position);
    }
    if let Some(a) = props.get("align").and_then(|v| v.clone().into_string().ok()) {
        node.style.text_align = match a.as_str() {
            "center" => skia_safe::textlayout::TextAlign::Center,
            "right" => skia_safe::textlayout::TextAlign::Right,
            "left" => skia_safe::textlayout::TextAlign::Left,
            _ => skia_safe::textlayout::TextAlign::Center,
        };
    }
    if let Some(d) = props
        .get("text_direction")
        .and_then(|v| v.clone().into_string().ok())
    {
        node.style.text_direction = crate::text_direction::parse_text_direction_mode(&d);
    }
    if let Some(m) = props.get("bottom_margin").and_then(|v| v.as_float().ok()) {
        node.style.bottom_margin = m as f32;
    }
    if let Some(m) = props.get("top_margin").and_then(|v| v.as_float().ok()) {
        node.style.top_margin = m as f32;
    }
    if let Some(f) = props.get("font_family").and_then(|v| v.clone().into_string().ok()) {
        node.style.font_family = f;
    }
    if let Some(w) = props
        .get("font_weight")
        .or_else(|| props.get("weight"))
        .and_then(super::super::utils::parse_font_weight)
    {
        node.style.font_weight = w;
    }
    if let Some(v) = props.get("stroke_enabled").and_then(|v| v.as_bool().ok()) {
        node.style.stroke_enabled = v;
    }
    if let Some(v) = props.get("stroke_width").and_then(|v| v.as_float().ok()) {
        node.style.stroke_width = v as f32;
    }
    if let Some(c) = props.get("stroke_color").and_then(|v| v.clone().into_string().ok()) {
        if let Some(col) = parse_hex_color(&c) {
            node.style.stroke_color = col;
        }
    }
    if let Some(v) = props.get("glow_enabled").and_then(|v| v.as_bool().ok()) {
        node.style.glow_enabled = v;
    }
    if let Some(v) = props.get("glow_blur").and_then(|v| v.as_float().ok()) {
        node.style.glow_blur = v as f32;
    }
    if let Some(s) = props.get("line_spacing").and_then(|v| v.as_float().ok()) {
        node.style.line_spacing = s as f32;
    }
    if let Some(d) = props.get("color_transition_duration").and_then(|v| v.as_float().ok()) {
        node.style.color_transition_duration = d;
    }
    if let Some(o) = props.get("future_opacity").and_then(|v| v.as_float().ok()) {
        node.style.future_opacity = o as f32;
    }
    if let Some(o) = props.get("spoken_opacity").and_then(|v| v.as_float().ok()) {
        node.style.spoken_color_opacity = o as f32;
    }
    if let Some(o) = props.get("active_opacity").and_then(|v| v.as_float().ok()) {
        node.style.active_opacity = o as f32;
    }
    if let Some(v) = props.get("text_shadow_enabled").and_then(|v| v.as_bool().ok()) {
        node.style.text_shadow_enabled = v;
    }
    if let Some(c) = props.get("text_shadow_color").and_then(|v| v.clone().into_string().ok()) {
        if let Some(col) = parse_hex_color(&c) {
            node.style.text_shadow_color = col;
        }
    }
    if let Some(v) = props.get("text_shadow_blur").and_then(|v| v.as_float().ok()) {
        node.style.text_shadow_blur = v as f32;
    }
    if let Some(v) = props.get("text_shadow_offset_x").and_then(|v| v.as_float().ok()) {
        node.style.text_shadow_offset_x = v as f32;
    }
    if let Some(v) = props.get("text_shadow_offset_y").and_then(|v| v.as_float().ok()) {
        node.style.text_shadow_offset_y = v as f32;
    }
    if let Some(v) = props.get("active_pop_scale").and_then(|v| v.as_float().ok()) {
        node.style.active_pop_scale = (v as f32).clamp(1.0, 1.10);
    }
    if let Some(d) = props.get("active_pop_duration").and_then(|v| v.as_float().ok()) {
        node.style.active_pop_duration = (d as f64).clamp(0.05, 0.15);
    }
    if let Some(v) = props.get("line_reveal_offset_1").and_then(|v| v.as_float().ok()) {
        let f = v as f32;
        if node.style.line_reveal_offsets.len() >= 2 {
            node.style.line_reveal_offsets[1] = f;
        }
    }
    if let Some(v) = props.get("line_reveal_duration_1").and_then(|v| v.as_float().ok()) {
        let d = v as f32;
        if node.style.line_reveal_durations.len() >= 2 {
            node.style.line_reveal_durations[1] = d;
        }
    }
    if let Some(v) = props.get("auto_adapt_shadow").and_then(|v| v.as_bool().ok()) {
        node.style.auto_adapt_shadow = v;
    }
    if let Some(v) = props.get("pill_enabled").and_then(|v| v.as_bool().ok()) {
        node.style.pill_enabled = v;
    }
    if let Some(c) = props.get("pill_color").and_then(|v| v.clone().into_string().ok()) {
        if let Some(col) = parse_hex_color(&c) {
            node.style.pill_color = col;
        }
    }
    if let Some(v) = props.get("pill_adaptive").and_then(|v| v.as_bool().ok()) {
        node.style.pill_adaptive = v;
    }
    if let Some(v) = props.get("pill_padding_h").and_then(|v| v.as_float().ok()) {
        node.style.pill_padding_h = v as f32;
    }
    if let Some(v) = props.get("pill_padding_v").and_then(|v| v.as_float().ok()) {
        node.style.pill_padding_v = v as f32;
    }
    if let Some(v) = props.get("pill_radius").and_then(|v| v.as_float().ok()) {
        node.style.pill_radius = v as f32;
    }
    if let Some(v) = props.get("auto_phrase_break").and_then(|v| v.as_bool().ok()) {
        node.style.auto_phrase_break = v;
    }
    if let Some(v) = props.get("max_words_per_line").and_then(|v| v.as_int().ok()) {
        node.style.max_words_per_line = (v as usize).max(1);
    }
    if let Some(m) = props
        .get("phrase_line_mode")
        .and_then(|v| v.clone().into_string().ok())
    {
        node.style.phrase_line_mode = match m.to_ascii_lowercase().as_str() {
            "stack" => crate::node::caption::PhraseLineMode::Stack,
            _ => crate::node::caption::PhraseLineMode::Replace,
        };
    }
    if let Some(p) = props
        .get("safe_area")
        .or_else(|| props.get("safe_area_preset"))
        .and_then(|v| v.clone().into_string().ok())
    {
        node.style.safe_area_preset = Some(p);
    }
    if let Some(v) = props.get("scale_with_canvas").and_then(|v| v.as_bool().ok()) {
        node.style.scale_with_canvas = v;
    }
    if let Some(v) = props.get("reference_canvas_height").and_then(|v| v.as_float().ok()) {
        node.style.reference_canvas_height = v as f32;
    }
    if let Some(v) = props.get("uppercase").and_then(|v| v.as_bool().ok()) {
        node.style.uppercase = v;
    }
    // Rebuild the Taffy style now that all caption props (including margins)
    // have been applied so the layout rect reflects the chosen position.
    node.rebuild_layout_style();
    node.after_style_changed();
}

fn apply_text_props(text_node: &mut TextNode, props: &rhai::Map) {
    if let Some(s) = props.get("size").and_then(|v| v.as_float().ok()) {
        text_node.default_font_size = Animated::new(s as f32);
    }
    if let Some(c) = props
        .get("color")
        .and_then(|v| v.clone().into_string().ok())
    {
        if let Some(col) = parse_hex_color(&c) {
            text_node.default_color = Animated::new(col);
        }
    }

    let weight = props.get("weight").and_then(super::super::utils::parse_font_weight);

    // Top-level rich text defaults
    let default_background_color = props
        .get("background_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));
    let default_background_padding = props
        .get("background_padding")
        .and_then(|v| v.as_float().ok())
        .map(|v| v as f32);
    let default_stroke_width = props
        .get("stroke_width")
        .and_then(|v| v.as_float().ok())
        .map(|v| v as f32);
    let default_stroke_color = props
        .get("stroke_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));
    let default_fill_gradient = props.get("fill_gradient").and_then(parse_gradient_config);

    // Apply span-level defaults if missing
    for span in &mut text_node.spans {
        if span.font_weight.is_none() {
            span.font_weight = weight;
        }
        if span.background_color.is_none() {
            span.background_color = default_background_color;
        }
        if span.background_padding.is_none() {
            span.background_padding = default_background_padding;
        }
        if span.stroke_width.is_none() {
            span.stroke_width = default_stroke_width;
        }
        if span.stroke_color.is_none() {
            span.stroke_color = default_stroke_color;
        }
        if span.fill_gradient.is_none() {
            span.fill_gradient = default_fill_gradient.clone();
        }
    }

    if let Some(s) = props.get("fit").and_then(|v| v.clone().into_string().ok()) {
        match s.as_str() {
            "shrink" => {
                text_node.fit_mode = TextFit::Shrink;
                // Only set a default width if the user didn't explicitly specify one.
                // Shrink-to-fit needs a definite width to compare against.
                if text_node.style.size.width == Dimension::auto() {
                    text_node.style.size.width = Dimension::percent(1.0);
                }
            }
            "auto" => text_node.fit_mode = TextFit::None,
            "wrap" => text_node.fit_mode = TextFit::None,
            _ => text_node.fit_mode = TextFit::None,
        }
    }
    if let Some(v) = props.get("min_size").and_then(|v| v.as_float().ok()) {
        text_node.min_size = v as f32;
    }
    if let Some(v) = props.get("max_size").and_then(|v| v.as_float().ok()) {
        text_node.max_size = v as f32;
    }
    if let Some(a) = props.get("align").and_then(|v| v.clone().into_string().ok()) {
        text_node.text_align = match a.as_str() {
            "center" => skia_safe::textlayout::TextAlign::Center,
            "right" => skia_safe::textlayout::TextAlign::Right,
            "justify" => skia_safe::textlayout::TextAlign::Justify,
            _ => skia_safe::textlayout::TextAlign::Left,
        };
    }
    if let Some(d) = props
        .get("text_direction")
        .and_then(|v| v.clone().into_string().ok())
    {
        text_node.set_text_direction(crate::text_direction::parse_text_direction_mode(&d));
    }
}

/// Apply common post-creation steps for a smart caption node (filter, z-index)
/// and attach it to `parent_id`. `parent_desc` is used only for error messages.
fn attach_smart_caption_node(
    d: &mut crate::director::Director,
    id: NodeId,
    props: &rhai::Map,
    parent_id: NodeId,
    director: std::sync::Arc<std::sync::Mutex<crate::director::Director>>,
    parent_desc: &str,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    if let Some(n) = d.scene.get_node_mut(id) {
        n.filter = parse_filter_config(props);
    }
    if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
        if let Some(n) = d.scene.get_node_mut(id) {
            n.z_index = z as i32;
        }
    }
    if !d.add_child(parent_id, id) {
        d.scene.destroy_node(id);
        return Err(format!(
            "Failed to attach smart caption node {} to {}",
            id, parent_desc
        )
        .into());
    }

    Ok(NodeHandle { director, id })
}

/// Build a `SmartCaptionNode` from a `SemanticManifest` and add it to the
/// scene graph. When `force_continuous` is `true`, the continuous temporal
/// solver is used regardless of `config.continuous`.
fn add_smart_caption_from_manifest(
    d: &mut crate::director::Director,
    manifest: &SemanticManifest,
    config: SmartCaptionConfig,
    props: &rhai::Map,
    force_continuous: bool,
) -> Result<NodeId, Box<rhai::EvalAltResult>> {
    let font_collection = d.assets.font_collection.clone();
    let lines = parse_caption_lines(d, props);
    let mut caption_node = CaptionNode::new(lines, font_collection);
    parse_layout_style(props, &mut caption_node.style_cache);
    apply_caption_props(&mut caption_node, props);

    let smart_node = if force_continuous || config.continuous {
        SmartCaptionNode::from_manifest_continuous(caption_node, &d.entities, manifest, config)
    } else {
        SmartCaptionNode::from_manifest(caption_node, &d.entities, manifest, config)
    };

    let id = d.scene.add_node(Box::new(smart_node));
    if let Some(n) = d.scene.get_node_mut(id) {
        n.entity_refs = manifest.entity_refs.clone();
    }
    Ok(id)
}

/// Build a `CaptionTrajectory` for a smart caption from Rhai props.
///
/// Expected props:
/// - `entity_refs`: array of entity UUID strings
/// - `start_time`, `end_time`: shot boundaries (defaults from caption lines)
/// - `clearance`, `max_velocity`, `smoothing_window`, `motion_penalty`,
///   `overlap_penalty`, `safe_zone_penalty`: solver tuning
/// - `sample_rate`: solver samples per second
fn build_smart_caption_trajectory(
    d: &mut crate::director::Director,
    caption_node: &CaptionNode,
    props: &rhai::Map,
) -> Result<(crate::semantic::CaptionTrajectory, Vec<uuid::Uuid>), Box<rhai::EvalAltResult>> {
    use crate::semantic::{solve_caption_trajectory, SemanticManifest, SolverConfig, SolverParams};

    let mut entity_refs = Vec::new();
    if let Some(arr) = props.get("entity_refs").and_then(|v| v.clone().into_array().ok()) {
        for item in arr {
            if let Ok(s) = item.clone().into_string() {
                let id = uuid::Uuid::parse_str(&s)
                    .map_err(|e| format!("invalid entity UUID: {}", e))?;
                entity_refs.push(id);
            }
        }
    }

    let start_time = props
        .get("start_time")
        .and_then(|v| v.as_float().ok())
        .unwrap_or_else(|| {
            caption_node
                .lines
                .first()
                .map(|l| l.start_sec)
                .unwrap_or(0.0)
        });
    let end_time = props
        .get("end_time")
        .and_then(|v| v.as_float().ok())
        .unwrap_or_else(|| {
            caption_node
                .lines
                .last()
                .map(|l| l.end_sec)
                .unwrap_or(start_time + 1.0)
        });

    let mut solver = SolverConfig::default();
    if let Some(v) = props.get("clearance").and_then(|v| v.as_float().ok()) {
        solver.clearance = v as f32;
    }
    if let Some(v) = props.get("smoothing_window").and_then(|v| v.as_int().ok()) {
        solver.smoothing_window = v as usize;
    }
    if let Some(v) = props.get("motion_penalty").and_then(|v| v.as_float().ok()) {
        solver.motion_penalty = v as f32;
    }
    if let Some(v) = props.get("overlap_penalty").and_then(|v| v.as_float().ok()) {
        solver.overlap_penalty = v as f32;
    }
    if let Some(v) = props.get("safe_zone_penalty").and_then(|v| v.as_float().ok()) {
        solver.safe_zone_penalty = v as f32;
    }

    let manifest = SemanticManifest {
        entity_refs,
        start_time,
        end_time,
        solver,
    };

    let canvas_w = d.width as f32;
    let canvas_h = d.height as f32;
    let caption_w = canvas_w * caption_node.style.layout_width_fraction;
    let caption_h = caption_node.style.font_size * 2.0f32;
    let safe_top = caption_node.style.top_margin;
    let safe_bottom = caption_node.style.bottom_margin;
    let safe_left = caption_node.style.left_margin;
    let safe_right = caption_node.style.right_margin;
    let sample_rate = props
        .get("sample_rate")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(10.0);

    let params = SolverParams {
        canvas_w,
        canvas_h,
        caption_w,
        caption_h,
        safe_top,
        safe_bottom,
        safe_left,
        safe_right,
        sample_rate,
    };
    let trajectory = solve_caption_trajectory(&d.entities, &manifest, &params);
    Ok((trajectory, manifest.entity_refs))
}

/// Build a `VideoSource` from a path, falling back to loaded bytes when the
/// file does not exist on disk.
fn build_video_source(d: &crate::director::Director, path: &str) -> VideoSource {
    let p = std::path::Path::new(path);
    if p.exists() && p.is_file() {
        VideoSource::Path(p.to_path_buf())
    } else {
        let blob = d.assets.load_blob(path).unwrap_or(Arc::new(Vec::new()));
        VideoSource::Bytes((*blob).clone())
    }
}

/// Build an `ActiveSpeakerCamera` and its entity references from Rhai props.
fn build_active_speaker_camera(
    d: &mut crate::director::Director,
    path: &str,
    props: &rhai::Map,
) -> Result<(ActiveSpeakerCamera, Vec<uuid::Uuid>), Box<rhai::EvalAltResult>> {
    let mode = d.render_mode;
    let source = build_video_source(d, path);

    let groups = parse_continuity_groups(props)?;
    if groups.is_empty() {
        return Err("add_active_speaker_camera requires at least one continuity group".into());
    }

    let config = parse_active_speaker_camera_config(props);
    let mut camera = ActiveSpeakerCamera::from_source(source, mode, &d.entities, groups.clone(), config);
    if let Some(fit_str) = props
        .get("object_fit")
        .and_then(|v| v.clone().into_string().ok())
    {
        if let Some(fit) = parse_object_fit(&fit_str) {
            camera.set_object_fit(fit);
        }
    }
    if let Some(v) = props.get("border_radius").and_then(|v| v.as_float().ok()) {
        camera.set_border_radius(v as f32);
    }
    parse_layout_style(props, &mut camera.inner_mut().style);

    let entity_refs: Vec<uuid::Uuid> = groups.iter().map(|g| g.target_id).collect();
    Ok((camera, entity_refs))
}

/// Add a built `ActiveSpeakerCamera` to the scene graph and attach it to a parent.
fn attach_active_speaker_camera_node(
    d: &mut crate::director::Director,
    camera: ActiveSpeakerCamera,
    entity_refs: Vec<uuid::Uuid>,
    props: &rhai::Map,
    parent_id: NodeId,
    director: std::sync::Arc<std::sync::Mutex<crate::director::Director>>,
    parent_desc: &str,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut unique_refs: Vec<uuid::Uuid> = entity_refs.into_iter().collect::<std::collections::HashSet<_>>().into_iter().collect();
    unique_refs.sort();

    let id = d.scene.add_node(Box::new(camera));
    if let Some(n) = d.scene.get_node_mut(id) {
        n.entity_refs = unique_refs;
        n.filter = parse_filter_config(props);
    }
    if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
        if let Some(n) = d.scene.get_node_mut(id) {
            n.z_index = z as i32;
        }
    }
    if !d.add_child(parent_id, id) {
        d.scene.destroy_node(id);
        return Err(format!(
            "Failed to attach active speaker camera node {} to {}",
            id, parent_desc
        )
        .into());
    }
    Ok(NodeHandle { director, id })
}

/// Parse a framing preset string into a `Framing` value.
fn parse_framing(value: &str) -> Framing {
    match value.to_ascii_lowercase().as_str() {
        "close_up" | "closeup" => Framing::CloseUp,
        "medium" => Framing::Medium,
        "wide" => Framing::Wide,
        _ => {
            if let Ok(multiplier) = value.parse::<f32>() {
                Framing::Custom(multiplier)
            } else {
                Framing::Medium
            }
        }
    }
}

/// Parse an `ActiveSpeakerCameraConfig` from a Rhai props map.
fn parse_active_speaker_camera_config(props: &rhai::Map) -> ActiveSpeakerCameraConfig {
    let mut config = ActiveSpeakerCameraConfig::default();
    if let Some(v) = props.get("canvas_w").and_then(|v| v.as_float().ok()) {
        config.canvas_w = v as f32;
    }
    if let Some(v) = props.get("canvas_h").and_then(|v| v.as_float().ok()) {
        config.canvas_h = v as f32;
    }
    if let Some(v) = props.get("sample_rate").and_then(|v| v.as_float().ok()) {
        config.sample_rate = v;
    }
    if let Some(v) = props.get("smoothing_window").and_then(|v| v.as_int().ok()) {
        config.smoothing_window = v as usize;
    }
    if let Some(v) = props.get("center_offset_y").and_then(|v| v.as_float().ok()) {
        config.center_offset_y = v as f32;
    }
    config
}

/// Parse continuity groups for an active-speaker camera from a Rhai props map.
///
/// Supports either a single group via `entity_id`, `start_time`, `end_time`,
/// and `framing`, or an array of groups via `groups`.
fn parse_continuity_groups(props: &rhai::Map) -> Result<Vec<ContinuityGroup>, Box<rhai::EvalAltResult>> {
    let mut groups = Vec::new();

    if let Some(arr) = props.get("groups").and_then(|v| v.clone().into_array().ok()) {
        for item in arr {
            if let Some(map) = item.clone().try_cast::<rhai::Map>() {
                let entity_id = map
                    .get("entity_id")
                    .and_then(|v| v.clone().into_string().ok())
                    .ok_or_else(|| "continuity group missing entity_id".to_string())?;
                let target_id = uuid::Uuid::parse_str(&entity_id)
                    .map_err(|e| format!("invalid entity UUID: {}", e))?;
                let start_time = map.get("start_time").and_then(|v| v.as_float().ok()).unwrap_or(0.0);
                let end_time = map
                    .get("end_time")
                    .and_then(|v| v.as_float().ok())
                    .unwrap_or(start_time + 1.0);
                let framing = map
                    .get("framing")
                    .and_then(|v| v.clone().into_string().ok())
                    .map(|s| parse_framing(&s))
                    .unwrap_or(Framing::Medium);
                groups.push(ContinuityGroup::new(start_time, end_time, target_id, framing));
            }
        }
    } else if let Some(entity_id) = props.get("entity_id").and_then(|v| v.clone().into_string().ok()) {
        let target_id = uuid::Uuid::parse_str(&entity_id)
            .map_err(|e| format!("invalid entity UUID: {}", e))?;
        let start_time = props.get("start_time").and_then(|v| v.as_float().ok()).unwrap_or(0.0);
        let end_time = props
            .get("end_time")
            .and_then(|v| v.as_float().ok())
            .unwrap_or(start_time + 1.0);
        let framing = props
            .get("framing")
            .and_then(|v| v.clone().into_string().ok())
            .map(|s| parse_framing(&s))
            .unwrap_or(Framing::Medium);
        groups.push(ContinuityGroup::new(start_time, end_time, target_id, framing));
    }

    Ok(groups)
}

/// Rebuild a `TextNode` from a Rhai props map (the same vocabulary `add_text`
/// consumes), reusing the canonical text/layout parsers. Used by the project
/// save/load round-trip to faithfully restore rich text. `font_collection`
/// comes from `Director::assets`.
pub fn build_text_node_from_props(
    font_collection: Arc<std::sync::Mutex<skia_safe::textlayout::FontCollection>>,
    props: &rhai::Map,
) -> TextNode {
    let spans = if let Some(c) = props.get("content") {
        parse_spans_from_dynamic(c.clone())
    } else {
        Vec::new()
    };
    let mut text_node = TextNode::new(spans, font_collection);
    parse_layout_style(props, &mut text_node.style);
    apply_text_props(&mut text_node, props);
    text_node.shadow = parse_text_shadow(props);
    text_node.init_paragraph();
    text_node
}
