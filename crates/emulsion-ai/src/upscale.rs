//! Super-resolution with Swin2SR, tiled with overlap so any size fits in
//! memory and seams blend away.

use emulsion_raster::Raster;
use rayon::prelude::*;

use crate::jobs::Job;
use crate::models::{self, Task};
use crate::prep::Planes;
use crate::runner::{self, RunError};

/// Tile edge fed to the model and the overlap between neighbours, in
/// source pixels. Swin2SR wants multiples of its 8-pixel window.
const TILE: usize = 192;
const OVERLAP: usize = 16;

pub fn available() -> Option<&'static models::ModelSpec> {
    models::installed_for(Task::Upscale)
}

/// The scale factor of the installed upscaler.
pub fn factor() -> u32 {
    match available().map(|m| m.id) {
        Some(id) if id.contains("x2") => 2,
        _ => 4,
    }
}

/// `image` enlarged by the model's factor.
pub fn upscale(image: &Raster, job: &Job) -> Result<Raster, RunError> {
    let spec = available().ok_or_else(|| RunError::NotInstalled("an upscale model".into()))?;
    let model = runner::model(&models::file_path(spec, &spec.files[0]))?;
    let f = factor() as usize;
    job.set_stage("preparing");
    let src = Planes::from_raster(image);
    let (w, h) = (src.w, src.h);
    let flat = src.flattened(0.0);
    let step = TILE - 2 * OVERLAP;
    let cols = w.div_ceil(step).max(1);
    let rows = h.div_ceil(step).max(1);
    let (ow, oh) = (w * f, h * f);
    let mut acc = vec![[0.0f32; 4]; ow * oh];
    let total = (cols * rows) as f32;
    let mut done = 0.0;
    for ty in 0..rows {
        for tx in 0..cols {
            job.check()?;
            // Source tile with overlap, clamped to the image, then padded
            // to a multiple of 8 by edge repetition.
            let x0 = (tx * step).saturating_sub(OVERLAP).min(w.saturating_sub(1));
            let y0 = (ty * step).saturating_sub(OVERLAP).min(h.saturating_sub(1));
            let x1 = ((tx + 1) * step + OVERLAP).min(w);
            let y1 = ((ty + 1) * step + OVERLAP).min(h);
            let (tw, th) = (x1 - x0, y1 - y0);
            let (pw, ph) = (tw.div_ceil(8) * 8, th.div_ceil(8) * 8);
            let mut t = ndarray::Array4::<f32>::zeros((1, 3, ph, pw));
            for y in 0..ph {
                for x in 0..pw {
                    let p = flat.px[(y0 + y.min(th - 1)) * w + x0 + x.min(tw - 1)];
                    for c in 0..3 {
                        t[[0, c, y, x]] = p[c];
                    }
                }
            }
            job.set_stage(format!(
                "upscaling tile {}/{}",
                done as usize + 1,
                total as usize
            ));
            let out = model.run(&[("pixel_values", t.into_dyn())])?;
            let r = out
                .get("reconstruction")
                .ok_or_else(|| RunError::Shape(spec.id.into(), "no reconstruction".into()))?;
            let sh = r.shape();
            let (rh, rw) = (sh[2], sh[3]);
            // Weight fades over the overlap so tiles blend.
            let fade = (OVERLAP * f) as f32;
            for y in 0..(th * f).min(rh) {
                let sy = y0 * f + y;
                if sy >= oh {
                    continue;
                }
                let wy = ramp(y, th * f, fade, y0 == 0, y1 == h);
                for x in 0..(tw * f).min(rw) {
                    let sx = x0 * f + x;
                    if sx >= ow {
                        continue;
                    }
                    let wgt = wy * ramp(x, tw * f, fade, x0 == 0, x1 == w);
                    let o = &mut acc[sy * ow + sx];
                    for c in 0..3 {
                        o[c] += r[[0, c, y, x]].clamp(0.0, 1.0) * wgt;
                    }
                    o[3] += wgt;
                }
            }
            done += 1.0;
            job.progress(0.05 + 0.9 * done / total);
        }
    }
    // Normalise weights and bring the alpha along, enlarged bilinearly.
    let alpha = Planes {
        w,
        h,
        px: src.px.clone(),
    }
    .resized(ow, oh);
    let px: Vec<[f32; 4]> = acc
        .par_iter()
        .zip(alpha.px.par_iter())
        .map(|(o, a)| {
            let k = if o[3] > 0.0 { 1.0 / o[3] } else { 0.0 };
            [o[0] * k, o[1] * k, o[2] * k, a[3]]
        })
        .collect();
    job.progress(1.0);
    Ok(Planes { w: ow, h: oh, px }.to_raster())
}

/// Blend weight across a tile: 1 inside, easing to 0 over `fade` at edges
/// that touch another tile (image borders stay at 1).
fn ramp(i: usize, len: usize, fade: f32, at_start: bool, at_end: bool) -> f32 {
    let mut wgt = 1.0f32;
    if !at_start {
        wgt = wgt.min(((i as f32 + 0.5) / fade).clamp(0.02, 1.0));
    }
    if !at_end {
        wgt = wgt.min(((len - i) as f32 - 0.5) / fade).max(0.02);
    }
    wgt
}
