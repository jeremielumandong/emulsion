//! Try the matte and SAM models on a photo:
//! `cargo run --release -p emulsion-ai --example segment -- photo.jpg out_prefix [x y]`
use emulsion_ai::{jobs::Job, matte, sam};
use emulsion_raster::color;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("photo");
    let out = args.next().unwrap_or_else(|| "target/seg".into());
    let img = image::open(&path).expect("open").to_rgba8();
    let (w, h) = img.dimensions();
    let px: Vec<[u16; 4]> = img
        .pixels()
        .map(|p| color::f_to_px(color::srgba8_to_premul(p.0)))
        .collect();
    let raster = emulsion_raster::Raster::from_pixels(w, h, [0; 4], &px);
    let job = Job::new();
    let t = Instant::now();
    match matte::matte(&raster, &Default::default(), &job) {
        Ok(m) => {
            println!("matte: {:?}", t.elapsed());
            save_mask(&m, &format!("{out}-matte.png"));
            let cut = matte::cut_out(&raster, &m);
            save_raster(&cut, &format!("{out}-cut.png"));
        }
        Err(e) => println!("matte failed: {e}"),
    }
    let (x, y) = match (args.next(), args.next()) {
        (Some(x), Some(y)) => (x.parse().unwrap(), y.parse().unwrap()),
        _ => (w as f32 / 2.0, h as f32 / 2.0),
    };
    let t = Instant::now();
    match sam::encode(&raster, &job) {
        Ok(emb) => {
            println!("sam encode: {:?}", t.elapsed());
            let t = Instant::now();
            let (m, score) = sam::decode(
                &emb,
                &[sam::Point {
                    x,
                    y,
                    positive: true,
                }],
                None,
            )
            .unwrap();
            println!("sam decode: {:?} score {score:.2}", t.elapsed());
            save_mask(&m, &format!("{out}-sam.png"));
        }
        Err(e) => println!("sam failed: {e}"),
    }
}

fn save_mask(m: &emulsion_raster::Mask, p: &str) {
    image::GrayImage::from_raw(m.width(), m.height(), m.to_pixels())
        .unwrap()
        .save(p)
        .unwrap();
    println!("wrote {p}");
}

fn save_raster(r: &emulsion_raster::Raster, p: &str) {
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
