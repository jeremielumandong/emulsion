//! Responsive canvas widths are isolated views, never authored page resizes.
use super::*;
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
};

#[derive(Default)]
pub(super) struct ResponsivePreview {
    pub(super) doc: Option<Document>,
    saved_view: Option<View>,
    generation: u64,
    gpu: Rc<RefCell<crate::viewport_gpu::Status>>,
}
impl EditorView {
    pub(super) fn responsive_preview_active(&self) -> bool {
        self.responsive_preview.doc.is_some()
    }
    pub(super) fn responsive_canvas_size(&self) -> (u32, u32) {
        self.responsive_preview
            .doc
            .as_ref()
            .map_or((self.editor.doc.width, self.editor.doc.height), |d| {
                (d.width, d.height)
            })
    }
    pub(super) fn responsive_preview_gpu_frame(
        &self,
    ) -> Option<(Document, u64, Rc<RefCell<crate::viewport_gpu::Status>>)> {
        Some((
            self.responsive_preview.doc.as_ref()?.clone(),
            self.responsive_preview.generation,
            self.responsive_preview.gpu.clone(),
        ))
    }
    pub(crate) fn set_responsive_preview(
        &mut self,
        width: u32,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.is_design() && !self.is_diagram() {
            return Err(
                "Responsive width preview is available in Design and Diagram projects.".into(),
            );
        }
        if !(1..=100000).contains(&width) {
            return Err("Preview width must be 1–100000 pixels.".into());
        }
        if self.editor.in_transaction()
            || self.styles_ui.dialog_for.is_some()
            || self.raw.is_pending()
        {
            return Err("Finish the current edit before previewing responsive widths.".into());
        }
        self.stop_motion(cx);
        self.finish_pointer_gesture(cx);
        self.close_text_field(cx);
        self.finish_shape_color_edit(cx);
        let mut source = self.editor.doc.clone();
        // View-only layout evaluation may move locked source objects in the clone.
        for node in &mut source.nodes {
            node.locked = false;
            node.locks = Default::default();
        }
        let resized =
            emulsion_core::design_metadata::resize_variant(&source, width, source.height)?;
        self.responsive_preview.saved_view.get_or_insert(self.view);
        self.responsive_preview.doc = Some(resized.doc);
        self.responsive_preview.generation = self.responsive_preview.generation.wrapping_add(1);
        self.responsive_preview.gpu = Default::default();
        self.fit_pending = true;
        self.seen_rev = u64::MAX;
        self.tree_dirty = emulsion_core::Dirty::All;
        self.notify_canvas(cx);
        cx.notify();
        Ok(())
    }
    pub(super) fn exit_responsive_preview(&mut self, cx: &mut Context<Self>) -> bool {
        if self.responsive_preview.doc.take().is_none() {
            return false;
        }
        if let Some(view) = self.responsive_preview.saved_view.take() {
            self.view = view;
        }
        self.responsive_preview.gpu = Default::default();
        self.responsive_preview.generation = self.responsive_preview.generation.wrapping_add(1);
        self.fit_pending = false;
        self.seen_rev = u64::MAX;
        self.tree_dirty = emulsion_core::Dirty::All;
        self.notify_canvas(cx);
        cx.notify();
        true
    }
    pub(super) fn responsive_preview_state(&self) -> serde_json::Value {
        serde_json::json!({"active":self.responsive_preview_active(),"width":self.responsive_preview.doc.as_ref().map(|d|d.width),"height":self.responsive_preview.doc.as_ref().map(|d|d.height),"breakpoints":self.responsive_preview.doc.as_ref().map(|doc|doc.design.frames.keys().map(|id|serde_json::json!({"group":id,"active_breakpoint":emulsion_core::design_layout::active_breakpoint(doc,*id),"effective_frame":emulsion_core::design_layout::effective_frame(doc,*id)})).collect::<Vec<_>>())})
    }
    pub(crate) fn responsive_preview_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).default_value(self.responsive_canvas_size().0.to_string())
        });
        let owner = cx.weak_entity();
        let error = cx.new(|_| String::new());
        window.open_dialog(cx, move |dialog, _, cx| {
            let input = input.clone();
            let owner = owner.clone();
            let error_apply = error.clone();
            dialog
                .title("Preview canvas width")
                .width(px(380.))
                .child("View-only width in pixels. Page height stays unchanged.")
                .child(Input::new(&input).id("responsive-preview-width"))
                .footer(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .when(!error.read(cx).is_empty(), |d| {
                            d.child(
                                div()
                                    .id("responsive-preview-error")
                                    .test_support()
                                    .child(error.read(cx).clone()),
                            )
                        })
                        .child(crate::widgets::form_dialog_footer("Preview")),
                )
                .on_ok(move |_, window, cx| {
                    let result = input
                        .read(cx)
                        .value()
                        .parse::<u32>()
                        .map_err(|_| "Enter an integer width.".to_owned())
                        .and_then(|width| {
                            owner
                                .update(cx, |this, cx| this.set_responsive_preview(width, cx))
                                .unwrap_or_else(|_| {
                                    Err("The document is no longer available.".into())
                                })
                        });
                    match result {
                        Ok(()) => {
                            owner
                                .update(cx, |this, cx| window.focus(&this.canvas_focus, cx))
                                .ok();
                            true
                        }
                        Err(e) => {
                            error_apply.update(cx, |v, cx| {
                                *v = e;
                                cx.notify();
                            });
                            window.refresh();
                            false
                        }
                    }
                })
        });
    }
    pub(super) fn responsive_preview_controls(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(
                [(1440, "Desktop"), (768, "Tablet"), (390, "Phone")]
                    .into_iter()
                    .map(|(width, label)| {
                        Button::new(("responsive-preview-preset", width as usize))
                            .label(label)
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Err(e) = this.set_responsive_preview(width, cx) {
                                    this.set_status(e, true, cx);
                                }
                                window.focus(&this.canvas_focus, cx);
                            }))
                    }),
            )
            .child(
                Button::new("responsive-preview-custom")
                    .label("Custom width…")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.responsive_preview_dialog(window, cx)
                    })),
            )
            .when(self.responsive_preview_active(), |d| {
                d.child(
                    Button::new("responsive-preview-exit")
                        .label("Exit preview")
                        .small()
                        .primary()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.exit_responsive_preview(cx);
                        })),
                )
            })
            .into_any_element()
    }
    pub(super) fn responsive_preview_view(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let (width, height) = self.responsive_canvas_size();
        div()
            .id("design-responsive-preview")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.exit_responsive_preview(cx);
                    window.focus(&this.canvas_focus, cx);
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .p_2()
                    .bg(p.panel)
                    .child(format!(
                        "Responsive preview · {width} × {height} · Read only"
                    ))
                    .child(self.responsive_preview_controls(cx)),
            )
            .child(self.canvas_region())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::{
        Node,
        command::Slot,
        design_layout::{self, Breakpoint, Flow, Frame, FrameOverrides},
        project::{ProjectEditor, ProjectKind},
    };
    use gpui_kit::test::TestWindowExt;
    #[gpui_kit::test]
    fn responsive_preview_native_presets_host_state_and_exit_preserve_authored_stamp(
        cx: &mut TestAppContext,
    ) {
        let mut editor = emulsion_core::Editor::new(Document::new(1200, 600), None);
        let node = editor
            .execute(Command::AddNode {
                node: Box::new(Node::path(
                    0,
                    "Card",
                    Arc::new(emulsion_raster::vector_geometry::rectangle(
                        20., 20., 80., 40.,
                    )),
                    Default::default(),
                    1200,
                    600,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let group = editor
            .execute(Command::Group {
                ids: vec![node],
                name: "Responsive".into(),
            })
            .unwrap()
            .unwrap();
        design_layout::enable(
            &mut editor,
            group,
            Frame {
                flow: Flow::Row,
                breakpoints: vec![Breakpoint {
                    min_width: 600.,
                    overrides: FrameOverrides {
                        flow: Some(Flow::Column),
                        gap: Some(30.),
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            (400., 300.),
        )
        .unwrap();
        let authored = editor.doc.clone();
        let (workspace, cx) = crate::tests::open(cx, authored.clone());
        cx.simulate_resize(size(px(1200.), px(900.)));
        let view = cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, authored.clone()).unwrap(),
                    "Widths".into(),
                    window,
                    cx,
                )
            });
            workspace.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        let (stamp, old_view) = cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.set_layer_selection(vec![group], Some(group));
                let stamp = v.editor.stamp();
                let old_view = v.view;
                v.presentation_host_action(
                    emulsion_mcp::design_motion_tools::HostAction::ResponsivePreview(Some(768)),
                    window,
                    cx,
                )
                .unwrap();
                (stamp, old_view)
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("design-responsive-preview").visible());
            let v = view.read(cx);
            assert_eq!(v.responsive_canvas_size(), (768, 600));
            assert_eq!(
                design_layout::effective_frame(v.responsive_preview.doc.as_ref().unwrap(), group)
                    .unwrap()
                    .flow,
                Flow::Column
            );
            assert_eq!(v.editor.doc, authored);
            assert_eq!(v.editor.stamp(), stamp);
            window.click(("responsive-preview-preset", 390usize), cx);
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("ctrl-z delete backspace ctrl-v right");
        cx.run_until_parked();
        cx.update(|window, cx| {
            let v = view.read(cx);
            assert_eq!(v.responsive_canvas_size(), (390, 600));
            assert_eq!(
                design_layout::effective_frame(v.responsive_preview.doc.as_ref().unwrap(), group)
                    .unwrap()
                    .flow,
                Flow::Row
            );
            assert_eq!(v.editor.stamp(), stamp);
            assert_eq!(v.editor.doc, authored);
            window.click("responsive-preview-exit", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                assert!(!v.responsive_preview_active());
                assert_eq!(v.view, old_view);
                assert_eq!(v.selected, Some(group));
                assert_eq!(v.editor.stamp(), stamp);
                v.set_responsive_preview(1440, cx).unwrap();
                v.start_motion(true, cx);
                assert!(!v.responsive_preview_active());
                assert!(v.motion.presenting);
                v.stop_motion(cx);
                window.focus(&v.canvas_focus, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(view.read(cx).editor.doc, authored);
            assert_eq!(view.read(cx).editor.stamp(), stamp);
        });
    }
}
