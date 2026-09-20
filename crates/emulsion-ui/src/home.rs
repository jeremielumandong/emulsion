//! The Home screen: a headline, new/open, and recent files.

use crate::theme::{self, Palette};
use crate::viewport::bgra_image;
use crate::widgets::{button, mono};
use crate::workspace::Workspace;
use emulsion_io::recent;
use gpui_kit::*;
use std::sync::Arc;

pub(crate) struct GalleryThumbnail {
    requested_width: u32,
    image: Option<Arc<RenderImage>>,
}

fn thumbnail_width(viewport_width: f32, scale_factor: f32) -> u32 {
    let pixels = (viewport_width / 4.0 * scale_factor).ceil() as u32;
    // Bucket resize requests to avoid rebuilding on every single-pixel drag.
    pixels.div_ceil(128).saturating_mul(128).clamp(128, 2048)
}

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
    /// Drop a file from the recent list without touching the file.
    pub fn remove_recent(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        self.recents = emulsion_io::recent::remove(path);
        self.invalidate_thumbnail(path);
        cx.notify();
    }

    pub(crate) fn invalidate_thumbnail(&mut self, path: &std::path::Path) {
        self.thumbs.remove(path);
        // An earlier read must not overwrite a newly saved document's preview.
        self.thumbs_loading.remove(path);
    }

    fn load_thumbs(&mut self, width: u32, cx: &mut Context<Self>) {
        for r in self.recents.clone() {
            if self
                .thumbs
                .get(&r.path)
                .is_some_and(|thumb| thumb.requested_width >= width)
                || self.thumbs_loading.contains_key(&r.path)
            {
                continue;
            }
            // Full saved composites can be large. Limit simultaneous decodes.
            if self.thumbs_loading.len() >= 2 {
                break;
            }
            self.thumb_generation = self.thumb_generation.wrapping_add(1);
            let generation = self.thumb_generation;
            self.thumbs_loading.insert(r.path.clone(), generation);
            let path = r.path.clone();
            cx.spawn(async move |this, cx| {
                let p = path.clone();
                let result = cx
                    .background_spawn(async move {
                        emulsion_io::thumb::thumbnail_cover(&p, width, width * 3 / 4).map(
                            |(w, h, mut rgba)| {
                                for px in rgba.as_chunks_mut::<4>().0 {
                                    px.swap(0, 2);
                                }
                                (w, h, rgba)
                            },
                        )
                    })
                    .await;
                this.update(cx, |this, cx| {
                    if this.thumbs_loading.get(&path) != Some(&generation) {
                        return;
                    }
                    this.thumbs_loading.remove(&path);
                    let previous = this.thumbs.remove(&path).and_then(|thumb| thumb.image);
                    let image = result
                        .ok()
                        .map(|(w, h, bgra)| Arc::new(bgra_image(w, h, bgra)))
                        .or(previous);
                    this.thumbs.insert(
                        path,
                        GalleryThumbnail {
                            requested_width: width,
                            image,
                        },
                    );
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    pub(crate) fn home(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = theme::palette(cx);
        self.load_thumbs(
            thumbnail_width(
                f32::from(window.viewport_size().width),
                window.scale_factor(),
            ),
            cx,
        );
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
            .child(self.hero(date, &p, cx))
            .children(self.recovered_rows(&p, cx))
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

    /// Work an earlier session left unsaved, if any.
    fn recovered_rows(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        if self.recovered.is_empty() {
            return None;
        }
        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(40.))
            .py(px(22.))
            .border_b_1()
            .border_color(p.line)
            .bg(p.panel)
            .child(mono("RECOVERED WORK · NOT SAVED LAST TIME", 9.5, p.accent));
        for (i, (path, t)) in self.recovered.iter().enumerate() {
            let (open, discard) = (path.clone(), path.clone());
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .child(crate::workspace::recovered_name(path)),
                    )
                    .child(mono(format!("autosaved {}", recent::ago(*t)), 10., p.muted))
                    .child(div().flex_1())
                    .child(
                        button(("recover", i), "Open", true, p).on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.open_recovered(open.clone(), window, cx)
                            },
                        )),
                    )
                    .child(
                        button(("discard-recovered", i), "Discard", false, p).on_click(
                            cx.listener(move |this, _, _, cx| this.discard_recovered(&discard, cx)),
                        ),
                    ),
            );
        }
        Some(list)
    }

    /// The landing image, full width, with the headline and actions over it.
    fn hero(&self, date: String, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let foreground = p.chrome_fg;
        let image: AnyElement = match &self.landing {
            Some(i) => img(ImageSource::Render(i.clone()))
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div().size_full().bg(p.chrome).into_any_element(),
        };
        div()
            .id("hero")
            .relative()
            .w_full()
            .h(px(480.))
            .flex_none()
            .overflow_hidden()
            .bg(p.chrome)
            .border_b_1()
            .border_color(p.line)
            .child(image)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(p.chrome.opacity(0.88), 0.),
                        linear_color_stop(p.chrome.opacity(0.0), 0.62),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .left(px(40.))
                    .right(px(40.))
                    .bottom(px(36.))
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .child(mono(date.to_uppercase(), 9.5, foreground.opacity(0.75)))
                    .child(
                        div()
                            .text_size(px(64.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(relative(0.95))
                            .text_color(foreground)
                            .child("Every edit,")
                            .child("still undoable."),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(10.))
                            .child(
                                button("open", "Open a file", true, p)
                                    .px(px(22.))
                                    .py(px(14.))
                                    .on_click(cx.listener(|_, _, window, cx| {
                                        window.dispatch_action(Box::new(crate::actions::Open), cx)
                                    })),
                            )
                            .child(
                                button("new", "New canvas", false, p)
                                    .px(px(22.))
                                    .py(px(14.))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.new_document(window, cx)
                                    })),
                            )
                            .child(
                                button("edit-landing", "Edit this image", false, p)
                                    .px(px(22.))
                                    .py(px(14.))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_landing(window, cx)
                                    })),
                            ),
                    ),
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
        let thumb: AnyElement = match self
            .thumbs
            .get(&r.path)
            .and_then(|thumb| thumb.image.as_ref())
        {
            Some(t) => img(ImageSource::Render(t.clone()))
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div().size_full().bg(p.line).into_any_element(),
        };
        let path = r.path.clone();
        let forget = r.path.clone();
        let accent = p.accent;
        let accent_fg = p.accent_fg;
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
                    )
                    .child(
                        // Forget this entry (the file stays where it is).
                        div()
                            .id(("recent-forget", i))
                            .absolute()
                            .top(px(7.))
                            .right(px(7.))
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(p.ink.opacity(0.75))
                            .text_color(gpui_kit::white())
                            .text_size(px(12.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(accent).text_color(accent_fg))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.remove_recent(&forget, cx);
                            }))
                            .child("×"),
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
