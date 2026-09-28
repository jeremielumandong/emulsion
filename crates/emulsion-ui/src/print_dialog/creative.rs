use super::*;

impl PrintDialog {
    pub(super) fn creative_draft(&self, settings: &mut Settings, cx: &App) -> anyhow::Result<()> {
        use anyhow::Context;
        let number = |i: usize, name: &str| -> anyhow::Result<f64> {
            self.fields[i]
                .read(cx)
                .value()
                .parse()
                .with_context(|| format!("Enter {name}"))
        };
        let c = &mut settings.creative;
        if settings.layout == Layout::Document {
            c.artwork_mm = None;
        } else if c.artwork_mm.is_some() {
            c.artwork_mm = Some([
                number(5, "artwork width in mm")?,
                number(6, "artwork height in mm")?,
            ]);
        }
        if matches!(settings.layout, Layout::Contact | Layout::Repeat) {
            c.rows = self.fields[7]
                .read(cx)
                .value()
                .parse()
                .context("Rows must be a whole number from 1 to 20")?;
            c.columns = self.fields[8]
                .read(cx)
                .value()
                .parse()
                .context("Columns must be a whole number from 1 to 20")?;
            c.gutter_mm = number(9, "gutter in mm")?;
        }
        if matches!(settings.layout, Layout::Document | Layout::Poster) {
            c.crop = [0.5, 0.5];
        } else {
            c.crop = [
                number(10, "horizontal crop position (0–100%)")? / 100.,
                number(11, "vertical crop position (0–100%)")? / 100.,
            ];
        }
        if settings.layout == Layout::Poster {
            c.bleed_mm = 0.;
            c.crop_marks = false;
        } else {
            c.bleed_mm = number(12, "bleed in mm")?;
        }
        Ok(())
    }
    pub(super) fn creative_controls(&self, cx: &Context<Self>) -> AnyElement {
        let mut controls = div().id("print-creative").flex().flex_col().gap_3();
        if self.settings.layout != Layout::Document {
            controls = controls.child(self.select(
                "print-artwork-mode",
                "Artwork size",
                self.settings.creative.artwork_mm.is_some().to_string(),
                vec![
                    ("false".into(), "Automatic / document size".into()),
                    ("true".into(), "Custom artwork box".into()),
                ],
                |s, v, cx| {
                    s.settings.creative.artwork_mm = (v == "true").then_some([101.6, 152.4]);
                    s.changed(cx);
                },
                cx,
            ));
            if self.settings.creative.artwork_mm.is_some() {
                controls = controls.child(self.field(5, "Artwork width (mm)")).child(self.field(6, "Artwork height (mm)"))
                    .child(div().text_color(theme::palette(cx).muted).child(if self.settings.layout == Layout::Poster {
                        "Poster fits within these dimensions, preserving proportions."
                    } else { "Fit keeps the entire image. Fill crops to this box. Image proportions stay unchanged." }));
            }
        }
        if matches!(self.settings.layout, Layout::Contact | Layout::Repeat) {
            controls = controls.child(self.select(
                "print-labels",
                "Contact-sheet labels",
                format!("{:?}", self.settings.creative.labels),
                vec![
                    ("None".into(), "Off".into()),
                    ("Name".into(), "Filename / frame timestamp".into()),
                    ("NumberAndName".into(), "Page number and name".into()),
                ],
                |s, v, cx| {
                    s.settings.creative.labels = match v.as_str() {
                        "Name" => print::LabelMode::Name,
                        "NumberAndName" => print::LabelMode::NumberAndName,
                        _ => print::LabelMode::None,
                    };
                    s.changed(cx);
                },
                cx,
            ));
            controls = controls
                .child(self.field(7, "Rows (1–20)"))
                .child(self.field(8, "Columns (1–20)"))
                .child(self.field(9, "Gutter between cells (mm)"));
        }
        if !matches!(self.settings.layout, Layout::Document | Layout::Poster) {
            controls = controls
                .child(self.field(10, "Horizontal position (%) · left 0 / right 100"))
                .child(self.field(11, "Vertical position (%) · top 0 / bottom 100"))
                .child(
                    Button::new("print-center-crop")
                        .label("Center artwork / crop")
                        .small()
                        .outline()
                        .disabled(self.busy)
                        .on_click(cx.listener(|s, _, window, cx| {
                            for i in [10, 11] {
                                s.fields[i].update(cx, |f, cx| f.set_value("50", window, cx));
                            }
                            s.changed(cx);
                        })),
                );
        }
        if self.settings.layout != Layout::Poster {
            controls = controls
                .child(self.field(12, "Bleed outside trim (mm)"))
                .child(self.select(
                    "print-crop-marks",
                    "Crop marks",
                    self.settings.creative.crop_marks.to_string(),
                    vec![
                        ("false".into(), "Off".into()),
                        ("true".into(), "On · 5 mm marks".into()),
                    ],
                    |s, v, cx| {
                        s.settings.creative.crop_marks = v == "true";
                        s.changed(cx);
                    },
                    cx,
                ));
        }
        controls.into_any_element()
    }
    pub(super) fn load_presets(&mut self, cx: &mut Context<Self>) {
        self.preset_busy = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async { print::presets::load(&print::presets::path()) })
                .await;
            this.update(cx, |s, cx| {
                s.preset_busy = false;
                match result {
                    Ok(p) => s.presets = p,
                    Err(e) => s.preset_notice = Some(format!("Presets: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn apply_preset(
        &mut self,
        preset: &print::presets::Preset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(caps) = self.caps.as_ref() else {
            return;
        };
        let (settings, notice) =
            print::presets::apply(preset, &self.settings, caps, self.destination == "pdf");
        let c = &settings.creative;
        let size = c.artwork_mm.unwrap_or([101.6, 152.4]);
        for (i, value) in [
            (1, settings.scale),
            (2, settings.extra_margin),
            (3, settings.overlap),
            (5, size[0]),
            (6, size[1]),
            (7, c.rows as f64),
            (8, c.columns as f64),
            (9, c.gutter_mm),
            (10, c.crop[0] * 100.),
            (11, c.crop[1] * 100.),
            (12, c.bleed_mm),
        ] {
            self.fields[i].update(cx, |f, cx| f.set_value(value.to_string(), window, cx));
        }
        self.fields[13].update(cx, |f, cx| f.set_value(preset.name.clone(), window, cx));
        self.fields[14].update(cx, |f, cx| {
            f.set_value(
                settings
                    .production
                    .profile
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                window,
                cx,
            )
        });
        self.fields[15].update(cx, |f, cx| {
            f.set_value(settings.production.dpi.to_string(), window, cx)
        });
        self.fields[16].update(cx, |f, cx| {
            f.set_value(settings.production.condition.clone(), window, cx)
        });
        self.settings = settings;
        self.paper_chosen = true;
        self.sheet = 0;
        self.preset_notice = notice;
        self.changed(cx);
    }
    fn save_preset(&mut self, remove: bool, cx: &mut Context<Self>) {
        if self.preset_busy || self.busy {
            return;
        }
        let name = self.fields[13].read(cx).value().trim().to_string();
        let settings = if remove {
            None
        } else {
            match self.draft(cx) {
                Ok((settings, _)) => Some(settings),
                Err(e) => {
                    self.preset_notice = Some(e.to_string());
                    cx.notify();
                    return;
                }
            }
        };
        self.preset_busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    if let Some(settings) = settings {
                        print::presets::save(&print::presets::path(), &name, &settings)
                    } else {
                        print::presets::remove(&print::presets::path(), &name)
                    }
                })
                .await;
            this.update(cx, |s, cx| {
                s.preset_busy = false;
                match result {
                    Ok(p) => {
                        s.presets = p;
                        s.preset_notice = Some(
                            if remove {
                                "Preset deleted."
                            } else {
                                "Layout preset saved. Printer options and copies remain separate."
                            }
                            .into(),
                        );
                    }
                    Err(e) => s.preset_notice = Some(format!("Presets: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn preset_controls(&self, cx: &Context<Self>) -> AnyElement {
        let presets = self.presets.clone();
        let owner = cx.weak_entity();
        let name = self.fields[13].read(cx).value().trim().to_string();
        div()
            .id("print-presets")
            .flex()
            .flex_col()
            .gap_2()
            .border_t_1()
            .border_color(theme::palette(cx).line)
            .pt_2()
            .child(
                Button::new("print-load-preset")
                    .label("Saved layout presets")
                    .small()
                    .outline()
                    .dropdown_caret(true)
                    .disabled(
                        self.busy || self.preset_busy || presets.is_empty() || self.caps.is_none(),
                    )
                    .dropdown_menu(move |mut menu, _, _| {
                        for preset in &presets {
                            let owner = owner.clone();
                            let preset = preset.clone();
                            menu = menu.item(PopupMenuItem::new(preset.name.clone()).on_click(
                                move |_, window, cx| {
                                    owner
                                        .update(cx, |s, cx| {
                                            if !s.busy {
                                                s.apply_preset(&preset, window, cx);
                                            }
                                        })
                                        .ok();
                                },
                            ));
                        }
                        menu
                    }),
            )
            .child(self.field(13, "Preset name · same name replaces layout"))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("print-save-preset")
                            .label("Save preset")
                            .small()
                            .outline()
                            .disabled(
                                self.busy
                                    || self.preset_busy
                                    || name.is_empty()
                                    || self.draft(cx).is_err(),
                            )
                            .on_click(cx.listener(|s, _, _, cx| s.save_preset(false, cx))),
                    )
                    .child(
                        Button::new("print-delete-preset")
                            .label("Delete preset")
                            .small()
                            .ghost()
                            .disabled(
                                self.busy
                                    || self.preset_busy
                                    || !self.presets.iter().any(|p| p.name == name),
                            )
                            .on_click(cx.listener(|s, _, _, cx| s.save_preset(true, cx))),
                    ),
            )
            .when_some(self.preset_notice.clone(), |d, n| {
                d.child(div().text_color(theme::palette(cx).muted).child(n))
            })
            .into_any_element()
    }
}
