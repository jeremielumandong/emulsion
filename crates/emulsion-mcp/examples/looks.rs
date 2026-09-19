//! Contact sheet of the camera presets on a photo:
//! `cargo run -p emulsion-mcp --example looks -- photo.jpg target/looks.png`
use emulsion_core::command::Slot;
use emulsion_core::{Document, Editor, Node};
use emulsion_raster::composite::flatten;
use emulsion_raster::{Placement, Raster, color};
use std::sync::Arc;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("photo");
    let out = args.next().unwrap_or_else(|| "target/looks.png".into());
    let img = image::open(&path).expect("open").to_rgba8();
    let img = image::imageops::thumbnail(&img, 360, 360 * img.height() / img.width());
    let (w, h) = img.dimensions();
    let px: Vec<[u16; 4]> = img
        .pixels()
        .map(|p| color::f_to_px(color::srgba8_to_premul(p.0)))
        .collect();
    let base = Arc::new(Raster::from_pixels(w, h, [0; 4], &px));
    let presets = emulsion_recipes::cameras::presets();
    let cols = 4u32;
    let rows = (presets.len() as u32 + 1).div_ceil(cols);
    let mut sheet =
        image::RgbaImage::from_pixel(cols * w, rows * h, image::Rgba([30, 30, 30, 255]));
    let mut cells: Vec<(String, Raster)> = vec![("original".into(), (*base).clone())];
    for r in &presets {
        let mut doc = Document::new(w, h);
        emulsion_core::Command::AddNode {
            node: Box::new(Node::raster(0, "Photo", base.clone(), Placement::default())),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let mut ed = Editor::new(doc, None);
        let compiled = emulsion_recipes::compile_sized(r, w, h).unwrap();
        emulsion_recipes::store::add_to(&mut ed, compiled, Slot::TOP).unwrap();
        let t = std::time::Instant::now();
        let flat = flatten(&ed.doc.composite_tree(), 0);
        println!("{:<24} {:?}", r.name, t.elapsed());
        cells.push((r.name.clone(), flat));
    }
    for (i, (_, r)) in cells.iter().enumerate() {
        let (cx, cy) = ((i as u32 % cols) * w, (i as u32 / cols) * h);
        let px: Vec<u8> = r
            .to_pixels()
            .into_iter()
            .flat_map(|p| color::premul_to_srgba8(color::px_to_f(p)))
            .collect();
        let cell = image::RgbaImage::from_raw(r.width(), r.height(), px).unwrap();
        image::imageops::overlay(&mut sheet, &cell, cx as i64, cy as i64);
    }
    sheet.save(&out).unwrap();
    println!("wrote {out}");
}
