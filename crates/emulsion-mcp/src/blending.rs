//! Agent access to native layer and effect blending, with atomic validation.
use crate::server::ToolResult;
use emulsion_core::{Command, Editor};
use emulsion_raster::blend::{BlendMode, BlendSpace};
use emulsion_raster::composite::{BlendIfChannel, BlendRange, BlendingOptions, Knockout};
use serde_json::{Value, json};

fn error(message: impl Into<String>) -> ToolResult {
    ToolResult::error(message)
}
fn keys(args: &Value, allowed: &[&str]) -> Result<(), ToolResult> {
    let object = args
        .as_object()
        .ok_or_else(|| error("arguments must be an object"))?;
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(error(format!("unknown blending field '{key}'")));
        }
    }
    Ok(())
}
fn number(value: &Value, key: &str, min: f64, max: f64) -> Result<f32, ToolResult> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v >= min && *v <= max)
        .map(|v| v as f32)
        .ok_or_else(|| error(format!("{key} must be a number from {min} to {max}")))
}
fn boolean(value: &Value, key: &str) -> Result<bool, ToolResult> {
    value
        .as_bool()
        .ok_or_else(|| error(format!("{key} must be boolean")))
}
pub(crate) fn mode(value: &Value, group: bool) -> Result<BlendMode, ToolResult> {
    let value = value
        .as_str()
        .ok_or_else(|| error("blend mode must be a string"))?;
    let key = value.trim().to_lowercase().replace(['_', '-'], " ");
    if key == "pass through" {
        return if group {
            Ok(BlendMode::PassThrough)
        } else {
            Err(error("Pass Through is only valid for groups"))
        };
    }
    BlendMode::MENU
        .iter()
        .flatten()
        .copied()
        .find(|m| m.label() == key)
        .ok_or_else(|| error(format!("unknown blend mode '{value}'")))
}
fn range(value: &Value, current: BlendRange) -> Result<BlendRange, ToolResult> {
    keys(value, &["black", "black_fade", "white_fade", "white"])?;
    let mut next = current;
    for (key, target) in [
        ("black", &mut next.black),
        ("black_fade", &mut next.black_fade),
        ("white_fade", &mut next.white_fade),
        ("white", &mut next.white),
    ] {
        if let Some(value) = value.get(key) {
            *target = number(value, key, 0., 255.)? / 255.;
        }
    }
    if !next.valid() {
        return Err(error(
            "Blend If requires black <= black_fade <= white_fade <= white",
        ));
    }
    Ok(next)
}
fn range_json(range: BlendRange) -> Value {
    json!({"black":range.black*255.,"black_fade":range.black_fade*255.,"white_fade":range.white_fade*255.,"white":range.white*255.})
}
pub(crate) fn describe(options: &BlendingOptions) -> Value {
    json!({
        "fill_opacity":options.fill_opacity*100.,"channels":options.channels,
        "knockout":match options.knockout { Knockout::None=>"none", Knockout::Shallow=>"shallow", Knockout::Deep=>"deep" },
        "blend_interior_effects_as_group":options.blend_interior_effects_as_group,
        "blend_clipped_layers_as_group":options.blend_clipped_layers_as_group,
        "transparency_shapes_layer":options.transparency_shapes_layer,
        "layer_mask_hides_effects":options.layer_mask_hides_effects,
        "blend_if":{"channel":match options.blend_if.channel{BlendIfChannel::Gray=>"gray",BlendIfChannel::Red=>"red",BlendIfChannel::Green=>"green",BlendIfChannel::Blue=>"blue"},"source":range_json(options.blend_if.source),"backdrop":range_json(options.blend_if.backdrop)},
        "units":{"fill_opacity":"percent","blend_if":"0..255"}
    })
}
/// Keep editable settings inspectable without expanding embedded pattern pixels into tool output.
pub(crate) fn describe_style(options: &emulsion_core::style_options::StyleOptions) -> Value {
    let image = options
        .pattern
        .image
        .as_ref()
        .map(|image| json!({"width":image.width,"height":image.height,"embedded":true}));
    let mut summary = options.clone();
    summary.pattern.image = None;
    let mut value = json!(summary);
    value["pattern"]["image"] = image.unwrap_or(Value::Null);
    value
}

