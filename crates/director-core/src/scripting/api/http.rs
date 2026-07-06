//! # HTTP Fetching API
//!
//! Enables Rhai scripts to pull live data from URLs.
//!
//! ## Functions
//! - `fetch_text(url)`: Downloads raw text from a URL.
//! - `fetch_json(url)`: Downloads JSON and converts it to a Rhai `Map`.
//!
//! ## Error Handling
//! Network or parsing errors are surfaced as Rhai `EvalAltResult` strings
//! so that script authors can see exactly what failed.

use rhai::{Engine, Map};
use serde_json::Value;
use std::time::Duration;

const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

/// Register HTTP-related Rhai functions.
pub fn register(engine: &mut Engine) {
    engine.register_fn("fetch_text", fetch_text);
    engine.register_fn("fetch_json", fetch_json);
}

/// Fetch raw text from a URL.
fn fetch_text(url: &str) -> Result<String, Box<rhai::EvalAltResult>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|e| format!("HTTP client build error: {}", e))?;

    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("HTTP request failed for '{}': {}", url, e))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!(
            "HTTP request failed for '{}': status {}",
            url, status
        )
        .into());
    }

    resp.text()
        .map_err(|e| format!("HTTP body read failed for '{}': {}", url, e).into())
}

/// Fetch JSON from a URL and convert it to a Rhai Map/Dynamic structure.
fn fetch_json(url: &str) -> Result<Map, Box<rhai::EvalAltResult>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|e| format!("HTTP client build error: {}", e))?;

    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("HTTP request failed for '{}': {}", url, e))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!(
            "HTTP request failed for '{}': status {}",
            url, status
        )
        .into());
    }

    let json: Value = resp
        .json()
        .map_err(|e| format!("JSON parse failed for '{}': {}", url, e))?;

    // If the root JSON is an object, return it directly as a Rhai Map.
    // Otherwise wrap the scalar/array in a map under key "data".
    if let Value::Object(obj) = &json {
        let mut map = Map::new();
        for (k, v) in obj {
            map.insert(k.clone().into(), json_value_to_dynamic(v));
        }
        Ok(map)
    } else {
        let mut map = Map::new();
        map.insert("data".into(), json_value_to_dynamic(&json));
        Ok(map)
    }
}

/// Recursively convert a `serde_json::Value` into a `rhai::Dynamic`.
fn json_value_to_dynamic(value: &Value) -> rhai::Dynamic {
    match value {
        Value::Null => rhai::Dynamic::UNIT,
        Value::Bool(b) => rhai::Dynamic::from_bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                rhai::Dynamic::from_int(i)
            } else {
                rhai::Dynamic::from_float(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => rhai::Dynamic::from(s.clone()),
        Value::Array(arr) => {
            let mut rhai_arr = rhai::Array::new();
            for item in arr {
                rhai_arr.push(json_value_to_dynamic(item));
            }
            rhai::Dynamic::from(rhai_arr)
        }
        Value::Object(obj) => {
            let mut map = Map::new();
            for (k, v) in obj {
                map.insert(k.clone().into(), json_value_to_dynamic(v));
            }
            rhai::Dynamic::from(map)
        }
    }
}
