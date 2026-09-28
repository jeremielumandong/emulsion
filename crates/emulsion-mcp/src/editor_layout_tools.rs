//! Strict schemas for native workspace controls and bounded canvas gestures.
use crate::ToolDef;
use serde::Deserialize;
use serde_json::{Value, json};
pub const READ_ONLY: &[&str] = &["get_editor_controls", "get_playback_setup"];
pub const MUTATING: &[&str] = &[
    "set_editor_layout",
    "manage_editor_workspace",
    "canvas_gesture",
    "open_playback_setup",
    "install_playback_runtime",
];
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gesture {
    pub points: Vec<[f64; 2]>,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub control: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub platform: bool,
    #[serde(default)]
    pub middle_button: bool,
    #[serde(default = "one")]
    pub click_count: u8,
    pub expected_revision: Option<u64>,
    pub expected_page: Option<u64>,
}
fn one() -> u8 {
    1
}
pub fn layout_schema() -> Value {
    let toolbar = json!({"type":"object","additionalProperties":false,"required":["id"],"properties":{"id":{"type":"string","enum":["tools","options","view","color","brushes","dock"]},"edge":{"type":"string","enum":["left","right","top","bottom","floating"]},"visible":{"type":"boolean"},"x":{"type":"number","minimum":0,"maximum":10000},"y":{"type":"number","minimum":0,"maximum":10000},"scale":{"type":"number","minimum":0.75,"maximum":2}}});
    let strings = json!({"type":"array","maxItems":64,"uniqueItems":true,"items":{"type":"string","maxLength":80}});
    json!({"type":"object","additionalProperties":false,"properties":{
        "toolbar_placements":{"type":"array","maxItems":6,"items":toolbar},"tool_ids":strings,"hidden_menu_ids":strings,
        "sidebar_collapsed":{"type":"boolean"},"sidebar_width":{"type":"number","minimum":220,"maximum":560},
        "sidebar_tab":{"type":"string","enum":["properties","adjustments","reference","navigator","info","recipes","timeline","history","histogram","brush-settings","brush-presets","blending-options","assistant","character"]},
        "dock_tab":{"type":"string","enum":["layers","channels","paths"]},
        "sidebar_upper_collapsed":{"type":"boolean"},"sidebar_layers_collapsed":{"type":"boolean"},"sidebar_colors_collapsed":{"type":"boolean"},"sidebar_color_tab":{"type":"boolean"},"sidebar_colors_height":{"type":"number","minimum":48,"maximum":240},"toolbars_overlay":{"type":"boolean"},"tool_columns":{"type":"integer","minimum":1,"maximum":2}
    }})
}
pub fn definitions() -> Vec<ToolDef> {
    let empty = json!({"type":"object","properties":{},"required":[],"additionalProperties":false});
    vec![
        ("get_editor_controls","Inspect available panel/toolbar/menu/tool IDs, current workspace layout and canvas coordinates. No document mutation.",empty.clone()),
        ("set_editor_layout","Patch the originating editor's workspace panels, toolbar docking/position/scale, tool order and menu visibility. Omitted values remain unchanged; toolbar entries patch by ID. Presentation state only; document history and pixels do not change.",json!({"type":"object","properties":{"layout":layout_schema()},"required":["layout"],"additionalProperties":false})),
        ("manage_editor_workspace","Save/apply/remove a named native workspace preset, save/load the default, or reset the current layout. Settings writes report actual completion. Names are required for save/apply/remove; maximum 32 presets.",json!({"type":"object","additionalProperties":false,"required":["operation"],"properties":{"operation":{"type":"string","enum":["save","apply","remove","save_default","load_default","reset"]},"name":{"type":"string","minLength":1,"maxLength":80}}})),
        ("canvas_gesture","Perform one bounded native canvas click/drag using document coordinates and the current tool. Uses native hit testing, connector ports/handles, shape/pen/brush/selection behavior and Undo. The originating editor must be the visible active tab; all points must be inside its canvas. Rejects active user gestures, stale revisions and presentation previews. Set the tool first through set_editor_state. Receipts include pending edit/stroke/RAW flags; asynchronous native jobs must finish before the next gesture.",json!({"type":"object","additionalProperties":false,"required":["points"],"properties":{"points":{"type":"array","minItems":1,"maxItems":2048,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":-1000000,"maximum":1000000}}},"shift":{"type":"boolean"},"control":{"type":"boolean"},"alt":{"type":"boolean"},"platform":{"type":"boolean"},"middle_button":{"type":"boolean"},"click_count":{"type":"integer","minimum":1,"maximum":3},"expected_revision":{"type":"integer","minimum":0},"expected_page":{"type":"integer","minimum":0}}})),
        ("get_playback_setup","Inspect the platform playback runtime setup plan. Does not install packages or launch dialogs.",empty.clone()),
        ("open_playback_setup","Open the originating window's native video runtime/codec setup dialog.",empty.clone()),
        ("install_playback_runtime","Run the fixed platform playback package installer where available. May request operating-system administrator authentication. No arbitrary command/package arguments; Flatpak uses its shared runtime and rejects host installation. Returns actual installer completion or error.",empty),
    ].into_iter().map(|(name,description,input_schema)|ToolDef{name:name.into(),description:description.into(),input_schema}).collect()
}
/// This module's deliberately small schema vocabulary is also enforced at execution.
pub fn validate(schema: &Value, value: &Value) -> Result<(), String> {
    if let Some(choices) = schema["enum"].as_array()
        && !choices.contains(value)
    {
        return Err("Unknown control value".into());
    }
    match schema["type"].as_str() {
        Some("object") => {
            let map = value.as_object().ok_or("Expected an object")?;
            let properties = schema["properties"]
                .as_object()
                .ok_or("Invalid control schema")?;
            if map.keys().any(|key| !properties.contains_key(key)) {
                return Err("Unknown control argument".into());
            }
            if let Some(required) = schema["required"].as_array()
                && required
                    .iter()
                    .any(|key| !map.contains_key(key.as_str().unwrap()))
            {
                return Err("Missing required control argument".into());
            }
            for (key, val) in map {
                validate(&properties[key], val)?;
            }
        }
        Some("array") => {
            let array = value.as_array().ok_or("Expected an array")?;
            if array.len() < schema["minItems"].as_u64().unwrap_or(0) as usize
                || array.len() > schema["maxItems"].as_u64().unwrap_or(2048) as usize
            {
                return Err("Control array exceeds its bounds".into());
            }
            for (i, v) in array.iter().enumerate() {
                if schema["uniqueItems"] == true && array[..i].contains(v) {
                    return Err("Duplicate control entries".into());
                }
                validate(&schema["items"], v)?;
            }
        }
        Some("number" | "integer") => {
            let number = value.as_f64().ok_or("Expected a finite number")?;
            if !number.is_finite()
                || number < schema["minimum"].as_f64().unwrap_or(-f64::MAX)
                || number > schema["maximum"].as_f64().unwrap_or(f64::MAX)
                || (schema["type"] == "integer" && value.as_u64().is_none())
            {
                return Err("Control number is outside its bounds".into());
            }
        }
        Some("boolean") if !value.is_boolean() => return Err("Expected a boolean".into()),
        Some("string") => {
            let s = value.as_str().ok_or("Expected a string")?;
            if s.chars().count() > schema["maxLength"].as_u64().unwrap_or(256) as usize
                || s.chars().count() < schema["minLength"].as_u64().unwrap_or(0) as usize
            {
                return Err("Control string too long".into());
            }
        }
        _ => {}
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_control_registration_and_workspace_arguments_are_consistent() {
        for name in READ_ONLY {
            assert!(crate::editor_host_tools::is_tool(name));
            assert!(crate::tools::read_only_names().any(|n| n == *name));
        }
        for name in MUTATING {
            assert!(crate::editor_host_tools::is_tool(name));
            assert!(crate::tools::uses_native_history(name));
        }
        for args in [
            json!({"operation":"save"}),
            json!({"operation":"save","name":"   "}),
            json!({"operation":"reset","name":"oops"}),
            json!({"operation":"shell"}),
        ] {
            assert!(crate::editor_host_tools::parse("manage_editor_workspace", &args).is_err());
        }
        assert!(
            crate::editor_host_tools::parse(
                "manage_editor_workspace",
                &json!({"operation":"save","name":"Painting"})
            )
            .is_ok()
        );
    }
    #[test]
    fn layout_and_gesture_schemas_reject_unknown_null_and_unbounded_input() {
        let defs = definitions();
        let gesture = &defs
            .iter()
            .find(|d| d.name == "canvas_gesture")
            .unwrap()
            .input_schema;
        for invalid in [
            json!({"points":[]}),
            json!({"points":[[0,0]],"click_count":0}),
            json!({"points":[[0,0]],"shift":null}),
            json!({"points":[[0,0]],"shell":"x"}),
        ] {
            assert!(validate(gesture, &invalid).is_err());
        }
        assert!(validate(gesture, &json!({"points":[[5,6],[10,20]],"control":true})).is_ok());
        assert!(validate(&layout_schema(), &json!({"sidebar_width":-1})).is_err());
        assert!(
            validate(
                &layout_schema(),
                &json!({"toolbar_placements":[{"id":"tools","scale":1.5}]})
            )
            .is_ok()
        );
    }
}
