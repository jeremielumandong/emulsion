//! Panel controls that sliders cannot express: the curves editor, gradient
//! map colours, Levels' Auto button, the histogram, and LUT import.

use super::*;
use emulsion_raster::adjust::{Cube, Histogram, Stop, curve_at, straight_curve};

/// Which curve of a Curves node is being edited.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Channel {
    #[default]
    Master,
    Red,
    Green,
    Blue,
}

impl Channel {
    fn points(self, a: &Adjustment) -> Option<&Vec<[f32; 2]>> {
        let Adjustment::Curves {
            master,
            red,
            green,
            blue,
        } = a
        else {
            return None;
        };
        Some(match self {
            Channel::Master => master,
            Channel::Red => red,
            Channel::Green => green,
            Channel::Blue => blue,
        })
    }

    fn points_mut(self, a: &mut Adjustment) -> Option<&mut Vec<[f32; 2]>> {
        let Adjustment::Curves {
            master,
            red,
            green,
            blue,
        } = a
        else {
            return None;
        };
        Some(match self {
            Channel::Master => master,
            Channel::Red => red,
            Channel::Green => green,
            Channel::Blue => blue,
        })
    }
}

/// A point of a curve being dragged.
#[derive(Clone, Debug)]
pub struct CurveDrag {
    pub id: NodeId,
    pub channel: Channel,
    pub idx: usize,
    pub bounds: TrackBounds,
}

#[derive(Default)]
pub(crate) struct AdjustUi {
    pub channel: Channel,
    /// Histogram of the composite, by revision.
    hist: Option<(u64, Arc<Histogram>)>,
    hist_loading: Option<u64>,
}

const CURVE_PX: f32 = 200.0;

