//! Selection comparison and survey share normalized zoom and pan.
use super::*;
impl Workspace {
    fn library_prepare_culling(&mut self, cx: &mut Context<Self>) {
        let key: Vec<_> = self
            .batch
            .items
            .iter()
            .filter(|i| i.selected)
            .take(if self.batch.develop.culling_mode == 2 {
                8
            } else {
                2
            })
            .map(|i| (i.path.clone(), self.batch.develop.current_params(&i.path)))
            .collect();
        if self.batch.develop.culling_loading || self.batch.develop.busy || key == self.batch.develop.culling_key {
            return;
        }
        self.batch.develop.culling_loading = true;
        self.batch.develop.source = None;
        self.batch
            .develop
            .culling_images
            .retain(|path, _| key.iter().any(|(p, _)| p == path));
        cx.spawn(async move |this, cx| {
            let work = key.clone();
            let results = cx
                .background_spawn(async move {
                    work.into_iter()
                        .map(|(path, params)| {
                            let result = (|| -> emulsion_io::Result<_> {
                                let source = emulsion_io::photo_develop::PhotoSource::load(&path)?;
                                let p = match params {
                                    Some(p) => p,
                                    None => emulsion_io::raw_settings::adjacent_settings(
                                        &path,
                                        &source.source_sha256,
                                    )?,
                                };
                                let raster = source.develop_preview(
                                    &p,
                                    &std::sync::atomic::AtomicBool::new(false),
                                )?;
                                let (w, h, mut pixels) = super::develop::display_raster(&raster)
                                    .ok_or_else(|| {
                                        emulsion_io::IoError::Unsupported(
                                            "Comparison preview".into(),
                                        )
                                    })?;
                                for p in pixels.chunks_exact_mut(4) {
                                    p.swap(0, 2);
                                }
                                Ok((w, h, pixels))
                            })();
                            (path, result)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                this.batch.develop.culling_loading = false;
                this.batch.develop.culling_key = key;
                for (path, result) in results {
                    match result {
                        Ok((w, h, pixels)) => {
                            this.batch
                                .develop
                                .culling_images
                                .insert(path, Arc::new(bgra_image(w, h, pixels)));
                        }
                        Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn library_culling_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.library_prepare_culling(cx);
        let p = classic::palette(cx);
        let survey = self.batch.develop.culling_mode == 2;
        let selected: Vec<_> = self
            .batch
            .items
            .iter()
            .filter(|i| i.selected)
            .take(if survey { 8 } else { 2 })
            .collect();
        if selected.len() < 2 {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    "Select at least two photos using Ctrl-click or Shift-click in the filmstrip.",
                )
                .into_any_element();
        }
        let zoom = self.batch.develop.culling_zoom.max(1.);
        let center = self.batch.develop.culling_center.unwrap_or([0.5, 0.5]);
        let mut view = div()
            .id("library-culling-view")
            .test_support()
            .flex()
            .flex_col()
            .size_full()
            .gap_2();
        let columns = if survey { selected.len().min(4) } else { 2 };
        for (row_index, row) in selected.chunks(columns).enumerate() {
            let mut line = div().flex().flex_1().min_h_0().gap_2();
            for (index, item) in row.iter().enumerate() {
                let id = row_index * columns + index;
                let image = self
                    .batch
                    .develop
                    .culling_images
                    .get(&item.path)
                    .cloned()
                    .or_else(|| item.thumb.clone());
                let name = item
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                line = line.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .bg(p.stage)
                        .child(mono(name, 11., p.ink))
                        .child(
                            div()
                                .id(("library-compare-photo", id))
                                .test_support()
                                .flex_1()
                                .min_h_0()
                                .overflow_hidden()
                                .on_scroll_wheel(cx.listener(
                                    |this, e: &ScrollWheelEvent, _, cx| {
                                        let delta = e.delta.pixel_delta(px(20.));
                                        if e.modifiers.control {
                                            this.batch.develop.culling_zoom =
                                                (this.batch.develop.culling_zoom.max(1.)
                                                    * (f32::from(delta.y) * 0.004).exp())
                                                .clamp(1., 8.);
                                        } else {
                                            let mut center = this
                                                .batch
                                                .develop
                                                .culling_center
                                                .unwrap_or([0.5, 0.5]);
                                            center[0] = (center[0] - f32::from(delta.x) / 1000.)
                                                .clamp(0., 1.);
                                            center[1] = (center[1] - f32::from(delta.y) / 1000.)
                                                .clamp(0., 1.);
                                            this.batch.develop.culling_center = Some(center);
                                        }
                                        cx.stop_propagation();
                                        cx.notify();
                                    },
                                ))
                                .child(
                                    canvas(
                                        |_, _, _| {},
                                        move |bounds, _, window, _| {
                                            if let Some(image) = &image {
                                                let dimensions = image.size(0);
                                                let w = dimensions.width.0 as f32;
                                                let h = dimensions.height.0 as f32;
                                                let scale = (f32::from(bounds.size.width) / w)
                                                    .min(f32::from(bounds.size.height) / h)
                                                    * zoom;
                                                let origin = bounds.center()
                                                    - point(
                                                        px(w * center[0] * scale),
                                                        px(h * center[1] * scale),
                                                    );
                                                let rect = Bounds::new(
                                                    origin,
                                                    size(px(w * scale), px(h * scale)),
                                                );
                                                let _ = window.paint_image(
                                                    rect,
                                                    rect,
                                                    Corners::default(),
                                                    image.clone(),
                                                    0,
                                                    false,
                                                );
                                            }
                                        },
                                    )
                                    .size_full(),
                                ),
                        ),
                );
            }
            view = view.child(line);
        }
        view.child(Button::new("library-compare-retry").label("Refresh previews").small().ghost().on_click(cx.listener(|this,_,_,cx|{this.batch.develop.culling_key.clear();cx.notify();}))).child(mono(
            if self.batch.develop.culling_loading {
                "Refining previews… · Ctrl-scroll linked zoom · Scroll linked pan"
            } else {
                "Developed previews · Ctrl-scroll linked zoom · Scroll linked pan"
            },
            10.,
            p.muted,
        ))
        .into_any_element()
    }
}
