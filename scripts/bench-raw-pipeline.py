#!/usr/bin/env python3
"""Run read-only RAW stage benchmarks; preserve every timing and output hash."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--reference", type=Path, help="alternate reference/candidate runs for each photo")
    parser.add_argument("--photos", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--samples", type=int, default=7)
    parser.add_argument("--files", nargs="*", help="optional basename subset")
    args = parser.parse_args()
    if args.runs < 1 or args.samples < 1:
        parser.error("runs and samples must be positive")
    files = sorted(p for p in args.photos.rglob("*") if p.suffix.lower() in
                   {".cr2", ".cr3", ".nef", ".dng", ".raf", ".arw"}
                   and (not args.files or p.name in args.files))
    if not files:
        parser.error("no RAW files selected")
    args.output.mkdir(parents=True, exist_ok=True)
    metadata = {"platform": platform.platform(), "processor": platform.processor(),
                "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                "runs": args.runs, "samples": args.samples, "files": len(files)}
    if args.reference:
        metadata["reference_sha256"] = hashlib.sha256(args.reference.read_bytes()).hexdigest()
    (args.output / "manifest.json").write_text(json.dumps(metadata, indent=2))
    failures = []
    for run in range(args.runs):
        # Reverse alternating runs to reduce persistent ordering bias.
        for photo in files if run % 2 == 0 else list(reversed(files)):
            variants = [("reference", args.reference), ("candidate", args.binary)] if args.reference else [("", args.binary)]
            for label, binary in variants if run % 2 == 0 else list(reversed(variants)):
                output = args.output / label
                output.mkdir(exist_ok=True)
                start = time.perf_counter()
                result = subprocess.run([str(binary.resolve()), str(photo.resolve()),
                                         str(args.samples)], capture_output=True, text=True)
                name = f"{photo.parent.name}-{photo.name}-{run + 1}"
                (output / f"{name}.stderr.txt").write_text(result.stderr)
                if result.returncode:
                    failures.append(f"{label}/{name}")
                    print(f"FAIL {label}/{name}: {result.stderr[-500:]}", flush=True)
                    continue
                data = json.loads(result.stdout)
                data["file"] = f"{photo.parent.name}/{photo.name}"
                (output / f"{name}.json").write_text(json.dumps(data, indent=2))
                print(f"{label}/{name}: load {data['load_ms']:.1f} ms, first preview "
                      f"{data['cold_fit_ms']:.1f} ms, full {data['full_develop_ms']:.1f} ms "
                      f"({time.perf_counter() - start:.1f}s)", flush=True)
    if failures:
        raise SystemExit(f"Failed: {', '.join(failures)}")


if __name__ == "__main__":
    main()
