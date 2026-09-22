//! Read-only RAW proof images and schemas for live workspace controls.
use crate::{ToolDef, ToolResult};
use base64::Engine as _;
use emulsion_core::{Command, Document, raw::DevelopParams};
use emulsion_io::{
    raw::RawSource,
    raw_settings::{RawSettingsGroup, merge_settings},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Edited,
    #[default]
    Split,
    WithoutTone,
    WithoutCurve,
    Clipping,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default = "middle")]
    pub position: f32,
}
fn middle() -> f32 {
    0.5
}
impl Comparison {
    pub fn parse(args: &Value) -> Result<Self, ToolResult> {
        if !args.is_object() {
            return Err(ToolResult::error("Arguments must be an object"));
        }
        let value: Self =
            serde_json::from_value(args.clone()).map_err(|e| ToolResult::error(e.to_string()))?;
        if !value.position.is_finite() || !(0.0..=1.0).contains(&value.position) {
            return Err(ToolResult::error("position must be between 0 and 1"));
        }
        Ok(value)
    }
}

pub fn definitions() -> Vec<ToolDef> {
    let comparison = json!({"mode":{"type":"string","enum":["edited","split","without_tone","without_curve","clipping"],"default":"split"},"position":{"type":"number","minimum":0,"maximum":1,"default":0.5}});
    let def = |name: &str, description: &str, properties: Value, required: Value| ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    };
    let mut preview = comparison.clone();
    preview["max_size"] = json!({"type":"integer","minimum":64,"maximum":1568,"default":1024});
    vec![
        def(
            "get_raw_preview",
            "Return a bounded PNG proof of RAW edits, as-shot/edited split, section bypass or output clipping. Read-only; never changes the canvas, history or export. Split is as-shot left/current right. Output clipping is not sensor recoverability.",
            preview,
            json!([]),
        ),
        def(
            "set_raw_comparison",
            "Control the live RAW canvas comparison. split shows the draggable as-shot/edited divider at position 0..1; edited closes comparison. without_tone, without_curve and clipping show diagnostic views. No saved edits or history changes. Requires the running app.",
            comparison,
            json!([]),
        ),
        def(
            "list_raw_documents",
            "List RAW photos open in this workspace, their stable session document IDs, camera identities and pending-edit states. Use IDs as explicit synchronize_raw targets. Requires the running app.",
            json!({}),
            json!([]),
        ),
        def(
            "synchronize_raw",
            "Copy current RAW settings to explicitly selected open document IDs. Each target is independently undoable; originals are untouched. Sampled white balance requires matching camera models. Returns per-target results; changes are not an all-or-nothing batch. Requires the running app.",
            json!({"targets":{"type":"array","minItems":1,"maxItems":64,"uniqueItems":true,"items":{"type":"integer","minimum":1}},"group":{"type":"string","enum":["all","white_balance","tone","curve"],"default":"all"}}),
            json!(["targets"]),
        ),
    ]
}

pub fn preview(doc: &Document, args: &Value) -> Result<ToolResult, ToolResult> {
    let mut input = args.clone();
    let max = input
        .as_object_mut()
        .and_then(|v| v.remove("max_size"))
        .unwrap_or(json!(1024));
    let max = max
        .as_u64()
        .filter(|v| (64..=1568).contains(v))
        .ok_or_else(|| ToolResult::error("max_size must be 64..1568"))?;
    let settings = Comparison::parse(&input)?;
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| ToolResult::error("This document has no editable RAW source"))?;
    let view_args = json!({"max_size":max});
    if settings.mode == Mode::Edited {
        return crate::preview::view(doc, &view_args);
    }
    let params = match settings.mode {
        Mode::Split => DevelopParams::default(),
        Mode::WithoutTone => {
            merge_settings(raw.params, DevelopParams::default(), RawSettingsGroup::Tone)
        }
        Mode::WithoutCurve => merge_settings(
            raw.params,
            DevelopParams::default(),
            RawSettingsGroup::Curve,
        ),
        _ => raw.params,
    };
    let source = RawSource::load_verified(&raw.source, &raw.source_sha256)
        .map_err(|e| ToolResult::error(e.to_string()))?;
    let mut raster = source
        .develop_with(&params)
        .map_err(|e| ToolResult::error(e.to_string()))?;
    drop(source);
    if settings.mode == Mode::Clipping {
        let mut pixels = raster.to_pixels();
        for p in &mut pixels {
            *p = if p[..3].contains(&u16::MAX) {
                [65535, 0, 0, 65535]
            } else if p[..3].iter().all(|v| *v == 0) {
                [0, 0, 65535, 65535]
            } else {
                let y = ((p[0] as u32 + p[1] as u32 + p[2] as u32) / 3) as u16;
                [y, y, y, p[3]]
            };
        }
        raster =
            emulsion_raster::Raster::from_pixels(raster.width(), raster.height(), [0; 4], &pixels);
    }
    let mut before = doc.clone();
    Command::DevelopRaw {
        id: raw.node_id,
        raster: Arc::new(raster),
        params,
    }
    .apply(&mut before)
    .map_err(|e| ToolResult::error(e.to_string()))?;
    let mut result = crate::preview::view(&before, &view_args)?;
    if settings.mode == Mode::Split {
        let after = crate::preview::view(doc, &view_args)?;
        let decode = |r: &ToolResult| -> Result<image::RgbaImage, ToolResult> {
            let text = r
                .content
                .iter()
                .find(|b| b["type"] == "image")
                .and_then(|b| b["data"].as_str())
                .ok_or_else(|| ToolResult::error("Missing RAW preview pixels"))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(text)
                .map_err(|e| ToolResult::error(e.to_string()))?;
            Ok(image::load_from_memory(&bytes)
                .map_err(|e| ToolResult::error(e.to_string()))?
                .into_rgba8())
        };
        let mut left = decode(&result)?;
        let right = decode(&after)?;
        let boundary = (left.width() as f32 * settings.position).round() as u32;
        for y in 0..left.height() {
            for x in boundary..left.width() {
                left.put_pixel(x, y, *right.get_pixel(x, y));
            }
        }
        let block = crate::preview::png_block(&left)?;
        if let Some(image) = result.content.iter_mut().find(|b| b["type"] == "image") {
            *image = block;
        }
    }
    result.content.insert(0,json!({"type":"text","text":format!("RAW {:?} proof; split position {}. Saved edits unchanged.",settings.mode,settings.position)}));
    Ok(result)
}

#[cfg(test)]
#[path = "raw_preview_tests.rs"]
mod tests;
