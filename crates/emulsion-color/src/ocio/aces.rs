//! `BuiltinTransform` styles and the ACES maths behind them: primaries
//! matrices (built with moxcms from the published chromaticities), the
//! ACEScct/ACEScc curves, the ACES 1.0 RRT and SDR ODT pieces (glow, red
//! modifier, segmented-spline tone scales from the ACES 1.0.3 CTL, dark to
//! dim surround), gamut compression and the display encodings.

use super::ops::{self, Op};
use super::transform::{LogParams, NegativeStyle};
use super::{Error, Result};
use moxcms::{Chromaticity, ColorPrimaries, Matrix3d, XyY, Xyz, adaption_matrix_d};

/// Chromaticities of a colour space's red, green and blue primaries and
/// white point.
#[derive(Clone, Copy, Debug)]
pub struct Primaries {
    pub rgb: [(f64, f64); 3],
    pub white: (f64, f64),
}

const D65: (f64, f64) = (0.3127, 0.3290);
const ACES_WHITE: (f64, f64) = (0.32168, 0.33767);

pub const AP0: Primaries = Primaries {
    rgb: [(0.7347, 0.2653), (0.0, 1.0), (0.0001, -0.077)],
    white: ACES_WHITE,
};
pub const AP1: Primaries = Primaries {
    rgb: [(0.713, 0.293), (0.165, 0.830), (0.128, 0.044)],
    white: ACES_WHITE,
};
pub const REC709: Primaries = Primaries {
    rgb: [(0.64, 0.33), (0.30, 0.60), (0.15, 0.06)],
    white: D65,
};
pub const REC2020: Primaries = Primaries {
    rgb: [(0.708, 0.292), (0.170, 0.797), (0.131, 0.046)],
    white: D65,
};
pub const P3_D65: Primaries = Primaries {
    rgb: [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)],
    white: D65,
};
pub const P3_DCI: Primaries = Primaries {
    rgb: P3_D65.rgb,
    white: (0.314, 0.351),
};
pub const P3_D60: Primaries = Primaries {
    rgb: P3_D65.rgb,
    white: ACES_WHITE,
};

fn white_xyz((x, y): (f64, f64)) -> Xyz {
    let v = XyY::new(x, y, 1.).to_xyzd();
    Xyz::new(v.x as f32, v.y as f32, v.z as f32)
}

/// RGB → CIE XYZ in the space's own white.
pub fn rgb_to_xyz(p: &Primaries) -> Matrix3d {
    let c = |(x, y): (f64, f64)| Chromaticity::new(x as f32, y as f32);
    ColorPrimaries {
        red: c(p.rgb[0]),
        green: c(p.rgb[1]),
        blue: c(p.rgb[2]),
    }
    .transform_to_xyz_d(XyY::new(p.white.0, p.white.1, 1.))
}

fn bradford(from: (f64, f64), to: (f64, f64)) -> Matrix3d {
    if from == to {
        return Matrix3d::IDENTITY;
    }
    adaption_matrix_d(white_xyz(from), white_xyz(to))
}

/// `src` RGB → `dst` RGB, with a Bradford white-point adaptation when
/// `adapt` (otherwise colorimetric, as for AP0 ↔ AP1 which share a white).
pub fn conversion(src: &Primaries, dst: &Primaries, adapt: bool) -> Matrix3d {
    let cat = if adapt {
        bradford(src.white, dst.white)
    } else {
        Matrix3d::IDENTITY
    };
    rgb_to_xyz(dst)
        .inverse()
        .mat_mul(cat.mat_mul(rgb_to_xyz(src)))
}

/// `src` RGB → CIE XYZ with a D65 white.
pub fn to_xyz_d65(src: &Primaries, adapt: bool) -> Matrix3d {
    let cat = if adapt {
        bradford(src.white, D65)
    } else {
        Matrix3d::IDENTITY
    };
    cat.mat_mul(rgb_to_xyz(src))
}

fn mat(m: Matrix3d) -> Op {
    ops::matrix(m.v)
}

/// The ACES saturation matrix around `weights` (the luminance row).
pub fn saturation_matrix(sat: f64, weights: [f64; 3]) -> Matrix3d {
    Matrix3d {
        v: std::array::from_fn(|r| {
            std::array::from_fn(|c| (1. - sat) * weights[c] + if r == c { sat } else { 0. })
        }),
    }
}

