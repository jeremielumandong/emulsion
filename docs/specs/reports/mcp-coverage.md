# MCP functionality coverage

Audit date: 2026-09-30. See the [current inventory audit](mcp-audit-2026-09-30.md) for registration checks, new tools and verification scope. This is a workflow audit, not a claim that every UI control has a tool. The authoritative callable inventory is MCP `tools/list`; tools use the same native models and validation as the editor. Optional AI models, image providers, native video runtimes, and platform codecs must still be available.

## Editing and authoring

| Workflow | MCP coverage | Boundary |
| --- | --- | --- |
| Document inspection and rendered previews | `describe_document`, `get_view`, `critique`, attached reference inspection | Relay targets the originating editor; changing visible tabs does not retarget it. |
| Native editor controls | `get_editor_state`, `set_editor_state`, `set_document_guides`, `editor_clipboard`, `get_editor_controls`, `set_editor_layout`, `manage_editor_workspace`, `canvas_gesture` | Live originating editor; bounded native gestures preserve Undo. Layout patches preserve omitted panels; preset persistence reports completion. Clipboard text is not read. |
| Playback setup | `get_playback_setup`, `open_playback_setup`, `install_playback_runtime` | Fixed supported Linux package plans with OS authentication; no arbitrary commands. Flatpak uses the shared runtime; Windows opens official setup through the dialog. |
| Native printing | `list_printers`, `get_printer_capabilities`, `preview_print_job`, `submit_print_job`, `open_print_dialog` | Immutable source snapshots; installed queues only; accepted does not mean physically printed. |
| Photo sources | `get_photo_source`, `replace_photo_source`, `crop_photo_source`, `restore_smart_source` | Retains full source resolution and native effects/filter stacks; transformed masks need reset before differently sized replacement. |
| Layer operations, transformations, selections, masks, clipping and blending | Existing native node/selection/paint/blending tools | Native canvas gestures use current-tool hit testing and reject stale revisions or active edits. |
| Raster painting and brush libraries | Paint/hatch, brush discovery/previews, brush catalog authoring and import/export | Native raster engines; external brush format limitations still apply. |
| Photo adjustments, smart filters, effects and AI | Adjustment/filter/style tools, local model discovery, configured image generation and removal | Provider/model availability and normal approval policy still apply. |
| RAW and photo Library | RAW describe/develop/compare/synchronize; Library folder/individual import, refresh, catalog removal, filters, selection, metadata, collection lifecycle, Develop/snapshot lifecycle, export and open tools | Library tools require a live workspace. RAW operations retain camera/source validation. |
| Editable text and vector shapes | Text/range/path formatting, shape geometry/styles, warp, appearance copy/apply, rounded corners, text backgrounds, alignment/distribution | Keeps editable sources; renderer/export fallback limitations are unchanged. |
| Design pages | `describe_project`, add/select/duplicate/delete/rename/reorder pages, responsive resize copies | Project-wide Undo/Redo, stable page IDs, native page limits. |
| Starter templates and data generation | `list_design_templates`, `add_template_page`, `list_design_data_fields`, `generate_design_pages` | Local bounded CSV and authored `{{field}}` text placeholders. |
| Responsive frames and constraints | Layout inspection/configuration/removal, child positioning/fill, resize constraints | Uses native frame layout/reflow and protected-object validation. |
| Image placement and frame crops | `import_image`, `place_image_in_frame`, `fit_frame_image` | Local native raster decoder formats; frame placement uses an existing raster node. |
| Charts and tables | List/add/update/detach native charts; table is a chart kind | Schema and native chart data bounds apply. |
| Reusable components and variants | List/create/insert/publish/reset/switch/detach, fine property overrides, save variant and explicit project publication | Cross-page family/member identity; scoped variant propagation preserves overrides and grouped Undo. |
| Reusable named styles | List/create/apply/publish/reset/detach/rename/remove | Preserves object identity/content. Typography sampling follows the native saved-style model. |
| Cross-page asset reuse | `list_project_design_assets`, `insert_component_from_page`, `apply_style_from_page` | Imports/placement are atomic and undoable; no invisible global synchronization. |
| Diagram authoring | Shape/stencil catalog and insertion, connected endpoints/ports, connector and shape metadata, layout, `get_diagram_formatting` / `set_diagram_formatting` | Native editable connectors; supported stencil subset matches core. |
| Diagram generation and data refresh | `generate_diagram_page`, `generate_diagram`, `quick_create_diagram` | Local text, CSV, Mermaid flowchart or SQL schema parser; never executes input. Refresh updates data-linked shapes. |
| Diagram/template/stencil import | `import_project_pages` | Local `.emu`, `.pptx`, Lottie JSON, packs, draw.io, supported Visio/Lucid formats; supported GitHub pack URLs. Compatibility warnings are returned. Binary Visio requires conversion. |
| Presentation authoring | Page notes/transitions/durations/FPS, object motion, YouTube insert/update/detach | YouTube keeps official embedded playback through the platform runtime. No bundled browser or stream extraction. |
| Live presentation | State/start/end/navigation/fullscreen, presenter timer, media play/pause/seek/stop/state, optional presenter window and automatic advance | Requires the originating visible workspace. Fullscreen audience hides controls; edits are blocked until End. |
| Persistence and output | `save_project`; live project-aware `save_document`; `export_project` for PDF/PNG/JPEG/SVG/GIF/draw.io/HTML/PPTX; `export_template_pack` | Saves all pages/history to `.emu`. Static/GIF video exports use posters. Files are local; no automatic online publication. |
| Project variable libraries | Share/import/publish, grouped rename/remove, detach and inspection | Stable identities preserve page aliases; publishing is explicit, atomic and undoable across pages. |
| Creative catalog | Asset references/metadata/folders, brand kits/logos, typography roles, RGBA palettes, portable fonts, collections, native brand import/export and local template installation | Revision-checked native catalog writes; original files remain unchanged. See [creative catalog tools](../../guides/mcp/mcp-creative-workspace.md). |
| Workspace lifecycle | `open_workspace_file` opens local artwork/projects asynchronously; list/select/close stable tab IDs; `create_canvas` for Photo/Paint/Design/Diagram with native dimensions, units, resolution, depth and background; `create_design_project` remains supported | Live host only; close rejects unsaved work. Replies retain originating relay identity. |
| Version branches and history | Branch/list/compare/merge; project-aware Undo/Redo | Branch history remains page-local; page structure uses chronological project history. |

