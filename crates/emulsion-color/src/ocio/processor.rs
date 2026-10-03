//! Building processors from a config: colour space → colour space through
//! the scene (or display) reference, and colour space → display/view with
//! looks.

use super::config::{ColorSpace, Config, Reference, USE_DISPLAY_NAME, View};
use super::ops::{self, Op, Processor};
use super::transform::{Direction, Transform};
use super::{Error, Result, aces, lut};
use std::sync::Arc;

/// Nesting limit for colour spaces and looks that refer to each other.
const MAX_DEPTH: usize = 32;

struct Compiler<'a> {
    config: &'a Config,
    depth: usize,
}

impl Compiler<'_> {
    fn nested(&self) -> Result<Compiler<'_>> {
        if self.depth >= MAX_DEPTH {
            return Err(Error::Config(
                "colour spaces or looks refer to each other in a loop".into(),
            ));
        }
        Ok(Compiler {
            config: self.config,
            depth: self.depth + 1,
        })
    }

    fn colorspace(&self, name: &str) -> Result<&ColorSpace> {
        self.config
            .colorspace(name)
            .ok_or_else(|| Error::NotFound(format!("colour space “{name}” is not in the config")))
    }

    fn transform(&self, t: &Transform, dir: Direction, out: &mut Vec<Op>) -> Result<()> {
        let leaf = |ops: Vec<Op>, d: Direction, out: &mut Vec<Op>| -> Result<()> {
            let eff = d.within(dir);
            out.extend(if eff == Direction::Inverse {
                ops::invert(ops)?
            } else {
                ops
            });
            Ok(())
        };
        match t {
            Transform::Group { children, dir: d } => {
                let inner = self.nested()?;
                if d.within(dir) == Direction::Forward {
                    for c in children {
                        inner.transform(c, Direction::Forward, out)?;
                    }
                } else {
                    for c in children.iter().rev() {
                        inner.transform(c, Direction::Inverse, out)?;
                    }
                }
            }
            Transform::ColorSpace {
                src,
                dst,
                data_bypass,
                dir: d,
            } => {
                let (a, b) = if d.within(dir) == Direction::Forward {
                    (src, dst)
                } else {
                    (dst, src)
                };
                let (a, b) = (self.colorspace(a)?, self.colorspace(b)?);
                if !(*data_bypass && (a.isdata || b.isdata)) {
                    self.nested()?.convert(a, b, out)?;
                }
            }
            Transform::Look {
                src,
                dst,
                looks,
                dir: d,
            } => {
                let inner = self.nested()?;
                let mut fwd = Vec::new();
                let src = self.colorspace(src)?;
                let current = inner.looks(src, looks, &mut fwd)?;
                inner.convert(current, self.colorspace(dst)?, &mut fwd)?;
                leaf(fwd, *d, out)?;
            }
            Transform::Matrix {
                matrix,
                offset,
                dir: d,
            } => leaf(
                vec![Op::Matrix {
                    m: moxcms::Matrix3d {
                        v: std::array::from_fn(|r| std::array::from_fn(|c| matrix[r * 4 + c])),
                    },
                    offset: [offset[0], offset[1], offset[2]],
                }],
                *d,
                out,
            )?,
            Transform::Exponent {
                gamma,
                offset,
                negative,
                dir: d,
            } => leaf(
                vec![Op::Gamma {
                    gamma: *gamma,
                    offset: *offset,
                    negative: *negative,
                    inverse: false,
                }],
                *d,
                out,
            )?,
            Transform::Log { params, dir: d } => leaf(
                vec![Op::Log {
                    p: params.clone(),
                    inverse: false,
                }],
                *d,
                out,
            )?,
            Transform::Cdl {
                slope,
                offset,
                power,
                sat,
                clamp,
                dir: d,
            } => leaf(
                vec![Op::Cdl {
                    slope: *slope,
                    offset: *offset,
                    power: *power,
                    sat: *sat,
                    clamp: *clamp,
                    inverse: false,
                }],
                *d,
                out,
            )?,
            Transform::Range {
                min_in,
                max_in,
                min_out,
                max_out,
                clamp,
                dir: d,
            } => leaf(
                vec![ops::range(*min_in, *max_in, *min_out, *max_out, *clamp)?],
                *d,
                out,
            )?,
            Transform::File {
                src,
                interpolation,
                dir: d,
                ..
            } => {
                let path = self.config.resolve_file(src)?;
                let cached = self
                    .config
                    .files
                    .lock()
                    .ok()
                    .and_then(|f| f.get(&path).cloned());
                let file_ops = match cached {
                    Some(ops) => ops,
                    None => {
                        let ops = Arc::new(lut::load(&path)?);
                        if let Ok(mut files) = self.config.files.lock() {
                            files.insert(path, ops.clone());
                        }
                        ops
                    }
                };
                let ops = file_ops
                    .iter()
                    .map(|op| match op {
                        Op::Lut3d { lut, .. } => Op::Lut3d {
                            lut: lut.clone(),
                            interp: *interpolation,
                        },
                        other => other.clone(),
                    })
                    .collect();
                leaf(ops, *d, out)?;
            }
            Transform::Builtin { style, dir: d } => leaf(aces::builtin(style)?, *d, out)?,
            Transform::FixedFunction {
                style,
                params,
                dir: d,
            } => leaf(
                vec![Op::Fixed {
                    f: aces::Fixed::from_style(style, params)?,
                    inverse: false,
                }],
                *d,
                out,
            )?,
            Transform::Unsupported { kind } => {
                return Err(Error::Unsupported(format!("transform {kind}")));
            }
        }
        Ok(())
    }

    /// `cs` → its own reference.
    fn to_own_reference(&self, cs: &ColorSpace, out: &mut Vec<Op>) -> Result<()> {
        match (&cs.to_reference, &cs.from_reference) {
            (Some(t), _) => self.transform(t, Direction::Forward, out),
            (None, Some(t)) => self.transform(t, Direction::Inverse, out),
            (None, None) => Ok(()),
        }
        .map_err(|e| context(e, &cs.name))
    }

    fn own_reference_to(&self, cs: &ColorSpace, out: &mut Vec<Op>) -> Result<()> {
        match (&cs.from_reference, &cs.to_reference) {
            (Some(t), _) => self.transform(t, Direction::Forward, out),
            (None, Some(t)) => self.transform(t, Direction::Inverse, out),
            (None, None) => Ok(()),
        }
        .map_err(|e| context(e, &cs.name))
    }

    /// Scene reference → display reference through the default view
    /// transform (or back).
    fn bridge(&self, to_display: bool, out: &mut Vec<Op>) -> Result<()> {
        let vt = self.config.default_view_transform().ok_or_else(|| {
            Error::Config(
                "converting between scene and display colour spaces needs a view transform".into(),
            )
        })?;
        self.scene_view_transform(vt, to_display, out)
    }

    fn scene_view_transform(
        &self,
        vt: &super::config::ViewTransform,
        to_display: bool,
        out: &mut Vec<Op>,
    ) -> Result<()> {
        let dir = |fwd: bool| {
            if fwd {
                Direction::Forward
            } else {
                Direction::Inverse
            }
        };
        match (&vt.from_scene, &vt.to_scene) {
            (Some(t), _) => self.transform(t, dir(to_display), out),
            (None, Some(t)) => self.transform(t, dir(!to_display), out),
            (None, None) => Err(Error::Config(format!(
                "view transform “{}” has no scene-reference transform",
                vt.name
            ))),
        }
    }

    /// `src` → `dst` through their references.
    fn convert(&self, src: &ColorSpace, dst: &ColorSpace, out: &mut Vec<Op>) -> Result<()> {
        if src.name == dst.name || src.isdata || dst.isdata {
            return Ok(());
        }
        self.to_own_reference(src, out)?;
        match (src.reference, dst.reference) {
            (Reference::Scene, Reference::Display) => self.bridge(true, out)?,
            (Reference::Display, Reference::Scene) => self.bridge(false, out)?,
            _ => {}
        }
        self.own_reference_to(dst, out)
    }

    /// Apply a look string (`"a, +b, -c"`) starting in `current`; returns
    /// the space the result is in.
    fn looks<'c>(
        &'c self,
        mut current: &'c ColorSpace,
        looks: &str,
        out: &mut Vec<Op>,
    ) -> Result<&'c ColorSpace> {
        for token in looks
            .split([',', ':'])
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            let (name, inverse) = match token.strip_prefix('-') {
                Some(n) => (n.trim(), true),
                None => (token.trim_start_matches('+').trim(), false),
            };
            let look = self
                .config
                .look(name)
                .ok_or_else(|| Error::NotFound(format!("look “{name}” is not in the config")))?;
            let space = self.colorspace(&look.process_space)?;
            self.convert(current, space, out)?;
            current = space;
            let inner = self.nested()?;
            let result = match (inverse, &look.transform, &look.inverse_transform) {
                (false, Some(t), _) => inner.transform(t, Direction::Forward, out),
                (false, None, Some(t)) => inner.transform(t, Direction::Inverse, out),
                (true, _, Some(t)) => inner.transform(t, Direction::Forward, out),
                (true, Some(t), None) => inner.transform(t, Direction::Inverse, out),
                (_, None, None) => Ok(()),
            };
            result.map_err(|e| context(e, &format!("look {name}")))?;
        }
        Ok(current)
    }
}

