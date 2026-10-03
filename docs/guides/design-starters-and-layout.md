# Design starters, responsive layout, and canvas shortcuts

## Supplied starter library

The template picker offers **164 editable templates in 12 categories**. The
updated `Application Template Design-handoff UPdated New.zip` supplies 154
artwork templates in 11 categories, with fourteen designs per category:
Instagram Post, Portrait Post, Your Story, Certificate & Quote, Presentation,
Business Card, Resume & Flyer, Poster, Video Thumbnail, Banner, and Invitation.
The twelfth category, **Responsive layouts**, adds ten native starters built
from responsive frames; see [Responsive starter layouts](design-responsive-starters.md).
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


Each tile opens a larger preview using its authored proportions. A selected
format chip overrides that size; clicking the selected chip again restores
native sizes. Choose **Add as new page** or **Replace current page** only after
reviewing the preview. Replace keeps the current page ID, name, position and
bleed; Undo restores all original artwork, including locked objects. Cancel or
Escape makes no document changes. Local multi-page templates can be previewed
page by page, applied one page at a time, or added together with **Add all pages**.
Open **Explore templates** to browse the handoff’s colored category cards.
Choose a category to see its templates; **All** restores the full library.
Search matches names, categories, and format names. Preview generation runs in
bounded background batches for visible and nearby tiles.
The earlier announcement and editorial starters remain available alongside the
164 catalog designs (154 artwork templates and 10 responsive layouts). Templates can still be saved and exchanged through the existing
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

The catalog demonstrates currently supported authoring tools; template count
does not indicate feature completeness. Future components, variables, richer layout and motion
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

## Curated style families

In the Design drawer, start with **Wedding**, **Birthday**, **Social posts**, **Flyers & posters**
or **Presentations** under **What are you making?** Choose a visual family, then
compare its three layouts and three curated palettes in the preview. The ten
families offer thirty alternative layouts:

| Purpose | Families | Native canvas | Content in one selected set |
| --- | --- | --- | --- |
| Wedding | Garden Vows, Modern Heirloom | 1500 × 2100 px | Invitation, Details, RSVP |
| Birthday | Confetti Club, Midnight Toast | 1500 × 2100 px | Invitation, Details, RSVP |
| Social posts | Field Notes, Signal Studio | 1080 × 1080 px | Cover, Story, Call to action |
| Flyers & posters | After Hours, Market Day | 1500 × 2100 px | One flyer/poster |
| Presentations | Studio Brief, Momentum | 1920 × 1080 px | Title, Overview, Next steps |

These original native families supplement the supplied template library, which
remains under **More templates**. Search within a purpose to find a family by its
name, description, layout or palette. Bulk CSV creation and data binding remain
under **Elements → All tools**.

Layout and color choices change only the isolated preview. **Previous page** and
**Next page** inspect the chosen composition's coordinated content. Use **Add
matching set (3)** for invitations, **Add carousel (3)** for social designs, or
**Add slide set (3)** for presentations. Each inserts only that selected content
set as one undoable action; alternative layouts and palettes are never added as
extra pages. Posters contain a single page and have no add-set action. **Add as
new page** and **Replace current page** apply only the page being previewed.
Cancel or Escape leaves the current design unchanged, including when a preview
is still loading. Previews preserve each family's authored proportions.
Invitation sets retain their 300 PPI, 5 × 7-inch print size; poster families use
the same print dimensions. Social and presentation families use 72 PPI.

After inserting a choice, double-click its native text to personalize the copy;
use the contextual text controls to change font, size and color. All artwork
remains editable without needing Layers for ordinary text changes. Save the
`.emu` project, then use **Pages** to select and export the desired pages. A later
template selection is a fresh starter: it does not transfer personalization from
a previously edited layout. Each content page's sample text is independently
editable.

The bundled **Cormorant Garamond** and **Fraunces** families add serif and expressive
display options alongside Geist and Geist Mono. Upright and italic faces work
offline in native previews, canvas rendering and exports. Their SIL Open Font
Licenses and pinned source provenance are included with the app; see
[`assets/fonts/README.md`](../../assets/fonts/README.md).

For a reproducible visual and export review, run:

```sh
cargo run --locked -p emulsion-io --example design_family_preview -- NEW_DIRECTORY
```

