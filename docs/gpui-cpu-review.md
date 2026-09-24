# GPUI CPU review

*Snapshot from 2026-09-23. For current behavior see [performance-strategy.md](performance-strategy.md).*

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
| Editor ownership | Pointer overlays and marching ants formerly notified the same entity that built the panels. | Canvas redraws now notify a separate `CanvasView`; an independently cached sibling `SidebarView` observes editor notifications for document, tool, and panel changes. Visible Info updates explicitly invalidate the sidebar. |
| Layers list | Formerly built every matching layer and effect before scrolling. | Now uses GPUI's variable-height `ListState`: layers, effect headers, and individual effects render only in the viewport (plus an offscreen focused rename field). Domain IDs preserve scroll anchors; revision, rem, density, and topology changes invalidate measurements. |
| Document drawing | Composite trees and image tiles already have revision-aware caches. For unrotated magnification, movement avoids the expensive crisp screen path, then restores it after settling. Rotation still requires the screen path. | Preserve these optimizations; measure image processing separately from GPUI tree/layout work. |

The settle worker preserves the existing 90 ms quiet period before crisp
rendering, and does not cap interaction frame rate. A follow-up virtualizes the
Layers list and retains estimated heights in the vendored GPUI list across
initial layout and resize, allowing wheel scrolling to unmeasured items.
The list still prepares lightweight row identities in proportion to the layer
and effect count; expensive elements and thumbnails are restricted to visible
rows. Canvas and sidebar rendering now have separate entity boundaries.
The composition shell remains uncached: GPUI still visits the editor root, but
can reuse the sidebar subtree during canvas-only frames. Toolbars and other
root-owned controls still rebuild. Pan, zoom, and document edits continue to
notify the editor because other controls depend on their state.

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

### Canvas and panel invalidation follow-up

- Added sibling `CanvasView` and cached `SidebarView` entities. The sidebar
  observes editor notifications, so document, tool, and panel changes still
  rebuild it. Its geometry matches the regular and compact layouts, including
  automatic collapse at narrow widths.
- Pointer overlays, marching ants, zoom cursor modifiers, magnetic lasso
  previews, settled-image redraws, and completed tile batches notify only the
  canvas. Hidden marching ants no longer request frames. Visible Info updates
  notify the sidebar directly; hidden Info retains pointer state without
  invalidating its cache.
- `cargo test -p emulsion-ui --lib canvas_invalidation_tests`: 7 passed.
  Real pointer/key dispatch and the test clock verify canvas rendering while
  sidebar render counts stay unchanged. Coverage includes both layouts,
  document commands, visible/hidden/collapsed Info, and zoom cursor modifiers.
  Measured interactions do not force a global window refresh.
- The editor composition shell still runs and builds root-owned controls.
  This removes repeated sidebar construction/layout/paint work on canvas-only
  frames; it does not implement transform-only rendering in GPUI or establish
  a measured process CPU reduction.

- Full parallel UI run: 423 passed, 3 failed, 1 ignored. The landing test expects
  2172x724, while the bundled asset and its own import test use 2508x627. The two
  brush save failures pass separately and all 11 brush usability tests pass
  serially; parallel App contexts share one process-wide storage directory and
  can reject each other's stale catalog revisions.
- Clippy passed with the existing explicit-lifetime warning in
  `photoshop_shortcut_tests.rs`. New files and changed small modules passed
  rustfmt checks; unrelated module ordering in `editor.rs` was preserved.
- Re-ran the compiled UI suite with `--test-threads=1`, excluding only
  `tests::splash_dismisses_and_the_landing_image_opens_for_editing`: 425 passed,
  1 existing ignored test, 1 excluded stale landing assertion, no failures.
  This includes the seven new cache regressions and both brush save workflows.


### Inactive-tab lifecycle follow-up

Inactive tabs now cancel presentation timers/playback and release canvas display
images immediately, while preserving documents, undo, and background document
jobs. This fixes `TileCache::clear` retaining images in its deferred disposal
queue until an inactive tab next painted. Stale rendering completions cannot
repopulate a suspended cache. See [Performance strategy](performance-strategy.md)
for the implementation boundaries, benchmark workload matrix, and the separate
upstream work required for persistent element/layout trees and damage tracking.
