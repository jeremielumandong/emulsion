# Diagram completion work — September 28, 2026

Legacy Visio recovery is excluded at the user’s request. Existing modern imports
and reusable packs remain available; no legacy file recovery was attempted.

## Implemented and checked

- Indexed orthogonal routing with A* search, a one-million-cell memory bound,
  regression coverage above 64 obstacles, and toolbar/MCP warnings when no clear
  automatic route is found.
- Movement skips unchanged body-bound calculations, unrelated straight routes,
  unchanged secondary-label layout, and redundant metadata-target pruning.
- Structured UML/ERD field editors, native compartment rendering, default stencils,
  the UML template, direct label updates, rotation-aware reflow, save/reopen,
  draw.io round trips, undo and MCP operations.
- Saved shape/connector defaults, selection/view links, preview thumbnail bounds,
  and local comment threads with replies, resolution and deletion. UI/MCP share
  model commands; copied objects receive independent comments.
- Twelve additional draw.io dynamic geometries plus invisible anchor support;
  explicit HTML-table column/cell widths, padding, row heights, backgrounds and
  alignment; deterministic bundled SVG text fallback avoids symbol-font glyphs.
- 335 full core tests passed (one ignored), followed by 33 diagram-specific tests including review-reference retirement, 75 import/rendering tests, 18 diagram MCP
  tests and 20 diagram UI/project tests. All 18 template native round trips and
  vector-only exports passed. Proofs: `target/diagram-completion-review/index.html`.
- Draw.io corpus: 621/623 inputs and 821 pages; every imported page’s movement/undo
  check passed. Rejections are a URL index and an input with a missing graph node.

## Performance measurements and external limits

For 1,000 objects / 999 connectors, command median is 6.67 ms and only three SVG
layers rebuild. GPU command-to-completion median/p95 is 13.00/23.36 ms on the
RX 7700 XT. A concurrent-compilation run was substantially slower, so these are
workload measurements, not a universal frame-rate guarantee.

The native foreground benchmark timed out because the compositor did not retain
active-window state. Its isolated window was closed and previous focus restored.
No new native foreground drag/FPS claim is made. Physical display latency and
Windows/macOS runtime validation remain external verification gates.

Import is not pixel-identical draw.io compatibility. The sample corpus still has
unsupported dynamic/map shapes, sketch effects, some source font/caption and CSS
layout differences; import notes expose these. Arbitrary JavaScript shape code
is not executed. Missing/corrupt source data and remote image URLs remain source
limitations. These are not silently marked complete.

Machine-readable evidence: [diagram-completion-results.json](diagram-completion-results.json).
The compressed per-input audit is `diagram-completion-drawio.jsonl.gz`.

## Packaging

Final optimized package validation and installation: in progress.
