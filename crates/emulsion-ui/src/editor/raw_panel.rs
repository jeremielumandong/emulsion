//! The RAW develop panel: exposure, white balance, highlights and
//! shadows for a document opened from a camera RAW. The decoded RAW is
//! kept in memory; every change re-develops it in the background and
//! replaces the base layer's pixels (one undo step per develop).

use super::*;
use emulsion_io::raw::{DevelopParams, RawSource};
use std::path::PathBuf;

#[derive(Default)]
pub struct RawState {
    source: Option<Arc<RawSource>>,
    loading: bool,
    pub params: DevelopParams,
    /// Bumped per change; a develop only lands if it is still the newest.
    generation: u64,
    busy: bool,
}

/// One panel slider: key, label, value text, normalised position, spec.
type RawRow = (&'static str, &'static str, String, f32, (f32, f32, f32));

/// Debounce between the last slider move and the develop.
const SETTLE_MS: u64 = 220;

impl EditorView {
    /// The file this document was developed from, when it is a RAW.
    fn raw_path(&self) -> Option<PathBuf> {
        self.source.clone().filter(|p| emulsion_io::raw::is_raw(p))
    }

    /// The node the RAW was developed into: the bottom-most pixel node.
    fn raw_node(&self) -> Option<NodeId> {
        self.editor
            .doc
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Raster { .. }))
            .map(|n| n.id)
    }

    fn ensure_raw_loaded(&mut self, cx: &mut Context<Self>) {
        if self.raw.source.is_some() || self.raw.loading {
            return;
        }
        let Some(path) = self.raw_path() else {
            return;
        };
        self.raw.loading = true;
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_spawn(async move { RawSource::load(&path) })
                .await;
            this.update(cx, |this, cx| {
                this.raw.loading = false;
                match loaded {
                    Ok(src) => this.raw.source = Some(Arc::new(src)),
                    Err(e) => this.set_status(format!("RAW: {e}"), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A RAW slider moved: remember the value and develop once it settles.
    pub(crate) fn raw_slider(&mut self, name: &'static str, v: f32, cx: &mut Context<Self>) {
        let p = &mut self.raw.params;
        match name {
            "exposure" => p.exposure = v / 100.0,
            "temperature" => p.temperature = v / 100.0,
            "tint" => p.tint = v / 100.0,
            "highlights" => p.highlights = v / 100.0,
            "shadows" => p.shadows = v / 100.0,
            _ => return,
        }
        self.schedule_develop(cx);
    }

    pub(crate) fn raw_reset(&mut self, cx: &mut Context<Self>) {
        self.raw.params = DevelopParams::default();
        self.schedule_develop(cx);
    }

    fn schedule_develop(&mut self, cx: &mut Context<Self>) {
        self.raw.generation += 1;
        let generation = self.raw.generation;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(SETTLE_MS))
                .await;
            this.update(cx, |this, cx| {
                if this.raw.generation == generation {
                    this.develop_now(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn develop_now(&mut self, cx: &mut Context<Self>) {
        let (Some(src), Some(id)) = (self.raw.source.clone(), self.raw_node()) else {
            return;
        };
        let params = self.raw.params;
        let generation = self.raw.generation;
        self.raw.busy = true;
        self.set_status("Developing…", false, cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { src.develop_with(&params) })
                .await;
            this.update(cx, |this, cx| {
                this.raw.busy = false;
                this.status = None;
                match result {
                    Ok(r) => {
                        let dirty = r.bounds();
                        this.execute(
                            Command::ReplacePixels {
                                id,
                                raster: Arc::new(r),
                                dirty,
                                label: "Develop RAW".into(),
                            },
                            cx,
                        );
                        // A newer change arrived meanwhile: develop again.
                        if this.raw.generation != generation {
                            this.develop_now(cx);
                        }
                    }
                    Err(e) => this.set_status(format!("Develop failed: {e}"), true, cx),
                }
                cx.notify();
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
        if self.raw_path().is_none() || self.raw_node() != Some(id) {
            return None;
        }
        self.ensure_raw_loaded(cx);
        let prm = self.raw.params;
        let mut body = div().flex().flex_col().gap(px(8.)).pt(px(4.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(label("RAW develop", p))
                .child(div().flex_1())
                .child(mono(
                    if self.raw.loading {
                        "decoding…".to_string()
                    } else if self.raw.busy {
                        "developing…".to_string()
                    } else if let Some(s) = &self.raw.source {
                        format!("{} {}", s.info.make, s.info.model)
                            .trim()
                            .to_string()
                    } else {
                        String::new()
                    },
                    9.5,
                    p.muted,
                ))
                .child(
                    chip("raw-reset", "as shot", prm == DevelopParams::default(), p)
                        .on_click(cx.listener(|this, _, _, cx| this.raw_reset(cx))),
                ),
        );
        let rows: [RawRow; 5] = [
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
                    "as shot".into()
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
                    "as shot".into()
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
                "shadows",
                format!("{:+.0}", prm.shadows * 100.0),
                (prm.shadows + 1.0) / 2.0,
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
        body = body.child(mono(
            "re-develops the camera file; adjustments above it stay as they are",
            9.5,
            p.muted,
        ));
        Some(body.into_any_element())
    }
}
