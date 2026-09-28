# Interactive presentations

Select an object and open **Animate → Object click action…**. Add up to eight
actions in execution order, choose their targets, and save. Remove rows to clear
actions. Saving uses normal Undo; invalid targets keep the dialog open.

Available actions are next slide, previous slide, back to the previously visited
slide, a specific slide, show/hide/toggle an overlay, close the top overlay, and
switch a component instance to a saved variant. Slide navigation must be the final
action. Specific slides use persistent page IDs; deleted destinations report an
error when activated. Back follows the presentation's visited-slide history.

Overlays are top-level groups. Mark a selected group as an overlay or choose it
as an overlay action's target. Their authored visibility stays unchanged: they
are hidden only in presentation until opened. Open overlays are raised in order;
the top overlay is modal and blocks background click actions. Escape closes it
first; another Escape exits presentation. Removing overlay status also removes
incoming overlay actions on that page. Masks and editable contents are retained.

Click targets use native visible geometry, including clipped frames, in paint
order. A child click bubbles to the nearest object with an action. Object locks
protect authoring but do not disable a presented button. Existing keyboard slide
navigation and clean fullscreen remain available. Navigation stops media playback,
clears transient overlays/variant changes, and retains the back history.

Component variant switches affect an isolated preview, including its native text,
paths and media. They do not publish component edits or change authored document
history. Ending presentation restores the original editing page, view and selection.
Actions and overlay membership persist in native projects; clipboard operations
remap included object targets and drop links to objects outside the copied fragment.
Slide references remain project page IDs, so links copied to another project may
need a new destination. Static image/PDF exports show authored artwork, not an
interactive presentation runtime.

## MCP

- `get_presentation_actions` reads the active page's actions and overlay groups.
- `set_presentation_actions` replaces an object's action list; `[]` removes it.
- `set_presentation_overlay` enables/disables overlay status for a top-level group.
- `trigger_presentation_object` executes a visible object's saved actions in the
  running UI presentation. Offline servers return an explicit host-required error.

```json
{"tool":"set_presentation_overlay","arguments":{"node":20,"enabled":true}}
{"tool":"set_presentation_actions","arguments":{"node":12,"actions":[{"type":"overlay","target":20,"operation":"show"}]}}
{"tool":"set_presentation_actions","arguments":{"node":21,"actions":[{"type":"close_overlay"}]}}
{"tool":"set_presentation_actions","arguments":{"node":30,"actions":[{"type":"variant","target":40,"variant":"Active"}]}}
{"tool":"set_presentation_actions","arguments":{"node":50,"actions":[{"type":"slide","page":2}]}}
{"tool":"trigger_presentation_object","arguments":{"node":12}}
```

Live presentation state includes open overlay IDs and transient component variants.
These controls implement native click interactions; they do not execute arbitrary
scripts or embed a browser-based prototype runtime.

The interaction dialog also selects **Click**, **Pointer enters**, or **Drag and release**. Older files and objects without an explicit trigger keep Click. Hover runs once each time the pointer enters the object. Drag and release requires at least four screen pixels of movement and never moves authored artwork. Hidden objects and background objects blocked by a modal overlay cannot trigger. The same actions, triggers and remapped object references travel through native projects, copies and reusable components. MCP `set_presentation_actions` accepts `trigger: "click" | "hover" | "drag_end"`.
