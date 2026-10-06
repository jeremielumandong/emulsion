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

/// Own the terminal worker's completion, including early errors and a future
/// dropped before its first poll. Intermediate phases must keep their job open.
pub(super) fn finishing_job<T>(
    job: Arc<Job>,
    work: impl std::future::Future<Output = T>,
) -> impl std::future::Future<Output = T> {
    struct Completion(Arc<Job>);
    impl Drop for Completion {
        fn drop(&mut self) {
            self.0.finish();
        }
    }
    let completion = Completion(job);
    async move {
        let _completion = completion;
        work.await
    }
}

#[derive(Default)]
pub(crate) struct AiState {
    /// Quick select uses SAM when a model is installed and this is on.
    pub ai_select: bool,
    /// SAM embedding of the composite, by document revision.
    sam: Option<((u64, u64), Arc<sam::Embedding>)>,
    sam_loading: Option<((u64, u64), u64)>,
    /// The running job, for the status line and cancel.
    pub job: Option<Arc<Job>>,
    /// What the running job is doing and when it began, for its card.
    pub job_busy: Option<crate::busy_card::Busy>,
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
    /// Document state after the last application of this refine session.
    epoch: (u64, u64),
}

/// What to tell someone when a task's model is not installed.
pub(crate) fn missing(task: Task) -> String {
    let want = emulsion_ai::models::MANIFEST
        .iter()
        .find(|m| m.task == task && m.default)
        .map(|m| m.name)
        .map(std::borrow::Cow::Borrowed)
        .unwrap_or_else(|| t!("editor.ai_tools.a_model"));
    t!("editor.ai_tools.needs_model", model = want).into_owned()
}

impl EditorView {
    /// The flattened document as a raster, computed off the UI thread.
    pub(crate) fn composite_raster(
        &self,
    ) -> impl std::future::Future<Output = Result<Raster, String>> + use<> {
        let doc = self.editor.doc.clone();
        async move {
            let tree = doc.try_composite_tree().map_err(|e| e.to_string())?;
            let (w, h) = (doc.width, doc.height);
            let px: Vec<[u16; 4]> = region(&tree, IRect::new(0, 0, w as i32, h as i32))
                .into_iter()
                .map(color::f_to_px)
                .collect();
            Ok(Raster::from_pixels(w, h, [0; 4], &px))
        }
    }