fn ap1_luminance() -> [f64; 3] {
    rgb_to_xyz(&AP1).v[1]
}

/// ACEScct: `log_side_slope 1/17.52`, `log_side_offset 9.72/17.52`, with
/// a straight segment below 2⁻⁷.
pub fn acescct_log() -> LogParams {
    LogParams {
        base: 2.,
        log_slope: [1. / 17.52; 3],
        log_offset: [9.72 / 17.52; 3],
        lin_slope: [1.; 3],
        lin_offset: [0.; 3],
        lin_break: Some([0.0078125; 3]),
        linear_slope: None,
    }
}

// ── Fixed functions ─────────────────────────────────────────────────────

/// The `FixedFunctionTransform` styles this crate runs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Fixed {
    Glow {
        gain: f64,
        mid: f64,
    },
    RedMod {
        one_minus_scale: f64,
        pivot: f64,
        inv_width: f64,
        restore_hue: bool,
    },
    DarkToDim {
        gamma: f64,
    },
    GamutComp {
        lim: [f64; 3],
        thr: [f64; 3],
        power: f64,
    },
    /// ACEScc's log encoding (forward: ACEScc → linear AP1).
    AcesCcToLin,
}

const NOISE_LIMIT: f64 = 1e-2;

fn sat_weight([r, g, b]: [f64; 3]) -> f64 {
    let min = r.min(g).min(b);
    let max = r.max(g).max(b);
    (max.max(1e-10) - min.max(1e-10)) / max.max(NOISE_LIMIT)
}

fn yc([r, g, b]: [f64; 3]) -> f64 {
    let chroma = (b * (b - g) + g * (g - r) + r * (r - b)).max(0.).sqrt();
    (b + g + r + 1.75 * chroma) / 3.
}

fn sigmoid(sat: f64) -> f64 {
    let x = (sat - 0.4) * 5.;
    let sign = 1f64.copysign(x);
    let t = (1. - 0.5 * sign * x).max(0.);
    (1. + sign * (1. - t * t)) * 0.5
}

fn hue_weight([r, g, b]: [f64; 3], inv_width: f64) -> f64 {
    let a = 2. * r - (g + b);
    let bb = 3f64.sqrt() * (g - b);
    let hue = bb.atan2(a);
    let knot = hue * inv_width + 2.;
    let j = knot.floor();
    if !(0. ..4.).contains(&j) {
        return 0.;
    }
    const M: [[f64; 4]; 4] = [
        [0.25, 0.00, 0.00, 0.00],
        [-0.75, 0.75, 0.75, 0.25],
        [0.75, -1.50, 0.00, 1.00],
        [-0.25, 0.75, -0.75, 0.25],
    ];
    let t = knot - j;
    let c = M[j as usize];
    c[3] + t * (c[2] + t * (c[1] + t * c[0]))
}

fn compress(dist: f64, thr: f64, scale: f64, power: f64, inverse: bool) -> f64 {
    let nd = (dist - thr) / scale;
    let p = nd.powf(power);
    if !inverse {
        thr + scale * nd / (1. + p).powf(1. / power)
    } else if dist >= thr + scale {
        dist
    } else {
        thr + scale * (-(p / (p - 1.))).powf(1. / power)
    }
}

impl Fixed {
    pub(crate) fn from_style(style: &str, params: &[f64]) -> Result<Self> {
        Ok(match style.to_ascii_lowercase().as_str() {
            "aces_glow03" => Fixed::Glow {
                gain: 0.075,
                mid: 0.1,
            },
            "aces_glow10" => Fixed::Glow {
                gain: 0.05,
                mid: 0.08,
            },
            "aces_redmod03" => Fixed::RedMod {
                one_minus_scale: 1. - 0.85,
                pivot: 0.03,
                inv_width: 1.909_859_317_102_744_3,
                restore_hue: true,
            },
            "aces_redmod10" => Fixed::RedMod {
                one_minus_scale: 1. - 0.82,
                pivot: 0.03,
                inv_width: 1.697_652_726_313_550_4,
                restore_hue: false,
            },
            "aces_darktodim10" => Fixed::DarkToDim { gamma: 0.9811 },
            "aces_gamutcomp13" => match params {
                [lc, lm, ly, tc, tm, ty, p] => Fixed::GamutComp {
                    lim: [*lc, *lm, *ly],
                    thr: [*tc, *tm, *ty],
                    power: *p,
                },
                _ => {
                    return Err(Error::Config("ACES_GamutComp13 needs 7 params".into()));
                }
            },
            _ => {
                return Err(Error::Unsupported(format!(
                    "FixedFunctionTransform style “{style}”"
                )));
            }
        })
    }

