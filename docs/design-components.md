# Reusable components

Create a component from editable Design objects, insert linked instances, and save named variants in the page's local library. Components may contain other linked components. Creating a component from an existing instance adds an outer group and keeps the inner link.

- **Update linked instances** publishes the selected instance's current artwork to that variant. It updates dependent nested component variants on this page, preserving matched native member IDs, each instance's top-left placement, and explicit property overrides. Unrelated variants are left alone.
- **Preserve object properties** appears for a selected instance or one of its children. Choose which local properties survive later publishing: text/image content; paint, typography and effects; position, size and shape; opacity; visibility. Appearance excludes opacity and visibility so those can follow the source separately. Content overrides support text and raster images; geometry overrides support individual text, path and image objects.
- **Reset selected instance** restores the saved variant and clears that instance's explicit override flags. **Switch variant** also resets properties. Clearing a checkbox alone permits the next update to replace that property; it does not immediately change the object.
- **Save as new variant** captures current artwork under a new name. Publishing includes the selected instance's current local edits, including properties it has marked to preserve.
- **Detach** removes the selected group's link and retains its editable artwork and any nested links.

Override flags describe property groups, not every individual style field. Rich text appearance preserves character runs when content matches; when source text changes, a retained appearance applies the old first-character style uniformly to the new text. Geometry overrides remain subject to responsive layout constraints. Removing a source member removes the corresponding instance member and its override flags. If an overridden member changes object type, publishing fails until its incompatible flags are cleared.

Dependencies must be acyclic and no more than 32 components deep. There are at most 128 components per page and 32 variants per component. Source groups remain hidden native layers, and their visibility is protected by document validation. Definitions remain available after their last visible instance is deleted.

All changes preflight affected locks and commit atomically in one Undo step. Locked objects in unrelated variants do not block publishing. Undo restores IDs, metadata, content and placement. Instances with movement links must be unlinked before reset or update. Component definitions, nested dependency libraries, member mappings and property flags persist in projects and clipboard fragments. Old documents without member mappings initialize them from matching hierarchy when first used.

Importing from another page or document copies its required dependency library into the active page. Name collisions receive numeric suffixes. Cross-page import and insertion form one Undo step; subsequent publishing stays local to the destination page. Project-wide and remote shared-library propagation are not implemented.

Cross-page import preserves native masks. Document-sized masks currently require the destination canvas to have matching dimensions; incompatible imports fail without changing either page. Raster-layer masks do not have that restriction.
