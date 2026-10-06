//! AI on storyboard panels: subject masks, background removal, upscale,
//! denoise, expand and generative fill on a panel whose size is fixed.
//!
//! [`plan`] turns one panel's drawing and a [`Request`] into ordinary
//! Commands (a selection, a layer mask, or a new layer above the source with
//! the source hidden or kept), so the caller commits them as any other
//! edit. [`run`] does that for several panels with progress, cancel and an
//! error per panel. The models and image providers sit behind [`Backend`]:
//! [`Models`] is the real one, tests use a stub.
//!
//! The canvas never changes size: Upscale keeps the layer's place and size
//! and stores more pixels in it, and Expand shrinks the picture inside the
//! frame and fills the border around it.

use std::collections::BTreeMap;
use std::sync::Arc;

use emulsion_core::command::Slot;
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::{Command, Document, Node, NodeId, NodeKind};
use emulsion_raster::composite::flatten;
use emulsion_raster::{IRect, Mask, Placement, Raster, select};

use crate::generate;
use crate::jobs::Job;
use crate::models::{self, Task};
use crate::prep::Planes;
use crate::{inpaint, matte, upscale};

/// What the operation works on in each panel.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// The whole panel as it looks (every visible layer).
    Panel,
    /// One layer by ID (on the Stage, the selected layer).
    Layer(NodeId),
    /// The first layer with this name (case-insensitive), for batches.
    Named(String),
}

/// Where a generative fill paints.
#[derive(Clone, Debug, PartialEq)]
pub enum Area {
    /// Each panel's own selection.
    Selection,
    /// The opaque pixels of the layer with this name.
    Layer(String),
    /// The whole frame (needs a prompt).
    Whole,
}

/// What a subject mask becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskOutput {
    /// The panel's selection.
    Selection,
    /// A mask on the layer itself.
    LayerMask,
    /// A cut-out copy above the layer, which is hidden (Remove background).
    CutOut,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Subject(MaskOutput),
    Upscale,
    Denoise,
    /// Shrink the picture by `amount` of the frame on every side and fill
    /// the new border; from `prompt` with the image provider, else locally.
    Expand {
        amount: f32,
        prompt: Option<String>,
    },
    /// Fill `area` from `prompt` with the image provider, or from its
    /// surroundings with the local model when there is no prompt.
    Fill {
        prompt: Option<String>,
        area: Area,
    },
}

impl Op {
    /// The Undo label.
    pub fn label(&self) -> &'static str {
        match self {
            Op::Subject(MaskOutput::Selection) => "Select subject",
            Op::Subject(MaskOutput::LayerMask) => "Subject mask",
            Op::Subject(MaskOutput::CutOut) => "Remove background",
            Op::Upscale => "Upscale",
            Op::Denoise => "Denoise",
            Op::Expand { .. } => "Expand",
            Op::Fill {
                prompt: Some(_), ..
            } => "Generative fill",
            Op::Fill { prompt: None, .. } => "AI fill",
        }
    }

    /// The prompt sent to the image provider, if any.
    pub fn prompt(&self) -> Option<&str> {
        match self {
            Op::Expand { prompt, .. } | Op::Fill { prompt, .. } => prompt.as_deref(),
            _ => None,
        }
    }

    /// Blank prompts count as none.
    pub fn normalized(mut self) -> Op {
        if let Op::Expand { prompt, .. } | Op::Fill { prompt, .. } = &mut self {
            *prompt = prompt
                .take()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty());
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub op: Op,
    pub target: Target,
}

/// Models and providers behind the operations.
pub trait Backend: Sync {
    /// Whether `op` can run here, or what to install or set up first.
    fn ready(&self, op: &Op) -> Result<(), String>;
    fn matte(&self, image: &Raster, job: &Job) -> Result<Mask, String>;
    fn upscale_factor(&self) -> u32;
    fn upscale(&self, image: &Raster, job: &Job) -> Result<Raster, String>;
    fn denoise(&self, image: &Raster, job: &Job) -> Result<Raster, String>;
    /// Fill `hole` on `image`: from `prompt` with the image provider, or
    /// from the surroundings with the local model. Returns pixels and where
    /// they go.
    fn fill(
        &self,
        image: &Raster,
        hole: &Mask,
        prompt: Option<&str>,
        job: &Job,
    ) -> Result<(Raster, IRect), String>;
    /// The provenance written on layers `op` makes.
    fn model_id(&self, op: &Op) -> String;
}

