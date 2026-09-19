//! Detect and restore faces: `cargo run -p emulsion-ai --example faces -- photo.png out.png`
use emulsion_ai::{face, jobs::Job};
use emulsion_raster::{Raster, color};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("photo");
    let out = args.next().unwrap_or_else(|| "target/faces.png".into());
    let img = image::open(&path).expect("open").to_rgba8();
    let (w, h) = img.dimensions();
    let px: Vec<[u16; 4]> = img
        .pixels()
        .map(|p| color::f_to_px(color::srgba8_to_premul(p.0)))
        .collect();
    let raster = Raster::from_pixels(w, h, [0; 4], &px);
    let job = Job::new();
    let t = Instant::now();
    match face::detect(&raster, &job) {
        Ok(faces) => {
            println!("detect: {:?} → {} faces", t.elapsed(), faces.len());
            for f in &faces {
                println!(
                    "  {:.0},{:.0}–{:.0},{:.0} score {:.2} eyes {:?} {:?}",
                    f.x0, f.y0, f.x1, f.y1, f.score, f.landmarks[0], f.landmarks[1]
                );
            }
        }
        Err(e) => println!("detect failed: {e}"),
    }
    let t = Instant::now();
    match face::restore(&raster, 1.0, &job) {
        Ok((r, n)) => {
            println!("restore: {:?} ({n} faces)", t.elapsed());
            let px: Vec<u8> = r
                .to_pixels()
                .into_iter()
                .flat_map(|p| color::premul_to_srgba8(color::px_to_f(p)))
                .collect();
            image::RgbaImage::from_raw(w, h, px)
                .unwrap()
                .save(&out)
                .unwrap();
            println!("wrote {out}");
        }
        Err(e) => println!("restore failed: {e}"),
    }
}
