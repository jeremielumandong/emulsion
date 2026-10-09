# Photo workspace

Photo uses a familiar desktop image-editor arrangement: grouped tools on the
left, the active tool's options above the document tabs, and a panel dock on the
right. Emulsion keeps its own identity, editable documents and editing engine.
This is familiarity with industry-standard workspace conventions, not a claim of
complete feature or PSD file-format parity with other editors.

## Tools and options

- The Tools column toggle chooses one or two columns. Photo's two-column order
  reads left to right, then down. Short windows scroll instead of moving tools
  into additional columns.
- Related tools remain in their existing groups. Right-click a grouped tool or
  use its corner disclosure to open the other tools. Existing shortcuts are
  unchanged.
- Brush options put Size, blend mode, Opacity and Flow first. Hardness, presets,
  brush settings and other existing controls remain available in **More** when
  the row cannot fit them. Healing and other tools keep controls appropriate to
  their actual editing behavior.
- Selection options put New, Add, Subtract and Intersect together, followed by
  Feather or the active selection tool's tolerance controls. Other selection
  tools and advanced operations remain in **More**.
- Options overflow stays available at small window sizes and enlarged toolbar
  scales. The floating/custom-docked layouts keep their existing placements.

## Panels

The Photo dock has Properties, Adjustments and History tabs. Its panel menu
opens Enhance, Assistant, Info, Reference, Navigator, Histogram, brush panels,
Character and other existing panels. RAW Original remains available for RAW
files. The compact icon strip beside the dock has its own reserved space, so it
does not cover the image. Its flyouts can be closed or moved into the dock.

Layers, Channels and Paths remain together below. Layer blending, Opacity,
Fill and locks are available without opening the optional layer search/filter
controls. Selecting an adjustment layer puts its live adjustment controls at
the top of Properties. These controls still use the existing command/history
path, including Undo and Redo.

In the default compact layout, fresh Photo documents open on Properties. Saved
workspace choices still win.
Use **Window > Layout** to switch, customize, save or reset arrangements. Photo
respects the current light/dark theme and accent; its quieter surfaces and
selected states do not change the user's global appearance settings.

Paint, Design, Diagram and Storyboard retain their own layouts and controls.

## Scope

This pass changes the visibility, hierarchy and placement of existing controls.
It does not change shortcut mappings, introduce new tools, or extend
PSD import/export compatibility. Further parity work needs separate behavior
and interchange tests; matching a screenshot alone is not sufficient.

The reference conventions are described in the vendor's official documentation:
[workspace overview](https://helpx.adobe.com/photoshop/desktop/get-started/learn-the-basics/workspace-overview.html),
[customizing the toolbar](https://helpx.adobe.com/photoshop/desktop/get-started/set-up-toolbars-panels/customize-the-toolbar.html),
[marquee selections](https://helpx.adobe.com/photoshop/desktop/make-selections/get-started-selections/make-selections-with-marquee-tools.html),
and [painting tools](https://helpx.adobe.com/photoshop/using/painting-tools.html).