## Explicit remaining gaps

These UI workflows do not yet have complete dedicated MCP interfaces. Generic object editing is not counted as full coverage for them:

- Saved alpha-channel authoring is not a native capability.

Live media play/pause/seek/stop and observed playback state are available through the UI host, along with presenter timer pause/resume/reset/state. Media commands are asynchronous and bounded; inspect readiness and pending commands before depending on delivery. Offline servers return an explicit UI-host requirement.
Diagram handles and connectors can be driven through `canvas_gesture`; semantic formatting, ports, markers and routing also have dedicated tools. Gestures operate on visible canvas points and the current native tool.

- Installing/configuring OS printer drivers and administering print queues. Installed-printer discovery/capabilities, print preview/setup dialog and explicit native queue submission are exposed. Physical printing still requires a configured device.
- Cloud/account configuration, credentials and sync conflict UI remain explicit user-facing setup flows.
- Desktop shell actions such as registering file associations and launching the lightweight image viewer. Presentation window control is exposed because it is an authoring workflow.

MCP exposes only functionality the native app implements.

See [native workspace/canvas controls](../../guides/mcp/mcp-native-controls.md) for layout, preset, gesture and playback setup semantics.

## Integration contract

`tools/list` publishes read-only hints from the same classification used by assistant approval policy. Destructive hints remain conservative for mutating tools. Registered native Design tools validate arguments and preserve normal locking/Undo rules; malformed requests must not partially change artwork.

Ordinary photo edits retain the assistant's existing grouped history. Before the first native Design/project operation, the live host commits the assistant's preceding edit batch and switches to individual native Undo steps. It refuses this transition while a nested user gesture is active. This is necessary for page history and component/style transactions; it must not close or cancel someone else's edit.

Project file IO and image decoding run off the UI thread. Import results are rejected if their target changed while loading. Saves acknowledge the actual written snapshot and mark only those saved revisions clean. All dependent relay calls remain ordered.

For regression tests see `emulsion-mcp` module tests and `emulsion-ui`'s `project_mcp_`/`presentation_host_` relay and presentation tests. Platform runtime testing on macOS and Windows remains separate from Linux/headless tests.

Previous-batch audit verification on Linux, 2026-09-28: 188 MCP tests and 64 focused native UI checks passed, including project/presentation/host workflows. Clippy passed for core, IO, MCP, UI and engine library/test targets with warnings denied. The release executable advertised 315 unique tools with valid required-argument declarations and read-only annotations. The 51.1 MiB AppImage passed its version smoke check. These checks do not imply coverage of the remaining UI-only workflows above; see [complete acceptance results](design-completion-results.json).

### Live Smart source sessions

`inspect_smart_source`, `open_smart_source`, `apply_smart_source`, `link_smart_source`, `refresh_smart_source`, `set_smart_source_auto_refresh`, `unlink_smart_source`, `save_smart_source_as`, and `write_linked_smart_source` cover native nested source editing and linked-file lifecycle. Filesystem work runs off the UI thread with source identity checks. Automatic refresh never writes files; explicit writes protect externally changed originals. See [source workflow and limits](../../guides/smart-object-sources.md).

Current editable-interchange/native-host acceptance is recorded in [interchange results](design-interchange-results.json). The previous AppImage and its 315-tool inventory predate these additions.
