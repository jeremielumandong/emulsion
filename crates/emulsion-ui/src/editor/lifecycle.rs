//! Presentation resources belong to the visible document, not every open tab.
use super::*;

impl EditorView {
    pub(crate) fn finish_pointer_gesture(&mut self, cx: &mut Context<Self>) {
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
            // Finish a pointer gesture before its mouse-up dispatch disappears.
            // This also stops the quick-shape polling loop for an active stroke.
            self.finish_pointer_gesture(cx);
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
