# MCP: storyboards

A storyboard is a project whose pages are panels at one resolution. These tools
let a connected assistant turn a written scenario into scenes, timed panels with
captions and shot data, and drawn frames. They need the live workspace relay and
act on the storyboard in the relay's tab. Every changing call is one Undo step.

| Tool | Purpose |
| --- | --- |
| `create_design_project` | With `kind: "storyboard"`, create a storyboard project: `width` × `height` panels, `pages` blank panels. |
| `describe_storyboard` | Read the frame rate, naming rules, Smart add layers, caption fields (with `multiline` and `print`), running time, active panel and the act → sequence → scene → panel outline with each panel's ID, duration, captions, shot size, angle, status, tag, lock and layer count. Captions are plain text; a caption with styled text also lists its `formatting` ranges in characters. Thumbnail sheets list their cell rectangles. |
| `set_storyboard_settings` | Set the frame rate (`23.976`–`60`), the default duration of new panels, the `naming` rules (scene prefix, start, step and padding; panel prefix and padding; per-scene panel numbers; letters for inserted scenes) and the `smart_add_layers` list. |
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
| `add_storyboard_caption_field` | Add a caption field (`multiline`, `print`, `position`). |
| `update_storyboard_caption_field` | Rename a field, change `multiline` or `print`, or move it to `position`. |
| `remove_storyboard_caption_field` | Remove a field and its text on every panel (asks for confirmation). |
| `format_storyboard_caption` | Style part of a caption (bold, italic, underline, strikethrough, colour, size and other character styles) by `start`/`end` character offsets, by `match` text, or the whole caption. |
| `find_in_storyboard_captions` | Find text in captions (`match_case`, `whole_word`, optional `field`). Read-only. |
| `replace_in_storyboard_captions` | Replace text in captions with the same options, keeping formatting. Returns `replaced` and `locked_panels_skipped`. |

The project tools work on panels too:

| Tool | In a storyboard |
| --- | --- |
| `select_project_page` | Select a panel before drawing on it. |
| `duplicate_project_page` | Make the next frame: the copy goes right after its source, in the same scene, with its layers, timing, captions and shot data, and becomes the active panel. |
| `copy_page_nodes` | Copy chosen layers (a character, a prop) from one panel to another with a `dx`/`dy` offset. Works in any project. |
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

Offsets in `format_storyboard_caption`, `find_in_storyboard_captions` and
`formatting` are Unicode characters, not bytes. Invalid calls change nothing.

The assistant's system prompt includes a storyboarding playbook (shot sizes,
continuity, timing and this workflow); its worked example is executed by the
`storyboard_playbook_runs_against_a_live_storyboard_project` test.
