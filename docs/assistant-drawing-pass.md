# Assistant drawing and MCP reliability pass

This pass improves the native drawing workflow used by the in-app assistant. It addresses concrete rendering and tool-use defects; it does not establish artistic parity with Photoshop, Procreate, or a human artist.

## Changes

- **Inspect completed work.** Document reads now share the assistant's ordered tool queue with edits. A `get_view` or `critique` requested after painting waits for that paint to finish, including animated playback. Subsequent edits wait for the inspection result. Provider completion waits for outstanding inspection; stopping rejects queued and stale inspection results. Brush, font, and attached-reference discovery remain independent.
- **Preserve soft edges while shading.** MCP paint now uses the brush engine's alpha lock, preserving the original alpha instead of multiplying it into the selection. It also prevents an alpha-locked eraser from removing pixels. Selection strength remains independent.
- **Use canvas symmetry consistently.** Scripted mirror and radial marks now use document coordinates on rotated and nonuniformly scaled layers, matching the interactive tools.
- **Honor stroke settings and reject unusable input.** Settings combine in order: named preset, call settings, then stroke settings. Changing a stroke's brush no longer loses global size or flow overrides. Geometry and pressure validation reports malformed calls before applying paint, instead of silently substituting default pressure or accepting empty strokes. Hatch rectangles define centerline generation bounds; use an active selection to clip the full brush footprint precisely.
- **Make brush discovery manageable.** `list_brushes` returns a compact catalog of matching names and intended uses, with complete settings and swatches in matching pages of up to 12. Use `query` to narrow results and top-level `next_offset` to continue. The older `swatch_layout.next_offset` remains available. Invalid query and offset types produce errors.
- **Improve drawing decisions.** Studio guidance emphasizes proportions, silhouettes, value masses, deliberate pressure, short feature tapers, and targeted corrections before texture. It distinguishes editable paths from raster ink, artwork translation from stack reordering, and alpha-locked recoloring from erasing. It tells the assistant to preserve user selections, await dependent tool results, inspect before retrying, and respect skipped edits.

Scripted paint does not receive physical tablet tilt or timestamps. Tilt and speed dynamics are inactive, and hand stabilization is deliberately disabled for computed geometry. Explicit pressure and tapers control planned marks. Preset names and numerical critique metrics do not certify material realism or drawing quality.

## Validation scope

Regression coverage includes translucent shading, erasing, selection strength, transformed symmetry, paginated brush discovery, real local MCP relay ordering, cancellation, provider completion, and an editable drawing/correction workflow. Existing playbook examples execute against the current MCP tools.

Live model drawing quality still needs representative artist review: run the same briefs and reference images before and after this change, with the same provider/model and budget. Compare composition, proportions, edge control, medium character, and retained editability. These code and tool tests cannot prove that every model will make better artistic decisions.
