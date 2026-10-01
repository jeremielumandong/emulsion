# Storyboard workspace plan

Status: proposal, 2026-10-01. Nothing here is implemented. This plan adds a
sixth workspace, **Storyboard**, for planning film, animation and video shots
as drawn panels with shot notes, timing and an animatic. It reuses the Paint
brush engine, Design annotation objects, multi-page projects, presentation
playback and the print dialog's storyboard sheets, and adds only what those
do not cover. [Storyboard parity with Toon Boom Storyboard Pro](storyboard-pro-parity.md)
widens the scope to full parity: it adds an act/sequence/scene hierarchy,
camera and layer animation, 3D, audio/video tracks and editorial conform, and
its ten phases replace *Delivery phases* below.

## Product direction

A storyboard is an ordered list of **panels**. Each panel is one drawing inside
a camera frame, plus structured shot information (scene, shot, duration, camera,
action, dialogue). Artists move between three views of the same project:

1. **Board**: a grid of panel thumbnails with their notes, for ordering and
   reviewing the sequence.
2. **Panel**: one panel open on the canvas for drawing and annotation.
3. **Animatic**: panels played in order at their durations, with camera moves
   and an optional scratch audio track.

The same project prints as storyboard sheets, exports as PDF, an image sequence
or a video animatic, and opens in Design as an ordinary multi-page project.

Considered alternatives:

- **Storyboard mode inside Design.** Design already has pages, notes and timing,
  but its object-first canvas, layout and component tooling get in the way of
  sketching, and shot metadata does not fit its panels. Rejected as the primary
  surface; the file stays Design-compatible instead (see *Data model*).
- **Paint animation frames (one layer per panel).** Simple, but a panel is not a
  layer: it needs its own layers, notes, history and size. Rejected.
- **Recommended: a distinct `ProjectKind::Storyboard`** whose pages are normal
  `Document`s, so rendering, history, `.emu` packaging, printing and export keep
  working, plus a storyboard-specific UI.

## What already exists

| Need | Existing code | Gap |
| --- | --- | --- |
| Ordered panels | `Project` / `ProjectEditor` in `crates/emulsion-core/src/project.rs`: up to 4096 pages, page undo/redo, reorder | Storyboard kind and shot metadata |
| Panel drawing | Paint tools, brushes, guides, QuickShape, onion skin (`docs/guides/paint.md`); `NodeKind::Raster` layers | Brush tools enabled on project pages (verify current gating) |
| Annotation | Design shapes, text, vector paths, components | Storyboard-specific arrow presets and stamps |
| Per-panel timing | `Design::duration_ms`, `fps`, `page_transition`, `transition_ms` in `crates/emulsion-core/src/design_metadata.rs` | Board/timeline editing UI |
| Per-panel notes | `Design::speaker_notes` | Structured fields (action, dialogue, SFX) |
| Camera motion | `Design::keyframes` / `motion` per node | Camera move as a page-level move of the frame |
| Playback | Design Present mode, auto-advance; `editor/animation.rs` frame playback | Animatic view with scrubbing and audio |
| Animation export | `emulsion-io/src/project_animation.rs::write_gif` | MP4/WebM video, audio muxing |
| Sheets | Print dialog storyboard contact sheets (`emulsion-io/src/printing/sources.rs`) | Storyboard-native layouts with notes |
| File format | `.emu` package: manifest plus one ORA per page (`emulsion-io/src/project.rs`) | Manifest field for shot metadata |
| Video frames | FFmpeg on PATH for reading local video (print sources) | FFmpeg for writing video |

## Data model

### Project kind

Add `ProjectKind::Storyboard` in `emulsion-core/src/project.rs`. Every match on
`ProjectKind` gains an arm: label, destination mapping in
`emulsion-ui/src/workspace/destinations.rs` (`Destination::Storyboard`, Home card,
"New storyboard…", "Open storyboard…"), and `.emu` read/write. Older builds must
reject a Storyboard `.emu` with a clear message, not misread it as Design.

### Shot metadata

Add a `Shot` struct stored per page. Keep it out of `PageMeta`, which holds page
identity and print bleed, and out of `Design`, which is shared with Design pages.
Store it in the `.emu` manifest's `PageRecord` with `#[serde(default)]` so the
manifest version stays readable.

