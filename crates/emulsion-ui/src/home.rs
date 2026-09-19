//! The Home screen: a headline, new/open, and recent files.

use crate::theme::{self, Palette};
use crate::viewport::bgra_image;
use crate::widgets::{button, label, mono};
use crate::workspace::Workspace;
use emulsion_io::recent;
use gpui_kit::*;
use std::sync::Arc;

const FACTS: [(&str, &str); 3] = [
    (
        "Nodes, not layers",
        "Every adjustment is a node with live parameters. Reopen it next month and change one number.",
    ),
    (
        "Nothing is destroyed",
        "Layers keep their full resolution however you place them. Every step is undoable.",
    ),
    (
        "Opens what you have",
        "OpenRaster, PNG, JPEG, WebP and TIFF, with 16-bit sources kept at 16 bits.",
    ),
];

impl Workspace {
    fn load_thumbs(&mut self, cx: &mut Context<Self>) {
        for r in self.recents.clone() {
            if self.thumbs.contains_key(&r.path) || !self.thumbs_loading.insert(r.path.clone()) {
                continue;
            }
            let path = r.path.clone();
            cx.spawn(async move |this, cx| {
                let p = path.clone();
                let result = cx
                    .background_spawn(async move {
                        emulsion_io::thumb::thumbnail(&p, 480).map(|(w, h, mut rgba)| {
                            for px in rgba.as_chunks_mut::<4>().0 {
                                px.swap(0, 2);
                            }
                            (w, h, rgba)
                        })
                    })
                    .await;
                this.update(cx, |this, cx| {
                    if let Ok((w, h, bgra)) = result {
                        this.thumbs.insert(path, Arc::new(bgra_image(w, h, bgra)));
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    pub(crate) fn home(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = theme::palette(cx);
        self.load_thumbs(cx);
        let date = {
            let n = self.recents.len();
            format!("{n} recent file{}", if n == 1 { "" } else { "s" })
        };
        let cells: Vec<AnyElement> = self
            .recents
            .clone()
            .into_iter()
            .enumerate()
            .map(|(i, r)| self.recent_cell(i, &r, &p, cx).into_any_element())
            .collect();
        div()
            .id("home")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .justify_between()
                    .gap(px(30.))
                    .px(px(40.))
                    .pt(px(52.))
                    .pb(px(32.))
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .min_w(px(250.))
                            .child(label(date, &p))
                            .child(
                                div()
                                    .text_size(px(64.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .line_height(relative(0.95))
                                    .child("Every edit,")
                                    .child("still undoable."),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(
                                button("new", "New canvas", false, &p)
                                    .px(px(22.))
                                    .py(px(14.))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.new_document(window, cx)
                                    })),
                            )
                            .child(
                                button("open", "Open a file", true, &p)
                                    .px(px(22.))
                                    .py(px(14.))
                                    .on_click(cx.listener(|_, _, window, cx| {
                                        window.dispatch_action(Box::new(crate::actions::Open), cx)
                                    })),
                            ),
                    ),
            )
            .child(if cells.is_empty() {
                div()
                    .px(px(40.))
                    .py(px(40.))
                    .border_b_1()
                    .border_color(p.line)
                    .child(mono(
                        "No recent files yet. Open an image or start a new canvas.",
                        11.,
                        p.muted,
                    ))
            } else {
                div()
                    .grid()
                    .grid_cols(4)
                    .gap(px(1.))
                    .bg(p.line)
                    .border_b_1()
                    .border_color(p.line)
                    .children(cells)
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(26.))
                    .px(px(40.))
                    .pt(px(32.))
                    .pb(px(54.))
                    .children(FACTS.iter().map(|(tag, body)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .w(px(300.))
                            .child(mono(tag.to_uppercase(), 9.5, p.accent))
                            .child(div().text_size(px(15.)).child(*body))
                    })),
            )
    }

    fn recent_cell(
        &self,
        i: usize,
        r: &recent::Recent,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let kind = r
            .path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let name = r
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let meta = format!("{} · {}", recent::ago(r.opened), r.summary);
        let thumb: AnyElement = match self.thumbs.get(&r.path) {
            Some(t) => img(ImageSource::Render(t.clone()))
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div().size_full().bg(p.line).into_any_element(),
        };
        let path = r.path.clone();
        let accent = p.accent;
        div()
            .id(("recent", i))
            .flex()
            .flex_col()
            .bg(p.paper)
            .cursor_pointer()
            .hover(move |s| s.bg(accent.opacity(0.06)))
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_path(path.clone(), window, cx)),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .aspect_ratio(4.0 / 3.0)
                    .overflow_hidden()
                    .child(thumb)
                    .child(
                        div()
                            .absolute()
                            .top(px(9.))
                            .left(px(9.))
                            .px(px(6.))
                            .py(px(2.))
                            .bg(p.ink.opacity(0.8))
                            .child(mono(kind, 9.5, gpui_kit::white())),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .px(px(13.))
                    .pt(px(12.))
                    .pb(px(15.))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(name),
                    )
                    .child(mono(meta, 10., p.muted)),
            )
    }
}
