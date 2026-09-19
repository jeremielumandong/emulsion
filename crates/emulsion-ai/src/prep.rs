//! Image ↔ tensor plumbing shared by the tasks: sRGB float planes, resizing,
//! normalisation and mask resampling.

use emulsion_raster::{Mask, Raster, color};
use ndarray::{Array4, ArrayD};
use rayon::prelude::*;

/// An image as interleaved sRGB f32 in 0–1, unpremultiplied, with alpha.
pub struct Planes {
    pub w: usize,
    pub h: usize,
    /// rgba per pixel.
    pub px: Vec<[f32; 4]>,
}

impl Planes {
    pub fn from_raster(r: &Raster) -> Planes {
        let (w, h) = (r.width() as usize, r.height() as usize);
        let src = r.to_pixels();
        let px: Vec<[f32; 4]> = src
            .par_iter()
            .map(|p| {
                let c = color::premul_to_srgba8(color::px_to_f(*p));
                [
                    c[0] as f32 / 255.0,
                    c[1] as f32 / 255.0,
                    c[2] as f32 / 255.0,
                    c[3] as f32 / 255.0,
                ]
            })
            .collect();
        Planes { w, h, px }
    }

    /// Flatten transparency onto `bg` (0–1 grey) so models see a solid image.
    pub fn flattened(&self, bg: f32) -> Planes {
        Planes {
            w: self.w,
            h: self.h,
            px: self
                .px
                .iter()
                .map(|p| {
                    let a = p[3];
                    [
                        p[0] * a + bg * (1.0 - a),
                        p[1] * a + bg * (1.0 - a),
                        p[2] * a + bg * (1.0 - a),
                        1.0,
                    ]
                })
                .collect(),
        }
    }

