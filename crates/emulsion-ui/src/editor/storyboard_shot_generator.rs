//! The Shot Generator (phase 11, SG1–SG8): a 3D set for the active panel,
//! shown in place of the Stage. A viewport rendered by `emulsion-scene` off
//! the UI thread (`storyboard_shot_viewport`), side panels to add, pose and
//! light (`storyboard_shot_panels`), a "Describe a shot" field, the Shot
//! Explorer, and the way back into the board: the set as a locked reference
//! layer or a snapshot layer to draw on.
//!
//! The view edits a working copy of the panel's set. Each gesture (a drag,
//! a slider pull, a click) changes the copy while it runs and commits it
//! as one Undo step when it ends (`ProjectEditor::edit_panel_shot`). The
//! copy follows the project (Undo, the assistant, another panel becoming
//! active) whenever no gesture runs.
use super::storyboard_shot_viewport::{Drag, Orbit};
use super::*;
use crate::widgets::tip as tip_on;
use emulsion_core::project::PageId;
use emulsion_core::storyboard_shot::{PanelShot, ReferenceSettings, ShotLibrary};
use emulsion_scene as s3;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};

/// What the viewport looks through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShotView {
    /// The shot camera: what the panel will show.
    Camera,
    /// A free orbit camera that leaves the shot alone.
    Free,
    /// Orthographic top view (V6).
    Top,
    /// Orthographic side view (V6).
    Side,
}

/// What a left drag in the viewport does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShotTool {
    /// Drag objects over the ground.
    Move,
    /// Drag sideways to turn an object.
    Rotate,
    /// Drag up and down to scale an object.
    Scale,
    /// Drag joint handles: FK rotation, or IK for hands and feet.
    Pose,
}

/// What a frame was rendered from; a frame is stale when it differs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FrameKey {
    pub(crate) scene: u64,
    pub(crate) camera: s3::Camera,
    pub(crate) size: (u32, u32),
    pub(crate) style: s3::RenderStyle,
    pub(crate) shadows: bool,
    pub(crate) highlight: Option<s3::ObjectId>,
}

/// The Shot Explorer's proposals and their thumbnails.
pub(crate) struct Explorer {
    pub(crate) proposals: Vec<s3::ShotProposal>,
    pub(crate) thumbs: Vec<Option<Arc<RenderImage>>>,
}

/// Explorer thumbnails are this wide.
pub(crate) const THUMB_W: u32 = 192;
const EXPLORER_COUNT: usize = 12;
/// Longest side of a viewport frame, in pixels.
const MAX_VIEWPORT: f32 = 1600.;
/// Frames at rest render at the display's pixels per logical pixel, at
/// most this many (more is not seen; less on 1× displays saves half the
/// work).
const IDLE_SCALE: f32 = 1.5;
/// Frames while dragging start at this scale, and shrink (down to
/// [`MIN_DRAFT_SCALE`]) when they take longer than [`DRAFT_BUDGET_MS`], so
/// drags stay smooth on slower machines and in large viewports.
const DRAFT_SCALE: f32 = 0.75;
const MIN_DRAFT_SCALE: f32 = 0.3;
const DRAFT_BUDGET_MS: f32 = 30.;

