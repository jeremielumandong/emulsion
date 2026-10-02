//! Export chooser: pick the format, quality and bit depth before the save
//! dialog opens, instead of guessing from the extension typed there.

use super::*;

use emulsion_io::export::{ExportColorSpace, ExportScale, ExportWorkflow};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Selectable, Sizable, WindowExt};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExportPrefs {
    pub open: bool,
    /// File extension of the chosen format.
    pub ext: &'static str,
    /// JPEG quality 1–100.
    pub quality: u8,
    /// 16-bit output where the format allows (PNG, TIFF).
    pub depth16: bool,
    /// The longer list of formats is unfolded.
    pub more: bool,
    pub scale: ExportScale,
    pub color_space: ExportColorSpace,
    pub dpi: Option<u16>,
    diagram_quality_initialized: bool,
}

impl Default for ExportPrefs {
    fn default() -> Self {
        Self {
            open: false,
            ext: "png",
            quality: 92,
            depth16: false,
            more: false,
            scale: ExportScale::Full,
            color_space: ExportColorSpace::Srgb,
            dpi: None,
            diagram_quality_initialized: false,
        }
    }
}

impl ExportPrefs {
    pub fn workflow(self) -> ExportWorkflow {
        if !matches!(self.ext, "png" | "jpg" | "tif" | "webp") {
            return ExportWorkflow::default();
        }
        ExportWorkflow {
            scale: self.scale,
            color_space: self.color_space,
            dpi: if self.ext == "webp" { None } else { self.dpi },
        }
    }
}

/// (extension, label, catalog key for what it is for). The everyday formats first; the
/// rest sit behind "more formats". Converter-backed ones show only when
/// this machine can write them.
const FORMATS: &[(&str, &str, &str)] = &[
    ("png", "PNG", "editor.export_ui.help_png"),
    ("jpg", "JPEG", "editor.export_ui.help_jpg"),
    ("webp", "WebP", "editor.export_ui.help_webp"),
    ("tif", "TIFF", "editor.export_ui.help_tif"),
    ("psd", "PSD", "editor.export_ui.help_psd"),
    ("xcf", "XCF", "editor.export_ui.help_xcf"),
];

/// Formats past the everyday ones, GIMP's list.
const MORE_FORMATS: &[(&str, &str, &str)] = &[
    ("avif", "AVIF", "editor.export_ui.help_avif"),
    ("heic", "HEIC", "editor.export_ui.help_heic"),
    ("jxl", "JPEG XL", "editor.export_ui.help_jxl"),
    ("pdf", "PDF", "editor.export_ui.help_pdf"),
    ("exr", "OpenEXR", "editor.export_ui.help_exr"),
    ("hdr", "Radiance HDR", "editor.export_ui.help_hdr"),
    ("bmp", "BMP", "editor.export_ui.help_bmp"),
    ("gif", "GIF", "editor.export_ui.help_gif"),
    ("tga", "Targa", "editor.export_ui.help_tga"),
    ("ppm", "PPM", "editor.export_ui.help_ppm"),
    ("ico", "ICO", "editor.export_ui.help_ico"),
    ("qoi", "QOI", "editor.export_ui.help_qoi"),
    ("ff", "farbfeld", "editor.export_ui.help_ff"),
];

