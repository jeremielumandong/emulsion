# Performance strategy

## Open-document lifecycle

Keep document pixels, undo/history, selection, tool settings, and viewport position
in memory for every open tab. Give only the visible editor presentation resources.
Inactive documents still need their source data; this is not disk hibernation and
cannot make twenty independent documents cost the same memory as one.

Implemented:

- Route tab and Home/Settings/Batch/About navigation through explicit visibility
  transitions. Closing a tab also releases its presentation resources.
- Cancel hidden marching-ants timers, timeline playback, and history replay.
  Playback returns paused at the saved position; the replay picture is rebuilt.
  Reduced-motion settings also suppress marching-ants updates.
- Drop canvas tile images, crisp screen images, deferred GPU disposals, layer and
  channel thumbnails, mask overlays, and the replay picture immediately.
- Cancel tile-task continuations and stop raster work between tiles. Reject late
  completions with a visibility generation, including a quick switch away/back.
  A raster calculation already executing can finish; it cannot install a hidden
  result or start another hidden batch.
- Stop stale composite-tree completions from chaining hidden rebuilds. Keep the
  last composite tree and the underlying document for operations needing them.
- Preserve save/autosave, export, import, AI jobs, and document correctness.
  Those operations may still consume resources while their tab is hidden.

The previous tab switch called `TileCache::clear`, which moved image references
into `to_drop`. Only canvas painting drained that queue, so a hidden tab retained
its images until it painted again. Suspension now calls `release(window)`, which
also removes the images from GPUI's sprite atlas and drains those references.

A nominal 480-tile cache of 256x256 BGRA8 images represents 120 MiB of pixel bytes,
excluding crisp screen images, GPU storage, allocation overhead, and source data.
This is a capacity calculation, not a measured saving or strict cache maximum:
visible tiles can exceed the eviction target. CPU allocators and GPU atlases may
retain freed capacity, so process working set need not fall immediately.

## Live experimental layout setting

**Settings > Experimental > Reuse interface layout** enables or disables retained
layout immediately for all open windows and saves the preference. The application
default is on for new settings and older settings without this field. An explicit
saved off choice remains off. Switching resets framework layout caches and
refreshes windows, without reopening documents or resetting their undo history. New windows inherit the
session's effective choice.

`EMULSION_RETAINED_LAYOUT=0`/`1` remains a startup override for comparisons and
troubleshooting. It is read once when the first workspace opens, so it cannot
prevent live switching. It does not overwrite the saved preference unless the
user changes the switch. Removing the variable restores the saved choice at the
next launch. A Settings note explains when a launch override is present.

Validation: the full UI suite passed 454 tests with each startup override (`0`
and `1`), with one existing ignored test and the known stale landing-image
assertion excluded (`tests::splash_dismisses_and_the_landing_image_opens_for_editing`).
New regressions cover pointer/keyboard switching, persisted choice, existing and
new windows, and document/undo preservation. Two settings compatibility tests,
the normal application check, UI clippy, and the release build also passed;
existing unrelated lifetime and Windows backend unused-import warnings remain.
Logs: `target/performance-settings-{suite-0,suite-1,persistence,check,clippy,release-build}.log`.

## Canvas navigation invalidation

Wheel pan/zoom, hand-tool pan, canvas rotation, and pinch now notify the canvas
view instead of the editor owner. This keeps the cached Layers/Properties sibling
reusable during navigation. Visible Info and Navigator panels still receive a
sidebar notification because their values depend on the current viewport.
Manually or automatically collapsed panels do not redraw. Uncached ancestors
continue updating zoom/rotation controls.

The `editor_navigation` benchmark exercises the actual editor pan handler with
64 in-memory raster layers, both chrome layouts, bundled icons, native text, and
warmed canvas tiles. It compares targeted notifications with the former owner
notification in the same binary, with cold and retained layout. Rendering counts
and the resulting view transform are asserted. GPU presentation is excluded. [Release measurements](../specs/reports/canvas-navigation-release-results.md)
show 41-42% less CPU-side pan-update time in roomy chrome and 22-28% less in compact
from targeted notifications. Additional layout-reuse gains were mixed. Enabling
layout reuse by default is a product choice, not evidence of a universal CPU saving.

```powershell
cargo bench -p emulsion-ui --features layout-bench --bench editor_navigation
```

