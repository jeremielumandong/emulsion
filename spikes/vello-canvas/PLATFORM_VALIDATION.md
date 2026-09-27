# Migration platform validation

Run this branch in an interactive desktop session, with Rust and the repository's
normal build dependencies installed:

```sh
bash scripts/validate-vello-migration.sh
# Optionally benchmark a specific saved document (opened into disposable state):
bash scripts/validate-vello-migration.sh /absolute/path/document.ora
```

The script requires GPU tests to execute (adapter failures cannot silently skip),
runs core/UI/fidelity tests, then opens the actual `EditorView` in a temporary
data store. It prints the report directory. The release benchmark exercises real
pan/brush handlers and text commands, including canvas submission through GPUI.
It records whether the GPU brush actually ran. JSON times end at submission or
the next platform frame callback; neither measures physical display latency.
The default fixture is a synthetic 4K document with 24 raster layers, editable
text and a paint layer. It differs from the original `layers-4k.ora` spike fixture.

On Windows, run the cargo commands from the script in PowerShell; the executable
is `target/release/examples/editor_canvas_bench.exe`. Set
`$env:EMULSION_REQUIRE_GPU_TESTS = "1"` before tests, and set
`$env:EMULSION_GPU_CANVAS` to `"1"` and `"0"` for the two benchmark runs.

Manual checks still needed on **macOS/Metal**, and a follow-up on Windows for the
new brush/recovery code (the recorded Windows results predate this patch):

- Open the shipping app; confirm GPU canvas activation in its log. Pan and zoom
  with rulers and pixel grid enabled; resize and move between differently scaled
  displays. Check IOSurface/D3D12 presentation for stale or black frames.
- Paint with the default brush, hold to trigger QuickShape, erase, cancel, undo
  and redo. Save/reopen the project and compare the committed stroke. Repeat with
  pressure dynamics, selections, masks and transparency locks, which retain CPU
  brush handling. Close during a stroke: the unsaved-change prompt must appear.
- Copy a whole text layer, paste in the same and a different tab, enlarge it,
  and verify it remains editable. Selection/mixed-layer and external copies use
  the portable PNG representation.
- Add an adjustment or layer style to an already active GPU document. Verify the
  CPU-canvas badge, correct pixels, and recovery after undo. Translucent vectors
  should show compatibility rendering and match the CPU canvas's colors.
- Suspend/resume and, where safely available, exercise real device replacement.
  The automatic test destroys an owned engine device and recreates it; it does
  **not** simulate a reset of GPUI's shared device or the OS compositor.

Record OS/driver/hardware, actual canvas dimensions and display scaling, branch
commit, JSON reports, screenshots and failures in `RESULTS.md`. Do not mark macOS
validated based only on Linux tests or cross-compilation.
