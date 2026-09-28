# Reusable components

Design's Elements panel can create a component from selected editable objects. A component has a persistent local library, named variants, and ordinary editable group instances. Browse project components lists the source page, component, and variant; it remains available when adding or switching pages.

- **Create from selection** saves a Default variant and links the selected group.
- **Insert** creates another linked native group. Text, vectors, photos, charts, clipping and layout metadata remain editable.
- **Update linked instances** publishes the selected group's artwork to instances of the same variant on the current page. It replaces child overrides, preserves each group ID, and retains the group's top-left placement.
- **Save as new variant** creates a named variant without changing other variants. **Switch** and **Reset** replace the selected instance's child edits with the saved variant. Resizing and rotation overrides are reset to the source geometry.
- **Detach** preserves the artwork and removes its component link.

Updates preflight all affected locks and commit atomically. Unlink movement links on an instance before resetting or propagating to it, so unrelated artwork cannot move. Each local operation supports Undo. Definitions and links persist in native project files, recovery snapshots, and clipboard fragments. Libraries survive deleting their last visible instance. Hidden native source groups hold editable originals and are excluded from normal scene display and page exports. Their visibility is protected by document validation. They can be inspected in Layers; deleting a source removes that variant and detaches affected instances.

Importing from another page creates an independent definition on the active page. Propagation is page-local, not project-wide. Cross-page import and insertion form one Undo step. Clipboard imports with colliding library names receive a numeric suffix so neither library is overwritten.

Nested linked components are rejected: detach inner instances before creating a larger component. There are at most 128 components per page and 32 variants per component. Child IDs may change on publish/reset; the instance group's identity stays stable. These are local reusable assets, without remote team libraries or automatic override merging.

Cross-page import preserves native masks. If a component uses a document-sized mask and the destination canvas has a different size, the import currently rejects without changing either page. Raster-layer masks do not have this restriction.
