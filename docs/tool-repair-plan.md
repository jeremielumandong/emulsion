# Tool repair plan

Baseline: `750271a`, findings in [the audit](tool-audit-2026-09-19.md).

## Implementation

- [x] Protect edits: inherited locks; reject stale fill/heal/warp/distort/selection results; independent Smart filter requests; undo/cancellation ownership; magnetic cache dimensions; collision-safe batch export.
- [x] Repair painting: Clone first dab; clipped Heal; Liquify selection and Restore session; alpha preservation; document-space symmetry; complete brush preset persistence and mode switching.
- [x] Repair persistence: consistent Smart masks and legacy loading; layered PSD supported constructs plus explicit appearance-preserving fallback; standard ORA appearance without losing native editability.
- [x] Complete common Mac workflows: native pressure/tilt capture; pixel cut/copy/paste/clear; Command shortcut aliases; selected-pixel transform workflow; clear unsupported transform states.
- [x] Verify: targeted regressions, workspace library tests, Clippy, Mac app build; update audit with implemented changes and remaining limitations.

## Acceptance checks

- Saved masked Smart documents reopen; transformed/filtered masks retain their coordinate system.
- PSD and standard ORA reconstruction match the original visible image for the audit fixtures; unsupported editable structures have explicit fallback behavior.
- Locked ancestors prevent edits; late workers cannot undo newer user intent or close another gesture's transaction.
- Clone single clicks copy the chosen source; Heal/Liquify preserve unselected pixels; alpha lock preserves alpha; Restore reverses session warps; rotated-layer symmetry uses canvas axes.
- Preset dynamics survive saving/switching; persistence errors are visible.
- Selection replacement, cancellation, resizing, and magnetic tracing are safe.
- Batch output cannot replace inputs, existing files, or another item's output.
- Clipboard operations preserve selection coverage and document placement and undo correctly; text-input shortcuts remain usable.
- Mac pressure/tilt implementation compiles and has deterministic sample tests. Physical tablet feel, third-party application fidelity, and drawing-quality parity require hands-on evaluation and will not be claimed from unit tests.

This repair pass addresses the defects and everyday workflow gaps identified in the audit. Advanced feature expansion (CMYK editing, rich text, vector Boolean operations, ABR/dual brushes, and a dedicated GPU image-processing engine) remains separate product development.

## Implemented behavior

| Area | Change | Regression coverage |
| --- | --- | --- |
| Protected edits | Central ancestor/subtree lock checks, including indirect changes to clipping relationships | Core command lock cases; UI clipboard/group protection |
| Delayed work | Monotonic operation and selection tickets; undo invalidation; authoritative document snapshots; independent filter requests by layer; cancellation of replaced AI jobs | Fill after newer painting/undo, selection after deselect/resize, Smart filter concurrency and one-step slider undo, AI snapshot/refinement lifecycle |
| Painting | Clone offset before first dab; effective Heal coverage; preserved alpha; session-based Liquify Restore and selection clipping; full canvas-space brush symmetry | Real pointer-driven Clone/Heal/Liquify regressions, stale Heal transaction protection, raster alpha/coverage/symmetry tests |
| Brushes | Full preset parameter identity, persistent tool slots, explicit persistence errors | Preset dynamics and switching tests |
| Files | Source-space Smart masks, conversion/rasterization consistency, legacy mask/history migration | Native ORA/history round trips, transformed and padded filter masks |
| Interchange | PSD clipping runs/group masks and fractional placement; appearance fallback for unsupported PSD/standard ORA constructs | Layered and fallback image comparisons; standard ORA round trips |
| Batch | Exclusive collision-safe output naming, input protection, canceled-run ownership | Same-stem and existing-file regression |
| Mac input | Native AppKit pressure/tilt observation with fresh-sample fallback | Compiled native path and deterministic sample freshness tests |
| Familiar editing | Pixel Copy/Cut/Paste/Clear, selection-to-new-layer Free Transform, Mac Command aliases, 16-bit clipboard encoding | Clipboard placement/masks/precision, one-step undo, locked destinations, text-input dispatch |

### Format and interaction limits

- Native ORA manifest/history version is now 3. Current Emulsion reads older versions and migrates affected Smart masks; older Emulsion versions may reject newly saved documents.
- PSD export retains supported layers and masks. Adjustments, layer styles, and arbitrary clipping relationships require a flattened appearance layer; export status explicitly reports that fallback. Save native ORA to retain Emulsion's editable structure. Text and Smart content are not exported as Photoshop-native text/Smart objects.
- Free Transform lifts selected raster pixels to a new layer as one undoable edit; clipboard Copy includes the selected layer's visible appearance, mask, and opacity. Destructive Cut/Clear require a raster layer. Smart Warp/Distort asks for rasterization rather than offering an ineffective preview.
- Physical tablet pressure/tilt feel, actual external-app imports, live AI model/provider calls, and large-canvas latency still need manual evaluation. Headless tests do not establish artistic or performance parity with Photoshop/Procreate.

## Verification

- `cargo test --release --workspace --lib --no-fail-fast`: **361 passed, 0 failed, 1 ignored**. The ignored test requires the live Claude CLI. Counts: AI 38, assistant 18, core 37, filters 3, IO 34, MCP 45, raster 79, recipes 13, UI 94. This adds 44 passing tests over the audited baseline.
- `cargo clippy --workspace --all-targets -- -D warnings`: **passed**.
- `cargo fmt --all -- --check` and `git diff --check`: **passed**.
- `scripts/build-macos.sh`: release compilation **passed**; packaging completed with `--no-build` outside the sandbox after macOS iconutil failed inside it. Ad hoc signature verification passed.
- Artifacts: `target/macos/Emulsion.app` and `target/macos/Emulsion-0.0.1-arm64.dmg`.
- Local mock-server tests were rerun outside the sandbox because socket binding was denied. They passed without using live image-generation services. The Heal race regression was corrected to schedule the newer edit before GPUI drains background jobs; it then passed.
- Final logs for this session: `/tmp/emulsion-repair-tests-approved.log`, `/tmp/emulsion-repair-build.log`, `/tmp/emulsion-repair-package.log`.
