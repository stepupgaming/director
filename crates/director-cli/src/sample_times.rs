//! Inspect `--sample` times.
//! `tests/inspect_samples.rs` includes this file, so it must stay dependency-free.

/// No `--sample` keeps a single inspection at `--time`.
/// Repeated times stay in the order given.
#[derive(Debug, PartialEq)]
pub enum SamplePlan {
    Single,
    Repeated(Vec<f64>),
}

/// Empty is [`SamplePlan::Single`]. Every listed time must be finite and nonnegative.
pub fn check_sample_times(samples: &[f64]) -> Result<SamplePlan, &'static str> {
    if samples.is_empty() {
        return Ok(SamplePlan::Single);
    }
    for time in samples {
        if !time.is_finite() || *time < 0.0 {
            return Err("sample time must be finite and nonnegative");
        }
    }
    Ok(SamplePlan::Repeated(samples.to_vec()))
}
