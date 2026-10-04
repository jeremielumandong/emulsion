//! Asset placement and an isolated, non-destructive frame-crop preview.
use super::*;
use emulsion_core::project::PageId;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};

pub(super) struct FrameCrop {
    pub(super) preview: Document,
    pub(super) pointer: Option<(f64, f64)>,
    pub(super) image: Option<NodeId>,
    original: Option<Placement>,
    pub(super) commit_label: Option<String>,
    pub(super) color_picker: Option<Entity<gpui_kit::component::color_picker::ColorPickerState>>,
    pub(super) color_subscription: Option<Subscription>,
    ticket: (u64, u64),
    page: PageId,
    generation: u64,
    gpu: Rc<RefCell<crate::viewport_gpu::Status>>,
}

/// Photo formats retain their full decoded raster; layered/vector sources use
/// the existing compositor at their native dimensions, never the thumbnail.
pub(super) fn frame_asset_raster(
    path: &std::path::Path,
) -> emulsion_io::Result<(Arc<Raster>, bool)> {
    // The application opener owns RAW/JXL routing, orientation, color profiles
    // and saved development recipes, matching thumbnails and blank insertion.
    let opened = emulsion_io::open_full(path)?;
    if opened.history_error.is_some() {
        return Err(emulsion_io::IoError::Manifest(
            t!("editor.design_asset_ui.history_damaged").into_owned(),
        ));
    }
    let doc = opened.doc;
    emulsion_io::import::check_size(doc.width, doc.height)?;
    if let [node] = doc.nodes.as_slice()
        && let NodeKind::Raster { raster, placement } = &node.kind
        && *placement == Placement::default()
        && (raster.width(), raster.height()) == (doc.width, doc.height)
        && node.parent.is_none()
        && node.visible
        && node.opacity == 1.
        && node.blend == BlendMode::Normal
        && node.blending == Default::default()
        && node.clip_to.is_none()
        && node.mask.is_none()
        && (!node.effects_enabled || node.styles.is_empty())
    {
        // Preserve all original source samples, including hidden transparent RGB.
        return Ok((raster.clone(), false));
    }
    Ok((
        Arc::new(emulsion_raster::composite::flatten(
            &doc.composite_tree(),
            0,
        )),
        true,
    ))
}

impl EditorView {
    pub(super) fn begin_design_asset_request(&mut self) -> (u64, u64) {
        let ticket = self.begin_edit_job();
        self.design_ui.asset_job = Some(ticket);
        ticket
    }

    pub(super) fn accept_design_asset_result(
        &mut self,
        ticket: (u64, u64),
        page: PageId,
        cx: &mut Context<Self>,
    ) -> bool {
        // A superseded completion must not clear a newer job or its status.
        if self.design_ui.asset_job != Some(ticket) {
            return false;
        }
        self.design_ui.asset_job = None;
        cx.notify();
        self.accept_edit_result(ticket, "Asset placement", cx) && self.editor.active_page() == page
    }

    pub(super) fn cancel_design_asset_load(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(ticket) = self.design_ui.asset_job.take() else {
            return false;
        };
        if self.pending_edit_job == Some(ticket) {
            self.pending_edit_job = None;
        }
        if self.edit_ticket() == ticket {
            self.invalidate_pending_edits();
            self.set_status(t!("editor.design_asset_ui.placement_canceled"), false, cx);
        } else {
            // This load was already superseded; never cancel a newer job.
            cx.notify();
        }
        true
    }

