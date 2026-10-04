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

Photo group recall, Alt-click switching, explicit Rectangle menu behavior,
pending-shape cancellation and workspace isolation are covered by
`photo_tool_interaction_tests.rs`. Photo's exact brush option entry and timed
opacity/flow shortcuts have regression coverage in `editor/photo_numeric.rs`,
including compact/roomy chrome, focused text, native Linux symbol keys,
custom key bindings and supported brush ranges.

## Limits

Passing tests are evidence for the cases above, not proof of every brush preset,
setting combination, document size or hardware configuration. Headless UI tests
do not validate physical tablet drivers, native operating-system cursors,
screen-reader output, display/color calibration, or live AI-service quality.
Real-window renderer smoke tests and GPU parity tests cover separate rendering
contracts. Performance benchmarks are explicit ignored tests so timing variance
does not make correctness tests fail.


## Photo modal transform and repeat coverage

Run focused checks serially before the aggregate suite:

```sh
cargo test --locked -p emulsion-core history::tests:: -- --test-threads=1
cargo test --locked -p emulsion-ui --lib actions::tests:: -- --test-threads=1
cargo test --locked -p emulsion-ui --lib photo_repeat_transform_tests -- --test-threads=1
cargo test --locked -p emulsion-ui --lib transform_workflow_tests -- --test-threads=1
cargo test --locked -p emulsion-ui --lib clipboard_tests -- --test-threads=1
cargo test --locked -p emulsion-ui --lib context_menu_tests -- --test-threads=1
```

The modal tests assert deterministic copy IDs/allocator rollback, operation-wide
cancel, one Undo, identity behavior, D^n repeat copies, masks and editable content,
locks, invalid/shear preflight, unrelated-command exclusion, Save/export/page
safety, and actual GPUI shortcut/pointer dispatch. Keymap resolver tests separately
cover Ctrl/Cmd aliases, explicit user scopes, and empty-array unbinding.

Native QA must still exercise all four transform chords on the Photo canvas and
Layers panel, numeric/text/search/dialog focus, pointer release/focus loss,
Save/Save As/export/close during a provisional copy, and a committed native-file
save/reopen. Verify the repeat-transform reference fixture against the declared
document-space delta semantics. Headless coverage alone does not demonstrate
Photoshop-identical pivot behavior or current Photoshop runtime parity.

Selected-pixel non-affine regression coverage in `photo_repeat_transform_tests.rs`:

- `photo_selected_warp_and_distort_escape_restore_lift_and_existing_redo`:
  dispatched modes, immediate/held-pointer Escape, late release, exact source
  pixels/selection/allocator/revision/saved state, and existing Redo contents.
- `photo_selected_warp_apply_and_distort_noop_restore_modified_baseline_and_redo`:
  untouched/out-and-back geometry and Enter-before-Distort restore a modified
  baseline without losing the earlier Redo history.
- `photo_selected_degenerate_warp_and_singular_distort_roll_back_the_lift`:
  rejected lattice/quad geometry cannot leave a destructive pixel lift behind.
- `photo_selected_warp_and_distort_async_commit_undo_in_two_accurate_steps`:
  actual async resampling replaces only the selected-pixel layer; first Undo
  restores the lift, second Undo restores the original source/selection, and
  two Redos reproduce the committed result. This explicitly retains the legacy
  two-step projective workflow, so it is partial Photoshop workflow parity rather
  than the affine modal session's one-step operation-wide Undo.

## Photo locked-transparency Eraser coverage

`photo_locked_eraser_tests.rs` checks that Photo's Eraser replaces existing
coverage with the captured background color when the target layer or a parent
has Lock Transparency enabled. It covers fractional selection and opacity,
foreground/blend independence, unchanged brush settings, Undo/Redo, Escape,
window deactivation, raster/vector masks, Quick Mask, and Paint/Design/Storyboard
isolation. A tool-only alpha lock retains its existing erase no-op.

Raster `paint_locked_eraser_tests.rs` checks every 16-bit alpha value, unchanged
transparent pixels, all brush blend modes, color-only brush effects, dual-brush
replay, and CPU fallback when accelerators cannot represent the background
policy. Clear and Behind remain separate brush modes that do nothing under alpha
lock; their existing regressions remain in `painting_tests.rs` and raster paint
tests.

Focused commands (run UI tests serially):

```sh
cargo test --locked -p emulsion-raster --lib locked_eraser -- --test-threads=1
cargo test --locked -p emulsion-ui --lib photo_locked_eraser_tests -- --test-threads=1
```

The behavior follows Adobe's [Eraser documentation](https://helpx.adobe.com/photoshop/desktop/repair-retouch/clean-restore-images/erase-parts-of-an-image-with-the-eraser-tool.html)
and the separate [Clear/Behind restrictions](https://helpx.adobe.com/photoshop/desktop/repair-retouch/adjust-light-tone/blending-mode-descriptions.html).
These tests do not establish Photoshop-identical output for every brush preset
or hardware backend, or add Background Eraser, Magic Eraser, or Erase to History.
Native QA should additionally check visible background-color erasing and a
save/reopen round trip on opaque and soft-edged layers.
