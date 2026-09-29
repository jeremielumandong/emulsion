# Diagram export quality

The supplied AstroTag PDF was produced by ImageMagick and embedded one 1600×1000 image at 72 PPI. The original PNG was also 1600×1000. Single-document PDF export previously flattened the composite and handed the pixels to ImageMagick even though the project exporter already supported vector PDF.

PDF now uses that page-aware SVG/PDF export implementation. Native text becomes font-resolved outlines, and paths stay vector. This also handles AI-generated native artwork without diagram metadata. Native PDF availability no longer depends on ImageMagick. Effects unsupported by the page exporter retain its documented raster fallback; embedded images are not magically converted into vectors.

PNG/JPEG/TIFF/WebP workflows offer double and quadruple sizes. Diagram scenes and enlarged native vector artwork are rendered at output size through the SVG renderer. Ordinary photographic output retains its established workflow. The diagram chooser defaults to 2× once per editor, preserving subsequent size choices. A 64-megapixel output guard limits allocations. Existing color-profile, bit-depth and DPI output controls remain available; DPI alone does not increase resolution.

Regression tests check vector PDF output has no full-page image for native artwork, 2×/4× PNG dimensions, unchanged source data, and the existing output-profile and RAW workflow behavior. `diagram_reexport` is an example CLI for reproducing saved-project exports without changing the source file.

AstroTag verification: `/home/arkane/Documents/AstroTag.emu` is the newer 1600×1000, 231-node diagram matching the reported exports; `/home/arkane/AstroTag.emu` is an older 1600×900 project. Corrected copies were generated from the newer file as `AstroTag-vector.pdf` and `AstroTagArchitecture-4x.png`. `pdfimages -list` reports no images in the corrected PDF; the PNG is 6400×4000. Both the PNG and an independently rendered PDF preview were visually inspected. Original project files and exports were preserved.

Validation: five export-workflow tests and six MCP/export-related tests passed. Full formatting and workspace/all-target Clippy checks passed. The app was rebuilt and packaged.
