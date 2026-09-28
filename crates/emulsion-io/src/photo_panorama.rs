//! Planar panorama stitching with geometric registration and feathered overlaps.
use crate::{
    IoError, Result,
    photo_hdr::{FloatImage, Merge, Options, Report, Source},
    photo_registration as registration,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
fn bad(s: &str) -> IoError {
    IoError::Unsupported(s.into())
}
fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(bad("Panorama cancelled"))
    } else {
        Ok(())
    }
}
fn load(path: &std::path::Path, small: bool, cancel: &AtomicBool) -> Result<FloatImage> {
    check(cancel)?;
    let source = crate::photo_develop::PhotoSource::load(path)?;
    let mut params = crate::raw_settings::adjacent_settings(&source.source, &source.source_sha256)?;
    // Canvas registration and output use display-linear sRGB consistently.
    params.wide_gamut = false;
    let raster = if small {
        source.develop_preview(&params, cancel)?
    } else {
        source.develop_with_cancel(&params, cancel)?
    };
    let pixels = raster.to_pixels();
    if pixels.iter().any(|p| p[3] != u16::MAX) {
        return Err(bad("Panorama inputs must be opaque photographs"));
    }
    let image = FloatImage {
        width: raster.width(),
        height: raster.height(),
        pixels: pixels
            .iter()
            .map(|p| {
                [
                    p[0] as f32 / 65535.,
                    p[1] as f32 / 65535.,
                    p[2] as f32 / 65535.,
                ]
            })
            .collect(),
    };
    Ok(if small { image.resized(640) } else { image })
}
fn matrix(m: [f64; 9]) -> glam::DMat3 {
    glam::DMat3::from_cols_array(&m).transpose()
}
fn array(m: glam::DMat3) -> [f64; 9] {
    m.transpose().to_cols_array()
}
fn sample(image: &FloatImage, x: f64, y: f64) -> Option<[f32; 3]> {
    if !(0.0..=1.).contains(&x) || !(0.0..=1.).contains(&y) {
        return None;
    }
    let (x, y) = (x * (image.width - 1) as f64, y * (image.height - 1) as f64);
    let (ix, iy) = (x as u32, y as u32);
    let (fx, fy) = ((x - ix as f64) as f32, (y - iy as f64) as f32);
    let get = |x: u32, y: u32| image.pixels[(y * image.width + x) as usize];
    let (a, b, c, d) = (
        get(ix, iy),
        get((ix + 1).min(image.width - 1), iy),
        get(ix, (iy + 1).min(image.height - 1)),
        get(
            (ix + 1).min(image.width - 1),
            (iy + 1).min(image.height - 1),
        ),
    );
    Some(std::array::from_fn(|i| {
        (a[i] * (1. - fx) + b[i] * fx) * (1. - fy) + (c[i] * (1. - fx) + d[i] * fx) * fy
    }))
}
pub fn merge(paths: &[PathBuf], preview: bool, cancel: &AtomicBool) -> Result<Merge> {
    if !(2..=9).contains(&paths.len()) {
        return Err(bad(
            "Panorama needs 2–9 overlapping photos in capture order",
        ));
    }
    let mut sources = Vec::new();
    let mut thumbnails = Vec::new();
    for path in paths {
        check(cancel)?;
        sources.push(Source {
            path: path.clone(),
            sha256: crate::raw::source_digest(path)?,
            exposure_ev: 0.,
        });
        thumbnails.push(load(path, true, cancel)?);
    }
    let mut maps = vec![registration::IDENTITY];
    let mut gains = vec![1f32];
    for i in 1..paths.len() {
        let local = registration::register(&thumbnails[i - 1], &thumbnails[i], cancel)?;
        maps.push(array(matrix(local) * matrix(maps[i - 1])));
        let mut ratios = Vec::new();
        for y in 1..20 {
            for x in 1..20 {
                let p = [x as f64 / 20., y as f64 / 20.];
                if let Some(q) = registration::project(&local, p[0], p[1])
                    && let (Some(a), Some(b)) = (
                        sample(&thumbnails[i - 1], p[0], p[1]),
                        sample(&thumbnails[i], q[0], q[1]),
                    )
                {
                    let (a, b) = (a.iter().sum::<f32>(), b.iter().sum::<f32>());
                    if a > 0.03 && b > 0.03 && a < 2.7 && b < 2.7 {
                        ratios.push(a / b);
                    }
                }
            }
        }
        ratios.sort_by(f32::total_cmp);
        gains.push(
            gains[i - 1]
                * ratios
                    .get(ratios.len() / 2)
                    .copied()
                    .unwrap_or(1.)
                    .clamp(0.5, 2.),
        );
    }
    let (mut min, mut max) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for m in &maps {
        let mat = matrix(*m);
        if mat.determinant().abs() < 1e-8 {
            return Err(bad("Degenerate panorama alignment"));
        }
        let inv = array(mat.inverse());
        for p in [[0., 0.], [1., 0.], [0., 1.], [1., 1.]] {
            let q = registration::project(&inv, p[0], p[1])
                .ok_or_else(|| bad("Unbounded panorama projection"))?;
            for c in 0..2 {
                min[c] = min[c].min(q[c]);
                max[c] = max[c].max(q[c]);
            }
        }
    }
    let first = load(&paths[0], preview, cancel)?;
    let (w, h) = (
        ((max[0] - min[0]) * first.width as f64).ceil(),
        ((max[1] - min[1]) * first.height as f64).ceil(),
    );
    if !w.is_finite()
        || !h.is_finite()
        || w < 1.
        || h < 1.
        || w * h > crate::photo_hdr::MAX_PIXELS as f64
    {
        return Err(bad(
            "Panorama exceeds the 60 megapixel output limit or has unstable geometry",
        ));
    }
    let (w, h) = (w as u32, h as u32);
    let mut sum = vec![[0f32; 4]; w as usize * h as usize];
    let base = (first.width, first.height);
    drop(first);
    for (i, path) in paths.iter().enumerate() {
        let image = load(path, preview, cancel)?;
        for y in 0..h {
            check(cancel)?;
            for x in 0..w {
                let p = [
                    min[0] + x as f64 / base.0 as f64,
                    min[1] + y as f64 / base.1 as f64,
                ];
                let Some(q) = registration::project(&maps[i], p[0], p[1]) else {
                    continue;
                };
                let Some(rgb) = sample(&image, q[0], q[1]) else {
                    continue;
                };
                let weight =
                    (q[0].min(1. - q[0]).min(q[1]).min(1. - q[1]) * 20.).clamp(0.001, 1.) as f32;
                let dst = &mut sum[(y * w + x) as usize];
                for c in 0..3 {
                    dst[c] += rgb[c] * gains[i] * weight;
                }
                dst[3] += weight;
            }
        }
    }
    let pixels = sum
        .into_iter()
        .map(|p| std::array::from_fn(|c| if p[3] > 0. { p[c] / p[3] } else { 0. }))
        .collect();
    for s in &sources {
        check(cancel)?;
        if crate::raw::source_digest(&s.path)? != s.sha256 {
            return Err(bad("A panorama original changed during stitching"));
        }
    }
    Ok(Merge {
        image: FloatImage {
            width: w,
            height: h,
            pixels,
        },
        ghosts: vec![0; w as usize * h as usize],
        report: Report {
            panorama: true,
            sources,
            offsets: vec![[0, 0]; paths.len()],
            homographies: maps,
            reference: 0,
            deghosted_pixels: 0,
            options: Options {
                align: true,
                auto_tone: false,
                ..Default::default()
            },
            display_exposure: 0.,
            width: w,
            height: h,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlap_stitches_saves_reopens_and_preserves_originals() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<_> = (0..2)
            .map(|i| dir.path().join(format!("{i}.png")))
            .collect();
        for (i, path) in paths.iter().enumerate() {
            let image = image::RgbImage::from_fn(256, 192, |x, y| {
                let mut n =
                    (((x + i as u32 * 48) / 5) as u64 * 73856093) ^ ((y / 5) as u64 * 19349663);
                n ^= n >> 13;
                let v = (n % 190 + 30) as u8;
                image::Rgb([v, v, v])
            });
            image.save(path).unwrap();
        }
        let hashes: Vec<_> = paths
            .iter()
            .map(|p| crate::raw::source_digest(p).unwrap())
            .collect();
        let cancel = AtomicBool::new(false);
        let merged = merge(&paths, false, &cancel).unwrap();
        assert!(
            merged.image.width >= 298 && merged.image.width <= 312,
            "{}",
            merged.image.width
        );
        assert!(merged.image.height >= 190 && merged.image.height <= 200);
        assert_eq!(merged.report.homographies.len(), 2);
        let output = dir.path().join("panorama.tif");
        merged.save(&output, &cancel).unwrap();
        assert!(merged.save(&output, &cancel).is_err());
        let loaded = crate::photo_hdr::load(&output).unwrap().unwrap();
        assert_eq!(loaded.0.width, merged.image.width);
        for (path, hash) in paths.iter().zip(hashes) {
            assert_eq!(crate::raw::source_digest(path).unwrap(), hash);
        }
        assert!(merge(&paths, false, &AtomicBool::new(true)).is_err());
    }
}
