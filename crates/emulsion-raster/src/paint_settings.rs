//! Versionable, neutral-by-default brush controls shared by painting and Studio.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseCurve(pub [f32; 5]);

impl Default for ResponseCurve {
    fn default() -> Self {
        Self([0.0, 0.25, 0.5, 0.75, 1.0])
    }
}

impl ResponseCurve {
    pub fn sample(self, input: f32) -> f32 {
        let x = unit(input) * 4.0;
        let index = (x as usize).min(3);
        self.0[index] + (self.0[index + 1] - self.0[index]) * (x - index as f32)
    }
    fn sanitized(self) -> Self {
        Self(self.0.map(unit))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StrokePathSettings {
    /// Random offset perpendicular / parallel to the path, in tip diameters.
    pub lateral_jitter: f32,
    pub linear_jitter: f32,
    pub spacing_jitter: f32,
    /// Fade over this many layer pixels; zero disables distance falloff.
    pub falloff: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StabilizationSettings {
    pub stages: u8,
    pub amount: f32,
    pub pressure: f32,
}
impl Default for StabilizationSettings {
    fn default() -> Self {
        Self {
            stages: 1,
            amount: 0.0,
            pressure: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShapeSettings {
    pub count: u8,
    /// Randomly omit up to this fraction of additional stamps.
    pub count_jitter: f32,
    /// Random rotation range as a fraction of a full turn.
    pub rotation_jitter: f32,
    pub flip_x: bool,
    pub flip_y: bool,
}
impl Default for ShapeSettings {
    fn default() -> Self {
        Self {
            count: 1,
            count_jitter: 0.0,
            rotation_jitter: 0.0,
            flip_x: false,
            flip_y: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrainMode {
    #[default]
    Canvas,
    Moving,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrainSettings {
    pub mode: GrainMode,
    /// Multiplier on the legacy grain scale.
    pub scale: f32,
    pub rotation: f32,
    pub brightness: f32,
    pub contrast: f32,
    /// Random phase for each moving-grain dab, in texture periods.
    pub offset_jitter: f32,
}
impl Default for GrainSettings {
    fn default() -> Self {
        Self {
            mode: GrainMode::Canvas,
            scale: 1.0,
            rotation: 0.0,
            brightness: 0.0,
            contrast: 1.0,
            offset_jitter: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DynamicsSettings {
    pub pressure: ResponseCurve,
    pub pressure_opacity: f32,
    /// Independent velocity controls, active even with real pen pressure.
    pub speed_size: f32,
    pub speed_opacity: f32,
    pub tilt_opacity: f32,
    pub opacity_jitter: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaperSettings {
    /// Fade opacity with the existing start/end taper distances.
    pub opacity: f32,
    /// Shape the taper ramp; 1 is the legacy linear profile.
    pub tip_curve: f32,
}
impl Default for TaperSettings {
    fn default() -> Self {
        Self {
            opacity: 0.0,
            tip_curve: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorDynamicsSettings {
    pub stamp_hue: f32,
    pub stamp_saturation: f32,
    pub stamp_lightness: f32,
    pub stroke_hue: f32,
    pub stroke_saturation: f32,
    pub stroke_lightness: f32,
    /// Signed shifts driven by pressure relative to its midpoint.
    pub pressure_hue: f32,
    pub pressure_saturation: f32,
    pub pressure_lightness: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushProperties {
    /// Absolute limits on resolved dab diameter, in layer pixels.
    pub min_size: f32,
    pub max_size: f32,
    pub min_opacity: f32,
    pub max_opacity: f32,
}
impl Default for BrushProperties {
    fn default() -> Self {
        Self {
            min_size: 0.0,
            max_size: 4000.0,
            min_opacity: 0.0,
            max_opacity: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WetMixSettings {
    pub dilution: f32,
    /// Pigment reservoir length in layer pixels; zero means unlimited.
    pub charge: f32,
    /// Additional persistence of the sampled pigment carried by a stroke.
    pub pull: f32,
}
impl Default for WetMixSettings {
    fn default() -> Self {
        Self {
            dilution: 0.0,
            charge: 0.0,
            pull: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderingMode {
    /// Legacy whole-stroke opacity cap.
    #[default]
    Glaze,
    /// Apply opacity to every dab, allowing overlaps to build up.
    Accumulating,
}

/// Combine independently sampled brush components before painting the layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DualBlend {
    #[default]
    Normal,
    /// Intersect the two coverages, retaining the primary pigment.
    Multiply,
    /// Screen the pigments and union their coverage.
    Screen,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedBrush {
    pub path: StrokePathSettings,
    pub stabilization: StabilizationSettings,
    pub shape: ShapeSettings,
    pub grain: GrainSettings,
    pub dynamics: DynamicsSettings,
    pub taper: TaperSettings,
    pub color: ColorDynamicsSettings,
    pub properties: BrushProperties,
    pub wet: WetMixSettings,
    pub rendering: RenderingMode,
}

fn finite(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}
fn unit(value: f32) -> f32 {
    finite(value, 0.0, 1.0, 0.0)
}

impl AdvancedBrush {
    pub fn sanitized(mut self) -> Self {
        self.path.lateral_jitter = unit(self.path.lateral_jitter);
        self.path.linear_jitter = unit(self.path.linear_jitter);
        self.path.spacing_jitter = unit(self.path.spacing_jitter);
        self.path.falloff = finite(self.path.falloff, 0.0, 100_000.0, 0.0);
        self.stabilization.stages = self.stabilization.stages.clamp(1, 16);
        self.stabilization.amount = unit(self.stabilization.amount);
        self.stabilization.pressure = unit(self.stabilization.pressure);
        self.shape.count = self.shape.count.clamp(1, 16);
        self.shape.count_jitter = unit(self.shape.count_jitter);
        self.shape.rotation_jitter = unit(self.shape.rotation_jitter);
        self.grain.scale = finite(self.grain.scale, 0.01, 100.0, 1.0);
        self.grain.rotation = finite(self.grain.rotation, -360.0, 360.0, 0.0);
        self.grain.brightness = finite(self.grain.brightness, -1.0, 1.0, 0.0);
        self.grain.contrast = finite(self.grain.contrast, 0.0, 4.0, 1.0);
        self.grain.offset_jitter = unit(self.grain.offset_jitter);
        self.dynamics.pressure = self.dynamics.pressure.sanitized();
        self.dynamics.pressure_opacity = unit(self.dynamics.pressure_opacity);
        self.dynamics.speed_size = unit(self.dynamics.speed_size);
        self.dynamics.speed_opacity = unit(self.dynamics.speed_opacity);
        self.dynamics.tilt_opacity = unit(self.dynamics.tilt_opacity);
        self.dynamics.opacity_jitter = unit(self.dynamics.opacity_jitter);
        self.taper.opacity = unit(self.taper.opacity);
        self.taper.tip_curve = finite(self.taper.tip_curve, 0.1, 8.0, 1.0);
        self.color.stamp_hue = unit(self.color.stamp_hue);
        self.color.stamp_saturation = unit(self.color.stamp_saturation);
        self.color.stamp_lightness = unit(self.color.stamp_lightness);
        self.color.stroke_hue = unit(self.color.stroke_hue);
        self.color.stroke_saturation = unit(self.color.stroke_saturation);
        self.color.stroke_lightness = unit(self.color.stroke_lightness);
        self.color.pressure_hue = finite(self.color.pressure_hue, -1.0, 1.0, 0.0);
        self.color.pressure_saturation = finite(self.color.pressure_saturation, -1.0, 1.0, 0.0);
        self.color.pressure_lightness = finite(self.color.pressure_lightness, -1.0, 1.0, 0.0);
        self.properties.max_size = finite(self.properties.max_size, 1.0, 4000.0, 4000.0);
        self.properties.min_size =
            finite(self.properties.min_size, 0.0, self.properties.max_size, 0.0);
        self.properties.max_opacity = unit(self.properties.max_opacity);
        self.properties.min_opacity =
            unit(self.properties.min_opacity).min(self.properties.max_opacity);
        self.wet.dilution = unit(self.wet.dilution);
        self.wet.charge = finite(self.wet.charge, 0.0, 100_000.0, 0.0);
        self.wet.pull = unit(self.wet.pull);
        self
    }
}
