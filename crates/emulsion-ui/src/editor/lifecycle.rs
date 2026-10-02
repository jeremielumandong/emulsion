//! Presentation resources belong to the visible document, not every open tab.
use super::*;

impl EditorView {
    pub(super) fn pointer_moved(
        &mut self,
        event: &MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        // A cached frame can still dispatch after its tab has been hidden.
        if !self.visible {
            return;
        }
        if self.frame_crop_pointer_moved(event, cx) {
            return;
        }
        if event.pressed_button.is_none() {
            // Mouse-up can be consumed by chrome or lost outside the window.
            // Finish at the last pressed position, before processing this hover.
            // Preserve click-to-connect previews, which intentionally follow hover.
            if self.diagram_cancel_pointer_gesture() {
                cx.notify();
            }
            self.drag_end(cx);
        }
        self.snap_bypass = event.modifiers.control;
        self.drag_shift = event.modifiers.shift;
        self.drag_move(event.position, window, cx);
    }

    pub(super) fn pointer_released(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if !self.visible || !matches!(event.button, MouseButton::Left | MouseButton::Middle) {
            return;
        }
        if self.frame_crop_active() {
            if let Some(crop) = &mut self.design_ui.frame_crop {
                crop.pointer = None;
            }
            return;
        }
        self.drag_shift = event.modifiers.shift;
        if event.button == MouseButton::Left {
            let point = self
                .canvas_bounds()
                .filter(|bounds| bounds.contains(&event.position))
                .and_then(|_| self.doc_point(event.position));
            self.diagram_pointer_up(point, cx);
        }
        self.drag_end(cx);
    }

    pub(crate) fn finish_pointer_gesture(&mut self, cx: &mut Context<Self>) {
        if let Some(crop) = &mut self.design_ui.frame_crop {
            crop.pointer = None;
        }
        self.diagram_cancel_connection();
        self.drag_end(cx);
    }

    /// Keep editing state and background document jobs; suspend presentation.
    pub(crate) fn set_visible(
        &mut self,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.visible == visible {
            return;
        }
        if !visible {
            self.cancel_frame_crop(cx);
            self.cancel_design_asset_load(cx);
            // Finish a pointer gesture before its mouse-up dispatch disappears.
            // This also stops the quick-shape polling loop for an active stroke.
            self.finish_pointer_gesture(cx);
            self.remember_storyboard_layout(cx);
        }
        self.visible = visible;
        if visible {
            self.ants_task = Some(Self::start_ants(cx));
            self.resume_rendering(cx);
            cx.notify();
            return;
        }
        self.render_epoch = self.render_epoch.wrapping_add(1);
        self.ants_task = None;
        if let Some(cancelled) = self.tile_cancel.take() {
            cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.tile_task = None;
        self.suspend_playback(window, cx);
        self.cache.borrow_mut().release(window);
        self.svg_canvas.borrow_mut().release(window);
        self.release_document_stencil_previews(window);
        self.release_creative_thumbnails(window);
        for (_, image) in self.thumbs.drain() {
            let _ = window.drop_image(image);
        }
        self.thumbs.shrink_to_fit();
        self.channels.release_images(window);
        self.quick_mask_cache.release(window);
        self.mask_view.cache.release(window);
        self.tools.remove.cache.release(window);
        self.tools.pointer = None;
        self.space_held = false;
    }

    /// A completed batch belongs to the visible lifetime that requested it.
    pub(crate) fn install_tile_batch(
        &mut self,
        epoch: u64,
        channel: channels::ChannelView,
        out: Vec<(viewport::Request, Vec<u8>)>,
        elapsed: std::time::Duration,
        cx: &mut Context<Self>,
    ) {
        if !self.visible || self.render_epoch != epoch {
            return;
        }
        let count = out.len();
        {
            let mut cache = self.cache.borrow_mut();
            for (request, bytes) in out {
                // Channel changes already clear pending tiles. Reject images
                // calculated for the previous channel just as before suspension.
                if self.channels.view != channel {
                    continue;
                }
                cache.insert(
                    request.key,
                    request.rev,
                    Arc::new(viewport::bgra_image(256, 256, bytes)),
                );
            }
            cache.in_flight = false;
            cache.last_batch = Some((count, elapsed));
        }
        self.notify_canvas(cx);
    }
}
