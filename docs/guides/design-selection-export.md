# Selection and frame export

Select objects in Design, then choose **Export → Export selected objects…**. The native dialog writes a single **SVG**, **PDF**, or **PNG** file. It keeps the source document, editable objects, selection, and Undo history unchanged.

Choose **Content bounds** to fit visible selected artwork, **Frame bounds** for one responsive frame's rectangle, or **Canvas bounds** to retain the current page dimensions. Padding adds up to 1000 pixels around those bounds. Transparent output is the default; turn it off for white. Content bounds reserve conservative space for layer effects. Frame/canvas bounds deliberately crop outside artwork and effects.

Selected groups include their descendants. Ancestor group opacity, masks, effects, and responsive clips remain applied; unrelated siblings and the page background are excluded unless selected. Responsive breakpoint settings are resolved using the source page width before cropping, so a narrow export does not switch to a different layout preset.

Include a clipped object's sibling clipping base, or select their containing group. An incomplete clipping stack is rejected with the required base ID; it is never silently detached. Backdrop-dependent blending and adjustment appearance can change when unselected backdrop artwork is removed, and the export reports this isolation explicitly.

Supported native paths and text stay vector geometry in SVG/PDF. Glyphs are outlined with the editor's fonts, so recipients do not need those fonts. Unsupported masks/effects/blending use the existing rendered fallback, with a diagnostic. **Require vector appearance** rejects that fallback before replacing a destination. PNG preserves transparency and renders the same supported contours at the output pixel dimensions. Neither SVG nor PDF carries native chart/component associations; the original project retains them.

MCP `export_design_selection` accepts required `nodes`, `path`, and `format` (`svg`, `pdf`, `png`), plus `bounds` (`content`, `frame`, `canvas`), integer `padding`, `transparent`, `strict_vectors`, and `overwrite`. Files must use the matching extension. Existing destinations require `overwrite:true`; failed validation leaves their bytes unchanged. The result reports normalized root IDs, source crop origin, pixel dimensions, rasterization status, and diagnostics. The live host performs the work off the UI thread.

```json
{"nodes":[12,15],"path":"/tmp/card.svg","format":"svg","bounds":"content","padding":16,"transparent":true,"strict_vectors":true}
```

Exports are limited to 64 million pixels and the normal document dimension limit. Local and remote videos export their current authored poster artwork; portable playback is a separate presentation export workflow.
