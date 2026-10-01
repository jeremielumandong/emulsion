# Storyboard parity with Toon Boom Storyboard Pro

Status: proposal, 2026-10-01. This document extends the
[Storyboard workspace plan](storyboard-plan.md) to cover the functionality of
Toon Boom Storyboard Pro, the industry reference for animation and live-action
boards. Nothing here is implemented.

**Parity target: Storyboard Pro 27** (27.0, build 25334, released 2026-06-16),
the latest release. Rows include the features added in Storyboard Pro 24, 25,
25.1, 25.2 and 27 (marked *SBP 24*, *SBP 25*, etc.); unmarked rows are long-standing
features. Sources: the official release notes for
[27](https://docs.toonboom.com/help/storyboard-pro-27/storyboard/release-notes/storyboard-pro-27-release-notes.html),
[25](https://docs.toonboom.com/help/storyboard-pro-25/storyboard/release-notes/storyboard-pro-25-release-notes.html),
[25.1](https://docs.toonboom.com/help/storyboard-pro-25/storyboard/release-notes/storyboard-pro-25-1-release-notes.html),
[25.2](https://docs.toonboom.com/help/storyboard-pro-25/storyboard/release-notes/storyboard-pro-25-2-release-notes.html)
and [24](https://docs.toonboom.com/help/storyboard-pro-24/storyboard/release-notes/storyboard-pro-24-release-notes.html),
the [Panel Timer](https://docs.toonboom.com/help/storyboard-pro-27/storyboard/timing/panel-timer.html)
page, and Toon Boom's [Ember](https://helpcentre.toonboom.com/hc/en-ca/articles/42113018317203-About-Ember-for-Storyboard-Pro)
AI overview. The list was compiled from public documentation, not a licensed
copy; when a phase starts, check its rows against the Storyboard Pro 27 manual
and record differences here. Parity means comparable **capability**. Emulsion does not copy
Storyboard Pro's interface, icons, names of proprietary features or file formats.
Proprietary formats are out of scope unless openly documented (see *Out of scope*).

Storyboard is a separate workspace and does not change the Design workspace;
see [Workspace boundary](storyboard-plan.md#workspace-boundary). Where a
**Today** cell names a Design feature, it is the shared code underneath that
Storyboard borrows. Existing code is reused, never duplicated. The Design
feature itself is not modified; generic code underneath it may be moved into a
shared module both workspaces use, as described in
[Shared modules](storyboard-plan.md#shared-modules).

Legend for **Today**: ✅ exists and can be reused · 🟡 partial foundation ·
❌ nothing yet. **Phase** refers to *Delivery phases* below; **Later** means the
deferred 3D phase.

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
| P9 | *SBP 25:* Thumbnailing page: a grid of thumbnail frames drawn on one page, then **Convert to panels** | ❌ | Thumbnail page type with a configurable grid; conversion creates one panel per cell, cropped and fitted to the camera frame | 2 |
| P8 | Project resolution and frame rate presets (film, HDTV, 4K, vertical, custom; 23.976–60 fps) | 🟡 per-page `fps` | One project resolution and frame rate; per-panel `fps` stays in sync | 1 |

## 2. Views and workspaces

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| V1 | Stage view (drawing, with overscan and field guide) | 🟡 canvas | Camera frame, overscan, field guide and safe area overlays | 3 |
| V2 | Camera view (shows only the framed shot) | ❌ | Toggle that renders only the framed area at the current time | 3 |
| V3 | Thumbnails / board view | ❌ | Board grid from the plan, with sequence and scene headers | 2 |
| V4 | Panel view (layers, captions for the current panel) | 🟡 layers panel | Panel inspector: layer list, captions, duration, transition | 2 |
| V5 | Timeline view (panels, transitions, camera, audio/video tracks) | ❌ | Track-based timeline (section 6) | 5 |
| V6 | Top and side views for 3D positioning | ❌ | Orthographic views of layer depth and 3D objects (section 5) | Later |
| V7 | Workspace presets (overview, drawing, timing, 3D) and custom layouts | 🟡 Paint layout presets | Storyboard layout presets; save and restore custom layouts | 3 |
| V8 | Light table and onion skin across panels | 🟡 onion skin across layers | Light table showing selected neighbouring panels at set opacity and tint | 3 |
| V10 | Reference view; *SBP 27:* mirror reference content without changing the source | 🟡 reference images | Reference view docked beside the stage, with flip | 3 |
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
| D12 | *SBP 24:* Symmetry drawing guides | ✅ mirror, radial | Reuse | 3 |
| D13 | *SBP 25:* Pencil retouch: brush over pencil lines to increase, decrease, replace or smooth thickness or opacity | ❌ | Retouch tool for vector stroke layers (needs D2) | 4 |
| D14 | *SBP 24:* Variable-opacity pencil (pressure, tilt, speed, fade distance) | 🟡 brush pressure dynamics | Opacity dynamics on vector strokes | 4 |
| D15 | *SBP 24:* Brush stamp randomization (count, offset) | 🟡 brush engine dynamics | Add count/offset scatter if missing | 4 |
| D16 | *SBP 25:* Import Photoshop `.abr` brushes | ✅ `emulsion-io/src/abr.rs` | Reuse | 3 |
| D17 | *SBP 24:* Paste drawing in place | 🟡 clipboard | Paste in place across panels | 3 |

## 4. Layers and layer animation

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| L1 | Bitmap, vector and group layers; opacity, visibility, lock | ✅ raster, group, locks; 🟡 vector | Vector stroke layer (D2) | 4 |
| L2 | Blending modes and masks | ✅ | Reuse | 3 |
| L3 | Layer motion keyframes: position, scale, rotation, skew, opacity, with easing | 🟡 `design_keyframes`: offset, scale, rotation, opacity, 5 easings | Storyboard layer tracks with skew and pivot, on the shared `motion` track and easing module; keyframe editing on the timeline and in the stage view | 6 |
| L4 | Function curves / velocity editing; *SBP 24:* opacity curves and opacity keyframes in the timeline | ❌ | Bezier ease editor per keyframe segment; keyframes shown on timeline clips | 6 |
| L5 | Motion paths shown on stage | ❌ | Draw the layer path with keyframe handles; drag to edit | 6 |
| L6 | Layer depth (Z) for parallax with the camera | ❌ | Per-layer depth; parallax evaluated with camera moves | Later |
| L7 | Import layered PSD into a panel | ✅ PSD/PSB import | Import into the current panel or as new panels (one per file) | 3 |
| L9 | *SBP 25:* Non-destructive effect stack on layers, with keyframed effect values | 🟡 adjustments, filters, layer styles | Keyframable effect parameters (needs the L3 track model) | 6 |
| L10 | *SBP 25.2:* Clipping mask layers | ✅ clip-to in the layer model | Reuse | 3 |
| L11 | *SBP 27:* PSD import keeps clipping masks and 24 Photoshop blend modes | ✅ `psd.rs` maps clipping and blend modes | Verify all 24 modes map; report any that don't | 3 |
| L12 | *SBP 25:* Drag across visibility, lock and onion-skin toggles to set many layers | ❌ | Drag-to-toggle in the layer panel | 3 |
| L13 | *SBP 24:* Keyframe sync mode when panel duration changes (scale or keep keyframes) | ❌ | Per-project option applied by every duration edit | 6 |
| L8 | Layer comps (save and recall layer visibility) | ❌ | Named visibility sets per panel | 6 |

## 5. Camera and 3D

3D rows (C6–C8, C9–C13, V6, L6's 3D use) are **deferred** until the 2D
workspace is complete. They stay listed so the data model leaves room for them.

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| C1 | Camera keyframes: pan, zoom (truck), rotate, with easing | ❌ (plan has start/end only) | Full camera keyframe track per scene, using the `design_keyframes` easings | 6 |
| C2 | Camera spanning several panels in a scene | ❌ | The camera belongs to the scene; keyframes are timed across its panels | 6 |
| C3 | Static camera per panel, reset camera, copy/paste camera | ❌ | Commands on the camera track | 6 |
| C4 | Camera shake and handheld presets | ❌ | Seeded noise generator applied on top of keyframes | 6 |
| C5 | Field guide, safe areas, custom overlays | ❌ | Overlay set from V1 | 3 |
| C6 | 3D-capable scenes: layers positioned in depth, 3D camera with field of view | ❌ | Perspective camera evaluating layer depth (L6); top/side views (V6) | Later |
| C7 | Import 3D models (e.g. FBX, OBJ, glTF) and pose/position them | ❌ | glTF 2.0 and OBJ import, rendered with `emulsion-gpu`; position, rotate, scale and keyframe; no rigging or modelling | Later |
| C9 | *SBP 25:* USDZ model import; multi-frame models with frame-rate interpretation | ❌ | USDZ alongside glTF | Later |
| C10 | *SBP 24:* Pose bones of FBX-compatible rigs | ❌ | Pose existing skeletons from glTF skins; no rigging | Later |
| C11 | *SBP 24:* Toon shader render with contour lines | ❌ | Cel-shaded GPU render style | Later |
| C12 | *SBP 24:* Parent 2D layers to 3D models; create layers on model surfaces | ❌ | 2D layer attached to a model transform; surface-aligned layer creation | Later |
| C13 | *SBP 24:* 3D models in 2D scenes; freeze a model to a bitmap | ❌ | Allow models without a 3D scene; render-to-layer (C8) | Later |
| C8 | Snapshot a 3D view into a drawing layer to trace over | ❌ | Render current 3D view to a new bitmap layer | Later |

## 6. Timing, animatic and sound

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| T1 | Panel durations edited in the timeline (frames or timecode) | 🟡 `duration_ms` per page | Timeline with frame and SMPTE timecode display; trim edges; ripple and roll | 5 |
| T2 | Scene transitions: cut, dissolve, wipe (edge, clock, radial), slide, fade to colour | 🟡 8 Design page transitions | Storyboard transition set (cut, dissolve, wipes, slide, fade to colour) on the shared transition renderer moved out of Design; new wipes added there; transition length edited on the timeline | 5 |
| T3 | Multiple audio tracks with waveforms, volume, mute/solo, fades | 🟡 embedded audio objects with trim, volume, loop | Project audio tracks (up to 16) with clips, waveforms, gain envelope and fades | 5 |
| T4 | Record voice directly into a track | ❌ | Microphone capture to WAV on Linux, macOS and Windows | 7 |
| T5 | Import video as a reference track | 🟡 local video in Design | Video track with frame-accurate scrubbing (FFmpeg decode) | 7 |
| T6 | Real-time playback with audio scrubbing; play range and loop | 🟡 Present mode | Animatic player at project fps, dropping frames rather than drifting | 5 |
| T7 | Timing tools: fit selection to duration, conform panels to audio markers | ❌ | Fit to duration; markers on audio tracks; snap panels to markers | 5 |
| T9 | *SBP 27:* Panel Timer: tap timing live while performing; create new panels or apply to the selection; review timings in a table before applying; optionally record audio into the timeline; works with thumbnail pages | ❌ | Panel Timer view (tap key, review table, apply as one undo step); audio capture arrives with T4 | 5 |
| T10 | *SBP 24:* Retime a sequence proportionally; Shift+drag to change selection duration; duration overlay on the timeline | ❌ | Timeline commands | 5 |
| T11 | *SBP 24/25.2:* Rename clips; audio library with folders; preview with zoomable waveform and draggable in/out points | ❌ | Audio library panel and clip preview | 5 |
| T12 | *SBP 24:* Audio effects with keyframes | ❌ | Gain, EQ and fades as keyframable clip effects | 7 |
| T13 | *SBP 24:* Choose the audio input device | ❌ | Device picker for T4 and T9 recording | 7 |
| T8 | Timecode burn-in and overlays (scene, panel, timecode) on playback and export | ❌ | Overlay options shared by player and export | 5 |

## 7. Captions and script

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| S1 | Caption fields per panel: action, dialogue, slugging, notes | 🟡 `speaker_notes` | Caption fields from the plan's `Shot`, with slugging added | 1 |
| S2 | Custom caption fields defined per project | ❌ | Project-level caption definitions (name, multi-line, export visibility) | 2 |
| S3 | Rich text in captions (bold, italic, colour, size) | 🟡 shared rich text model | Reuse the text model in captions | 2 |
| S4 | Spell checking | ❌ | Platform spell checker where available, otherwise a bundled open dictionary | 7 |
| S5 | Find and replace across captions | ❌ | Project-wide find/replace, with undo | 2 |
| S6 | Import script text and split it into panels | ❌ | Import plain text and Fountain; split per paragraph or dialogue block into panels and captions | 7 |
| S7 | Import Final Draft scripts; *SBP 24:* start a new project from a Final Draft file | ❌ | Final Draft `.fdx` is XML; import scene headings, action and dialogue; offered on the New storyboard dialog | 7 |

## 8. Library and reuse

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| R1 | Library of reusable drawings, characters, props, backgrounds | 🟡 Design components, brand assets | Project and personal libraries of layers and panels; drag onto a panel | 3 |
| R2 | Templates of panels or scenes with animation | ❌ | Save a panel or scene, with keyframes and camera, as a library item | 6 |
| R3 | Copy and paste panels, layers and cameras between projects | 🟡 clipboard | Storyboard clipboard formats for panels, scenes and camera tracks | 2 |

## 9. Collaboration and review

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| K1 | Extract a range of scenes for another artist and merge it back | ❌ | **Extract** a range to a new `.emu` with a source ID; **Merge** replaces the range and reports conflicts | 8 |
| K2 | Change tracking: mark new or modified panels since a version | 🟡 branchable history | Compare against a saved commit; show new, changed and deleted panels on the board | 8 |
| K3 | Compare two versions side by side | 🟡 history graph | Panel-by-panel compare view | 8 |
| K4 | Review notes and annotation layers that don't print | ❌ | Non-printing review layer type and per-panel review status | 8 |
| K5 | Shared projects / database workflows | 🟡 cloud sync plan | Use the cloud sync plan for file sharing; no server-side database | 10 |

## 10. Editorial conform

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| E1 | Export the animatic to editing software (EDL, AAF, Final Cut XML) | ❌ | Export CMX 3600 EDL, FCP 7 XML (xmeml) and OpenTimelineIO, with per-panel media | 8 |
| E2 | Import an edit back and conform panels to it; *SBP 25.1:* convert or keep frame rate on import | ❌ | Read EDL, xmeml and OpenTimelineIO; match clips to panels by name; apply durations, order and audio; frame-rate conversion choice | 8 |
| E3 | AAF interchange with Avid; *SBP 27:* keeps Avid bin clip names | ❌ | Needed for Avid round-trips. Needs a compound-file (structured storage) reader; see open questions | — |

## 11. Export, print and publishing

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| X1 | PDF export profiles (panels per page, captions, header/footer, logos); *SBP 24/25:* live preview, option search, image fitting, frame thickness, second panel header, captions left/right/below with optional frames, header alignment, camera-move frame and arrow thickness | 🟡 print dialog contact sheets | Storyboard PDF layout profiles with all of these options, a live preview and option search; saved and shareable | 4 |
| X2 | Image export per panel or per layer | 🟡 page export | Naming tokens (`{seq}_{scene}_{panel}`), per-layer export | 4 |
| X3 | Movie export (H.264/MP4, ProRes MOV) with audio | ❌ | FFmpeg-based export with burn-in options (T8) | 5 |
| X4 | Animated GIF | ✅ `write_gif` | Extend with camera, transitions and burn-in | 5 |
| X5 | Export scenes as layered files for animation production | 🟡 ORA/PSD export | Per-scene ORA or PSD with layers, plus a JSON of camera and layer keyframes | 8 |
| X6 | Export captions and shot data (CSV) | ❌ | CSV with all caption fields and timing | 4 |
| X9 | *SBP 24:* Expand the render area beyond the camera or to all panels' artwork | ❌ | Render-area option on image and movie export | 5 |
| X10 | *SBP 24:* Import vector files (PDF, AI, SVG), one panel per file or artboard | 🟡 SVG import | Add PDF and AI (PDF-compatible) import, one panel per page/artboard | 7 |
| X7 | Export to Toon Boom Harmony | ❌ | Out of scope (proprietary); X5 is the open alternative | — |
| X8 | Print with the native print dialog | ✅ print dialog | Reuse with X1 layouts | 4 |

## 12. Automation and extensibility

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| A1 | Scripting API for automating tasks | 🟡 MCP tools | Storyboard MCP tools covering every command in this document | Each phase |
| A2 | Customizable keyboard shortcuts | 🟡 | Storyboard commands registered in the shortcut system | 3 |
| A3 | Preferences for naming, defaults and display; *SBP 24:* searchable preferences | 🟡 settings | Storyboard section in settings, with search | 2 |
| A4 | *SBP 24:* OpenColorIO colour management (ACES) | 🟡 ICC management | OCIO config support for viewing and export | 8 |

## 13. AI assistance

Storyboard Pro's AI features (*Ember*, SBP 24.1 and later) use hosted providers.
Emulsion keeps AI optional: local models where it has them, the assistant through a
connected coding CLI, and any hosted provider only when the user configures it.
Every AI result is an ordinary undoable edit.

| ID | Capability | Today | Emulsion work | Phase |
| --- | --- | --- | --- | --- |
| AI1 | AI masking (select a subject) | ✅ quick select, matte models (`emulsion-ai` `sam`, `matte`) | Reuse on panels | 9 |
| AI2 | Expand image; *SBP 25.2:* optional prompt | ✅ Expand (Photo **Enhance**) | Prompt option where the model supports it | 9 |
| AI3 | Increase image resolution | ✅ `emulsion-ai` `upscale` | Reuse | 9 |
| AI4 | *SBP 25.2:* Generative fill from a text prompt in a masked area | 🟡 `inpaint`, `generate` | Prompted fill inside a selection | 9 |
| AI5 | *SBP 25.2:* Batch AI image operations | 🟡 batch processing | Run AI1–AI4 over selected panels | 9 |
| AI6 | Analyse a script with AI into scenes, panels and captions | 🟡 assistant and MCP | MCP tools from S6/S7 so the assistant can break down a script | 9 |
| AI7 | Generate scene lengths with AI | ❌ | Estimate durations from dialogue and action text; offline word-rate estimate as the fallback | 9 |
| AI8 | Character voices for a scratch dialogue track | ❌ | Text-to-speech via a user-configured provider; nothing sent without one | 9 |
| AI9 | *SBP 25.1:* Adjust dialogue intonation with AI (record, enhance or generate dialogue audio) | ❌ | Same provider as AI8; results land as new clips, originals kept | 9 |

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
  `x, y, zoom, rotation, (later) fov, depth`, with the shared `motion` easing
  plus Bezier handles (L4), in storyboard modules.
- **Layer animation (L3, L6):** per-node tracks (position, scale, rotation,
  skew, pivot, opacity, later depth) stored with the panel and evaluated by the
  shared `motion` module moved out of `design_keyframes.rs`. Design's own
  keyframe data and behaviour stay as they are.
- **Timeline (T1–T5):** built in the shared `timeline` module. Project-level `tracks: Vec<Track>` where a track is
  video, audio or marker. Clips reference media embedded in the package. The
  32 MiB per-asset limit for Design media is too small for dialogue; storyboard
  audio and video get a separate limit (proposed 2 GiB per package, streamed
  from the zip, never fully loaded).
- **3D (deferred; C6–C13):** `Model3d { mesh asset, transform keyframes }` nodes,
  stored in the package as glTF binary. Not built until the deferred phase, but
  layer depth and camera fields reserve room for it.

Every addition uses `#[serde(default)]`, validates bounds like `PageMeta`, and is
undoable through `ProjectEditor`.

## Delivery phases

This replaces the plan's seven phases. Each phase ships on Linux, macOS and
Windows, includes MCP tools for its features (A1) and adds tests and a guide
section. 3D is deferred to the last phase.

| Phase | Scope | Rows |
| --- | --- | --- |
| 1 | Foundation: project kind, hierarchy, resolution/fps, caption model, `.emu` support, Home card | P1, P2, P8, S1 |
| 2 | Board and sequence editing: naming, renumber, lock, smart add, split/join, thumbnail pages, panel inspector, custom and rich captions, find/replace, clipboard, preferences | P3–P6, P9, V3, V4, S2, S3, S5, R3, A3 |
| 3 | Panel drawing: stage/camera/reference views, overlays, light table, Paint tools on panels, palettes, flip view, `.abr` brushes, paste in place, PSD import with clipping and blend modes, clipping masks, layer toggles, libraries, layouts, shortcuts | P7, V1, V2, V7, V8, V10, C5, D1, D9–D12, D16, D17, L2, L7, L10–L12, R1, A2 |
| 4 | Drawing parity and print: vector stroke layers, shape tools, gap-closing fill, cutter and distort, stroke tools, pencil retouch, opacity pencil, brush scatter, 4-/5-point guides, ruler; PDF profiles, image and CSV export | D2–D8, D13–D15, L1, X1, X2, X6, X8 |
| 5 | Timeline and animatic: timeline, transitions, audio tracks and library, player, timing tools, Panel Timer, retiming, burn-in, render area, movie and GIF export | V5, V9, T1–T3, T6–T11, X3, X4, X9 |
| 6 | Animation: camera keyframes across panels, shake, layer keyframes with skew/pivot, curves, motion paths, effect stack keyframes, keyframe sync, layer comps, animated library items | C1–C4, L3–L5, L8, L9, L13, R2 |
| 7 | Script and media: voice recording and input devices, audio effects, video reference track, spell check, text/Fountain/Final Draft import, PDF/AI vector import | T4, T5, T12, T13, S4, S6, S7, X10 |
| 8 | Production: extract/merge, change tracking, compare, review layers, EDL/XML/OTIO export and conform, layered scene export, OpenColorIO | K1–K4, E1, E2, X5, A4 |
| 9 | AI assistance: masking, expand, upscale, generative fill, batch, script breakdown, scene lengths, scratch voices, dialogue enhancement | AI1–AI9 |
| 10 | Shared projects through cloud sync | K5 |
| Later | 3D: layer depth and parallax, perspective camera, top/side views, glTF/OBJ/USDZ import, bone posing, toon shader, 2D-on-3D layers, snapshot to layer | V6, L6, C6–C13 |

Phases 1–5 give a complete board-to-animatic tool. Phases 6–10 reach
Storyboard Pro 27 parity for 2D work. The deferred 3D phase completes it. Phase
order can change after phase 1, except that phase 6 depends on phase 5's timeline
model and T9's audio capture on phase 7.

## Out of scope

- Reading or writing Storyboard Pro project files (`.sboard`, `.sbpz`) and Harmony
  scenes, including *Export to Harmony*: proprietary and undocumented. Use PSD/ORA,
  PDF, EDL/XML/OTIO and images to exchange work instead.
- Licensing and license-server features.
- Server-hosted database projects; Emulsion stays local-first.
- 3D modelling and rigging. Posing existing rigs is in the deferred 3D phase.

## Open questions

1. Are vector stroke layers (D2) required, or are bitmap layers enough for the
   intended users? Vector layers are the largest single drawing item, and pencil
   retouch (D13) depends on them.
2. Which editing applications must conform round-trip (E2, E3)? Storyboard Pro
   27 round-trips with Avid through AAF; matching that needs an AAF reader and
   writer, which would be the largest interchange item.
3. Which hosted providers, if any, should AI8 and AI9 (voices and dialogue
   audio) support? Without one those rows stay unavailable.
4. Are there specific pipeline outputs (studio PDF layouts, naming
   conventions) that should ship as built-in presets?