    pub(crate) fn apply(&self, rgb: [f64; 3], inverse: bool) -> [f64; 3] {
        match *self {
            Fixed::Glow { gain, mid } => {
                let yc = yc(rgb);
                let gain = gain * sigmoid(sat_weight(rgb));
                let out = if !inverse {
                    if yc >= mid * 2. {
                        0.
                    } else if yc <= mid * 2. / 3. {
                        gain
                    } else {
                        gain * (mid / yc - 0.5)
                    }
                } else if yc >= mid * 2. {
                    0.
                } else if yc <= (1. + gain) * mid * 2. / 3. {
                    -gain / (1. + gain)
                } else {
                    gain * (mid / yc - 0.5) / (gain * 0.5 - 1.)
                };
                rgb.map(|v| v * (1. + out))
            }
            Fixed::RedMod {
                one_minus_scale,
                pivot,
                inv_width,
                restore_hue,
            } => {
                let [r, mut g, mut b] = rgb;
                let f_h = hue_weight(rgb, inv_width);
                if f_h <= 0. {
                    return rgb;
                }
                let new_r = if !inverse {
                    let f_s = sat_weight(rgb);
                    r + f_h * f_s * (pivot - r) * one_minus_scale
                } else {
                    let min = g.min(b);
                    let a = f_h * one_minus_scale - 1.;
                    let bq = r - f_h * (pivot + min) * one_minus_scale;
                    let c = f_h * pivot * min * one_minus_scale;
                    (-bq - (bq * bq - 4. * a * c).sqrt()) / (2. * a)
                };
                if restore_hue {
                    if g >= b {
                        let f = (g - b) / (r - b).max(1e-10);
                        g = f * (new_r - b) + b;
                    } else {
                        let f = (b - g) / (r - g).max(1e-10);
                        b = f * (new_r - g) + g;
                    }
                }
                [new_r, g, b]
            }
            Fixed::DarkToDim { gamma } => {
                let w = [
                    0.272_228_716_780_914_54,
                    0.674_081_765_811_148_3,
                    0.053_689_517_407_937_05,
                ];
                let y = (w[0] * rgb[0] + w[1] * rgb[1] + w[2] * rgb[2]).max(1e-10);
                let g = if inverse { 1. / gamma } else { gamma };
                let k = y.powf(g - 1.);
                rgb.map(|v| v * k)
            }
            Fixed::GamutComp { lim, thr, power } => {
                let ach = rgb[0].max(rgb[1]).max(rgb[2]);
                std::array::from_fn(|c| {
                    if ach == 0. {
                        return 0.;
                    }
                    let scale = (lim[c] - thr[c])
                        / (((1. - thr[c]) / (lim[c] - thr[c])).powf(-power) - 1.).powf(1. / power);
                    let dist = (ach - rgb[c]) / ach.abs();
                    if dist < thr[c] {
                        return rgb[c];
                    }
                    ach - compress(dist, thr[c], scale, power, inverse) * ach.abs()
                })
            }
            Fixed::AcesCcToLin => rgb.map(|v| {
                if !inverse {
                    if v < (9.72 - 15.) / 17.52 {
                        (2f64.powf(v * 17.52 - 9.72) - 2f64.powi(-16)) * 2.
                    } else {
                        2f64.powf(v * 17.52 - 9.72).min(65504.)
                    }
                } else if v <= 0. {
                    -0.358_447_488_584_474_9 // (log2(2^-16) + 9.72) / 17.52
                } else if v < 2f64.powi(-15) {
                    ((2f64.powi(-16) + v * 0.5).log2() + 9.72) / 17.52
                } else {
                    (v.log2() + 9.72) / 17.52
                }
            }),
        }
    }
}

// ── ACES 1.0 tone scales (ACESlib.Tonescales, aces-dev 1.0.3) ───────────

