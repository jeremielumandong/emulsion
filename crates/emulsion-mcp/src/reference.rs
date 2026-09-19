//! An attached visual reference, independent of the editable document.

use crate::server::ToolResult;
use anyhow::{Context as _, Result, anyhow};
use base64::Engine as _;
use serde_json::{Value, json};
use std::path::Path;

/// Matches the existing bounded preview renderer and stays below 1600 pixels.
pub const MAX_PREVIEW_SIDE: u32 = 1568;

#[derive(Clone, Debug)]
pub struct ReferenceImage {
    name: String,
    width: u32,
    height: u32,
    preview_width: u32,
    preview_height: u32,
    png: Vec<u8>,
}

impl ReferenceImage {
    /// Decode with the normal import size limits, EXIF orientation and colour
    /// conversion. Retain only a bounded PNG; the source file is not modified.
    pub fn load(path: &Path) -> Result<Self> {
        let doc =
            emulsion_io::import::import(path).context("Could not load the reference image")?;
        let result = crate::preview::view(&doc, &json!({"max_size": MAX_PREVIEW_SIDE})).map_err(
            |error| anyhow!("Could not preview the reference image: {:?}", error.content),
        )?;
        let encoded = result
            .content
            .iter()
            .find(|block| block["type"] == "image" && block["mimeType"] == "image/png")
            .and_then(|block| block["data"].as_str())
            .context("Reference preview did not contain a PNG")?;
        let mapping = result
            .content
            .iter()
            .filter_map(|block| block["text"].as_str())
            .filter_map(|text| serde_json::from_str::<Value>(text).ok())
            .find(|value| value["image_size"].is_array())
            .context("Reference preview did not contain image dimensions")?;
        let dimension = |axis: usize| -> Result<u32> {
            mapping["image_size"][axis]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0 && *n <= MAX_PREVIEW_SIDE)
                .context("Invalid reference preview dimensions")
        };
        Ok(Self {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Reference image".into()),
            width: doc.width,
            height: doc.height,
            preview_width: dimension(0)?,
            preview_height: dimension(1)?,
            png: base64::engine::general_purpose::STANDARD.decode(encoded)?,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Original dimensions after EXIF orientation is applied.
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn preview_width(&self) -> u32 {
        self.preview_width
    }

    pub fn preview_height(&self) -> u32 {
        self.preview_height
    }

    pub fn png(&self) -> &[u8] {
        &self.png
    }

    /// Coordinates belong to the reference, not the drawing canvas.
    pub fn metadata(&self) -> Value {
        json!({
            "source": "attached_reference",
            "name": self.name,
            "reference_size": [self.width, self.height],
            "image_size": [self.preview_width, self.preview_height],
            "image_to_reference": {
                "origin": [0, 0],
                "scale": [self.width as f64 / self.preview_width as f64,
                          self.height as f64 / self.preview_height as f64],
                "convention": "Image edge coordinates: reference = origin + image * scale. Pixel (i,j) centre uses (i+0.5,j+0.5)."
            },
            "coordinate_space": "Reference image pixels after orientation; these are not drawing document coordinates."
        })
    }

    pub fn tool_result(&self) -> ToolResult {
        ToolResult {
            content: vec![
                json!({"type": "image", "mimeType": "image/png",
                    "data": base64::engine::general_purpose::STANDARD.encode(&self.png)}),
                json!({"type": "text", "text": self.metadata().to_string()}),
            ],
            is_error: false,
        }
    }
}

pub fn missing_reference() -> ToolResult {
    ToolResult::error(
        "No reference image is attached. Attach a reference image in the assistant panel first.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(bytes: &[u8]) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "emulsion-reference-{}-{}.png",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }

        fn image(width: u32, height: u32) -> Self {
            let pixels = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 0, 0, 255]));
            Self::new(&emulsion_io::export::png8(width, height, pixels.as_raw()).unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn reference_image_returns_cached_png_and_original_dimensions() {
        let file = Fixture::image(40, 20);
        let reference = ReferenceImage::load(&file.0).unwrap();
        assert_eq!(
            reference.name(),
            file.0.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!((reference.width(), reference.height()), (40, 20));
        assert_eq!(
            (reference.preview_width(), reference.preview_height()),
            (40, 20)
        );
        drop(file);
        let result = reference.tool_result();
        assert!(!result.is_error);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(result.content[0]["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(bytes, reference.png());
        assert_eq!(result.content[0]["mimeType"], "image/png");
        let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (40, 20));
        assert_eq!(image.get_pixel(20, 10).0, [255, 0, 0, 255]);
        let metadata: Value =
            serde_json::from_str(result.content[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(metadata["reference_size"], json!([40, 20]));
        assert_eq!(metadata["image_to_reference"]["scale"], json!([1.0, 1.0]));
        assert!(metadata.get("image_to_document").is_none());
    }

    #[test]
    fn reference_image_preview_is_bounded_without_upscaling() {
        for (width, height) in [(3201, 7), (7, 3201), (1, 1)] {
            let file = Fixture::image(width, height);
            let reference = ReferenceImage::load(&file.0).unwrap();
            let (pw, ph) = (reference.preview_width(), reference.preview_height());
            assert_eq!((reference.width(), reference.height()), (width, height));
            assert!(pw <= MAX_PREVIEW_SIDE && ph <= MAX_PREVIEW_SIDE);
            assert!(pw <= width && ph <= height);
            let image = image::load_from_memory(reference.png()).unwrap();
            assert_eq!((image.width(), image.height()), (pw, ph));
            let mapping = reference.metadata();
            assert_eq!(
                mapping["image_to_reference"]["scale"],
                json!([width as f64 / pw as f64, height as f64 / ph as f64])
            );
        }
    }

    #[test]
    fn reference_image_rejects_invalid_and_oversized_input() {
        let invalid = Fixture::new(b"not an image");
        assert!(ReferenceImage::load(&invalid.0).is_err());
        let large = Fixture::image(emulsion_core::document::MAX_SIDE + 1, 1);
        assert!(ReferenceImage::load(&large.0).is_err());
        let missing = invalid.0.with_extension("missing");
        assert!(ReferenceImage::load(&missing).is_err());
    }

    #[test]
    fn reference_tool_is_read_only_and_requires_an_attachment() {
        let definition = crate::tools::definitions()
            .into_iter()
            .find(|tool| tool.name == "get_reference_image")
            .unwrap();
        assert!(crate::tools::READ_ONLY.contains(&"get_reference_image"));
        assert_eq!(definition.input_schema["properties"], json!({}));
        assert_eq!(definition.input_schema["additionalProperties"], false);
        let mut editor = emulsion_core::Editor::new(emulsion_core::Document::new(20, 20), None);
        let before = editor.doc.clone();
        let revision = editor.revision;
        let result = crate::exec::execute(&mut editor, "get_reference_image", &json!({}));
        assert_eq!(result, missing_reference());
        assert_eq!(editor.doc, before);
        assert_eq!(editor.revision, revision);
        assert_eq!(editor.history.len(), 0);
        assert_eq!(
            crate::exec::inspect(&editor.doc, "get_reference_image", &json!({})).unwrap_err(),
            missing_reference()
        );
        assert!(crate::exec::plan_heavy(&editor.doc, "get_reference_image", &json!({})).is_err());
    }
}
