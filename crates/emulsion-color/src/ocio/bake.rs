//! Baking a processor into a 3D LUT for interactive display. Exports use
//! the exact ops; the canvas uses the bake, which costs one tetrahedral
//! lookup per pixel whatever the transform.

use super::lut::Lut3d;
use super::ops::Processor;
use super::transform::Interpolation;

/// How input values are spread over the LUT's lattice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shaper {
    /// Inputs in `[0, 1]` map straight to the lattice (display-referred and
    /// log encodings).
    Identity,
    /// Scene-linear inputs: `lo..hi` stops (log₂) are spread evenly, with a
    /// straight segment from 0 to 2^`lo` taking one stop's share. Values
    /// above 2^`hi` clamp.
    Log2 { lo: f64, hi: f64 },
}

impl Shaper {
    fn toe(lo: f64, hi: f64) -> f64 {
        1. / (hi - lo + 1.)
    }

    /// Value → lattice coordinate in `[0, 1]`.
    pub fn forward(self, x: f64) -> f64 {
        match self {
            Shaper::Identity => x.clamp(0., 1.),
            Shaper::Log2 { lo, hi } => {
                let d = Self::toe(lo, hi);
                let knee = lo.exp2();
                if x <= 0. {
                    0.
                } else if x < knee {
                    d * x / knee
                } else {
                    (d + (1. - d) * (x.log2() - lo) / (hi - lo)).min(1.)
                }
            }
        }
    }

    /// Lattice coordinate → value.
    pub fn inverse(self, s: f64) -> f64 {
        match self {
            Shaper::Identity => s,
            Shaper::Log2 { lo, hi } => {
                let d = Self::toe(lo, hi);
                if s <= d {
                    s / d * lo.exp2()
                } else {
                    (lo + (s - d) / (1. - d) * (hi - lo)).exp2()
                }
            }
        }
    }
}

/// A processor sampled on a lattice, applied with tetrahedral
/// interpolation.
#[derive(Clone, Debug)]
pub struct BakedLut {
    shaper: Shaper,
    lut: Lut3d,
    /// The shaped lattice coordinate of each 8-bit code value.
    codes: Box<[f64; 256]>,
}

impl BakedLut {
    pub fn size(&self) -> usize {
        self.lut.size
    }

    pub fn shaper(&self) -> Shaper {
        self.shaper
    }

    /// Display RGB for an input colour.
    pub fn apply_rgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        let s = rgb.map(|v| self.shaper.forward(f64::from(v)));
        self.lut
            .apply(s, Interpolation::Tetrahedral)
            .map(|v| v as f32)
    }

    fn apply_codes(&self, rgb: [u8; 3]) -> [u8; 3] {
        let s = rgb.map(|v| self.codes[v as usize]);
        self.lut
            .apply(s, Interpolation::Tetrahedral)
            .map(|v| (v.clamp(0., 1.) * 255. + 0.5) as u8)
    }

    /// Interleaved RGBA8 in place; alpha untouched.
    pub fn apply_rgba8(&self, pixels: &mut [u8]) {
        for p in pixels.as_chunks_mut::<4>().0 {
            let out = self.apply_codes([p[0], p[1], p[2]]);
            p[..3].copy_from_slice(&out);
        }
    }

    /// Interleaved BGRA8 in place (the viewport's pixel order); alpha
    /// untouched.
    pub fn apply_bgra8(&self, pixels: &mut [u8]) {
        for p in pixels.as_chunks_mut::<4>().0 {
            let [r, g, b] = self.apply_codes([p[2], p[1], p[0]]);
            p[0] = b;
            p[1] = g;
            p[2] = r;
        }
    }
}

impl Processor {
    /// Sample this processor on a `size`³ lattice (2–129) through `shaper`.
    pub fn bake(&self, size: usize, shaper: Shaper) -> BakedLut {
        let size = size.clamp(2, 129);
        let last = (size - 1) as f64;
        let axis: Vec<f64> = (0..size).map(|i| shaper.inverse(i as f64 / last)).collect();
        let mut data = Vec::with_capacity(size.pow(3));
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let out = self.apply_f64([axis[r], axis[g], axis[b]]);
                    data.push(out.map(|v| if v.is_finite() { v as f32 } else { 0. }));
                }
            }
        }
        BakedLut {
            shaper,
            lut: Lut3d {
                size,
                domain_min: [0.; 3],
                domain_max: [1.; 3],
                data,
            },
            codes: Box::new(std::array::from_fn(|i| shaper.forward(i as f64 / 255.))),
        }
    }
}
