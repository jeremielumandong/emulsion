//! Academy/ASC Common LUT Format (`.clf`) and Autodesk CTF (`.ctf`), the
//! subset of process nodes most LUTs use: Matrix, LUT1D, LUT3D, Range, Log,
//! Exponent and ASC_CDL. Any other node is refused by name.

use super::lut::{Lut1d, Lut3d};
use super::ops::{self, Op};
use super::transform::{Interpolation, LogParams, NegativeStyle};
use super::{Error, Result};
use std::sync::Arc;

/// The code value 1.0 maps to, for a CLF bit depth.
fn depth_scale(depth: Option<&str>) -> Result<f64> {
    Ok(match depth.unwrap_or("32f") {
        "8i" => 255.,
        "10i" => 1023.,
        "12i" => 4095.,
        "16i" => 65535.,
        "16f" | "32f" => 1.,
        other => return Err(Error::Lut(format!("unknown bit depth “{other}”"))),
    })
}

fn numbers(text: &str, file: &str, node: &str) -> Result<Vec<f64>> {
    text.split_whitespace()
        .map(|w| {
            w.parse::<f64>()
                .map_err(|_| Error::Lut(format!("{file}: {node} has “{w}”, not a number")))
        })
        .collect()
}

fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    node.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
}

fn array(node: roxmltree::Node, file: &str) -> Result<(Vec<usize>, Vec<f64>)> {
    let name = node.tag_name().name();
    let arr =
        child(node, "Array").ok_or_else(|| Error::Lut(format!("{file}: {name} has no Array")))?;
    let dims: Vec<usize> = arr
        .attribute("dim")
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|d| d.parse().ok())
        .collect();
    let values = numbers(arr.text().unwrap_or_default(), file, name)?;
    Ok((dims, values))
}

fn float_attr(node: roxmltree::Node, key: &str) -> Option<f64> {
    node.attribute(key).and_then(|v| v.trim().parse().ok())
}

