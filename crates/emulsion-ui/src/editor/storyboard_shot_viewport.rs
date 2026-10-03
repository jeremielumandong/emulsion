//! The Shot Generator's viewport: the rendered picture with joint handles,
//! and what the pointer does there. Right (or middle) drag orbits, with
//! Shift it pans, and the wheel dollies (zooms the top and side views); in
//! Camera view these move the shot camera. A left drag moves, turns or
//! scales the object under the pointer (by the tool), or poses a joint: FK
//! rotation, or IK for hands and feet. Every drag is one Undo step.
use super::storyboard_shot_generator::{ShotGenerator, ShotTool, ShotView};
use super::*;
use emulsion_scene as s3;
use glam::{Vec2, Vec3};

/// A camera circling a target point (the free view, and navigating the
/// shot camera).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Orbit {
    pub(crate) target: Vec3,
    pub(crate) distance: f32,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            target: Vec3::new(0., 1., 0.),
            distance: 7.,
            yaw: 200.,
            pitch: -15.,
        }
    }
}

impl Orbit {
    /// The orbit around a point `distance` in front of `camera`.
    pub(crate) fn of(camera: &s3::Camera, distance: f32) -> Self {
        Self {
            target: camera.position + camera.forward() * distance,
            distance,
            yaw: camera.yaw,
            pitch: camera.pitch,
        }
    }

    /// The orbit's camera on `film`, a 28 mm lens.
    pub(crate) fn camera(&self, film: s3::FilmBack) -> s3::Camera {
        let mut camera = s3::Camera {
            yaw: self.yaw,
            pitch: self.pitch.clamp(-89., 89.),
            roll: 0.,
            focal_length_mm: 28.,
            film_back: film,
            ..s3::Camera::default()
        };
        camera.position = self.target - camera.forward() * self.distance;
        camera
    }

    /// `camera` moved to this orbit, keeping its lens and roll.
    fn place(&self, camera: &s3::Camera) -> s3::Camera {
        let at = self.camera(camera.film_back);
        s3::Camera {
            position: at.position,
            yaw: at.yaw,
            pitch: at.pitch,
            ..*camera
        }
    }
}

/// What a side-panel slider sets on the working set.
pub(crate) type SliderApply = Rc<dyn Fn(&mut ShotGenerator, f32)>;

/// A gesture in progress; it commits one Undo step when it ends.
pub(crate) enum Drag {
    /// Orbit (or with `pan`, slide) the free view or the shot camera.
    Navigate {
        start: Point<Pixels>,
        from: Orbit,
        pan: bool,
    },
    /// Slide an orthographic view.
    OrthoPan { start: Point<Pixels>, from: Vec2 },
    Move {
        id: s3::ObjectId,
        plane_y: f32,
        grab: Vec3,
        from: Vec3,
    },
    Rotate {
        id: s3::ObjectId,
        start: f32,
        from: (f32, f32, f32),
    },
    Scale {
        id: s3::ObjectId,
        start: f32,
        from: Vec3,
    },
    /// FK: turn one joint.
    Joint {
        id: s3::ObjectId,
        bone: s3::Bone,
        start: Point<Pixels>,
        from: s3::JointRotation,
    },
    /// IK: move a hand or foot over a plane facing the viewer.
    Ik {
        id: s3::ObjectId,
        limb: s3::Limb,
        origin: Vec3,
        normal: Vec3,
    },
    /// A side-panel slider.
    Slider {
        track: TrackBounds,
        range: (f32, f32),
        apply: SliderApply,
        label: &'static str,
    },
}

impl Drag {
    fn label(&self) -> &'static str {
        match self {
            Drag::Navigate { .. } => "Move camera",
            Drag::OrthoPan { .. } => "",
            Drag::Move { .. } => "Move object",
            Drag::Rotate { .. } => "Rotate object",
            Drag::Scale { .. } => "Scale object",
            Drag::Joint { .. } => "Pose joint",
            Drag::Ik { .. } => "Pose with IK",
            Drag::Slider { label, .. } => label,
        }
    }
}

/// The limb a hand or foot drives by IK.
fn limb_of(bone: s3::Bone) -> Option<s3::Limb> {
    use s3::Bone::*;
    Some(match bone {
        HandL | LowerArmL => s3::Limb::LeftArm,
        HandR | LowerArmR => s3::Limb::RightArm,
        FootL | LowerLegL => s3::Limb::LeftLeg,
        FootR | LowerLegR => s3::Limb::RightLeg,
        _ => return None,
    })
}

/// Where a ray meets a plane.
fn ray_plane(origin: Vec3, dir: Vec3, point: Vec3, normal: Vec3) -> Option<Vec3> {
    let d = dir.dot(normal);
    if d.abs() < 1e-6 {
        return None;
    }
    let t = (point - origin).dot(normal) / d;
    (t > 0.).then(|| origin + dir * t)
}

