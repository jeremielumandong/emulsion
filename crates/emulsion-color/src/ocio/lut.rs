//! 1D and 3D LUTs and the text LUT formats: Resolve/Iridas `.cube` (1D, 3D
//! or a 1D shaper followed by a 3D LUT), Sony Imageworks `.spi1d` and
//! `.spi3d`. CLF/CTF lives in [`super::clf`].

use super::ops::Op;
use super::transform::Interpolation;
use super::{Error, Result};
use std::sync::Arc;

/// Channels sampled at `n` evenly spaced inputs between the domain ends.
#[derive(Clone, Debug, PartialEq)]
pub struct Lut1d {
    pub domain_min: [f64; 3],
    pub domain_max: [f64; 3],
    /// One table per channel, all the same length.
    pub values: [Vec<f32>; 3],
}

impl Lut1d {
    pub fn len(&self) -> usize {
        self.values[0].len()
    }

    pub fn is_empty(&self) -> bool {
        self.values[0].is_empty()
    }

    fn check(&self) -> Result<()> {
        let n = self.len();
        if n < 2 || self.values.iter().any(|v| v.len() != n) {
            return Err(Error::Lut(
                "a 1D LUT needs at least 2 entries per channel".into(),
            ));
        }
        if (0..3).any(|c| self.domain_max[c] <= self.domain_min[c]) {
            return Err(Error::Lut("a 1D LUT domain must increase".into()));
        }
        Ok(())
    }

    pub(crate) fn apply(&self, rgb: [f64; 3]) -> [f64; 3] {
        let last = (self.len() - 1) as f64;
        std::array::from_fn(|c| {
            let t = ((rgb[c] - self.domain_min[c]) / (self.domain_max[c] - self.domain_min[c])
                * last)
                .clamp(0., last);
            let i = (t.floor() as usize).min(self.len() - 2);
            let f = t - i as f64;
            let v = &self.values[c];
            f64::from(v[i]) * (1. - f) + f64::from(v[i + 1]) * f
        })
    }

    pub(crate) fn is_monotonic(&self) -> bool {
        self.values
            .iter()
            .all(|v| v.windows(2).all(|w| w[1] >= w[0]) || v.windows(2).all(|w| w[1] <= w[0]))
    }