impl EditorView {
    /// The composite histogram, refreshed in the background when the
    /// document changes.
    pub(crate) fn histogram(&mut self, cx: &mut Context<Self>) -> Option<Arc<Histogram>> {
        let rev = self.editor.revision;
        if let Some((r, h)) = &self.adjust_ui.hist
            && *r == rev
        {
            return Some(h.clone());
        }
        if self.adjust_ui.hist_loading != Some(rev) {
            self.adjust_ui.hist_loading = Some(rev);
            let tree = self.tree.clone();
            cx.spawn(async move |this, cx| {
                let h = cx
                    .background_spawn(async move {
                        // A reduced level is plenty for a histogram.
                        let mut level = 0;
                        while level_size(tree.width, tree.height, level)
                            .0
                            .max(level_size(tree.width, tree.height, level).1)
                            > 512
                        {
                            level += 1;
                        }
                        let small = emulsion_raster::composite::flatten(&tree, level);
                        let px: Vec<[f32; 4]> = small
                            .read_rect(small.bounds())
                            .into_iter()
                            .map(color::px_to_f)
                            .collect();
                        Arc::new(Histogram::of(&px))
                    })
                    .await;
                this.update(cx, |this, cx| {
                    this.adjust_ui.hist = Some((rev, h));
                    this.adjust_ui.hist_loading = None;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        self.adjust_ui.hist.as_ref().map(|(_, h)| h.clone())
    }

    /// The histogram strip at the top of the node panel.
    pub(crate) fn histogram_view(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(h) = self.histogram(cx) else {
            return div().h(px(48.)).into_any_element();
        };
        let (r, g, b) = (
            Histogram::normalized(&h.r),
            Histogram::normalized(&h.g),
            Histogram::normalized(&h.b),
        );
        let line = p.line;
        div()
            .h(px(48.))
            .w_full()
            .border_1()
            .border_color(line)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let (w, hgt) = (
                            f32::from(bounds.size.width),
                            f32::from(bounds.size.height) - 2.0,
                        );
                        let bw = w / 256.0;
                        for (bins, colour) in [
                            (&r, rgb(0xE0453A)),
                            (&g, rgb(0x5DBB63)),
                            (&b, rgb(0x4A90E2)),
                        ] {
                            let colour: Hsla = colour.into();
                            for (i, v) in bins.iter().enumerate() {
                                let hh = (v.sqrt() * hgt).max(if *v > 0.0 { 1.0 } else { 0.0 });
                                if hh <= 0.0 {
                                    continue;
                                }
                                let x = bounds.origin.x + px(i as f32 * bw);
                                let y = bounds.origin.y + px(hgt + 1.0 - hh);
                                window.paint_quad(fill(
                                    Bounds::new(point(x, y), size(px(bw.max(1.0)), px(hh))),
                                    colour.opacity(0.55),
                                ));
                            }
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }

    /// Extra controls for an adjustment node, above its sliders.
    pub(crate) fn adjust_extras(
        &mut self,
        id: NodeId,
        a: &Adjustment,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut v = Vec::new();
        match a {
            Adjustment::Curves { .. } => {
                let cur = self.adjust_ui.channel;
                let mut chips = div().flex().gap(px(6.));
                for (cid, t, ch) in [
                    ("cv-rgb", "RGB", Channel::Master),
                    ("cv-r", "R", Channel::Red),
                    ("cv-g", "G", Channel::Green),
                    ("cv-b", "B", Channel::Blue),
                ] {
                    chips = chips.child(chip(cid, t, cur == ch, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.adjust_ui.channel = ch;
                            cx.notify();
                        },
                    )));
                }
                chips = chips.child(div().flex_1()).child(
                    chip("cv-reset", "reset", false, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            if let Some(NodeKind::Adjust(a)) =
                                this.editor.doc.node(id).map(|n| &n.kind)
                            {
                                let mut a = a.clone();
                                if let Some(pts) = this.adjust_ui.channel.points_mut(&mut a) {
                                    *pts = straight_curve();
                                }
                                this.execute(Command::SetAdjustment { id, adjustment: a }, cx);
                            }
                        },
                    )),
                );
                v.push(chips.into_any_element());
                v.push(self.curves_editor(id, a, p, cx));
                v.push(
                    mono(
                        "drag points · click the line to add · alt-click removes",
                        9.5,
                        p.muted,
                    )
                    .into_any_element(),
                );
            }
            Adjustment::GradientMap { stops, .. } => {
                let mut row = div().flex().items_center().gap(px(6.));
                for (i, s) in stops.iter().enumerate() {
                    let c: Hsla = rgb(((s.color[0] as u32) << 16)
                        | ((s.color[1] as u32) << 8)
                        | s.color[2] as u32)
                    .into();
                    row = row.child(
                        div()
                            .id(("gm-stop", i))
                            .size(px(16.))
                            .border_1()
                            .border_color(p.ink)
                            .bg(c),
                    );
                }
                row = row.child(div().flex_1());
                let (fg, bg) = (self.tools.fg, self.tools.bg);
                row = row.child(chip("gm-fgbg", "fg → bg", false, p).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.set_gradient_stops(
                            id,
                            vec![
                                Stop {
                                    pos: 0.0,
                                    color: [fg[0], fg[1], fg[2]],
                                },
                                Stop {
                                    pos: 1.0,
                                    color: [bg[0], bg[1], bg[2]],
                                },
                            ],
                            cx,
                        )
                    },
                )));
                for (cid, name, stops) in [
                    (
                        "gm-sepia",
                        "sepia",
                        vec![
                            Stop {
                                pos: 0.0,
                                color: [20, 12, 6],
                            },
                            Stop {
                                pos: 0.5,
                                color: [150, 110, 70],
                            },
                            Stop {
                                pos: 1.0,
                                color: [250, 240, 220],
                            },
                        ],
                    ),
                    (
                        "gm-cool",
                        "cool",
                        vec![
                            Stop {
                                pos: 0.0,
                                color: [8, 14, 40],
                            },
                            Stop {
                                pos: 0.5,
                                color: [80, 120, 170],
                            },
                            Stop {
                                pos: 1.0,
                                color: [240, 245, 255],
                            },
                        ],
                    ),
                    (
                        "gm-bw",
                        "b&w",
                        vec![
                            Stop {
                                pos: 0.0,
                                color: [0, 0, 0],
                            },
                            Stop {
                                pos: 1.0,
                                color: [255, 255, 255],
                            },
                        ],
                    ),
                ] {
                    row = row.child(chip(cid, name, false, p).on_click(cx.listener(
                        move |this, _, _, cx| this.set_gradient_stops(id, stops.clone(), cx),
                    )));
                }
                v.push(row.into_any_element());
            }
            Adjustment::Levels { .. } => {
                v.push(
                    chip("levels-auto", "auto", false, p)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(h) = this.histogram(cx) {
                                let auto = Adjustment::auto_levels(&h, 0.1);
                                this.execute(
                                    Command::SetAdjustment {
                                        id,
                                        adjustment: auto,
                                    },
                                    cx,
                                );
                            } else {
                                this.set_status(
                                    "Still reading the histogram; try again in a moment.",
                                    false,
                                    cx,
                                );
                            }
                        }))
                        .into_any_element(),
                );
            }
            Adjustment::Lut3D { cube, .. } => {
                v.push(
                    mono(
                        format!(
                            "{} · {}³",
                            if cube.name.is_empty() {
                                "LUT"
                            } else {
                                cube.name.as_str()
                            },
                            cube.size
                        ),
                        10.5,
                        p.muted,
                    )
                    .into_any_element(),
                );
                v.push(
                    chip("lut-load", "load .cube…", false, p)
                        .on_click(cx.listener(move |this, _, _, cx| this.import_lut(Some(id), cx)))
                        .into_any_element(),
                );
            }
            _ => {}
        }
        v
    }

    fn set_gradient_stops(&mut self, id: NodeId, stops: Vec<Stop>, cx: &mut Context<Self>) {
        if let Some(NodeKind::Adjust(Adjustment::GradientMap { reverse, .. })) =
            self.editor.doc.node(id).map(|n| &n.kind)
        {
            let reverse = *reverse;
            self.execute(
                Command::SetAdjustment {
                    id,
                    adjustment: Adjustment::GradientMap { stops, reverse },
                },
                cx,
            );
        }
    }

    /// Pick a .cube file and add a LUT node (or load it into `into`).
    pub(crate) fn import_lut(&mut self, into: Option<NodeId>, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Load LUT".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let parsed = cx
                .background_spawn(async move {
                    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                    let mut cube = Cube::parse(&text)?;
                    if cube.name.is_empty() {
                        cube.name = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                    }
                    Ok::<Cube, String>(cube)
                })
                .await;
            this.update(cx, |this, cx| match parsed {
                Ok(cube) => {
                    let name = cube.name.clone();
                    let adj = Adjustment::Lut3D {
                        cube,
                        strength: 100.0,
                    };
                    match into {
                        Some(id) => {
                            this.execute(
                                Command::SetAdjustment {
                                    id,
                                    adjustment: adj,
                                },
                                cx,
                            );
                        }
                        None => {
                            let mut node = Node::adjust(0, adj);
                            node.name = format!("LUT {name}");
                            this.add_node(node, cx);
                        }
                    }
                }
                Err(e) => this.set_status(format!("Could not load the LUT: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    fn curves_editor(
        &mut self,
        id: NodeId,
        a: &Adjustment,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let channel = self.adjust_ui.channel;
        let pts = channel.points(a).cloned().unwrap_or_else(straight_curve);
        let hist = self.histogram(cx).map(|h| {
            Histogram::normalized(match channel {
                Channel::Master => &h.luma,
                Channel::Red => &h.r,
                Channel::Green => &h.g,
                Channel::Blue => &h.b,
            })
        });
        let bounds_cell: TrackBounds = self.tracks.entry(SliderKey::Curve(id)).or_default().clone();
        let cell2 = bounds_cell.clone();
        let (line, ink, accent, muted) = (p.line, p.ink, p.accent, p.muted);
        let curve_colour: Hsla = match channel {
            Channel::Master => ink,
            Channel::Red => rgb(0xE0453A).into(),
            Channel::Green => rgb(0x5DBB63).into(),
            Channel::Blue => rgb(0x4A90E2).into(),
        };
        let pts2 = pts.clone();
        div()
            .id("curves")
            .size(px(CURVE_PX))
            .border_1()
            .border_color(line)
            .bg(p.soft_bg)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    this.curve_down(id, channel, &bounds_cell, e, cx)
                }),
            )
            .child(
                canvas(
                    move |bounds, _, _| cell2.set(Some(bounds)),
                    move |bounds, _, window, _| {
                        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                        let to = |x: f32, y: f32| {
                            bounds.origin + point(px(x / 255.0 * w), px((1.0 - y / 255.0) * h))
                        };
                        if let Some(hist) = &hist {
                            for (i, v) in hist.iter().enumerate() {
                                let hh = v.sqrt() * h * 0.9;
                                if hh > 0.0 {
                                    let x = bounds.origin.x + px(i as f32 / 256.0 * w);
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            point(x, bounds.origin.y + px(h - hh)),
                                            size(px(w / 256.0 + 0.5), px(hh)),
                                        ),
                                        muted.opacity(0.25),
                                    ));
                                }
                            }
                        }
                        for k in 1..4 {
                            let f = k as f32 / 4.0;
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin + point(px(f * w), px(0.)),
                                    size(px(1.), px(h)),
                                ),
                                line,
                            ));
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin + point(px(0.), px(f * h)),
                                    size(px(w), px(1.)),
                                ),
                                line,
                            ));
                        }
                        let mut pb = PathBuilder::stroke(px(1.5));
                        pb.move_to(to(0.0, curve_at(&pts2, 0.0)));
                        for x in 1..=64 {
                            let xv = x as f32 / 64.0 * 255.0;
                            pb.line_to(to(xv, curve_at(&pts2, xv)));
                        }
                        if let Ok(path) = pb.build() {
                            window.paint_path(path, curve_colour);
                        }
                        for q in &pts2 {
                            let c = to(q[0], q[1]);
                            window.paint_quad(
                                fill(
                                    Bounds::new(c - point(px(3.5), px(3.5)), size(px(7.), px(7.))),
                                    gpui_kit::white(),
                                )
                                .border_widths(px(1.))
                                .border_color(accent),
                            );
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }

    #[cfg(test)]
    pub(crate) fn curve_bounds(&self, id: NodeId) -> Option<Bounds<Pixels>> {
        self.tracks.get(&SliderKey::Curve(id)).and_then(|c| c.get())
    }

    /// Position in curve space (0–255 both axes) of a window point.
    fn curve_pos(bounds: &Bounds<Pixels>, pos: Point<Pixels>) -> (f32, f32) {
        let (w, h) = (
            f32::from(bounds.size.width).max(1.0),
            f32::from(bounds.size.height).max(1.0),
        );
        let x = (f32::from(pos.x - bounds.origin.x) / w * 255.0).clamp(0.0, 255.0);
        let y = ((1.0 - f32::from(pos.y - bounds.origin.y) / h) * 255.0).clamp(0.0, 255.0);
        (x, y)
    }

    fn curve_down(
        &mut self,
        id: NodeId,
        channel: Channel,
        cell: &TrackBounds,
        e: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = cell.get() else { return };
        let Some(NodeKind::Adjust(a)) = self.editor.doc.node(id).map(|n| &n.kind) else {
            return;
        };
        let mut a = a.clone();
        let (x, y) = Self::curve_pos(&bounds, e.position);
        let Some(pts) = channel.points_mut(&mut a) else {
            return;
        };
        let tol = 8.0 / f32::from(bounds.size.width).max(1.0) * 255.0;
        let near = pts
            .iter()
            .position(|q| (q[0] - x).abs() <= tol && (q[1] - y).abs() <= tol * 1.5);
        self.editor.begin("Curves");
        let idx = match near {
            Some(i) if e.modifiers.alt => {
                if pts.len() > 2 {
                    pts.remove(i);
                    self.execute(Command::SetAdjustment { id, adjustment: a }, cx);
                }
                self.editor.end();
                return;
            }
            Some(i) => i,
            None => {
                let i = pts.iter().position(|q| q[0] > x).unwrap_or(pts.len());
                pts.insert(i, [x, curve_at(pts, x)]);
                self.execute(Command::SetAdjustment { id, adjustment: a }, cx);
                i
            }
        };
        self.drag = Some(Drag::Curve(CurveDrag {
            id,
            channel,
            idx,
            bounds: cell.clone(),
        }));
    }

    pub(crate) fn curve_move(&mut self, d: &CurveDrag, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(bounds) = d.bounds.get() else { return };
        let Some(NodeKind::Adjust(a)) = self.editor.doc.node(d.id).map(|n| &n.kind) else {
            return;
        };
        let mut a = a.clone();
        let (x, y) = Self::curve_pos(&bounds, pos);
        let Some(pts) = d.channel.points_mut(&mut a) else {
            return;
        };
        if d.idx >= pts.len() {
            return;
        }
        // Points keep their order; the ends stay pinned to the sides.
        let lo = if d.idx == 0 {
            0.0
        } else {
            pts[d.idx - 1][0] + 1.0
        };
        let hi = if d.idx + 1 == pts.len() {
            255.0
        } else {
            pts[d.idx + 1][0] - 1.0
        };
        let x = if d.idx == 0 {
            0.0
        } else if d.idx + 1 == pts.len() {
            255.0
        } else {
            x.clamp(lo, hi.max(lo))
        };
        pts[d.idx] = [x, y];
        self.execute(
            Command::SetAdjustment {
                id: d.id,
                adjustment: a,
            },
            cx,
        );
    }
}

