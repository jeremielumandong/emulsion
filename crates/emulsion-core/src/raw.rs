//! Versioned, nondestructive development settings owned by the document.
use crate::NodeId;
use std::path::PathBuf;

/// Measured Lensfun coefficients, embedded so saved edits do not depend on a database revision.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LensCorrection {
    pub distortion: [f32; 3],
    pub vignette: [f32; 3],
    pub tca: [f32; 6],
    pub scale: f32,
}

/// Local coordinates are normalized to the oriented source, before crop/geometry.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalAdjustment {
    pub enabled: bool,
    pub bitmap: Option<[u8; 32]>,
    pub linear: bool,
    pub inverted: bool,
    pub center: [f32; 2],
    pub radius: [f32; 2],
    pub feather: f32,
    pub exposure: f32,
    pub saturation: f32,
    pub temperature: f32,
}
impl Default for LocalAdjustment {
    fn default() -> Self {
        Self {
            enabled: false,
            bitmap: None,
            linear: false,
            inverted: false,
            center: [0.5; 2],
            radius: [0.25; 2],
            feather: 0.5,
            exposure: 0.,
            saturation: 0.,
            temperature: 0.,
        }
    }
}
impl LocalAdjustment {
    pub fn validate(&self) -> Result<(), &'static str> {
        for (v, min, max) in [
            (self.center[0], 0., 1.),
            (self.center[1], 0., 1.),
            (self.radius[0], 0.001, 2.),
            (self.radius[1], 0.001, 2.),
            (self.feather, 0.001, 1.),
            (self.exposure, -5., 5.),
            (self.saturation, -1., 1.),
            (self.temperature, -1., 1.),
        ] {
            if !v.is_finite() || !(min..=max).contains(&v) {
                return Err("Invalid local adjustment");
            }
        }
        Ok(())
    }
    pub fn weight(&self, x: f32, y: f32) -> f32 {
        if !self.enabled {
            return 0.;
        }
        let distance = if self.linear {
            (y - self.center[1]) / self.radius[1] + 0.5
        } else {
            (((x - self.center[0]) / self.radius[0]).powi(2)
                + ((y - self.center[1]) / self.radius[1]).powi(2))
            .sqrt()
        };
        let t = ((1. - distance) / self.feather).clamp(0., 1.);
        let weight = t * t * (3. - 2. * t);
        if self.inverted { 1. - weight } else { weight }
    }
}

