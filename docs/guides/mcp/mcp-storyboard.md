# MCP: storyboards

A storyboard is a project whose pages are panels at one resolution. These tools
let a connected assistant turn a written scenario into scenes, timed panels with
captions and shot data, and drawn frames. They need the live workspace relay and
act on the storyboard in the relay's tab. Every changing call is one Undo step.

| Tool | Purpose |
| --- | --- |
| `create_design_project` | With `kind: "storyboard"`, create a storyboard project: `width` × `height` panels, `pages` blank panels. |
| `describe_storyboard` | Read the frame rate, naming rules, Smart add layers, Stage `guides` with their rectangles in panel pixels, the colour `palette`, caption fields (with `multiline` and `print`), running time, active panel and the act → sequence → scene → panel outline with each panel's ID, duration, animatic `start` frame and `timecode`, `transition` (absent for a cut), captions, shot size, angle, status, tag, lock and layer count. Captions are plain text; a caption with styled text also lists its `formatting` ranges in characters. Thumbnail sheets list their cell rectangles. `animatic` holds the total frames and timecode, audio tracks with clips and markers, and the sound library (see [the animatic](#the-animatic-timing-transitions-and-sound)). `video` holds the reference video tracks with their clips and the videos they show (see [Reference video](#reference-video)). `animation` holds the keyframe sync mode and counts scenes with a camera and panels with layer keyframes or comps; such scenes show `camera` (`keys`, `shake`) and such panels `animated_layers` and `comps`. |
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
| `import_storyboard_script` | Lay a screenplay out as panels: Fountain (`.fountain`, `.spmd`), Final Draft (`.fdx`) or plain text, by absolute `path`. Each scene heading starts a scene; `split` `beat` (default, one panel per action paragraph or dialogue block) or `scene` (one panel per scene). Text goes to the Action, Dialogue and Slugging captions, durations come from the words and DISSOLVE/FADE/WIPE lines become transitions. The scenes go after the scene holding `after` (default: the active panel) or `at_start`. Returns the `title`, `scenes` and new `panels`. |
| `import_storyboard_pdf` | Add every page of a PDF or Illustrator (`.ai`, PDF-compatible) file at an absolute `path` as a new panel of editable vector art, after a panel or `at_start`; panels are named after the file (`Layouts page 2`). Needs Poppler (`pdftocairo`) or MuPDF (`mutool`) on PATH. |
| `add_storyboard_caption_field` | Add a caption field (`multiline`, `print`, `position`). |
| `update_storyboard_caption_field` | Rename a field, change `multiline` or `print`, or move it to `position`. |
| `remove_storyboard_caption_field` | Remove a field and its text on every panel (asks for confirmation). |
| `format_storyboard_caption` | Style part of a caption (bold, italic, underline, strikethrough, colour, size and other character styles) by `start`/`end` character offsets, by `match` text, or the whole caption. |
| `find_in_storyboard_captions` | Find text in captions (`match_case`, `whole_word`, optional `field`). Read-only. |
| `replace_in_storyboard_captions` | Replace text in captions with the same options, keeping formatting. Returns `replaced` and `locked_panels_skipped`. |
| `check_storyboard_spelling` | Check caption spelling against the bundled English dictionary and the person's personal dictionary, optionally only some `panels` or one `field`. Returns each misspelt `word` with its panel, field, character `start`/`end`, `suggestions` (up to `suggestions`, default 5) and `locked`. Words in capitals, with digits or of one letter are skipped. Read-only. |
| `list_storyboard_library` | List library items (`scope`: `project`, `personal` or `all`; optional `query` over names and tags): ID, name, tags and kind (`layers`, `panel` or `scene`); project items also give their size and top-level layer names, `animated` when they bring keyframes, comps or camera moves, and a scene item's `panels`. Read-only. |
| `add_to_storyboard_library` | Add a drawing to the `project` library (saved in the `.emu`, the default) or the `personal` library (shared by every storyboard): with `layers`, copies of those layers at their positions; with `scene`, the whole scene with its timing, captions, keyframes, comps and camera; otherwise the whole `panel` (default: the active panel), with its animation when it has any. `name` and optional `tags`. Returns `animated`. |
| `place_storyboard_library_item` | Place an `item` from a `scope`: a layers item goes on top of the active panel at its original position, a panel item becomes a new panel after it, a scene item a new scene after the active panel's scene. One Undo step. |
| `update_storyboard_library_item` | Rename an item and, with `tags`, replace its tags. |
| `remove_storyboard_library_item` | Delete an item (asks for confirmation). Undo restores a project item; a personal item and its file are removed for every storyboard. |
| `list_storyboard_templates` | List installed storyboard templates with their resolution, frame rate and panel count. Read-only. |
| `save_storyboard_template` | Save this storyboard as a template in the personal library (`name`, optional `tags`, `author`, `license`, `description`). Returns the `template` ID. |
| `create_storyboard_from_template` | Workspace tool: open a new tab with an unsaved copy of a template (`template`, `name`), fresh history. |
| `list_storyboard_pdf_profiles` | List the built-in and saved storyboard PDF layout profiles with every option. Read-only. |
| `export_storyboard_pdf` | Write a PDF board to an absolute `.pdf` `path` with a `profile` (built-in or saved name) and optional `options` laid over it: `columns`, `rows`, `paper`, `landscape`, `captions` (`below`, `right`, `left`, `none`), `caption_fields`, panel and page headers with tokens, `logo`, `camera_frame`, `safe_areas` and the rest. `panels` or `scene` limit it; `title` fills `{project}`. Returns the page count. |
| `export_storyboard_images` | Write PNG or JPEG panels into an absolute `directory`, named by `pattern` (tokens such as `{seq}_{scene}_{panel}`, `{index:3}`), optionally one image per visible top-level layer (`per_layer`, `{layer}`). A pattern that names two files alike writes nothing. |
| `export_storyboard_csv` | Write captions (plain text), timing (frames, seconds, timecode) and shot data, one row per panel, to an absolute `.csv` `path`. |
| `import_storyboard_sound` | Import a WAV, MP3, M4A, AAC, FLAC, OGG, Opus or AIFF file (absolute `path`) into the sound library, optionally in a `folder` and with a `name`. The bytes are copied into the project and saved in the `.emu`. Needs FFmpeg. Returns the `sound` ID for `place_storyboard_sound` and its duration. |
| `export_storyboard_movie` | Write the animatic with its transitions and mixed sound to an absolute `path`: `.mp4` (H.264), `.mov` (ProRes 422) or, with `format: "png_sequence"`, a folder of `frame_00000.png`… plus `soundtrack.wav`. Options: `start_frame`/`end_frame`, `width`, `render_area` (`camera`, `overscan`, `all_artwork`), `burn_in` (`timecode`, `scene`, `panel`, `caption`, `position`, `size`), `quality`, `audio`, `reference_video` (`none`, `overlay`, `picture_in_picture`). Needs FFmpeg for movies. |
| `export_storyboard_gif` | Write the animatic as a looping GIF to an absolute `.gif` `path`, sampled at `fps` (default 12) at `width` (default 640), with the same range, render area and burn-in options. |
| `set_storyboard_transitions` | Set how `panels` enter from the panel before: `kind` `cut`, `dissolve`, `wipe` or `slide` (`edge` left, right, top, bottom), `clock`, `iris` or `fade_to_color` (`color`, default black), for `frames`, `seconds` or `timecode` (default half a second). Never longer than the panel. |
| `set_storyboard_timing` | Set many panels' durations at once, each in `frames`, `seconds` or a `timecode` length. Transitions that no longer fit are shortened and listed. |
| `fit_storyboard_timing` | Retime `panels`, `scenes` or `scene_names` to a total `frames`, `seconds` or `timecode`, keeping their proportions. |
| `roll_storyboard_cut` | Move the cut after `panel` by `frames` or `seconds` (negative is earlier); the next panel gives or takes the time. Returns the frames `moved`. |
| `snap_storyboard_cuts` | Move panel cuts onto audio markers within a tolerance (`frames` or `seconds`, default a quarter second). Returns the cuts moved. |
| `add_storyboard_audio_track` | Add an audio track (up to 16) with a `name` and optional `volume_db`. Returns its number. |
| `update_storyboard_audio_track` | Rename a `track`, set `volume_db`, `muted` or `solo`. |
| `delete_storyboard_audio_track` | Delete a track with its clips and markers (asks for confirmation); the sounds stay in the library. |
| `place_storyboard_sound` | Place a library `sound` on a `track` at `at` (frame), `at_timecode`, `at_seconds` or `at_panel`, with `offset_ms`, a length (default the rest of the sound), `gain_db`, `fade_in`, `fade_out` and `name`. Clips on a track cannot overlap. |
| `update_storyboard_audio_clip` | Move (`to_track`, a new start), trim (length, `offset_ms`), rename or change the gain and fades of `clip` on `track`. |
| `delete_storyboard_audio_clips` | Delete `clips` from a `track` by number. |
| `describe_storyboard_clip_effects` | Read a `clip`'s gain `envelope` keys and its `eq` bands (`low`, `mid`, `high`, each with `hz`, a fixed `db` and `keys`), each key with its `frame` from the clip's start, `db` and `easing`; optional `frames` also returns every parameter's value there. Read-only. |
| `set_storyboard_clip_effect_keys` | Add `keys` to one `param` of a clip: `envelope` (−60 to +24 dB, added to the clip's gain) or an EQ band `low`, `mid` or `high` (−24 to +24 dB), each at a `frame` or `seconds` from the clip's start with `db` and `easing` (`linear`, `ease_in`, `ease_out`, `ease_in_out`, `step`). A key on an existing key's frame replaces it. |
| `delete_storyboard_clip_effect_keys` | Delete a `param`'s keys at `frames` (from the clip's start), or all of them. A band without keys plays its fixed gain. |
| `set_storyboard_clip_eq` | Set fixed EQ gains `low_db` (200 Hz shelf), `mid_db` (1 kHz peak) and `high_db` (5 kHz shelf) on a clip; a band given here loses its keys. All 0 turns the EQ off. |
| `add_storyboard_markers` | Add named `markers` to a `track`, each at a frame, timecode, second or panel start. |
| `update_storyboard_marker` | Rename or move a `marker`. |
| `delete_storyboard_markers` | Delete `markers` from a `track` by number. |
| `update_storyboard_sounds` | Rename library `sounds` and set their `folder` (`/`-separated). |
| `remove_storyboard_sounds` | Delete the chosen unused `sounds`, or without it every sound no clip uses (asks for confirmation). |
| `import_storyboard_video` | Import a video file (absolute `path`: `.mp4`, `.mov`, `.m4v`, `.mkv`, `.webm` or `.avi`) as a reference clip at `at`, `at_timecode`, `at_seconds` or `at_panel` (default 0) on video `track` (default the first with room; a new one when none has; at most 4), lasting the whole video. `with_audio` also imports its sound as a library sound and lines a clip of it up on an audio track. Saved in the `.emu` (up to 2 GiB of video). Needs FFmpeg. Returns the clip's `track`, `clip` and the video's size and fps. |
| `update_storyboard_video_clip` | Move (`to_track`, a new start), trim (length, `offset_ms` in point; never past the end of the video), rename, or set `opacity` (0–1), `visible` or `locked` of `clip` on video `track`. A locked clip refuses everything but `locked: false`. |
| `delete_storyboard_video_clips` | Delete `clips` from a video `track` by number (asks for confirmation); a video no clip uses leaves the project. `delete_track` also removes the track. Locked clips refuse. |
| `describe_storyboard_camera` | Read a `scene`'s camera: its `start` and `frames` in the animatic, its playing `panels` with their start within the scene, the `rest` framing, the `keys` (scene frame, seconds, timecode, the panel it falls in, `x`/`y`/`zoom`/`rotation`, `easing` or `curve`) and the `shake`. Read-only. |
| `set_storyboard_camera_keys` | Set camera `keys` on a `scene`, each at a `frame`, `seconds` or `timecode` within the scene (or from a `panel`'s start), with any of `x`, `y` (centre, panel pixels), `zoom` (0.05–20), `rotation` (degrees), `easing` and `curve`. Values left out keep the camera's value there; `replace` replaces every key. |
| `delete_storyboard_camera_keys` | Delete the camera keys at the given times. |
| `reset_storyboard_camera` | Remove a scene's keys and shake. |
| `set_storyboard_static_camera` | Hold the camera still through one `panel` at a framing (values left out keep the framing at its start); the scene's move resumes after it. |
| `set_storyboard_camera_shake` | Shake a scene's camera: a `preset` (`handheld`, `bumpy_ride`, `earthquake`) adjusted by `amplitude`, `rotation`, `frequency` and `seed`, or `remove`. |
| `copy_storyboard_camera` | Copy a camera `from` one scene `to` another, stretched to its length unless `fit` is false. |
| `describe_storyboard_layer_motion` | Read a panel's animated layers (pivot, tracks of keys with frame, seconds, value, easing or curve), the layers' animatable `effects` and the layer comps. Read-only. |
| `set_storyboard_layer_keys` | Set keyframes on a `layer` of a `panel`: `tracks` of `property` (`x`, `y`, `scale_x`, `scale_y`, `rotation`, `skew_x`, `skew_y`, `opacity`, or `effect` with an `effect` parameter key) and `keys` (`frame`, `seconds` or `timecode` within the panel, `value`, `easing`, `curve`); `replace` replaces a track. |
| `delete_storyboard_layer_keys` | Delete keys at times on a `property`, a whole track, or all of a layer's animation. |
| `set_storyboard_layer_pivot` | Set the point a layer turns, scales and skews about (`x`, `y`), or `clear` it. |
| `list_storyboard_layer_comps` | List a panel's layer comps and the layers each hides. Read-only. |
| `capture_storyboard_layer_comp` | Save the panel's hidden layers as a comp `name` (replacing one of that name). |
| `apply_storyboard_layer_comp` | Show and hide the panel's layers as a comp saved them. |
| `rename_storyboard_layer_comp`, `delete_storyboard_layer_comp` | Rename (`new_name`) or delete a comp. |
| `set_storyboard_keyframe_sync` | `mode` `scale` (keys stretch when a panel's duration changes, the default) or `keep`. |

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
`set_transform`, `get_view` and others), and the vector stroke tools
(`add_vector_layer`, `draw_vector_strokes`, `retouch_vector_strokes` and the
rest, see [native vectors](mcp-design-vectors.md#vector-stroke-layers-pencil-lines)).

## From a scenario

If the scenario is a screenplay file, `import_storyboard_script` lays it out
in one call; then time, shoot and draw the panels as below:

```json
{"path":"/Users/me/Scripts/storm.fountain","split":"beat","at_start":true}
```

Otherwise:

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

- **Check spelling.** `check_storyboard_spelling` lists misspelt caption
  words with suggestions; fix each with `replace_in_storyboard_captions`
  (`match_case` and `whole_word`). Names in capitals are not flagged; leave
  invented names alone rather than "correcting" them.

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

Bring layout pages from a PDF or Illustrator file in as panels with
`import_storyboard_pdf` (`{"path":"/Users/me/Layouts/sc12.pdf"}`). Bring a
layered layout into the current panel (or omit `into` for one new panel per
file):

```json
{"paths":["/Users/me/Layouts/sc12.psd"],"into":"layers","panel":4}
```

Drawing uses the shared tools: `paint` with `mirror` or `symmetry`, brushes from
`list_brushes` (`import_brushes` imports `.abr`), `set_blend_mode`, `set_clip`
and `add_mask`.

### Vector line work

Put clean lines that should stay editable (outlines, props, speed lines,
camera arrows) on a vector stroke layer, and keep rough tone and texture on
bitmap layers with `paint`. Each point's `width` multiplies the stroke's
`line_width`, so ramping it gives a pressure-like taper:

```json
{"name":"add_vector_layer","arguments":{"name":"Line"}}
```

```json
{"name":"draw_vector_strokes","arguments":{"node":3,"color":"#2B2B2B","line_width":8,"strokes":[{"points":[{"x":1130,"y":975,"width":0.1},{"x":1310,"y":962,"width":1},{"x":1490,"y":978,"width":0.1}]}]}}
```

After review, `retouch_vector_strokes` thickens, thins, fades or smooths the
line along a path, `edit_vector_strokes` smooths, simplifies, recolours,
rewidths or moves strokes by index (`describe_vector_strokes` lists them), and
`erase_vector_strokes` trims overshoots. Next-frame copies keep the strokes
editable. Locked panels refuse every vector edit.

## The animatic: timing, transitions and sound

Panels play end to end in page order; thumbnail sheets are left out.
`describe_storyboard` gives each playing panel its `start` frame and
`timecode` and, unless it cuts, the `transition` into it. `animatic` holds the
running time (`frames`, `timecode`, `drop_frame`), the audio `tracks` and the
`sounds` in the library (`import_storyboard_sound` adds a sound file to it):

```json
{"frames":240,"timecode":"00:00:10:00","drop_frame":false,"audio_end":96,
 "tracks":[{"track":1,"name":"Dialogue","volume_db":0,"muted":false,"solo":false,"audible":true,
   "clip_count":1,"clips":[{"clip":1,"name":"Mia line","sound":1,"start":24,"start_timecode":"00:00:01:00","frames":72,"seconds":3,"offset_ms":0,"gain_db":0,"fade_in":0,"fade_out":6}],
   "marker_count":1,"markers":[{"marker":1,"name":"Who's there?","frame":40,"timecode":"00:00:01:16"}]}],
 "sound_count":1,"sounds":[{"sound":1,"name":"Mia line","folder":"Dialogue","format":"wav","duration_ms":3000,"frames":72,"clips":1}],
 "audio_from":0,"more":false}
```

Clip, marker and sound lists show 200 items at a time; while `more` is true,
call again with `audio_from` set to the next item. Tracks, clips and markers
are numbered from 1, clips and markers in time order, so a moved clip or
marker can change number (the tools return the new one). A clip's `effects`
is true when it has a gain envelope or EQ; `describe_storyboard_clip_effects`
reads them. Effect keys sit at frames from the clip's start, so moving a clip
takes its effects along. Fade a line down by 12 dB over its second second:

```json
{"track":1,"clip":1,"param":"envelope","keys":[{"seconds":1,"db":0},{"seconds":2,"db":-12,"easing":"ease_out"}]}
```

Sound is recorded from a microphone in the app (the Timeline's **Record**
and the Panel Timer), not through these tools; recordings arrive as library
sounds in the `Recordings` folder.

Durations are given as `frames`, `seconds` or a `timecode` length
(`HH:MM:SS:FF`); positions as `at` (a frame from 0), `at_timecode`,
`at_seconds` or `at_panel` (that panel's first frame). Drop-frame rates
(29.97, 59.94) write and read timecode with `;` before the frames.

Time a scene to its dialogue: put markers on the dialogue track at each line,
then snap nearby cuts onto them, and fit the next scene to a length:

```json
{"track":1,"markers":[{"name":"Who's there?","at_timecode":"00:00:01:16"},{"name":"Door slam","at":96}]}
```

```json
{"frames":6}
```

```json
{"scene_names":["Street"],"seconds":4}
```

A dissolve into a panel, half a second long:

```json
{"panels":[7],"kind":"dissolve","seconds":0.5}
```

Transitions play over the first frames of the panel they enter, so they never
change the running time. Timing and transition edits respect locks: a locked
panel, or a panel in a locked scene, refuses them and nothing changes. Audio
tracks, clips, markers and sounds are not locked by panel locks. Every call is
one Undo step.

### Reference video

A video (live action, a previs or an earlier animatic cut) can sit on its
own video tracks as timing reference. `import_storyboard_video` copies the
file into the project and places it; `describe_storyboard` lists it under
`video`:

```json
{"tracks":[{"track":1,"name":"Video 1","clips":[{"clip":1,"name":"Previs","video":3,"start":0,"start_timecode":"00:00:00:00","frames":240,"seconds":10,"offset_ms":0,"opacity":1,"visible":true,"locked":false}]}],
 "videos":[{"video":3,"name":"Previs","format":"mp4","duration_ms":10000,"fps":24,"width":1920,"height":1080,"has_audio":true}],
 "end":240}
```

Video clips keep their frames when panels are retimed (as audio clips do),
so time panels against them. Where clips overlap on several tracks, the
first track's visible clip shows. The person sees the reference over the
Stage and the player; `export_storyboard_movie` draws it with
`reference_video` (`overlay` fits it over the frame with the clip's opacity,
`picture_in_picture` insets it at the bottom right).

## Animation: cameras, layer keyframes and comps

Each scene has one camera. Its keys are timed from the start of the scene, so
one move can run across several panels: a pan or truck that follows a
character from panel to panel is two or three keys on the scene, not a
camera per panel. `x` and `y` are the centre of the shot in panel pixels and
`zoom` 2 shows half the frame; `describe_storyboard_camera` gives the `rest`
framing (the centre, zoom 1) and where each panel starts within the scene.
Give a key's time as `frame`, `seconds` or `timecode`, from the scene start or
from a `panel`'s start; values left out keep what the camera does there, so a
key can change only the zoom.

A truck right that settles on the third panel, easing in and out:

```json
{"scene":3,"keys":[{"frame":0,"easing":"ease_in_out"},{"panel":12,"frame":0,"x":1400,"zoom":1.3}]}
```

`easing` shapes the move to the next key: `ease_in_out` for natural starts
and stops, `linear` for a constant-speed pan, `step` to hold and cut. A
`curve` (`{"x1":0.2,"y1":0,"x2":0.2,"y2":1}`, like CSS `cubic-bezier`) gives
any other ease. Shake goes on top of the keys, for an impact or a handheld
feel:

```json
{"scene":3,"preset":"earthquake","amplitude":12}
```

Layer keyframes animate a layer within one panel: a character sliding in,
a door swinging open about its hinge (`set_storyboard_layer_pivot`), a fade.
That keeps one drawing in one panel instead of many next-frame panels. Keys
are timed from the panel's start; `x`/`y` are offsets in pixels, scale 1 is
unchanged, rotation and skew are degrees, opacity 0–1 multiplies the layer's
own, and adjustment layers animate their parameters (`property: "effect"`
with a key from `effects` in `describe_storyboard_layer_motion`):

```json
{"panel":12,"layer":40,"tracks":[{"property":"x","keys":[{"frame":0,"value":-300,"easing":"ease_out"},{"seconds":0.75,"value":0}]},{"property":"opacity","keys":[{"frame":0,"value":0},{"frame":6,"value":1}]}]}
```

Layer comps save which layers are hidden, to switch a panel between
alternate looks (day and night, with or without a prop): hide layers with
`set_visibility`, `capture_storyboard_layer_comp`, then
`apply_storyboard_layer_comp` to recall it.

When a panel's duration changes its layer keys and its scene's camera keys
stretch with it; `set_storyboard_keyframe_sync` with `keep` leaves them on
their frames. Every change is one Undo step and invalid input changes
nothing. Locked panels refuse layer keys, pivots and comps; a locked scene
refuses camera changes. Panel items and scene items in the library keep this
animation (see below).

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
recurring set. A panel item keeps the panel's animation: its duration, layer
keyframes, comps and the scene camera's keys over it; placed, the camera keys
join the scene camera over the new panel. Add a whole scene with `scene` (its
ID): its panels, durations, captions, keyframes, comps and camera come back
as a new scene after the active panel's scene, in one Undo step. Frames keep
their time at another frame rate. Project library changes and placing are Undo steps in the live
project; personal library changes and templates are saved on disk at once.
`save_storyboard_template` captures the board's settings, caption fields,
naming, Smart add layers, guides, palette, library and panels;
`create_storyboard_from_template` starts a new storyboard from one.

Exports read the live board and never change it; invalid arguments write
nothing. For a pitch board with three panels a page and captions beside them:

```json
{"path":"/home/me/boards/pitch.pdf","profile":"3 per page · captions right","options":{"caption_fields":["Action","Dialogue"],"page_header":"{project} · {scene}"},"title":"Pitch"}
```

Offsets in `format_storyboard_caption`, `find_in_storyboard_captions` and
`formatting` are Unicode characters, not bytes. Invalid calls change nothing.

The assistant's system prompt includes a storyboarding playbook (shot sizes,
continuity, timing and this workflow); its worked example is executed by the
`storyboard_playbook_runs_against_a_live_storyboard_project` test.
