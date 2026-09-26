# Emulsion changes to gpui-pre 0.3.5

These changes remain under Apache-2.0. Original source notices are retained.

- `src/window.rs`: suppress high-input-rate presentation of unchanged scenes
  on software renderers, while retaining required presentations and dirty-frame
  draws. Refresh the renderer classification on forced redraw/device recovery.
  Adapted to this version's frame scheduler from the approach in AgentOps'
  Apache-2.0 GPUI fork (`src-gpui/vendor/gpui/src/window.rs`).
- `src/presentation_policy.rs`: extracted decision and regression test.

Archive and upstream revision information are in `../UPSTREAM.json`.

## Variable-height list estimates

`src/elements/list.rs` retains item height hints when invalidating measurements
for initial layout or a width change. Visible items are still remeasured. Dropping
all hints reduced the wheel scroll range to the few already measured rows,
preventing Emulsion's virtual Layers list from reaching unmeasured content.
The UI virtualization regressions cover scrolling through hundreds of layers
and effects after window resizing without measuring the entire list.

## Experimental cross-frame layout reuse

`src/taffy.rs`, `src/window.rs`, and the public exports in `src/gpui.rs` add
opt-in `Window::set_layout_reuse_enabled` and last-frame `LayoutReuseStats`.
The standalone framework default still builds a fresh layout tree each frame.
Emulsion enables reuse by default through its saved experimental setting, with a
live switch under Settings > Experimental > Reuse interface layout. An explicit
saved off choice is honored; `EMULSION_RETAINED_LAYOUT=0` or `=1` overrides the
starting mode without preventing live switching.

The experiment retains Taffy allocation slots and reconciles each frame's
converted layout style and ordered child IDs. Equal inputs preserve Taffy's
existing layout caches. Changed styles and topology invalidate through Taffy.
Slots are not element identities: render descriptions, hitboxes, event listeners,
and painting still follow their existing frame lifecycle.

Every opaque measured node receives a fresh callback and invalidation each frame.
Measurement callbacks initialize frame-local text painting state, so equal
measured dimensions do not authorize skipping them. Callback contexts are explicitly
removed at the end of each frame in both modes: Taffy 0.13's `clear` and `remove`
do not clear its separate context map. Retained-mode cleanup prunes unrequested
nodes, and independent roots detach stale previous-frame parent links. Absolute
bounds/origin caches are always cleared between frames.

Window-root automatic stretching remains conservative: restoring an authored
`auto` dimension and applying viewport stretch can invalidate the root each
frame. Wrapping text and custom measured leaves also invalidate their ancestor paths.
This is a layout reuse prototype, not a persistent element/fiber tree, paint
damage renderer, or proof of an application-wide speedup.

Differential Emulsion UI tests exercise geometry, topology, text callbacks,
DPI/rem changes, pointer dispatch, cache pruning, independent roots, and view
cache transitions. The optional `layout-bench` feature provides a same-process
cold/retained CPU rendering benchmark; see `docs/layout-reuse-experiment.md` in
the repository root for commands, limitations, and measured results.

## Intrinsic text layout specialization

`src/elements/text.rs`, `src/taffy.rs`, and `src/window.rs` extend the opt-in layout
experiment to text with `WhiteSpace::Nowrap`, no overflow/truncation, and no line
clamp. It shapes fresh glyph and decoration state during request-layout, then
supplies a pure intrinsic size to Taffy. Only numeric device-pixel sizes persist;
this does not retain text, colors, bounds, closures, or element identity. Equal
snapped sizes preserve Taffy's cached layout; changed size or node kind invalidates.

The intrinsic size remains a measured leaf, not an explicit CSS width/height, so
flex sizing keeps the existing semantics. Constraint-sensitive text and custom
callbacks keep the ordinary measurement path. Numeric contexts are explicitly
removed when pruning or resetting nodes, because Taffy does not clear its context
side map automatically. The specialization remains off when layout reuse is off.

Test/benchmark builds expose `Window::set_intrinsic_text_layout_reuse` to compare
cold, prior geometry-only reuse, and full reuse in the same binary. Diagnostics
include intrinsic requests/reuse and actual opaque callback invocations. Tests
compare glyph/font data, current paint decorations, DPI, empty/multiline text,
and transitions between intrinsic, wrapping/truncating, plain, and custom leaves.

`src/text_system.rs` also reserves decoration storage based on input run count,
capped at the previous 32-record reservation, instead of reserving 32 records for
every one-run label. This allocation reduction applies in all rendering modes.

## Application benchmark assets

`src/app/bench_context.rs` adds `BenchAppContext::with_assets`, available only with
benchmark support. Application benchmarks can install the normal bundled icons
before opening windows, rather than timing failed asset lookups. The editor
navigation benchmark uses this with the real GPUI Kit asset bundle. Production
application asset handling is unchanged.

## External textures (Linux canvas spike)

`src/scene.rs` adds `ExternalTexture`, an opaque application-owned GPU texture
handle, and carries it on `PaintSurface` on platforms other than macOS (where
the field remains the CoreVideo buffer). `src/window.rs` adds
`Window::paint_external_texture`. Renderers that cannot use the handle skip it;
only the wgpu renderer draws it (see `gpui-pre-wgpu`). Emulsion's application does
not use this yet: it exists for `spikes/vello-canvas`, which renders a canvas on
GPUI's own device and lets GPUI composite it without a CPU copy.
