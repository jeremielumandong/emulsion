//! The scene camera (C1–C4): the Camera tool draws the camera's frame over
//! the Stage at the playhead, with handles to pan (drag inside), zoom (drag
//! a corner) and turn it (drag just outside a corner); a finished drag sets
//! or updates the scene's key at the playhead. Commands add, delete and step
//! through keys, choose a key's easing, hold the camera for a panel, reset,
//! copy and paste a scene's camera, and set shake. The Timeline's camera row
//! shows each scene's keys, dragged to retime and clicked to jump there.
//!
//! The camera belongs to the scene, so its keys count frames from the
//! scene's first panel and span all its panels. Every change is one Undo
//! step through `edit_board`; the core's keyframe sync keeps keys in step
//! with panel durations.
use super::storyboard_timeline::{TimelineDrag, TimingEdit};
use super::*;
use emulsion_core::motion::{Curve, Easing};
use emulsion_core::project::PageId;
use emulsion_core::storyboard::{CameraKey, CameraState, GroupId, SceneCamera, Shake, Storyboard};
use emulsion_core::storyboard_motion::ZOOM;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};

#[cfg(test)]
#[path = "storyboard_camera_tests.rs"]
mod tests;

/// Grab distance for a corner (zoom), in screen pixels.
const GRAB_PX: f64 = 8.;
/// Outside a corner up to this far turns the camera.
const ROTATE_PX: f64 = 30.;
/// The camera frame's colour, as on printed boards.
const CAMERA_INK: u32 = 0xE03C3C;

/// View state for the camera: whether the Camera tool is on, and the
/// in-process clipboard for Copy / Paste Camera.
#[derive(Default)]
pub(crate) struct CameraUi {
    pub(crate) editing: bool,
    pub(crate) clipboard: Option<SceneCamera>,
    /// The ease curve editor shows under the Camera tool's bar.
    pub(crate) curve_open: bool,
}

/// What a Stage drag on the camera frame changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CameraGrab {
    Pan,
    Zoom,
    Rotate,
}

/// A drag on the camera frame: previewed live, one key on release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CameraDrag {
    pub(crate) grab: CameraGrab,
    scene: GroupId,
    /// The key's frame, from the scene's start.
    frame: u64,
    /// Where the pointer went down, in document pixels.
    start: (f64, f64),
    from: CameraState,
    pub(crate) state: CameraState,
}

/// Where the Stage's camera stands: the active panel's scene, at the
/// playhead while it is inside the panel, else at the panel's first frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CameraSpot {
    pub(crate) panel: PageId,
    pub(crate) scene: GroupId,
    /// Animatic frame.
    pub(crate) frame: u64,
    pub(crate) panel_start: u64,
    pub(crate) scene_start: u64,
    pub(crate) scene_frames: u64,
}

impl CameraSpot {
    /// The spot's frame from the scene's start, as keys count.
    pub(crate) fn local(&self) -> u64 {
        self.frame - self.scene_start
    }
}

/// What the Stage paints for the camera, in document pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CameraPaint {
    /// The camera's frame, and whether it has handles (the Camera tool).
    pub(crate) frame: Option<([(f64, f64); 4], bool)>,
    /// Camera view: outside this frame is masked.
    pub(crate) mask: Option<[(f64, f64); 4]>,
}

/// Set the key at `frame` (scene frames) to `state`, adding it when there
/// is none; an existing key keeps its easing.
pub(crate) fn set_key(camera: &mut SceneCamera, frame: u64, state: CameraState) {
    match camera.keys.binary_search_by_key(&frame, |k| k.frame) {
        Ok(i) => {
            let key = &mut camera.keys[i];
            key.x = state.x;
            key.y = state.y;
            key.zoom = state.zoom;
            key.rotation = state.rotation;
        }
        Err(i) => camera.keys.insert(i, CameraKey::at(frame, state)),
    }
}

/// The keyed camera at animatic `frame`: shake left out, so edits and
/// handles hold still.
pub(crate) fn keyed_camera(board: &Storyboard, layout: &[PageId], frame: u64) -> CameraState {
    let mut state = board.camera_at(layout, frame as f64);
    let shake = board.animatic_frame(layout, frame).and_then(|at| {
        let scene = board.panels[&at.panel].scene;
        let shake = board.cameras.get(&scene)?.shake?;
        let start = board.scene_span(layout, scene)?.0;
        Some((shake, frame.saturating_sub(start)))
    });
    if let Some((shake, local)) = shake {
        let seconds = board.settings.frame_rate.frames_to_seconds(1) * local as f64;
        let (dx, dy, dr) = shake.offset(seconds);
        state.x -= dx;
        state.y -= dy;
        state.rotation -= dr;
    }
    state
}

