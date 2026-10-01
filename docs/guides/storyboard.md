# Storyboard workspace

Storyboard is Emulsion's workspace for boarding a film, animation or ad. A
storyboard is one project file (`.emu`): every panel is a page at the project
resolution, grouped into acts, sequences and scenes, with a duration, captions
and shot data. Panels are drawn with Paint's tools. Agents can build and edit
storyboards too; see [MCP: storyboards](mcp/mcp-storyboard.md).

## Start

- Choose **Storyboard** on Home or in the workspace switcher, or pick the
  Storyboard type in the **New canvas** dialog. Pick a video, film or social
  preset and the number of panels.
- New storyboards start from your storyboard preferences (see
  [Preferences](#preferences)): naming rules, panel length, caption fields and
  Smart add layers. The New canvas dialog shows the defaults it will use.
- Or choose **Templates** in the New canvas dialog to start from a storyboard
  template you saved or installed (see [Templates](#templates)).

## Stage and Board

The **Stage** is the canvas for drawing the active panel. The panel strip under
it shows every panel and the running time.

The **Board** shows the whole storyboard as a grid of panel cards under act,
sequence and scene headers. Switch with **Board**/**Stage** in the panel strip
or **View → Storyboard Board**. Act and sequence headers appear once a board
has more than one of them. Each header shows the group's name, panel count and
running time.

Each card shows the picture, panel name, duration, tag colour, status, and the
first caption line (turn this off in preferences). Badges mark **Locked**
panels and thumbnail sheets.

- Click a card to select it and make it active. Shift-click selects a range;
  Ctrl-click (Cmd-click on macOS) adds or removes one panel.
- Double-click a card, or choose **Open on the Stage**, to draw on it.
- Drag cards to reorder them. Dropping on a card places the panels before it;
  dropping on a scene puts them at the end of that scene, so panels move
  between scenes.

## Edit the board

The Board toolbar and the panel menu (right-click a card, or **More ▾**) hold
the editing commands. Each one is a single Undo step.

| Command | What it does |
| --- | --- |
| **Add panel** | A blank panel after the active one, in its scene, named by the naming rules. |
| **Smart add** | Like Add panel, but carries the layers listed in Smart add (for example the set or background) from the active panel. |
| **Duplicate** | The next frame: a copy right after the original, in the same scene, with its drawing, timing and captions. |
| **Delete** | Removes the selected panels. Locked panels cannot be deleted. |
| **Lock** / **Unlock** | Locks the selected panels, or their scene. |
| **Start a scene here** (also sequence, act) | Starts a new group at the panel. A scene started inside scene 10 is named 10A. |
| **Copy**, **Cut**, **Paste** | The panel clipboard; see below. |
| **Make thumbnail sheet…**, **Convert sheet to panels** | See [Thumbnail sheets](#thumbnail-sheets). |
| **Renumber…** | Renames scenes and/or panels by the naming rules, for the whole board or the selected panels' scenes. |

Each group header's **···** menu renames the group, joins it with the
previous group of the same level, renumbers it, or locks the scene.

### Locks

A locked panel, or every panel of a locked scene, cannot be drawn on, changed,
renamed or deleted, by you or by an agent. The Stage shows **Locked panel ·
drawing is off** with an **Unlock** button. Undo still steps back through
earlier edits. Moving or regrouping panels never removes a lock: unlock first
to move a panel out of a locked scene.

### Copy and paste panels

Copy panels on one board and paste them into the same storyboard or another
open one. When the copied panels are whole scenes, they paste as new scenes
after the scene you paste into. Other panels join the scene they land in.
Captions follow their field names (missing fields are added), durations keep
their length in seconds at the new frame rate, and pictures at another
resolution are cropped to the centre and scaled.

## Drawing on the Stage

Panels are drawn with Paint's tools: the same brushes (with pressure and
tilt), brush library and **File → Import brushes…** (including Photoshop
`.abr`), symmetry (mirror and radial, in Brush settings) and colour picker.
Draw clean-up lines on a **vector layer** (**Layer → New Vector Layer**) to
keep them editable: the Brush and Eraser, the Line, Rectangle, Ellipse and
Polyline tools, the contour editor, Smooth and Optimize, and pencil retouch
work on its strokes, with opacity that follows pressure, tilt, speed and a
fade length. See [Vector layers](paint.md#vector-layers).

The Stage toolbar at the bottom left of the Stage, and **View** in the menu
bar, hold the Stage's viewing aids:

| Control | What it does |
| --- | --- |
| **Camera** | Camera view: only the framed shot, as the audience sees it, with no overscan, guides or light table. |
| **Safe** | Shows the action and title safe areas inside the camera frame. |
| **Field** | Shows the field guide, with a cross at its centre. |
| **Light table** (Ctrl+Alt+O) | Shows neighbouring panels faintly over the paper, earlier panels tinted red and later ones blue. ◀ and ▶ set how many panels before and after to show; the percentage sets the nearest panel's opacity (farther ones fade). |
| **⇋**, **⇵** | Flip the view horizontally or vertically to check a drawing. The art is not changed, and drawing still lands under the pointer. Also in **View → Flip View** for every canvas. |
| **Reference** | Docks the reference images beside the Stage. **mirror** flips their preview without changing the reference. |

Around the camera frame, the Stage shows an overscan margin (10% by default)
as a grey band; art outside the frame is not drawn there. **Fit on Screen**
frames the margin too. Each board keeps its own safe areas, field guide,
overscan and palette: change the field guide size and overscan in **View**
(each change is one Undo step), or set the values in preferences and choose
**Apply storyboard preferences** on the Board.

The board's palette heads **Swatches** and the colour picker. Click a swatch
to paint with it, **+** adds the foreground colour, and right-click removes a
swatch; each change is one Undo step.

### Bring in art

For storyboards, **File → Import** offers:

- **Import into panel…**: puts the layers of a PSD, PSB, ORA or image file on
  top of the active panel, fitted to the frame. Photoshop groups, masks, blend
  modes and clipping masks are kept.
- **Import as panels…**: adds one panel per file (up to 100) after the active
  panel, each named after its file and fitted to the frame.

Each import is one Undo step; locked panels are refused.

**Edit → Paste in Place** (Ctrl+Shift+V) pastes copied layers at the position
they were copied from, so a character or prop lands in the same place on
another panel, or in another open document. On the Board it pastes panels.

In the layers panel, every row has an eye and a lock. Press one and drag
across other rows to set them all the same way, as one Undo step.

## Layouts and shortcuts

**Window → Layout** offers two storyboard layouts:

- **Overview**: the Board with the Panel inspector open, for arranging and
  writing.
- **Drawing**: the Stage with Paint's toolbars and the layers panel, for
  drawing. **Reset layout** returns to it.

Saved layouts work as in Paint and remember whether the Board is open. The
storyboard layout you last used comes back when you open a storyboard.

Storyboard commands have their own heading in **Settings → Shortcuts**, where
they can be changed:

| Command | Default |
| --- | --- |
| Board / Stage | Ctrl+Alt+B |
| Add panel / Smart add | Ctrl+Alt+P / Ctrl+Alt+Shift+P |
| Duplicate panel | Ctrl+Alt+J |
| Delete panel | Ctrl+Shift+Backspace |
| Lock or unlock panel | Ctrl+Alt+L |
| Start a scene | Ctrl+Alt+N |
| Renumber | Ctrl+Alt+Shift+R |
| Copy / paste panels | Ctrl+Alt+Shift+C / Ctrl+Alt+Shift+V |
| Previous / next panel | Page Up / Page Down |
| Light table | Ctrl+Alt+O |
| Camera view | Ctrl+Alt+K |
| Find and replace captions | Ctrl+H |

On the Board, commands act on the selected panels; on the Stage, on the active
panel. On macOS, Cmd also works in place of Ctrl.

## Thumbnail sheets

A thumbnail sheet is a panel for roughing out a sequence small, many frames to
a page. **Make thumbnail sheet…** turns a panel into a sheet with the columns
and rows you choose. The Stage draws a camera frame for each cell; draw one
thumbnail in each. Sheets do not count towards the running time.

**Convert sheet to panels** replaces the sheet with one panel per cell, in row
order, in the sheet's scene. Each new panel is its cell cropped to the frame
and scaled to the project resolution, and every layer stays editable.

## Panel inspector

The **Panel** tab at the top of the sidebar edits the active panel:

- Name, duration in frames or seconds at the project frame rate, shot size,
  camera angle, status and tag colour.
- **Lock panel** and **Lock scene**. A locked panel's fields are read-only until
  you choose **Unlock**.
- One caption box per caption field. Text is saved when you leave the box or
  press Enter (Ctrl+Enter in multi-line fields), as one Undo step.
- **B**, **I**, **U**, **S** and **A** format the selected caption text (bold,
  italic, underline, strikethrough, colour), or the whole caption when nothing
  is selected. Formatted captions show a styled preview under the box.
- **Layers** shows the panel's layers.

**Caption fields…** adds, renames, reorders and removes caption fields, and
sets whether each is multi-line and printed. Removing a field removes its text
from every panel.

## Find and replace captions

Choose **Edit → Find and Replace Captions…** or press Ctrl+H. Search all
caption fields or one, with **Match case** and **Whole word**. Results list the
panel, field and context; click one to select its panel. **Replace All** is one
Undo step and reports how many matches were replaced and how many locked panels
were skipped.

## Library

The **Library** tab at the top of the sidebar keeps drawings you reuse:
characters, props, backgrounds and whole set-ups. It has two parts:

- **In this storyboard**: the project library, saved in the `.emu` file, so it
  travels with the storyboard and with templates made from it.
- **Personal library**: shared by every storyboard on this computer and kept
  with your other creative library assets.

An item is either **layers** (one or more layers from a panel) or a **panel**
(a whole panel). To add one, select layers in the Layers dock and choose
**Add layers…**, or choose **Add panel…** for the active panel. Give it a name
and optional tags, and tick **Personal library** to share it with every
storyboard.

Click an item, choose **Place** in its **···** menu, or drag it onto the Stage
or a panel on the Board:

- Layers go on top of the panel at the position they were drawn in, so a
  character lands where it stood. Drawings from a storyboard at another
  resolution are fitted to the frame.
- A panel item becomes a new panel after the panel (named by the naming rules)
  and becomes the active panel.

Placing is one Undo step. Adding, renaming and deleting items in the project
library are part of the storyboard, so each is also one Undo step and marks the
storyboard as changed. The personal library is saved on disk at once and is not
part of any storyboard's Undo; deleting from it asks first, and panels it was
placed on keep their copies. **···** also copies an item between the two
libraries and renames it or changes its tags. Search matches names and tags.

## Templates

A storyboard template starts new storyboards with your resolution, frame rate,
caption fields, naming rules, Smart add layers, stage guides, palette, project
library and starting panels with their layers.

- **File → Save as Storyboard Template…** (or **Save as storyboard
  template…** in the Library tab) saves the current storyboard as a template in
  your personal library. Version history is left out.
- **Export template file…** in the Library tab writes an `.emutemplate` file to
  share. Opening one installs it, like Design templates and stencil packs.
- In **New canvas**, choose **Storyboard**, then **Templates**, and pick one
  under **My templates**. The new storyboard is an unsaved copy with fresh
  history; the template is never changed.

## Preferences

**Settings → Storyboard** holds the defaults for new storyboards and the board
display. Use the search box at the top of Settings to find any setting.

- **Naming rules**: scene prefix, first number, step and zero-padding (for
  example SC010, SC020), panel prefix and padding, whether panel numbers
  restart in each scene, and whether scenes inserted inside another get a
  letter. A live example shows the result.
- **Panel length** for new panels, in seconds.
- **Caption fields** new storyboards start with.
- **Smart add layers**: layer names Smart add carries into a new panel.
- **Board thumbnail width** and whether cards show a caption line.
- **Stage**: action and title safe areas, field guide size and whether new
  boards show it, overscan, and the palette new storyboards start with.
- **Light table**: on or off, panels before and after, opacity and tint.
  These apply to every storyboard.

Each storyboard keeps its own naming rules and Smart add list once created.
To give an existing storyboard the current ones, choose **Apply storyboard
preferences** in the Board's panel menu (one Undo step), then **Renumber…** if
you want existing names to follow.

## Export and print

**File** offers three storyboard exports. Each one uses the board as it is,
including unsaved changes, and never changes it.

### Storyboard PDF and printing

**File → Export Storyboard PDF…** opens the print dialog with a storyboard
layout. **File → Print…** (Ctrl+P) on a storyboard opens the same layouts for
your printers or the system print dialog. The live preview shows each page;
**Previous** and **Next** step through them.

- **Panels**: all panels, the Board's selected panels, or one scene.
- **Paper** and **Orientation** come from the dialog's usual controls.
- **Profile** picks a layout. Three are built in: *3 per page · captions
  right*, *6 per page · captions below* and *1 per page · large*.

The options are grouped and can be searched by name (type "caption" or
"camera" in **Search options**):

| Group | Options |
| --- | --- |
| Page | Margin inside the printable area. |
| Panels | Panels across and down, the space between them, image fitting (**Fit** shows the whole panel, **Fill** crops it to its box), panel frame thickness (0 for none), a panel header and a second panel header, their alignment and size. |
| Captions | Position (below, right or left of the panel, or none), the share of the panel's box they get, frames around them, field names in bold, which fields to print (empty prints the fields marked for printing in **Caption fields…**) and text size. Captions keep their bold, italic, underline, strikethrough and colour. |
| Header and footer | Page header and footer text, their alignment and size, and a PNG or JPEG logo with its position and height. |
| Camera | The camera frame on each panel, the board's action and title safe areas, their line thickness, and the line thickness of camera-move arrows (used once camera moves arrive). |

Headers and file names use tokens in braces:

| Token | Value |
| --- | --- |
| `{project}` | The storyboard's name. |
| `{act}`, `{seq}`, `{scene}` | Act, sequence and scene names. |
| `{panel}` | The panel's number in its scene. |
| `{name}` | The panel's name. |
| `{index}` | The panel's position in the board. |
| `{frames}`, `{duration}` | Duration in frames, and in seconds. |
| `{timecode}` | Where the panel starts, as HH:MM:SS:FF at the board's frame rate. |
| `{shot}`, `{angle}`, `{status}` | Shot size, camera angle and status. |
| `{page}`, `{pages}`, `{date}` | Page header and footer only: page number, page count and today's date. There, `{act}`, `{seq}` and `{scene}` name the page's first panel. |

`{index:3}` pads a number with zeros (`007`); names that are not numbers are
left as they are.

**Save profile** keeps the current options under the name you type; saving
under an existing name replaces it. Built-in profiles cannot be replaced, so
save a changed one under a new name. **Share…** writes the profile as a
`.json` file and **Import…** reads one, so a studio can use one layout. A
shared profile refers to its logo by its path on disk. Fields a profile lists
that a board does not have are left out, with a note under the preview.

### Panel images

**File → Export Panel Images…** writes one PNG (transparency kept) or JPEG
(on white) per panel into a folder you choose. The **File name pattern** uses
the tokens above, for example `{seq}_{scene}_{panel}` or `SC{scene:3}_{index:4}`,
and the dialog shows the first file name it gives. Characters that file names
cannot hold become `_`.

**One image per layer** writes an image for each visible top-level layer of
each panel, with the layers clipped to it; add `{layer}` to the pattern, or the
layer name is added at the end. Choose all panels, the selected panels or one
scene. If the pattern would give two images the same name, nothing is written
and the dialog says so: add `{index}` or `{panel}`.

### Captions CSV

**File → Export Captions CSV…** writes one row per panel: its position, act,
sequence, scene and panel names, duration in frames and seconds, start, end
and duration as timecode at the board's frame rate, every caption field as
plain text, shot size, angle, status, tag, whether it is locked and whether it
is a thumbnail sheet. Thumbnail sheets take no time. Fields with commas, quotes
or line breaks are quoted, so spreadsheets read them as one cell.

## Not yet available

Transitions, the timeline and animatic playback arrive with the timeline phase;
camera moves come later, and PDF pages then draw their frames and arrows. The light table does not show while the view is
rotated. See the
[Storyboard Pro parity plan](../specs/storyboard-pro-parity.md#delivery-phases).
