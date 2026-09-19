//! Tier-1 local models in the editor: Select Subject, Remove Background and
//! SAM-backed quick selection. Each runs off the UI thread against a
//! snapshot, reports progress in the status line, and lands as an ordinary
//! selection or node so undo and the assistant see nothing special.

use super::*;
use emulsion_ai::jobs::Job;
use emulsion_ai::models::Task;
use emulsion_ai::{depth, inpaint, matte, sam, upscale};
use emulsion_raster::IRect;
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
                    let m = matte::harden(&m, 20, 235);
                    this.apply_selection(m, combine, cx);
                    this.set_status("Subject selected.", false, cx);
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
                    this.apply_selection(matte::harden(&m, 96, 160), combine, cx);
                    this.set_status(
                        format!("AI select · confidence {:.0} %", score * 100.0),
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
}

#[derive(Clone, Copy)]
enum Prompt {
    Point(f32, f32),
    Box(f32, f32, f32, f32),
}