pub(crate) struct ShotGenerator {
    pub(crate) editor: WeakEntity<EditorView>,
    pub(crate) panel: PageId,
    /// The set being edited.
    pub(crate) shot: PanelShot,
    /// The panel's set in the project when last read.
    pub(crate) synced: Option<PanelShot>,
    /// The project's models and custom poses when last read.
    pub(crate) library: ShotLibrary,
    pub(crate) aspect: f32,
    pub(crate) panel_size: (u32, u32),
    pub(crate) selected: Option<s3::ObjectId>,
    /// The joint the inspector's joint sliders turn.
    pub(crate) joint: Option<s3::Bone>,
    /// The imported-rig joint the inspector turns.
    pub(crate) model_joint: Option<String>,
    pub(crate) view: ShotView,
    pub(crate) tool: ShotTool,
    /// Pose tool: dragging a hand or foot moves it by IK.
    pub(crate) ik: bool,
    /// The next viewport click aims the selected character's head.
    pub(crate) picking_look: bool,
    /// The next viewport click lays this panel layer on the surface there.
    pub(crate) picking_surface: Option<NodeId>,
    /// Gizmos use the object's own axes rather than the world's.
    pub(crate) gizmo_local: bool,
    /// The gizmo handle under the pointer.
    pub(crate) hover: Option<emulsion_core::shot_gizmo::Handle>,
    /// The viewport's keyboard focus (W, E, R and X).
    pub(crate) viewport_focus: FocusHandle,
    pub(crate) free: Orbit,
    /// Orthographic views: zoom and pan (metres).
    pub(crate) ortho_zoom: f32,
    pub(crate) ortho_pan: glam::Vec2,
    pub(crate) drag: Option<Drag>,
    /// Counts wheel dollies, to commit once the wheel rests.
    pub(crate) wheel_gen: u64,
    /// An angle and side picked for the next framing while the camera is
    /// not in a framed shot (otherwise the chips reframe right away).
    pub(crate) frame_angle: Option<s3::CameraAngle>,
    pub(crate) frame_side: Option<s3::ShotSide>,
    /// The viewport's bounds, from layout.
    pub(crate) bounds: TrackBounds,
    /// Bumps whenever the set (not just the camera) changes.
    pub(crate) scene_gen: u64,
    assets: Option<(Vec<String>, Arc<s3::AssetLibrary>)>,
    pub(crate) prepared: Option<(u64, Arc<s3::PreparedScene>)>,
    pub(crate) frame: Option<(FrameKey, Arc<RenderImage>)>,
    in_flight: Option<FrameKey>,
    /// Frames replaced, to free on the GPU.
    retired: Vec<Arc<RenderImage>>,
    /// The scale drag frames render at, kept within their time budget.
    pub(crate) draft_scale: f32,
    /// The window's pixels per logical pixel.
    display_scale: f32,
    pub(crate) explorer: Option<Explorer>,
    explorer_gen: u64,
    pub(crate) describe: Option<Entity<InputState>>,
    pub(crate) pose_name: Option<Entity<InputState>>,
    pub(crate) status: Option<(String, bool)>,
    _observe: Option<Subscription>,
}

impl ShotGenerator {
    pub(crate) fn new(editor: &Entity<EditorView>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(editor, |this, _, cx| {
            if this.drag.is_none() {
                this.sync(cx);
            }
        });
        let generator = Self {
            editor: editor.downgrade(),
            panel: 0,
            shot: PanelShot::new(16. / 9.),
            synced: None,
            library: ShotLibrary::default(),
            aspect: 16. / 9.,
            panel_size: (1920, 1080),
            selected: None,
            joint: None,
            model_joint: None,
            view: ShotView::Camera,
            tool: ShotTool::Move,
            ik: true,
            picking_look: false,
            picking_surface: None,
            gizmo_local: false,
            hover: None,
            viewport_focus: cx.focus_handle(),
            free: Orbit::default(),
            ortho_zoom: 1.,
            ortho_pan: glam::Vec2::ZERO,
            drag: None,
            wheel_gen: 0,
            frame_angle: None,
            frame_side: None,
            bounds: Rc::default(),
            scene_gen: 1,
            assets: None,
            prepared: None,
            frame: None,
            in_flight: None,
            retired: Vec::new(),
            draft_scale: DRAFT_SCALE,
            display_scale: IDLE_SCALE,
            explorer: None,
            explorer_gen: 0,
            describe: None,
            pose_name: None,
            status: None,
            _observe: Some(observe),
        };
        // The editor is being updated while this opens: read it right after.
        let me = cx.weak_entity();
        cx.defer(move |cx| {
            me.update(cx, |this, cx| this.sync(cx)).ok();
        });
        generator
    }

    /// Follow the project: the active panel, its set and the library.
    pub(crate) fn sync(&mut self, cx: &mut Context<Self>) {
        self.follow_project(false, cx);
    }

    /// Read the panel's set from the project; with `force`, drop the
    /// working copy even when the project did not change (a refused edit).
    fn follow_project(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let e = &editor.read(cx).editor;
        let Some(board) = e.storyboard() else {
            return;
        };
        let panel = e.active_page();
        let project = e.panel_shot(panel).cloned();
        let size = (board.settings.width, board.settings.height);
        let library = board.shot_library.clone();
        let aspect = board.aspect();
        let changed_panel = panel != self.panel;
        // Models compare by id (their content's hash), not byte by byte.
        let same_library = library.poses == self.library.poses
            && library.models.keys().eq(self.library.models.keys());
        if !force && !changed_panel && project == self.synced && same_library {
            return;
        }
        self.panel = panel;
        self.panel_size = size;
        self.aspect = aspect;
        if !same_library {
            self.library = library;
            self.assets = None;
            self.scene_gen += 1;
        }
        if force || changed_panel || project != self.synced {
            let shot = project.clone().unwrap_or_else(|| PanelShot::new(aspect));
            if shot.set != self.shot.set {
                self.scene_gen += 1;
            }
            self.shot = shot;
            self.synced = project;
            if changed_panel {
                self.explorer = None;
                self.free = Orbit::default();
            }
            if self
                .selected
                .is_some_and(|id| self.shot.set.object(id).is_none())
            {
                self.selected = None;
            }
        }
        self.request_frame(false, cx);
        cx.notify();
    }

