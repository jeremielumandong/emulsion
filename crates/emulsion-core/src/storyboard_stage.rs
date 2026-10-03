//! What the storyboard Stage draws around and under a panel: the camera
//! frame's guides (safe areas, field guide, overscan), the light table, and
//! the board's colour palette. Geometry lives here so the Stage, agents and
//! later exports agree.
use serde::{Deserialize, Serialize};

/// A rectangle in panel pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Frame {
    /// The rectangle `percent` of the size of `width` × `height`, centred.
    pub fn centred(width: u32, height: u32, percent: f64) -> Self {
        let (w, h) = (f64::from(width), f64::from(height));
        let scale = percent / 100.;
        let (fw, fh) = (w * scale, h * scale);
        Self {
            x: (w - fw) / 2.,
            y: (h - fh) / 2.,
            w: fw,
            h: fh,
        }
    }
}

/// Guides drawn over the camera frame. A percentage of 0 hides that guide.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StageGuides {
    /// Action safe area, as a percentage of the frame.
    pub action_safe: f64,
    /// Title safe area, as a percentage of the frame.
    pub title_safe: f64,
    pub field_guide: bool,
    /// Fields in the field guide; the frame is the outermost field.
    pub fields: u8,
    /// Space shown around the camera frame, as a percentage of its size, for
    /// art that runs outside the shot.
    pub overscan: f64,
}

impl Default for StageGuides {
    fn default() -> Self {
        Self {
            action_safe: 90.,
            title_safe: 80.,
            field_guide: false,
            fields: 12,
            overscan: 10.,
        }
    }
}

impl StageGuides {
    pub const MAX_OVERSCAN: f64 = 100.;

    pub fn validate(&self) -> Result<(), String> {
        for safe in [self.action_safe, self.title_safe] {
            if !safe.is_finite() || !(0. ..=100.).contains(&safe) {
                return Err("Safe areas are 0–100% of the frame (0 hides them).".into());
            }
        }
        if !(2..=24).contains(&self.fields) {
            return Err("A field guide has 2–24 fields.".into());
        }
        if !self.overscan.is_finite() || !(0. ..=Self::MAX_OVERSCAN).contains(&self.overscan) {
            return Err(format!(
                "Overscan is 0–{}% of the frame.",
                Self::MAX_OVERSCAN
            ));
        }
        Ok(())
    }

    /// Safe-area rectangles that are shown: action first, then title.
    pub fn safe_areas(&self, width: u32, height: u32) -> Vec<Frame> {
        [self.action_safe, self.title_safe]
            .into_iter()
            .filter(|p| *p > 0.)
            .map(|p| Frame::centred(width, height, p))
            .collect()
    }

    /// The field guide's rectangles, from field 1 (smallest) to the frame.
    /// Empty when the guide is off.
    pub fn field_rects(&self, width: u32, height: u32) -> Vec<Frame> {
        if !self.field_guide {
            return Vec::new();
        }
        let fields = f64::from(self.fields);
        (1..=self.fields)
            .map(|n| Frame::centred(width, height, f64::from(n) / fields * 100.))
            .collect()
    }

    /// The area the Stage shows: the frame plus overscan on every side.
    pub fn stage_area(&self, width: u32, height: u32) -> Frame {
        Frame::centred(width, height, 100. + 2. * self.overscan)
    }
}

/// Neighbouring panels shown faintly under the panel being drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightTable {
    pub enabled: bool,
    /// Panels before and after the active one to show.
    pub before: u8,
    pub after: u8,
    /// Opacity of the nearest neighbour; farther ones fade.
    pub opacity: f64,
    /// Tint earlier panels red and later ones blue.
    pub tint: bool,
}

impl Default for LightTable {
    fn default() -> Self {
        Self {
            enabled: false,
            before: 1,
            after: 0,
            opacity: 0.3,
            tint: true,
        }
    }
}

impl LightTable {
    pub const MAX_NEIGHBOURS: u8 = 3;
    pub const BEFORE_TINT: [u8; 3] = [214, 72, 72];
    pub const AFTER_TINT: [u8; 3] = [64, 120, 214];