/// The segmented-spline tone scales of the ACES 1.0 RRT and the 48-nit
/// cinema ODT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Spline {
    Rrt,
    Odt48,
}

const SPLINE_M: [[f64; 3]; 3] = [[0.5, -1.0, 0.5], [-1.0, 1.0, 0.5], [0.5, 0.0, 0.0]];

struct SplineParams {
    low: &'static [f64],
    high: &'static [f64],
    min: (f64, f64),
    mid: (f64, f64),
    max: (f64, f64),
    slope_low: f64,
    slope_high: f64,
    knots: usize,
}

const HALF_MIN: f64 = 5.960_464_48e-8;

fn rrt_params() -> SplineParams {
    SplineParams {
        low: &[
            -4.0,
            -4.0,
            -3.1573765773,
            -0.4852499958,
            1.8477324706,
            1.8477324706,
        ],
        high: &[-0.7185482425, 2.0810307172, 3.6681241237, 4.0, 4.0, 4.0],
        min: (0.18 * 2f64.powi(-15), 0.0001),
        mid: (0.18, 4.8),
        max: (0.18 * 2f64.powi(18), 10000.),
        slope_low: 0.,
        slope_high: 0.,
        knots: 4,
    }
}

fn odt48_params() -> SplineParams {
    let c5 = |x: f64| spline_fwd(&rrt_params(), x);
    SplineParams {
        low: &[
            -1.6989700043,
            -1.6989700043,
            -1.4779,
            -1.2291,
            -0.8648,
            -0.448,
            0.00518,
            0.4511080334,
            0.9113744414,
            0.9113744414,
        ],
        high: &[
            0.5154386965,
            0.8470437783,
            1.1358,
            1.3802,
            1.5197,
            1.5985,
            1.6467,
            1.6746091357,
            1.6878733390,
            1.6878733390,
        ],
        min: (c5(0.18 * 2f64.powf(-6.5)), 0.02),
        mid: (c5(0.18), 4.8),
        max: (c5(0.18 * 2f64.powf(6.5)), 48.),
        slope_low: 0.,
        slope_high: 0.04,
        knots: 8,
    }
}

fn spline_eval(cf: [f64; 3], t: f64) -> f64 {
    let m: [f64; 3] = std::array::from_fn(|c| (0..3).map(|r| cf[r] * SPLINE_M[r][c]).sum());
    m[0] * t * t + m[1] * t + m[2]
}

fn spline_fwd(p: &SplineParams, x: f64) -> f64 {
    let logx = x.max(HALF_MIN).log10();
    let (lmin, lmid, lmax) = (p.min.0.log10(), p.mid.0.log10(), p.max.0.log10());
    let n = (p.knots - 1) as f64;
    let logy = if logx <= lmin {
        logx * p.slope_low + (p.min.1.log10() - p.slope_low * lmin)
    } else if logx < lmid {
        let k = n * (logx - lmin) / (lmid - lmin);
        let j = (k as usize).min(p.knots - 2);
        spline_eval([p.low[j], p.low[j + 1], p.low[j + 2]], k - j as f64)
    } else if logx < lmax {
        let k = n * (logx - lmid) / (lmax - lmid);
        let j = (k as usize).min(p.knots - 2);
        spline_eval([p.high[j], p.high[j + 1], p.high[j + 2]], k - j as f64)
    } else {
        logx * p.slope_high + (p.max.1.log10() - p.slope_high * lmax)
    };
    10f64.powf(logy)
}

