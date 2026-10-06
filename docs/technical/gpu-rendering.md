# GPU image processing

Emulsion uses two graphics paths: GPUI draws the interface and presents images;
`emulsion-gpu` optionally computes document pixels using wgpu. The compute device
starts in the background. Editing remains available before initialization and
when no compatible compute device exists.
The desktop editor and MCP server both initialize this optional backend.

This is a hybrid pipeline, not a fully GPU-resident editor. Canonical document
tiles, undo, saving, layer source sampling, and brush dab accumulation remain on
the CPU. Compute jobs upload inputs and download results before committing them.

## Startup readiness

`begin_initialize()` starts at most one background initialization attempt and
returns an `InitializationHandle` promptly. Its `status()` is a nonblocking
snapshot: `Pending`, `Ready`, `DisabledByConfiguration`, `Unavailable(reason)`,
`Cancelled`, or `TimedOut`. The compatibility `initialize()` call waits for the
same attempt with a finite bound; UI code uses the nonblocking entry point.

While startup is pending, document rendering, filtering, and painting use their
ordinary CPU paths. `context()` and `screen_context()` do not wait for startup.
No compute hooks become usable until every requested capability has passed
preparation and known-result readback on the production device:

- All enabled compute modes prepare the tile compositor and filters.
- `EMULSION_GPU_BRUSHES=1` additionally prepares final brush composition.
- `EMULSION_GPU_BRUSHES=persistent` instead prepares persistent append and
  preview, retaining the actual pipeline for later stroke sessions.
- `EMULSION_GPU=force` and `software` also prepare experimental screen sampling.
- `EMULSION_GPU=cpu` skips the worker, adapter, device, and preparation, even with
  either optional brush flag set.

Startup has one **20-second aggregate readiness budget**, including device
creation, all selected preparation, validation, and publication. This is a
one-time startup allowance, not a new dispatch timeout. Normal dispatch/readback
waits remain **two seconds**, including the normal-path known-result checks
after preparation. Neither figure is an end-to-end guarantee for synchronous
driver calls, CPU preparation, or locks. Unused optional kernels are not prepared
in automatic hardware mode, and startup work does not count as proof of a later
successful application dispatch.

If any requested capability fails preparation or validation, the backend
makes none of the requested hooks usable and continues with CPU operations.
Registration conflicts are diagnosed; any partially registered hooks hold only
weak device references and remain CPU-declining, so failed preparation can be
released. Context queries return cloned `Arc` handles without taking a lock. The
app-lifetime owner cancels pending startup on shutdown without joining the worker
on the UI thread; closing a document does not cancel app-wide startup.
Cancellation, timeout, and failure are terminal for that attempt. A late driver
completion cannot publish a context, and repeated initialization calls do not
spawn replacements. Cancellation after a completed `Ready` commit does not
uninstall process-global hooks.

A synchronous driver call can outlive the readiness budget. The control plane
still reports timeout and keeps the CPU path available, but cannot forcibly stop
arbitrary in-process driver code. Worker-owned resources are released when that
call returns. Separate-process acceptance therefore also has an outer watchdog.
Readiness does not certify the independent `emulsion-engine` canvas or GPUI
presentation, nor does it prevent a later device fault from disabling compute.

## Coverage

| Operation | Implementation and default |
| --- | --- |
| Interface and tile presentation | GPUI graphics backend; existing software fallback on Linux/Windows |
| Layer/group compositing | GPU kernels for every blend mode, masks, clipping, and prepared adjustments; automatic routing selects expensive compositions |
| Filters | GPU kernels for all current filter variants; automatic routing selects positive-strength Reduce Noise at 512×512 pixels or larger |
| Brush final composition | Experimental GPU normal/behind/multiply, erasing, clipping, alpha lock; CPU by default |
| Rotated/magnified nearest-pixel viewport | Experimental GPU sampling with exact CPU correction near pixel boundaries; CPU by default |
| Brush dabs, wet sampling, clone, complex tips/grain, selections, transforms, import/export, history | Existing CPU algorithms |

