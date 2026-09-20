//! Brush discovery uses the real presets and stroke renderer.
use crate::{preview::png_block, server::ToolResult};
use emulsion_raster::{
    Raster, color, library,
    paint::{Ink, Stroke},
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) fn list(args: &Value) -> Result<ToolResult, ToolResult> {
    let query = match args.get("query") {
        None => "",
        Some(value) => value
            .as_str()
            .ok_or_else(|| ToolResult::error("query must be a string"))?,
    }
    .to_lowercase();
    let presets: Vec<_> = library::library()
        .into_iter()
        .chain(crate::exec::saved_brushes())
        .filter(|b| {
            b.name.to_lowercase().contains(&query) || b.category.to_lowercase().contains(&query)
        })
        .collect();
    if presets.is_empty() {
        return Err(ToolResult::error(
            "no matching brushes; omit query to list all",
        ));
    }
    let swatches = match args.get("swatches") {
        None => true,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| ToolResult::error("swatches must be a boolean"))?,
    };
    let offset = match args.get("offset") {
        None => 0,
        Some(value) => value
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| ToolResult::error("offset must be a nonnegative integer"))?,
    };
    let page: Vec<_> = presets.iter().skip(offset).take(12).collect();
    let next_offset =
        (offset.saturating_add(page.len()) < presets.len()).then(|| offset + page.len());
    // Keep names discoverable without repeating the complete settings for
    // every preset on every page. Detailed settings match the swatch rows.
    let catalog: Vec<_> = presets
        .iter()
        .map(|b| json!({"name": b.name, "category": b.category, "for": b.note}))
        .collect();
    let brushes: Vec<_> = page
        .iter()
        .map(|b| {
            json!({"name": b.name, "category": b.category,
        "for": b.note, "size": b.brush.size, "settings": b.brush})
        })
        .collect();
    let mut result = ToolResult::text(json!({
        "catalog": catalog, "brushes": brushes,
        "total": presets.len(), "offset": offset, "next_offset": next_offset,
        "settings": {
            "description": "Per-preset settings on this page are complete supported fields and current values. Use query to narrow results or next_offset for another page. Override with paint.settings; numeric values are clamped by the brush engine. Scripted paint has no tablet tilt or timestamps; tilt/speed dynamics are inactive and stabilizer is disabled. Use explicit pressure and tapers for planned marks.",
            "ranges": {"size": [1,1000], "hardness": [0,1], "opacity": [0.01,1], "flow": [0.01,1], "spacing": [0.02,2], "roundness": [0.05,1], "grain_scale": [1,64], "taper_start": [0,2000], "taper_end": [0,2000]},
            "unit_interval": ["grain_strength", "size_pressure", "flow_pressure", "speed_thins", "stabilizer", "size_jitter", "scatter", "color_jitter", "wetness", "edge_darken", "relief"],
            "angle": "degrees, wraps to 0..360", "follow_path": "boolean",
            "grain": ["None", "Paper", "Canvas", "Chalk", "Speckle", "Bristle", "Halftone", "Hatch", "CrossHatch"],
            "blend": ["Normal", "Multiply", "Behind"]
        },
        "material_limits": "These are procedural brush presets: grain, pigment pickup, edge darkening and relief approximate material character. Names do not establish faithful water, drying, diffusion or physical oil simulation. paint.sample_merged opts into lower-layer pickup; default samples the current layer only.",
        "swatch_layout": {"enabled": swatches, "rows": page.iter().enumerate().map(|(i,b)| json!({"name": b.name, "rect": [0,i*72,256,72]})).collect::<Vec<_>>(),
            "next_offset": next_offset,
            "conditions": "Top-to-bottom rows; native preset settings except size capped at 40 px to fit. Pressure rises 0.2 to 1 then falls to 0.2. Opaque white base with a coloured stripe for wet pickup, eraser and smudge. No lower-layer backdrop."}
    }).to_string());
    if swatches && !page.is_empty() {
        let mut sheet = image::RgbaImage::new(256, page.len() as u32 * 72);
        for (row, preset) in page.iter().enumerate() {
            let base = Raster::from_srgba8(
                256,
                72,
                image::RgbaImage::from_fn(256, 72, |x, _| {
                    image::Rgba(if (108..140).contains(&x) {
                        [196, 105, 65, 255]
                    } else {
                        [255, 255, 255, 255]
                    })
                })
                .as_raw(),
            );
            let mut brush = preset.brush;
            brush.size = brush.size.min(40.0);
            let ink = match preset.category.as_str() {
                "Eraser" => Ink::Erase,
                "Smudge" => Ink::Smudge,
                _ => Ink::Color(color::srgba8_to_premul([35, 65, 105, 255])),
            };
            let mut stroke = Stroke::new(Arc::new(base.clone()), brush, ink, None);
            for i in 0..=48 {
                let t = i as f32 / 48.0;
                stroke.point_at(
                    20.0 + t * 216.0,
                    36.0 + (t * std::f32::consts::TAU).sin() * 10.0,
                    Some(0.2 + 0.8 * (1.0 - (2.0 * t - 1.0).abs())),
                    None,
                );
            }
            stroke.finish();
            let (painted, _) = stroke.render(&base);
            let img = image::RgbaImage::from_raw(256, 72, painted.to_srgba8())
                .expect("raster dimensions");
            image::imageops::replace(&mut sheet, &img, 0, row as i64 * 72);
        }
        result.content.push(png_block(&sheet)?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    #[test]
    fn discovery_shows_real_settings_and_decodable_swatch() {
        let r = list(&json!({"query": "Wash"})).unwrap();
        let meta: Value = serde_json::from_str(r.content[0]["text"].as_str().unwrap()).unwrap();
        let b = &meta["brushes"][0];
        assert_eq!(
            b["settings"],
            serde_json::to_value(library::find(b["name"].as_str().unwrap()).unwrap().brush)
                .unwrap()
        );
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(r.content[1]["data"].as_str().unwrap())
            .unwrap();
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!(img.width(), 256);
        assert_eq!(
            img.height() as usize,
            meta["swatch_layout"]["rows"].as_array().unwrap().len() * 72
        );
        assert_eq!(list(&json!({"swatches": false})).unwrap().content.len(), 1);
        assert!(list(&json!({"query": "nonexistent preset"})).is_err());
    }

    #[test]
    fn settings_pages_match_swatches_and_preserve_a_compact_catalog() {
        let metadata = |args| {
            let result = list(&args).unwrap();
            serde_json::from_str::<Value>(result.content[0]["text"].as_str().unwrap()).unwrap()
        };
        let first = metadata(json!({"swatches": false}));
        assert!(first["total"].as_u64().unwrap() > 12);
        assert_eq!(first["brushes"].as_array().unwrap().len(), 12);
        assert_eq!(
            first["catalog"].as_array().unwrap().len() as u64,
            first["total"]
        );
        assert!(
            first["catalog"]
                .as_array()
                .unwrap()
                .iter()
                .all(|b| b.get("settings").is_none())
        );
        let second = metadata(json!({"swatches": false, "offset": first["next_offset"]}));
        assert_eq!(second["offset"], 12);
        assert_eq!(first["catalog"], second["catalog"]);
        for (index, brush) in second["brushes"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                brush["name"],
                second["swatch_layout"]["rows"][index]["name"]
            );
            assert_eq!(brush["name"], second["catalog"][12 + index]["name"]);
            assert!(brush["settings"].is_object());
        }
        let end = metadata(json!({"swatches": false, "offset": first["total"]}));
        assert!(end["brushes"].as_array().unwrap().is_empty());
        assert!(end["next_offset"].is_null());
        assert!(end["swatch_layout"]["next_offset"].is_null());
        let name = second["brushes"][0]["name"].as_str().unwrap();
        let exact = metadata(json!({"query": name, "swatches": false}));
        assert!(
            exact["brushes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b["name"] == name)
        );
    }

    #[test]
    fn malformed_discovery_arguments_do_not_silently_choose_defaults() {
        for args in [
            json!({"query": 12}),
            json!({"query": null}),
            json!({"offset": -1}),
            json!({"offset": 1.5}),
            json!({"offset": "12"}),
            json!({"offset": null}),
            json!({"swatches": "false"}),
        ] {
            assert!(list(&args).is_err(), "{args}");
        }
    }
}