The fixture isolates its application data and disables tablet hooks, autosave,
and selection timers. Those services retain their normal behavior outside the
benchmark. It is a controlled UI workload, not a measurement of idle process CPU
or arbitrary large-image rasterization.

Validation: all 12 canvas invalidation regressions passed, including five new
navigation tests covering real wheel/pinch/drag events, live zoom/rotation labels,
painted Navigator geometry, and manual/automatic sidebar collapse. The complete
UI suite passed 450 tests in each layout-reuse mode, with one existing ignored
test and one excluded stale landing-image dimensions assertion. Application and
Clippy checks passed; the existing test lifetime warning remains.

## Measurement before more caching

Use the same release build, hardware, document set, window size, DPI, refresh rate,
and background-job state for each comparison. Record two separate workloads:

| Workload | Measurements |
| --- | --- |
| 1, 5, and 20 idle tabs, with selections | Process CPU time per second; foreground wakeups; private bytes; GPU memory |
| Switch repeatedly among those tabs | First useful frame and settled-frame latency; peak memory; tile rebuild count |
| Pointer movement and selection animation | Canvas/sidebar render counts; frame count; p50/p95/p99 frame time |
| Large Layers list with expanded effects | Visible row construction; layout time; scroll frame time |
| Continuous pan/zoom or painting | Raster time versus UI render/layout time; input latency; queued work |

Headless regression tests prove resource ownership, cancellation, stale-result
rejection, and correct restoration. They do not report real-window CPU savings.
Our tests exercise both editor layouts and preserve edits and undo across switches.

The vendored framework already has `BenchAppContext`, `#[gpui::bench]`, and finite
`bench_renderer_session` workloads behind `bench-support` in
`vendor/gpui/gpui-pre/src/app/bench_context.rs`. It reports rendering time,
invalidations, and foreground work, including work that does not draw a frame.
Use that infrastructure for a repeatable UI benchmark rather than adding another
clock-loop harness. Windows headless results do not include GPU submission; pair
those results with a real-window process/GPU profile.

## Next steps, in order

1. Establish the workload measurements above and a baseline artifact. Use visible
   tab reactivation latency to decide whether a small, bounded cache of recently
   used tabs is worth its memory cost. The current policy releases immediately.
2. Profile remaining root-owned controls. Split expensive regions into independently
   cached siblings where their invalidation ownership is clear. Keep changing
   cached ancestors from forcing all descendant caches to miss.
3. Add a shared memory budget if image/composite caches dominate. Account separately
   for document data, undo, composite intermediates, CPU display pixels, and GPU
   images; a count of open tabs is not a reliable byte budget.
4. Consider optional disk suspension for very large inactive documents only after
   defining transactional persistence of unsaved pixels, history, and in-flight
   jobs. It introduces I/O and reactivation latency and is not implemented here.
5. Evaluate the [layout reuse experiment](layout-reuse-experiment.md) against the
   workload matrix in both modes. Continue toward incremental
   framework rendering with explicit dependency tracking and upstream collaboration.

## The proposed persistent element tree

Stable element identities, layout reuse, and damage tracking could reduce active
frame work more deeply. Our pinned GPUI starts drawing at the root. Its standalone
default clears the Taffy layout tree for the next frame (`window.rs` and `taffy.rs`);
Emulsion enables the experiment that retains and reconciles layout nodes between
frames by default. Its explicit view cache requires compatible bounds, clip,
text style, dirty state, and refresh
state (`view.rs`). A persistent element tree must define which inputs invalidate
layout, paint, transforms, and inherited state independently.

That work also needs correct hitboxes, focus/action dispatch, text measurement,
scroll clipping, overlays, and removal of old elements. Merely retaining Taffy
nodes or ignoring cache bounds checks would risk stale geometry and input.
The first framework step now retains layout allocations and reconciles styles and
children, with fresh opaque measurement callbacks and bounds each frame. Eligible
non-wrapping text now refreshes its glyph/paint state separately and retains only
its numeric intrinsic size, allowing unchanged label geometry to reuse layout.
Wrapping, truncating, clamped, and custom measurements remain conservative. Differential
tests and same-binary benchmarks compare it with cold layout. See the
[experiment details and validation](layout-reuse-experiment.md) and the
[intrinsic-text release measurements](../specs/reports/intrinsic-text-release-results.md). Element rendering,
paint damage tracking, and stable component identities remain future work.

A retained element tree can use more memory even as it reduces CPU; it does not
replace inactive-document lifecycle management. The app-level changes here are
useful alongside that framework work.

