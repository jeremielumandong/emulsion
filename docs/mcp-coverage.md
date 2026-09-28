# MCP functionality coverage

Audit date: 2026-09-27. This is a workflow audit, not a claim that every UI control has a tool. The authoritative callable inventory is MCP `tools/list`; tools use the same native models and validation as the editor. Optional AI models, image providers, native video runtimes, and platform codecs must still be available.

## Editing and authoring

| Workflow | MCP coverage | Boundary |
| --- | --- | --- |
| Document inspection and rendered previews | `describe_document`, `get_view`, `critique`, attached reference inspection | Relay targets the originating editor; changing visible tabs does not retarget it. |
| Layer operations, transformations, selections, masks, clipping and blending | Existing native node/selection/paint/blending tools | Some interactive view and gesture controls remain UI-only. |
| Raster painting and brush libraries | Paint/hatch, brush discovery/previews, brush catalog authoring and import/export | Native raster engines; external brush format limitations still apply. |
| Photo adjustments, smart filters, effects and AI | Adjustment/filter/style tools, local model discovery, configured image generation and removal | Provider/model availability and normal approval policy still apply. |
| RAW and photo Library | RAW describe/develop/compare/synchronize; Library import/filter/selection/metadata/collections/develop/export/open tools | Library tools require a live workspace. RAW operations retain camera/source validation. |
| Editable text and vector shapes | Text/range/path formatting, shape geometry/styles, warp, appearance copy/apply, rounded corners, text backgrounds, alignment/distribution | Keeps editable sources; renderer/export fallback limitations are unchanged. |
| Design pages | `describe_project`, add/select/duplicate/delete/rename/reorder pages, responsive resize copies | Project-wide Undo/Redo, stable page IDs, native page limits. |
| Starter templates and data generation | `list_design_templates`, `add_template_page`, `list_design_data_fields`, `generate_design_pages` | Local bounded CSV and authored `{{field}}` text placeholders. |
| Responsive frames and constraints | Layout inspection/configuration/removal, child positioning/fill, resize constraints | Uses native frame layout/reflow and protected-object validation. |
| Image placement and frame crops | `import_image`, `place_image_in_frame`, `fit_frame_image` | Local native raster decoder formats; frame placement uses an existing raster node. |
| Charts and tables | List/add/update/detach native charts; table is a chart kind | Schema and native chart data bounds apply. |
| Reusable components and variants | List/create/insert/publish/reset/switch/detach and save variant | Publishing is page-local. Cross-page import makes an independent local definition. |
| Reusable named styles | List/create/apply/publish/reset/detach/rename/remove | Preserves object identity/content. Typography sampling follows the native saved-style model. |
| Cross-page asset reuse | `list_project_design_assets`, `insert_component_from_page`, `apply_style_from_page` | Imports/placement are atomic and undoable; no invisible global synchronization. |
| Diagram authoring | Shape/stencil catalog and insertion, connected endpoints/ports, connector and shape metadata, layout | Native editable connectors; supported stencil subset matches core. |
| Diagram generation and data refresh | `generate_diagram_page`, `generate_diagram`, `quick_create_diagram` | Local text, CSV, Mermaid flowchart or SQL schema parser; never executes input. Refresh updates data-linked shapes. |
| Diagram/template/stencil import | `import_project_pages` | Local `.emu`, packs, draw.io, supported Visio/Lucid formats; supported GitHub pack URLs. Compatibility warnings are returned. Binary Visio requires conversion. |
| Presentation authoring | Page notes/transitions/durations/FPS, object motion, YouTube insert/update/detach | YouTube keeps official embedded playback through the platform runtime. No bundled browser or stream extraction. |
| Live presentation | State/start/end/navigation/fullscreen, optional presenter window and automatic advance | Requires the originating visible workspace. Fullscreen audience hides controls; edits are blocked until End. |
| Persistence and output | `save_project`; live project-aware `save_document`; `export_project` for PDF/PNG/JPEG/SVG/GIF/draw.io; `export_template_pack` | Saves all pages/history to `.emu`. Static/GIF video exports use posters. Files are local; no automatic online publication. |
| Version branches and history | Branch/list/compare/merge; project-aware Undo/Redo | Branch history remains page-local; page structure uses chronological project history. |

## Explicit remaining gaps

These UI workflows do not yet have complete dedicated MCP interfaces. Generic object editing is not counted as full coverage for them:

- Creative catalog administration: saved brand kits/logos, asset collections and local template installation/catalog management. MCP can share/import package files and reuse document components/styles, but cannot manage the entire creative Home catalog.
- Interactive editor state: guides/rulers/snapping preferences, saved channel/quick-mask UI, clipboard integration, panel layout, tool selection and gesture-specific controls.
- Advanced smart-source relinking/edit-in-place workflows beyond existing conversion/filter tools.
- Specialized diagram formatting beyond the exposed shape metadata/conditional-fill fields.
- Device printing, print-preview dialogs and printer setup. Exporting a PDF is covered; submitting a print job is not.
- Cloud/account configuration, credentials, sync conflict UI and operating-system codec installation. These remain explicit user-facing setup flows.
- Desktop shell actions such as registering file associations and launching the lightweight image viewer. Presentation window control is exposed because it is an authoring workflow.

The app does not currently promise full Canva, PowerPoint or Lucid feature parity; MCP cannot expose functionality the native app has not implemented.

## Integration contract

`tools/list` publishes read-only hints from the same classification used by assistant approval policy. Destructive hints remain conservative for mutating tools. Registered native Design tools validate arguments and preserve normal locking/Undo rules; malformed requests must not partially change artwork.

Ordinary photo edits retain the assistant's existing grouped history. Before the first native Design/project operation, the live host commits the assistant's preceding edit batch and switches to individual native Undo steps. It refuses this transition while a nested user gesture is active. This is necessary for page history and component/style transactions; it must not close or cancel someone else's edit.

Project file IO and image decoding run off the UI thread. Import results are rejected if their target changed while loading. Saves acknowledge the actual written snapshot and mark only those saved revisions clean. All dependent relay calls remain ordered.

For regression tests see `emulsion-mcp` module tests and `emulsion-ui`'s `project_mcp_`/`presentation_host_` relay and presentation tests. Platform runtime testing on macOS and Windows remains separate from Linux/headless tests.
