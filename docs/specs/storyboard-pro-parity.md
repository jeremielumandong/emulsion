# Storyboard parity with Toon Boom Storyboard Pro

Status: proposal, 2026-10-01. This document extends the
[Storyboard workspace plan](storyboard-plan.md) to cover the functionality of
Toon Boom Storyboard Pro, the industry reference for animation and live-action
boards. Nothing here is implemented.

The capability list is compiled from Storyboard Pro's public product information
and documentation, not from a licensed copy. Before work begins on each phase,
verify that phase's rows against a current Storyboard Pro release and record any
differences here. Parity means comparable **capability**. Emulsion does not copy
Storyboard Pro's interface, icons, names of proprietary features or file formats.
Proprietary formats are out of scope unless openly documented (see *Out of scope*).

Legend for **Today**: ✅ exists and can be reused · 🟡 partial foundation ·
❌ nothing yet. **Phase** refers to *Delivery phases* below.

## 1. Project structure

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| P1 | Project → sequence → scene → panel hierarchy | 🟡 flat page list in `Project` | `Sequence` and `Scene` records in the `.emu` manifest; pages stay panels. Scene is the unit of shot naming and the camera. | 1 |
| P2 | Acts or other optional top-level grouping | ❌ | Optional `Act` grouping above sequences; hidden when unused | 1 |
| P3 | Automatic scene/panel naming and renumbering, with configurable rules | ❌ | Naming rules (prefix, padding, increment, letter suffix for inserts); **Renumber** for the project, a sequence or a selection | 2 |
| P4 | Panel/scene lock (protect from edits) | 🟡 layer locks exist | Lock flag on panel and scene, honoured by every command and MCP tool | 2 |
| P5 | Add, insert, duplicate, delete panels; smart add (copies chosen layers) | 🟡 page add/duplicate | **Smart add** with a per-project list of layers to carry over | 2 |
| P6 | Split and join scenes and panels; move panels between scenes | ❌ | Split scene at a panel, join adjacent scenes, drag panels across scene boundaries | 2 |
| P7 | Project templates (resolution, aspect, captions, layers, naming) | 🟡 Design templates | Storyboard templates packaged with the existing template pack format | 3 |
| P8 | Project resolution and frame rate presets (film, HDTV, 4K, vertical, custom; 23.976–60 fps) | 🟡 per-page `fps` | One project resolution and frame rate; per-panel `fps` stays in sync | 1 |

## 2. Views and workspaces

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| V1 | Stage view (drawing, with overscan and field guide) | 🟡 canvas | Camera frame, overscan, field guide and safe area overlays | 3 |
| V2 | Camera view (shows only the framed shot) | ❌ | Toggle that renders only the framed area at the current time | 3 |
| V3 | Thumbnails / board view | ❌ | Board grid from the plan, with sequence and scene headers | 2 |
| V4 | Panel view (layers, captions for the current panel) | 🟡 layers panel | Panel inspector: layer list, captions, duration, transition | 2 |
| V5 | Timeline view (panels, transitions, camera, audio/video tracks) | ❌ | Track-based timeline (section 6) | 5 |
| V6 | Top and side views for 3D positioning | ❌ | Orthographic views of layer depth and 3D objects (section 5) | 7 |
| V7 | Workspace presets (overview, drawing, timing, 3D) and custom layouts | 🟡 Paint layout presets | Storyboard layout presets; save and restore custom layouts | 3 |
| V8 | Light table and onion skin across panels | 🟡 onion skin across layers | Light table showing selected neighbouring panels at set opacity and tint | 3 |
| V9 | Full-screen and second-monitor playback | 🟡 Design presenter window | Animatic player on the audience window | 5 |

