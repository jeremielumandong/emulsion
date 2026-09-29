# Advanced motion and interchange

Select objects in Design and open **Animate**. **Motion preset** adds original native property tracks: Soft entrance, Rise, Pop into place, Gentle pulse, Full turn and Typewriter reveal. A preset replaces only its affected property tracks; other tracks and existing entrance/exit effects remain. Full turn rejects linked video/audio frames. Typewriter requires native text.

**Retime selected objects** scales and shifts all saved tracks and entrance/exit timing on the selection in one Undo step. A scale of 0.5 doubles speed; 2 halves speed. A new page duration may be applied in the same operation. Negative/out-of-duration points and millisecond rounding collisions reject the whole edit. Locked objects cannot be retimed. Cancel does not change the source.

Property keyframes include visibility and text reveal. Visibility uses 0 for hidden and 1 for shown (threshold 0.5), composed with authored visibility: hidden source objects stay hidden. Text reveal progresses from 0 to 1 over complete Unicode grapheme clusters and retains native rich-text formatting. Preview remains an isolated copy of the document. This is a typewriter reveal, not a arbitrary shape clipping channel.

Slide transitions include fade, slide from each of four directions, zoom in and zoom out, with the existing transition-duration control. Object interactions can use Click, Pointer enters or Drag and release; drag gestures trigger actions without moving authored objects.

**Export animated SVG** samples the active page at its saved frame rate and embeds scalable outlined SVG frames with discrete SVG animation. Unsupported compositing effects fall back to rendered PNG appearance and are counted in the export report. Browser SVG animation support is required; static SVG readers show the first frame. Text reveal is evaluated by the native engine before export.

**Import Lottie animation** brings supported JSON layers, paths, paint, text, embedded images and timing into native editable objects. **Export Lottie · editable vectors** writes the supported native vector/transform subset and rejects unsupported appearance with a clear diagnostic. **Export Lottie · rendered frames** remains an explicit PNG appearance fallback at up to 1024px. See [Lottie interchange](lottie-interchange.md) for supported features, approximation diagnostics and limits. Animated exports support at most 600 frames and 64 MiB per file. They cover active-page animation only: slide transitions and actions are excluded, and video/audio remain silent posters.

MCP uses `retime_design_motion`, `apply_design_motion_preset`, `set_design_keyframe` , `import_design_lottie` and `export_design_motion`, with the same validation and structured export diagnostics. Lottie encoding follows the public [layer](https://lottie.github.io/lottie-spec/latest/specs/layers/) and [asset](https://lottie.github.io/lottie-spec/latest/specs/assets/) formats.