/// The installed local models, and the image provider from Settings when
/// one is configured. Nothing goes to a provider without one, and only
/// requests with a prompt use it.
#[derive(Clone, Debug, Default)]
pub struct Models {
    pub image: Option<generate::Config>,
}

const NO_PROVIDER: &str =
    "Prompts need an image provider: choose one under Settings › Image generation.";
const DENOISE_MODEL: &str = "real-esrgan-x4";

fn denoise_ready() -> Result<(), String> {
    models::spec(DENOISE_MODEL)
        .filter(|m| models::status(m) == models::Status::Installed)
        .map(|_| ())
        .ok_or_else(|| {
            let name = models::spec(DENOISE_MODEL).map_or("Real-ESRGAN", |m| m.name);
            format!("Needs {name}: install it under Settings › Local models.")
        })
}

impl Models {
    fn provider(&self) -> Result<&generate::Config, String> {
        let cfg = self.image.as_ref().ok_or(NO_PROVIDER)?;
        cfg.validate().map_err(|e| e.to_string())?;
        Ok(cfg)
    }
}

impl Backend for Models {
    fn ready(&self, op: &Op) -> Result<(), String> {
        let need = |task: Task, ok: bool| {
            if ok {
                Ok(())
            } else {
                Err(models::missing(task))
            }
        };
        match op {
            Op::Subject(_) => need(Task::Matte, matte::available().is_some()),
            Op::Upscale => need(Task::Upscale, upscale::available().is_some()),
            Op::Denoise => denoise_ready(),
            Op::Expand {
                prompt: Some(_), ..
            }
            | Op::Fill {
                prompt: Some(_), ..
            } => self.provider().map(|_| ()),
            Op::Expand { prompt: None, .. } | Op::Fill { prompt: None, .. } => {
                need(Task::Inpaint, inpaint::available().is_some())
            }
        }
    }

    fn matte(&self, image: &Raster, job: &Job) -> Result<Mask, String> {
        matte::matte(image, &Default::default(), job).map_err(|e| e.to_string())
    }

    fn upscale_factor(&self) -> u32 {
        upscale::factor()
    }

    fn upscale(&self, image: &Raster, job: &Job) -> Result<Raster, String> {
        upscale::upscale(image, job).map_err(|e| e.to_string())
    }

    fn denoise(&self, image: &Raster, job: &Job) -> Result<Raster, String> {
        upscale::denoise(image, job).map_err(|e| e.to_string())
    }

    fn fill(
        &self,
        image: &Raster,
        hole: &Mask,
        prompt: Option<&str>,
        job: &Job,
    ) -> Result<(Raster, IRect), String> {
        match prompt {
            Some(prompt) => generate::fill(self.provider()?, image, hole, prompt, None, job)
                .map_err(|e| e.to_string()),
            None => inpaint::fill(image, hole, job).map_err(|e| e.to_string()),
        }
    }

    fn model_id(&self, op: &Op) -> String {
        let id = match op {
            Op::Subject(_) => matte::available().map(|m| m.id),
            Op::Upscale => upscale::available().map(|m| m.id),
            Op::Denoise => Some(DENOISE_MODEL),
            _ if op.prompt().is_some() => {
                return self
                    .image
                    .as_ref()
                    .map_or_else(|| "image-provider".into(), |c| c.model_id());
            }
            _ => inpaint::available().map(|m| m.id),
        };
        id.unwrap_or("local-model").to_string()
    }
}

/// One panel's result: Commands for its drawing and a line on what they do.
#[derive(Clone, Debug)]
pub struct Planned {
    pub commands: Vec<Command>,
    pub summary: String,
}

/// The flattened picture of `doc`.
pub fn flatten_doc(doc: &Document) -> Result<Raster, String> {
    Ok(flatten(
        &doc.try_composite_tree().map_err(|e| e.to_string())?,
        0,
    ))
}

/// `id` alone in canvas space: every other layer hidden.
pub fn layer_on_canvas(doc: &Document, id: NodeId) -> Result<Raster, String> {
    let mut keep = vec![id];
    let mut up = doc.node(id).and_then(|n| n.parent);
    while let Some(parent) = up {
        keep.push(parent);
        up = doc.node(parent).and_then(|n| n.parent);
    }
    let mut solo = doc.clone();
    solo.selection = None;
    for node in &mut solo.nodes {
        let inside = keep.contains(&node.id) || descends(doc, node.id, id);
        node.visible &= inside;
    }
    flatten_doc(&solo)
}

