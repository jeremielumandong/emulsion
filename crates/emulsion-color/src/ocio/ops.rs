//! The compiled form of a transform: a flat list of operations evaluated per
//! pixel in double precision. Every op can say what its inverse is, so a
//! transform used in the inverse direction is compiled forward and then
//! reversed op by op.

use super::aces::{Fixed, Spline};
use super::lut::{Lut1d, Lut3d};
use super::transform::{Interpolation, LogParams, NegativeStyle};
use super::{Error, Result};
use moxcms::{Matrix3d, Vector3d};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) enum Op {
    /// `m · rgb + offset`.
    Matrix {
        m: Matrix3d,
        offset: [f64; 3],
    },
    /// Power curves. `offset: None` is a pure power (`x^gamma` forward);
    /// otherwise the sRGB-style "moncurve" with a linear toe, whose forward
    /// direction decodes.
    Gamma {
        gamma: [f64; 3],
        offset: Option<[f64; 3]>,
        negative: NegativeStyle,
        inverse: bool,
    },
    /// Forward is linear → log.
    Log {
        p: LogParams,
        inverse: bool,
    },
    Cdl {
        slope: [f64; 3],
        offset: [f64; 3],
        power: [f64; 3],
        sat: f64,
        clamp: bool,
        inverse: bool,
    },
    /// `clamp(x · scale + offset, lo, hi)` per channel.
    Range {
        scale: f64,
        offset: f64,
        lo: Option<f64>,
        hi: Option<f64>,
    },
    Lut1d {
        lut: Arc<Lut1d>,
        inverse: bool,
    },
    Lut3d {
        lut: Arc<Lut3d>,
        interp: Interpolation,
    },
    Fixed {
        f: Fixed,
        inverse: bool,
    },
    Spline {
        s: Spline,
        inverse: bool,
    },
    /// Linear (1.0 = 100 nits, mirrored for negatives) → SMPTE ST 2084.
    LinToPq {
        inverse: bool,
    },
}

pub(crate) fn matrix(rows: [[f64; 3]; 3]) -> Op {
    Op::Matrix {
        m: Matrix3d { v: rows },
        offset: [0.; 3],
    }
}

pub(crate) fn scale_offset(scale: f64, offset: f64) -> Op {
    Op::Matrix {
        m: Matrix3d {
            v: [[scale, 0., 0.], [0., scale, 0.], [0., 0., scale]],
        },
        offset: [offset; 3],
    }
}

/// A Range op from OCIO's RangeTransform parameters.
pub(crate) fn range(
    min_in: Option<f64>,
    max_in: Option<f64>,
    min_out: Option<f64>,
    max_out: Option<f64>,
    clamp: bool,
) -> Result<Op> {
    let (scale, offset) = match (min_in, max_in, min_out, max_out) {
        (Some(a), Some(b), Some(c), Some(d)) => {
            if (b - a).abs() < 1e-12 {
                return Err(Error::Config("RangeTransform min_in equals max_in".into()));
            }
            let s = (d - c) / (b - a);
            (s, c - s * a)
        }
        (Some(a), _, Some(c), _) if max_in.is_none() || max_out.is_none() => (1., c - a),
        (_, Some(b), _, Some(d)) => (1., d - b),
        (None, None, None, None) => (1., 0.),
        _ => {
            return Err(Error::Config(
                "RangeTransform needs matching in and out values".into(),
            ));
        }
    };
    let (lo, hi) = if clamp {
        let lo = min_in.and(min_out);
        let hi = max_in.and(max_out);
        // With only one end given, OCIO clamps only that end.
        (lo, hi)
    } else {
        (None, None)
    };
    Ok(Op::Range {
        scale,
        offset,
        lo,
        hi,
    })
}

fn pow_curve(x: f64, g: f64, negative: NegativeStyle) -> f64 {
    if x >= 0. {
        x.powf(g)
    } else {
        match negative {
            NegativeStyle::Mirror => -(-x).powf(g),
            NegativeStyle::PassThru => x,
            NegativeStyle::Clamp | NegativeStyle::Linear => 0.,
        }
    }
}

