//! Local model inference. Enhanced files are derivatives; originals remain linked.
use super::*;
use gpui_kit::component::Disableable;
impl Workspace {
    pub(super) fn library_enhance_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = classic::palette(cx);
        let mut panel = div().flex().flex_col().gap_2();
        for (action, title) in [
            (0, "AI subject mask"),
            (1, "AI denoise / restore"),
            (2, "Super resolution"),
            (4, "Automatic sky mask"),
            (5, "Automatic perspective"),
        ] {
            panel = panel.child(
                Button::new(("library-ai", action))
                    .label(title)
                    .small()
                    .outline()
                    .disabled(self.batch.develop.ai_job.is_some())
                    .on_click(cx.listener(move |this, _, _, cx| this.library_enhance(action, cx))),
            );
        }
        panel = panel.child(
            Button::new("library-ai-sky")
                .label("Sky mask · pick sky…")
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
                    this.batch.note = Some((
                        "Click inside the sky in the original preview to guide the AI mask.".into(),
                        false,
                    ));
                    cx.notify();
                })),
        );
        panel=panel.child(mono("Denoise restores developed RGB with Real-ESRGAN. Enhanced images are saved as new 16-bit PNG files.",10.,p.muted));
        if let Some(job) = &self.batch.develop.ai_job {
            panel = panel.child(mono(job.summary(), 11., p.muted)).child(
                Button::new("library-ai-cancel")
                    .label("Cancel enhancement")
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
                        if matches!(action,0|3|4|5) {
                            let mut mask_params = params;
                            mask_params.crop = [0., 0., 1., 1.];
                            mask_params.straighten = 0.;
                            mask_params.perspective = [0.; 2];
                            mask_params.distortion = 0.;
                            mask_params.lens_profile = None;
                            mask_params.aberration = [0.; 2];
                            mask_params.masks = Default::default();
                            let raster = source
                                .develop_with(&mask_params)
                                .map_err(|e| e.to_string())?;
                            let (w, h, pixels) = super::develop::display_raster(&raster)
                                .ok_or("Could not build subject input")?;
                            let image = Raster::from_srgba8(w, h, &pixels);
                            if action==5 {task.check().map_err(|e|e.to_string())?;let next=emulsion_io::photo_geometry::automatic(&image,params).map_err(|e|e.to_string())?;task.check().map_err(|e|e.to_string())?;return Ok((Some(next),None));}
                            let mask = if action==4 {emulsion_ai::sky::mask(&image,&task).map_err(|e|e.to_string())?} else if action == 3 {
                                let [x, y] = seed.ok_or("Pick a point inside the sky first")?;
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
                        let (catalog,_)=emulsion_io::creative_library::update(&emulsion_io::creative_library::root(),|c|{c.add_asset(out,emulsion_io::creative_library::AssetKind::Image)?;Ok(())}).map_err(|e|e.to_string())?;
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
                            this.batch.note = Some((
                                "Photo changed during analysis; adjustment was not applied.".into(),
                                true,
                            ));
                        }
                    }
                    Ok((_, Some(catalog))) => {
                        this.batch.library.catalog=catalog;
                        this.batch.library.loaded=true;
                        this.batch.note=Some(("Enhanced photo saved to the local catalog. Choose All photos to view it.".into(),false));
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
