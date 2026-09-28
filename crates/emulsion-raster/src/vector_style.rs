use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternKind {
    #[default]
    Checker,
    Stripes,
    Dots,
}
pub const MAX_GRADIENT_STOPS: usize = 16;
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientStop {
    pub offset: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum PathPaint {
    #[default]
    Solid,
    LinearGradient {
        end: [u8; 4],
        angle: f32,
    },
    RadialGradient {
        end: [u8; 4],
    },
    LinearStops {
        stops: [GradientStop; MAX_GRADIENT_STOPS],
        count: u8,
        angle: f32,
    },
    RadialStops {
        stops: [GradientStop; MAX_GRADIENT_STOPS],
        count: u8,
    },
    Pattern {
        kind: PatternKind,
        secondary: [u8; 4],
        size: f32,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeAlignment {
    #[default]
    Center,
    Inside,
    Outside,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeCap {
    Butt,
    #[default]
    Round,
    Square,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeJoin {
    Miter,
    #[default]
    Round,
    Bevel,
}

/// Editable native shape paint. Colors are straight sRGB; rendering is linear premultiplied.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PathStyle {
    /// SVG even-odd winding for compound paths.
    pub even_odd: bool,
    pub stroke: Option<[u8; 4]>,
    pub width: f32,
    pub fill: Option<[u8; 4]>,
    pub fill_paint: PathPaint,
    pub stroke_paint: PathPaint,
    pub alignment: StrokeAlignment,
    pub cap: StrokeCap,
    pub join: StrokeJoin,
    pub miter_limit: f32,
    /// Alternating dash/gap lengths in document pixels. Odd lists repeat twice.
    /// Zero-length dashes produce dots with round caps; positive lengths have
    /// a quarter-pixel minimum to bound work for subpixel dash sequences.
    pub dash: [f32; 6],
    pub dash_count: u8,
    pub dash_offset: f32,
}
impl Default for PathStyle {
    fn default() -> Self {
        Self {
            even_odd: false,
            stroke: Some([10, 10, 11, 255]),
            width: 3.0,
            fill: None,
            fill_paint: PathPaint::Solid,
            stroke_paint: PathPaint::Solid,
            alignment: StrokeAlignment::Center,
            cap: StrokeCap::Round,
            join: StrokeJoin::Round,
            miter_limit: 4.0,
            dash: [0.0; 6],
            dash_count: 0,
            dash_offset: 0.0,
        }
    }
}
fn finite(v: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if v.is_finite() {
        v.clamp(min, max)
    } else {
        fallback
    }
}
impl PathPaint {
    pub fn from_stops(stops: &[GradientStop], radial: bool, angle: f32) -> Result<Self, String> {
        if !(2..=MAX_GRADIENT_STOPS).contains(&stops.len())
            || !angle.is_finite()
            || angle.abs() > 360000.
            || stops
                .iter()
                .any(|s| !s.offset.is_finite() || !(0. ..=1.).contains(&s.offset))
            || stops.windows(2).any(|p| p[0].offset > p[1].offset)
        {
            return Err("Use 2–16 ordered gradient stops at 0–100%, with a finite angle.".into());
        }
        let mut data = [GradientStop::default(); MAX_GRADIENT_STOPS];
        data[..stops.len()].copy_from_slice(stops);
        Ok(if radial {
            Self::RadialStops {
                stops: data,
                count: stops.len() as u8,
            }
        } else {
            Self::LinearStops {
                stops: data,
                count: stops.len() as u8,
                angle: angle.rem_euclid(360.),
            }
        })
    }
    pub fn gradient_stops(self, primary: [u8; 4]) -> Option<Vec<GradientStop>> {
        match self {
            Self::LinearGradient { end, .. } | Self::RadialGradient { end } => Some(vec![
                GradientStop {
                    offset: 0.,
                    color: primary,
                },
                GradientStop {
                    offset: 1.,
                    color: end,
                },
            ]),
            Self::LinearStops { stops, count, .. } | Self::RadialStops { stops, count } => {
                Some(stops[..usize::from(count).min(MAX_GRADIENT_STOPS)].to_vec())
            }
            _ => None,
        }
    }
    pub fn gradient_angle(self) -> f32 {
        match self {
            Self::LinearGradient { angle, .. } | Self::LinearStops { angle, .. } => angle,
            _ => 0.,
        }
    }
    pub fn is_radial(self) -> bool {
        matches!(self, Self::RadialGradient { .. } | Self::RadialStops { .. })
    }
    fn sanitized(self) -> Self {
        match self {
            Self::LinearGradient { end, angle } => Self::LinearGradient {
                end,
                angle: finite(angle, 0.0, -360000.0, 360000.0).rem_euclid(360.0),
            },
            Self::LinearStops {
                stops,
                count,
                angle,
            } => Self::from_stops(
                &stops[..usize::from(count).min(MAX_GRADIENT_STOPS)],
                false,
                angle,
            )
            .unwrap_or(Self::Solid),
            Self::RadialStops { stops, count } => Self::from_stops(
                &stops[..usize::from(count).min(MAX_GRADIENT_STOPS)],
                true,
                0.,
            )
            .unwrap_or(Self::Solid),
            Self::Pattern {
                kind,
                secondary,
                size,
            } => Self::Pattern {
                kind,
                secondary,
                size: finite(size, 16.0, 1.0, 4096.0),
            },
            p => p,
        }
    }
}
impl PathStyle {
    pub fn sanitized(mut self) -> Self {
        self.width = finite(self.width, 3.0, 0.0, 500.0);
        self.miter_limit = finite(self.miter_limit, 4.0, 1.0, 100.0);
        self.dash_count = self.dash_count.min(6);
        self.dash_offset = finite(self.dash_offset, 0.0, -100000.0, 100000.0);
        for d in &mut self.dash {
            *d = finite(*d, 1.0, 0.0, 10000.0);
            if *d > 0.0 {
                *d = d.max(0.25);
            }
        }
        if self.dash[..self.dash_count as usize]
            .iter()
            .all(|d| *d == 0.0)
        {
            self.dash_count = 0;
        }
        self.fill_paint = self.fill_paint.sanitized();
        self.stroke_paint = self.stroke_paint.sanitized();
        self
    }
    /// Conservative outward extent used for allocation and document geometry bounds.
    /// Call on a sanitized style when values originate outside the application.
    pub fn stroke_padding(&self) -> f32 {
        let half = if self.alignment == StrokeAlignment::Center {
            self.width / 2.0
        } else {
            self.width
        };
        let join = if self.join == StrokeJoin::Miter {
            self.miter_limit
        } else {
            1.0
        };
        let cap = if self.cap == StrokeCap::Square {
            std::f32::consts::SQRT_2
        } else {
            1.0
        };
        half * join.max(cap)
    }
}