```rust
pub struct Shot {
    pub scene: String,          // "12", "12A"; up to 16 chars
    pub shot: String,           // "3", "3B"; up to 16 chars
    pub size: ShotSize,         // ExtremeWide, Wide, Full, Medium, MediumClose,
                                // CloseUp, ExtremeClose, Insert, Unset
    pub angle: CameraAngle,     // Eye, High, Low, Overhead, Dutch, Pov, Unset
    pub movement: CameraMove,   // Static, Pan, Tilt, Dolly, Truck, Zoom, Crane,
                                // Handheld, Custom(String)
    pub action: String,         // up to 4000 chars
    pub dialogue: String,       // up to 4000 chars
    pub sfx: String,            // up to 1000 chars
    pub status: PanelStatus,    // Rough, Clean, Approved
    pub color_tag: Option<u8>,  // index into a fixed 8-colour palette
}
```

- **Duration and fps** reuse `Design::duration_ms` and `fps` so the animatic,
  GIF export and Design compatibility share one timing source. Storyboard
  defaults: 2000 ms, 24 fps; allowed range 100 ms – 10 min per panel.
- **Shot numbers** are free text, not computed, because productions renumber
  manually ("12A"). The Board offers **Renumber shots…** as one undoable action.
- **Validation** mirrors `PageMeta::validate`: length limits, no control
  characters except newlines in multi-line fields.
- **Undo**: shot edits are page-level steps in `ProjectEditor`, alongside page
  reorder, so one undo stack covers sequence editing.

### Camera frame

Each project has one **aspect ratio** (16:9, 1.85:1, 2.39:1, 4:3, 1:1, 9:16 or
custom) and every panel uses that page size. Each panel can have an
**overscan** margin (0–50 %) drawn outside the frame. Only the framed area
appears in sheets and the animatic. Title-safe and action-safe guides are
overlays, never part of the document.

A **camera move** is two rectangles inside the overscan area, *start* and
*end*, interpolated over the panel duration with an easing (linear, ease in,
ease out, ease in-out). It is stored in `Shot` and applied at playback/export
time; it never rewrites pixels. Static is the default.

### Design compatibility

Storyboard pages are complete `Document`s. **Open in Design** creates a Design
project copy. Durations, transitions and art carry over, and shot fields go into
`speaker_notes` as text. Nothing round-trips back automatically.

## User interface

### Board view (default)

- Responsive grid of panel cards. Each card shows the thumbnail (framed area),
  scene/shot badge, duration, status chip and colour tag, and the first lines of
  action and dialogue. Thumbnails use the existing thumbnail cache.
- Zoom slider: 3–8 cards per row; a compact list mode with a larger notes column.
- Selection: click, Shift-range, Ctrl/Cmd-toggle. Drag to reorder, with an
  insertion marker. Moving several panels keeps their relative order.
- Commands: New panel (after selection), Duplicate, Delete, Split scene here,
  Renumber shots…, Set duration…, Set status, Colour tag. All are undoable.
- Scene dividers: a header appears wherever the scene field changes, and can be
  collapsed.
- Double-click or Enter opens the panel in Panel view.
- Keyboard: arrows move the selection, Ctrl/Cmd+D duplicates, Delete removes,
  Alt+arrows move the selection left or right.

### Panel view

- The canvas shows the camera frame, a dimmed overscan area and optional safe
  guides. A narrow filmstrip above the canvas shows neighbouring panels;
  Page Up/Page Down moves between panels.
- **Tool rail** combines Paint and Design tools:
  - Paint: Brush/Liquify, Smudge, Eraser, Eyedropper, Bucket/Gradient,
    Marquee/Lasso/Quick select, Move, Hand/Rotate, Zoom.
  - Storyboard: **Arrow** (curved movement arrow with presets for character
    move, camera pan, push in, pull out), **Text** (Design text with caption and
    SFX styles), **Camera** (edit frame, start/end rectangles, easing).
- Drawing aids available from Paint: perspective guides with Drawing Assist,
  QuickShape, symmetry, reference images.
- **Onion skin** shows the previous and/or next panel at adjustable opacity.
  It reuses the animation onion code but reads neighbouring pages instead of
  layers.
- **Shot inspector** (right panel): every `Shot` field, duration and transition.
  Each commit is one undoable step.
- New panels start with a configurable layer stack (e.g. *Background*,
  *Characters*, *Notes*). Panels may use any number of layers.

### Animatic view

- Player using the framed area, with camera moves and transitions applied.
- Timeline strip: one clip per panel, width proportional to duration. Drag a
  clip edge to change duration and drag a clip to reorder (both undoable).
  A playhead, total running time and time per scene are shown.
- One optional **scratch audio** track (WAV, MP3, FLAC, OGG), shown as a waveform
  and offset in milliseconds. It plays in sync and is muted when it ends.
- Controls: play/pause (Space), step panel (←/→), loop range, playback speed
  (0.5×, 1×, 2×), fullscreen.
