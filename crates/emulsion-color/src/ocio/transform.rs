//! OCIO transforms as written in a config, parsed from YAML. Parsing never
//! fails on a transform kind this crate cannot run: it becomes
//! [`Transform::Unsupported`] and compiling it reports the kind by name, so
//! one exotic colour space does not stop the rest of a config from loading.

use super::{Error, Result};
use serde_yaml::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Forward,
    Inverse,
}

impl Direction {
    pub fn flip(self) -> Self {
        match self {
            Self::Forward => Self::Inverse,
            Self::Inverse => Self::Forward,
        }
    }

    /// `self` applied inside a transform running in `outer` direction.
    pub fn within(self, outer: Direction) -> Self {
        if outer == Self::Inverse {
            self.flip()
        } else {
            self
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "forward" => Ok(Self::Forward),
            "inverse" => Ok(Self::Inverse),
            other => Err(Error::Config(format!("unknown direction “{other}”"))),
        }
    }
}

/// How power functions treat negative input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NegativeStyle {
    Clamp,
    Mirror,
    PassThru,
    /// Only for curves with a linear segment: extend that segment.
    Linear,
}

/// LUT interpolation requested by a FileTransform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Interpolation {
    Nearest,
    #[default]
    Linear,
    Tetrahedral,
}

/// Parameters of the log family (LogTransform, LogAffineTransform,
/// LogCameraTransform). Forward is linear → log.
#[derive(Clone, Debug, PartialEq)]
pub struct LogParams {
    pub base: f64,
    pub log_slope: [f64; 3],
    pub log_offset: [f64; 3],
    pub lin_slope: [f64; 3],
    pub lin_offset: [f64; 3],
    /// LogCameraTransform: below this linear value a straight line is used.
    pub lin_break: Option<[f64; 3]>,
    pub linear_slope: Option<[f64; 3]>,
}

