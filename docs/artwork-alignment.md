# Aligning artwork

Select a layer or group, choose **Move (V)**, then open **Align** in the toolbar.

- Choose **Canvas** to align to the document edges or center.
- Choose **Selection** to align to the bounds of an active pixel selection. This option is disabled until a nonempty selection exists.
- Each target offers left, horizontal center, right, top, vertical center, and bottom alignment.

Each alignment is one undo step. Repeating an alignment already applied adds no undo step. Alignment uses whole-pixel offsets, preserving existing fractional placements and source pixels. Centering uses the nearest pixel; when artwork and target dimensions have different parity, the center can differ by half a pixel. Repeated centering stays in place.

Groups retain their relative arrangement, including hidden children. Text and paths stay editable, and raster/Smart layers retain source pixels and local masks. Locked layers, ancestors, or group descendants prevent alignment. Canvas-wide fill and adjustment layers need a mask to provide spatial bounds.

Alignment uses artwork bounds, including enabled masks. If masks hide the entire object, it uses the underlying geometry. Document-space masks remain bounded by the canvas: an alignment that would discard mask detail is rejected. Enlarge the canvas first if necessary.

The assistant can use `align_node` with `node`, `alignment` (`left`, `horizontal_center`, `right`, `top`, `vertical_center`, or `bottom`), and optional `target` (`canvas`, the default, or `selection`).

See [Moving artwork](artwork-movement.md) for dragging, nudging, and snapping.

## Verification

- Workspace release library run: 389 passed, one UI test assertion failed, one live Claude CLI test ignored. The failed assertion depended on menu metadata absent from GPUI's test interface.
- Corrected the interaction test to check disabled behavior by clicking, and to dispatch keys to the focused window after a submenu opens. `cargo test --release -p emulsion-ui --lib alignment_tests`: all five passed, including the previously failing interaction test. Combined coverage: **390 passing tests, one ignored**.
- Ten added regressions cover all six directions and both targets, repeated alignment, fractional placement, mixed groups, source preservation, selection preservation, locks, mask clipping, undo, active-edit guards, MCP validation, and mouse/keyboard menu interaction.
- `cargo clippy --workspace --all-targets -- -D warnings`, formatting, and diff checks passed.
- Test logs: `/tmp/emulsion-alignment-tests.log` and `/tmp/emulsion-alignment-ui-tests.log`.
- `scripts/build-macos.sh` passed: release build, ad hoc signature verification, and DMG creation. Updated outputs: `target/macos/Emulsion.app` and `target/macos/Emulsion-0.0.1-arm64.dmg`. Build log: `/tmp/emulsion-alignment-package.log`.