fn spline_rev(p: &SplineParams, y: f64) -> f64 {
    let n = p.knots;
    let (lmin, lmid, lmax) = (p.min.0.log10(), p.mid.0.log10(), p.max.0.log10());
    let inc_low = (lmid - lmin) / (n - 1) as f64;
    let inc_high = (lmax - lmid) / (n - 1) as f64;
    let logy = y.max(1e-10).log10();
    let solve = |coefs: &[f64], base: f64, inc: f64| {
        let knot_y: Vec<f64> = (0..n).map(|i| (coefs[i] + coefs[i + 1]) / 2.).collect();
        let j = (0..n - 1)
            .find(|&j| logy > knot_y[j] && logy <= knot_y[j + 1])
            .unwrap_or(if logy <= knot_y[0] { 0 } else { n - 2 });
        let cf = [coefs[j], coefs[j + 1], coefs[j + 2]];
        let m: [f64; 3] = std::array::from_fn(|c| (0..3).map(|r| cf[r] * SPLINE_M[r][c]).sum());
        let (a, b, c) = (m[0], m[1], m[2] - logy);
        let d = (b * b - 4. * a * c).max(0.).sqrt();
        let t = (2. * c) / (-d - b);
        base + (t + j as f64) * inc
    };
    let logx = if logy <= p.min.1.log10() {
        if p.slope_low > 0. {
            (logy - (p.min.1.log10() - p.slope_low * lmin)) / p.slope_low
        } else {
            lmin
        }
    } else if logy <= p.mid.1.log10() {
        solve(p.low, lmin, inc_low)
    } else if logy < p.max.1.log10() {
        solve(p.high, lmid, inc_high)
    } else if p.slope_high > 0. {
        (logy - (p.max.1.log10() - p.slope_high * lmax)) / p.slope_high
    } else {
        lmax
    };
    10f64.powf(logx)
}

impl Spline {
    fn params(self) -> SplineParams {
        match self {
            Spline::Rrt => rrt_params(),
            Spline::Odt48 => odt48_params(),
        }
    }

    pub(crate) fn forward(self, x: f64) -> f64 {
        spline_fwd(&self.params(), x)
    }

    pub(crate) fn reverse(self, y: f64) -> f64 {
        spline_rev(&self.params(), y)
    }
}

// ── ST 2084 ──────────────────────────────────────────────────────────────

const PQ_M1: f64 = 0.25 * 2610. / 4096.;
const PQ_M2: f64 = 128. * 2523. / 4096.;
const PQ_C2: f64 = 32. * 2413. / 4096.;
const PQ_C3: f64 = 32. * 2392. / 4096.;
const PQ_C1: f64 = PQ_C3 - PQ_C2 + 1.;

/// Linear (1.0 = 100 nits) → PQ code value, for non-negative input.
pub(crate) fn lin_to_pq(v: f64) -> f64 {
    let y = (v * 0.01).powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * y) / (1. + PQ_C3 * y)).max(0.).powf(PQ_M2)
}

pub(crate) fn pq_to_lin(v: f64) -> f64 {
    let x = v.powf(1. / PQ_M2);
    ((x - PQ_C1).max(0.) / (PQ_C2 - PQ_C3 * x)).powf(1. / PQ_M1) * 100.
}

// ── Builtin transforms ───────────────────────────────────────────────────

fn rrt_preamble(out: &mut Vec<Op>) -> Result<()> {
    out.push(Op::Fixed {
        f: Fixed::from_style("ACES_Glow10", &[])?,
        inverse: false,
    });
    out.push(Op::Fixed {
        f: Fixed::from_style("ACES_RedMod10", &[])?,
        inverse: false,
    });
    out.push(ops::range(Some(0.), None, Some(0.), None, true)?);
    out.push(mat(conversion(&AP0, &AP1, false)));
    out.push(ops::range(Some(0.), None, Some(0.), None, true)?);
    out.push(mat(saturation_matrix(0.96, ap1_luminance())));
    Ok(())
}

fn tone_curve(out: &mut Vec<Op>) {
    out.push(Op::Spline {
        s: Spline::Rrt,
        inverse: false,
    });
    out.push(Op::Spline {
        s: Spline::Odt48,
        inverse: false,
    });
    // Cinema white and black: 48 nits and 0.02 nits.
    let scale = 1. / (48. - 0.02);
    out.push(ops::scale_offset(scale, -0.02 * scale));
}

fn video_adjustment(out: &mut Vec<Op>) {
    out.push(Op::Fixed {
        f: Fixed::DarkToDim { gamma: 0.9811 },
        inverse: false,
    });
    out.push(mat(saturation_matrix(0.93, ap1_luminance())));
}

fn primary_clamp(out: &mut Vec<Op>, limit: &Primaries) -> Result<()> {
    out.push(mat(conversion(&AP1, limit, true)));
    out.push(ops::range(Some(0.), Some(1.), Some(0.), Some(1.), true)?);
    out.push(mat(rgb_to_xyz(limit)));
    Ok(())
}