impl LogParams {
    pub fn plain(base: f64) -> Self {
        Self {
            base,
            log_slope: [1.; 3],
            log_offset: [0.; 3],
            lin_slope: [1.; 3],
            lin_offset: [0.; 3],
            lin_break: None,
            linear_slope: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Transform {
    ColorSpace {
        src: String,
        dst: String,
        data_bypass: bool,
        dir: Direction,
    },
    Look {
        src: String,
        dst: String,
        looks: String,
        dir: Direction,
    },
    /// Row-major 4×4 matrix and offset; only RGB are used (alpha is never
    /// changed).
    Matrix {
        matrix: [f64; 16],
        offset: [f64; 4],
        dir: Direction,
    },
    /// ExponentTransform (`offset: None`) or ExponentWithLinearTransform.
    /// Forward decodes: `x^gamma`, or the sRGB-style curve.
    Exponent {
        gamma: [f64; 3],
        offset: Option<[f64; 3]>,
        negative: NegativeStyle,
        dir: Direction,
    },
    Log {
        params: LogParams,
        dir: Direction,
    },
    Cdl {
        slope: [f64; 3],
        offset: [f64; 3],
        power: [f64; 3],
        sat: f64,
        clamp: bool,
        dir: Direction,
    },
    Range {
        min_in: Option<f64>,
        max_in: Option<f64>,
        min_out: Option<f64>,
        max_out: Option<f64>,
        clamp: bool,
        dir: Direction,
    },
    Group {
        children: Vec<Transform>,
        dir: Direction,
    },
    File {
        src: String,
        cccid: Option<String>,
        interpolation: Interpolation,
        dir: Direction,
    },
    Builtin {
        style: String,
        dir: Direction,
    },
    FixedFunction {
        style: String,
        params: Vec<f64>,
        dir: Direction,
    },
    /// A transform kind this crate does not implement, by its YAML tag.
    Unsupported {
        kind: String,
    },
}

impl Transform {
    pub fn kind(&self) -> &str {
        match self {
            Self::ColorSpace { .. } => "ColorSpaceTransform",
            Self::Look { .. } => "LookTransform",
            Self::Matrix { .. } => "MatrixTransform",
            Self::Exponent { offset: None, .. } => "ExponentTransform",
            Self::Exponent { .. } => "ExponentWithLinearTransform",
            Self::Log { .. } => "LogTransform",
            Self::Cdl { .. } => "CDLTransform",
            Self::Range { .. } => "RangeTransform",
            Self::Group { .. } => "GroupTransform",
            Self::File { .. } => "FileTransform",
            Self::Builtin { .. } => "BuiltinTransform",
            Self::FixedFunction { .. } => "FixedFunctionTransform",
            Self::Unsupported { kind } => kind,
        }
    }
}

/// A YAML tag without the `!` or `!<…>` decoration.
pub(crate) fn tag_name(tag: &serde_yaml::value::Tag) -> String {
    tag.to_string()
        .trim_start_matches('!')
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}

/// Parse YAML, keeping OCIO's verbatim tags (`!<ColorSpace>`): serde_yaml
/// drops verbatim tags, so they are rewritten as local ones (`!ColorSpace`)
/// first.
pub(crate) fn parse_yaml(yaml: &str) -> Result<Value> {
    let mut text = String::with_capacity(yaml.len());
    let mut rest = yaml;
    while let Some(i) = rest.find("!<") {
        let after = &rest[i + 2..];
        match after.find('>') {
            Some(end)
                if after[..end]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_') =>
            {
                text.push_str(&rest[..i + 1]);
                text.push_str(&after[..end]);
                rest = &after[end + 1..];
            }
            _ => {
                text.push_str(&rest[..i + 2]);
                rest = after;
            }
        }
    }
    text.push_str(rest);
    serde_yaml::from_str(&text).map_err(|e| Error::Config(format!("not valid YAML: {e}")))
}

/// A scalar as text (OCIO names may look like numbers).
pub(crate) fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Tagged(t) => text(&t.value),
        _ => None,
    }
}

pub(crate) fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub(crate) fn boolean(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" => Some(true),
            "false" | "no" | "off" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn numbers(value: &Value, what: &str) -> Result<Vec<f64>> {
    match value {
        Value::Sequence(items) => items
            .iter()
            .map(|v| number(v).ok_or_else(|| Error::Config(format!("{what} must be numbers"))))
            .collect(),
        other => number(other)
            .map(|n| vec![n])
            .ok_or_else(|| Error::Config(format!("{what} must be numbers"))),
    }
}

/// One number for all three channels, or three numbers.
fn rgb(value: &Value, what: &str) -> Result<[f64; 3]> {
    match numbers(value, what)?.as_slice() {
        [v] => Ok([*v; 3]),
        [r, g, b] | [r, g, b, _] => Ok([*r, *g, *b]),
        other => Err(Error::Config(format!(
            "{what} needs 1 or 3 numbers, found {}",
            other.len()
        ))),
    }
}

fn negative_style(value: &str) -> Result<NegativeStyle> {
    match value.to_ascii_lowercase().as_str() {
        "clamp" => Ok(NegativeStyle::Clamp),
        "mirror" => Ok(NegativeStyle::Mirror),
        "pass_thru" | "passthru" => Ok(NegativeStyle::PassThru),
        "linear" => Ok(NegativeStyle::Linear),
        other => Err(Error::Config(format!("unknown negative style “{other}”"))),
    }
}

pub(crate) fn interpolation(value: &str) -> Result<Interpolation> {
    match value.to_ascii_lowercase().as_str() {
        "nearest" => Ok(Interpolation::Nearest),
        "linear" | "default" | "unknown" => Ok(Interpolation::Linear),
        "tetrahedral" | "best" => Ok(Interpolation::Tetrahedral),
        "cubic" => Err(Error::Unsupported("cubic LUT interpolation".into())),
        other => Err(Error::Config(format!("unknown interpolation “{other}”"))),
    }
}

/// The key/value pairs of a (possibly tagged) mapping.
pub(crate) fn mapping(value: &Value) -> Option<&serde_yaml::Mapping> {
    match value {
        Value::Mapping(m) => Some(m),
        Value::Tagged(t) => mapping(&t.value),
        _ => None,
    }
}

pub(crate) fn get<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a Value> {
    map.get(key).filter(|v| !v.is_null())
}

pub(crate) fn get_text(map: &serde_yaml::Mapping, key: &str) -> Option<String> {
    get(map, key).and_then(text)
}

/// Parse one transform node, e.g. `!<MatrixTransform> {matrix: [...]}`.
pub fn parse(value: &Value) -> Result<Transform> {
    let (kind, body) = match value {
        Value::Tagged(t) => (tag_name(&t.tag), &t.value),
        // A bare list is a group.
        Value::Sequence(_) => {
            return Ok(Transform::Group {
                children: parse_list(value)?,
                dir: Direction::Forward,
            });
        }
        _ => {
            return Err(Error::Config(
                "a transform needs a !<…Transform> tag".into(),
            ));
        }
    };
    let empty = serde_yaml::Mapping::new();
    let map = mapping(body).unwrap_or(&empty);
    let dir = match get_text(map, "direction") {
        Some(d) => Direction::parse(&d)?,
        None => Direction::Forward,
    };
    let need = |key: &str| {
        get_text(map, key).ok_or_else(|| Error::Config(format!("{kind} needs “{key}”")))
    };
    Ok(match kind.as_str() {
        "ColorSpaceTransform" => Transform::ColorSpace {
            src: need("src")?,
            dst: need("dst")?,
            data_bypass: get(map, "data_bypass").and_then(boolean).unwrap_or(true),
            dir,
        },
        "LookTransform" => Transform::Look {
            src: need("src")?,
            dst: need("dst")?,
            looks: get_text(map, "looks").unwrap_or_default(),
            dir,
        },
        "MatrixTransform" => {
            let mut matrix = [0.; 16];
            for i in 0..4 {
                matrix[i * 5] = 1.;
            }
            if let Some(v) = get(map, "matrix") {
                let m = numbers(v, "matrix")?;
                if m.len() != 16 {
                    return Err(Error::Config(format!(
                        "MatrixTransform matrix needs 16 numbers, found {}",
                        m.len()
                    )));
                }
                matrix.copy_from_slice(&m);
            }
            let mut offset = [0.; 4];
            if let Some(v) = get(map, "offset") {
                let o = numbers(v, "offset")?;
                if o.len() != 4 {
                    return Err(Error::Config(format!(
                        "MatrixTransform offset needs 4 numbers, found {}",
                        o.len()
                    )));
                }
                offset.copy_from_slice(&o);
            }
            Transform::Matrix {
                matrix,
                offset,
                dir,
            }
        }
        "ExponentTransform" => Transform::Exponent {
            gamma: rgb(
                get(map, "value")
                    .ok_or_else(|| Error::Config("ExponentTransform needs “value”".into()))?,
                "ExponentTransform value",
            )?,
            offset: None,
            negative: match get_text(map, "style") {
                Some(s) => negative_style(&s)?,
                None => NegativeStyle::Clamp,
            },
            dir,
        },
        "ExponentWithLinearTransform" => Transform::Exponent {
            gamma: rgb(
                get(map, "gamma").ok_or_else(|| {
                    Error::Config("ExponentWithLinearTransform needs “gamma”".into())
                })?,
                "gamma",
            )?,
            offset: Some(rgb(
                get(map, "offset").ok_or_else(|| {
                    Error::Config("ExponentWithLinearTransform needs “offset”".into())
                })?,
                "offset",
            )?),
            negative: match get_text(map, "style") {
                Some(s) => negative_style(&s)?,
                None => NegativeStyle::Linear,
            },
            dir,
        },
        "LogTransform" | "LogAffineTransform" | "LogCameraTransform" => {
            let mut p = LogParams::plain(get(map, "base").and_then(number).unwrap_or(2.));
            if kind == "LogTransform" {
                p.base = get(map, "base").and_then(number).unwrap_or(2.);
            } else {
                let set = |key: &str, slot: &mut [f64; 3]| -> Result<()> {
                    if let Some(v) = get(map, key) {
                        *slot = rgb(v, key)?;
                    }
                    Ok(())
                };
                set("log_side_slope", &mut p.log_slope)?;
                set("log_side_offset", &mut p.log_offset)?;
                set("lin_side_slope", &mut p.lin_slope)?;
                set("lin_side_offset", &mut p.lin_offset)?;
                if kind == "LogCameraTransform" {
                    p.lin_break = Some(rgb(
                        get(map, "lin_side_break").ok_or_else(|| {
                            Error::Config("LogCameraTransform needs “lin_side_break”".into())
                        })?,
                        "lin_side_break",
                    )?);
                    if let Some(v) = get(map, "linear_slope") {
                        p.linear_slope = Some(rgb(v, "linear_slope")?);
                    }
                }
            }
            if !(p.base > 0. && p.base != 1.) {
                return Err(Error::Config(format!(
                    "{kind} base must be positive and not 1"
                )));
            }
            Transform::Log { params: p, dir }
        }
        "CDLTransform" => {
            let triple = |key: &str, default: f64| -> Result<[f64; 3]> {
                get(map, key).map_or(Ok([default; 3]), |v| rgb(v, key))
            };
            Transform::Cdl {
                slope: triple("slope", 1.)?,
                offset: triple("offset", 0.)?,
                power: triple("power", 1.)?,
                sat: get(map, "sat")
                    .or_else(|| get(map, "saturation"))
                    .and_then(number)
                    .unwrap_or(1.),
                // OCIO v2's default is no clamping; "asc" is the v1.2 ASC
                // clamping style.
                clamp: get_text(map, "style").is_some_and(|s| s.eq_ignore_ascii_case("asc")),
                dir,
            }
        }
        "RangeTransform" => {
            let v = |key: &str| get(map, key).and_then(number);
            Transform::Range {
                min_in: v("min_in_value"),
                max_in: v("max_in_value"),
                min_out: v("min_out_value"),
                max_out: v("max_out_value"),
                clamp: !get_text(map, "style").is_some_and(|s| s.eq_ignore_ascii_case("noclamp")),
                dir,
            }
        }
        "GroupTransform" => Transform::Group {
            children: match get(map, "children") {
                Some(list) => parse_list(list)?,
                None => Vec::new(),
            },
            dir,
        },
        "FileTransform" => Transform::File {
            src: need("src")?,
            cccid: get_text(map, "cccid"),
            interpolation: match get_text(map, "interpolation") {
                Some(i) => interpolation(&i)?,
                None => Interpolation::Linear,
            },
            dir,
        },
        "BuiltinTransform" => Transform::Builtin {
            style: need("style")?,
            dir,
        },
        "FixedFunctionTransform" => Transform::FixedFunction {
            style: need("style")?,
            params: match get(map, "params") {
                Some(v) => numbers(v, "params")?,
                None => Vec::new(),
            },
            dir,
        },
        _ => Transform::Unsupported { kind },
    })
}

fn parse_list(value: &Value) -> Result<Vec<Transform>> {
    match value {
        Value::Sequence(items) => items.iter().map(parse).collect(),
        other => Ok(vec![parse(other)?]),
    }
}
