# Brush Library and Brush Studio through MCP

The MCP exposes persistent brush authoring as well as document painting. Tool names below are unprefixed; a host may display them as `mcp__emulsion__…`. Examples show MCP `tools/call` payloads.

Replace example IDs with IDs returned by discovery and replace each `expected_revision` with the latest catalog revision. The numbers below are illustrative, not a runnable sequence.

## Discover the catalog

`describe_brush_library` returns ordered libraries and sets, complete brush definitions, pinned/recent IDs, tool memories, warnings, and a revision. Brush definitions are paginated: follow `next_offset`, or filter by `brush_id`, `set_id`, `library_id`, or `query`.

```json
{"name":"describe_brush_library","arguments":{"query":"pencil","limit":12,"offset":0}}
```

`list_brushes` remains available for brush discovery with rendered swatches. Prefer stable brush IDs; painting also accepts an unambiguous name. Complete settings use the serialized `Brush` field names returned by these tools.

## Manage libraries, sets, and brushes

`manage_brush_library` commits 1–100 operations atomically: if any operation fails, none of that batch is saved. New IDs appear in `result.results` in operation order. To create a library and then a set inside it, make separate calls so the second call can use the returned library ID.

```json
{"name":"manage_brush_library","arguments":{"expected_revision":12,"operations":[{"op":"create_library","name":"Illustration"}]}}
```

```json
{"name":"manage_brush_library","arguments":{"expected_revision":13,"operations":[{"op":"create_set","library_id":"<returned-library-id>","name":"Inking"}]}}
```

```json
{"name":"manage_brush_library","arguments":{"expected_revision":14,"operations":[{"op":"create_brush","set_id":"<returned-set-id>","name":"Fine ink","settings":{"size":8,"hardness":1,"flow":0.7}},{"op":"pin","id":"<existing-brush-id>","pinned":true}]}}
```

Supported operations:

| Operation | Fields besides `op` |
| --- | --- |
| `create_library` | `name` |
| `create_set` | `library_id`, `name` |
| `create_brush` | `set_id`, `name`, optional `settings` |
| `rename_library`, `rename_set`, `rename_brush` | `id`, `name` |
| `duplicate_library` | `id` |
| `duplicate_set` | `id`, destination `library_id` |
| `duplicate_brush` | `id`, destination `set_id` |
| `delete_library`, `delete_set`, `delete_brush` | `id` |
| `move_library` | `id`, `index` |
| `move_set` | `id`, destination `library_id`, `index` |
| `move_brush` | `id`, destination `set_id`, `index` |
| `pin` | `id`, `pinned` |
| `record_use` | `id` |
| `combine` | `primary_id`, `secondary_id` |
| `uncombine` | `id` |

Move indices are zero-based within the destination. Built-in protection follows the library UI. Combine creates a new dual brush while retaining both originals; already-dual inputs cannot be nested. Uncombine creates standalone component copies while retaining the combined brush.

```json
{"name":"manage_brush_library","arguments":{"expected_revision":20,"operations":[{"op":"combine","primary_id":"<primary-id>","secondary_id":"<secondary-id>"}]}}
```

## Persist Studio settings and reset points

`edit_brush` recursively merges `patch` into one component. Omitted settings remain unchanged; unknown fields are rejected, and numeric settings are clamped to engine limits. `component` defaults to `primary`. A secondary component must already exist. Metadata and `combine_mode` belong to the definition.

```json
{"name":"edit_brush","arguments":{"expected_revision":21,"brush_id":"<dual-brush-id>","component":"secondary","patch":{"size":18,"advanced":{"shape":{"count":3,"rotation_jitter":0.15},"grain":{"scale":1.5}}},"combine_mode":"Multiply","note":"Textured ink","author":{"name":"Artist","website":"https://example.com"}}}
```

The complete advanced groups include path, shape, grain, dynamics, wet mix, rendering, taper, color, properties, and stabilization. Read current settings to discover exact enum names and values. `tip` and `grain_tex` are runtime handles and cannot be saved through `patch`; use `brush_source`.