/// XYZ (D65) → display RGB, then an encoding.
fn display(out: &mut Vec<Op>, p: &Primaries, adapt: bool, encode: Encoding) {
    out.push(mat(to_xyz_d65(p, adapt).inverse()));
    match encode {
        Encoding::Gamma(g, negative) => out.push(Op::Gamma {
            gamma: [g; 3],
            offset: None,
            negative,
            inverse: true,
        }),
        Encoding::Srgb(negative) => out.push(Op::Gamma {
            gamma: [2.4; 3],
            offset: Some([0.055; 3]),
            negative,
            inverse: true,
        }),
        Encoding::Pq => out.push(Op::LinToPq { inverse: false }),
    }
}

#[derive(Clone, Copy)]
enum Encoding {
    Gamma(f64, NegativeStyle),
    Srgb(NegativeStyle),
    Pq,
}

/// Every BuiltinTransform style this crate implements, for listing.
pub const BUILTIN_STYLES: &[&str] = &[
    "IDENTITY",
    "UTILITY - ACES-AP0_to_CIE-XYZ-D65_BFD",
    "UTILITY - ACES-AP1_to_CIE-XYZ-D65_BFD",
    "UTILITY - ACES-AP1_to_LINEAR-REC709_BFD",
    "CURVE - ACEScct-LOG_to_LINEAR",
    "ACEScct_to_ACES2065-1",
    "ACEScc_to_ACES2065-1",
    "ACEScg_to_ACES2065-1",
    "ACES-LMT - BLUE_LIGHT_ARTIFACT_FIX",
    "ACES-LMT - ACES 1.3 Reference Gamut Compression",
    "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-CINEMA_1.0",
    "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-VIDEO_1.0",
    "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-CINEMA-REC709lim_1.1",
    "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-VIDEO-REC709lim_1.1",
    "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-VIDEO-P3lim_1.1",
    "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.709",
    "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.709 - MIRROR NEGS",
    "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.2020",
    "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.2020 - MIRROR NEGS",
    "DISPLAY - CIE-XYZ-D65_to_G2.2-REC.709",
    "DISPLAY - CIE-XYZ-D65_to_G2.2-REC.709 - MIRROR NEGS",
    "DISPLAY - CIE-XYZ-D65_to_sRGB",
    "DISPLAY - CIE-XYZ-D65_to_sRGB - MIRROR NEGS",
    "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-DCI-BFD",
    "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-D65",
    "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-D65 - MIRROR NEGS",
    "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-D60-BFD",
    "DISPLAY - CIE-XYZ-D65_to_DisplayP3",
    "CURVE - ST-2084_to_LINEAR",
    "CURVE - LINEAR_to_ST-2084",
    "DISPLAY - CIE-XYZ-D65_to_REC.2100-PQ",
    "DISPLAY - CIE-XYZ-D65_to_ST2084-P3-D65",
];

