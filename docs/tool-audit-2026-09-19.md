# Common-tool audit — 2026-09-19

Audited revision: `750271a`. Scope: all 12 editor toolbar tools and their modes, painting input, selections/transforms, layers/masks, adjustments/filters, text, persistence/export, batch, and common keyboard workflows. This is a source and automated-behavior audit on macOS, not a hands-on comparison with installed Photoshop or Procreate.

> Follow-up: the audit below records the original baseline. See [the repair plan and implementation results](tool-repair-plan.md) for the subsequent fixes and remaining manual checks, [moving artwork](artwork-movement.md) for group dragging and keyboard nudging, and [aligning artwork](artwork-alignment.md) for canvas and selection alignment added afterward.

## Verdict

**Emulsion is not yet at parity with Procreate or Photoshop for dependable everyday tools.** It has substantial feature coverage, including real brush dynamics, editable paths, adjustment layers, masks, and raster transforms. However, basic workflows contain correctness defects, some operations can discard newer work, and important Mac input and clipboard workflows are missing. All 12 toolbar entries being enabled does not establish that each workflow is complete.

The largest painting gap is native Mac pen input. The largest editing gaps are selection/lock consistency, asynchronous operation ownership, and file round-trip fidelity. Fixing these takes priority over increasing the tool count. No numerical parity percentage is justified by this audit.

## Evidence and validation

- **Executed** means a diagnostic reproduction or existing automated test ran. **Source-confirmed** means the implementation establishes the defect, but the described interactive scenario was not executed. Race timing and subjective drawing quality remain unverified.
- `cargo test --release --workspace --lib`: **317 passed, 0 failed, 1 ignored**. Counts: AI 38, assistant 18, core 34, filters 3, IO 25, MCP 45, raster 75, recipes 13, UI 66. The ignored UI test invokes a real Claude CLI. Color/GPU/tools crates have no library tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: **passed**. Cargo also reports a future Rust compatibility notice for dependency `block 0.1.6`; this is not a project Clippy failure.
- Targeted lock diagnostic: **reproduced**. Under a locked parent, `SetPlacement` and `ReplacePixels` both returned success and changed the document. `RotateNode` rejected the same child as expected.
- Export/native-load diagnostics: **reproduced**. An exposed gray pixel changed from `[188,188,188,255]` on canvas to `[99,99,99,255]` after PSD layered reopening, with zero adjustment nodes. At separate clipping and group-mask test points, white became blue; reopened clipping relationships and masked groups were both zero. Reading only the standard ORA stack produced `[99,99,99,255]`; the native extension produced `[187,187,187,255]` (an additional one-level quantization difference from the original).
- Masked Smart native-file diagnostic: **reproduced**. Saving succeeded; reopening returned `invalid manifest: mask emulsion/mask-1.png does not match its node's size`.
- Export reproduction files are retained at `/var/folders/fw/p654cdpd3bn3s_kq6712y1500000gn/T/emulsion-audit-exports-95752/`: `adjustment.psd`, `adjustment.ora`, `adjustment-standard.ora`, `group-mask-clipping.psd`, and `masked-smart.ora`. These are synthetic documents, not user artwork.
- `cargo test --release -p emulsion-ui --lib audit_paint_tests -- --nocapture`, using two temporary GPUI pointer-input regressions: **0 passed, 2 failed, confirming both bugs**. Clone's selected red source did not replace blue: destination stayed `[0,0,65535,65535]` in 16-bit RGBA. Heal changed **25 pixels outside the active selection**, where the expected count was zero. These failures are separate from the passing existing suite above.
- Production code was not changed for this audit. Temporary diagnostic harnesses are removed after execution.

## Priority findings

P1: potential lost work, unreadable files, crashes, or edits outside protected regions. P2: incorrect or incomplete common behavior. P3: narrower persistence/reporting problems. Locations below refer to the audited revision.

### P1 — Native masked Smart layers can save but fail to reopen

