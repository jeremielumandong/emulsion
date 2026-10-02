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
**My templates**. Inside a project, open **Design** in the left rail to add more
template pages; each tile adds a page at its authored size.

The left rail groups the editing tools into **Design**, **Elements**, **Text**,
**Uploads**, **Tools**, **Frames**, **Brand**, **Photos**, **Magic**, **Motion**
and **Position**. The canvas toolbar adds **Position**, **Animate**, **Present**
and **Magic resize**. The page strip below the canvas adds, orders and removes
pages.

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
bleed on every side. Print PDF also records the trim box inside the bleed. See
[Printing](printing.md) for physical-size PDF output.

The status bar reports how many pages were exported. It also reports how many
pages use rendered images for effects that vector export does not support; the
project itself stays editable.

The same menu offers **Export selected objects…**, **Interactive HTML** and
**Editable PowerPoint** for all or current pages, plus **Import / export
notes…**. The dialog's own **Export…** button exports only the current page in
the format chosen above it.

## Related

- [Your first Design project](tutorials/design-first-project.md)
- [Emulsion workspaces compared](workspaces.md)
- [Design interchange acceptance matrix](../technical/design-interchange-matrix.md)
- [Design MCP tools](mcp/mcp-design-presentation.md)