    /// Show a job's stage and percentage until it finishes.
    pub(crate) fn watch_job(&mut self, job: Arc<Job>, title: &str, cx: &mut Context<Self>) {
        self.ai.job_busy = Some(crate::busy_card::Busy::new(title.to_string()));
        if let Some(previous) = self.ai.job.replace(job.clone())
            && !Arc::ptr_eq(&previous, &job)
        {
            previous.cancel();
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                let more = this.update(cx, |this, cx| this.poll_ai_job(&job, cx));
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One watcher tick. A terminal or superseded job cannot replace a result's
    /// status message, and only the matching job may retire its busy state.
    fn poll_ai_job(&mut self, job: &Arc<Job>, cx: &mut Context<Self>) -> bool {
        if !self
            .ai
            .job
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, job))
        {
            return false;
        }
        if job.is_finished() || job.cancelled() {
            self.ai.job = None;
            self.ai.job_busy = None;
            cx.notify();
            return false;
        }
        self.set_status(job.summary(), false, cx);
        true
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
        if (raw.width(), raw.height()) != (self.editor.doc.width, self.editor.doc.height) {
            self.set_status(t!("editor.ai_tools.selection_mismatch"), true, cx);
            return;
        }
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
            epoch: self.edit_ticket(),
        });
        self.refine_apply(cx);
    }

    /// Recompute the selection from the kept matte and current settings.
    pub(crate) fn refine_apply(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.ai.refine.clone() else {
            return;
        };
        if self.edit_ticket() != r.epoch
            || (r.raw.width(), r.raw.height()) != (self.editor.doc.width, self.editor.doc.height)
        {
            self.ai.refine = None;
            cx.notify();
            return;
        }
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
        let epoch = self.edit_ticket();
        if let Some(refine) = &mut self.ai.refine {
            refine.epoch = epoch;
        }
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
            .map(|c| {
                t!(
                    "editor.ai_tools.confident",
                    percent = format!("{:.0}", c * 100.0)
                )
                .into_owned()
            })
            .unwrap_or_default();
        Some(
            t!(
                "editor.ai_tools.refine_why",
                source = r.source,
                confidence = conf,
                hi = format!("{:.0}", r.hi),
                lo = format!("{:.0}", r.lo),
                grow = if r.grow >= 0.0 {
                    t!("editor.ai_tools.grown", px = r.grow.abs().round())
                } else {
                    t!("editor.ai_tools.shrunk", px = r.grow.abs().round())
                },
                feather = format!("{:.0}", r.feather)
            )
            .into_owned(),
        )
    }

    /// The running job's progress card, floating at the foot of the canvas.
    /// Quick jobs finish before it appears, so it never flickers.
    pub(super) fn ai_job_card(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let job = self.ai.job.as_ref()?;
        let busy = self.ai.job_busy.as_ref()?;
        if job.is_finished() || busy.started.elapsed() < crate::busy_card::SHOW_AFTER {
            return None;
        }
        let stage = job.stage();
        let busy = match stage.trim() {
            "" => busy.clone(),
            s => {
                let mut chars = s.chars();
                let first = chars.next().map(|c| c.to_uppercase().collect::<String>());
                busy.clone()
                    .detail(format!("{}{}", first.unwrap_or_default(), chars.as_str()))
            }
        };
        let fraction = job.fraction();
        Some(
            div()
                .absolute()
                .bottom(px(24.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    crate::busy_card::busy_card(
                        "ai-job-card",
                        &busy,
                        (fraction > 0.).then_some(fraction),
                        p,
                    )
                    .child(
                        div().flex().justify_end().child(
                            button("ai-job-cancel", t!("shell.cancel"), false, p)
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_ai(cx)))
                                .test_support(),
                        ),
                    ),
                )
                .into_any_element(),
        )
    }

    pub fn cancel_ai(&mut self, cx: &mut Context<Self>) {
        self.ai.job_busy = None;
        if let Some(j) = self.ai.job.take() {
            j.cancel();
            self.ai.sam_loading = None;
            self.set_status(t!("editor.ai_tools.cancelled"), false, cx);
        }
    }

    /// Select the subject of the picture with the matte model.
    pub fn select_subject(&mut self, cx: &mut Context<Self>) {
        if matte::available().is_none() {
            self.set_status(missing(Task::Matte), true, cx);
            return;
        }
        let Some(ticket) = self.selection_ticket() else {
            self.photo_transform_ready(cx);
            return;
        };
        let img = self.composite_raster();
        let combine = self.tools.combine;
        let job = Job::new();
        job.set_stage(t!("editor.ai_tools.stage_finding_subject"));
        self.watch_job(job.clone(), &t!("editor.ai_tools.selecting_subject"), cx);
        let j = job.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(finishing_job(j.clone(), async move {
                    let img = img.await.map_err(emulsion_ai::runner::RunError::Other)?;
                    matte::matte(&img, &Default::default(), &j)
                }))
                .await;
            this.update(cx, |this, cx| {
                if job.cancelled() || !this.selection_is_current(ticket) {
                    return;
                }
                match r {
                    Ok(m) => {
                        let source = matte::available().map(|m| m.name).unwrap_or("matte model");
                        this.select_from_matte(m, source, None, combine, 20.0, 235.0, cx);
                        this.set_status(t!("editor.ai_tools.subject_selected"), false, cx);
                    }
                    Err(e) => this.set_status(
                        t!("editor.ai_tools.select_subject_failed", error = e),
                        true,
                        cx,
                    ),
                }
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
        let Some(ticket) = self.begin_edit_job() else {
            self.photo_transform_ready(cx);
            return;
        };
        let job = Job::new();
        job.set_stage(t!("editor.ai_tools.stage_finding_subject"));
        self.watch_job(job.clone(), &t!("editor.ai_tools.removing_background"), cx);
        let j = job.clone();
        let composite = self.composite_raster();
        let slot = self.insertion_slot();
        cx.spawn(async move |this, cx| {
            let src = source.clone();
            let r = cx
                .background_spawn(finishing_job(j.clone(), async move {
                    let img: Arc<Raster> = match &src {
                        Some((_, _, r, _)) => r.clone(),
                        None => Arc::new(
                            composite
                                .await
                                .map_err(emulsion_ai::runner::RunError::Other)?,
                        ),
                    };
                    matte::matte(&img, &Default::default(), &j).map(|m| {
                        let m = matte::harden(&m, 12, 240);
                        matte::cut_out(&img, &m)
                    })
                }))
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, &t!("editor.ai_tools.remove_background"), cx)
                    || job.cancelled()
                {
                    return;
                }
                match r {
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
                            this.set_layer_selection(vec![id], Some(id));
                        }
                        this.editor.end();
                        this.set_status(t!("editor.ai_tools.background_removed"), false, cx);
                    }
                    Err(e) => this.set_status(
                        t!("editor.ai_tools.remove_background_failed", error = e),
                        true,
                        cx,
                    ),
                }
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
        if ![first.0, first.1, last.0, last.1]
            .iter()
            .all(|v| v.is_finite())
        {
            return;
        }
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let clamp = |p: (f64, f64)| {
            (
                p.0.clamp(0., w.saturating_sub(1) as f64) as f32,
                p.1.clamp(0., h.saturating_sub(1) as f64) as f32,
            )
        };
        let (first, last) = (clamp(first), clamp(last));
        let prompt = if (last.0 - first.0).hypot(last.1 - first.1) > 12.0 {
            Prompt::Box(
                first.0.min(last.0),
                first.1.min(last.1),
                first.0.max(last.0),
                first.1.max(last.1),
            )
        } else {
            Prompt::Point(first.0, first.1)
        };
        let Some(ticket) = self.selection_ticket() else {
            self.photo_transform_ready(cx);
            return;
        };
        let job = Job::new();
        self.watch_job(job.clone(), &t!("editor.ai_tools.selecting_with_ai"), cx);
        if let Some((key, emb)) = &self.ai.sam
            && *key == ticket.0
            && (emb.width, emb.height) == (w, h)
        {
            let emb = emb.clone();
            self.sam_decode(emb, prompt, combine, ticket, job, cx);
            return;
        }
        self.ai.sam_loading = Some(ticket);
        let img = self.composite_raster();
        let j = job.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    let img = img.await.map_err(emulsion_ai::runner::RunError::Other)?;
                    j.check()
                        .map_err(emulsion_ai::runner::RunError::from)
                        .and_then(|_| sam::encode(&img, &j))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.ai.sam_loading == Some(ticket) {
                    this.ai.sam_loading = None;
                }
                if job.cancelled() || !this.selection_is_current(ticket) {
                    job.finish();
                    return;
                }
                match r {
                    Ok(emb)
                        if (emb.width, emb.height)
                            == (this.editor.doc.width, this.editor.doc.height) =>
                    {
                        let emb = Arc::new(emb);
                        this.ai.sam = Some((ticket.0, emb.clone()));
                        this.sam_decode(emb, prompt, combine, ticket, job, cx);
                    }
                    Ok(_) => {
                        job.finish();
                        this.set_status(t!("editor.ai_tools.embedding_mismatch"), true, cx);
                    }
                    Err(e) => {
                        job.finish();
                        this.set_status(
                            t!("editor.ai_tools.ai_select_failed", error = e),
                            true,
                            cx,
                        );
                    }
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
        ticket: ((u64, u64), u64),
        job: Arc<Job>,
        cx: &mut Context<Self>,
    ) {
        let j = job.clone();
        job.set_stage(t!("editor.ai_tools.stage_selecting_object"));
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(finishing_job(j.clone(), async move {
                    j.check()
                        .map_err(emulsion_ai::runner::RunError::from)
                        .and_then(|_| match prompt {
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
                        })
                }))
                .await;
            this.update(cx, |this, cx| {
                if job.cancelled() || !this.selection_is_current(ticket) {
                    return;
                }
                match r {
                    Ok((m, score)) => {
                        let source = sam::available().map(|m| m.name).unwrap_or("SAM");
                        this.select_from_matte(m, source, Some(score), combine, 96.0, 160.0, cx);
                        this.set_status(
                            t!(
                                "editor.ai_tools.ai_select_done",
                                percent = format!("{:.0}", score * 100.0)
                            ),
                            false,
                            cx,
                        );
                    }
                    Err(e) => {
                        this.set_status(t!("editor.ai_tools.ai_select_failed", error = e), true, cx)
                    }
                }
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
            self.set_status(t!("editor.ai_tools.select_area_first"), false, cx);
            return;
        };
        let Some(ticket) = self.selection_ticket() else {
            self.photo_transform_ready(cx);
            return;
        };
        let img = self.composite_raster();
        let job = Job::new();
        self.watch_job(job.clone(), &t!("editor.ai_tools.filling_with_ai"), cx);
        let j = job.clone();
        let slot = self.insertion_slot();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(finishing_job(j.clone(), async move {
                    let img = img.await.map_err(emulsion_ai::runner::RunError::Other)?;
                    inpaint::fill(&img, &sel, &j)
                }))
                .await;
            this.update(cx, |this, cx| {
                if job.cancelled() || !this.selection_is_current(ticket) {
                    return;
                }
                match r {
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
                            this.set_layer_selection(vec![id], Some(id));
                            this.set_status(t!("editor.ai_tools.filled"), false, cx);
                        }
                    }
                    Err(e) => {
                        this.set_status(t!("editor.ai_tools.ai_fill_failed", error = e), true, cx)
                    }
                }
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
        let Some(ticket) = self.begin_edit_job() else {
            self.photo_transform_ready(cx);
            return;
        };
        let img = self.composite_raster();
        let job = Job::new();
        self.watch_job(job.clone(), &t!("editor.ai_tools.building_depth"), cx);
        let j = job.clone();
        let slot = self.insertion_slot();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(finishing_job(j.clone(), async move {
                    let img = img.await.map_err(emulsion_ai::runner::RunError::Other)?;
                    depth::estimate(&img, &j).map(|m| m.to_grey_raster())
                }))
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, &t!("editor.ai_tools.depth"), cx)
                    || job.cancelled()
                {
                    return;
                }
                match r {
                    Ok(grey) => {
                        let node =
                            Node::raster(0, "Depth (AI)", Arc::new(grey), Placement::default())
                                .from_model(depth::available().map(|m| m.id).unwrap_or("depth"));
                        if let Some(id) = this.execute(
                            Command::AddNode {
                                node: Box::new(node),
                                slot,
                            },
                            cx,
                        ) {
                            this.set_layer_selection(vec![id], Some(id));
                            this.set_status(t!("editor.ai_tools.depth_added"), false, cx);
                        }
                    }
                    Err(e) => {
                        this.set_status(t!("editor.ai_tools.depth_failed", error = e), true, cx)
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Enlarge the whole picture with the upscale model: the canvas grows by
    /// the model's factor and the result lands as a new node on top.
    pub fn ai_upscale(&mut self, cx: &mut Context<Self>) {
        // A storyboard panel keeps the project resolution: upscale the layer.
        if self.editor.storyboard().is_some() {
            self.panel_ai_now(emulsion_ai::panels::Op::Upscale, cx);
            return;
        }
        if upscale::available().is_none() {
            self.set_status(missing(Task::Upscale), true, cx);
            return;
        }
        let f = upscale::factor();
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        if w.saturating_mul(f) > 16_384 || h.saturating_mul(f) > 16_384 {
            self.set_status(t!("editor.ai_tools.too_large"), true, cx);
            return;
        }
        let Some(ticket) = self.begin_edit_job() else {
            self.photo_transform_ready(cx);
            return;
        };
        let img = self.composite_raster();
        let job = Job::new();
        self.watch_job(job.clone(), &t!("editor.ai_tools.upscaling"), cx);
        let j = job.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(finishing_job(j.clone(), async move {
                    let img = img.await.map_err(emulsion_ai::runner::RunError::Other)?;
                    upscale::upscale(&img, &j)
                }))
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, &t!("editor.ai_tools.upscale"), cx)
                    || job.cancelled()
                {
                    return;
                }
                match r {
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
                            this.set_layer_selection(vec![id], Some(id));
                        }
                        this.editor.end();
                        this.fit_pending = true;
                        this.set_status(t!("editor.ai_tools.upscaled", factor = f), false, cx);
                    }
                    Err(e) => {
                        this.set_status(t!("editor.ai_tools.upscale_failed", error = e), true, cx)
                    }
                }
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
        let Some(ticket) = self.begin_edit_job() else {
            self.photo_transform_ready(cx);
            return;
        };
        let img = self.composite_raster();
        let job = Job::new();
        self.watch_job(job.clone(), &t!("editor.ai_tools.restoring_faces"), cx);
        let j = job.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(finishing_job(j.clone(), async move {
                    let img = img.await.map_err(emulsion_ai::runner::RunError::Other)?;
                    face::restore(&img, 1.0, &j)
                }))
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, &t!("editor.ai_tools.restore_faces"), cx)
                    || job.cancelled()
                {
                    return;
                }
                match r {
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
                            this.set_layer_selection(vec![id], Some(id));
                            this.set_status(
                                crate::home::recency::plural(
                                    n,
                                    "editor.ai_tools.faces_restored_one",
                                    "editor.ai_tools.faces_restored_many",
                                ),
                                false,
                                cx,
                            );
                        }
                    }
                    Err(e) => this.set_status(
                        t!("editor.ai_tools.restore_faces_failed", error = e),
                        true,
                        cx,
                    ),
                }
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

