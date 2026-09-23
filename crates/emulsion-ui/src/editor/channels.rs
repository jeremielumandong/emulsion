//! Per-document channel inspection. Display changes never alter saved pixels.
use super::*;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) enum ChannelView {
    #[default]
    Rgb,
    Red,
    Green,
    Blue,
}

impl ChannelView {
    pub(crate) fn apply(self, bgra: &mut [u8]) {
        let index = match self {
            Self::Rgb => return,
            Self::Red => 2,
            Self::Green => 1,
            Self::Blue => 0,
        };
        for pixel in bgra.as_chunks_mut::<4>().0 {
            let value = pixel[index];
            pixel[..3].fill(value);
        }
    }
}

#[derive(Default)]
pub(crate) struct ChannelState {
    pub view: ChannelView,
    thumbs: Option<(Arc<CompositeTree>, Vec<Arc<RenderImage>>)>,
}

impl ChannelState {
    pub(super) fn release_images(&mut self, window: &mut Window) {
        if let Some((_, images)) = self.thumbs.take() {
            for image in images {
                let _ = window.drop_image(image);
            }
        }
    }
}

impl EditorView {
    pub(crate) fn select_channel(&mut self, channel: ChannelView, cx: &mut Context<Self>) {
        if self.channels.view == channel {
            return;
        }
        self.channels.view = channel;
        self.cache.borrow_mut().clear();
        self.gen_counter += 1;
        self.render_gen = self.gen_counter;
        self.gen_counter += 1;
        self.before_gen = self.gen_counter;
        cx.notify();
    }

    pub(super) fn channels_panel(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        const CHANNELS: [(ChannelView, &str, &str); 4] = [
            (ChannelView::Rgb, "RGB", "channel-rgb"),
            (ChannelView::Red, "Red", "channel-red"),
            (ChannelView::Green, "Green", "channel-green"),
            (ChannelView::Blue, "Blue", "channel-blue"),
        ];
        if !self
            .channels
            .thumbs
            .as_ref()
            .is_some_and(|(tree, _)| Arc::ptr_eq(tree, &self.tree))
        {
            let mut level = 0;
            while level_size(self.tree.width, self.tree.height, level)
                .0
                .max(level_size(self.tree.width, self.tree.height, level).1)
                > 80
                && level < 16
            {
                level += 1;
            }
            let raster = emulsion_raster::composite::flatten(&self.tree, level);
            let rgba =
                image::RgbaImage::from_raw(raster.width(), raster.height(), raster.to_srgba8())
                    .expect("sized raster");
            let thumbnail = image::imageops::thumbnail(&rgba, 40, 40);
            let (w, h) = thumbnail.dimensions();
            let mut bytes = thumbnail.into_raw();
            for pixel in bytes.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            let thumbs = CHANNELS
                .iter()
                .map(|(channel, _, _)| {
                    let mut bytes = bytes.clone();
                    channel.apply(&mut bytes);
                    Arc::new(viewport::bgra_image(w, h, bytes))
                })
                .collect();
            if let Some((_, old)) = self.channels.thumbs.replace((self.tree.clone(), thumbs)) {
                self.cache.borrow_mut().to_drop.extend(old);
            }
        }
        let thumbs = &self.channels.thumbs.as_ref().unwrap().1;
        div()
            .id("channels-panel")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap_1()
            .p_2()
            .children(
                CHANNELS
                    .into_iter()
                    .enumerate()
                    .map(|(i, (channel, name, id))| {
                        button(id, name, self.channels.view == channel, p)
                            .justify_start()
                            .gap_2()
                            .child(
                                img(ImageSource::Render(thumbs[i].clone()))
                                    .w(px(40.))
                                    .h(px(40.))
                                    .object_fit(ObjectFit::Contain),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_channel(channel, cx);
                                window.focus(&this.canvas_focus, cx);
                            }))
                            .test_support()
                    }),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child("Channel preview · RGB restores full color"),
            )
            .test_support()
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::ChannelView;

    #[test]
    fn channel_preview_uses_bgra_order_and_preserves_alpha() {
        for (channel, expected) in [
            (ChannelView::Red, 190),
            (ChannelView::Green, 80),
            (ChannelView::Blue, 20),
        ] {
            let mut bytes = [20, 80, 190, 128];
            channel.apply(&mut bytes);
            assert_eq!(bytes, [expected, expected, expected, 128]);
        }
        let mut bytes = [20, 80, 190, 128];
        ChannelView::Rgb.apply(&mut bytes);
        assert_eq!(bytes, [20, 80, 190, 128]);
    }
}
