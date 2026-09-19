//! Contact sheet of every brush preset: `cargo run -p emulsion-ai --example brushes -- target/brushes.png`
use emulsion_raster::paint::{Ink, Stroke};
use emulsion_raster::{Raster, color, library};
use std::sync::Arc;

const CW: u32 = 360;
const CH: u32 = 120;
const COLS: u32 = 4;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/brushes.png".into());
    let lib = library::library();
    let rows = (lib.len() as u32).div_ceil(COLS);
    let (w, h) = (CW * COLS, CH * rows);
    // Warm paper.
    let paper = color::f_to_px(color::srgba8_to_premul([246, 242, 232, 255]));
    let mut canvas = Raster::from_pixels(w, h, paper, &vec![paper; (w * h) as usize]);
    for (i, p) in lib.iter().enumerate() {
        let (cx, cy) = ((i as u32 % COLS * CW) as f32, (i as u32 / COLS * CH) as f32);
        println!("{:2} {:<12} {}", i, p.category, p.name);
        let mut brush = p.brush;
        brush.size = brush.size.clamp(6.0, 40.0);
        let base = Arc::new(canvas.clone());
        // First stroke: dark blue, S-curve with a pressure ramp.
        let mut s = Stroke::new(
            base.clone(),
            brush,
            Ink::Color(color::srgba8_to_premul([30, 50, 110, 255])),
            None,
        );
        let b = base.clone();
        s.set_backdrop(Arc::new(move |x, y| {
            if x < 0 || y < 0 || x >= b.width() as i32 || y >= b.height() as i32 {
                [0.0; 4]
            } else {
                color::px_to_f(b.get(x as u32, y as u32))
            }
        }));
        let n = 120;
        for k in 0..=n {
            let t = k as f32 / n as f32;
            let x = cx + 20.0 + t * (CW as f32 - 40.0);
            let y = cy + CH as f32 * 0.5 + (t * std::f32::consts::TAU).sin() * 28.0;
            let pressure = (t * std::f32::consts::PI).sin().max(0.05);
            s.point_at(x, y, Some(pressure), Some(t as f64 * 900.0));
        }
        s.finish();
        let (r, _) = s.render(&canvas);
        canvas = r;
        // Second stroke crossing it in red, to show mixing and grain.
        let base = Arc::new(canvas.clone());
        let mut s = Stroke::new(
            base.clone(),
            brush,
            Ink::Color(color::srgba8_to_premul([190, 40, 30, 255])),
            None,
        );
        let b = base.clone();
        s.set_backdrop(Arc::new(move |x, y| {
            if x < 0 || y < 0 || x >= b.width() as i32 || y >= b.height() as i32 {
                [0.0; 4]
            } else {
                color::px_to_f(b.get(x as u32, y as u32))
            }
        }));
        for k in 0..=n {
            let t = k as f32 / n as f32;
            let x = cx + 40.0 + t * (CW as f32 - 80.0);
            let y = cy + CH as f32 * 0.75 - t * CH as f32 * 0.5;
            s.point_at(x, y, Some(0.6), Some(t as f64 * 600.0));
        }
        s.finish();
        let (r, _) = s.render(&canvas);
        canvas = r;
    }
    let px: Vec<u8> = canvas
        .to_pixels()
        .into_iter()
        .flat_map(|p| color::premul_to_srgba8(color::px_to_f(p)))
        .collect();
    image::RgbaImage::from_raw(w, h, px)
        .unwrap()
        .save(&out)
        .unwrap();
    println!("wrote {out} ({w}×{h})");
}