const HANDLE: f32 = 9.;

impl ShotGenerator {
    /// A window position as a point on the picture (logical pixels), with
    /// the picture's size.
    fn on_picture(&self, at: Point<Pixels>) -> Option<(f32, f32, f32, f32)> {
        let viewport = self.bounds.get()?;
        let rect = self.picture_rect()?;
        let x = f32::from(at.x - viewport.origin.x - rect.origin.x);
        let y = f32::from(at.y - viewport.origin.y - rect.origin.y);
        Some((
            x,
            y,
            f32::from(rect.size.width),
            f32::from(rect.size.height),
        ))
    }

    /// The view camera with the picture's size.
    fn picture_camera(&self) -> Option<(s3::Camera, u32, u32)> {
        let rect = self.picture_rect()?;
        let (w, h) = (f32::from(rect.size.width), f32::from(rect.size.height));
        Some((self.view_camera(w / h), w.round() as u32, h.round() as u32))
    }

    /// The world ray under a window position.
    fn ray_at(&self, at: Point<Pixels>) -> Option<(Vec3, Vec3)> {
        let (x, y, _, _) = self.on_picture(at)?;
        let (camera, w, h) = self.picture_camera()?;
        Some(camera.view(w, h).ray(x, y))
    }

    pub(super) fn viewport(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let bounds = self.bounds.clone();
        let rect = self.picture_rect();
        let mut root = div()
            .id("shot-viewport")
            .test_support()
            .relative()
            .flex_1()
            .min_h(px(160.))
            .overflow_hidden()
            .bg(p.soft_bg)
            .border_1()
            .border_color(p.line)
            .cursor(if self.picking_look {
                CursorStyle::Crosshair
            } else {
                CursorStyle::Arrow
            })
            .on_mouse_down(MouseButton::Left, cx.listener(Self::viewport_down))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, e: &MouseDownEvent, _, cx| this.navigate_down(e, cx)),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, e: &MouseDownEvent, _, cx| this.navigate_down(e, cx)),
            )
            .on_scroll_wheel(cx.listener(Self::wheel))
            .child(
                canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            );
        let Some(rect) = rect else {
            return root.into_any_element();
        };
        let at = |x: f32, y: f32| (rect.origin.x + px(x), rect.origin.y + px(y));
        if let Some((_, image)) = &self.frame {
            root = root.child(
                img(image.clone())
                    .absolute()
                    .left(rect.origin.x)
                    .top(rect.origin.y)
                    .w(rect.size.width)
                    .h(rect.size.height),
            );
        }
        // Joint handles of the selected character, in the Pose tool.
        if self.tool == ShotTool::Pose
            && let Some(id) = self
                .selected
                .filter(|id| self.shot.set.character(*id).is_some())
            && let Some((camera, w, h)) = self.picture_camera()
            && let Some((_, prepared)) = &self.prepared
        {
            for handle in s3::joint_handles(prepared, &camera, w, h)
                .into_iter()
                .filter(|j| j.object == id && !j.bone.is_finger())
            {
                let (x, y) = at(handle.screen.x, handle.screen.y);
                let chosen = self.joint == Some(handle.bone);
                let ik = self.ik && limb_of(handle.bone).is_some();
                root = root.child(
                    div()
                        .absolute()
                        .left(x - px(HANDLE / 2.))
                        .top(y - px(HANDLE / 2.))
                        .size(px(HANDLE))
                        .rounded_full()
                        .border_1()
                        .border_color(p.paper)
                        .bg(if chosen {
                            p.accent
                        } else if ik {
                            p.ink
                        } else {
                            p.muted
                        }),
                );
            }
        }
        if self.view == ShotView::Camera {
            root = root.child(
                div()
                    .absolute()
                    .left(rect.origin.x)
                    .top(rect.origin.y)
                    .w(rect.size.width)
                    .h(rect.size.height)
                    .border_1()
                    .border_color(p.ink.opacity(0.5)),
            );
        }
        root.into_any_element()
    }

    fn viewport_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let Some((x, y, w, h)) = self.on_picture(e.position) else {
            return;
        };
        let Some((camera, _, _)) = self.picture_camera() else {
            return;
        };
        let Some(prepared) = self.prepared() else {
            return;
        };
        let (pw, ph) = (w.round() as u32, h.round() as u32);
        let hit = s3::pick(&prepared, &camera, pw, ph, x, y);
        if self.picking_look {
            self.picking_look = false;
            let Some(id) = self
                .selected
                .filter(|id| self.shot.set.character(*id).is_some())
            else {
                return;
            };
            let (o, d) = camera.view(pw, ph).ray(x, y);
            let target = hit.as_ref().map_or(o + d * 4., |h| h.point);
            self.shot.set.character_mut(id).unwrap().look_at = Some(target);
            self.touched(false, cx);
            self.commit("Look at", cx);
            return;
        }
        // Joint handles first in the Pose tool.
        if self.tool == ShotTool::Pose
            && let Some(id) = self
                .selected
                .filter(|id| self.shot.set.character(*id).is_some())
            && let Some(joint) = s3::pick_joint(&prepared, &camera, pw, ph, x, y, HANDLE)
                .filter(|j| j.object == id && !j.bone.is_finger())
        {
            self.start_pose(id, joint.bone, joint.world, e.position, &camera);
            cx.notify();
            return;
        }
        let Some(hit) = hit else {
            self.selected = None;
            if matches!(self.view, ShotView::Top | ShotView::Side) {
                self.drag = Some(Drag::OrthoPan {
                    start: e.position,
                    from: self.ortho_pan,
                });
            }
            self.request_frame(false, cx);
            cx.notify();
            return;
        };
        let id = hit.object;
        if self.selected != Some(id) {
            self.joint = None;
            self.model_joint = None;
        }
        self.selected = Some(id);
        let Some(object) = self.shot.set.object(id) else {
            return;
        };
        let t = object.transform;
        self.drag = match self.tool {
            ShotTool::Pose => match hit.bone() {
                Some(bone) => {
                    let world = prepared
                        .character(id)
                        .map_or(hit.point, |c| c.joint_world(bone));
                    self.start_pose(id, bone, world, e.position, &camera);
                    None
                }
                None => None,
            },
            ShotTool::Move => Some(Drag::Move {
                id,
                plane_y: t.position.y,
                // Where the pointer's ray meets the object's ground plane,
                // so the object does not jump when the drag starts.
                grab: {
                    let (o, d) = camera.view(pw, ph).ray(x, y);
                    ray_plane(o, d, t.position, Vec3::Y).unwrap_or(Vec3::new(
                        hit.point.x,
                        t.position.y,
                        hit.point.z,
                    ))
                },
                from: t.position,
            }),
            ShotTool::Rotate => Some(Drag::Rotate {
                id,
                start: f32::from(e.position.x),
                from: t.rotation.to_yaw_pitch_roll(),
            }),
            ShotTool::Scale => Some(Drag::Scale {
                id,
                start: f32::from(e.position.y),
                from: t.scale,
            }),
        }
        .or(self.drag.take());
        self.request_frame(false, cx);
        cx.notify();
    }

    fn start_pose(
        &mut self,
        id: s3::ObjectId,
        bone: s3::Bone,
        world: Vec3,
        start: Point<Pixels>,
        camera: &s3::Camera,
    ) {
        self.joint = Some(bone);
        let ik = self.ik.then(|| limb_of(bone)).flatten();
        self.drag = Some(match ik {
            Some(limb) => Drag::Ik {
                id,
                limb,
                origin: world,
                normal: -camera.forward(),
            },
            None => Drag::Joint {
                id,
                bone,
                start,
                from: self
                    .shot
                    .set
                    .character(id)
                    .map_or(s3::JointRotation::ZERO, |c| c.pose.rotation(bone)),
            },
        });
    }

    fn navigate_down(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if self.on_picture(e.position).is_none() {
            return;
        }
        self.drag = Some(match self.view {
            ShotView::Top | ShotView::Side => Drag::OrthoPan {
                start: e.position,
                from: self.ortho_pan,
            },
            ShotView::Free => Drag::Navigate {
                start: e.position,
                from: self.free,
                pan: e.modifiers.shift,
            },
            ShotView::Camera => Drag::Navigate {
                start: e.position,
                from: Orbit::of(&self.shot.set.camera, self.focus_distance()),
                pan: e.modifiers.shift,
            },
        });
    }

    /// How far in front of the shot camera to orbit: the selection, else
    /// the set's middle.
    fn focus_distance(&self) -> f32 {
        let camera = &self.shot.set.camera;
        let target = self
            .object()
            .map(|o| o.transform.position + Vec3::Y)
            .or_else(|| self.prepared.as_ref().map(|(_, p)| p.bounds.center()))
            .unwrap_or(Vec3::new(0., 1., 0.));
        (target - camera.position)
            .dot(camera.forward())
            .clamp(0.5, 200.)
    }

    fn wheel(&mut self, e: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let dy = f32::from(e.delta.pixel_delta(window.line_height()).y);
        if dy == 0. {
            return;
        }
        let factor = (1.0015f32).powf(-dy);
        match self.view {
            ShotView::Top | ShotView::Side => {
                self.ortho_zoom = (self.ortho_zoom * factor).clamp(0.05, 50.);
            }
            ShotView::Free => {
                self.free.distance = (self.free.distance / factor).clamp(0.3, 400.);
            }
            ShotView::Camera => {
                let camera = &mut self.shot.set.camera;
                let step = (1. - 1. / factor) * self_distance(camera);
                camera.position += camera.forward() * step;
                self.schedule_wheel_commit(cx);
            }
        }
        self.request_frame(false, cx);
        cx.notify();
    }

    /// Commit a dolly with the wheel once the wheel rests.
    fn schedule_wheel_commit(&mut self, cx: &mut Context<Self>) {
        self.wheel_gen = self.wheel_gen.wrapping_add(1);
        let mine = self.wheel_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(400))
                .await;
            this.update(cx, |this, cx| {
                if this.wheel_gen == mine && this.drag.is_none() {
                    this.commit("Dolly camera", cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn drag_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        let at = e.position;
        let mut scene_changed = true;
        match &drag {
            Drag::Navigate { start, from, pan } => {
                let dx = f32::from(at.x - start.x);
                let dy = f32::from(at.y - start.y);
                let mut orbit = *from;
                if *pan {
                    let camera = from.camera(s3::FilmBack::default());
                    let k = from.distance * 0.0015;
                    orbit.target = from.target - camera.right() * dx * k + camera.up() * dy * k;
                } else {
                    orbit.yaw = from.yaw - dx * 0.4;
                    orbit.pitch = (from.pitch + dy * 0.3).clamp(-89., 89.);
                }
                if self.view == ShotView::Camera {
                    self.shot.set.camera = orbit.place(&self.shot.set.camera);
                } else {
                    self.free = orbit;
                }
                scene_changed = false;
            }
            Drag::OrthoPan { start, from } => {
                let k = 0.01 / self.ortho_zoom.max(0.05);
                self.ortho_pan =
                    *from + Vec2::new(f32::from(at.x - start.x), f32::from(at.y - start.y)) * k;
                scene_changed = false;
            }
            Drag::Move {
                id,
                plane_y,
                grab,
                from,
            } => {
                if let Some((o, d)) = self.ray_at(at)
                    && let Some(hit) = ray_plane(o, d, Vec3::new(0., *plane_y, 0.), Vec3::Y)
                {
                    let mut delta = hit - *grab;
                    delta.y = 0.;
                    self.shot.set.set_position(*id, *from + delta);
                }
            }
            Drag::Rotate { id, start, from } => {
                let yaw = from.0 + (f32::from(at.x) - start) * 0.5;
                self.shot.set.set_rotation_euler(*id, yaw, from.1, from.2);
            }
            Drag::Scale { id, start, from } => {
                let k = (1.006f32).powf(start - f32::from(at.y));
                self.shot.set.set_scale(*id, *from * k);
            }
            Drag::Joint {
                id,
                bone,
                start,
                from,
            } => {
                let dx = f32::from(at.x - start.x) * 0.5;
                let dy = f32::from(at.y - start.y) * 0.5;
                let mut r = *from;
                r.x += dy;
                if e.modifiers.shift {
                    r.z += dx;
                } else {
                    r.y += dx;
                }
                self.shot.set.set_joint(*id, *bone, r);
            }
            Drag::Ik {
                id,
                limb,
                origin,
                normal,
            } => {
                if let Some((o, d)) = self.ray_at(at)
                    && let Some(target) = ray_plane(o, d, *origin, *normal)
                {
                    self.shot.set.set_ik_target(*id, *limb, target);
                }
            }
            Drag::Slider {
                track,
                range,
                apply,
                ..
            } => {
                if let Some(f) = track_fraction(track, at.x) {
                    apply(self, range.0 + (range.1 - range.0) * f);
                }
            }
        }
        self.drag = Some(drag);
        if scene_changed {
            self.touched(true, cx);
        } else {
            self.request_frame(true, cx);
            cx.notify();
        }
    }

    pub(super) fn drag_end(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        let label = drag.label();
        if !label.is_empty() {
            self.commit(label, cx);
        }
        self.request_frame(false, cx);
        cx.notify();
    }

    /// Start dragging a side-panel slider at `x` (window), with `apply`
    /// setting the value; one Undo step when released.
    pub(crate) fn start_slider(
        &mut self,
        track: TrackBounds,
        range: (f32, f32),
        x: Pixels,
        label: &'static str,
        apply: SliderApply,
        cx: &mut Context<Self>,
    ) {
        if let Some(f) = track_fraction(&track, x) {
            apply(self, range.0 + (range.1 - range.0) * f);
            self.touched(true, cx);
        }
        self.drag = Some(Drag::Slider {
            track,
            range,
            apply,
            label,
        });
    }
}

/// How far a wheel notch dollies the shot camera: a share of the distance
/// to the ground in front of it, at least 10 cm.
fn self_distance(camera: &s3::Camera) -> f32 {
    let f = camera.forward();
    let to_ground = if f.y < -0.05 {
        camera.position.y / -f.y
    } else {
        5.
    };
    to_ground.clamp(0.1, 50.)
}
