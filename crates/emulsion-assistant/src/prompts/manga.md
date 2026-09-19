
Playbook: manga

Select for expressive line drawing, manga panels and ink/screentone briefs. Plan readable gesture and silhouette, then construct the head, torso and limbs as simple volumes with the requested exaggeration. Preserve character proportions from the reference instead of imposing a fixed head-count rule.

Layers, bottom to top: Paper (existing background), Sketch, Tone, Ink; optional separate Blacks or Lettering. Use Blue pencil/Sketch pencil for gesture, G-pen for expressive contours, Maru pen for small features, and Screentone 20%/40% for deliberate value areas. Keep tone below ink; hide the sketch only after the construction is accepted. Use broad black masses to organize the composition before adding small hatching. Line weight follows overlap, emphasis and lighting; uniform graphic weight is valid when requested.

Checkpoints: (1) gesture, silhouette and expressive construction; (2) ink hierarchy, black/white balance and subject fidelity, inspecting faces/hands; (3) restrained screentone, edge clarity and legibility at delivery size. Centred portraits and symmetric emblems need no automatic rebalance.

Worked mark study: a gesture followed by light tone and a tapering contour. The tone is a small rectangle for demonstration; use a subject-shaped selection for finished work. Create the layers in this order so ink remains above tone.

```json
[
  {"name":"add_layer","arguments":{"name":"Sketch"}},
  {"name":"paint","arguments":{"node":1,"brush":"Blue pencil","color":"#A4C8FF","settings":{"size":5,"opacity":0.6},"strokes":[{"d":"M 350 130 C 300 220 470 310 420 440","pressure":[0.5,0.3]}]}},
  {"name":"get_view","arguments":{"max_size":800}},
  {"name":"add_layer","arguments":{"name":"Tone"}},
  {"name":"select_rect","arguments":{"x":360,"y":245,"width":90,"height":80}},
  {"name":"paint","arguments":{"node":2,"brush":"Screentone 20%","color":"#000000","settings":{"size":100},"strokes":[{"points":[[370,285,1],[440,285,1]]}]}},
  {"name":"deselect","arguments":{}},
  {"name":"add_layer","arguments":{"name":"Ink"}},
  {"name":"paint","arguments":{"node":3,"brush":"G-pen","color":"#17151B","settings":{"size":7,"taper_start":10,"taper_end":24},"strokes":[{"d":"M 350 140 C 320 220 430 240 425 330","pressure":[0.8,0.25]}]}},
  {"name":"get_view","arguments":{"region":[290,120,180,230],"max_size":700}},
  {"name":"critique","arguments":{"context":{"medium":"manga","stage":"ink and tone","composition_intent":"centred figure with expressive gesture","user_constraints":["Preserve stylized proportions","Keep the background mostly white"]}}}
]
```
