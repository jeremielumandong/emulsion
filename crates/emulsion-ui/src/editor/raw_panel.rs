//! The RAW develop panel: exposure, white balance, highlights and
//! shadows for a document opened from a camera RAW. The decoded RAW is
//! developed on a worker; only the current request can commit its pixels
//! and persisted recipe together. Decoded mosaics are released after use.

use super::*;
use emulsion_core::raw::RawDocument;
use emulsion_io::raw::{DevelopParams, RawSource};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Disableable, Selectable, Sizable};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum RawSection {
    #[default]
    Adjust,
    Curve,
    Settings,
}

#[derive(Clone, Copy, Default)]
enum RawAnalysis {
    #[default]
    None,
    Auto,
    Neutral(u32, u32),
}

#[derive(Default)]
pub struct RawState {
    section: RawSection,
    pub(super) settings_group: emulsion_io::raw_settings::RawSettingsGroup,
    pub(super) settings_busy: bool,
    analysis: RawAnalysis,
    pub(super) picking_neutral: bool,
    preview: Option<RawSection>,
    clipping: bool,
    split_requested: bool,
    split_tree: Option<Arc<CompositeTree>>,
    baseline: Option<RawDocument>,
    draft: Option<DevelopParams>,
    /// Bumped per change; a develop only lands if it is still the newest.
    generation: u64,
    busy: bool,
    debounce: Option<Task<()>>,
    cancel: Option<Arc<AtomicBool>>,
    error: Option<String>,
    completion: Option<async_channel::Sender<()>>,
}

impl RawState {
    pub(crate) fn is_pending(&self) -> bool {
        self.draft.is_some()
    }

