//! Save an edit the person likes as a named preset, export it as an XMP
//! develop preset, and reuse saved presets on other photos.
use crate::{
    exec::Planned,
    server::{ToolDef, ToolResult},
};
use emulsion_core::{Document, raw::DevelopParams};
use emulsion_io::{develop_presets as presets, lightroom_presets};
use serde_json::{Value, json};
use std::path::PathBuf;

fn error(message: impl ToString) -> ToolResult {
    ToolResult::error(message.to_string())
}

fn strict(args: &Value, allowed: &[&str]) -> Result<(), ToolResult> {
    let object = args
        .as_object()
        .ok_or_else(|| error("Arguments must be an object"))?;
    match object.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(key) => Err(error(format!("Unknown argument '{key}'"))),
        None => Ok(()),
    }
}

fn flag(args: &Value, key: &str, default: bool) -> Result<bool, ToolResult> {
    match args.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| error(format!("{key} must be a boolean"))),
    }
}

fn params(doc: &Document) -> Result<DevelopParams, ToolResult> {
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| error("No editable RAW source; open a supported camera RAW file first"))?;
    raw.validate().map_err(error)?;
    Ok(raw.params)
}

pub fn save(doc: &Document, args: &Value) -> Result<ToolResult, ToolResult> {
    strict(
        args,
        &[
            "name",
            "include_exposure",
            "include_white_balance",
            "library",
            "export_xmp",
            "overwrite",
        ],
    )?;
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| error("'name' must be a nonempty string"))?;
    let scope = presets::Scope {
        exposure: flag(args, "include_exposure", false)?,
        white_balance: flag(args, "include_white_balance", false)?,
    };
    let library = flag(args, "library", true)?;
    let overwrite = flag(args, "overwrite", false)?;
    let xmp = match args.get("export_xmp") {
        None | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(presets::default_export_path(name).map_err(error)?),
        Some(Value::String(path)) if !path.trim().is_empty() => Some(PathBuf::from(path)),
        Some(_) => return Err(error("export_xmp must be true, false, or an .xmp path")),
    };
    if !library && xmp.is_none() {
        return Err(error("Nothing to save: set library or export_xmp"));
    }
    let current = params(doc)?;
    let preset = presets::portable(&current, scope);
    if preset == presets::portable(&DevelopParams::default(), scope) {
        return Err(error(
            "The photo has no look to save yet; edit it first (e.g. apply_raw_look or develop_raw)",
        ));
    }
    // Validate the export target before writing anything.
    if let Some(path) = &xmp {
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xmp"))
        {
            return Err(error("export_xmp path must end in .xmp"));
        }
        if path.exists() && !overwrite {
            return Err(error(format!(
                "{} already exists; choose another path or set overwrite",
                path.display()
            )));
        }
    }
    let saved = library
        .then(|| presets::save_to_library(preset, name, overwrite))
        .transpose()
        .map_err(error)?;
    if let Some(path) = &xmp {
        presets::export_xmp(&preset, name, path, overwrite).map_err(error)?;
    }
    Ok(ToolResult::text(
        json!({
            "name": name.trim(),
            "library_preset": saved,
            "xmp": xmp,
            "kept_with_photo": presets::excluded(&current, scope),
            "included": {"exposure": scope.exposure, "white_balance": scope.white_balance},
            "where": "Library presets appear in Emulsion's Develop preset bank and in list_raw_presets; apply them with apply_raw_preset. The .xmp imports into raw editors that read XMP (crs) develop presets, and back into Emulsion.",
            "document_unchanged": true,
        })
        .to_string(),
    ))
}

pub fn list(args: &Value) -> Result<ToolResult, ToolResult> {
    strict(args, &[])?;
    let presets: Vec<_> = presets::library()
        .into_iter()
        .map(|(name, path)| {
            let format = path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            json!({"name": name, "format": format, "path": path})
        })
        .collect();
    Ok(ToolResult::text(
        json!({"presets": presets, "library": lightroom_presets::library_dir(),
            "film_library": format!("{} built-in film and creative presets; search with list_raw_looks query and apply by name", emulsion_io::film_library::all().len())})
        .to_string(),
    ))
}

