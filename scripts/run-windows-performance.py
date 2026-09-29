"""Run the macOS reference workloads sequentially on Windows after release builds.

Build the seven reference examples and emulsion-gpu's release test binary first.
Pass the latter with --gpu-test-binary. Raw outputs and provenance are retained.
"""
import argparse
import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import time
import winreg

ROOT = Path(__file__).resolve().parents[1]
SOURCE_CRATES = ("core", "io", "raster", "filters", "gpu", "engine")
COMPETING_PROCESSES = {"cargo.exe", "rustc.exe", "emulsion.exe"}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_fingerprint():
    paths = [ROOT / "Cargo.toml", ROOT / "Cargo.lock"]
    for name in SOURCE_CRATES:
        paths += [p for p in (ROOT / "crates" / f"emulsion-{name}").rglob("*")
                  if p.is_file() and p.suffix in (".rs", ".toml", ".wgsl")]
    return {p.relative_to(ROOT).as_posix(): digest(p) for p in sorted(paths)}


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding="utf-8").strip()


class ProcessEntry(ctypes.Structure):
    _fields_ = [("size", wintypes.DWORD), ("usage", wintypes.DWORD),
                ("pid", wintypes.DWORD), ("heap", ctypes.c_size_t),
                ("module", wintypes.DWORD), ("threads", wintypes.DWORD),
                ("parent", wintypes.DWORD), ("priority", wintypes.LONG),
                ("flags", wintypes.DWORD), ("filename", wintypes.WCHAR * 260)]


def processes():
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    snapshot = kernel.CreateToolhelp32Snapshot
    snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    snapshot.restype = wintypes.HANDLE
    handle = snapshot(2, 0)
    if handle == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    first, following = kernel.Process32FirstW, kernel.Process32NextW
    for fn in (first, following):
        fn.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
        fn.restype = wintypes.BOOL
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    entry = ProcessEntry()
    entry.size = ctypes.sizeof(entry)
    names = []
    try:
        available = first(handle, ctypes.byref(entry))
        while available:
            names.append(entry.filename.lower())
            available = following(handle, ctypes.byref(entry))
    finally:
        kernel.CloseHandle(handle)
    return names


class MemoryStatus(ctypes.Structure):
    _fields_ = [("length", wintypes.DWORD), ("load", wintypes.DWORD)] + [
        (name, ctypes.c_ulonglong) for name in
        ("total_physical", "available_physical", "total_page", "available_page",
         "total_virtual", "available_virtual", "available_extended")]


class ProcessMemory(ctypes.Structure):
    _fields_ = [("cb", wintypes.DWORD), ("page_faults", wintypes.DWORD)] + [
        (name, ctypes.c_size_t) for name in
        ("peak_working_set", "working_set", "peak_paged", "paged", "peak_nonpaged",
         "nonpaged", "pagefile", "peak_pagefile")]


get_memory = ctypes.WinDLL("psapi").GetProcessMemoryInfo
get_memory.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessMemory), wintypes.DWORD]
get_memory.restype = wintypes.BOOL


def host_info():
    memory = MemoryStatus()
    memory.length = ctypes.sizeof(memory)
    if not ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(memory)):
        raise ctypes.WinError()
    with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,
                        r"HARDWARE\DESCRIPTION\System\CentralProcessor\0") as key:
        cpu = winreg.QueryValueEx(key, "ProcessorNameString")[0].strip()
    length = wintypes.DWORD()
    core_info = ctypes.windll.kernel32.GetLogicalProcessorInformationEx
    core_info.argtypes = [wintypes.DWORD, ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD)]
    core_info(0, None, ctypes.byref(length))
    buffer = ctypes.create_string_buffer(length.value)
    if not core_info(0, buffer, ctypes.byref(length)):
        raise ctypes.WinError()
    offset = cores = 0
    while offset < length.value:
        size = wintypes.DWORD.from_buffer(buffer, offset + 4).value
        assert size > 0
        offset += size
        cores += 1
    return {"os": platform.platform(), "cpu": cpu, "logical_cpus": os.cpu_count(),
            "physical_cores": cores,
            "physical_memory_bytes": memory.total_physical, "arch": platform.machine()}


