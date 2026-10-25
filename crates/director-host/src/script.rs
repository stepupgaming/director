//! Rhai script evaluation and movie resolution.
//!
//! Flow:
//! 1. Register Rhai API + composition registry (export mode by default).
//! 2. Compile once, eval as `Dynamic`, keep the AST.
//! 3. Resolve a [`MovieHandle`]: named composition builder, or legacy eval return.

use director_core::scripting::{
    props_from_file_or_inline, register_rhai_api_with_compositions, CompositionRegistry,
    MovieHandle,
};
use director_core::video_wrapper::RenderMode;
use director_core::DefaultAssetLoader;
use rhai::{Dynamic, Engine, Map, AST};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

/// Result of evaluating a script that may register compositions and/or return a movie.
pub struct EvaluatedScript {
    pub engine: Engine,
    pub ast: AST,
    pub eval_result: Dynamic,
    pub registry: CompositionRegistry,
}

/// Compile and evaluate a Rhai script with the composition API enabled.
pub fn eval_script(script_path: &Path, mode: RenderMode) -> Result<EvaluatedScript, String> {
    let script = fs::read_to_string(script_path)
        .map_err(|e| format!("error reading script '{}': {}", script_path.display(), e))?;

    let mut engine = Engine::new();
    let registry =
        register_rhai_api_with_compositions(&mut engine, Arc::new(DefaultAssetLoader), mode);

    let ast = engine
        .compile(&script)
        .map_err(|e| format!("script compile error: {}", e))?;

    let eval_result = engine
        .eval_ast::<Dynamic>(&ast)
        .map_err(|e| format!("script error: {}", e))?;

    Ok(EvaluatedScript {
        engine,
        ast,
        eval_result,
        registry,
    })
}

/// Optional composition selection + props override for resolve.
#[derive(Default)]
pub struct ResolveOpts<'a> {
    pub composition: Option<&'a str>,
    pub props_spec: Option<&'a str>,
}

/// Resolve a movie from eval result + registry.
///
/// 1. Registry non-empty → pick named / sole composition, merge props, call builder.
/// 2. Registry empty → cast eval result to [`MovieHandle`] (legacy scripts).
pub fn resolve_movie(
    evaluated: &EvaluatedScript,
    opts: ResolveOpts<'_>,
) -> Result<MovieHandle, String> {
    let override_props: Option<Map> = match opts.props_spec {
        Some(spec) => Some(props_from_file_or_inline(spec)?),
        None => None,
    };

    if !evaluated.registry.is_empty() {
        let movie = evaluated.registry.build(
            &evaluated.engine,
            &evaluated.ast,
            opts.composition,
            override_props.as_ref(),
        )?;
        info!(
            "Resolved composition '{}'",
            opts.composition
                .map(|s| s.to_string())
                .or_else(|| evaluated.registry.sole().map(|e| e.name))
                .unwrap_or_else(|| "?".into())
        );
        return Ok(movie);
    }

    if opts.composition.is_some() {
        return Err(
            "script did not register any compositions; cannot use --composition".into(),
        );
    }
    if opts.props_spec.is_some() {
        return Err(
            "script did not register any compositions; --props requires a composition registry"
                .into(),
        );
    }

    evaluated
        .eval_result
        .clone()
        .try_cast::<MovieHandle>()
        .ok_or_else(|| {
            "script must return a Movie (legacy) or call register_composition(...)".into()
        })
}