#[cfg(test)]
mod lifecycle_tests {
    use super::{Combine, EditorView, IRect, Job, Mask, Placement, Slot};
    use emulsion_core::{Command, Document, Node};
    use emulsion_raster::Raster;
    use gpui_kit::{AppContext, Entity, TestAppContext};
    use std::sync::Arc;

    fn editor(cx: &mut TestAppContext) -> Entity<EditorView> {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
        });
        let mut doc = Document::new(8, 8);
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Source",
                Arc::new(Raster::empty(8, 8, [65535, 0, 0, 65535])),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        cx.new(|cx| EditorView::new(doc, None, None, None, "AI lifecycle test".into(), cx))
    }

    #[gpui_kit::test]
    async fn checked_input_failure_finishes_job_and_keeps_error_and_scene(cx: &mut TestAppContext) {
        use emulsion_core::Mapping2;
        use emulsion_raster::projective::Projective2;
        use std::sync::atomic::{AtomicBool, Ordering};
        let view = editor(cx);
        let job = Job::new();
        job.set_stage("Preparing checked input");
        let (before, revision, history, scene) = cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.watch_job(job.clone(), "Checked input", cx);
                (
                    view.editor.doc.clone(),
                    view.editor.revision,
                    view.editor.history.len(),
                    view.tree.clone(),
                )
            })
        });
        let mut rejected = before.clone();
        // A projective component on a Raster owner is rejected by the real
        // checked preparation boundary, even with no attached mask plane.
        rejected.nodes[0].mask_transform = Mapping2::Projective(Projective2::IDENTITY);
        let model_called = Arc::new(AtomicBool::new(false));
        let called = model_called.clone();
        let result = super::finishing_job(job.clone(), async move {
            let _tree = rejected
                .try_composite_tree()
                .map_err(|error| error.to_string())?;
            called.store(true, Ordering::SeqCst);
            Ok::<_, String>(())
        })
        .await;
        let error = result.unwrap_err();
        assert!(error.contains("projective"), "{error}");
        assert!(!model_called.load(Ordering::SeqCst));
        assert!(job.is_finished());
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.set_status(error.clone(), true, cx);
                let status = view.status.clone();
                assert!(!view.poll_ai_job(&job, cx));
                assert!(view.ai.job.is_none());
                assert!(view.ai.job_busy.is_none());
                assert_eq!(
                    view.status, status,
                    "a finished watcher must retain the input error"
                );
                assert!(!view.poll_ai_job(&job, cx), "later ticks stay retired");
                assert_eq!(view.status, status);
                assert_eq!(view.editor.doc, before);
                assert_eq!(
                    (view.editor.revision, view.editor.history.len()),
                    (revision, history)
                );
                assert!(
                    Arc::ptr_eq(&view.tree, &scene),
                    "failed input cannot replace the accepted scene"
                );
            })
        });
    }

    #[gpui_kit::test]
    async fn terminal_job_success_and_cancellation_retire_progress_without_overwriting_status(
        cx: &mut TestAppContext,
    ) {
        let view = editor(cx);
        for cancelled in [false, true] {
            let job = Job::new();
            cx.update(|cx| {
                view.update(cx, |view, cx| {
                    view.watch_job(job.clone(), "Terminal work", cx);
                    if cancelled {
                        view.cancel_ai(cx);
                    }
                })
            });
            let work_job = job.clone();
            let result = super::finishing_job(job.clone(), async move {
                work_job.check().map_err(|error| error.to_string())?;
                Ok::<_, String>(42)
            })
            .await;
            assert_eq!(result.is_err(), cancelled);
            assert!(job.is_finished());
            cx.update(|cx| {
                view.update(cx, |view, cx| {
                    if !cancelled {
                        view.set_status("Work complete", false, cx);
                    }
                    let status = view.status.clone();
                    assert!(!view.poll_ai_job(&job, cx));
                    assert!(view.ai.job.is_none());
                    assert!(view.ai.job_busy.is_none());
                    assert_eq!(view.status, status);
                })
            });
        }
    }

    #[test]
    fn abandoned_terminal_worker_marks_the_job_finished_before_first_poll() {
        let job = Job::new();
        let work = super::finishing_job(job.clone(), std::future::pending::<()>());
        assert!(!job.is_finished());
        drop(work);
        assert!(job.is_finished());
    }

    #[gpui_kit::test]
    async fn ai_composite_uses_captured_document_not_display_tree(cx: &mut TestAppContext) {
        let view = editor(cx);
        let snapshot = cx.update(|cx| {
            view.update(cx, |view, cx| {
                let id = view.editor.doc.nodes[0].id;
                view.execute(
                    Command::ReplacePixels {
                        id,
                        raster: Arc::new(Raster::empty(8, 8, [0, 65535, 0, 65535])),
                        dirty: IRect::new(0, 0, 8, 8),
                        label: "New source".into(),
                    },
                    cx,
                );
                let snapshot = view.composite_raster();
                view.execute(
                    Command::ReplacePixels {
                        id,
                        raster: Arc::new(Raster::empty(8, 8, [0, 0, 65535, 65535])),
                        dirty: IRect::new(0, 0, 8, 8),
                        label: "Later source".into(),
                    },
                    cx,
                );
                snapshot
            })
        });
        let raster = snapshot.await.unwrap();
        assert_eq!(raster.get(4, 4), [0, 65535, 0, 65535]);
    }

    #[gpui_kit::test]
    fn ai_refinement_cannot_restore_a_deselected_or_resized_matte(cx: &mut TestAppContext) {
        let view = editor(cx);
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.select_from_matte(
                    Mask::empty(8, 8, 255),
                    "test",
                    None,
                    Combine::Replace,
                    20.,
                    235.,
                    cx,
                );
                assert!(view.editor.doc.selection.is_some());
                view.set_refine(|r| r.lo = 30., cx);
                assert!(view.ai.refine.is_some(), "own refinement can continue");
                view.deselect(cx);
                view.set_refine(|r| r.feather = 2., cx);
                assert!(view.editor.doc.selection.is_none());
                assert!(view.ai.refine.is_none());
                view.select_from_matte(
                    Mask::empty(8, 8, 255),
                    "test",
                    None,
                    Combine::Replace,
                    20.,
                    235.,
                    cx,
                );
                view.execute(
                    Command::ImageSize {
                        width: 16,
                        height: 16,
                    },
                    cx,
                );
                view.set_refine(|r| r.grow = 2., cx);
                assert!(view.ai.refine.is_none());
                assert_eq!(
                    view.editor
                        .doc
                        .selection
                        .as_ref()
                        .map(|m| (m.width(), m.height())),
                    Some((16, 16))
                );
            })
        });
    }

    #[gpui_kit::test]
    fn ai_new_job_cancels_previous_and_cancel_drops_current_job(cx: &mut TestAppContext) {
        let view = editor(cx);
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                let old = Job::new();
                let current = Job::new();
                view.watch_job(old.clone(), "Old", cx);
                view.watch_job(current.clone(), "Current", cx);
                assert!(old.cancelled());
                assert!(!current.cancelled());
                view.cancel_ai(cx);
                assert!(current.cancelled());
                assert!(view.ai.job.is_none());
            })
        });
    }
}
