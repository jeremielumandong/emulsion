# Editable PowerPoint presentations

Open a local `.pptx` with **Open**, or add its slides to the current project with MCP `import_project_pages`. Imported slides are native Design pages with editable text, vector paths, groups and pictures. Import never runs macros, follows remote image links, or opens hyperlinks.

Use **Export → Editable PowerPoint → all pages/current page**. MCP `export_project` accepts `format: "pptx"` and optional ordered page IDs. All selected pages must have the same dimensions; PowerPoint has one presentation-wide slide size. Print bleed and HTML responsive widths do not apply. Export preserves the source and writes the destination atomically.

Supported interchange includes:

- Slide order and names, speaker notes, and simple fade transitions.
- Native text boxes with Unicode, rich runs, font families/sizes, color, bold, italic, underline, strike, character spacing, paragraph alignment and spacing. List markers remain editable text on export, preserving nested numbering exactly.
- Editable custom vector contours, cubic/quadratic paths, standard rectangles/rounded rectangles/ellipses/triangles/diamonds, solid and multi-stop gradient paints, and group transforms.
- Embedded raster pictures, placement, rotation, transparency and imported picture crops. Imported crops retain original pixels under an editable mask.
- Internal slide links, next/previous-slide actions, and HTTP(S) links. Web links open the platform browser only on a presentation click. Edit saved links from **Object interactions → Edit saved web link**; empty URLs remove the link. MCP `set_presentation_actions` accepts `{ "type": "url", "url": "https://example.com" }` with a click trigger.
- Basic PowerPoint tables imported as grouped editable cell shapes and text.

This is an editable interchange bridge, not lossless PowerPoint compatibility. **Import / export notes** in the Export menu lists affected slide/object names. MCP returns the same diagnostics. Unsupported custom geometry or external images are diagnosed; charts, SmartArt and OLE frames become labeled editable placeholders. Imported tables do not retain table formulas, merged-cell semantics or theme cell styles. Master/layout decorations become ordinary editable objects; complex placeholder/theme inheritance can differ.

Layer masks, clipping stacks, non-normal blending, layer effects, adjustment layers, patterns, custom dash patterns, advanced text warps/path text, responsive rules, component links, video playback and animation timelines do not have complete portable equivalents. Export keeps editable base artwork and reports the unsupported feature; it does not replace each slide with a screenshot. Smart Objects export their source picture and report lost Smart/filter behavior. Imported picture masks also require a compatibility note on re-export. Hidden objects and hidden component definitions are excluded from visible slide output.

The reader accepts at most 100 slides, 4,096 package parts, 128 MiB compressed/256 MiB expanded data, 16 MiB per XML part, 64 MiB per binary part, 16,384 parsed objects per slide (also subject to the native document node limit) and 64 million decoded image pixels per presentation. XML depth/element counts, image dimensions and path anchors are bounded. DTDs, package traversal, duplicate parts and external resources are rejected or diagnosed.

Native project save/recovery and clipboard preserve the imported editable objects. Adding slides to an existing project uses its ordinary single-step import Undo.

The `pptx_fixture` IO example creates a two-slide presentation for independent-reader checks. LibreOffice Impress 26.8 opened the generated file, produced a two-page searchable PDF, preserved speaker notes and hyperlink targets when saving it again as PPTX, and the saved presentation reimported as two pages with 12 native editable objects and no compatibility warnings. This smoke test covers rich text, custom vector contours, groups, a rotated picture and a web link; it is not a claim of compatibility with every PowerPoint feature. Extremely small rotated source images can render differently in external readers; the original image pixels remain embedded.

Implementation follows Microsoft's [PresentationML package structure](https://learn.microsoft.com/en-us/office/open-xml/presentation/structure-of-a-presentationml-document), [slide master structure](https://learn.microsoft.com/en-us/office/open-xml/presentation/working-with-slide-masters), and [notes slide structure](https://learn.microsoft.com/en-us/office/open-xml/presentation/working-with-notes-slides). The bridge uses local ZIP/XML/image libraries and does not bundle an Office runtime or browser.
