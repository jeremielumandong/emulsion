# Design starters, responsive layout, and canvas shortcuts

## Supplied starter library

The updated `Application Template Design-handoff UPdated New.zip` supplies **154
editable templates in 11 categories**, with fourteen designs per category:
Instagram Post, Portrait Post, Your Story, Certificate & Quote, Presentation,
Business Card, Resume & Flyer, Poster, Video Thumbnail, Banner, and Invitation.
The Invitation collection covers Wedding, Birthday, Baby shower, Graduation,
Dinner party, Housewarming, Engagement, Anniversary, Kids party, and Retirement.
The source geometry, copy, colors, type styles,
rounded shapes, and native sizes live in `crates/emulsion-core/assets/design-starters.json`.
Photo placeholders are native clipping frames with editable diagonal paint.
Replace their media using the existing frame controls; source pixels and text
remain editable. No browser runtime or network request is needed.

The latest artwork includes Editorial, Outline, Sticker and Type Stack variants.
The loader preserves native gradients, inside/center/outside borders, opacity,
rotation about each object's center, shadows, heart shapes, text outlines,
letter spacing, line height, curved text and editable text backdrops. Rounded
corners use the handoff's page-relative units; pill buttons retain semicircular
ends. Catalog order follows the supplied data without relying on legacy enum
positions. The ten responsive starters remain available as a separate category.
Letter spacing stays in document pixels across the editor, brand styles and MCP;
the text engine receives the equivalent em value at each run's font size. Large
poster tracking is preserved instead of being capped at the old −50 px limit.

Print-oriented starters use 300 PPI: A4 flyer (approximately 210 × 297 mm,
rounded to whole pixels), business card (3.5 × 2 in), poster (18 × 24 in), and
invitation (5 × 7 in). Screen formats retain 72 PPI. Existing saved documents keep
their stored resolution. See [Printing](printing.md) for physical-size PDF output.
Simple shadows export separately from foreground vector contours; text outlines
and aligned borders also retain scalable geometry. More complex layer effects
still use the exporter's rendered fallback.

`cargo run -p emulsion-io --example design_starter_preview -- NEW_DIRECTORY`
generates a native contact sheet, artwork index and representative PNG/SVG pairs
for visual review, without running the handoff's JavaScript.


Each tile adds a page using its authored proportions. A selected format chip
overrides that size; clicking the selected chip again restores native sizes.
Open **Explore templates** to browse the handoff’s colored category cards.
Choose a category to see its fourteen templates; **All** restores the full library.
Search matches names, categories, and format names. Preview generation runs in
bounded background batches for visible and nearby tiles.
The earlier announcement and editorial starters remain available alongside the
154 supplied designs. Templates can still be saved and exchanged through the existing
local/GitHub package workflow.

Every Design page thumbnail has a visible **×** action. Removing an active page
selects an adjacent page; removing an inactive page preserves the current one.
Undo restores the removed page and its editable content. A project keeps at least
one page; the document tab closes the whole design.

The canvas header includes **Select objects (V)**. Adding a text preset returns
to Select with the new text selected. Click objects to select and drag them;
Shift-click adds/removes an object, Alt-click picks inside a group, and a blank
canvas click clears the selection. Hidden and locked objects are skipped.
Double-click text to edit it. Clicking another object commits the text and
selects that object; Ctrl/Cmd+Enter finishes text editing and returns to Select.
Escape cancels the text edit and returns to Select. These interactions preserve
native editable text and Undo.

The catalog demonstrates currently supported authoring tools; template count is
not a feature-parity claim. Future components, variables, richer layout and motion
can introduce additional templates without removing or flattening existing ones.
Bundled templates are compiled data; no imported JavaScript runs in the app.

| Template content | Authoring controls |
| --- | --- |
| Rectangles, circles, triangles, diamonds, stars, rules and arrows | Elements, fill/stroke properties, Position and the full vector tools |
| Rounded shapes and custom geometry | Editable path points and handles in the full tool set |
| Headings, paragraphs and labels | Text presets, font/size/style/alignment toolbar, Character properties |
| Photo placeholders and cropping | Frames, image replacement, Cover/Contain/Stretch and crop focus |
| Layer order, grouping and placement | Select, Layers, Position, alignment and transforms |
| Reusable local designs | Save page as template; import/export file packages or install from GitHub |

For native review, `cargo run -p emulsion-io --example design_starter_fixture --
NEW_DIRECTORY` creates one fourteen-page project per supplied category and a
ten-page Responsive layouts project. It respects the
existing 100-page project limit and refuses to overwrite an existing directory.

## Responsive layout

Select objects or a group, open **Position**, and choose **Row**, **Column**, or
**Grid** under Responsive layout. Set frame width/height, individual padding,
gap, columns, row wrapping, and alignment. Choose **Width: fit content** or
**Height: fit content** to size a frame around its content and padding. Optional
minimum and maximum width/height constrain the frame; blank limits are unset.
Dimensions retain decimal values when reopening the dialog.

