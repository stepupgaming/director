#!/usr/bin/env python3
"""Record and compare frame-pipeline timings paired with exact RGBA frames."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import socket
import subprocess
import sys
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[3]
BENCH = Path("crates/director-core/benches/layout_snapshot_bench.rs")
SCHEMA_VERSION = 1


class TrackingError(Exception):
    pass


def read_json(path):
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(data, dict):
            raise TrackingError(f"Expected a JSON object in {path}")
        return data
    except (OSError, ValueError) as error:
        raise TrackingError(f"Cannot read {path}: {error}") from error


def write_json(path, data):
    path.write_text(json.dumps(data, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def command_text(command):
    try:
        return subprocess.check_output(command, cwd=ROOT, text=True).strip()
    except (OSError, subprocess.CalledProcessError) as error:
        raise TrackingError(f"Cannot run {' '.join(command)}: {error}") from error


def environment():
    return {
        "hostname": socket.gethostname(),
        "os": platform.system(),
        "os_release": platform.release(),
        "architecture": platform.machine(),
        "processor": platform.processor(),
        "logical_cpus": os.cpu_count(),
        "rustc": command_text(["rustc", "-Vv"]),
        "cargo": command_text(["cargo", "-V"]),
        "build_environment": {name: value for name, value in sorted(os.environ.items())
                              if name.startswith("CARGO_PROFILE_") or name in [
                                  "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET",
                                  "RAYON_NUM_THREADS", "SKIA_BINARIES_URL",
                              ]},
    }


def positive(value, label):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value <= 0:
        raise TrackingError(f"Invalid positive number for {label}: {value}")
    return value


def validate_timing(timing, label):
    point = positive(timing.get("point_ns"), label)
    low = positive(timing.get("lower_ns"), label)
    high = positive(timing.get("upper_ns"), label)
    if not low <= point <= high or timing.get("estimator") not in ["slope", "mean"]:
        raise TrackingError(f"Invalid timing interval for {label}")


def load_bundle(directory):
    manifest = read_json(directory / "manifest.json")
    if manifest.get("schema_version") != SCHEMA_VERSION:
        raise TrackingError(f"Unsupported baseline schema in {directory}")
    suite = manifest.get("suite", {})
    if not isinstance(suite, dict) or not isinstance(manifest.get("environment"), dict):
        raise TrackingError(f"Invalid suite/environment metadata in {directory}")
    if not isinstance(manifest.get("frames"), dict) or not isinstance(manifest.get("timings"), dict):
        raise TrackingError(f"Invalid frame/timing records in {directory}")
    width = positive(suite.get("width"), "width")
    height = positive(suite.get("height"), "height")
    workloads = suite.get("workloads", [])
    phases = suite.get("phases", [])
    times = suite.get("times", [])
    if not workloads or not phases or not times:
        raise TrackingError(f"Empty benchmark suite in {directory}")
    if not manifest.get("environment") or not manifest.get("benchmark_sha256"):
        raise TrackingError(f"Missing compatibility metadata in {directory}")
    names = [w["name"] for w in workloads]
    if any(not isinstance(n, str) or not n or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789_" for c in n)
           for n in names) or len(set(names)) != len(names):
        raise TrackingError(f"Invalid workload names in {directory}")
    if len(set(phases)) != len(phases) or any(p not in ["update", "layout", "paint", "whole_frame"] for p in phases):
        raise TrackingError(f"Invalid benchmark phases in {directory}")
    expected_metrics = {f"frame_pipeline/{w['name']}/{p}" for w in workloads for p in phases}
    expected_frames = {f"{w['name']}_{i}.rgba" for w in workloads for i in range(len(times))}
    if set(manifest.get("timings", {})) != expected_metrics:
        raise TrackingError(f"Missing or unexpected timing metrics in {directory}")
    if set(manifest.get("frames", {})) != expected_frames:
        raise TrackingError(f"Missing or unexpected frame records in {directory}")
    pixels = directory / "pixels"
    if {p.name for p in pixels.glob("*.rgba")} != expected_frames:
        raise TrackingError(f"Missing or unexpected RGBA files in {pixels}")
    for name, record in manifest["frames"].items():
        if not isinstance(record, dict):
            raise TrackingError(f"Invalid frame record for {name}")
        path = pixels / name
        size = path.stat().st_size
        if size != width * height * 4 or record.get("bytes") != size:
            raise TrackingError(f"Wrong frame size: {path}")
        if digest(path) != record.get("sha256"):
            raise TrackingError(f"Frame integrity check failed: {path}")
    for name, timing in manifest["timings"].items():
        if not isinstance(timing, dict):
            raise TrackingError(f"Invalid timing record for {name}")
        validate_timing(timing, name)
    return manifest


def capture(output):
    # Never reuse a directory: stale Criterion output cannot become a new run.
    output.mkdir(parents=True, exist_ok=False)
    pixels = output / "pixels"
    criterion = output / "criterion"
    env = os.environ.copy()
    env["DIRECTOR_PROFILE_PIXELS"] = str(pixels)
    env["CRITERION_HOME"] = str(criterion)
    env.pop("CARGO_CRITERION_PORT", None)
    command = ["cargo", "bench", "--locked", "-p", "director-core", "--bench",
               "layout_snapshot_bench", "--", "frame_pipeline", "--noplot"]
    env_info = environment()
    bench_hash = digest(ROOT / BENCH)
    started = datetime.now(timezone.utc).isoformat()
    print(f"Running frame benchmarks. Log: {output / 'bench.log'}", flush=True)
    with (output / "bench.log").open("w", encoding="utf-8") as log:
        try:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        except OSError as error:
            raise TrackingError(f"Cannot start Cargo: {error}") from error
    if result.returncode:
        raise TrackingError(f"Cargo benchmark failed with exit {result.returncode}; see {output / 'bench.log'}")
    if digest(ROOT / BENCH) != bench_hash:
        raise TrackingError("Benchmark source changed during capture; discard this run")
    suite = read_json(pixels / "suite.json")
    timings = {}
    for path in sorted(criterion.glob("**/new/benchmark.json")):
        benchmark = read_json(path)
        name = benchmark["full_id"]
        if not name.startswith("frame_pipeline/"):
            continue
        estimates = read_json(path.with_name("estimates.json"))
        estimator = "slope" if estimates.get("slope") else "mean"
        estimate = estimates[estimator]
        interval = estimate["confidence_interval"]
        if interval["confidence_level"] != 0.95:
            raise TrackingError(f"Expected a 95% timing interval for {name}")
        timings[name] = {"estimator": estimator, "point_ns": estimate["point_estimate"],
                         "lower_ns": interval["lower_bound"], "upper_ns": interval["upper_bound"]}
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "created_at": started,
        "environment": env_info,
        "benchmark_sha256": bench_hash,
        "suite": suite,
        "source": {"commit": command_text(["git", "rev-parse", "HEAD"]),
                   "status": command_text(["git", "status", "--short"]),
                   "cargo_lock_sha256": digest(ROOT / "Cargo.lock")},
        "command": command,
        "timings": timings,
        "frames": {p.name: {"bytes": p.stat().st_size, "sha256": digest(p)}
                   for p in sorted(pixels.glob("*.rgba"))},
    }
    write_json(output / "manifest.json", manifest)
    load_bundle(output)
    return manifest


def compare(baseline, current, max_regression):
    if not math.isfinite(max_regression) or max_regression < 0:
        raise TrackingError("Regression threshold must be a finite non-negative percentage")
    before = load_bundle(baseline)
    after = load_bundle(current)
    for key in ["environment", "benchmark_sha256", "suite"]:
        if before.get(key) != after.get(key):
            raise TrackingError(f"Incompatible {key}; record a fresh baseline on this machine/setup")
    frames = []
    for name in sorted(before["frames"]):
        a = (baseline / "pixels" / name).read_bytes()
        b = (current / "pixels" / name).read_bytes()
        same = a == b  # Compare bytes, not only checksums.
        frames.append({"name": name, "equal": same,
                       "differing_channels": 0 if same else sum(x != y for x, y in zip(a, b))})
    timings = []
    factor = 1 + max_regression / 100
    for name, a in sorted(before["timings"].items()):
        b = after["timings"][name]
        if a["estimator"] != b["estimator"]:
            raise TrackingError(f"Incompatible timing estimator for {name}")
        change = (b["point_ns"] / a["point_ns"] - 1) * 100
        regression = b["lower_ns"] > a["upper_ns"] * factor
        status = "regression" if regression else ("noisy" if change > max_regression else "pass")
        timings.append({"name": name, "before_ns": a["point_ns"], "after_ns": b["point_ns"],
                        "change_percent": change, "status": status})
    failed = any(not f["equal"] for f in frames) or any(t["status"] == "regression" for t in timings)
    noisy = any(t["status"] == "noisy" for t in timings)
    status = "fail" if failed else ("inconclusive" if noisy else "pass")
    return {"schema_version": SCHEMA_VERSION, "status": status,
            "baseline": str(baseline), "current": str(current),
            "max_regression_percent": max_regression, "frames": frames, "timings": timings}


def save_report(output, report):
    write_json(output / "comparison.json", report)
    equal = sum(frame["equal"] for frame in report["frames"])
    lines = ["# Frame pipeline regression check", "", f"Result: {report['status'].upper()}", "",
             f"Exact RGBA frames: {equal}/{len(report['frames'])} identical.", "",
             f"Timing allowance: {report['max_regression_percent']:g}%. A regression fails only when the current",
             "95% interval lower bound exceeds the baseline upper bound plus the allowance.", "",
             "| Metric | Baseline, us | Current, us | Change | Status |",
             "|---|---:|---:|---:|---|"]
    for item in report["timings"]:
        lines.append(f"| {item['name']} | {item['before_ns']/1000:.2f} | {item['after_ns']/1000:.2f} | "
                     f"{item['change_percent']:+.1f}% | {item['status']} |")
    for frame in report["frames"]:
        if not frame["equal"]:
            lines.extend(["", f"Pixel mismatch: {frame['name']}, {frame['differing_channels']} differing channels."])
    lines.extend(["", "Noisy metrics require a repeat run; they are not evidence of equivalence.", ""])
    (output / "comparison.md").write_text("\n".join(lines), encoding="utf-8")
    print(f"{report['status'].upper()}: {equal}/{len(report['frames'])} exact frames; "
          f"report: {output / 'comparison.md'}")
    for item in report["timings"]:
        if item["status"] != "pass":
            print(f"  {item['status']}: {item['name']} {item['change_percent']:+.1f}%")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    record = commands.add_parser("record", help="Capture a new baseline; existing directories are refused")
    record.add_argument("--output", type=Path, required=True)
    check = commands.add_parser("check", help="Capture current output and compare it with a baseline")
    check.add_argument("--baseline", type=Path, required=True)
    check.add_argument("--output", type=Path, required=True)
    offline = commands.add_parser("compare", help="Compare two captured bundles without running Cargo")
    offline.add_argument("--baseline", type=Path, required=True)
    offline.add_argument("--current", type=Path, required=True)
    offline.add_argument("--output", type=Path, required=True)
    for command in [check, offline]:
        command.add_argument("--max-regression-percent", type=float, default=15.0)
    args = parser.parse_args(argv)
    try:
        output = args.output.resolve()
        if output.exists():
            raise TrackingError(f"Output already exists: {output}. Choose a new run name; nothing was overwritten.")
        if args.command == "record":
            capture(output)
            print(f"Recorded baseline: {output / 'manifest.json'}")
            return 0
        baseline = args.baseline.resolve()
        load_bundle(baseline)
        if not math.isfinite(args.max_regression_percent) or args.max_regression_percent < 0:
            raise TrackingError("Regression threshold must be a finite non-negative percentage")
        if args.command == "check":
            current = output
            capture(current)
        else:
            current = args.current.resolve()
        report = compare(baseline, current, args.max_regression_percent)
        if args.command == "compare":
            output.mkdir(parents=True, exist_ok=False)
        save_report(output, report)
        return {"pass": 0, "fail": 1, "inconclusive": 3}[report["status"]]
    except (TrackingError, OSError, KeyError, TypeError, ValueError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
