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
