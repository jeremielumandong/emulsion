Additional medium playbooks

Choose the medium separately from the visual style. These recipes describe available digital techniques, not validated material simulation or proven artistic quality. Combine them with the requested style and the studio's bounded inspection loop. Each study starts in a separate empty 800×600 document; replace its illustrative ids with returned ids. Add or preserve the intended paper/background in real work. Layers are listed bottom to top. The final preview in a study demonstrates one checkpoint; real drawings also need the full composition and relevant detail checks described in the studio rules.

Playbook: graphite

Use for tonal pencil studies and precise construction. Establish proportion with Sketch pencil or Mechanical, group values with 6B side, then place a few firm contour accents. Follow the form with marks; avoid outlining every tonal transition. Layers: Construction, Graphite tone, Accents. Check proportions before shading, separation of light/mid/dark masses before accents, and whether grain obscures the subject at delivery size. Keep intentional pale or unfinished passages.

Worked study: a light arch, broad side-pencil tone, and one sharpened edge.

```json
[
  {"name":"add_layer","arguments":{"name":"Construction"}},
  {"name":"paint","arguments":{"node":1,"brush":"Sketch pencil","color":"#77736E","settings":{"size":3,"opacity":0.4},"strokes":[{"d":"M 250 380 Q 400 160 550 380"}]}},
  {"name":"add_layer","arguments":{"name":"Graphite tone"}},
  {"name":"paint","arguments":{"node":2,"brush":"6B side","color":"#595550","settings":{"size":55,"opacity":0.5},"strokes":[{"d":"M 325 360 Q 420 275 495 360"}]}},
  {"name":"add_layer","arguments":{"name":"Accents"}},
  {"name":"paint","arguments":{"node":3,"brush":"Mechanical","color":"#302F2D","settings":{"size":3},"strokes":[{"d":"M 445 294 Q 505 326 540 374","pressure":[0.7,0.2]}]}},
  {"name":"get_view","arguments":{"region":[240,200,320,210],"max_size":800}}
]
```

Playbook: charcoal

Start with silhouette and large light/dark masses, using Charcoal stick broadly before Conté edges. Reserve light areas; mask or erase only on the intended charcoal layer if lifting is requested. Layers: Gesture, Masses, Selected edges. Check gesture and negative space, then value grouping, then the few edges that carry recognition. Do not make every edge equally dark or smooth away all grain. Broad dusty marks distinguish this workflow from fine graphite construction; physical charcoal lifting is only approximated.

Worked study: gesture, a dark mass, and a broken directional accent.

```json
[
  {"name":"add_layer","arguments":{"name":"Gesture"}},
  {"name":"paint","arguments":{"node":1,"brush":"Charcoal stick","color":"#514B46","settings":{"size":9,"opacity":0.45},"strokes":[{"d":"M 300 180 Q 460 260 360 440"}]}},
  {"name":"add_layer","arguments":{"name":"Masses"}},
  {"name":"paint","arguments":{"node":2,"brush":"Charcoal stick","color":"#292725","settings":{"size":95},"strokes":[{"d":"M 370 250 Q 425 310 390 360"}]}},
  {"name":"add_layer","arguments":{"name":"Selected edges"}},
  {"name":"paint","arguments":{"node":3,"brush":"Conté","color":"#181716","settings":{"size":9},"strokes":[{"d":"M 423 280 L 439 314 M 434 340 L 414 375"}]}},
  {"name":"get_view","arguments":{"max_size":800}}
]
```

Playbook: coloured-pencil

Use Coloured pencil with light pressure and directional overlapping strokes. Build colour by separate sparse passes instead of opaque block-in; increase coverage locally for a burnished-looking accent without claiming wax simulation. Layers: Light colour, Colour buildup, Fine accents. Check construction before saturation, colour relationships after overlap, and the balance of paper gaps against dense accents. Keep stroke direction meaningful and avoid unwanted dark outlines. The requested palette takes priority over a fixed warm/cool recipe.

Worked study: two coloured passes and a short tip accent.

