//! Named develop presets: save the look of an edit to the local preset bank
//! and export it as an Adobe Camera Raw/Lightroom compatible `.xmp`.
//!
//! A preset carries the look (tone, presence, curves, HSL, grading,
//! calibration, detail, effects) but never photo-specific settings: crop,
//! geometry, lens profile, local masks, depth and sampled camera gains stay
//! with the photo the preset is applied to.
use crate::{IoError, Result, lightroom_presets, raw_settings};
use emulsion_core::raw::DevelopParams;
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

fn invalid(message: impl Into<String>) -> IoError {
    IoError::Manifest(format!("Develop preset: {}", message.into()))
}

/// What to carry besides the look itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub exposure: bool,
    pub white_balance: bool,
}

/// Keep only the portable look of `params`.
pub fn portable(params: &DevelopParams, scope: Scope) -> DevelopParams {
    let d = DevelopParams::default();
    let mut p = *params;
    p.crop = d.crop;
    p.rotation = d.rotation;
    p.straighten = d.straighten;
    p.perspective = d.perspective;
    p.distortion = d.distortion;
    p.lens_profile = d.lens_profile;
    p.aberration = d.aberration;
    p.masks = d.masks;
    p.local_edits = d.local_edits;
    p.depth_map = d.depth_map;
    p.depth_blur = d.depth_blur;
    p.depth_focus = d.depth_focus;
    p.depth_range = d.depth_range;
    p.camera_profile = d.camera_profile;
    p.wb_override = None;
    if !scope.exposure {
        p.exposure = d.exposure;
        p.brightness = d.brightness;
        p.black_point = d.black_point;
    }
    if !scope.white_balance {
        p.kelvin = d.kelvin;
        p.temperature = d.temperature;
        p.tint = d.tint;
    }
    p
}

/// Settings left behind by `portable`, for telling the person what stayed.
pub fn excluded(params: &DevelopParams, scope: Scope) -> Vec<&'static str> {
    let d = DevelopParams::default();
    let mut out = Vec::new();
    if params.crop != d.crop
        || params.rotation != d.rotation
        || params.straighten != d.straighten
        || params.perspective != d.perspective
        || params.distortion != d.distortion
    {
        out.push("crop and geometry");
    }
    if params.lens_profile.is_some() || params.aberration != d.aberration {
        out.push("lens corrections");
    }
    if params.masks.iter().any(|m| m.enabled) || params.local_edits.is_some() {
        out.push("local masks and retouching");
    }
    if params.depth_map.is_some() {
        out.push("depth blur");
    }
    if params.camera_profile.is_some() {
        out.push("camera profile");
    }
    if params.wb_override.is_some() {
        out.push("sampled white balance (camera-specific)");
    }
    if !scope.exposure && (params.exposure, params.brightness, params.black_point) != (0., 0., 0.) {
        out.push("exposure");
    }
    if !scope.white_balance
        && (params.kelvin.is_some() || params.temperature != 0. || params.tint != 0.)
    {
        out.push("white balance");
    }
    out
}

/// Apply a preset to a photo, keeping the photo's own geometry, lens, masks,
/// depth and camera profile. Exposure and white balance are kept unless the
/// preset carries them.
pub fn apply(current: &DevelopParams, preset: &DevelopParams) -> DevelopParams {
    let mut p = *preset;
    p.crop = current.crop;
    p.rotation = current.rotation;
    p.straighten = current.straighten;
    p.perspective = current.perspective;
    p.distortion = current.distortion;
    p.lens_profile = current.lens_profile;
    p.aberration = current.aberration;
    p.masks = current.masks;
    p.local_edits = current.local_edits;
    p.depth_map = current.depth_map;
    p.depth_blur = current.depth_blur;
    p.depth_focus = current.depth_focus;
    p.depth_range = current.depth_range;
    p.camera_profile = current.camera_profile;
    if (preset.exposure, preset.brightness, preset.black_point) == (0., 0., 0.) {
        p.exposure = current.exposure;
        p.brightness = current.brightness;
        p.black_point = current.black_point;
    }
    if preset.kelvin.is_none() && preset.temperature == 0. && preset.tint == 0. {
        p.kelvin = current.kelvin;
        p.temperature = current.temperature;
        p.tint = current.tint;
        p.wb_override = current.wb_override;
    }
    p
}