## 3. Drawing and paint

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| D1 | Bitmap brushes with pressure, tilt, textures | ✅ 55 brushes, GPU engine, tablets | Enable on storyboard panels | 3 |
| D2 | Vector drawing layers (editable strokes, pencil lines with variable width) | 🟡 vector paths (`NodeKind` path, Design vector editing) | Vector stroke layer: pressure-width centreline strokes, editable points, reshape, smoothing | 4 |
| D3 | Pencil, brush, eraser, line, rectangle, ellipse, polyline | 🟡 bitmap brush/eraser, QuickShape, Design shapes | Line/rectangle/ellipse/polyline drawing tools that create vector strokes or bitmap pixels depending on the layer | 4 |
| D4 | Paint bucket with gap closing, paint behind / paint unpainted | 🟡 bitmap bucket | Gap-closing fill (bitmap and vector), fill modes | 4 |
| D5 | Select, transform, cutter, contour editor, perspective distort | 🟡 marquee, lasso, move, transform, vector editing | Cutter (lasso cut to new layer), perspective and envelope distortion on selections | 4 |
| D6 | Smooth, flatten, convert pencil ↔ brush strokes, optimize | ❌ | Stroke smoothing and flattening; outline/centreline conversion on vector layers | 4 |
| D7 | Perspective guides: 1-, 2-, 3-point, 4-/5-point curvilinear, grid, isometric; snapping | 🟡 1-, 2-, 3-point, grid, isometric with Drawing Assist | Add 4- and 5-point (fish-eye) guides and saved guide sets per panel | 4 |
| D8 | Rulers and guides | 🟡 Design guides | Straight-edge ruler that strokes snap to | 4 |
| D9 | Colour palettes, swatches, palette libraries | ✅ project palette, Design styles | Storyboard default palettes (greys, accent, notes colours) | 3 |
| D10 | Rotate, flip and mirror view | ✅ Rotate View; 🟡 flip | Flip view horizontally/vertically without changing the art | 3 |
| D11 | Reference images | ✅ | Reuse | 3 |
| D12 | Symmetry | ✅ mirror, radial | Reuse | 3 |

## 4. Layers and layer animation

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| L1 | Bitmap, vector and group layers; opacity, visibility, lock | ✅ raster, group, locks; 🟡 vector | Vector stroke layer (D2) | 4 |
| L2 | Blending modes and masks | ✅ | Reuse | 3 |
| L3 | Layer motion keyframes: position, scale, rotation, skew, opacity, with easing | 🟡 `design_keyframes`: offset, scale, rotation, opacity, 5 easings | Add skew and pivot point; keyframe editing on the timeline and in the stage view | 6 |
| L4 | Function curves / velocity editing | ❌ | Bezier ease editor per keyframe segment | 6 |
| L5 | Motion paths shown on stage | ❌ | Draw the layer path with keyframe handles; drag to edit | 6 |
| L6 | Layer depth (Z) for parallax with the camera | ❌ | Per-layer depth; parallax evaluated with camera moves | 7 |
| L7 | Import layered PSD into a panel | ✅ PSD/PSB import | Import into the current panel or as new panels (one per file) | 3 |
| L8 | Layer comps (save and recall layer visibility) | ❌ | Named visibility sets per panel | 6 |

## 5. Camera and 3D

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| C1 | Camera keyframes: pan, zoom (truck), rotate, with easing | ❌ (plan has start/end only) | Full camera keyframe track per scene, using the `design_keyframes` easings | 6 |
| C2 | Camera spanning several panels in a scene | ❌ | The camera belongs to the scene; keyframes are timed across its panels | 6 |
| C3 | Static camera per panel, reset camera, copy/paste camera | ❌ | Commands on the camera track | 6 |
| C4 | Camera shake and handheld presets | ❌ | Seeded noise generator applied on top of keyframes | 6 |
| C5 | Field guide, safe areas, custom overlays | ❌ | Overlay set from V1 | 3 |
| C6 | 3D-capable scenes: layers positioned in depth, 3D camera with field of view | ❌ | Perspective camera evaluating layer depth (L6); top/side views (V6) | 7 |
| C7 | Import 3D models (e.g. FBX, OBJ, glTF) and pose/position them | ❌ | glTF 2.0 and OBJ import, rendered with `emulsion-gpu`; position, rotate, scale and keyframe; no rigging or modelling | 7 |
| C8 | Snapshot a 3D view into a drawing layer to trace over | ❌ | Render current 3D view to a new bitmap layer | 7 |

## 6. Timing, animatic and sound

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| T1 | Panel durations edited in the timeline (frames or timecode) | 🟡 `duration_ms` per page | Timeline with frame and SMPTE timecode display; trim edges; ripple and roll | 5 |
| T2 | Scene transitions: cut, dissolve, wipe (edge, clock, radial), slide, fade to colour | 🟡 8 Design page transitions | Add wipes and fade to colour; transition length edited on the timeline | 5 |
| T3 | Multiple audio tracks with waveforms, volume, mute/solo, fades | 🟡 embedded audio objects with trim, volume, loop | Project audio tracks (up to 16) with clips, waveforms, gain envelope and fades | 5 |
| T4 | Record voice directly into a track | ❌ | Microphone capture to WAV on Linux, macOS and Windows | 8 |
| T5 | Import video as a reference track | 🟡 local video in Design | Video track with frame-accurate scrubbing (FFmpeg decode) | 8 |
| T6 | Real-time playback with audio scrubbing; play range and loop | 🟡 Present mode | Animatic player at project fps, dropping frames rather than drifting | 5 |
| T7 | Timing tools: fit selection to duration, conform panels to audio markers | ❌ | Fit to duration; markers on audio tracks; snap panels to markers | 5 |
| T8 | Timecode burn-in and overlays (scene, panel, timecode) on playback and export | ❌ | Overlay options shared by player and export | 5 |

