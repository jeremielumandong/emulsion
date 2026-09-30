#!/usr/bin/env python3
"""Repeat exact-output CPU comparisons and optional native canvas smoke runs."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
from statistics import median
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def compilers():
    if platform.system() != "Windows":
        return False
    processes = subprocess.check_output(["tasklist", "/FO", "CSV", "/NH"], text=True).lower()
    return any(name in processes for name in [
        '"rustc.exe"', '"clippy-driver.exe"', '"rust-lld.exe"',
        '"emulsion_ui-', '"emulsion_core-', '"emulsion_raster-',
    ])


def wait_for_builds():
    waiting = False
    while compilers():
        if not waiting:
            print("Waiting for Rust compilation before timing...", flush=True)
            waiting = True
        time.sleep(2)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--native-binary", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--native-only", action="store_true", help="append native checks to an existing CPU result")
    parser.add_argument("--allow-background-activity", action="store_true", help="run native smoke checks during builds and record contention")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("runs must be positive")
    if args.native_only and (not args.native_binary or not args.output.exists()):
        parser.error("native-only requires --native-binary and an existing result file")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    sources = ["crates/emulsion-raster/src/color.rs", "crates/emulsion-raster/src/image.rs",
               "crates/emulsion-core/src/document.rs", "crates/emulsion-core/examples/interaction_bench.rs",
               "crates/emulsion-ui/src/viewport.rs", "crates/emulsion-ui/src/viewport/sampling.rs",
               "crates/emulsion-ui/src/editor/layers_panel.rs",
               "crates/emulsion-ui/src/editor/canvas_benchmark.rs", "scripts/bench-interaction.py"]
    fingerprints = {p: digest(ROOT / p) for p in sources}
    result = {"date": datetime.now(timezone.utc).isoformat(), "platform": platform.platform(),
              "processor": platform.processor(), "logical_cpus": os.cpu_count(),
              "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
              "reference": "previous algorithms in the same release executable",
              "binary_sha256": digest(args.binary), "source_sha256": fingerprints,
              "runs": [], "native": []}
    if args.native_only:
        result = json.loads(args.output.read_text(encoding="utf-8"))
        assert result["binary_sha256"] == digest(args.binary)
        assert all(result["source_sha256"][p] == fingerprints[p]
                   for p in sources if p != "scripts/bench-interaction.py")
        result["native_runner_sha256"] = digest(Path(__file__))
    env = os.environ.copy()
    for key in list(env):
        if key.startswith(("EMULSION_GPU", "GPUI_", "WGPU_")) or key == "RAYON_NUM_THREADS":
            env.pop(key)
    env["PATH"] = str(ROOT / "target/release") + os.pathsep + env["PATH"]
    for run in range(0 if args.native_only else args.runs):
        wait_for_builds()
        process = subprocess.run([str(args.binary.resolve())], capture_output=True, text=True,
                                 env=env, cwd=ROOT, timeout=180, check=True)
        measurement = json.loads(process.stdout)
        assert not measurement["debug_assertions"]
        assert all(case["exact_output"] for case in measurement["results"])
        measurement["stderr"] = process.stderr
        measurement["compiler_active_at_completion"] = compilers()
        result["runs"].append(measurement)
        print(f"CPU run {run + 1}/{args.runs} passed", flush=True)
    result["summary"] = []
    for index, case in enumerate(result["runs"][0]["results"]):
        cases = [run["results"][index] for run in result["runs"]]
        assert all(c["case"] == case["case"] for c in cases)
        summary = {"case": case["case"]}
        for key in ["reference_median_ms", "candidate_median_ms", "reduction_pct"]:
            summary[key] = median(c[key] for c in cases)
        result["summary"].append(summary)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    if args.native_binary:
        result["native_binary_sha256"] = digest(args.native_binary)
        for canvas in ["1", "0"]:
            if not args.allow_background_activity:
                wait_for_builds()
            busy = compilers()
            native_env = dict(env, EMULSION_GPU_CANVAS=canvas)
            print(f"Native canvas smoke: GPU canvas={canvas}", flush=True)
            process = subprocess.run([str(args.native_binary.resolve())], capture_output=True,
                                     text=True, env=native_env, cwd=ROOT, timeout=180)
            if process.returncode:
                result["native"].append({"gpu_canvas": canvas, "exit_code": process.returncode,
                                         "stdout": process.stdout, "stderr": process.stderr})
                args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
                raise RuntimeError(process.stderr[-4000:])
            measurement = json.loads(process.stdout)
            assert not measurement["debug_assertions"]
            assert len(measurement["results"]) == 5
            assert all(r["samples"] == 40 for r in measurement["results"])
            expected = "gpu" if canvas == "1" else "cpu"
            assert all(r["renderer"] == expected for r in measurement["results"][:4])
            assert measurement["results"][4]["renderer"] == "cpu"
            assert all(r["inactive_window_samples"] == 0 for r in measurement["results"])
            result["native"].append({"gpu_canvas": canvas, "measurement": measurement,
                                     "compiler_active_at_start": busy,
                                     "compiler_active_at_completion": compilers(),
                                     "stderr": process.stderr})
    assert fingerprints == {p: digest(ROOT / p) for p in sources}, "sources changed during measurement"
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    for case in result["summary"]:
        print(f"{case['case']}: {case['reference_median_ms']:.3f} -> "
              f"{case['candidate_median_ms']:.3f} ms ({case['reduction_pct']:.1f}% less)")


if __name__ == "__main__":
    main()