## Document lifecycle validation

- `cargo test -p emulsion-ui --lib inactive_tab_tests -- --nocapture`: seven
  regressions passed, including both layouts, clock-driven animation suspension,
  image-reference deallocation, cache reconstruction, late tile completion
  rejection, Home/Settings navigation, timeline/replay suspension, and safe close
  during an unfinished pointer gesture.
- These tests establish lifecycle behavior; no real-window CPU or memory benchmark
  was performed. The workload matrix above defines the follow-up measurements.
- Complete compiled UI suite run serially: 433 passed, 1 existing ignored test,
  1 excluded stale landing-image dimensions assertion. Serial execution avoids
  the existing shared brush-catalog write contention documented in the CPU review.
- `cargo clippy -p emulsion-ui --lib --tests --no-deps --message-format=short`
  passed with the existing explicit-lifetime warning in `photoshop_shortcut_tests.rs`.
- New/changed small modules and navigation files passed rustfmt checks; diff
  whitespace checks passed. Existing unrelated module order in `editor.rs` remains.

## Additional interaction passes — 2026-09-30

Three further changes target work shared by previews, canvas interaction, and the
Layers panel:

1. **Display conversion:** `Raster::to_srgba8` encodes opaque 16-bit channels using
   a 64 KiB table of the exact results of the previous float conversion. It keeps
   the existing dark-tone curve and translucent-pixel arithmetic. Table access
   is prepared once per row. The table is initialized once per process.
2. **Layer search:** `Document::filter_panel_rows` builds a child index once and
   walks it in panel order. Search and kind filtering no longer repeatedly scan
   all nodes for each child/group. Matching descendants of collapsed groups keep
   their original IDs and depths; expansion state is untouched. No persistent
   document cache is introduced.
3. **CPU canvas sampling:** each output row retains the last sampled tile's bytes.
   Adjacent pixels in the same tile reuse its lookup, including missing tiles.
   The key distinguishes the current and before sides of a compare wipe. The
   sampler is local to one row in one frame, so edits cannot leave stale cache
   entries. Coordinate arithmetic, nearest-pixel selection, GPU-result passthrough,
   and exact CPU correction at ambiguous GPU sample boundaries are unchanged.

[`interaction_bench`](../../crates/emulsion-core/examples/interaction_bench.rs)
compares previous algorithms with production functions in the same release
executable. The canvas sampler is included directly from its UI source module;
this lets the CPU comparison run without a window or GPU. It asserts exact output
before timing, alternates old/new order, discards four warmup pairs, and keeps 31
measured pairs per case. Allocation is included; dropping the returned buffer is
outside the timer. Preview sizes are 1280×854 and 3840×2160, with opaque,
translucent, and dark pixels. Search fixtures contain 1,000, 4,000, or 10,000 raster
layers plus one group per 100 layers. Sampling fixtures cover 1280×720 and
1920×1080 at 0°, 33°, and 90°, including missing tiles and a halfway compare wipe.

These comparisons isolate CPU operations. They do not establish an equivalent
percentage gain in whole-window FPS, GPU rendering, or physical input latency.
The native `editor_canvas_bench` now also exercises a rotated 200% viewport through
the real editor, alongside pan, brush, vector edits, and zoom. Its frame boundary
is a platform callback, not a physical display measurement.

```powershell
cargo build --locked --release -p emulsion-core --example interaction_bench
cargo build --locked --release -p emulsion-app --example editor_canvas_bench --features canvas-bench
python scripts/bench-interaction.py --binary target/release/examples/interaction_bench.exe --native-binary target/release/examples/editor_canvas_bench.exe --runs 3 --output target/performance/interaction-results.json
```

The runner waits for Rust compilers before measurement, removes inherited GPU
overrides for the fixture, and records timings and source/executable hashes. The
native example uses a disposable application-data store and synthetic documents.

Validation for these passes: 165 raster tests, the new core hierarchy regression,
and 30 targeted UI/viewport tests pass (196 total). They cover all opaque u16
channel values and representative alpha boundaries against scalar conversion,
collapsed-group search and ordering, missing tiles and compare wipes, canvas
navigation, sidebar invalidation, and preservation of artwork during GPU fallback.
Clippy passes for all targets of `emulsion-raster`, `emulsion-core`, and
`emulsion-ui`, including the canvas benchmark feature, with the existing
platform-specific dead-code exception.