## 7. Captions and script

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| S1 | Caption fields per panel: action, dialogue, slugging, notes | 🟡 `speaker_notes` | Caption fields from the plan's `Shot`, with slugging added | 1 |
| S2 | Custom caption fields defined per project | ❌ | Project-level caption definitions (name, multi-line, export visibility) | 2 |
| S3 | Rich text in captions (bold, italic, colour, size) | 🟡 Design rich text | Reuse the text model in captions | 2 |
| S4 | Spell checking | ❌ | Platform spell checker where available, otherwise a bundled open dictionary | 8 |
| S5 | Find and replace across captions | ❌ | Project-wide find/replace, with undo | 2 |
| S6 | Import script text and split it into panels | ❌ | Import plain text and Fountain; split per paragraph or dialogue block into panels and captions | 8 |
| S7 | Import Final Draft scripts | ❌ | Final Draft `.fdx` is XML; import scene headings, action and dialogue | 8 |

## 8. Library and reuse

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| R1 | Library of reusable drawings, characters, props, backgrounds | 🟡 Design components, brand assets | Project and personal libraries of layers and panels; drag onto a panel | 3 |
| R2 | Templates of panels or scenes with animation | ❌ | Save a panel or scene, with keyframes and camera, as a library item | 6 |
| R3 | Copy and paste panels, layers and cameras between projects | 🟡 clipboard | Storyboard clipboard formats for panels, scenes and camera tracks | 2 |

## 9. Collaboration and review

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| K1 | Extract a range of scenes for another artist and merge it back | ❌ | **Extract** a range to a new `.emu` with a source ID; **Merge** replaces the range and reports conflicts | 9 |
| K2 | Change tracking: mark new or modified panels since a version | 🟡 branchable history | Compare against a saved commit; show new, changed and deleted panels on the board | 9 |
| K3 | Compare two versions side by side | 🟡 history graph | Panel-by-panel compare view | 9 |
| K4 | Review notes and annotation layers that don't print | ❌ | Non-printing review layer type and per-panel review status | 9 |
| K5 | Shared projects / database workflows | 🟡 cloud sync plan | Use the cloud sync plan for file sharing; no server-side database | 10 |

## 10. Editorial conform

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| E1 | Export the animatic to editing software (EDL, AAF, Final Cut XML) | ❌ | Export CMX 3600 EDL, FCP 7 XML (xmeml) and OpenTimelineIO, with per-panel media | 9 |
| E2 | Import an edit back and conform panels to it | ❌ | Read EDL, xmeml and OpenTimelineIO; match clips to panels by name; apply durations, order and audio | 9 |
| E3 | AAF interchange | ❌ | Investigate; AAF needs a structured-storage parser (see *Out of scope*) | — |

## 11. Export, print and publishing

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| X1 | PDF export with configurable layouts (panels per page, captions, header/footer, logos) | 🟡 print dialog contact sheets | Storyboard PDF layout profiles: grid, columns, caption fields, header/footer tokens, logo; saved and shareable | 4 |
| X2 | Image export per panel or per layer | 🟡 page export | Naming tokens (`{seq}_{scene}_{panel}`), per-layer export | 4 |
| X3 | Movie export (H.264/MP4, ProRes MOV) with audio | ❌ | FFmpeg-based export with burn-in options (T8) | 5 |
| X4 | Animated GIF | ✅ `write_gif` | Extend with camera, transitions and burn-in | 5 |
| X5 | Export scenes as layered files for animation production | 🟡 ORA/PSD export | Per-scene ORA or PSD with layers, plus a JSON of camera and layer keyframes | 9 |
| X6 | Export captions and shot data (CSV) | ❌ | CSV with all caption fields and timing | 4 |
| X7 | Export to Toon Boom Harmony | ❌ | Out of scope (proprietary); X5 is the open alternative | — |
| X8 | Print with the native print dialog | ✅ print dialog | Reuse with X1 layouts | 4 |

## 12. Automation and extensibility

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| A1 | Scripting API for automating tasks | 🟡 MCP tools | Storyboard MCP tools covering every command in this document | Each phase |
| A2 | Customizable keyboard shortcuts | 🟡 | Storyboard commands registered in the shortcut system | 3 |
| A3 | Preferences for naming, defaults and display | 🟡 settings | Storyboard section in settings | 2 |

## Data model additions