Select a child and open **Object sizing & limits** to choose fixed or fill sizing
on either axis, set minimum/maximum dimensions, or **Keep aspect ratio**. The
aspect ratio starts from the selected object's current bounds and can be edited.
Flexible children in a row share the remaining width; in a column they share
the remaining height. Space left by a child reaching its maximum goes to the
other flexible children. Grid children fill their cells. Text reflows without
changing its font size, and native paths and image sources remain editable.
In a wrapped row, height fill uses the frame's available height for each row;
additional rows can overflow. It does not divide the frame height among wrapped rows.

The resulting group has an editable rectangular boundary. Reflow runs as part of
the originating command when text, objects, or the boundary changes. Nested frames
lay out from parent to child. Child controls also expose absolute/in-layout
placement. Absolute children keep their positions and do not contribute to
content sizing or receive flow sizing. Media frames remain replaceable inside layouts.
Removing automatic layout preserves the current artwork and boundary.

Settings persist with the document and participate in Undo/Redo, duplication,
clipboard ID remapping, and version merges. Invalid spacing and changes that would
move protected content fail atomically. Rotating or reshaping the layout boundary
requires removing automatic layout first. Layout cells snap to document pixels to
avoid cumulative drift; authored text remains text.

Content sizing and child fill cannot depend on one another on the same axis.
A nested frame cannot both fit its content and fill its parent's corresponding
axis; aspect-ratio sizing is unavailable for nested content-sized frames.
Conflicting rules or impossible aspect-ratio limits reject the whole edit.
With both fill axes enabled, an aspect-locked object fits inside its allocated
cell. With only height fill enabled, height drives the ratio; otherwise width
drives it. Minimum sizes can make content overflow a frame. Enable **Clip content
to frame** in the layout dialog to hide overflow while retaining editable artwork
and authored masks; the default permits overflow.

The same controls are available through `set_responsive_layout` and
`set_layout_child`; optional limits and ratios accept `null` to clear.
See the [MCP layout reference](mcp-design-appearance.md) for exact parameters.

Select a responsive frame and open **Canvas width breakpoints…** to add, edit or
remove up to 16 width thresholds. These respond to the page canvas width, not the
window, zoom or frame's content width. The highest matching threshold inherits
directly from base settings. Each entry can override flow, spacing, padding,
columns, wrapping, alignment, content sizing and clipping; blank numeric fields
and controls marked **inherit** use the base. Child sizing and frame dimension
limits stay shared across widths. Apply validates inactive entries too and creates
one Undo step. See [breakpoint authoring and MCP examples](design-layout-breakpoints.md).

Variables, interactive prototypes, and responsive HTML export remain separate milestones.
Large-scene layout performance still needs measurement.

Regression coverage includes bounded nested reflow, repeated-layout stability,
native text wrapping, protected-object rollback, dialog Cancel/Apply and Undo,
and project/template/clipboard round trips. The UI and MCP use the same core
sizing implementation.

## Reuse formatting and generate designs

Use **Copy style** and **Paste style** in Position or the selected object's
**Object actions** menu. Text formatting, shape fill/stroke, opacity, blending and
layer effects transfer while the target keeps its content, geometry, masks and
identity. Text-to-text copying applies the source's first character formatting
throughout the target; it does not copy the source string or its character ranges.
Pasting onto several selected objects is one Undo step, and a protected target
rejects the whole operation. Object actions also exposes flip, lock, duplicate
and delete.

To generate local design variations, write text such as `Hello {{name}}`, then
choose **Bulk create from CSV…** in Design. Paste data or import a CSV with a
`name` column. Each data row creates an editable page, preserving native text,
rich formatting, source media and responsive layout. The source page stays intact;
one Undo removes the generated batch. CSV is data only, supports quoted commas
and newlines, and never executes formulas or fetches URLs. Input is limited to
2 MB, 64 columns and the remaining project capacity (100 total pages). Missing or
duplicate columns, invalid rows and protected fields reject the batch. This first
workflow binds text; image bindings and multi-page record sets remain pending.

## Photo/Paint shortcuts

The canvas-side strip follows the handoff: dock toggle, Properties, Brushes,
History, Character, and Assistant. Each shortcut opens one live flyout beside the
strip. Clicking it again or **×** closes it; **Move to dock** returns its controls
to the dock. These are the existing editing controls, sharing document state and
history. The strip and flyout fit narrow windows without resizing the document.

## Checks

Regression coverage includes all 110 templates and category counts, source
geometry/text, editable frame replacement, vector SVG export, category search,
layout wrapping and text reflow, lock rejection, nested fractional layout stability,
clipboard/duplicate/Undo, native multi-page save/reopen, page removal, and Photo
flyout switching/docking in both compact and roomy chrome. Canvas selection
coverage includes text insertion, Shift selection, editing and switching objects,
blank clicks, and the visible Select control. Windows and macOS runtime checks
remain with the separate platform machines.
Deferred CPU raster sources initialize before parallel tile composition, avoiding
recursive initialization waits while preserving lazy vector scene construction.
