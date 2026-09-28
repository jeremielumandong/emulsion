//! Cancellable color-difference reconstruction for three-color CFA sensors.
use super::*;

pub(super) fn interpolate(raw: &RawImage, data: &[f32], cancel: &AtomicBool) -> Result<Vec<f32>> {
    let RawPhotometricInterpretation::Cfa(cfa) = &raw.photometric else {
        return Err(invalid("Expected a CFA sensor"));
    };
    let (w, h) = (raw.width, raw.height);
    let mut green = vec![0.; w * h];
    green
        .par_chunks_mut(w)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            cancelled(cancel)?;
            for (x, out) in row.iter_mut().enumerate() {
                if cfa.cfa.color_at(y, x) == 1 {
                    *out = data[y * w + x];
                    continue;
                }
                let center = data[y * w + x];
                let mut sum = 0.;
                let mut weight = 0.;
                for yy in y.saturating_sub(3)..=(y + 3).min(h - 1) {
                    for xx in x.saturating_sub(3)..=(x + 3).min(w - 1) {
                        if cfa.cfa.color_at(yy, xx) != 1 {
                            continue;
                        }
                        let distance =
                            (xx as f32 - x as f32).powi(2) + (yy as f32 - y as f32).powi(2);
                        // Prefer nearby samples on an axis with a small same-color gradient.
                        let mx = (2 * xx).saturating_sub(x).min(w - 1);
                        let my = (2 * yy).saturating_sub(y).min(h - 1);
                        let gradient = if cfa.cfa.color_at(my, mx) == cfa.cfa.color_at(y, x) {
                            (data[my * w + mx] - center).abs()
                        } else {
                            0.
                        };
                        let a = 1. / (distance.max(1.) * (0.01 + gradient));
                        sum += data[yy * w + xx] * a;
                        weight += a;
                    }
                }
                *out = if weight > 0. { sum / weight } else { center };
            }
            Ok(())
        })?;
    let mut rgb = vec![0.; w * h * 3];
    rgb.par_chunks_mut(w * 3)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            cancelled(cancel)?;
            for (x, p) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let g = green[y * w + x];
                p[1] = g;
                for c in [0, 2] {
                    if cfa.cfa.color_at(y, x) == c {
                        p[c] = data[y * w + x];
                        continue;
                    }
                    let mut sum = 0.;
                    let mut weight = 0.;
                    for yy in y.saturating_sub(3)..=(y + 3).min(h - 1) {
                        for xx in x.saturating_sub(3)..=(x + 3).min(w - 1) {
                            if cfa.cfa.color_at(yy, xx) != c {
                                continue;
                            }
                            let distance =
                                (xx as f32 - x as f32).powi(2) + (yy as f32 - y as f32).powi(2);
                            let a =
                                1. / (distance.max(1.) * (0.01 + (green[yy * w + xx] - g).abs()));
                            sum += (data[yy * w + xx] - green[yy * w + xx]) * a;
                            weight += a;
                        }
                    }
                    p[c] = (g + if weight > 0. { sum / weight } else { 0. }).max(0.);
                }
            }
            Ok(())
        })?;
    Ok(rgb)
}
