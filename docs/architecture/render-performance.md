# Render performance measurement

## Scope and environment

Measured on Windows, AMD Ryzen 9 9900X, 12 cores / 24 logical processors, Rust 1.97.0, Cargo's optimized bench profile, CPU Skia raster rendering. No FFmpeg, fonts, GPU, encoding, or frame readback is included in timing.

The baseline is the local working tree immediately before the layout synchronization fix, including the earlier decoder and caption changes. This is not a comparison against an older Git commit.

Three 640x360 scenes exercise the export-facing frame loop with a persistent `LayoutEngine` (today `director_render::FrameState`):

- `cards_144`: a 12x12 grid of colored boxes, 157 total nodes.
- `animated_1024`: a 32x32 grid, 1057 total nodes, each card translating linearly by 3 pixels over 3 seconds.
- `static_cached_1024`: the same dense grid with 32 row subtrees marked static.

Criterion uses 20 samples, 1 second warmup and 3 seconds requested measurement per benchmark. Update, layout and paint are timed separately after their prerequisites; whole-frame timing includes the actual update/layout/post-layout/paint pipeline. Phase measurements are separate runs and should not be summed as an exact whole-frame estimate. Timing overhead is included equally before and after. Frames cycle through 0 to 89 at 30 FPS.

## Diagnosis and fix

Taffy 0.9.2 `set_children` always rewires relationships and marks the parent dirty. Director called it on every node every frame, including empty and unchanged child lists. Paint-only box updates also caused unconditional `set_style`, which marks layout dirty even when the style is identical.

Dense-scene baseline layout cost was 449 to 462 microseconds per frame. Static-subtree painting cost 584 microseconds versus 370 microseconds for ordinary painting. Although static painting was the largest absolute phase, its additional cost was about 214 microseconds. Avoidable layout invalidation was a larger shared opportunity and did not require changing static-cache semantics.

`systems/layout.rs` now compares existing child iterators with the desired ordered children and optional mask before allocating or calling `set_children`. It compares styles before `set_style`. Dirty intrinsic-size nodes still call `mark_dirty` when their style is unchanged, so content-only edits cannot reuse stale measurements.

Static-cache behavior is unchanged. Full render passes still re-rasterize static subtrees; fixing that safely requires accounting for subtree pixels, layout, inherited opacity and time-dependent state.

## Results

The table uses Criterion slope point estimates in microseconds. The first optimized run and a second confirmation run both improved layout and whole-frame timing. Percentages below compare the baseline with the confirmation run.

| Workload | Phase | Before, us | After, us | Time reduction |
|---|---|---:|---:|---:|
| 144 cards | Update | 38.58 | 29.39 | 23.8% |
| 144 cards | Layout | 88.43 | 38.76 | 56.2% |
| 144 cards | Paint | 81.27 | 80.03 | 1.5% |
| 144 cards | Whole frame | 354.91 | 256.20 | 27.8% |
| 1024 animated cards | Update | 153.45 | 146.92 | 4.3% |
| 1024 animated cards | Layout | 461.64 | 179.47 | 61.1% |
| 1024 animated cards | Paint | 370.33 | 390.42 | -5.4% |
| 1024 animated cards | Whole frame | 998.76 | 737.69 | 26.1% |
| 1024 static-cached cards | Update | 153.58 | 150.37 | 2.1% |
| 1024 static-cached cards | Layout | 448.63 | 185.04 | 58.8% |
| 1024 static-cached cards | Paint | 584.16 | 588.07 | -0.7% |
| 1024 static-cached cards | Whole frame | 1224.34 | 963.85 | 21.3% |

First-run whole-frame reductions were 23.1%, 23.7%, and 19.7%, respectively. Confirmation reductions were 27.8%, 26.1%, and 21.3%. Use those ranges rather than treating one run as a machine-independent guarantee.

Paint was not changed. The animated paint point estimate rose by 5.4%; Criterion's confirmation change analysis classified it within its noise threshold. Static paint showed no statistically detected change. No paint speedup is claimed. The smaller update phase also varies with scheduling and was not optimized by this patch.

## Correctness

- Captured frames at 0, 0.5, 1, 2, and 3 seconds for all three workloads before and after the fix.
- All 15 RGBA files matched byte-for-byte, each exactly 921600 bytes, with zero differing channels.
- The layout unit regression confirms unchanged hierarchy and paint-only updates reuse intrinsic measurements, while content-only edits still change measured width.
- `persistent_layout_matches_fresh_layout_after_edits` compares exact pixels and rectangles after child reversal, size edits, reparenting, mask addition/removal, destroy plus generational slot reuse, viewport resize, and transform animation.
- Existing layout and pipeline equivalence tests remain unchanged in their assertions.

Verification completed on the final source:

- 176 core unit tests, 3 layout tests, and 8 pipeline-equivalence tests passed.
- 2 composition tests, 2 masking tests, and 5 typography tests passed.
- `cargo check --workspace --all-targets` passed with existing warnings.
- Both optimized benchmark runs completed successfully.
- `cargo clippy -p director-core --all-targets -- -D warnings` failed on existing lint debt, including `LayoutEngine::new` without `Default`, unused imports/results and Skia `Arc` thread-safety warnings. No suppressions were added.

The raw frame comparison covers these box scenes, not every possible node. Full workspace tests and GPU performance were not measured. Existing caption snapshot/font-loading failures are a separate limitation; their baselines and tolerances were not changed.

## Regression tracking

