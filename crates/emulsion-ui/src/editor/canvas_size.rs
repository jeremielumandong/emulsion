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
use std::cell::Cell;
use std::rc::Rc;

/// Which Photoshop dialog the panel stands in for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeMode {
    /// Image Size: resample everything to new pixel dimensions.
    Image,
    /// Canvas Size: change the frame around the image from an anchor.
    Canvas,
}

pub(crate) struct SizePanel {
    width: Entity<InputState>,
    height: Entity<InputState>,
    mode: SizeMode,
    /// Column and row of the anchor, 0–2 each (Canvas).
    anchor: (u8, u8),
    /// Keep width and height in proportion as either is typed.
    constrain: bool,
    /// Canvas: the fields are amounts to add (or remove), not totals.
    relative: bool,
    /// Fields are percentages of the current size instead of pixels.
    percent: bool,
    /// Set while one field updates the other, so they do not ping-pong.
    syncing: Rc<Cell<bool>>,
    _subs: Vec<Subscription>,
}

impl EditorView {
    /// Toggle the panel; opens as Image Size.
    pub fn toggle_size_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.size_panel.take().is_some() {
            cx.notify();
            return;
        }
        self.open_size_panel(SizeMode::Image, window, cx);
    }

    /// Open the panel in `mode` (Ctrl+Alt+I / Ctrl+Alt+C, as in Photoshop).
    pub fn open_size_panel(&mut self, mode: SizeMode, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.size_panel {
            p.mode = mode;
            cx.notify();
            return;
        }
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let width = cx.new(|cx| InputState::new(window, cx).default_value(w.to_string()));
        let height = cx.new(|cx| InputState::new(window, cx).default_value(h.to_string()));
        let syncing = Rc::new(Cell::new(false));
        let mut subs = Vec::new();
        for (this_side, other_side, from_width) in
            [(&width, &height, true), (&height, &width, false)]
        {
            let other = other_side.clone();
            let flag = syncing.clone();
            subs.push(cx.subscribe_in(
                this_side,
                window,
                move |this, st, ev: &InputEvent, window, cx| {
                    match ev {
                        InputEvent::PressEnter { .. } => {
                            this.apply_size(cx);
                            return;
                        }
                        InputEvent::Change => {}
                        _ => return,
                    }
                    let Some(panel) = &this.size_panel else {
                        return;
                    };
                    if !panel.constrain || flag.get() {
                        return;
                    }
                    let text = st.read(cx).value().trim().to_string();
                    let Ok(v) = text.parse::<f64>() else { return };
                    let (ow, oh) = (this.editor.doc.width as f64, this.editor.doc.height as f64);
                    let linked =
                        if panel.percent || panel.relative && panel.mode == SizeMode::Canvas {
                            // Percent and relative amounts scale both sides alike.
                            if panel.percent {
                                v
                            } else {
                                v * if from_width { oh / ow } else { ow / oh }
                            }
                        } else if from_width {
                            v * oh / ow
                        } else {
                            v * ow / oh
                        };
                    let shown = if panel.percent {
                        format!("{linked:.1}")
                    } else {
                        format!("{}", linked.round() as i64)
                    };
                    if other.read(cx).value().trim() != shown {
                        flag.set(true);
                        other.update(cx, |o, cx| o.set_value(shown, window, cx));
                        flag.set(false);
                    }
                    cx.notify();
                },
            ));
        }
        self.size_panel = Some(SizePanel {
            width,
            height,
            mode,
            anchor: (1, 1),
            constrain: true,
            relative: false,
            percent: false,
            syncing,
            _subs: subs,
        });
        cx.notify();
    }

    /// Put fresh values in both fields (after a unit or relative toggle).
    fn reset_size_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = &self.size_panel else {
            return;
        };
        let (ow, oh) = (self.editor.doc.width, self.editor.doc.height);
        let (wv, hv) = if panel.percent {
            ("100".to_string(), "100".to_string())
        } else if panel.relative && panel.mode == SizeMode::Canvas {
            ("0".to_string(), "0".to_string())
        } else {
            (ow.to_string(), oh.to_string())
        };
        let (w, h, flag) = (
            panel.width.clone(),
            panel.height.clone(),
            panel.syncing.clone(),
        );
        flag.set(true);
        w.update(cx, |s, cx| s.set_value(wv, window, cx));
        h.update(cx, |s, cx| s.set_value(hv, window, cx));
        flag.set(false);
    }

    /// The new size the fields describe, in pixels.
    fn size_target(&self, cx: &App) -> Option<(u32, u32)> {
        let panel = self.size_panel.as_ref()?;
        let (ow, oh) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
        let read = |e: &Entity<InputState>| e.read(cx).value().trim().parse::<f64>().ok();
        let (w, h) = (read(&panel.width)?, read(&panel.height)?);
        let (nw, nh) = if panel.percent {
            (ow * w / 100.0, oh * h / 100.0)
        } else if panel.relative && panel.mode == SizeMode::Canvas {
            (ow + w, oh + h)
        } else {
            (w, h)
        };
        let (nw, nh) = (nw.round(), nh.round());
        if !(1.0..=30_000.0).contains(&nw) || !(1.0..=30_000.0).contains(&nh) {
            return None;
        }
        Some((nw as u32, nh as u32))
    }

    fn apply_size(&mut self, cx: &mut Context<Self>) {
        let Some((nw, nh)) = self.size_target(cx) else {
            self.set_status("Sizes must come out between 1 and 30000 pixels.", true, cx);
            return;
        };
        let Some(panel) = &self.size_panel else {
            return;
        };
        let (ow, oh) = (self.editor.doc.width, self.editor.doc.height);
        match panel.mode {
            SizeMode::Image => {
                if (nw, nh) == (ow, oh) {
                    self.size_panel = None;
                    cx.notify();
                    return;
                }
                // Pixel nodes scale uniformly by width, so the height follows
                // the aspect; a typed height only steers when unconstrained.
                let nh = if panel.constrain {
                    ((nw as f64 * oh as f64 / ow as f64).round() as u32).max(1)
                } else {
                    nh
                };
                self.execute(
                    Command::ImageSize {
                        width: nw,
                        height: nh,
                    },
                    cx,
                );
                self.fit_pending = true;
                self.set_status(format!("Image resized to {nw}×{nh}."), false, cx);
            }
            SizeMode::Canvas => {
                let (ax, ay) = (panel.anchor.0 as i64, panel.anchor.1 as i64);
                let x = -((nw as i64 - ow as i64) * ax / 2);
                let y = -((nh as i64 - oh as i64) * ay / 2);
                let rect = IRect::new(x as i32, y as i32, nw as i32, nh as i32);
                let fill = self.tools.fill_edges;
                self.crop_canvas(rect, 0.0, fill, cx);
                self.set_status(format!("Canvas is now {nw}×{nh}."), false, cx);
            }
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
        let mode = panel.mode;
        let (ow, oh) = (self.editor.doc.width, self.editor.doc.height);
        let target = self.size_target(cx);
        let summary = match target {
            Some((w, h)) => {
                let mp = w as f64 * h as f64 / 1e6;
                let bytes = w as f64 * h as f64 * 8.0 / 1e6;
                format!("→ {w}×{h} · {mp:.1} MP · ~{bytes:.0} MB per layer")
            }
            None => "→ enter whole numbers".to_string(),
        };
        let mode_chip = |id: &'static str, text: &'static str, m: SizeMode| {
            chip(id, text, mode == m, p).on_click(cx.listener(move |this, _, window, cx| {
                if let Some(s) = &mut this.size_panel {
                    s.mode = m;
                    s.relative = false;
                }
                this.reset_size_fields(window, cx);
                cx.notify();
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
        let (constrain, relative, percent, fill) = (
            panel.constrain,
            panel.relative,
            panel.percent,
            self.tools.fill_edges,
        );
        let unit = if percent { "%" } else { "px" };
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
                .child(label(
                    match mode {
                        SizeMode::Image => format!("Image size · now {ow}×{oh}"),
                        SizeMode::Canvas => format!("Canvas size · now {ow}×{oh}"),
                    },
                    p,
                ))
                .child(mode_chip("size-image", "image size", SizeMode::Image))
                .child(mode_chip("size-canvas", "canvas size", SizeMode::Canvas))
                .child(mono(if relative { "add W" } else { "W" }, 10., p.muted))
                .child(div().w(px(80.)).child(Input::new(&panel.width)))
                .child(mono(if relative { "add H" } else { "H" }, 10., p.muted))
                .child(div().w(px(80.)).child(Input::new(&panel.height)))
                .child(chip("size-unit", unit, percent, p).on_click(cx.listener(
                    move |this, _, window, cx| {
                        if let Some(s) = &mut this.size_panel {
                            s.percent = !percent;
                        }
                        this.reset_size_fields(window, cx);
                        cx.notify();
                    },
                )))
                .child(
                    chip("size-lock", "🔗 constrain", constrain, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            if let Some(s) = &mut this.size_panel {
                                s.constrain = !constrain;
                                cx.notify();
                            }
                        },
                    )),
                )
                .when(mode == SizeMode::Canvas, |d| {
                    d.child(
                        chip("size-relative", "relative", relative, p).on_click(cx.listener(
                            move |this, _, window, cx| {
                                if let Some(s) = &mut this.size_panel {
                                    s.relative = !relative;
                                }
                                this.reset_size_fields(window, cx);
                                cx.notify();
                            },
                        )),
                    )
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
                .child(mono(summary, 10., p.muted))
                .child(div().flex_1())
                .child(
                    button("size-apply", "OK", true, p)
                        .on_click(cx.listener(|this, _, _, cx| this.apply_size(cx))),
                )
                .child(
                    button("size-close", "Cancel", false, p).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.size_panel = None;
                            cx.notify();
                        },
                    )),
                ),
        )
    }
}