/// Shake from "amplitude, tilt, frequency, seed" (the seed may be left
/// out, keeping `seed`).
pub(crate) fn parse_shake(text: &str, seed: u64) -> Result<Shake, String> {
    let parts: Vec<&str> = text
        .split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let number = |s: &str| s.parse::<f64>().ok().filter(|v| v.is_finite());
    let usage =
        "Type amplitude (px), tilt (°), wobbles per second and a seed, such as 4, 0.4, 1.2, 1.";
    let (Some(amplitude), Some(rotation), Some(frequency)) = (
        parts.first().and_then(|s| number(s)),
        parts.get(1).and_then(|s| number(s)),
        parts.get(2).and_then(|s| number(s)),
    ) else {
        return Err(usage.into());
    };
    let seed = match parts.get(3) {
        Some(s) => s.parse::<u64>().map_err(|_| usage.to_string())?,
        None => seed,
    };
    if parts.len() > 4 {
        return Err(usage.into());
    }
    let shake = Shake {
        amplitude,
        rotation,
        frequency,
        seed,
    };
    shake.validate()?;
    Ok(shake)
}

/// The shake as the dialog shows it.
fn shake_text(shake: &Shake) -> String {
    format!(
        "{}, {}, {}, {}",
        shake.amplitude, shake.rotation, shake.frequency, shake.seed
    )
}

/// Whether screen point `p` lies inside the convex quad `q`.
fn inside(q: &[(f64, f64); 4], p: (f64, f64)) -> bool {
    let cross =
        |a: (f64, f64), b: (f64, f64)| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    let signs: Vec<f64> = (0..4).map(|i| cross(q[i], q[(i + 1) % 4])).collect();
    signs.iter().all(|s| *s >= 0.) || signs.iter().all(|s| *s <= 0.)
}

/// Paint the camera frame and the Camera view mask, under the other
/// overlays.
pub(super) fn paint_camera(
    paint: &CameraPaint,
    view: &View,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) {
    let to_screen = |p: (f64, f64)| {
        let s = view.doc_to_screen(p, &bounds);
        point(px(s.0 as f32), px(s.1 as f32))
    };
    if let Some(quad) = &paint.mask {
        // The canvas with the shot cut out (the inner outline runs the
        // other way, so it is a hole under either fill rule).
        let (o, s) = (bounds.origin, bounds.size);
        let mut path = PathBuilder::fill();
        path.move_to(o);
        path.line_to(point(o.x + s.width, o.y));
        path.line_to(point(o.x + s.width, o.y + s.height));
        path.line_to(point(o.x, o.y + s.height));
        path.close();
        let pts: Vec<_> = quad.iter().map(|p| to_screen(*p)).collect();
        let clockwise = {
            let a = (pts[1].x - pts[0].x, pts[1].y - pts[0].y);
            let b = (pts[2].x - pts[1].x, pts[2].y - pts[1].y);
            f32::from(a.0) * f32::from(b.1) - f32::from(a.1) * f32::from(b.0) > 0.
        };
        let order: [usize; 4] = if clockwise {
            [0, 3, 2, 1]
        } else {
            [0, 1, 2, 3]
        };
        path.move_to(pts[order[0]]);
        for i in &order[1..] {
            path.line_to(pts[*i]);
        }
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, hsla(0., 0., 0.06, 0.9));
        }
    }
    if let Some((quad, handles)) = &paint.frame {
        let ink: Hsla = rgb(CAMERA_INK).into();
        let pts: Vec<_> = quad.iter().map(|p| to_screen(*p)).collect();
        let mut path = PathBuilder::stroke(px(if *handles { 2. } else { 1. }));
        path.add_polygon(&pts, true);
        if let Ok(path) = path.build() {
            window.paint_path(path, ink);
        }
        if *handles {
            let centre = to_screen(((quad[0].0 + quad[2].0) / 2., (quad[0].1 + quad[2].1) / 2.));
            let mut cross = PathBuilder::stroke(px(1.));
            cross.move_to(point(centre.x - px(6.), centre.y));
            cross.line_to(point(centre.x + px(6.), centre.y));
            cross.move_to(point(centre.x, centre.y - px(6.)));
            cross.line_to(point(centre.x, centre.y + px(6.)));
            if let Ok(path) = cross.build() {
                window.paint_path(path, ink);
            }
            let half = px(GRAB_PX as f32 / 2. + 1.);
            for c in &pts {
                window.paint_quad(
                    fill(
                        Bounds::new(point(c.x - half, c.y - half), size(half * 2., half * 2.)),
                        gpui_kit::white(),
                    )
                    .border_widths(px(1.5))
                    .border_color(ink),
                );
            }
        }
    }
}

