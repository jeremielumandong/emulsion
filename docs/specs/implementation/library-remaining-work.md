# Library follow-up implementation

Updated 2026-09-28. The following implementations share persisted Develop recipes,
rendering and Library automation. Scope limits below are intentional and must not
be described as full photographic parity with another editor.

- [x] Visible quarter-turn rotation, undo and persistence.
- [x] CMYK soft proofing with bidirectional printer-profile validation.
- [x] Cached full-resolution inspection and cancellable three-color CFA reconstruction.
- [x] Multi-guide perspective solving and its desktop workflow.
- [x] Portable nondestructive content-aware healing.
- [x] Partial-channel RAW highlight reconstruction, disabled for existing recipes.
- [x] Optional sensor-domain Bayer AI denoise using the RawNIND model.
- [x] As-shot dual-illuminant profile interpolation from camera white balance.
- [x] Profile-preserving RGB development and linked-photo wide-gamut export.
- [x] Planar panorama registration, feathered blending, TIFF output and catalog insertion.
- [x] Portable generated depth maps with editable focus, focus range and blur strength.
- [x] Projective HDR registration, color-aware deghosting and larger-image input support.
- [x] Expanded numerical, real-model and real-camera regression checks.

## Current boundaries

Inspection caches one developed frame per source under a shared 256 MiB budget.
The first inspection still develops the whole source; files that exceed the budget
are not cached. The new demosaicer checks cancellation per row. Legacy recipes retain
the previous demosaicer until **Interruptible RAW reconstruction** is enabled.
Unsupported four-color or rotated sensor layouts retain the decoder's legacy path.
RAW decoding and individual ONNX inference calls cannot be interrupted mid-call.

Highlight reconstruction estimates a clipped channel from nearby unclipped color
ratios. It cannot recover a region where every channel is saturated. As-shot profile
interpolation requires a compatible imported matrix profile; it is opt-in for saved
recipes and does not replace a measured camera calibration.

Sensor AI denoise requires the optional **RawNIND Bayer denoise** download in Models.
Its model weights are GPL-3.0 and inference runs locally. The tested model is pinned
by checksum. Bayer phases and tile seams are tested; X-Trans and rotated Fuji sensors
are rejected explicitly by this model. Existing RGB restoration remains a separate tool.

Library RGB inputs can use **ProPhoto working gamut**, retaining embedded RGB ICC
colors before development. Photo links profiled originals and supports nondestructive
exposure/color edits and sRGB, Adobe RGB or ProPhoto export. Wide Photo export currently
supports the linked raster layer with its placement and mask. A multilayer document
uses the existing sRGB compositor; requesting a wide export of such a linked document
returns an explicit error rather than silently discarding the original gamut.

Panoramas require 2–9 opaque, overlapping photos in capture order. They use a planar
projection, robust feature registration, exposure matching and feathered overlaps.
They do not yet provide spherical/cylindrical projections, content-aware seams or
moving-subject removal. Existing Develop settings are applied to panorama inputs.

Depth blur uses relative monocular depth, an editable focus interval and a Gaussian
blurred image. It is not a physical lens simulation and can show halos at depth
boundaries. Depth assets travel with Library backups. Generate a new map after
replacing the original photo.

HDR uses projective registration when textured matches are sufficient and translation
alignment as fallback. Color and luminance disagreement identify motion, with a small
expanded rejection boundary. The 60 MP limit is a safety ceiling, not a promise of
low memory use. A 25.2 MP two-input test measured 4.4 seconds for merge and approximately
1.9 GiB peak process memory on the development machine.

## Validation

Nikon D50 NEF, Fujifilm X-Pro1 RAF and Canon EOS M50 CRAW passed full-resolution
open/edit/save/reopen/export with pixel-identical preview/export and unchanged source
hashes. New/legacy preview RGB RMSE was 0.004795, 0.006155 and 0.000919 respectively.
These comparisons detect regressions; legacy output is not a calibrated ground truth.

On those fixtures, cold inspection measured 0.44, 1.07 and 3.49 seconds; repeated
cached inspection measured 5.7, 20.8 and 29.0 milliseconds. Timings are observations,
not portable performance thresholds.

Additional tests cover all Bayer phases, interruption during reconstruction,
partial-channel recovery, profile temperature interpolation, wide RGB gamut retention,
real denoise-model noise reduction/color/seams, real depth inference, focus editing,
textured overlap registration, panorama save/reopen/no-clobber output, and HDR inputs
larger than 24 MP. Broader camera vendors, calibrated color charts, real handheld
panorama sets and difficult motion/occlusion scenes still need photographic evaluation.