fn descends(doc: &Document, mut node: NodeId, from: NodeId) -> bool {
    while let Some(parent) = doc.node(node).and_then(|n| n.parent) {
        if parent == from {
            return true;
        }
        node = parent;
    }
    false
}

/// Where a layer goes right above `id`, or on top without one.
fn above(doc: &Document, id: Option<NodeId>) -> Slot {
    let Some(node) = id.and_then(|id| doc.node(id)) else {
        return Slot::TOP;
    };
    let siblings = doc.children(node.parent);
    let index = siblings.iter().position(|s| *s == node.id).unwrap_or(0) + 1;
    Slot {
        parent: node.parent,
        index,
    }
}

/// The coverage of `raster`'s alpha as a mask.
fn alpha_mask(raster: &Raster) -> Mask {
    let px: Vec<u8> = raster
        .to_pixels()
        .iter()
        .map(|p| (p[3] >> 8) as u8)
        .collect();
    Mask::from_gray8(raster.width(), raster.height(), &px)
}

/// The border of a `w × h` frame outside `inner`, reaching `overlap` pixels
/// into it so a fill blends with the picture.
pub fn ring(w: u32, h: u32, inner: IRect, overlap: i32) -> Mask {
    let core = IRect::new(
        inner.x + overlap,
        inner.y + overlap,
        (inner.w - 2 * overlap).max(0),
        (inner.h - 2 * overlap).max(0),
    );
    Mask::from_fn(w, h, 0, move |x, y| {
        let (x, y) = (x as i32, y as i32);
        let inside = x >= core.x && x < core.right() && y >= core.y && y < core.bottom();
        if inside { 0 } else { 255 }
    })
}