impl EditorView {
    fn camera_layout(&self) -> Vec<PageId> {
        self.editor.page_list().iter().map(|m| m.id).collect()
    }

    /// Where the camera stands on the Stage, while it shows a panel that
    /// plays (not a thumbnail sheet).
    pub(crate) fn camera_spot(&self) -> Option<CameraSpot> {
        if self.board_open() {
            return None;
        }
        let board = self.editor.storyboard()?;
        let layout = self.camera_layout();
        let panel = self.editor.active_page();
        let panel_start = board
            .panel_starts(&layout)
            .into_iter()
            .find(|(id, _)| *id == panel)?
            .1;
        let frames = u64::from(board.panels[&panel].frames);
        let at = self.transport.frame;
        let frame = if (panel_start..panel_start + frames).contains(&at) {
            at
        } else {
            panel_start
        };
        let scene = board.panels[&panel].scene;
        let (scene_start, scene_frames) = board.scene_span(&layout, scene)?;
        Some(CameraSpot {
            panel,
            scene,
            frame,
            panel_start,
            scene_start,
            scene_frames,
        })
    }

    /// The Camera tool is on and the Stage shows a panel that plays.
    pub(crate) fn camera_tool_on(&self) -> bool {
        self.camera_ui.editing && !self.player.showing && self.camera_spot().is_some()
    }