def run_case(name, args, env, directory):
    print(f"START {name}", flush=True)
    stdout_path, stderr_path = directory / f"{name}.stdout", directory / f"{name}.stderr"
    started = time.perf_counter()
    peak = 0
    final_memory_available = False
    timed_out = False
    competing = set()
    next_process_sample = started
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        proc = subprocess.Popen(args, cwd=ROOT, env=env, stdout=stdout, stderr=stderr)
        while True:
            now = time.perf_counter()
            if now >= next_process_sample:
                competing.update(set(processes()) & COMPETING_PROCESSES)
                next_process_sample = now + 1.0
            memory = ProcessMemory()
            memory.cb = ctypes.sizeof(memory)
            available = bool(get_memory(int(proc._handle), ctypes.byref(memory), memory.cb))
            if available:
                peak = max(peak, memory.peak_working_set)
            if proc.poll() is not None:
                final_memory_available = available
                break
            if time.perf_counter() - started > 1200:
                proc.kill()
                proc.wait()
                timed_out = True
                break
            time.sleep(0.02)
    result = {"name": name, "command": args, "exit_code": proc.returncode,
              "wall_seconds": time.perf_counter() - started,
              "peak_working_set_bytes": peak,
              "memory_method": "Windows GetProcessMemoryInfo PeakWorkingSetSize, polled every 20 ms",
              "final_memory_sample_available": final_memory_available,
              "timed_out": timed_out,
              "competing_processes_observed": sorted(competing),
              "competing_process_sample_interval_seconds": 1,
              "stdout": stdout_path.read_text(encoding="utf-8", errors="replace"),
              "stderr": stderr_path.read_text(encoding="utf-8", errors="replace")}
    result["passed"] = proc.returncode == 0 and not timed_out
    if "Skipping GPU" in result["stdout"] + result["stderr"]:
        result["passed"] = False
    if "benchmark_" in name or name == "gpu-correctness":
        match = re.search(r"test result: ok\. (\d+) passed", result["stdout"])
        result["passed"] &= bool(match and int(match[1]) > 0)
    try:
        result["measurement"] = json.loads(result["stdout"])
    except ValueError:
        pass
    print(f"{'PASS' if result['passed'] else 'FAIL'} {name}: {result['wall_seconds']:.2f}s", flush=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gpu-test-binary", type=Path, required=True)
    parser.add_argument("--reference", type=Path, default=ROOT / "docs/specs/reports/macos-performance-suite-results.json")
    parser.add_argument("--output", type=Path, default=ROOT / "docs/specs/reports/windows-performance-suite-results.json")
    parser.add_argument("--allow-background-activity", action="store_true",
                        help="Measure under load; record competing builds/app activity instead of refusing.")
    options = parser.parse_args()
    options.output.parent.mkdir(parents=True, exist_ok=True)
    busy = set(processes()) & COMPETING_PROCESSES
    if busy and not options.allow_background_activity:
        raise RuntimeError(f"Finish compilation and close Emulsion before measuring: {sorted(busy)}")
    reference = json.loads(options.reference.read_text(encoding="utf-8"))
    directory = ROOT / "target/performance-windows"
    directory.mkdir(parents=True, exist_ok=True)
    sources = source_fingerprint()
    manifest = json.loads((ROOT / "crates/emulsion-io/tests/fixtures/raw-corpus.json").read_text())
    for item in manifest:
        assert digest(ROOT / "target/raw-corpus" / item["file"]) == item["sha256"], item["file"]
    baseline_env = os.environ.copy()
    removed = [key for key in baseline_env if key.startswith(("EMULSION_GPU", "GPUI_", "WGPU_"))
               or key == "RAYON_NUM_THREADS"]
    for key in removed:
        baseline_env.pop(key)
    baseline_env["PATH"] = str(ROOT / "target/release") + os.pathsep + baseline_env["PATH"]
    baseline_env.update(EMULSION_REQUIRE_GPU_TESTS="1", WGPU_DX12_COMPILER="dxc")
    result = {
        "measured_at_utc": datetime.now(timezone.utc).isoformat(),
        "base_revision": command("git", "rev-parse", "HEAD"),
        "fetched_main_revision": command("git", "rev-parse", "origin/main"),
        "runner_sha256": digest(Path(__file__)),
        "working_tree_diff": command("git", "diff", "--", "Cargo.toml", "Cargo.lock",
                                     *[f"crates/emulsion-{name}" for name in SOURCE_CRATES]),
        "source_sha256": sources, "platform": host_info(), "rustc": command("rustc", "--version"),
        "profile": "locked release, thin LTO, codegen-units=1",
        "reference_revision": reference["base_revision"],
        "reference_sha256": digest(options.reference),
        "conditions": "Sequential benchmark processes after their compilation; normal desktop services active; no thermal controls.",
        "allow_background_activity": options.allow_background_activity,
        "competing_processes_at_start": sorted(busy),
        "environment_overrides": {"EMULSION_REQUIRE_GPU_TESTS": "1", "WGPU_DX12_COMPILER": "dxc",
                                  "PATH_prepend": "target/release", "removed_inherited_keys": removed},
        "limitations": ["Different hardware, code revision and local loading fixes from the Mac baseline; not an OS-only comparison.",
                        "Component workloads exclude native-window presentation and physical input latency.",
                        "Windows working set and macOS RSS are different memory accounting metrics.",
                        "Compute adapter selection ignores WGPU_BACKEND; actual GPU_ADAPTER diagnostics identify the chosen backend.",
                        "Only the Gaussian case pins Rayon to eight threads; other workloads use the machine default, as on Mac."],
        "raw_corpus": manifest, "executables": {}, "runs": []}
    result["runtime_sha256"] = {name: digest(ROOT / "target/release" / name)
                                for name in ("dxcompiler.dll", "dxil.dll")}
    for original in reference["runs"]:
        old = original["command"]
        binary = (options.gpu_test_binary.resolve() if "/deps/" in old[0]
                  else ROOT / "target/release/examples" / (Path(old[0]).name + ".exe"))
        args = [str(binary)] + [part.replace("target/performance-macos", "target/performance-windows") for part in old[1:]]
        assert binary.is_file(), binary
        if str(binary) not in result["executables"]:
            result["executables"][str(binary)] = digest(binary)
        env = baseline_env.copy()
        overrides = {key: ("dx12" if key == "WGPU_BACKEND" else value)
                     for key, value in original["env"].items()}
        env.update(overrides)
        run = run_case(original["name"], args, env, directory)
        run["env"] = overrides
        result["runs"].append(run)
        options.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    unchanged = source_fingerprint() == sources
    originals_unchanged = all(digest(ROOT / "target/raw-corpus" / item["file"]) == item["sha256"] for item in manifest)
    result["validation"] = {"runs_passed": sum(run["passed"] for run in result["runs"]),
                            "runs_total": len(result["runs"]), "source_unchanged_during_runs": unchanged,
                            "raw_originals_unchanged": originals_unchanged,
                            "no_competing_processes_observed": all(not run["competing_processes_observed"] for run in result["runs"])}
    result["finished_at_utc"] = datetime.now(timezone.utc).isoformat()
    options.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result["validation"]), flush=True)
    acceptable_conditions = options.allow_background_activity or result["validation"]["no_competing_processes_observed"]
    return 0 if unchanged and originals_unchanged and acceptable_conditions and all(run["passed"] for run in result["runs"]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
