# Creative catalog and workspace MCP

Creative catalog tools use the same local catalog as Home, the Design assets drawer and brand kits. Calls do not upload assets, download fonts, run repository scripts or purchase anything. File work runs off the UI thread in the live relay; completed catalog writes refresh Home and every editor in its workspace.

`get_creative_catalog` returns the revision, paginated asset references (offset/limit, at most 200), brand kits and collections. Before changing existing catalog metadata, pass that revision as `expected_revision`. Native writes reload under the catalog's OS lock and reject stale revisions or invalid references without partially writing the catalog.

- `register_creative_asset` registers an existing local image or logo. It preserves the original file.
- `update_creative_asset` changes a reference's name, tags, attribution or license, including installed templates.
- `remove_creative_asset` removes the reference and unlinks it from kits/collections. It never deletes the original or installed pack files.
- `set_brand_kit` creates a kit, or replaces a kit by ID, with name, font, RGBA palette and existing logo asset IDs. `remove_brand_kit` removes the kit only.
- `set_asset_collection` creates or replaces a named collection and its membership; `remove_asset_collection` removes it without deleting assets.
- `import_brand_kit` and `export_brand_kit` use the native brand JSON format. Version 2 contains named typography roles, palette collections and embedded font resources in addition to the kit name, font and palette; legacy version 1 remains readable. Logos remain local references and are not embedded.
- `install_design_template` validates a local `.emutemplate` pack and installs its content-addressed copy, or validates/references a local `.emu` project as a reusable template. Installing a pack repeatedly reuses its asset reference. Metadata edits/removal use the ordinary catalog tools. `export_template_pack` remains the portable export workflow; `import_project_pages` supports explicit GitHub package imports.

Portable fonts, typography roles, palette targeting and nested asset folders have additional [native and MCP controls](../design-portable-brands.md). Catalog writes are independent of document Undo. Every file argument is an absolute local path. Image/logo registration adds a reference; it does not decode, transform or place the image. Existing `import_image` and template-page import tools place content in documents.

`get_workspace_tabs` returns stable `tab_id` values, active tab, original relay tab, names, paths, project kinds and unsaved state. IDs remain stable when tab order changes and cease to exist when a tab closes.

`create_design_project` creates a Design or Diagram project in a new tab using native canvas validation, including dimensions, page count and bleed. Existing tabs and unsaved artwork remain open. `select_workspace_tab` changes the visible editor. Both return `origin_tab_id` and `active_tab_id`: selecting or creating a tab **does not redirect subsequent edits from the original relay**. To edit the new document, start/use its own editor relay.

`close_workspace_tab` closes only a saved, idle tab. Unsaved changes and unfinished edits return an error; save in the owning editor first. There is no force/discard argument. Other tabs with active assistant work cannot be closed through this tool. Requests are deferred until editor leases are released and checked against the original workspace and relay generation; stale or closed origins produce errors instead of targeting the visible tab.

Tests cover strict schemas and limits, stale revision rejection, invalid logo references, safe unlinking, source-file preservation, and the native create/select/close workflow with unsaved artwork and stable originating identity. Platform-specific UI checks remain separate from these native/headless regressions.