`ConvertToSmart` preserves a raster-sized mask, while the native reader requires Smart masks to have canvas dimensions. Create an 8×8 document, add a masked 4×4 raster, convert it to Smart, save ORA, then reopen. The writer accepts a document that the reader rejects. Fix the mask coordinate/dimension contract across conversion, rendering, editing, and loading, including compatibility with existing files.

Evidence: [command.rs](../crates/emulsion-core/src/command.rs), line 669; [ora.rs](../crates/emulsion-io/src/ora.rs), line 841. Related: Smart masks are edited in document coordinates (`editor/tools.rs:1130`) but lowered to placed pixel layers (`document.rs:382`), causing inconsistent mask placement on transformed/filtered Smart layers.

### P1 — Background edits can overwrite newer work

Heal, bucket fill, warp, and distort capture a raster and later replace the full layer without checking that the source content is still current. Start a slow operation, then paint elsewhere or undo before it finishes: its callback can install the older snapshot over newer work. Heal can also close a transaction belonging to a later gesture. Use operation ownership and source identity/revision checks; define cancellation and undo behavior before committing results.

Evidence: [tools.rs](../crates/emulsion-ui/src/editor/tools.rs), lines 1813, 1852, 1908–1952; [transform.rs](../crates/emulsion-ui/src/editor/transform.rs), lines 210 and 397. **Source-confirmed; interactive race reproduction pending.** Smart filters have another lifecycle problem: their generation counter is global across layers (`editor/smart.rs:104`), so a newer request for layer B can discard layer A's pending request. Undo does not invalidate all pending filter callbacks.

### P1 — Heal ignores selection boundaries when committing

The preview applies the selection clip, but the final content-aware solve uses raw stroke coverage. Select the left half of an image and heal a speck in the right half: pixels outside the selection can change. Brush opacity and alpha clipping are also absent from the final coverage mask. Build the healing mask from effective clipped coverage and preserve pixels outside it.

Evidence: [tools.rs](../crates/emulsion-ui/src/editor/tools.rs), line 1908; [paint.rs](../crates/emulsion-raster/src/paint.rs), line 1206.

### P1 — Locked groups do not consistently protect their children

Move/painting check the selected node's lock, not all ancestor locks. Core `SetPlacement`, `ReplacePixels`, and `SetPath` also lack that protection. Lock a group and select its unlocked child: common edits remain possible. `RotateNode` already rejects this case. Centralize the editable-target check and use it consistently across direct and asynchronous commands.

Evidence: [editor.rs](../crates/emulsion-ui/src/editor.rs), line 945; [command.rs](../crates/emulsion-core/src/command.rs), mutation handlers and `RotateNode`.

### P1 — Magnetic lasso can use an obsolete edge-map allocation

After canvas enlargement, `magnetic_track` can pass the old edge vector together with the new width/height to `live_wire`, which indexes it directly. Build a magnetic cache, enlarge the canvas, then trace in the added area before recalculation finishes. This creates an out-of-bounds panic path. Validate cache revision, dimensions, and allocation length; reject obsolete worker completions.

Evidence: [tools.rs](../crates/emulsion-ui/src/editor/tools.rs), lines 1646 and 1703; [select.rs](../crates/emulsion-raster/src/select.rs), line 388. **Source-confirmed; resize/timing scenario not executed.**

### P1 — Layered PSD round trips lose visible content

Adjustment layers are rendered with their underlying layers hidden and therefore export transparent results. Clipping relationships and group masks are omitted. The stored merged image can look correct while reconstruction from editable layers is wrong. Exporting a gray raster with Exposure, or clipped pixels inside a masked group, demonstrates separate losses. Preserve supported PSD constructs and explicitly handle unsupported ones with an appearance-preserving export strategy.

Evidence: [psd.rs](../crates/emulsion-io/src/psd.rs), lines 256, 344, 399. Ordinary raster layer export also bypasses Emulsion styles. Editable text, native adjustment definitions, and Smart objects are not preserved as their corresponding Photoshop objects.

### Other defects and gaps

