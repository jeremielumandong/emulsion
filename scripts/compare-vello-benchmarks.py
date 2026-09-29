#!/usr/bin/env python3
"""Reject unmatched native measurements before comparing CPU and GPU timings."""
import json
import math
import sys
from pathlib import Path


def compare(gpu, cpu):
    for key in ("os", "arch", "debug_assertions", "document_px", "nodes",
                "display_scale", "editor_logical_px", "canvas_device_px", "adapter"):
        if key not in gpu or key not in cpu or gpu[key] != cpu[key]:
            raise ValueError(f"Unmatched benchmark field: {key}")
    if gpu["debug_assertions"]:
        raise ValueError("Use release builds for performance comparisons")
    for report in (gpu, cpu):
        if report.get("window_active_at_completion") is not True:
            raise ValueError("Benchmark window was inactive at completion")
        for key in ("document_px", "editor_logical_px", "canvas_device_px"):
            value = report[key]
            if not isinstance(value, list) or len(value) != 2 or not all(
                    isinstance(v, (int, float)) and math.isfinite(v) and v > 0 for v in value):
                raise ValueError(f"Invalid dimensions: {key}")
        if not isinstance(report["display_scale"], (int, float)) or not math.isfinite(report["display_scale"]) or report["display_scale"] <= 0:
            raise ValueError("Invalid display scale")
    cases = ("pan", "brush", "vector_edit")
    indexed = []
    for report, backend in ((gpu, "gpu"), (cpu, "cpu")):
        rows = report.get("results", [])
        if len(rows) != len(cases) or {row.get("scenario") for row in rows} != set(cases):
            raise ValueError(f"Missing or duplicated {backend} benchmark scenarios")
        table = {row["scenario"]: row for row in rows}
        for name, row in table.items():
            if row.get("renderer") != backend:
                raise ValueError(f"{name}: expected {backend} canvas; fallback cannot be compared as GPU")
            if row.get("inactive_window_samples") != 0:
                raise ValueError(f"{name}: inactive-window samples invalidate frame pacing")
            if row.get("samples") != 40:
                raise ValueError(f"{name}: expected 40 measured samples")
            if row.get("gpu_brush") is not (backend == "gpu" and name == "brush"):
                raise ValueError(f"{name}: unexpected brush routing")
            for metric in ("input_to_canvas_submission_ms", "input_to_next_frame_boundary_ms"):
                values = row.get(metric, {})
                for percentile in ("p50", "p95"):
                    v = values.get(percentile)
                    if not isinstance(v, (int, float)) or not math.isfinite(v) or v <= 0:
                        raise ValueError(f"{name}: invalid {metric}/{percentile}")
                if values["p95"] < values["p50"]:
                    raise ValueError(f"{name}: unordered percentiles")
        indexed.append(table)
    output = ["Matched native EditorView measurements (milliseconds).",
              "Frame callback times do not measure physical display latency.",
              "scenario | metric | CPU p50 / p95 | GPU p50 / p95"]
    for name in cases:
        for metric, label in (("input_to_canvas_submission_ms", "submission"),
                              ("input_to_next_frame_boundary_ms", "next frame callback")):
            g, c = indexed[0][name][metric], indexed[1][name][metric]
            output.append(f"{name} | {label} | {c['p50']:.3f} / {c['p95']:.3f} | {g['p50']:.3f} / {g['p95']:.3f}")
    return "\n".join(output)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: compare-vello-benchmarks.py GPU.json CPU.json")
    try:
        print(compare(*(json.loads(Path(path).read_text()) for path in sys.argv[1:])))
    except (ValueError, OSError, TypeError, KeyError) as error:
        sys.exit(f"Benchmark comparison rejected: {error}")
