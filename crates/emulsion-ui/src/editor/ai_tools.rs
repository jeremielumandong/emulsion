//! Tier-1 local models in the editor: Select Subject, Remove Background and
//! SAM-backed quick selection. Each runs off the UI thread against a
//! snapshot, reports progress in the status line, and lands as an ordinary
//! selection or node so undo and the assistant see nothing special.

use super::*;
use emulsion_ai::jobs::Job;
use emulsion_ai::models::Task;
use emulsion_ai::{depth, face, inpaint, matte, sam, upscale};
use emulsion_raster::IRect;
use emulsion_raster::Mask;
use emulsion_raster::composite::region;
use emulsion_raster::select::Combine;
use std::time::Duration;

#[derive(Default)]
pub(crate) struct AiState {
    /// Quick select uses SAM when a model is installed and this is on.
    pub ai_select: bool,
    /// SAM embedding of the composite, by document revision.
    sam: Option<(u64, Arc<sam::Embedding>)>,
    sam_loading: Option<u64>,
    /// The running job, for the status line and cancel.
    pub job: Option<Arc<Job>>,
    /// The last model-made selection, kept soft so it can be refined.
    pub refine: Option<Refine>,
}

/// A soft matte from a model with the settings that turn it into a
/// selection, so the person can tune them after the fact.
#[derive(Clone)]
pub(crate) struct Refine {
    /// The model's soft matte, 0–255.
    pub raw: Mask,
    /// What produced it, e.g. "SlimSAM" or "RMBG 1.4".
    pub source: String,
    /// The model's own confidence, when it gives one.
    pub confidence: Option<f32>,
    /// Selection before the model ran, and how the result joins it.
    pub base: Option<Arc<Mask>>,
    pub combine: Combine,
    /// Below `lo` is out, above `hi` is in, soft between.
    pub lo: f32,
    pub hi: f32,
    /// Pixels to grow (+) or shrink (−) the result.
    pub grow: f32,
    pub feather: f32,
}

/// What to tell someone when a task's model is not installed.
pub(crate) fn missing(task: Task) -> String {
    let want = emulsion_ai::models::MANIFEST
        .iter()
        .find(|m| m.task == task && m.default)
        .map(|m| m.name)
        .unwrap_or("a model");
    format!("Needs {want}: install it under Settings › Local models.")
}

impl EditorView {
    /// The flattened document as a raster, computed off the UI thread.
    pub(crate) fn composite_raster(&self) -> impl std::future::Future<Output = Raster> + use<> {
        let tree = self.tree.clone();
        async move {
            let (w, h) = (tree.width, tree.height);
            let px: Vec<[u16; 4]> = region(&tree, IRect::new(0, 0, w as i32, h as i32))
                .into_iter()
                .map(color::f_to_px)
                .collect();
            Raster::from_pixels(w, h, [0; 4], &px)
        }
    }

