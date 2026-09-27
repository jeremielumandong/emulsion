# Design starters, responsive layout, and canvas shortcuts

## Supplied starter library

The updated `Application Template Design-handoff - update.zip` supplies **110
editable templates in 11 categories**, with ten designs per category:
Instagram Post, Portrait Post, Your Story, Certificate & Quote, Presentation,
Business Card, Resume & Flyer, Poster, Video Thumbnail, Banner, and Invitation.
The Invitation collection covers Wedding, Birthday, Baby shower, Graduation,
Dinner party, Housewarming, Engagement, Anniversary, Kids party, and Retirement.
The source geometry, copy, colors, type styles,
rounded shapes, and native sizes live in `crates/emulsion-core/assets/design-starters.json`.
Photo placeholders are native clipping frames with editable diagonal paint.
Replace their media using the existing frame controls; source pixels and text
remain editable. No browser runtime or network request is needed.

Each tile adds a page using its authored proportions. A selected format chip
overrides that size; clicking the selected chip again restores native sizes.
Open **Explore templates** to browse the handoff’s colored category cards.
Choose a category to see its ten templates; **All** restores the full library.
Search matches names, categories, and format names. Preview generation runs in
bounded background batches for visible and nearby tiles.
The earlier announcement and editorial starters remain available alongside the
110 supplied designs. Templates can still be saved and exchanged through the existing
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
NEW_DIRECTORY` creates one ten-page project per category. It respects the
existing 100-page project limit and refuses to overwrite an existing directory.

## Responsive layout

Select objects or a group, open **Position**, and choose **Row**, **Column**, or
**Grid** under Responsive layout. Set frame width/height, individual padding,
gap, columns, row wrapping, and alignment. Children can retain their width or
fill the available width. Text reflows without changing its font size.

The resulting group has an editable rectangular boundary. Reflow runs as part of
the originating command when text, objects, or the boundary changes. Nested frames
lay out from parent to child. Selecting a child exposes fixed/fill width and
absolute/in-layout placement. Media frames remain replaceable inside layouts.
Removing automatic layout preserves the current artwork and boundary.

Settings persist with the document and participate in Undo/Redo, duplication,
clipboard ID remapping, and version merges. Invalid spacing and changes that would
move protected content fail atomically. Rotating or reshaping the layout boundary
requires removing automatic layout first. Layout cells snap to document pixels to
avoid cumulative drift; authored text remains text.

This is the first responsive-layout workflow. Hug sizing, height fill, min/max
constraints, automatic clipping, aspect locks, authored breakpoints, variables,
components/variants, interactive prototypes, and responsive HTML export remain
separate milestones. Large-scene layout performance still needs measurement.

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
