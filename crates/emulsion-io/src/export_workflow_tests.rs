use super::*;
use emulsion_core::Node;
use image::ImageDecoder;
use std::{
    io::BufReader,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "emulsion-export-workflow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn photo() -> Document {
    let mut doc = Document::new(20, 12);
    doc.source_depth = 16;
    doc.nodes.push(Node::raster(
        1,
        "Photo",
        Arc::new(Raster::solid(20, 12, [0.25, 0.4, 0.6, 1.0])),
        Default::default(),
    ));
    doc.next_id = 2;
    doc
}

#[test]
fn workflow_resize_profile_depth_and_resolution_are_written() {
    let temp = Scratch::new();
    let doc = photo();
    for ext in ["png", "jpg", "tif", "webp"] {
        let path = temp.0.join(format!("photo.{ext}"));
        let flow = ExportWorkflow {
            scale: ExportScale::Half,
            color_space: ExportColorSpace::AdobeRgb,
            dpi: if ext == "webp" { None } else { Some(300) },
        };
        export_with_workflow(&doc, &path, ExportOptions::for_doc(&doc), flow).unwrap();
        let mut decoder: Box<dyn ImageDecoder> = if ext == "tif" {
            Box::new(
                image::codecs::tiff::TiffDecoder::new(BufReader::new(
                    std::fs::File::open(&path).unwrap(),
                ))
                .unwrap(),
            )
        } else {
            Box::new(
                image::ImageReader::open(&path)
                    .unwrap()
                    .with_guessed_format()
                    .unwrap()
                    .into_decoder()
                    .unwrap(),
            )
        };
        assert_eq!(decoder.dimensions(), (10, 6));
        let mut embedded = decoder.icc_profile().unwrap().unwrap();
        let mut expected = ColorProfile::new_adobe_rgb().encode().unwrap();
        // ICC header creation time differs if encoding crosses a wall-clock
        // second. All actual colorimetry and transfer tags must still match.
        ColorProfile::new_from_slice(&embedded).unwrap();
        embedded[24..36].fill(0);
        expected[24..36].fill(0);
        assert_eq!(embedded, expected);
        if ext == "png" || ext == "tif" {
            assert_eq!(decoder.color_type(), image::ColorType::Rgba16);
        }
        match ext {
            "png" => {
                let png = png::Decoder::new(BufReader::new(std::fs::File::open(&path).unwrap()))
                    .read_info()
                    .unwrap();
                let density = png.info().pixel_dims.unwrap();
                assert_eq!(density.unit, png::Unit::Meter);
                assert_eq!((density.xppu, density.yppu), (11811, 11811));
            }
            "jpg" => {
                let data = std::fs::read(&path).unwrap();
                let offset = data.windows(5).position(|p| p == b"JFIF\0").unwrap();
                assert_eq!(&data[offset + 7..offset + 12], &[1, 1, 44, 1, 44]);
            }
            "tif" => {
                let mut tiff = tiff::decoder::Decoder::new(BufReader::new(
                    std::fs::File::open(&path).unwrap(),
                ))
                .unwrap();
                assert_eq!(
                    tiff.get_tag_unsigned::<u16>(tiff::tags::Tag::ResolutionUnit)
                        .unwrap(),
                    2
                );
                for tag in [tiff::tags::Tag::XResolution, tiff::tags::Tag::YResolution] {
                    assert!(matches!(
                        tiff.get_tag(tag).unwrap(),
                        tiff::decoder::ifd::Value::Rational(300, 1)
                    ));
                }
            }
            _ => {}
        }
    }
    assert_eq!((doc.width, doc.height), (20, 12));
}

#[test]
fn profile_conversion_preserves_color_and_alpha() {
    let raster = Raster::solid(3, 2, [0.25, 0.4, 0.6, 0.5]);
    let baseline = raster.to_srgba16();
    let (mut converted, profile) = converted(&raster, ExportColorSpace::AdobeRgb, false).unwrap();
    assert_ne!(converted, baseline);
    assert_eq!(converted[3], baseline[3]);
    crate::icc::to_srgb_16(&profile, &mut converted);
    for (a, b) in converted.iter().zip(baseline.iter()) {
        assert!((*a as i32 - *b as i32).abs() < 30, "{a} != {b}");
    }
    assert_eq!(ExportScale::Quarter.dimensions(21, 13), (6, 4));
}

#[test]
fn invalid_workflow_leaves_destination_untouched() {
    let temp = Scratch::new();
    let doc = photo();
    for (ext, flow) in [
        (
            "png",
            ExportWorkflow {
                dpi: Some(0),
                ..Default::default()
            },
        ),
        (
            "webp",
            ExportWorkflow {
                dpi: Some(300),
                ..Default::default()
            },
        ),
        (
            "psd",
            ExportWorkflow {
                scale: ExportScale::Half,
                ..Default::default()
            },
        ),
    ] {
        let path = temp.0.join(format!("existing.{ext}"));
        std::fs::write(&path, b"keep output").unwrap();
        assert!(export_with_workflow(&doc, &path, ExportOptions::for_doc(&doc), flow).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"keep output");
    }
}
