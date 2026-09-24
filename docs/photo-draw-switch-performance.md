# Photo/Draw switch pauses

*Snapshot from 2026-09-23. For current behavior see [performance-strategy.md](performance-strategy.md).*

The switch handler previously saved settings synchronously before restoring the
next workspace. `app_state::update_settings` called `Settings::save`, which
serialized JSON, called `File::sync_all`, and atomically renamed the temporary
file on the UI thread. A slow storage flush therefore blocked input and drawing
in either switch direction. This is a concrete blocking path; the reported
intermittent pause has not been captured in a live profile, so disk latency is
not yet proven to be the cause of that particular occurrence.

## Change

Settings are applied in memory immediately and queued to a single background
writer. Writes execute in request order so an older mode snapshot cannot replace
a newer choice. Explicit workspace and native assistant stroke-preset saves use the same queue and report success
or failure after persistence finishes. A failed write or a dropped receipt does
not prevent subsequent writes. The application quit hook closes and attempts to drain the
queue within GPUI's existing shutdown time budget (200 ms); forced termination
or a longer pending write can still lose the most recent preference changes.

A settings write over 50 ms logs its duration. A Photo/Draw handler over 50 ms
logs total, settings, workspace-layout, and tool-switch durations under
`emulsion_ui::mode_switch`. These timings exclude the subsequent layout/paint
frame and GPU presentation; they are diagnostic, not FPS measurements.

## Other paths to distinguish

Entering Draw selects the Brush tool. If the outgoing painting tool has changed
library-brush memory, that separate catalog transaction can still perform
synchronous disk work. Switching from Brush back to Brush skips it. Draw shelf
catalog loading is also synchronous on first access but then cached globally;
no after-idle cache expiry was found. Brush preview generation already runs in
the background. Those paths have different transaction semantics and were not
changed without evidence that they caused this pause.

To investigate a remaining pause, run the updated release app with console logs,
repeat Photo/Draw switches immediately after launch and after idle or painting,
and note the active outgoing tool. A slow handler's `tool_ms` distinguishes the
brush path; a slow settings-write warning now describes background work. If the
handler is fast while the visible transition stalls, profile the next frame and
compare layout reuse on/off in Settings.

## Validation

- Full UI suite: 462 passed, one existing ignored test, one excluded known stale
  landing-image dimensions assertion
  (`tests::splash_dismisses_and_the_landing_image_opens_for_editing`).
- Three deterministic writer tests hold a save pending while foreground work runs,
  verify ordering and failure recovery, and verify worker draining. Integration
  tests cover rapid Photo/Draw switches and native stroke-preset saves mixed with
  later preferences. Workspace/document/undo and live-layout-switch tests pass.
- All four MCP stroke-preset tests pass, including the new in-memory operation.
- Normal app check, final release build, and UI clippy pass. Existing workspace
  question-mark and shortcut-test lifetime warnings remain, as does the Windows
  backend unused-import warning during release compilation.
- Scoped formatting and whitespace checks pass. A small return-type correction
  in the concurrently added progress card was necessary to compile its test
  observation wrapper; its two UI regressions also pass.

Logs: `target/mode-switch-{writer-tests,ui-suite,preset-tests,app-check,clippy-final,release-build}.log`.
Release executable: `target/release/emulsion.exe`. The running installed process
was not replaced or restarted. No live reproduction or before/after transition
latency measurement was performed; repeat the after-idle scenario in this build.
