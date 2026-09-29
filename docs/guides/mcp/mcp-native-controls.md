# Native workspace and canvas controls

These tools operate on the editor that owns the MCP relay. Switching the visible tab does not retarget that relay. `get_editor_controls` returns the current layout, available toolbar/tool/menu IDs, saved presets, layout schema and canvas/view coordinates.

`set_editor_layout` patches only supplied fields. Toolbar entries merge by ID; omitted toolbars and their properties remain unchanged. Tool order, menu visibility, sidebar tabs/splits/width, toolbar visibility/docking/position/scale, overlay placement and tool columns are supported. All IDs and bounds are validated before applying the patch. Layout changes do not alter artwork or document Undo.

```json
{"layout":{"sidebar_width":360,"sidebar_tab":"history","toolbar_placements":[{"id":"tools","edge":"floating","x":24,"y":80,"scale":1.25}]}}
```

`manage_editor_workspace` accepts `save`, `apply`, `remove`, `save_default`, `load_default` or `reset`. The first three require a preset `name` of 1–80 characters. A save replaces an existing name; up to 32 presets are supported. Settings use the native ordered background writer and return its completion or failure. Saving settings does not mark a document saved.

`canvas_gesture` takes 1–2048 points in document pixels, optional expected page/revision, click count and native modifier flags. Set the tool with `set_editor_state` first. One point clicks; multiple points press, move and release through native hit testing. This includes shape/brush/pen tools, selections, transform handles and diagram connector ports. Selected/hovered objects expose the same ports as the UI; semantic diagram tools remain available when a gesture is unnecessary.

The originating editor must be the visible active tab. Every point is checked against the visible canvas before input begins. Stale revisions, active gestures, pending edits and presentation/responsive previews reject the request without changing artwork. Native document gestures retain their normal Undo behavior. Receipts include pending edit, stroke and RAW flags: asynchronous native tools may still be working after input dispatch, so inspect state before depending on their output.

```json
{"points":[[100,100],[240,180]],"expected_page":1,"expected_revision":42,"shift":true}
```

`get_playback_setup` reports platform guidance, fixed package plan availability and whether installation is running. `open_playback_setup` opens the native dialog in the original workspace window. `install_playback_runtime` executes the supported Linux distribution's fixed package plan and reports actual completion; the OS may require administrator authentication. It accepts no command, package or shell arguments. Flatpak uses its shared GNOME runtime, macOS its system WebKit, and Windows its separately installed WebView2 runtime. Their setup guidance does not claim that host Linux packages repair a Flatpak sandbox.

These controls require a live editor host. Offline MCP returns an explicit host-required error. They do not provide arbitrary desktop input or access to other applications.