```json
[
  {"name":"add_layer","arguments":{"name":"Light colour"}},
  {"name":"paint","arguments":{"node":1,"brush":"Coloured pencil","color":"#C59B55","settings":{"size":16,"opacity":0.5},"strokes":[{"d":"M 280 350 Q 400 190 520 340","pressure":[0.35,0.55]}]}},
  {"name":"add_layer","arguments":{"name":"Colour buildup"}},
  {"name":"paint","arguments":{"node":2,"brush":"Coloured pencil","color":"#967141","settings":{"size":12,"opacity":0.65},"strokes":[{"d":"M 310 348 Q 405 224 495 340","pressure":[0.5,0.65]}]}},
  {"name":"add_layer","arguments":{"name":"Fine accents"}},
  {"name":"paint","arguments":{"node":3,"brush":"Coloured pencil","color":"#614A31","settings":{"size":4},"strokes":[{"d":"M 475 319 L 499 342","pressure":[0.8,0.2]}]}},
  {"name":"get_view","arguments":{"region":[270,220,270,160],"max_size":800}}
]
```

Playbook: pastel

Group colour masses with Pastel and scumble selected edges with Chalk. Keep separate hue passages legible; excessive mixing can erase the intended colour structure. Layers: Colour masses, Scumble, Accents. Check silhouette and palette, value separation between adjacent masses, then broken edges and accents at native size. For a soft-pastel appearance preserve tooth; for an oil-pastel appearance try denser Coloured pencil/Chalk strokes and inspect the approximation. Use sample_merged only when deliberate lower-layer pickup helps; it is not physical pastel blending.

Worked study: a broad unmixed mass, a pale broken overlay, and a small accent.

```json
[
  {"name":"add_layer","arguments":{"name":"Colour masses"}},
  {"name":"paint","arguments":{"node":1,"brush":"Pastel","color":"#82709C","settings":{"size":110,"wetness":0},"strokes":[{"d":"M 290 330 Q 405 240 515 330"}]}},
  {"name":"add_layer","arguments":{"name":"Scumble"}},
  {"name":"paint","arguments":{"node":2,"brush":"Chalk","color":"#D5B5C5","settings":{"size":50,"opacity":0.55},"strokes":[{"d":"M 320 290 Q 405 245 475 290"}]}},
  {"name":"add_layer","arguments":{"name":"Accents"}},
  {"name":"paint","arguments":{"node":3,"brush":"Conté","color":"#584565","settings":{"size":8},"strokes":[{"d":"M 450 350 L 495 335"}]}},
  {"name":"get_view","arguments":{"max_size":800}}
]
```

Playbook: pen-and-ink

Plan white reserves and contour hierarchy; use Fine liner for even lines, G-pen for expressive weight, and hatch for deliberate line shading. Layers: Construction, Hatching, Ink. Check silhouette and overlaps, line-spacing/value balance, then tangencies and small features. Etching-like density, spare architectural lines and expressive brush ink require different spacing and pressure; choose from the brief. Keep disconnected marks as separate strokes or SVG subpaths. Confine finished hatching with a subject-shaped selection; the rectangle below is only a mark study.

```json
[
  {"name":"add_layer","arguments":{"name":"Construction"}},
  {"name":"paint","arguments":{"node":1,"brush":"Sketch pencil","color":"#A8A6A1","settings":{"size":2,"opacity":0.3},"strokes":[{"d":"M 280 350 L 400 230 L 520 350"}]}},
  {"name":"add_layer","arguments":{"name":"Hatching"}},
  {"name":"hatch","arguments":{"node":2,"brush":"Fine liner","color":"#35332F","settings":{"size":2},"rect":[355,295,85,55],"angle":60,"spacing":9,"jitter":0,"cross":false}},
  {"name":"add_layer","arguments":{"name":"Ink"}},
  {"name":"paint","arguments":{"node":3,"brush":"G-pen","color":"#242321","settings":{"size":5},"strokes":[{"d":"M 280 350 L 400 230 L 520 350","pressure":[0.8,0.3]}]}},
  {"name":"get_view","arguments":{"region":[270,220,260,160],"max_size":800}}
]
```

Playbook: oil

Choose direct opaque painting or layered glazing according to the brief; Renaissance composition is not required. Block values with Round oil/Flat bristle, model selected planes, then place restrained Impasto accents or low-opacity glazes. Layers: Block-in, Modelling, Accents. Check large values before local colour, light/plane consistency before texture, and hard/soft/lost edges before finishing. Keep a clean colour layer when mixing is unwanted; sample_merged explicitly enables lower-layer pickup. Relief and wetness are digital effects, not paint thickness or drying states.

Worked study: a base mass, sampled colour modelling, and a narrow loaded accent.