    /// The set changed (not just the camera): render it again.
    pub(crate) fn touched(&mut self, draft: bool, cx: &mut Context<Self>) {
        self.scene_gen += 1;
        self.request_frame(draft, cx);
        cx.notify();
    }

    /// Commit the working set as one Undo step labelled `label`; render the
    /// reference layer again when it updates itself.
    pub(crate) fn commit(&mut self, label: &str, cx: &mut Context<Self>) {
        let Some(shot) = self.pending() else {
            return;
        };
        self.edit_project(label, cx, move |s, _| {
            *s = shot;
            Ok(())
        });
    }

    /// The working set when it differs from the project's.
    fn pending(&self) -> Option<PanelShot> {
        let unchanged = match &self.synced {
            Some(synced) => *synced == self.shot,
            // An untouched empty set is not worth keeping.
            None => self.shot == PanelShot::new(self.aspect),
        };
        (!unchanged).then(|| self.shot.clone())
    }

    /// Closing: the panel, the set still to commit and the pictures to free.
    fn close(&mut self) -> (PageId, Option<PanelShot>, Vec<Arc<RenderImage>>) {
        self.retire_explorer();
        if let Some((_, image)) = self.frame.take() {
            self.retired.push(image);
        }
        (
            self.panel,
            self.pending(),
            std::mem::take(&mut self.retired),
        )
    }