/// OCIO's moncurve: `gamma` and `offset` define a power segment and a
/// linear toe that meets it with matching slope.
fn moncurve(x: f64, gamma: f64, offset: f64, inverse: bool, mirror: bool) -> f64 {
    if mirror && x < 0. {
        return -moncurve(-x, gamma, offset, inverse, false);
    }
    const EPS: f64 = 1e-6;
    let g = gamma.max(1. + EPS);
    let o = offset.max(EPS);
    if !inverse {
        let brk = o / (g - 1.);
        let slope = (g - 1.) / o * (o * g / ((g - 1.) * (1. + o))).powf(g);
        if x <= brk {
            x * slope
        } else {
            ((x + o) / (1. + o)).powf(g)
        }
    } else {
        let brk = (o * g / ((g - 1.) * (1. + o))).powf(g);
        let slope = ((g - 1.) / o).powf(g - 1.) * ((1. + o) / g).powf(g);
        if x <= brk {
            x * slope
        } else {
            (1. + o) * x.powf(1. / g) - o
        }
    }
}

impl LogParams {
    fn ln_base(&self) -> f64 {
        self.base.ln()
    }

    /// The straight segment of a camera log curve, as (slope, offset, log
    /// value at the break) for channel `c`.
    fn camera(&self, c: usize) -> Option<(f64, f64, f64)> {
        let brk = self.lin_break?[c];
        let inner = self.lin_slope[c] * brk + self.lin_offset[c];
        let log_break = self.log_slope[c] * inner.ln() / self.ln_base() + self.log_offset[c];
        let slope = match self.linear_slope {
            Some(s) => s[c],
            None => self.log_slope[c] * self.lin_slope[c] / (inner * self.ln_base()),
        };
        Some((slope, log_break - slope * brk, log_break))
    }

    pub(crate) fn lin_to_log(&self, x: f64, c: usize) -> f64 {
        if let Some((slope, offset, _)) = self.camera(c)
            && x <= self.lin_break.unwrap_or_default()[c]
        {
            return slope * x + offset;
        }
        let inner = (self.lin_slope[c] * x + self.lin_offset[c]).max(f32::MIN_POSITIVE as f64);
        self.log_slope[c] * inner.ln() / self.ln_base() + self.log_offset[c]
    }

    pub(crate) fn log_to_lin(&self, y: f64, c: usize) -> f64 {
        if let Some((slope, offset, log_break)) = self.camera(c)
            && y <= log_break
        {
            return (y - offset) / slope;
        }
        let e = (y - self.log_offset[c]) / self.log_slope[c];
        ((e * self.ln_base()).exp() - self.lin_offset[c]) / self.lin_slope[c]
    }
}

/// Rec. 709 luma, as the ASC CDL saturation uses.
const CDL_LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];

fn saturate(rgb: [f64; 3], sat: f64) -> [f64; 3] {
    let luma = rgb[0] * CDL_LUMA[0] + rgb[1] * CDL_LUMA[1] + rgb[2] * CDL_LUMA[2];
    rgb.map(|v| luma + sat * (v - luma))
}

fn cdl_power(v: f64, p: f64, clamp: bool) -> f64 {
    if clamp {
        v.clamp(0., 1.).powf(p)
    } else if v < 0. {
        v
    } else {
        v.powf(p)
    }
}

