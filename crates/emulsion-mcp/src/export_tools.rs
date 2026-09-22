//! Shared, validated export arguments for single and batch exports.
use emulsion_core::Document;
use emulsion_io::export::{
    ExportColorSpace, ExportFormat, ExportOptions, ExportScale, ExportWorkflow,
};
use serde_json::Value;
use std::path::Path;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ExportRequest {
    depth: Option<u8>,
    quality: Option<u8>,
    pub(crate) workflow: ExportWorkflow,
}

impl ExportRequest {
    pub(crate) fn parse(args: &Value) -> Result<Self, String> {
        let depth = match args.get("bit_depth") {
            None => None,
            Some(v) => match v.as_u64() {
                Some(8) => Some(8),
                Some(16) => Some(16),
                _ => return Err("bit_depth must be the integer 8 or 16".into()),
            },
        };
        let quality = match args.get("quality") {
            None => None,
            Some(v) => match v.as_u64() {
                Some(q @ 1..=100) => Some(q as u8),
                _ => return Err("quality must be an integer between 1 and 100".into()),
            },
        };
        let color_space = match args.get("color_space") {
            None => ExportColorSpace::Srgb,
            Some(v) => match v.as_str() {
                Some("srgb") => ExportColorSpace::Srgb,
                Some("adobe_rgb") => ExportColorSpace::AdobeRgb,
                _ => return Err("color_space must be srgb or adobe_rgb".into()),
            },
        };
        let scale = match args.get("scale") {
            None => ExportScale::Full,
            Some(v) => match v.as_str() {
                Some("full") => ExportScale::Full,
                Some("half") => ExportScale::Half,
                Some("quarter") => ExportScale::Quarter,
                _ => return Err("scale must be full, half, or quarter".into()),
            },
        };
        let dpi = match args.get("dpi") {
            None => None,
            Some(v) => match v.as_u64() {
                Some(dpi @ 1..=1200) => Some(dpi as u16),
                _ => return Err("dpi must be an integer between 1 and 1200".into()),
            },
        };
        Ok(Self {
            depth,
            quality,
            workflow: ExportWorkflow {
                scale,
                color_space,
                dpi,
            },
        })
    }

    pub(crate) fn validate_path(self, path: &Path) -> Result<(), String> {
        let format = ExportFormat::from_path(path).ok_or("unknown output format")?;
        if self.depth == Some(16) && !matches!(format, ExportFormat::Png | ExportFormat::Tiff) {
            return Err("explicit 16-bit export requires PNG or TIFF".into());
        }
        if self.depth == Some(8)
            && matches!(
                format,
                ExportFormat::Exr | ExportFormat::Hdr | ExportFormat::Farbfeld
            )
        {
            return Err("this format cannot encode 8-bit samples".into());
        }
        if self.workflow != ExportWorkflow::default()
            && !matches!(
                format,
                ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Tiff | ExportFormat::Webp
            )
        {
            return Err(
                "scale, color_space, and dpi options require PNG, JPEG, TIFF, or WebP".into(),
            );
        }
        if format == ExportFormat::Webp && self.workflow.dpi.is_some() {
            return Err("WebP does not support dpi metadata; choose PNG, JPEG, or TIFF".into());
        }
        Ok(())
    }

