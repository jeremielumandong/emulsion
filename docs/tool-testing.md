# Tool behavior tests

The tool suite combines algorithm unit tests with headless GPUI interaction
tests. Headless tests open the real editor and send pointer events, shortcuts,
and actions; they assert pixel values, selections, editable node data, undo,
redo, cancellation, and locked-layer behavior. Merely selecting a tool is not
considered coverage of its editing behavior.

Run the local tool suites with:

```sh
cargo test -p emulsion-raster -p emulsion-core -p emulsion-ui --lib --locked -- --test-threads=1
```

Run all workspace tests with:

```sh
cargo test --workspace --locked -- --test-threads=1
```

The existing CI workspace test step includes these tests automatically. Use
one test thread for the UI suite because its setup changes process environment
variables. No cloud credentials are required for ordinary tool tests; explicitly
ignored live-service tests and benchmarks are separate.

## Coverage map

Paths below are relative to `crates/emulsion-ui/src/` unless specified otherwise.

| Tool / mode | Observable behavior checked | Test files |
| --- | --- | --- |
| Hand / temporary Space pan | Pointer-following pan at rotation and zoom; no artwork or history changes; focus releases temporary pan | `navigation_functionality_tests.rs`, `tool_usability_tests.rs` |
| Move / transform | Layer/group/mask movement, nudges, locks, undo, Escape, stale edits, scale/rotate/distort | `movement_tests.rs`, `tests.rs`, `editor/tool_lifecycle_tests.rs` |
| Rectangle / ellipse selection | Mask coverage, undo/redo, additive/subtractive modifiers, moving selected area | `geometry_functionality_tests.rs`, `tests.rs` |
| Lasso / polygon selection | Interior/exterior mask pixels, Enter commit, Escape, unfinished-point deletion | `geometry_functionality_tests.rs`, `editor/selection_delete_tests.rs` |
| Wand / quick / magnetic selection | Color boundaries, replacement selection, edge tracking, stale-size rejection | `tests.rs`, `tool_safety_tests.rs`, raster `select.rs` |
| Brush / eraser | Painted or erased pixels, selection clipping, undo/redo, locks, pressure, taper, symmetry, alpha lock | `paint_functionality_tests.rs`, raster `paint.rs` |
| Smudge | Color transport and undo; wet pigment behavior | `paint_functionality_tests.rs`, `tests.rs`, raster `paint.rs` |
| Bucket | Connected versus global fill, selection clipping and undo | `paint_functionality_tests.rs` |
| Linear / radial gradient | Layer output and undo, endpoint behavior | `paint_functionality_tests.rs`, raster `fill.rs` |
| Liquify | Pixel displacement, restore, selection clipping, mode-specific sampling | `painting_tests.rs`, raster `liquify.rs` |
| Mask | Hide/reveal coverage, original color pixels unchanged, undo and locks | `paint_functionality_tests.rs` |
| Clone | Source sampling and copied pixels, undo, selection/alpha-lock behavior | `painting_tests.rs`, raster `paint.rs` |
| Heal | Blemish output and undo, selection exclusion, stale job protection, locks | `paint_functionality_tests.rs`, `painting_tests.rs` |
| Grade | Editable adjustment creation, exposure changes rendered pixels, undo | `tests.rs::tools` (see test name `grade_rail_adds_editable_adjustments_without_painting`) |
| Type | Editable text input, move/style behavior, typing-session undo/redo | `tests.rs`, `geometry_functionality_tests.rs` |
| Crop | Preview/cancel, committed dimensions, undo, centered crop and added-edge fill | `geometry_functionality_tests.rs`, `tests.rs` |
| Rectangle / ellipse shape | Rendered inside/outside pixels, constrained geometry, undo | `geometry_functionality_tests.rs`, `editor/tool_lifecycle_tests.rs` |
| Pen | Open/closed paths, anchor edits, path strokes, Enter and Escape | `geometry_functionality_tests.rs`, `tests.rs` |
| Eyedropper | Foreground/background sampled color, transparent-sample no-op, unchanged artwork/history | `navigation_functionality_tests.rs` |
| Zoom | Click anchor, Shift/Alt zoom-out cursor, double-click 100%, unchanged history | `tool_usability_tests.rs`, `navigation_functionality_tests.rs` |

Shortcut dispatch, text-field focus isolation, keyboard-operated controls, and
lifecycle cleanup have separate coverage in `tool_usability_tests.rs`,
`widget_accessibility_tests.rs`, `clipboard_tests.rs`, and
`editor/tool_lifecycle_tests.rs`.

## Limits

Passing tests are evidence for the cases above, not proof of every brush preset,
setting combination, document size or hardware configuration. Headless UI tests
do not validate physical tablet drivers, native operating-system cursors,
screen-reader output, display/color calibration, or live AI-service quality.
Real-window renderer smoke tests and GPU parity tests cover separate rendering
contracts. Performance benchmarks are explicit ignored tests so timing variance
does not make correctness tests fail.
