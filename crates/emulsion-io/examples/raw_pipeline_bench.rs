//! Read-only stage timings and exact output hashes for RAW performance passes.
use emulsion_core::raw::DevelopParams;
use emulsion_io::{photo_develop::PhotoSource, raw::source_digest};
use emulsion_raster::Raster;
use sha2::{Digest, Sha256};
use std::{hint::black_box, path::PathBuf, sync::atomic::AtomicBool, time::Instant};

fn raster_hash(raster: &Raster) -> String {
    let mut hash = Sha256::new();
    hash.update(raster.width().to_le_bytes());
    hash.update(raster.height().to_le_bytes());
    for pixel in raster.to_pixels() {
        for channel in pixel {
            hash.update(channel.to_le_bytes());
        }
    }
    hash.finalize().iter().map(|v| format!("{v:02x}")).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::args_os().nth(1).ok_or("supply a RAW path")?);
    let samples: usize = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "7".into())
        .parse()?;
    assert!(samples > 0);
    let digest = source_digest(&path)?;
    let start = Instant::now();
    let source = PhotoSource::load(&path)?;
    let load_ms = start.elapsed().as_secs_f64() * 1000.;
    let cancel = AtomicBool::new(false);
    let neutral = DevelopParams::default();
    let start = Instant::now();
    let preview = source.develop_preview(&neutral, &cancel)?;
    let cold_fit_ms = start.elapsed().as_secs_f64() * 1000.;
    let cold_hash = raster_hash(&preview);
    let preview_dimensions = [preview.width(), preview.height()];
    drop(preview);
    let cases = [
        (
            "exposure",
            DevelopParams {
                exposure: 0.7,
                ..neutral
            },
        ),
        (
            "white-balance",
            DevelopParams {
                temperature: 0.2,
                tint: -0.1,
                ..neutral
            },
        ),
        (
            "tone",
            DevelopParams {
                contrast: 0.2,
                highlights: -0.3,
                shadows: 0.2,
                ..neutral
            },
        ),
        (
            "color",
            DevelopParams {
                saturation: 0.2,
                vibrance: 0.1,
                ..neutral
            },
        ),
        (
            "rotation",
            DevelopParams {
                rotation: 1,
                ..neutral
            },
        ),
        ("neutral", neutral),
    ];
    let mut edits = Vec::new();
    for (name, params) in cases {
        let mut timings = Vec::new();
        let mut display_ms = Vec::new();
        let mut expected = None;
        for sample in 0..=samples {
            let start = Instant::now();
            let result = source.develop_preview(black_box(&params), &cancel)?;
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            let start = Instant::now();
            black_box(result.to_srgba8());
            let display = start.elapsed().as_secs_f64() * 1000.;
            let hash = raster_hash(&result);
            if let Some(expected) = &expected {
                assert_eq!(expected, &hash, "non-deterministic {name} output");
            } else {
                expected = Some(hash);
            }
            if sample > 0 {
                timings.push(elapsed);
                display_ms.push(display);
            }
        }
        edits.push(serde_json::json!({"case":name,"develop_ms":timings,"display_ms":display_ms,"pixel_sha256":expected}));
    }
    let start = Instant::now();
    let full = source.develop_with_cancel(&neutral, &cancel)?;
    let full_ms = start.elapsed().as_secs_f64() * 1000.;
    let full_hash = raster_hash(&full);
    let dimensions = [full.width(), full.height()];
    assert_eq!(digest, source_digest(&path)?, "original changed");
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "file":path,"source_sha256":digest,"samples":samples,"dimensions":dimensions,
            "preview_dimensions":preview_dimensions,"load_ms":load_ms,"cold_fit_ms":cold_fit_ms,
            "cold_pixel_sha256":cold_hash,"edits":edits,"full_develop_ms":full_ms,
            "full_pixel_sha256":full_hash,"original_unchanged":true,
            "scope":"CPU loading, sensor preview/edit development, display conversion and full development; OS file cache is warm; no GPU/FPS measurements"
        }))?
    );
    Ok(())
}
