//! CPU reference measurements, not end-to-end display latency.
//! cargo run -p emulsion-io --example brush_benchmark --offline -- target/brush-validation
use emulsion_raster::{
    Raster,
    paint::{Brush, DualBlend, GrainKind, Ink, Stroke, textures},
    preview,
};
use std::{path::PathBuf, sync::Arc, time::Instant};
fn main() -> anyhow::Result<()> {
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "target/brush-validation".into());
    std::fs::create_dir_all(&output)?;
    // Original diamond source, generated here; no third-party asset dependency.
    let tip: Vec<u8> = (0u32..64)
        .flat_map(|y| {
            (0u32..64).map(move |x| {
                if (x as i32 - 32).abs() + (y as i32 - 32).abs() < 30 {
                    255
                } else {
                    0
                }
            })
        })
        .collect();
    textures::register(0xE001, textures::Texture::from_gray8(64, 64, &tip).unwrap());
    let dry = Brush {
        size: 64.,
        ..Brush::default()
    };
    let textured = Brush {
        grain: GrainKind::Paper,
        grain_strength: 0.7,
        ..dry
    };
    let image_tip = Brush {
        tip: 0xE001,
        spacing: 0.3,
        ..dry
    };
    let wet = Brush {
        wetness: 0.7,
        ..dry
    };
    let mut scatter = image_tip;
    scatter.advanced.path.lateral_jitter = 0.5;
    scatter.advanced.shape.rotation_jitter = 1.;
    scatter.advanced.shape.count = 3;
    let ink = Ink::Color([0.03, 0.12, 0.3, 1.]);
    let cases = [
        ("dry", dry, ink.clone(), None),
        ("grain", textured, ink.clone(), None),
        ("image-tip", image_tip, ink.clone(), None),
        ("scatter", scatter, ink.clone(), None),
        ("wet", wet, ink.clone(), None),
        ("smudge", wet, Ink::Smudge, None),
        ("erase", textured, Ink::Erase, None),
        ("dual", dry, ink.clone(), Some(textured)),
    ];
    let mut report = String::from(
        "# Brush CPU reference measurements\n\n4096 × 4096 canvas; 64 px brush; 65 timestamped pressure/tilt samples; three strokes per case. Update includes sampling and CPU compositing. No GPU or display/input latency measurement. Debug profile uses workspace optimization settings.\n\n|Case|p50 update ms|p95 update ms|p95 finish ms|\n|---|---:|---:|---:|\n",
    );
    for (name, brush, ink, secondary) in cases {
        let base = Arc::new(Raster::solid(4096, 4096, [0.7, 0.4, 0.2, 1.]));
        let samples = preview::sample_stroke(2048, 1024);
        let mut updates = vec![];
        let mut finishes = vec![];
        for seed in 1..=3 {
            let mut stroke =
                Stroke::new_with_persistent(base.clone(), brush, ink.clone(), None, None);
            stroke.set_seed(seed);
            if let Some(secondary) = secondary {
                stroke.set_secondary(secondary, DualBlend::Normal);
            }
            for sample in &samples {
                let now = Instant::now();
                stroke.point_full(
                    sample.x,
                    sample.y,
                    sample.pressure,
                    sample.tilt,
                    Some(sample.time_ms),
                );
                std::hint::black_box(stroke.render_with_compositor(&base, None));
                updates.push(now.elapsed().as_secs_f64() * 1000.);
            }
            let now = Instant::now();
            stroke.finish();
            std::hint::black_box(stroke.render_with_compositor(&base, None));
            finishes.push(now.elapsed().as_secs_f64() * 1000.);
        }
        updates.sort_by(f64::total_cmp);
        finishes.sort_by(f64::total_cmp);
        let row = format!(
            "|{name}|{:.3}|{:.3}|{:.3}|\n",
            updates[updates.len() / 2],
            updates[updates.len() * 95 / 100],
            finishes[finishes.len() - 1]
        );
        print!("{row}");
        report.push_str(&row);
        let samples = preview::sample_stroke(512, 192);
        let background: Vec<u8> = (0..192)
            .flat_map(|y| {
                (0..512).flat_map(move |x| {
                    if (x / 80 + y / 64) % 2 == 0 {
                        [230, 170, 80, 255]
                    } else {
                        [70, 150, 210, 255]
                    }
                })
            })
            .collect();
        let base = Arc::new(Raster::from_srgba8(512, 192, &background));
        let mode = match ink {
            Ink::Color(c) => preview::PreviewMode::Paint(c),
            Ink::Smudge => preview::PreviewMode::Smudge,
            _ => preview::PreviewMode::Erase,
        };
        let raster = if let Some(secondary) = secondary {
            preview::render_dual_stroke(
                base,
                brush,
                secondary,
                DualBlend::Normal,
                mode,
                &samples,
                1,
            )
        } else {
            preview::render_stroke(base, brush, mode, &samples, 1)
        };
        image::save_buffer(
            output.join(format!("{name}.png")),
            &raster.to_srgba8(),
            512,
            192,
            image::ColorType::Rgba8,
        )?;
    }
    std::fs::write(output.join("measurements.md"), report)?;
    Ok(())
}
