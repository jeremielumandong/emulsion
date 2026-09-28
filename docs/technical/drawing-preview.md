# Drawing preview and tile boundaries

Pointer samples retain pressure, tilt and timing, while changed brush tiles are
published once per editor render rather than on every mouse-move event. Pointer-up
flushes remaining samples even when finishing adds no taper or stabilizer dabs.
Cancel discards pending samples; previews remain inside one undo transaction.
This reduces repeated composition and document updates, but CPU dab generation
still happens for each input event. No end-to-end speedup has been measured yet.

GPUI's WGPU atlas packs images without padding. Linear filtering near an image
edge could therefore sample an unrelated atlas image, producing light or dark
tile seams. The polychrome fragment shader now clamps coordinates to that image's
first and last texel centers. This protects all canvas media and backgrounds.
It is edge replication, not interpolation using neighboring document tiles.

## Open-source references

- [Krita texture tiles](https://github.com/KDE/krita/blob/master/libs/ui/opengl/kis_texture_tile.cpp)
  use texture borders, clamp-to-edge sampling and partial texture uploads.
- [MyPaint canvas rendering](https://github.com/mypaint/mypaint/blob/master/gui/tileddrawwidget.py)
  composites exposed tiles into a contiguous pixbuf and expands transformed
  render regions for interpolation to avoid dark stripes.
- [GIMP paint scheduling](https://github.com/GNOME/gimp/blob/master/app/tools/gimppainttool-paint.c)
  queues paint work, schedules display flushes at 10 ms intervals and drains work
  at stroke end.

These are architectural references; no upstream implementation was copied.
Future work can add document-neighbor gutters and reusable partial texture
uploads. That requires changes beyond preventing unrelated atlas-image bleed.

## Regression coverage

The UI regression checks several input samples produce one preview, the final
samples survive pointer-up, undo restores the raster and cancel suppresses pending
samples. The GPU regression renders the production GPUI shader using white/black
atlas neighbors at several fractional and enlarged tile scales.

Validation: 11 tool lifecycle tests and 5 viewport tests passed. The production
shader test passed with a required software WGPU adapter. Replacing only the
clamp with the original sampling expression in a temporary harness reproduced
the defect: a white corner became RGB 235 instead of 255 at a 277-pixel tile
width. The fixed shader passed both background contrasts at all tested sizes.
Interactive latency and the running application's visual result remain unmeasured.

## Wet-media pickup latency

Oil and smudge sampled the stroke-start composite by calling `region(1×1)` for
every pickup tap. Each call rendered an entire 256×256 tile. A stroke now keeps a
bounded 16-tile (16 MiB) `PixelSampler` for that immutable snapshot; repeated taps
reuse the same exact pixels. Transformed coordinates and canvas bounds retain
their previous behavior. The cache ends with the stroke.

Same-executable CPU measurements on a 1920×1080 white canvas (development profile,
opt-level 1, 30 points) reduced mean point processing from 24.76 to 1.08 ms for
Flat bristle, 28.38 to 0.68 ms for Round oil, and 35.24 to 1.40 ms for Impasto.
These are input-processing measurements, not whole-window latency.

The default 120 px Airbrush averaged 2.02 ms for point processing plus preview,
and 7.07 ms for dirty-tile CPU composition and BGRA conversion over 180 points.
This excludes GPUI rendering/upload and does not establish the cause of a reported
one-second dry-airbrush delay. Reproduce the CPU measurements with:

```sh
cargo run -p emulsion-raster --example wet_pickup_latency
cargo run -p emulsion-raster --example airbrush_latency
```

For live diagnosis, start a rebuilt app with
`RUST_LOG=info,emulsion_ui::paint_timing=debug`. The logs separate brush-input
processing, preview publication and background viewport tile-batch duration.
