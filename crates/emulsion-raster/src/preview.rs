//! Deterministic CPU previews use the same stroke implementation as the canvas.
use crate::{
    Raster,
    paint::{Brush, DualBlend, Ink, Stroke},
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeSample {
    pub x: f32,
    pub y: f32,
    pub time_ms: f64,
    pub pressure: Option<f32>,
    pub tilt: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PreviewMode {
    /// Premultiplied linear RGBA, matching Ink::Color.
    Paint([f32; 4]),
    Smudge,
    Erase,
}

pub fn render_stroke(
    base: Arc<Raster>,
    brush: Brush,
    mode: PreviewMode,
    samples: &[StrokeSample],
    seed: u64,
) -> Raster {
    render_components(base, brush, None, mode, samples, seed)
}

pub fn render_dual_stroke(
    base: Arc<Raster>,
    primary: Brush,
    secondary: Brush,
    blend: DualBlend,
    mode: PreviewMode,
    samples: &[StrokeSample],
    seed: u64,
) -> Raster {
    render_components(base, primary, Some((secondary, blend)), mode, samples, seed)
}

fn render_components(
    base: Arc<Raster>,
    brush: Brush,
    secondary: Option<(Brush, DualBlend)>,
    mode: PreviewMode,
    samples: &[StrokeSample],
    seed: u64,
) -> Raster {
    let ink = match mode {
        PreviewMode::Paint(color) => Ink::Color(color),
        PreviewMode::Smudge => Ink::Smudge,
        PreviewMode::Erase => Ink::Erase,
    };
    let mut stroke = Stroke::new_with_persistent(base.clone(), brush, ink, None, None);
    stroke.set_seed(seed);
    if let Some((brush, mode)) = secondary {
        stroke.set_secondary(brush, mode);
    }
    for sample in samples {
        stroke.point_full(
            sample.x,
            sample.y,
            sample.pressure,
            sample.tilt,
            Some(sample.time_ms),
        );
    }
    stroke.finish();
    stroke.render_with_compositor(&base, None).0
}

/// A fixed, pressure-varying S curve for library thumbnails and Studio defaults.
pub fn sample_stroke(width: u32, height: u32) -> Vec<StrokeSample> {
    (0..65)
        .map(|i| {
            let t = i as f32 / 64.0;
            StrokeSample {
                x: width as f32 * (0.12 + 0.76 * t),
                y: height as f32 * (0.5 + 0.2 * (t * std::f32::consts::TAU).sin()),
                time_ms: i as f64 * 8.0,
                pressure: Some(0.25 + 0.75 * (t * std::f32::consts::PI).sin()),
                tilt: Some((20.0 * t, 10.0)),
            }
        })
        .collect()
}