```json
[
  {"name":"add_layer","arguments":{"name":"Block-in"}},
  {"name":"paint","arguments":{"node":1,"brush":"Round oil","color":"#695C4A","settings":{"size":110,"wetness":0},"strokes":[{"d":"M 300 335 Q 400 245 500 335"}]}},
  {"name":"add_layer","arguments":{"name":"Modelling"}},
  {"name":"paint","arguments":{"node":2,"brush":"Flat bristle","color":"#B49464","sample_merged":true,"settings":{"size":65,"wetness":0.25},"strokes":[{"d":"M 320 315 Q 400 250 460 300"}]}},
  {"name":"add_layer","arguments":{"name":"Accents"}},
  {"name":"paint","arguments":{"node":3,"brush":"Impasto","color":"#D8BF8A","settings":{"size":12,"wetness":0},"strokes":[{"d":"M 363 268 L 403 258","pressure":[0.7,0.2]}]}},
  {"name":"get_view","arguments":{"region":[270,210,270,170],"max_size":800}}
]
```

Playbook: acrylic

Use Acrylic for decisive opaque shapes and layered overpainting; use Dry brush for selective texture. Layers: Base shapes, Overpaint, Details. Check shape placement, then coverage and edge hierarchy, then the amount of surface texture. Set wetness to zero for clean overlays, and do not invent drying delays. For graphic acrylic work prefer crisp masks or paths; for painterly work vary brush size and leave visible strokes. Colour and opacity settings approximate the look without establishing material fidelity.

```json
[
  {"name":"add_layer","arguments":{"name":"Base shapes"}},
  {"name":"paint","arguments":{"node":1,"brush":"Acrylic","color":"#387E89","settings":{"size":110,"wetness":0},"strokes":[{"points":[[300,340,1],[480,340,1]]}]}},
  {"name":"add_layer","arguments":{"name":"Overpaint"}},
  {"name":"paint","arguments":{"node":2,"brush":"Acrylic","color":"#D59255","settings":{"size":70,"wetness":0},"strokes":[{"points":[[370,275,1],[430,350,1]]}]}},
  {"name":"add_layer","arguments":{"name":"Details"}},
  {"name":"paint","arguments":{"node":3,"brush":"Dry brush","color":"#E8C28D","settings":{"size":13,"opacity":0.7},"strokes":[{"points":[[372,270,0.5],[404,307,0.3]]}]}},
  {"name":"get_view","arguments":{"max_size":800}}
]
```

Playbook: gouache

Approximate matte opaque shapes with Acrylic at wetness=0, relief=0 and restrained grain; use Dry brush sparingly for broken coverage. No dedicated gouache engine is implied. Layers: Flat masses, Opaque corrections, Dry accents. Reserve broad simple shapes, then place light-over-dark corrections deliberately. Check silhouette and palette, opaque coverage against the chosen ground, and a few telling edges. Distinguish this from watercolour by using opaque corrections when intended, and from oil by avoiding obligatory gloss/relief or extensive mixing.

```json
[
  {"name":"add_layer","arguments":{"name":"Flat masses"}},
  {"name":"paint","arguments":{"node":1,"brush":"Acrylic","color":"#445D67","settings":{"size":110,"wetness":0,"relief":0,"grain_strength":0.15},"strokes":[{"points":[[290,335,1],[495,335,1]]}]}},
  {"name":"add_layer","arguments":{"name":"Opaque corrections"}},
  {"name":"paint","arguments":{"node":2,"brush":"Acrylic","color":"#DCCCA0","settings":{"size":48,"wetness":0,"relief":0,"grain_strength":0.1},"strokes":[{"d":"M 345 320 Q 390 270 435 320"}]}},
  {"name":"add_layer","arguments":{"name":"Dry accents"}},
  {"name":"paint","arguments":{"node":3,"brush":"Dry brush","color":"#A3B3AC","settings":{"size":10},"strokes":[{"d":"M 450 350 L 485 345"}]}},
  {"name":"get_view","arguments":{"region":[275,255,240,130],"max_size":800}}
]
```

Playbook: digital-painting

Select digital painting when the brief prioritizes shape, lighting and controlled edges without a physical-medium claim. Use Acrylic for blocking, Round oil with wetness/relief/grain disabled for smooth plane transitions, and Fine liner or a small opaque brush for accents only if wanted. Layers: Shapes, Lighting, Accents; keep later adjustments editable. Check silhouette/value grouping, then light direction and perspective or stylized construction, then focal edge control. Avoid obligatory glossy rendering or texture. For airbrushed work use a soft brush setting inside masks and retain structural edges.