    /// The input a monotonic table maps to `rgb`, clamped to the table's
    /// output range.
    pub(crate) fn apply_inverse(&self, rgb: [f64; 3]) -> [f64; 3] {
        let last = (self.len() - 1) as f64;
        std::array::from_fn(|c| {
            let v = &self.values[c];
            let rising = v[v.len() - 1] >= v[0];
            let at = |i: usize| f64::from(if rising { v[i] } else { v[v.len() - 1 - i] });
            let y = rgb[c].clamp(at(0), at(v.len() - 1));
            // First entry not below y.
            let mut lo = 0;
            let mut hi = v.len() - 1;
            while hi - lo > 1 {
                let mid = (lo + hi) / 2;
                if at(mid) < y {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let span = at(hi) - at(lo);
            let f = if span > 0. { (y - at(lo)) / span } else { 0. };
            let mut t = lo as f64 + f;
            if !rising {
                t = last - t;
            }
            self.domain_min[c] + t / last * (self.domain_max[c] - self.domain_min[c])
        })
    }
}

/// An `n³` lattice with red changing fastest.
#[derive(Clone, Debug, PartialEq)]
pub struct Lut3d {
    pub size: usize,
    pub domain_min: [f64; 3],
    pub domain_max: [f64; 3],
    pub data: Vec<[f32; 3]>,
}

impl Lut3d {
    fn check(&self) -> Result<()> {
        if self.size < 2 || self.size > 256 || self.data.len() != self.size.pow(3) {
            return Err(Error::Lut(format!(
                "a 3D LUT of size {} needs {} entries, found {}",
                self.size,
                self.size.pow(3),
                self.data.len()
            )));
        }
        if (0..3).any(|c| self.domain_max[c] <= self.domain_min[c]) {
            return Err(Error::Lut("a 3D LUT domain must increase".into()));
        }
        Ok(())
    }

    #[inline]
    fn at(&self, r: usize, g: usize, b: usize) -> [f64; 3] {
        self.data[r + self.size * (g + self.size * b)].map(f64::from)
    }

    pub(crate) fn apply(&self, rgb: [f64; 3], interp: Interpolation) -> [f64; 3] {
        let last = (self.size - 1) as f64;
        let pos: [f64; 3] = std::array::from_fn(|c| {
            ((rgb[c] - self.domain_min[c]) / (self.domain_max[c] - self.domain_min[c]) * last)
                .clamp(0., last)
        });
        if interp == Interpolation::Nearest {
            let [r, g, b] = pos.map(|p| p.round() as usize);
            return self.at(r, g, b);
        }
        let i: [usize; 3] = pos.map(|p| (p.floor() as usize).min(self.size - 2));
        let [fr, fg, fb]: [f64; 3] = std::array::from_fn(|c| pos[c] - i[c] as f64);
        let [r, g, b] = i;
        let c000 = self.at(r, g, b);
        let c111 = self.at(r + 1, g + 1, b + 1);
        let lerp = |a: [f64; 3], b: [f64; 3], t: f64| -> [f64; 3] {
            std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
        };
        if interp == Interpolation::Linear {
            let c100 = self.at(r + 1, g, b);
            let c010 = self.at(r, g + 1, b);
            let c110 = self.at(r + 1, g + 1, b);
            let c001 = self.at(r, g, b + 1);
            let c101 = self.at(r + 1, g, b + 1);
            let c011 = self.at(r, g + 1, b + 1);
            let x00 = lerp(c000, c100, fr);
            let x10 = lerp(c010, c110, fr);
            let x01 = lerp(c001, c101, fr);
            let x11 = lerp(c011, c111, fr);
            return lerp(lerp(x00, x10, fg), lerp(x01, x11, fg), fb);
        }
        // Tetrahedral: walk from c000 to c111 through the two corners the
        // fractions' order picks.
        let (a, wa, b2, wb, c2, wc) = if fr > fg {
            if fg > fb {
                (
                    self.at(r + 1, g, b),
                    fr,
                    self.at(r + 1, g + 1, b),
                    fg,
                    c111,
                    fb,
                )
            } else if fr > fb {
                (
                    self.at(r + 1, g, b),
                    fr,
                    self.at(r + 1, g, b + 1),
                    fb,
                    c111,
                    fg,
                )
            } else {
                (
                    self.at(r, g, b + 1),
                    fb,
                    self.at(r + 1, g, b + 1),
                    fr,
                    c111,
                    fg,
                )
            }
        } else if fb > fg {
            (
                self.at(r, g, b + 1),
                fb,
                self.at(r, g + 1, b + 1),
                fg,
                c111,
                fr,
            )
        } else if fb > fr {
            (
                self.at(r, g + 1, b),
                fg,
                self.at(r, g + 1, b + 1),
                fb,
                c111,
                fr,
            )
        } else {
            (
                self.at(r, g + 1, b),
                fg,
                self.at(r + 1, g + 1, b),
                fr,
                c111,
                fb,
            )
        };
        std::array::from_fn(|k| {
            c000[k] + wa * (a[k] - c000[k]) + wb * (b2[k] - a[k]) + wc * (c2[k] - b2[k])
        })
    }
}

fn bad(file: &str, line: usize, what: impl std::fmt::Display) -> Error {
    Error::Lut(format!("{file}, line {line}: {what}"))
}

fn floats<'a>(words: impl Iterator<Item = &'a str>, file: &str, line: usize) -> Result<Vec<f64>> {
    words
        .map(|w| {
            w.parse::<f64>()
                .map_err(|_| bad(file, line, format!("“{w}” is not a number")))
        })
        .collect()
}

/// Resolve or Iridas `.cube`.
pub fn parse_cube(text: &str, file: &str) -> Result<Vec<Op>> {
    let mut size_1d = None;
    let mut size_3d = None;
    let mut dmin = [0.; 3];
    let mut dmax = [1.; 3];
    let mut range_1d = None;
    let mut range_3d = None;
    let mut rows: Vec<[f32; 3]> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = n + 1;
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let mut words = content.split_whitespace();
        let key = words.next().unwrap_or_default();
        let int = |words: &mut std::str::SplitWhitespace| -> Result<usize> {
            words
                .next()
                .and_then(|w| w.parse().ok())
                .ok_or_else(|| bad(file, line, format!("{key} needs a size")))
        };
        match key {
            "TITLE" => {}
            "LUT_1D_SIZE" => size_1d = Some(int(&mut words)?),
            "LUT_3D_SIZE" => size_3d = Some(int(&mut words)?),
            "DOMAIN_MIN" | "DOMAIN_MAX" => {
                let v = floats(words, file, line)?;
                let v: [f64; 3] = v
                    .try_into()
                    .map_err(|_| bad(file, line, format!("{key} needs 3 numbers")))?;
                if key == "DOMAIN_MIN" {
                    dmin = v;
                } else {
                    dmax = v;
                }
            }
            "LUT_1D_INPUT_RANGE" | "LUT_3D_INPUT_RANGE" => {
                let v = floats(words, file, line)?;
                let [lo, hi] = v[..] else {
                    return Err(bad(file, line, format!("{key} needs 2 numbers")));
                };
                if key == "LUT_1D_INPUT_RANGE" {
                    range_1d = Some((lo, hi));
                } else {
                    range_3d = Some((lo, hi));
                }
            }
            k if k.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) => {
                return Err(bad(file, line, format!("unknown keyword {k}")));
            }
            _ => {
                let v = floats(content.split_whitespace(), file, line)?;
                let [r, g, b] = v[..] else {
                    return Err(bad(file, line, "a LUT entry needs 3 numbers"));
                };
                rows.push([r as f32, g as f32, b as f32]);
            }
        }
    }
    let mut ops = Vec::new();
    let mut used = 0;
    if let Some(n) = size_1d {
        if rows.len() < n {
            return Err(Error::Lut(format!("{file}: expected {n} 1D LUT entries")));
        }
        let (lo, hi) = range_1d.map_or((dmin, dmax), |(a, b)| ([a; 3], [b; 3]));
        let lut = Lut1d {
            domain_min: lo,
            domain_max: hi,
            values: std::array::from_fn(|c| rows[..n].iter().map(|r| r[c]).collect()),
        };
        lut.check()?;
        ops.push(Op::Lut1d {
            lut: Arc::new(lut),
            inverse: false,
        });
        used = n;
    }
    if let Some(n) = size_3d {
        // With a 1D shaper first, the 3D LUT's domain is the shaper's
        // output, which the file states with LUT_3D_INPUT_RANGE (if at all).
        let (lo, hi) = match range_3d {
            Some((a, b)) => ([a; 3], [b; 3]),
            None if size_1d.is_some() => ([0.; 3], [1.; 3]),
            None => (dmin, dmax),
        };
        let lut = Lut3d {
            size: n,
            domain_min: lo,
            domain_max: hi,
            data: rows[used..].to_vec(),
        };
        lut.check()
            .map_err(|e| Error::Lut(format!("{file}: {e}")))?;
        ops.push(Op::Lut3d {
            lut: Arc::new(lut),
            interp: Interpolation::Linear,
        });
        used = rows.len();
    }
    if ops.is_empty() {
        return Err(Error::Lut(format!("{file}: no LUT_1D_SIZE or LUT_3D_SIZE")));
    }
    if used != rows.len() {
        return Err(Error::Lut(format!(
            "{file}: {} LUT entries left over",
            rows.len() - used
        )));
    }
    Ok(ops)
}

