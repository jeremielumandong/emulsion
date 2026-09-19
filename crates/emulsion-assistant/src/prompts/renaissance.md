
Playbook: renaissance-inspired

Select for Renaissance-inspired composition and layered representational painting. Begin with the intended arrangement (including central, axial or triangular designs), horizon and vanishing directions; organize light and dark masses before colour. Construct figures or objects with consistent perspective. Historical inspiration is not a promise of authentic materials or a single historical technique.

Layers, bottom to top: Ground, Construction, Underpainting, Glazes, Accents. Sketch pencil establishes perspective and forms. Round oil or Flat bristle can build a restrained value underpainting; use low-opacity colour on a separate glaze layer for digital glazing. Use hardness, brush size and selective overpainting to control edges. Avoid automatic blur or an obligatory ink outline. Keep the light direction coherent; choose warm/cool relationships from the brief, not a fixed formula. Do not confuse a digital oil preset or translucent overlay with physically simulated oil glazing.

Checkpoints: (1) composition, perspective and major proportions before colour; (2) readable value underpainting and unified illumination, inspecting anatomy or a vanishing-point junction; (3) glaze colour and selective hard/soft/lost edges. Stop before decorative texture weakens the forms.

Worked mark study: a construction curve, an umber value mass, then a restrained warm glaze. Here low wetness is set to zero intentionally so the marks do not depend on sampling paint underneath. For mixing effects, use only the tool's documented sampling setting and inspect the result.

```json
[
  {"name":"add_layer","arguments":{"name":"Construction"}},
  {"name":"paint","arguments":{"node":1,"brush":"Sketch pencil","color":"#6B5947","settings":{"size":4,"opacity":0.5},"strokes":[{"d":"M 270 390 C 290 220 475 190 520 390","pressure":[0.6,0.4]}]}},
  {"name":"add_layer","arguments":{"name":"Underpainting"}},
  {"name":"paint","arguments":{"node":2,"brush":"Round oil","color":"#756657","settings":{"size":80,"opacity":0.8,"wetness":0},"strokes":[{"d":"M 320 355 C 330 270 435 255 470 350","pressure":[0.9,0.65]}]}},
  {"name":"get_view","arguments":{"max_size":800}},
  {"name":"get_view","arguments":{"region":[260,190,290,220],"max_size":800}},
  {"name":"critique","arguments":{"context":{"medium":"renaissance-inspired","stage":"value underpainting","composition_intent":"balanced central arch","user_constraints":["Keep the central axis","Assess perspective before surface detail"]}}},
  {"name":"add_layer","arguments":{"name":"Glazes"}},
  {"name":"paint","arguments":{"node":3,"brush":"Round oil","color":"#B78752","settings":{"size":60,"opacity":0.18,"flow":0.25,"wetness":0,"hardness":0.3},"strokes":[{"d":"M 330 340 C 345 280 415 270 450 320","pressure":[0.65,0.4]}]}},
  {"name":"get_view","arguments":{"max_size":800}}
]
```