/// File-name stem for a preset name; rejects names that sanitise to nothing.
fn file_stem(name: &str) -> Result<String> {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_ ".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    let stem = stem.trim().to_string();
    if stem.is_empty() || stem.chars().all(|c| c == '_') {
        return Err(invalid("give the preset a name with letters or digits"));
    }
    Ok(stem)
}

pub fn library_path(name: &str) -> Result<PathBuf> {
    Ok(lightroom_presets::library_dir().join(format!("{}.json", file_stem(name)?)))
}

/// Save into the preset bank shown in Emulsion's Develop panel.
pub fn save_to_library(params: DevelopParams, name: &str, overwrite: bool) -> Result<PathBuf> {
    save_to(&lightroom_presets::library_dir(), params, name, overwrite)
}

fn save_to(dir: &Path, params: DevelopParams, name: &str, overwrite: bool) -> Result<PathBuf> {
    let path = dir.join(format!("{}.json", file_stem(name)?));
    if path.exists() && !overwrite {
        return Err(invalid(format!(
            "a preset named \"{name}\" already exists; choose another name or overwrite it"
        )));
    }
    std::fs::create_dir_all(dir)?;
    raw_settings::save_preset(params, &path)?;
    Ok(path)
}

/// Saved presets in the bank: (display name, path).
pub fn library() -> Vec<(String, PathBuf)> {
    lightroom_presets::installed()
        .into_iter()
        .map(|path| {
            let name = lightroom_presets::load(&path, DevelopParams::default())
                .map(|p| p.name)
                .unwrap_or_else(|_| {
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into()
                });
            (name, path)
        })
        .collect()
}

