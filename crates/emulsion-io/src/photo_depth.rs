//! Nondestructive relative-depth blur. Maps use the existing portable mask store.
use crate::{IoError, Result};
use emulsion_core::raw::DevelopParams;
use emulsion_raster::{Mask, Raster};
use std::sync::atomic::{AtomicBool, Ordering};

pub fn apply(image: Raster, p: &DevelopParams, cancel: &AtomicBool) -> Result<Raster> {
    if p.depth_blur == 0. {
        return Ok(image);
    }
    let map = crate::photo_develop::load_mask(
        &p.depth_map
            .ok_or_else(|| IoError::Manifest("Missing depth map".into()))?,
    )?;
    apply_map(
        image,
        &map,
        p.depth_blur,
        p.depth_focus,
        p.depth_range,
        cancel,
    )
}

fn apply_map(
    image: Raster,
    map: &Mask,
    amount: f32,
    focus: f32,
    range: f32,
    cancel: &AtomicBool,
) -> Result<Raster> {
    let check = || {
        if cancel.load(Ordering::Relaxed) {
            Err(IoError::Unsupported("Depth blur cancelled".into()))
        } else {
            Ok(())
        }
    };
    check()?;
    let (w, h) = (image.width(), image.height());
    let original = image.to_pixels();
    let rgba = original
        .iter()
        .map(|p| p.map(|v| v as f32 / 65535.))
        .collect();
    let blurred = emulsion_filters::apply_pixels_cpu(
        &emulsion_filters::Filter::GaussianBlur {
            radius: (amount * w.min(h) as f32).min(256.),
        },
        w as usize,
        h as usize,
        rgba,
    )
    .ok_or_else(|| IoError::Unsupported("Depth blur failed".into()))?;
    let mut output = original.clone();
    for y in 0..h {
        check()?;
        for x in 0..w {
            let depth = map.get(
                (x as u64 * map.width() as u64 / w as u64) as u32,
                (y as u64 * map.height() as u64 / h as u64) as u32,
            ) as f32
                / 255.;
            let t = ((depth - focus).abs() - range).max(0.) / (1. - range).max(0.001);
            let weight = (t * 4.).clamp(0., 1.);
            let weight = weight * weight * (3. - 2. * weight);
            let i = (y * w + x) as usize;
            for c in 0..3 {
                let sharp = original[i][c] as f32;
                let blur = blurred[i][c] / blurred[i][3].max(1e-6) * original[i][3] as f32;
                output[i][c] = (sharp * (1. - weight) + blur * weight)
                    .round()
                    .clamp(0., original[i][3] as f32) as u16;
            }
        }
    }
    Ok(Raster::from_pixels(w, h, [0; 4], &output))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focus_is_editable_and_alpha_is_preserved() {
        let pixels: Vec<_> = (0..1024)
            .map(|i| {
                if i % 2 == 0 {
                    [0, 0, 0, 65535]
                } else {
                    [65535; 4]
                }
            })
            .collect();
        let image = Raster::from_pixels(32, 32, [0; 4], &pixels);
        let map = Mask::from_pixels(32, 32, 0, &vec![255; 1024]);
        let cancel = AtomicBool::new(false);
        let focused = apply_map(image.clone(), &map, 0.05, 1., 0.1, &cancel).unwrap();
        assert_eq!(focused.to_pixels(), pixels);
        let blurred = apply_map(image.clone(), &map, 0.05, 0., 0.1, &cancel).unwrap();
        assert!(blurred.get(16, 16)[0] > 1000);
        assert_eq!(blurred.get(16, 16)[3], 65535);
        assert!(apply_map(image, &map, 0.05, 0., 0.1, &AtomicBool::new(true)).is_err());
    }
}
