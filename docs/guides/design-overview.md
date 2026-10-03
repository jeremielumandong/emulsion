# Design at a glance

Design makes social posts, print pieces, presentations and responsive layouts
from native, editable objects. Text stays text, shapes stay vector paths, and
images keep their original pixels inside frames. A Design project holds one or
more pages and saves as a multi-page `.emu` file with its history.

New to Design? Follow [Your first Design project](tutorials/design-first-project.md).
To compare Design with the other workspaces, see
[Emulsion workspaces compared](workspaces.md).

## Start a project

1. Choose **Design** on Home. If a Design tab is already open, Home switches to
   it; choose **File → New design…** to start another project.
2. In the **New document** dialog, choose **Templates** or **Blank canvas**.
3. To start from a template, pick a category from the **All templates** menu
   and select a template.
4. To start blank, choose **Blank document**, then set the name, size,
   resolution, page count and bleed.
5. Choose **Create**.

The template library holds 164 editable templates in 12 categories:

- 154 artwork templates, fourteen in each of 11 categories: Instagram Post,
  Portrait Post, Your Story, Certificate & Quote, Presentation, Business Card,
  Resume & Flyer, Poster, Video Thumbnail, Banner and Invitation.
- 10 starters in the **Responsive layouts** category, built from responsive
  frames.

The picker also lists two earlier starters and your own saved templates under
**My templates**. Inside a project, open **Design** in the left rail and click a
tile to preview it without changing your page. Choose **Add as new page** or
**Replace current page**; both support one-step Undo and Redo. Replace preserves
the page name, position and bleed. **Cancel** or **Escape** discards the preview.
A format chip changes the template size shown in the preview. Local multi-page
templates have Previous/Next controls and an **Add all pages** option.

In **Text**, ten named heading/body combinations show the actual editable layout
and the fonts available on your computer. Search by family or purpose, such as
editorial, report or poster. Inserting a combination creates a selected group
with editable heading and body layers as one undoable action. Optional font
families use bundled fallbacks when unavailable.

Click a text font control to search installed, bundled and page-embedded fonts.
Each result previews your selected text in that font using the document's text
renderer. Use **Up/Down** to browse, **Enter** to apply or **Escape** to close.
Applying a font preserves a selected character range and supports Undo.

The left rail groups the editing tools into **Design**, **Elements**, **Text**,
**Uploads**, **Tools**, **Frames**, **Brand**, **Photos**, **Magic**, **Motion**
and **Position**. The canvas toolbar adds **Position**, **Animate**, **Present**
and **Resize**. The page strip below the canvas adds, orders and removes
pages. Choose **Pages** beside the strip for the [page organizer](design-page-organizer.md),
with a grid, multi-page selection, drag reordering and selected-page exports.

## Place assets and adjust frame crops

Open **Uploads** or **Photos** to browse local images as thumbnail cards. Search
by name or tag, choose an asset folder, or use **Previous** and **Next** for more
cards. Each card's **···** menu includes **Properties / relink…** and **Retry
preview** if a source file has moved or its preview cannot load.

- Select a frame, then click an image or logo card to replace its image. Drag a
  card directly onto a frame to target that frame instead. A locked frame
  rejects replacement.
- With no frame selected, clicking a card adds an image. Dropping onto empty
  canvas places it at the drop position. Layered or vector assets use a rendered
  copy inside a photo frame; insert them onto blank canvas to retain editable
  layers. Loading can be canceled with **Cancel** or **Escape**.
- Double-click a frame containing an image to open the crop preview. Drag to
  pan, scroll or use **+ / −** to zoom, and use arrow keys for fine adjustments
  (**Shift** moves farther). The image stays large enough to cover the frame.
- Choose **Done** or press **Enter** to apply the crop as one undoable edit.
  **Cancel** or **Escape** discards the preview. Cropping keeps the embedded
  source pixels, frame shape, rotation and flips; undo restores the previous
  crop. Replacing an image is also one undoable edit.

In **Brand**, select the text, shapes or groups you want to change before
choosing **Apply to selected objects**. This applies the kit only to the selected
objects and their contents. The button is disabled when nothing is selected.

## Align, space and resize without Layers

Select objects on the canvas, then open **Align & space** beside **Arrange**.
Choose **Canvas** or **Selected objects** before aligning. Canvas alignment
works with one object; aligning objects to each other needs at least two.
**Equal gaps** and **Tidy up** need three movable objects. Tidy up chooses the row
or column from the existing object centers and equalizes gaps without changing
object sizes, stack order or frame structure. The button states its chosen axis.
Locked objects or locked descendants must be unlocked before moving them. Each
operation is one undoable edit.

Choose **Resize** in the canvas toolbar to open an isolated preview of the current
page. **Size preset** includes square and portrait posts, a vertical story, a wide
slide, A4 and US Letter. Screen sizes are common starting points, so check the
requirements of your destination. Print presets use 300 ppi and physical units:
A4 is 210 × 297 mm (2480 × 3508 px); Letter is 8.5 × 11 in (2550 × 3300 px).
Custom fields accept pixels, millimeters or inches and a resolution in ppi.
PDF export records the physical page size using that resolution. PNG page
exports preserve pixel dimensions and embed the page's print resolution without
resampling. PDF records physical page, trim and bleed sizes.

A preset renders its preview immediately. After editing custom dimensions, choose
**Preview** again. Applying is disabled until the shown preview matches the input.
Objects reuse their saved resize anchors and responsive rules. Resize checks flag
page overflow, changed text wrapping, text-box overflow and photo-frame gaps.
These are geometry checks, not a guarantee about transparent pixels or visual
balance. Review the preview and any named objects before exporting. A page
background photo is refitted to cover the new page while retaining its original
pixels and editable crop; check its new framing. Ordinary grouped Cover frames
also retain their crop and image proportions. If a masked, responsive, complex
or non-Cover photo group cannot change aspect ratio safely, resize reports an
error without applying it; use proportional dimensions or adjust the frame
separately.

