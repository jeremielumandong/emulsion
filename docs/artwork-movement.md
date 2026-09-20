# Moving artwork

## Controls

- Select a layer or group, choose **Move (V)**, then drag it on the canvas.
- With the canvas focused, **arrow keys** nudge the selection by one document pixel; **Shift+arrow** moves ten pixels, independent of zoom.
- Hold **Shift** while dragging to constrain movement horizontally or vertically.
- **Escape** cancels the current move. Releasing the pointer commits it as one undo step.
- Snapping uses canvas edges/centres, guides, and other visible artwork. A group does not snap to its own children. Hold **Ctrl** to bypass snapping.
- Arrow keys in text fields retain their text-editing behavior. The nudge actions are configurable in the keymap.
- The assistant can use `translate_node` with `node`, `dx`, and `dy`. Positive offsets move right/down. `move_node` continues to mean layer-stack reordering.
- The compact **Align** menu positions artwork against the canvas or an active selection; see [Aligning artwork](artwork-alignment.md).

## What stays editable

Groups move their descendants together, including hidden content. Raster and Smart layers retain their source pixels and local masks; paths and text retain editable geometry. Document-space masks move with their corresponding artwork. Locked ancestors or descendants prevent a group move.

Each drag preview starts from the original document. Moving back to the starting point adds no undo step or unsaved-change marker. A newer edit ends the move instead of allowing an old preview to replace it.

Document-space masks are bounded by the canvas in the current file model. Moves that would discard mask data are rejected; enlarge the canvas first. Raster/Smart masks follow their layer placement and do not have this restriction. This update adds group movement, not group scale/warp handles.

## Verification

- `cargo test --release --workspace --lib --no-fail-fast`: **380 passed, 0 failed, 1 ignored** (live Claude CLI). This adds 19 passing tests to the preceding repair pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- Formatting and diff checks passed.
- `scripts/build-macos.sh`: release build, ad hoc signature verification, app packaging, and DMG creation passed. Output: `target/macos/Emulsion.app` and `target/macos/Emulsion-0.0.1-arm64.dmg`.
- Coverage includes mixed/hidden group content, source preservation, editable text/paths, mask movement and boundary protection, locked ancestors/descendants, snapping exclusions, keyboard focus, drag cancellation and saved state, secondary mouse buttons, newer edits during dragging, and the assistant tool.
- The full run exposed accepted mock sockets inheriting nonblocking mode on macOS. The OpenAI and UI image-server test fixtures now explicitly use blocking reads with timeouts; no production provider behavior changed.
- Full test log: `/tmp/emulsion-movement-final-tests.log`.
