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
layout immediately for all open windows and saves the preference. The default is
off. Switching resets framework layout caches and refreshes windows, without
reopening documents or resetting their undo history. New windows inherit the
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
and the resulting view transform are asserted. GPU presentation is excluded. [Release measurements](canvas-navigation-release-results.md)
show 41-42% less CPU-side pan-update time in roomy chrome and 22-28% less in compact
from targeted notifications. Additional layout-reuse gains were mixed, so it
remains opt-in.

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
5. Evaluate the opt-in [layout reuse experiment](layout-reuse-experiment.md)
   against the workload matrix before enabling it. Continue toward incremental
   framework rendering with explicit dependency tracking and upstream collaboration.

## The proposed persistent element tree

Stable element identities, layout reuse, and damage tracking could reduce active
frame work more deeply. Our pinned GPUI starts drawing at the root. By default it
clears its Taffy layout tree for the next frame (`window.rs` and `taffy.rs`); the new
opt-in experiment retains and reconciles layout nodes between frames. Its explicit
view cache requires compatible bounds, clip, text style, dirty state, and refresh
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
[intrinsic-text release measurements](intrinsic-text-release-results.md). Element rendering,
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
