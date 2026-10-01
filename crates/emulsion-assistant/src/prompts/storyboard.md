
Playbook: storyboarding

Use when the brief is a script, scenario or shot list for film, animation, games or advertising. The deliverable is a sequence of readable panels, not finished illustrations: clarity of staging, continuity and timing come before rendering.

Plan before drawing. Read the scenario and list its scenes (one location and time each) and, inside each scene, its beats: every change of action, emotion, speaker or camera. One beat is usually one panel; a camera move or a large action can need a start and an end panel. Decide a shot for each beat: establish a new location with a wide or extreme-wide shot, move closer for emotion and detail (medium, medium-close, close-up), and use inserts for objects that matter. Vary size and angle with purpose; low angles add power, high angles vulnerability, Dutch angles unease. Keep the 180° line within a scene: characters keep their screen side and look in a consistent direction unless a panel shows the camera crossing.

Build the outline with the storyboard tools before drawing. Create one scene per add_storyboard_panels call, with Slugging such as "INT. KITCHEN - NIGHT" on its first panel, Action describing what happens in present tense, Dialogue as NAME: line, and Notes for camera and sound (PAN LEFT, PUSH IN, SFX: door slam). Give each panel the time its action or line needs: roughly 2–3 seconds for a simple beat, about one second per three words of dialogue plus a beat, shorter for fast action. Check total_seconds against any target length.

Draw rough and readable. On each panel keep separate named layers: Background, then one layer per character or prop, then Effects/Arrows. Draw a clear silhouette and the key pose, simple perspective for the set (a horizon line, a vanishing point), and use value only to separate planes. Movement arrows: a curved arrow on its own layer shows character motion; a framed arrow or a second rectangle shows camera moves.

Continue shots instead of redrawing them. When the next panel keeps the same set-up, duplicate_project_page the previous panel, then translate_node or set_transform the character layers, repaint only the pose that changes, and update its captions with update_storyboard_panel. To bring a character into a different shot, copy_page_nodes from the panel where it was drawn and rescale or repaint it there. Inspect each finished scene with get_view on its panels and describe_storyboard, then fix staging, continuity or timing before moving on.

Rough a sequence on thumbnail sheets first when the staging is still open. set_storyboard_thumbnail_sheet turns a panel into a grid of small camera frames and returns each cell's rectangle; draw one quick thumbnail per cell with the drawing tools, compare the flow on one page, then convert_storyboard_thumbnails turns the sheet into one panel per cell, in order, each scaled to full size with its layers still editable. Add captions and timing to the converted panels afterwards.

Keep the set with Smart add. Put the set or background on a layer named in the Smart add list (set_storyboard_settings smart_add_layers, "Background" by default); smart_add_storyboard_panel then starts the next panel with a copy of that layer and the same shot size, so only the characters need drawing.

Edit the board without losing work. move_storyboard_panels reorders panels and moves them between scenes; start_storyboard_group splits a scene and join_storyboard_group merges it back into the previous one; copy_storyboard_panels repeats panels or whole scenes, and import_storyboard_panels brings scenes in from another storyboard file. Lock panels and scenes the user has approved with set_storyboard_locks so later edits cannot touch them. After inserting, moving or deleting panels, renumber_storyboard renames panels (and scenes, if the board uses numbered scenes) by the naming rules set with set_storyboard_settings naming. When a character is renamed, replace_in_storyboard_captions with match_case for each spelling (Mia, MIA) and whole_word, after checking the matches with find_in_storyboard_captions; it reports locked panels it skipped. Use add_storyboard_caption_field for extra fields (Camera, Sound, VFX), and format_storyboard_caption to bold a character's first appearance or colour a sound cue.

Worked plan: a scene, its next frame and a Smart add panel, a thumbnail sheet for the next scene, then a character rename, a lock and panel renumbering.

```json storyboard
[
  {"name":"create_design_project","arguments":{"kind":"storyboard","name":"Night visit","width":1920,"height":1080,"pages":1}},
  {"name":"add_storyboard_panels","arguments":{"after":1,"start":"scene","group_name":"Kitchen","panels":[
    {"seconds":3,"size":"wide","angle":"eye","captions":{"Slugging":"INT. KITCHEN - NIGHT","Action":"Mia stands at the sink. The back door creaks open behind her."}},
    {"seconds":2,"size":"close_up","captions":{"Action":"Mia freezes.","Dialogue":"MIA: Who's there?"}}
  ]}},
  {"name":"select_project_page","arguments":{"page":2}},
  {"name":"draw_shape","arguments":{"shape":"ellipse","name":"Mia","x":1180,"y":260,"width":260,"height":700,"mode":"shape","style":{"fill":"#5B5F66","stroke":"none"}}},
  {"name":"duplicate_project_page","arguments":{"page":2}},
  {"name":"translate_node","arguments":{"node":2,"dx":-240,"dy":0}},
  {"name":"update_storyboard_panel","arguments":{"panel":4,"seconds":1.5,"captions":{"Action":"Mia turns toward the door."}}},
  {"name":"smart_add_storyboard_panel","arguments":{"after":4}},
  {"name":"update_storyboard_panel","arguments":{"panel":5,"seconds":2,"size":"medium","captions":{"Action":"The door swings shut. Nobody is there."}}},
  {"name":"add_storyboard_panels","arguments":{"after":3,"start":"scene","group_name":"Street","panels":[{}]}},
  {"name":"set_storyboard_thumbnail_sheet","arguments":{"panel":6,"columns":2,"rows":1}},
  {"name":"draw_shape","arguments":{"shape":"rectangle","name":"Car","x":120,"y":520,"width":500,"height":180,"mode":"shape","style":{"fill":"#3A3F47","stroke":"none"}}},
  {"name":"convert_storyboard_thumbnails","arguments":{"panel":6}},
  {"name":"update_storyboard_panel","arguments":{"panel":7,"seconds":3,"size":"wide","captions":{"Slugging":"EXT. STREET - NIGHT","Action":"A car idles under the streetlight."}}},
  {"name":"find_in_storyboard_captions","arguments":{"query":"mia","whole_word":true}},
  {"name":"replace_in_storyboard_captions","arguments":{"query":"Mia","replacement":"Maya","match_case":true,"whole_word":true}},
  {"name":"replace_in_storyboard_captions","arguments":{"query":"MIA","replacement":"MAYA","match_case":true,"whole_word":true}},
  {"name":"format_storyboard_caption","arguments":{"panel":2,"field":"Action","match":"Maya","bold":true}},
  {"name":"set_storyboard_locks","arguments":{"panels":[2],"locked":true}},
  {"name":"renumber_storyboard","arguments":{"scenes":false}},
  {"name":"describe_storyboard","arguments":{}}
]
```
