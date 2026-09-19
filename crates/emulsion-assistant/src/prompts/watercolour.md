
Playbook: watercolour

Select for watercolour/watercolor briefs with luminous paper, transparent washes and economical marks. Map reserved paper before painting. Begin with broad pale washes, build darker pigment in a few selected passages, then add only essential detail. Avoid a universal opaque block-in or ink pass. A large quiet area and a narrow value range may be deliberate.

Layers, bottom to top: Paper (existing background), optional light Sketch, Washes, Pigment, Details. Use Wash for broad translucent areas, Wet blend only when its sampled-colour behaviour is appropriate, Dry brush for broken accents, and Detail round for a few identifying edges. Protect paper with selections or masks; retain editable wash layers. Low-opacity overlays approximate pigment buildup. The presets offer digital marks and mixing, not validated water flow, drying time or pigment transport; don't wait for imaginary drying or claim physical fidelity.

Checkpoints: (1) reserved whites and placement of the first wash; (2) transparent overlap and value hierarchy, inspecting an important edge against the paper; (3) subject recognition and restraint of final detail. Inspect both the whole image and a detail after corrections. Stop when additional detail would consume the light or looseness requested.

Worked mark study: reserve a white rectangular highlight, lay a pale wash, add a darker passage on a new layer, then one small dry accent. A finished picture should use the intended paper silhouette, not this demonstration rectangle. Wetness is zero here to make transparent layering independent of lower-layer colour sampling.

```json
[
  {"name":"add_layer","arguments":{"name":"Washes"}},
  {"name":"select_all","arguments":{}},
  {"name":"select_rect","arguments":{"x":385,"y":245,"width":30,"height":65,"mode":"subtract"}},
  {"name":"paint","arguments":{"node":1,"brush":"Wash","color":"#769CAF","settings":{"size":130,"opacity":0.3,"flow":0.18,"wetness":0},"strokes":[{"d":"M 240 300 C 350 235 465 340 570 275","pressure":[0.8,0.6]}]}},
  {"name":"deselect","arguments":{}},
  {"name":"get_view","arguments":{"max_size":800}},
  {"name":"add_layer","arguments":{"name":"Pigment"}},
  {"name":"paint","arguments":{"node":2,"brush":"Wash","color":"#526C89","settings":{"size":50,"opacity":0.25,"flow":0.2,"wetness":0},"strokes":[{"d":"M 450 320 C 485 325 520 300 550 290","pressure":[0.5,0.2]}]}},
  {"name":"add_layer","arguments":{"name":"Details"}},
  {"name":"paint","arguments":{"node":3,"brush":"Dry brush","color":"#45586B","settings":{"size":8,"opacity":0.6},"strokes":[{"d":"M 465 330 C 478 322 489 328 500 315","pressure":[0.6,0.2]}]}},
  {"name":"get_view","arguments":{"region":[350,220,190,140],"max_size":760}},
  {"name":"critique","arguments":{"context":{"medium":"watercolour","stage":"final restrained accents","composition_intent":"airy wash around reserved paper","user_constraints":["Keep paper highlights unpainted","Preserve low contrast and quiet negative space"]}}}
]
```
