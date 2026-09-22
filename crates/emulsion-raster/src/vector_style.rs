use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternKind {
    #[default]
    Checker,
    Stripes,
    Dots,
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
    fn sanitized(self) -> Self {
        match self {
            Self::LinearGradient { end, angle } => Self::LinearGradient {
                end,
                angle: finite(angle, 0.0, -360000.0, 360000.0).rem_euclid(360.0),
            },
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
