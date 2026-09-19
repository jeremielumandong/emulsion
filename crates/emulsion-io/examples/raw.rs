//! Develop a camera RAW to PNG: `cargo run --release -p emulsion-io --example raw -- shot.ARW out.png`
fn main() {
    let mut args = std::env::args().skip(1);
    let path = std::path::PathBuf::from(args.next().expect("raw file"));
    let out = args.next().unwrap_or_else(|| "target/raw.png".into());
    let t = std::time::Instant::now();
    let (raster, info) = emulsion_io::raw::develop(&path).expect("develop");
    println!(
        "{} {} · {}×{} · wb {:?} · {:?}",
        info.make,
        info.model,
        raster.width(),
        raster.height(),
        info.wb_coeffs,
        t.elapsed()
    );
    let img =
        image::RgbaImage::from_raw(raster.width(), raster.height(), raster.to_srgba8()).unwrap();
    let small = image::imageops::thumbnail(&img, 1200, 1200 * raster.height() / raster.width());
    small.save(&out).unwrap();
    println!("wrote {out}");
}
