
Playbook: manga

Select for manga characters, panels and ink/screentone briefs. Match the user's reference, intended age, mood and degree of exaggeration. Manga is not a single face formula: do not automatically substitute childlike proportions, oversized eyes, spiky hair or speed lines. For colour manga, use intentional flat colour shapes and cel shadows when requested.

Construction before decoration:
- Establish the action line, ribcage and pelvis tilt, supporting foot and silhouette before costume details. Build foreshortened limbs as overlapping volumes. Preserve the requested stylization, including chibi proportions; avoid imposing a universal head count.
- Construct the cranium and jaw, then a curved centre line and eye line that follow the head's turn and tilt. Place nose, mouth, ear and neck on that same volume. In three-quarter view the far eye is usually narrower and nearer the facial edge; do not paste a front-facing pair onto a turned head. Preserve intentional symmetry in a front view.
- Draw the eyelid opening first, with the upper lid overlapping the iris. Keep both pupils looking in a coherent direction and highlights consistent with the light. Give the upper lid more emphasis than the lower lid unless the reference says otherwise. Express emotion through the brows, lid angles and mouth together, not through extra eye detail alone.
- Establish the scalp envelope and hair part, then a few large connected locks and a clear black/white pattern. Vary their size and curvature; avoid repeated zigzags and evenly spaced parallel strands. Construct hands from palm, thumb and finger groups with a believable wrist and grip before nails or creases.

Organize editable work into named Construction, Tone, Blacks, Contours and Features nodes. Place each tone below its corresponding ink. Hide construction only after checking the finished shapes. Use draw_path for silhouettes, closed black masses, eyelids and hair locks that may need set_path corrections. A path stroke has constant width: use a closed filled wedge for an editable taper, or pressure-sensitive paint for expressive raster ink. Do not try to repair a raster stroke with set_path.

Inking choices:
- Blue pencil/Sketch pencil for construction; G-pen for contour emphasis; Maru pen for features. At an 800×600 portrait scale, start around 3–5 px for major contours, 1–2 px for facial detail and 5–8 px for selected heavy overlaps. Scale these with the head size, not just the canvas. Small eyelid and mouth strokes need short tapers (roughly 1–4 px); a long default taper can consume the entire feature.
- Weight selected overlaps and shadow edges. Preserve light-facing breaks and quiet interior detail. Use confident connected contours instead of scratchy retracing for a clean-ink brief. Place clothing folds at tension and compression points; avoid filling every surface with lines.
- Plan solid blacks and white reserves before tone. Use a subject-shaped selection for each tone region: draw_path with stroke/fill "none" can define the boundary, then path_to_selection, paint, deselect. Selections are snapshots; rebuild them after changing their source path. Deselect before unrelated ink.
- Screentone 20%/40%/60% mean covered ink area. Choose a consistent grain_scale in layer pixels and judge the dots at delivery size. Keep tone subordinate to the expression and silhouette; do not cover the whole face with dots or stack different pitches accidentally. Hatching follows the form. Do not blur final ink to disguise broken anatomy or crowded marks.

Review checkpoints: (1) gesture, head turn, feature placement and hand construction; (2) expression, lid/iris overlaps, hair masses and ink hierarchy; (3) confined tone, black/white balance and legibility at delivery size. Inspect both a face/hand crop and the full composition. Correct the most consequential visible mismatch before adding decoration, within the studio review budget. Do not automatically rebalance a centred portrait or remove intentional symmetry.

Worked construction-to-ink study: a compact three-quarter head with editable face, hair and eye shapes, a shaped neck tone, and fine raster features. This demonstrates relationships between tools, not a template to repeat for every character. Here node IDs assume an empty document; in real work use the IDs returned by each call. The blue face boundary is deliberately converted to final ink after inspecting the features. Add an appropriate paper background when needed.

