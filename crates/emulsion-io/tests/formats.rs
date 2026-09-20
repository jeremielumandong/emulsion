//! Every import format opens to the same pixels: the `image` crate's wider
//! set, JPEG XL, GIMP's XCF with its layer facts, and (when the machine has
//! ImageMagick) a converter-backed format.

use emulsion_core::NodeKind;
use emulsion_io::open;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("emulsion-format-tests-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// Red on the left, blue on the right, 6×4, fully opaque.
fn picture() -> RgbaImage {
    RgbaImage::from_fn(6, 4, |x, _| {
        if x < 3 {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 0, 255, 255])
        }
    })
}

fn first_raster(path: &Path) -> (u32, u32, [u16; 4], [u16; 4]) {
    let doc = open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let NodeKind::Raster { raster, .. } = &doc.nodes[0].kind else {
        panic!("not a raster");
    };
    (doc.width, doc.height, raster.get(1, 1), raster.get(4, 1))
}

fn assert_red_blue(path: &Path) {
    let (w, h, left, right) = first_raster(path);
    assert_eq!((w, h), (6, 4), "{}", path.display());
    // Linear storage: pure primaries survive the transfer curve exactly.
    assert_eq!(left, [65535, 0, 0, 65535], "{} left", path.display());
    assert_eq!(right, [0, 0, 65535, 65535], "{} right", path.display());
}

#[test]
fn image_crate_formats_open() {
    let img = DynamicImage::ImageRgba8(picture());
    for (ext, fmt) in [
        ("tga", ImageFormat::Tga),
        ("pam", ImageFormat::Pnm),
        ("qoi", ImageFormat::Qoi),
        ("ico", ImageFormat::Ico),
        ("bmp", ImageFormat::Bmp),
    ] {
        let p = scratch(&format!("pic.{ext}"));
        img.save_with_format(&p, fmt).unwrap();
        assert_red_blue(&p);
    }
    // farbfeld is 16-bit only.
    let p = scratch("pic.ff");
    DynamicImage::ImageRgba16(img.to_rgba16())
        .save_with_format(&p, ImageFormat::Farbfeld)
        .unwrap();
    assert_red_blue(&p);
    // Float formats: written from linear values, read back as 16-bit.
    let f32 = img.to_rgba32f();
    let p = scratch("pic.exr");
    DynamicImage::ImageRgba32F(f32)
        .save_with_format(&p, ImageFormat::OpenExr)
        .unwrap();
    let (w, h, left, right) = first_raster(&p);
    assert_eq!((w, h), (6, 4));
    assert!(left[0] > 60000 && left[2] == 0 && right[2] > 60000 && right[0] == 0);
}

#[test]
fn ppm_without_alpha_opens_opaque() {
    let p = scratch("pic.ppm");
    DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(picture()).to_rgb8())
        .save_with_format(&p, ImageFormat::Pnm)
        .unwrap();
    assert_red_blue(&p);
}

#[test]
fn jpeg_xl_opens() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny.jxl");
    assert_red_blue(&p);
}

#[test]
fn gimp_xcf_opens_with_layer_names_offsets_and_opacity() {
    use xcf_rs::create::XcfCreator;
    use xcf_rs::data::color::ColorType;
    use xcf_rs::data::layer::Layer;
    use xcf_rs::data::pixeldata::PixelData;
    use xcf_rs::data::property::{Property, PropertyIdentifier, PropertyPayload};
    use xcf_rs::data::rgba::RgbaPixel;
    use xcf_rs::{LayerColorType, LayerColorValue};

    let layer = |name: &str, w: u32, h: u32, px: [u8; 4], props: Vec<Property>| Layer {
        width: w,
        height: h,
        kind: LayerColorType {
            kind: LayerColorValue::Rgb,
            alpha: true,
        },
        name: name.to_string(),
        pixels: PixelData {
            width: w,
            height: h,
            pixels: vec![RgbaPixel(px); (w * h) as usize],
        },
        properties: props,
    };
    let mut xcf = XcfCreator::new(11, 6, 4, ColorType::Rgb);
    xcf.add_properties(&vec![]);
    // GIMP order: top layer first.
    let layers = vec![
        layer(
            "Sticker",
            2,
            2,
            [0, 255, 0, 255],
            vec![
                Property {
                    kind: PropertyIdentifier::PropOffsets,
                    length: 8,
                    payload: PropertyPayload::OffsetsLayer(3, 1),
                },
                // The writer stores opacity as four big-endian bytes.
                Property {
                    kind: PropertyIdentifier::PropOpacity,
                    length: 4,
                    payload: PropertyPayload::OpacityLayer(RgbaPixel([0, 0, 0, 127])),
                },
                Property {
                    kind: PropertyIdentifier::PropVisible,
                    length: 4,
                    payload: PropertyPayload::VisibleLayer(),
                },
            ],
        ),
        layer("Background", 6, 4, [255, 0, 0, 255], vec![]),
    ];
    xcf.add_layers(&layers);
    let p = scratch("layered.xcf");
    xcf.save(&p).unwrap();

    let doc = open(&p).unwrap();
    assert_eq!((doc.width, doc.height), (6, 4));
    assert_eq!(doc.nodes.len(), 2);
    let names: Vec<&str> = doc.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["Background", "Sticker"], "bottom first");
    let sticker = &doc.nodes[1];
    assert!(sticker.visible);
    assert!((sticker.opacity - 127.0 / 255.0).abs() < 1e-3);
    assert!(doc.nodes[0].visible && doc.nodes[0].opacity == 1.0);
    let NodeKind::Raster { raster, placement } = &sticker.kind else {
        panic!("raster");
    };
    assert_eq!((raster.width(), raster.height()), (2, 2));
    assert_eq!((placement.x, placement.y), (3.0, 1.0));
}

#[test]
fn converter_backed_format_opens_when_imagemagick_is_installed() {
    if !emulsion_io::external::can_open(Path::new("x.pcx")) {
        eprintln!("skipped: no ImageMagick on PATH");
        return;
    }
    let png = scratch("pic.png");
    DynamicImage::ImageRgba8(picture())
        .save_with_format(&png, ImageFormat::Png)
        .unwrap();
    let pcx = scratch("pic.pcx");
    let ok = std::process::Command::new("magick")
        .arg(&png)
        .arg(&pcx)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("skipped: magick could not write PCX");
        return;
    }
    assert!(emulsion_io::is_openable(&pcx));
    assert_red_blue(&pcx);
}

#[test]
fn unknown_kind_gives_a_clear_error() {
    let p = scratch("mystery.zzz");
    std::fs::write(&p, b"nothing").unwrap();
    let err = open(&p).unwrap_err().to_string();
    assert!(
        err.contains("unsupported") || err.contains("no decoder") || err.contains("converted"),
        "{err}"
    );
    assert!(!emulsion_io::is_openable(&p));
}
