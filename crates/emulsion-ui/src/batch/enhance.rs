//! Local model inference. Enhanced files are derivatives; originals remain linked.
use super::*;
use gpui_kit::component::Disableable;
impl Workspace {
    pub(super) fn library_enhance_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = classic::palette(cx);
        let mut panel = div().flex().flex_col().gap_2();
        let params = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .and_then(|i| self.batch.develop.current_params(&i.path));
        panel = panel.child(
            Checkbox::new("library-ai-sensor-denoise")
                .label(SharedString::from(t!("library.enhance.sensor_denoise")))
                .checked(params.is_some_and(|p| p.sensor_ai_denoise))
                .on_change(cx.listener(|this, value, _, cx| {
                    if let Some(mut p) = this
                        .batch
                        .current
                        .and_then(|i| this.batch.items.get(i))
                        .and_then(|i| this.batch.develop.current_params(&i.path))
                    {
                        p.sensor_ai_denoise = *value;
                        this.library_adjust(p, cx);
                    }
                })),
        );
        let raw = self
            .batch
            .develop
            .source
            .as_ref()
            .is_some_and(|s| emulsion_io::photo_develop::is_raw_photo(&s.source));
        panel = panel.child(
            Checkbox::new("library-cancellable-demosaic")
                .label(SharedString::from(t!("library.enhance.interruptible")))
                .checked(params.is_some_and(|p| p.demosaic_version == 1))
                .disabled(!raw)
                .on_change(cx.listener(|this, value, _, cx| {
                    if let Some(mut params) = this
                        .batch
                        .current
                        .and_then(|i| this.batch.items.get(i))
                        .and_then(|i| this.batch.develop.current_params(&i.path))
                    {
                        params.demosaic_version = u8::from(*value);
                        this.library_adjust(params, cx);
                    }
                })),
        );
        panel = panel.child(
            Checkbox::new("library-as-shot-profile")
                .label(SharedString::from(t!("library.enhance.as_shot")))
                .checked(params.is_some_and(|p| p.profile_as_shot))
                .disabled(!raw || params.is_none_or(|p| p.camera_profile.is_none()))
                .on_change(cx.listener(|this, value, _, cx| {
                    if let Some(mut params) = this
                        .batch
                        .current
                        .and_then(|i| this.batch.items.get(i))
                        .and_then(|i| this.batch.develop.current_params(&i.path))
                    {
                        params.profile_as_shot = *value;
                        this.library_adjust(params, cx);
                    }
                })),
        );
        for (action, title) in [
            (0, "library.enhance.subject_mask"),
            (1, "library.enhance.denoise"),
            (2, "library.enhance.super_resolution"),
            (4, "library.enhance.sky_auto"),
            (5, "library.enhance.perspective"),
            (6, "library.enhance.depth_map"),
        ] {
            panel = panel.child(
                Button::new(("library-ai", action))
                    .label(t!(title))
                    .small()
                    .outline()
                    .disabled(self.batch.develop.ai_job.is_some())
                    .on_click(cx.listener(move |this, _, _, cx| this.library_enhance(action, cx))),
            );
        }
        panel = panel.child(
            Button::new("library-ai-sky")
                .label(t!("library.enhance.sky_pick"))
                .small()
                .outline()
                .disabled(self.batch.develop.ai_job.is_some())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.batch.develop.picking_sky = true;
                    this.batch.develop.before = true;
                    this.batch.develop.compare = false;
                    this.batch.develop.loupe = true;
                    this.batch.navigation.borrow_mut().fit();
                    this.invalidate_library_preview();
                    this.batch.note = Some((t!("library.enhance.sky_hint").into(), false));
                    cx.notify();
                })),
        );
        panel = panel.child(mono(t!("library.enhance.about"), 10., p.muted));
        if let Some(job) = &self.batch.develop.ai_job {
            panel = panel.child(mono(job.summary(), 11., p.muted)).child(
                Button::new("library-ai-cancel")
                    .label(t!("library.enhance.cancel"))
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(job) = &this.batch.develop.ai_job {
                            job.cancel();
                        }
                        cx.notify();
                    })),
            );
        }
        if let Some(params) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .and_then(|i| self.batch.develop.current_params(&i.path))
            .filter(|p| p.depth_map.is_some())
        {
            for (index, title, value, max) in [
                (0, "library.enhance.depth_blur", params.depth_blur, 0.05),
                (1, "library.enhance.focus_depth", params.depth_focus, 1.),
                (2, "library.enhance.focus_range", params.depth_range, 1.),
            ] {
                panel = panel.child(self.library_numeric_control(
                    1100 + index,
                    &t!(title),
                    super::advanced::Field::Depth(index),
                    value,
                    0.,
                    max,
                    0.01,
                    cx,
                ));
            }
        }
        panel.into_any_element()
    }
    pub(super) fn library_enhance(&mut self, action: usize, cx: &mut Context<Self>) {
        if self.batch.develop.ai_job.is_some() {
            return;
        }
        let Some(path) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone())
        else {
            return;
        };
        let Some(params) = self.batch.develop.current_params(&path) else {
            return;
        };
        let Some(source) = self
            .batch
            .develop
            .source
            .clone()
            .filter(|s| s.source == path)
        else {
            return;
        };
        let seed = self.batch.develop.mask_seed;
        let slot = self.batch.develop.mask.min(7);
        let job = emulsion_ai::jobs::Job::new();
        self.batch.develop.ai_job = Some(job.clone());
        self.batch.note = None;
        let progress = job.clone();
        cx.spawn(async move |this, cx| {
            let file = path.clone();
            let task = job.clone();
            let result = cx
                .background_spawn(async move {
                    (|| -> Result<_, String> {
                        if matches!(action, 0 | 3 | 4 | 5 | 6) {
                            let mut mask_params = params;
                            mask_params.crop = [0., 0., 1., 1.];
                            mask_params.straighten = 0.;
                            mask_params.perspective = [0.; 2];
                            mask_params.distortion = 0.;
                            mask_params.lens_profile = None;
                            mask_params.aberration = [0.; 2];
                            mask_params.masks = Default::default();
                            mask_params.rotation = 0;
                            mask_params.depth_blur = 0.;
                            let raster = source
                                .develop_with(&mask_params)
                                .map_err(|e| e.to_string())?;
                            let (w, h, pixels) = super::develop::display_raster(&raster)
                                .ok_or_else(|| t!("library.enhance.err_subject").into_owned())?;
                            let image = Raster::from_srgba8(w, h, &pixels);
                            if action == 5 {
                                task.check().map_err(|e| e.to_string())?;
                                let next = emulsion_io::photo_geometry::automatic(&image, params)
                                    .map_err(|e| e.to_string())?;
                                task.check().map_err(|e| e.to_string())?;
                                return Ok((Some(next), None));
                            }
                            if action == 6 {
                                let map = emulsion_ai::depth::estimate(&image, &task)
                                    .map_err(|e| e.to_string())?;
                                let digest = emulsion_io::photo_develop::save_mask(&map.to_mask())
                                    .map_err(|e| e.to_string())?;
                                let next = emulsion_core::raw::DevelopParams {
                                    depth_map: Some(digest),
                                    depth_blur: 0.01,
                                    ..params
                                };
                                return Ok((Some(next), None));
                            }
                            let mask = if action == 4 {
                                emulsion_ai::sky::mask(&image, &task).map_err(|e| e.to_string())?
                            } else if action == 3 {
                                let [x, y] = seed.ok_or_else(|| {
                                    t!("library.enhance.err_pick_sky").into_owned()
                                })?;
                                let embedding = emulsion_ai::sam::encode(&image, &task)
                                    .map_err(|e| e.to_string())?;
                                emulsion_ai::sam::decode(
                                    &embedding,
                                    &[emulsion_ai::sam::Point {
                                        x: x * w as f32,
                                        y: y * h as f32,
                                        positive: true,
                                    }],
                                    None,
                                )
                                .map_err(|e| e.to_string())?
                                .0
                            } else {
                                emulsion_ai::matte::matte(&image, &Default::default(), &task)
                                    .map_err(|e| e.to_string())?
                            };
                            task.check().map_err(|e| e.to_string())?;
                            let digest = emulsion_io::photo_develop::save_mask(&mask)
                                .map_err(|e| e.to_string())?;
                            let mut next = params;
                            next.masks[slot] = emulsion_core::raw::LocalAdjustment {
                                enabled: true,
                                bitmap: Some(digest),
                                ..Default::default()
                            };
                            return Ok((Some(next), None));
                        }
                        let raster = source.develop_with(&params).map_err(|e| e.to_string())?;
                        let raster = if action == 1 {
                            emulsion_ai::upscale::denoise(&raster, &task)
                        } else {
                            emulsion_ai::upscale::upscale(&raster, &task)
                        }
                        .map_err(|e| e.to_string())?;
                        task.check().map_err(|e| e.to_string())?;
                        let mut doc = Document::new(raster.width(), raster.height());
                        doc.source_depth = 16;
                        doc.nodes.push(Node::raster(
                            1,
                            "Enhanced",
                            Arc::new(raster),
                            Default::default(),
                        ));
                        doc.next_id = 2;
                        let dir = emulsion_io::creative_library::root().join("enhanced");
                        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                        let stage = BatchStage::new(&dir, "png").map_err(|e| e.to_string())?;
                        emulsion_io::export::export(&doc, &stage.0, ExportOptions::for_doc(&doc))
                            .map_err(|e| e.to_string())?;
                        task.check().map_err(|e| e.to_string())?;
                        let stem = format!(
                            "{}-{}",
                            file.file_stem().unwrap_or_default().to_string_lossy(),
                            if action == 1 { "restored" } else { "enhanced" }
                        );
                        let out = publish_batch_file(&stage.0, &dir, &stem, "png")
                            .map_err(|e| e.to_string())?;
                        let (catalog, _) = emulsion_io::creative_library::update(
                            &emulsion_io::creative_library::root(),
                            |c| {
                                c.add_asset(out, emulsion_io::creative_library::AssetKind::Image)?;
                                Ok(())
                            },
                        )
                        .map_err(|e| e.to_string())?;
                        Ok((None, Some(catalog)))
                    })()
                })
                .await;
            job.finish();
            this.update(cx, |this, cx| {
                this.batch.develop.ai_job = None;
                match result {
                    Ok((Some(next), _)) => {
                        if this
                            .batch
                            .current
                            .and_then(|i| this.batch.items.get(i))
                            .map(|i| &i.path)
                            == Some(&path)
                            && this.batch.develop.current_params(&path) == Some(params)
                        {
                            this.library_adjust(next, cx);
                            this.batch.develop.section = if action == 5 { 1 } else { 5 };
                            this.batch.develop.slider_key = None;
                        } else {
                            this.batch.note =
                                Some((t!("library.enhance.photo_changed").into(), true));
                        }
                    }
                    Ok((_, Some(catalog))) => {
                        this.batch.library.catalog = catalog;
                        this.batch.library.loaded = true;
                        this.batch.note = Some((t!("library.enhance.saved_catalog").into(), false));
                    }
                    Err(e) => this.batch.note = Some((e.into(), true)),
                    _ => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.spawn(async move |this, cx| {
            while !progress.is_finished() {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(150))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    progress.cancel();
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }
}
