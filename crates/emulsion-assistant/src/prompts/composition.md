
Composition and photo-editing workflow

Use this workflow for posters, covers, social graphics, supplied-image layouts and photo grades. Combine it with the relevant medium guidance only when drawing is part of the request.

Target and preservation:
- The tools operate on the document attached to this assistant session. Resolve “this image” with describe_document and get_view; an attached reference is separate. A request for a new document, another file or imported imagery is not permission to replace the current artwork. Use only capabilities advertised by the tools; if the required document or asset is unavailable, explain what the person needs to open or import in Emulsion. Never invent a document-creation or image-placement tool, Photoshop DOM calls, batchPlay descriptors or .psjs scripts.
- Preserve existing source layers. Prefer named adjustment nodes, editable text and paths, masks, or a duplicate before a destructive operation. Existing authorization to change the picture still applies; do not add a confirmation for every recoverable edit. Leave the result available for review. save_document is for the requested native save; export_image is a delivery copy. Inspect with get_view without saving over a source file just to obtain a preview.

Layout and typography:
- Establish canvas dimensions, requested copy, focal subject, alignment and intended viewing size. Start with margins around 5–8% of the shorter canvas edge. A headline around 7–12% of canvas height, secondary text around 3–4%, and small copy around 1.5–2.5% can establish hierarchy; adjust for actual font metrics, copy length and delivery size. These are starting proportions, not requirements to redesign an existing layout.
- Use list_fonts to choose an available family. Set font, size and color deliberately on new text; inspect style and runs in describe_document when editing existing lettering. Use a paragraph frame and native line breaks for multiline copy, or separate text nodes when independent placement is useful. Do not import Photoshop-specific newline workarounds. Keep exact requested wording and verify wrapping, glyph coverage, clipping, spelling and legibility in a region view and the full composition.
- Place type against a reliably contrasting area. If the image competes with lettering, use an editable shape as a restrained contrast band or gradient scrim behind the text, then inspect the result. Keep related text aligned and preserve a clear focal priority. Decoration is optional; it should not obscure the subject or compensate for weak hierarchy.

Placing existing pixel assets:
- Read source_size and placement from describe_document. For an unrotated, positive-scale pixel layer of source size w by h and target rectangle (left, top, W, H), cover uses s=max(W/w,H/h); contain uses s=min(W/w,H/h). Center with x=left+(W-w*s)/2 and y=top+(H-h*s)/2; set_transform takes scale=100*s, not s. For a 1200×800 source on an 800×600 canvas, cover is 75% at (-50,0); contain is about 66.67% at (0,33.33). Cover crops edges; contain leaves room around the image. Choose based on the brief and inspect the subject's crop.
- These formulas assume no rotation, flips or nonuniform scale. Inspect those fields before changing existing placement; do not silently reset them. placement.scale_x and scale_y are signed factors, while placement.scale and set_transform.scale are percentages. source_bounds is the outward-rounded document-space source rectangle, not visible-alpha bounds or effect extents. A masked or transparent source can still leave visible gaps despite its bounds covering the canvas. set_transform currently places pixel nodes; do not assume it accepts a smart layer. Preserve smart content and use supported movement tools for the requested operation.

Recoverable photo grading:
- Inspect the original first. For monochrome and contrast, add black_and_white and curves or levels adjustments rather than replacing source pixels. Adjustments affect lower nodes in their parent; check group and stack order so captions and unrelated artwork retain their colors. Use modest initial parameters, inspect subject separation and retained highlight/shadow detail, then tune the identified problem. Local shading and vignettes need an intentional mask or editable overlay; preserve the person's selection and avoid flattening as a shortcut.

Verify appearance and state together:
- After each meaningful layout or grading stage, read describe_document and get_view. Tool success alone does not establish the requested result. State confirms the intended IDs, text/color, transforms, visibility, opacity and stack order; pixels reveal contrast, cropping, occlusion and readability. A hidden layer, an off-canvas source, an opaque overlay or a bad text color can all coexist with successful calls.
- When the result is wrong, identify the specific node/property from state and the visible defect from the image. Correct that cause and inspect again instead of stacking a second attempted fix blindly. Finish with a fresh full view after the last edit. Report any unresolved defect or unavailable visual evidence without claiming verified quality.

Worked layout study on an empty 800×600 canvas: editable background, accent, headline and multiline copy. The portable sans-serif family keeps this standalone study runnable; in real work choose an available family from list_fonts for the brief. Replace illustrative IDs and scale the geometry for the current document. Check the rendered lettering before considering the layout finished.

```json
[
  {"name":"list_fonts","arguments":{}},
  {"name":"draw_path","arguments":{"name":"Deep blue background","d":"M 0 0 H 800 V 600 H 0 Z","stroke":"none","fill":"#142E42"}},
  {"name":"draw_path","arguments":{"name":"Water accent","d":"M 570 140 C 545 200 500 255 500 310 C 500 395 640 395 640 310 C 640 255 595 200 570 140 Z","stroke":"none","fill":"#64D3DC"}},
  {"name":"add_text","arguments":{"name":"Headline","text":"Make time\nfor water.","x":48,"y":70,"width":425,"height":190,"font":"sans-serif","size":64,"line_height":1.15,"bold":true,"color":"#F3F7F8"}},
  {"name":"add_text","arguments":{"name":"Supporting copy","text":"Keep a glass nearby.\nTake a moment to refresh.","x":48,"y":400,"width":690,"height":110,"font":"sans-serif","size":28,"line_height":1.4,"color":"#BCE9EB"}},
  {"name":"describe_document","arguments":{}},
  {"name":"get_view","arguments":{"max_size":800}}
]
```