    pub(crate) fn toggle_camera_tool(&mut self, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            return;
        }
        self.camera_ui.editing = !self.camera_ui.editing;
        let message = if !self.camera_ui.editing {
            "Camera tool off."
        } else if self.camera_spot().is_some() {
            "Camera tool: drag inside the frame to pan, a corner to zoom, just outside a corner to turn. Each drag sets a key at the playhead."
        } else {
            "Camera tool on: open a panel on the Stage (not a thumbnail sheet) to frame its scene."
        };
        self.set_status(message, false, cx);
        self.notify_canvas(cx);
        cx.notify();
    }

    /// The camera the Stage shows: the drag's while dragging, else the
    /// keyed camera at the spot.
    pub(crate) fn stage_camera(&self) -> Option<(CameraSpot, CameraState)> {
        let spot = self.camera_spot()?;
        if let Some(Drag::Camera(drag)) = &self.drag {
            return Some((spot, drag.state));
        }
        let board = self.editor.storyboard()?;
        Some((spot, keyed_camera(board, &self.camera_layout(), spot.frame)))
    }

    /// What the Stage paints for the camera: the frame while the tool is on
    /// or the scene has a camera, and the mask in Camera view.
    pub(crate) fn camera_paint(&self) -> CameraPaint {
        let Some(board) = self.editor.storyboard() else {
            return CameraPaint::default();
        };
        let Some((spot, state)) = self.stage_camera() else {
            return CameraPaint::default();
        };
        let tool = self.camera_tool_on();
        let has_camera = board.cameras.contains_key(&spot.scene);
        if !tool && !has_camera {
            return CameraPaint::default();
        }
        let corners = board.camera_corners(state);
        let camera_view = self.camera_view();
        CameraPaint {
            frame: (tool || !camera_view).then_some((corners, tool)),
            mask: camera_view.then_some(corners),
        }
    }

    /// Change the camera of `scene` as one Undo step; a camera left with
    /// no keys and no shake is removed.
    fn edit_camera(
        &mut self,
        scene: GroupId,
        edit: impl FnOnce(&mut SceneCamera) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let done = self.edit_board(
            |board| {
                let mut camera = board.cameras.remove(&scene).unwrap_or_default();
                edit(&mut camera)?;
                if !camera.is_empty() {
                    board.cameras.insert(scene, camera);
                }
                Ok(())
            },
            cx,
        );
        self.notify_canvas(cx);
        done
    }

    /// The spot, or a status saying where the camera works.
    fn camera_spot_or_say(&mut self, cx: &mut Context<Self>) -> Option<CameraSpot> {
        let spot = self.camera_spot();
        if spot.is_none() && self.editor.storyboard().is_some() {
            self.set_status(
                "Open a panel on the Stage (not a thumbnail sheet) to edit its scene's camera.",
                true,
                cx,
            );
        }
        spot
    }

    /// Key the camera as it is at the playhead.
    pub(crate) fn camera_add_key(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(spot) = self.camera_spot_or_say(cx) else {
            return false;
        };
        let Some(board) = self.editor.storyboard() else {
            return false;
        };
        let state = keyed_camera(board, &self.camera_layout(), spot.frame);
        self.edit_camera(
            spot.scene,
            |camera| {
                set_key(camera, spot.local(), state);
                Ok(())
            },
            cx,
        )
    }

    /// Remove the camera key at the playhead.
    pub(crate) fn camera_delete_key(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(spot) = self.camera_spot_or_say(cx) else {
            return false;
        };
        self.edit_camera(
            spot.scene,
            |camera| {
                let i = camera
                    .keys
                    .iter()
                    .position(|k| k.frame == spot.local())
                    .ok_or("There is no camera key at the playhead.")?;
                camera.keys.remove(i);
                Ok(())
            },
            cx,
        )
    }

    /// Move the playhead to the scene's next or previous camera key.
    pub(crate) fn camera_step_key(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let Some(spot) = self.camera_spot_or_say(cx) else {
            return false;
        };
        let keys: Vec<u64> = self
            .editor
            .storyboard()
            .and_then(|b| b.cameras.get(&spot.scene))
            .map(|c| c.keys.iter().map(|k| k.frame).collect())
            .unwrap_or_default();
        let local = spot.local();
        let target = if forward {
            keys.into_iter().find(|f| *f > local)
        } else {
            keys.into_iter().rev().find(|f| *f < local)
        };
        let Some(target) = target else {
            self.set_status(
                if forward {
                    "No camera key after the playhead in this scene."
                } else {
                    "No camera key before the playhead in this scene."
                },
                false,
                cx,
            );
            return false;
        };
        let last = spot.scene_start + spot.scene_frames.saturating_sub(1);
        self.timeline_seek((spot.scene_start + target).min(last), cx);
        self.notify_canvas(cx);
        true
    }

    /// The key whose move the playhead is in: the key at or before it.
    fn camera_current_key(&self, spot: &CameraSpot) -> Option<CameraKey> {
        self.editor
            .storyboard()?
            .cameras
            .get(&spot.scene)?
            .keys
            .iter()
            .rev()
            .find(|k| k.frame <= spot.local())
            .copied()
    }

    /// Ease the move from the key at (or before) the playhead.
    pub(crate) fn camera_set_easing(&mut self, easing: Easing, cx: &mut Context<Self>) -> bool {
        self.camera_set_ease(easing, None, cx)
    }

    /// Ease the move from the key at (or before) the playhead with a
    /// preset, or on a bezier `curve`.
    pub(crate) fn camera_set_ease(
        &mut self,
        easing: Easing,
        curve: Option<Curve>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(spot) = self.camera_spot_or_say(cx) else {
            return false;
        };
        let Some(key) = self.camera_current_key(&spot) else {
            self.set_status("Add a camera key first.", true, cx);
            return false;
        };
        self.edit_camera(
            spot.scene,
            |camera| {
                if let Some(k) = camera.keys.iter_mut().find(|k| k.frame == key.frame) {
                    k.easing = easing;
                    k.curve = curve;
                }
                Ok(())
            },
            cx,
        )
    }

    /// Remove the scene's camera keys (shake stays until removed).
    pub(crate) fn camera_reset(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(spot) = self.camera_spot_or_say(cx) else {
            return false;
        };
        self.edit_camera(
            spot.scene,
            |camera| {
                camera.keys.clear();
                Ok(())
            },
            cx,
        )
    }

    /// Hold the camera still through the active panel: keys at its first
    /// and last frames with the camera as it enters, none between.
    pub(crate) fn camera_static_panel(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(spot) = self.camera_spot_or_say(cx) else {
            return false;
        };
        let Some(board) = self.editor.storyboard() else {
            return false;
        };
        let state = keyed_camera(board, &self.camera_layout(), spot.panel_start);
        let first = spot.panel_start - spot.scene_start;
        let last = first + u64::from(board.panels[&spot.panel].frames).saturating_sub(1);
        self.edit_camera(
            spot.scene,
            |camera| {
                camera.keys.retain(|k| k.frame < first || k.frame > last);
                set_key(camera, first, state);
                set_key(camera, last, state);
                Ok(())
            },
            cx,
        )
    }

    /// Copy the active scene's camera (keys and shake).
    pub(crate) fn camera_copy(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(spot) = self.camera_spot_or_say(cx) else {
            return false;
        };
        let camera = self
            .editor
            .storyboard()
            .and_then(|b| b.cameras.get(&spot.scene).cloned())
            .unwrap_or_default();
        self.camera_ui.clipboard = Some(camera);
        self.set_status("Camera copied.", false, cx);
        cx.notify();
        true
    }

    /// Replace the active scene's camera with the copied one.
    pub(crate) fn camera_paste(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(spot) = self.camera_spot_or_say(cx) else {
            return false;
        };
        let Some(copied) = self.camera_ui.clipboard.clone() else {
            self.set_status("Copy a scene's camera first.", true, cx);
            return false;
        };
        self.edit_camera(
            spot.scene,
            |camera| {
                *camera = copied;
                Ok(())
            },
            cx,
        )
    }

    /// Set (or with `None`, remove) the scene's camera shake.
    pub(crate) fn camera_set_shake(
        &mut self,
        shake: Option<Shake>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(spot) = self.camera_spot_or_say(cx) else {
            return false;
        };
        if let Some(Err(error)) = shake.map(|s| s.validate()) {
            self.set_status(error, true, cx);
            return false;
        }
        self.edit_camera(
            spot.scene,
            |camera| {
                camera.shake = shake;
                Ok(())
            },
            cx,
        )
    }

    /// Edit the shake's amplitude, tilt, frequency and seed.
    pub(crate) fn camera_shake_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(spot) = self.camera_spot_or_say(cx) else {
            return;
        };
        let shake = self
            .editor
            .storyboard()
            .and_then(|b| b.cameras.get(&spot.scene))
            .and_then(|c| c.shake)
            .unwrap_or(Shake::PRESETS[0].1);
        self.timeline_text_dialog(
            "Camera shake",
            "Amplitude (px), tilt (°), wobbles per second and seed, separated by commas",
            shake_text(&shake),
            "Set",
            move |this, text, cx| match parse_shake(&text, shake.seed) {
                Ok(shake) => this.camera_set_shake(Some(shake), cx),
                Err(error) => {
                    this.set_status(error, true, cx);
                    false
                }
            },
            window,
            cx,
        );
    }

    // ── Stage pointer ──

    /// What the pointer at `pos` would grab on a camera frame `state`.
    pub(crate) fn camera_hit(&self, pos: Point<Pixels>, state: CameraState) -> Option<CameraGrab> {
        let board = self.editor.storyboard()?;
        let b = self.canvas_bounds()?;
        let corners = board
            .camera_corners(state)
            .map(|c| self.view.doc_to_screen(c, &b));
        let p = (f32::from(pos.x) as f64, f32::from(pos.y) as f64);
        let near = |r: f64| corners.iter().any(|c| (c.0 - p.0).hypot(c.1 - p.1) <= r);
        if near(GRAB_PX) {
            Some(CameraGrab::Zoom)
        } else if inside(&corners, p) {
            Some(CameraGrab::Pan)
        } else if near(ROTATE_PX) {
            Some(CameraGrab::Rotate)
        } else {
            None
        }
    }

    /// A left press on the Stage while the Camera tool is on: grab the
    /// frame. The tool owns the press even off the frame, so it never
    /// draws.
    pub(crate) fn camera_down(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) -> bool {
        if !self.camera_tool_on() {
            return false;
        }
        let Some((spot, state)) = self.stage_camera() else {
            return false;
        };
        if let (Some(grab), Some(start)) = (
            self.camera_hit(e.position, state),
            self.doc_point(e.position),
        ) {
            self.drag = Some(Drag::Camera(CameraDrag {
                grab,
                scene: spot.scene,
                frame: spot.local(),
                start,
                from: state,
                state,
            }));
            self.notify_canvas(cx);
        }
        true
    }

    pub(crate) fn camera_drag_move(&mut self, d: (f64, f64), cx: &mut Context<Self>) {
        let snap = self.drag_shift;
        let Some(Drag::Camera(drag)) = &mut self.drag else {
            return;
        };
        let (from, start) = (drag.from, drag.start);
        let centre = (from.x, from.y);
        let mut state = from;
        match drag.grab {
            CameraGrab::Pan => {
                state.x = from.x + d.0 - start.0;
                state.y = from.y + d.1 - start.1;
            }
            CameraGrab::Zoom => {
                let r0 = (start.0 - centre.0).hypot(start.1 - centre.1);
                let r1 = (d.0 - centre.0).hypot(d.1 - centre.1).max(1e-6);
                state.zoom = (from.zoom * r0 / r1).clamp(*ZOOM.start(), *ZOOM.end());
            }
            CameraGrab::Rotate => {
                let angle = |p: (f64, f64)| (p.1 - centre.1).atan2(p.0 - centre.0);
                let mut delta = (angle(d) - angle(start)).to_degrees();
                delta = (delta + 180.).rem_euclid(360.) - 180.;
                let mut rotation = from.rotation + delta;
                if snap {
                    rotation = (rotation / 15.).round() * 15.;
                }
                state.rotation = rotation;
            }
        }
        drag.state = state;
        self.notify_canvas(cx);
        cx.notify();
    }

    /// Release: the dragged camera becomes the key at the playhead.
    pub(crate) fn camera_drag_end(&mut self, drag: CameraDrag, cx: &mut Context<Self>) {
        if drag.state == drag.from {
            self.notify_canvas(cx);
            return;
        }
        self.edit_camera(
            drag.scene,
            |camera| {
                set_key(camera, drag.frame, drag.state);
                Ok(())
            },
            cx,
        );
    }

    // ── Controls ──

    /// The Camera tool's bar over the Stage.
    pub(crate) fn camera_controls(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.camera_tool_on() {
            return None;
        }
        let (spot, state) = self.stage_camera()?;
        let board = self.editor.storyboard()?;
        let camera = board.cameras.get(&spot.scene);
        let keys = camera.map_or(0, |c| c.keys.len());
        let on_key = camera.is_some_and(|c| c.keys.iter().any(|k| k.frame == spot.local()));
        let shake = camera.and_then(|c| c.shake);
        let current = self.camera_current_key(&spot);
        let easing = current.map(|k| k.easing);
        let curve = (self.camera_ui.curve_open)
            .then_some(current)
            .flatten()
            .map(|key| {
                let change: super::storyboard_curve_editor::EaseChange = Rc::new(
                    |e: &mut EditorView,
                     easing: Easing,
                     curve: Option<Curve>,
                     cx: &mut Context<EditorView>| {
                        e.camera_set_ease(easing, curve, cx);
                    },
                );
                div()
                    .id("storyboard-camera-curve")
                    .occlude()
                    .p_1()
                    .rounded(px(6.))
                    .bg(p.panel.opacity(0.94))
                    .border_1()
                    .border_color(p.line)
                    .child(self.ease_curve_editor(
                        "camera-ease-curve",
                        key.easing,
                        key.curve,
                        false,
                        change,
                        p,
                        cx,
                    ))
            });
        let curve_open = self.camera_ui.curve_open;
        let owner = cx.weak_entity();
        let button = |id: &'static str, label: &str, tip: &'static str| {
            Button::new(id)
                .label(label.to_string())
                .tooltip(tip)
                .xsmall()
                .ghost()
        };
        let summary = format!(
            "Scene frame {} · {} key{}{} · {:.0}% · {:.0}°",
            spot.local() + 1,
            keys,
            if keys == 1 { "" } else { "s" },
            if on_key { " · on a key" } else { "" },
            state.zoom * 100.,
            state.rotation
        );
        let ease_owner = owner.clone();
        let shake_owner = owner.clone();
        let bar = div()
            .id("storyboard-camera-bar")
            .test_support()
            .occlude()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_1()
            .py(px(2.))
            .rounded(px(6.))
            .bg(p.panel.opacity(0.94))
            .border_1()
            .border_color(p.line)
            .text_xs()
            .text_color(p.ink)
            .child(div().px_1().child(summary))
            .child(
                button("camera-previous-key", "◀", "Previous camera key").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.camera_step_key(false, cx);
                    },
                )),
            )
            .child(
                button(
                    "camera-add-key",
                    if on_key { "Update key" } else { "Add key" },
                    "Key the camera at the playhead",
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.camera_add_key(cx);
                })),
            )
            .child(
                button(
                    "camera-delete-key",
                    "Delete key",
                    "Delete the camera key at the playhead",
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.camera_delete_key(cx);
                })),
            )
            .child(
                button("camera-next-key", "▶", "Next camera key").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.camera_step_key(true, cx);
                    },
                )),
            )
            .child(
                button(
                    "camera-ease",
                    &format!("{} ▾", easing.map_or("Ease", |e| e.label())),
                    "How the camera eases from the key at or before the playhead",
                )
                .dropdown_menu(move |menu, _, _| {
                    Self::camera_ease_items(menu, ease_owner.clone(), easing)
                }),
            )
            .child(
                button("camera-curve", "Curve", "Shape the ease as a curve")
                    .when(curve_open, |b| b.primary())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.camera_ui.curve_open = !this.camera_ui.curve_open;
                        cx.notify();
                    })),
            )
            .child(
                button(
                    "camera-shake",
                    if shake.is_some() {
                        "Shake ▾ on"
                    } else {
                        "Shake ▾"
                    },
                    "Camera shake for the scene",
                )
                .dropdown_menu(move |menu, _, _| {
                    Self::camera_shake_items(menu, shake_owner.clone(), shake)
                }),
            )
            .child(
                button(
                    "camera-static",
                    "Hold panel",
                    "Keep the camera still through this panel",
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.camera_static_panel(cx);
                })),
            )
            .child(
                button("camera-reset", "Reset", "Remove the scene's camera keys").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.camera_reset(cx);
                    }),
                ),
            )
            .child(
                button("camera-copy", "Copy", "Copy the scene's camera").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.camera_copy(cx);
                    },
                )),
            )
            .child(
                button("camera-paste", "Paste", "Give this scene the copied camera").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.camera_paste(cx);
                    }),
                ),
            )
            .child(
                Button::new("camera-done")
                    .label("Done")
                    .xsmall()
                    .primary()
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_camera_tool(cx))),
            );
        Some(
            div()
                .absolute()
                .top_2()
                .left_2()
                .flex()
                .flex_col()
                .items_start()
                .gap_1()
                .child(bar)
                .children(curve)
                .into_any_element(),
        )
    }

    fn camera_ease_items(
        menu: PopupMenu,
        owner: WeakEntity<Self>,
        current: Option<Easing>,
    ) -> PopupMenu {
        let mut menu = menu;
        for easing in Easing::ALL {
            let owner = owner.clone();
            menu = menu.item(
                PopupMenuItem::new(easing.label())
                    .checked(current == Some(easing))
                    .on_click(move |_, _, cx| {
                        owner
                            .update(cx, |e, cx| {
                                e.camera_set_easing(easing, cx);
                            })
                            .ok();
                    }),
            );
        }
        menu
    }

    fn camera_shake_items(
        menu: PopupMenu,
        owner: WeakEntity<Self>,
        current: Option<Shake>,
    ) -> PopupMenu {
        let mut menu = menu;
        for (name, preset) in Shake::PRESETS {
            let owner = owner.clone();
            menu = menu.item(
                PopupMenuItem::new(name)
                    .checked(current == Some(preset))
                    .on_click(move |_, _, cx| {
                        owner
                            .update(cx, |e, cx| {
                                e.camera_set_shake(Some(preset), cx);
                            })
                            .ok();
                    }),
            );
        }
        let edit = owner.clone();
        menu.separator()
            .item(
                PopupMenuItem::new("Shake Settings…").on_click(move |_, window, cx| {
                    edit.update(cx, |e, cx| e.camera_shake_dialog(window, cx))
                        .ok();
                }),
            )
            .item(
                PopupMenuItem::new("No Shake")
                    .checked(current.is_none())
                    .on_click(move |_, _, cx| {
                        owner
                            .update(cx, |e, cx| {
                                e.camera_set_shake(None, cx);
                            })
                            .ok();
                    }),
            )
    }

    /// View → Camera: the Camera tool and its commands.
    pub(super) fn camera_view_items(
        menu: PopupMenu,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        if editor.read(cx).editor.storyboard().is_none() {
            return menu;
        }
        let editing = editor.read(cx).camera_ui.editing;
        let owner = editor.downgrade();
        menu.submenu("Camera", window, cx, move |menu, _, _| {
            use crate::actions::*;
            let run = |label: &'static str, f: fn(&mut EditorView, &mut Context<EditorView>)| {
                let owner = owner.clone();
                PopupMenuItem::new(label).on_click(move |_, _, cx| {
                    owner.update(cx, f).ok();
                })
            };
            let shake = owner.clone();
            let ease = owner.clone();
            menu.menu_with_check("Camera Tool", editing, Box::new(ToggleCameraTool))
                .menu("Add Camera Key", Box::new(AddCameraKey))
                .menu("Delete Camera Key", Box::new(DeleteCameraKey))
                .menu("Previous Camera Key", Box::new(PreviousCameraKey))
                .menu("Next Camera Key", Box::new(NextCameraKey))
                .separator()
                .item(run("Static Camera for Panel", |e, cx| {
                    e.camera_static_panel(cx);
                }))
                .item(run("Reset Camera", |e, cx| {
                    e.camera_reset(cx);
                }))
                .item(run("Copy Camera", |e, cx| {
                    e.camera_copy(cx);
                }))
                .item(run("Paste Camera", |e, cx| {
                    e.camera_paste(cx);
                }))
                .separator()
                .item(
                    PopupMenuItem::new("Ease In and Out").on_click(move |_, _, cx| {
                        ease.update(cx, |e, cx| {
                            e.camera_set_easing(Easing::EaseInOut, cx);
                        })
                        .ok();
                    }),
                )
                .item(
                    PopupMenuItem::new("Camera Shake…").on_click(move |_, window, cx| {
                        shake
                            .update(cx, |e, cx| e.camera_shake_dialog(window, cx))
                            .ok();
                    }),
                )
        })
        .separator()
    }

    /// The camera shortcuts on the canvas (storyboards only).
    pub(crate) fn camera_actions<E: InteractiveElement>(el: E, cx: &mut Context<Self>) -> E {
        use crate::actions::*;
        el.on_action(cx.listener(|this, _: &ToggleCameraTool, _, cx| this.toggle_camera_tool(cx)))
            .on_action(cx.listener(|this, _: &AddCameraKey, _, cx| {
                this.camera_add_key(cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteCameraKey, _, cx| {
                this.camera_delete_key(cx);
            }))
            .on_action(cx.listener(|this, _: &PreviousCameraKey, _, cx| {
                this.camera_step_key(false, cx);
            }))
            .on_action(cx.listener(|this, _: &NextCameraKey, _, cx| {
                this.camera_step_key(true, cx);
            }))
    }

    // ── Timeline ──

    /// The Timeline's camera row: each scene's span with a camera, its keys
    /// as diamonds (drag to retime, click to jump there). Shown while any
    /// scene has a camera or the Camera tool is on.
    pub(crate) fn timeline_camera_row(
        &mut self,
        p: &Palette,
        header_w: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let layout = self.camera_layout();
        let (zoom, scroll) = (self.timeline_ui.zoom, self.timeline_ui.scroll);
        let width = self.timeline_lane_width();
        struct Span {
            scene: GroupId,
            start: u64,
            frames: u64,
            keys: Vec<u64>,
            shake: bool,
        }
        let spans: Vec<Span> = {
            let board = self.timeline_board()?;
            board
                .cameras
                .iter()
                .filter_map(|(scene, camera)| {
                    let (start, frames) = board.scene_span(&layout, *scene)?;
                    Some(Span {
                        scene: *scene,
                        start,
                        frames,
                        keys: camera.keys.iter().map(|k| k.frame).collect(),
                        shake: camera.shake.is_some(),
                    })
                })
                .collect()
        };
        if spans.is_empty() && !self.camera_ui.editing {
            return None;
        }
        let mut lane = div()
            .id("timeline-camera")
            .test_support()
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(height))
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    this.timeline_begin(TimelineDrag::Scrub, e.position, e.modifiers, window, cx);
                    cx.stop_propagation();
                }),
            );
        let ink: Hsla = rgb(CAMERA_INK).into();
        for span in &spans {
            let x = span.start as f32 * zoom - scroll;
            let w = span.frames as f32 * zoom;
            if x + w >= 0. && x <= width {
                lane = lane.child(
                    div()
                        .absolute()
                        .left(px(x))
                        .w(px(w))
                        .top(px(height / 2. - 5.))
                        .h(px(10.))
                        .rounded(px(3.))
                        .bg(ink.opacity(0.14))
                        .when(span.shake, |d| {
                            d.child(div().px_1().text_xs().text_color(p.muted).child("shake"))
                        }),
                );
            }
            for (index, frame) in span.keys.iter().enumerate() {
                let at = span.start + frame;
                let x = at as f32 * zoom - scroll;
                if x < -8. || x > width + 8. {
                    continue;
                }
                let scene = span.scene;
                lane = lane.child(
                    div()
                        .id(SharedString::from(format!(
                            "timeline-camera-key-{scene}-{index}"
                        )))
                        .test_support()
                        .absolute()
                        .left(px(x - 6.))
                        .top(px(height / 2. - 8.))
                        .w(px(12.))
                        .h(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(ink)
                        .cursor(CursorStyle::ResizeLeftRight)
                        .child("◆")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                this.timeline_seek(at, cx);
                                this.timeline_begin(
                                    TimelineDrag::CameraKey { scene, index },
                                    e.position,
                                    e.modifiers,
                                    window,
                                    cx,
                                );
                                cx.stop_propagation();
                            }),
                        ),
                );
            }
        }
        Some(
            div()
                .flex()
                .border_b_1()
                .border_color(p.line)
                .child(
                    div()
                        .flex_none()
                        .w(px(header_w))
                        .h(px(height))
                        .px_2()
                        .flex()
                        .items_center()
                        .border_r_1()
                        .border_color(p.line)
                        .bg(p.panel)
                        .text_xs()
                        .child("Camera"),
                )
                .child(lane)
                .into_any_element(),
        )
    }

    /// Retime camera key `index` of `scene` by `travel` frames, between its
    /// neighbours and within the scene: the edit and the overlay text.
    pub(crate) fn timeline_camera_key_move(
        &self,
        scene: GroupId,
        index: usize,
        travel: f64,
    ) -> Option<(TimingEdit, String)> {
        let board = self.editor.storyboard()?;
        let camera = board.cameras.get(&scene)?;
        let key = camera.keys.get(index)?;
        let (start, frames) = board.scene_span(&self.camera_layout(), scene)?;
        let lo = index.checked_sub(1).map_or(0, |i| camera.keys[i].frame + 1);
        let hi = camera
            .keys
            .get(index + 1)
            .map_or(frames.saturating_sub(1).max(key.frame), |k| k.frame - 1)
            .max(lo);
        let frame = ((key.frame as f64 + travel).round().max(0.) as u64).clamp(lo, hi);
        let mut next = camera.clone();
        next.keys[index].frame = frame;
        let rate = board.settings.frame_rate;
        Some((
            TimingEdit::Camera {
                scene,
                camera: next,
            },
            format!(
                "Camera key · {} · scene frame {}",
                rate.timecode(start + frame),
                frame + 1
            ),
        ))
    }
}