```json
{"name":"edit_brush","arguments":{"expected_revision":22,"brush_id":"<brush-id>","action":"create_reset_point"}}
```

`create_reset_point` captures current primary/secondary settings, source references, and the combine mode. `reset` restores that point, or the original baseline if no reset point exists. `restore_original` restores the original baseline. These actions take no patch or metadata and apply to the complete brush, not just the selected component.

## Edit source images

`brush_source` persists a shape or grain source on the primary or secondary component. Every write requires `expected_revision`.

```json
{"name":"brush_source","arguments":{"expected_revision":23,"brush_id":"<brush-id>","component":"primary","source":"shape","action":"import","path":"C:\\Art\\tips\\ink-tip.png"}}
```

```json
{"name":"brush_source","arguments":{"expected_revision":24,"brush_id":"<dual-brush-id>","component":"secondary","source":"grain","action":"paper"}}
```

Actions are `import`, `invert`, `rotate` (90 degrees), `seamless` (mirrored tile), `diamond`, `paper`, and `clear`. Import accepts PNG/JPEG up to 32 MiB and 4096 pixels per dimension. Transforms use the source coverage registered by the brush engine, including alpha-derived coverage. Grain assignment enables grain strength. Sources are immutable content-addressed assets; editing or clearing a current source preserves original/reset-point references.

## Import and export packages

`import_brushes` defaults to a dry run. It converts into temporary storage and returns converted definitions, hierarchy, and warnings without saving the catalog or assets into the persistent store.

```json
{"name":"import_brushes","arguments":{"paths":["C:\\Art\\brushes\\Inking.brushset"],"target_set":"<destination-set-id>","dry_run":true}}
```

Review warnings, read the current revision, and apply the same paths explicitly:

```json
{"name":"import_brushes","arguments":{"paths":["C:\\Art\\brushes\\Inking.brushset"],"target_set":"<destination-set-id>","dry_run":false,"expected_revision":25}}
```

Dry-run IDs are temporary. Use the committed import's returned IDs for later editing or painting. Accepted formats are `.embrushes`, `.brush`, `.brushset`, `.brushlibrary`, and supported sampled-tip ABR files. Native packages preserve supported settings, sources, and hierarchy. External conversions are approximate; their warnings identify unsupported or recovered data. Retained external archives support future conversion improvements.

`export_brushes` writes a native `.embrushes` package. Choose individual `brush_ids`, or `scope: "set"` / `"library"` with `scope_id`. Packages include current/original/reset sources and dual settings. Existing files require explicit `overwrite: true`.

```json
{"name":"export_brushes","arguments":{"path":"C:\\Art\\exports\\Inking.embrushes","scope":"set","scope_id":"<set-id>"}}
```

```json
{"name":"export_brushes","arguments":{"path":"C:\\Art\\exports\\Favorites.embrushes","brush_ids":["<brush-id>","<dual-brush-id>"]}}
```

## Tool memories and marks

`brush_memories` manages per-brush size/opacity for `paint`, `smudge`, `erase`, `heal`, `clone`, and `mask`. `inspect` is read-only and needs no revision.

```json
{"name":"brush_memories","arguments":{"action":"inspect","brush_id":"<brush-id>","tool":"paint"}}
```

```json
{"name":"brush_memories","arguments":{"action":"save_mark","expected_revision":26,"brush_id":"<brush-id>","tool":"paint","index":0,"size":24,"opacity":0.6}}
```

Other actions are `save`, `recall_mark`, `clear_mark`, `clear`, and `transfer`. Mark indices are 0–3. `save` and `save_mark` accept optional size/opacity; otherwise they use remembered/current values. `recall_mark` updates the saved tool memory.

```json
{"name":"brush_memories","arguments":{"action":"transfer","expected_revision":27,"brush_id":"<brush-id>","tool":"paint","target_tool":"smudge"}}
```

Transfer copies remembered size/opacity into the destination tool's memory. It does not switch the active UI tool or paint pixels.

## Preview without changing a document