Kernels cover filter families including blur, sharpening, edge detection, noise,
emboss, distortion, and lens correction. Large or malformed jobs still fall back:
for example Lens Blur radii above 32, Motion Blur distance above 500, and jobs
exceeding buffer or memory limits. Compositing is bounded to 64 nodes, depth 16,
512 commands, and 64 MiB of prepared source data. Each dispatch is bounded to
256 MiB for input/output/readback buffers and 384 MiB including transient upload
staging, subject to tighter device limits. Busy tile
preparation uses the parallel CPU renderer instead of queuing unbounded uploads.
For measured dense four-tile brush jobs, the context can retain one completed
compute workspace of up to 64 MiB for reuse;
larger jobs are released after completion. Retained buffers do not accumulate
per shader or per stroke, and count against the memory budget when another job
runs between brush updates.

## Performance routing

Release benchmarks on Intel Iris Plus Graphics measured complete preparation,
dispatch, and readback, with CPU/GPU output comparisons:

| Workload | Result |
| --- | --- |
| Eight complex blend layers | GPU about 2.2–2.4× faster |
| Eight adjustments | GPU about 6–11× faster |
| One Hue/Saturation adjustment | GPU about 1.5–2× faster |
| Reduce Noise, 512² and 2048² | GPU about 2× faster |
| Normal-only layers, single exposure LUT, simple filters | CPU generally faster |
| Brush final composition, 4–16 tiles | CPU remains faster; see subsequent [brush timings](gpu-brush-performance.md) |

These measurements inform conservative defaults; they are not guarantees for
other hardware. Normal/LUT-only compositions use CPU. GPU eligibility considers
costly adjustments and the density of expensive blends relative to source
layers. Cheap filtering stays on CPU. The viewport kernel is opt-in until
end-to-end UI latency is established. Brush accumulation and direct GPU texture
presentation need further work to remove repeated transfers before GPU painting
can become the default.
See [brush performance investigation](gpu-brush-performance.md) for subsequent
transfer optimization and the proposed persistent dab-accumulation pipeline.

## Controls

Set before starting Emulsion:

| Variable | Effect |
| --- | --- |
| Unset `EMULSION_GPU` | Select hardware compute and use performance routing |
| `EMULSION_GPU=cpu` | Disable image compute; GPUI presentation is unaffected |
| `EMULSION_GPU=force` | Prefer available hardware compute for supported compositions, filters, and screen sampling, regardless of measured cost |
| `EMULSION_GPU=software` | Force a CPU graphics adapter for shader validation; fails compute initialization if none exists, leaving ordinary CPU algorithms available |
| `EMULSION_GPU_BRUSHES=1` | Also enable experimental final brush composition for eligible batches of 4–32 tiles |
| `EMULSION_GPU_BRUSHES=persistent` | Instead install the persistent GPU brush backend described in [Making GPU brushes faster](gpu-brush-performance.md); experimental |
| `EMULSION_GPU_CANVAS=0` | Disable the experimental `emulsion-engine` canvas (enabled by default on supported platforms); independent of image compute and GPU brushes |

Overrides never bypass correctness checks or device/memory limits. Unsupported
jobs use their complete CPU reference operation. A failed shader is disabled for
the session; a lost device disables compute until restart. CPU document data
remains available. See [VM rendering](../guides/rendering.md) for GPUI's independent
`GPUI_FORCE_SOFTWARE_RENDERING` diagnostic.

The wgpu compute code is portable across Linux, macOS, and Windows. Local hardware
validation was on Linux/Intel, with software compute also tested on Mesa llvmpipe;
macOS/Windows runtime performance and pixel parity
still require testing on those platforms. The native macOS GPUI renderer remains
unchanged; this does not add a macOS software interface renderer.

