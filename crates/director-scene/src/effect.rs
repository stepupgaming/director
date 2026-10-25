//! Visual effects, named by the `director-core` effect catalog.

use crate::channel::{Channel, Rgba};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One effect in an effect node's stack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Effect {
    /// Catalog name, such as `blur` or `vignette`.
    pub kind: String,
    /// Parameters by catalog name.
    pub params: BTreeMap<String, EffectParam>,
}

/// An effect parameter value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectParam {
    /// An animated scalar.
    Scalar(Channel),
    /// A fixed color.
    Color(Rgba),
    /// An animated color.
    ColorChannel(Channel<Rgba>),
    /// A whole number, such as a sample count.
    Count(u32),
    /// A list of numbers, such as a 4x5 color matrix.
    Numbers(Vec<f32>),
    /// Source text, such as SkSL.
    Text(String),
    /// Shader uniforms by name.
    Uniforms(BTreeMap<String, Uniform>),
}

/// A runtime-shader uniform.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Uniform {
    Float(Channel),
    Vector(Channel<Vec<f32>>),
}
