# Canvas width breakpoints

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

The standard frame dialog's **Clip content to frame** control maps to `clip_content`. A breakpoint can inherit, enable, or disable that control without changing authored layer masks.
