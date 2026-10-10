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

## September 29: workspace comparison, guides and tutorials

- Added the `#guide-compare` article and its docs-nav entry. It condenses the
  canonical table in `docs/guides/workspaces.md`, whose "Where the numbers come
  from" section names the source file for every count.
- Each workspace article links its GitHub guide and tutorial:
  `photo-editing.md`, `paint.md`, `library-develop.md`, `design-overview.md`,
  `diagram-functionality.md` and the five pages in `docs/guides/tutorials/`.
- Corrected the Paint caption and tour copy from "170+ presets" to 55 built-in
  brushes (`crates/emulsion-raster/src/library.rs`). The screenshot may show
  imported brushes. The caption no longer says Paint has path layers, because
  the Paint rail has no Pen, Type or Shape tool (`editor/rail.rs`).
- Retitled `#guide-raw` as "RAW in Library". RAW files open in Library Develop
  (`workspace.rs`), and **Edit in Photo…** hands developed pixels to Photo. The
  Photo RAW panel is described only for older projects with an embedded recipe.
- Diagram export now lists draw.io, PDF, SVG, PNG and JPEG, and routing adds
  Cyclical (`crates/emulsion-core/src/diagram.rs`). The drawer tab is "Packs".
  The Design and Diagram row of the format table lists PowerPoint, HTML,
  Lottie, project export, and draw.io, Visio and Lucid import.
- The Brush library entry point is **Brush library…** in the brush panel's
  Brushes tab, not the Presets sidebar.
- The Design article adds whole-project export through **File → Export… →
  Project export**.
- Checked with `npm run build`. No application code changed in this pass.


## Library screenshots and preset workflow — September 30, 2026

- Replaced the Library showcase placeholder and the tour's earlier Batch capture
  with the supplied Library catalog screenshot (`Library2.png`). The recipes/export
  workflow also uses this current capture.
- Added the supplied Develop screenshots for imported presets and Before / After
  (`Library1.png`), and local masking (`Library3.png`). All captures remain
  unchanged at 2530×1377, with descriptive alt text and full-size links.
- Added Library panel guidance and a linked preset-import walkthrough. Checked
  labels and the separate import/apply actions against
  `crates/emulsion-ui/src/batch/advanced.rs` (`library_preset_bank`) and panel
  placement against `crates/emulsion-ui/src/batch/layout.rs`.
- Checked supported preset formats and compatibility limits against
  `docs/guides/library-develop.md`, in its preset interoperability section. The
  copy does not promise exact reproduction of proprietary third-party looks.


## Design poster capture — September 30, 2026

- Updated the Design showcase and tour to the supplied neon OMARCHY poster
  screenshot (`screenshot-2026-09-30_13-07-34.png`).
- Preserved the original 2530×1377 image as `design-poster.png`, with full-size
  viewing and alt text describing the poster, templates, text, paths and effects.
- Removed captions referring to the previous Product launch and six-page capture.


## Portrait editing capture — September 30, 2026

- Replaced the Photo showcase placeholder and earlier tour capture with the
  supplied `portrait.png`, preserved unchanged as `photo-portrait.png` at 2530×1377.
- Updated the initial tour image, caption, lightbox source, and Photo tab together.
  Descriptions reflect the visible Faithful recipe preview and adjustment layers.
- All five workspace showcases now use actual application captures; updated the
  showcase introduction and image-maintenance notes accordingly.


## RAW workflow ownership and application icon — September 30, 2026

- Checked the current root README's workspace comparison, “How they work
  together”, and “Developing RAW photos”, alongside `photo-editing.md` and
  `library-develop.md` in `docs/guides/`.
- Replaced the obsolete “PHOTO / DEVELOP & COMPOSE” workflow with Photo
  retouching, recipes, adjustments, and ORA saving, using the portrait capture.
- Moved the RAW development story into Library's workflow, using the current
  Develop capture. Preserved `#raw` as a Library anchor and explained the
  **Edit in Photo…** handoff and separate Library/Photo persistence.
- Corrected the Photo capability copy, added the README's Enhance panel to its
  guide, and clarified legacy Photo RAW migration as an independent Library copy.
- Replaced the square header/footer marks and SVG favicon reference with the
  application's multicolour icon. PNG and ICO assets are unchanged copies of
  `assets/icons/emulsion.png` and `assets/icons/emulsion.ico`.


## Printing workflow — September 30, 2026

- Added the supplied `printing.png` unchanged as `library-printing.png` at
  2530×1377, with a full-size link and a caption describing Library printing.
- Added a sixth practical workflow and a linked Printing guide in the navigation,
  with a cross-link from Library delivery instructions.
- Verified document and Library entry points, shortcuts, unsaved-edit snapshots,
  placement, contact sheets, poster tiles, and PDF layouts against
  `docs/guides/printing.md`.
- Kept the root README's distinction between included Linux/macOS/Windows
  adapters and physical output/platform acceptance still being validated.


## Home dashboard capture — September 30, 2026

- Replaced `public/assets/home.png` with the supplied Home dashboard screenshot,
  unchanged at 2530×1377.
- Added Home as the initial tour tab and synchronized its image, caption, alt
  text, tab-panel label, and initial lightbox source. The five workspace tabs remain.
- Captions describe the visible workspace shortcuts, project actions, and recent
  files. Project removal keeps source files, as documented in the Home guide.
