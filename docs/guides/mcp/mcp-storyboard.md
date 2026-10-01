# MCP: storyboards

A storyboard is a project whose pages are panels at one resolution. These tools
let a connected assistant turn a written scenario into scenes, timed panels with
captions and shot data, and drawn frames. They need the live workspace relay and
act on the storyboard in the relay's tab. Every changing call is one Undo step.

| Tool | Purpose |
| --- | --- |
| `create_design_project` | With `kind: "storyboard"`, create a storyboard project: `width` × `height` panels, `pages` blank panels. |
| `describe_storyboard` | Read the frame rate, naming rules, Smart add layers, Stage `guides` with their rectangles in panel pixels, the colour `palette`, caption fields (with `multiline` and `print`), running time, active panel and the act → sequence → scene → panel outline with each panel's ID, duration, captions, shot size, angle, status, tag, lock and layer count. Captions are plain text; a caption with styled text also lists its `formatting` ranges in characters. Thumbnail sheets list their cell rectangles. |
| `set_storyboard_settings` | Set the frame rate (`23.976`–`60`), the default duration of new panels, the `naming` rules (scene prefix, start, step and padding; panel prefix and padding; per-scene panel numbers; letters for inserted scenes), the `smart_add_layers` list, the Stage `guides` (action and title safe %, field guide and `fields`, `overscan` %) and the `palette` (`reset`, `set`, `remove`, `add` #RRGGBB colours). Returns the guides and palette. |
| `add_storyboard_panels` | Add up to 200 blank panels after a panel (or `at_start`) with durations (`seconds` or `frames`), captions by field name and shot data. `start` begins a new scene, sequence or act named `group_name`. Returns the new panel IDs. |
| `update_storyboard_panel` | Change one panel's duration, captions (merged; an empty string clears a field; unchanged text keeps its formatting), shot size, angle, status or colour tag. |
| `start_storyboard_group` | Split: start a new scene, sequence or act at a panel; the rest of its group moves with it. |
| `join_storyboard_group` | Join a scene, sequence or act into the one of the same level before it. |
| `rename_storyboard_group` | Rename an act, sequence or scene by group ID. |
| `renumber_storyboard` | Rename scenes and/or panels by the naming rules, for the whole board or within chosen `groups`. Locked scenes and panels keep their names. |
| `set_storyboard_locks` | Lock or unlock `panels` and `scenes`. Locked panels refuse drawing, data changes, moving and removal; a locked scene protects all its panels. |
| `smart_add_storyboard_panel` | Add the next panel in the same scene, with the same shot size and copies of the source panel's layers named in the Smart add list. |
| `move_storyboard_panels` | Move panels after a panel or `at_start`, across scene boundaries; `scene` (ID) or `scene_name` puts them in that scene (at its end when no position is given). |
| `set_storyboard_thumbnail_sheet` | Make a panel a thumbnail sheet with `columns` × `rows` cells (optional `gap` and `margin` in pixels), or `clear` it. Returns the cell rectangles. Sheets do not count towards running time. |
| `convert_storyboard_thumbnails` | Turn a sheet into one full-size panel per cell, in row order, in the sheet's place. |
| `copy_storyboard_panels` | Copy `panels` and/or whole `scenes` and paste them after a panel or `at_start`. Whole scenes come back as new scenes unless `new_scenes` is false. |
| `import_storyboard_panels` | Import panels from another storyboard `.emu` file (`path`), optionally only the named `scenes`. Caption fields are matched by name, durations keep their time and other resolutions are fitted. |
| `import_storyboard_files` | Bring in PSD/PSB, ORA, PNG, JPEG, WebP or TIFF files by absolute `paths`: as new panels named after the files (`into: "panels"`, after a panel or `at_start`) or as layers on top of `panel` (`into: "layers"`). Pictures are cropped to the centre and fitted to the frame; groups, masks, blend modes and clipping are kept. Returns the panels or layers with each layer's `blend` and `clipped_to`. |
| `add_storyboard_caption_field` | Add a caption field (`multiline`, `print`, `position`). |
| `update_storyboard_caption_field` | Rename a field, change `multiline` or `print`, or move it to `position`. |
| `remove_storyboard_caption_field` | Remove a field and its text on every panel (asks for confirmation). |
| `format_storyboard_caption` | Style part of a caption (bold, italic, underline, strikethrough, colour, size and other character styles) by `start`/`end` character offsets, by `match` text, or the whole caption. |
| `find_in_storyboard_captions` | Find text in captions (`match_case`, `whole_word`, optional `field`). Read-only. |
| `replace_in_storyboard_captions` | Replace text in captions with the same options, keeping formatting. Returns `replaced` and `locked_panels_skipped`. |
| `list_storyboard_library` | List library items (`scope`: `project`, `personal` or `all`; optional `query` over names and tags): ID, name, tags and kind (`layers` or `panel`); project items also give their size and top-level layer names. Read-only. |
| `add_to_storyboard_library` | Add a drawing to the `project` library (saved in the `.emu`, the default) or the `personal` library (shared by every storyboard): with `layers`, copies of those layers at their positions; without, the whole `panel` (default: the active panel). `name` and optional `tags`. |
| `place_storyboard_library_item` | Place an `item` from a `scope`: a layers item goes on top of the active panel at its original position, a panel item becomes a new panel after it. One Undo step. |
| `update_storyboard_library_item` | Rename an item and, with `tags`, replace its tags. |
| `remove_storyboard_library_item` | Delete an item (asks for confirmation). Undo restores a project item; a personal item and its file are removed for every storyboard. |
| `list_storyboard_templates` | List installed storyboard templates with their resolution, frame rate and panel count. Read-only. |
| `save_storyboard_template` | Save this storyboard as a template in the personal library (`name`, optional `tags`, `author`, `license`, `description`). Returns the `template` ID. |
| `create_storyboard_from_template` | Workspace tool: open a new tab with an unsaved copy of a template (`template`, `name`), fresh history. |

The project tools work on panels too:

| Tool | In a storyboard |
| --- | --- |
| `select_project_page` | Select a panel before drawing on it. |
| `duplicate_project_page` | Make the next frame: the copy goes right after its source, in the same scene, with its layers, timing, captions and shot data, and becomes the active panel. |
| `copy_page_nodes` | Copy chosen layers (a character, a prop) from one panel to another with a `dx`/`dy` offset. Without an offset the copy is pasted in place, at the same position in the frame. Works in any project. |
| `move_project_page`, `delete_project_page` | Reorder or remove panels. A panel dropped between panels of another scene joins that scene. |
| `describe_project`, `save_project`, `export_project` | Page list, saving the `.emu` and image/PDF export. |

Drawing uses the ordinary editing tools on the selected panel (`add_layer`,
`paint`, `draw_path`, `draw_shape`, `add_text`, `translate_node`,
`set_transform`, `get_view` and others).

## From a scenario

1. Create the project, then call `describe_storyboard` for the caption field
   names (`Action`, `Dialogue`, `Slugging`, `Notes` by default).
2. Break the scenario into scenes and beats. Add each scene with one
   `add_storyboard_panels` call:

```json
{"after":1,"start":"scene","group_name":"Kitchen","panels":[
  {"seconds":3,"size":"wide","captions":{"Slugging":"INT. KITCHEN - NIGHT","Action":"Mia stands at the sink."}},
  {"seconds":2,"size":"close_up","angle":"low","captions":{"Dialogue":"MIA: Who's there?"}}
]}
```

3. Select each panel and draw it on named layers (background, each character,
   arrows), so later frames can reuse them.
4. For a continuing shot, `duplicate_project_page` the panel, move or repaint
   what changes, and update its captions and duration with
   `update_storyboard_panel`.
5. Check the outline and total running time with `describe_storyboard`, and
   each panel with `get_view`.

## Editing the board

- **Thumbnails first.** Make a panel a sheet with `set_storyboard_thumbnail_sheet`,
  draw one rough frame inside each returned cell, then
  `convert_storyboard_thumbnails`:

```json
{"panel":6,"columns":3,"rows":2}
```

- **Keep the set.** Name the set layer `Background` (or list other names in
  `smart_add_layers`) and use `smart_add_storyboard_panel` for each new beat.
- **Rearrange.** `move_storyboard_panels`, `start_storyboard_group` and
  `join_storyboard_group` change order and grouping; `copy_storyboard_panels`
  and `import_storyboard_panels` reuse panels and scenes.
- **Protect approved work.** `set_storyboard_locks` with `locked: true`.
- **Tidy names.** Set `naming` with `set_storyboard_settings`, then
  `renumber_storyboard` (for example `{"scenes":false}` to renumber only panels).
- **Rename a character.** Check with `find_in_storyboard_captions`, then
  `replace_in_storyboard_captions` with `match_case` and `whole_word` for each
  spelling:

```json
{"query":"MIA","replacement":"MAYA","match_case":true,"whole_word":true}
```

## Stage guides, palette and outside art

`describe_storyboard` returns the guides drawn over the camera frame and their
rectangles in panel pixels: `action_safe_rect` and `title_safe_rect` (`null`
when set to 0), `field_rects` from field 1 to the frame (empty while the field
guide is off) and `stage_area`, the frame plus overscan on every side. Turn on
a 12-field guide and add a colour to the palette:

```json
{"guides":{"field_guide":true,"fields":12},"palette":{"add":["#E07020"]}}
```

The default palette has six greys for roughs, an accent, and red, blue and
green for notes and corrections. The light table, camera view and flipped view
are Stage view settings in the app, so they have no tools; agents compare
panels with `get_view`.

Bring a layered layout into the current panel (or omit `into` for one new panel
per file):

```json
{"paths":["/Users/me/Layouts/sc12.psd"],"into":"layers","panel":4}
```

Drawing uses the shared tools: `paint` with `mirror` or `symmetry`, brushes from
`list_brushes` (`import_brushes` imports `.abr`), `set_blend_mode`, `set_clip`
and `add_mask`.

## Library and templates

Draw a character once, then reuse it on every panel it appears in. Select its
layers with `describe_document`, add them to the project library, and place
the item on each new panel; it lands where it was drawn:

```json
{"name":"Mia","tags":["character"],"layers":[12,13]}
```

```json
{"item":1}
```

Use `scope: "personal"` for drawings that belong in every storyboard, such as a
recurring set. Project library changes and placing are Undo steps in the live
project; personal library changes and templates are saved on disk at once.
`save_storyboard_template` captures the board's settings, caption fields,
naming, Smart add layers, guides, palette, library and panels;
`create_storyboard_from_template` starts a new storyboard from one.

Offsets in `format_storyboard_caption`, `find_in_storyboard_captions` and
`formatting` are Unicode characters, not bytes. Invalid calls change nothing.

The assistant's system prompt includes a storyboarding playbook (shot sizes,
continuity, timing and this workflow); its worked example is executed by the
`storyboard_playbook_runs_against_a_live_storyboard_project` test.
