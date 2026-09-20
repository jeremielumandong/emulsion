//! How much smaller PNG layers get at higher compression, and what it costs:
//! `cargo run --release -p emulsion-io --example pngsize -- photo.ARW`
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("image path");
    let (raster, _) = emulsion_io::raw::develop(std::path::Path::new(&path)).expect("develop");
    let (w, h) = (raster.width(), raster.height());
    let rgba8 = raster.to_srgba8();
    let rgba16 = raster.to_srgba16();
    let bytes16: Vec<u8> = rgba16.iter().flat_map(|v| v.to_ne_bytes()).collect();
    println!("{w}×{h}");
    for (name, ct) in [
        ("fast", CompressionType::Fast),
        ("default", CompressionType::Default),
        ("best", CompressionType::Best),
    ] {
        for (depth, data, color) in [
            (8, &rgba8, ExtendedColorType::Rgba8),
            (16, &bytes16, ExtendedColorType::Rgba16),
        ] {
            let t = Instant::now();
            let mut out = Vec::new();
            PngEncoder::new_with_quality(&mut out, ct, FilterType::Adaptive)
                .write_image(data, w, h, color)
                .unwrap();
            println!(
                "{name:<8} {depth:>2}-bit  {:>7.1} MB  {:>7.0} ms",
                out.len() as f64 / 1e6,
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}
