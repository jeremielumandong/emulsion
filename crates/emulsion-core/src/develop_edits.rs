//! Versioned, bounded, nondestructive edits in oriented source coordinates.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEdits {
    pub version: u32,
    pub masks: Vec<Mask>,
    pub spots: Vec<Spot>,
}
impl Default for LocalEdits {
    fn default() -> Self {
        Self {
            version: 1,
            masks: vec![],
            spots: vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mask {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub components: Vec<Component>,
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub temperature: f32,
    pub tint: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub operation: Operation,
    pub shape: Shape,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Add,
    Subtract,
    Intersect,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    Brush {
        points: Vec<[f32; 2]>,
        radius: f32,
        feather: f32,
    },
    Radial {
        center: [f32; 2],
        radius: [f32; 2],
        feather: f32,
    },
    Linear {
        start: [f32; 2],
        end: [f32; 2],
    },
    Luminance {
        range: [f32; 2],
        feather: f32,
    },
    Color {
        rgb: [f32; 3],
        tolerance: f32,
        feather: f32,
    },
    Bitmap {
        digest: [u8; 32],
        inverted: bool,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpotMode {
    Heal,
    Clone,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spot {
    pub id: u32,
    pub source: [f32; 2],
    pub target: [f32; 2],
    /// Optional freehand region in oriented source coordinates; empty means a spot.
    #[serde(default)]
    pub stroke: Vec<[f32; 2]>,
    pub radius: f32,
    pub feather: f32,
    pub opacity: f32,
    pub mode: SpotMode,
}
fn unit(v: f32) -> bool {
    v.is_finite() && (0.0..=1.0).contains(&v)
}
fn point(p: &[f32; 2]) -> bool {
    p.iter().all(|v| unit(*v))
}
impl LocalEdits {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != 1 || self.masks.len() > 64 || self.spots.len() > 256 {
            return Err("Unsupported or oversized local edit set");
        }
        let mut ids = std::collections::HashSet::new();
        for m in &self.masks {
            if m.id == 0
                || !ids.insert(m.id)
                || m.name.trim().is_empty()
                || m.name.len() > 200
                || m.components.is_empty()
                || m.components.len() > 64
            {
                return Err("Invalid mask identity or component count");
            }
            if !m.exposure.is_finite()
                || m.exposure.abs() > 5.
                || [m.contrast, m.saturation, m.temperature, m.tint]
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 1.)
            {
                return Err("Invalid local adjustment");
            }
            for c in &m.components {
                let valid = match &c.shape {
                    Shape::Brush {
                        points,
                        radius,
                        feather,
                    } => {
                        !points.is_empty()
                            && points.len() <= 4096
                            && points.iter().all(point)
                            && unit(*radius)
                            && *radius > 0.
                            && unit(*feather)
                    }
                    Shape::Radial {
                        center,
                        radius,
                        feather,
                    } => {
                        point(center)
                            && radius.iter().all(|v| unit(*v) && *v > 0.)
                            && unit(*feather)
                    }
                    Shape::Linear { start, end } => point(start) && point(end) && start != end,
                    Shape::Luminance { range, feather } => {
                        point(range) && range[0] < range[1] && unit(*feather)
                    }
                    Shape::Color {
                        rgb,
                        tolerance,
                        feather,
                    } => {
                        rgb.iter().all(|v| unit(*v))
                            && unit(*tolerance)
                            && *tolerance > 0.
                            && unit(*feather)
                    }
                    Shape::Bitmap { .. } => true,
                };
                if !valid {
                    return Err("Invalid mask component");
                }
            }
        }
        ids.clear();
        for s in &self.spots {
            if s.id == 0
                || !ids.insert(s.id)
                || !point(&s.source)
                || !point(&s.target)
                || s.stroke.len() > 512
                || !s.stroke.iter().all(point)
                || !unit(s.radius)
                || s.radius == 0.
                || !unit(s.feather)
                || !unit(s.opacity)
            {
                return Err("Invalid healing spot");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_nonfinite_and_unbounded_assets() {
        let mut e = LocalEdits::default();
        e.spots.push(Spot {
            id: 1,
            source: [0.2, 0.3],
            target: [0.4, 0.5],
            stroke: vec![],
            radius: f32::NAN,
            feather: 0.5,
            opacity: 1.,
            mode: SpotMode::Clone,
        });
        assert!(e.validate().is_err());
        e.spots[0].radius = 0.1;
        assert!(e.validate().is_ok());
        e.spots.push(e.spots[0].clone());
        assert!(e.validate().is_err());
    }
    #[test]
    fn legacy_process_is_explicit_and_new_controls_require_upgrade() {
        let p: crate::raw::DevelopParams = serde_json::from_str("{}").unwrap();
        assert_eq!(p.process_version, 1);
        let mut p = p;
        p.parametric[1] = 0.5;
        assert!(p.validate().is_err());
        p.process_version = 2;
        assert!(p.validate().is_ok());
    }
}