Three release processes measured 93 old/new pairs per CPU case in total. Every
case produced exactly matching output. Medians of the three process medians:

| CPU operation | Previous | Optimized | Median paired time reduction |
| --- | ---: | ---: | ---: |
| 4K opaque display conversion | 8.811 ms | 3.083 ms | 65.2% |
| 4K dark opaque display conversion | 14.232 ms | 3.168 ms | 78.1% |
| 4K translucent display conversion | 8.389 ms | 7.920 ms | 5.6% |
| Search 1,000 layers | 0.266 ms | 0.049 ms | 81.7% |
| Search 4,000 layers | 3.814 ms | 0.206 ms | 94.6% |
| Search 10,000 layers | 23.181 ms | 0.650 ms | 97.2% |
| 1080p CPU canvas sampling, 33° | 4.752 ms | 1.987 ms | 58.6% |

The other viewport cases improve by 57–60%; 1280×854 opaque and dark display
conversion improve by 57% and 75%. All 15 cases improve in the aggregate. The
percentage column is the median of the three paired reductions, so it need not
equal the ratio of the displayed medians.

These are **measurements in a busy development workspace**: the runner waited
for compilers before each CPU process, but other builds restarted during the
measurements and activity was detected at each process's completion. All samples
are retained. For example, the three 4K opaque reductions are 65.5%, 65.2%, and
65.0%; 4,000-layer search reductions are 94.5%, 94.6%, and 94.6%. These consistent
same-process comparisons support the targeted gains, but are not idle-machine
latency or whole-application frame-rate claims.

The native smoke runs also pass, with the GPU canvas both enabled and disabled.
Each covers five scenarios with 40 measured samples after eight warmups, verifies
the expected renderer, and records zero inactive-window samples. Painting also
checks committed pixels, undo, and redo. Rotated 200% views correctly use the CPU
fallback in both modes. Native tests ran during continuing build activity and
their timings are retained as smoke-test diagnostics, without a before/after
frame-time claim.

The complete CPU samples, native results, contention flags, and executable/source
hashes are preserved in [the interaction performance data](../specs/reports/data/interaction-performance-2026-09-30.json).
The native checks were appended to the completed CPU results using
`--native-only --allow-background-activity`; the data retains the CPU runner's
original source fingerprint and the updated native runner's hash separately.

## Design library retention: Linux AstroTag investigation

AstroTag is a 1600×900 Design project with 37 nodes. Its visible artwork compiles
into one native vector run without raster fallbacks. Offscreen GPU-complete pan
and zoom frames took approximately 0.65–0.73 ms on the local RX 7700 XT; the full
Design window spent considerably longer rebuilding the template library.

The Design library now has a retained render boundary, like the existing Layers
sidebar. Canvas navigation reuses its layout and draw commands. Editor and library
changes still invalidate it, and its allocated width follows the wide/overlay
breakpoint. Toolbar zoom commands notify the canvas and sidebar without
invalidating the library. No platform GPU backend or artwork representation changes.

Three alternating before/after pairs used the same release executable, unchanged
project, 1924×1068 device-pixel canvas, and 2× display scale on Linux/Wayland.
Compilation was paused for all six processes; ordinary desktop activity remained.
Each scenario discarded eight warmup frames and measured 40 frames, all with the
window active. Medians of the three process medians, from scripted input to canvas
submission (not physical display latency):

| Operation | Previous library | Retained library |
| --- | ---: | ---: |
| Pan | 8.33 ms | 2.25 ms |
| Wheel zoom | 7.14 ms | 1.80 ms |
| Object drag | 8.66 ms | 8.59 ms |
| Command move | 8.77 ms | 8.71 ms |
| Rotated 200% view, CPU fallback | 14.29 ms | 7.65 ms |

The native benchmark preserves the active page and project kind, exercises existing
Design artwork rather than adding synthetic paint/text layers, and dispatches zoom
through the real wheel handler. It no longer forces a full window refresh after
every input; that bypassed retained views and differed from shipping input handling.
For a same-executable reference, set `EMULSION_BENCH_UNCACHED_DESIGN_LIBRARY=1`.
This switch exists only with the `canvas-bench` feature.