// ── The Adjust strip: the usual tools, one click each ───────────────────

/// Adjustments grouped the way an editor's menu would show them.
const QUICK_ADJUST: &[(&str, &[&str])] = &[
    (
        "Light",
        &["exposure", "brightness_contrast", "levels", "curves"],
    ),
    (
        "Colour",
        &[
            "white_balance",
            "hue_saturation",
            "color_balance",
            "vibrance",
            "photo_filter",
            "black_and_white",
            "gradient_map",
        ],
    ),
    ("Effects", &["grain", "vignette", "posterize", "threshold"]),
];

/// Filters offered in the strip, by catalogue label.
const QUICK_FILTERS: &[&str] = &[
    "Gaussian blur",
    "Lens blur",
    "Motion blur",
    "Unsharp mask",
    "Smart sharpen",
    "Reduce noise",
    "Add noise",
    "High pass",
    "Lens correction",
];

impl EditorView {
    /// Add an adjustment above the selection and show its sliders.
    pub fn quick_adjust(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(a) = Adjustment::catalogue().into_iter().find(|a| a.key() == key) else {
            return;
        };
        let node = Node::adjust(0, a);
        let slot = self.insertion_slot();
        if let Some(id) = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot,
            },
            cx,
        ) {
            self.selected = Some(id);
            self.set_status(
                "Added — drag its sliders below; hide or delete the node to undo the look.",
                false,
                cx,
            );
        }
    }

    /// Add a filter to the selected pixel node, making it a smart layer
    /// first if it is plain pixels.
    pub fn quick_filter(&mut self, label: &str, cx: &mut Context<Self>) {
        let Some(f) = emulsion_filters::Filter::catalogue()
            .into_iter()
            .find(|f| f.label() == label)
        else {
            return;
        };
        let Some(id) = self.selected else {
            self.set_status("Select a pixel node to filter first.", false, cx);
            return;
        };
        let kind = self.editor.doc.node(id).map(|n| n.kind.tag());
        match kind {
            Some("pixels") => {
                self.editor.begin(format!("{} filter", f.label()));
                self.convert_smart(cx);
                self.add_filter(id, f, cx);
                self.editor.end();
            }
            Some("smart") => self.add_filter(id, f, cx),
            _ => self.set_status("Filters apply to pixel nodes; select one first.", false, cx),
        }
    }

    pub(crate) fn quick_adjust_view(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .px(px(15.))
            .py(px(9.))
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(label("Adjust", p))
                    .child(div().flex_1())
                    .child(
                        chip("qa-lut", "LUT…", false, p)
                            .on_click(cx.listener(|this, _, _, cx| this.import_lut(None, cx))),
                    ),
            );
        let catalogue = Adjustment::catalogue();
        for (group, keys) in QUICK_ADJUST {
            let mut row = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(4.))
                .child(mono(group.to_string(), 9., p.muted).w(px(44.)).flex_none());
            for (i, key) in keys.iter().enumerate() {
                let Some(a) = catalogue.iter().find(|a| a.key() == *key) else {
                    continue;
                };
                let text = a.label();
                let k: &'static str = key;
                let id = group.len() * 100 + i;
                row = row.child(
                    chip(("qa", id), text, false, p)
                        .on_click(cx.listener(move |this, _, _, cx| this.quick_adjust(k, cx))),
                );
            }
            body = body.child(row);
        }
        let mut frow = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.))
            .child(mono("Filters", 9., p.muted).w(px(44.)).flex_none());
        for (i, name) in QUICK_FILTERS.iter().enumerate() {
            let n: &'static str = name;
            frow = frow.child(
                chip(("qf", i), n, false, p)
                    .on_click(cx.listener(move |this, _, _, cx| this.quick_filter(n, cx))),
            );
        }
        body.child(frow).into_any_element()
    }
}
