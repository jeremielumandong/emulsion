//! A reference stays beside the canvas and is shared with the assistant through
//! a read-only image tool. It never becomes a document node or an undo step.

use crate::editor::EditorView;
use crate::theme::Palette;
use crate::widgets::{chip, label, mono};
use emulsion_mcp::reference::ReferenceImage;
use emulsion_mcp::server::ToolResult;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) struct AttachedReference {
    pub image: Arc<ReferenceImage>,
    preview: Arc<RenderImage>,
}

impl AttachedReference {
    fn load(path: &std::path::Path) -> Result<Self, String> {
        let reference = ReferenceImage::load(path).map_err(|e| e.to_string())?;
        let image = image::load_from_memory_with_format(reference.png(), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?
            .thumbnail(480, 480)
            .into_rgba8();
        let (w, h) = image.dimensions();
        let mut bgra = image.into_raw();
        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        Ok(Self {
            image: Arc::new(reference),
            preview: Arc::new(crate::viewport::bgra_image(w, h, bgra)),
        })
    }
}

pub(crate) fn reference_prompt(text: &str, reference: Option<&AttachedReference>) -> String {
    match reference {
        Some(r) => format!(
            "{text}\n\n[Emulsion reference attachment]\nA reference image is attached ({} × {} pixels). Call get_reference_image to inspect it before drawing or painting. Use it as visual material according to my request. Reference coordinates and canvas coordinates are separate. Keep the artwork editable and compare it with the reference when reviewing.",
            r.image.width(),
            r.image.height()
        ),
        None => format!(
            "{text}\n\n[Emulsion reference attachment]\nNo reference image is attached for this turn. Do not reuse an earlier attachment unless I explicitly ask you to."
        ),
    }
}

impl EditorView {
    pub(crate) fn prompt_reference(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.assistant.running || self.assistant.reference_loading {
            self.set_status(
                "Finish the current request before changing its reference.",
                false,
                cx,
            );
            return;
        }
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Add reference image".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| this.load_reference(path, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub(crate) fn load_reference(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // Recheck after the picker: a turn may have started while it was open.
        if self.assistant.running || self.assistant.reference_loading {
            self.set_status(
                "Finish the current request before changing its reference.",
                false,
                cx,
            );
            return;
        }
        self.assistant.reference_loading = true;
        self.set_status("Loading reference…", false, cx);
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_spawn(async move { AttachedReference::load(&path) })
                .await;
            this.update(cx, |this, cx| {
                this.assistant.reference_loading = false;
                match loaded {
                    Ok(reference) => {
                        this.assistant.reference = Some(reference);
                        this.select_sidebar(crate::editor::SidebarTab::Reference, cx);
                        this.assistant.reference_collapsed = false;
                        this.set_status(
                            "Reference added. Draw alongside it or ask the assistant to use it.",
                            false,
                            cx,
                        );
                    }
                    Err(e) => this.set_status(format!("Could not load reference: {e}"), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn remove_reference(&mut self, cx: &mut Context<Self>) {
        if self.assistant.running || self.assistant.reference_loading {
            self.set_status(
                "Finish the current request before changing its reference.",
                false,
                cx,
            );
            return;
        }
        self.assistant.reference = None;
        self.set_status("Reference removed.", false, cx);
    }

    pub(crate) fn reference_result(&self) -> ToolResult {
        self.assistant.reference.as_ref().map_or_else(
            || ToolResult::error("No reference image is attached. Add one with Add reference."),
            |reference| reference.image.tool_result(),
        )
    }

    pub(crate) fn reference_panel(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let attached = self.assistant.reference.as_ref();
        let collapsed = self.assistant.reference_collapsed;
        let busy = self.assistant.running || self.assistant.reference_loading;
        div()
            .id("reference-panel")
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(8.))
            .p(px(12.))
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(label("REFERENCE", p))
                    .child(div().flex_1())
                    .when(attached.is_some(), |d| {
                        d.child(
                            chip(
                                "reference-collapse",
                                if collapsed { "show" } else { "hide" },
                                false,
                                p,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.assistant.reference_collapsed =
                                    !this.assistant.reference_collapsed;
                                cx.notify();
                            })),
                        )
                    }),
            )
            .when_some(attached, |d, reference| {
                d.when(!collapsed, |d| {
                    d.child(
                        img(ImageSource::Render(reference.preview.clone()))
                            .object_fit(ObjectFit::Contain)
                            .w_full()
                            .h(px(200.)),
                    )
                })
                .child(mono(reference.image.name().to_string(), 10., p.ink).truncate())
                .child(mono(
                    format!(
                        "{} × {} · reference only",
                        reference.image.width(),
                        reference.image.height()
                    ),
                    9.,
                    p.muted,
                ))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(!busy, |d| {
                        d.child(
                            chip(
                                "reference-add",
                                if attached.is_some() {
                                    "Replace"
                                } else {
                                    "Add reference"
                                },
                                false,
                                p,
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.prompt_reference(window, cx)
                                }),
                            ),
                        )
                    })
                    .when(attached.is_some() && !busy, |d| {
                        d.child(
                            chip("reference-remove", "Remove", false, p)
                                .on_click(cx.listener(|this, _, _, cx| this.remove_reference(cx))),
                        )
                    })
                    .when(self.assistant.reference_loading, |d| {
                        d.child(mono("Loading…", 10., p.muted))
                    }),
            )
            .test_support()
            .into_any_element()
    }
}