/// `layers` (raster, placement) composited on a transparent `w × h` frame.
fn compose(w: u32, h: u32, layers: Vec<(Raster, Placement)>) -> Result<Raster, String> {
    let mut doc = Document::new(w, h);
    for (raster, placement) in layers {
        Command::AddNode {
            node: Box::new(Node::raster(0, "layer", Arc::new(raster), placement)),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .map_err(|e| e.to_string())?;
    }
    flatten_doc(&doc)
}

fn resolve(doc: &Document, target: &Target) -> Result<Option<NodeId>, String> {
    match target {
        Target::Panel => Ok(None),
        Target::Layer(id) => doc
            .node(*id)
            .map(|n| Some(n.id))
            .ok_or_else(|| "That layer is no longer on the panel.".into()),
        Target::Named(name) => {
            let want = name.trim().to_lowercase();
            doc.nodes
                .iter()
                .find(|n| n.name.trim().to_lowercase() == want)
                .map(|n| Some(n.id))
                .ok_or_else(|| format!("No layer named “{}”.", name.trim()))
        }
    }
}

/// The layer's own pixels and placement, or the whole panel's picture.
fn native(doc: &Document, id: Option<NodeId>) -> Result<(Raster, Placement, String), String> {
    let Some(id) = id else {
        return Ok((flatten_doc(doc)?, Placement::default(), "Panel".into()));
    };
    let node = doc
        .node(id)
        .ok_or("That layer is no longer on the panel.")?;
    match &node.kind {
        NodeKind::Raster { raster, placement } => {
            Ok(((**raster).clone(), *placement, node.name.clone()))
        }
        _ => Err(format!(
            "“{}” is not a pixel layer; rasterize it first.",
            node.name
        )),
    }
}

fn canvas(doc: &Document, id: Option<NodeId>) -> Result<Raster, String> {
    match id {
        Some(id) => layer_on_canvas(doc, id),
        None => flatten_doc(doc),
    }
}

/// A new layer above the source; the source is hidden when `hide`.
fn add_layer(doc: &Document, source: Option<NodeId>, node: Node, hide: bool) -> Vec<Command> {
    let mut commands = vec![Command::AddNode {
        node: Box::new(node),
        slot: above(doc, source),
    }];
    if let (true, Some(id)) = (hide, source) {
        commands.push(Command::SetVisible { id, visible: false });
    }
    commands
}

fn name_of(doc: &Document, id: Option<NodeId>) -> String {
    id.and_then(|id| doc.node(id))
        .map_or_else(|| "Panel".into(), |n| n.name.clone())
}

fn short(prompt: &str) -> String {
    let s: String = prompt.chars().take(40).collect();
    if s.len() < prompt.len() {
        format!("{s}…")
    } else {
        s
    }
}

/// How many times more pixels Upscale keeps: twice the layer's on-panel
/// size (sharp under a 2× camera push-in), at most the model's factor.
pub fn detail_factor(placement: &Placement, model: u32) -> f64 {
    let shown = placement
        .scale_x
        .abs()
        .max(placement.scale_y.abs())
        .max(1.0);
    (2.0 * shown).min(model as f64).max(1.0)
}

/// Largest side Upscale and Expand produce.
const MAX_SIDE: u32 = 16_384;

/// The Commands that run `request` on one panel's drawing.
pub fn plan(
    doc: &Document,
    request: &Request,
    backend: &dyn Backend,
    job: &Job,
) -> Result<Planned, String> {
    let target = resolve(doc, &request.target)?;
    let op = &request.op;
    let model = backend.model_id(op);
    let (w, h) = (doc.width, doc.height);
    match op {
        Op::Subject(MaskOutput::Selection) => {
            let m = backend.matte(&canvas(doc, target)?, job)?;
            let m = matte::harden(&m, 20, 235);
            if select::bounds(&m).is_empty() {
                return Err("No subject found.".into());
            }
            Ok(Planned {
                commands: vec![Command::SetSelection {
                    selection: Some(Arc::new(m)),
                }],
                summary: "Selected the subject.".into(),
            })
        }
        Op::Subject(MaskOutput::LayerMask) => {
            let id = target.ok_or("Choose a layer to mask.")?;
            if doc.node(id).is_some_and(|n| n.mask.is_some()) {
                return Err(format!(
                    "“{}” already has a layer mask.",
                    name_of(doc, target)
                ));
            }
            let (raster, _, name) = native(doc, target)?;
            let m = matte::harden(&backend.matte(&raster, job)?, 12, 240);
            Ok(Planned {
                commands: vec![Command::SetMask {
                    id,
                    mask: Some(Arc::new(m)),
                }],
                summary: format!("Masked “{name}” to its subject."),
            })
        }
        Op::Subject(MaskOutput::CutOut) => {
            let (raster, placement, name) = native(doc, target)?;
            let m = matte::harden(&backend.matte(&raster, job)?, 12, 240);
            let cut = matte::cut_out(&raster, &m);
            let layer = format!("{name} cut-out");
            let node = Node::raster(0, layer.clone(), Arc::new(cut), placement).from_model(&model);
            Ok(Planned {
                commands: add_layer(doc, target, node, target.is_some()),
                summary: format!("Cut the subject out into “{layer}”."),
            })
        }
        Op::Upscale => {
            let (raster, placement, name) = native(doc, target)?;
            let f = backend.upscale_factor().max(1);
            if raster.width().saturating_mul(f) > MAX_SIDE
                || raster.height().saturating_mul(f) > MAX_SIDE
            {
                return Err(format!("“{name}” is too large to upscale."));
            }
            let k = detail_factor(&placement, f);
            let big = backend.upscale(&raster, job)?;
            let (nw, nh) = (
                ((raster.width() as f64 * k).round() as u32).max(1),
                ((raster.height() as f64 * k).round() as u32).max(1),
            );
            let big = if (big.width(), big.height()) == (nw, nh) {
                big
            } else {
                Planes::from_raster(&big)
                    .resized(nw as usize, nh as usize)
                    .to_raster()
            };
            let placement = Placement {
                scale_x: placement.scale_x / k,
                scale_y: placement.scale_y / k,
                ..placement
            };
            let layer = format!("{name} ×{k:.0} detail");
            let node = Node::raster(0, layer.clone(), Arc::new(big), placement).from_model(&model);
            Ok(Planned {
                commands: add_layer(doc, target, node, target.is_some()),
                summary: format!(
                    "“{layer}” holds {k:.0}× the pixels at the same size on the panel."
                ),
            })
        }
        Op::Denoise => {
            let (raster, placement, name) = native(doc, target)?;
            let clean = backend.denoise(&raster, job)?;
            let layer = format!("{name} denoised");
            let node =
                Node::raster(0, layer.clone(), Arc::new(clean), placement).from_model(&model);
            Ok(Planned {
                commands: add_layer(doc, target, node, target.is_some()),
                summary: format!("Denoised into “{layer}”."),
            })
        }
        Op::Expand { amount, prompt } => {
            let amount = amount.clamp(0.02, 0.5) as f64;
            let scale = 1.0 / (1.0 + 2.0 * amount);
            let (iw, ih) = (
                ((w as f64 * scale).round() as i32).max(1),
                ((h as f64 * scale).round() as i32).max(1),
            );
            let inner = IRect::new((w as i32 - iw) / 2, (h as i32 - ih) / 2, iw, ih);
            let shrunk = compose(
                w,
                h,
                vec![(
                    canvas(doc, target)?,
                    Placement {
                        scale_x: scale,
                        scale_y: scale,
                        ..Placement::at(inner.x as f64, inner.y as f64)
                    },
                )],
            )?;
            job.check().map_err(|e| e.to_string())?;
            let hole = ring(w, h, inner, 4);
            let (fill, at) = backend.fill(&shrunk, &hole, prompt.as_deref(), job)?;
            let expanded = compose(
                w,
                h,
                vec![
                    (shrunk, Placement::default()),
                    (fill, Placement::at(at.x as f64, at.y as f64)),
                ],
            )?;
            let layer = format!("{} expanded", name_of(doc, target));
            let node = Node::raster(0, layer.clone(), Arc::new(expanded), Placement::default())
                .from_model(&model);
            Ok(Planned {
                commands: add_layer(doc, target, node, target.is_some()),
                summary: format!(
                    "Expanded by {:.0} % on each side into “{layer}”.",
                    amount * 100.0
                ),
            })
        }
        Op::Fill { prompt, area } => {
            let hole = match area {
                Area::Selection => doc
                    .selection
                    .as_deref()
                    .cloned()
                    .ok_or("This panel has no selection.")?,
                Area::Layer(name) => {
                    let id = resolve(doc, &Target::Named(name.clone()))?
                        .expect("named layers resolve to a layer");
                    alpha_mask(&layer_on_canvas(doc, id)?)
                }
                Area::Whole if prompt.is_none() => {
                    return Err("Filling the whole frame needs a prompt.".into());
                }
                Area::Whole => Mask::white(w, h),
            };
            let frame = IRect::new(0, 0, w as i32, h as i32);
            if select::bounds(&hole).intersect(&frame).is_empty() {
                return Err("The area to fill is empty.".into());
            }
            let (fill, at) = backend.fill(&canvas(doc, target)?, &hole, prompt.as_deref(), job)?;
            let layer = match prompt {
                Some(p) => format!("Generated: {}", short(p)),
                None => "AI fill".to_string(),
            };
            let node = Node::raster(
                0,
                layer.clone(),
                Arc::new(fill),
                Placement::at(at.x as f64, at.y as f64),
            )
            .from_model(&model);
            Ok(Planned {
                commands: add_layer(doc, target, node, false),
                summary: format!("Filled into “{layer}”; the original is kept."),
            })
        }
    }
}

/// `(panel, panel name, text)`: a summary, an error or why a panel was skipped.
pub type PanelNote = (PageId, String, String);

/// One panel to work on.
#[derive(Clone, Debug)]
pub struct PanelInput {
    pub panel: PageId,
    pub name: String,
    pub doc: Document,
    /// The drawing's revision when it was read, to spot later edits.
    pub revision: u64,
}

/// What a batch did, panel by panel.
#[derive(Debug, Default)]
pub struct Report {
    pub results: Vec<(PageId, String, Result<Planned, String>)>,
    /// Stopped by Cancel; finished panels are in `results`, the rest were
    /// not started.
    pub cancelled: bool,
}

impl Report {
    /// The Commands for every panel that succeeded.
    pub fn edits(&self) -> BTreeMap<PageId, Vec<Command>> {
        self.results
            .iter()
            .filter_map(|(id, _, r)| r.as_ref().ok().map(|p| (*id, p.commands.clone())))
            .collect()
    }

    /// `(panel name, error)` for every panel that failed.
    pub fn errors(&self) -> Vec<PanelNote> {
        self.results
            .iter()
            .filter_map(|(id, name, r)| r.as_ref().err().map(|e| (*id, name.clone(), e.clone())))
            .collect()
    }
}

/// Run `request` on each panel in turn off the UI thread. `job` shows which
/// panel is running and how many are done; cancelling it stops after the
/// current panel's model call returns. Fails as a whole only when the
/// operation cannot run here at all (a missing model or provider).
pub fn run(
    panels: &[PanelInput],
    request: &Request,
    backend: &dyn Backend,
    job: &Arc<Job>,
) -> Result<Report, String> {
    backend.ready(&request.op)?;
    let mut report = Report::default();
    let n = panels.len().max(1) as f32;
    for (i, panel) in panels.iter().enumerate() {
        if job.cancelled() {
            report.cancelled = true;
            break;
        }
        job.set_stage(format!("{} · {} of {}", panel.name, i + 1, panels.len()));
        let step = job.child();
        let result = plan(&panel.doc, request, backend, &step);
        if job.cancelled() {
            report.cancelled = true;
            break;
        }
        report
            .results
            .push((panel.panel, panel.name.clone(), result));
        job.progress((i + 1) as f32 / n);
    }
    Ok(report)
}

/// Panels of `editor` to run on, in board order, with locked or missing
/// ones reported as `(panel, name, why)` instead.
pub fn gather(
    editor: &ProjectEditor,
    ids: &[PageId],
) -> Result<(Vec<PanelInput>, Vec<PanelNote>), String> {
    let board = editor
        .storyboard()
        .ok_or("This is not a storyboard project.")?;
    let (mut inputs, mut skipped) = (Vec::new(), Vec::new());
    for id in ids {
        if !editor.page_list().iter().any(|m| m.id == *id) {
            skipped.push((*id, format!("Panel {id}"), "Panel does not exist.".into()));
        }
    }
    for meta in editor.page_list().iter().filter(|m| ids.contains(&m.id)) {
        let page = editor.page(meta.id).expect("listed page");
        if board.is_locked(meta.id) {
            skipped.push((meta.id, meta.name.clone(), "Locked panel.".into()));
            continue;
        }
        inputs.push(PanelInput {
            panel: meta.id,
            name: meta.name.clone(),
            doc: page.doc.clone(),
            revision: page.revision,
        });
    }
    if inputs.is_empty() && skipped.is_empty() {
        return Err("Choose at least one panel.".into());
    }
    Ok((inputs, skipped))
}

/// What a batch changed on the board.
#[derive(Debug, Default)]
pub struct Applied {
    /// `(panel, name, summary)` for each changed panel.
    pub done: Vec<PanelNote>,
    /// `(panel, name, error)` for each panel left as it was.
    pub failed: Vec<PanelNote>,
    pub cancelled: bool,
}

impl Applied {
    /// One line for a status bar.
    pub fn summary(&self, label: &str) -> String {
        let panels = |n: usize| {
            if n == 1 {
                "1 panel".into()
            } else {
                format!("{n} panels")
            }
        };
        if self.cancelled {
            return format!("{label} cancelled: no panel was changed.");
        }
        let mut line = format!("{label}: {} changed", panels(self.done.len()));
        if !self.failed.is_empty() {
            line.push_str(&format!(", {} failed", panels(self.failed.len())));
            if let Some((_, name, why)) = self.failed.first() {
                line.push_str(&format!(" ({name}: {why})"));
            }
        }
        line.push('.');
        line
    }
}

/// Commit a finished batch as one Undo step. A cancelled batch changes
/// nothing; a panel drawn on while the batch ran is left alone and reported.
pub fn commit(
    editor: &mut ProjectEditor,
    inputs: &[PanelInput],
    report: Report,
    skipped: Vec<PanelNote>,
    label: &str,
) -> Result<Applied, String> {
    let mut applied = Applied {
        failed: skipped,
        cancelled: report.cancelled,
        ..Applied::default()
    };
    if report.cancelled {
        return Ok(applied);
    }
    let mut edits = BTreeMap::new();
    for (id, name, result) in report.results {
        let current = editor.page(id).map(|p| p.revision);
        let read = inputs.iter().find(|i| i.panel == id).map(|i| i.revision);
        match result {
            Ok(_) if current.is_none() || current != read => applied.failed.push((
                id,
                name,
                "The panel changed while the AI ran; run it again.".into(),
            )),
            Ok(planned) => {
                edits.insert(id, planned.commands);
                applied.done.push((id, name, planned.summary));
            }
            Err(e) => applied.failed.push((id, name, e)),
        }
    }
    if !edits.is_empty() {
        let n = edits.len();
        let label = if n == 1 {
            label.to_string()
        } else {
            format!("{label} on {n} panels")
        };
        editor.edit_panels(edits, &label)?;
    }
    Ok(applied)
}

#[cfg(test)]
#[path = "panels_tests.rs"]
mod tests;