| Priority | Finding and reproduction | Source |
| --- | --- | --- |
| P1 | Batch outputs collide: `photo.jpg` and `photo.png` produce the same target stem/extension and overwrite one another. Preflight and resolve duplicate/existing destinations. Source-confirmed. | `ui/src/batch.rs:133`, `io/src/lib.rs` atomic writer |
| P2 | Clone's first dab is committed before the source offset is installed. Alt-click red, then single-click blue: the destination does not clone. Configure the offset before starting the stroke. | `ui/src/editor/tools.rs:795,802,908` |
| P2 | Liquify Restore captures the already-warped image as its new original on every stroke. Push, release, switch to Restore: it cannot restore the previous shape. Keep an appropriate session original. | `ui/src/editor/tools.rs:935`, `raster/src/liquify.rs:173` |
| P2 | Liquify ignores the active selection. Warping outside a selection changes unselected pixels. Apply a transformed selection clip to the output. | `ui/src/editor/tools.rs:959` |
| P2 | Alpha lock only multiplies coverage by existing alpha; ordinary paint/erase still changes alpha. Half-transparent pixels can become more opaque or more transparent. Preserve original alpha explicitly. | `ui/src/editor/tools.rs:866`, `raster/src/paint.rs:1179` |
| P2 | Mirror axes follow layer-local X/Y after rotation. Rotate a layer 90° and use left/right mirror: reflection occurs along the wrong document axis. Transform the complete reflection, not just the center. | `ui/src/editor/tools.rs:893`, `raster/src/paint.rs:655` |
| P2 | Saving a brush preset rejects changes limited to pressure, spacing, tilt, taper, textures, and other omitted fields as duplicates. Compare all persisted behavior fields. | `ui/src/editor/presets.rs:38,199` |
| P2 | Preset switching bypasses saved brush/eraser/smudge slots; selecting a paint preset while Liquify is active leaves Liquify active. Route through the common mode-switch path. | `ui/src/editor/presets.rs:80`, `ui/src/editor/tools.rs:413` |
| P2 | Smart layers expose warp/distort handles but completion accepts only Raster. Applying the preview silently does nothing. Implement the operation or disable it with a clear explanation. | `ui/src/editor/transform.rs:92,186,361` |
| P2 | Feather, wand, Quick Select, and Modify Selection can apply an obsolete result after deselect/undo. Guard selection operation ownership. | `ui/src/editor/tools.rs:596,1635` |
| P2 | Quick Select changes explicit New/Replace into Add when a selection already exists. Selecting another object keeps the old selection. Respect the chosen combine mode. | `ui/src/editor/tools.rs:697` |
| P2 | Standard ORA `stack.xml` omits Fill and Adjust nodes; native extension fidelity does not imply other readers see the same layered result. Masks/clips/styles are also not consistently baked into standard layers. | `io/src/ora.rs:232,491` |
| P3 | Brush import discards texture-store and preset-save errors, then can report success. Imported brushes/textures may disappear on relaunch. Propagate persistence failures. | `ui/src/editor/presets.rs:143,173` |

Except where the validation section records execution, these additional findings are from source inspection. Paths in this table are relative to `crates/`.

## Tool-by-tool coverage

“Implemented” describes executable code and covered basic paths; it is not an unconditional pass for every input or document type.