```json
[
  {"name":"add_layer","arguments":{"name":"Shapes"}},
  {"name":"paint","arguments":{"node":1,"brush":"Acrylic","color":"#315B79","settings":{"size":125,"wetness":0,"relief":0,"grain_strength":0},"strokes":[{"d":"M 310 350 Q 395 210 490 350"}]}},
  {"name":"add_layer","arguments":{"name":"Lighting"}},
  {"name":"paint","arguments":{"node":2,"brush":"Round oil","color":"#7CC1D4","settings":{"size":75,"hardness":0.15,"opacity":0.5,"wetness":0,"relief":0,"grain_strength":0},"strokes":[{"d":"M 345 300 Q 395 245 425 275"}]}},
  {"name":"add_layer","arguments":{"name":"Accents"}},
  {"name":"paint","arguments":{"node":3,"brush":"Fine liner","color":"#B9E5E3","settings":{"size":4},"strokes":[{"d":"M 365 250 Q 388 231 410 242"}]}},
  {"name":"get_view","arguments":{"max_size":800}}
]
```

Playbook: vector-flat

Use draw_path for editable silhouettes and set_path for revisions. Keep one named path node per independently adjustable shape; add_text keeps lettering editable. Plan limited colour, overlap and negative space before small details. Layers/nodes: Back shape, Front shape, Accent; no raster layer is needed in this example. Check silhouette at small size, spacing/alignment and curve continuity close up, then delivery-size legibility. Flat values or exact symmetry may be the intended outcome. Brushes are optional; do not rasterize paths to satisfy a paint-call quota.

Worked study: three editable shapes and a deliberate contour revision.

```json
[
  {"name":"draw_path","arguments":{"name":"Back shape","d":"M 260 380 L 400 200 L 540 380 Z","stroke":"none","fill":"#345B70"}},
  {"name":"draw_path","arguments":{"name":"Front shape","d":"M 340 380 L 435 260 L 530 380 Z","stroke":"none","fill":"#D8A666"}},
  {"name":"draw_path","arguments":{"name":"Accent","d":"M 385 335 L 430 280 L 475 335 Z","stroke":"none","fill":"#F0D9AA"}},
  {"name":"set_path","arguments":{"node":3,"d":"M 390 335 L 435 280 L 475 335 Z"}},
  {"name":"get_view","arguments":{"max_size":800}}
]
```

Worked shape study: an editable gradient frame and three patterned accents in one path. Create the frame with exact bounds, cut its opening, then resize its geometry with linked proportions. Append accents as separate components so alignment preserves their curves. The preset changes the frame's stroke without replacing its gradient fill. Use ids returned by the tools in a real document; component indices can change after geometry operations.

```json
[
  {"name":"draw_shape","arguments":{"shape":"rectangle","name":"Frame","x":220,"y":160,"width":360,"height":260,"mode":"shape","align_edges":true,"style":{"fill":"#345B70","fill_paint":{"kind":"linear_gradient","end":"#6F8D92","angle":90},"stroke":"#F0D9AA","width":3,"stroke_alignment":"inside","join":"miter","miter_limit":4}}},
  {"name":"combine_path","arguments":{"node":1,"operation":"subtract","d":"M 250 190 L 550 190 L 550 390 L 250 390 Z"}},
  {"name":"resize_path","arguments":{"node":1,"width":324,"linked":true,"align_edges":true}},
  {"name":"apply_shape_stroke_preset","arguments":{"node":1,"name":"dashed","source":"builtin"}},
  {"name":"draw_shape","arguments":{"shape":"ellipse","name":"Accents","x":300,"y":250,"width":36,"height":36,"style":{"fill":"#D8A666","stroke":"none"}}},
  {"name":"combine_path","arguments":{"node":2,"operation":"component","d":"M 370 245 L 406 245 L 406 281 L 370 281 Z"}},
  {"name":"combine_path","arguments":{"node":2,"operation":"component","d":"M 445 258 L 481 258 L 481 294 L 445 294 Z"}},
  {"name":"align_path_components","arguments":{"node":2,"alignment":"center_y"}},
  {"name":"align_path_components","arguments":{"node":2,"alignment":"distribute_x"}},
  {"name":"set_path","arguments":{"node":2,"fill_paint":{"kind":"pattern","end":"#F0D9AA","pattern":"dots","size":8}}},
  {"name":"describe_document","arguments":{}},
  {"name":"get_view","arguments":{"max_size":800}}
]
```

