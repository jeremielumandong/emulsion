//! Navigator and Info panels.
//!
//! The navigator is a thumbnail of the composite with the visible area
//! outlined; click or drag on it to pan. Info shows the pointer's document
//! position, the colour under it, the selection's size and the zoom.

use super::*;
use emulsion_raster::IRect;
use emulsion_raster::composite::region;

#[derive(Default)]
pub(crate) struct PanelState {
    pub navigator: bool,
    pub info: bool,
    thumb: Option<(u64, Arc<RenderImage>, (u32, u32))>,
    thumb_loading: Option<u64>,
    /// Last pointer position over the canvas, in window pixels.
    pub pointer: Option<Point<Pixels>>,
    nav_bounds: TrackBounds,
}

const NAV_MAX: u32 = 256;

impl EditorView {
    pub fn toggle_navigator(&mut self, cx: &mut Context<Self>) {
        self.panels.navigator = !self.panels.navigator;
        cx.notify();
    }

    pub fn toggle_info(&mut self, cx: &mut Context<Self>) {
        self.panels.info = !self.panels.info;
        cx.notify();
    }

    /// Remember where the pointer is for the Info panel.
    pub(crate) fn note_pointer(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        if self.panels.info && self.panels.pointer != Some(pos) {
            self.panels.pointer = Some(pos);
            cx.notify();
        }
    }