- **Fit to audio…** scales all durations proportionally so the sequence matches
  the audio length (one undoable step).

## Import

- **Images as panels:** a folder or multi-select of images makes one panel per
  file, ordered by name, fitted into the frame.
- **Script / shot list CSV:** columns map to `Shot` fields and make blank panels.
  A preview lets the user map columns and shows rejected rows.
- **Video frames:** reuse the print dialog's FFmpeg extraction to turn a
  reference video's timestamps into panels.
- **From Design:** convert a Design project; `speaker_notes` becomes action text.

## Export

| Output | Approach |
| --- | --- |
| Storyboard PDF / print | New print layouts using the print dialog: 1, 2, 3, 6 or 9 panels per sheet; portrait or landscape; notes beside or below panels; header with project, scene and page numbers. |
| Image sequence | PNG/JPEG per panel, named `{scene}_{shot}.png` with collisions suffixed. |
| GIF | Existing `project_animation::write_gif`, extended with camera moves. |
| MP4 (H.264) / WebM | Render frames, then pipe raw RGBA to FFmpeg on PATH with the audio track muxed in. Same limits and errors as the existing FFmpeg use: missing FFmpeg is a clear error and no codec is bundled. |
| Shot list CSV | `Shot` fields plus duration and running timecode. |
| PPTX | Existing Design PPTX export, with shot notes as speaker notes. |

Export renders from a snapshot, like printing, so it never modifies the project.
Video export runs off the UI thread with progress and cancel, and writes atomically
like `write_atomic`.

## Assistant and MCP

Add MCP tools in `emulsion-mcp` alongside the existing editor tools:
`storyboard_list_panels`, `storyboard_add_panel`, `storyboard_move_panels`,
`storyboard_set_shot`, `storyboard_set_camera_move` and
`storyboard_import_shot_list`. Drawing on a panel uses the existing editing
tools on the active page. Each tool call is one undo step, as elsewhere.

## Limits

- Panels: `MAX_PAGES` (4096) and the existing project pixel limit.
- Animatic: up to 3 hours total; video export is bounded the same way as
  `write_gif`'s frame cap, but raised for streaming encode (frames are never all
  held in memory).
- Audio: one track, up to 3 hours, decoded once into a bounded cache.

## Delivery phases

Superseded by the [parity phases](storyboard-pro-parity.md#delivery-phases);
kept for the original minimal scope.


1. **Foundation.** `ProjectKind::Storyboard`, `Shot` model with validation and
   serde, `.emu` manifest support, Destination and Home card, "New storyboard"
   dialog (aspect ratio, panel count, default duration). Tests: round-trip,
   rejection by older builds, page-level undo of shot edits.
2. **Board view.** Grid, selection, drag reorder, scene dividers, inspector,
   renumbering. Tests: reorder and undo, multi-select moves, renumber.
3. **Panel view.** Camera frame and overscan, Paint tools on storyboard pages,
   neighbour onion skin, filmstrip, Arrow and Text presets.
4. **Animatic.** Player, timeline durations, camera moves, transitions, GIF
   export with camera moves.
5. **Sheets and exports.** Print layouts, PDF, image sequence, shot list CSV,
   PPTX.
6. **Audio and video.** Scratch track, waveform, Fit to audio, MP4/WebM via
   FFmpeg.
7. **Import and assistant.** Image folder, CSV shot list, video frames, Design
   conversion, MCP tools and a guide (`docs/guides/storyboard.md`) with a
   tutorial.

Each phase ships on Linux, macOS and Windows together, following
[Design platform acceptance](design-platform-acceptance.md).

## Acceptance criteria

- A 120-panel 16:9 board at 1920×1080 opens, scrolls and reorders without
  visible stalls on the reference machines in the performance reports.
- Every Board, inspector and timeline change is undoable and survives save and
  reopen.
- The animatic's running time equals the sum of panel durations to ±1 frame,
  and audio stays within one frame of the picture over 10 minutes.
- Printed sheets show the framed area only, with shot fields matching the
  inspector.
- MP4 export without FFmpeg fails before rendering with an actionable message
  and leaves no partial file.
- Opening in Design preserves art, order, durations and transitions.

## Open questions

1. Can Paint's brush tools already run on project (Design) pages, or does the
   tool rail gate them by workspace? This decides how much of Phase 3 is wiring
   versus new work.
2. Should a panel allow several frames (sub-panels for action beats), or should
   beats always be separate panels with a shared shot number?
3. Is camera move interpolation enough, or is per-layer parallax motion
   (reusing `Design::keyframes`) needed in the first release?
4. Should storyboards support sharing comments or review approval through the
   cloud sync plan, or stay local-only at first?