These extend the `Shot` model in the [plan](storyboard-plan.md#data-model):

- **Hierarchy (P1, P2):** manifest records `acts`, `sequences` and `scenes`, each
  holding an ID, a name and an ordered list of child IDs. Pages stay the panels,
  and `Project::pages` order must match the flattened hierarchy (validated on
  read). Captions move from `Shot` to the panel; scene number and naming come
  from the scene.
- **Caption definitions (S2):** `Vec<CaptionField { id, name, multiline,
  print }>` per project. Panel captions are `BTreeMap<CaptionId, RichText>`.
- **Camera (C1–C4):** per scene, a keyframe track over scene time:
  `x, y, zoom, rotation, (later) fov, depth`, with `design_keyframes::Easing`
  plus Bezier handles (L4).
- **Layer animation (L3, L6):** reuse `Design::keyframes` per node, adding
  skew, pivot and depth properties.
- **Timeline (T1–T5):** project-level `tracks: Vec<Track>` where a track is
  video, audio or marker. Clips reference media embedded in the package. The
  32 MiB per-asset limit for Design media is too small for dialogue; storyboard
  audio and video get a separate limit (proposed 2 GiB per package, streamed
  from the zip, never fully loaded).
- **3D (C6–C8):** `Model3d { mesh asset, transform keyframes }` nodes, stored in
  the package as glTF binary.

Every addition uses `#[serde(default)]`, validates bounds like `PageMeta`, and is
undoable through `ProjectEditor`.

## Delivery phases

This replaces the plan's seven phases. Each phase ships on Linux, macOS and
Windows, includes MCP tools for its features (A1) and adds tests and a guide
section.

| Phase | Scope | Rows |
| --- | --- | --- |
| 1 | Foundation: project kind, hierarchy, resolution/fps, caption model, `.emu` support, Home card | P1, P2, P8, S1 |
| 2 | Board and sequence editing: naming, renumber, lock, smart add, split/join, panel inspector, custom and rich captions, find/replace, clipboard, preferences | P3–P6, V3, V4, S2, S3, S5, R3, A3 |
| 3 | Panel drawing: stage/camera views, overlays, light table, Paint tools on panels, palettes, flip view, PSD import, libraries, layouts, shortcuts | P7, V1, V2, V7, V8, C5, D1, D9–D12, L2, L7, R1, A2 |
| 4 | Drawing parity and print: vector stroke layers, shape tools, gap-closing fill, cutter and distort, stroke tools, 4-/5-point guides, ruler; PDF layouts, image and CSV export | D2–D8, L1, X1, X2, X6, X8 |
| 5 | Timeline and animatic: timeline, transitions, audio tracks, player, timing tools, burn-in, movie and GIF export | V5, V9, T1–T3, T6–T8, X3, X4 |
| 6 | Animation: camera keyframes across panels, shake, layer keyframes with skew/pivot, curves, motion paths, layer comps, animated library items | C1–C4, L3–L5, L8, R2 |
| 7 | 3D: layer depth and parallax, perspective camera, top/side views, glTF/OBJ import, snapshot to layer | V6, L6, C6–C8 |
| 8 | Script and media: voice recording, video reference track, spell check, text/Fountain/Final Draft import | T4, T5, S4, S6, S7 |
| 9 | Production: extract/merge, change tracking, compare, review layers, EDL/XML/OTIO export and conform, layered scene export | K1–K4, E1, E2, X5 |
| 10 | Shared projects through cloud sync | K5 |

Phases 1–5 give a complete board-to-animatic tool. Phases 6–9 bring animation,
3D and production pipeline features up to Storyboard Pro's level. Phase order
can change after phase 1, except that phase 6 depends on phase 5's timeline
model.

## Out of scope

- Reading or writing Storyboard Pro project files (`.sboard`) and Harmony scenes:
  proprietary and undocumented. Use PSD/ORA, PDF, EDL/XML/OTIO and images to
  exchange work instead.
- AAF until an open, maintained Rust or C reader is available and licence-compatible.
- Server-hosted database projects; Emulsion stays local-first.
- 3D modelling, rigging or character posing beyond rigid transforms.

## Open questions

1. Which Storyboard Pro version is the parity target? Rows should be checked
   against that release's documentation before each phase starts.
2. Are vector stroke layers (D2) required, or are bitmap layers enough for the
   intended users? Vector layers are the largest single drawing item.
3. Is 3D (phase 7) needed for the first full release, or can it follow?
4. Which editing applications must conform round-trip (E2)? This decides
   whether EDL and xmeml are enough or AAF is required.
5. Are there specific pipeline outputs (studio PDF layouts, naming
   conventions) that should ship as built-in presets?