impl Op {
    pub(crate) fn apply(&self, rgb: [f64; 3]) -> [f64; 3] {
        match self {
            Op::Matrix { m, offset } => {
                let v = m.mul_vector(Vector3d { v: rgb }).v;
                [v[0] + offset[0], v[1] + offset[1], v[2] + offset[2]]
            }
            Op::Gamma {
                gamma,
                offset,
                negative,
                inverse,
            } => std::array::from_fn(|c| match offset {
                None => {
                    let g = if *inverse { 1. / gamma[c] } else { gamma[c] };
                    pow_curve(rgb[c], g, *negative)
                }
                Some(o) => moncurve(
                    rgb[c],
                    gamma[c],
                    o[c],
                    *inverse,
                    *negative == NegativeStyle::Mirror,
                ),
            }),
            Op::Log { p, inverse } => std::array::from_fn(|c| {
                if *inverse {
                    p.log_to_lin(rgb[c], c)
                } else {
                    p.lin_to_log(rgb[c], c)
                }
            }),
            Op::Cdl {
                slope,
                offset,
                power,
                sat,
                clamp,
                inverse,
            } => {
                let clamp01 = |v: [f64; 3]| {
                    if *clamp {
                        v.map(|x| x.clamp(0., 1.))
                    } else {
                        v
                    }
                };
                if !inverse {
                    let v: [f64; 3] = std::array::from_fn(|c| {
                        cdl_power(rgb[c] * slope[c] + offset[c], power[c], *clamp)
                    });
                    clamp01(saturate(v, *sat))
                } else {
                    let v = saturate(clamp01(rgb), 1. / sat);
                    clamp01(std::array::from_fn(|c| {
                        (cdl_power(v[c], 1. / power[c], *clamp) - offset[c]) / slope[c]
                    }))
                }
            }
            Op::Range {
                scale,
                offset,
                lo,
                hi,
            } => rgb.map(|v| {
                let mut v = v * scale + offset;
                if let Some(lo) = lo {
                    v = v.max(*lo);
                }
                if let Some(hi) = hi {
                    v = v.min(*hi);
                }
                v
            }),
            Op::Lut1d { lut, inverse } => {
                if *inverse {
                    lut.apply_inverse(rgb)
                } else {
                    lut.apply(rgb)
                }
            }
            Op::Lut3d { lut, interp } => lut.apply(rgb, *interp),
            Op::Fixed { f, inverse } => f.apply(rgb, *inverse),
            Op::Spline { s, inverse } => {
                rgb.map(|v| if *inverse { s.reverse(v) } else { s.forward(v) })
            }
            Op::LinToPq { inverse } => rgb.map(|v| {
                let r = if *inverse {
                    super::aces::pq_to_lin(v.abs())
                } else {
                    super::aces::lin_to_pq(v.abs())
                };
                r.copysign(v)
            }),
        }
    }

    pub(crate) fn inverse(&self) -> Result<Op> {
        Ok(match self {
            Op::Matrix { m, offset } => {
                if m.determinant().is_none_or(|d| d.abs() < 1e-15) {
                    return Err(Error::Unsupported(
                        "inverse of a singular MatrixTransform".into(),
                    ));
                }
                let inv = m.inverse();
                let o = inv.mul_vector(Vector3d { v: *offset }).v;
                Op::Matrix {
                    m: inv,
                    offset: o.map(|v| -v),
                }
            }
            Op::Gamma {
                gamma,
                offset,
                negative,
                inverse,
            } => Op::Gamma {
                gamma: *gamma,
                offset: *offset,
                negative: *negative,
                inverse: !inverse,
            },
            Op::Log { p, inverse } => Op::Log {
                p: p.clone(),
                inverse: !inverse,
            },
            Op::Cdl {
                slope,
                offset,
                power,
                sat,
                clamp,
                inverse,
            } => Op::Cdl {
                slope: *slope,
                offset: *offset,
                power: *power,
                sat: *sat,
                clamp: *clamp,
                inverse: !inverse,
            },
            Op::Range {
                scale,
                offset,
                lo,
                hi,
            } => {
                if scale.abs() < 1e-15 {
                    return Err(Error::Unsupported(
                        "inverse of a flat RangeTransform".into(),
                    ));
                }
                let back = |v: f64| (v - offset) / scale;
                Op::Range {
                    scale: 1. / scale,
                    offset: -offset / scale,
                    lo: lo.map(back),
                    hi: hi.map(back),
                }
            }
            Op::Lut1d { lut, inverse } => {
                if !inverse && !lut.is_monotonic() {
                    return Err(Error::Unsupported(
                        "inverse of a 1D LUT that is not monotonic".into(),
                    ));
                }
                Op::Lut1d {
                    lut: lut.clone(),
                    inverse: !inverse,
                }
            }
            Op::Lut3d { .. } => {
                return Err(Error::Unsupported(
                    "inverse of a 3D LUT (FileTransform with direction: inverse)".into(),
                ));
            }
            Op::Fixed { f, inverse } => Op::Fixed {
                f: f.clone(),
                inverse: !inverse,
            },
            Op::Spline { s, inverse } => Op::Spline {
                s: *s,
                inverse: !inverse,
            },
            Op::LinToPq { inverse } => Op::LinToPq { inverse: !inverse },
        })
    }
}

