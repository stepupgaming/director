//! # Capability Registry
//!
//! Runtime registry backed by [`crate::api_catalog`]. Introspection and JSON
//! schema generation both read the same catalog so they cannot drift.

use crate::api_catalog::{
    EASING_CATALOG, EFFECT_CATALOG, NODE_CATALOG, TRANSITION_CATALOG,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Metadata about a renderable node type.
#[derive(Debug, Clone, Serialize)]
pub struct NodeTypeInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub supports_children: bool,
    pub animatable_properties: Vec<&'static str>,
    pub features: Vec<&'static str>,
}

/// Metadata about a visual effect.
#[derive(Debug, Clone, Serialize)]
pub struct EffectInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub params: Vec<(&'static str, &'static str)>,
}

/// Metadata about a scene transition.
#[derive(Debug, Clone, Serialize)]
pub struct TransitionInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub params: Vec<(&'static str, &'static str)>,
}

/// Metadata about an easing function.
#[derive(Debug, Clone, Serialize)]
pub struct EasingInfo {
    pub name: &'static str,
    pub description: &'static str,
}

/// Global capability registry - lazily initialized.
static REGISTRY: OnceLock<CapabilityRegistry> = OnceLock::new();

/// The capability registry containing all Director engine capabilities.
pub struct CapabilityRegistry {
    pub node_types: HashMap<&'static str, NodeTypeInfo>,
    pub effects: HashMap<&'static str, EffectInfo>,
    pub transitions: HashMap<&'static str, TransitionInfo>,
    pub easings: HashMap<&'static str, EasingInfo>,
}

impl CapabilityRegistry {
    /// Get the global registry instance.
    pub fn global() -> &'static Self {
        REGISTRY.get_or_init(Self::build)
    }

    fn build() -> Self {
        let mut reg = Self {
            node_types: HashMap::new(),
            effects: HashMap::new(),
            transitions: HashMap::new(),
            easings: HashMap::new(),
        };

        for node in NODE_CATALOG {
            reg.node_types.insert(
                node.name,
                NodeTypeInfo {
                    name: node.name,
                    description: node.description,
                    supports_children: node.supports_children,
                    animatable_properties: node.animatable_properties.to_vec(),
                    features: node.features.to_vec(),
                },
            );
        }

        for effect in EFFECT_CATALOG {
            reg.effects.insert(
                effect.name,
                EffectInfo {
                    name: effect.name,
                    description: effect.description,
                    params: effect.params.to_vec(),
                },
            );
        }

        for transition in TRANSITION_CATALOG {
            reg.transitions.insert(
                transition.name,
                TransitionInfo {
                    name: transition.name,
                    description: transition.description,
                    params: transition.params.to_vec(),
                },
            );
        }

        for easing in EASING_CATALOG {
            reg.easings.insert(
                easing.name,
                EasingInfo {
                    name: easing.name,
                    description: easing.description,
                },
            );
        }

        reg
    }
}

// ============ Public API ============

pub use crate::api_catalog::{api_manifest, api_manifest_json, ApiManifest};

/// List all registered node types.
pub fn list_node_types() -> Vec<&'static NodeTypeInfo> {
    CapabilityRegistry::global().node_types.values().collect()
}

/// Get info for a specific node type.
pub fn get_node_type(name: &str) -> Option<&'static NodeTypeInfo> {
    CapabilityRegistry::global().node_types.get(name)
}

/// List all registered effects.
pub fn list_effects() -> Vec<&'static EffectInfo> {
    CapabilityRegistry::global().effects.values().collect()
}

/// Get info for a specific effect.
pub fn get_effect(name: &str) -> Option<&'static EffectInfo> {
    CapabilityRegistry::global().effects.get(name)
}

/// List all registered transitions.
pub fn list_transitions() -> Vec<&'static TransitionInfo> {
    CapabilityRegistry::global().transitions.values().collect()
}

/// List all registered easings.
pub fn list_easings() -> Vec<&'static EasingInfo> {
    CapabilityRegistry::global().easings.values().collect()
}

