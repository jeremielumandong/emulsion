# Website content audit

Reviewed 2026-09-24 against the current working tree. UI source takes precedence
when the application README still describes an older layout.

| Area | Source checked | Outcome |
| --- | --- | --- |
| Default shortcuts and macOS equivalents | `crates/emulsion-ui/src/actions.rs` (`DEFAULTS`, `platform_defaults`) | Expanded the shortcut list; separated image/canvas size; added fit, 100%, zoom, layers, save/export, and Photo/Draw controls. |
| Canvas navigation and focus | `crates/emulsion-ui/src/editor.rs`, `editor/rail.rs` | Documented Hand, temporary Space pan, Zoom modifiers, Rotate View, and canvas-only tool keys. |
| Personal shortcuts | `crates/emulsion-ui/src/settings_screen.rs` | Linked to Settings → Shortcuts for effective bindings and keymap edit/reload. |
| Current desktop layout | `crates/emulsion-io/src/settings.rs`, `crates/emulsion-ui/src/workspace.rs` | Removed the obsolete Layout → compact chrome setting; documented sun/moon theme buttons and Linux Omarchy control. |
| Layers and panels | `editor/menu_bar.rs`, `editor/sidebar.rs`, `editor/layers_panel.rs` | Replaced the old + Layer menu instruction with the current shortcut; distinguished the dock’s Panels menu from Window panel commands. |
| Home canvas creation | `crates/emulsion-ui/src/home.rs` | Located New transparent canvas in the Home ellipsis menu. |
| Assistant and generation | `crates/emulsion-ui/src/assistant.rs`, `editor/ask_ai_entry.rs`, `settings_screen.rs`, application README | Clarified Ask AI / F1 / Alt+F1 and provider selection in the Ask bar; retained provider access, approval, and billing limits. |
| Batch navigation | `crates/emulsion-ui/src/batch/preview.rs`, `batch.rs` | Kept its own pointer/toolbar controls separate from editor shortcuts; disclosed preview resolution. |
| RAW saving and formats | Application README, `docs/nikon-he.md`, `editor/raw_panel.rs`, `editor/raw_settings_ui.rs` | Added experimental Nikon HE/HE★ caveat, project-save conditions, and export gamut limitation. |
| History and replay | Application README, `editor/sidebar.rs` | Confirmed Timeline → Replay drawing and ORA versus RAW-sidecar persistence. |
| Installation | Application README, `scripts/install-appimage.sh`, `scripts/build-macos.sh`, `scripts/build-windows.ps1` | Existing platform commands still match; they build from source rather than claim downloadable binaries. |

Paths starting with `editor/` are under `crates/emulsion-ui/src/`.
The screenshot captions describe the supplied Home, Photo, Draw, RAW, and batch
captures. All workflow image slots now contain actual application screenshots. No platform binary availability or universal RAW camera
compatibility is promised.

Shortcut rows in `index.html` have `data-shortcut-action` and `data-shortcut-key`
attributes to make comparison with `actions.rs` explicit. When updating grouped
rows, also verify their additional alternatives (Alt+F1, Space, `]`, F7, F8).

## Follow-up: new documentation index and user guides

Compared the updated `docs/README.md`, `docs/files-and-environment.md`,
`docs/troubleshooting.md`, `docs/brush-workflow.md`, `docs/brush-import-formats.md`,
`docs/artwork-movement.md`, `docs/artwork-alignment.md`, and the current application
README with the site. Corrected these omissions or ambiguities:

- Added discovery links for the documentation index, troubleshooting, files and
  environment, renderer/compute controls, and brush MCP. Dated plans remain
  distinguished from current feature guidance.
- Added Brush Library/Studio, native brush exchange, and external conversion limits.
- Added artwork movement guidance distinct from view navigation.
- Clarified Flatpak as a local build alternative, Windows executable versus
  installer packaging, and the signing status of local desktop builds.
- Added RAW sidecar backup/rename/error behavior, missing-original restrictions,
  and retry guidance while development is running.
- Added batch view reset/preservation behavior and MCP catalog discovery.
- Added API-key precedence and zero-quota guidance, recovery entry points, and
  the credential-file caveat when collecting troubleshooting information.

The default shortcut rows remain consistent with `actions.rs`. The README still
describes `Settings → Layout → compact chrome` and the older `+ Layer` menu.
These are upstream wording discrepancies; the site retains the current UI
instructions confirmed in the first audit rather than copying them back.

Omarchy compatibility is highlighted in the workspace introduction and its own
capability card. The setup instructions match README → Appearance: Linux-only
live theme following via the top-bar control, no restart or hooks required,
and document colours unaffected.