/// The inverse of an op list: reversed, each op inverted.
pub(crate) fn invert(ops: Vec<Op>) -> Result<Vec<Op>> {
    ops.iter().rev().map(Op::inverse).collect()
}

/// Fold neighbouring matrices into one and drop identities.
pub(crate) fn optimize(ops: Vec<Op>) -> Vec<Op> {
    let mut out: Vec<Op> = Vec::with_capacity(ops.len());
    for op in ops {
        if let (Some(Op::Matrix { m: m1, offset: o1 }), Op::Matrix { m: m2, offset: o2 }) =
            (out.last(), &op)
        {
            let m = m2.mat_mul(*m1);
            let o = m2.mul_vector(Vector3d { v: *o1 }).v;
            let merged = Op::Matrix {
                m,
                offset: [o[0] + o2[0], o[1] + o2[1], o[2] + o2[2]],
            };
            out.pop();
            out.push(merged);
        } else {
            out.push(op);
        }
        if let Some(Op::Matrix { m, offset }) = out.last()
            && is_identity(m, offset)
        {
            out.pop();
        }
    }
    out
}

fn is_identity(m: &Matrix3d, offset: &[f64; 3]) -> bool {
    (0..3).all(|r| (0..3).all(|c| (m.v[r][c] - f64::from(r == c)).abs() < 1e-12))
        && offset.iter().all(|o| o.abs() < 1e-12)
}

/// A compiled colour transform. Alpha is never touched.
#[derive(Clone, Debug, Default)]
pub struct Processor {
    pub(crate) ops: Vec<Op>,
}

impl Processor {
    pub(crate) fn new(ops: Vec<Op>) -> Self {
        Self { ops: optimize(ops) }
    }

    /// Whether the processor leaves every colour unchanged.
    pub fn is_noop(&self) -> bool {
        self.ops.is_empty()
    }

    /// The number of compiled operations (after folding matrices).
    pub fn op_count(&self) -> usize {
        self.ops.len()
    }

    pub fn apply_f64(&self, rgb: [f64; 3]) -> [f64; 3] {
        self.ops.iter().fold(rgb, |v, op| op.apply(v))
    }

    pub fn apply_rgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.apply_f64(rgb.map(f64::from)).map(|v| v as f32)
    }

    /// Interleaved RGBA floats, in place.
    pub fn apply_rgba(&self, pixels: &mut [f32]) {
        for p in pixels.as_chunks_mut::<4>().0 {
            let out = self.apply_rgb([p[0], p[1], p[2]]);
            p[..3].copy_from_slice(&out);
        }
    }

    /// Interleaved RGBA8 (straight alpha), in place: exact ops, then
    /// clamped and rounded to 8 bits.
    pub fn apply_rgba8(&self, pixels: &mut [u8]) {
        if self.is_noop() {
            return;
        }
        for p in pixels.as_chunks_mut::<4>().0 {
            let out = self.apply_f64([p[0], p[1], p[2]].map(|v| f64::from(v) / 255.));
            for c in 0..3 {
                p[c] = (out[c].clamp(0., 1.) * 255. + 0.5) as u8;
            }
        }
    }

    /// The processor run backwards.
    pub fn inverse(&self) -> Result<Processor> {
        Ok(Processor::new(invert(self.ops.clone())?))
    }
}