    pub(super) fn design_asset_loading_controls(&self, cx: &Context<Self>) -> Option<AnyElement> {
        self.design_ui.asset_job.map(|_| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .text_size(px(11.))
                .child(t!("editor.design_asset_ui.loading_asset").to_string())
                .child(
                    Button::new("design-cancel-asset-load")
                        .label(t!("shell.cancel"))
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_design_asset_load(cx);
                        })),
                )
                .into_any_element()
        })
    }

    pub(super) fn place_design_asset(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let target = (self.selected_layer_ids().len() == 1)
            .then_some(self.selected)
            .flatten()
            .filter(|id| emulsion_core::design::frame_parts(&self.editor.doc, *id).is_some());
        if let Some(id) = target {
            self.place_design_asset_in_frame(path, id, cx);
        } else {
            self.place_design_assets(vec![path], cx);
        }
    }

    pub(super) fn drop_design_asset(
        &mut self,
        path: PathBuf,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.is_design()
            || self.previewing()
            || !self.canvas_bounds().is_some_and(|b| b.contains(&position))
        {
            return;
        }
        let Some(point) = self.doc_point(position) else {
            return;
        };
        // Include locked artwork in hit-testing: a locked frame is a rejected
        // target, not a hole through which a drop lands on something behind it.
        let target = self
            .design_hit_for_drop(point, true, true)
            .filter(|id| emulsion_core::design::frame_parts(&self.editor.doc, *id).is_some());
        if let Some(id) = target {
            self.place_design_asset_in_frame(path, id, cx);
        } else {
            self.place_design_assets_at(vec![path], Some(point), cx);
        }
    }

    pub(super) fn place_design_asset_in_frame(
        &mut self,
        path: PathBuf,
        id: NodeId,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        if let Err(error) = emulsion_core::design::frame_image_replaceable(&self.editor.doc, id) {
            self.set_status(error, false, cx);
            return;
        }
        let ticket = self.begin_design_asset_request();
        let page = self.editor.active_page();
        self.set_status(t!("editor.design_asset_ui.loading_frame_image"), false, cx);
        cx.spawn(async move |this, cx| {
            let source = path.clone();
            let result = cx
                .background_spawn(async move { frame_asset_raster(&source) })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_design_asset_result(ticket, page, cx) {
                    return;
                }
                let rendered = result.as_ref().is_ok_and(|(_, rendered)| *rendered);
                match result
                    .map_err(|error| error.to_string())
                    .and_then(|(raster, _)| {
                        emulsion_core::design::place_in_frame(&mut this.editor, id, raster)
                    }) {
                    Ok(image) => {
                        // Keep the frame selected instead of exposing an unbounded image move.
                        let boundary = emulsion_core::design::frame_parts(&this.editor.doc, image)
                            .unwrap()
                            .0;
                        let selected = this
                            .editor
                            .doc
                            .node(boundary)
                            .and_then(|n| n.parent)
                            .unwrap_or(boundary);
                        this.set_layer_selection(vec![selected], Some(selected));
                        this.after_change(cx);
                        this.note_creative_asset(
                            path,
                            emulsion_io::creative_library::AssetKind::Image,
                            cx,
                        );
                        this.set_status(
                            if rendered {
                                t!("editor.design_asset_ui.rendered_into_frame")
                            } else {
                                t!("editor.design_asset_ui.frame_image_replaced")
                            },
                            false,
                            cx,
                        );
                    }
                    Err(error) => this.set_status(error, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn frame_crop_active(&self) -> bool {
        self.design_ui.frame_crop.is_some()
    }

    pub(super) fn frame_crop_gpu_frame(
        &self,
    ) -> Option<(Document, u64, Rc<RefCell<crate::viewport_gpu::Status>>)> {
        let crop = self.design_ui.frame_crop.as_ref()?;
        Some((crop.preview.clone(), crop.generation, crop.gpu.clone()))
    }

    pub(super) fn start_frame_crop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_design() || !self.prepare_page_action(cx) {
            return;
        }
        let result = (|| {
            let selected = self
                .selected
                .ok_or_else(|| t!("editor.design_asset_ui.select_image_frame").into_owned())?;
            emulsion_core::design::frame_image_editable(&self.editor.doc, selected)?;
            let (_, image) = emulsion_core::design::frame_parts(&self.editor.doc, selected)
                .ok_or_else(|| t!("editor.design_asset_ui.select_frame").into_owned())?;
            let image =
                image.ok_or_else(|| t!("editor.design_asset_ui.place_image_first").into_owned())?;
            let NodeKind::Raster { placement, .. } = self.editor.doc.node(image).unwrap().kind
            else {
                unreachable!()
            };
            Ok::<_, String>((image, placement))
        })();
        let (image, original) = match result {
            Ok(value) => value,
            Err(error) => {
                self.set_status(error, false, cx);
                return;
            }
        };
        if self.editor.in_transaction() {
            self.set_status(t!("editor.design_asset_ui.finish_before_crop"), false, cx);
            return;
        }
        self.cancel_design_asset_load(cx);
        // All older async edit/picker results belong to the authored view that
        // preceded this modal preview, including jobs outside the asset drawer.
        self.invalidate_pending_edits();
        self.anim.open = false;
        self.anim.playing = false;
        self.design_ui.frame_crop = Some(FrameCrop {
            preview: self.editor.doc.clone(),
            pointer: None,
            image: Some(image),
            original: Some(original),
            commit_label: None,
            color_picker: None,
            color_subscription: None,
            ticket: self.edit_ticket(),
            page: self.editor.active_page(),
            generation: 1,
            gpu: Default::default(),
        });
        window.focus(&self.canvas_focus, cx);
        self.frame_crop_changed(cx);
    }

    pub(super) fn begin_background_preview(
        &mut self,
        preview: Document,
        image: Option<NodeId>,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_font_picker(cx);
        self.cancel_design_asset_load(cx);
        self.invalidate_pending_edits();
        self.anim.open = false;
        self.anim.playing = false;
        self.design_ui.frame_crop = Some(FrameCrop {
            preview,
            pointer: None,
            image,
            original: None,
            commit_label: Some(label),
            color_picker: None,
            color_subscription: None,
            ticket: self.edit_ticket(),
            page: self.editor.active_page(),
            generation: 1,
            gpu: Default::default(),
        });
        window.focus(&self.canvas_focus, cx);
        self.frame_crop_changed(cx);
    }

    pub(super) fn frame_crop_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(crop) = &mut self.design_ui.frame_crop {
            crop.generation = crop.generation.wrapping_add(1);
        }
        self.seen_rev = u64::MAX;
        self.tree_dirty = emulsion_core::Dirty::All;
        self.notify_canvas(cx);
        cx.notify();
    }

    pub(super) fn cancel_frame_crop(&mut self, cx: &mut Context<Self>) -> bool {
        if self.design_ui.frame_crop.take().is_none() {
            return false;
        }
        self.frame_crop_changed(cx);
        true
    }

    pub(super) fn finish_frame_crop(&mut self, cx: &mut Context<Self>) {
        let Some(crop) = self.design_ui.frame_crop.take() else {
            return;
        };
        self.frame_crop_changed(cx);
        if !self.edit_is_current(crop.ticket) || self.editor.active_page() != crop.page {
            self.set_status(t!("editor.design_asset_ui.crop_canceled"), false, cx);
            return;
        }
        if let Some(label) = crop.commit_label {
            match self.editor.commit_design_document(crop.preview, &label) {
                Ok(()) => {
                    if self.selected.is_some_and(|id| {
                        emulsion_core::design_background::is_background_node(&self.editor.doc, id)
                    }) {
                        self.set_layer_selection(Vec::new(), None);
                    }
                    self.after_change(cx);
                }
                Err(error) => self.set_status(error, true, cx),
            }
            return;
        }
        let Some(image) = crop.image else {
            return;
        };
        if let Err(error) = emulsion_core::design::frame_image_editable(&self.editor.doc, image) {
            self.set_status(error, true, cx);
            return;
        }
        if let Some(NodeKind::Raster { placement, .. }) = crop.preview.node(image).map(|n| &n.kind)
            && Some(*placement) != crop.original
        {
            self.execute(
                Command::SetPlacement {
                    id: image,
                    placement: *placement,
                },
                cx,
            );
        }
    }

    pub(super) fn zoom_frame_crop(&mut self, zoom: f64, cx: &mut Context<Self>) {
        self.adjust_frame_crop([0.; 2], zoom, cx);
    }

    fn adjust_frame_crop(&mut self, pan: [f64; 2], zoom: f64, cx: &mut Context<Self>) {
        let Some(crop) = &mut self.design_ui.frame_crop else {
            return;
        };
        let Some(image) = crop.image else {
            return;
        };
        let result = emulsion_core::design::crop_frame_image(&crop.preview, image, pan, zoom)
            .and_then(|command| {
                command
                    .apply(&mut crop.preview)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            });
        match result {
            Ok(()) => self.frame_crop_changed(cx),
            Err(error) => self.set_status(error, false, cx),
        }
    }

    pub(super) fn frame_crop_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.visible || !self.frame_crop_active() {
            return false;
        }
        let pointer = self.doc_point(event.position);
        if event.button == MouseButton::Left {
            window.focus(&self.canvas_focus, cx);
            self.design_ui.frame_crop.as_mut().unwrap().pointer = pointer;
        }
        true
    }

    pub(super) fn frame_crop_pointer_moved(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.frame_crop_active() {
            return false;
        }
        let pointer = self.doc_point(event.position);
        let crop = self.design_ui.frame_crop.as_mut().unwrap();
        if event.pressed_button != Some(MouseButton::Left) {
            crop.pointer = None;
        } else if let (Some(last), Some(current)) = (crop.pointer, pointer) {
            crop.pointer = Some(current);
            self.adjust_frame_crop([current.0 - last.0, current.1 - last.1], 1., cx);
        }
        true
    }

    pub(super) fn frame_crop_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.frame_crop_active() {
            return false;
        }
        let delta = event.delta.pixel_delta(px(20.));
        self.adjust_frame_crop(
            [0.; 2],
            (f64::from(f32::from(delta.y)) * 0.004).exp().clamp(0.5, 2.),
            cx,
        );
        true
    }

    pub(super) fn frame_crop_view(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        use rust_i18n::t;
        if self
            .design_ui
            .frame_crop
            .as_ref()
            .is_some_and(|crop| crop.color_picker.is_some())
        {
            return self.page_background_color_view(p, cx);
        }
        div()
            .id("design-frame-crop-editor")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .key_context("FrameCrop")
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space")
                    && !this.canvas_focus.is_focused(window)
                {
                    return;
                }
                let step = if event.keystroke.modifiers.shift {
                    10.
                } else {
                    1.
                };
                match event.keystroke.key.as_str() {
                    "escape" => {
                        this.cancel_frame_crop(cx);
                        window.focus(&this.canvas_focus, cx);
                    }
                    "enter" => {
                        this.finish_frame_crop(cx);
                        window.focus(&this.canvas_focus, cx);
                    }
                    "left" => this.adjust_frame_crop([-step, 0.], 1., cx),
                    "right" => this.adjust_frame_crop([step, 0.], 1., cx),
                    "up" => this.adjust_frame_crop([0., -step], 1., cx),
                    "down" => this.adjust_frame_crop([0., step], 1., cx),
                    "+" | "=" => this.adjust_frame_crop([0.; 2], 1.1, cx),
                    "-" => this.adjust_frame_crop([0.; 2], 1. / 1.1, cx),
                    "tab" => return,
                    _ => {}
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .bg(p.panel)
                    .child(
                        div().flex_1().min_w_0().child(
                            if self
                                .design_ui
                                .frame_crop
                                .as_ref()
                                .is_some_and(|crop| crop.commit_label.is_some())
                            {
                                t!("design.background.crop_hint").to_string()
                            } else {
                                t!("editor.design_asset_ui.crop_hint").to_string()
                            },
                        ),
                    )
                    .child(
                        Button::new("frame-crop-zoom-out")
                            .label("−")
                            .accessibility_label(t!("editor.design_asset_ui.zoom_out"))
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.adjust_frame_crop([0.; 2], 1. / 1.1, cx)
                            })),
                    )
                    .child(
                        Button::new("frame-crop-zoom-in")
                            .label("+")
                            .accessibility_label(t!("editor.design_asset_ui.zoom_in"))
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.adjust_frame_crop([0.; 2], 1.1, cx)
                            })),
                    )
                    .child(
                        Button::new("frame-crop-cancel")
                            .label(t!("shell.cancel"))
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel_frame_crop(cx);
                                window.focus(&this.canvas_focus, cx);
                            })),
                    )
                    .child(
                        Button::new("frame-crop-done")
                            .label(t!("design.background.done"))
                            .small()
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.finish_frame_crop(cx);
                                window.focus(&this.canvas_focus, cx);
                            })),
                    ),
            )
            .child(self.canvas_region())
            .child(
                div()
                    .p_2()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(t!("editor.design_asset_ui.crop_keys_hint").to_string()),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::{
        design::{Element, frame, place_in_frame},
        project::{ProjectEditor, ProjectKind},
    };
    use gpui_kit::test::TestWindowExt;

    fn fixture() -> (Document, NodeId, NodeId) {
        let mut editor = Editor::new(Document::new(600, 400), None);
        let group = frame(&editor.doc, Element::Rectangle)
            .paste(&mut editor, Slot::TOP, (0., 0.))
            .unwrap()[0];
        let image = place_in_frame(
            &mut editor,
            group,
            Arc::new(Raster::solid(400, 100, [0.2, 0.3, 0.4, 1.])),
        )
        .unwrap();
        (editor.doc, group, image)
    }

    #[gpui_kit::test]
    fn framed_image_selection_has_one_visual_crop_and_replace_pair(cx: &mut TestAppContext) {
        let (authored, group, image) = fixture();
        let boundary = emulsion_core::design::frame_parts(&authored, group)
            .unwrap()
            .0;
        let (workspace, cx) = crate::tests::open(cx, authored.clone());
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, authored.clone()).unwrap(),
                    "Frame selection".into(),
                    window,
                    cx,
                )
            });
            workspace.read(cx).editor.clone().unwrap()
        });
        for viewport in [size(px(1200.), px(800.)), size(px(610.), px(510.))] {
            cx.simulate_resize(viewport);
            for selected in [group, boundary, image] {
                cx.update(|_, cx| {
                    view.update(cx, |v, cx| {
                        v.set_layer_selection(vec![selected], Some(selected));
                        cx.notify();
                    })
                });
                cx.run_until_parked();
                cx.update(|window, cx| {
                    assert!(window.try_find("design-image-crop").is_none());
                    assert!(window.try_find("design-image-replace").is_none());
                    assert!(
                        window.find("design-selection-crop-frame").visible(),
                        "crop hidden for node {selected} at {viewport:?}"
                    );
                    assert!(
                        window.find("design-selection-replace-frame").visible(),
                        "replace hidden for node {selected} at {viewport:?}"
                    );
                    window.click("design-selection-crop-frame", cx);
                });
                cx.run_until_parked();
                cx.update(|window, cx| {
                    assert!(window.find("design-frame-crop-editor").visible());
                    view.update(cx, |v, cx| {
                        v.adjust_frame_crop([0.; 2], 1.5, cx);
                        assert_ne!(v.design_ui.frame_crop.as_ref().unwrap().preview, authored);
                        assert_eq!(v.editor.doc, authored);
                    });
                    window.click("frame-crop-cancel", cx);
                });
                cx.run_until_parked();
                cx.update(|_, cx| {
                    assert!(!view.read(cx).frame_crop_active());
                    assert_eq!(view.read(cx).editor.doc, authored);
                });
            }
        }
        // A deep-picked image still commits a visual crop as one undoable edit,
        // and the advanced source actions remain in More rather than duplicates.
        cx.update(|window, cx| window.click("design-selection-crop-frame", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |v, cx| v.adjust_frame_crop([0.; 2], 1.5, cx));
            window.click("frame-crop-done", cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                assert_ne!(v.editor.doc, authored);
                assert_eq!(v.editor.history.len(), 1);
                v.undo(cx);
                assert_eq!(v.editor.doc, authored);
            })
        });
    }

    #[test]
    fn frame_assets_use_full_source_pixels_and_native_vector_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let png = directory.path().join("full.png");
        image::RgbaImage::from_pixel(480, 320, image::Rgba([40, 80, 120, 255]))
            .save(&png)
            .unwrap();
        let (raster, rendered) = frame_asset_raster(&png).unwrap();
        assert_eq!((raster.width(), raster.height()), (480, 320));
        assert!(!rendered);
        assert_eq!(raster.to_srgba8()[..4], [40, 80, 120, 255]);
        let digest = emulsion_io::raw::source_digest(&png).unwrap();
        emulsion_io::raw_settings::save_photo_settings(
            &png,
            &digest,
            emulsion_core::raw::DevelopParams {
                exposure: 1.,
                ..Default::default()
            },
        )
        .unwrap();
        let expected = emulsion_io::open_full(&png).unwrap().doc;
        let (developed, rendered) = frame_asset_raster(&png).unwrap();
        assert!(!rendered);
        assert_ne!(developed.to_srgba8(), raster.to_srgba8());
        assert_eq!(
            developed.to_srgba16(),
            emulsion_raster::composite::flatten(&expected.composite_tree(), 0).to_srgba16()
        );
        let svg = directory.path().join("vector.svg");
        std::fs::write(&svg, r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24"><rect width="32" height="24" fill="red"/></svg>"#).unwrap();
        let (raster, rendered) = frame_asset_raster(&svg).unwrap();
        assert_eq!((raster.width(), raster.height()), (32, 24));
        assert!(rendered);
        assert_eq!(raster.to_srgba8()[..4], [255, 0, 0, 255]);
    }

    #[gpui_kit::test]
    fn frame_crop_preview_cancel_commit_undo_and_page_change(cx: &mut TestAppContext) {
        let (authored, group, image) = fixture();
        let (workspace, cx) = crate::tests::open(cx, authored.clone());
        cx.simulate_resize(size(px(900.), px(700.)));
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, authored.clone()).unwrap(),
                    "Crop".into(),
                    window,
                    cx,
                )
            });
            workspace.read(cx).editor.clone().unwrap()
        });
        let stamp = cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.set_layer_selection(vec![group], Some(group));
                let stamp = v.editor.stamp();
                v.start_frame_crop(window, cx);
                v.adjust_frame_crop([0.; 2], 2., cx);
                v.adjust_frame_crop([12., -8.], 1., cx);
                assert_eq!(v.editor.doc, authored);
                assert_eq!(v.editor.stamp(), stamp);
                assert_ne!(v.design_ui.frame_crop.as_ref().unwrap().preview, authored);
                stamp
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| assert!(window.find("design-frame-crop-editor").visible()));
        cx.simulate_keystrokes("ctrl-j ctrl-z delete backspace ctrl-v right =");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let v = view.read(cx);
            assert!(v.frame_crop_active());
            assert_eq!(v.editor.doc, authored);
            assert_eq!(v.editor.stamp(), stamp);
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                assert!(!v.frame_crop_active());
                assert_eq!(v.editor.doc, authored);
                assert_eq!(v.editor.stamp(), stamp);
                v.start_frame_crop(window, cx);
                v.adjust_frame_crop([0.; 2], 2., cx);
                v.adjust_frame_crop([10., 0.], 1., cx);
                let preview = v.design_ui.frame_crop.as_ref().unwrap().preview.clone();
                v.finish_frame_crop(cx);
                assert_eq!(v.editor.doc, preview);
                assert_eq!(v.editor.history.len(), 1);
                let NodeKind::Raster { raster: before, .. } = &authored.node(image).unwrap().kind
                else {
                    unreachable!()
                };
                let NodeKind::Raster { raster: after, .. } =
                    &v.editor.doc.node(image).unwrap().kind
                else {
                    unreachable!()
                };
                assert!(Arc::ptr_eq(before, after));
                v.undo(cx);
                assert_eq!(v.editor.doc, authored);
                v.start_frame_crop(window, cx);
                v.adjust_frame_crop([0.; 2], 1.5, cx);
                v.add_project_page(false, cx);
                assert!(!v.frame_crop_active());
                let next_page = v.editor.active_page();
                v.finish_frame_crop(cx); // A late Done cannot target the new page.
                assert_eq!(v.editor.active_page(), next_page);
            })
        });
    }

    #[gpui_kit::test]
    fn asset_load_tickets_reject_repeated_cancelled_and_changed_pages(cx: &mut TestAppContext) {
        let (doc, _, _) = fixture();
        let view = cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            cx.set_global(crate::app_state::AppSettings(Default::default()));
            cx.new(|cx| EditorView::new(doc, None, None, None, "Assets".into(), cx))
        });
        view.update(cx, |v, cx| {
            let page = v.editor.active_page();
            let old = v.begin_design_asset_request();
            let new = v.begin_design_asset_request();
            assert!(!v.accept_design_asset_result(old, page, cx));
            assert_eq!(v.design_ui.asset_job, Some(new));
            assert_eq!(v.pending_edit_job, Some(new));
            assert!(v.accept_design_asset_result(new, page, cx));
            let cancelled = v.begin_design_asset_request();
            assert!(v.cancel_design_asset_load(cx));
            assert!(!v.accept_design_asset_result(cancelled, page, cx));
            assert!(v.pending_edit_job.is_none());
            v.begin_design_asset_request();
            let unrelated = v.begin_edit_job();
            assert!(v.cancel_design_asset_load(cx));
            assert_eq!(v.edit_ticket(), unrelated);
            assert_eq!(v.pending_edit_job, Some(unrelated));
            assert!(v.accept_edit_result(unrelated, "New operation", cx));
            let stale = v.begin_design_asset_request();
            v.after_change(cx);
            assert!(!v.accept_design_asset_result(stale, page, cx));
            assert!(v.pending_edit_job.is_none());
        });
    }
    #[gpui_kit::test]
    fn asset_click_replaces_blank_drop_inserts_and_locked_drop_is_rejected(
        cx: &mut TestAppContext,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("replacement.png");
        image::RgbaImage::from_pixel(48, 32, image::Rgba([120, 40, 80, 255]))
            .save(&path)
            .unwrap();
        let (authored, group, image) = fixture();
        let count = authored.nodes.len();
        let (workspace, cx) = crate::tests::open(cx, authored.clone());
        cx.simulate_resize(size(px(1200.), px(800.)));
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, authored).unwrap(),
                    "Asset drops".into(),
                    window,
                    cx,
                )
            });
            workspace.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                v.execute(
                    Command::SetLocked {
                        id: group,
                        locked: true,
                    },
                    cx,
                );
                let before = v.editor.doc.clone();
                let canvas = v.canvas_bounds().unwrap();
                let center = v.view.doc_to_screen((300., 200.), &canvas);
                v.drop_design_asset(
                    path.clone(),
                    point(px(center.0 as f32), px(center.1 as f32)),
                    cx,
                );
                assert!(v.design_ui.asset_job.is_none());
                assert_eq!(v.editor.doc, before);
                v.execute(
                    Command::SetLocked {
                        id: group,
                        locked: false,
                    },
                    cx,
                );
                v.set_layer_selection(vec![group], Some(group));
                v.place_design_asset(path.clone(), cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                assert_eq!(v.editor.doc.nodes.len(), count);
                let NodeKind::Raster { raster, .. } = &v.editor.doc.node(image).unwrap().kind
                else {
                    unreachable!()
                };
                assert_eq!((raster.width(), raster.height()), (48, 32));
                let canvas = v.canvas_bounds().unwrap();
                let blank = v.view.doc_to_screen((25., 25.), &canvas);
                v.drop_design_asset(
                    path.clone(),
                    point(px(blank.0 as f32), px(blank.1 as f32)),
                    cx,
                );
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(view.read(cx).editor.doc.nodes.len(), count + 1);
        });
    }

    #[gpui_kit::test]
    fn escape_cancels_asset_loading_while_library_search_has_focus(cx: &mut TestAppContext) {
        let (authored, _, _) = fixture();
        let (workspace, cx) = crate::tests::open(cx, authored.clone());
        cx.simulate_resize(size(px(1200.), px(800.)));
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, authored).unwrap(),
                    "Cancel assets".into(),
                    window,
                    cx,
                )
            });
            let view = workspace.read(cx).editor.clone().unwrap();
            view.update(cx, |v, cx| {
                v.show_design_section(super::super::design_ui::Section::Uploads, cx)
            });
            view
        });
        cx.run_until_parked();
        let (ticket, page) = cx.update(|window, cx| {
            window.click("design-library-search", cx);
            view.update(cx, |v, cx| {
                let ticket = v.begin_design_asset_request();
                cx.notify();
                (ticket, v.editor.active_page())
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| assert!(window.find("design-cancel-asset-load").visible()));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                assert!(v.design_ui.asset_job.is_none());
                assert!(!v.accept_design_asset_result(ticket, page, cx));
            })
        });
    }
}
