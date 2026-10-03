//! Drawing tools for storyboard panels: the bucket's gap closing, fill
//! modes and sampling; the cutter (the selection to a new layer);
//! perspective and envelope distortion of the selection with a live
//! preview; and guide sets and the ruler on the Drawing row. The work is
//! done by `emulsion_core::{bucket, cutter, distort, drawing_guides}`.

use super::*;
use crate::widgets::tip;
use emulsion_core::bucket::{BucketOptions, bucket_fill};
use emulsion_core::distort::{DistortKind, Distortion, distort_area, distort_command};
use emulsion_core::drawing_guides::{GuideKind, Ruler};
use emulsion_raster::Mask;
use emulsion_raster::gap_fill::FillMode;

/// Gap sizes the bucket's gap chip cycles through, in pixels.
const GAPS: [u32; 6] = [0, 2, 4, 8, 16, 32];
/// Unpainted thresholds the chip cycles through (alpha, 0–255).
const THRESHOLDS: [u8; 4] = [64, 128, 192, 255];

#[derive(Default)]
pub struct DrawingTools {
    /// Bucket options besides tolerance and contiguity (the shared ones).
    pub bucket: BucketOptions,
    pub distort: Option<SelectionDistort>,
}

/// A perspective or envelope distortion in progress. The document is in a
/// transaction, so the preview and the result are one Undo step.
pub struct SelectionDistort {
    pub id: NodeId,
    /// The document before distorting, which every preview starts from.
    base: Document,
    selection: Option<Arc<Mask>>,
    pub d: Distortion,
    /// A preview is being computed; `stale` asks for another after it.
    busy: bool,
    stale: bool,
}

fn next<T: PartialEq + Copy>(list: &[T], current: T) -> T {
    let i = list.iter().position(|v| *v == current).map_or(0, |i| i + 1);
    list[i % list.len()]
}

impl EditorView {
    // ── Bucket ─────────────────────────────────────────────────────────

    /// The bucket's options with the shared tolerance and contiguity.
    pub(crate) fn bucket_options(&self) -> BucketOptions {
        BucketOptions {
            tolerance: self.tools.tolerance,
            contiguous: self.tools.contiguous,
            ..self.tools.drawing.bucket
        }
    }

    /// The layer the bucket fills: the selected vector stroke layer, or the
    /// paint target (a new pixel layer when nothing paintable is selected).
    pub(crate) fn bucket_target(&mut self, cx: &mut Context<Self>) -> Option<NodeId> {
        if let Some(id) = self.selected
            && matches!(
                self.editor.doc.node(id).map(|n| &n.kind),
                Some(NodeKind::Strokes { .. })
            )
        {
            if self.editor.doc.locked_ancestor(id).is_some() {
                self.set_status("That layer is locked.", true, cx);
                return None;
            }
            return Some(id);
        }
        self.paint_target(cx)
    }

