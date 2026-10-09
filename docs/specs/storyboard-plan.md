# Storyboard workspace plan

Status: proposal, 2026-10-01. Nothing here is implemented. This plan adds a
sixth workspace, **Storyboard**, for planning film, animation and video shots
as drawn panels with shot notes, timing and an animatic. It reuses the Paint
brush engine, Design annotation objects, multi-page projects, presentation
playback and the print dialog's storyboard sheets, and adds only what those
do not cover. [Storyboard parity plan](storyboard-pro-parity.md)
widens the scope to parity with industry-standard storyboard software: it adds an act/sequence/scene
hierarchy, thumbnail pages, camera and layer animation, audio/video tracks,
the Panel Timer, editorial conform and AI assistance, with 3D deferred. Its
phases replace *Delivery phases* below.

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
or a video animatic.

Considered alternatives:

- **Storyboard mode inside Design.** Design already has pages, notes and timing,
  but its object-first canvas, layout and component tooling get in the way of
  sketching, shot metadata does not fit its panels, and it would mean changing
  the Design workspace. Rejected.
- **Paint animation frames (one layer per panel).** Simple, but a panel is not a
  layer: it needs its own layers, notes, history and size. Rejected.
- **Recommended: a distinct `ProjectKind::Storyboard`** whose pages are normal
  `Document`s, so rendering, history, `.emu` packaging, printing and export keep
  working, plus a storyboard-specific UI.

## Workspace boundary

Storyboard is a **separate workspace**. It borrows existing code but does not
change the Design workspace.

- **Not modified:** the Design workspace's UI, commands, menus, file format and
  behaviour, and the `Design` metadata struct. A storyboard never shows
  Design's panels, sections or toolbar. Design-only modules change only to
  move reusable code out (see *Shared modules*).