fn context(e: Error, what: &str) -> Error {
    match e {
        Error::Unsupported(m) if !m.contains(" in ") => {
            Error::Unsupported(format!("{m} in “{what}”"))
        }
        other => other,
    }
}

impl Config {
    fn compiler(&self) -> Compiler<'_> {
        Compiler {
            config: self,
            depth: 0,
        }
    }

    /// Colour space `src` → colour space `dst`.
    pub fn processor(&self, src: &str, dst: &str) -> Result<Processor> {
        let c = self.compiler();
        let mut ops = Vec::new();
        c.convert(c.colorspace(src)?, c.colorspace(dst)?, &mut ops)?;
        Ok(Processor::new(ops))
    }

    /// One transform on its own (FileTransforms resolve against this
    /// config's search path).
    pub fn transform_processor(&self, t: &Transform) -> Result<Processor> {
        let mut ops = Vec::new();
        self.compiler().transform(t, Direction::Forward, &mut ops)?;
        Ok(Processor::new(ops))
    }

    /// The view `view` of `display` (names ignore case).
    pub fn find_view(&self, display: &str, view: &str) -> Result<&View> {
        let d = self
            .display(display)
            .ok_or_else(|| Error::NotFound(format!("display “{display}” is not in the config")))?;
        d.views
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(view))
            .ok_or_else(|| Error::NotFound(format!("display “{display}” has no view “{view}”")))
    }

    /// The colour space a display/view ends in.
    pub fn view_colorspace(&self, display: &str, view: &str) -> Result<String> {
        let v = self.find_view(display, view)?;
        let name = v
            .display_colorspace
            .as_deref()
            .or(v.colorspace.as_deref())
            .unwrap_or_default();
        Ok(if name == USE_DISPLAY_NAME {
            self.display(display)
                .map(|d| d.name.clone())
                .unwrap_or_default()
        } else {
            name.to_string()
        })
    }

    /// `src` as `display`/`view` shows it, after `looks` (or the view's own
    /// looks when `None`; an empty string turns them off).
    pub fn display_processor(
        &self,
        src: &str,
        display: &str,
        view: &str,
        looks: Option<&str>,
    ) -> Result<Processor> {
        let c = self.compiler();
        let src = c.colorspace(src)?;
        let v = self.find_view(display, view)?;
        if src.isdata {
            return Ok(Processor::default());
        }
        let mut ops = Vec::new();
        let current = c.looks(src, looks.unwrap_or(&v.looks), &mut ops)?;
        let target = c.colorspace(&self.view_colorspace(display, view)?)?;
        match &v.view_transform {
            None => c.convert(current, target, &mut ops)?,
            Some(name) => {
                let vt = self.view_transform(name).ok_or_else(|| {
                    Error::NotFound(format!("view transform “{name}” is not in the config"))
                })?;
                if target.isdata {
                    return Ok(Processor::default());
                }
                if vt.from_scene.is_some() || vt.to_scene.is_some() {
                    c.to_own_reference(current, &mut ops)?;
                    if current.reference == Reference::Display {
                        c.bridge(false, &mut ops)?;
                    }
                    c.scene_view_transform(vt, true, &mut ops)
                        .map_err(|e| context(e, &format!("view transform {name}")))?;
                } else {
                    c.to_own_reference(current, &mut ops)?;
                    if current.reference == Reference::Scene {
                        c.bridge(true, &mut ops)?;
                    }
                    match (&vt.from_display, &vt.to_display) {
                        (Some(t), _) => c.transform(t, Direction::Forward, &mut ops)?,
                        (None, Some(t)) => c.transform(t, Direction::Inverse, &mut ops)?,
                        (None, None) => {}
                    }
                }
                if target.reference != Reference::Display {
                    return Err(Error::Config(format!(
                        "view “{}” ends in “{}”, which is not a display colour space",
                        v.name, target.name
                    )));
                }
                c.own_reference_to(target, &mut ops)?;
            }
        }
        Ok(Processor::new(ops))
    }
}
