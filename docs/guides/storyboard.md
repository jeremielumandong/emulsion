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

Each storyboard keeps its own naming rules and Smart add list once created.
To give an existing storyboard the current ones, choose **Apply storyboard
preferences** in the Board's panel menu (one Undo step), then **Renumber…** if
you want existing names to follow.

## Not yet available

Transitions, the timeline and animatic playback arrive with the timeline phase;
camera moves and the light table come later. See the
[Storyboard Pro parity plan](../specs/storyboard-pro-parity.md#delivery-phases).