Playbook: pixel-art

Choose a document-pixel grid and a small palette. Block readable clusters before isolated pixels, place integer coordinates, and inspect at native size. Use integer select_rect plus fill_selection for exact raster cells; a hard round brush or antialiased path is not a guarantee of pixel alignment. Layers: Base clusters, Highlights, optionally separate Background. Check silhouette at intended display size, cluster spacing/jaggies at a native-resolution region, then palette consistency. Do not add blur or resample the document to manufacture detail. For a strict low-resolution sprite, establish its actual canvas size first; the study below is a tiny motif inside the shared test canvas.

```json
[
  {"name":"add_layer","arguments":{"name":"Base clusters"}},
  {"name":"select_rect","arguments":{"x":384,"y":284,"width":32,"height":32}},
  {"name":"fill_selection","arguments":{"node":1,"color":"#345B70"}},
  {"name":"add_layer","arguments":{"name":"Highlights"}},
  {"name":"select_rect","arguments":{"x":388,"y":288,"width":8,"height":8}},
  {"name":"fill_selection","arguments":{"node":2,"color":"#B9D7CF"}},
  {"name":"deselect","arguments":{}},
  {"name":"get_view","arguments":{"region":[368,268,64,64],"max_size":64}}
]
```

Playbook: collage-mixed-media

Arrange already available source layers and native cutout shapes before adding marks. Use masks, duplicate_node, move_node and set_transform for supplied pixel assets as appropriate; preserve source layers and respect crop/placement intent. Use draw_path for editable paper-like pieces and Dry brush for optional drawn texture. Layers: Back cutout, Front cutout, Drawn marks, with each supplied asset separately named. Check overlap and focal hierarchy, cutout boundaries/reference fidelity, then whether contrasting materials remain intentional. Do not invent unavailable assets or silently call an image-generation backend. Paper fibres, torn edges and physical adhesion are not simulated by a polygon.

Worked study uses only native cutout shapes; it needs no asset files or backend.

```json
[
  {"name":"draw_path","arguments":{"name":"Back cutout","d":"M 260 250 L 485 220 L 520 365 L 285 385 Z","stroke":"none","fill":"#B28B63"}},
  {"name":"draw_path","arguments":{"name":"Front cutout","d":"M 350 285 L 535 270 L 500 415 L 330 395 Z","stroke":"none","fill":"#6F8D92"}},
  {"name":"add_layer","arguments":{"name":"Drawn marks"}},
  {"name":"paint","arguments":{"node":3,"brush":"Dry brush","color":"#E4D7AF","settings":{"size":18},"strokes":[{"d":"M 375 325 L 455 315 M 370 350 L 448 340"}]}},
  {"name":"get_view","arguments":{"max_size":800}}
]
```

Playbook: printmaking-inspired

Select a branch from the brief: woodcut/linocut uses strong silhouettes and reserved cuts; engraving/etching uses directional Fine liner hatching; screenprint/risograph-inspired work uses separate flat colour shapes and optional inspected texture. Keep plates/colours on separate editable nodes. Check positive/negative shape, line density or colour overlap, then registration and delivery-size clarity. Misregistration is an optional deliberate offset, never a required defect. These are visual approximations, not printable separations, ink chemistry or press simulation. Layers: Main plate, Second plate, Line texture; brushes are optional for flat plates.

Worked two-colour study: separate plate shapes with one restrained hatch passage. Mask finished hatching to the intended plate shape.

```json
[
  {"name":"draw_path","arguments":{"name":"Main plate","d":"M 275 380 L 350 230 L 450 230 L 525 380 Z","stroke":"none","fill":"#394B58"}},
  {"name":"draw_path","arguments":{"name":"Second plate","d":"M 325 360 L 390 245 L 455 360 Z","stroke":"none","fill":"#C67B51"}},
  {"name":"add_layer","arguments":{"name":"Line texture"}},
  {"name":"hatch","arguments":{"node":3,"brush":"Fine liner","color":"#E7D8B5","settings":{"size":2},"rect":[375,300,40,40],"angle":65,"spacing":8,"jitter":0,"cross":false}},
  {"name":"get_view","arguments":{"region":[260,215,280,180],"max_size":800}}
]
```
