//! Export chooser: pick the format, quality and bit depth before the save
//! dialog opens, instead of guessing from the extension typed there.

use super::*;
use crate::widgets::tip;

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
}

impl Default for ExportPrefs {
    fn default() -> Self {
        Self {
            open: false,
            ext: "png",
            quality: 92,
            depth16: false,
            more: false,
        }
    }
}

/// (extension, label, what it is for). The everyday formats first; the
/// rest sit behind "more formats". Converter-backed ones show only when
/// this machine can write them.
const FORMATS: &[(&str, &str, &str)] = &[
    (
        "png",
        "PNG",
        "Lossless, 8 or 16-bit. Web, screenshots, archiving a flat copy.",
    ),
    (
        "jpg",
        "JPEG",
        "Small photos for sharing. Lossy; quality below sets the trade.",
    ),
    (
        "webp",
        "WebP",
        "Lossless here, smaller than PNG for the web.",
    ),
    (
        "tif",
        "TIFF",
        "Lossless, 8 or 16-bit. Print shops and other editors.",
    ),
    (
        "psd",
        "PSD",
        "Layered Photoshop file. Layers, groups, masks and blend modes carry over; effects flatten.",
    ),
    (
        "xcf",
        "XCF",
        "Layered GIMP file, 8-bit. Visible top-level layers carry over with their opacity; everything else bakes into them.",
    ),
];

/// Formats past the everyday ones, GIMP's list.
const MORE_FORMATS: &[(&str, &str, &str)] = &[
    (
        "avif",
        "AVIF",
        "Modern lossy photo format, small at high quality. Written by avifenc or ImageMagick.",
    ),
    (
        "heic",
        "HEIC",
        "Apple's photo format. Written by heif-enc or ImageMagick.",
    ),
    (
        "jxl",
        "JPEG XL",
        "Next-generation JPEG, lossy or lossless at quality 100. Written by cjxl or ImageMagick.",
    ),
    (
        "pdf",
        "PDF",
        "One-page PDF of the flat picture, via ImageMagick.",
    ),
    (
        "exr",
        "OpenEXR",
        "Float linear RGBA for compositing and VFX; 16-bit source precision kept.",
    ),
    (
        "hdr",
        "Radiance HDR",
        "Float RGB without alpha, for HDR pipelines.",
    ),
    ("bmp", "BMP", "Uncompressed Windows bitmap with alpha."),
    (
        "gif",
        "GIF",
        "256 colours, one frame. For the Timeline's animated GIF use that panel.",
    ),
    (
        "tga",
        "Targa",
        "Game and 3D pipelines; lossless with alpha.",
    ),
    (
        "ppm",
        "PPM",
        "Plain portable pixmap, no alpha; every image tool reads it.",
    ),
    ("ico", "ICO", "Windows icon, scaled to fit 256 px."),
    ("qoi", "QOI", "Quite OK Image: lossless, fast, small."),
    ("ff", "farbfeld", "suckless 16-bit RGBA, lossless."),
];

impl EditorView {
    pub fn toggle_export_panel(&mut self, cx: &mut Context<Self>) {
        self.export_prefs.open = !self.export_prefs.open;
        if self.export_prefs.open {
            self.export_prefs.depth16 = self.editor.doc.source_depth == 16;
        }
        cx.notify();
    }

    pub(crate) fn export_panel_view(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.export_prefs.open {
            return None;
        }
        let prefs = self.export_prefs;
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(10.))
            .px(px(16.))
            .py(px(10.))
            .border_b_1()
            .border_color(p.line)
            .bg(p.panel)
            .child(label(format!("Export · {w}×{h}"), p));
        let more_open = prefs.more || MORE_FORMATS.iter().any(|(e, _, _)| *e == prefs.ext);
        let listed: Vec<&(&str, &str, &str)> = FORMATS
            .iter()
            .chain(
                more_open
                    .then_some(MORE_FORMATS.iter())
                    .into_iter()
                    .flatten(),
            )
            .filter(|(ext, _, _)| {
                emulsion_io::export::ExportFormat::from_path(std::path::Path::new(&format!(
                    "x.{ext}"
                )))
                .is_some_and(|f| f.available())
            })
            .collect();
        for (i, (ext, name, help)) in listed.into_iter().enumerate() {
            let (ext, name, help) = (*ext, *name, *help);
            row = row.child(
                tip(
                    chip(("export-fmt", i), name, prefs.ext == ext, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.export_prefs.ext = ext;
                            cx.notify();
                        },
                    )),
                    help,
                )
                .into_any_element(),
            );
        }
        row = row.child(
            chip(
                "export-more",
                if more_open {
                    "fewer formats"
                } else {
                    "more formats…"
                },
                false,
                p,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.export_prefs.more = !more_open;
                if !this.export_prefs.more
                    && MORE_FORMATS
                        .iter()
                        .any(|(e, _, _)| *e == this.export_prefs.ext)
                {
                    this.export_prefs.ext = "png";
                }
                cx.notify();
            })),
        );
        if matches!(prefs.ext, "jpg" | "avif" | "heic" | "jxl") {
            row = row.child(self.opt_slider(
                SliderKey::ExportQuality,
                "quality",
                format!("{}", prefs.quality),
                prefs.quality as f32 / 100.0,
                (1.0, 100.0, 1.0),
                p,
                cx,
            ));
        }
        if matches!(
            prefs.ext,
            "png" | "tif" | "exr" | "ff" | "avif" | "heic" | "jxl"
        ) {
            for (bits, on) in [(8u8, !prefs.depth16), (16u8, prefs.depth16)] {
                row = row.child(
                    chip(
                        ("export-depth", bits as usize),
                        format!("{bits}-bit"),
                        on,
                        p,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.export_prefs.depth16 = bits == 16;
                        cx.notify();
                    })),
                );
            }
        }
        let note = match prefs.ext {
            "jpg" | "avif" | "heic" => "flat image · lossy · no transparency for JPEG",
            "jxl" => "flat image · lossy below quality 100",
            "webp" | "qoi" | "ff" | "tga" | "bmp" => "flat image · lossless",
            "psd" => "layers kept · adjustments and styles flattened into pixels",
            "xcf" => "visible layers kept, 8-bit · hidden layers left out",
            "gif" => "flat image · 256 colours",
            "ppm" | "hdr" | "pdf" => "flat image · no transparency",
            "ico" => "flat image · 256 px at most",
            _ => "flat image · lossless",
        };
        row = row
            .child(mono(note, 10., p.muted))
            .child(div().flex_1())
            .child(
                button("export-go", "Export…", true, p).on_click(cx.listener(
                    |_, _, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::Export), cx);
                    },
                )),
            )
            .child(
                button("export-cancel", "Cancel", false, p).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.export_prefs.open = false;
                        cx.notify();
                    },
                )),
            );
        Some(row.into_any_element())
    }
}
