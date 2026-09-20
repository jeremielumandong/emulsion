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
}

impl Default for ExportPrefs {
    fn default() -> Self {
        Self {
            open: false,
            ext: "png",
            quality: 92,
            depth16: false,
        }
    }
}

/// (extension, label, what it is for).
const FORMATS: [(&str, &str, &str); 5] = [
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
        for (ext, name, help) in FORMATS {
            row = row.child(
                tip(
                    chip(
                        ("export-fmt", ext.len() * 7 + name.len()),
                        name,
                        prefs.ext == ext,
                        p,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.export_prefs.ext = ext;
                        cx.notify();
                    })),
                    help,
                )
                .into_any_element(),
            );
        }
        if prefs.ext == "jpg" {
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
        if matches!(prefs.ext, "png" | "tif") {
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
            "jpg" => "flat image · lossy",
            "webp" => "flat image · lossless",
            "psd" => "layers kept · adjustments and styles flattened into pixels",
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
