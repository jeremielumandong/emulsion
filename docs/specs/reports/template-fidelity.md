# Native UI handoff fidelity

The visual reference is `Emulsion Editor v2.dc.html` and the accompanying UI
specification from the supplied handoff. The application keeps native documents,
menus, keyboard actions, history and file operations. Prototype sample data is
replaced by the user's local records.

## Surface contract

| Surface | Native implementation |
| --- | --- |
| Shared frame | 36 px compact menu, 38 px document tabs, 24 px status. Existing Photo/Paint workspace customization remains available. |
| Home | 220 px navigation, 1240 px maximum content width, five launch cards, project previews, recent files and workspace filters. Below 640 px, navigation uses a popup and launch cards form two columns. |
| File cards/list | 16:10 thumbnails, workspace/pinned badges, project metadata, overflow actions. List headings and 44 px rows show name, project, workspace, file size and opened time; project/size columns yield at narrow widths. File size loads alongside thumbnails off the UI thread. |
| Design | 68 px rail with seven sections, 250 px drawer, 38 px canvas controls, 32 px selection toolbar and 88 px page strip. Document tabs align with the canvas column. Inspector opens explicitly. |
| Diagram | 250 px shape browser with native shape previews in four columns, 38 px canvas controls, selection/connection properties in the inspector and 32 px page tabs. Import, generation, layout, stencil packs and advanced graph controls remain available. |
| Photo/Paint | Existing 48 px rail, contextual options, 300 px dock and grouped Properties/Adjust/History/Assistant, color and layer sections. Saved toolbar layouts and advanced tools remain supported. |
| Library | Existing 220 px navigation, grid/preview/develop workflows, local collections and batch controls; settings overlay at narrow widths. |
| New Document | 880 px dialog, 200 px type column, proportional preset previews, categories/search and 260 px settings. Columns wrap and the body scrolls in narrow windows. |
| Appearance | Geist/Geist Mono, supplied light/dark palettes, six accents and saved corner choices. Built-in navigation icons are bundled offline. |

## Fidelity fixes found through native review

- Register the complete bundled icon source for dynamic workspace/file icons.
  Inline Design/Diagram SVGs explicitly set their paint color.
- Keep launch-card contents in a vertical column inside the native button;
  preserve keyboard activation and accessible labels.
- Paint narrow Design/Diagram drawers after the canvas so overlays stay visible
  and receive pointer input.
- Clip GPU presentation to the document. GPUI owns the themed stage outside it;
  the engine's fixed benchmark background must not replace that stage.
- Keep Photo/Paint workspace presets out of the Design/Diagram header, move page
  export into the shared header, and hide the first-run AI hint in project editors.
- Start project editors without rulers; the View command still enables them.

## Native adaptations

Home uses local projects, real file metadata and local storage wording. Shared
accounts, fictional collaborators and cloud quotas are omitted because this
release is local-only. Save and export retain native semantics. Single-click
selects a recent file for details/batch operations; double-click or its Open
menu action opens it. Native document tabs remain available above the canvas.
The optional inspector retains the shared Layers/Channels/Paths and advanced
panels rather than dropping those workflows.

This is a layout/control-fidelity pass. It does not complete responsive frames,
components, new stencil interchange formats, contextual AI authoring or the
remaining renderer/platform acceptance work in the separate feature plans.

## Validation

The native interaction suite exercises Home filtering/details/navigation, opening
photos, New Document validation/cancellation, Design selection edits and Undo,
Diagram creation/generation/Undo, and existing Photo/Paint/Library workflows.
Additional bounds checks cover project editors at 480, 800 and 1440 px in both
light and dark themes; existing Home checks cover 800, 1280 and 3840 px. Native
Linux capture is used to catch paint-order and asset issues that bounds tests
cannot detect. Windows and macOS runtime review remains on the separate machines.

The final workspace run passed 1,298 tests (13 ignored); formatting, Clippy with
warnings denied, and the Linux AppImage build passed. Earlier native captures
identified the fixes listed above. A fresh capture of the final build remains
pending because the desktop was locked with its display asleep during review.

The subsequent [starter/layout follow-up](../../guides/design-starters-and-layout.md) adds the
updated 110-template catalog in 11 categories, visible Design page removal and the handoff's
Photo/Paint shortcut strip with dockable flyouts.
