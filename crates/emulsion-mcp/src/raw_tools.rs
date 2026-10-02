//! RAW tools share the document's decoder, versioned recipes and command history.
use crate::{
    exec::Planned,
    server::{ToolDef, ToolResult},
};
use emulsion_core::{
    Command, Document,
    raw::{DevelopParams, RawDocument},
};
use emulsion_io::{
    photo_develop::PhotoSource,
    raw_settings::{self as settings, RawSettingsGroup},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

pub const HEAVY: &[&str] = &[
    "develop_raw",
    "auto_develop_raw",
    "pick_raw_white_balance",
    "reset_raw",
    "raw_settings",
    "relink_raw",
    "apply_raw_look",
    "apply_raw_preset",
    "mask_raw",
    "auto_mask_raw",
];
fn error(message: impl ToString) -> ToolResult {
    ToolResult::error(message.to_string())
}
fn raw(doc: &Document) -> Result<&RawDocument, ToolResult> {
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| error("No editable RAW source; open a supported camera RAW file first"))?;
    raw.validate().map_err(error)?;
    Ok(raw)
}
fn strict(args: &Value, allowed: &[&str]) -> Result<(), ToolResult> {
    let object = args
        .as_object()
        .ok_or_else(|| error("Arguments must be an object"))?;
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(error(format!("Unknown argument '{key}'")));
        }
    }
    Ok(())
}
fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolResult> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| error(format!("'{key}' must be a nonempty string")))
}
fn group(args: &Value) -> Result<RawSettingsGroup, ToolResult> {
    match args.get("group") {
        None => Ok(RawSettingsGroup::All),
        Some(v) => match v.as_str() {
            Some("all") => Ok(RawSettingsGroup::All),
            Some("white_balance") => Ok(RawSettingsGroup::WhiteBalance),
            Some("tone") => Ok(RawSettingsGroup::Tone),
            Some("curve") => Ok(RawSettingsGroup::Curve),
            _ => Err(error("group must be all, white_balance, tone, or curve")),
        },
    }
}
pub fn describe(doc: &Document, args: &Value) -> Result<ToolResult, ToolResult> {
    strict(args, &[])?;
    let raw = raw(doc)?;
    Ok(ToolResult::text(json!({"raw":raw,"source_exists":raw.source.is_file(),
        "working_space":if raw.params.wide_gamut {"linear ProPhoto RGB"} else {"linear sRGB"}, "white_balance_units":"relative offsets, not Kelvin",
        "neutral_picker_coordinates":"oriented/cropped RAW raster pixels, before layer transforms",
        "curve_presets":{"linear":DevelopParams::LINEAR_CURVE,"medium":DevelopParams::MEDIUM_CONTRAST_CURVE,"strong":DevelopParams::STRONG_CONTRAST_CURVE},
        "settings_format":"Emulsion JSON, not Adobe XMP", "original_preserved":true}).to_string()))
}
fn planned(commands: Vec<Command>, message: impl Into<String>) -> Planned {
    Planned {
        commands,
        message: message.into(),
        feedback: None,
        deferred: None,
    }
}
pub(crate) fn develop(
    doc: &Document,
    params: DevelopParams,
    source: Option<PhotoSource>,
) -> Result<Planned, ToolResult> {
    params.validate().map_err(error)?;
    let raw = raw(doc)?;
    if params == raw.params {
        return Ok(planned(Vec::new(), "RAW settings unchanged"));
    }
    let source = match source {
        Some(s) => s,
        None => PhotoSource::load_verified(&raw.source, &raw.source_sha256).map_err(error)?,
    };
    let raster = Arc::new(source.develop_with(&params).map_err(error)?);
    Ok(planned(
        vec![Command::DevelopRaw {
            id: raw.node_id,
            raster,
            params: Box::new(params),
        }],
        json!({"settings":params,"undo_steps":1}).to_string(),
    ))
}

