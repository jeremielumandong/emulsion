//! Embedded pattern import for layer effects.
use super::*;
use emulsion_core::style_options::PatternImage;
use image::{DynamicImage, ImageDecoder, ImageReader};

fn load_pattern(path: &std::path::Path) -> Result<PatternImage, String> {
    let reader = ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || width > 2048 || height > 2048 {
        return Err("Use a pattern image no larger than 2048 × 2048 pixels.".into());
    }
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut decoded = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    decoded.apply_orientation(orientation);
    let rgba = decoded.into_rgba8();
    Ok(PatternImage {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
    })
}

impl EditorView {
    pub(super) fn import_style_pattern(
        &mut self,
        id: NodeId,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        if index >= node.styles.len() {
            return;
        }
        let effect_id = node.style_options.get(index).map(|option| option.id);
        let ticket = self.edit_ticket();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import layer style pattern".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move { load_pattern(&path) })
                .await;
            this.update(cx, |this, cx| {
                let same_effect = this.editor.doc.node(id).is_some_and(|node| {
                    index < node.styles.len()
                        && node.style_options.get(index).map(|option| option.id) == effect_id
                });
                let current = this.edit_ticket() == ticket
                    && (!this.editor.in_transaction() || this.styles_ui.dialog_for == Some(id));
                if !current || !same_effect {
                    this.set_status(
                        "Pattern import canceled because the document changed.",
                        false,
                        cx,
                    );
                    return;
                }
                match result {
                    Ok(pattern) => this.set_style_pattern(id, index, Arc::new(pattern), cx),
                    Err(error) => this.set_status(error, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }
}