    /// Show a job's stage and percentage until it finishes.
    fn watch_job(&mut self, job: Arc<Job>, cx: &mut Context<Self>) {
        self.ai.job = Some(job.clone());
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                let more = this.update(cx, |this, cx| {
                    if job.is_finished() {
                        if this.ai.job.as_ref().is_some_and(|j| Arc::ptr_eq(j, &job)) {
                            this.ai.job = None;
                        }
                        return false;
                    }
                    this.set_status(job.summary(), false, cx);
                    true
                });
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Turn a model's matte into the selection through the refine settings
    /// and remember it for further tuning.
    #[allow(clippy::too_many_arguments)]
    fn select_from_matte(
        &mut self,
        raw: Mask,
        source: &str,
        confidence: Option<f32>,
        combine: Combine,
        lo: f32,
        hi: f32,
        cx: &mut Context<Self>,
    ) {
        let base = self.editor.doc.selection.clone();
        self.ai.refine = Some(Refine {
            raw,
            source: source.to_string(),
            confidence,
            base,
            combine,
            lo,
            hi,
            grow: 0.0,
            feather: self.tools.feather,
        });
        self.refine_apply(cx);
    }

    /// Recompute the selection from the kept matte and current settings.
    pub(crate) fn refine_apply(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.ai.refine.clone() else {
            return;
        };
        let mut m = matte::harden(
            &r.raw,
            r.lo.round() as u8,
            r.hi.round().max(r.lo + 1.0) as u8,
        );
        if r.grow.round() != 0.0 {
            m = emulsion_raster::select::grow(&m, r.grow.round() as i32);
        }
        if r.feather > 0.5 {
            m = emulsion_raster::select::feather(&m, r.feather);
        }
        let combined = emulsion_raster::select::combine(r.base.as_deref(), &m, r.combine);
        let selection =
            (!emulsion_raster::select::bounds(&combined).is_empty()).then(|| Arc::new(combined));
        self.execute(Command::SetSelection { selection }, cx);
    }

    pub(crate) fn set_refine(&mut self, f: impl Fn(&mut Refine), cx: &mut Context<Self>) {
        if let Some(r) = &mut self.ai.refine {
            f(r);
            self.refine_apply(cx);
        }
    }

    /// Stop refining; the selection stays as it is.
    pub fn refine_done(&mut self, cx: &mut Context<Self>) {
        self.ai.refine = None;
        cx.notify();
    }

    /// One line on where the mask came from.
    pub(crate) fn refine_why(&self) -> Option<String> {
        let r = self.ai.refine.as_ref()?;
        let conf = r
            .confidence
            .map(|c| format!(", {:.0} % confident", c * 100.0))
            .unwrap_or_default();
        Some(format!(
            "{}{conf} · in above {:.0}, out below {:.0}, {} {} px, feather {:.0}",
            r.source,
            r.hi,
            r.lo,
            if r.grow >= 0.0 { "grown" } else { "shrunk" },
            r.grow.abs().round(),
            r.feather
        ))
    }

    pub fn cancel_ai(&mut self, cx: &mut Context<Self>) {
        if let Some(j) = &self.ai.job {
            j.cancel();
            self.set_status("Cancelled.", false, cx);
        }
    }

    /// Select the subject of the picture with the matte model.
    pub fn select_subject(&mut self, cx: &mut Context<Self>) {
        if matte::available().is_none() {
            self.set_status(missing(Task::Matte), true, cx);
            return;
        }
        let img = self.composite_raster();
        let combine = self.tools.combine;
        let job = Job::new();
        job.set_stage("finding the subject");
        self.watch_job(job.clone(), cx);
        let j = job.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    let img = img.await;
                    let r = matte::matte(&img, &Default::default(), &j);
                    j.finish();
                    r
                })
                .await;
            this.update(cx, |this, cx| match r {
                Ok(m) => {
                    let source = matte::available().map(|m| m.name).unwrap_or("matte model");
                    this.select_from_matte(m, source, None, combine, 20.0, 235.0, cx);
                    this.set_status(
                        "Subject selected — refine it in the Select options.",
                        false,
                        cx,
                    );
                }
                Err(e) => this.set_status(format!("Select subject: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Cut the subject out of the selected pixel node (or the whole
    /// picture) into a new node, and hide the original.
    pub fn remove_background(&mut self, cx: &mut Context<Self>) {
        if matte::available().is_none() {
            self.set_status(missing(Task::Matte), true, cx);
            return;
        }
        // The selected raster node's own pixels, else the composite.
        let source = match self.selected.and_then(|id| self.editor.doc.node(id)) {
            Some(n) if matches!(n.kind, NodeKind::Raster { .. }) => {
                let NodeKind::Raster { raster, placement } = &n.kind else {
                    unreachable!()
                };
                Some((n.id, n.name.clone(), raster.clone(), *placement))
            }
            _ => None,
        };
        let job = Job::new();
        job.set_stage("finding the subject");
        self.watch_job(job.clone(), cx);
        let j = job.clone();
        let composite = self.composite_raster();
        let slot = self.insertion_slot();
        cx.spawn(async move |this, cx| {
            let src = source.clone();
            let r = cx
                .background_spawn(async move {
                    let img: Arc<Raster> = match &src {
                        Some((_, _, r, _)) => r.clone(),
                        None => Arc::new(composite.await),
                    };
                    let r = matte::matte(&img, &Default::default(), &j).map(|m| {
                        let m = matte::harden(&m, 12, 240);
                        matte::cut_out(&img, &m)
                    });
                    j.finish();
                    r
                })
                .await;
            this.update(cx, |this, cx| match r {
                Ok(cut) => {
                    let (name, placement, hide) = match &source {
                        Some((id, name, _, pl)) => (format!("{name} cut-out"), *pl, Some(*id)),
                        None => ("Cut-out".to_string(), Placement::default(), None),
                    };
                    this.editor.begin("Remove background");
                    let node = Node::raster(0, name, Arc::new(cut), placement)
                        .from_model(matte::available().map(|m| m.id).unwrap_or("matte"));
                    if let Some(id) = this.execute(
                        Command::AddNode {
                            node: Box::new(node),
                            slot,
                        },
                        cx,
                    ) {
                        if let Some(h) = hide {
                            this.execute(
                                Command::SetVisible {
                                    id: h,
                                    visible: false,
                                },
                                cx,
                            );
                        }
                        this.selected = Some(id);
                    }
                    this.editor.end();
                    this.set_status(
                        "Background removed into a new node; the original is hidden.",
                        false,
                        cx,
                    );
                }
                Err(e) => this.set_status(format!("Remove background: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Quick select through SAM: a click is a point prompt, a drag a box.
    pub(crate) fn sam_select(
        &mut self,
        pts: Vec<(f64, f64)>,
        combine: Combine,
        cx: &mut Context<Self>,
    ) {
        let Some((&first, &last)) = pts.first().zip(pts.last()) else {
            return;
        };
        let prompt = if (last.0 - first.0).hypot(last.1 - first.1) > 12.0 {
            Prompt::Box(first.0 as f32, first.1 as f32, last.0 as f32, last.1 as f32)
        } else {
            Prompt::Point(first.0 as f32, first.1 as f32)
        };
        let rev = self.editor.revision;
        if let Some((r, emb)) = &self.ai.sam
            && *r == rev
        {
            let emb = emb.clone();
            self.sam_decode(emb, prompt, combine, cx);
            return;
        }
        if self.ai.sam_loading == Some(rev) {
            self.set_status("Still reading the picture…", false, cx);
            return;
        }
        self.ai.sam_loading = Some(rev);
        let img = self.composite_raster();
        let job = Job::new();
        self.watch_job(job.clone(), cx);
        let j = job.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    let img = img.await;
                    let r = sam::encode(&img, &j);
                    j.finish();
                    r
                })
                .await;
            this.update(cx, |this, cx| {
                this.ai.sam_loading = None;
                match r {
                    Ok(emb) => {
                        let emb = Arc::new(emb);
                        this.ai.sam = Some((rev, emb.clone()));
                        this.sam_decode(emb, prompt, combine, cx);
                    }
                    Err(e) => this.set_status(format!("AI select: {e}"), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn sam_decode(
        &mut self,
        emb: Arc<sam::Embedding>,
        prompt: Prompt,
        combine: Combine,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    match prompt {
                        Prompt::Point(x, y) => sam::decode(
                            &emb,
                            &[sam::Point {
                                x,
                                y,
                                positive: true,
                            }],
                            None,
                        ),
                        Prompt::Box(x0, y0, x1, y1) => {
                            sam::decode(&emb, &[], Some((x0, y0, x1, y1)))
                        }
                    }
                })
                .await;
            this.update(cx, |this, cx| match r {
                Ok((m, score)) => {
                    let source = sam::available().map(|m| m.name).unwrap_or("SAM");
                    this.select_from_matte(m, source, Some(score), combine, 96.0, 160.0, cx);
                    this.set_status(
                        format!(
                            "AI select · confidence {:.0} % — refine below",
                            score * 100.0
                        ),
                        false,
                        cx,
                    );
                }
                Err(e) => this.set_status(format!("AI select: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Fill the selection from its surroundings with the inpainting model,
    /// into a new node.
    pub fn ai_fill(&mut self, cx: &mut Context<Self>) {
        if inpaint::available().is_none() {
            self.set_status(missing(Task::Inpaint), true, cx);
            return;
        }
        let Some(sel) = self.editor.doc.selection.clone() else {
            self.set_status("Select the area to fill first.", false, cx);
            return;
        };
        let img = self.composite_raster();
        let job = Job::new();
        self.watch_job(job.clone(), cx);
        let j = job.clone();
        let slot = self.insertion_slot();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    let img = img.await;
                    let r = inpaint::fill(&img, &sel, &j);
                    j.finish();
                    r
                })
                .await;
            this.update(cx, |this, cx| match r {
                Ok((layer, reg)) => {
                    let node = Node::raster(
                        0,
                        "AI fill",
                        Arc::new(layer),
                        Placement::at(reg.x as f64, reg.y as f64),
                    )
                    .from_model(inpaint::available().map(|m| m.id).unwrap_or("lama"));
                    if let Some(id) = this.execute(
                        Command::AddNode {
                            node: Box::new(node),
                            slot,
                        },
                        cx,
                    ) {
                        this.selected = Some(id);
                        this.set_status("Filled into a new node. Hide it to compare.", false, cx);
                    }
                }
                Err(e) => this.set_status(format!("AI fill: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// A grey depth map of the picture as a new node (near is bright).
    pub fn depth_layer(&mut self, cx: &mut Context<Self>) {
        if depth::available().is_none() {
            self.set_status(missing(Task::Depth), true, cx);
            return;
        }
        let img = self.composite_raster();
        let job = Job::new();
        self.watch_job(job.clone(), cx);
        let j = job.clone();
        let slot = self.insertion_slot();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    let img = img.await;
                    let r = depth::estimate(&img, &j).map(|m| m.to_grey_raster());
                    j.finish();
                    r
                })
                .await;
            this.update(cx, |this, cx| match r {
                Ok(grey) => {
                    let node = Node::raster(0, "Depth (AI)", Arc::new(grey), Placement::default())
                        .from_model(depth::available().map(|m| m.id).unwrap_or("depth"));
                    if let Some(id) = this.execute(
                        Command::AddNode {
                            node: Box::new(node),
                            slot,
                        },
                        cx,
                    ) {
                        this.selected = Some(id);
                        this.set_status(
                            "Depth map added: near is bright. Use it as a mask for depth of field or fog.",
                            false,
                            cx,
                        );
                    }
                }
                Err(e) => this.set_status(format!("Depth: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Enlarge the whole picture with the upscale model: the canvas grows by
    /// the model's factor and the result lands as a new node on top.
    pub fn ai_upscale(&mut self, cx: &mut Context<Self>) {
        if upscale::available().is_none() {
            self.set_status(missing(Task::Upscale), true, cx);
            return;
        }
        let f = upscale::factor();
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        if w.saturating_mul(f) > 16_384 || h.saturating_mul(f) > 16_384 {
            self.set_status(
                "Too large to upscale in one go: crop or downsize first.",
                true,
                cx,
            );
            return;
        }
        let img = self.composite_raster();
        let job = Job::new();
        self.watch_job(job.clone(), cx);
        let j = job.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    let img = img.await;
                    let r = upscale::upscale(&img, &j);
                    j.finish();
                    r
                })
                .await;
            this.update(cx, |this, cx| match r {
                Ok(big) => {
                    this.editor.begin(format!("Upscale ×{f}"));
                    this.execute(
                        Command::ImageSize {
                            width: w * f,
                            height: h * f,
                        },
                        cx,
                    );
                    let node = Node::raster(
                        0,
                        format!("Upscaled ×{f} (AI)"),
                        Arc::new(big),
                        Placement::default(),
                    )
                    .from_model(upscale::available().map(|m| m.id).unwrap_or("upscale"));
                    if let Some(id) = this.execute(
                        Command::AddNode {
                            node: Box::new(node),
                            slot: Slot::TOP,
                        },
                        cx,
                    ) {
                        this.selected = Some(id);
                    }
                    this.editor.end();
                    this.fit_pending = true;
                    this.set_status(
                        format!("Upscaled ×{f}: the canvas grew and the result is the top node."),
                        false,
                        cx,
                    );
                }
                Err(e) => this.set_status(format!("Upscale: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Restore every face in the picture into a new node.
    pub fn restore_faces(&mut self, cx: &mut Context<Self>) {
        if face::detector_available().is_none() {
            self.set_status(missing(Task::FaceDetect), true, cx);
            return;
        }
        if face::available().is_none() {
            self.set_status(missing(Task::FaceRestore), true, cx);
            return;
        }
        let img = self.composite_raster();
        let job = Job::new();
        self.watch_job(job.clone(), cx);
        let j = job.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    let img = img.await;
                    let r = face::restore(&img, 1.0, &j);
                    j.finish();
                    r
                })
                .await;
            this.update(cx, |this, cx| match r {
                Ok((restored, n)) => {
                    let node = Node::raster(
                        0,
                        "Faces restored (AI)",
                        Arc::new(restored),
                        Placement::default(),
                    )
                    .from_model(face::available().map(|m| m.id).unwrap_or("gfpgan"));
                    if let Some(id) = this.execute(
                        Command::AddNode {
                            node: Box::new(node),
                            slot: Slot::TOP,
                        },
                        cx,
                    ) {
                        this.selected = Some(id);
                        this.set_status(
                            format!(
                                "Restored {n} face{}: lower the node's opacity to keep it natural.",
                                if n == 1 { "" } else { "s" }
                            ),
                            false,
                            cx,
                        );
                    }
                }
                Err(e) => this.set_status(format!("Restore faces: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }
}

#[derive(Clone, Copy)]
enum Prompt {
    Point(f32, f32),
    Box(f32, f32, f32, f32),
}
