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
use crate::element::TextFit;
use crate::node::{BoxNode, BoxShadow, CaptionLine, CaptionNode, CaptionPosition, CaptionWord, CompositionNode, EffectNode, ImageNode, LottieNode, PathCommand, ShapeNode, ShapeType, TextNode, VectorNode, VideoNode, VideoSource};
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
            d.scene.destroy_node(node.id);
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
            if !d.scene.try_add_child(parent.id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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

            let img_node = ImageNode::from_image(image);
            let id = d.scene.add_node(Box::new(img_node));
            if !d.scene.try_add_child(parent.id, id) {
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

            let img_node = ImageNode::from_image(image);
            let id = d.scene.add_node(Box::new(img_node));
            if !d.scene.try_add_child(scene.root_id, id) {
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

            let mut img_node = ImageNode::from_image(image);
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
            if !d.scene.try_add_child(scene.root_id, id) {
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

            let mut img_node = ImageNode::from_image(image);
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
            if !d.scene.try_add_child(parent.id, id) {
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
                    if !d.scene.try_add_child(parent.id, id) {
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
                    if !d.scene.try_add_child(parent.id, id) {
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
                    if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(parent.id, id) {
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
            if !d.scene.try_add_child(parent.id, id) {
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
            let p = std::path::Path::new(path);

            let source = if p.exists() && p.is_file() {
                VideoSource::Path(p.to_path_buf())
            } else {
                let blob = d.assets.load_blob(path).unwrap_or(Arc::new(Vec::new()));
                VideoSource::Bytes((*blob).clone())
            };

            let vid_node = VideoNode::new(source, mode);
            let id = d.scene.add_node(Box::new(vid_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = FilterConfig::default();
            }
            if !d.scene.try_add_child(parent.id, id) {
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
            let p = std::path::Path::new(path);

            let source = if p.exists() && p.is_file() {
                VideoSource::Path(p.to_path_buf())
            } else {
                let blob = d.assets.load_blob(path).unwrap_or(Arc::new(Vec::new()));
                VideoSource::Bytes((*blob).clone())
            };

            let mut vid_node = VideoNode::new(source, mode);
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
            if !d.scene.try_add_child(parent.id, id) {
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
            let p = std::path::Path::new(path);

            let source = if p.exists() && p.is_file() {
                VideoSource::Path(p.to_path_buf())
            } else {
                let blob = d.assets.load_blob(path).unwrap_or(Arc::new(Vec::new()));
                VideoSource::Bytes((*blob).clone())
            };

            let vid_node = VideoNode::new(source, mode);
            let id = d.scene.add_node(Box::new(vid_node));
            if let Some(n) = d.scene.get_node_mut(id) {
                n.filter = FilterConfig::default();
            }
            if !d.scene.try_add_child(scene.root_id, id) {
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
            let p = std::path::Path::new(path);

            let source = if p.exists() && p.is_file() {
                VideoSource::Path(p.to_path_buf())
            } else {
                let blob = d.assets.load_blob(path).unwrap_or(Arc::new(Vec::new()));
                VideoSource::Bytes((*blob).clone())
            };

            let mut vid_node = VideoNode::new(source, mode);
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(parent.id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(parent.id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(parent.id, id) {
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
            if !d.scene.try_add_child(parent.id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(parent.id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(parent.id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
            if !d.scene.try_add_child(parent.id, id) {
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
            if !d.scene.try_add_child(scene.root_id, id) {
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
                if !d.scene.try_add_child(parent.id, id) {
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
                if !d.scene.try_add_child(parent.id, id) {
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
    if let Some(c) = props.get("background_color").and_then(|v| v.clone().into_string().ok()) {
        if let Some(col) = parse_hex_color(&c) {
            node.style.background_color = col;
        }
    }
    if let Some(p) = props.get("background_padding").and_then(|v| v.as_float().ok()) {
        node.style.background_padding = p as f32;
    }
    if let Some(p) = props.get("caption_position").and_then(|v| v.clone().into_string().ok()) {
        node.style.position = CaptionPosition::from_str(&p);
    }
    if let Some(a) = props.get("align").and_then(|v| v.clone().into_string().ok()) {
        node.style.text_align = match a.as_str() {
            "center" => skia_safe::textlayout::TextAlign::Center,
            "right" => skia_safe::textlayout::TextAlign::Right,
            "left" => skia_safe::textlayout::TextAlign::Left,
            _ => skia_safe::textlayout::TextAlign::Center,
        };
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
    if let Some(s) = props.get("line_spacing").and_then(|v| v.as_float().ok()) {
        node.style.line_spacing = s as f32;
    }
    if let Some(d) = props.get("word_transition_duration").and_then(|v| v.as_float().ok()) {
        node.style.word_transition_duration = d;
    }
    if let Some(d) = props.get("fade_in_duration").and_then(|v| v.as_float().ok()) {
        node.style.fade_in_duration = d;
    }
    if let Some(d) = props.get("fade_out_duration").and_then(|v| v.as_float().ok()) {
        node.style.fade_out_duration = d;
    }
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
}
