# Modal sizing and Lucidchart VSDX import fixes

## Multiline dialogs

The Mermaid/CSV/SQL/text source modal is now 960 logical pixels wide and its
editor takes half the window height, bounded to 240–520 pixels. The shared dialog
already clamps to the window and scrolls its body while retaining the footer.
Explicit editor heights also fix collapsed fields in UML/ERD, comments, speaker
notes, bulk CSV creation and chart CSV editing. Object notes, alternative text,
labels and JSON data now use multiline editors. Short numeric/name/settings
forms retain their compact layouts.

A native UI regression pastes a 40-connection Mermaid graph, verifies a large
source area, resizes to 640×480 and applies all 41 nodes/40 edges successfully.
The 21 diagram UI/project tests pass. The broader `design_` UI run passed 59 tests,
including chart CSV, bulk creation and presentation checks. Two gallery tests
failed outside the modified dialogs: a stale expected element count (10 vs 12),
and a missing template tile ID 100. These failures remain recorded rather than
reported as a green full UI suite.

## Lucidchart VSDX

Reviewed `/home/arkane/Downloads/Value stream map example.vsdx` against the
provided PNG. Fixed:

- Group labels were covered by child artwork: labels now paint above children.
- Text-only shapes incorrectly received filled fallback rectangles.
- Geometry sections now retain independent NoFill/NoLine/NoShow settings.
- Text frames use ShapeSheet positions, dimensions, rotation and vertical alignment.
- Character runs, paragraph alignment/spacing and bullets are retained as editable text.
- Missing fonts use an explicit recorded substitute instead of an arbitrary fallback.
- Embedded bitmap relationships from masters and pages retain placement and clipping.
- Unbound connectors retain native arrowheads as well as line geometry.

Sixteen interchange tests pass, including geometry visibility, mixed text,
label stacking, movement, native persistence and embedded master bitmaps.
The actual VSDX saves and reopens with identical vector content and image
placements; image pixels allow one 8-bit level for native 16-bit storage rounding.
Every imported root object also passed movement/undo against the original document.
SVG export keeps native vectors/text and embedded source PNGs without whole-page
raster fallback.

Proofs: `target/lucid-visio-review/index.html`.

## Remaining source/format differences

The VSDX embeds different resource-card bitmaps from the provided PNG. The importer
uses the actual embedded images. The VSDX page includes blank space outside the
artwork; original page dimensions are preserved. Preview PNGs fit the artwork.
Arial is unavailable on this host and uses Liberation Sans. Some source lines are
not endpoint-glued; they remain editable paths and require reconnection for
automatic routing. Theme-dependent colors without evaluated RGB values and
non-endpoint glue constraints remain reported import notes.

## Delivery

Optimized build, packaged version smoke test and installation succeeded.
Installed `/home/arkane/Applications/Emulsion.AppImage`; running documents were left open. Restart to use this update.

SHA-256: `ca0cd5113c3346dbc6f646214608be7fe7b00a806d39070fefba51f616ded226`.

Previous installation: `/home/arkane/Applications/Emulsion.AppImage.bak-modal-visio-20260928T160137864716Z`.

Corrected editable project: `/home/arkane/Downloads/Value stream map example - corrected.emu`.
