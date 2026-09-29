# Standalone HTML presentations

Choose **Export → Interactive HTML**, for the current page or all pages. Enter
comma-separated widths or leave the field blank for phone (375), tablet (768),
authored frame breakpoints and the original page width. The exporter evaluates
native responsive layout at those widths, then scales each view fluidly in a
browser. This is a set of native layout samples, not a second continuous layout
engine. Supply additional widths when a design needs closer sampling.

The single `.html` file embeds native SVG outlines, images and deduplicated local
media. Fonts are outlined so recipients do not need your font installation. Page
names and metadata remain data, never executable template content. Nothing is
uploaded or published. Open the file in a modern browser and use arrows/Page Up/
Page Down to navigate, or its visible controls. Fullscreen hides controls; Escape
closes an overlay first and the browser can exit fullscreen. F toggles fullscreen.

Click actions support next/previous/back/specific page, modal overlays, closing an
overlay and referenced component variants. Entry/exit presets, property keyframes,
easing and page transitions play locally. Replay animation restarts the page's
timeline. Resizing selects the closest authored width at or below the browser
width while retaining active overlays and component state.

Local video/audio keeps trim, volume and loop settings, subject to the receiving
browser's codecs. YouTube uses its official embed and needs internet access;
players load only after a user clicks Play. Local playback does not require the
Emulsion application or its system player runtime. It does not add a browser to
the Emulsion package.

Unsupported vector blending, masks or effects currently reject HTML export with
an actionable diagnostic; no file is replaced on failure. Static PNG/PDF exports
remain available for those appearances. Missing slide destinations are reported.
The exporter supports at most 16 custom widths plus the original width, 64
referenced component states per page, and 128 MiB of output. Cross-platform
browser/runtime acceptance should include any codecs used by the shared file.

MCP uses `export_project` with `format:"html"`, a local `.html` path, optional
`pages`, and optional `widths` (64–8192 pixels). Print bleed does not apply.

Click, pointer-entry and drag-release action triggers are supported. Drag actions
activate after four screen pixels without moving authored objects. Transparent
SVG hit regions make text and hollow shapes usable throughout their bounds;
keyboard activation remains available. Visibility keyframes compose with authored
visibility. Native typewriter/text-reveal tracks currently reject HTML export with
a per-object diagnostic because the output uses font-independent glyph outlines.
Use native presentation or animated SVG for those tracks.