/// Sony Imageworks `.spi1d`.
pub fn parse_spi1d(text: &str, file: &str) -> Result<Lut1d> {
    let mut from = (0., 1.);
    let mut length = None;
    let mut components = 1;
    let mut values: Vec<Vec<f64>> = Vec::new();
    let mut inside = false;
    for (n, raw) in text.lines().enumerate() {
        let line = n + 1;
        let content = raw.trim();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        if inside {
            if content.starts_with('}') {
                inside = false;
                continue;
            }
            let v = floats(content.split_whitespace(), file, line)?;
            if v.len() != components {
                return Err(bad(file, line, format!("expected {components} values")));
            }
            values.push(v);
            continue;
        }
        let mut words = content.split_whitespace();
        match words.next().unwrap_or_default() {
            "Version" => {}
            "From" => {
                let v = floats(words, file, line)?;
                let [a, b] = v[..] else {
                    return Err(bad(file, line, "From needs 2 numbers"));
                };
                from = (a, b);
            }
            "Length" => {
                length = words.next().and_then(|w| w.parse::<usize>().ok());
            }
            "Components" => {
                components = words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
                if !(1..=3).contains(&components) {
                    return Err(bad(file, line, "Components must be 1, 2 or 3"));
                }
            }
            "{" => inside = true,
            other => return Err(bad(file, line, format!("unknown keyword {other}"))),
        }
    }
    let length = length.ok_or_else(|| Error::Lut(format!("{file}: no Length")))?;
    if values.len() != length {
        return Err(Error::Lut(format!(
            "{file}: Length says {length} entries, found {}",
            values.len()
        )));
    }
    let lut = Lut1d {
        domain_min: [from.0; 3],
        domain_max: [from.1; 3],
        values: std::array::from_fn(|c| {
            values
                .iter()
                .map(|v| {
                    // Two components leave blue unchanged in OCIO; one
                    // component applies to all three.
                    let k = if components == 1 {
                        0
                    } else {
                        c.min(components - 1)
                    };
                    v[k] as f32
                })
                .collect()
        }),
    };
    if components == 2 {
        return Err(Error::Unsupported(format!(
            "{file}: two-component .spi1d LUTs"
        )));
    }
    lut.check()?;
    Ok(lut)
}