/// Check if a node type supports animation of a specific property.
pub fn supports_animation(node_type: &str, property: &str) -> bool {
    if let Some(info) = get_node_type(node_type) {
        info.animatable_properties.contains(&property)
    } else {
        false
    }
}

/// Get all animatable properties for a node type.
pub fn get_animatable_properties(node_type: &str) -> Vec<&'static str> {
    if let Some(info) = get_node_type(node_type) {
        info.animatable_properties.clone()
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_catalog::{
        catalog_effect_schema_tags, catalog_transition_names, API_MANIFEST_VERSION,
    };

    #[test]
    fn test_registry_initializes() {
        let registry = CapabilityRegistry::global();
        assert_eq!(registry.node_types.len(), NODE_CATALOG.len());
        assert_eq!(registry.effects.len(), EFFECT_CATALOG.len());
        assert_eq!(registry.transitions.len(), TRANSITION_CATALOG.len());
        assert_eq!(registry.easings.len(), EASING_CATALOG.len());
    }

    #[test]
    fn test_registry_matches_catalog_counts() {
        assert_eq!(list_effects().len(), EFFECT_CATALOG.len());
        assert_eq!(list_transitions().len(), TRANSITION_CATALOG.len());
        let names: Vec<_> = list_transitions().iter().map(|t| t.name).collect();
        for expected in catalog_transition_names() {
            assert!(names.contains(&expected), "missing transition {expected}");
        }
        let tags = catalog_effect_schema_tags();
        assert!(tags.contains(&"grain"));
        assert!(tags.contains(&"sksl_shader"));
    }

    #[test]
    fn test_supports_animation() {
        assert!(supports_animation("Box", "opacity"));
        assert!(supports_animation("Text", "font_size"));
        assert!(!supports_animation("Image", "border_radius"));
    }

    #[test]
    fn test_api_manifest_json_roundtrip() {
        let json = api_manifest_json();
        let v: serde_json::Value = serde_json::from_str(&json).expect("manifest json");
        assert_eq!(v["version"], API_MANIFEST_VERSION);
        assert!(v["effects"].as_array().unwrap().len() >= 30);

        // rhai_functions entries should include the params array in the JSON output.
        let rhai_functions = v["rhai_functions"].as_array().expect("rhai_functions array");
        let compiler_fn = rhai_functions
            .iter()
            .find(|f| f["name"] == "compiler_affected_nodes")
            .expect("compiler_affected_nodes in manifest");
        let params = compiler_fn["params"].as_array().expect("params array");
        assert!(!params.is_empty(), "compiler_affected_nodes params should be serialized");
        let first_param = params[0].as_array().expect("param tuple as array");
        assert_eq!(first_param[0], "inputs");
        assert_eq!(first_param[1], "Array");
    }

    #[test]
    fn test_api_manifest_includes_compiler_introspection_functions() {
        let manifest = api_manifest();
        let by_name: std::collections::HashMap<_, _> =
            manifest.rhai_functions.iter().map(|f| (f.name, f)).collect();

        assert!(by_name.contains_key("compiler_inputs_for_node"));
        assert!(by_name.contains_key("compiler_dependents_of_entity"));
        assert!(by_name.contains_key("compiler_dependents_of_audio_track"));
        assert!(by_name.contains_key("compiler_dependents_of_node"));
        assert!(by_name.contains_key("compiler_graph_json"));
        assert!(by_name.contains_key("compiler_affected_nodes"));

        // Functions that take parameters should report them in the manifest.
        assert_eq!(by_name["compiler_inputs_for_node"].params, &[("node_id", "Integer")]);
        assert_eq!(by_name["compiler_dependents_of_entity"].params, &[("entity_id", "String")]);
        assert_eq!(
            by_name["compiler_dependents_of_audio_track"].params,
            &[("track_id", "Integer")]
        );
        assert_eq!(by_name["compiler_dependents_of_node"].params, &[("node_id", "Integer")]);
        assert!(by_name["compiler_graph_json"].params.is_empty());
        assert_eq!(by_name["compiler_affected_nodes"].params, &[("inputs", "Array")]);
    }
}