| Tool / workflow | Present | Audit assessment |
| --- | --- | --- |
| Hand / navigation | Pan, space-pan, zoom, fit, canvas rotation, rulers | Basic paths covered by tests; physical trackpad feel and Retina targeting need manual checks. |
| Move / transform | Raster/Smart placement, scaling, rotation; numeric controls; raster warp/distort; subtree rotation | Partial: locked-parent bypass, stale results, Smart warp no-op. Group translation/scaling and path/text handles are incomplete. |
| Select | Rectangle, ellipse, lasso, polygon, wand, Quick Select, magnetic; combine, feather, grow/shrink; mask move/scale/rotate | Broad coverage with correctness gaps above. Transforming the selection boundary is not selected-pixel transformation. |
| Mask | Paint reveal/hide, layer masks, clipping, refinement controls | Basic raster masks work; Smart mask contract and export fidelity need repair. |
| Brush / pencil | Opacity/flow, hardness, spacing, roundness/angle, pressure response, textures, grain, wet mixing, taper, jitter, stabilization | Real implementation. Mac hardware pressure/tilt absent; alpha lock, presets, and transformed symmetry defective. Artistic quality unmeasured. |
| Eraser | Brush-driven erasing, selection clipping, own settings slot | Basic paths covered; fractional alpha lock and preset switching need repair. |
| Smudge | Brush-driven color mixing, backdrop sampling | Implemented; complex transparency and large-canvas responsiveness need manual evaluation. |
| Brush library / drawing aids | Categorized presets, custom brushes, partial Procreate brush import, symmetry, grid/isometric/perspective assistance, hold-to-snap QuickShape | Imported brushes approximate supported fields. No ABR import, dual brush, physical eraser-tip routing, or stationary time-based spray found. Symmetry centers are fixed. |
| Bucket / gradient | Flood/color fill, selected fill, linear/radial gradients | Async fill stale-result risk. Gradients are rasterized rather than editable multi-stop gradient objects. |
| Liquify | Push, twirl, pinch, expand, Restore | Engine exists; selection enforcement and UI Restore are broken. |
| Heal | Content-aware spot healing | Selection bypass and stale-result/transaction issues. No source-sampled healing brush equivalent found. |
| Clone | Aligned cloning from the current raster | First-click defect. No source-layer choice or aligned toggle found. |
| Grade / adjustments | 17 adjustment variants including curves, levels, exposure, HSL, color balance, white balance, LUT | Implemented in this revision, with basic UI/render tests. PSD export loses adjustments. |
| Type | Editable text, fonts, wrapping, alignment, spacing, rotation | One style per text node; no mixed character formatting. Missing fonts may change layout when reopening native files. |
| Crop / resize | Crop, straighten, centered crop, anchored canvas resize, proportional image resize, optional edge fill | Implemented basic paths. No editable crop handles/aspect-preset workflow found; independent width/height image scaling is limited. |
| Shape | Rectangle/ellipse as masked fills | Limited: no retained shape parameters, rounded corners, or shape stroke controls found. |
| Pen | Bézier anchors/handles, insertion/removal, corner/smooth, path-to-selection, brush along path | Meaningful vector support. No Boolean shape operations found; SVG arc commands unsupported. |
| Layers / groups / undo | Reorder, duplicate, group, visibility, opacity/blends, masks/clipping, styles, branching history | Substantial implementation; inheritance and async transaction behavior prevent a reliability pass. |
| Filters / Smart layers | 16 filter variants, cached Smart rendering and editable stacks | Basic filters covered. Per-layer worker ownership, undo races, Smart masks, and round trips need repair. |
| Color / eyedropper | Foreground/background, swapping/reset, color picker and sampling; import ICC conversion to sRGB | ICC import exists in `emulsion-io`; working space is fixed sRGB. No CMYK or wide-gamut document workflow. |
| Clipboard / familiar shortcuts | Familiar bare tool letters; configurable keymap | No pixel cut/copy/paste actions found. Delete targets the layer rather than selected pixels. Defaults use literal Ctrl combinations, not Mac Command equivalents. |
| Save / export | Native ORA, PNG/JPEG/WebP/TIFF/PSD; 16-bit PNG/TIFF; RAW import/development | Native Smart-mask failure and PSD/standard-ORA fidelity gaps are material. PSD output is 8-bit RGB; CMYK PSD import rejected. |
| Batch | Folder selection, recipes, preview, selected-file export | Output collisions need fixing; large real-folder workflow not manually tested. |
| Image generation / Ctrl-K | Local SD, OpenAI, Google routing, selection-aware insertion, undo/cancel paths | Implementation and tests exist. Live services, account quotas, output quality, and provider mask fidelity not certified by this audit. |
| Assistant drawing | Tool calls, live stroke playback, reference workflows, style guidance | Engineering tests do not establish artistic parity. See the existing [artist evaluation protocol](artist-evaluation.md). |

The `emulsion-tools`, `emulsion-color`, and `emulsion-gpu` crates are stubs, but their names are not a reliable feature inventory: tools are implemented in UI/raster and ICC conversion in IO. No dedicated GPU brush/filter/compositor backend is implemented in the GPU crate. GPUI's hardware-rendered UI does not establish GPU image-processing or latency parity.

