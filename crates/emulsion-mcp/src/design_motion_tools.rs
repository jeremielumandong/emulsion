//! Native active-page presentation metadata and UI-host presentation contracts.
use crate::{ToolDef, ToolResult};
use emulsion_core::{Command, Editor, NodeId, design::media, design_metadata::Motion};
use serde_json::{Value, json};

pub(crate) const READ_ONLY: &[&str] = &[
    "get_design_presentation",
    "list_design_videos",
    "get_presentation_state",
];
pub(crate) const DESTRUCTIVE: &[&str] = &[
    "set_design_presentation",
    "set_design_motion",
    "remove_design_motion",
    "update_design_video",
    "detach_design_video",
];
pub const HOST_TOOLS: &[&str] = &[
    "get_presentation_state",
    "start_presentation",
    "end_presentation",
    "navigate_presentation",
    "set_presentation_fullscreen",
];

fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    }
}
fn node() -> Value {
    json!({"type":"integer","minimum":1})
}
fn pair() -> Value {
    json!({"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2})
}
fn effect() -> Value {
    json!({"type":"string","enum":["none","fade","slide","zoom"]})
}
pub(crate) fn definitions() -> Vec<ToolDef> {
    vec![
        def(
            "get_design_presentation",
            "Read active-page speaker notes, slide transition, timing and object motion. Does not start playback.",
            json!({}),
            &[],
        ),
        def(
            "set_design_presentation",
            "Patch active-page notes, transition and timing in one Undo step. Existing animations must remain within duration; invalid combinations fail atomically. Omitted fields remain unchanged.",
            json!({"speaker_notes":{"type":"string","maxLength":20000},"page_transition":effect(),"transition_ms":{"type":"integer","minimum":100,"maximum":3000},"duration_ms":{"type":"integer","minimum":100,"maximum":60000},"fps":{"type":"integer","minimum":1,"maximum":60}}),
            &[],
        ),
        def(
            "set_design_motion",
            "Patch animation for an unlocked native node. New animations default to fade-in over the page duration; transition must fit within half the interval. Times are milliseconds. One Undo step.",
            json!({"node":node(),"enter":effect(),"exit":effect(),"start_ms":{"type":"integer","minimum":0},"end_ms":{"type":"integer","minimum":1},"transition_ms":{"type":"integer","minimum":1},"offset":pair()}),
            &["node"],
        ),
        def(
            "remove_design_motion",
            "Remove an unlocked node's saved animation, preserving its artwork. One Undo step.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "list_design_videos",
            "List active-page YouTube link metadata and native poster bounds. No network requests or video downloading.",
            json!({}),
            &[],
        ),
        def(
            "add_design_video",
            "Create an editable YouTube poster/link group. Strict YouTube URLs only; default origin [0,0], size [640,360]. Playback is online through the official embedded player, with at least 200 by 200 screen pixels.",
            json!({"url":{"type":"string"},"origin":pair(),"size":pair()}),
            &["url"],
        ),
        def(
            "update_design_video",
            "Change the URL/start time of an unlocked linked YouTube group while keeping its placement. One Undo step.",
            json!({"node":node(),"url":{"type":"string"}}),
            &["node", "url"],
        ),
        def(
            "detach_design_video",
            "Remove a YouTube association, retaining its editable native poster artwork. One Undo step.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "get_presentation_state",
            "Read live audience/presenter state. Requires the running Emulsion UI host; offline servers cannot report playback state.",
            json!({}),
            &[],
        ),
        def(
            "start_presentation",
            "Start a live nonmutating presentation of the current project page. Requires a visible Emulsion editor. Fullscreen defaults true; presenter opens a separate notes window; auto_advance defaults false. Repeated starts keep the original return page/view.",
            json!({"fullscreen":{"type":"boolean"},"presenter":{"type":"boolean"},"auto_advance":{"type":"boolean"}}),
            &[],
        ),
        def(
            "end_presentation",
            "End live presentation, stop video, and restore original editing page, selection and view. Requires Emulsion UI host; no saved edits.",
            json!({}),
            &[],
        ),
        def(
            "navigate_presentation",
            "Navigate the running presentation. direction is next, previous, first or last. Requires Emulsion UI host; stops the previous slide video.",
            json!({"direction":{"type":"string","enum":["next","previous","first","last"]}}),
            &["direction"],
        ),
        def(
            "set_presentation_fullscreen",
            "Set live audience fullscreen; all application controls are hidden in fullscreen. Requires an active presentation in Emulsion UI host.",
            json!({"enabled":{"type":"boolean"}}),
            &["enabled"],
        ),
    ]
}
fn validate(name: &str, args: &Value) -> Result<(), String> {
    let definition = definitions()
        .into_iter()
        .find(|d| d.name == name)
        .ok_or("Unknown presentation tool")?;
    let values = args.as_object().ok_or("Arguments must be an object")?;
    for key in values.keys() {
        if definition.input_schema["properties"].get(key).is_none() {
            return Err(format!("Unknown argument: {key}"));
        }
    }
    for key in definition.input_schema["required"].as_array().unwrap() {
        let key = key.as_str().unwrap();
        if !values.contains_key(key) {
            return Err(format!("Missing argument: {key}"));
        }
    }
    Ok(())
}
fn read<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(args[key].clone()).map_err(|e| format!("Invalid {key}: {e}"))
}
fn id(args: &Value) -> Result<NodeId, String> {
    let id = read(args, "node")?;
    if id == 0 {
        return Err("node must be a positive native ID".into());
    }
    Ok(id)
}
fn patch<T: serde::de::DeserializeOwned>(
    target: &mut T,
    args: &Value,
    key: &str,
) -> Result<(), String> {
    if args.get(key).is_some() {
        *target = read(args, key)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Next,
    Previous,
    First,
    Last,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostAction {
    State,
    Start {
        fullscreen: bool,
        presenter: bool,
        auto_advance: bool,
    },
    End,
    Navigate(Direction),
    Fullscreen(bool),
}
pub fn parse_host_action(name: &str, args: &Value) -> Result<HostAction, String> {
    validate(name, args)?;
    let boolean = |key, default| -> Result<bool, String> {
        if args.get(key).is_some() {
            read(args, key)
        } else {
            Ok(default)
        }
    };
    Ok(match name {
        "get_presentation_state" => HostAction::State,
        "start_presentation" => {
            let fullscreen = boolean("fullscreen", true)?;
            let presenter = boolean("presenter", false)?;
            if presenter && !fullscreen {
                return Err("Presenter view requires a fullscreen audience".into());
            }
            HostAction::Start {
                fullscreen,
                presenter,
                auto_advance: boolean("auto_advance", false)?,
            }
        }
        "end_presentation" => HostAction::End,
        "navigate_presentation" => {
            HostAction::Navigate(match read::<String>(args, "direction")?.as_str() {
                "next" => Direction::Next,
                "previous" => Direction::Previous,
                "first" => Direction::First,
                "last" => Direction::Last,
                _ => return Err("direction must be next, previous, first or last".into()),
            })
        }
        "set_presentation_fullscreen" => HostAction::Fullscreen(read(args, "enabled")?),
        _ => return Err("Not a live presentation tool".into()),
    })
}
fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    if HOST_TOOLS.contains(&name) {
        parse_host_action(name, args)?;
        return Err("This tool requires the running Emulsion UI host; offline document servers cannot control or report live presentation state.".into());
    }
    if !READ_ONLY.contains(&name) && editor.in_transaction() {
        return Err("Finish the current edit before changing presentation metadata".into());
    }
    match name {
        "get_design_presentation" => {
            let d = &editor.doc.design;
            return Ok(
                json!({"scope":"active_page","speaker_notes":d.speaker_notes,"page_transition":d.page_transition,"transition_ms":d.transition_ms,"duration_ms":d.duration_ms,"fps":d.fps,"motion":d.motion}),
            );
        }
        "list_design_videos" => {
            return Ok(
                json!({"scope":"active_page","videos":editor.doc.design.media.iter().map(|(node,video)| json!({"node":node,"url":video.url(),"video":video,"bounds":media::bounds(&editor.doc,*node)})).collect::<Vec<_>>()}),
            );
        }
        "add_design_video" => {
            let url: String = read(args, "url")?;
            let origin = if args.get("origin").is_some() {
                read(args, "origin")?
            } else {
                (0., 0.)
            };
            let size = if args.get("size").is_some() {
                read(args, "size")?
            } else {
                (640., 360.)
            };
            return Ok(json!({"node":media::insert_youtube(editor,&url,origin,size)?}));
        }
        "update_design_video" => {
            media::update_youtube(editor, id(args)?, &read::<String>(args, "url")?)?;
            return Ok(json!({"updated":true}));
        }
        "detach_design_video" => {
            media::detach_youtube(editor, id(args)?)?;
            return Ok(json!({"detached":true}));
        }
        _ => (),
    }
    let mut design = editor.doc.design.clone();
    match name {
        "set_design_presentation" => {
            if args.as_object().unwrap().is_empty() {
                return Err("Provide at least one presentation field".into());
            }
            patch(&mut design.speaker_notes, args, "speaker_notes")?;
            patch(&mut design.page_transition, args, "page_transition")?;
            patch(&mut design.transition_ms, args, "transition_ms")?;
            patch(&mut design.duration_ms, args, "duration_ms")?;
            patch(&mut design.fps, args, "fps")?;
        }
        "set_design_motion" | "remove_design_motion" => {
            let node = id(args)?;
            if editor.doc.node(node).is_none() || editor.doc.locked_ancestor(node).is_some() {
                return Err("Choose an existing unlocked node".into());
            }
            if name == "remove_design_motion" {
                if design.motion.remove(&node).is_none() {
                    return Err("Node has no saved animation".into());
                }
            } else {
                let motion = design.motion.entry(node).or_insert_with(|| Motion {
                    end_ms: design.duration_ms,
                    transition_ms: (design.duration_ms / 2).min(500),
                    ..Motion::default()
                });
                patch(&mut motion.enter, args, "enter")?;
                patch(&mut motion.exit, args, "exit")?;
                patch(&mut motion.start_ms, args, "start_ms")?;
                patch(&mut motion.end_ms, args, "end_ms")?;
                patch(&mut motion.transition_ms, args, "transition_ms")?;
                patch(&mut motion.offset, args, "offset")?;
            }
        }
        _ => return Err("Unknown presentation tool".into()),
    }
    if design == editor.doc.design {
        return Ok(json!({"changed":false}));
    }
    editor
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|e| e.to_string())?;
    Ok(json!({"changed":true}))
}
pub(crate) fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    if !definitions().iter().any(|d| d.name == name) {
        return None;
    }
    Some(
        match validate(name, args).and_then(|()| run(editor, name, args)) {
            Ok(value) => ToolResult::text(value.to_string()),
            Err(error) => ToolResult::error(error),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::Document;
    #[test]
    fn metadata_is_atomic_and_undoable() {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let before = editor.doc.design.clone();
        assert!(!execute(&mut editor,"set_design_presentation",&json!({"speaker_notes":"Private notes","page_transition":"slide","duration_ms":1000,"transition_ms":700})).unwrap().is_error);
        assert_eq!(editor.doc.design.speaker_notes, "Private notes");
        let changed = editor.doc.design.clone();
        assert!(
            execute(
                &mut editor,
                "set_design_presentation",
                &json!({"speaker_notes":"bad","fps":61})
            )
            .unwrap()
            .is_error
        );
        assert_eq!(editor.doc.design, changed);
        editor.undo();
        assert_eq!(editor.doc.design, before);
        editor.redo();
        assert_eq!(editor.doc.design, changed);
    }
    #[test]
    fn video_and_motion_use_native_validation_and_undo() {
        let mut editor = Editor::new(Document::new(800, 600), None);
        assert!(
            execute(
                &mut editor,
                "add_design_video",
                &json!({"url":"https://example.com/watch?v=M7lc1UVf-VE"})
            )
            .unwrap()
            .is_error
        );
        assert!(editor.doc.design.media.is_empty());
        assert!(
            !execute(
                &mut editor,
                "add_design_video",
                &json!({"url":"https://youtu.be/M7lc1UVf-VE?t=15"})
            )
            .unwrap()
            .is_error
        );
        let id = *editor.doc.design.media.keys().next().unwrap();
        assert!(
            !execute(
                &mut editor,
                "set_design_motion",
                &json!({"node":id,"enter":"zoom","end_ms":2000})
            )
            .unwrap()
            .is_error
        );
        let before = editor.doc.design.clone();
        assert!(
            execute(
                &mut editor,
                "set_design_presentation",
                &json!({"duration_ms":1000})
            )
            .unwrap()
            .is_error
        );
        assert_eq!(editor.doc.design, before);
        editor.doc.node_mut(id).unwrap().locked = true;
        assert!(
            execute(&mut editor, "remove_design_motion", &json!({"node":id}))
                .unwrap()
                .is_error
        );
        editor.doc.node_mut(id).unwrap().locked = false;
        assert!(
            !execute(&mut editor, "detach_design_video", &json!({"node":id}))
                .unwrap()
                .is_error
        );
        assert!(editor.doc.node(id).is_some());
        assert!(editor.doc.design.media.is_empty());
        editor.undo();
        assert!(editor.doc.design.media.contains_key(&id));
    }
    #[test]
    fn strict_contract_and_offline_host_errors() {
        let mut editor = Editor::new(Document::new(100, 100), None);
        for (name, args) in [
            ("start_presentation", json!({})),
            ("get_presentation_state", json!({})),
            ("set_design_presentation", json!({"fps":null})),
            ("get_design_presentation", json!({"extra":1})),
            ("navigate_presentation", json!({"direction":"no"})),
        ] {
            assert!(
                execute(&mut editor, name, &args).unwrap().is_error,
                "{name}"
            );
        }
        assert_eq!(
            parse_host_action("start_presentation", &json!({})).unwrap(),
            HostAction::Start {
                fullscreen: true,
                presenter: false,
                auto_advance: false
            }
        );
        assert!(
            parse_host_action(
                "start_presentation",
                &json!({"fullscreen":false,"presenter":true})
            )
            .is_err()
        );
    }
}
