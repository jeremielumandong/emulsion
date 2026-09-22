//! Develop a camera RAW to PNG, optionally with panel settings:
//! `cargo run --release -p emulsion-io --example raw -- shot.ARW out.png [ev temp tint highlights shadows]`
use emulsion_io::raw::{DevelopParams, RawSource};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::PathBuf::from(args.first().expect("raw file"));
    let out = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "target/raw.png".into());
    let num = |i: usize| {
        args.get(i)
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0)
    };
    let params = DevelopParams {
        exposure: num(2),
        temperature: num(3),
        tint: num(4),
        highlights: num(5),
        shadows: num(6),
        ..Default::default()
    };
    let t = std::time::Instant::now();
    let src = RawSource::load(&path).expect("decode");
    let decoded = t.elapsed();
    let raster = src.develop_with(&params).expect("develop");
    println!(
        "{} {} · {}×{} · wb {:?} · decode {:?} · develop {:?} · {params:?}",
        src.info.make,
        src.info.model,
        raster.width(),
        raster.height(),
        src.info.wb_coeffs,
        decoded,
        t.elapsed() - decoded
    );
    let img =
        image::RgbaImage::from_raw(raster.width(), raster.height(), raster.to_srgba8()).unwrap();
    let small = image::imageops::thumbnail(&img, 900, 900 * raster.height() / raster.width());
    small.save(&out).unwrap();
    println!("wrote {out}");
}
