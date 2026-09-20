//! Canvas size and image size, typed in; and filling canvas that a crop
//! or size change adds.
//!
//! Canvas size changes the frame around the image: it grows or shrinks
//! from the chosen anchor, and nothing is resampled. Image size scales
//! everything (losslessly, since pixel nodes keep their sources). With
//! "fill new edges", canvas that the change adds is filled from the image
//! by content-aware fill, into its own node so it can be hidden or masked.

use super::*;
use emulsion_raster::{IRect, fill, select};
use glam::{DAffine2, dvec2};

pub(crate) struct SizePanel {
    width: Entity<InputState>,
    height: Entity<InputState>,
    /// Column and row of the anchor, 0–2 each.
    anchor: (u8, u8),
    /// Scale the image instead of changing the canvas.
    resample: bool,
}

impl EditorView {
    pub fn toggle_size_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.size_panel.take().is_some() {
            cx.notify();
            return;
        }
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let width = cx.new(|cx| InputState::new(window, cx).default_value(w.to_string()));
        let height = cx.new(|cx| InputState::new(window, cx).default_value(h.to_string()));
        self.size_panel = Some(SizePanel {
            width,
            height,
            anchor: (1, 1),
            resample: false,
        });
        cx.notify();
    }

    fn apply_size(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = &self.size_panel else {
            return;
        };
        let parse = |e: &Entity<InputState>| {
            e.read(cx)
                .value()
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|v| (1..=30_000).contains(v))
        };
        let (Some(nw), nh) = (parse(&panel.width), parse(&panel.height)) else {
            self.set_status("Width must be a whole number from 1 to 30000.", true, cx);
            return;
        };
        let (ow, oh) = (self.editor.doc.width, self.editor.doc.height);
        if panel.resample {
            let nh = ((nw as f64 * oh as f64 / ow as f64).round() as u32).max(1);
            self.execute(
                Command::ImageSize {
                    width: nw,
                    height: nh,
                },
                cx,
            );
            self.fit_pending = true;
        } else {
            let Some(nh) = nh else {
                self.set_status("Height must be a whole number from 1 to 30000.", true, cx);
                return;
            };
            let (ax, ay) = (panel.anchor.0 as i64, panel.anchor.1 as i64);
            let x = -((nw as i64 - ow as i64) * ax / 2);
            let y = -((nh as i64 - oh as i64) * ay / 2);
            let rect = IRect::new(x as i32, y as i32, nw as i32, nh as i32);
            let fill = self.tools.fill_edges;
            self.crop_canvas(rect, 0.0, fill, cx);
        }
        self.size_panel = None;
        cx.notify();
    }

    /// Crop (or extend) the canvas, then optionally fill what was added.
    pub fn crop_canvas(
        &mut self,
        rect: IRect,
        rotation: f64,
        fill_edges: bool,
        cx: &mut Context<Self>,
    ) {
        let (ow, oh) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
        let before = self.editor.revision;
        self.execute(Command::Crop { rect, rotation }, cx);
        self.fit_pending = true;
        cx.notify();
        if !fill_edges || self.editor.revision == before {
            return;
        }
        // Where the old canvas landed; everything else is new.
        let c = dvec2(ow / 2.0, oh / 2.0);
        let to_new = DAffine2::from_translation(dvec2(-rect.x as f64, -rect.y as f64))
            * DAffine2::from_translation(c)
            * DAffine2::from_angle(rotation.to_radians())
            * DAffine2::from_translation(-c);
        let corners: Vec<(f32, f32)> = [(0.0, 0.0), (ow, 0.0), (ow, oh), (0.0, oh)]
            .into_iter()
            .map(|(x, y)| {
                let p = to_new.transform_point2(dvec2(x, y));
                (p.x as f32, p.y as f32)
            })
            .collect();
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let footprint = select::polygon(w, h, &corners);
        // One pixel into the old image too, so antialiased edges are redone.
        let hole = select::grow(&select::invert(&footprint), 1);
        if select::bounds(&hole).is_empty() {
            return;
        }
        let doc = self.editor.doc.clone();
        self.set_status("Filling the new edges from the image…", false, cx);
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let layer = cx
                .background_spawn(
                    async move { fill::content_aware_layer(&doc.composite_tree(), &hole) },
                )
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, "Edge fill", cx) {
                    return;
                }
                this.status = None;
                let Some((raster, reg)) = layer else { return };
                let node = Node::raster(
                    0,
                    "Extended edges",
                    Arc::new(raster),
                    Placement::at(reg.x as f64, reg.y as f64),
                );
                if let Some(id) = this.execute(
                    Command::AddNode {
                        node: Box::new(node),
                        slot: Slot::TOP,
                    },
                    cx,
                ) {
                    this.selected = Some(id);
                    this.set_status("New edges filled into their own node.", false, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn size_panel_view(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let panel = self.size_panel.as_ref()?;
        let resample = panel.resample;
        let (ow, oh) = (self.editor.doc.width, self.editor.doc.height);
        let mode = |id: &'static str, text: &'static str, on: bool| {
            chip(id, text, on, p).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(s) = &mut this.size_panel {
                    s.resample = id == "size-image";
                    cx.notify();
                }
            }))
        };
        let mut anchors = div().flex().flex_col().gap(px(2.));
        for row in 0..3u8 {
            let mut r = div().flex().gap(px(2.));
            for col in 0..3u8 {
                let on = panel.anchor == (col, row);
                let accent = p.accent;
                r = r.child(
                    div()
                        .id(("anchor", (row * 3 + col) as usize))
                        .size(px(14.))
                        .border_1()
                        .border_color(p.ink)
                        .when(on, |d| d.bg(p.ink))
                        .cursor_pointer()
                        .hover(move |s| s.border_color(accent))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(s) = &mut this.size_panel {
                                s.anchor = (col, row);
                                cx.notify();
                            }
                        })),
                );
            }
            anchors = anchors.child(r);
        }
        let fill = self.tools.fill_edges;
        Some(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(12.))
                .px(px(16.))
                .py(px(10.))
                .border_b_1()
                .border_color(p.line)
                .bg(p.panel)
                .child(label(format!("Size · now {ow}×{oh}"), p))
                .child(mode("size-canvas", "canvas", !resample))
                .child(mode("size-image", "image", resample))
                .child(mono("W", 10., p.muted))
                .child(div().w(px(80.)).child(Input::new(&panel.width)))
                .when(!resample, |d| {
                    d.child(mono("H", 10., p.muted))
                        .child(div().w(px(80.)).child(Input::new(&panel.height)))
                        .child(mono("anchor", 10., p.muted))
                        .child(anchors)
                        .child(
                            chip("size-fill", "fill new edges", fill, p).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.tools.fill_edges = !fill;
                                    cx.notify();
                                },
                            )),
                        )
                })
                .when(resample, |d| {
                    d.child(mono("height follows the aspect ratio", 10., p.muted))
                })
                .child(div().flex_1())
                .child(
                    button("size-apply", "Apply", true, p)
                        .on_click(cx.listener(|this, _, _, cx| this.apply_size(cx))),
                )
                .child(
                    button("size-close", "Close", false, p).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.size_panel = None;
                            cx.notify();
                        },
                    )),
                ),
        )
    }
}