fn apply(editor: &mut Editor, command: Command, message: &str) -> Result<ToolResult, ToolResult> {
    editor.execute(command).map_err(|e| error(e.to_string()))?;
    Ok(ToolResult::text(message))
}
pub(crate) fn execute(
    editor: &mut Editor,
    name: &str,
    args: &Value,
) -> Result<ToolResult, ToolResult> {
    if name == "set_blend_space" {
        keys(args, &["space"])?;
        let space = match args.get("space").and_then(Value::as_str) {
            Some("linear") => BlendSpace::Linear,
            Some("srgb") => BlendSpace::Srgb,
            _ => return Err(error("space must be linear or srgb")),
        };
        return apply(
            editor,
            Command::SetBlendSpace { space },
            "Document blend space updated",
        );
    }
    let id = args
        .get("node")
        .and_then(Value::as_u64)
        .ok_or_else(|| error("node must be an integer"))?;
    let node = editor
        .doc
        .node(id)
        .ok_or_else(|| error(format!("no node {id}")))?;
    match name {
        "set_blending_options" => {
            keys(
                args,
                &[
                    "node",
                    "fill_opacity",
                    "channels",
                    "blend_if",
                    "knockout",
                    "blend_interior_effects_as_group",
                    "blend_clipped_layers_as_group",
                    "transparency_shapes_layer",
                    "layer_mask_hides_effects",
                ],
            )?;
            let mut options = node.blending;
            if let Some(value) = args.get("fill_opacity") {
                options.fill_opacity = number(value, "fill_opacity", 0., 100.)? / 100.;
            }
            if let Some(value) = args.get("channels") {
                let values = value
                    .as_array()
                    .filter(|v| v.len() == 3)
                    .ok_or_else(|| error("channels must contain three RGB booleans"))?;
                for (slot, value) in options.channels.iter_mut().zip(values) {
                    *slot = boolean(value, "channels")?;
                }
            }
            if let Some(value) = args.get("blend_if") {
                keys(value, &["channel", "source", "backdrop"])?;
                if let Some(channel) = value.get("channel") {
                    options.blend_if.channel = match channel.as_str() {
                        Some("gray") => BlendIfChannel::Gray,
                        Some("red") => BlendIfChannel::Red,
                        Some("green") => BlendIfChannel::Green,
                        Some("blue") => BlendIfChannel::Blue,
                        _ => {
                            return Err(error("Blend If channel must be gray, red, green or blue"));
                        }
                    };
                }
                if let Some(value) = value.get("source") {
                    options.blend_if.source = range(value, options.blend_if.source)?;
                }
                if let Some(value) = value.get("backdrop") {
                    options.blend_if.backdrop = range(value, options.blend_if.backdrop)?;
                }
            }
            if let Some(value) = args.get("knockout") {
                options.knockout = match value.as_str() {
                    Some("none") => Knockout::None,
                    Some("shallow") => Knockout::Shallow,
                    Some("deep") => Knockout::Deep,
                    _ => return Err(error("knockout must be none, shallow or deep")),
                };
            }
            for (key, target) in [
                (
                    "blend_interior_effects_as_group",
                    &mut options.blend_interior_effects_as_group,
                ),
                (
                    "blend_clipped_layers_as_group",
                    &mut options.blend_clipped_layers_as_group,
                ),
                (
                    "transparency_shapes_layer",
                    &mut options.transparency_shapes_layer,
                ),
                (
                    "layer_mask_hides_effects",
                    &mut options.layer_mask_hides_effects,
                ),
            ] {
                if let Some(value) = args.get(key) {
                    *target = boolean(value, key)?;
                }
            }
            if args.as_object().unwrap().len() == 1 {
                return Err(error("provide an advanced blending field"));
            }
            apply(
                editor,
                Command::SetBlendingOptions { id, options },
                "Layer blending options updated",
            )
        }
        "set_style_blending" => {
            keys(
                args,
                &[
                    "node",
                    "index",
                    "blend",
                    "enabled",
                    "highlight_blend",
                    "shadow_blend",
                ],
            )?;
            let index = args
                .get("index")
                .and_then(Value::as_u64)
                .ok_or_else(|| error("index must be an integer"))? as usize;
            let styles = node.styles.clone();
            if index >= styles.len() {
                return Err(error(format!("no style at index {index}")));
            }
            if (args.get("highlight_blend").is_some() || args.get("shadow_blend").is_some())
                && !matches!(
                    styles[index],
                    emulsion_core::styles::LayerStyle::BevelEmboss { .. }
                )
            {
                return Err(error(
                    "highlight_blend and shadow_blend require a bevel_emboss effect",
                ));
            }
            let mut options: Vec<_> = styles
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    node.style_options
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| emulsion_core::style_options::StyleOptions::for_style(s))
                })
                .collect();
            let option = &mut options[index];
            for (key, target) in [
                ("blend", &mut option.blend),
                ("highlight_blend", &mut option.highlight_blend),
                ("shadow_blend", &mut option.shadow_blend),
            ] {
                if let Some(value) = args.get(key) {
                    *target = mode(value, false)?;
                }
            }
            if let Some(value) = args.get("enabled") {
                option.enabled = boolean(value, "enabled")?;
            }
            if args.as_object().unwrap().len() == 2 {
                return Err(error("provide an effect blend mode or enabled state"));
            }
            apply(
                editor,
                Command::SetLayerEffects {
                    id,
                    styles,
                    options,
                },
                "Effect blending updated",
            )
        }
        "set_effects_enabled" => {
            keys(args, &["node", "enabled"])?;
            let enabled = boolean(
                args.get("enabled")
                    .ok_or_else(|| error("missing enabled"))?,
                "enabled",
            )?;
            apply(
                editor,
                Command::SetEffectsEnabled { id, enabled },
                "Layer effects visibility updated",
            )
        }
        _ => Err(error("unknown blending tool")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Document, Node, command::Slot};
    fn editor() -> Editor {
        let mut editor = Editor::new(Document::new(32, 32), None);
        editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Title",
                    emulsion_core::text::TextSpec {
                        text: "A".into(),
                        size: 16.,
                        ..Default::default()
                    },
                    32,
                    32,
                )),
                slot: Slot::TOP,
            })
            .unwrap();
        editor
    }
    fn call(editor: &mut Editor, name: &str, args: Value) {
        let result = crate::exec::execute(editor, name, &args);
        assert!(!result.is_error, "{result:?}");
    }
    #[test]
    fn advanced_blending_patch_describe_undo_and_redo() {
        let mut editor = editor();
        let id = editor.doc.nodes[0].id;
        let before = editor.doc.clone();
        let history = editor.history.len();
        call(
            &mut editor,
            "set_blending_options",
            json!({"node":id,"fill_opacity":37.5,"channels":[true,false,true],"knockout":"deep","blend_interior_effects_as_group":false,"blend_clipped_layers_as_group":false,"transparency_shapes_layer":false,"layer_mask_hides_effects":true,"blend_if":{"channel":"green","source":{"black":10,"black_fade":30,"white_fade":220,"white":245},"backdrop":{"black":20,"black_fade":50,"white_fade":200,"white":255}}}),
        );
        assert_eq!(editor.history.len(), history + 1);
        let after = editor.doc.clone();
        let described = crate::exec::describe(&editor);
        let b = &described["nodes"][0]["blending"];
        assert_eq!(b["fill_opacity"], 37.5);
        assert_eq!(b["channels"], json!([true, false, true]));
        assert_eq!(b["knockout"], "deep");
        assert_eq!(b["blend_interior_effects_as_group"], false);
        assert_eq!(b["blend_clipped_layers_as_group"], false);
        assert_eq!(b["transparency_shapes_layer"], false);
        assert_eq!(b["layer_mask_hides_effects"], true);
        assert_eq!(b["blend_if"]["channel"], "green");
        assert!((b["blend_if"]["source"]["black"].as_f64().unwrap() - 10.).abs() < 0.001);
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
        assert!(editor.redo());
        assert_eq!(editor.doc, after);
        call(
            &mut editor,
            "set_blending_options",
            json!({"node":id,"fill_opacity":60}),
        );
        assert_eq!(
            editor.doc.nodes[0].blending.blend_if,
            after.nodes[0].blending.blend_if
        );
        assert_eq!(editor.doc.nodes[0].blending.channels, [true, false, true]);
    }
    #[test]
    fn invalid_blending_is_atomic_including_partial_ranges_and_locked_nodes() {
        let mut editor = editor();
        let id = editor.doc.nodes[0].id;
        for args in [
            json!({"node":id,"fill_opacity":101}),
            json!({"node":id,"fill_opacity":25,"channels":[true,false]}),
            json!({"node":id,"channels":[true,1,false]}),
            json!({"node":id,"blend_if":{"source":{"black":200}}}),
            json!({"node":id,"blend_if":{"channel":"cyan"}}),
            json!({"node":id,"blend_if":{"backdrop":{"white":256}}}),
            json!({"node":id,"typo":1}),
            json!({"node":id}),
        ] {
            let before = editor.doc.clone();
            let count = editor.history.len();
            let result = crate::exec::execute(&mut editor, "set_blending_options", &args);
            assert!(result.is_error, "{args}");
            assert_eq!(editor.doc, before);
            assert_eq!(editor.history.len(), count);
        }
        editor
            .execute(Command::SetLocked { id, locked: true })
            .unwrap();
        let before = editor.doc.clone();
        assert!(
            crate::exec::execute(
                &mut editor,
                "set_blending_options",
                &json!({"node":id,"fill_opacity":12})
            )
            .is_error
        );
        assert_eq!(editor.doc, before);
    }
    #[test]
    fn all_layer_modes_roundtrip_and_pass_through_is_group_only() {
        let mut editor = editor();
        let id = editor.doc.nodes[0].id;
        for mode in BlendMode::MENU.iter().flatten() {
            call(
                &mut editor,
                "set_blend_mode",
                json!({"node":id,"mode":mode.label()}),
            );
            assert_eq!(editor.doc.node(id).unwrap().blend, *mode);
            assert_eq!(
                crate::exec::describe(&editor)["nodes"][0]["blend"],
                mode.label()
            );
        }
        let before = editor.doc.clone();
        assert!(
            crate::exec::execute(
                &mut editor,
                "set_blend_mode",
                &json!({"node":id,"mode":"pass through"})
            )
            .is_error
        );
        assert_eq!(editor.doc, before);
        editor
            .execute(Command::AddNode {
                node: Box::new(Node::group(0, "Group")),
                slot: Slot::TOP,
            })
            .unwrap();
        let group = editor.doc.nodes.last().unwrap().id;
        call(
            &mut editor,
            "set_blend_mode",
            json!({"node":group,"mode":"pass-through"}),
        );
        assert_eq!(
            editor.doc.node(group).unwrap().blend,
            BlendMode::PassThrough
        );
    }
    #[test]
    fn effect_blending_preserves_identity_settings_and_is_undoable() {
        let mut editor = editor();
        let id = editor.doc.nodes[0].id;
        call(
            &mut editor,
            "add_style",
            json!({"node":id,"kind":"bevel_emboss"}),
        );
        let before = editor.doc.clone();
        let option_id = before.nodes[0].style_options[0].id;
        call(
            &mut editor,
            "set_style_blending",
            json!({"node":id,"index":0,"blend":"overlay","enabled":false,"highlight_blend":"screen","shadow_blend":"multiply"}),
        );
        let node = editor.doc.node(id).unwrap();
        assert_eq!(node.styles, before.nodes[0].styles);
        assert_eq!(node.style_options[0].id, option_id);
        assert!(!node.style_options[0].enabled);
        assert_eq!(node.style_options[0].blend, BlendMode::Overlay);
        let value = crate::exec::describe(&editor);
        assert_eq!(value["nodes"][0]["styles"][0]["options"]["enabled"], false);
        assert_eq!(
            value["nodes"][0]["styles"][0]["options"]["shadow_blend"],
            "multiply"
        );
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
        call(
            &mut editor,
            "set_effects_enabled",
            json!({"node":id,"enabled":false}),
        );
        assert_eq!(
            crate::exec::describe(&editor)["nodes"][0]["effects_enabled"],
            false
        );
        assert_eq!(
            editor.doc.nodes[0].style_options,
            before.nodes[0].style_options
        );
        for args in [
            json!({"node":id,"index":0,"blend":"pass through"}),
            json!({"node":id,"index":0,"enabled":"false"}),
            json!({"node":id,"index":99,"blend":"screen"}),
        ] {
            let before = editor.doc.clone();
            assert!(crate::exec::execute(&mut editor, "set_style_blending", &args).is_error);
            assert_eq!(editor.doc, before);
        }
    }
    #[test]
    fn blend_space_is_described_validated_and_undoable() {
        let mut editor = editor();
        let before = editor.doc.clone();
        call(&mut editor, "set_blend_space", json!({"space":"srgb"}));
        assert_eq!(editor.doc.blend_space, BlendSpace::Srgb);
        assert_eq!(crate::exec::describe(&editor)["blend_space"], "srgb");
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
        assert!(
            crate::exec::execute(&mut editor, "set_blend_space", &json!({"space":"lab"})).is_error
        );
        assert_eq!(editor.doc, before);
    }
    #[test]
    fn describe_effect_settings_omits_embedded_pattern_pixel_payloads() {
        let mut options = emulsion_core::style_options::StyleOptions::default();
        options.pattern.image = Some(std::sync::Arc::new(
            emulsion_core::style_options::PatternImage {
                width: 128,
                height: 128,
                pixels: vec![255; 128 * 128 * 4],
            },
        ));
        let value = describe_style(&options);
        assert_eq!(value["pattern"]["image"]["width"], 128);
        assert_eq!(value["pattern"]["image"]["embedded"], true);
        assert!(value["pattern"]["image"].get("pixels").is_none());
        assert!(value.to_string().len() < 3000);
    }

    #[test]
    fn blending_tools_are_discoverable_with_bounded_units() {
        let definitions = crate::tools::definitions();
        for name in [
            "set_blending_options",
            "set_blend_space",
            "set_style_blending",
            "set_effects_enabled",
        ] {
            let definition = definitions.iter().find(|d| d.name == name).unwrap();
            assert_eq!(definition.input_schema["type"], "object");
            assert!(!crate::tools::READ_ONLY.contains(&name));
        }
        let schema = &definitions
            .iter()
            .find(|d| d.name == "set_blending_options")
            .unwrap()
            .input_schema;
        assert_eq!(schema["properties"]["fill_opacity"]["maximum"], 100);
        assert_eq!(
            schema["properties"]["blend_if"]["properties"]["source"]["properties"]["white"]["maximum"],
            255
        );
    }
}
