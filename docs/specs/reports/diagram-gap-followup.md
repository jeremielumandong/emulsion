# Diagram attachment and import follow-up

## Implemented

- Picked connector points are transformed along with their shape, then stored in
  the new bounds. Rotation and reflection therefore preserve the actual picked
  position, including transforms whose bounding box does not change. Move, resize,
  undo, native persistence and MCP use the same core representation.
- Visio lines without complete glue metadata are now native connectors. Existing
  glue is retained. An endpoint within two document pixels of exactly one clear
  outline can be inferred; ambiguous contacts remain free. Free endpoints are
  invisible children of their connector, excluded from layout and stencil packs,
  so connector moves include them. Source paths and manual bends are retained.
- Numeric Visio formulas support arithmetic, parentheses, local/inherited cell
  references, GUARD/THEMEGUARD, ABS, SQRT, trigonometry, MIN/MAX and common length
  and angle units. Evaluated source values take precedence. Evaluation is bounded
  by depth and operation budgets, with no script or external reference execution.
- Explicit RGB and THEMEVAL palette references use the packaged theme when there
  is a single unambiguous theme. Unresolved themed colors can inherit a concrete
  master/style value. Multi-theme assignment and full theme style matrices are
  still outside this subset. Semantics follow Microsoft's
  [THEMEVAL reference](https://learn.microsoft.com/en-us/office/client-developer/visio/themeval-function).
- draw.io fixed-layout HTML tables retain equal unassigned column widths,
  requested minimum table height, border color, width and dashed/dotted styling.
- The iOS app bar stencil now renders its status icons as editable vectors using
  the already pinned and attributed upstream definition, rather than a rectangle.
- The native window benchmark can use a real imported diagram, without adding
  synthetic paint/text layers to it, and reports pan, drag and command-move timing.

## Scope and remaining format differences

This is not a claim of full draw.io/Visio rendering parity. Arbitrary JavaScript
shape definitions, browser CSS layout, every ShapeSheet function, multiple theme
assignments and all source routing algorithms still require dedicated support.
Missing fonts still need substitution; source bitmaps retain their original
resolution. External images are not fetched automatically. Existing native files
keep their original imported lines; reimport the source to use the new connector
conversion. Legacy binary Visio recovery remains excluded as requested.

## Validation

The draw.io sample audit imported 621 of 623 files and exercised movement/undo
successfully on every imported diagram. The two unchanged source failures are
`blog/er-diagram-library.drawio` (a missing object reference with no endpoint
coordinates) and `blog/template-index.xml` (an index, not a diagram).

All 22 diagram UI tests passed. The full IO suite passed 348 tests, with two
ignored and one unrelated OpenRaster limit-test failure: that test constructs
100,000 nodes but the application now allows 150,000. The changed diagram cases
passed. Final targeted checks passed: 34 core diagram tests, 21 diagram-import tests and
22 diagram UI tests. Optimized build, AppImage packaging and version smoke test passed.
Native frame-latency measurements remain unverified: the desktop kept the
disposable benchmark window inactive despite explicit focus requests. No timing
results from those incomplete runs are accepted. No desktop configuration was changed.

The Value stream map passed native save/reopen and movement/undo checks. Its SVG
export has no whole-page raster fallback. Visual review confirms the main layout
and labels; embedded resource-card images differ from the reference PNG because
the VSDX contains different images, and Arial uses Liberation Sans.

Build and review artifacts are in `target/diagram-gap-review`.
An isolated checkout excludes unrelated photo/batch work in the shared workspace.

The 913-shape, six-page requirements-flow sample also passed native persistence and
movement/undo verification and exported without a whole-page raster fallback.

## Rendering and delivery

The 1,000-shape/999-connector incremental SVG benchmark rebuilt at most three
layers per move. Measured p50/p95: editing 7.70/9.01 ms, scene updates 7.65/8.08 ms,
and patch rendering 1.44/1.52 ms. This run overlapped compilation; it is a bounded
renderer check, not a physical input-to-display latency measurement.

Installed `/home/arkane/Applications/Emulsion.AppImage`. Restart to use the update.
The build uses baseline commit `3ab79c3`, retaining the installed photo/sidebar
update while excluding unfinished photo/batch changes in the shared workspace.

Backup: `/home/arkane/Applications/Emulsion.AppImage.bak-diagram-gaps-20260928T182418966983Z`.
SHA-256: `17543c2450edce0d3c1b446454a3536e6af06882a0e0147137ada61ce1810c2f`.

Updated editable sample: `/home/arkane/Downloads/Value stream map example - connector update.emu`.
Review previews: `target/diagram-gap-review/index.html`.