`preview_brush` returns a deterministic PNG and rendering metadata. Supply a saved brush ID or omit it to start from the default brush. Nested settings are temporary preview overrides. Secondary settings can override a saved dual component or create a temporary secondary component. Pass `secondary_settings: null` to disable it; `combine_mode` requires a secondary component.

```json
{"name":"preview_brush","arguments":{"brush_id":"<brush-id>","width":480,"height":240,"seed":42,"mode":"paint","color":[30,60,100,255],"background":[245,245,245,255],"settings":{"size":24,"advanced":{"dynamics":{"tilt_opacity":0.3}}},"secondary_settings":{"size":10},"combine_mode":"Screen","samples":[{"x":40,"y":120,"pressure":0.2,"tilt":[10,20],"time_ms":0},{"x":240,"y":80,"pressure":1,"tilt":[20,30],"time_ms":100},{"x":440,"y":120,"pressure":0.2,"tilt":[10,20],"time_ms":200}]}}
```

Dimensions are 16–1024 pixels. Omit samples for the standard pressure-varying preview stroke. Preview coordinates must remain within its canvas. `background_path` accepts a bounded PNG/JPEG resized to the preview dimensions; use a varied background for meaningful smudge previews. Colors here are four sRGB bytes, including alpha.

## Paint and hatch with the same settings

`paint` changes a document layer and creates one undo step per call. Rich `samples` support pressure, tilt, timing, speed dynamics, and stabilization. Each stroke supplies exactly one of `samples`, legacy `points`, or SVG `d`.

```json
{"name":"paint","arguments":{"node":"<pixel-layer-id>","brush":"<brush-id>","mode":"paint","color":"#1e3c64","seed":42,"settings":{"size":24,"advanced":{"dynamics":{"tilt_opacity":0.3}}},"secondary_settings":{"size":10},"combine_mode":"Screen","sample_merged":false,"strokes":[{"samples":[{"x":40,"y":120,"pressure":0.2,"tilt":[10,20],"time_ms":0},{"x":240,"y":80,"pressure":1,"tilt":[20,30],"time_ms":100},{"x":440,"y":120,"pressure":0.2,"tilt":[10,20],"time_ms":200}]}]}}
```

Rich sample arrays contain 1–2000 points. Pressure is 0–1; each tilt axis is −90–90 degrees. Timestamps must be finite, nonnegative, and monotonic; omitted timestamps start at zero and advance 16 ms. Legacy points/SVG preserve their computed geometry with stabilization disabled. Settings precedence is saved definition, call overrides, then stroke overrides. Call-level mode, seed, secondary settings, and combine mode can also be overridden per stroke. `secondary_settings: null` disables a saved secondary component for painting.

`mode` explicitly selects paint, smudge, or erase independently of the brush. For backward compatibility, omitting it preserves the operation of Eraser/Smudge-category brushes. `sample_merged` defaults to false; true allows wet/smudge pickup from a frozen lower-layer backdrop in addition to the current layer.

`hatch` generates stroke geometry and accepts the same temporary brush/dual settings, mode, seed, sampling, alpha-lock, mirror, and symmetry options. Its generated geometry keeps stabilization disabled.

```json
{"name":"hatch","arguments":{"node":"<pixel-layer-id>","brush":"<brush-id>","mode":"paint","color":"#1e3c64","rect":[40,40,320,160],"angle":45,"spacing":8,"jitter":0.1,"cross":true,"seed":42,"settings":{"size":3},"secondary_settings":{"size":1.5},"combine_mode":"Multiply","sample_merged":false}}
```

Hatch rectangles bound centrelines; brush footprints and jitter can extend outside. Use a selection for a strict boundary.

## Revisions and history

Catalog writes require the current `expected_revision`; a stale revision fails without replacing newer edits. Read again and reconcile changes before retrying. Returned revisions should be carried into the next write. Catalog operations are independent of document history: document undo does not reverse saved library edits, source edits, imports, or tool memories. Standalone preview writes neither catalog nor document. Paint and hatch overrides affect their marks without saving changes to brush definitions.