    pub(crate) fn write(self, doc: &Document, path: &Path) -> Result<(), String> {
        self.validate_path(path)?;
        let mut opts = ExportOptions::for_doc(doc);
        if let Some(depth) = self.depth {
            opts.depth = depth;
        }
        if let Some(quality) = self.quality {
            opts.jpeg_quality = quality;
        }
        emulsion_io::export::export_with_workflow(doc, path, opts, self.workflow)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Editor, Node};
    use emulsion_raster::Raster;
    use image::ImageDecoder;
    use serde_json::json;
    use std::{path::PathBuf, sync::Arc};

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let mut nonce = [0; 16];
            getrandom::fill(&mut nonce).unwrap();
            let dir = std::env::temp_dir().join(format!(
                "emulsion-mcp-export-{:032x}",
                u128::from_le_bytes(nonce)
            ));
            std::fs::create_dir(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn editor() -> Editor {
        let mut doc = Document::new(9, 7);
        doc.nodes.push(Node::raster(
            1,
            "Photo",
            Arc::new(Raster::solid(9, 7, [0.2, 0.4, 0.6, 1.0])),
            Default::default(),
        ));
        doc.next_id = 2;
        Editor::new(doc, None)
    }

    fn png_chunk<'a>(bytes: &'a [u8], name: &[u8; 4]) -> &'a [u8] {
        let mut offset = 8;
        while offset + 12 <= bytes.len() {
            let len = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            if &bytes[offset + 4..offset + 8] == name {
                return &bytes[offset + 8..offset + 8 + len];
            }
            offset += len + 12;
        }
        panic!("missing PNG chunk {name:?}");
    }

    #[test]
    fn export_workflow_arguments_are_strict_and_defaults_unchanged() {
        let defaults = ExportRequest::parse(&json!({})).unwrap();
        assert_eq!(defaults.workflow, ExportWorkflow::default());
        assert_eq!(defaults.depth, None);
        assert_eq!(defaults.quality, None);
        for args in [
            json!({"bit_depth":12}),
            json!({"bit_depth":"16"}),
            json!({"bit_depth":null}),
            json!({"dpi":0}),
            json!({"dpi":1201}),
            json!({"dpi":1.5}),
            json!({"scale":0.5}),
            json!({"color_space":"display_p3"}),
            json!({"quality":101}),
        ] {
            assert!(ExportRequest::parse(&args).is_err(), "{args}");
        }
        for (args, path) in [
            (json!({"bit_depth":16}), "a.jpg"),
            (json!({"dpi":300}), "a.webp"),
            (json!({"scale":"half"}), "a.bmp"),
            (json!({"bit_depth":8}), "a.exr"),
        ] {
            assert!(
                ExportRequest::parse(&args)
                    .unwrap()
                    .validate_path(Path::new(path))
                    .is_err()
            );
        }
    }

    #[test]
    fn export_image_writes_requested_depth_profile_size_and_resolution_without_edits() {
        let dir = TestDir::new();
        let path = dir.0.join("output.png");
        let mut editor = editor();
        let before = editor.doc.clone();
        let revision = editor.revision;
        let result = crate::exec::execute(
            &mut editor,
            "export_image",
            &json!({"path":path,"bit_depth":16,"color_space":"adobe_rgb","scale":"half","dpi":300}),
        );
        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(editor.doc, before);
        assert_eq!(editor.revision, revision);
        let bytes = std::fs::read(&path).unwrap();
        let mut decoder =
            image::codecs::png::PngDecoder::new(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(decoder.dimensions(), (5, 4));
        assert_eq!(decoder.color_type(), image::ColorType::Rgba16);
        let profile = decoder.icc_profile().unwrap().unwrap();
        assert_eq!(&profile[16..20], b"RGB ");
        let phys = png_chunk(&bytes, b"pHYs");
        assert_eq!(u32::from_be_bytes(phys[..4].try_into().unwrap()), 11811);
        assert_eq!(u32::from_be_bytes(phys[4..8].try_into().unwrap()), 11811);
        assert_eq!(phys[8], 1);
        let srgb_path = dir.0.join("srgb.png");
        ExportRequest::parse(&json!({"bit_depth":8}))
            .unwrap()
            .write(&editor.doc, &srgb_path)
            .unwrap();
        let mut srgb = image::codecs::png::PngDecoder::new(std::io::BufReader::new(
            std::fs::File::open(&srgb_path).unwrap(),
        ))
        .unwrap();
        assert_eq!(srgb.color_type(), image::ColorType::Rgba8);
        assert_ne!(
            &profile[128..],
            &srgb.icc_profile().unwrap().unwrap()[128..]
        );
    }

    #[test]
    fn export_rejects_bad_options_and_protects_raw_originals_before_writing() {
        let dir = TestDir::new();
        let path = dir.0.join("source.png");
        std::fs::write(&path, b"original sensor file").unwrap();
        let mut editor = editor();
        editor.doc.raw_originals.push(path.clone());
        let result = crate::exec::execute(
            &mut editor,
            "export_image",
            &json!({"path":path,"scale":"half","dpi":300}),
        );
        assert!(result.is_error);
        assert_eq!(std::fs::read(&path).unwrap(), b"original sensor file");
        let invalid = dir.0.join("invalid.jpg");
        let result = crate::exec::execute(
            &mut editor,
            "export_image",
            &json!({"path":invalid,"bit_depth":16}),
        );
        assert!(result.is_error);
        assert!(!invalid.exists());
    }

    #[test]
    fn batch_export_uses_workflow_without_replacing_sources() {
        let dir = TestDir::new();
        let path = dir.0.join("photo.png");
        let mut editor = editor();
        ExportRequest::default().write(&editor.doc, &path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let result = crate::exec::execute(
            &mut editor,
            "batch_export",
            &json!({"paths":[path],"out_dir":dir.0,"format":"png","bit_depth":16,"scale":"quarter","dpi":240}),
        );
        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let output = image::open(dir.0.join("photo-1.png")).unwrap();
        assert_eq!((output.width(), output.height()), (3, 2));
        assert_eq!(output.color(), image::ColorType::Rgba16);
    }
}