```json
[
  {"name":"add_layer","arguments":{"name":"Construction"}},
  {"name":"paint","arguments":{"node":1,"brush":"Blue pencil","color":"#A4C8FF","settings":{"size":2,"opacity":0.4},"strokes":[{"d":"M 307 235 C 296 102 493 100 493 231 C 492 301 462 351 426 379 C 378 365 331 323 307 235 Z"},{"d":"M 424 140 C 442 212 444 297 426 379 M 308 252 Q 405 230 491 250 M 303 431 Q 414 365 532 431"}]}},
  {"name":"get_view","arguments":{"max_size":800}},
  {"name":"draw_path","arguments":{"name":"Coat black mass","d":"M 394 395 L 361 405 Q 323 413 285 454 L 269 493 L 558 493 L 538 448 Q 503 412 468 397 L 429 430 Z","stroke":"none","fill":"#17151B"}},
  {"name":"draw_path","arguments":{"name":"Neck","d":"M 391 345 L 455 343 L 463 394 L 482 407 L 430 452 L 372 414 L 395 397 Z","stroke":"#17151B","width":2.5,"fill":"#FFFFFF"}},
  {"name":"draw_path","arguments":{"name":"Hair silhouette","d":"M 292 288 C 269 240 275 170 313 136 L 304 114 L 344 122 C 385 93 448 99 484 133 L 512 126 L 500 153 C 533 188 528 244 513 281 L 526 314 L 491 303 L 468 267 L 315 307 Z","stroke":"none","fill":"#17151B"}},
  {"name":"draw_path","arguments":{"name":"Face contour","d":"M 306 234 C 304 182 335 141 394 138 C 456 136 490 175 492 226 C 495 276 480 337 426 379 C 396 377 345 342 322 297 Z","stroke":"#85A8D8","width":2,"fill":"#FFFFFF"}},
  {"name":"draw_path","arguments":{"name":"Near ear","d":"M 314 270 C 284 250 282 291 298 311 Q 308 323 320 309","stroke":"#17151B","width":2.5,"fill":"#FFFFFF"}},
  {"name":"draw_path","arguments":{"name":"Neck shadow boundary","d":"M 396 369 Q 426 395 455 359 L 460 391 Q 433 418 397 402 Z","stroke":"none","fill":"none"}},
  {"name":"add_layer","arguments":{"name":"Neck tone"}},
  {"name":"path_to_selection","arguments":{"node":7}},
  {"name":"paint","arguments":{"node":8,"brush":"Screentone 20%","color":"#17151B","settings":{"size":100,"grain_scale":6},"strokes":[{"points":[[409,377,1],[448,385,1]]}]}},
  {"name":"deselect","arguments":{}},
  {"name":"draw_path","arguments":{"name":"Fringe locks","d":"M 309 195 C 344 156 392 141 436 164 Q 465 155 492 190 L 486 247 L 469 220 L 445 248 L 446 205 L 418 244 L 414 198 L 385 251 L 389 201 L 351 261 L 359 213 L 323 247 Z","stroke":"none","fill":"#17151B"}},
  {"name":"draw_path","arguments":{"name":"Hair light reserve","d":"M 321 164 C 354 133 416 119 455 145 C 404 131 361 148 336 177 Z","stroke":"none","fill":"#FFFFFF"}},
  {"name":"draw_path","arguments":{"name":"Near upper lid","d":"M 338 268 C 354 246 390 247 413 264 L 409 269 C 385 258 357 258 341 273 Z","stroke":"none","fill":"#17151B"}},
  {"name":"draw_path","arguments":{"name":"Far upper lid","d":"M 440 260 C 451 250 468 250 481 258 L 478 264 C 464 257 452 258 442 265 Z","stroke":"none","fill":"#17151B"}},
  {"name":"draw_path","arguments":{"name":"Near iris","d":"M 367 261 Q 383 257 393 266 L 392 280 Q 383 292 372 282 Z","stroke":"none","fill":"#17151B"}},
  {"name":"draw_path","arguments":{"name":"Far iris","d":"M 453 260 Q 463 257 469 262 L 468 275 Q 461 283 455 275 Z","stroke":"none","fill":"#17151B"}},
  {"name":"draw_path","arguments":{"name":"Eye light reserves","d":"M 372 265 Q 376 261 380 265 Q 382 270 377 272 Q 372 273 372 265 Z M 456 263 Q 459 260 462 263 Q 464 267 460 268 Q 456 268 456 263 Z","stroke":"none","fill":"#FFFFFF"}},
  {"name":"add_layer","arguments":{"name":"Fine ink"}},
  {"name":"paint","arguments":{"node":16,"brush":"Maru pen","color":"#17151B","settings":{"size":2,"taper_start":1,"taper_end":3,"wetness":0},"strokes":[{"d":"M 340 239 Q 371 227 404 241","pressure":[0.85,0.35]},{"d":"M 442 238 Q 460 231 477 239","pressure":[0.7,0.25]},{"d":"M 346 281 Q 367 294 397 284","pressure":[0.5,0.2]},{"d":"M 447 277 Q 461 284 475 275","pressure":[0.4,0.15]},{"d":"M 435 270 Q 432 287 442 299 L 450 301 L 438 305","pressure":[0.4,0.2]},{"d":"M 402 326 Q 425 334 447 321","pressure":[0.65,0.2]},{"d":"M 416 342 Q 426 345 434 339","pressure":[0.3,0.1]},{"d":"M 300 278 Q 310 274 312 289 L 302 291 Q 299 301 311 304","pressure":[0.5,0.2]}]}},
  {"name":"get_view","arguments":{"region":[276,126,245,262],"max_size":800}},
  {"name":"set_path","arguments":{"node":5,"stroke":"#17151B","width":2.5}},
  {"name":"paint","arguments":{"node":16,"brush":"G-pen","color":"#17151B","settings":{"size":4,"taper_start":2,"taper_end":5},"strokes":[{"d":"M 321 298 Q 350 352 400 372","pressure":[0.85,0.25]},{"d":"M 455 360 Q 457 381 459 391","pressure":[0.75,0.3]},{"d":"M 391 410 L 427 439 L 465 410","pressure":[0.75,0.35]}]}},
  {"name":"set_visibility","arguments":{"node":1,"visible":false}},
  {"name":"get_view","arguments":{"region":[276,126,245,326],"max_size":800}},
  {"name":"get_view","arguments":{"max_size":800}},
  {"name":"critique","arguments":{"context":{"medium":"manga","style":"clean black-and-white character ink with restrained dot tone","stage":"final ink","composition_intent":"quiet three-quarter head study","user_constraints":["Preserve the head turn and coherent gaze","Check lid and iris overlaps before adding details","Keep neck tone confined to its shadow boundary","Preserve white space"]}}}
]
```
