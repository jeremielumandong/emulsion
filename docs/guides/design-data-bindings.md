# Local CSV design generation

Use **Templates → Bind selected text/image** or **Object actions → Bind CSV data**
to save a CSV column mapping on an editable text object, raster image, or image
frame. Text bindings replace the whole object using its starting character style.
Inline `{{column}}` placeholders continue to preserve the surrounding rich text on
objects without a whole-object binding.

Image cells contain local file paths. Cover crops to the authored bounds, contain
fits inside them, and stretch uses both dimensions. Focal points control cover
cropping; rotation, flips, masks, layer styles, and frame clipping remain editable.
Importing a CSV sets its parent directory as the base for relative image paths.
Pasted CSV can use the folder field or absolute paths. URLs are not downloaded.

**Bulk create from CSV** copies the current template page or the whole ordered
page set for every record. Source pages stay intact. Internal slide links are
rewritten to the corresponding pages in each generated set. Importing those pages
rewrites the links again to their destination project IDs. A link to a page outside
the selected templates remains explicitly unavailable instead of accidentally
opening an unrelated page with a matching ID.

All rows and images are validated before insertion; one missing file or locked
bound object rejects the batch. Generation and image decoding run off the native
UI thread. Changing the originating project during generation prevents insertion.
One Undo removes the complete batch. Native projects, clipboard fragments,
duplicates and reusable components retain saved bindings.

Limits: CSV files up to 2 MiB, 64 columns, 4096 bytes per cell, the project's
100-page/total-area limits, and 32 million unique decoded image pixels per batch.
A record containing three template pages consumes three available pages. Static
pages can be included in a record set as long as another selected page has a field.

MCP uses `list_design_data_bindings`, `set_design_data_binding`,
`list_design_data_fields`, and `generate_design_pages`. The latter two accept
`template_pages` for an ordered subset (active page by default). Generation also
accepts `base_directory`. A null binding removes a mapping. These operations use
the same document validation and native Undo as the UI.
