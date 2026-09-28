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
| RAW saving and formats | Application README, `docs/guides/nikon-he.md`, `editor/raw_panel.rs`, `editor/raw_settings_ui.rs` | Added experimental Nikon HE/HE★ caveat, project-save conditions, and export gamut limitation. |
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

Compared the updated `docs/README.md`, `docs/guides/files-and-environment.md`,
`docs/guides/troubleshooting.md`, `docs/guides/brush-workflow.md`, `docs/guides/brush-import-formats.md`,
`docs/guides/artwork-movement.md`, `docs/guides/artwork-alignment.md`, and the current application
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


## September 28: Emulsion positioning and workspace showcases

- The primary artwork is the app's existing splash PNG, copied without changes.
- The hero and name explanation introduce distinct creative tools working
  together: Photo, Paint, Library, Design, and Diagram. This is not a claim of
  feature parity with other commercial products.
- The five new `showcase-*` media slots are explicitly labeled placeholders for
  upcoming showcase films. Existing application screenshots remain in the tour
  and detailed workflow sections.
- Workspace names and blank/template creation follow
  `crates/emulsion-ui/src/workspace/destinations.rs` and
  `crates/emulsion-ui/src/workspace/new_canvas_templates.rs`. Design/Diagram
  use the existing editable project types in `crates/emulsion-core/src/creation.rs`.
- Existing installation links, platform distinctions, and detailed capability
  limitations remain. The new Library guide link targets the existing recipes
  and batch guide.


## September 28: details for all five workspaces

- Replaced the three-view tour with Photo, Paint, Library, Design, and Diagram.
  Library's image is identified as an earlier Batch capture; Design and Diagram
  have explicit screenshot placeholders and working links to their guides.
- Capabilities and practical workflows now cover all five destinations. The
  documentation has one guide per workspace plus shared saving, format,
  installation, assistant, and troubleshooting topics. Existing anchors remain.
- Checked Library collections, culling, Develop, sidecars, history, virtual copies,
  export, and portable backups against `docs/guides/library-develop.md`.
- Checked Design templates, blank starts, layout, pages, presentation, and export
  against `docs/guides/design-starters-and-layout.md`, `design-presentation.md`,
  `design-selection-export.md`, and `workspace/new_canvas_templates.rs`.
- Checked Diagram shapes, bound connectors, containers, context actions, and
  saving against `docs/guides/diagram-functionality.md`.
- Checked Paint library/studio guidance against `docs/guides/brush-workflow.md`.
- Saving guidance distinguishes ORA layered documents, EMU page projects,
  Library sidecars/history, and EMULIBRARY backups. Page project storage is
  defined in `crates/emulsion-io/src/project.rs`. Photo RAW and Library Develop
  are described separately; native Design selection PDF export is distinguished
  from raster export paths that may use converters.
- Production build and Chromium review cover all five tour tabs, placeholder
  links, keyboard navigation, screenshot lightbox, installation dialog, and
  layouts from 320 to 1440 pixels. No application code changed in this pass.

## Supplied Design showcase capture

The user-supplied 2530×1377 Design workspace screenshot now fills both the
`showcase-design` slot and the Design tour tab. It shows the template library,
editable product-launch artwork, page thumbnails, and presentation controls.
`design.png` is copied unchanged; full-size viewing remains available. Diagram
continues to use an explicitly labeled screenshot placeholder.

The supplied Diagram capture also fills `showcase-diagram` and the Diagram
tour tab. It shows a credit approval process with swimlanes, shapes, connections,
and properties. `diagram.png` is copied unchanged. Both supplied captures
support full-size viewing; the remaining film placeholders stay explicit.
