use super::*;
use print::production::PdfStandard;
impl PrintDialog {
    pub(super) fn production_draft(&self, s: &mut Settings, cx: &App) -> anyhow::Result<()> {
        use anyhow::Context;
        if s.production.enabled() {
            let path = self.fields[14].read(cx).value().trim().to_string();
            s.production.profile = (!path.is_empty()).then(|| path.into());
            s.production.dpi = self.fields[15]
                .read(cx)
                .value()
                .parse()
                .context("Enter output resolution from 150–600 PPI")?;
            s.production.condition = self.fields[16].read(cx).value().trim().to_string();
        }
        if self.destination == "pdf" {
            s.production.validate()
        } else {
            s.production.validate_device(self.destination == "portal")
        }
    }
    fn choose_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = request.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update_in(cx, |s, window, cx| {
                    s.fields[14].update(cx, |f, cx| {
                        f.set_value(path.display().to_string(), window, cx)
                    });
                    s.changed(cx);
                })
                .ok();
            }
        })
        .detach();
    }
    pub(super) fn production_controls(&self, cx: &Context<Self>) -> AnyElement {
        let mut controls = div().flex().flex_col().gap_2().child(self.select(
            "print-color-management",
            "Color management",
            self.settings.production.managed.to_string(),
            vec![
                ("false".into(), "Printer-managed / ordinary PDF".into()),
                ("true".into(), "App-managed ICC output".into()),
            ],
            |s, v, cx| {
                s.settings.production.managed = v == "true";
                if v == "false" {
                    s.settings.production.standard = PdfStandard::Pdf;
                }
                s.changed(cx);
            },
            cx,
        ));
        if self.destination == "pdf" {
            controls = controls.child(self.select(
                "print-pdf-standard",
                "PDF standard",
                format!("{:?}", self.settings.production.standard),
                vec![
                    ("Pdf".into(), "Ordinary PDF".into()),
                    ("PdfX1a2001".into(), "PDF/X-1a:2001 · flattened CMYK".into()),
                    ("PdfX32002".into(), "PDF/X-3:2002 · flattened CMYK".into()),
                ],
                |s, v, cx| {
                    s.settings.production.standard = match v.as_str() {
                        "PdfX1a2001" => PdfStandard::PdfX1a2001,
                        "PdfX32002" => PdfStandard::PdfX32002,
                        _ => PdfStandard::Pdf,
                    };
                    if s.settings.production.standard != PdfStandard::Pdf {
                        s.settings.production.managed = true;
                    }
                    s.changed(cx);
                },
                cx,
            ));
        }
        if self.settings.production.enabled() {
            controls = controls
                .child(self.field(14, "Output ICC profile path"))
                .child(
                    Button::new("print-choose-icc")
                        .label("Choose ICC profile…")
                        .small()
                        .outline()
                        .disabled(self.busy)
                        .on_click(cx.listener(|s, _, window, cx| s.choose_profile(window, cx))),
                )
                .child(self.select(
                    "print-rendering-intent",
                    "Rendering intent",
                    self.settings.production.intent.to_string(),
                    vec![
                        ("0".into(), "Perceptual".into()),
                        ("1".into(), "Relative colorimetric".into()),
                        ("2".into(), "Saturation".into()),
                        ("3".into(), "Absolute colorimetric".into()),
                    ],
                    |s, v, cx| {
                        s.settings.production.intent = v.parse().unwrap_or(1);
                        s.changed(cx);
                    },
                    cx,
                ))
                .child(self.field(15, "Managed output resolution (150–600 PPI)"))
                .child(self.field(16, "Print condition / profile description"));
            if self.destination != "pdf" {
                controls = controls.child(self.select(
                    "print-driver-color",
                    "Printer color correction",
                    self.settings.production.driver_color_disabled.to_string(),
                    vec![
                        ("false".into(), "Not disabled".into()),
                        ("true".into(), "Disabled in printer settings".into()),
                    ],
                    |s, v, cx| {
                        s.settings.production.driver_color_disabled = v == "true";
                        s.changed(cx);
                    },
                    cx,
                ));
            }
            controls=controls.child(div().text_color(theme::palette(cx).muted).child("Managed output flattens at the chosen resolution. PDF/X requires a CMYK v2 output profile; native printers require an RGB printer profile. Preview simulates the profile in sRGB; printed color still depends on paper and driver settings."));
        }
        controls.into_any_element()
    }
}
