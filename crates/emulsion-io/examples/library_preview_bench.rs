//! CPU benchmark of the Library's old compositor roundtrip versus its actual
//! opaque display helper. Includes the dependency-free UI helper directly so
//! this measurement does not require a window or a second implementation.
#[path = "../../emulsion-ui/src/batch/preview_pixels.rs"]
mod preview_pixels;

use emulsion_core::{Command, Document, Editor, Node, command::Slot, raw::DevelopParams};
use emulsion_io::photo_develop::PhotoSource;
use emulsion_raster::{Placement, Raster, composite::flatten};
use std::{
    hint::black_box,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

fn legacy(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let source = Arc::new(Raster::from_srgba8(w, h, rgba));
    let mut doc = Document::new(w, h);
    Command::AddNode {
        node: Box::new(Node::raster(0, "Photo", source, Placement::default())),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    let ed = Editor::new(doc, None);
    let mut bytes = flatten(&ed.doc.composite_tree(), 0).to_srgba8();
    for p in bytes.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    bytes
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::args_os().nth(1).ok_or("supply RAW path")?);
    let source = PhotoSource::load(&path)?;
    let raster = source.develop_preview(&DevelopParams::default(), &AtomicBool::new(false))?;
    let image =
        image::RgbaImage::from_raw(raster.width(), raster.height(), raster.to_srgba8()).unwrap();
    let image = image::DynamicImage::ImageRgba8(image)
        .thumbnail(1100, 1100)
        .into_rgba8();
    let (w, h) = image.dimensions();
    let rgba = image.into_raw();
    let expected = legacy(w, h, &rgba);
    let mut old = Vec::new();
    let mut new = Vec::new();
    for iteration in 0..22 {
        // Alternate order and prepare owned buffers outside timed sections.
        for fast in if iteration % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let input = rgba.clone();
            let start = Instant::now();
            let result = if fast {
                preview_pixels::opaque_bgra(black_box(input)).unwrap()
            } else {
                legacy(w, h, black_box(&input))
            };
            let ms = start.elapsed().as_secs_f64() * 1000.;
            assert_eq!(result, expected, "display pixels changed");
            if iteration >= 2 {
                if fast {
                    new.push(ms);
                } else {
                    old.push(ms);
                }
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({"dimensions":[w,h],"legacy_ms":old,"direct_ms":new,
        "identical_display_pixels":true,"scope":"CPU display handoff only, no GPU/FPS"})
    );
    Ok(())
}
