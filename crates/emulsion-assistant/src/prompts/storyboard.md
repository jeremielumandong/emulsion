
Playbook: storyboarding

Use when the brief is a script, scenario or shot list for film, animation, games or advertising. The deliverable is a sequence of readable panels, not finished illustrations: clarity of staging, continuity and timing come before rendering.

Plan before drawing. Read the scenario and list its scenes (one location and time each) and, inside each scene, its beats: every change of action, emotion, speaker or camera. One beat is usually one panel; a camera move or a large action can need a start and an end panel. Decide a shot for each beat: establish a new location with a wide or extreme-wide shot, move closer for emotion and detail (medium, medium-close, close-up), and use inserts for objects that matter. Vary size and angle with purpose; low angles add power, high angles vulnerability, Dutch angles unease. Keep the 180° line within a scene: characters keep their screen side and look in a consistent direction unless a panel shows the camera crossing.

Build the outline with the storyboard tools before drawing. Create one scene per add_storyboard_panels call, with Slugging such as "INT. KITCHEN - NIGHT" on its first panel, Action describing what happens in present tense, Dialogue as NAME: line, and Notes for camera and sound (PAN LEFT, PUSH IN, SFX: door slam). Give each panel the time its action or line needs: roughly 2–3 seconds for a simple beat, about one second per three words of dialogue plus a beat, shorter for fast action. Check total_seconds against any target length.

Draw rough and readable. On each panel keep separate named layers: Background, then one layer per character or prop, then Effects/Arrows. Draw a clear silhouette and the key pose, simple perspective for the set (a horizon line, a vanishing point), and use value only to separate planes. Movement arrows: a curved arrow on its own layer shows character motion; a framed arrow or a second rectangle shows camera moves.

Continue shots instead of redrawing them. When the next panel keeps the same set-up, duplicate_project_page the previous panel, then translate_node or set_transform the character layers, repaint only the pose that changes, and update its captions with update_storyboard_panel. To bring a character into a different shot, copy_page_nodes from the panel where it was drawn and rescale or repaint it there. Inspect each finished scene with get_view on its panels and describe_storyboard, then fix staging, continuity or timing before moving on.

Worked plan: a two-panel scene, then the next frame.

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
  {"name":"describe_storyboard","arguments":{}}
]
```
