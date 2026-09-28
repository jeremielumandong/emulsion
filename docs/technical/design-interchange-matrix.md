# Design interchange acceptance matrix

This matrix describes supported Emulsion behavior. A native project is the
editable source; export formats carry different subsets. Export reports must
identify flattening or unsupported interactive content rather than silently imply
that every native feature survives.

| Feature | `.emu` / `.emutemplate` | SVG / PDF | PNG / JPEG | Standalone HTML |
| --- | --- | --- | --- | --- |
| Pages and native object identity | Editable pages, history in projects | Selected pages; SVG page archive / multipage PDF | Selected page archive | Ordered pages with navigation |
| Rich text, decorations and lists | Editable source and font resources | Native glyph outlines when appearance is supported | Raster output at requested dimensions | SVG glyph outlines; crisp when scaled |
| Multistop gradients and paths | Editable geometry, stops and styles | Native vector gradients | Raster output | Native SVG gradients |
| Semantic frame clipping and breakpoints | Base rules, child settings and thresholds | Resolved native frame clipping | Resolved image | Native snapshots at exported widths; fluid scaling between snapshots |
| Masks, blend modes and unsupported effects | Editable native sources | Explicit appearance fallback with report | Composited appearance | Strict rejection with diagnostic |
| Charts, tables, formulas and data mappings | Editable data, formulas and CSV bindings | Resolved chart/table artwork | Resolved artwork | Resolved vector artwork |
| Linked components, variables and reusable styles | Identities and links retained | Resolved appearance | Resolved appearance | Supported component states embedded as vector snapshots |
| Local audio/video and YouTube | Embedded local assets; official YouTube references | Poster appearance | Poster appearance | Local media embedded; YouTube requires network and user playback |
| Selection/frame crop | Native document remains unchanged | Bounded vector subtree when supported | Crop with optional transparency (PNG) | Whole selected pages |
| Presentation state and timing | Authored notes/actions/timing retained | Static pages | Static pages; separate GIF motion export | Supported actions, overlays, component states and motion |
| Advanced motion interchange | Native tracks, easing, presets and paragraph-aware reveal remain editable | Separate animated SVG embeds sampled frames; static SVG/PDF remains static | GIF or rendered-frame Lottie uses sampled raster appearance | Supported transforms/visibility; text reveal rejects with diagnostic |

Regression evidence lives in `design_responsive_tests.rs`, `design_font_tests.rs`,
`design_charts.rs`, `design_variable_tests.rs`, `design_bulk.rs`,
`selection_export.rs`, `design_html_tests.rs`, and `project_export.rs` in
`emulsion-io`. Native GPU comparisons cover decorated text and multistop edges.
Browser acceptance checks navigation, overlays, component states, responsive views,
metadata escaping and hidden controls in fullscreen. Current run results are
recorded separately; this matrix is not a substitute for executing those checks.

## PowerPoint interchange

A `.pptx` file is a package of linked presentation, slide, layout/master and notes
parts; it is not an SVG or HTML page collection with a different suffix. Microsoft
also defines extensions for features beyond the base PresentationML standard.
See Microsoft's [PresentationML structure](https://learn.microsoft.com/en-us/office/open-xml/presentation/structure-of-a-presentationml-document)
and [PowerPoint extension specification](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-pptx/efd8bb2d-d888-4e2e-af25-cad476730c9f).

The native [editable PowerPoint bridge](../guides/design-pptx.md) imports and exports `.pptx` using bounded ZIP/XML processing. Slides retain editable text, vector shapes, groups and pictures, with notes, slide order and supported links. The native UI and MCP share the same reader/writer and compatibility diagnostics. No Office runtime is bundled.

Unsupported geometry, charts/SmartArt/OLE, advanced effects, timelines and other format-specific behavior are reported per slide/object. Export retains supported editable base artwork rather than flattening complete slides. External resources are not fetched and macro packages are rejected. Binary `.ppt` is a separate format and remains unsupported. See the bridge documentation for its supported subset and independent-reader acceptance.

## Motion interchange boundaries

[Editable Lottie interchange](../guides/lottie-interchange.md) imports supported 2D shape/text/image layers and transform tracks with per-feature diagnostics. Default Lottie export writes editable vector geometry and supported keyframes. Unsupported native effects or motion combinations produce an explicit error directing the user to the separate rendered-frame option. No network resources are fetched during import.

[Animated SVG and rendered-frame Lottie](../guides/design-advanced-motion.md) remain available for appearance-oriented animation output. Animated SVG retains native outlined vectors where supported and reports rendered fallbacks. Rendered Lottie embeds sampled PNG layers up to 1024px on the longest side. These outputs are bounded to 600 frames and 64 MiB, exclude slide transitions and object interactions, and represent media as silent posters.
