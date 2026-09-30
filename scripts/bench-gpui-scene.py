#!/usr/bin/env python3
"""Build GPUI alone, then benchmark production Scene::finish and run regressions."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess


ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runs", type=int, default=2)
    parser.add_argument("--seed", type=int, default=0x5eed)
    parser.add_argument("--seed-step", type=int, default=0)
    parser.add_argument("--sizes", default="32,256,2000,10000")
    parser.add_argument("--workloads", help="comma-separated subset; default is all workloads")
    parser.add_argument("--output", type=Path, default=ROOT / "target/performance/gpui-scene")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    seeds = [args.seed + index * args.seed_step for index in range(args.runs)]
    if any(seed < 1 or seed >= 2**64 for seed in seeds):
        parser.error("seeds must be positive 64-bit integers")
    try:
        sizes = [int(value) for value in args.sizes.split(',')]
        if not sizes or any(value < 1 for value in sizes):
            raise ValueError()
    except ValueError:
        parser.error("--sizes must be a comma-separated list of positive integers")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    build = subprocess.run(
        ["cargo", "build", "--locked", "-p", "gpui-pre", "--message-format=json"],
        cwd=ROOT, capture_output=True, text=True,
    )
    (output / "build.log").write_text(build.stderr, encoding="utf-8")
    if build.returncode:
        raise RuntimeError(f"GPUI build failed; see {output / 'build.log'}\n{build.stderr}")
    events = [json.loads(line) for line in build.stdout.splitlines() if line.startswith("{")]
    artifact = next(event for event in events
                    if event.get("reason") == "compiler-artifact"
                    and event["target"]["name"] == "gpui")
    if artifact["profile"]["opt_level"] != "3":
        raise RuntimeError("GPUI must be compiled at opt-level=3 for this comparison")
    library = next(Path(name) for name in artifact["filenames"] if name.endswith(".rlib"))
    native_paths = sorted({path for event in events
                           if event.get("reason") == "build-script-executed"
                           for path in event.get("linked_paths", [])})
    rustc = ["rustc", "--edition=2024", "-C", "opt-level=3",
             "-C", f"debug-assertions={str(artifact['profile']['debug_assertions']).lower()}",
             "-C", f"overflow-checks={str(artifact['profile']['overflow_checks']).lower()}",
             "--extern", f"gpui_kit={library}", "-L", f"dependency={library.parent}",
             "-L", f"dependency={library.parent / 'deps'}"]
    for path in native_paths:
        rustc.extend(["-L", path])
    suffix = ".exe" if os.name == "nt" else ""
    benchmark = output / ("scene-finish" + suffix)
    tests = output / ("scene-tests" + suffix)
    sources = [
        "crates/emulsion-ui/benches/scene_finish.rs",
        "crates/emulsion-ui/benches/support/scene_sort_candidate.rs",
        "crates/emulsion-ui/src/gpui_fast_tests.rs",
        "vendor/gpui/gpui-pre/src/scene.rs",
        "crates/emulsion-ui/benches/support/scene_sort_initial.rs",
        "crates/emulsion-ui/benches/support/scene_sort_previous.rs",
        "scripts/bench-gpui-scene.py",
        "vendor/gpui/gpui-pre/src/scene/sort.rs",
    ]
    subprocess.run(rustc + [sources[0], "-o", str(benchmark)], cwd=ROOT, check=True)
    subprocess.run(rustc + ["--test", sources[2], "-o", str(tests)], cwd=ROOT, check=True)
    test_run = subprocess.run([str(tests), "--test-threads=1"], check=True,
                              capture_output=True, text=True)
    (output / "tests.log").write_text(test_run.stdout, encoding="utf-8")
    print(test_run.stdout, flush=True)
    metadata = {
        "platform": platform.platform(),
        "processor": platform.processor(),
        "logical_cpus": os.cpu_count(),
        "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "gpui_profile": artifact["profile"],
        "benchmark_opt_level": 3,
        "benchmark_assertions_match_gpui": True,
        "timed_implementation": "Production Scene::finish; legacy baseline and historical candidates compiled with matching profile settings",
        "seeds": seeds,
        "sizes": sizes,
        "workloads": args.workloads or "all",
        "gpui_fast_reviewed": "7ab23f46f2ba3a040ceb27d387383a2896bc5ae1",
        "gpui_fast_sort_commit": "8111e627725c1868930141bfba3c1663acc8e978",
        "source_hash_newlines": "LF",
        "source_sha256": {name: hashlib.sha256((ROOT / name).read_bytes().replace(b"\r\n", b"\n")).hexdigest() for name in sources},
        "scope": "Synthetic CPU scene sorting; no GPU draws or application FPS; counters off during timings",
    }
    if os.name == "nt":
        import winreg
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,
                            r"HARDWARE\DESCRIPTION\System\CentralProcessor\0") as key:
            metadata["processor"] = winreg.QueryValueEx(key, "ProcessorNameString")[0].strip()
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    for index in range(1, args.runs + 1):
        print(f"Running scene benchmark {index}/{args.runs}...", flush=True)
        environment = dict(os.environ, EMULSION_SCENE_BENCH_SEED=str(seeds[index - 1]),
                           EMULSION_SCENE_BENCH_SIZES=args.sizes)
        environment.pop("EMULSION_SCENE_BENCH_WORKLOADS", None)
        if args.workloads is not None:
            environment["EMULSION_SCENE_BENCH_WORKLOADS"] = args.workloads
        run = subprocess.run([str(benchmark)], check=True, capture_output=True, text=True,
                             env=environment)
        lines = run.stdout.splitlines()
        if len(lines) < 3 or not lines[1].startswith("workload,"):
            raise RuntimeError("Unexpected benchmark output")
        path = output / f"run-{index}.csv"
        path.write_text("\n".join(lines[1:]) + "\n", encoding="utf-8")
        print(f"Saved {path}", flush=True)


if __name__ == "__main__":
    main()
