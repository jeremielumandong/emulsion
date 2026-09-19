//! Try depth, upscale and fill: `cargo run -p emulsion-ai --example enhance -- photo.png out_prefix`
use emulsion_ai::{depth, inpaint, jobs::Job, upscale};
use emulsion_raster::{IRect, Mask, Raster, color};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("photo");
    let out = args.next().unwrap_or_else(|| "target/enh".into());
    let img = image::open(&path).expect("open").to_rgba8();
    let (w, h) = img.dimensions();
    let px: Vec<[u16; 4]> = img
        .pixels()
        .map(|p| color::f_to_px(color::srgba8_to_premul(p.0)))
        .collect();
    let raster = Raster::from_pixels(w, h, [0; 4], &px);
    let job = Job::new();
    let t = Instant::now();
    match depth::estimate(&raster, &job) {
        Ok(m) => {
            println!("depth: {:?}", t.elapsed());
            save(&m.to_grey_raster(), &format!("{out}-depth.png"));
        }
        Err(e) => println!("depth failed: {e}"),
    }
    // Fill a rectangle in the middle.
    let hole = Mask::from_fn(w, h, 0, |x, y| {
        let r = IRect::new(w as i32 / 2 - 60, h as i32 / 2 - 40, 120, 80);
        let (xi, yi) = (x as i32, y as i32);
        if xi >= r.x && yi >= r.y && xi < r.right() && yi < r.bottom() {
            255
        } else {
            0
        }
    });
    let t = Instant::now();
    match inpaint::fill(&raster, &hole, &job) {
        Ok((filled, rect)) => {
            println!("inpaint: {:?} region {rect:?}", t.elapsed());
            let px = filled.to_pixels();
            let merged = raster.write_rect(rect, &px);
            save(&merged, &format!("{out}-fill.png"));
        }
        Err(e) => println!("inpaint failed: {e}"),
    }
    // Upscale a 200×150 crop so it stays quick.
    let crop = IRect::new(w as i32 / 2 - 100, h as i32 / 2 - 75, 200, 150);
    let small = Raster::from_pixels(200, 150, [0; 4], &raster.read_rect(crop));
    let t = Instant::now();
    match upscale::upscale(&small, &job) {
        Ok(big) => {
            println!(
                "upscale ×{}: {:?} -> {}×{}",
                upscale::factor(),
                t.elapsed(),
                big.width(),
                big.height()
            );
            save(&small, &format!("{out}-crop.png"));
            save(&big, &format!("{out}-up.png"));
        }
        Err(e) => println!("upscale failed: {e}"),
    }
}

fn save(r: &Raster, p: &str) {
    let px: Vec<u8> = r
        .to_pixels()
        .into_iter()
        .flat_map(|p| color::premul_to_srgba8(color::px_to_f(p)))
        .collect();
    image::RgbaImage::from_raw(r.width(), r.height(), px)
        .unwrap()
        .save(p)
        .unwrap();
    println!("wrote {p}");
}