The command refuses to overwrite an existing directory. It creates category
layout/palette contact sheets, a nine-choice matrix and coordinated-set PNG for
each family, full-resolution page PNG/SVG files, and editable `.emu` and PDF sets.
It verifies all ninety layout/palette selections: proportional native-renderer
previews, native page geometry and unclipped/nonoverlapping text, vector SVG
parsing, and editable project round trips. Default sets additionally save and
reopen at native size and export without PDF raster fallback. `verification.json`
records the completed check counts; `index.tsv` lists all selected content pages.
The earlier `design_invitation_preview` command remains invitation-only. To
verify an existing invitation review after changes to shared artwork code,
append `--invitation-baseline EXISTING_INVITATION_PREVIEW_DIRECTORY`; this compares
all 36 individual invitation PNGs pixel for pixel without changing the baseline.

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
See the [MCP layout reference](mcp/mcp-design-appearance.md) for exact parameters.

Select a responsive frame and open **Canvas width breakpoints…** to add, edit or
remove up to 16 width thresholds. These respond to the page canvas width, not the
window, zoom or frame's content width. The highest matching threshold inherits
directly from base settings. Each entry can override flow, spacing, padding,
columns, wrapping, alignment, content sizing and clipping; blank numeric fields
and controls marked **inherit** use the base. Child sizing and frame dimension
limits stay shared across widths. Apply validates inactive entries too and creates
one Undo step. See [breakpoint authoring and MCP examples](design-layout-breakpoints.md).

See [Design variables](design-variables.md),
[Interactive presentations](design-interactions.md) and
[Standalone HTML presentations](design-html-export.md) for the related workflows.
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
choose **Elements → All tools → Bulk create from CSV…** in Design. Paste data or import a CSV with a
`name` column. Each data row creates an editable page, preserving native text,
rich formatting, source media and responsive layout. The source page stays intact;
one Undo removes the generated batch. CSV is data only, supports quoted commas
and newlines, and never executes formulas or fetches URLs. Input is limited to
2 MB, 64 columns and the remaining project capacity (100 total pages). Missing or
duplicate columns, invalid rows and protected fields reject the batch. Saved text
and image bindings and multi-page record sets are described in
[Local CSV design generation](design-data-bindings.md).

## Overlapping objects and layer order

Selecting an object in Layers keeps it as the drag target when other artwork
covers it. Drag inside the selected object's visible geometry to move that
selection; other selected objects move with it. Shift selection and Alt picking
inside groups retain their existing behavior. A pointer move with no button held
cannot continue a brush stroke, shape drag, or text resize after a missed release,
closed document tab, or window deactivation.

In Design, open **Position → Arrange · layer order**, or right-click the canvas
and choose **Arrange**. **Bring forward / Send backward** move the selection one
level; **Bring to front / Send to back** move it to an end of its current group
or page. These operations preserve artwork coordinates and use Undo/Redo.
**Set layer index…** assigns one selected object an exact sibling position:
**1 is the back**, and the highest displayed number is the front. It keeps the
object in its group, validates the range, and rejects locked objects or a stale
selection/page. MCP's existing `move_node` tool provides the corresponding stack
operations through `above`, `below`, or `to: "top" / "bottom"`.

## Photo/Paint shortcuts

The canvas-side strip follows the handoff: dock toggle, Properties, Brushes,
History, Character, and Assistant. Each shortcut opens one live flyout beside the
strip. Clicking it again or **×** closes it; **Move to dock** returns its controls
to the dock. These are the existing editing controls, sharing document state and
history. The strip and flyout fit narrow windows without resizing the document.

## Checks

Regression coverage includes all 154 templates and category counts, source
geometry/text, editable frame replacement, vector SVG export, category search,
layout wrapping and text reflow, lock rejection, nested fractional layout stability,
clipboard/duplicate/Undo, native multi-page save/reopen, page removal, and Photo
flyout switching/docking in both compact and roomy chrome. Canvas selection
coverage includes text insertion, Shift selection, editing and switching objects,
blank clicks, and the visible Select control. Windows and macOS runtime checks
remain with the separate platform machines.
Deferred CPU raster sources initialize before parallel tile composition, avoiding
recursive initialization waits while preserving lazy vector scene construction.