## GPU canvas

The editor enables the new wgpu/Vello canvas engine by default on Windows,
Linux and macOS. Only `EMULSION_GPU_CANVAS=0` disables it; leaving the variable
unset or setting it to `1` enables it. No additional Cargo feature is needed.
On Windows, build and launch from PowerShell:

```powershell
.\scripts\build-windows.ps1
Remove-Item Env:EMULSION_GPU_CANVAS -ErrorAction SilentlyContinue
& .\target\release\emulsion.exe 'C:\images\example.ora'
```

Replace the example path with your image or project, or omit it to open the home
screen. To compare the CPU canvas, close the editor, set
`$env:EMULSION_GPU_CANVAS = '0'`, and launch again. Remove the variable before
the next launch to restore the GPU canvas. These changes apply to apps launched
from that PowerShell session.

On Windows, the engine uses D3D12 on the same adapter as GPUI's D3D11 renderer
and presents through shared textures. It logs `gpu canvas active` when a
document starts using the engine. An unsupported document or an initialization
failure logs `gpu canvas unavailable, using the CPU path`; a rotated view also
uses the existing canvas. The engine presents edits and reuses unchanged GPU
tiles on reload; editor strokes still use the existing brush implementation.
Enabling the canvas does not require `EMULSION_GPU_BRUSHES` or `EMULSION_GPU=force`.

Run the engine's regression checks explicitly on the Windows D3D12 backend:

```powershell
$env:WGPU_BACKEND = 'dx12'
$env:EMULSION_REQUIRE_GPU_TESTS = '1'
cargo test --locked --release -p emulsion-engine -p vello-canvas-spike -- --test-threads=1
Remove-Item Env:WGPU_BACKEND
Remove-Item Env:EMULSION_REQUIRE_GPU_TESTS
```

These checks cover compositing, brush readback, atlas reuse, and visibility of
pixel, layer, and vector changes after reload. Also check the editor itself:
paint and undo/redo, add and move layers and text, pan/zoom, resize the window,
and rotate the view to exercise the existing canvas fallback. See the
[canvas spike results](../../spikes/vello-canvas/RESULTS.md) for measurements and
remaining limitations.

## Verification

```sh
cargo test --locked -p emulsion-raster -p emulsion-filters --lib -- --test-threads=1
EMULSION_REQUIRE_GPU_TESTS=1 cargo test --locked -p emulsion-gpu --lib -- --test-threads=1
cargo build --locked -p emulsion-gpu --example backend_smoke
EMULSION_GPU=force target/debug/examples/backend_smoke
EMULSION_GPU=cpu EMULSION_GPU_BRUSHES=persistent target/debug/examples/backend_smoke
```

GPU tests otherwise skip when no adapter is available. `EMULSION_REQUIRE_GPU_TESTS=1`
turns that into failure. The shared parity-test context uses the same preparation
implementation before testing normal dispatches; dedicated raw-device fault
fixtures remain separate. Tests cover transparent
pixels, blend spaces, groups, masks, clipping, mip levels, deterministic noise,
all filter variants, viewport sampling, rejected jobs, and device loss. GPU/CPU
results have small floating-point differences; nearest viewport pixels use CPU
correction where necessary rather than moving pixel boundaries.

### Cold and warm software acceptance

On Linux with Mesa Lavapipe installed, build the smoke binary and test targets
first, then run the harness. CI builds its smoke programs together with workspace
features resolved consistently:

```sh
cargo test --workspace --locked --no-run --message-format=json > /tmp/emulsion-test-artifacts.json
cargo build --workspace --locked --example backend_smoke --example renderer_smoke
EMULSION_GPU_TEST_ARTIFACTS=/tmp/emulsion-test-artifacts.json bash scripts/gpu-startup-smoke.sh
```

