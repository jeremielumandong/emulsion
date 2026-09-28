# Diagram visual and performance review

Reviewed September 28, 2026 against the local `drawio-diagrams` checkout. This is
representative visual inspection, not pixel-perfect certification of all samples.

The reproducible preview tool emits native SVG and a PNG, with `--fit` framing the
artwork. The source SVG files below contain embedded editable draw.io data, so the
browser-rendered source and native import can be compared without recreating the
diagram manually.

| Sample | Findings and changes verified |
| --- | --- |
| `diagrams/flowchart.drawio.svg` | Thin vector outlines, editable text, hollow markers and connector captions. Relative caption positions and perpendicular offsets now survive import and movement. |
| `diagrams/infographic.svg` | Embedded icons, scaled bitmap logo, colored panels and rich labels. Removed unintended white image frames, corrected bitmap placement, and preserved percentage line spacing. Source fonts missing on this machine use substitutes; browser/native wrapping can differ. |
| `diagrams/connect-shapes.svg` | Rich labels and hyperlinks remain editable. Remote illustrations are deliberately not fetched; the importer reports this. Image placeholders no longer add unintended painted frames. |
| `examples/infographic-example-3.drawio` | Parameterized ribbons and cylinders are native vectors. Some caption placement still needs manual adjustment; this is not exact source-layout parity. |
| `templates/business/pert_4.xml` | Editable table cells, spans, borders and wrapped text; attached straight/curved connectors. Browser CSS sizing and cell-border appearance remain approximate. |

All five reviewed imports export without a whole-page raster fallback. Embedded
source bitmaps remain bitmaps. Test coverage separately checks sharp glyph geometry
at 64× zoom, retained SVG subtrees, movement damage, graph bindings, undo and native
persistence.

A local comparison gallery and rendered evidence are available at
[`target/diagram-review/index.html`](../../../target/diagram-review/index.html).
These are generated build artifacts. Regenerate a native preview with:

```sh
cargo run --release --offline -p emulsion-io --example drawio_preview -- \
  /path/to/diagram.drawio.svg /tmp/import.png --fit
```

## Measured performance

Release build, AMD Radeon RX 7700 XT / RADV, 1,000 shapes and 999 connectors.
Final measurements ran after compilation and sample audits completed. Normal desktop
applications remained open. The native window used 1.25 display scaling and a
1,873 × 1,323 physical-pixel canvas.

| Measurement | Median | 95th percentile |
| --- | ---: | ---: |
| Core connected-object move | 7.45 ms | 8.73 ms |
| Display-tree construction | 1.04 ms | 1.30 ms |
| Command through completed GPU frame, without UI/compositor | 11.70 ms | 12.98 ms |
| Native pan: input through canvas submission | 5.88 ms | 6.26 ms |
| Native object drag: input through canvas submission | 20.35 ms | 21.03 ms |
| Native command move: input through canvas submission | 35.71 ms | 40.11 ms |
| Native object drag: input through following platform frame callback | 30.30 ms | 30.41 ms |

The native benchmark remained active for all measured samples. Physical display
latency is excluded. Large connected-object drags still miss a steady 60 fps target;
GPU completion by itself must not be presented as end-to-end frame rate.

The update removes redundant undo copies, indexes display-tree hierarchy once,
retains pointer hit geometry, and skips photo-analysis suggestions for Diagram
projects. That photo analysis had been forcing document-sized vector raster caches
in the background; the final native run peaked at approximately 540 MiB process
resident memory. The exact measurements, adapter and corpus results are recorded
in [the machine-readable audit](diagram-sample-audit.json).