/// Up to 32 arbitrary control points. Empty means identity; x must increase,
/// while y may decrease to preserve creative/inverted imported curves.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "Vec<[f32; 2]>", into = "Vec<[f32; 2]>")]
pub struct PointCurve {
    pub points: [[f32; 2]; 32],
    pub len: u8,
}
impl Default for PointCurve {
    fn default() -> Self {
        Self {
            points: [[0.; 2]; 32],
            len: 0,
        }
    }
}
impl From<PointCurve> for Vec<[f32; 2]> {
    fn from(v: PointCurve) -> Self {
        v.points[..v.len as usize].to_vec()
    }
}
impl TryFrom<Vec<[f32; 2]>> for PointCurve {
    type Error = &'static str;
    fn try_from(points: Vec<[f32; 2]>) -> Result<Self, Self::Error> {
        if points.len() > 32
            || points.len() == 1
            || points
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || points.windows(2).any(|p| p[0][0] >= p[1][0])
        {
            return Err(
                "Curves need 2–32 normalized points in increasing x order, or [] for identity",
            );
        }
        let mut curve = Self::default();
        curve.len = points.len() as u8;
        curve.points[..points.len()].copy_from_slice(&points);
        Ok(curve)
    }
}
impl PointCurve {
    pub fn output(&self, x: f32) -> f32 {
        let p = &self.points[..self.len as usize];
        if p.is_empty() {
            return x;
        }
        if x <= p[0][0] {
            return p[0][1];
        }
        for pair in p.windows(2) {
            if x <= pair[1][0] {
                let t = (x - pair[0][0]) / (pair[1][0] - pair[0][0]);
                return pair[0][1] * (1. - t) + pair[1][1] * t;
            }
        }
        p.last().unwrap()[1]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DevelopParams {
    /// Content-addressed, scalable composed masks and healing spots.
    pub local_edits: Option<[u8; 32]>,
    pub camera_profile: Option<[u8; 32]>,
    /// Renderer contract. Recipes without this field retain process 1.
    #[serde(default = "legacy_process")]
    pub process_version: u32,
    /// Opt-in linear ProPhoto working primaries for camera RAW.
    pub wide_gamut: bool,
    /// Shadows, darks, lights, highlights in gamma curve coordinates.
    pub parametric: [f32; 4],
    pub parametric_splits: [f32; 3],
    /// Primary hue rotation and saturation, independent of the HSL mixer.
    pub calibration: [[f32; 2]; 3],
    pub shadow_tint: f32,
    pub global_grading: [f32; 3],
    pub grading_balance: f32,
    pub grading_blending: f32,
    pub sharpening_radius: f32,
    pub sharpening_detail: f32,
    pub sharpening_masking: f32,
    pub luminance_detail: f32,
    pub luminance_contrast: f32,
    pub color_noise_reduction: f32,
    pub color_noise_detail: f32,
    pub color_noise_smoothness: f32,
    /// Normalized left, top, right, bottom in the oriented source.
    pub crop: [f32; 4],
    /// Clockwise quarter turns after crop/geometry. Pixel-exact and non-destructive.
    pub rotation: u8,
    /// Composite, red, green and blue point curves.
    pub point_curves: [PointCurve; 4],
    /// Per-channel smooth interpolation. Missing in saved settings preserves linear rendering.
    #[serde(default)]
    pub smooth_point_curves: [bool; 4],
    /// CFA-aware denoise before demosaicing; zero preserves the source.
    pub sensor_noise_reduction: f32,
    pub straighten: f32,
    /// Horizontal and vertical keystone correction.
    pub perspective: [f32; 2],
    pub distortion: f32,
    pub lens_profile: Option<LensCorrection>,
    /// Red and blue radial scale corrections.
    pub aberration: [f32; 2],
    /// Hue shift, saturation and luminance for red/orange/yellow/green/aqua/blue/purple/magenta.
    pub hsl: [[f32; 3]; 8],
    /// Hue (degrees), saturation and luminance for shadows/midtones/highlights.
    pub grading: [[f32; 3]; 3],
    /// Absolute illuminant temperature. None retains camera as-shot balance.
    pub kelvin: Option<f32>,
    /// Up to eight nondestructive local radial/linear adjustments.
    pub masks: [LocalAdjustment; 8],
    pub exposure: f32,
    pub temperature: f32,
    pub tint: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub black_point: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub vibrance: f32,
    pub texture: f32,
    pub clarity: f32,
    pub dehaze: f32,
    pub vignette: f32,
    pub sharpening: f32,
    pub noise_reduction: f32,
    /// Output ordinates at gamma-2.2 input positions 0, .25, .5, .75, 1.
    pub tone_curve: [f32; 5],
    /// Smooth monotone cubic interpolation. Missing in legacy recipes means
    /// piecewise linear, preserving their existing rendering.
    #[serde(default)]
    pub smooth_curve: bool,
    /// Camera-channel gains, normalized to green; None uses the as-shot gains.
    pub wb_override: Option<[f32; 4]>,
}

fn legacy_process() -> u32 {
    1
}

impl Default for DevelopParams {
    fn default() -> Self {
        Self {
            local_edits: None,
            camera_profile: None,
            process_version: 2,
            wide_gamut: false,
            parametric: [0.; 4],
            parametric_splits: [0.25, 0.5, 0.75],
            calibration: [[0.; 2]; 3],
            shadow_tint: 0.,
            global_grading: [0.; 3],
            grading_balance: 0.,
            grading_blending: 0.5,
            sharpening_radius: 0.8,
            sharpening_detail: 0.5,
            sharpening_masking: 0.,
            luminance_detail: 0.5,
            luminance_contrast: 0.,
            color_noise_reduction: 0.,
            color_noise_detail: 0.5,
            color_noise_smoothness: 0.5,
            crop: [0., 0., 1., 1.],
            rotation: 0,
            point_curves: [PointCurve::default(); 4],
            smooth_point_curves: [true; 4],
            sensor_noise_reduction: 0.,
            straighten: 0.,
            perspective: [0.; 2],
            distortion: 0.,
            lens_profile: None,
            aberration: [0.; 2],
            hsl: [[0.; 3]; 8],
            grading: [[0.; 3]; 3],
            kelvin: None,
            masks: [LocalAdjustment::default(); 8],
            exposure: 0.0,
            temperature: 0.0,
            tint: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            black_point: 0.0,
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            vibrance: 0.0,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            vignette: 0.0,
            sharpening: 0.0,
            noise_reduction: 0.0,
            tone_curve: Self::LINEAR_CURVE,
            smooth_curve: true,
            wb_override: None,
        }
    }
}

impl DevelopParams {
    /// Same monotone cubic evaluator as Photo Curves, with normalized coordinates.
    pub fn point_curve_output(&self, channel: usize, input: f32) -> f32 {
        let curve = &self.point_curves[channel];
        if self.smooth_point_curves[channel] {
            emulsion_raster::adjust::curve_at(&curve.points[..curve.len as usize], input)
                .clamp(0., 1.)
        } else {
            curve.output(input)
        }
    }

    /// Evaluate in gamma-2.2 curve coordinates, shared by graph and developer.
    pub fn curve_output(&self, input: f32) -> f32 {
        let input = input.clamp(0.0, 1.0);
        if self.smooth_curve {
            let points =
                std::array::from_fn::<_, 5, _>(|i| [i as f32 * 63.75, self.tone_curve[i] * 255.0]);
            emulsion_raster::adjust::curve_at(&points, input * 255.0) / 255.0
        } else {
            let position = input * 4.0;
            let segment = (position as usize).min(3);
            let t = position - segment as f32;
            self.tone_curve[segment] * (1.0 - t) + self.tone_curve[segment + 1] * t
        }
    }

    pub const LINEAR_CURVE: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
    pub const MEDIUM_CONTRAST_CURVE: [f32; 5] = [0.0, 0.20, 0.5, 0.80, 1.0];
    pub const STRONG_CONTRAST_CURVE: [f32; 5] = [0.0, 0.15, 0.5, 0.85, 1.0];

    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=2).contains(&self.process_version) {
            return Err("Unsupported rendering process version");
        }
        for value in self
            .parametric
            .iter()
            .chain(self.calibration.iter().flatten())
            .chain([&self.shadow_tint, &self.grading_balance])
        {
            if !value.is_finite() || !(-1.0..=1.0).contains(value) {
                return Err("Invalid tonal/color control");
            }
        }
        if self
            .parametric_splits
            .iter()
            .any(|v| !v.is_finite() || !(0.01..=0.99).contains(v))
            || self
                .parametric_splits
                .windows(2)
                .any(|v| v[1] - v[0] < 0.01)
        {
            return Err("Curve splits must be ordered with at least 0.01 spacing");
        }
        for value in [
            self.grading_blending,
            self.sharpening_detail,
            self.sharpening_masking,
            self.luminance_detail,
            self.luminance_contrast,
            self.color_noise_reduction,
            self.color_noise_detail,
            self.color_noise_smoothness,
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err("Detail/blending control must be 0–1");
            }
        }
        if !self.sharpening_radius.is_finite() || !(0.5..=3.0).contains(&self.sharpening_radius) {
            return Err("Sharpening radius must be 0.5–3 pixels");
        }
        if !self.global_grading[0].is_finite()
            || !(0.0..=360.0).contains(&self.global_grading[0])
            || !self.global_grading[1].is_finite()
            || !(0.0..=1.0).contains(&self.global_grading[1])
            || !self.global_grading[2].is_finite()
            || !(-1.0..=1.0).contains(&self.global_grading[2])
        {
            return Err("Invalid global grading");
        }
        if self.process_version == 1
            && (self.wide_gamut
                || self.parametric != [0.; 4]
                || self.calibration != [[0.; 2]; 3]
                || self.shadow_tint != 0.
                || self.global_grading != [0.; 3]
                || self.grading_balance != 0.
                || self.grading_blending != 0.5
                || self.color_noise_reduction != 0.
                || self.sharpening_radius != 0.8
                || self.sharpening_detail != 0.5
                || self.sharpening_masking != 0.
                || self.luminance_detail != 0.5
                || self.luminance_contrast != 0.
                || self.color_noise_detail != 0.5
                || self.color_noise_smoothness != 0.5)
        {
            return Err("Upgrade rendering process before applying new color controls");
        }
        if !self.sensor_noise_reduction.is_finite()
            || !(0. ..=1.).contains(&self.sensor_noise_reduction)
        {
            return Err("Sensor noise reduction must be 0–1");
        }
        for curve in self.point_curves {
            if curve.len > 32 {
                return Err("Too many curve points");
            }
            PointCurve::try_from(Vec::from(curve))?;
        }
        if self.rotation > 3 {
            return Err("Rotation must be 0, 1, 2, or 3 clockwise quarter turns");
        }
        if self
            .crop
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || self.crop[2] - self.crop[0] < 0.001
            || self.crop[3] - self.crop[1] < 0.001
        {
            return Err("Crop must be a nonempty normalized rectangle");
        }
        if let Some(lens) = self.lens_profile
            && (lens
                .distortion
                .iter()
                .chain(lens.vignette.iter())
                .chain(lens.tca.iter())
                .any(|v| !v.is_finite() || v.abs() > 100.)
                || !lens.scale.is_finite()
                || !(0.01..=100.).contains(&lens.scale))
        {
            return Err("Invalid measured lens profile");
        }
        if self
            .kelvin
            .is_some_and(|k| !k.is_finite() || !(2000.0..=50000.0).contains(&k))
        {
            return Err("White balance temperature must be 2000–50000 K");
        }
        for values in self.hsl {
            for v in values {
                if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                    return Err("HSL values must be between -1 and 1");
                }
            }
        }
        for [h, s, l] in self.grading {
            if !h.is_finite()
                || !(0.0..=360.0).contains(&h)
                || !s.is_finite()
                || !(0.0..=1.0).contains(&s)
                || !l.is_finite()
                || !(-1.0..=1.0).contains(&l)
            {
                return Err("Invalid color grading values");
            }
        }
        for mask in self.masks {
            mask.validate()?;
        }
        for (value, low, high) in [
            (self.straighten, -45., 45.),
            (self.perspective[0], -0.8, 0.8),
            (self.perspective[1], -0.8, 0.8),
            (self.distortion, -0.5, 0.5),
            (self.aberration[0], -0.05, 0.05),
            (self.aberration[1], -0.05, 0.05),
            (self.exposure, -5.0, 5.0),
            (self.temperature, -1.0, 1.0),
            (self.tint, -1.0, 1.0),
            (self.highlights, -1.0, 1.0),
            (self.shadows, -1.0, 1.0),
            (self.whites, -1.0, 1.0),
            (self.blacks, -1.0, 1.0),
            (self.black_point, 0.0, 0.25),
            (self.brightness, -1.0, 1.0),
            (self.contrast, -1.0, 1.0),
            (self.saturation, -1.0, 1.0),
            (self.vibrance, -1.0, 1.0),
            (self.texture, -1.0, 1.0),
            (self.clarity, -1.0, 1.0),
            (self.dehaze, -1.0, 1.0),
            (self.vignette, -1.0, 1.0),
            (self.sharpening, 0.0, 1.0),
            (self.noise_reduction, 0.0, 1.0),
        ] {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err("RAW development parameter outside its supported range");
            }
        }
        if self
            .tone_curve
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || self.tone_curve.windows(2).any(|pair| pair[0] > pair[1])
        {
            return Err("RAW tone curve must be finite, bounded, and monotonic");
        }
        if self.wb_override.is_some_and(|wb| {
            wb.iter()
                .any(|v| !v.is_finite() || !(0.01..=100.0).contains(v))
        }) {
            return Err("RAW white balance gains must be finite and between 0.01 and 100");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RawMetadata {
    pub make: String,
    pub model: String,
    pub format: String,
    pub compression: String,
    pub bits_per_sample: u32,
    pub sensor: String,
    pub decoder: String,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<String>,
}

impl Default for RawMetadata {
    fn default() -> Self {
        Self {
            make: String::new(),
            model: String::new(),
            format: "unknown".into(),
            compression: "unknown".into(),
            bits_per_sample: 0,
            sensor: "unknown".into(),
            decoder: "unknown".into(),
            width: 0,
            height: 0,
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RawDocument {
    pub schema_version: u32,
    pub node_id: NodeId,
    /// Linked original. Never overwritten by saving the document.
    pub source: PathBuf,
    pub source_sha256: String,
    pub params: DevelopParams,
    pub metadata: RawMetadata,
}

impl RawDocument {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 {
            return Err("unsupported RAW recipe version");
        }
        if self.source.as_os_str().is_empty() {
            return Err("missing RAW source path");
        }
        if self.source_sha256.len() != 64
            || !self.source_sha256.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("invalid RAW source fingerprint");
        }
        self.params.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Document, Editor, Node, NodeKind};
    use emulsion_raster::Raster;
    use std::sync::Arc;

    fn document() -> Document {
        let mut doc = Document::new(2, 2);
        doc.nodes.push(Node::raster(
            1,
            "RAW",
            Arc::new(Raster::solid(2, 2, [0.2, 0.3, 0.4, 1.0])),
            Default::default(),
        ));
        doc.next_id = 2;
        doc.raw_originals = vec!["photo.dng".into()];
        doc.raw = Some(RawDocument {
            schema_version: 1,
            node_id: 1,
            source: "photo.dng".into(),
            source_sha256: "a".repeat(64),
            params: DevelopParams::default(),
            metadata: RawMetadata::default(),
        });
        doc
    }

    #[test]
    fn legacy_recipe_defaults_and_new_controls_roundtrip() {
        let legacy: DevelopParams =
            serde_json::from_str(r#"{"exposure":1.0,"temperature":0.2}"#).unwrap();
        assert_eq!(legacy.tone_curve, DevelopParams::LINEAR_CURVE);
        assert!(!legacy.smooth_curve);
        assert!(DevelopParams::default().smooth_curve);
        assert_eq!(legacy.wb_override, None);
        legacy.validate().unwrap();
        let params = DevelopParams {
            tone_curve: DevelopParams::MEDIUM_CONTRAST_CURVE,
            wb_override: Some([2.0, 1.0, 1.5, 1.0]),
            black_point: 0.02,
            brightness: 0.2,
            contrast: 0.3,
            saturation: -0.1,
            ..legacy
        };
        let json = serde_json::to_string(&params).unwrap();
        assert_eq!(
            serde_json::from_str::<DevelopParams>(&json).unwrap(),
            params
        );
        let mut invalid = params;
        invalid.tone_curve[2] = 0.0;
        assert!(invalid.validate().is_err());
        invalid = params;
        invalid.wb_override = Some([f32::NAN; 4]);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn smooth_raw_curve_matches_adjustment_and_has_continuous_tangents() {
        let params = DevelopParams {
            tone_curve: DevelopParams::STRONG_CONTRAST_CURVE,
            ..Default::default()
        };
        let points: [[f32; 2]; 5] =
            std::array::from_fn(|i| [i as f32 * 63.75, params.tone_curve[i] * 255.]);
        for i in 0..=1000 {
            let x = i as f32 / 1000.;
            assert_eq!(
                params.curve_output(x),
                emulsion_raster::adjust::curve_at(&points, x * 255.) / 255.
            );
        }
        for i in 1..4 {
            let x = i as f32 / 4.;
            assert!((params.curve_output(x) - params.tone_curve[i]).abs() < 1e-6);
            let left = (params.curve_output(x) - params.curve_output(x - 0.0001)) / 0.0001;
            let right = (params.curve_output(x + 0.0001) - params.curve_output(x)) / 0.0001;
            assert!((left - right).abs() < 0.01);
        }
        let legacy = DevelopParams {
            smooth_curve: false,
            ..params
        };
        assert!((legacy.curve_output(0.125) - 0.075).abs() < 1e-6);
        assert!((params.curve_output(0.125) - legacy.curve_output(0.125)).abs() > 0.001);
        let saved = serde_json::to_string(&params).unwrap();
        assert_eq!(
            serde_json::from_str::<DevelopParams>(&saved).unwrap(),
            params
        );
    }

    #[test]
    fn development_and_recipe_undo_together() {
        let before = document();
        let mut editor = Editor::new(before.clone(), None);
        let params = DevelopParams {
            exposure: 1.0,
            ..Default::default()
        };
        let pixels = Arc::new(Raster::solid(2, 2, [0.4, 0.6, 0.8, 1.0]));
        editor
            .execute(Command::DevelopRaw {
                id: 1,
                raster: pixels.clone(),
                params: Box::new(params),
            })
            .unwrap();
        assert_eq!(editor.doc.raw.as_ref().unwrap().params, params);
        assert!(
            matches!(&editor.doc.nodes[0].kind, NodeKind::Raster { raster, .. } if Arc::ptr_eq(raster, &pixels))
        );
        editor.undo();
        assert_eq!(editor.doc, before);
        editor.redo();
        assert_eq!(editor.doc.raw.as_ref().unwrap().params, params);
    }

    #[test]
    fn raw_crop_resizes_single_photo_and_undo_restores_canvas() {
        let before = document();
        let mut editor = Editor::new(before.clone(), None);
        let params = DevelopParams {
            crop: [0., 0., 0.5, 1.],
            ..Default::default()
        };
        let raster = Arc::new(Raster::solid(1, 2, [0.2, 0.3, 0.4, 1.]));
        editor
            .execute(Command::DevelopRaw {
                id: 1,
                raster: raster.clone(),
                params: Box::new(params),
            })
            .unwrap();
        assert_eq!((editor.doc.width, editor.doc.height), (1, 2));
        editor.undo();
        assert_eq!(editor.doc, before);
        editor.redo();
        assert_eq!((editor.doc.width, editor.doc.height), (1, 2));
        let mut layered = before;
        layered.nodes.push(Node::raster(
            2,
            "Overlay",
            Arc::new(Raster::solid(2, 2, [1.; 4])),
            Default::default(),
        ));
        layered.next_id = 3;
        let mut editor = Editor::new(layered.clone(), None);
        assert!(
            editor
                .execute(Command::DevelopRaw {
                    id: 1,
                    raster,
                    params: Box::new(params),
                })
                .is_err()
        );
        assert_eq!(editor.doc, layered);
    }

    #[test]
    fn painting_detaches_recipe_but_undo_restores_it() {
        let mut editor = Editor::new(document(), None);
        editor
            .execute(Command::ReplacePixels {
                id: 1,
                raster: Arc::new(Raster::solid(2, 2, [1.0; 4])),
                dirty: emulsion_raster::IRect::new(0, 0, 2, 2),
                label: "Paint".into(),
            })
            .unwrap();
        assert!(editor.doc.raw.is_none());
        editor.undo();
        assert!(editor.doc.raw.is_some());
        editor.execute(Command::RemoveNode { id: 1 }).unwrap();
        assert!(editor.doc.raw.is_none());
    }

    #[test]
    fn raw_validation_and_locks_are_atomic() {
        let mut doc = document();
        let before = doc.clone();
        let mut command = Command::DevelopRaw {
            id: 1,
            raster: Arc::new(Raster::solid(2, 2, [1.0; 4])),
            params: Box::new(DevelopParams {
                exposure: f32::NAN,
                ..Default::default()
            }),
        };
        assert!(command.apply(&mut doc).is_err());
        assert_eq!(doc, before);
        if let Command::DevelopRaw { params, .. } = &mut command {
            params.exposure = 1.0;
        }
        doc.nodes[0].locked = true;
        let locked = doc.clone();
        assert!(command.apply(&mut doc).is_err());
        assert_eq!(doc, locked);
        doc.nodes[0].locked = false;
        doc.raw.as_mut().unwrap().schema_version = 2;
        assert!(doc.validate().is_err());
    }

    #[test]
    fn smart_conversion_preserves_raw_and_development_updates_source() {
        let mut doc = document();
        Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
        assert!(doc.raw.is_some());
        let pixels = Arc::new(Raster::solid(2, 2, [0.5; 4]));
        Command::DevelopRaw {
            id: 1,
            raster: pixels.clone(),
            params: Box::default(),
        }
        .apply(&mut doc)
        .unwrap();
        assert!(
            matches!(&doc.nodes[0].kind, NodeKind::Smart { source, .. } if Arc::ptr_eq(source, &pixels))
        );
    }

    #[test]
    fn merge_keeps_recipe_with_chosen_pixels_and_relink_is_undoable() {
        use crate::graph::{ConflictKey, MergeOutcome, Side, merge};
        use std::collections::HashMap;
        let base = document();
        let mut ours = base.clone();
        let mut theirs = base.clone();
        for (doc, exposure, level) in [(&mut ours, 1.0, 0.7), (&mut theirs, -1.0, 0.1)] {
            Command::DevelopRaw {
                id: 1,
                raster: Arc::new(Raster::solid(2, 2, [level; 4])),
                params: Box::new(DevelopParams {
                    exposure,
                    ..Default::default()
                }),
            }
            .apply(doc)
            .unwrap();
        }
        assert!(matches!(
            merge(&base, &ours, &theirs, &HashMap::new()).unwrap(),
            MergeOutcome::Conflicts(_)
        ));
        let choices = HashMap::from([(ConflictKey::Node(1), Side::Theirs)]);
        let MergeOutcome::Merged(merged) = merge(&base, &ours, &theirs, &choices).unwrap() else {
            panic!("resolved merge")
        };
        assert_eq!(merged.raw, theirs.raw);
        let MergeOutcome::Merged(merged) = merge(&base, &base, &theirs, &HashMap::new()).unwrap()
        else {
            panic!("clean merge")
        };
        assert_eq!(merged.raw, theirs.raw);
        let mut editor = Editor::new(base.clone(), None);
        editor
            .execute(Command::RelinkRaw {
                source: "moved/photo.dng".into(),
            })
            .unwrap();
        assert_eq!(
            editor.doc.raw.as_ref().unwrap().source,
            PathBuf::from("moved/photo.dng")
        );
        editor.undo();
        assert_eq!(editor.doc.raw, base.raw);
        assert!(
            editor
                .doc
                .raw_originals
                .contains(&PathBuf::from("moved/photo.dng"))
        );
        editor.redo();
        assert!(
            editor
                .doc
                .raw_originals
                .contains(&PathBuf::from("photo.dng"))
        );
    }
}

#[cfg(test)]
mod point_curve_tests {
    use super::*;
    #[test]
    fn smooth_point_curves_match_photo_pass_knots_and_have_no_kinks() {
        let points = vec![[0., 0.], [0.2, 0.08], [0.6, 0.8], [1., 1.]];
        let mut p = DevelopParams::default();
        p.point_curves[0] = PointCurve::try_from(points.clone()).unwrap();
        let photo = points
            .iter()
            .map(|v| v.map(|c| c * 255.))
            .collect::<Vec<_>>();
        let mut last = 0.;
        for i in 0..=1000 {
            let x = i as f32 / 1000.;
            let y = p.point_curve_output(0, x);
            assert!((y - emulsion_raster::adjust::curve_at(&photo, x * 255.) / 255.).abs() < 1e-6);
            assert!(y >= last - 1e-6);
            last = y;
        }
        for [x, y] in points {
            assert!((p.point_curve_output(0, x) - y).abs() < 1e-6);
        }
        for x in [0.2, 0.6] {
            let h = 0.0001;
            let left = (p.point_curve_output(0, x) - p.point_curve_output(0, x - h)) / h;
            let right = (p.point_curve_output(0, x + h) - p.point_curve_output(0, x)) / h;
            assert!((left - right).abs() < 0.01);
        }
        p.point_curves[1] =
            PointCurve::try_from(vec![[0., 1.], [0.4, 0.1], [0.7, 0.8], [1., 0.]]).unwrap();
        for i in 0..=1000 {
            assert!((0. ..=1.).contains(&p.point_curve_output(1, i as f32 / 1000.)));
        }
    }
    #[test]
    fn existing_point_curve_rendering_is_preserved_until_edited() {
        let p: DevelopParams =
            serde_json::from_str(r#"{"point_curves":[[[0,0],[0.25,0.1],[1,1]],[],[],[]]}"#)
                .unwrap();
        assert_eq!(p.smooth_point_curves, [false; 4]);
        assert_eq!(p.rotation, 0);
        assert_eq!(p.point_curve_output(0, 0.125), 0.05);
        let mut edit = p;
        edit.smooth_point_curves[0] = true;
        assert_ne!(edit.point_curve_output(0, 0.125), 0.05);
        assert_eq!(
            serde_json::from_str::<DevelopParams>(&serde_json::to_string(&edit).unwrap()).unwrap(),
            edit
        );
        assert!(
            DevelopParams {
                rotation: 4,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