**Make a copy** is the primary action and leaves the original page untouched.
**Resize this page** keeps its ID, name, bleed and history. Either action supports
Undo and Redo. Cancel, Escape or closing the preview makes no resize change.
Copies keep their version graph, redirect self-navigation to the new page, and
retain links to other pages. The native project keeps editable text, fonts,
styles, images and frames.

## Guide map

Each Design guide covers one feature area.

**Layout**

- [Design starters, responsive layout, and canvas shortcuts](design-starters-and-layout.md): the template library, selection, responsive frames and layer order.
- [Responsive starter layouts](design-responsive-starters.md): the ten Responsive layouts starters.
- [Canvas width breakpoints](design-layout-breakpoints.md): width previews and per-width layout overrides.

**Appearance and reuse**

- [Design appearance controls](design-appearance.md): fill, stroke, opacity and effects from the Appearance row.
- [Reusable appearance styles](design-styles.md): saved styles linked to their consumers.
- [Reusable components](design-components.md): components, linked instances and variants.
- [Design variables](design-variables.md): named colors and numbers bound to properties.

**Text and vectors**

- [Native text lists, paragraph spacing and decorations](design-text-formatting.md): lists, spacing, underline and strikethrough.
- [Native vector editing and precision](design-vector-editing.md): point editing, Boolean operations and precise geometry.

**Data and charts**

- [Native charts and tables](design-charts.md): eight chart and table types with editable data.
- [Local CSV design generation](design-data-bindings.md): bind text and images to CSV columns and generate pages.

**Brand**

- [Portable typography, palettes and asset folders](design-portable-brands.md): typography roles, palettes and asset folders.

**AI**

- [Design AI capabilities and availability](design-ai-capabilities.md): what the assistant and image tools can change.

**Presenting and motion**

- [Design presentations](design-presentation.md): speaker notes, page transitions, Present and the presenter window.
- [Interactive presentations](design-interactions.md): object click actions.
- [Advanced motion and interchange](design-advanced-motion.md): motion presets and retiming.
- [Local video, audio and property keyframes](design-local-media-keyframes.md): embedded media and keyframes.
- [YouTube in Design presentations](design-video.md): YouTube video objects.

**Interchange**

- [Editable PowerPoint presentations](design-pptx.md): PPTX import and export.
- [Standalone HTML presentations](design-html-export.md): interactive HTML export.
- [Selection and frame export](design-selection-export.md): export selected objects as SVG, PDF or PNG.
- [Editable Lottie interchange](lottie-interchange.md): Lottie import and export.

**Packs**

- [Portable Design templates and Diagram stencils](template-pack-format.md): author, export and install template packs.

## Export a whole project

Project export writes every page, or only the current page, without flattening
the project.

1. Choose **File → Export…**. In a compact window, the **Export** button opens
   the same dialog.
2. Under **Project export**, open **Pages, vectors and presentations…**.
3. For print output, turn on **Include page bleed**.
4. Choose a format and range, such as **Print PDF · all pages** or
   **PNG pages · current page**.
5. Choose a destination file.

| Menu item | Output |
| --- | --- |
| **PNG pages** | ZIP archive with one PNG per page |
| **JPEG pages** | ZIP archive with one JPEG per page |
| **SVG pages** | ZIP archive with one vector SVG per page |
| **Print PDF** | One multi-page PDF with vector pages |

Archive entries are named by page order, such as `page-001-….png`. Each format
offers **all pages** or **current page**; there is no custom page range.

Bleed is a per-page setting in millimetres. Set it in the **New document**
dialog, or open a page's **···** menu in the page strip and choose **Page name
and bleed…**. With **Include page bleed** on, each exported page extends by its
bleed on every side. Native background-photo frames reveal existing image pixels
outside the trim without changing the crop. If a photo ends at the trim, the
export warns you to zoom the photo or adjust its frame; it never stretches or
invents edge pixels. Customized frames remain unchanged. Intentional transparency,
opacity and masks remain as authored, so check the preview before printing.
Print PDF also records the trim box inside the bleed. See
[Printing](printing.md) for physical-size PDF output.

The status bar reports how many pages were exported. It also reports how many
pages use rendered images and lists their document PPI. Native shadows/glows use
a full-page rendered appearance because PDF's sRGB transparency would otherwise
change Design's linear-light glow brightness. This preserves pixels at the
existing resolution; it does not upscale a low-resolution design. Start print
artwork at suitable dimensions and PPI, typically 300 PPI. Pages without such
effects retain supported vector paths and outlined text. Imported SVG filters
render only the filtered parts at at least 300 effective PPI. Oversized filters
fail clearly instead of dropping effects or silently reducing quality.
The project itself stays editable; export never flattens its native objects.

The same menu offers **Export selected objects…**, **Interactive HTML** and
**Editable PowerPoint** for all or current pages, plus **Import / export
notes…**. The dialog's own **Export…** button exports only the current page in
the format chosen above it.

## Related

- [Your first Design project](tutorials/design-first-project.md)
- [Emulsion workspaces compared](workspaces.md)
- [Design interchange acceptance matrix](../technical/design-interchange-matrix.md)
- [Design MCP tools](mcp/mcp-design-presentation.md)

For page color, background photos, and selection-first formatting, see [Backgrounds and direct controls](design-backgrounds-and-controls.md).
