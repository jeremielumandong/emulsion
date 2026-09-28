//! Read-only RAW workflow benchmark; no sidecar, catalog or original writes.
use std::{path::PathBuf, sync::atomic::AtomicBool, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("supply an original RAW path")?,
    );
    let digest = emulsion_io::raw::source_digest(&file)?;
    let start = Instant::now();
    let source = emulsion_io::raw::RawSource::load(&file)?;
    let open_ms = start.elapsed().as_secs_f64() * 1000.;
    let p = emulsion_core::raw::DevelopParams::default();
    let start = Instant::now();
    let full = source.develop_with(&p)?;
    let full_ms = start.elapsed().as_secs_f64() * 1000.;
    let dimensions = [full.width(), full.height()];
    drop(full);
    let start = Instant::now();
    let preview = source.develop_preview(&p, &AtomicBool::new(false))?;
    let cold_ms = start.elapsed().as_secs_f64() * 1000.;
    let preview_dimensions = [preview.width(), preview.height()];
    let neutral = preview.to_pixels();
    drop(preview);
    let mut hot_ms = vec![];
    let mut distinct = true;
    for i in 0..8 {
        let start = Instant::now();
        let out = source.develop_preview(
            &emulsion_core::raw::DevelopParams {
                exposure: 0.1 * (i + 1) as f32,
                ..p
            },
            &AtomicBool::new(false),
        )?;
        hot_ms.push(start.elapsed().as_secs_f64() * 1000.);
        distinct &= out.to_pixels() != neutral;
    }
    let rss = std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("VmRSS:") || l.starts_with("VmHWM:"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert!(distinct);
    assert_eq!(digest, emulsion_io::raw::source_digest(&file)?);
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"file":file,"source_sha256":digest,"original_unchanged":true,"dimensions":dimensions,"preview_dimensions":preview_dimensions,"decode_ms":open_ms,"full_develop_ms":full_ms,"cold_fit_ms":cold_ms,"cached_edit_ms":hot_ms,"memory":rss,"preview_byte_budget":128*1024*1024})
        )?
    );
    Ok(())
}