The harness accepts paths to already-built `backend_smoke` and `emulsion_gpu`
test executables. If the test path is omitted, it resolves the unique executable
from `EMULSION_GPU_TEST_ARTIFACTS`, the recorded Cargo build output above. It
looks for the system Lavapipe ICD, or accepts its explicit JSON path through
`EMULSION_LAVAPIPE_ICD`. It requires Bash, Python 3, and GNU coreutils; it does not
install drivers, rebuild either executable, or guess among stale test binaries. No external shader-prewarming helper
is used.

Each of these software configurations starts in a fresh process and a distinct,
verified empty shader-cache directory:

1. Default brushes, with `EMULSION_GPU_BRUSHES` unset
2. Final brush composition, with `EMULSION_GPU_BRUSHES=1`
3. Persistent brushes, with `EMULSION_GPU_BRUSHES=persistent`

Software mode selects a CPU graphics adapter and also exercises screen sampling.
The Linux harness adds `--require-vulkan` and verifies the returned adapter type
and backend rather than assuming the driver environment selected them.
For each configuration, the smoke requires `Ready`, reports preparation elapsed
time, and verifies fresh post-readiness execution and correct output for every
selected capability through its published production interface. Startup
dispatches cannot satisfy these execution assertions. Immediately after each
cold success, a second process repeats the same assertions using that case's
cache and is recorded separately as warm-cache acceptance.

The harness overrides Mesa's cache location and format and disables inherited
read-only Fossilize cache inputs, using
[Mesa's documented cache controls](https://docs.mesa3d.org/envvars.html#envvar-MESA_SHADER_CACHE_DIR).
It requires nonempty compiled-shader entries in the isolated multi-file cache;
an empty directory or index alone is insufficient evidence. Unsupported cache
behavior fails the acceptance check rather than being reported as a cold/warm
pass. The harness never deletes unrelated caches. A warm run means the same
populated cache was available; it does not assert that every lookup was a hit.

Two more independent processes verify CPU override with each optional brush
flag. They require `DisabledByConfiguration`, zero preparation and installed
compute hooks, no context, correct CPU outputs, and untouched shader caches.
Finally, the required `emulsion-gpu` suite runs serially against another verified
empty cache with `EMULSION_REQUIRE_GPU_TESTS=1`, `--nocapture`, and
`--test-threads=1`. No smoke process has used that suite's cache.

The outer process watchdogs remain 60 seconds per smoke and 180 seconds for the
required suite. A timeout is a failure; the harness does not retry or raise any
production deadline, tolerance, resource limit, or GPU-required assertion. It
retains the source revision, worktree status, smoke and test binary hashes, ICD identity,
selected environment, adapter/driver output, timing, cache manifests, and full
first-failure logs under a newly created directory printed at launch. CI uploads
the report files even on failure; large driver-cache files remain outside the
artifact. These are acceptance procedures, not a claim that a particular local
checkout or hardware platform has passed them.

Automatic and forced hardware startup, macOS/Windows runtime parity, the native
engine tests, and GPUI renderer smoke still need their own platform acceptance.
CI retains its separate software Vulkan draw/present smoke. Interactive checks
must also establish that editing remains responsive while compute startup is
pending or cancelled and that shutdown does not wait for the startup worker;
compute smoke success alone does not establish those UI properties.

For repeatable performance measurements, run the ignored release benchmarks
without other GPU workloads:

```sh
EMULSION_REQUIRE_GPU_TESTS=1 cargo test -p emulsion-gpu --release benchmark_ -- --ignored --nocapture --test-threads=1
```


The [brush-specific persistent router](gpu-brush-performance.md#brush-specific-routing)
is available with `EMULSION_GPU_BRUSHES=persistent`. It selects persistent GPU
accumulation for supported large dry round brushes on rasters up to 1024×1024;
small brushes and unsupported settings use CPU. Backend failure replays the
resolved dab journal on CPU, preserving the current raster/history contract.
This mode remains experimental and off by default. Direct GPU presentation and
broader brush support remain future work.