## Comparison with common Procreate / Photoshop workflows

Comparison targets documented behavior, not a claim that the other products are bug-free. Procreate is an iPad drawing app; Photoshop is a desktop editor. Platform-specific interactions need equivalent usable behavior, not identical gestures.

| Common expectation | Emulsion assessment | Official comparison reference |
| --- | --- | --- |
| Pressure/tilt-sensitive drawing | Brush math exists; Mac hardware ingestion does not. Speed fallback is not pen-pressure parity. | [Procreate Apple Pencil](https://help.procreate.com/procreate/handbook/interface-gestures/pencil) documents native pressure/tilt response. |
| Customizable paint, erase, smudge | Strong starting coverage, but preset persistence, alpha behavior, and physical input need repair and drawing trials. | [Procreate Brush Studio](https://help.procreate.com/procreate/handbook/brushes/brush-studio-settings); [Photoshop painting tools](https://helpx.adobe.com/photoshop/desktop/apply-painting-techniques/fill-objects-selections-layers/painting-tools-overview.html). |
| Select, copy/cut, paste, edit selected pixels | Selection tools exist; pixel clipboard workflow is absent. | [Procreate Copy/Paste](https://help.procreate.com/procreate/handbook/interface-gestures/copypaste); [Photoshop copy/paste selections](https://helpx.adobe.com/sg/photoshop/desktop/make-selections/refine-modify-selections/copy-and-paste-selections.html). |
| Reliable locks, alpha lock, masks | Implemented concepts with ancestor-lock, alpha, and Smart-mask defects. | [Procreate masks and locks](https://help.procreate.com/procreate/handbook/layers/layers-mask) includes group protection and alpha/layer/clipping masks. |
| Scale, rotate, distort, warp | Raster path is comparatively broad; selected-pixel and cross-layer-type behavior incomplete. | [Procreate Transform](https://help.procreate.com/procreate/handbook/transform) covers freeform, uniform, distort, warp, snapping, interpolation. |
| Selection refinement | Several controls exist; async lifecycle and magnetic-cache defects need repair. No equivalent quality comparison has been run on hair/fur/soft edges. | [Photoshop Select and Mask](https://helpx.adobe.com/photoshop/desktop/make-selections/refine-modify-selections/refine-your-selection-and-mask.html). |

## Recommended implementation order

1. **Protect work:** native masked-Smart reopening; operation ownership/cancellation/undo; inherited locks; Heal/Liquify selection enforcement; magnetic-cache bounds; batch collision protection.
2. **Make everyday painting predictable:** Clone first dab, Liquify Restore, exact alpha preservation, transformed symmetry, complete preset save/switching, native Mac tablet pressure/tilt.
3. **Complete common editing workflows:** pixel cut/copy/paste, selected-pixel clear/transform, Mac Command shortcut defaults, consistent transforms for supported layer types, explicit unsupported-tool states.
4. **Preserve exchanged documents:** PSD adjustments/clipping/masks/styles and standard ORA appearance; add round-trip fixtures before expanding format claims.
5. **Evaluate quality and performance:** real stylus tests, representative large drawings, image fixtures, long strokes, imported brushsets, and artist trials. Set measured latency/memory and visual acceptance criteria before claiming parity.

## Remaining manual checks

- Real Mac tablet pressure/tilt, hover, eraser tip, hotplug; Linux device permissions and sample freshness.
- Stroke feel at different speeds, zoom levels, and event rates; dry/wet brushes over translucent multilayer content.
- Long documents and large brushes: input-to-mark latency, frame time, memory, worker cancellation, save/autosave recovery.
- Opening exported files in actual Photoshop, Procreate, and an independent ORA reader. Automated Emulsion reimports do not certify those applications.
- Third-party fonts and real brush libraries; imported preset persistence across relaunch.
- Live generation providers and installed AI models. Quota/access failures are distinct from local tool correctness.

This audit identifies concrete repair work. It does not certify every combination of tool, node type, mask, transform, file, or external service.
