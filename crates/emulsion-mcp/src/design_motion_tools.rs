//! Native active-page presentation metadata and UI-host presentation contracts.
use crate::{ToolDef, ToolResult};
use emulsion_core::{Command, Editor, NodeId, design::media, design_metadata::Motion};
use serde_json::{Value, json};

pub(crate) const READ_ONLY: &[&str] = &[
    "get_design_presentation",
    "list_design_media",
    "get_design_keyframes",
    "get_responsive_preview",
    "get_presentation_media_state",
    "get_presenter_timer",
    "list_design_videos",
    "get_presentation_state",
];
pub(crate) const DESTRUCTIVE: &[&str] = &[
    "import_design_lottie",
    "set_design_presentation",
    "update_design_media",
    "detach_design_media",
    "set_design_keyframe",
    "retime_design_motion",
    "apply_design_motion_preset",
    "remove_design_keyframe",
    "clear_design_keyframes",
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
    "trigger_presentation_object",
    "play_presentation_media",
    "pause_presentation_media",
    "seek_presentation_media",
    "stop_presentation_media",
    "get_presentation_media_state",
    "set_presenter_timer",
    "reset_presenter_timer",
    "get_presenter_timer",
    "set_responsive_preview",
    "end_responsive_preview",
    "get_responsive_preview",
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
            "import_design_lottie",
            "Import Lottie JSON as editable native groups, paths, text, embedded images and property keyframes in one Undo. Does not fetch external assets. Returns explicit unsupported-feature diagnostics; existing page dimensions stay unchanged and duration extends as required.",
            json!({"path":{"type":"string"}}),
            &["path"],
        ),
        def(
            "export_design_motion",
            "Export active-page motion as animated_svg, editable vector lottie, or explicit lottie_raster rendered frames. Vector export supports paths, gradients, native transform keyframes, plain text and embedded images; unsupported masks/effects/advanced typography return a diagnostic rather than silently rasterizing. Up to 600 frames/64MiB; rendered frames fit 1024px.",
            json!({"path":{"type":"string"},"format":{"enum":["animated_svg","lottie","lottie_raster"]}}),
            &["path", "format"],
        ),
        def(
            "retime_design_motion",
            "Scale and shift selected objects' property keyframes and enter/exit timing atomically. Rejects rounded time collisions or out-of-duration points. Optional duration_ms changes page duration. One Undo.",
            json!({"nodes":{"type":"array","items":node(),"minItems":1,"maxItems":256},"scale":{"type":"number","minimum":0.01,"maximum":100},"offset_ms":{"type":"integer","minimum":-60000,"maximum":60000},"duration_ms":{"type":"integer","minimum":100,"maximum":60000}}),
            &["nodes", "scale", "offset_ms"],
        ),
        def(
            "apply_design_motion_preset",
            "Apply original native keyframe presets in one Undo. Replaces affected property tracks only; other tracks and enter/exit effects remain. Typewriter requires text; spin rejects linked media.",
            json!({"nodes":{"type":"array","items":node(),"minItems":1,"maxItems":256},"preset":{"enum":["fade_in","slide_up","pop","pulse","spin","typewriter"]},"start_ms":{"type":"integer","minimum":0},"end_ms":{"type":"integer","minimum":2}}),
            &["nodes", "preset", "start_ms", "end_ms"],
        ),
        def(
            "get_presentation_media_state",
            "Read active player readiness, observed time/duration, pause/error state and pending commands. Requires UI host; commands are asynchronous until the player reports state.",
            json!({}),
            &[],
        ),
        def(
            "play_presentation_media",
            "Start or resume visible local audio/video or an official YouTube embed during presentation. Requires valid fully visible frame and system playback runtime.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "pause_presentation_media",
            "Pause the active native presentation media player. Requires UI host. Delivery is asynchronous; inspect get_presentation_media_state.",
            json!({}),
            &[],
        ),
        def(
            "seek_presentation_media",
            "Seek the ready active media player in source-file milliseconds. Must fit duration and local trim interval. Official YouTube seeking follows its keyframe availability.",
            json!({"position_ms":{"type":"integer","minimum":0,"maximum":86400000}}),
            &["position_ms"],
        ),
        def(
            "stop_presentation_media",
            "Close the active player and show its editable poster. Requires UI host.",
            json!({}),
            &[],
        ),
        def(
            "get_presenter_timer",
            "Read the live presenter elapsed time and paused state; does not change slide timing.",
            json!({}),
            &[],
        ),
        def(
            "set_presenter_timer",
            "Pause or resume the separate presenter elapsed timer. Does not pause slide animation or media playback.",
            json!({"paused":{"type":"boolean"}}),
            &["paused"],
        ),
        def(
            "reset_presenter_timer",
            "Reset the presenter elapsed timer to zero and start it. Does not alter the document or slide timing.",
            json!({}),
            &[],
        ),
        def(
            "get_responsive_preview",
            "Read live responsive preview state; requires the Emulsion UI host.",
            json!({}),
            &[],
        ),
        def(
            "set_responsive_preview",
            "Preview responsive layout at a width without modifying the authored page. Requires the Emulsion UI host.",
            json!({"width":{"type":"integer","minimum":1,"maximum":100000}}),
            &["width"],
        ),
        def(
            "end_responsive_preview",
            "Restore the authored page and view after responsive preview. Requires the Emulsion UI host.",
            json!({}),
            &[],
        ),
        def(
            "list_design_media",
            "List portable local audio/video metadata, omitting embedded bytes. No network requests.",
            json!({}),
            &[],
        ),
        def(
            "add_design_media",
            "Import local video/audio bytes into an editable poster. Supports MP4/M4V/WebM/MOV, MP3/M4A/WAV/Ogg/Opus; maximum32 MiB per asset and 64 MiB per page. Path is read once, never saved. System codec support varies.",
            json!({"path":{"type":"string"},"origin":pair(),"size":pair()}),
            &["path"],
        ),
        def(
            "update_design_media",
            "Patch local media trim milliseconds, volume 0–1 and looping in one Undo step. Null trim_end_ms means file end; omitted fields remain unchanged.",
            json!({"node":node(),"trim_start_ms":{"type":"integer","minimum":0},"trim_end_ms":{"type":["integer","null"],"minimum":1},"volume":{"type":"number","minimum":0,"maximum":1},"looping":{"type":"boolean"}}),
            &["node"],
        ),
        def(
            "detach_design_media",
            "Remove a local audio/video association while retaining editable poster artwork. One Undo.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "get_design_keyframes",
            "Read deterministic property tracks for the active page. Values are offsets/multipliers relative to authored artwork.",
            json!({}),
            &[],
        ),
        def(
            "set_design_keyframe",
            "Add or replace a property keyframe at a millisecond time in one Undo. Easing controls interpolation from this point to the next; first/last values hold outside their interval. Times must fit page duration. Geometry is relative; opacity multiplies authored opacity.",
            json!({"node":node(),"property":{"type":"string","enum":["translation_x","translation_y","scale_x","scale_y","rotation","opacity","visibility","text_reveal"]},"time_ms":{"type":"integer","minimum":0},"value":{"type":"number"},"easing":{"type":"string","enum":["linear","ease_in","ease_out","ease_in_out","step"]}}),
            &["node", "property", "time_ms", "value"],
        ),
        def(
            "remove_design_keyframe",
            "Delete one saved property keyframe. One Undo.",
            json!({"node":node(),"property":{"type":"string","enum":["translation_x","translation_y","scale_x","scale_y","rotation","opacity","visibility","text_reveal"]},"time_ms":{"type":"integer","minimum":0}}),
            &["node", "property", "time_ms"],
        ),
        def(
            "clear_design_keyframes",
            "Remove all property tracks from an object without changing authored artwork. One Undo.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "trigger_presentation_object",
            "Trigger a saved interaction on a visible object during a live presentation. Requires the Emulsion UI host.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "get_design_presentation",
            "Read active-page speaker notes, slide transition, timing and object motion. Does not start playback.",
            json!({}),
            &[],
        ),
        def(
            "set_design_presentation",
            "Patch active-page notes, transition and timing in one Undo step. Existing animations must remain within duration; invalid combinations fail atomically. Omitted fields remain unchanged.",
            json!({"speaker_notes":{"type":"string","maxLength":20000},"page_transition":{"enum":["none","fade","slide","slide_left","slide_up","slide_down","zoom","zoom_out"]},"transition_ms":{"type":"integer","minimum":100,"maximum":3000},"duration_ms":{"type":"integer","minimum":100,"maximum":60000},"fps":{"type":"integer","minimum":1,"maximum":60}}),
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
    Trigger(NodeId),
    ResponsivePreview(Option<u32>),
    MediaPlay(NodeId),
    MediaPause,
    MediaSeek(u32),
    MediaStop,
    TimerPaused(bool),
    TimerReset,
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
        "get_presentation_state"
        | "get_responsive_preview"
        | "get_presentation_media_state"
        | "get_presenter_timer" => HostAction::State,
        "play_presentation_media" => HostAction::MediaPlay(id(args)?),
        "pause_presentation_media" => HostAction::MediaPause,
        "stop_presentation_media" => HostAction::MediaStop,
        "seek_presentation_media" => {
            let position: u32 = read(args, "position_ms")?;
            if position > 86_400_000 {
                return Err("Seek position must be at most 24 hours.".into());
            }
            HostAction::MediaSeek(position)
        }
        "set_presenter_timer" => HostAction::TimerPaused(read(args, "paused")?),
        "reset_presenter_timer" => HostAction::TimerReset,
        "end_responsive_preview" => HostAction::ResponsivePreview(None),
        "set_responsive_preview" => {
            let width: u32 = read(args, "width")?;
            if !(1..=100_000).contains(&width) {
                return Err("Preview width must be 1–100000.".into());
            }
            HostAction::ResponsivePreview(Some(width))
        }
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
        "trigger_presentation_object" => HostAction::Trigger(id(args)?),
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
        "import_design_lottie" => {
            let path: String = read(args, "path")?;
            let (doc, report) = emulsion_io::lottie::read(std::path::Path::new(&path))
                .map_err(|e| e.to_string())?;
            let ids = emulsion_io::lottie::insert(editor, &doc).map_err(|e| e.to_string())?;
            return Ok(json!({"nodes":ids,"report":report}));
        }
        "export_design_motion" => {
            let path: String = read(args, "path")?;
            let report = emulsion_io::design_motion_export::write(
                &editor.doc,
                std::path::Path::new(&path),
                read(args, "format")?,
            )
            .map_err(|e| e.to_string())?;
            return serde_json::to_value(report).map_err(|e| e.to_string());
        }
        "retime_design_motion" => {
            let duration = args
                .get("duration_ms")
                .map(|_| read(args, "duration_ms"))
                .transpose()?;
            emulsion_core::design_keyframes::retime(
                editor,
                &read::<Vec<NodeId>>(args, "nodes")?,
                read(args, "scale")?,
                read(args, "offset_ms")?,
                duration,
            )?;
            return Ok(json!({"retimed":true}));
        }
        "apply_design_motion_preset" => {
            emulsion_core::design_keyframes::apply_preset(
                editor,
                &read::<Vec<NodeId>>(args, "nodes")?,
                read(args, "preset")?,
                read(args, "start_ms")?,
                read(args, "end_ms")?,
            )?;
            return Ok(json!({"applied":true}));
        }
        "list_design_media" => {
            return Ok(
                json!({"media":editor.doc.design.local_media.iter().map(|(node,m)|json!({"node":node,"name":m.name,"kind":m.kind,"mime":m.mime,"byte_length":m.bytes.len(),"trim_start_ms":m.trim_start_ms,"trim_end_ms":m.trim_end_ms,"volume":m.volume,"looping":m.looping,"bounds":media::bounds(&editor.doc,*node)})).collect::<Vec<_>>()}),
            );
        }
        "add_design_media" => {
            let path: String = read(args, "path")?;
            let media = emulsion_io::design_media::read_local(std::path::Path::new(&path))?;
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
            return Ok(json!({"node":media::insert_local(editor,media,origin,size)?}));
        }
        "update_design_media" => {
            let id = id(args)?;
            let mut item = editor
                .doc
                .design
                .local_media
                .get(&id)
                .cloned()
                .ok_or("Choose a local media object")?;
            patch(&mut item.trim_start_ms, args, "trim_start_ms")?;
            patch(&mut item.trim_end_ms, args, "trim_end_ms")?;
            patch(&mut item.volume, args, "volume")?;
            patch(&mut item.looping, args, "looping")?;
            media::update_local(
                editor,
                id,
                item.trim_start_ms,
                item.trim_end_ms,
                item.volume,
                item.looping,
            )?;
            return Ok(json!({"updated":true}));
        }
        "detach_design_media" => {
            media::detach_local(editor, id(args)?)?;
            return Ok(json!({"detached":true}));
        }
        "get_design_keyframes" => {
            return Ok(
                json!({"duration_ms":editor.doc.design.duration_ms,"tracks":editor.doc.design.keyframes}),
            );
        }
        "set_design_keyframe" => {
            let easing = if args.get("easing").is_some() {
                read(args, "easing")?
            } else {
                Default::default()
            };
            emulsion_core::design_keyframes::set_keyframe(
                editor,
                id(args)?,
                read(args, "property")?,
                emulsion_core::design_keyframes::Keyframe {
                    time_ms: read(args, "time_ms")?,
                    value: read(args, "value")?,
                    easing,
                },
            )?;
            return Ok(json!({"saved":true}));
        }
        "remove_design_keyframe" => {
            emulsion_core::design_keyframes::remove_keyframe(
                editor,
                id(args)?,
                read(args, "property")?,
                read(args, "time_ms")?,
            )?;
            return Ok(json!({"removed":true}));
        }
        "clear_design_keyframes" => {
            emulsion_core::design_keyframes::clear(editor, id(args)?)?;
            return Ok(json!({"removed":true}));
        }
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

#[cfg(test)]
mod local_keyframe_tests {
    use super::*;
    #[test]
    fn design_media_and_keyframe_tools_share_native_history() {
        let mut editor = Editor::new(emulsion_core::Document::new(800, 600), None);
        let asset =
            media::LocalMedia::from_bytes("tone.wav".into(), b"RIFF\0\0\0\0WAVEdata".to_vec())
                .unwrap();
        let id = media::insert_local(&mut editor, asset, (0., 0.), (400., 225.)).unwrap();
        let listed = run(&mut editor, "list_design_media", &json!({})).unwrap();
        assert_eq!(listed["media"][0]["byte_length"], 16);
        assert!(listed["media"][0].get("bytes").is_none());
        let original = editor.doc.design.clone();
        assert!(!execute(&mut editor,"update_design_media",&json!({"node":id,"trim_start_ms":100,"trim_end_ms":500,"volume":0.2,"looping":true})).unwrap().is_error);
        editor.undo();
        assert_eq!(editor.doc.design, original);
        assert!(
            !execute(
                &mut editor,
                "set_design_keyframe",
                &json!({"node":id,"property":"opacity","time_ms":0,"value":0.5,"easing":"ease_out"})
            )
            .unwrap()
            .is_error
        );
        let changed = editor.doc.design.clone();
        assert!(
            execute(
                &mut editor,
                "set_design_keyframe",
                &json!({"node":id,"property":"opacity","time_ms":90000,"value":0.5})
            )
            .unwrap()
            .is_error
        );
        assert_eq!(editor.doc.design, changed);
        assert!(
            execute(
                &mut editor,
                "set_design_keyframe",
                &json!({"node":id,"property":"opacity","time_ms":0,"value":0.5,"typo":true})
            )
            .unwrap()
            .is_error
        );
        editor.undo();
        assert_eq!(editor.doc.design, original);
        assert!(
            execute(
                &mut editor,
                "trigger_presentation_object",
                &json!({"node":id})
            )
            .unwrap()
            .is_error
        );
    }
}

#[cfg(test)]
mod runtime_control_tests {
    use super::*;
    #[test]
    fn design_live_media_and_timer_contract_is_strict_and_host_only() {
        assert_eq!(
            parse_host_action("play_presentation_media", &json!({"node":9})).unwrap(),
            HostAction::MediaPlay(9)
        );
        assert_eq!(
            parse_host_action("seek_presentation_media", &json!({"position_ms":500})).unwrap(),
            HostAction::MediaSeek(500)
        );
        assert!(
            parse_host_action("seek_presentation_media", &json!({"position_ms":86400001})).is_err()
        );
        assert!(parse_host_action("seek_presentation_media", &json!({"position_ms":-1})).is_err());
        assert!(
            parse_host_action(
                "pause_presentation_media",
                &json!({"arbitrary_script":"alert(1)"})
            )
            .is_err()
        );
        assert!(parse_host_action("set_presenter_timer", &json!({"paused":"true"})).is_err());
        let mut editor = Editor::new(emulsion_core::Document::new(10, 10), None);
        let before = editor.doc.clone();
        for (name, args) in [
            ("play_presentation_media", json!({"node":9})),
            ("pause_presentation_media", json!({})),
            ("seek_presentation_media", json!({"position_ms":500})),
            ("get_presenter_timer", json!({})),
            ("reset_presenter_timer", json!({})),
        ] {
            assert!(execute(&mut editor, name, &args).unwrap().is_error);
            assert_eq!(editor.doc, before);
        }
    }
}

#[cfg(test)]
mod lottie_workflow_tests {
    use super::*;
    use emulsion_core::{Document, Node};
    use std::sync::Arc;
    #[test]
    fn lottie_tools_import_export_validate_and_undo_native_objects() {
        let dir = std::env::temp_dir().join(format!("emulsion-mcp-lottie-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("animation.json");
        let mut source = Document::new(100, 80);
        let id = source.alloc_id();
        source.nodes.push(Node::path(
            id,
            "Editable",
            Arc::new(emulsion_raster::vector_geometry::rectangle(
                10., 10., 20., 20.,
            )),
            Default::default(),
            100,
            80,
        ));
        let (bytes, _) = emulsion_io::lottie::encode(&source).unwrap();
        std::fs::write(&path, bytes).unwrap();
        let mut editor = Editor::new(Document::new(200, 150), None);
        let before = editor.doc.clone();
        assert!(
            !execute(&mut editor, "import_design_lottie", &json!({"path":path}))
                .unwrap()
                .is_error
        );
        assert!(!editor.doc.nodes.is_empty());
        assert_eq!(editor.doc.width, 200);
        let out = dir.join("vectors.json");
        assert!(
            !execute(
                &mut editor,
                "export_design_motion",
                &json!({"path":out,"format":"lottie"})
            )
            .unwrap()
            .is_error
        );
        let value: Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
        assert!(
            value["assets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["layers"].is_array())
        );
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
        assert!(
            execute(
                &mut editor,
                "import_design_lottie",
                &json!({"path":path,"unsafe":true})
            )
            .unwrap()
            .is_error
        );
        assert_eq!(editor.doc, before);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
