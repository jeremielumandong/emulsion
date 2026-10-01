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
| Timeline | Ctrl+Alt+T |

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
in and out. Right-click a clip to rename it, set its gain in dB, show its
sound in the library or delete it. Click a clip and press **Delete** to
remove it.

**Markers** are named points on a track for timing panels to sound: press
**M** with the Timeline focused (or **Add marker at playhead** in **Timing ▾**
or a track's menu) to add one at the playhead on the selected track. Drag a
marker to move it; right-click to rename or delete it. With the Timeline
focused, the arrow keys step the playhead one frame (Shift: one second).

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
take times them in order. Recording sound while timing comes with sound
recording.

## Sound files

Sounds come into the board's sound library from WAV, MP3, M4A, AAC, FLAC,
OGG, Opus and AIFF files. Emulsion reads them with FFmpeg, so FFmpeg
(`ffmpeg` and `ffprobe`) must be installed and on your PATH; without it,
importing, waveforms, sound playback and movie export say so instead of
working. Importing copies the file: editing, moving or deleting the
original afterwards changes nothing in the board.

Sounds are saved inside the `.emu` file, exactly as imported (no
re-encoding), up to 2 GiB of sound per storyboard. While a storyboard is
open its sounds are kept in a temporary media folder, which Emulsion clears
the next time it starts. A storyboard whose saved sound is missing or
damaged does not open, rather than opening with silent clips.

## Export and print

**File** offers five storyboard exports. Each one uses the board as it is,
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

The dialog shows the size, length and frame count before you choose where to
save. The export runs in the background with a frame counter; **Cancel
export** stops it. An existing file is replaced only when the export
finishes. Panels are drawn over white.

**File → Export Animated GIF…** writes a looping GIF with the same range,
render area and burn-in choices, at 320–1280 pixels wide and 6–24 frames per
second: the animatic is sampled at that rate, so a 12 fps GIF of a 24 fps
board shows every other frame. GIFs have no sound and at most 6,000 frames.

## Not yet available

Camera moves come later, and PDF pages then draw their frames and arrows. The light table does not show while the view is
rotated. See the
[Storyboard Pro parity plan](../specs/storyboard-pro-parity.md#delivery-phases).