    /// Run `edit` on the panel's set and library in the project as one Undo
    /// step, then follow the project.
    pub(crate) fn edit_project(
        &mut self,
        label: &str,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut PanelShot, &mut ShotLibrary) -> Result<(), String>,
    ) -> bool {
        let panel = self.panel;
        let Some(editor) = self.editor.upgrade() else {
            return false;
        };
        let result = editor.update(cx, |e, cx| {
            if !e.prepare_page_action(cx) {
                return Err(String::new());
            }
            let result = e.editor.edit_panel_shot(panel, label, edit);
            match &result {
                Ok(stale) => {
                    e.after_change(cx);
                    if *stale {
                        e.refresh_shot_reference(panel, false, cx);
                    }
                }
                Err(error) => e.set_status(error.clone(), true, cx),
            }
            result
        });
        self.status = result
            .as_ref()
            .err()
            .filter(|e| !e.is_empty())
            .map(|e| (e.clone(), true));
        // Show the project's set again (after a refusal too).
        self.follow_project(true, cx);
        result.is_ok()
    }

    pub(crate) fn object(&self) -> Option<&s3::SceneObject> {
        self.shot.set.object(self.selected?)
    }

    /// The subject for framing and the Explorer: the selection, else the
    /// first character, else the first prop.
    pub(crate) fn subject(&self) -> Option<s3::ObjectId> {
        let set = &self.shot.set;
        self.selected
            .filter(|id| {
                set.object(*id)
                    .is_some_and(|o| !matches!(o.kind, s3::ObjectKind::Light(_)))
            })
            .or_else(|| set.character_ids().first().copied())
            .or_else(|| {
                set.objects
                    .iter()
                    .find(|o| matches!(o.kind, s3::ObjectKind::Prop(_)))
                    .map(|o| o.id)
            })
    }

    /// The models parsed, rebuilt when the library changes.
    pub(crate) fn assets(&mut self) -> Arc<s3::AssetLibrary> {
        let key: Vec<String> = self.library.models.keys().cloned().collect();
        if let Some((k, assets)) = &self.assets
            && *k == key
        {
            return assets.clone();
        }
        let assets = Arc::new(self.library.assets());
        self.assets = Some((key, assets.clone()));
        assets
    }

    /// The set prepared for picking: the latest one rendered, or prepared
    /// now when there is none.
    pub(crate) fn prepared(&mut self) -> Option<Arc<s3::PreparedScene>> {
        if let Some((generation, prepared)) = &self.prepared
            && (*generation == self.scene_gen || self.drag.is_some())
        {
            return Some(prepared.clone());
        }
        let assets = self.assets();
        let prepared = Arc::new(s3::prepare(&self.shot.set, &assets).ok()?);
        self.prepared = Some((self.scene_gen, prepared.clone()));
        Some(prepared)
    }

    /// The camera the viewport looks through for an image of `aspect`.
    pub(crate) fn view_camera(&self, aspect: f32) -> s3::Camera {
        let bounds = self
            .prepared
            .as_ref()
            .map_or(s3::Aabb::EMPTY, |(_, p)| p.bounds);
        let ortho = |mut camera: s3::Camera, pan: glam::Vec3| {
            if let s3::Projection::Orthographic { height } = camera.projection {
                camera.projection = s3::Projection::Orthographic {
                    height: height / self.ortho_zoom.max(0.05),
                };
            }
            camera.position += pan;
            camera
        };
        match self.view {
            ShotView::Camera => self.shot.set.camera,
            ShotView::Free => self.free.camera(self.shot.set.camera.film_back),
            ShotView::Top => ortho(
                s3::Camera::top_view(&bounds, aspect),
                glam::Vec3::new(-self.ortho_pan.x, 0., self.ortho_pan.y),
            ),
            ShotView::Side => ortho(
                s3::Camera::side_view(&bounds, aspect),
                glam::Vec3::new(0., self.ortho_pan.y, -self.ortho_pan.x),
            ),
        }
    }

    /// Where the picture sits in the viewport (logical pixels, relative to
    /// the viewport): the panel's shape in Camera view, all of it otherwise.
    pub(crate) fn picture_rect(&self) -> Option<Bounds<Pixels>> {
        let b = self.bounds.get()?;
        let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
        if w < 4. || h < 4. {
            return None;
        }
        if self.view != ShotView::Camera {
            return Some(Bounds::new(point(px(0.), px(0.)), size(px(w), px(h))));
        }
        let (fw, fh) = if w / h > self.aspect {
            (h * self.aspect, h)
        } else {
            (w, w / self.aspect)
        };
        Some(Bounds::new(
            point(px((w - fw) / 2.), px((h - fh) / 2.)),
            size(px(fw), px(fh)),
        ))
    }

    /// Render the viewport again if what it shows changed: at a reduced
    /// size while `draft` (a drag runs), full size otherwise. One frame
    /// renders at a time; frames asked for meanwhile collapse into the
    /// latest, and a frame that is already stale when it lands is dropped.
    pub(crate) fn request_frame(&mut self, draft: bool, cx: &mut Context<Self>) {
        let Some(rect) = self.picture_rect() else {
            return;
        };
        let scale = if draft {
            self.draft_scale
        } else {
            self.display_scale.clamp(1., IDLE_SCALE)
        };
        let (w, h) = (f32::from(rect.size.width), f32::from(rect.size.height));
        let fit = (MAX_VIEWPORT / w.max(h)).min(scale);
        let size = (
            (w * fit).round().max(16.) as u32,
            (h * fit).round().max(16.) as u32,
        );
        let key = FrameKey {
            scene: self.scene_gen,
            camera: self.view_camera(w / h),
            size,
            style: self.shot.reference.style,
            shadows: self.shot.reference.shadows,
            highlight: self.selected,
        };
        if self.frame.as_ref().is_some_and(|(k, _)| *k == key)
            || self.in_flight.as_ref() == Some(&key)
        {
            return;
        }
        if self.in_flight.is_some() {
            // The frame rendering now asks again when it lands.
            return;
        }
        self.in_flight = Some(key.clone());
        let set = self.shot.set.clone();
        let prepared = self
            .prepared
            .as_ref()
            .filter(|(g, _)| *g == key.scene)
            .map(|(_, p)| p.clone());
        // Models are parsed off the UI thread too, the first time.
        let models: Vec<String> = self.library.models.keys().cloned().collect();
        let assets = match &self.assets {
            Some((k, assets)) if *k == models => Ok(assets.clone()),
            _ => Err(self.library.clone()),
        };
        let draft_now = draft;
        cx.spawn(async move |this, cx| {
            let job_key = key.clone();
            let result = cx
                .background_spawn(async move {
                    let started = std::time::Instant::now();
                    let assets = assets.unwrap_or_else(|library| Arc::new(library.assets()));
                    let prepared = match prepared {
                        Some(p) => p,
                        None => Arc::new(s3::prepare(&set, &assets).map_err(|e| e.to_string())?),
                    };
                    let options = s3::RenderOptions {
                        highlight: job_key.highlight,
                        ..ReferenceSettings {
                            style: job_key.style,
                            shadows: job_key.shadows,
                            ..Default::default()
                        }
                        .options()
                    };
                    let image = s3::render(
                        &prepared,
                        &job_key.camera,
                        job_key.size.0,
                        job_key.size.1,
                        &options,
                    );
                    let ms = started.elapsed().as_secs_f32() * 1000.;
                    Ok::<_, String>((prepared, bgra(image), assets, ms))
                })
                .await;
            this.update(cx, |this, cx| {
                this.in_flight = None;
                match result {
                    Ok((prepared, image, assets, ms)) => {
                        if draft_now {
                            // Pixels go with the square of the scale.
                            let fit = (DRAFT_BUDGET_MS / ms.max(1.)).sqrt();
                            this.draft_scale =
                                (this.draft_scale * fit).clamp(MIN_DRAFT_SCALE, DRAFT_SCALE);
                        }
                        let current: Vec<String> = this.library.models.keys().cloned().collect();
                        if current == models {
                            this.assets = Some((models, assets));
                        }
                        if key.scene == this.scene_gen {
                            this.prepared = Some((key.scene, prepared));
                        }
                        if let Some((_, old)) = this.frame.replace((key, image)) {
                            this.retired.push(old);
                        }
                        this.free_retired(cx);
                    }
                    Err(error) => this.status = Some((error, true)),
                }
                // Catch up with what changed while this frame rendered.
                this.request_frame(draft_now && this.drag.is_some(), cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn free_retired(&mut self, cx: &mut Context<Self>) {
        let images = std::mem::take(&mut self.retired);
        if let Some(editor) = self.editor.upgrade() {
            editor.update(cx, |_, cx| {
                super::storyboard_timeline::drop_images(images, cx)
            });
        }
    }

    /// Propose shots of the subject and render their thumbnails.
    pub(crate) fn explore(&mut self, cx: &mut Context<Self>) {
        let Some(subject) = self.subject() else {
            self.status = Some(("Add a character or prop to explore shots of.".into(), true));
            cx.notify();
            return;
        };
        let assets = self.assets();
        let proposals = match s3::explore_shots(
            &self.shot.set,
            &assets,
            subject,
            EXPLORER_COUNT,
            self.aspect,
        ) {
            Ok(p) => p,
            Err(error) => {
                self.status = Some((error.to_string(), true));
                cx.notify();
                return;
            }
        };
        self.explorer_gen += 1;
        let generation = self.explorer_gen;
        let count = proposals.len();
        let cameras: Vec<s3::Camera> = proposals.iter().map(|p| p.camera).collect();
        self.retire_explorer();
        self.explorer = Some(Explorer {
            proposals,
            thumbs: vec![None; count],
        });
        let set = self.shot.set.clone();
        let options = self.shot.reference.options();
        let (w, h) = (
            THUMB_W,
            (THUMB_W as f32 / self.aspect).round().max(8.) as u32,
        );
        cx.spawn(async move |this, cx| {
            let prepared = cx
                .background_spawn(async move { s3::prepare(&set, &assets).ok().map(Arc::new) })
                .await;
            let Some(prepared) = prepared else {
                return;
            };
            for (i, camera) in cameras.into_iter().enumerate() {
                let prepared = prepared.clone();
                let image = cx
                    .background_spawn(async move {
                        bgra(s3::render(&prepared, &camera, w, h, &options))
                    })
                    .await;
                let live = this
                    .update(cx, |this, cx| {
                        let Some(explorer) = this
                            .explorer
                            .as_mut()
                            .filter(|_| this.explorer_gen == generation)
                        else {
                            return false;
                        };
                        explorer.thumbs[i] = Some(image);
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !live {
                    return;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn retire_explorer(&mut self) {
        if let Some(explorer) = self.explorer.take() {
            self.retired.extend(explorer.thumbs.into_iter().flatten());
        }
    }

    /// Use proposal `i` of the Explorer as the shot camera.
    pub(crate) fn use_proposal(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some((spec, camera)) = self
            .explorer
            .as_ref()
            .and_then(|e| e.proposals.get(i))
            .map(|p| (p.spec, p.camera))
        else {
            return;
        };
        self.shot.set.apply_shot(spec, camera);
        self.frame_angle = None;
        self.frame_side = None;
        self.view = ShotView::Camera;
        self.commit("Use shot", cx);
        self.request_frame(false, cx);
    }

    /// Build the set from the "Describe a shot" field.
    pub(crate) fn describe_shot(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self
            .describe
            .as_ref()
            .map(|i| i.read(cx).value().to_string())
        else {
            return;
        };
        self.describe_text(&text, cx);
    }

    /// Replace the set with one built from `text` (offline parser).
    pub(crate) fn describe_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.trim().is_empty() {
            return;
        }
        let panel = self.panel;
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let text = text.to_string();
        let result = editor.update(cx, |e, cx| {
            if !e.prepare_page_action(cx) {
                return Err(String::new());
            }
            let result = e.editor.describe_panel_shot(panel, &text);
            match &result {
                Ok(described) => {
                    e.after_change(cx);
                    if described.stale {
                        e.refresh_shot_reference(panel, false, cx);
                    }
                }
                Err(error) => e.set_status(error.clone(), true, cx),
            }
            result
        });
        self.status = Some(match result {
            Ok(d) if d.unrecognized.is_empty() => (d.interpretation, false),
            Ok(d) => (
                format!(
                    "{} · not understood: {}",
                    d.interpretation,
                    d.unrecognized.join(", ")
                ),
                false,
            ),
            Err(error) => (error, true),
        })
        .filter(|(s, _)| !s.is_empty());
        self.selected = None;
        self.view = ShotView::Camera;
        self.follow_project(true, cx);
    }

    /// Lay panel layer `layer` on the surface `hit` (C12): warped to the
    /// surface's angle as the shot camera sees it. One Undo step.
    pub(crate) fn lay_on_surface(
        &mut self,
        layer: NodeId,
        hit: &s3::PickHit,
        cx: &mut Context<Self>,
    ) {
        let panel = self.panel;
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let result = editor.update(cx, |e, cx| {
            if !e.prepare_page_action(cx) {
                return Err(String::new());
            }
            let result = e.editor.attach_layer_to_shot(
                panel,
                layer,
                hit.object,
                hit.bone(),
                Some(hit.point),
                Some(hit.normal),
            );
            match &result {
                Ok(()) => e.after_change(cx),
                Err(error) => e.set_status(error.clone(), true, cx),
            }
            result
        });
        self.status = match result {
            Ok(()) => Some((
                "The layer lies on the surface and follows it; edit its hidden “(flat)” copy to change the drawing.".into(),
                false,
            )),
            Err(error) if error.is_empty() => None,
            Err(error) => Some((error, true)),
        };
        self.follow_project(true, cx);
    }

    /// Render the set into the panel: the reference layer, or a snapshot.
    pub(crate) fn render_into_panel(&mut self, snapshot: bool, cx: &mut Context<Self>) {
        self.commit("Shot Generator set", cx);
        let panel = self.panel;
        if let Some(editor) = self.editor.upgrade() {
            editor.update(cx, |e, cx| e.refresh_shot_reference(panel, snapshot, cx));
        }
        self.status = Some((
            if snapshot {
                "Rendering a snapshot layer…"
            } else {
                "Rendering the reference layer…"
            }
            .into(),
            false,
        ));
        cx.notify();
    }
}

/// Straight RGBA → a GPU picture.
pub(crate) fn bgra(mut image: s3::RgbaImage) -> Arc<RenderImage> {
    for px in image.pixels.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    Arc::new(viewport::bgra_image(
        image.width,
        image.height,
        image.pixels,
    ))
}

impl Render for ShotGenerator {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        self.display_scale = window.scale_factor();
        if self.describe.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Describe a shot: low-angle close-up of two people at a table")
            });
            let sub = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.describe_shot(cx);
                }
            });
            sub.detach();
            self.describe = Some(input);
        }
        if self.pose_name.is_none() {
            self.pose_name =
                Some(cx.new(|cx| InputState::new(window, cx).placeholder("Pose name")));
        }
        // Keep the frame current with the viewport's size (known after the
        // first layout).
        if self.bounds.get().is_none() {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
        } else if self.drag.is_none() {
            self.request_frame(false, cx);
        }
        let toolbar = self.toolbar(&p, cx);
        let describe = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(self.describe.as_ref().unwrap()).small()),
            )
            .child(
                Button::new("shot-describe-go")
                    .label("Build shot")
                    .tooltip("Build the set and frame the camera from the description (offline)")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.describe_shot(cx))),
            )
            .child(
                Button::new("shot-explore")
                    .label("Shot Explorer")
                    .tooltip("Propose camera setups on the selection")
                    .small()
                    .when(self.explorer.is_some(), |b| b.primary())
                    .when(self.explorer.is_none(), |b| b.outline())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.explorer.is_some() {
                            this.retire_explorer();
                            this.free_retired(cx);
                            cx.notify();
                        } else {
                            this.explore(cx);
                        }
                    })),
            );
        let centre = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(6.))
            .child(describe)
            .child(self.viewport(&p, cx))
            .children(self.explorer_strip(&p, cx))
            .children(self.status.clone().map(|(text, error)| {
                div()
                    .id("shot-status")
                    .test_support()
                    .text_size(px(11.))
                    .text_color(if error { p.accent } else { p.muted })
                    .child(text)
            }));
        div()
            .id("shot-generator")
            .test_support()
            .key_context("panel")
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .bg(p.paper)
            .text_size(px(12.))
            .text_color(p.ink)
            .on_mouse_move(cx.listener(Self::drag_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::drag_end))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::drag_end))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::drag_end))
            .child(toolbar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(8.))
                    .p(px(8.))
                    .child(self.left_panel(&p, cx))
                    .child(centre)
                    .child(self.right_panel(&p, cx)),
            )
    }
}

