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
- Or choose **Start from a script…** under the Storyboard size to start from a
  Fountain, Final Draft or text script: the storyboard is created with the
  chosen size and your preferences, then holds one panel per action paragraph
  or dialogue block (see [Import a script](#import-a-script)). The script's
  title names the project.

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
  panel, each named after its file and fitted to the frame. SVG files arrive
  as editable vectors. PDF and Illustrator (`.ai`) files add one panel per
  page, in file then page order, named after the file ("Layouts page 2"),
  as editable vector art; up to 200 pages a file.
- **Import script…**: lays a screenplay out as panels; see
  [Import a script](#import-a-script).
- **Conform to Edit…**: takes durations, order and sound from an edit made
  in editing software; see [Editorial interchange](#editorial-interchange).

Each import is one Undo step; locked panels are refused.

PDF and Illustrator pages are converted by Poppler (`pdftocairo`) or MuPDF
(`mutool`), which must be installed and on your PATH; without them the import
says so and adds nothing. A progress card shows the page being converted, with
**Cancel**. Illustrator files open when they were saved with PDF
compatibility (Illustrator's default).

### Paper worksheets

Draw on paper and bring the drawings back onto their panels.

**File → Print Worksheets…** saves sheets of empty frames as a PDF to print
(at 100% scale). Choose:

- **Panels**: all panels, the Board's selected panels, one scene, or **New
  panels** and how many. Frames for existing panels are labelled with their
  scene and panel number ("Scene 3 · Panel 2") and print the panel's captions
  over ruled lines; new frames read "New panel 1", "New panel 2"…
- **Layout**: a storyboard PDF profile (see
  [Storyboard PDF and printing](#storyboard-pdf-and-printing)) sets the panels
  per page (up to 12) and where the caption lines go.
- **Paper** and **Orientation**.

Frames have the board's shape. Each sheet carries a black square mark in each
corner and a QR code in its header naming this storyboard, the sheet and the
panel of every frame. Draw inside the frames with pencil or ink, and keep the
corner marks and the code clear.

**File → Import → Paper Worksheets…** takes photos or flatbed scans of the
drawn sheets (JPEG, PNG, HEIC and the other formats **File → Open** reads),
several at once. They are read in the background, with progress and
**Cancel** on the progress card; each sheet's code and corner marks are found
even when the photo is turned, taken at an angle or unevenly lit, and every
frame is straightened and cut out at the panel's resolution. The preview then
shows each photo's sheet and frames with the panel each goes to:

| Option | What it does |
| --- | --- |
| **Paper transparent** | Default. Light and paper colour are evened out, the paper becomes transparent and strokes keep their colour. |
| **Paper white** | The paper becomes even white; the frame stays opaque. |
| **Line art only** | Black strokes only: paper, light shading and specks are dropped. |
| **Photo as it is** | Only straightened. |
| **Add a layer** / **Replace earlier paper drawings** | Add the drawing on top, or remove the panel's earlier paper drawing layers first (to bring in a redrawn sheet). |

**Import** puts each drawing on its panel as a new top layer named "Paper
drawing (date)"; frames for new panels become panels after the active panel,
in print order. The whole import is one Undo step; locked panels refuse it.
Empty frames are skipped, and a sheet photographed twice is read once.

- A sheet printed from another storyboard is refused with its project named:
  open that storyboard to import it.
- When a photo's code cannot be read (covered, torn, out of focus), choose the
  layout the sheet was printed with under **Sheets without a code**; its
  frames become new panels, read in the corner marks' order with the sheet
  upright.
- Photograph the whole sheet, flat, filling most of the picture; a scan at
  150–300 ppi works best.

### Import a script

**File → Import → Import script…** reads a Fountain (`.fountain`, `.spmd`),
Final Draft (`.fdx`) or plain text (`.txt`) script. Choose the file to see its
title and how many scenes and beats it has, then choose:

- **Panels**: one per action paragraph or dialogue block, or one per scene
  with all of its action and dialogue.
- **Insert**: after the active panel's scene, or at the end of the board.

Each scene heading starts a new scene named after it. Action goes in the
**Action** caption, dialogue in **Dialogue** ("MIA (quietly): Is anyone
there?") and the heading in the first panel's **Slugging** caption; fields the
board lacks are added. Panels start with a duration from their words, by
the default word rates of [Estimate durations from
captions](#estimate-durations-from-captions) (never shorter than the default
panel length), and DISSOLVE, FADE and WIPE transitions become panel
transitions. Plain text becomes action, one panel per paragraph. The import
is one Undo step, and the new panels are selected on the Board.

### Break down a script with the assistant

The import above splits a script mechanically. **Break down with the
assistant…** in the same dialog hands the chosen script to the assistant
instead, which works like a storyboard artist:

1. It reads the script scene by scene (`read_storyboard_script`): headings,
   beats with IDs such as `s2b5`, speaking characters, word counts and
   estimated seconds.
2. It plans shot coverage: an establishing wide shot for each new location,
   a panel per dialogue exchange with a close-up for the line that turns the
   scene, inserts for key objects and action, reaction panels.
3. It builds the whole breakdown in one step
   (`build_storyboard_from_breakdown`): scenes named by their headings,
   panels with Action, Dialogue and Slugging captions, shot notes in
   **Notes**, the camera move in a **Camera** caption, shot size and angle,
   and a duration. Missing caption fields are added, and the scenes land
   after the active panel's scene or at the end, as chosen under
   **Insert**. It checks every beat ID against the script and covers the
   beats it missed.
4. It estimates durations as a dry run and adjusts action beats by
   judgement.

The breakdown is one Undo step, and the assistant's turn shows in the
assistant dock. The button needs an assistant CLI (see Settings); without
one it is disabled and the dialog says why.

**Edit → Paste in Place** (Ctrl+Shift+V) pastes copied layers at the position
they were copied from, so a character or prop lands in the same place on
another panel, or in another open document. On the Board it pastes panels.

In the layers panel, every row has an eye and a lock. Press one and drag
across other rows to set them all the same way, as one Undo step.

## Layouts and shortcuts

**Window → Layout** offers three storyboard layouts:

- **Overview**: the Board with the Panel inspector open, for arranging and
  writing.
- **Drawing**: the Stage with Paint's toolbars and the layers panel, for
  drawing. **Reset layout** returns to it.
- **Timing**: the Stage over the [Timeline](#timeline), with the Panel
  inspector, for timing panels to sound.

Saved layouts work as in Paint and remember whether the Board and the
Timeline are open. The storyboard layout you last used comes back when you
open a storyboard.

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
| Check spelling | Ctrl+Alt+H |
| Timeline | Ctrl+Alt+T |
| Shared Project / Check for changes | Ctrl+Alt+Y / Ctrl+Alt+Shift+Y |

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
- **Review**: the panel's review status and notes, and review layers (see
  [Review](#review)).
- **Layers** shows the panel's layers.

**Caption fields…** adds, renames, reorders and removes caption fields, and
sets whether each is multi-line and printed. Removing a field removes its text
from every panel.

## Camera

Each scene has one camera. Its keys count frames from the scene's first
panel, so a single move can run across several panels. With no keys the
camera shows the whole frame.

Turn on **Move camera** in the Stage toolbar (or **View → Camera → Camera
Tool**, Ctrl+Alt+E). The scene camera's frame shows in red over the panel,
at the playhead while it is inside the panel, otherwise at the panel's first
frame:

- drag **inside** the frame to pan;
- drag a **corner** to zoom (towards the centre zooms in);
- drag **just outside a corner** to turn it (Shift snaps to 15°).

Each drag sets the key at the playhead, or updates the key already there,
and is one Undo step. The tool takes clicks on the Stage, so it never draws;
Space still pans the view. The bar at the top of the Stage reads the scene
frame, the keys and the camera's zoom and angle, and holds the commands:

| Command | What it does |
| --- | --- |
| **◀** / **▶** | Move the playhead to the scene's previous or next camera key. |
| **Add key** | Key the camera as it is at the playhead (**Update key** on a key). |
| **Delete key** | Remove the key at the playhead. |
| **Ease ▾** | How the camera eases from the key at (or before) the playhead: linear, ease in, ease out, ease in and out, or hold. |
| **Shake ▾** | Handheld, Bumpy ride or Earthquake shake for the whole scene, **Shake Settings…** (amplitude in pixels, tilt in degrees, wobbles per second and a seed, such as `4, 0.4, 1.2, 1`) or **No Shake**. The same seed always shakes the same way. |
| **Hold panel** | Static camera: keys at the panel's first and last frames with the camera as the panel starts, and no keys between, so it holds still. |
| **Reset** | Remove the scene's camera keys (shake stays until you choose No Shake). |
| **Copy** / **Paste** | Copy one scene's camera (keys and shake) and give it to another. |

Every command is one Undo step, and is also in **View → Camera**. When a
panel's duration changes, the camera keys of its scene stretch with it, as
the Timeline's keyframe option says.

The Timeline shows a **Camera** row while any scene has a camera (or the
tool is on): each scene's span with its keys as diamonds. Click a key to
move the playhead there; drag it to retime it (between its neighbours, one
Undo step).

**Camera view** (Ctrl+Alt+K) shows only the shot: outside the camera frame
at the playhead is masked. The player, movie and GIF exports show every
frame through the camera (shake included), with layer keyframes applied,
and panels render larger when the camera zooms in so they stay sharp. On
storyboard PDFs, a panel whose camera moves prints the frame where the move
starts and where it ends, with an arrow between them (at the profile's
camera frame and arrow thickness).

| Command | Default |
| --- | --- |
| Camera tool | Ctrl+Alt+E |
| Add camera key | Ctrl+Alt+Shift+E |
| Delete camera key | Ctrl+Alt+Shift+Backspace |
| Previous / next camera key | Ctrl+Alt+Shift+, / Ctrl+Alt+Shift+. |

## Shot Generator

Build a panel's shot in 3D, Storyboarder style, and draw over it. Press
**3D** on the Stage toolbar (or **View › Shot Generator**,
**Ctrl+Alt+Shift+G**) to open the active panel's set in place of the
Stage: add posable mannequins, props and lights, pose them with presets,
joints, IK, hand shapes and faces, choose a lens, shot size and angle, or
type a shot ("low-angle close-up of two people at a table") and pick
angles from the Shot Explorer. **Use as reference layer** renders the camera
view into a locked, half-transparent "Shot Generator" layer above the paper,
re-rendered whenever the set changes; **Snapshot to layer** makes an
editable copy instead. Duplicated panels keep their set, so the next shot
of a scene starts from the same staging.

The panel inspector's **3D set and layer depth** section opens the Shot
Generator, places layers in depth for parallax under the scene camera and
lets a layer follow an object of the set. See [Storyboard 3D and Shot Generator](storyboard-3d.md) for the view,
models, posing and the renderer.

## Layer animation

Each panel can animate its layers: position, scale, rotation, skew, opacity
and adjustment-layer values, keyed at frames within the panel. A layer's own
placement is its rest pose; keys hold the offsets from it. The Stage shows
the panel as it looks at the playhead (on another panel, at its first
frame).

- **◆ Key** on the Stage toolbar, or **◆ Set key** in the Panel inspector's
  **Layer animation** section, keys the selected layer's current position,
  scale, rotation, skew and opacity at the playhead.
- **Auto-key**: while it is on, moving, scaling and turning the selected
  layer with the Move tool's box records keys at the playhead instead of
  moving the layer. Each drag is one Undo step. With Auto-key off, a drag
  moves the layer's rest pose and its keys move with it. On an animated
  layer, the Move tool's box follows the layer as the Stage shows it.
- The **Layer animation** section lists the selected layer's values at the
  playhead, each with a key toggle (◆ keyed here, ◇ not), so you can key
  or unkey opacity or a single value. **Values…** types exact values;
  changed values become keys. Skew is set there.
- **Pivot**: the layer turns, scales and skews about its pivot, at the
  layer's centre until you move it. The pivot shows as a cross on the Stage
  with the Move tool; drag it (Alt-drag where it sits on a key point).
  **Centre** puts it back.
- **Motion path**: the Stage draws the selected layer's path over the
  panel's frames, with a handle at each position key. Drag a handle to
  move that key (one Undo step).
- **Adjustment values**: on an adjustment layer, **◇ Key** under each
  parameter slider keys that value at the playhead (set the slider, then
  key it). The slider sets the value where there are no keys.
- **Easing**: select a key on the Timeline (or a path handle) and the
  section shows how the move to the next key eases: **Linear**, **Ease
  in**, **Ease out**, **Ease in and out**, **Hold**, or **Custom curve**.
  Drag on the graph to shape a custom curve with its two handles.
- **When durations change**: keys stretch with the panel (the default) or
  keep their frames. This is a board setting, also under **Timing ▾** on
  the Timeline, and applies to every duration edit.

On the Timeline, each panel with layer keys has a row under the panel
track; ▸ opens it into a row per animated layer, and a layer into a row
per property (opacity and adjustment values included). Diamonds mark keys:
click to select, drag to retime within the panel, **Delete** to remove.
Locked panels keep their keys.

## Layer comps

A layer comp saves which of a panel's layers are hidden, to switch the
panel's look later. The Panel inspector's **Layer comps** section lists the
panel's comps: **Save current as…** saves the layers hidden now under a
name (replacing a comp of that name), **Apply** shows and hides the layers
as saved, and **Rename…** and **Delete** manage the list. Each is one Undo
step; locked panels refuse them.

## Find and replace captions

Choose **Edit → Find and Replace Captions…** or press Ctrl+H. Search all
caption fields or one, with **Match case** and **Whole word**. Results list the
panel, field and context; click one to select its panel. **Replace All** is one
Undo step and reports how many matches were replaced and how many locked panels
were skipped.

## Spelling

Captions are checked against a bundled English (US) dictionary and your own
words. Words in capitals (names, sluglines and character cues), words with
digits and single letters are left alone.

- In the Panel inspector, a caption with misspelt words shows **n spelling
  issues** under its field. Its menu lists each word with corrections; choose
  one to replace the word (one Undo step, keeping the caption's formatting).
- **Add to dictionary** keeps the word in your personal dictionary, for every
  storyboard. **Ignore** accepts it until Emulsion quits.
- **Edit → Check Spelling…** (Ctrl+Alt+H) lists every misspelt word on the
  board with its panel, field and context. Click a row to select its panel; **Fix** offers the
  same corrections, Add to dictionary and Ignore.

Turn checking off, or remove words from the personal dictionary (one at a time
or **Clear dictionary**), in **Settings → Storyboard**.

## Library

The **Library** tab at the top of the sidebar keeps drawings you reuse:
characters, props, backgrounds and whole set-ups. It has two parts:

- **In this storyboard**: the project library, saved in the `.emu` file, so it
  travels with the storyboard and with templates made from it.
- **Personal library**: shared by every storyboard on this computer and kept
  with your other creative library assets.

An item is **layers** (one or more layers from a panel), a **panel** (a whole
panel) or a **scene** (a whole scene). To add one, select layers in the Layers
dock and choose **Add layers…**, choose **Add panel…** for the active panel,
or **Add scene…** for its scene. Give it a name
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

### Animated library items

Panel and scene items keep their animation. **Add panel…** on an animated
panel saves its duration, layer keyframes, layer comps and the scene camera's
moves over it; placing it adds the panel with its keys and comps, and the
camera moves join its new scene's camera over that panel. **Add scene…** saves
the active panel's whole scene: every panel with its drawing, duration,
captions, keyframes and comps, and the scene camera. Placing a scene item adds
it as a new scene after the active panel's scene, in one Undo step. Cards show
**animated** and a scene's panel count. Timing keeps its length in seconds on
a storyboard at another frame rate. Items saved before animation still place
as they did.

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
- **Check spelling in captions** and the **personal dictionary** (see
  [Spelling](#spelling)).
- **Audio input device**: the microphone the Timeline and the Panel Timer
  record from (see [Recording sound](#recording-sound)). **System default**
  follows the system's choice; **Refresh** lists the inputs again after you
  plug one in.
- **Scratch voices**: the text-to-speech engine scratch dialogue speaks with
  (**Automatic**, **Piper** or **eSpeak NG**) and the folder of your
  downloaded Piper voices (see [Scratch voices](#scratch-voices)). The row
  says which engines are installed; **Refresh** checks again.
- **External editor**: the program **Edit in external editor** opens panels
  with (a path such as `/usr/bin/krita` or `C:\Program Files\GIMP 2\bin\gimp-2.10.exe`,
  a command on the PATH, or an app such as `Adobe Photoshop 2025.app` on
  macOS), and whether it gets OpenRaster (`.ora`) instead of PSD. Leave it
  blank to use the system's app for the file type (see
  [Edit in an external editor](#edit-in-an-external-editor)).

Each storyboard keeps its own naming rules and Smart add list once created.
To give an existing storyboard the current ones, choose **Apply storyboard
preferences** in the Board's panel menu (one Undo step), then **Renumber…** if
you want existing names to follow.

## Timeline

The Timeline docks under the Stage or the Board. Open it with **Timeline** in
the panel strip, **View → Timeline**, **Ctrl+Alt+T** or the **Timing** layout.
Drag its top edge to make it taller.

- **Ruler**: frames or SMPTE timecode (**Timecode**/**Frames** switches;
  29.97 and 59.94 fps count drop-frame). Click or drag the ruler to move the
  playhead; the panel under it becomes the active panel. While the Timeline
  is open, its toolbar holds the transport bar (see
  [Playing the animatic](#playing-the-animatic)). The play range shows on the
  ruler; drag its ends to change it.
- **Zoom and scroll**: **+**, **−** and **Fit**, or Ctrl+scroll (Cmd on
  macOS) to zoom around the pointer; Shift+scroll, a sideways scroll or the
  scroll bar move through time.
- **Panel track**: each panel is as long as its duration, with its picture,
  name and duration. Scenes are tinted and start at a dark line with the
  scene name. Thumbnail sheets do not play, so they are not on the Timeline.
  Click a panel to select it (the same selection as the Board; Shift and Ctrl
  work the same way); double-click opens it on the Stage. Right-click for
  **Set duration…**, **Fit selection to duration…** and **Snap cuts to
  markers**.

### Timing panels

Drag a panel's right edge:

| Drag | What changes |
| --- | --- |
| Drag | The panel's duration; later panels move along (ripple). |
| Alt+drag | The cut moves; the next panel gives or takes the frames, so the total stays the same (roll). |
| Shift+drag | The selected panels' total duration, scaled in proportion (retime). |

While you drag, the toolbar shows the new duration as timecode and frames.
Drags snap to cuts, markers, the playhead and the play range when **Snap** is
on; hold Ctrl (Cmd) to drag freely. Each drag is one Undo step. Locked panels,
or panels in locked scenes, keep their timing: drags and commands that would
change them are refused.

**Set duration…** and **Fit selection to duration…** (also in **Timing ▾**)
take frames (`36`), seconds (`1.5s`) or timecode (`00:00:01:12`, or just
`1:12`). Fitting scales the selected panels to the total you type, keeping
their proportions. **Snap cuts to markers** moves every cut within half a
second of an audio marker onto it.

### Estimate durations from captions

**Timing ▾ → Estimate durations from captions…** times panels from the words
in their captions, offline:

- **Dialogue** is spoken at the dialogue rate (150 words a minute by
  default), plus a pause after each line (0.5 s) and each parenthetical such
  as "(beat)" (0.5 s). Speaker names before a colon ("MIA:") are not
  counted.
- **Action** reads at the action rate (120 words a minute by default).
- A panel lasts at least the minimum (1 s by default); its dialogue and
  action add up.

Only the **Dialogue** and **Action** captions count; panels without words in
them keep their duration. Choose the scope (**Selected panels**, the active
panel's **Scene** or the **Whole board**) and the preview lists each scene's
and panel's old → new duration and the total. **Apply** changes them as one
Undo step: locked panels keep their length, layer keyframes follow the
keyframe sync mode and transitions shorten to fit. The rates you apply are
remembered for next time and used by the assistant. Script imports use the
same model with the default rates.

Words time speech well but not action: a fight or a reveal usually needs
longer than its sentence, and a held look needs a beat with no words, so
adjust those panels afterwards. The assistant does the same through
`estimate_storyboard_durations`: a dry run first, then it refines action
beats by judgement.

### Transitions

The button at each cut opens the transitions menu for the panel after it:
**Cut**, **Dissolve**, **Wipe** and **Slide** (from the left, right, top or
bottom), **Clock wipe**, **Iris**, **Fade to black**, **Fade to white** and
**Fade to colour…**. A new transition lasts half a second. A transition
plays over the first frames of its panel, so it never changes timing; it
shows as a dark band there. Drag the band's end, or choose **Length…**, to
change it. A transition is never longer than its panel and shortens with it.

### Audio tracks

**Add track** adds an audio track (up to 16). Each track has a name, **M**
(mute), **S** (solo) and a volume slider (−60 to +24 dB). Right-click a
track's name to rename it, add a marker or delete it.

Clips show their waveform. Drag a clip to move it, along its track or to
another track; clips on a track never overlap. Drag a clip's ends to trim it
(the sound stays in place), and the round handles at its top corners to fade
in and out. Right-click a clip to rename it, set its gain in dB, open its
**Effects…**, add a gain key at the playhead, show its sound in the library
or delete it. Click a clip and press **Delete** to remove it.

**Markers** are named points on a track for timing panels to sound: press
**M** with the Timeline focused (or **Add marker at playhead** in **Timing ▾**
or a track's menu) to add one at the playhead on the selected track. Drag a
marker to move it; right-click to rename or delete it. With the Timeline
focused, the arrow keys step the playhead one frame (Shift: one second).

### Clip effects

Each clip has a **gain envelope** and a three-band **EQ**: a low shelf at
200 Hz, a peak at 1 kHz and a high shelf at 5 kHz. Both can change over the
clip with keys, eased like layer keys. Right-click a clip and choose
**Effects…**:

- The sliders set the envelope (−60 to +24 dB, added to the clip's gain;
  fades still apply) and each EQ band (−24 to +24 dB) at the playhead. Move
  the playhead on the Timeline to work on another point; when it is outside
  the clip, keys go at the clip's nearest end.
- **◇** adds a key at the playhead with the value there; **◆** (a key is
  there) removes it. **‹** and **›** move the playhead to the previous or
  next key. The easing menu sets how the value moves from that key to the
  next (**Hold** keeps it until the next key). **Clear** removes a row's
  keys, keeping its value at the playhead.
- An EQ band without keys has one fixed gain; once it has keys, it follows
  them. The envelope always uses keys; without any it is 0 dB.

The envelope shows as a yellow line on the clip, with a square at each key:
drag a key sideways to retime it (it stays between its neighbours and inside
the clip) and up or down to change its level; right-click it to ease or
delete it. A clip with effects shows **fx** after its name. Trimming a
clip's start keeps its keys where they are in the sound.

Effects play everywhere sound plays: the player, scrubbing, and movie
export all use the same mixdown. Every change is one Undo step.

### Recording sound

**● Record** on the Timeline toolbar records from the microphone chosen in
**Settings → Storyboard → Audio input device** (or the system default),
starting at the playhead, with no count-in. The animatic plays while you
record, so you can perform to the pictures; a level meter and the recorded
time show beside the button. **■ Stop** ends the take. **into … ▾** chooses
the track: **New track** (the default) or an existing track; when the take
would overlap a clip there, it goes on a new track instead.

The take is added to the sound library in the **Recordings** folder as
"Recording 1", "Recording 2"… (a 16-bit WAV, saved in the `.emu` like any
imported sound) and placed as a clip; adding and placing it is one Undo
step. Adding a recording needs FFmpeg, like importing. Use headphones if the
board already has sound, or the microphone records it too.

If there is no microphone, the chosen one is not connected, or the system
does not allow Emulsion to use it, Record says so and nothing is recorded.
On macOS, allow Emulsion in **System Settings → Privacy & Security →
Microphone**; on Windows, in **Settings → Privacy → Microphone**.

### Scratch voices

A scratch dialogue track lets you time the animatic to the script before
anyone records it. The voices come from a text-to-speech engine installed on
your computer; **nothing is sent over the network**. Emulsion uses
[Piper](https://github.com/rhasspy/piper) (`piper`, natural-sounding voices
from `.onnx` voice files you download, each with its `.onnx.json`) when it is
installed and has a voice, otherwise [eSpeak NG](https://github.com/espeak-ng/espeak-ng)
(`espeak-ng`, robotic but available everywhere). Put the program on your
PATH, keep Piper voices in one folder and choose it in **Settings →
Storyboard → Piper voices folder**; the engine choice there can force one
or the other. With neither installed, the commands say what to install.

Dialogue is read from the **Dialogue** caption, one line per caption line
in the form script import writes: `MIA (quietly): Is anyone there?`. The
name before the colon is the character; parentheticals and extensions such
as `(V.O.)` are not spoken, and a caption line without a name continues the
line before it.

**Timing ▾ → Voice cast…** lists every character who speaks on the board.
For each, choose a voice (a Piper voice and, for voices with several
speakers, a speaker; or an eSpeak NG language and variant), a **Rate** (×0.5
to ×2) and a **Pitch** (0–99; 50 is the voice's own; Piper voices are shifted
afterwards). **▶ Preview** speaks a sample. Characters left on **Default
voice** get different voices of the chosen engine. **Save cast** keeps the
cast with the board (one Undo step).

**Timing ▾ → Generate scratch dialogue…** speaks the **Selected panels**,
the **Active scene** or the **Whole board**. Each panel's lines play back to
back from the panel's start on the **Scratch dialogue** track (a take that
would overlap another goes on "Scratch dialogue 2"), and the sounds go in
the library's **Scratch dialogue** folder. One track keeps the cast together
and leaves room for your own tracks (a board has up to 16); clip names say who
speaks ("MIA: Is anyone there?"). With **Lengthen panels to fit their
lines**, unlocked panels that are shorter than their lines grow to fit;
locked panels keep their length and are reported. Generating again for the
same panels replaces their earlier scratch takes, never sounds you imported
or recorded. The voices are made in the background with a progress card you
can cancel, and placing them is one Undo step.

To change how one line is delivered, right-click its clip and choose
**Regenerate line…**: change its **Rate** and **Pitch** (added to the
character's voice), **Emphasis** (None, Moderate, Strong; eSpeak NG stresses
the words, Piper speaks a little slower and livelier) and, for Piper,
**Variation** (how expressive the voice is). The clip plays the new take;
the earlier take stays in the library. One Undo step.

### Enhance dialogue

Right-click any dialogue clip — a recording, an imported line or a scratch
line — and choose **Enhance dialogue…** to clean it up through FFmpeg: rumble
below 80 Hz is cut, steady background noise reduced, harsh "s" sounds tamed,
levels evened by a compressor and the loudness set to −16 LUFS. The result
is a new sound (named "… (enhanced)", in the same library folder); the
original sound is kept. Choose whether **the clip plays the enhanced sound**
or the clip stays and **a copy plays it on the Enhanced dialogue track**
(mute or delete one of them to hear the other alone). It runs on your
computer in the background with Cancel, and is one Undo step.

### Sound library

**Sounds** in the Timeline toolbar opens the board's sound library beside
the tracks. **Import…** adds sound files (see [Sound files](#sound-files))
into the current folder. **New folder** makes a folder inside the current
one; click a folder to make it current, click it again to fold it, and
right-click to rename it. Right-click a sound to rename it, move it to a
folder, place it at the playhead or delete it (only while no clip uses it).
**Delete unused** removes every sound no clip uses. Sounds are kept in the
`.emu` file.

Click a sound to preview it: a waveform you can zoom (**+**/**−** or
Ctrl+scroll) and scroll, with **in** and **out** points to drag. Drag the
preview, or a sound in the list, onto a track to place it where you drop it,
or press **Place on track** to place the in–out part at the playhead on the
selected track. Placing, moving and every other library change is one Undo
step.

### Reference video

A video (live action, previs, an earlier cut) can sit above the audio
tracks as timing reference. The **Video** row's **Import…** button, or
right-clicking its name, imports a video file at the playhead: MP4, MOV,
M4V, MKV, WebM or AVI. **Import video with its sound…** also brings the
video's sound into the sound library and lines a clip of it up under the
video on the first audio track with room (after that the two clips move
separately). You can also drop video files on the Video row, at the frame
you drop them on. A video that overlaps another goes on a new video track
(up to 4).

Video clips show a strip of their pictures. Drag a clip to move it, along
its track or to another video track; drag its ends to trim it (the picture
stays in place, and a clip never runs past the end of its video). Like audio
clips, video clips keep their frames when you retime panels. Right-click a
clip to rename it, set its **Opacity…**, **Hide** or **Show** it, **Lock**
it (a locked clip cannot be moved, trimmed or deleted) or delete it. Click a
clip and press **Delete** to remove it. Every change is one Undo step, and
a video no clip uses leaves the project.

The picture under the playhead shows over the Stage and over the player,
frame for frame. **View → Reference Video** chooses **Overlay** (the
picture fitted over the panel at the clip's opacity, the default),
**Picture in Picture** (a small inset at the bottom right) or **Hidden**.
Pictures are read in the background as you scrub or play, so the Timeline
never waits for them; when the computer cannot keep up, the newest frame
wins. Where clips on several tracks overlap, the top track's visible clip
shows.

Videos are read with FFmpeg and saved inside the `.emu` file as imported,
up to 2 GiB of video per storyboard (beside the 2 GiB of sound).

## Playing the animatic

Press **Play** (▶) on the transport bar (in the Timeline's toolbar, or at the
bottom right of the Stage or Board while the Timeline is closed), tap **Space**, or choose **View → Play / Pause Animatic**. The
animatic plays over the Stage at the board's frame rate: every panel for its
duration, thumbnail sheets left out, with each panel's transition. When the
board has sound, the pictures follow the sound card's clock, so they never
drift from what you hear; if the computer is slow, frames are skipped rather
than falling behind. Without sound, or without an audio device, playback
runs on the system clock (the transport bar says when it plays without
sound).

Pausing leaves the animatic on screen; **Stop** (■ or **Esc**) goes back to
drawing on the panel under the playhead. While stopped, moving the playhead
(stepping, or dragging it on the Timeline) plays a short grain of sound
there, so you can find a line or a beat by ear.

The **play range** limits playback: **In** and **Out** set its start and end
at the playhead, and the range's timecodes on the bar clear it. **Loop**
plays the range (or the whole animatic) over and over.

**Options ▾** on the transport bar sets the **burn-in** drawn over the
pictures: timecode, scene and panel numbers, one caption field, at the top or
bottom, in five text sizes. Movie exports use the same burn-in drawing. The
burn-in choices are remembered, and the movie and GIF export dialogs start
with them.

**Play full screen** on a display (from **Options ▾**, or **View → Play
Animatic Full Screen** for the main display) opens a full-screen window on
that display with only the pictures, for a second monitor or a projector.
Space plays and pauses there, the arrow keys and comma and full stop step
frames, and **Esc** stops and closes it.

| Command | Default |
| --- | --- |
| Play / pause | Space (a quick tap on the Stage; holding Space still pans) |
| Stop | Esc |
| Previous / next frame | , / . |
| Start / end of the play range | Home / End |
| Set in / out | Shift+I / Shift+O |
| Clear the play range | Alt+X |
| Loop | Alt+Shift+R |

These work on the Stage and the Board and can be changed in **Settings →
Shortcuts**.

## Panel Timer

The Panel Timer times panels while you perform: act out the scene or read
the lines, and tap at each cut. Open it from **Options ▾ → Panel Timer…** or
**View → Panel Timer…**.

1. Choose **Time the selected panels** (select them on the Board first) or
   **Create new panels**.
2. Press **Space** or **T** to start, then tap **Space** or **T** at the end
   of each panel. Timing the selection stops after its last panel; for new
   panels, or to stop early, press **Esc**. **Play the sound while timing**
   plays the board's sound from the first panel (or the playhead).
3. Review the take: a table lists each panel with its old and new duration
   in frames. Correct any new duration, then **Apply**, or **Retake**.

Applying is one Undo step. Timing the selection changes the panels in order
(a short take changes only the first ones); new panels are blank, named by
the naming rules and go after the selection. When a thumbnail sheet is
selected, **Convert sheet to panels** turns it into panels first and the
take times them in order.

**Record from the microphone while timing** records the take's sound with
the same input as the Timeline's **Record**, starting with the first timed
panel. The review says how much was recorded; **Apply** places it on a new
audio track from the first timed panel (a second Undo step after the
timing), and **Retake** or closing the timer discards it.

## Edit in an external editor

Draw a panel in Photoshop, Krita, GIMP or any painting app and have every
save come straight back. Choose **View → Edit Panel in External Editor** for
the active panel, or **Edit in external editor** in a Board card's panel menu.

1. The panel is written as a layered PSD (or OpenRaster, see
   [Preferences](#preferences)) into a temporary folder of this project and
   opened in the external editor from **Settings → Storyboard** (the system's
   app for the file type when none is set).
2. A chip over the Stage says **Editing *panel* in *app*…**. Draw and save
   there as often as you like: each save is read once the file has stopped
   changing and replaces the panel's drawing as one Undo step, **Edit in
   *app***.
3. **Stop** ends the edit and deletes the temporary files. Starting another
   external edit, or closing the project, does the same.

The panel keeps its duration, captions, shot data and camera. Layers keep
their identity by name: a layer that comes back with the same name keeps its
layer keyframes and layer comps; renamed or new layers are new layers. A
drawing saved at another size is fitted to the panel the way imports are
(centre crop and scale), so the panel never changes size. A locked panel
refuses the save (unlock it and save again).

If the panel also changed in Emulsion since it went out (or since the last
save came back), the chip asks what to do with the save:

| Choice | What happens |
| --- | --- |
| **Keep both** | The saved layers go on top of your layers, named *layer (app)*. |
| **Take *app*'s** | The save replaces the drawing, as when nothing changed here. |
| **Keep mine** | The save is ignored; the next save is compared with your drawing. |

## Sketch Sprint

A Sketch Sprint is a timed drawing session for fast thumbnails and roughs.
Choose **View → Sketch Sprint…**; a card over the Stage sets the time per
panel (15 seconds to 10 minutes) and how many panels (4 to 40), and whether
to draw on **New panels** (added after the active panel) or **From this panel
on** (the active panel, then the ones after it, adding panels at the end).

- **Start** begins the countdown in the Stage's top right corner. When a
  panel's time runs out the Stage moves to the next panel by itself; the
  countdown turns red in the last five seconds.
- **Pause** stops the clock (strokes drawn while paused are not recorded);
  **Resume** carries on. **Stop** ends the session early.
- At the end a summary gives the panels drawn, the drawing time, the strokes
  and the line drawn (see [Line mileage](#line-mileage)).
- **Time-lapse GIF…** and **Time-lapse movie…** (H.264, needs FFmpeg) play
  the session back stroke by stroke, panel by panel, sped up to about twenty
  seconds (never slower than real time), holding the last picture.

Everything drawn in a sprint is ordinary drawing: each stroke is its own Undo
step, and new panels are named by the naming rules.

## Line mileage

Emulsion adds up the length of every stroke you draw: Brush and Eraser
strokes along the path the pen took, vector lines, and the outlines of
shapes (Line, Rectangle, Ellipse, Polyline and the Shape tool). Mask painting
and healing do not count. The panel inspector's **Line mileage** shows the
active panel's total and the project's, in metres, centimetres or
millimetres at the panel's resolution (72 pixels per inch unless the
document says otherwise), and over a metre how many football pitches that
is.

Mileage counts ink that is on the page: undoing a stroke takes its length
off and **Redo** puts it back, and a stroke undone and then replaced by
another edit never counts. Mileage is saved with the storyboard; deleting a
panel takes its mileage out of the project total. **Reset** counts the panel
from zero and **Reset all** the whole project; resetting is not an Undo step.
Agents read it with `describe_storyboard_mileage` (see
[MCP: storyboards](mcp/mcp-storyboard.md)).

## Sound files

Sounds come into the board's sound library from WAV, MP3, M4A, AAC, FLAC,
OGG, Opus and AIFF files. Emulsion reads them with FFmpeg, so FFmpeg
(`ffmpeg` and `ffprobe`) must be installed and on your PATH; without it,
importing, waveforms, sound playback and movie export say so instead of
working. Importing copies the file: editing, moving or deleting the
original afterwards changes nothing in the board. Recordings (see
[Recording sound](#recording-sound)) are stored the same way, as WAV.

Sounds are saved inside the `.emu` file, exactly as imported (no
re-encoding), up to 2 GiB of sound per storyboard. While a storyboard is
open its sounds are kept in a temporary media folder, which Emulsion clears
the next time it starts. A storyboard whose saved sound is missing or
damaged does not open, rather than opening with silent clips.

## Change tracking

A **board version** is a named snapshot of the whole storyboard: every
panel's drawing, its name, captions, timing, shot details, camera and layer
keys, and the order of the board. Choose **View → Review → Save Board
Version…** (or **Save version…** in the Changes list) before a round of
changes, such as sending the board to the director. Versions are kept in the
project's history: each panel's drawing is recorded in that panel's version
history (its History panel lists it under the version's name), so a version
costs little space. Save the project to keep them; versions are not Undo
steps. Templates leave versions out.

**Changes…** on the Board toolbar (or **View → Review → Changes Since…**)
lists what changed since a version, the **last save** (the board as you last
opened or saved it) or the **last export** (as you last opened an export or
print dialog in this session). Pick the point with **Since**. Each panel is
one of:

| Mark | Meaning |
| --- | --- |
| **New** (green) | Not in the earlier board. |
| **Changed** (orange) | Says what changed: drawing, captions, timing, camera, layer keys, shot details, name or review (review notes and review layers). |
| **Moved** (blue) | Its place among the panels both boards share changed. Adding or deleting panels does not move the others. |
| **Deleted** (red) | Only in the list, with its picture from the earlier board. |

Panels are matched by their identity, so renaming a panel keeps it the same
panel; a panel deleted and pasted back is matched by name.

- **Show marks** outlines new, changed and moved panels on the Board with a
  badge, and draws a coloured bar over them on the Timeline. **View →
  Review → Show Change Marks** (Ctrl+Alt+Shift+M) turns the marks on and
  off.
- **Next** and **Previous** (Ctrl+Alt+] and Ctrl+Alt+[) make the next or
  previous changed panel active, on the Stage and the Board.
- Click a row to go to that panel.

Comparing runs in the background and follows your edits.

## Compare versions

**View → Review → Compare Versions…** (or **Compare…** in the Changes list)
shows two states of the board panel by panel: pick the **Older** and
**Newer** side from the versions, the last save, the last export or the
board now. Panels are paired by identity, then by name. Each row shows both
pictures, what changed, the durations (`48 → 36 frames`) and, for each
caption that differs, the words added (highlighted) and removed (struck
through). **Unchanged panels too** lists every panel.

Click a row to see the pair at full size: **Side by side**, **Wipe** (drag
across the picture: the older version on the left of the line, the newer on
the right) or **Onion skin** (the newer picture at half strength over the
older one). Reading an old version never changes the open board.

## Review

Each panel has a review status and review notes, in the **Review** section
of the Panel inspector:

- **Status**: No review, To do, In review, Approved or Needs changes. The
  Board shows it as a badge on the picture, with the number of open notes.
- **Notes**: type a note and press Enter or **Add note**. Notes are signed
  with the name in **Settings → Storyboard → Your name** (also used for
  scene claims and cloud saves) and the time. **Resolve** closes a note (it stays, greyed out);
  **Reopen** brings it back; **Delete** removes it.
- Each status change and note is one Undo step. Locked panels can still be
  reviewed.

**Show** on the Board toolbar shows only panels with one status, or with
open notes.

**Review layers** are for drawing corrections over a panel. **New review
layer** in the Review section, or **View → Review → New Review Layer**
(Ctrl+Alt+R), adds an empty layer with a violet colour label above the
selected layer. **Make selected layer review-only** turns any layer (or
group) into a review layer, and back. Review layers draw on the Stage like
any layer but are left out of every export: PDF boards and printing, panel
images (also one image per layer), movies, GIFs and layered exports.
Thumbnails show them unless **Settings → Storyboard → Hide review layers in
thumbnails** is on.

To print the review, turn on **Review notes** in the Captions options of
the storyboard PDF: each panel's status and open notes print after its
captions, signed with their authors.

## Export and print

**File** offers six storyboard exports. Each one uses the board as it is,
including unsaved changes, and never changes it. Review layers are never
exported (see [Review](#review)).

### Colour management

With OpenColorIO on (Settings › Color management), the Stage and the
animatic player show panels through the chosen display and view (ACES 1.0 by
default), and the PDF, panel image, movie and GIF exports are converted to the
display's colours or to the colour space set under **Exports in**. Each
storyboard can keep its own working colour space (**this storyboard**). See
[Colour management](color-management.md).

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
| Captions | Position (below, right or left of the panel, or none), the share of the panel's box they get, frames around them, field names in bold, which fields to print (empty prints the fields marked for printing in **Caption fields…**), text size, and **Review notes** (off by default: each panel's review status and open notes after its captions; see [Review](#review)). Captions keep their bold, italic, underline, strikethrough and colour. |
| Header and footer | Page header and footer text, their alignment and size, and a PNG or JPEG logo with its position and height. |
| Camera | The camera frame on each panel, the board's action and title safe areas, their line thickness, and the line thickness of camera-move arrows: a panel whose camera moves prints its start and end frames and an arrow between them. |

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

### Animatic export

**File → Export Movie…** writes the animatic as a movie: every panel for its
duration (thumbnail sheets left out), with its transition, and the sound of
every audible track mixed in (muted tracks are left out; when any track is
soloed, only soloed tracks play), with each clip's gain and fades.

- **Format**: **H.264 (MP4)** plays almost everywhere; **ProRes 422 (MOV)**
  is for editing software; **PNG image sequence** writes `frame_00000.png`,
  `frame_00001.png`… into a folder, numbered by animatic frame, with the
  sound as `soundtrack.wav`. MP4 and MOV need FFmpeg.
- **Size**: the width; the height follows the render area's shape (rounded
  to even numbers for MP4 and MOV). **Full size** uses the panels' own
  resolution, up to 3840 pixels wide.
- **Range**: the whole animatic, the Board's selected panels (from the first
  to the end of the last), or one scene. Timecode burn-in counts from the
  start of the animatic, so a scene exported alone keeps its timecodes.
- **Render area**: **Camera frame** shows each panel as drawn; **With
  overscan** adds the board's overscan margin (see Stage guides); **All
  artwork** widens the picture to everything drawn on any panel, so nothing
  drawn outside the frame is cut off.
- **Burn-in**: none, the timecode, or scene, panel and timecode, optionally
  with one caption field (such as the dialogue), at the top or bottom. It is
  drawn exactly as in the player.
- **Quality** (draft, good or best) and **With sound** / **No sound**.
- **Reference video**: when the board has reference video, draw it over
  the panels (with each clip's opacity) or inset at the bottom right.

The dialog shows the size, length and frame count before you choose where to
save. The export runs in the background with a frame counter; **Cancel
export** stops it. An existing file is replaced only when the export
finishes. Panels are drawn over white.

**File → Export Animated GIF…** writes a looping GIF with the same range,
render area and burn-in choices, at 320–1280 pixels wide and 6–24 frames per
second: the animatic is sampled at that rate, so a 12 fps GIF of a 24 fps
board shows every other frame. GIFs have no sound and at most 6,000 frames.

### Layered scene export

**File → Export Layered Scenes (ORA, PSD)…** prepares scenes for animation
production. Each panel becomes a layered file, and each scene a JSON file
that describes its timing, camera and animation:

- **Format**: **OpenRaster (.ora)** or **Photoshop (.psd)**. Layers keep
  their names, groups, blend modes, opacity, visibility, masks and clipping,
  through the same writers as File → Save As. Review layers are left out.
- **Scenes**: all scenes, the scenes of the Board's selected panels, or one
  scene.
- **Panel file names** use the panel tokens of
  [Storyboard PDF and printing](#storyboard-pdf-and-printing) (default
  `{seq}_{scene}_{panel}`); **Scene file names** use `{project}`, `{act}`,
  `{seq}` and `{scene}` (default `{seq}_{scene}`). Patterns that would give
  two files one name write nothing.

The export runs in the background with a panel counter and **Cancel
export**.

Each scene file (`schema` `emulsion.storyboard.scene/1`) holds:

| Field | Meaning |
| --- | --- |
| `project`, `project_id` | The storyboard's name and its project ID. |
| `act`, `sequence`, `scene`, `scene_id` | Where the scene sits; `scene_id` is stable across exports. |
| `width`, `height`, `frame_rate` | Panel size in pixels; `frame_rate` has `num`, `den`, `fps` and `drop_frame`. |
| `start_frame`, `frames`, `timecode_in`, `timecode_out` | The scene's place on the running time (SMPTE timecode, drop-frame at 29.97 and 59.94; `timecode_out` is the first frame after it). |
| `camera` | The scene camera, or `null`: `keys` (frames from the scene start, `x`, `y` as the shot centre in panel pixels, `zoom`, `rotation` in degrees, `easing` and an optional bezier `curve` with `x1`, `y1`, `x2`, `y2`) and `shake`. |
| `panels` | The scene's panels in order, below. |

Each panel has:

| Field | Meaning |
| --- | --- |
| `id`, `name`, `number`, `index` | Panel ID, name, number in its scene and position in the board. |
| `file` | Its layered file, beside the JSON. |
| `frames`, `seconds`, `start_frame`, `scene_frame`, `timecode_in`, `timecode_out` | Duration and place, on the running time and from the scene start. Thumbnail sheets (`thumbnail_sheet`) take no time. |
| `transition` | How it enters from the panel before (`kind`, `frames`, and `edge` or `color` where they apply), or `null` for a cut. |
| `shot`, `angle`, `captions` | Shot size, camera angle and each caption field's plain text. |
| `camera` | When the scene has a camera: the framing at the panel's first (`start`) and last (`end`) frame without shake, and the `keys` that fall in the panel, with `frame` from the panel start and `scene_frame` from the scene start. |
| `layers` | Every layer, bottom to top: `id` (stable), `name`, `parent` (a group's `id`), `kind` (`px`, `grp`, `adj`, `fill`, `path`, `text`, `vec`, `smart`), `visible`, `opacity`, `blend` and `clip_to`. |
| `layer_keyframes` | Per animated layer: `layer_id`, `layer_name`, `pivot` and `tracks`, each a `property` (`x`, `y`, `scale_x`, `scale_y`, `rotation`, `skew_x`, `skew_y`, `opacity`, or `{"effect": key}`) with `keys` of `frame` (from the panel start), `value`, `easing` and `curve`. |
| `layer_comps` | Each comp's `name` and the layers it hides (`id`, `name`). |

## Editorial interchange

An animatic can go to editing software and come back. Emulsion writes and
reads three formats: **CMX 3600 EDL** (`.edl`), **Final Cut Pro 7 XML**
(`.xml`, also read by Premiere Pro, DaVinci Resolve and Avid Media Composer)
and **OpenTimelineIO** (`.otio`). AAF is not supported; use one of these
with Avid.

### Export an edit

**File → Export Edit (EDL, Final Cut XML, OpenTimelineIO)…** writes the
board as it is (unsaved changes included):

- Panels play end to end on the first picture track, starting at
  01:00:00:00; thumbnail sheets are left out. At 29.97 and 59.94 fps
  timecodes use drop-frame numbering (`01:00:00;00`).
- Each panel's transition starts at its cut. EDLs name dissolves and wipes
  from the left or top; other transitions are written as dissolves (Final
  Cut XML and OpenTimelineIO keep them).
- Sound clips go on their own tracks with their gain (clip gain plus track
  volume). EDLs hold four sound tracks and no levels.
- Final Cut XML and OpenTimelineIO also carry the reference video on a
  second picture track and the audio markers.
- **Panel media**: **Stills (PNG)** or **Movies (ProRes MOV)**, each the
  panel through its camera with its layer motion, and as many held frames
  after its end as the next panel's transition needs. Movies need FFmpeg.
  The width is 1920 or 1280 pixels, or the panels' own size.

The media go into a folder named after the edit with `_media` (for
`Film.edl`, `Film_media`), together with copies of the sounds and reference
videos. Clip names are the panel names, and each file name ends with the
panel's ID (`Panel_3_p12.png`), so a clip the editor renames still finds
its panel. The dialog shows progress; **Cancel export** stops it, and any
format limits are listed when it finishes.

### Conform to an edit

**File → Import → Conform to Edit…** reads an edit and shows what would
change before anything does:

- Picture clips are matched to panels by name (two panels with one name
  match in board order), or by the panel ID in the media file name.
- Matched panels take the edit's durations and transitions. A gap in the
  edit is added to the panel before it. A dissolve in the edit keeps a
  panel's own transition kind when it has one (EDLs cannot name an iris,
  for example), with the edit's length.
- If the edit reorders panels, the ones it moved go to their new places and
  join the scene they land in; the rest keep their scenes.
- Clips with no panel are listed, as are panels the edit leaves out: those
  keep their place and duration, so sound after them may no longer line up.
- Sound clips that play a sound of this board (matched by the file names
  Export Edit wrote, or by the sound's name) replace the board's sound
  clips, keeping each clip's fades, envelope and EQ. Clips playing other
  files are listed and left out. An edit with no sound leaves the board's
  sound alone.
- When the edit's frame rate differs from the board's, choose **Convert:
  keep times** (durations in seconds stay) or **Keep frame counts**. EDLs
  carry no frame rate and are read at the board's, or at 29.97 when they
  say drop frame.

**Apply** conforms the board as one Undo step and shows what changed.
Locked panels keep their durations and transitions, and an edit that moves
a locked panel cannot be applied until it is unlocked.

## Extract and merge

To hand part of a board to another artist, extract it, and merge their work
back when it returns.

### Extract scenes

**File → Extract Scenes…** writes a run of neighbouring whole scenes to a
new storyboard file. Choose the first and last scene (the dialog starts
from the scenes of the Board's selected panels) and **Extract to…**. The
new `.emu` holds:

- the panels with their drawings, names, captions, timing, shot details,
  transitions, layer keyframes and comps;
- the scenes' cameras, and their acts and sequences;
- the sound and reference video under the scenes: clips are cut at the
  range's ends and moved to start at frame 0, with the sounds and videos
  they play;
- the whole project library (placed library items are copies, so the board
  cannot tell which ones a panel used).

To mark the scenes as taken, check **Claim these scenes for** and enter
the artist's name (it starts as yours): the extract and this board both
carry the claim (see [Scene claims](#scene-claims)).

The board itself does not change, but it counts as unsaved until you save
it: the extract remembers this project's ID (every storyboard gets one,
saved with it from its first save), which
scenes and panels it took with a fingerprint of each panel's content, and
when it was made. The other artist opens it like any storyboard, edits,
adds or deletes panels and scenes, and saves it.

### Merge extracted scenes

**File → Merge Extracted Scenes…** → **Choose extract…** reads the file and
compares it with the board as it is now. The dialog shows how many panels
and how long the range is here and in the extract, and lists conflicts:

| Conflict | Default |
| --- | --- |
| **Changed here since the extract was made** (and whether the extract changed it too) | Take theirs if the extract changed it, otherwise keep mine. |
| **Deleted here, still in the extract** | Take theirs (bring it back) if the extract changed it, otherwise keep it deleted. |
| **Deleted in the extract, still here** | Keep mine if it changed here, otherwise delete it. |
| **Added here since the extract was made** | Keep mine. |

Choose **Take theirs** or **Keep mine** for each, then **Apply**. The
scenes are replaced as one Undo step:

- Panels come in the extract's order; panels you keep that the extract
  does not have go back after their neighbour.
- Scenes keep their identity, take the extract's names and cameras;
  scenes the artist added are new scenes in the same sequence. Acts and
  sequences stay as they are on this board.
- Caption fields match by name; fields the extract added are added.
- Sound and reference video in the range come from the extract (its tracks
  match by name), cut to the new range, and everything after the range
  moves by the change in running time, so sound stays in sync.
- Library items the extract added are added to the project library.

Merging needs the same frame rate and resolution. An extract of another
project is refused; if this project is a copy of the one it came from,
check **Merge anyway**. Locked panels in the range must be unlocked first.
If the board changes while the dialog is open, **Apply** shows the
conflicts again instead of merging.

## Shared projects

A storyboard [synced to a cloud account](cloud-setup.md#shared-storyboards)
can be shared by a team: everyone opens their own copy of the same cloud
file, works, and saves. There is no server database and no live
co-editing. Each save is an immutable cloud revision, so when two artists
save from the same version both saves are kept, side by side, until someone
merges them. Your saves are never overwritten.

**File → Shared Project…** (Ctrl+Alt+Y) shows:

- the file's cloud sync (provider, revisions, paused uploads);
- the **collaborators** seen in its revisions: the name each person set in
  **Settings › Storyboard › Your name**, or their computer, with how many
  saves and when;
- **Waiting to merge**: other artists' saves this copy does not include
  yet, each with **Review and merge…**;
- the **scene claims**, with **Release**, and **Claim selected scenes**;
- **Check for changes** (Ctrl+Alt+Shift+Y anywhere), which syncs and looks
  again. The app also checks when a synced storyboard opens, after each
  save uploads and every minute while it runs, and says in the status bar
  when someone else saved.

### Review and merge

**Review and merge…** downloads the other save and the version you both
started from (checked against their fingerprints, and kept for next time),
then lists what they changed since then, with pictures and change
tracking's marks (new, changed and what changed, moved, deleted). Your own
unsaved work stays: the merge goes into the board as it is now.

Everything only one of you changed comes across by itself, part by part:

- **Panels** match by identity. A panel changed by one artist takes that
  change. When both changed the same panel, each part merges separately
  (drawing, name, timing, each caption field, shot details, layer keys,
  review), so your caption and their drawing both stay; only a part you
  both changed differently is a conflict.
- New panels on either side are kept, beside the panels they followed. A
  panel deleted by one artist and untouched by the other is deleted.
- **Order**: if one of you reordered panels, that order is used.
- Scenes, sequences and acts (names, locks), caption fields, cameras,
  sounds, audio and video clips, the library and board settings merge the
  same way. Review notes and board versions from both sides are kept.

Conflicts are listed with a choice each: **Keep mine**, **Take theirs**,
or for a panel **Keep both** (yours, then theirs as a new panel after it,
named "… (theirs)"). A panel deleted on one side and changed on the other,
both reordering the panels, two renames of one scene, overlapping sound
clips on one track and the like are conflicts too. A conflict you leave
keeps your version; nothing is decided for you, and their save stays in
the cloud either way.

**Apply and save** merges as one Undo step and saves. The upload names
both saves as its parents, so everyone then sees one latest version;
the other artist's next check offers your merge to them, which brings
them level. Locked panels are never changed by a merge: unlock them
first. Merging needs the same resolution on both sides.

### Scene claims

A claim says who is working on a scene ("I'm on scenes 4–6"). Select
panels on the Board (or stay on a panel of the scene) and choose **Claim
selected scenes** in the Shared Project dialog; it uses your name from
Settings › Storyboard. Claims show as a badge on the Board's scene header
and after the scene name on the Timeline. They are advisory: on a panel in
a scene someone else claimed, the Stage shows who claimed it, and the
first edit there says so in the status bar, but nothing is blocked.
**Release** ends a claim (anyone's). Claims are saved with the board and
travel with every revision; when copies merge, the latest claim or release
of each scene wins. Claiming and releasing are Undo steps.

## AI tools

The AI tools from Paint and Photo work on panels too. They use the local
models from **Settings › Local models** and, only when you ask for a prompt,
the image provider you chose under **Settings › Image generation**. Local
tools work offline. When a model is missing, the tool names it and points to
Local models; nothing runs until it is installed. A panel never changes
size: the panel resolution stays the project resolution.

On the Stage, **Edit** holds:

| Command | What it does |
| --- | --- |
| **Select Subject** | Selects the subject of the panel, as in Paint; refine it in the Select tool's options. The Select tool's AI quick select (click a thing or drag a box) works on panels too. |
| **Remove Background…** | Cuts the subject of a layer (or of the whole panel) out into a new layer above it and hides the original. |
| **Expand Panel Image…** | Shrinks the picture inside the frame by 10, 15 or 25 % on each side and fills the new border. With a prompt, the image provider paints the border; without one, the local fill model continues the surroundings. The result is a new layer; the original is hidden. |
| **Upscale Layer…** | Replaces a layer with a copy holding more detail: the upscale model enlarges it, the copy keeps up to twice the layer's size on the panel (four times for a layer already shown enlarged), and it is scaled to sit exactly where the original was. The panel stays the same size, so the layer stays sharp when it is enlarged or under a camera push-in. The original is hidden. |
| **Generative Fill…** | Paints the selection, the pixels of a named layer (for example Sky), or the whole frame into a new layer; the original stays. With a prompt it uses the image provider; without one the local fill model fills from the surroundings. The whole frame needs a prompt. |
| **AI on Panels…** | The same dialog with every operation. |

Paint's commands work on the active panel as well: **Upscale** and **Expand**
in Enhance and the Layers panel run the panel versions above, so the panel
keeps its size. The Select tool's prompt field fills the selection or makes a
new layer, as in Paint.

The dialog lists the operations (Select subject, Subject mask, Remove
background, Upscale, Denoise, Expand, Generative fill), the **Layer** to work
on (blank for the whole panel as it looks; on the Stage it starts with the
selected layer) and, for Expand and Generative fill, the **Prompt**. The
prompt field shows only when an image provider is set up, with a note saying
which provider the prompt and the panel's picture go to; without a provider
the dialog says that prompts need one and that the local model is used
instead. Nothing is sent anywhere without a provider you chose.

### AI on selected panels

On the Board, select panels and choose **AI on selected panels…** in the
panel menu (or **Edit → AI on Panels…**). The operation runs on each panel in
turn, on the layer with the name you give (for example Background or a
character's layer) or on the whole panel:

- **Select subject** sets each panel's selection; **Subject mask** adds a
  mask to the named layer; **Remove background**, **Upscale** and **Denoise**
  add a new layer above it and hide the original; **Expand** adds an
  expanded layer; **Generative fill** fills each panel's own selection, the
  pixels of a named layer, or the whole frame, from one prompt.
- The progress card shows the panel being worked on with **Cancel**.
  Cancelling stops after the panel in progress and changes nothing.
- Panels that fail (no layer of that name, no selection, a provider error)
  are listed in the dialog and the status bar with the reason; the others
  still change. Locked panels are skipped and listed. A panel you draw on
  while the batch runs is left alone and listed.
- Everything the batch changed is one Undo step.

Panels run one after another in this project, so the Batch tab's file
folders are not used.

## Not yet available

The light table and the reference video do not show while the view is
rotated, and the reference video does not show in the full-screen player
or in GIF exports. See the
[Storyboard Pro parity plan](../specs/storyboard-pro-parity.md#delivery-phases).