/// The forward ops of a BuiltinTransform style.
pub(crate) fn builtin(style: &str) -> Result<Vec<Op>> {
    let mut out = Vec::new();
    let known = BUILTIN_STYLES
        .iter()
        .find(|s| s.eq_ignore_ascii_case(style.trim()))
        .ok_or_else(|| Error::Unsupported(format!("BuiltinTransform style “{style}”")))?;
    let mirror = known.ends_with(" - MIRROR NEGS");
    let neg = if mirror {
        NegativeStyle::Mirror
    } else {
        NegativeStyle::Clamp
    };
    let srgb_neg = if mirror {
        NegativeStyle::Mirror
    } else {
        NegativeStyle::Linear
    };
    let base = known.trim_end_matches(" - MIRROR NEGS");
    match base {
        "IDENTITY" => {}
        "UTILITY - ACES-AP0_to_CIE-XYZ-D65_BFD" => out.push(mat(to_xyz_d65(&AP0, true))),
        "UTILITY - ACES-AP1_to_CIE-XYZ-D65_BFD" => out.push(mat(to_xyz_d65(&AP1, true))),
        "UTILITY - ACES-AP1_to_LINEAR-REC709_BFD" => out.push(mat(conversion(&AP1, &REC709, true))),
        "CURVE - ACEScct-LOG_to_LINEAR" => out.push(Op::Log {
            p: acescct_log(),
            inverse: true,
        }),
        "ACEScct_to_ACES2065-1" => {
            out.push(Op::Log {
                p: acescct_log(),
                inverse: true,
            });
            out.push(mat(conversion(&AP1, &AP0, false)));
        }
        "ACEScc_to_ACES2065-1" => {
            out.push(Op::Fixed {
                f: Fixed::AcesCcToLin,
                inverse: false,
            });
            out.push(mat(conversion(&AP1, &AP0, false)));
            out.push(ops::range(Some(0.), None, Some(0.), None, true)?);
        }
        "ACEScg_to_ACES2065-1" => out.push(mat(conversion(&AP1, &AP0, false))),
        "ACES-LMT - BLUE_LIGHT_ARTIFACT_FIX" => out.push(ops::matrix([
            [0.9404372683, -0.0183068787, 0.0778696104],
            [0.0083786969, 0.8286599939, 0.1629613092],
            [0.0005471261, -0.0008833746, 1.0003362486],
        ])),
        "ACES-LMT - ACES 1.3 Reference Gamut Compression" => {
            let m = conversion(&AP0, &AP1, false);
            out.push(mat(m));
            out.push(Op::Fixed {
                f: Fixed::from_style(
                    "ACES_GamutComp13",
                    &[1.147, 1.264, 1.312, 0.815, 0.803, 0.880, 1.2],
                )?,
                inverse: false,
            });
            out.push(mat(m.inverse()));
        }
        "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-CINEMA_1.0" => {
            rrt_preamble(&mut out)?;
            tone_curve(&mut out);
            out.push(mat(to_xyz_d65(&AP1, true)));
        }
        "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-VIDEO_1.0" => {
            rrt_preamble(&mut out)?;
            tone_curve(&mut out);
            video_adjustment(&mut out);
            out.push(mat(to_xyz_d65(&AP1, true)));
        }
        "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-CINEMA-REC709lim_1.1" => {
            rrt_preamble(&mut out)?;
            tone_curve(&mut out);
            primary_clamp(&mut out, &REC709)?;
        }
        "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-VIDEO-REC709lim_1.1" => {
            rrt_preamble(&mut out)?;
            tone_curve(&mut out);
            video_adjustment(&mut out);
            primary_clamp(&mut out, &REC709)?;
        }
        "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-VIDEO-P3lim_1.1" => {
            rrt_preamble(&mut out)?;
            tone_curve(&mut out);
            video_adjustment(&mut out);
            primary_clamp(&mut out, &P3_D65)?;
        }
        "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.709" => {
            display(&mut out, &REC709, false, Encoding::Gamma(2.4, neg))
        }
        "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.2020" => {
            display(&mut out, &REC2020, false, Encoding::Gamma(2.4, neg))
        }
        "DISPLAY - CIE-XYZ-D65_to_G2.2-REC.709" => {
            display(&mut out, &REC709, false, Encoding::Gamma(2.2, neg))
        }
        "DISPLAY - CIE-XYZ-D65_to_sRGB" => {
            display(&mut out, &REC709, false, Encoding::Srgb(srgb_neg))
        }
        "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-DCI-BFD" => {
            display(&mut out, &P3_DCI, true, Encoding::Gamma(2.6, neg))
        }
        "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-D65" => {
            display(&mut out, &P3_D65, false, Encoding::Gamma(2.6, neg))
        }
        "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-D60-BFD" => {
            display(&mut out, &P3_D60, true, Encoding::Gamma(2.6, neg))
        }
        "DISPLAY - CIE-XYZ-D65_to_DisplayP3" => display(
            &mut out,
            &P3_D65,
            false,
            Encoding::Srgb(NegativeStyle::Linear),
        ),
        "CURVE - ST-2084_to_LINEAR" => out.push(Op::LinToPq { inverse: true }),
        "CURVE - LINEAR_to_ST-2084" => out.push(Op::LinToPq { inverse: false }),
        "DISPLAY - CIE-XYZ-D65_to_REC.2100-PQ" => display(&mut out, &REC2020, false, Encoding::Pq),
        "DISPLAY - CIE-XYZ-D65_to_ST2084-P3-D65" => display(&mut out, &P3_D65, false, Encoding::Pq),
        _ => unreachable!("listed in BUILTIN_STYLES"),
    }
    Ok(out)
}