impl ShotGenerator {
    fn toolbar(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let view = |id: &'static str, label: &'static str, tip: &'static str, v: ShotView| {
            tip_on(chip(id, label, self.view == v, p), tip)
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.view = v;
                    this.request_frame(false, cx);
                    cx.notify();
                }))
        };
        let tool = |id: &'static str, label: &'static str, tip: &'static str, t: ShotTool| {
            tip_on(chip(id, label, self.tool == t, p), tip)
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tool = t;
                    this.hover = None;
                    cx.notify();
                }))
        };
        div()
            .id("shot-toolbar")
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(6.))
            .border_b_1()
            .border_color(p.line)
            .child(label("Shot Generator", p))
            .child(mono(
                self.editor
                    .upgrade()
                    .and_then(|e| {
                        e.read(cx)
                            .editor
                            .page_list()
                            .iter()
                            .find(|m| m.id == self.panel)
                            .map(|m| m.name.clone())
                    })
                    .unwrap_or_default(),
                10.,
                p.muted,
            ))
            .child(div().w(px(8.)))
            .child(view(
                "shot-view-camera",
                "Camera",
                "Look through the shot camera",
                ShotView::Camera,
            ))
            .child(view(
                "shot-view-free",
                "Free",
                "Look around without moving the shot camera",
                ShotView::Free,
            ))
            .child(view(
                "shot-view-top",
                "Top",
                "Orthographic view from above",
                ShotView::Top,
            ))
            .child(view(
                "shot-view-side",
                "Side",
                "Orthographic view from the side",
                ShotView::Side,
            ))
            .child(div().w(px(8.)))
            .child(tool(
                "shot-tool-move",
                "Move",
                "Drag the arrows or squares to move along an axis or plane, or the object over the ground; Shift snaps to 10 cm (W)",
                ShotTool::Move,
            ))
            .child(tool(
                "shot-tool-rotate",
                "Rotate",
                "Drag a ring to turn about its axis, or the object sideways to turn it; Shift snaps to 15° (E)",
                ShotTool::Rotate,
            ))
            .child(tool(
                "shot-tool-scale",
                "Scale",
                "Drag an axis handle to stretch, the middle square or the object to scale it; Shift snaps to 10% (R)",
                ShotTool::Scale,
            ))
            .child(
                tip_on(
                    chip(
                        "shot-gizmo-local",
                        if self.gizmo_local { "Local" } else { "World" },
                        self.gizmo_local,
                        p,
                    ),
                    "Move and Rotate gizmos use the object's own axes (Local) or the world's (X)",
                )
                .test_support()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gizmo_local = !this.gizmo_local;
                    cx.notify();
                })),
            )
            .child(tool(
                "shot-tool-pose",
                "Pose",
                "Drag joints to pose; hands and feet use IK when IK is on",
                ShotTool::Pose,
            ))
            .child(
                tip_on(
                    chip("shot-ik", "IK", self.ik, p),
                    "Pose tool: drag hands and feet to a point; elbows and knees follow",
                )
                .test_support()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.ik = !this.ik;
                    cx.notify();
                })),
            )
            .child(div().flex_1())
            .child(
                Button::new("shot-use-reference")
                    .label("Use as reference layer")
                    .tooltip("Render the camera view into the panel's locked Shot Generator layer")
                    .small()
                    .primary()
                    .on_click(cx.listener(|this, _, _, cx| this.render_into_panel(false, cx))),
            )
            .child(
                Button::new("shot-snapshot")
                    .label("Snapshot to layer")
                    .tooltip("Render the camera view into a new layer you can draw on")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.render_into_panel(true, cx))),
            )
            .child(
                Button::new("shot-close")
                    .label("Done")
                    .tooltip("Back to the Stage (Ctrl+Alt+Shift+G)")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        // After this click, when the generator is free again.
                        if let Some(editor) = this.editor.upgrade() {
                            cx.defer(move |cx| {
                                editor.update(cx, |e, cx| e.close_shot_generator(cx));
                            });
                        }
                    })),
            )
            .into_any_element()
    }

    fn explorer_strip(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let explorer = self.explorer.as_ref()?;
        let h = (THUMB_W as f32 / self.aspect).round();
        let mut grid = div()
            .id("shot-explorer")
            .test_support()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .max_h(px(h * 2. + 60.))
            .overflow_y_scroll();
        for (i, proposal) in explorer.proposals.iter().enumerate() {
            let thumb = explorer.thumbs.get(i).cloned().flatten();
            grid = grid.child(
                div()
                    .id(("shot-proposal", i))
                    .test_support()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .w(px(THUMB_W as f32))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.use_proposal(i, cx)))
                    .child(
                        div()
                            .w(px(THUMB_W as f32))
                            .h(px(h))
                            .bg(p.soft_bg)
                            .border_1()
                            .border_color(p.line)
                            .hover(|s| s.border_color(p.accent))
                            .children(thumb.map(|t| img(t).size_full())),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .truncate()
                            .child(proposal.name.clone()),
                    ),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(mono(
                    "Shot Explorer · click a proposal to use it as the shot camera",
                    10.,
                    p.muted,
                ))
                .child(grid)
                .into_any_element(),
        )
    }
}