    fn cancel_pending(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.debounce = None;
        self.draft = None;
        self.analysis = RawAnalysis::None;
        self.picking_neutral = false;
        self.preview = None;
        self.clipping = false;
        self.split_requested = false;
        self.split_tree = None;
        self.completion = None;
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Drop for RawState {
    fn drop(&mut self) {
        self.cancel_pending();
    }
}

/// One panel slider: key, label, value text, normalised position, spec.
type RawRow = (&'static str, &'static str, String, f32, (f32, f32, f32));

/// Debounce between the last slider move and the develop.
const SETTLE_MS: u64 = 220;

impl EditorView {
    pub(crate) fn raw_comparison_wait(
        &mut self,
        request: &emulsion_mcp::raw_preview::Comparison,
        cx: &mut Context<Self>,
    ) -> Result<(async_channel::Receiver<()>, u64), String> {
        use emulsion_mcp::raw_preview::Mode;
        if self.editor.doc.raw.is_none() {
            return Err("This document has no editable RAW source".into());
        }
        if self.raw.is_pending() {
            return Err("Wait for pending RAW development before changing comparison".into());
        }
        if request.mode == Mode::Split && self.raw_split_active() {
            self.compare = request.position;
            cx.notify();
            let (_, receiver) = async_channel::bounded(1);
            return Ok((receiver, self.raw.generation));
        }
        self.raw_clear_preview(cx);
        match request.mode {
            Mode::Edited => {
                self.raw.error = None;
            }
            Mode::Split => {
                self.raw_toggle_split(cx);
                self.compare = request.position;
            }
            Mode::WithoutTone => self.raw_preview(RawSection::Adjust, false, cx),
            Mode::WithoutCurve => self.raw_preview(RawSection::Curve, false, cx),
            Mode::Clipping => self.raw_preview(RawSection::Adjust, true, cx),
        }
        let (sender, receiver) = async_channel::bounded(1);
        if self.raw.is_pending() {
            self.raw.completion = Some(sender);
        }
        Ok((receiver, self.raw.generation))
    }

    pub(crate) fn raw_comparison_result(&self, generation: u64) -> Result<(), String> {
        if self.raw.generation != generation {
            return Err("RAW comparison was cancelled or superseded".into());
        }
        if let Some(error) = &self.raw.error {
            return Err(error.clone());
        }
        Ok(())
    }

    /// RAW ownership is persisted by node identity, never inferred from order.
    fn raw_node(&self) -> Option<NodeId> {
        self.editor.doc.raw.as_ref().map(|raw| raw.node_id)
    }

    pub(crate) fn cancel_raw_develop(&mut self) {
        if self.raw.split_requested {
            self.compare = 0.0;
            if matches!(self.drag, Some(Drag::Compare)) {
                self.drag = None;
            }
            self.gen_counter += 1;
            self.before_gen = self.gen_counter;
            self.cache.borrow_mut().clear_which(Which::Before);
        }
        if self.raw.preview.is_some() {
            // Force the normal tree to replace any transient diagnostic image.
            self.seen_rev = u64::MAX;
            self.tree_dirty = emulsion_core::Dirty::All;
        }
        self.raw.cancel_pending();
        self.raw.baseline = self.editor.doc.raw.clone();
    }

    pub(crate) fn raw_split_requested(&self) -> bool {
        self.raw.split_requested
    }

    pub(crate) fn raw_split_active(&self) -> bool {
        self.raw.split_requested && self.raw.split_tree.is_some()
    }

    pub(crate) fn raw_split_tree(&self) -> Option<Arc<CompositeTree>> {
        self.raw.split_tree.clone()
    }

    /// The before image is a real development using the original as-shot
    /// defaults, not the last save or the camera's embedded JPEG.
    pub(crate) fn raw_toggle_split(&mut self, cx: &mut Context<Self>) {
        if self.raw.split_requested {
            self.raw_clear_preview(cx);
            return;
        }
        if self.raw.is_pending() || self.editor.doc.raw.is_none() {
            return;
        }
        self.raw_clear_preview(cx);
        self.raw.split_requested = true;
        self.compare = 0.5;
        self.raw.draft = Some(DevelopParams::default());
        self.schedule_develop(cx);
    }

    /// A RAW slider moved: remember the value and develop once it settles.
    pub(crate) fn raw_slider(&mut self, name: &'static str, v: f32, cx: &mut Context<Self>) {
        if !v.is_finite()
            || !matches!(
                name,
                "exposure"
                    | "temperature"
                    | "tint"
                    | "highlights"
                    | "shadows"
                    | "black_point"
                    | "brightness"
                    | "contrast"
                    | "saturation"
                    | "curve0"
                    | "curve1"
                    | "curve2"
                    | "curve3"
                    | "curve4"
            )
        {
            return;
        }
        let Some(raw) = self.editor.doc.raw.clone() else {
            return;
        };
        if self.raw.baseline.as_ref() != Some(&raw) {
            self.cancel_raw_develop();
        }
        if self.raw.preview.is_some() || self.raw.split_requested {
            self.raw_clear_preview(cx);
        }
        self.raw.preview = None;
        self.raw.clipping = false;
        self.raw.analysis = RawAnalysis::None;
        let p = self.raw.draft.get_or_insert(raw.params);
        match name {
            "exposure" => p.exposure = v / 100.0,
            "temperature" => p.temperature = v / 100.0,
            "tint" => p.tint = v / 100.0,
            "highlights" => p.highlights = v / 100.0,
            "shadows" => p.shadows = v / 100.0,
            "black_point" => p.black_point = v / 100.0,
            "brightness" => p.brightness = v / 100.0,
            "contrast" => p.contrast = v / 100.0,
            "saturation" => p.saturation = v / 100.0,
            "curve0" | "curve1" | "curve2" | "curve3" | "curve4" => {
                let ix = (name.as_bytes()[5] - b'0') as usize;
                let low = if ix == 0 { 0.0 } else { p.tone_curve[ix - 1] };
                let high = if ix == 4 { 1.0 } else { p.tone_curve[ix + 1] };
                p.tone_curve[ix] = (v / 100.0).clamp(low, high);
            }
            _ => return,
        }
        self.schedule_develop(cx);
    }

    pub(crate) fn raw_params(&self) -> Option<DevelopParams> {
        self.editor.doc.raw.as_ref().map(|raw| {
            if self.raw.split_requested {
                raw.params
            } else {
                self.raw.draft.unwrap_or(raw.params)
            }
        })
    }

    pub(crate) fn raw_apply_params(&mut self, params: DevelopParams, cx: &mut Context<Self>) {
        if self.editor.doc.raw.is_none() || params.validate().is_err() {
            return;
        }
        self.cancel_raw_develop();
        self.raw.draft = Some(params);
        self.schedule_develop(cx);
    }

    /// A batch waits for one photo before starting the next, so decoded sensor
    /// buffers cannot accumulate behind the global development lock.
    pub(crate) fn raw_apply_params_wait(
        &mut self,
        params: DevelopParams,
        cx: &mut Context<Self>,
    ) -> async_channel::Receiver<()> {
        let (sender, receiver) = async_channel::bounded(1);
        if self.editor.doc.raw.is_some() && params.validate().is_ok() {
            self.raw_apply_params(params, cx);
            self.raw.completion = Some(sender);
        }
        receiver
    }

    fn raw_auto(&mut self, cx: &mut Context<Self>) {
        let Some(params) = self.raw_params() else {
            return;
        };
        self.cancel_raw_develop();
        self.raw.draft = Some(params);
        self.raw.analysis = RawAnalysis::Auto;
        self.schedule_develop(cx);
    }

    pub(crate) fn raw_neutral_at(&mut self, point: (f64, f64), cx: &mut Context<Self>) {
        self.raw.picking_neutral = false;
        let Some(raw) = self.editor.doc.raw.as_ref() else {
            return;
        };
        let Some(node) = self.editor.doc.node(raw.node_id) else {
            return;
        };
        let local = emulsion_core::transform::local_to_document(node)
            .inverse()
            .transform_point2(glam::dvec2(point.0, point.1));
        let dimensions = match &node.kind {
            NodeKind::Raster { raster, .. } => (raster.width(), raster.height()),
            NodeKind::Smart { source, .. } => (source.width(), source.height()),
            _ => return,
        };
        if !local.is_finite()
            || local.x < 0.0
            || local.y < 0.0
            || local.x >= dimensions.0 as f64
            || local.y >= dimensions.1 as f64
        {
            self.set_status("Choose a neutral point inside the RAW photo.", true, cx);
            return;
        }
        let params = self.raw_params().unwrap();
        self.cancel_raw_develop();
        self.raw.draft = Some(params);
        self.raw.analysis = RawAnalysis::Neutral(local.x as u32, local.y as u32);
        self.schedule_develop(cx);
    }

    pub(crate) fn raw_clear_preview(&mut self, cx: &mut Context<Self>) {
        self.cancel_raw_develop();
        self.seen_rev = u64::MAX;
        self.sync_trees(cx);
        cx.notify();
    }

    pub(crate) fn raw_cancel_interaction(&mut self, cx: &mut Context<Self>) -> bool {
        if self.raw.preview.is_some() || self.raw.picking_neutral || self.raw.split_requested {
            self.raw_clear_preview(cx);
            true
        } else {
            false
        }
    }

    fn raw_preview(&mut self, section: RawSection, clipping: bool, cx: &mut Context<Self>) {
        if self.raw.is_pending() {
            return;
        }
        if self.raw.preview == Some(section) && self.raw.clipping == clipping {
            self.raw_clear_preview(cx);
            return;
        }
        let Some(mut params) = self.raw_params() else {
            return;
        };
        self.cancel_raw_develop();
        if !clipping {
            params = emulsion_io::raw_settings::merge_settings(
                params,
                DevelopParams::default(),
                if section == RawSection::Curve {
                    emulsion_io::raw_settings::RawSettingsGroup::Curve
                } else {
                    emulsion_io::raw_settings::RawSettingsGroup::Tone
                },
            );
        }
        self.raw.preview = Some(section);
        self.raw.clipping = clipping;
        self.raw.draft = Some(params);
        self.schedule_develop(cx);
    }

    pub(crate) fn raw_reset(&mut self, cx: &mut Context<Self>) {
        if self.editor.doc.raw.is_none() {
            return;
        }
        self.cancel_raw_develop();
        self.raw.draft = Some(DevelopParams::default());
        self.schedule_develop(cx);
    }

    fn schedule_develop(&mut self, cx: &mut Context<Self>) {
        self.raw.generation = self.raw.generation.wrapping_add(1);
        if let Some(cancel) = &self.raw.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        self.raw.error = None;
        let generation = self.raw.generation;
        cx.notify();
        self.raw.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(SETTLE_MS))
                .await;
            this.update(cx, |this, cx| {
                if this.raw.generation == generation {
                    this.develop_now(cx);
                }
            })
            .ok();
        }));
    }

    fn develop_now(&mut self, cx: &mut Context<Self>) {
        if self.raw.busy {
            return;
        }
        let (Some(raw), Some(params)) = (self.editor.doc.raw.clone(), self.raw.draft) else {
            return;
        };
        if params == raw.params
            && matches!(self.raw.analysis, RawAnalysis::None)
            && self.raw.preview.is_none()
            && !self.raw.split_requested
        {
            self.raw.draft = None;
            self.raw.completion = None;
            cx.notify();
            return;
        }
        let ticket = self.edit_ticket();
        let generation = self.raw.generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.raw.cancel = Some(cancel.clone());
        self.raw.busy = true;
        let source = raw.clone();
        let analysis = self.raw.analysis;
        let split = self.raw.split_requested;
        let preview = self.raw.preview.is_some() || split;
        let clipping = self.raw.clipping;
        let mut preview_doc = preview.then(|| self.editor.doc.clone());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(emulsion_io::IoError::Unsupported(
                            "RAW development cancelled".into(),
                        ));
                    }
                    let src = RawSource::load_verified(&source.source, &source.source_sha256)?;
                    let params = match analysis {
                        RawAnalysis::None => params,
                        RawAnalysis::Auto => src.auto_adjust(&params)?,
                        RawAnalysis::Neutral(x, y) => src.neutral_white_balance(&params, x, y)?,
                    };
                    let mut raster = src.develop_with_cancel(&params, &cancel)?;
                    if clipping {
                        // A diagnostic of output clipping, never saved into the document.
                        let pixels: Vec<_> = raster
                            .to_pixels()
                            .into_iter()
                            .map(|p| {
                                if p[..3].contains(&u16::MAX) {
                                    [65535, 0, 0, 65535]
                                } else if p[..3].iter().all(|v| *v == 0) {
                                    [0, 0, 65535, 65535]
                                } else {
                                    let y = ((p[0] as u32 + p[1] as u32 + p[2] as u32) / 3) as u16;
                                    [y, y, y, p[3]]
                                }
                            })
                            .collect();
                        raster =
                            Raster::from_pixels(raster.width(), raster.height(), [0; 4], &pixels);
                    }
                    let raster = Arc::new(raster);
                    let tree = if let Some(doc) = &mut preview_doc {
                        Command::DevelopRaw {
                            id: source.node_id,
                            raster: raster.clone(),
                            params,
                        }
                        .apply(doc)
                        .map_err(|e| emulsion_io::IoError::Unsupported(e.to_string()))?;
                        Some(doc.composite_tree())
                    } else {
                        None
                    };
                    Ok((params, raster, tree))
                })
                .await;
            this.update(cx, |this, cx| {
                this.raw.busy = false;
                this.raw.cancel = None;
                if this.raw.generation != generation {
                    // The next request starts only after this worker releases its mosaic.
                    this.develop_now(cx);
                    cx.notify();
                    return;
                }
                if this.edit_ticket() != ticket
                    || (this.editor.in_transaction() && !preview)
                    || this.editor.doc.raw.as_ref() != Some(&raw)
                {
                    this.cancel_raw_develop();
                    cx.notify();
                    return;
                }
                this.raw.draft = None;
                this.raw.completion = None;
                match result {
                    Ok((params, raster, tree)) => {
                        if let Some(tree) = tree {
                            if split {
                                this.raw.split_tree = Some(Arc::new(tree));
                                this.gen_counter += 1;
                                this.before_gen = this.gen_counter;
                                this.cache.borrow_mut().clear_which(Which::Before);
                                cx.notify();
                                return;
                            }
                            this.seen_rev = this.editor.revision;
                            this.tree_dirty = emulsion_core::Dirty::All;
                            this.install_tree(tree);
                            cx.notify();
                            return;
                        }
                        this.execute(
                            Command::DevelopRaw {
                                id: raw.node_id,
                                raster,
                                params,
                            },
                            cx,
                        );
                    }
                    Err(e) => {
                        if preview {
                            this.raw_clear_preview(cx);
                        }
                        this.raw.error =
                            Some(format!("{e}. The saved image and settings are unchanged."));
                        this.set_status(format!("Develop failed: {e}"), true, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn relink_raw(&mut self, cx: &mut Context<Self>) {
        let Some(raw) = self.editor.doc.raw.clone() else {
            return;
        };
        self.cancel_raw_develop();
        let ticket = self.edit_ticket();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Locate original RAW".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let expected = raw.source_sha256.clone();
            let result = cx
                .background_spawn(async move {
                    if emulsion_io::raw::source_digest(&path)? != expected {
                        return Err(emulsion_io::IoError::Unsupported(
                            "The selected file does not match the original RAW".into(),
                        ));
                    }
                    Ok(std::fs::canonicalize(path)?)
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.edit_is_current(ticket) || this.editor.doc.raw.as_ref() != Some(&raw) {
                    return;
                }
                match result {
                    Ok(source) => {
                        this.execute(Command::RelinkRaw { source }, cx);
                        this.raw.error = None;
                    }
                    Err(e) => this.set_status(format!("Could not relink RAW: {e}"), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// The panel, for the RAW's own node only.
    pub(crate) fn raw_panel(
        &mut self,
        id: NodeId,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.raw_node() != Some(id) {
            return None;
        }
        let raw = self.editor.doc.raw.as_ref()?;
        let prm = if self.raw.split_requested {
            raw.params
        } else {
            self.raw.draft.unwrap_or(raw.params)
        };
        let metadata = &raw.metadata;
        let depth = if metadata.bits_per_sample == 0 {
            "unknown bit depth".into()
        } else {
            format!("{}-bit", metadata.bits_per_sample)
        };
        let description = format!(
            "{} {} · {} · {} · {} · {}",
            metadata.make,
            metadata.model,
            metadata.format,
            depth,
            metadata.sensor,
            metadata.compression
        );
        let warnings = metadata.warnings.join("; ");
        let mut body = div().flex().flex_col().gap_2().pt_1().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(label("RAW develop", p))
                .child(div().flex_1())
                .child(mono(
                    if self.raw.is_pending() {
                        "developing…".to_string()
                    } else {
                        String::new()
                    },
                    9.5,
                    p.muted,
                ))
                .child(
                    Button::new("raw-reset")
                        .label("Reset RAW")
                        .xsmall()
                        .ghost()
                        .disabled(prm == DevelopParams::default())
                        .on_click(cx.listener(|this, _, _, cx| this.raw_reset(cx))),
                ),
        );
        body = body.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(description),
        );
        if !warnings.is_empty() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(warnings),
            );
        }
        if let Some(error) = &self.raw.error {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        body = body.child(
            Button::new("raw-relink")
                .label("Locate original…")
                .xsmall()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| this.relink_raw(cx))),
        );
        let mut sections = div().flex().flex_wrap().gap_1();
        for (key, title, section) in [
            ("raw-adjust", "Adjust", RawSection::Adjust),
            ("raw-curve", "Curve", RawSection::Curve),
            ("raw-settings", "Settings", RawSection::Settings),
        ] {
            sections = sections.child(
                Button::new(key)
                    .label(title)
                    .xsmall()
                    .ghost()
                    .selected(self.raw.section == section)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.raw.section = section;
                        cx.notify();
                    })),
            );
        }
        body = body.child(sections);
        body = body.child(
            Button::new("raw-before-after")
                .label(if self.raw.split_requested {
                    "Close before / after"
                } else {
                    "Before / after"
                })
                .xsmall()
                .ghost()
                .selected(self.raw.split_requested)
                .disabled(self.raw.is_pending() && !self.raw.split_requested)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.raw_toggle_split(cx);
                    window.focus(&this.canvas_focus, cx);
                })),
        );
        if self.raw.split_requested {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.raw.split_tree.is_some() {
                        "Drag the divider: as-shot RAW on the left, edited photo on the right. Escape closes."
                    } else {
                        "Preparing as-shot RAW comparison…"
                    }),
            );
        }
        if self.raw.preview.is_some() {
            body =
                body.child(div().text_xs().text_color(cx.theme().warning).child(
                    if self.raw.clipping {
                        "Output clipping: red highlights, blue black. Not saved or exported."
                    } else {
                        "Comparison only: this section is bypassed. Saved edits are unchanged."
                    },
                ))
                .child(
                    Button::new("raw-preview-end")
                        .label("Show edited photo")
                        .xsmall()
                        .on_click(cx.listener(|this, _, _, cx| this.raw_clear_preview(cx))),
                );
        }
        if self.raw.section == RawSection::Settings {
            return Some(body.child(self.render_raw_settings(cx)).into_any_element());
        }
        if self.raw.section == RawSection::Curve {
            let mut presets = div().flex().flex_wrap().gap_1();
            for (key, title, curve) in [
                ("raw-curve-linear", "Linear", DevelopParams::LINEAR_CURVE),
                (
                    "raw-curve-medium",
                    "Medium contrast",
                    DevelopParams::MEDIUM_CONTRAST_CURVE,
                ),
                (
                    "raw-curve-strong",
                    "Strong contrast",
                    DevelopParams::STRONG_CONTRAST_CURVE,
                ),
            ] {
                presets = presets.child(
                    Button::new(key)
                        .label(title)
                        .xsmall()
                        .ghost()
                        .selected(prm.tone_curve == curve)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(mut params) = this.raw_params() {
                                params.tone_curve = curve;
                                this.raw_apply_params(params, cx);
                            }
                        })),
                );
            }
            body = body.child(presets).child(div().text_xs().text_color(cx.theme().muted_foreground)
                .child("Luminance curve after tone adjustments. Output levels for five fixed input points (gamma 2.2)."));
            for (ix, key) in ["curve0", "curve1", "curve2", "curve3", "curve4"]
                .into_iter()
                .enumerate()
            {
                let value = prm.tone_curve[ix];
                body = body.child(self.param_slider(
                    SliderKey::Raw(key),
                    [
                        "Black · 0%",
                        "Shadows · 25%",
                        "Midtones · 50%",
                        "Lights · 75%",
                        "White · 100%",
                    ][ix],
                    format!("{:.0}%", value * 100.0),
                    value,
                    (0.0, 100.0, 1.0),
                    p,
                    cx,
                ));
            }
            return Some(
                body.child(
                    Button::new("raw-curve-preview")
                        .label("Compare without curve")
                        .xsmall()
                        .ghost()
                        .disabled(self.raw.is_pending())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.raw_preview(RawSection::Curve, false, cx)
                        })),
                )
                .into_any_element(),
            );
        }
        body = body.child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    Button::new("raw-auto")
                        .label("Auto tone")
                        .xsmall()
                        .ghost()
                        .disabled(self.raw.is_pending())
                        .on_click(cx.listener(|this, _, _, cx| this.raw_auto(cx))),
                )
                .child(
                    Button::new("raw-neutral")
                        .label(if self.raw.picking_neutral {
                            "Cancel picker"
                        } else {
                            "Pick neutral"
                        })
                        .xsmall()
                        .ghost()
                        .disabled(self.raw.is_pending())
                        .selected(self.raw.picking_neutral)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.raw.picking_neutral = !this.raw.picking_neutral;
                            window.focus(&this.canvas_focus, cx);
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("raw-wb-as-shot")
                        .label("As-shot WB")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(mut params) = this.raw_params() {
                                params.wb_override = None;
                                params.temperature = 0.0;
                                params.tint = 0.0;
                                this.raw_apply_params(params, cx);
                            }
                        })),
                ),
        );
        if self.raw.picking_neutral {
            body = body.child(
                div()
                    .text_xs()
                    .child("Click a neutral gray area in the photo. Escape cancels."),
            );
        }
        let rows: [RawRow; 9] = [
            (
                "exposure",
                "exposure",
                format!("{:+.2} EV", prm.exposure),
                (prm.exposure + 3.0) / 6.0,
                (-300.0, 300.0, 5.0),
            ),
            (
                "temperature",
                "temperature",
                if prm.temperature.abs() < 0.005 {
                    if prm.wb_override.is_some() {
                        "sampled".into()
                    } else {
                        "as shot".into()
                    }
                } else if prm.temperature > 0.0 {
                    format!("warmer {:.0}", prm.temperature * 100.0)
                } else {
                    format!("cooler {:.0}", -prm.temperature * 100.0)
                },
                (prm.temperature + 1.0) / 2.0,
                (-100.0, 100.0, 1.0),
            ),
            (
                "tint",
                "tint",
                if prm.tint.abs() < 0.005 {
                    if prm.wb_override.is_some() {
                        "sampled".into()
                    } else {
                        "as shot".into()
                    }
                } else if prm.tint > 0.0 {
                    format!("magenta {:.0}", prm.tint * 100.0)
                } else {
                    format!("green {:.0}", -prm.tint * 100.0)
                },
                (prm.tint + 1.0) / 2.0,
                (-100.0, 100.0, 1.0),
            ),
            (
                "highlights",
                "highlights",
                format!("{:.0}", prm.highlights * 100.0),
                prm.highlights,
                (0.0, 100.0, 1.0),
            ),
            (
                "shadows",
                "Shadow lift",
                format!("{:+.0}", prm.shadows * 100.0),
                (prm.shadows + 1.0) / 2.0,
                (-100.0, 100.0, 1.0),
            ),
            (
                "black_point",
                "Black clipping",
                format!("{:.1}%", prm.black_point * 100.0),
                prm.black_point / 0.25,
                (0.0, 25.0, 0.1),
            ),
            (
                "brightness",
                "Brightness",
                format!("{:+.0}", prm.brightness * 100.0),
                (prm.brightness + 1.0) / 2.0,
                (-100.0, 100.0, 1.0),
            ),
            (
                "contrast",
                "Contrast",
                format!("{:+.0}", prm.contrast * 100.0),
                (prm.contrast + 1.0) / 2.0,
                (-100.0, 100.0, 1.0),
            ),
            (
                "saturation",
                "Saturation",
                format!("{:+.0}", prm.saturation * 100.0),
                (prm.saturation + 1.0) / 2.0,
                (-100.0, 100.0, 1.0),
            ),
        ];
        for (key, name, display, norm, spec) in rows {
            body = body.child(self.param_slider(
                SliderKey::Raw(key),
                name,
                display,
                norm,
                spec,
                p,
                cx,
            ));
        }
        body = body.child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    Button::new("raw-tone-preview")
                        .label("Compare without tone")
                        .xsmall()
                        .ghost()
                        .disabled(self.raw.is_pending())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.raw_preview(RawSection::Adjust, false, cx)
                        })),
                )
                .child(
                    Button::new("raw-clipping")
                        .label("Show clipping")
                        .xsmall()
                        .ghost()
                        .disabled(self.raw.is_pending())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.raw_preview(RawSection::Adjust, true, cx)
                        })),
                ),
        );
        body = body.child(mono(
            "re-develops the camera file; adjustments above it stay as they are",
            9.5,
            p.muted,
        ));
        Some(body.into_any_element())
    }
}