    /// Fill at document point `d` with `color` on the bucket's target.
    pub(crate) fn bucket_fill_at(
        &mut self,
        d: (f64, f64),
        color: [u8; 4],
        label: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.bucket_target(cx) else {
            return;
        };
        let doc = self.editor.doc.clone();
        let options = self.bucket_options();
        self.set_status(t!("editor.tools.filling"), false, cx);
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { bucket_fill(&doc, id, d, color, &options) })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, "Fill", cx) {
                    return;
                }
                this.status = None;
                match result {
                    Ok(Some(mut command)) => {
                        if let Command::ReplacePixels { label: l, .. } = &mut command {
                            *l = label.into();
                        }
                        this.execute(command, cx);
                    }
                    Ok(None) => {}
                    Err(error) => this.set_status(error, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Gap closing, fill mode and sampling chips for the bucket.
    pub(crate) fn bucket_option_chips(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let o = self.tools.drawing.bucket;
        let mut v = Vec::new();
        let gap = if o.gap == 0 {
            "gaps: open".to_string()
        } else {
            format!("close gaps {} px", o.gap)
        };
        v.push(
            tip(
                chip("bucket-gap", gap, o.gap > 0, p).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.tools.drawing.bucket.gap = next(&GAPS, o.gap);
                        cx.notify();
                    },
                )),
                "Treat gaps in line art up to this size as closed; click to cycle",
            )
            .into_any_element(),
        );
        for (id, mode, help) in [
            ("bucket-normal", FillMode::Normal, "Fill over the area"),
            (
                "bucket-behind",
                FillMode::Behind,
                "Fill behind the layer's pixels, only showing where it is transparent",
            ),
            (
                "bucket-unpainted",
                FillMode::Unpainted,
                "Fill only pixels that are not painted yet",
            ),
        ] {
            v.push(
                tip(
                    chip(id, mode.label(), mode == o.mode, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.tools.drawing.bucket.mode = mode;
                            cx.notify();
                        },
                    )),
                    help,
                )
                .into_any_element(),
            );
        }
        if o.mode == FillMode::Unpainted {
            let pct = (o.threshold as f32 / 255.0 * 100.0).round();
            v.push(
                tip(
                    chip("bucket-threshold", format!("alpha < {pct:.0}%"), true, p).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.tools.drawing.bucket.threshold = next(&THRESHOLDS, o.threshold);
                            cx.notify();
                        }),
                    ),
                    "Pixels less opaque than this count as unpainted; click to cycle",
                )
                .into_any_element(),
            );
        }
        v.push(
            tip(
                chip(
                    "bucket-sample",
                    if o.sample_all {
                        "sample: all layers"
                    } else {
                        "sample: layer"
                    },
                    !o.sample_all,
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tools.drawing.bucket.sample_all = !o.sample_all;
                    cx.notify();
                })),
                "Find the area on every visible layer, or on the current layer only",
            )
            .into_any_element(),
        );
        v
    }

    // ── Cutter ─────────────────────────────────────────────────────────

    /// Lift (or copy) the selected part of the current layer into a new
    /// layer above it, as one Undo step.
    pub fn cut_to_new_layer(&mut self, copy: bool, cx: &mut Context<Self>) {
        let Some(selection) = self.editor.doc.selection.clone() else {
            self.set_status("Select an area to cut first.", true, cx);
            return;
        };
        let Some(id) = self.selected else {
            self.set_status("Select a layer to cut from.", true, cx);
            return;
        };
        if self.editor.in_transaction() {
            self.set_status("Finish the current edit first.", true, cx);
            return;
        }
        let label = if copy {
            "Copy to new layer"
        } else {
            "Cut to new layer"
        };
        match emulsion_core::cutter::cut_to_new_layer(&self.editor.doc, id, &selection, copy) {
            Ok(commands) => {
                if let Some(created) = self.execute_layer_commands(label, commands, cx)
                    && let Some(new) = created.first().copied()
                {
                    self.set_layer_selection(vec![new], Some(new));
                    self.set_status(label, false, cx);
                }
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }

    // ── Distortion ─────────────────────────────────────────────────────

    /// Start distorting the selected pixels (or the whole layer without a
    /// selection) by dragging corners or an envelope lattice.
    pub fn start_distort(&mut self, kind: DistortKind, cx: &mut Context<Self>) {
        if self.tools.drawing.distort.is_some() {
            self.cancel_distort(cx);
        }
        if self.drag.is_some() || self.warp.is_some() || self.editor.in_transaction() {
            self.set_status("Finish the current edit before distorting.", true, cx);
            return;
        }
        let Some(id) = self.selected else {
            self.set_status("Select a layer to distort.", true, cx);
            return;
        };
        let selection = self.editor.doc.selection.clone();
        let Some(area) = distort_area(&self.editor.doc, id, selection.as_deref()) else {
            self.set_status("Nothing to distort there.", true, cx);
            return;
        };
        let d = Distortion::new(area, kind);
        // Check the layer can be distorted before opening the transaction.
        if let Err(error) = distort_command(&self.editor.doc, id, selection.as_deref(), &d) {
            self.set_status(error, true, cx);
            return;
        }
        self.editor.begin("Distort");
        self.tools.drawing.distort = Some(SelectionDistort {
            id,
            base: self.editor.doc.clone(),
            selection,
            d,
            busy: false,
            stale: false,
        });
        self.set_status("Drag the handles to distort, then apply.", false, cx);
        cx.notify();
    }

    pub(crate) fn distort_handles(&self) -> Vec<(f64, f64)> {
        self.tools
            .drawing
            .distort
            .as_ref()
            .map(|s| s.d.grid.clone())
            .unwrap_or_default()
    }

    pub(crate) fn distort_lines(&self) -> Vec<super::guides::Polyline> {
        self.tools
            .drawing
            .distort
            .as_ref()
            .map(|s| s.d.lines())
            .unwrap_or_default()
    }

    pub(crate) fn move_distort_handle(&mut self, i: usize, d: (f64, f64), cx: &mut Context<Self>) {
        let Some(s) = &mut self.tools.drawing.distort else {
            return;
        };
        s.d.move_handle(i, d);
        self.preview_distort(cx);
    }

    /// Show the current lattice's result, computed off the UI thread. While
    /// one preview runs, later moves wait and the newest shape follows.
    fn preview_distort(&mut self, cx: &mut Context<Self>) {
        let Some(s) = &mut self.tools.drawing.distort else {
            return;
        };
        if s.busy {
            s.stale = true;
            return;
        }
        s.busy = true;
        s.stale = false;
        let (base, id, selection, d) = (s.base.clone(), s.id, s.selection.clone(), s.d.clone());
        cx.spawn(async move |this, cx| {
            let command = cx
                .background_spawn(
                    async move { distort_command(&base, id, selection.as_deref(), &d) },
                )
                .await;
            this.update(cx, |this, cx| {
                let Some(s) = &mut this.tools.drawing.distort else {
                    return;
                };
                s.busy = false;
                let again = s.stale;
                match command.map(|c| this.editor.preview(c).map_err(|e| e.to_string())) {
                    Ok(Ok(_)) => {
                        this.status = None;
                        this.after_change(cx);
                    }
                    Ok(Err(error)) => {
                        // The transaction is gone (Undo, another edit).
                        this.tools.drawing.distort = None;
                        this.set_status(error, true, cx);
                        return;
                    }
                    Err(error) => this.set_status(error, true, cx),
                }
                if again {
                    this.preview_distort(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Commit the distortion as one Undo step.
    pub fn apply_distort(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.tools.drawing.distort.take() else {
            return;
        };
        if !self.editor.in_transaction() {
            return;
        }
        match distort_command(&s.base, s.id, s.selection.as_deref(), &s.d) {
            Ok(command) => {
                let result = self.editor.execute(command).and_then(|_| {
                    if s.selection.is_some() {
                        self.editor
                            .execute(Command::SetSelection { selection: None })?;
                    }
                    Ok(())
                });
                match result {
                    Ok(()) => {
                        self.editor.end();
                        self.set_status("Distorted.", false, cx);
                    }
                    Err(error) => {
                        self.editor.cancel();
                        self.set_status(error.to_string(), true, cx);
                    }
                }
            }
            Err(error) => {
                self.editor.cancel();
                self.set_status(error, true, cx);
            }
        }
        self.after_change(cx);
    }

    pub fn cancel_distort(&mut self, cx: &mut Context<Self>) {
        if self.tools.drawing.distort.take().is_some() && self.editor.in_transaction() {
            self.editor.cancel();
            self.after_change(cx);
        }
    }

    /// Cutter and distortion chips for the selection tools.
    pub(crate) fn selection_layer_chips(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut v = vec![self.group("layer", p)];
        if self.tools.drawing.distort.is_some() {
            v.push(
                chip("distort-apply", "apply distort", true, p)
                    .on_click(cx.listener(|this, _, _, cx| this.apply_distort(cx)))
                    .into_any_element(),
            );
            v.push(
                chip("distort-cancel", "cancel", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_distort(cx)))
                    .into_any_element(),
            );
            return v;
        }
        if self.editor.doc.selection.is_some() {
            for (id, text, copy, help) in [
                (
                    "cut-to-layer",
                    "cut to layer",
                    false,
                    "Move the selected part of the layer into a new layer at the same place",
                ),
                (
                    "copy-to-layer",
                    "copy to layer",
                    true,
                    "Copy the selected part of the layer into a new layer at the same place",
                ),
            ] {
                v.push(
                    tip(
                        chip(id, text, false, p).on_click(
                            cx.listener(move |this, _, _, cx| this.cut_to_new_layer(copy, cx)),
                        ),
                        help,
                    )
                    .into_any_element(),
                );
            }
        }
        for (id, text, kind, help) in [
            (
                "distort-perspective",
                "perspective",
                DistortKind::Perspective,
                "Distort the selection (or the layer) by dragging its four corners",
            ),
            (
                "distort-envelope",
                "envelope 3×3",
                DistortKind::Envelope(3),
                "Bend the selection (or the layer) with a 3×3 envelope",
            ),
            (
                "distort-envelope-4",
                "4×4",
                DistortKind::Envelope(4),
                "Bend the selection (or the layer) with a finer 4×4 envelope",
            ),
        ] {
            v.push(
                tip(
                    chip(id, text, false, p)
                        .on_click(cx.listener(move |this, _, _, cx| this.start_distort(kind, cx))),
                    help,
                )
                .into_any_element(),
            );
        }
        v
    }

    // ── Guides ─────────────────────────────────────────────────────────

    /// Show or hide the ruler, placing it across the canvas the first time.
    pub fn toggle_ruler(&mut self, cx: &mut Context<Self>) {
        let (w, h) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
        let guides = &mut self.editor.doc.drawing_guides;
        let on = match &mut guides.ruler {
            Some(r) => {
                r.enabled = !r.enabled;
                r.enabled
            }
            None => {
                guides.ruler = Some(Ruler::centered(w, h));
                true
            }
        };
        self.set_status(
            if on {
                "Ruler: strokes started near its edge follow it"
            } else {
                "Ruler hidden"
            },
            false,
            cx,
        );
    }

    /// The guide, assist, ruler and guide-set chips of the Drawing row.
    pub(crate) fn guide_chips(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (w, h) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
        let guides = &self.editor.doc.drawing_guides;
        let primary = guides.primary();
        let any = guides.active().next().is_some();
        let mut v = Vec::new();
        let gk = primary.clone();
        v.push(
            tip(
                chip("draw-guide", primary.label(), primary != GuideKind::Off, p).on_click(
                    cx.listener(move |this, _, _, cx| {
                        // Off → grid → isometric → 1/2/3-point → 4/5-point → off.
                        this.editor.doc.drawing_guides.set_primary(gk.cycle(w, h));
                        cx.notify();
                    }),
                ),
                "Drawing guide over the canvas; click to cycle grid, isometric, 1-, 2-, 3-point, 4- and 5-point curvilinear perspective, off",
            )
            .into_any_element(),
        );
        if primary != GuideKind::Off
            && guides.guides.len() < emulsion_core::drawing_guides::MAX_GUIDES
        {
            v.push(
                tip(
                    chip("draw-guide-keep", "+ keep", false, p).on_click(cx.listener(
                        |this, _, _, cx| {
                            // Keep this guide and start choosing another.
                            this.editor
                                .doc
                                .drawing_guides
                                .guides
                                .insert(0, GuideKind::Off);
                            cx.notify();
                        },
                    )),
                    "Keep this guide on the canvas and cycle another one alongside it",
                )
                .into_any_element(),
            );
        }
        if guides.guides.len() > 1 {
            v.push(
                chip("draw-guide-clear", "clear guides", false, p)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let g = &mut this.editor.doc.drawing_guides;
                        g.guides.clear();
                        g.active_set = None;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        if any {
            let assist = self.tools.guide.assist;
            v.push(
                tip(
                    chip("draw-assist", "assist", assist, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.tools.guide.assist = !assist;
                            this.set_status(
                                if assist {
                                    "Drawing assist off"
                                } else {
                                    "Drawing assist: strokes follow the guide"
                                },
                                false,
                                cx,
                            );
                            cx.notify();
                        },
                    )),
                    "Strokes snap to the guide's lines and arcs",
                )
                .into_any_element(),
            );
        }
        let ruler = guides.ruler.is_some_and(|r| r.enabled);
        v.push(
            tip(
                chip("draw-ruler", "ruler", ruler, p)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_ruler(cx))),
                "A straight edge: brush, pencil and eraser strokes started near it follow it. Drag its ends to move it",
            )
            .into_any_element(),
        );
        if any {
            v.push(
                tip(
                    chip("guide-set-save", "save set", false, p).on_click(cx.listener(
                        |this, _, _, cx| {
                            let g = &mut this.editor.doc.drawing_guides;
                            let name = match g.active_set {
                                Some(i) => g.sets[i].name.clone(),
                                None => g.next_set_name(),
                            };
                            match g.save_set(&name) {
                                Ok(_) => this.set_status(format!("Saved {name}"), false, cx),
                                Err(e) => this.set_status(e, true, cx),
                            }
                        },
                    )),
                    "Save the guides shown as a set for this panel",
                )
                .into_any_element(),
            );
        }
        for (i, set) in guides.sets.iter().enumerate() {
            let on = guides.active_set == Some(i);
            v.push(
                tip(
                    chip(
                        SharedString::from(format!("guide-set-{i}")),
                        set.name.clone(),
                        on,
                        p,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.editor.doc.drawing_guides.switch_to(i);
                        cx.notify();
                    })),
                    "Switch to this guide set",
                )
                .into_any_element(),
            );
        }
        if let Some(i) = guides.active_set {
            v.push(
                chip("guide-set-delete", "delete set", false, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.editor.doc.drawing_guides.delete_set(i);
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        v
    }
}
