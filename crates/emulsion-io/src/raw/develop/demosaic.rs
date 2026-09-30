//! Cancellable color-difference reconstruction for three-color CFA sensors.
use super::*;

struct Neighbor {
    offset: isize,
    distance: f32,
    same_color: bool,
}

/// A CFA repeats. Interior pixels can reuse the ordered, color-filtered
/// neighborhood instead of inspecting all 49 positions for every channel.
struct Neighborhoods {
    width: usize,
    height: usize,
    phases: Vec<[Vec<Neighbor>; 3]>,
}

impl Neighborhoods {
    fn new(cfa: &rawler::CFA, stride: usize) -> Self {
        let mut phases = Vec::with_capacity(cfa.width * cfa.height);
        for y in 0..cfa.height {
            for x in 0..cfa.width {
                let mut channels: [Vec<Neighbor>; 3] = std::array::from_fn(|_| Vec::new());
                // Use positive coordinates in the repeated pattern.
                let (cy, cx) = (y + cfa.height * 6, x + cfa.width * 6);
                for dy in -3isize..=3 {
                    for dx in -3isize..=3 {
                        let color =
                            cfa.color_at(cy.wrapping_add_signed(dy), cx.wrapping_add_signed(dx));
                        if color < 3 {
                            channels[color].push(Neighbor {
                                offset: dy * stride as isize + dx,
                                distance: ((dx as f32).powi(2) + (dy as f32).powi(2)).max(1.),
                                same_color: cfa.color_at(
                                    cy.wrapping_add_signed(2 * dy),
                                    cx.wrapping_add_signed(2 * dx),
                                ) == cfa.color_at(cy, cx),
                            });
                        }
                    }
                }
                phases.push(channels);
            }
        }
        Self {
            width: cfa.width,
            height: cfa.height,
            phases,
        }
    }

    fn at(&self, x: usize, y: usize) -> &[Vec<Neighbor>; 3] {
        &self.phases[(y % self.height) * self.width + x % self.width]
    }
}

pub(super) fn interpolate(
    raw: &RawImage,
    data: &[f32],
    cancel: &AtomicBool,
) -> Result<Vec<[f32; 3]>> {
    interpolate_with(raw, data, cancel, true)
}

fn interpolate_with(
    raw: &RawImage,
    data: &[f32],
    cancel: &AtomicBool,
    planned: bool,
) -> Result<Vec<[f32; 3]>> {
    let RawPhotometricInterpretation::Cfa(cfa) = &raw.photometric else {
        return Err(invalid("Expected a CFA sensor"));
    };
    let (w, h) = (raw.width, raw.height);
    // Above f32's exact integer range, retain the original coordinate math.
    let neighborhoods = (planned
        && w <= 1 << 24
        && h <= 1 << 24
        && (1..=48).contains(&cfa.cfa.width)
        && (1..=48).contains(&cfa.cfa.height))
    .then(|| Neighborhoods::new(&cfa.cfa, w));
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
                if x >= 6
                    && x + 6 < w
                    && y >= 6
                    && y + 6 < h
                    && let Some(plan) = &neighborhoods
                {
                    let index = (y * w + x) as isize;
                    for n in &plan.at(x, y)[1] {
                        let gradient = if n.same_color {
                            (data[(index + 2 * n.offset) as usize] - center).abs()
                        } else {
                            0.
                        };
                        let a = 1. / (n.distance * (0.01 + gradient));
                        sum += data[(index + n.offset) as usize] * a;
                        weight += a;
                    }
                    *out = if weight > 0. { sum / weight } else { center };
                    continue;
                }
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
    let mut rgb = vec![[0.; 3]; w * h];
    rgb.par_chunks_mut(w)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            cancelled(cancel)?;
            for (x, p) in row.iter_mut().enumerate() {
                let g = green[y * w + x];
                p[1] = g;
                for c in [0, 2] {
                    if cfa.cfa.color_at(y, x) == c {
                        p[c] = data[y * w + x];
                        continue;
                    }
                    let mut sum = 0.;
                    let mut weight = 0.;
                    if x >= 6
                        && x + 6 < w
                        && y >= 6
                        && y + 6 < h
                        && let Some(plan) = &neighborhoods
                    {
                        let index = (y * w + x) as isize;
                        for n in &plan.at(x, y)[c] {
                            let neighbor = (index + n.offset) as usize;
                            let a = 1. / (n.distance * (0.01 + (green[neighbor] - g).abs()));
                            sum += (data[neighbor] - green[neighbor]) * a;
                            weight += a;
                        }
                        p[c] = (g + if weight > 0. { sum / weight } else { 0. }).max(0.);
                        continue;
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planned_neighborhoods_match_scalar_for_bayer_and_six_by_six_cfas() {
        let six: String = (0..36).map(|i| ['R', 'G', 'B'][(i + i / 6) % 3]).collect();
        for pattern in ["RGGB", "BGGR", "GRBG", "GBRG", six.as_str()] {
            for (w, h) in [(4, 5), (19, 17), (65, 63)] {
                let mut raw = super::super::tests::sensor();
                raw.width = w;
                raw.height = h;
                raw.cpp = 1;
                raw.photometric =
                    RawPhotometricInterpretation::Cfa(rawler::rawimage::CFAConfig::new(
                        &rawler::CFA::new(pattern),
                        &Default::default(),
                    ));
                let data: Vec<_> = (0..w * h)
                    .map(|i| ((i * 7919 + i / w * 31) % 65536) as f32 / 16000.)
                    .collect();
                let cancel = AtomicBool::new(false);
                let scalar = interpolate_with(&raw, &data, &cancel, false).unwrap();
                let planned = interpolate(&raw, &data, &cancel).unwrap();
                assert_eq!(planned, scalar, "{pattern} {w}x{h}");
            }
        }
    }
}
