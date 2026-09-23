# GPUI CPU review

Reviewed 2026-09-23 against Emulsion's vendored GPUI Kit 0.6.4 and
gpui-pre 0.3.5. This is a source audit and targeted redraw fix, not a measured
GPUI-versus-Qt benchmark.

## Why high frame rates can still cost CPU

Finishing a frame inside 8 ms establishes a latency budget, not low CPU usage.
Repeated construction, layout, text work, and paint preparation can keep CPU
cores busy even when the final drawing runs on the GPU.

In `vendor/gpui/gpui-pre/src/window.rs`, window drawing starts from the root.
In `view.rs`, ordinary entity children rerun rendering and layout. Explicit
`Entity::cached(style)` can reuse prepaint and paint ranges, but only while
bounds, clipping, inherited text style, dirty state, and refresh state allow it.
Moving a cached row during scrolling changes its bounds. A cache miss also sets
`window.refreshing` while rebuilding descendants, bypassing their nested caches.
Removing these checks would risk stale geometry and input hitboxes.

Normal div scrolling already notifies its owning view only when the offset
changes (`elements/div.rs`). Notification dirties ancestors too. A global
`window.refresh()` bypasses caching across the window.

Qt Quick retains scene graph geometry and can update a scrolling batch's
transform without re-uploading unchanged vertex buffers. This explains a
possible efficiency advantage, but does not establish a universal GPUI/Qt CPU
ratio, or identify which Qt rendering path the quoted mail app uses. See the
[Qt Quick renderer documentation](https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph-renderer.html).

## Concrete findings in Emulsion

| Area | Finding | Action |
| --- | --- | --- |
| Canvas settle redraw | Every prepaint while movement was pending spawned a detached 100 ms timer. Each timer notified the whole editor; induced frames could spawn more timers while movement continued. | Coalesce into one worker per tile cache. Recheck the latest movement deadline, wait again without notifying while moving, and notify once when the crisp image is due. Exit without notification if a subsequent frame no longer needs settling. |
| Editor ownership | `EditorView::render` builds the canvas, tools, panels, status, and other controls together. Pointer overlays and marching ants notify this same entity. | Next architectural improvement: give canvas/overlay animation its own entity and keep stable sidebar siblings independently cacheable. Use explicit data/events to preserve shared state correctness. |
| Layers list | Formerly built every matching layer and effect before scrolling. | Now uses GPUI's variable-height `ListState`: layers, effect headers, and individual effects render only in the viewport (plus an offscreen focused rename field). Domain IDs preserve scroll anchors; revision, rem, density, and topology changes invalidate measurements. |
| Document drawing | Composite trees and image tiles already have revision-aware caches. For unrotated magnification, movement avoids the expensive crisp screen path, then restores it after settling. Rotation still requires the screen path. | Preserve these optimizations; measure image processing separately from GPUI tree/layout work. |

The settle worker preserves the existing 90 ms quiet period before crisp
rendering, and does not cap interaction frame rate. A follow-up virtualizes the
Layers list and retains estimated heights in the vendored GPUI list across
initial layout and resize, allowing wheel scrolling to unmeasured items.
The list still prepares lightweight row identities in proportion to the layer
and effect count; expensive elements and thumbnails are restricted to visible
rows. The larger canvas/entity split remains a future improvement.

## Event documentation

The supplied versioned Event URL could not be retrieved; the official
[Event guide](https://gpui-kit.com/docs/event/) was available through search.
It describes Actions as routed commands and Events as typed reports of completed
changes. These APIs already exist in the pinned source. `emit` delivers events;
`notify` invalidates visuals. Replacing one with the other is not a rendering
optimization and can leave the interface stale.

The guide helps design narrower ownership: a canvas can notify itself for a pan
and report document changes to interested panels separately. It does not
announce automatic subtree retention or a transform-only rendering update.
The [coding guide](https://gpui-kit.com/docs/coding-guides/) also recommends
narrow notifications, virtualization, and explicit cache invalidation ownership.

## Validation and measurement

The settle regression tests simulate repeated frames and deadline changes without
sleeping. They check that only one worker starts, movement postpones the wakeup,
settling requests one redraw, and an unnecessary wakeup exits silently. A third
test exercises actual viewport prepaint when zooming out, rotating, and drawing
a settled frame before the worker fires.

For an end-to-end comparison, use the same release build configuration, document,
window size, DPI, monitor refresh rate, and hardware adapter before and after.
Record idle, continuous layers scrolling, continuous canvas pan/zoom, and the
tail after input stops separately. Include both a small document and a large
layer stack with expanded effects. Keep background exports and AI jobs idle.

Capture process CPU time divided by elapsed time (core equivalents), frame count,
and frame-time percentiles; state whether percentages are normalized to all
logical CPUs. Profile render/layout separately from raster tile work and software
GPU fallback. Repeat at the same refresh rate before comparing with any Qt app.
No CPU-percentage reduction is claimed from the regression tests alone.

Validation completed:

- `cargo test -p emulsion-ui --lib viewport:: -- --nocapture`: 8 passed,
  including 3 new regression tests.
- `cargo clippy -p emulsion-ui --lib --tests --no-deps --message-format=short`:
  passed.
- Viewport `rustfmt --check` and changed-file `git diff --check`: passed.
  Whole-file formatting of `editor.rs` reports pre-existing differences in
  unrelated action handlers; those existing edits were preserved.
- No real-window CPU benchmark or manual visual validation was performed.

### Layers virtualization follow-up

- `cargo test -p emulsion-ui --lib layer_`: 55 passed, including 7 new
  virtualization regressions. A 240-layer fixture constructs fewer than 40
  raster thumbnails initially. Another fixture scrolls through 240 effects
  across 20 layers, respecting the 12-effect limit per layer.
- Covered filter/rename, focused offscreen input, repeated keyboard edge
  navigation, range selection, effect expansion, drag/drop and undo, scroll
  anchors during group expansion, and density/rem/window resizing.
- Clippy passed with one unrelated pre-existing lifetime warning in
  `photoshop_shortcut_tests.rs`. New/refactored files passed rustfmt; diff
  whitespace checks passed.
- The vendor validation script reports 29 license hash mismatches caused by
  this checkout's CRLF line endings. All license hashes match the recorded
  originals after LF normalization; no license files were changed.
- These are headless UI construction and interaction checks, not a real-window
  CPU benchmark. Lightweight row identity preparation remains linear in the
  number of layers/effects.