- **Borrowed as is:** the document and layer model, the shared editor canvas
  and tool rail (with Paint's tool layout), brushes, raster and vector paths,
  text objects, selections, history and undo, `.emu` packaging, the print dialog,
  image/GIF export, the thumbnail cache, FFmpeg frame extraction, AI models and
  the assistant.
- **Storyboard-owned:** everything storyboard-specific lives in new modules
  (`emulsion-core/src/storyboard*.rs`, `emulsion-ui/src/editor/storyboard*.rs`,
  `emulsion-io/src/storyboard*.rs`, `emulsion-mcp` storyboard tools): shot
  metadata, captions, the Board/Panel/Animatic views, camera moves, naming
  rules and storyboard export layouts. Generic machinery they rely on (timeline,
  audio, motion, transitions, video export) goes in shared modules.
- **Shared code changes:** when shared code needs a hook (a `ProjectKind` arm,
  a `Destination`, a rail layout choice, a manifest field), the change is
  additive, applies only when the project kind is Storyboard, and leaves
  existing behaviour identical. Existing Design, Diagram, Photo and Paint tests
  must pass unchanged. Moving reusable code out of Design (below) is allowed
  under the same rule.
- **Reusable code moves out, not copied.** When Storyboard needs logic that
  lives in a Design module but isn't Design-specific, it is first moved into a
  neutral shared module, and both Design and Storyboard use it from there (see
  *Shared modules*). Storyboard-specific behaviour stays in storyboard modules.

### Shared modules

Extraction rules:

1. Each extraction is a **pure move**: its own commit (or PR) before any
   Storyboard code uses it, with no behaviour change. Design calls the new
   module, or the old path re-exports it, so Design's UI, file format and
   output are unchanged.
2. Design's existing tests must pass **without edits**. Tests for the moved
   logic move with it; new tests cover the shared API on its own.
3. Shared modules have **neutral names and no workspace knowledge**: no
   `ProjectKind` checks and no Design or Storyboard types. Each workspace adapts
   its own data to the shared API (for example, Design keeps `PageTransition`
   and maps it to the shared transition renderer).
4. Code written new for Storyboard that other workspaces could use is created in
   shared modules from the start, not in `storyboard*` files.
5. **No duplication.** Storyboard never copies existing code. If something it
   needs already exists anywhere in the codebase, it calls it, or moves it into a
   shared module first under rules 1–3. A copy is acceptable only when sharing
   would change another workspace's behaviour, and then the reason is
   written in a code comment and in this spec.
6. Reviews check for duplication: a Storyboard change that reimplements
   existing logic (rendering, export, timing, media, keyframes, transitions)
   is sent back to share it instead.

Candidates to move out of Design (sizes as of 2026-10-01; **Phase** is the
[parity phase](storyboard-pro-parity.md#delivery-phases) that first needs it):

| Shared module (proposed) | Moved from | What becomes reusable | Phase |
| --- | --- | --- | --- |
| `emulsion-core/src/motion/` (`easing`, `track`) | `design_keyframes.rs` (645 lines) | `Easing`, keyframe tracks, interpolation and evaluation over time; Design keeps its `Property` set and storage | 5 |
| `emulsion-core/src/transition.rs` and its renderer | `PageTransition` rendering in `design_presentation_ui.rs` | Fade/slide/zoom transition rendering between two frames | 5 |
| `emulsion-ui/src/playback/` | `design_presentation_ui.rs` (1,553 lines) | Audience/fullscreen window, presenter window, playback clock, auto-advance | 5 |
| `emulsion-core/src/media.rs`, `emulsion-io/src/media.rs`, `emulsion-ui/src/media_player.rs` | `design_local_media.rs` (core 301, io 404), `design_local_media_ui.rs` (155) | Embedded audio/video assets, limits, trim/volume, system-webview playback | 5 |
| `emulsion-io/src/frame_export.rs` | `project_animation.rs` (139), `editor/animation.rs` `encode_gif` | Frame-source trait plus bounded GIF writing, shared by Design, Paint and Storyboard | 5 |
| `emulsion-io/src/pptx/` slide input | PPTX writer's Design-specific entry | A slide list (image, notes, timing) any workspace can export | 5 |

New shared code created for Storyboard (reusable by Design, Paint or Diagram
later):

| Shared module (proposed) | Contents |
| --- | --- |
| `emulsion-core/src/timeline/` | Track/clip model, frame and timecode maths, ripple/roll edits, markers |
| `emulsion-io/src/audio/` | Decode, waveform peaks, mixing, recording |
| `emulsion-io/src/video_export.rs` | FFmpeg encode with audio muxing, progress and cancel |
| `emulsion-io/src/interchange/` | EDL, XML edit list (xmeml), OpenTimelineIO read/write |
| `emulsion-io/src/script/` | Plain text, Fountain and FDX parsing |
| `emulsion-ui/src/thumbnail_grid.rs` | Virtualized, reorderable thumbnail grid (Board view; reusable for Design pages or Library) |

If `timeline`, `audio` and `video_export` grow large, they can become their own
crate (for example `emulsion-timeline`) without changing the rules above.

## What already exists

| Need | Existing code | Gap |
| --- | --- | --- |
| Ordered panels | `Project` / `ProjectEditor` in `crates/emulsion-core/src/project.rs`: up to 4096 pages, page undo/redo, reorder | Storyboard kind and shot metadata |
| Panel drawing | Paint tools, brushes, guides, QuickShape, onion skin (`docs/guides/paint.md`); `NodeKind::Raster` layers | Brush tools enabled on project pages (verify current gating) |
| Annotation | Shared shape, text and vector path objects | Storyboard-specific arrow presets and stamps |
| Per-panel timing | Design's page timing (`design_metadata.rs`), as a reference only | Storyboard-owned duration and transition fields |
| Per-panel notes | Design's speaker notes, as a reference only | Storyboard-owned structured fields (action, dialogue, SFX) |
| Camera motion | Keyframes and easing in `design_keyframes.rs` (moved to the shared `motion` module) | Storyboard camera move on the shared tracks |
| Playback | `editor/animation.rs` frame playback; Design Present mode as a reference | Storyboard animatic player with scrubbing and audio |
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

*As implemented in phase 1:* shot data lives in `storyboard::Panel` (duration
in **frames** at the project frame rate rather than milliseconds, captions by
caption field, shot size, angle, status, tag); scene, shot and sequence numbers
come from the act/sequence/scene grouping in the
[parity spec](storyboard-pro-parity.md#data-model-additions). Camera movement
arrives with the camera phase. The original proposal follows.

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

- **Duration and transition** are `Shot` fields (`duration_ms`, `transition`,
  `transition_ms`), not the Design metadata, so the Design workspace is never
  involved. The project holds one frame rate. Defaults: 2000 ms, 24 fps;
  allowed range 100 ms – 10 min per panel.
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

### Interchange

Storyboard pages are complete `Document`s, so shared exporters (images, PDF,
print, PSD/ORA) work on them directly. There is no "Open in Design" command: a
storyboard is shared as PDF, PPTX, images or video instead.

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
- **Tool rail** uses the shared rail with Paint's tool layout, plus storyboard tools:
  - Paint: Brush/Liquify, Smudge, Eraser, Eyedropper, Bucket/Gradient,
    Marquee/Lasso/Quick select, Move, Hand/Rotate, Zoom.
  - Storyboard: **Arrow** (curved movement arrow with presets for character
    move, camera pan, push in, pull out), **Text** (the shared text object with caption
    and SFX styles), **Camera** (edit frame, start/end rectangles, easing).
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
- **From Design:** read a Design `.emu` file into a new storyboard (read-only;
  the Design file and workspace are untouched); speaker notes become action text.

## Export

| Output | Approach |
| --- | --- |
| Storyboard PDF / print | New print layouts using the print dialog: 1, 2, 3, 6 or 9 panels per sheet; portrait or landscape; notes beside or below panels; header with project, scene and page numbers. |
| Image sequence | PNG/JPEG per panel, named `{scene}_{shot}.png` with collisions suffixed. |
| GIF | The shared `frame_export` GIF writer (moved out of `project_animation.rs` and `editor/animation.rs`), fed by a storyboard frame source with timing and camera moves. |
| MP4 (H.264) / WebM | Render frames, then pipe raw RGBA to FFmpeg on PATH with the audio track muxed in. Same limits and errors as the existing FFmpeg use: missing FFmpeg is a clear error and no codec is bundled. |
| Shot list CSV | `Shot` fields plus duration and running timecode. |
| PPTX | Storyboard adapter calling the shared PPTX writer in `emulsion-io`, with shot notes as speaker notes. Any writer change is additive and leaves Design's export output unchanged. |

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
   file import, MCP tools and a guide (`docs/guides/storyboard.md`) with a
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
- The Design workspace is unchanged: its tests pass without edits, and no
  Design menu, panel or command appears in or depends on Storyboard.

## Open questions

1. ~~Can Paint's brush tools run on project pages?~~ Answered: yes. The shared
   editor already shows the tool rail, including Brush, on project pages
   (`editor/design_ui.rs`), and `rail_groups()` in `editor/rail.rs` switches to
   Paint's layout in draw mode. Storyboard enables that layout in its own
   workspace, so phase 3 is mostly wiring.
2. Should a panel allow several frames (sub-panels for action beats), or should
   beats always be separate panels with a shared shot number?
3. Is camera move interpolation enough, or is per-layer parallax motion
   (storyboard-owned layer tracks) needed in the first release?
4. Should storyboards support sharing comments or review approval through the
   cloud sync plan, or stay local-only at first?