    fn nav_thumb(&mut self, cx: &mut Context<Self>) -> Option<(Arc<RenderImage>, (u32, u32))> {
        let rev = self.editor.revision;
        if let Some((r, img, size)) = &self.panels.thumb
            && *r == rev
        {
            return Some((img.clone(), *size));
        }
        if self.panels.thumb_loading != Some(rev) {
            self.panels.thumb_loading = Some(rev);
            let doc = self.editor.doc.clone();
            cx.spawn(async move |this, cx| {
                let (w, h, bgra) = cx
                    .background_spawn(async move { super::history::doc_thumb(&doc, NAV_MAX) })
                    .await;
                this.update(cx, |this, cx| {
                    this.panels.thumb =
                        Some((rev, Arc::new(viewport::bgra_image(w, h, bgra)), (w, h)));
                    this.panels.thumb_loading = None;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        self.panels
            .thumb
            .as_ref()
            .map(|(_, img, size)| (img.clone(), *size))
    }

    /// Centre the view on the navigator point under `pos`.
    pub(crate) fn nav_click(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(b) = self.panels.nav_bounds.get() else {
            return;
        };
        let fx = (f32::from(pos.x - b.origin.x) / f32::from(b.size.width).max(1.0)).clamp(0.0, 1.0)
            as f64;
        let fy = (f32::from(pos.y - b.origin.y) / f32::from(b.size.height).max(1.0)).clamp(0.0, 1.0)
            as f64;
        self.view.center = (
            fx * self.editor.doc.width as f64,
            fy * self.editor.doc.height as f64,
        );
        cx.notify();
    }

    pub(crate) fn navigator_view(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.panels.navigator {
            return None;
        }
        let thumb = self.nav_thumb(cx);
        let (dw, dh) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
        // Visible document rectangle, as fractions.
        let vis = self.canvas_bounds().map(|cb| {
            let a = self.view.screen_to_doc(
                (f32::from(cb.origin.x) as f64, f32::from(cb.origin.y) as f64),
                &cb,
            );
            let b = self.view.screen_to_doc(
                (
                    f32::from(cb.origin.x + cb.size.width) as f64,
                    f32::from(cb.origin.y + cb.size.height) as f64,
                ),
                &cb,
            );
            (
                (a.0 / dw).clamp(0.0, 1.0) as f32,
                (a.1 / dh).clamp(0.0, 1.0) as f32,
                (b.0 / dw).clamp(0.0, 1.0) as f32,
                (b.1 / dh).clamp(0.0, 1.0) as f32,
            )
        });
        let aspect = dw / dh;
        let (w, h) = if aspect >= 1.0 {
            (240.0, 240.0 / aspect)
        } else {
            (240.0 * aspect, 240.0)
        };
        let cell = self.panels.nav_bounds.clone();
        let accent = p.accent;
        let image: AnyElement = match thumb {
            Some((img, _)) => img_el(img).size_full().into_any_element(),
            None => div().size_full().bg(p.stage).into_any_element(),
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(15.))
                .py(px(10.))
                .border_b_1()
                .border_color(p.line)
                .child(label("Navigator", p))
                .child(
                    div()
                        .id("navigator")
                        .w(px(w as f32))
                        .h(px(h as f32))
                        .relative()
                        .border_1()
                        .border_color(p.line)
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                this.nav_click(e.position, cx);
                                this.drag = Some(Drag::Navigator);
                            }),
                        )
                        .child(image)
                        .child(
                            canvas(
                                move |bounds, _, _| cell.set(Some(bounds)),
                                move |bounds, _, window, _| {
                                    if let Some((x0, y0, x1, y1)) = vis {
                                        let (bw, bh) = (
                                            f32::from(bounds.size.width),
                                            f32::from(bounds.size.height),
                                        );
                                        let r = Bounds::from_corners(
                                            bounds.origin + point(px(x0 * bw), px(y0 * bh)),
                                            bounds.origin + point(px(x1 * bw), px(y1 * bh)),
                                        );
                                        window.paint_quad(
                                            fill(r, accent.opacity(0.12))
                                                .border_widths(px(1.5))
                                                .border_color(accent),
                                        );
                                    }
                                },
                            )
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn info_view(&self, p: &Palette) -> Option<AnyElement> {
        if !self.panels.info {
            return None;
        }
        let (doc_pos, colour) = match self.panels.pointer.and_then(|pt| self.doc_point(pt)) {
            Some(d) => {
                let (w, h) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
                let inside = d.0 >= 0.0 && d.1 >= 0.0 && d.0 < w && d.1 < h;
                let colour = inside.then(|| {
                    let px = region(
                        &self.tree,
                        IRect::new(d.0.floor() as i32, d.1.floor() as i32, 1, 1),
                    )[0];
                    let c = color::premul_to_srgba8(px);
                    format!("#{:02X}{:02X}{:02X} · α {}", c[0], c[1], c[2], c[3])
                });
                (
                    format!("x {:.0}  y {:.0}", d.0.floor(), d.1.floor()),
                    colour.unwrap_or_else(|| "outside the canvas".into()),
                )
            }
            None => ("—".into(), "—".into()),
        };
        let selection = match &self.editor.doc.selection {
            Some(s) => {
                let b = emulsion_raster::select::bounds(s);
                format!("{}×{} at {}, {}", b.w, b.h, b.x, b.y)
            }
            None => "none".into(),
        };
        let row = |k: &str, v: String| {
            div()
                .flex()
                .justify_between()
                .gap(px(10.))
                .child(mono(k.to_string(), 10., p.muted))
                .child(mono(v, 10., p.ink))
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .px(px(15.))
                .py(px(10.))
                .border_b_1()
                .border_color(p.line)
                .child(label("Info", p))
                .child(row("pointer", doc_pos))
                .child(row("colour", colour))
                .child(row("selection", selection))
                .child(row(
                    "zoom",
                    format!(
                        "{:.0}% · {:.0}°",
                        self.view.zoom * 100.0,
                        self.view.rotation
                    ),
                ))
                .child(row(
                    "document",
                    format!(
                        "{}×{} · {} nodes",
                        self.editor.doc.width,
                        self.editor.doc.height,
                        self.editor.doc.nodes.len()
                    ),
                ))
                .into_any_element(),
        )
    }
}

fn img_el(image: Arc<RenderImage>) -> Img {
    img(ImageSource::Render(image)).object_fit(ObjectFit::Contain)
}
