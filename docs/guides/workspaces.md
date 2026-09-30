# Emulsion workspaces compared

Emulsion has five workspaces in one app: Library, Photo, Paint, Design and
Diagram. Library catalogues and develops photos, including camera RAW. Photo
retouches and composites images in layers. Paint is for drawing and painting
with brushes. Design lays out pages for social posts, print and presentations.
Diagram draws structured flowcharts and architecture diagrams.

Images move between the workspaces. A RAW file always opens in Library
Develop. **Edit in Photo…** sends the developed image to a new Photo tab. Photo
and Paint edit the same document: **Ctrl+Alt+Shift+D** switches between their
layouts, and so does **Window → Layout**. Design and Diagram save pages to an
`.emu` project.

## Which workspace do I use?

| I want to… | Use | Start with |
| --- | --- | --- |
| Import a shoot, cull it and develop RAW files | Library | [First Library shoot](tutorials/library-first-shoot.md) |
| Apply one look to a whole folder and export it | Library | [Library and Develop](library-develop.md) |
| Retouch a portrait, remove objects, or composite layers | Photo | [First photo edit](tutorials/photo-first-edit.md) |
| Add masked adjustment layers, filters and layer styles | Photo | [Photo editing](photo-editing.md) |
| Sketch, ink or paint on a blank canvas | Paint | [First painting](tutorials/paint-first-painting.md) |
| Tune, import or share brushes | Paint | [Brush Library and Brush Studio](brush-workflow.md) |
| Make a social post, poster, business card or slide deck | Design | [First Design project](tutorials/design-first-project.md) |
| Draw a flowchart, swimlane, ER or cloud architecture diagram | Diagram | [First diagram](tutorials/diagram-first-diagram.md) |
| Turn text, CSV, Mermaid or SQL into a diagram | Diagram | [Generate a diagram from text or data](diagram-functionality.md#generate-a-diagram-from-text-or-data) |

## Feature comparison

"—" means the workspace doesn't offer the feature. Library covers RAW
development.

| Feature | Library (RAW) | Photo | Paint | Design | Diagram |
| --- | --- | --- | --- | --- | --- |
| **Purpose** | Catalogue, cull and develop photos | Retouch and composite | Draw and paint | Pages, print, and decks with a presenter window | Structured diagrams |
| **Start from** | An imported folder | An opened image | A blank canvas | 164 templates in 12 categories, or blank | 22 templates, or blank |
| **Core tools** | 12 Develop sections | 14 toolbar tools, 7 selection modes | 15 Paint rail tools | Shapes, text, frames, charts, tables, components | 12 shape kinds, 4 connector routings, 13 markers |
| **Non-destructive editing** | Develop settings in sidecars, with history | Layers, adjustment layers, masks, Smart Objects | Layers, alpha lock | Pages and editable objects | Pages, containers and editable shapes |
| **Colour and tone** | Tone curve, colour mixer, colour grading, calibration, white balance | 18 adjustment types, 28 blend modes | 29 brush blend modes, 32-colour used palette | Saved styles, variables, brand palettes | 9 themes |
| **Effects** | Local masks (6 shapes), spot removal | 16 filters, 10 layer styles | 9 grain kinds, dual brush, symmetry | Appearance (fills, gradients, strokes, effects), motion | Markers and styles |
| **Content libraries** | Recipes, presets, smart collections | Recipes | 55 built-in brushes in 10 categories | Templates, components, brand kits | 92 native stencils, 42 draw.io families, 55 AWS/Azure icon packs |
| **Local AI** | Subject and sky masks, depth map | Select subject, Remove, generative fill | Assistant drawing | Assistant layout | Assistant and generation from data |
| **Merge and repair** | HDR merge, panorama stitch, lens profiles, highlight reconstruction | Heal, Clone stamp, Liquify, Warp | QuickShape, drawing guides and Drawing Assist | Vector point editing, Boolean operations | Auto-layout (4 layouts) |
| **Import** | RAW and common images, Lightroom presets and catalogs | Common images, PSD/PSB, XCF, and camera RAW through Library | `.ora`, brushes (`.brush`, `.brushset`, `.brushlibrary`, `.abr`, `.embrushes`) | PowerPoint, Lottie | draw.io, Visio, Lucid |
| **Export** | Batch export with presets | PNG, JPEG, WebP, TIFF, layered PSD and XCF, and more | Same as Photo, plus animation and replay GIF | PowerPoint, HTML, Lottie, PDF, SVG, PNG, JPEG | draw.io, PDF, SVG, PNG, JPEG |
| **Native file** | `<photo>.emulsion-raw.json` sidecar beside the original | `.ora` | `.ora` | `.emu` | `.emu` |
| **Pen pressure** | — | Yes | Yes (Windows, macOS, Linux) | — | — |
| **Print** | Print selected photos | **File → Print…** | **File → Print…** | **File → Print…**, Print PDF with bleed | **File → Print…** |

## How the workspaces work together

- **Library → Photo:** choose **Edit in Photo…** to take a developed photo into
  layers, selections and retouching. Library keeps the RAW settings in its
  sidecar.
- **Photo ↔ Paint:** Photo and Paint are two layouts of the same document.
  Paint a photo, or use photo tools on a painting.
- **Recipes:** a recipe saved in Photo applies to a whole folder in Library.
- **Packs:** share Design templates and Diagram stencils as `.emutemplate` and
  `.emustencil` files. See [portable templates and stencils](template-pack-format.md).
- **Assistant:** the assistant and its MCP tools work in all five workspaces.
  See the [MCP guides](../README.md#mcp-and-the-assistant).

## Guides and tutorials

| Workspace | Tutorial | Guides |
| --- | --- | --- |
| Library (RAW) | [Your first Library shoot](tutorials/library-first-shoot.md) | [Library and Develop](library-develop.md), [RAW panel controls and limits](raw-development.md), [Nikon HE/HE★](nikon-he.md) |
| Photo | [Your first photo edit](tutorials/photo-first-edit.md) | [Photo editing](photo-editing.md), [Moving artwork](artwork-movement.md), [Aligning artwork](artwork-alignment.md), [Smart Object sources](smart-object-sources.md), [Printing](printing.md) |
| Paint | [Your first painting](tutorials/paint-first-painting.md) | [Paint workspace](paint.md), [Brush Library and Brush Studio](brush-workflow.md), [Brush files](brush-import-formats.md) |
| Design | [Your first Design project](tutorials/design-first-project.md) | [Design at a glance](design-overview.md), which links every Design guide |
| Diagram | [Your first diagram](tutorials/diagram-first-diagram.md) | [Diagram functionality and compatibility](diagram-functionality.md) |

## Where the numbers come from

Each count in this page comes from the source code. To refresh a count, check
the file listed here. Paths are relative to the repository root.

| Count | Source |
| --- | --- |
| 12 Develop sections | `crates/emulsion-ui/src/batch/layout.rs` |
| 6 mask shapes; 64 masks and 256 spots per photo | `crates/emulsion-core/src/develop_edits.rs` |
| 14 Photo toolbar tools | `crates/emulsion-ui/src/editor.rs` (tool enum) |
| 7 selection modes | `crates/emulsion-ui/src/editor/tools.rs` |
| 18 adjustment types | `crates/emulsion-raster/src/adjust.rs` |
| 16 filters | `crates/emulsion-filters/src/lib.rs` |
| 28 layer blend modes (27 plus Pass Through) | `crates/emulsion-raster/src/blend.rs` |
| 10 layer styles | `crates/emulsion-core/src/styles.rs` |
| 15 Paint rail tools | `crates/emulsion-ui/src/editor/rail.rs` |
| 55 built-in brushes in 10 categories | `crates/emulsion-raster/src/library.rs` |
| 29 brush blend modes, 9 grain kinds | `crates/emulsion-raster/src/paint.rs` |
| 32-colour used palette | `crates/emulsion-core/src/document.rs` |
| 164 Design templates in 12 categories | `crates/emulsion-core/src/design.rs`, `crates/emulsion-core/assets/design-starters.json`, `crates/emulsion-core/src/design_responsive_templates.rs` |
| 22 Diagram templates, 9 themes | `crates/emulsion-core/src/diagram_library.rs` |
| 12 shape kinds, 4 routings | `crates/emulsion-core/src/diagram.rs` |
| 13 connector markers | `crates/emulsion-core/src/diagram_markers.rs` |
| 92 native stencils | `crates/emulsion-core/src/diagram_stencils.rs` |
| 42 draw.io families, 55 AWS/Azure icon packs | `crates/emulsion-io/src/diagram_packs.rs`, `assets/diagram-stencils/cloud-icons.json.gz` |
| 4 auto-layouts | `crates/emulsion-core/src/diagram_layout.rs` |