/// Sony Imageworks `.spi3d`.
pub fn parse_spi3d(text: &str, file: &str) -> Result<Lut3d> {
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(n, l)| (n + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#'));
    let header = lines.next().map(|(_, l)| l).unwrap_or_default();
    if !header.starts_with("SPILUT") {
        return Err(Error::Lut(format!("{file}: not an SPILUT file")));
    }
    let _ = lines.next(); // "3 3"
    let (line, sizes) = lines
        .next()
        .ok_or_else(|| Error::Lut(format!("{file}: missing LUT size")))?;
    let sizes = floats(sizes.split_whitespace(), file, line)?;
    let [a, b, c] = sizes[..] else {
        return Err(bad(file, line, "the size line needs 3 numbers"));
    };
    if a != b || b != c {
        return Err(bad(file, line, "only cubic 3D LUTs are supported"));
    }
    let size = a as usize;
    if !(2..=256).contains(&size) {
        return Err(bad(file, line, "3D LUT size must be 2–256"));
    }
    let mut data = vec![[f32::NAN; 3]; size.pow(3)];
    let mut count = 0;
    for (line, l) in lines {
        let v = floats(l.split_whitespace(), file, line)?;
        let [r, g, b, x, y, z] = v[..] else {
            return Err(bad(file, line, "an entry needs 3 indices and 3 values"));
        };
        let idx = [r, g, b].map(|i| i as usize);
        if idx.iter().any(|&i| i >= size) {
            return Err(bad(file, line, "index outside the LUT"));
        }
        data[idx[0] + size * (idx[1] + size * idx[2])] = [x as f32, y as f32, z as f32];
        count += 1;
    }
    if count != data.len() || data.iter().any(|v| v[0].is_nan()) {
        return Err(Error::Lut(format!("{file}: missing 3D LUT entries")));
    }
    let lut = Lut3d {
        size,
        domain_min: [0.; 3],
        domain_max: [1.; 3],
        data,
    };
    lut.check()?;
    Ok(lut)
}

/// Read a LUT file into ops by its extension.
pub fn load(path: &std::path::Path) -> Result<Vec<Op>> {
    let name = path.display().to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    const MAX: u64 = 256 * 1024 * 1024;
    let meta = std::fs::metadata(path).map_err(|e| Error::Lut(format!("{name}: {e}")))?;
    if meta.len() > MAX {
        return Err(Error::Lut(format!("{name}: larger than 256 MiB")));
    }
    let text = std::fs::read_to_string(path).map_err(|e| Error::Lut(format!("{name}: {e}")))?;
    match ext.as_str() {
        "cube" => parse_cube(&text, &name),
        "spi1d" => Ok(vec![Op::Lut1d {
            lut: Arc::new(parse_spi1d(&text, &name)?),
            inverse: false,
        }]),
        "spi3d" => Ok(vec![Op::Lut3d {
            lut: Arc::new(parse_spi3d(&text, &name)?),
            interp: Interpolation::Linear,
        }]),
        "clf" | "ctf" => super::clf::parse(&text, &name),
        other => Err(Error::Unsupported(format!(
            "LUT format “.{other}” ({name}); supported: .cube, .spi1d, .spi3d, .clf, .ctf"
        ))),
    }
}
