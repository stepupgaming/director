"""Run with python -m unittest discover -s crates/director-core/benches -p 'test_*.py'."""

import contextlib
import copy
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import frame_regression as tracker


class FrameRegressionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.before = self.root / "before"
        self.after = self.root / "after"
        self.make_bundle(self.before)
        self.make_bundle(self.after)

    def make_bundle(self, directory):
        pixels = directory / "pixels"
        pixels.mkdir(parents=True)
        frame = pixels / "cards_0.rgba"
        frame.write_bytes(bytes([10, 20, 30, 255]))
        manifest = {
            "schema_version": 1,
            "environment": {"hostname": "test-machine", "rustc": "test-toolchain"},
            "benchmark_sha256": "a" * 64,
            "suite": {"width": 1, "height": 1, "times": [0.0], "phases": ["whole_frame"],
                      "workloads": [{"name": "cards"}]},
            "timings": {"frame_pipeline/cards/whole_frame": {
                "estimator": "slope", "point_ns": 100, "lower_ns": 99, "upper_ns": 101}},
            "frames": {frame.name: {"bytes": 4, "sha256": tracker.digest(frame)}},
        }
        tracker.write_json(directory / "manifest.json", manifest)

    def edit(self, directory, mutate):
        manifest = tracker.read_json(directory / "manifest.json")
        mutate(manifest)
        tracker.write_json(directory / "manifest.json", manifest)

    def invoke(self, *args):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return tracker.main(list(map(str, args)))

    def offline(self, output, threshold=15):
        return self.invoke("compare", "--baseline", self.before, "--current", self.after,
                           "--output", output, "--max-regression-percent", threshold)

    def test_identical_bundles_pass_and_write_reports(self):
        output = self.root / "report"
        self.assertEqual(self.offline(output), 0)
        report = tracker.read_json(output / "comparison.json")
        self.assertEqual(report["status"], "pass")
        self.assertEqual(report["frames"][0]["differing_channels"], 0)
        self.assertIn("1/1 identical", (output / "comparison.md").read_text())

    def test_one_byte_pixel_change_fails_independently_of_timings(self):
        frame = self.after / "pixels/cards_0.rgba"
        frame.write_bytes(bytes([11, 20, 30, 255]))
        self.edit(self.after, lambda m: m["frames"][frame.name].update(sha256=tracker.digest(frame)))
        output = self.root / "pixel-report"
        self.assertEqual(self.offline(output), 1)
        report = tracker.read_json(output / "comparison.json")
        self.assertEqual(report["frames"][0]["differing_channels"], 1)
        self.assertEqual(report["timings"][0]["status"], "pass")

    def test_significant_timing_regression_fails(self):
        self.edit(self.after, lambda m: m["timings"]["frame_pipeline/cards/whole_frame"].update(
            point_ns=130, lower_ns=129, upper_ns=131))
        output = self.root / "slow-report"
        self.assertEqual(self.offline(output), 1)
        self.assertEqual(tracker.read_json(output / "comparison.json")["timings"][0]["status"], "regression")

    def test_configured_threshold_applies(self):
        self.edit(self.after, lambda m: m["timings"]["frame_pipeline/cards/whole_frame"].update(
            point_ns=120, lower_ns=119, upper_ns=121))
        self.assertEqual(self.offline(self.root / "default"), 1)
        self.assertEqual(self.offline(self.root / "larger", threshold=25), 0)

    def test_overlapping_intervals_warn_instead_of_claiming_regression(self):
        self.edit(self.after, lambda m: m["timings"]["frame_pipeline/cards/whole_frame"].update(
            point_ns=130, lower_ns=100, upper_ns=160))
        output = self.root / "noisy"
        self.assertEqual(self.offline(output), 3)
        report = tracker.read_json(output / "comparison.json")
        self.assertEqual(report["status"], "inconclusive")
        self.assertEqual(report["timings"][0]["status"], "noisy")

    def test_corrupt_frame_and_wrong_size_are_errors(self):
        frame = self.after / "pixels/cards_0.rgba"
        frame.write_bytes(b"bad!")
        self.assertEqual(self.offline(self.root / "corrupt"), 2)
        frame.write_bytes(b"bad")
        self.assertEqual(self.offline(self.root / "short"), 2)

    def test_missing_or_unexpected_frames_are_errors(self):
        frame = self.after / "pixels/cards_0.rgba"
        frame.unlink()
        self.assertEqual(self.offline(self.root / "missing"), 2)
        frame.write_bytes(bytes([10, 20, 30, 255]))
        (self.after / "pixels/extra.rgba").write_bytes(frame.read_bytes())
        self.assertEqual(self.offline(self.root / "extra"), 2)

    def test_incompatible_environment_benchmark_or_suite_are_errors(self):
        original = tracker.read_json(self.after / "manifest.json")
        for key in ["environment", "benchmark_sha256", "suite"]:
            manifest = copy.deepcopy(original)
            if key == "benchmark_sha256":
                manifest[key] = "b" * 64
            else:
                manifest[key]["changed"] = True
            tracker.write_json(self.after / "manifest.json", manifest)
            self.assertEqual(self.offline(self.root / key), 2)

    def test_missing_metrics_or_invalid_numbers_are_errors(self):
        original = tracker.read_json(self.after / "manifest.json")
        manifest = copy.deepcopy(original)
        manifest["timings"].clear()
        tracker.write_json(self.after / "manifest.json", manifest)
        self.assertEqual(self.offline(self.root / "missing-metric"), 2)
        for value in [0, -1, "invalid"]:
            manifest = copy.deepcopy(original)
            manifest["timings"]["frame_pipeline/cards/whole_frame"]["point_ns"] = value
            tracker.write_json(self.after / "manifest.json", manifest)
            self.assertEqual(self.offline(self.root / str(value)), 2)

    def test_invalid_threshold_and_schema_are_errors(self):
        for threshold in [-1, "nan", "inf"]:
            self.assertEqual(self.offline(self.root / str(threshold), threshold), 2)
        self.edit(self.after, lambda m: m.update(schema_version=99))
        self.assertEqual(self.offline(self.root / "version"), 2)

    def test_existing_outputs_and_baselines_are_not_overwritten(self):
        original = (self.before / "manifest.json").read_bytes()
        self.assertEqual(self.invoke("record", "--output", self.before), 2)
        self.assertEqual(self.offline(self.before), 2)
        self.assertEqual((self.before / "manifest.json").read_bytes(), original)

    def test_check_captures_and_compares_without_replacing_baseline(self):
        output = self.root / "check"
        original = (self.before / "manifest.json").read_bytes()
        with patch.object(tracker, "capture", side_effect=lambda path: self.make_bundle(path)):
            self.assertEqual(self.invoke("check", "--baseline", self.before, "--output", output), 0)
        self.assertTrue((output / "comparison.json").exists())
        self.assertEqual((self.before / "manifest.json").read_bytes(), original)

    def test_capture_reads_raw_criterion_output_and_requires_95_percent_intervals(self):
        from types import SimpleNamespace

        fixture = tracker.read_json(self.before / "manifest.json")

        def run(command, **kwargs):
            pixels = Path(kwargs["env"]["DIRECTOR_PROFILE_PIXELS"])
            pixels.mkdir()
            (pixels / "cards_0.rgba").write_bytes((self.before / "pixels/cards_0.rgba").read_bytes())
            tracker.write_json(pixels / "suite.json", fixture["suite"])
            raw = Path(kwargs["env"]["CRITERION_HOME"]) / "cards/whole_frame/new"
            raw.mkdir(parents=True)
            tracker.write_json(raw / "benchmark.json", {"full_id": "frame_pipeline/cards/whole_frame"})
            tracker.write_json(raw / "estimates.json", {"slope": {
                "point_estimate": 100,
                "confidence_interval": {"confidence_level": confidence, "lower_bound": 99, "upper_bound": 101}}})
            return SimpleNamespace(returncode=0)

        for index, confidence in enumerate([0.95, 0.9]):
            output = self.root / f"capture-{index}"
            with patch.object(tracker, "environment", return_value={"hostname": "test"}), \
                 patch.object(tracker, "command_text", return_value="test"), \
                 patch.object(tracker.subprocess, "run", side_effect=run):
                self.assertEqual(self.invoke("record", "--output", output), 0 if confidence == 0.95 else 2)
            self.assertEqual((output / "manifest.json").exists(), confidence == 0.95)

    def test_cargo_failure_does_not_create_success_manifest(self):
        output = self.root / "failed-capture"
        with patch.object(tracker, "environment", return_value={}), \
             patch.object(tracker.subprocess, "run") as run:
            run.return_value.returncode = 7
            self.assertEqual(self.invoke("record", "--output", output), 2)
        self.assertTrue((output / "bench.log").exists())
        self.assertFalse((output / "manifest.json").exists())

    def test_non_object_records_are_errors(self):
        original = tracker.read_json(self.after / "manifest.json")
        for key in ["suite", "environment", "frames", "timings"]:
            manifest = copy.deepcopy(original)
            manifest[key] = []
            tracker.write_json(self.after / "manifest.json", manifest)
            self.assertEqual(self.offline(self.root / f"invalid-{key}"), 2)

    def test_empty_compatibility_metadata_is_an_error(self):
        self.edit(self.after, lambda m: m.update(environment={}))
        self.assertEqual(self.offline(self.root / "empty-environment"), 2)

    def test_invalid_workload_names_are_errors(self):
        self.edit(self.after, lambda m: m["suite"]["workloads"][0].update(name="../escape"))
        self.assertEqual(self.offline(self.root / "invalid-workload"), 2)

    def test_malformed_json_is_an_error(self):
        for index, text in enumerate(["{broken", "[]", "null"]):
            (self.after / "manifest.json").write_text(text)
            self.assertEqual(self.offline(self.root / f"malformed-{index}"), 2)


if __name__ == "__main__":
    unittest.main()