/// Find a bank preset by display name or file stem, ignoring case.
pub fn find(name: &str) -> Option<PathBuf> {
    let wanted = name.trim().to_lowercase();
    library().into_iter().find_map(|(display, path)| {
        let stem = path.file_stem()?.to_string_lossy().to_lowercase();
        (display.to_lowercase() == wanted || stem == wanted).then_some(path)
    })
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Render an Adobe Camera Raw settings preset (`.xmp`) that Lightroom Classic,
/// Lightroom and Camera Raw can import, and that Emulsion reads back.
/// Emulsion-only controls (sensor denoise, depth, smooth curves) have no
/// Adobe equivalent and are left out.
pub fn to_xmp(params: &DevelopParams, name: &str) -> String {
    let p = params;
    let pct = |v: f32| format!("{:+.0}", v * 100.);
    let mut attrs: Vec<(String, String)> = vec![
        ("PresetType".into(), "Normal".into()),
        ("Cluster".into(), String::new()),
        ("Version".into(), "15.0".into()),
        ("ProcessVersion".into(), "11.0".into()),
        ("HasSettings".into(), "True".into()),
        ("SupportsAmount".into(), "False".into()),
        ("SupportsColor".into(), "True".into()),
        ("SupportsMonochrome".into(), "True".into()),
        ("SupportsNormalDynamicRange".into(), "True".into()),
        ("SupportsOutputReferred".into(), "False".into()),
    ];
    let mut set = |key: &str, value: String| attrs.push((key.into(), value));
    if p.exposure != 0. {
        set("Exposure2012", format!("{:+.2}", p.exposure));
    }
    for (key, v) in [
        ("Contrast2012", p.contrast),
        ("Highlights2012", -p.highlights),
        ("Shadows2012", p.shadows),
        ("Whites2012", p.whites),
        ("Blacks2012", p.blacks),
        ("Texture", p.texture),
        ("Clarity2012", p.clarity),
        ("Dehaze", p.dehaze),
        ("Vibrance", p.vibrance),
        ("Saturation", p.saturation),
        ("PostCropVignetteAmount", p.vignette),
        ("ParametricShadows", p.parametric[0]),
        ("ParametricDarks", p.parametric[1]),
        ("ParametricLights", p.parametric[2]),
        ("ParametricHighlights", p.parametric[3]),
        ("ShadowTint", p.shadow_tint),
        ("RedHue", p.calibration[0][0]),
        ("RedSaturation", p.calibration[0][1]),
        ("GreenHue", p.calibration[1][0]),
        ("GreenSaturation", p.calibration[1][1]),
        ("BlueHue", p.calibration[2][0]),
        ("BlueSaturation", p.calibration[2][1]),
        ("ColorGradeBalance", p.grading_balance),
    ] {
        set(key, pct(v));
    }
    for (key, v) in [
        ("ParametricShadowSplit", p.parametric_splits[0]),
        ("ParametricMidtoneSplit", p.parametric_splits[1]),
        ("ParametricHighlightSplit", p.parametric_splits[2]),
    ] {
        set(key, format!("{:.0}", v * 100.));
    }
    if p.saturation <= -1. {
        set("ConvertToGrayscale", "True".into());
        set("Treatment", "Monochrome".into());
        for (c, color) in [
            "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
        ]
        .iter()
        .enumerate()
        {
            set(&format!("GrayMixer{color}"), pct(p.gray_mixer[c]));
        }
    } else {
        set("Treatment", "Color".into());
    }
    set("GrainAmount", format!("{:.0}", p.grain[0] * 100.));
    set("GrainSize", format!("{:.0}", p.grain[1] * 100.));
    set("GrainFrequency", format!("{:.0}", p.grain[2] * 100.));
    match p.kelvin {
        Some(kelvin) => {
            set("WhiteBalance", "Custom".into());
            set("Temperature", format!("{kelvin:.0}"));
            set("Tint", format!("{:+.0}", p.tint * 150.));
        }
        None if p.temperature != 0. || p.tint != 0. => {
            set("IncrementalTemperature", pct(p.temperature));
            set("IncrementalTint", pct(p.tint));
        }
        None => {}
    }
    for (c, color) in [
        "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
    ]
    .iter()
    .enumerate()
    {
        for (j, prefix) in [
            "HueAdjustment",
            "SaturationAdjustment",
            "LuminanceAdjustment",
        ]
        .iter()
        .enumerate()
        {
            set(&format!("{prefix}{color}"), pct(p.hsl[c][j]));
        }
    }
    let [shadow, mid, high] = p.grading;
    set("ColorGradeShadowHue", format!("{:.0}", shadow[0]));
    set("ColorGradeShadowSat", format!("{:.0}", shadow[1] * 100.));
    set("ColorGradeHighlightHue", format!("{:.0}", high[0]));
    set("ColorGradeHighlightSat", format!("{:.0}", high[1] * 100.));
    set("ColorGradeMidtoneHue", format!("{:.0}", mid[0]));
    set("ColorGradeMidtoneSat", format!("{:.0}", mid[1] * 100.));
    set("ColorGradeShadowLum", pct(shadow[2]));
    set("ColorGradeMidtoneLum", pct(mid[2]));
    set("ColorGradeHighlightLum", pct(high[2]));
    set("ColorGradeGlobalHue", format!("{:.0}", p.global_grading[0]));
    set(
        "ColorGradeGlobalSat",
        format!("{:.0}", p.global_grading[1] * 100.),
    );
    set("ColorGradeGlobalLum", pct(p.global_grading[2]));
    set(
        "ColorGradeBlending",
        format!("{:.0}", p.grading_blending * 100.),
    );
    set("Sharpness", format!("{:.0}", p.sharpening * 150.));
    set("SharpenRadius", format!("{:+.1}", p.sharpening_radius));
    set(
        "SharpenDetail",
        format!("{:.0}", p.sharpening_detail * 100.),
    );
    set(
        "SharpenEdgeMasking",
        format!("{:.0}", p.sharpening_masking * 100.),
    );
    set(
        "LuminanceSmoothing",
        format!("{:.0}", p.noise_reduction * 100.),
    );
    set(
        "LuminanceNoiseReductionDetail",
        format!("{:.0}", p.luminance_detail * 100.),
    );
    set(
        "LuminanceNoiseReductionContrast",
        format!("{:.0}", p.luminance_contrast * 100.),
    );
    set(
        "ColorNoiseReduction",
        format!("{:.0}", p.color_noise_reduction * 100.),
    );
    set(
        "ColorNoiseReductionDetail",
        format!("{:.0}", p.color_noise_detail * 100.),
    );
    set(
        "ColorNoiseReductionSmoothness",
        format!("{:.0}", p.color_noise_smoothness * 100.),
    );

    // The five-point tone curve has no Adobe field; fold it into the
    // composite point curve when that curve is otherwise unused.
    let mut curves = p.point_curves.map(Vec::<[f32; 2]>::from);
    if curves[0].is_empty() && p.tone_curve != DevelopParams::LINEAR_CURVE {
        curves[0] = (0..5).map(|i| [i as f32 / 4., p.tone_curve[i]]).collect();
    }
    set("ToneCurveName2012", "Custom".into());
    let mut body = String::new();
    for (key, points) in [
        "ToneCurvePV2012",
        "ToneCurvePV2012Red",
        "ToneCurvePV2012Green",
        "ToneCurvePV2012Blue",
    ]
    .iter()
    .zip(&curves)
    {
        let points = if points.is_empty() {
            vec![[0., 0.], [1., 1.]]
        } else {
            points.clone()
        };
        let _ = write!(body, "   <crs:{key}>\n    <rdf:Seq>\n");
        for [x, y] in points {
            let _ = writeln!(
                body,
                "     <rdf:li>{:.0}, {:.0}</rdf:li>",
                x * 255.,
                y * 255.
            );
        }
        let _ = write!(body, "    </rdf:Seq>\n   </crs:{key}>\n");
    }
    let mut out = String::from(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Emulsion\">\n <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  <rdf:Description rdf:about=\"\"\n    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n",
    );
    for (key, value) in &attrs {
        let _ = writeln!(out, "   crs:{key}=\"{}\"", escape(value));
    }
    out.push_str("   >\n");
    let _ = write!(
        out,
        "   <crs:Name>\n    <rdf:Alt>\n     <rdf:li xml:lang=\"x-default\">{}</rdf:li>\n    </rdf:Alt>\n   </crs:Name>\n",
        escape(name.trim())
    );
    out.push_str(&body);
    out.push_str("  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n");
    out
}

/// Write an `.xmp` preset; never replaces an existing file unless asked.
pub fn export_xmp(params: &DevelopParams, name: &str, path: &Path, overwrite: bool) -> Result<()> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xmp"))
    {
        return Err(invalid("export path must end in .xmp"));
    }
    if path.exists() && !overwrite {
        return Err(invalid(format!(
            "{} already exists; choose another path or overwrite it",
            path.display()
        )));
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, to_xmp(params, name))?;
    Ok(())
}