/// The dialog observes its editor so changing a format updates its controls
/// without mounting a second editor or changing the canvas layout.
struct ExportDialog {
    owner: WeakEntity<EditorView>,
    _subscription: Subscription,
}
impl Render for ExportDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = crate::theme::palette(cx);
        self.owner
            .update(cx, |editor, cx| editor.export_dialog_body(&p, cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl EditorView {
    pub(crate) fn open_export_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.export_prefs.open {
            return;
        }
        if self.is_diagram() && !self.export_prefs.diagram_quality_initialized {
            self.export_prefs.scale = ExportScale::Double;
            self.export_prefs.diagram_quality_initialized = true;
        }
        self.export_prefs.open = true;
        self.export_prefs.depth16 = self.editor.doc.source_depth == 16;
        let owner = cx.entity();
        let body = cx.new(|cx| ExportDialog {
            owner: owner.downgrade(),
            _subscription: cx.observe(&owner, |_, _, cx| cx.notify()),
        });
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let close = owner.clone();
            let cancel = owner.clone();
            let confirm = owner.clone();
            let enter = owner.clone();
            dialog
                .title(SharedString::from(t!("editor.export_ui.title")))
                .width(px(620.))
                .child(body.clone())
                .footer(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("export-cancel")
                                .label(t!("shell.cancel"))
                                .on_click(move |_, window, cx| {
                                    cancel
                                        .update(cx, |editor, cx| {
                                            editor.dismiss_export_dialog(window, cx)
                                        })
                                        .ok();
                                }),
                        )
                        .child(
                            Button::new("export-go")
                                .label(t!("file.export"))
                                .primary()
                                .on_click(move |_, window, cx| {
                                    confirm
                                        .update(cx, |editor, cx| {
                                            editor.confirm_export_dialog(window, cx)
                                        })
                                        .ok();
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    enter
                        .update(cx, |editor, cx| editor.confirm_export_dialog(window, cx))
                        .ok();
                    false // Confirmation closes the dialog itself, once.
                })
                .on_close(move |_, _, cx| {
                    close
                        .update(cx, |editor, cx| {
                            editor.export_prefs.open = false;
                            cx.notify();
                        })
                        .ok();
                })
        });
        cx.notify();
    }

    pub(super) fn dismiss_export_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.export_prefs.open = false;
        window.close_dialog(cx);
        cx.notify();
    }

    fn export_dialog_body(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let prefs = self.export_prefs;
        let (w, h) = prefs
            .workflow()
            .scale
            .dimensions(self.editor.doc.width, self.editor.doc.height);
        let section = |title: std::borrow::Cow<'static, str>, controls: AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(title)),
                )
                .child(controls)
        };
        let mut formats =
            crate::widgets::command_bar("export-formats", t!("editor.export_ui.format_bar"));
        let more_open = prefs.more || MORE_FORMATS.iter().any(|(e, _, _)| *e == prefs.ext);
        for (i, (ext, name, help)) in FORMATS
            .iter()
            .chain(
                more_open
                    .then_some(MORE_FORMATS.iter())
                    .into_iter()
                    .flatten(),
            )
            .filter(|(ext, _, _)| {
                (self.is_diagram() && *ext == "pdf")
                    || emulsion_io::ExportFormat::from_path(std::path::Path::new(&format!(
                        "x.{ext}"
                    )))
                    .is_some_and(|f| f.available())
            })
            .enumerate()
        {
            let (ext, name, help) = (*ext, *name, *help);
            formats = formats.child(
                Button::new(("export-fmt", i))
                    .small()
                    .outline()
                    .label(name)
                    .selected(prefs.ext == ext)
                    .tooltip(t!(help))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.export_prefs.ext = ext;
                        cx.notify();
                    })),
            );
        }
        formats = formats.child(
            Button::new("export-more")
                .small()
                .ghost()
                .label(if more_open {
                    t!("editor.export_ui.fewer_formats")
                } else {
                    t!("editor.export_ui.more_formats")
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.export_prefs.more = !more_open;
                    if !this.export_prefs.more
                        && MORE_FORMATS
                            .iter()
                            .any(|(ext, _, _)| *ext == this.export_prefs.ext)
                    {
                        this.export_prefs.ext = "png";
                    }
                    cx.notify();
                })),
        );
        let mut body = div()
            .id("export-dialog-body")
            .test_support()
            .flex()
            .flex_col()
            .gap_4()
            .min_w_0()
            .child(div().text_sm().text_color(p.muted).child(format!(
                "{} · {w} × {h} px{}",
                self.name,
                if self.editor.kind().is_some() {
                    format!(" · {}", t!("editor.export_ui.current_page"))
                } else {
                    String::new()
                }
            )))
            .child(section(
                t!("editor.export_ui.format"),
                formats.into_any_element(),
            ));
        if matches!(prefs.ext, "jpg" | "avif" | "heic" | "jxl") {
            body = body.child(section(
                t!("editor.export_ui.quality"),
                self.opt_slider(
                    SliderKey::ExportQuality,
                    &t!("editor.export_ui.quality_slider"),
                    format!("{}", prefs.quality),
                    prefs.quality as f32 / 100.,
                    (1., 100., 1.),
                    p,
                    cx,
                )
                .into_any_element(),
            ));
        }
        if matches!(
            prefs.ext,
            "png" | "tif" | "exr" | "ff" | "avif" | "heic" | "jxl"
        ) {
            let mut controls =
                crate::widgets::command_bar("export-depth", t!("editor.export_ui.bit_depth"));
            for bits in [8usize, 16] {
                controls = controls.child(
                    Button::new(("export-depth", bits))
                        .small()
                        .ghost()
                        .label(t!("editor.export_ui.bits", bits = bits))
                        .selected(prefs.depth16 == (bits == 16))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.export_prefs.depth16 = bits == 16;
                            cx.notify();
                        })),
                );
            }
            body = body.child(section(
                t!("editor.export_ui.bit_depth"),
                controls.into_any_element(),
            ));
        }
        if matches!(prefs.ext, "png" | "jpg" | "tif" | "webp") {
            let mut controls =
                crate::widgets::command_bar("export-size", t!("editor.export_ui.output_size"));
            for (key, name, scale) in [
                ("full", "1×".into(), ExportScale::Full),
                ("double", "2×".into(), ExportScale::Double),
                ("quadruple", "4×".into(), ExportScale::Quadruple),
                ("half", t!("editor.export_ui.half"), ExportScale::Half),
                (
                    "quarter",
                    t!("editor.export_ui.quarter"),
                    ExportScale::Quarter,
                ),
            ] {
                controls = controls.child(
                    Button::new(format!("export-size-{key}"))
                        .small()
                        .ghost()
                        .label(name)
                        .selected(prefs.scale == scale)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.export_prefs.scale = scale;
                            cx.notify();
                        })),
                );
            }
            body = body.child(section(
                t!("editor.export_ui.output_size"),
                controls.into_any_element(),
            ));
            let mut controls = crate::widgets::command_bar(
                "export-profile",
                t!("editor.export_ui.output_profile"),
            );
            for (key, name, space) in [
                ("srgb", "sRGB", ExportColorSpace::Srgb),
                ("adobe", "Adobe RGB", ExportColorSpace::AdobeRgb),
                ("prophoto", "ProPhoto RGB", ExportColorSpace::ProPhoto),
            ] {
                controls = controls.child(
                    Button::new(format!("export-profile-{key}"))
                        .small()
                        .ghost()
                        .label(name)
                        .selected(prefs.color_space == space)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.export_prefs.color_space = space;
                            cx.notify();
                        })),
                );
            }
            body = body.child(section(
                t!("editor.export_ui.output_profile"),
                controls.into_any_element(),
            ));
            if prefs.color_space == ExportColorSpace::AdobeRgb {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(p.muted)
                        .child(t!("editor.export_ui.adobe_note")),
                );
            }
            if prefs.ext != "webp" {
                let mut controls =
                    crate::widgets::command_bar("export-ppi", t!("editor.export_ui.resolution"));
                for (key, name, dpi) in [
                    ("none", t!("editor.export_ui.unspecified"), None),
                    ("72", "72 ppi".into(), Some(72)),
                    ("240", "240 ppi".into(), Some(240)),
                    ("300", "300 ppi".into(), Some(300)),
                ] {
                    controls = controls.child(
                        Button::new(format!("export-ppi-{key}"))
                            .small()
                            .ghost()
                            .label(name)
                            .selected(prefs.dpi == dpi)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.export_prefs.dpi = dpi;
                                cx.notify();
                            })),
                    );
                }
                body = body.child(section(
                    t!("editor.export_ui.resolution"),
                    controls.into_any_element(),
                ));
            }
        }
        if let Some((_, _, help)) = FORMATS
            .iter()
            .chain(MORE_FORMATS)
            .find(|(ext, _, _)| *ext == prefs.ext)
        {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child(SharedString::from(t!(*help))),
            );
        }
        if self.editor.kind().is_some() {
            body = body.child(section(
                t!("editor.export_ui.project_export"),
                self.project_export_options(cx),
            ));
        }
        body.into_any_element()
    }

    fn confirm_export_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_export_dialog(window, cx);
        window.focus(&self.canvas_focus, cx);
        window.dispatch_action(Box::new(crate::actions::ConfirmExport), cx);
    }
}
