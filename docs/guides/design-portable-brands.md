# Portable typography, palettes and asset folders

The Brand drawer supports reusable typography roles and named palette collections. Roles store font, size, bold/italic, line height, letter spacing and an optional text color. Applying a role preserves text, paragraph geometry and native outlines. Leaving its color empty preserves the current text color.

“New palette / extract selection” starts with up to 32 unique colors from selected text, fills, paths, gradients and patterns. It does not sample raster photographs. Palette colors accept `#RRGGBB` or `#RRGGBBAA`; alpha `00` is transparent and `FF` is opaque. The application dialog explicitly targets fill, stroke, text or all paints. Applying a color replaces the chosen paint with a solid color and preserves the other targets. Locked objects reject the whole edit. Role, palette and brand application each use one native Undo step.

Use “Import portable font” on a kit to keep a local font with that kit, or “Embed font in selected text” to embed and apply it directly. Supported inputs are single-face TTF/OTF files of at most 8 MiB with editable embedding permitted by their font metadata. Font collections and bitmap-only/restricted-embedding fonts are rejected. No fonts are downloaded or installed into the operating system.

Embedded fonts receive a content-derived private family identity. Two files with the same original family name cannot replace each other or change text in another open document. UI font menus show the original family with an “embedded” label. CPU rendering, Vello and outlined SVG/PDF export use the same registered font bytes. Font-system generations refresh existing vector shaping caches when a font arrives.

A page supports up to 32 embedded fonts and 64 MiB of font data. The shared live font registry is bounded at 256 MiB. Font bytes use shared buffers accounted for by document history. Native page/history archives keep content-addressed font blobs and small manifest references instead of repeating base64 bytes through every saved history entry; loading validates blob size and digest. Old inline font resources remain readable. Clipboard, component imports, template packs and cross-page saved styles carry their font resources.

Native brand JSON v2 adds named roles, palettes and embedded fonts; legacy v1 kits remain readable. Logos are still local asset references and are not embedded into this brand JSON format. Removing a kit or an asset reference does not delete source files or copies already placed in a document. Existing documents retain their font copies when a kit is removed.

The assets drawer also supports nested creative folders, independently of Home project folders. Create, rename, move or remove folders, filter their assets, and move individual references with the asset menu. Removing a folder moves its direct assets and child folders to its parent. Cycles, missing parents and depth beyond 32 are rejected. Asset source files never move as a result of catalog organization.

MCP offers the same operations through `get_embedded_fonts`, `embed_design_font`, `extract_selection_colors`, `apply_palette_color`, `apply_brand_typography`, `apply_brand_kit`, `import_brand_font`, `remove_brand_font`, `set_creative_asset_folder`, `move_creative_asset` and `remove_creative_asset_folder`. `set_brand_kit` also accepts named `typography` and `palettes`. Catalog edits use the current `expected_revision`; document application uses normal protected-object validation and Undo. Live relay font/file reads run off the UI thread and reject application if the originating document changed while loading.

## Component edits and project variables

New component instances automatically retain native local edits as fine-grained
property overrides. The object’s **Preserve object properties** dialog can turn
tracking off or change the retained properties. Legacy files without the flag
keep manual tracking. Publishing follows unmodified properties; Reset restores
the source and clears overrides without changing tracking policy. The MCP
`set_design_component_overrides` tool accepts optional `auto_overrides` in the
same atomic edit.

The Variables panel supports **Share across project**, **Import from another
page**, **Publish value**, project-wide rename/removal and **Make local**.
Library identities survive native files, templates and object clipboard copies;
local aliases can differ across pages. Local edits remain local until Publish.
Publishing updates only matching identities and fails atomically for locked
consumers. Initial Share adds the variable to every current page; later pages
can import it. Unrelated name collisions are rejected without replacing values.
Removal retains resolved object appearance. Every project action has one grouped
Undo; detaching keeps the active page’s bindings but stops cross-page updates.

MCP provides `list_project_variables`, `share_project_variable`,
`import_project_variable`, `publish_project_variable`, `rename_project_variable`,
`remove_project_variable` and `detach_project_variable` through the live project
host. Import accepts a source page ID and a separate local target name.