    /// Bilinear resample to `nw × nh`.
    pub fn resized(&self, nw: usize, nh: usize) -> Planes {
        if nw == self.w && nh == self.h {
            return Planes {
                w: self.w,
                h: self.h,
                px: self.px.clone(),
            };
        }
        let px: Vec<[f32; 4]> = (0..nh)
            .into_par_iter()
            .flat_map_iter(|y| {
                let sy = ((y as f32 + 0.5) * self.h as f32 / nh as f32 - 0.5).max(0.0);
                (0..nw).map(move |x| {
                    let sx = ((x as f32 + 0.5) * self.w as f32 / nw as f32 - 0.5).max(0.0);
                    self.sample(sx, sy)
                })
            })
            .collect();
        Planes { w: nw, h: nh, px }
    }

    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let x0 = (x.floor() as usize).min(self.w - 1);
        let y0 = (y.floor() as usize).min(self.h - 1);
        let x1 = (x0 + 1).min(self.w - 1);
        let y1 = (y0 + 1).min(self.h - 1);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let p = |x: usize, y: usize| self.px[y * self.w + x];
        let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
        let mut o = [0.0; 4];
        for i in 0..4 {
            let top = a[i] + (b[i] - a[i]) * fx;
            let bot = c[i] + (d[i] - c[i]) * fx;
            o[i] = top + (bot - top) * fy;
        }
        o
    }

    /// NCHW tensor `[1, 3, h, w]` with per-channel `(x - mean) / std`.
    pub fn to_nchw(&self, mean: [f32; 3], std: [f32; 3]) -> ArrayD<f32> {
        let mut t = Array4::<f32>::zeros((1, 3, self.h, self.w));
        for y in 0..self.h {
            for x in 0..self.w {
                let p = self.px[y * self.w + x];
                for c in 0..3 {
                    t[[0, c, y, x]] = (p[c] - mean[c]) / std[c];
                }
            }
        }
        t.into_dyn()
    }

    /// Back to a premultiplied raster.
    pub fn to_raster(&self) -> Raster {
        let px: Vec<[u16; 4]> = self
            .px
            .par_iter()
            .map(|p| {
                let c = [
                    (p[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (p[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (p[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                    (p[3].clamp(0.0, 1.0) * 255.0).round() as u8,
                ];
                color::f_to_px(color::srgba8_to_premul(c))
            })
            .collect();
        Raster::from_pixels(self.w as u32, self.h as u32, [0; 4], &px)
    }
}

/// A single-channel float map, 0–1.
pub struct Map {
    pub w: usize,
    pub h: usize,
    pub v: Vec<f32>,
}

impl Map {
    /// From a `[.., h, w]` tensor's last two axes.
    pub fn from_hw(t: &ArrayD<f32>) -> Map {
        let sh = t.shape();
        let (h, w) = (sh[sh.len() - 2], sh[sh.len() - 1]);
        let v: Vec<f32> = t.iter().take(h * w).copied().collect();
        Map { w, h, v }
    }

    /// Stretch to 0–1 by min and max.
    pub fn normalized(mut self) -> Map {
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for &x in &self.v {
            lo = lo.min(x);
            hi = hi.max(x);
        }
        let span = (hi - lo).max(1e-6);
        for x in &mut self.v {
            *x = (*x - lo) / span;
        }
        self
    }

    pub fn sigmoid(mut self) -> Map {
        for x in &mut self.v {
            *x = 1.0 / (1.0 + (-*x).exp());
        }
        self
    }

    /// Bilinear resample.
    pub fn resized(&self, nw: usize, nh: usize) -> Map {
        if nw == self.w && nh == self.h {
            return Map {
                w: nw,
                h: nh,
                v: self.v.clone(),
            };
        }
        let v: Vec<f32> = (0..nh)
            .into_par_iter()
            .flat_map_iter(|y| {
                let sy = ((y as f32 + 0.5) * self.h as f32 / nh as f32 - 0.5).max(0.0);
                (0..nw).map(move |x| {
                    let sx = ((x as f32 + 0.5) * self.w as f32 / nw as f32 - 0.5).max(0.0);
                    let x0 = (sx.floor() as usize).min(self.w - 1);
                    let y0 = (sy.floor() as usize).min(self.h - 1);
                    let x1 = (x0 + 1).min(self.w - 1);
                    let y1 = (y0 + 1).min(self.h - 1);
                    let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
                    let g = |x: usize, y: usize| self.v[y * self.w + x];
                    let top = g(x0, y0) + (g(x1, y0) - g(x0, y0)) * fx;
                    let bot = g(x0, y1) + (g(x1, y1) - g(x0, y1)) * fx;
                    top + (bot - top) * fy
                })
            })
            .collect();
        Map { w: nw, h: nh, v }
    }

    pub fn to_mask(&self) -> Mask {
        let px: Vec<u8> = self
            .v
            .iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        Mask::from_pixels(self.w as u32, self.h as u32, 0, &px)
    }

    /// A grey raster (for depth layers).
    pub fn to_grey_raster(&self) -> Raster {
        let px: Vec<[u16; 4]> = self
            .v
            .par_iter()
            .map(|v| {
                let g = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                color::f_to_px(color::srgba8_to_premul([g, g, g, 255]))
            })
            .collect();
        Raster::from_pixels(self.w as u32, self.h as u32, [0; 4], &px)
    }
}

/// Guided filter (He et al.) with the image as guide: snaps a soft matte to
/// real edges and smooths its interior. `radius` in pixels, `eps` in 0–1².
pub fn guided_filter(guide: &Planes, p: &Map, radius: usize, eps: f32) -> Map {
    assert_eq!((guide.w, guide.h), (p.w, p.h));
    let (w, h) = (p.w, p.h);
    let grey: Vec<f32> = guide
        .px
        .iter()
        .map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2])
        .collect();
    let mean = |v: &[f32]| box_blur(v, w, h, radius);
    let mean_i = mean(&grey);
    let mean_p = mean(&p.v);
    let ip: Vec<f32> = grey.iter().zip(&p.v).map(|(a, b)| a * b).collect();
    let ii: Vec<f32> = grey.iter().map(|a| a * a).collect();
    let mean_ip = mean(&ip);
    let mean_ii = mean(&ii);
    let mut a = vec![0.0f32; w * h];
    let mut b = vec![0.0f32; w * h];
    for i in 0..w * h {
        let cov = mean_ip[i] - mean_i[i] * mean_p[i];
        let var = mean_ii[i] - mean_i[i] * mean_i[i];
        a[i] = cov / (var + eps);
        b[i] = mean_p[i] - a[i] * mean_i[i];
    }
    let mean_a = mean(&a);
    let mean_b = mean(&b);
    let v: Vec<f32> = (0..w * h)
        .map(|i| (mean_a[i] * grey[i] + mean_b[i]).clamp(0.0, 1.0))
        .collect();
    Map { w, h, v }
}

/// Mean over a (2r+1)² window, edge-clamped, via summed-area table.
fn box_blur(v: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut sat = vec![0.0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0.0f64;
        for x in 0..w {
            row += v[y * w + x] as f64;
            sat[(y + 1) * (w + 1) + x + 1] = sat[y * (w + 1) + x + 1] + row;
        }
    }
    (0..h)
        .into_par_iter()
        .flat_map_iter(|y| {
            let y0 = y.saturating_sub(r);
            let y1 = (y + r + 1).min(h);
            let sat = &sat;
            (0..w).map(move |x| {
                let x0 = x.saturating_sub(r);
                let x1 = (x + r + 1).min(w);
                let s = sat[y1 * (w + 1) + x1] - sat[y0 * (w + 1) + x1] - sat[y1 * (w + 1) + x0]
                    + sat[y0 * (w + 1) + x0];
                (s / ((y1 - y0) * (x1 - x0)) as f64) as f32
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planes_round_trip_and_resize() {
        let r = Raster::from_fn(8, 4, [0; 4], |x, _| {
            let c = if x < 4 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            };
            color::f_to_px(color::srgba8_to_premul(c))
        });
        let p = Planes::from_raster(&r);
        assert_eq!(p.px[0][0], 1.0);
        let small = p.resized(4, 2);
        assert_eq!((small.w, small.h), (4, 2));
        assert!(small.px[0][0] > 0.9 && small.px[3][2] > 0.9);
        let t = p.to_nchw([0.5; 3], [1.0; 3]);
        assert_eq!(t.shape(), &[1, 3, 4, 8]);
        assert!((t[[0, 0, 0, 0]] - 0.5).abs() < 1e-6);
        let back = p.to_raster();
        assert_eq!(back.get(7, 3), r.get(7, 3));
    }

    #[test]
    fn guided_filter_snaps_to_edges() {
        // Guide: hard vertical edge; matte: blurry version of it.
        let (w, h) = (32, 16);
        let guide = Planes {
            w,
            h,
            px: (0..w * h)
                .map(|i| {
                    if i % w < 16 {
                        [0.1, 0.1, 0.1, 1.0]
                    } else {
                        [0.9, 0.9, 0.9, 1.0]
                    }
                })
                .collect(),
        };
        let soft = Map {
            w,
            h,
            v: (0..w * h)
                .map(|i| (((i % w) as f32 - 12.0) / 8.0).clamp(0.0, 1.0))
                .collect(),
        };
        let out = guided_filter(&guide, &soft, 4, 1e-3);
        // Column 15 (dark side) drops and column 16 (bright side) rises.
        let row = |x: usize| out.v[8 * w + x];
        assert!(
            row(15) < soft.v[8 * w + 15],
            "{} vs {}",
            row(15),
            soft.v[8 * w + 15]
        );
        assert!(
            row(16) > soft.v[8 * w + 16],
            "{} vs {}",
            row(16),
            soft.v[8 * w + 16]
        );
        assert!(row(2) < 0.1 && row(29) > 0.9);
    }
}