pub fn parse(text: &str, file: &str) -> Result<Vec<Op>> {
    let doc = roxmltree::Document::parse(text)
        .map_err(|e| Error::Lut(format!("{file}: not valid XML ({e})")))?;
    let root = doc.root_element();
    if root.tag_name().name() != "ProcessList" {
        return Err(Error::Lut(format!(
            "{file}: the root element is not ProcessList"
        )));
    }
    let mut out = Vec::new();
    for node in root.children().filter(|n| n.is_element()) {
        let name = node.tag_name().name();
        if matches!(
            name,
            "Description" | "InputDescriptor" | "OutputDescriptor" | "Info"
        ) {
            continue;
        }
        let in_scale = depth_scale(node.attribute("inBitDepth"))?;
        let out_scale = depth_scale(node.attribute("outBitDepth"))?;
        match name {
            "Matrix" => {
                let (dims, v) = array(node, file)?;
                let (rows, cols) = match dims.as_slice() {
                    [r, c] | [r, c, _] => (*r, *c),
                    _ => {
                        return Err(Error::Lut(format!(
                            "{file}: Matrix dim is not rows × columns"
                        )));
                    }
                };
                if rows < 3 || !(3..=5).contains(&cols) || v.len() != rows * cols {
                    return Err(Error::Lut(format!(
                        "{file}: Matrix {rows}×{cols} with {} values",
                        v.len()
                    )));
                }
                let k = in_scale / out_scale;
                let m: [[f64; 3]; 3] =
                    std::array::from_fn(|r| std::array::from_fn(|c| v[r * cols + c] * k));
                // 3×4 (or 4×5 in CTF) carries offsets in the last column.
                let offset: [f64; 3] = if cols == 4 && rows == 3 || cols == 5 {
                    std::array::from_fn(|r| v[r * cols + cols - 1] / out_scale)
                } else {
                    [0.; 3]
                };
                out.push(Op::Matrix {
                    m: moxcms::Matrix3d { v: m },
                    offset,
                });
            }
            "LUT1D" | "InverseLUT1D" => {
                if node.attribute("halfDomain").is_some_and(|v| v == "true") {
                    return Err(Error::Unsupported(format!("{file}: half-domain LUT1D")));
                }
                if child(node, "IndexMap").is_some() {
                    return Err(Error::Unsupported(format!("{file}: LUT1D IndexMap")));
                }
                let (dims, v) = array(node, file)?;
                let (n, comps) = match dims.as_slice() {
                    [n, c] => (*n, *c),
                    _ => {
                        return Err(Error::Lut(format!(
                            "{file}: LUT1D dim is not entries × channels"
                        )));
                    }
                };
                if !matches!(comps, 1 | 3) || v.len() != n * comps {
                    return Err(Error::Lut(format!(
                        "{file}: LUT1D has {} values for {n}×{comps}",
                        v.len()
                    )));
                }
                let lut = Lut1d {
                    domain_min: [0.; 3],
                    domain_max: [1.; 3],
                    values: std::array::from_fn(|c| {
                        (0..n)
                            .map(|i| (v[i * comps + c.min(comps - 1)] / out_scale) as f32)
                            .collect()
                    }),
                };
                if lut.len() < 2 {
                    return Err(Error::Lut(format!("{file}: LUT1D needs 2 entries")));
                }
                out.push(Op::Lut1d {
                    lut: Arc::new(lut),
                    inverse: name == "InverseLUT1D",
                });
            }
            "LUT3D" => {
                let (dims, v) = array(node, file)?;
                let n = match dims.as_slice() {
                    [a, b, c, 3] if a == b && b == c => *a,
                    _ => return Err(Error::Lut(format!("{file}: LUT3D dim must be n n n 3"))),
                };
                if v.len() != n * n * n * 3 || !(2..=256).contains(&n) {
                    return Err(Error::Lut(format!("{file}: LUT3D has {} values", v.len())));
                }
                // CLF orders entries with blue changing fastest; the lattice
                // here has red fastest.
                let mut data = vec![[0f32; 3]; n * n * n];
                for r in 0..n {
                    for g in 0..n {
                        for b in 0..n {
                            let i = ((r * n + g) * n + b) * 3;
                            data[r + n * (g + n * b)] =
                                [v[i], v[i + 1], v[i + 2]].map(|x| (x / out_scale) as f32);
                        }
                    }
                }
                let interp = match node.attribute("interpolation") {
                    Some("tetrahedral") => Interpolation::Tetrahedral,
                    _ => Interpolation::Linear,
                };
                out.push(Op::Lut3d {
                    lut: Arc::new(Lut3d {
                        size: n,
                        domain_min: [0.; 3],
                        domain_max: [1.; 3],
                        data,
                    }),
                    interp,
                });
            }
            "Range" => {
                let value = |key: &str, scale: f64| {
                    child(node, key)
                        .and_then(|c| c.text())
                        .and_then(|t| t.trim().parse::<f64>().ok())
                        .map(|v| v / scale)
                };
                let clamp = !node
                    .attribute("style")
                    .is_some_and(|s| s.eq_ignore_ascii_case("noClamp"));
                out.push(ops::range(
                    value("minInValue", in_scale),
                    value("maxInValue", in_scale),
                    value("minOutValue", out_scale),
                    value("maxOutValue", out_scale),
                    clamp,
                )?);
            }
            "Log" => {
                let style = node.attribute("style").unwrap_or_default();
                let (base, inverse, camera) = match style {
                    "log10" => (10., false, false),
                    "antiLog10" => (10., true, false),
                    "log2" => (2., false, false),
                    "antiLog2" => (2., true, false),
                    "linToLog" => (10., false, false),
                    "logToLin" => (10., true, false),
                    "cameraLinToLog" => (10., false, true),
                    "cameraLogToLin" => (10., true, true),
                    other => {
                        return Err(Error::Unsupported(format!("{file}: Log style “{other}”")));
                    }
                };
                let mut p = LogParams::plain(base);
                let params: Vec<_> = node
                    .children()
                    .filter(|c| c.is_element() && c.tag_name().name() == "LogParams")
                    .collect();
                if params.iter().any(|c| c.attribute("gamma").is_some()) {
                    return Err(Error::Unsupported(format!(
                        "{file}: Cineon-style Log parameters (gamma, refWhite)"
                    )));
                }
                let mut brk = [f64::NAN; 3];
                let mut lin_slope_set = [None; 3];
                for lp in params {
                    let channels: Vec<usize> = match lp.attribute("channel") {
                        Some("R") => vec![0],
                        Some("G") => vec![1],
                        Some("B") => vec![2],
                        _ => vec![0, 1, 2],
                    };
                    if let Some(b) = float_attr(lp, "base") {
                        p.base = b;
                    }
                    for c in channels {
                        if let Some(v) = float_attr(lp, "logSideSlope") {
                            p.log_slope[c] = v;
                        }
                        if let Some(v) = float_attr(lp, "logSideOffset") {
                            p.log_offset[c] = v;
                        }
                        if let Some(v) = float_attr(lp, "linSideSlope") {
                            p.lin_slope[c] = v;
                        }
                        if let Some(v) = float_attr(lp, "linSideOffset") {
                            p.lin_offset[c] = v;
                        }
                        if let Some(v) = float_attr(lp, "linSideBreak") {
                            brk[c] = v;
                        }
                        lin_slope_set[c] = float_attr(lp, "linearSlope");
                    }
                }
                if camera {
                    if brk.iter().any(|b| b.is_nan()) {
                        return Err(Error::Lut(format!("{file}: camera Log needs linSideBreak")));
                    }
                    p.lin_break = Some(brk);
                    if lin_slope_set.iter().all(Option::is_some) {
                        p.linear_slope = Some(lin_slope_set.map(Option::unwrap_or_default));
                    }
                }
                out.push(Op::Log { p, inverse });
            }
            "Exponent" => {
                let style = node.attribute("style").unwrap_or_default();
                let (moncurve, negative, inverse) = match style {
                    "basicFwd" => (false, NegativeStyle::Clamp, false),
                    "basicRev" => (false, NegativeStyle::Clamp, true),
                    "basicMirrorFwd" => (false, NegativeStyle::Mirror, false),
                    "basicMirrorRev" => (false, NegativeStyle::Mirror, true),
                    "basicPassThruFwd" => (false, NegativeStyle::PassThru, false),
                    "basicPassThruRev" => (false, NegativeStyle::PassThru, true),
                    "monCurveFwd" => (true, NegativeStyle::Linear, false),
                    "monCurveRev" => (true, NegativeStyle::Linear, true),
                    "monCurveMirrorFwd" => (true, NegativeStyle::Mirror, false),
                    "monCurveMirrorRev" => (true, NegativeStyle::Mirror, true),
                    other => {
                        return Err(Error::Unsupported(format!(
                            "{file}: Exponent style “{other}”"
                        )));
                    }
                };
                let mut gamma = [1.; 3];
                let mut offset = [0.; 3];
                for ep in node
                    .children()
                    .filter(|c| c.is_element() && c.tag_name().name() == "ExponentParams")
                {
                    let channels: Vec<usize> = match ep.attribute("channel") {
                        Some("R") => vec![0],
                        Some("G") => vec![1],
                        Some("B") => vec![2],
                        Some("A") => vec![],
                        _ => vec![0, 1, 2],
                    };
                    for c in channels {
                        if let Some(v) = float_attr(ep, "exponent") {
                            gamma[c] = v;
                        }
                        if let Some(v) = float_attr(ep, "offset") {
                            offset[c] = v;
                        }
                    }
                }
                out.push(Op::Gamma {
                    gamma,
                    offset: moncurve.then_some(offset),
                    negative,
                    inverse,
                });
            }
            "ASC_CDL" => {
                let style = node.attribute("style").unwrap_or("Fwd");
                let (inverse, clamp) = match style {
                    "Fwd" | "v1.2_Fwd" => (false, true),
                    "Rev" | "v1.2_Rev" => (true, true),
                    "FwdNoClamp" => (false, false),
                    "RevNoClamp" => (true, false),
                    other => {
                        return Err(Error::Unsupported(format!(
                            "{file}: ASC_CDL style “{other}”"
                        )));
                    }
                };
                let triple = |parent: Option<roxmltree::Node>,
                              key: &str,
                              default: f64|
                 -> Result<[f64; 3]> {
                    match parent.and_then(|p| child(p, key)).and_then(|c| c.text()) {
                        Some(t) => numbers(t, file, key)?
                            .try_into()
                            .map_err(|_| Error::Lut(format!("{file}: {key} needs 3 numbers"))),
                        None => Ok([default; 3]),
                    }
                };
                let sop = child(node, "SOPNode");
                let sat = child(node, "SatNode")
                    .and_then(|s| child(s, "Saturation"))
                    .and_then(|c| c.text())
                    .and_then(|t| t.trim().parse().ok())
                    .unwrap_or(1.);
                out.push(Op::Cdl {
                    slope: triple(sop, "Slope", 1.)?,
                    offset: triple(sop, "Offset", 0.)?,
                    power: triple(sop, "Power", 1.)?,
                    sat,
                    clamp,
                    inverse,
                });
            }
            other => {
                return Err(Error::Unsupported(format!(
                    "{file}: CLF/CTF node “{other}” (supported: Matrix, LUT1D, LUT3D, Range, Log, Exponent, ASC_CDL)"
                )));
            }
        }
    }
    Ok(out)
}