```sh
cargo build --locked --release -p emulsion-app --features canvas-bench --example editor_canvas_bench
EMULSION_BENCH_UNCACHED_DESIGN_LIBRARY=1 target/release/examples/editor_canvas_bench /path/to/AstroTag.emu
target/release/examples/editor_canvas_bench /path/to/AstroTag.emu
cargo test --locked -p emulsion-ui --lib canvas_invalidation_tests -- --test-threads=1
```

All 13 navigation regressions passed, including library reuse, collapse/reopen,
overlay resizing, wheel/pinch and toolbar zoom, and live sidebar controls. The
implementation is shared across desktop platforms. Windows and macOS runtime
performance was **not measured for this change**; these Linux results do not imply
identical gains there. [Raw paired measurements and source hashes](../specs/reports/data/astrotag-navigation-2026-09-30.json)
retain all six run summaries and the unchanged fixture's hash. The private project is not
included in the repository.

## Diagram shadow retention: AstroTagIssue

The follow-up fixture is a different project: a 1600×1000 Diagram with 231 nodes
and nine drop shadows. Its SVG fallback was rebuilding Gaussian shadow filters
on the UI paint path at every zoom and pan. resvg permits intermediate filter
surfaces spanning five viewport widths and heights, plus working copies. The
native GPU alternative also exceeded the existing 128 MiB vector-target budget
at the larger native-window viewport; raising that budget would not bound SVG
filter memory.

Viewport scene construction now bakes each native drop shadow into a retained
image in document coordinates. Images use up to two samples per document pixel,
with a limit of 1,048,576 pixels and 2048 pixels per side (4 MiB of decoded pixels
per shadow). Unchanged subtrees retain their decoded images across revisions.
The primitive renderer skips PNG decoding during navigation and interpolates soft
shadow images while resvg renders foreground geometry at the current resolution.
Groups with opacity, clipping, masks or blending retain resvg's compositing path.
SVG/PDF export continues to emit scalable shadow filters. Very large effects use
lower-resolution preview shadows to stay within the image limit; foreground text
and paths are unaffected. Scene construction does more work once, on the existing
background worker, instead of during every navigation frame.

Large viewports containing retained shadows use at most four parallel horizontal
bands, writing into disjoint slices of one output buffer. This is enabled only
for trees without filters, masks, pattern paints or embedded SVG; those operations
can sample outside a band's boundary. Fractional zoom and rotation are compared
against rendering into a single surface to check for seams.

Regression coverage compares shadow appearance with scalable export, verifies
foreground geometry, image reuse and effect-edit invalidation, exercises nested
transforms and isolation boundaries, and checks image limits for large/thin bounds.
This implementation is shared across desktop platforms; runtime performance has
only been measured on Linux.

```sh
cargo run --locked --release -p emulsion-io --example svg_navigation -- /path/to/AstroTagIssue.emu
cargo test --locked -p emulsion-io --lib svg_viewport -- --test-threads=1
cargo test --locked -p emulsion-io --lib viewport_shadow
cargo test --locked -p emulsion-io --lib project_export -- --test-threads=1
```

At a fixed 1600×1000 pixel viewport, five pan frames at each zoom produced these
median render times, with no compilation running:

| Zoom | Previous SVG filters | Retained shadows |
| --- | ---: | ---: |
| 80% | 121.38 ms | 18.20 ms |
| 100% | 176.84 ms | 16.28 ms |
| 200% | 244.26 ms | 9.44 ms |
| 400% | 884.66 ms | 7.29 ms |
| 800% | 1813.03 ms | 5.94 ms |
| 1600% | 3804.54 ms | 5.69 ms |

Peak process RSS in this isolated renderer benchmark fell from 1,921,696 KiB
(1.83 GiB) to 66,068 KiB (64.5 MiB). Initial scene construction increased from
196 ms to 712 ms. These memory figures **exclude the full editor and GPU**.
The native editor benchmark used a 1680×1290 device-pixel canvas at 1.6× display
scale: median input-to-canvas submission was 21.32 ms for pan, 20.45 ms for wheel
zoom and 15.91 ms for rotated zoom. One of the 40 measured pan samples was
inactive; the other scenarios had no inactive samples. The full native process,
including GPU contexts and startup allocations, peaked at approximately 968 MiB
RSS. This is not a claim that total application memory is 64.5 MiB.

All 31 selected rendering, export, bounds and navigation tests passed. The
[raw measurements, limitations and source hashes](../specs/reports/data/astrotag-shadow-navigation-2026-09-30.json)
include both process-memory scopes and preserve the unchanged fixture's hash.