pub fn default_export_dir() -> PathBuf {
    crate::recent::data_dir().join("exported-presets")
}

pub fn default_export_path(name: &str) -> Result<PathBuf> {
    Ok(default_export_dir().join(format!("{}.xmp", file_stem(name)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graded() -> DevelopParams {
        let mut p = DevelopParams {
            exposure: 0.7,
            temperature: 0.2,
            contrast: 0.15,
            highlights: 0.3,
            shadows: 0.2,
            whites: 0.1,
            blacks: -0.1,
            vibrance: 0.2,
            saturation: -0.1,
            clarity: 0.1,
            texture: -0.05,
            dehaze: 0.05,
            vignette: -0.15,
            crop: [0.1, 0.1, 0.9, 0.9],
            straighten: 2.,
            grading: [[210., 0.12, -0.05], [30., 0.04, 0.], [45., 0.18, 0.02]],
            global_grading: [40., 0.06, 0.],
            calibration: [[0.05, 0.1], [0., 0.], [-0.05, 0.2]],
            sharpening: 0.4,
            grain: [0.3, 0.25, 0.5],
            ..DevelopParams::default()
        };
        p.hsl[1] = [0.03, -0.05, 0.1];
        p.hsl[3] = [-0.3, -0.4, -0.1];
        p.point_curves[0] =
            emulsion_core::raw::PointCurve::try_from(vec![[0., 0.04], [0.5, 0.5], [1., 0.96]])
                .unwrap();
        p.masks[0].enabled = true;
        p
    }

    #[test]
    fn presets_keep_the_look_and_leave_photo_specific_settings() {
        let p = graded();
        let preset = portable(&p, Scope::default());
        assert_eq!(preset.crop, DevelopParams::default().crop);
        assert!(!preset.masks[0].enabled);
        assert_eq!((preset.exposure, preset.temperature), (0., 0.));
        assert_eq!(preset.hsl, p.hsl);
        assert_eq!(preset.grading, p.grading);
        let skipped = excluded(&p, Scope::default());
        for item in [
            "crop and geometry",
            "local masks and retouching",
            "exposure",
            "white balance",
        ] {
            assert!(skipped.contains(&item), "{skipped:?}");
        }
        let full = portable(
            &p,
            Scope {
                exposure: true,
                white_balance: true,
            },
        );
        assert_eq!((full.exposure, full.temperature), (0.7, 0.2));

        // Applying keeps the target photo's crop, masks, exposure and balance.
        let target = DevelopParams {
            crop: [0.2, 0., 1., 1.],
            exposure: -0.3,
            kelvin: Some(5200.),
            ..DevelopParams::default()
        };
        let applied = apply(&target, &preset);
        assert_eq!(applied.crop, target.crop);
        assert_eq!((applied.exposure, applied.kelvin), (-0.3, Some(5200.)));
        assert_eq!(applied.hsl, p.hsl);
        assert_eq!(apply(&target, &full).exposure, 0.7);
        applied.validate().unwrap();
    }

    #[test]
    fn xmp_export_round_trips_through_the_lightroom_importer() {
        let p = portable(
            &graded(),
            Scope {
                exposure: true,
                white_balance: true,
            },
        );
        let dir = std::env::temp_dir().join(format!("emulsion-xmp-export-{}", std::process::id()));
        let path = dir.join("Warm & Moody.xmp");
        export_xmp(&p, "Warm & Moody", &path, false).unwrap();
        assert!(export_xmp(&p, "Warm & Moody", &path, false).is_err());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("crs:Exposure2012=\"+0.70\""));
        assert!(text.contains("crs:Highlights2012=\"-30\""));
        let back = lightroom_presets::load(&path, DevelopParams::default()).unwrap();
        assert_eq!(back.name, "Warm & Moody");
        let q = back.params;
        let close = |a: f32, b: f32| (a - b).abs() < 0.006;
        for (a, b) in [
            (q.exposure, p.exposure),
            (q.temperature, p.temperature),
            (q.contrast, p.contrast),
            (q.highlights, p.highlights),
            (q.shadows, p.shadows),
            (q.whites, p.whites),
            (q.blacks, p.blacks),
            (q.vibrance, p.vibrance),
            (q.saturation, p.saturation),
            (q.clarity, p.clarity),
            (q.texture, p.texture),
            (q.dehaze, p.dehaze),
            (q.vignette, p.vignette),
            (q.sharpening, p.sharpening),
        ] {
            assert!(close(a, b), "{a} vs {b}");
        }
        for i in 0..8 {
            for c in 0..3 {
                assert!(close(q.hsl[i][c], p.hsl[i][c]));
            }
        }
        for i in 0..3 {
            for c in 0..3 {
                assert!(close(q.grading[i][c], p.grading[i][c]), "{:?}", q.grading);
            }
            assert!(close(q.calibration[i][0], p.calibration[i][0]));
            assert!(close(q.calibration[i][1], p.calibration[i][1]));
        }
        assert!(close(q.global_grading[1], p.global_grading[1]));
        for c in 0..3 {
            assert!(close(q.grain[c], p.grain[c]), "{:?}", q.grain);
        }
        // Monochrome presets carry their B&W mix.
        let mono = DevelopParams {
            saturation: -1.,
            gray_mixer: [0.1, 0.15, 0.2, -0.1, -0.15, -0.2, 0., -0.05],
            ..DevelopParams::default()
        };
        let mono_path = dir.join("Mono.xmp");
        export_xmp(&mono, "Mono", &mono_path, false).unwrap();
        let back = lightroom_presets::load(&mono_path, DevelopParams::default()).unwrap();
        assert_eq!(back.params.saturation, -1.);
        for c in 0..8 {
            assert!(close(back.params.gray_mixer[c], mono.gray_mixer[c]));
        }
        for x in [0.1, 0.5, 0.9] {
            assert!((q.point_curves[0].output(x) - p.point_curves[0].output(x)).abs() < 0.01);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn library_saves_by_name_and_refuses_silent_overwrites() {
        let dir = std::env::temp_dir().join(format!("emulsion-preset-bank-{}", std::process::id()));
        let p = portable(&graded(), Scope::default());
        let path = save_to(&dir, p, "My Film Look", false).unwrap();
        assert_eq!(path.file_name().unwrap(), "My Film Look.json");
        assert!(save_to(&dir, p, "My Film Look", false).is_err());
        assert!(save_to(&dir, p, "My Film Look", true).is_ok());
        assert!(save_to(&dir, p, "../..", false).is_err());
        let loaded = lightroom_presets::load(&path, DevelopParams::default()).unwrap();
        assert_eq!(loaded.name, "My Film Look");
        assert_eq!(loaded.params, p);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