/// Persist only after the host accepts the snapshot. A second RAW identity check
/// also protects direct callers of `apply`; planning never writes settings.
pub(crate) struct SettingsWrite {
    expected: RawDocument,
    action: String,
    path: Option<PathBuf>,
}
impl SettingsWrite {
    pub(crate) fn apply(self, doc: &Document) -> ToolResult {
        if doc.raw.as_ref() != Some(&self.expected) {
            return error("RAW changed before settings could be saved; retry");
        }
        let result = match self.action.as_str() {
            "save_sidecar" => {
                settings::save_sidecar(doc, self.path.as_ref().expect("validated path"))
            }
            "save_preset" => {
                settings::save_document_preset(doc, self.path.as_ref().expect("validated path"))
            }
            "save_camera_defaults" => {
                settings::save_camera_defaults(&self.expected.metadata, self.expected.params)
            }
            "reset_camera_defaults" => settings::reset_camera_defaults(&self.expected.metadata),
            _ => unreachable!(),
        };
        match result {
            Ok(()) => ToolResult::text(
                json!({"action":self.action,"path":self.path,"document_unchanged":true})
                    .to_string(),
            ),
            Err(e) => error(e),
        }
    }
}
pub fn plan(doc: &Document, name: &str, args: &Value) -> Result<Planned, ToolResult> {
    let raw = raw(doc)?;
    match name {
        "develop_raw" => {
            strict(args, &["settings", "curve_preset"])?;
            let mut value = serde_json::to_value(raw.params).map_err(error)?;
            if let Some(patch) = args.get("settings") {
                let patch = patch
                    .as_object()
                    .ok_or_else(|| error("settings must be an object"))?;
                for (key, next) in patch {
                    if value.get(key).is_none() {
                        return Err(error(format!("Unknown RAW setting '{key}'")));
                    }
                    value[key] = next.clone();
                }
            }
            if let Some(preset) = args.get("curve_preset") {
                if args
                    .get("settings")
                    .and_then(|p| p.get("tone_curve"))
                    .is_some()
                {
                    return Err(error(
                        "Choose curve_preset or settings.tone_curve, not both",
                    ));
                }
                value["point_curves"][0] = json!([]);
                value["tone_curve"] = json!(match preset.as_str() {
                    Some("linear") => DevelopParams::LINEAR_CURVE,
                    Some("medium") => DevelopParams::MEDIUM_CONTRAST_CURVE,
                    Some("strong") => DevelopParams::STRONG_CONTRAST_CURVE,
                    _ => return Err(error("curve_preset must be linear, medium, or strong")),
                });
            }
            if args.get("settings").is_none() && args.get("curve_preset").is_none() {
                return Err(error("Supply settings or curve_preset"));
            }
            develop(doc, serde_json::from_value(value).map_err(error)?, None)
        }
        "apply_raw_look" => crate::raw_looks::plan(doc, args),
        "apply_raw_preset" => crate::raw_presets::plan(doc, args),
        "mask_raw" => crate::raw_masks::plan(doc, args),
        "auto_mask_raw" => crate::raw_masks::plan_auto(doc, args),
        "auto_develop_raw" => {
            strict(args, &[])?;
            let source =
                PhotoSource::load_verified(&raw.source, &raw.source_sha256).map_err(error)?;
            develop(
                doc,
                source.auto_adjust(&raw.params).map_err(error)?,
                Some(source),
            )
        }
        "pick_raw_white_balance" => {
            strict(args, &["x", "y"])?;
            let coordinate = |key| {
                args.get(key)
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(|| error(format!("{key} must be a nonnegative pixel integer")))
            };
            let (x, y) = (coordinate("x")?, coordinate("y")?);
            let source =
                PhotoSource::load_verified(&raw.source, &raw.source_sha256).map_err(error)?;
            develop(
                doc,
                source
                    .neutral_white_balance(&raw.params, x, y)
                    .map_err(error)?,
                Some(source),
            )
        }
        "reset_raw" => {
            strict(args, &["group"])?;
            develop(
                doc,
                settings::merge_settings(raw.params, DevelopParams::default(), group(args)?),
                None,
            )
        }
        "relink_raw" => {
            strict(args, &["path"])?;
            let path = PathBuf::from(string(args, "path")?);
            if !emulsion_io::raw::source_digest(&path)
                .map_err(error)?
                .eq_ignore_ascii_case(&raw.source_sha256)
            {
                return Err(error(
                    "Selected file does not match the original RAW fingerprint",
                ));
            }
            let source = std::fs::canonicalize(path).map_err(error)?;
            Ok(planned(
                vec![Command::RelinkRaw { source }],
                "Relinked RAW original; pixels and settings are unchanged",
            ))
        }
        "raw_settings" => {
            strict(args, &["action", "path", "group"])?;
            let action = string(args, "action")?;
            let group = group(args)?;
            let path = match action {
                "save_sidecar" | "load_sidecar" => Some(match args.get("path") {
                    Some(_) => PathBuf::from(string(args, "path")?),
                    None => settings::suggested_sidecar_path(doc).map_err(error)?,
                }),
                "save_preset" | "load_preset" => Some(PathBuf::from(string(args, "path")?)),
                "save_camera_defaults" | "apply_camera_defaults" | "reset_camera_defaults" => {
                    if args.get("path").is_some() {
                        return Err(error(
                            "Camera defaults use Emulsion's camera-model store; omit path",
                        ));
                    }
                    None
                }
                _ => return Err(error("Unknown RAW settings action")),
            };
            if matches!(
                action,
                "save_sidecar" | "save_preset" | "save_camera_defaults" | "reset_camera_defaults"
            ) {
                if args.get("group").is_some() {
                    return Err(error(
                        "group applies only when loading or applying settings",
                    ));
                }
                let mut p = planned(Vec::new(), "Save RAW settings");
                p.deferred = Some(SettingsWrite {
                    expected: raw.clone(),
                    action: action.into(),
                    path,
                });
                return Ok(p);
            }
            let params = match action {
                "load_sidecar" => {
                    settings::load_sidecar(doc, path.as_ref().unwrap()).map_err(error)?
                }
                "load_preset" => settings::load_document_preset(doc, path.as_ref().unwrap(), group)
                    .map_err(error)?,
                "apply_camera_defaults" => settings::camera_defaults(&raw.metadata)
                    .map_err(error)?
                    .ok_or_else(|| error("No defaults saved for this camera model"))?,
                _ => unreachable!(),
            };
            develop(
                doc,
                settings::merge_settings(raw.params, params, group),
                None,
            )
        }
        _ => Err(error("Unknown RAW tool")),
    }
}