The dependency-free tracker at `crates/director-core/benches/frame_regression.py` pairs performance gates with exact pixel checks. Use a quiet machine and the same build settings for both runs.

```bash
# Capture an immutable baseline, then change the engine.
python -B crates/director-core/benches/frame_regression.py record \
  --output target/frame-regression/baseline

# Capture current behavior and compare with the baseline.
python -B crates/director-core/benches/frame_regression.py check \
  --baseline target/frame-regression/baseline \
  --output target/frame-regression/check-001

# Re-evaluate saved captures without rerunning Cargo.
python -B crates/director-core/benches/frame_regression.py compare \
  --baseline target/frame-regression/baseline \
  --current target/frame-regression/check-001 \
  --output target/frame-regression/report-001 \
  --max-regression-percent 15
```

Output paths are resolved from the caller's working directory. Cargo always runs from the repository root. Every output directory must be new; the tool never deletes or overwrites a baseline or an earlier report. Interrupted/failed captures retain their log but do not become usable baselines. Choose a new run name on retry.

Capture bundles contain:

- `manifest.json`, schema version 1, including machine/toolchain/build settings, benchmark SHA-256, suite metadata, source revision/status, lockfile digest, frame sizes/hashes, and 12 timing estimates in nanoseconds.
- `pixels/suite.json` and 15 raw 640x360 RGBA8888 premultiplied sRGB frames, five per workload.
- `criterion/` with isolated raw Criterion output, and `bench.log` with the actual Cargo output.
- Checks also produce `comparison.json` and `comparison.md` listing exact-pixel and per-phase timing results.

The tracker validates complete metric/frame sets, expected byte sizes, and stored frame hashes before comparison. It then compares frame bytes directly with zero tolerance. Machine identity, OS/architecture, toolchain, selected build environment, benchmark source digest, and suite metadata must match. Engine revision/status and the lockfile digest are recorded as provenance, not compatibility restrictions, so engine/dependency changes can be evaluated. Cargo uses `--locked` and the default optimized CPU bench configuration. Changing workloads or benchmark settings requires a fresh baseline, never an automatic update.

The default timing allowance is 15%. A timing metric fails when its current 95% confidence lower bound exceeds the baseline upper bound multiplied by 1.15. A point estimate beyond the allowance with overlapping intervals is inconclusive and requires another run. This interval rule is a conservative gate, not Criterion's hypothesis test; it does not remove interference from other processes, thermal state, or system load.

| Exit | Meaning |
|---:|---|
| 0 | Exact pixels and timing checks pass |
| 1 | Any pixel mismatch or confident timing regression |
| 2 | Cargo failure, invalid/incomplete capture, incompatible setup, or existing output |
| 3 | No definite failure, but at least one noisy timing exceeds the point-estimate allowance |

Pixel failures are independent of timing thresholds. Never raise the threshold or replace a baseline to dismiss an unexplained failure. Review intended pixel changes explicitly and record a separately named baseline only after approval.

Tracker tests, requiring no additional Python packages:

```bash
python -B -m unittest discover -s crates/director-core/benches -p 'test_*.py'
```

Tracking verification on the local Windows machine:

- All 18 tracker unit/CLI tests passed, including raw Criterion capture and rejection of non-95% intervals.
- `cargo check --workspace --all-targets` passed after the benchmark metadata change.
- A real `record` produced the complete 12-metric, 15-frame baseline.
- Two real `check` runs matched all 15 frames exactly. Both returned exit 3 for noisy timing metrics at the unchanged 15% allowance, not a timing pass. The baseline was not replaced.
- Actual offline CLI comparisons returned exit 0 for identical bundles, exit 1 for a one-channel pixel change, exit 1 for a doubled timing estimate, exit 3 for an overlapping-interval slowdown, and exit 2 for a mismatched machine identity.

Tracking is local and opt-in. This change does not enable automatic GitHub Actions or compare timings across machines. The workloads remain box-based CPU raster scenes; they do not certify all node types or GPU behavior.

## Reproduce

From the workspace root, before changing the engine:

```bash
DIRECTOR_PROFILE_PIXELS="$PWD/target/profile-before" \
  cargo bench -p director-core --bench layout_snapshot_bench -- \
  frame_pipeline --save-baseline before
```

After the change:

```bash
DIRECTOR_PROFILE_PIXELS="$PWD/target/profile-after" \
  cargo bench -p director-core --bench layout_snapshot_bench -- \
  frame_pipeline --baseline before

cargo bench -p director-core --bench layout_snapshot_bench -- \
  frame_pipeline --baseline before
```

On Git Bash, use a Windows-compatible output path such as `C:/Projects/director/target/profile-before`. Pixel generation occurs outside timed iterations. Do not overwrite the baseline before comparing.

Exact frame comparison, using the existing Python installation without extra packages:

```bash
python - <<'PY'
from pathlib import Path
before = Path('target/profile-before')
after = Path('target/profile-after')
files = sorted(before.glob('*.rgba'))
assert len(files) == 15
assert {p.name for p in files} == {p.name for p in after.glob('*.rgba')}
for src in files:
    a = src.read_bytes()
    b = (after / src.name).read_bytes()
    assert len(a) == len(b) == 640 * 360 * 4
    assert a == b, src.name
print('15 frames matched byte-for-byte')
PY
```

Correctness and build checks:

```bash
cargo test -p director-core --release --lib --test integration -- layout:: pipeline_equivalence::
cargo check --workspace --all-targets
```

Raw logs, frames, and Criterion estimates live under ignored `target/`. Only the benchmark, regressions, engine fix and this summary are durable source.
