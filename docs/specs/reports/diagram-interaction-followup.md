# Diagram interaction follow-up — 2026-09-28

## Changes

- Connector endpoint and bend previews share the committed route builder. Curved paths are hit-tested along the visible curve; dragging inserts or moves a waypoint without changing either attachment. Routing obstacles are captured once per gesture.
- A selected connector uses its routed line, square endpoint handles and segment/curve midpoint handles. A single connector no longer displays a generic transform rectangle. Explicitly selected text labels retain their independent editing behavior.
- Original bitmap assets no longer force an otherwise supported diagram off the GPU compositor. Unsupported appearances and SVG Smart Objects retain the fidelity-preserving fallback. The vector-only incremental update contract remains separate.
- draw.io tables inherit table/row text styling and vertical placement; CSS font lists resolve their first family. Inline rounded rectangles retain native curves and physical corner radii under nonuniform scaling.
- Visio formulas support numeric comparisons, conditional geometry and additional mathematical functions. Theme major/minor Latin font tokens resolve before installed metric-compatible font fallback. This is not complete ShapeSheet or multi-theme support.
- MCP stdio startup no longer initializes an unused GPU, which caused the packaged process to hang after EOF.

## Permanent template stencils

Fourteen Web Systems cards are built into the default Shapes catalog and exposed through existing MCP stencil discovery/insertion. Each retains editable vector icon artwork, template typography and colors. Default size is 288 × 148.

A verified portable pack was exported to `/home/arkane/Downloads/Emulsion Web Systems.emustencil` and installed in the actual application creative library as pack 74. Existing builds can use it through Stencil packs; the built-in category requires the updated binary. All fourteen documents were compared exactly after reopening the pack.

## Verification

- 36 core diagram tests passed, including preview/commit path equality across four routing modes and stencil insertion/undo.
- 48 draw.io tests and 22 diagram-import tests passed.
- 19 MCP diagram tests passed, including permanent card discovery, artwork, insertion and undo.
- 28 UI diagram checks passed after the selection-handle revision, including actual curved-line mouse dragging, endpoint movement, no connector transform rectangle, and undo.
- An optimized development AppImage candidate returned 357 MCP tool schemas and exited successfully at EOF in 0.38 seconds. It is a validation artifact, not a replacement for the installed release.
- GPU eligibility regression passed for native bitmap assets and unsupported masks.
- The installed AppImage opened the four-page web template collection in the native GUI. Its stdio server returned initialize/discovery responses, but hung on EOF with GPU startup enabled. The rebuilt executable returned both responses and exited successfully after the fix.

## Limits

Preview routes omit arrow insets and crossing decorations so attachment handles stay on their actual anchors. Full draw.io JavaScript shapes, browser CSS, arbitrary Visio formulas and unavailable source fonts remain outside exact-fidelity support. Development-build measurements are not release benchmarks or physical input-to-display latency.

The original object-drag benchmark did not establish/commit a genuine move reliably. Its drag timings are invalid and are not evidence of dragging performance. The harness now selects a known editable object, calls the actual move handlers, commits on release and verifies undo/redo before accepting a run.

## Verified native import benchmark

Fixture: `drawio-diagrams/blog/requirements-flow.drawio`, first page, 1,145 nodes, 4430 × 3780 document; 1873 × 1323 device-pixel viewport, scale 1.25. Optimized development build on Radeon RX 7700 XT / RADV Mesa 26.2.2. Each case collected 40 samples with zero inactive-window samples. Actual object movement committed successfully and passed undo/redo equality checks.

| Operation | Submission median | Submission p95 | Next-frame median |
| --- | ---: | ---: | ---: |
| Pan | 41.11 ms | 44.67 ms | 46.87 ms |
| Object drag | 19.17 ms | 20.30 ms | 25.03 ms |
| Command move | 10.86 ms | 13.04 ms | 17.55 ms |
| Zoom | 45.71 ms | 51.60 ms | 52.20 ms |

Move commit: 0.20 ms. Evidence: `target/diagram-interaction-review/verified-import.json`.

This fixture still selects the SVG fallback despite a GPU engine scene being available; engine startup logs alone do not prove GPU canvas selection. The mixed-bitmap eligibility change does **not** remove this fixture's fallback bottleneck. Further work is needed to accelerate unsupported SVG content during pan/zoom. No 60-fps or general performance-improvement claim is made from this run.

The final development AppImage, including the selection-handle revision, opened the same draw.io import in an isolated native GUI smoke run and remained running until deliberately closed. Packaged GUI connector gestures were not automated; those are covered by the native UI event tests above. The candidate is `target/diagram-interaction-review/appimage/Emulsion-0.0.3-x86_64.AppImage`; the installed release was not overwritten.
