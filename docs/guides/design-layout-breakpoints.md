# Canvas width breakpoints

Use **Desktop**, **Tablet**, **Phone**, or **Custom width…** in the responsive
layout controls to preview the current page at another canvas width. The preview
uses native vectors and text, resolves the breakpoints for that width, and keeps
the page's height. It never resizes the authored page or adds an Undo/save change.
The preview header keeps width controls and **Exit preview** available; Escape
also exits. Editing gestures and shortcuts are disabled while previewing. Exiting
restores the editing viewport and selection. Starting presentation exits width
preview first. The presets are 1440, 768, and 390 pixels; custom widths are validated
against native document limits.

Select a responsive frame and open **Canvas width breakpoints…** in its responsive layout controls. Add a minimum canvas width, change the settings needed at that width, and apply. Each frame supports up to 16 entries. Remove an entry with **Remove**, then apply; normal Undo restores the entire edit. Draft edits and invalid values do not change the document.

The reference is the page's canvas width in document pixels. Window resizing, zoom, frame resizing, and fitting a frame to its contents do not affect breakpoint selection. The highest minimum width that is less than or equal to the canvas width wins. Below every threshold, the base frame settings apply. Thresholds may be entered in any order, but must be unique and between 1 and 100000 pixels.

Each matching entry inherits directly from the base settings, not from smaller breakpoints. For example, a 600px entry may set a 20px gap while a 900px entry changes only the flow: at 900px the base gap applies. The dialog shows the current canvas width and active entry. Blank numeric overrides inherit. Flow/alignment and boolean controls cycle through explicit values and **inherit**. Padding must supply all four sides or leave all four blank.

Supported overrides are flow, padding, gap, grid columns, wrapping, alignment, content-sized width/height, and content clipping. Frame min/max sizes and individual child sizing remain shared base settings. Every configuration, including inactive entries and its reachable nested-frame combinations, is validated before committing. Conflicting content-size/fill rules are rejected with an error. Breakpoints preserve native paths, text, source images, and normal save/clipboard behavior.

## MCP

`set_responsive_layout` accepts `clip_content` and `breakpoints`. Omitting `breakpoints` preserves the existing list; supplying an array replaces it atomically; `[]` removes every entry. Omitted or `null` override fields inherit the base. Unknown override names and invalid inactive configurations are rejected.

```json
{
  "group": 12,
  "flow": "column",
  "gap": 12,
  "clip_content": true,
  "breakpoints": [
    {"min_width": 600, "overrides": {"flow": "row", "gap": 24}},
    {"min_width": 1000, "overrides": {"flow": "grid", "columns": 3}}
  ]
}
```

Both `set_responsive_layout` and `describe_design_layout` report `active_breakpoint` and the resolved `effective_frame`. The persisted frame retains the base settings and all entries. No child ID overrides or separate breakpoint-specific artwork copies are created.

Live host tools `set_responsive_preview` (`width`), `get_responsive_preview`, and
`end_responsive_preview` expose the same temporary view. State reports the preview
size, active thresholds and effective frame settings. Offline servers report that
a running UI host is required; these tools do not create resized pages or exports.

The standard frame dialog's **Clip content to frame** control maps to `clip_content`. A breakpoint can inherit, enable, or disable that control without changing authored layer masks.

## Container rules and sizing overrides

A frame's **Responsive breakpoints** dialog can use canvas width or the immediate
responsive parent's inner width (its boundary minus horizontal padding). Top-level
frames use the canvas. Container queries reject content-sized width ancestors to
avoid a child deciding the size of the container that selects its own rule.

Each breakpoint can replace all four frame size limits. Switch **Size limits** to
Override; blank bounds are unrestricted. Inherit restores the base frame bounds.
To override one object's sizing, select the child and use **Position → Object
sizing at … px**. Its absolute/fill/min/max/aspect settings replace the complete
base child rule at that breakpoint; **Sizing: inherit base** removes the override.
Other children continue to use base settings. All inactive rules are validated too.
Copy/paste, duplicate, component reuse, native projects and Undo preserve child IDs.

MCP `set_responsive_layout` accepts `breakpoint_reference: canvas|container`.
A breakpoint's `overrides.limits` object replaces its min/max width/height; omit it
or use null to inherit. An empty limits object removes all bounds. `overrides.children`
maps immediate child IDs to complete sizing objects. Missing children inherit.
`describe_design_layout` returns the resolved query width and effective settings.

Use `cargo run --release -p emulsion-io --example design_layout_bench -- 1000`
for a reproducible native grid workload. It measures repeated edits, reports median
and p95 latency, and checks that a repeated layout pass leaves geometry unchanged.
