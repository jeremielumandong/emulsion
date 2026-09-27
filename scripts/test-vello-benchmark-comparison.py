#!/usr/bin/env python3
"""Acceptance tests for rejecting misleading native performance comparisons."""
import importlib.util
from pathlib import Path
import unittest
import sys

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("comparison", Path(__file__).with_name("compare-vello-benchmarks.py"))
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)


def report(backend):
    return dict(os="linux", arch="x86_64", debug_assertions=False,
                document_px=[3840, 2160], nodes=26, display_scale=1.,
                editor_logical_px=[1000, 700], canvas_device_px=[652, 536],
                adapter="fixture", window_active_at_completion=True,
                results=[dict(scenario=name, renderer=backend, gpu_brush=backend == "gpu" and name == "brush",
                              samples=40, inactive_window_samples=0,
                              input_to_canvas_submission_ms=dict(p50=2., p95=4.),
                              input_to_next_frame_boundary_ms=dict(p50=17., p95=34.))
                         for name in ("pan", "brush", "vector_edit")])


class ComparisonTests(unittest.TestCase):
    def test_matched_reports_compare_submission_and_callbacks_separately(self):
        text = comparison.compare(report("gpu"), report("cpu"))
        self.assertIn("brush | submission", text)
        self.assertIn("brush | next frame callback", text)

    def test_different_surfaces_hardware_or_documents_are_rejected(self):
        for field, value in (("canvas_device_px", [650, 536]), ("display_scale", 2.),
                             ("adapter", "another GPU"), ("document_px", [1024, 768]),
                             ("nodes", 20), ("debug_assertions", True)):
            with self.subTest(field=field):
                cpu = report("cpu")
                cpu[field] = value
                with self.assertRaises(ValueError):
                    comparison.compare(report("gpu"), cpu)

    def test_inactive_fallback_incomplete_and_invalid_samples_are_rejected(self):
        for field, value in (("inactive_window_samples", 1), ("renderer", "cpu"),
                             ("samples", 0), ("gpu_brush", True),
                             ("input_to_canvas_submission_ms", dict(p50=float("nan"), p95=4.))):
            with self.subTest(field=field):
                gpu = report("gpu")
                gpu["results"][0][field] = value
                with self.assertRaises(ValueError):
                    comparison.compare(gpu, report("cpu"))
        gpu = report("gpu")
        gpu["results"].pop()
        with self.assertRaises(ValueError):
            comparison.compare(gpu, report("cpu"))


if __name__ == "__main__":
    unittest.main()