pub fn definitions() -> Vec<ToolDef> {
    fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
        ToolDef {
            name: name.into(),
            description: description.into(),
            input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
        }
    }
    fn range(min: f32, max: f32) -> Value {
        json!({"type":"number","minimum":min,"maximum":max})
    }
    let point_curves = json!({"type":"array","minItems":4,"maxItems":4,"items":{"type":"array","maxItems":32,"items":{"type":"array","minItems":2,"maxItems":2,"items":range(0.,1.)}}});
    let groups =
        json!({"type":"string","enum":["all","white_balance","tone","curve"],"default":"all"});
    vec![
        def(
            "describe_raw",
            "Inspect editable RAW metadata, decoder, sensor, compression, source, current settings and curve presets. No RAW source bytes are decoded.",
            json!({}),
            &[],
        ),
        def(
            "develop_raw",
            "Patch high-precision RAW development; omitted settings are preserved. One undo step updates pixels and recipe together. Temperature/tint are relative offsets, not Kelvin. tone_curve is five monotonic output values at inputs 0,.25,.5,.75,1; wb_override null restores camera gains.",
            json!({"settings":{"type":"object","additionalProperties":false,"properties":{
            "lens_profile":{"type":["object","null"],"additionalProperties":false,"required":["distortion","vignette","tca","scale"],"properties":{"distortion":{"type":"array","minItems":3,"maxItems":3,"items":range(-100.,100.)},"vignette":{"type":"array","minItems":3,"maxItems":3,"items":range(-100.,100.)},"tca":{"type":"array","minItems":6,"maxItems":6,"items":range(-100.,100.)},"scale":range(0.01,100.)}},"crop":{"type":"array","minItems":4,"maxItems":4,"items":range(0.,1.)},"straighten":range(-45.,45.),"rotation":{"type":"integer","minimum":0,"maximum":3,"description":"Clockwise quarter turns after crop. Swaps portrait/landscape dimensions; original unchanged."},
            "perspective":{"type":"array","minItems":2,"maxItems":2,"items":range(-0.8,0.8)},"distortion":range(-0.5,0.5),
            "aberration":{"type":"array","minItems":2,"maxItems":2,"items":range(-0.05,0.05)},
            "kelvin":{"type":["number","null"],"minimum":2000,"maximum":50000},
            "hsl":{"type":"array","minItems":8,"maxItems":8,"items":{"type":"array","minItems":3,"maxItems":3,"items":range(-1.,1.)}},
            "grading":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number"}},"description":"Shadows, midtones, highlights; each [hue 0..360, saturation 0..1, luminance -1..1]"},
            "masks":{"type":"array","minItems":8,"maxItems":8,"items":{"type":"object","additionalProperties":false,"properties":{"bitmap":{"type":["array","null"],"minItems":32,"maxItems":32,"items":{"type":"integer","minimum":0,"maximum":255}},"enabled":{"type":"boolean"},"linear":{"type":"boolean"},"inverted":{"type":"boolean"},"center":{"type":"array","minItems":2,"maxItems":2,"items":range(0.,1.)},"radius":{"type":"array","minItems":2,"maxItems":2,"items":range(0.001,2.)},"feather":range(0.001,1.),"exposure":range(-5.,5.),"saturation":range(-1.,1.),"temperature":range(-1.,1.)}}},
            "exposure":range(-5.,5.),"temperature":range(-1.,1.),"tint":range(-1.,1.),"highlights":range(-1.,1.),"shadows":range(-1.,1.),"black_point":range(0.,0.25),"brightness":range(-1.,1.),"contrast":range(-1.,1.),"saturation":range(-1.,1.),
            "whites":range(-1.,1.),"blacks":range(-1.,1.),"vibrance":range(-1.,1.),"texture":range(-1.,1.),"clarity":range(-1.,1.),"dehaze":range(-1.,1.),"vignette":range(-1.,1.),"sharpening":range(0.,1.),"noise_reduction":range(0.,1.),
            "camera_profile":{"type":["array","null"],"minItems":32,"maxItems":32,"items":{"type":"integer","minimum":0,"maximum":255}},
            "profile_as_shot":{"type":"boolean"},"highlight_reconstruction":range(0.,1.),"depth_blur":range(0.,0.05),"depth_focus":range(0.,1.),"depth_range":range(0.,1.),"depth_map":{"type":["array","null"],"minItems":32,"maxItems":32,"items":{"type":"integer","minimum":0,"maximum":255}},
            "local_edits":{"type":["array","null"],"minItems":32,"maxItems":32,"items":{"type":"integer","minimum":0,"maximum":255}},
            "process_version":{"type":"integer","enum":[1,2]},"wide_gamut":{"type":"boolean"},
            "parametric":{"type":"array","minItems":4,"maxItems":4,"items":range(-1.,1.)},
            "parametric_splits":{"type":"array","minItems":3,"maxItems":3,"items":range(0.01,0.99)},
            "calibration":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"array","minItems":2,"maxItems":2,"items":range(-1.,1.)}},
            "shadow_tint":range(-1.,1.),
            "global_grading":{"type":"array","minItems":3,"maxItems":3,"prefixItems":[range(0.,360.),range(0.,1.),range(-1.,1.)]},
            "grading_balance":range(-1.,1.),"grading_blending":range(0.,1.),
            "sharpening_radius":range(0.5,3.),"sharpening_detail":range(0.,1.),"sharpening_masking":range(0.,1.),
            "luminance_detail":range(0.,1.),"luminance_contrast":range(0.,1.),"color_noise_reduction":range(0.,1.),"color_noise_detail":range(0.,1.),"color_noise_smoothness":range(0.,1.),
            "demosaic_version":{"type":"integer","enum":[0,1]},"sensor_ai_denoise":{"type":"boolean"},"sensor_noise_reduction":range(0.,1.),"point_curves":point_curves,"smooth_point_curves":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"boolean"},"description":"Smooth Photo-style interpolation for composite, red, green, and blue point curves."},"smooth_curve":{"type":"boolean"},"tone_curve":{"type":"array","minItems":5,"maxItems":5,"items":range(0.,1.)},"wb_override":{"type":["array","null"],"minItems":4,"maxItems":4,"items":range(0.01,100.)}}},"curve_preset":{"type":"string","enum":["linear","medium","strong"]}}),
            &[],
        ),
        def(
            "auto_develop_raw",
            "Compute deterministic auto tone from the RAW, preserving white balance; apply concrete editable settings in one undo step.",
            json!({}),
            &[],
        ),
        def(
            "pick_raw_white_balance",
            "Sample a neutral patch in oriented/cropped RAW source pixels (not transformed canvas or preview coordinates). Rejects clipped, dark or out-of-bounds samples. Applies camera gains in one undo step.",
            json!({"x":{"type":"integer","minimum":0,"maximum":u32::MAX},"y":{"type":"integer","minimum":0,"maximum":u32::MAX}}),
            &["x", "y"],
        ),
        def(
            "reset_raw",
            "Reset all or one RAW settings group to as-shot development; reversible in one undo step.",
            json!({"group":groups}),
            &[],
        ),
        def(
            "relink_raw",
            "Locate a moved original RAW; requires identical SHA-256 bytes. Preserves settings/pixels and original-file protection, with undo.",
            json!({"path":{"type":"string","minLength":1}}),
            &["path"],
        ),
        def(
            "raw_settings",
            "Save/load Emulsion JSON sidecars or camera-aware presets; save/apply/reset camera-model defaults. Not Adobe XMP. Sidecars default beside original; presets require path. group applies only to loads/apply. Saves/default resets change disk, not undo history; existing matching settings may be replaced but images/unrelated files are protected.",
            json!({"action":{"type":"string","enum":["save_sidecar","load_sidecar","save_preset","load_preset","save_camera_defaults","apply_camera_defaults","reset_camera_defaults"]},"path":{"type":"string","minLength":1},"group":groups}),
            &["action"],
        ),
    ]
}

#[cfg(test)]
#[path = "raw_tools_tests.rs"]
mod tests;