    pub fn validate(&self) -> Result<(), String> {
        let max = Self::MAX_NEIGHBOURS;
        if self.before > max || self.after > max {
            return Err(format!(
                "The light table shows 0–{max} panels on each side."
            ));
        }
        if !self.opacity.is_finite() || !(0.05..=1.).contains(&self.opacity) {
            return Err("Light table opacity is 5–100%.".into());
        }
        Ok(())
    }

    /// The panels to show under `active`, farthest first so nearer ones draw
    /// on top, each with its opacity and tint.
    pub fn layers<T: Copy + PartialEq>(
        &self,
        layout: &[T],
        active: T,
    ) -> Vec<(T, f64, Option<[u8; 3]>)> {
        let Some(at) = layout.iter().position(|id| *id == active) else {
            return Vec::new();
        };
        if !self.enabled {
            return Vec::new();
        }
        let mut out = Vec::new();
        let side = |count: u8, step: isize, tint: [u8; 3], out: &mut Vec<_>| {
            for distance in (1..=isize::from(count)).rev() {
                let Some(index) = at.checked_add_signed(distance * step) else {
                    continue;
                };
                if let Some(id) = layout.get(index) {
                    let opacity = self.opacity / distance as f64;
                    out.push((*id, opacity, self.tint.then_some(tint)));
                }
            }
        };
        side(self.before, -1, Self::BEFORE_TINT, &mut out);
        side(self.after, 1, Self::AFTER_TINT, &mut out);
        out
    }
}

/// The colours a new storyboard's palette starts with: greys for roughs,
/// an accent, and red and blue for notes and corrections.
pub const DEFAULT_PALETTE: [[u8; 3]; 10] = [
    [0, 0, 0],
    [51, 51, 51],
    [102, 102, 102],
    [153, 153, 153],
    [204, 204, 204],
    [255, 255, 255],
    [232, 160, 64],
    [214, 48, 48],
    [40, 96, 214],
    [48, 160, 96],
];

pub const MAX_PALETTE: usize = 64;

pub fn validate_palette(palette: &[[u8; 3]]) -> Result<(), String> {
    if palette.len() > MAX_PALETTE {
        return Err(format!("A palette holds at most {MAX_PALETTE} colours."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guides_are_centred_and_validated() {
        let guides = StageGuides {
            field_guide: true,
            fields: 4,
            ..StageGuides::default()
        };
        guides.validate().unwrap();
        let safe = guides.safe_areas(1000, 500);
        assert_eq!(safe.len(), 2);
        assert_eq!((safe[0].x, safe[0].w), (50., 900.));
        let fields = guides.field_rects(1000, 500);
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[3], Frame::centred(1000, 500, 100.));
        assert_eq!(fields[0].w, 250.);
        let area = guides.stage_area(1000, 500);
        assert_eq!((area.x, area.w), (-100., 1200.));
        let none = StageGuides {
            action_safe: 0.,
            title_safe: 0.,
            ..StageGuides::default()
        };
        assert!(none.safe_areas(10, 10).is_empty());
        assert!(none.field_rects(10, 10).is_empty());
        assert!(
            StageGuides {
                fields: 1,
                ..StageGuides::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            StageGuides {
                overscan: -1.,
                ..StageGuides::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn light_table_fades_with_distance_and_stops_at_the_ends() {
        let table = LightTable {
            enabled: true,
            before: 2,
            after: 1,
            opacity: 0.4,
            tint: true,
        };
        let layers = table.layers(&[1, 2, 3, 4], 2);
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0], (1, 0.4, Some(LightTable::BEFORE_TINT)));
        assert_eq!(layers[1], (3, 0.4, Some(LightTable::AFTER_TINT)));
        let layers = table.layers(&[1, 2, 3, 4], 4);
        assert_eq!(
            layers.iter().map(|l| l.0).collect::<Vec<_>>(),
            [2, 3],
            "farthest first"
        );
        assert_eq!(layers[0].1, 0.2);
        assert!(LightTable::default().layers(&[1, 2], 2).is_empty());
        assert!(LightTable { before: 9, ..table }.validate().is_err());
    }

    #[test]
    fn palettes_are_bounded() {
        validate_palette(&DEFAULT_PALETTE).unwrap();
        assert!(validate_palette(&[[0; 3]; MAX_PALETTE + 1]).is_err());
    }
}