impl EditorView {
    /// Whether the Shot Generator shows in place of the Stage.
    pub(crate) fn shot_generator_open(&self) -> bool {
        self.shot_generator.is_some()
    }

    /// View › Shot Generator (Ctrl+Alt+Shift+G): open it on the active
    /// panel, or go back to the Stage.
    pub(crate) fn toggle_shot_generator(&mut self, cx: &mut Context<Self>) {
        if self.shot_generator.is_some() {
            self.close_shot_generator(cx);
            return;
        }
        if self.editor.storyboard().is_none() {
            return;
        }
        let me = cx.entity();
        self.shot_generator = Some(cx.new(|cx| ShotGenerator::new(&me, cx)));
        cx.notify();
    }

    /// Open the Shot Generator so the next click in its viewport lays the
    /// selected layer on the surface there (C12).
    pub(crate) fn lay_layer_on_surface(&mut self, cx: &mut Context<Self>) {
        let Some(layer) = self
            .selected
            .filter(|id| self.editor.doc.node(*id).is_some())
        else {
            self.set_status("Select a pixel layer of the panel first.", true, cx);
            return;
        };
        if self.shot_generator.is_none() {
            self.toggle_shot_generator(cx);
        }
        if let Some(generator) = self.shot_generator.clone() {
            generator.update(cx, |g, cx| {
                g.picking_surface = Some(layer);
                g.view = ShotView::Camera;
                g.status = Some((
                    "Click a surface in the viewport to lay the layer on it.".into(),
                    false,
                ));
                cx.notify();
            });
        }
    }

