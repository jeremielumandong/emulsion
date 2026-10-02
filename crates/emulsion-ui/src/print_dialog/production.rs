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
                .context(t!("print.production.dpi_invalid"))?;
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
            &t!("print.production.color_management"),
            self.settings.production.managed.to_string(),
            vec![
                (
                    "false".into(),
                    t!("print.production.printer_managed").into(),
                ),
                ("true".into(), t!("print.production.app_managed").into()),
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
                &t!("print.production.pdf_standard"),
                format!("{:?}", self.settings.production.standard),
                vec![
                    ("Pdf".into(), t!("print.production.ordinary_pdf").into()),
                    ("PdfX1a2001".into(), t!("print.production.pdfx1a").into()),
                    ("PdfX32002".into(), t!("print.production.pdfx3").into()),
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
                .child(self.field(14, &t!("print.production.icc_path")))
                .child(
                    Button::new("print-choose-icc")
                        .label(t!("print.production.choose_icc"))
                        .small()
                        .outline()
                        .disabled(self.busy)
                        .on_click(cx.listener(|s, _, window, cx| s.choose_profile(window, cx))),
                )
                .child(self.select(
                    "print-rendering-intent",
                    &t!("print.production.rendering_intent"),
                    self.settings.production.intent.to_string(),
                    vec![
                        ("0".into(), t!("print.production.perceptual").into()),
                        ("1".into(), t!("print.production.relative").into()),
                        ("2".into(), t!("print.production.saturation").into()),
                        ("3".into(), t!("print.production.absolute").into()),
                    ],
                    |s, v, cx| {
                        s.settings.production.intent = v.parse().unwrap_or(1);
                        s.changed(cx);
                    },
                    cx,
                ))
                .child(self.field(15, &t!("print.production.dpi")))
                .child(self.field(16, &t!("print.production.condition")));
            if self.destination != "pdf" {
                controls = controls.child(self.select(
                    "print-driver-color",
                    &t!("print.production.driver_color"),
                    self.settings.production.driver_color_disabled.to_string(),
                    vec![
                        ("false".into(), t!("print.production.not_disabled").into()),
                        ("true".into(), t!("print.production.disabled").into()),
                    ],
                    |s, v, cx| {
                        s.settings.production.driver_color_disabled = v == "true";
                        s.changed(cx);
                    },
                    cx,
                ));
            }
            controls = controls.child(
                div()
                    .text_color(theme::palette(cx).muted)
                    .child(t!("print.production.note")),
            );
        }
        controls.into_any_element()
    }
}
