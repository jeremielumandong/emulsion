//! Shape stroke presets shared by MCP and the native Properties panel.
use crate::server::ToolResult;
use emulsion_core::{Command, Editor, NodeKind};
use emulsion_io::settings::{Settings, ShapeStrokePreset};
use emulsion_raster::vector::{PathStyle, StrokeCap};
use serde_json::{Value, json};

pub fn is_tool(name: &str) -> bool {
    matches!(
        name,
        "list_shape_stroke_presets" | "save_shape_stroke_preset" | "apply_shape_stroke_preset"
    )
}

/// Hosts pass their current settings so edits cannot overwrite newer UI preferences.
/// Persist successfully before publishing a changed preset library in memory.
pub fn execute(
    editor: &mut Editor,
    name: &str,
    args: &Value,
    settings: &mut Settings,
) -> ToolResult {
    let mut next = settings.clone();
    match run(editor, name, args, &mut next) {
        Err(e) => e,
        Ok(result) => {
            if name == "save_shape_stroke_preset" {
                if let Err(e) = next.save() {
                    return ToolResult::error(format!("Could not save stroke preset: {e}"));
                }
                *settings = next;
            }
            result
        }
    }
}

fn run(
    editor: &mut Editor,
    name: &str,
    args: &Value,
    settings: &mut Settings,
) -> Result<ToolResult, ToolResult> {
    let error = ToolResult::error;
    if name == "list_shape_stroke_presets" {
        return Ok(ToolResult::text(json!({
            "builtin": ["solid", "dashed", "dotted"],
            "saved": settings.shape_stroke_presets.iter().map(|p| json!({"name":p.name,"style":crate::shape_style::style_json(&p.style)})).collect::<Vec<_>>()
        }).to_string()));
    }
    let id = args
        .get("node")
        .and_then(Value::as_u64)
        .ok_or_else(|| error("node must be an integer"))?;
    let node = editor
        .doc
        .node(id)
        .ok_or_else(|| error("node does not exist"))?;
    let NodeKind::Path { path, style, .. } = &node.kind else {
        return Err(error("node must be an editable vector path"));
    };
    let path = path.clone();
    let mut style = *style;
    let preset_name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty() && v.chars().count() <= 80)
        .ok_or_else(|| error("name must contain 1 to 80 characters"))?;
    match name {
        "save_shape_stroke_preset" => {
            let overwrite = match args.get("overwrite") {
                None => false,
                Some(Value::Bool(v)) => *v,
                _ => return Err(error("overwrite must be boolean")),
            };
            let preset = ShapeStrokePreset {
                name: preset_name.into(),
                style,
            };
            if let Some(existing) = settings
                .shape_stroke_presets
                .iter_mut()
                .find(|p| p.name == preset_name)
            {
                if !overwrite {
                    return Err(error(
                        "preset already exists; use overwrite=true to replace it",
                    ));
                }
                *existing = preset;
            } else {
                if settings.shape_stroke_presets.len() >= 32 {
                    return Err(error("The preset library is full (32 presets)"));
                }
                settings.shape_stroke_presets.push(preset);
            }
            Ok(ToolResult::text(format!(
                "Saved stroke preset {preset_name:?}"
            )))
        }
        "apply_shape_stroke_preset" => {
            let source = match args.get("source") {
                None => "saved",
                Some(Value::String(s)) => s.as_str(),
                _ => return Err(error("source must be builtin or saved")),
            };
            match source {
                "builtin" => match preset_name {
                    "solid" => style.dash_count = 0,
                    "dashed" => {
                        style.dash = [12., 6., 0., 0., 0., 0.];
                        style.dash_count = 2;
                    }
                    "dotted" => {
                        style.cap = StrokeCap::Round;
                        style.dash = [0., (style.width * 2.).max(2.), 0., 0., 0., 0.];
                        style.dash_count = 2;
                    }
                    _ => {
                        return Err(error(
                            "unknown built-in preset; use solid, dashed or dotted",
                        ));
                    }
                },
                "saved" => {
                    let saved = settings
                        .shape_stroke_presets
                        .iter()
                        .find(|p| p.name == preset_name)
                        .ok_or_else(|| {
                            error("saved preset not found; call list_shape_stroke_presets")
                        })?;
                    style = PathStyle {
                        fill: style.fill,
                        fill_paint: style.fill_paint,
                        ..saved.style
                    };
                }
                _ => return Err(error("source must be builtin or saved")),
            }
            editor
                .execute(Command::SetPath {
                    id,
                    path,
                    style: style.sanitized(),
                })
                .map_err(|e| ToolResult::error(e.to_string()))?;
            Ok(ToolResult::text(format!(
                "Applied stroke preset {preset_name:?} to node {id}"
            )))
        }
        _ => Err(error("unknown stroke preset tool")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Document, NodeId};
    use emulsion_raster::vector::{PathPaint, StrokeAlignment};

    fn add_path(editor: &mut Editor, extra: Value) -> NodeId {
        let mut args =
            json!({"d":"M 8 8 L 56 8 L 56 56 L 8 56 Z","fill":"#112233","stroke":"#aabbcc"});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let result = crate::exec::execute(editor, "draw_path", &args);
        assert!(!result.is_error, "{result:?}");
        editor.doc.nodes.last().unwrap().id
    }

    fn style(editor: &Editor, id: NodeId) -> PathStyle {
        let NodeKind::Path { style, .. } = editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        style
    }

    #[test]
    fn saved_strokes_preserve_target_fill_geometry_and_undo() {
        let mut editor = Editor::new(Document::new(64, 64), None);
        let mut settings = Settings::default();
        let source = add_path(
            &mut editor,
            json!({"width":7,"stroke_alignment":"inside","cap":"square","join":"bevel","dashes":[4,2],"dash_offset":3,"stroke_paint":{"kind":"pattern","pattern":"dots","end":"#ffffff00","size":8}}),
        );
        let target = add_path(
            &mut editor,
            json!({"fill":"#ff000080","fill_paint":{"kind":"linear_gradient","end":"#0000ff00","angle":60}}),
        );
        let before = editor.doc.node(target).unwrap().clone();
        let steps = editor.history.len();
        run(
            &mut editor,
            "save_shape_stroke_preset",
            &json!({"node":source,"name":"Dotted paint"}),
            &mut settings,
        )
        .unwrap();
        assert_eq!(
            editor.history.len(),
            steps,
            "saving settings does not edit the document"
        );
        let listed = run(
            &mut editor,
            "list_shape_stroke_presets",
            &json!({}),
            &mut settings,
        )
        .unwrap();
        let list: Value =
            serde_json::from_str(listed.content[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(list["saved"][0]["name"], "Dotted paint");
        assert_eq!(
            list["saved"][0]["style"]["stroke_paint"]["end"],
            "#ffffff00"
        );
        run(
            &mut editor,
            "apply_shape_stroke_preset",
            &json!({"node":target,"name":"Dotted paint"}),
            &mut settings,
        )
        .unwrap();
        let result = style(&editor, target);
        assert_eq!(result.fill, Some([255, 0, 0, 128]));
        assert!(matches!(
            result.fill_paint,
            PathPaint::LinearGradient {
                end: [0, 0, 255, 0],
                angle: 60.0
            }
        ));
        assert_eq!(result.stroke_paint, style(&editor, source).stroke_paint);
        assert_eq!(result.alignment, StrokeAlignment::Inside);
        assert_eq!(result.width, 7.0);
        let NodeKind::Path {
            path: before_path, ..
        } = &before.kind
        else {
            panic!()
        };
        let NodeKind::Path {
            path: after_path, ..
        } = &editor.doc.node(target).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(before_path, after_path);
        assert_eq!(editor.history.len(), steps + 1);
        assert!(editor.undo());
        assert_eq!(editor.doc.node(target).unwrap(), &before);
    }

    #[test]
    fn builtin_strokes_and_locked_targets() {
        let mut editor = Editor::new(Document::new(64, 64), None);
        let mut settings = Settings::default();
        let id = add_path(&mut editor, json!({"width":5}));
        for (name, dash, count) in [
            ("dashed", [12., 6.], 2),
            ("dotted", [0., 10.], 2),
            ("solid", [0., 10.], 0),
        ] {
            run(
                &mut editor,
                "apply_shape_stroke_preset",
                &json!({"node":id,"name":name,"source":"builtin"}),
                &mut settings,
            )
            .unwrap();
            let result = style(&editor, id);
            assert_eq!(result.dash_count, count);
            assert_eq!(result.dash[..2], dash);
            assert_eq!(result.fill, Some([17, 34, 51, 255]));
            assert_eq!(result.cap, StrokeCap::Round);
        }
        editor
            .execute(Command::SetLocked { id, locked: true })
            .unwrap();
        let before = editor.doc.clone();
        let steps = editor.history.len();
        assert!(
            run(
                &mut editor,
                "apply_shape_stroke_preset",
                &json!({"node":id,"name":"dashed","source":"builtin"}),
                &mut settings
            )
            .is_err()
        );
        assert_eq!(editor.doc, before);
        assert_eq!(editor.history.len(), steps);
    }

    #[test]
    fn preset_library_validates_duplicates_overwrite_limits_and_arguments() {
        let mut editor = Editor::new(Document::new(64, 64), None);
        let mut settings = Settings::default();
        let id = add_path(&mut editor, json!({}));
        let steps = editor.history.len();
        run(
            &mut editor,
            "save_shape_stroke_preset",
            &json!({"node":id,"name":"One"}),
            &mut settings,
        )
        .unwrap();
        let before = settings.shape_stroke_presets.clone();
        for args in [
            json!({"node":id,"name":"One"}),
            json!({"node":id,"name":"Two","overwrite":"yes"}),
            json!({"node":id,"name":" "}),
            json!({"node":id,"name":"x".repeat(81)}),
            json!({"node":999,"name":"Two"}),
        ] {
            assert!(
                run(
                    &mut editor,
                    "save_shape_stroke_preset",
                    &args,
                    &mut settings
                )
                .is_err()
            );
            assert_eq!(settings.shape_stroke_presets, before);
            assert_eq!(editor.history.len(), steps);
        }
        run(
            &mut editor,
            "save_shape_stroke_preset",
            &json!({"node":id,"name":"One","overwrite":true}),
            &mut settings,
        )
        .unwrap();
        assert_eq!(settings.shape_stroke_presets.len(), 1);
        for n in 1..32 {
            run(
                &mut editor,
                "save_shape_stroke_preset",
                &json!({"node":id,"name":format!("Preset {n}")}),
                &mut settings,
            )
            .unwrap();
        }
        assert!(
            run(
                &mut editor,
                "save_shape_stroke_preset",
                &json!({"node":id,"name":"Overflow"}),
                &mut settings
            )
            .is_err()
        );
        assert_eq!(settings.shape_stroke_presets.len(), 32);
        run(
            &mut editor,
            "save_shape_stroke_preset",
            &json!({"node":id,"name":"One","overwrite":true}),
            &mut settings,
        )
        .unwrap();
        for args in [
            json!({"node":id,"name":"Missing"}),
            json!({"node":id,"name":"dashed","source":null}),
            json!({"node":id,"name":"dashed","source":"invalid"}),
            json!({"node":id,"name":"invalid","source":"builtin"}),
        ] {
            assert!(
                run(
                    &mut editor,
                    "apply_shape_stroke_preset",
                    &args,
                    &mut settings
                )
                .is_err()
            );
            assert_eq!(editor.history.len(), steps);
        }
    }
}