    pub(crate) fn close_shot_generator(&mut self, cx: &mut Context<Self>) {
        if let Some(generator) = self.shot_generator.take() {
            let (panel, pending, images) = generator.update(cx, |g, _| g.close());
            if let Some(shot) = pending {
                match self
                    .editor
                    .edit_panel_shot(panel, "Shot Generator set", |s, _| {
                        *s = shot;
                        Ok(())
                    }) {
                    Ok(stale) => {
                        self.after_change(cx);
                        if stale {
                            self.refresh_shot_reference(panel, false, cx);
                        }
                    }
                    Err(error) => self.set_status(error, true, cx),
                }
            }
            super::storyboard_timeline::drop_images(images, cx);
        }
        cx.notify();
    }

    /// Render `panel`'s set at the panel's size off the UI thread, then put
    /// it in the reference layer (joining the set edit just made when it is
    /// still the last thing done) or, with `snapshot`, a new layer.
    pub(crate) fn refresh_shot_reference(
        &mut self,
        panel: PageId,
        snapshot: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let Some(shot) = self.editor.panel_shot(panel).cloned() else {
            self.set_status("That panel has no Shot Generator set.", true, cx);
            return;
        };
        let library = board.shot_library.clone();
        let (w, h) = (board.settings.width, board.settings.height);
        cx.spawn(async move |this, cx| {
            let set = shot.set.clone();
            let image = cx
                .background_spawn(async move { shot.render_reference(&library.assets(), w, h) })
                .await;
            this.update(cx, |this, cx| {
                let result = image.and_then(|image| {
                    if !this.prepare_page_action(cx) {
                        return Err("Finish the current edit first.".into());
                    }
                    if snapshot {
                        this.editor.snapshot_shot(panel, &image)
                    } else {
                        this.editor.set_shot_reference(panel, &image, &set, true)
                    }
                });
                match result {
                    Ok(_) => {
                        this.after_change(cx);
                        this.set_status(
                            if snapshot {
                                "Snapshot added as a layer."
                            } else {
                                "Shot Generator reference layer updated."
                            },
                            false,
                            cx,
                        );
                    }
                    // A newer set renders again by itself.
                    Err(error) if error.contains("changed while") => {}
                    Err(error) => this.set_status(error, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// View menu entry for the Shot Generator.
    pub(super) fn shot_generator_view_items(
        menu: gpui_kit::component::menu::PopupMenu,
        editor: &Entity<EditorView>,
        cx: &mut Context<gpui_kit::component::menu::PopupMenu>,
    ) -> gpui_kit::component::menu::PopupMenu {
        if editor.read(cx).editor.storyboard().is_none() {
            return menu;
        }
        let open = editor.read(cx).shot_generator_open();
        let owner = editor.downgrade();
        menu.item(
            gpui_kit::component::menu::PopupMenuItem::new("Shot Generator")
                .checked(open)
                .on_click(move |_, _, cx| {
                    owner.update(cx, |e, cx| e.toggle_shot_generator(cx)).ok();
                }),
        )
    }
}

#[cfg(test)]
#[path = "storyboard_shot_tests.rs"]
mod tests;
