//! CPU Gaussian benchmark. Run with RAYON_NUM_THREADS=8 in release mode.
use emulsion_filters::{Filter, apply_pixels_cpu};
use std::{hint::black_box, time::Instant};

fn main() {
    let (width, height) = (6030, 4030);
    let pixels = (0..width * height)
        .map(|i| {
            let alpha = (i % 251) as f32 / 250.0;
            [alpha * 0.25, alpha * 0.5, alpha * 0.75, alpha]
        })
        .collect();
    let start = Instant::now();
    let output =
        apply_pixels_cpu(&Filter::GaussianBlur { radius: 5.0 }, width, height, pixels).unwrap();
    let elapsed = start.elapsed();
    let checksum = output.iter().flatten().fold(0u64, |hash, value| {
        hash.wrapping_mul(31).wrapping_add(value.to_bits() as u64)
    });
    black_box(&output);
    println!(
        "{{\"elapsed_ms\":{},\"checksum\":{checksum}}}",
        elapsed.as_secs_f64() * 1000.0
    );
}