pub fn plan(doc: &Document, args: &Value) -> Result<Planned, ToolResult> {
    strict(args, &["name", "path", "strength"])?;
    let path = match (args.get("name"), args.get("path")) {
        (Some(name), None) => {
            let name = name
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| error("'name' must be a nonempty string"))?;
            match (presets::find(name), emulsion_io::film_library::find(name)) {
                (Some(path), _) => path,
                // The bundled film library is the fallback for names the bank lacks.
                (None, Some(film)) => {
                    return apply_loaded(
                        doc,
                        film.load(params(doc)?).map_err(error)?,
                        json!(format!("film library: {}", film.category)),
                        args,
                    );
                }
                (None, None) => {
                    return Err(error(format!(
                        "No saved or film library preset named \"{name}\"; see list_raw_presets or list_raw_looks query"
                    )));
                }
            }
        }
        (None, Some(path)) => PathBuf::from(
            path.as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| error("'path' must be a nonempty string"))?,
        ),
        _ => return Err(error("Give exactly one of name or path")),
    };
    let current = params(doc)?;
    let loaded = lightroom_presets::load(&path, current).map_err(error)?;
    apply_loaded(doc, loaded, json!(path), args)
}

fn apply_loaded(
    doc: &Document,
    loaded: lightroom_presets::ImportedPreset,
    path: Value,
    args: &Value,
) -> Result<Planned, ToolResult> {
    let strength = match args.get("strength") {
        None => 1.,
        Some(v) => {
            v.as_f64()
                .filter(|v| (0.0..=1.5).contains(v))
                .ok_or_else(|| error("strength must be a number from 0 to 1.5"))? as f32
        }
    };
    let current = params(doc)?;
    let mut next = presets::apply(&current, &loaded.params);
    if strength != 1. {
        next = crate::raw_looks::blend(&current, &next, strength);
    }
    next.validate().map_err(error)?;
    let mut planned = crate::raw_tools::develop(doc, next, None)?;
    planned.message = json!({
        "preset": loaded.name,
        "path": path,
        "strength": strength,
        "applied": loaded.applied.len(),
        "warnings": loaded.warnings,
        "kept_from_photo": "crop, geometry, lens, masks, depth, camera profile; exposure and white balance unless the preset carries them",
        "settings": next,
        "undo_steps": if planned.commands.is_empty() { 0 } else { 1 },
    })
    .to_string();
    Ok(planned)
}

pub fn definitions() -> Vec<ToolDef> {
    let def = |name: &str, description: &str, properties: Value, required: Value| ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
    };
    vec![
        def(
            "save_raw_preset",
            "Save the current RAW edit as a named, reusable preset when the person likes it. Saves the look (tone, presence, curves, HSL, colour grading, calibration, detail, vignette) to Emulsion's preset bank, shown in the Develop panel; optionally exports an XMP (crs) .xmp develop preset for other raw editors. Crop, geometry, lens, masks, depth and sampled white balance stay with the photo; exposure and white balance are included only on request. Never overwrites an existing preset or file unless overwrite is true. The document is unchanged.",
            json!({
                "name":{"type":"string","minLength":1,"maxLength":120},
                "include_exposure":{"type":"boolean","default":false},
                "include_white_balance":{"type":"boolean","default":false},
                "library":{"type":"boolean","default":true,"description":"Save to Emulsion's preset bank"},
                "export_xmp":{"type":["boolean","string"],"default":false,"description":"true writes <name>.xmp to Emulsion's exported-presets folder; a string is an explicit .xmp path"},
                "overwrite":{"type":"boolean","default":false}
            }),
            json!(["name"]),
        ),
        def(
            "list_raw_presets",
            "List presets in Emulsion's develop preset bank (saved looks plus installed XMP / .lrtemplate develop presets) by name. Read-only.",
            json!({}),
            json!([]),
        ),
        def(
            "apply_raw_preset",
            "Apply a saved preset to the open RAW by name (from list_raw_presets) or by .xmp/.lrtemplate/.json path, in one undo step. Keeps the photo's crop, geometry, lens, masks and depth; keeps its exposure and white balance unless the preset carries them. strength 0–1.5 scales the look.",
            json!({
                "name":{"type":"string","minLength":1},
                "path":{"type":"string","minLength":1},
                "strength":{"type":"number","minimum":0,"maximum":1.5,"default":1}
            }),
            json!([]),
        ),
    ]
}

#[cfg(test)]
#[path = "raw_presets_tests.rs"]
mod tests;
