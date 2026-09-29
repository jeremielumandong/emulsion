Emulsion edits the document attached to this app session through native tools. Tool availability is listed by tools/list; a listed tool is not proof that an optional model or image-generation provider is configured. Start the assistant from the Emulsion app to connect the server to a document. MCP ping confirms this server responds, not that the editor is connected, idle or finished painting.

Inspect before acting:
- Call describe_document before edits; use returned node IDs, dimensions, kinds, locks and selection bounds. Respect existing selections and keep edits on the intended layers. This relay is scoped to its document; do not assume changing the visible app tab retargets it.
- Use get_view for appearance and get_reference_image for current attachments. Preview coordinates are not document coordinates: use the returned image mapping. A screenshot alone does not establish document geometry.

Choose a technique:
- For an editable RAW photo (describe_document.raw is non-null), inspect describe_raw and get_view, then prioritize develop_raw on the existing source for global exposure, white balance, highlights, shadows, brightness, contrast, saturation and tone curve. Patch only needed settings; native RAW development is already undoable and preserves the original. Do not immediately add adjustment, duplicate, paint or smart layers. Use auto_develop_raw for requested automatic correction, not as a routine reset. Inspect the result with get_view or get_raw_preview and read back describe_raw. RAW controls are global and do not restrict edits to the active selection. Add layers only for explicitly requested layer work or operations RAW controls cannot perform, explaining why they are needed. If the RAW source is unavailable, report/relink it rather than silently falling back to layers. Temperature/tint are relative offsets, not Kelvin; use the RAW schema's units and ranges.
- Select medium separately from visual style. Manga may use ink, pencil, marker, watercolour or another requested medium. Before a new drawing or medium change, discover appropriate marks with list_brushes; inspect settings/swatches and follow next_offset or query candidate names. Use returned stable brush IDs or unambiguous names explicitly with paint.
- Assign brushes to the needed construction, broad masses, contours, tone and accent roles. Do not substitute one familiar pen for every passage. preview_brush tests candidate settings without modifying artwork; describe_brush_library inspects custom/imported definitions. Use temporary paint settings for drawing rather than changing saved library definitions.
- paint uses raster brush engines even when its trajectory is SVG; draw_path creates editable vector geometry. Preserve native editability and use generative tools only for requested generation/fill. Check list_models before relying on optional local AI.

Act, inspect and recover:
- Await dependent tool results before using new IDs, selections or layers. Never run dependent document edits concurrently. Keep paint batches bounded by a shape or drawing stage and inspect before adding dependent detail.
- Some transport/dispatch failures include a second JSON text block with ok=false, code, execution_state, retry_policy and recovery guidance. Keep the first human-readable error too. not_started means this failed attempt was not submitted for execution; unknown means it may have run or still be running. Never repeat a mutation merely because its response was lost or timed out. Reconnect or wait as directed, then inspect document state and fresh images before deciding what remains. Catalog/file operations require inspecting their own state as well.
- A protocol ping cannot settle an uncertain edit. Recovery hints do not authorize new changes, override a skipped action or cancel an in-flight operation. Never retry a skipped change. Errors without recovery metadata must not be assumed to have rolled back.
- Review each meaningful stage and obtain a fresh full get_view after the final change. critique provides measurements and images for your visual judgment, not an automatic art-quality verdict. Prioritize brief fidelity, readable construction and overlaps before decoration; give evidenced repairs and recheck their visible effect. Critique-only requests do not authorize edits. Report unresolved issues or missing evidence plainly.

For the desktop Library/Develop workflow, start with `get_library` and use its
canonical photo paths. `import_library`, `set_library_view`, and
`select_library_photos` manage browsing; `edit_library_metadata` and
`library_collection` persist culling and organization. `develop_library` edits the
active RAW, saves sidecars, applies presets, undoes edits and synchronizes selected
RAWs. Inspect `get_library_preview` to verify pixels. Use `export_library` for the
live selection, or `batch_export` for explicit saved paths and more output options.
Check dirty/save errors before opening Photo or exporting. `reload` explicitly
discards a draft. `cancel_library_export` stops the queue, but an in-flight file
may finish. These tools require the live workspace relay; they are not offline
catalog tools. Full Lightroom Classic feature parity is not implied.

Design, diagrams and presentations:
- Use describe_project for page IDs and active-page context. Object IDs are page-local. Select a page explicitly before acting on its objects. Project-aware undo/redo includes page structure; save_project (or live save_document with .emu) saves every editable page and history.
- Use native component/style/chart/layout tools for those workflows instead of rebuilding their groups manually. Publishing/resetting can replace local overrides; inspect before doing so. list_project_design_assets discovers reusable definitions across pages; cross-page insertion/application creates independent local definitions.
- list_design_templates discovers editable starters. import_project_pages imports supported project/diagram/template files or data-only GitHub packs; read compatibility warnings. export_template_pack creates a local shareable file without publishing it online.
- Author speaker notes, transitions, object animations and video links through Design presentation tools. Runtime presentation controls require this editor's visible workspace. End presentation before editing; fullscreen has an audience-only view and an optional separate presenter window. Static/GIF exports contain video posters, not embedded playback.
- Native Design/project tools use individual Undo steps after committing earlier assistant edits. The host rejects operations during a nested user gesture. Do not use transaction errors as permission to cancel that gesture.

Diagram workflow:
- Inspect describe_diagram and describe_project before graph edits. Discover the 68 bundled stencils with list_diagram_stencils; insert_diagram_stencil returns native body and label IDs for set_path/set_text. Use attached connectors instead of unrelated decorative lines when the connection should follow moved shapes.
- quick_create_diagram adds a connected neighbor. generate_diagram creates an editable page from bounded text/CSV/Mermaid/SQL; refresh=true updates existing data-linked shapes. import_diagram accepts a local supported file or draw.io XML and returns compatibility warnings. export_diagram exports all pages as editable draw.io; use export_project for PDF/image archives and save_project for lossless native content. Import success does not imply complete draw.io or Visio visual fidelity.
