//! The assistant's script breakdown: read a screenplay as scenes and beats
//! with stable beat IDs, build panels from the breakdown it writes (one
//! Undo step through the panel paste), and estimate panel durations from
//! their captions by word rate, as a dry run or applied.
use super::script::absolute;
use super::timing::{fit_transitions, scene_selection, seconds, selection};
use super::{def, insertion, layout, placement};
use crate::ToolDef;
use emulsion_core::project::{MAX_PAGES, PageId, ProjectEditor};
use emulsion_core::storyboard::{MAX_CAPTION_CHARS, MAX_PANEL_FRAMES, Storyboard};
use emulsion_core::storyboard_breakdown::{Breakdown, MAX_SOURCE_BEATS, is_beat_id};
use emulsion_core::storyboard_estimate::{self as estimate, Kept, MAX_RATE_FIELDS, WordRates};
use emulsion_io::script::{self, Beat, Script};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Scenes read_storyboard_script returns per page by default, and at most.
const SCENES_PER_PAGE: usize = 20;
const MAX_SCENES_PER_PAGE: usize = 100;
/// Beats one page holds before it stops at a scene boundary.
const MAX_PAGE_BEATS: usize = 400;

fn rates_schema() -> Value {
    let wpm = |what: &str| json!({"type":"number","minimum":20,"maximum":600,"description":format!("{what} words per minute.")});
    let pause = |what: &str| json!({"type":"number","minimum":0,"maximum":10,"description":format!("Seconds of pause {what}.")});
    let fields = |what: &str| json!({"type":"array","items":{"type":"string","minLength":1,"maxLength":200},"minItems":0,"maxItems":MAX_RATE_FIELDS,"description":format!("Caption fields read as {what}, by name.")});
    json!({
        "type":"object",
        "additionalProperties":false,
        "description":"Word rates; omitted values come from the person's Estimate durations settings (defaults: dialogue 150 wpm, action 120 wpm, 0.5 s after each dialogue line and each parenthetical, panels at least 1 s, Dialogue and Action fields).",
        "properties":{
            "dialogue_wpm":wpm("Spoken dialogue"),
            "action_wpm":wpm("Action line"),
            "line_pause":pause("after each line of dialogue"),
            "parenthetical_pause":pause("for each parenthetical such as (beat)"),
            "minimum_seconds":{"type":"number","minimum":0,"maximum":60,"description":"Shortest estimated panel."},
            "dialogue_fields":fields("dialogue"),
            "action_fields":fields("action")
        }
    })
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let path = json!({"type":"string","minLength":1,"maxLength":4096,"description":"Absolute path of the script (.fountain, .spmd, .fdx or .txt)."});
    let name = json!({"type":"string","maxLength":200});
    let text = json!({"type":"string","maxLength":MAX_CAPTION_CHARS});
    let panel = json!({
        "type":"object",
        "additionalProperties":false,
        "properties":{
            "captions":{"type":"object","additionalProperties":text,"description":"Caption text by field name (Action, Dialogue as \"NAME: line\", Slugging on a scene's first panel, or any other field); missing fields are added."},
            "notes":{"type":"string","maxLength":MAX_CAPTION_CHARS,"description":"Shot notes, added to the Notes caption."},
            "camera":{"type":"string","maxLength":MAX_CAPTION_CHARS,"description":"Camera hint (PUSH IN, PAN LEFT, HANDHELD), in the Camera caption."},
            "size":{"enum":["unset","extreme_wide","wide","full","medium","medium_close","close_up","extreme_close","insert"]},
            "angle":{"enum":["unset","eye","high","low","overhead","dutch","pov"]},
            "seconds":{"type":"number","minimum":0.01,"maximum":600,"description":"Suggested duration. Without seconds or frames the panel is timed from its captions by the word rates."},
            "frames":{"type":"integer","minimum":1,"maximum":MAX_PANEL_FRAMES},
            "source_beats":{"type":"array","items":{"type":"string","pattern":"^s[1-9][0-9]*b[1-9][0-9]*$"},"minItems":0,"maxItems":MAX_SOURCE_BEATS,"description":"Beat IDs from read_storyboard_script this panel shows."}
        }
    });
    let breakdown = json!({
        "type":"object",
        "additionalProperties":false,
        "properties":{
            "title":name,
            "scenes":{"type":"array","minItems":1,"maxItems":MAX_PAGES,"items":{
                "type":"object",
                "additionalProperties":false,
                "properties":{
                    "name":{"type":"string","maxLength":200,"description":"Scene name, usually its heading; empty takes the naming rules."},
                    "panels":{"type":"array","minItems":1,"maxItems":MAX_PAGES,"items":panel}
                },
                "required":["panels"]
            }}
        },
        "required":["scenes"]
    });
    let mut build = placement();
    build["breakdown"] = breakdown;
    build["script"] = json!({"type":"string","minLength":1,"maxLength":4096,"description":"Absolute path of the script the breakdown came from: source_beats are checked against it and the result lists beats no panel covers."});
    build["rates"] = rates_schema();
    let mut estimate = scene_selection();
    estimate["dry_run"] = json!({"type":"boolean","description":"Report old and new durations without changing the board."});
    estimate["rates"] = rates_schema();
    vec![
        def(
            "read_storyboard_script",
            "Read a screenplay (Fountain .fountain/.spmd, Final Draft .fdx or plain text) as structured scenes and beats for a breakdown, without changing anything. Each scene has an ID (s1), heading, speaking characters, word count and estimated seconds; each beat a stable ID (s1b3 = scene 1, beat 3), kind (action, dialogue or transition), text, character and parenthetical for dialogue, word count and estimated seconds at the word rates. Long scripts come in pages of scenes: pass `next_scene` back as from_scene. Works without an open storyboard.",
            json!({
                "path":path,
                "from_scene":{"type":"integer","minimum":0,"maximum":100000,"description":"First scene to return (0-based)."},
                "max_scenes":{"type":"integer","minimum":1,"maximum":MAX_SCENES_PER_PAGE,"description":"Scenes per page (default 20); a page also stops near 400 beats."},
                "rates":rates_schema()
            }),
            &["path"],
        ),
        def(
            "build_storyboard_from_breakdown",
            "Lay out a breakdown you wrote as storyboard scenes and panels: breakdown.scenes → panels, each with captions by field name, shot notes (Notes caption), a camera hint (Camera caption), shot size and angle, a duration in seconds or frames (or estimated from its captions by the word rates) and the source_beats it shows. The whole breakdown is checked first; errors name the scene and panel. The scenes go after the scene holding `after` (default: the active panel) or at_start; caption fields map by name and missing ones are added. With `script`, source_beats must exist in it and the result lists `uncovered_beats`. Returns the new panel IDs with their source beats. One Undo step.",
            build,
            &["breakdown"],
        ),
        def(
            "estimate_storyboard_durations",
            "Estimate panel durations from their captions by word rate: dialogue words per minute plus a pause per line and per parenthetical, action words per minute, never under the minimum; summed per scene. Scope: panels, scenes or scene_names (default: every panel). Panels without counted caption text and locked panels keep their duration. With dry_run the board is unchanged; otherwise the new durations apply as one Undo step (layer keyframes follow the keyframe sync mode, transitions shorten to fit). Returns each panel's and scene's old and new seconds. Refine single panels afterwards with set_storyboard_timing.",
            estimate,
            &[],
        ),
    ]
}

/// Run a breakdown tool; `None` when `name` is not one.
pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let result = match name {
        "read_storyboard_script" => read(args),
        "build_storyboard_from_breakdown" => build(editor, board, args),
        "estimate_storyboard_durations" => estimate_durations(editor, board, args),
        _ => return None,
    };
    Some(result)
}

/// The person's saved word rates with `args["rates"]` on top.
fn rates(args: &Value) -> Result<WordRates, String> {
    let saved = emulsion_io::settings::Settings::load()
        .storyboard
        .duration_rates;
    let mut merged = serde_json::to_value(saved).map_err(|e| e.to_string())?;
    for (key, value) in args["rates"].as_object().into_iter().flatten() {
        merged[key] = value.clone();
    }
    let rates: WordRates = serde_json::from_value(merged).map_err(|e| e.to_string())?;
    rates.validate()?;
    Ok(rates)
}

fn rates_json(rates: &WordRates) -> Value {
    serde_json::to_value(rates).unwrap_or_default()
}

fn round(seconds: f64) -> f64 {
    (seconds * 100.).round() / 100.
}

fn read_script(path: &std::path::Path) -> Result<Script, String> {
    script::read(path).map_err(|e| format!("{e:#}"))
}

/// read_storyboard_script; needs no storyboard.
pub(super) fn read(args: &Value) -> Result<Value, String> {
    let path = absolute(args)?;
    let script = read_script(path)?;
    let rates = rates(args)?;
    let from = args["from_scene"].as_u64().unwrap_or(0) as usize;
    let most = args["max_scenes"]
        .as_u64()
        .map_or(SCENES_PER_PAGE, |n| n as usize);
    let mut characters: Vec<(String, usize)> = Vec::new();
    let mut total = 0.;
    for beat in script.scenes.iter().flat_map(|s| &s.beats) {
        total += script::beat_seconds(beat, &rates);
        if let Beat::Dialogue { character, .. } = beat {
            match characters.iter_mut().find(|(n, _)| n == character) {
                Some((_, lines)) => *lines += 1,
                None => characters.push((character.clone(), 1)),
            }
        }
    }
    let (mut scenes, mut beats) = (Vec::new(), 0);
    let mut next = None;
    for (s, scene) in script.scenes.iter().enumerate().skip(from) {
        if scenes.len() >= most
            || (!scenes.is_empty() && beats + scene.beats.len() > MAX_PAGE_BEATS)
        {
            next = Some(s);
            break;
        }
        beats += scene.beats.len();
        let mut speaking: Vec<&str> = Vec::new();
        let (mut words, mut seconds) = (0, 0.);
        let items: Vec<Value> = scene
            .beats
            .iter()
            .enumerate()
            .map(|(b, beat)| {
                let beat_seconds = script::beat_seconds(beat, &rates);
                seconds += beat_seconds;
                let mut item = match beat {
                    Beat::Action(text) => json!({"kind":"action","text":text}),
                    Beat::Dialogue {
                        character,
                        parenthetical,
                        text,
                    } => {
                        if !speaking.contains(&character.as_str()) {
                            speaking.push(character);
                        }
                        json!({"kind":"dialogue","character":character,"parenthetical":parenthetical,"text":text})
                    }
                    Beat::Transition(text) => json!({"kind":"transition","text":text}),
                };
                let text = item["text"].as_str().unwrap_or_default();
                let n = match beat {
                    Beat::Dialogue { .. } => estimate::dialogue(text).words,
                    _ => estimate::words(text),
                };
                words += n;
                item["id"] = json!(script::beat_id(s, b));
                item["words"] = json!(n);
                item["estimated_seconds"] = json!(round(beat_seconds));
                item
            })
            .collect();
        scenes.push(json!({
            "id":format!("s{}", s + 1),
            "heading":scene.heading,
            "characters":speaking,
            "words":words,
            "estimated_seconds":round(seconds),
            "beats":items,
        }));
    }
    Ok(json!({
        "title":script.title,
        "scene_count":script.scenes.len(),
        "beat_count":script.beat_count(),
        "characters":characters.iter().map(|(n, l)| json!({"name":n,"lines":l})).collect::<Vec<_>>(),
        "estimated_seconds":round(total),
        "rates":rates_json(&rates),
        "from_scene":from,
        "next_scene":next,
        "scenes":scenes,
    }))
}

fn build(editor: &mut ProjectEditor, board: &Storyboard, args: &Value) -> Result<Value, String> {
    let after = insertion(args, editor.active_page())?;
    let breakdown: Breakdown =
        serde_json::from_value(args["breakdown"].clone()).map_err(|e| e.to_string())?;
    let rates = rates(args)?;
    let script = args["script"]
        .as_str()
        .map(|_| {
            let mut path = args.clone();
            path["path"] = args["script"].clone();
            absolute(&path).map(std::path::Path::to_path_buf)
        })
        .transpose()?
        .map(|path| read_script(&path))
        .transpose()?;
    let mut used = BTreeSet::new();
    for (s, scene) in breakdown.scenes.iter().enumerate() {
        for (p, panel) in scene.panels.iter().enumerate() {
            for id in &panel.source_beats {
                let known = match &script {
                    Some(script) => script.beat(id).is_some(),
                    None => is_beat_id(id),
                };
                if !known {
                    return Err(format!(
                        "Scene {} panel {}: beat {id} is not in the script.",
                        s + 1,
                        p + 1
                    ));
                }
                used.insert(id.clone());
            }
        }
    }
    let clip = breakdown.clip(board, &rates)?;
    let ids = editor.paste_panels(after, &clip)?;
    let board = editor.storyboard().unwrap();
    let panels: Vec<Value> = breakdown
        .scenes
        .iter()
        .flat_map(|s| &s.panels)
        .zip(&ids)
        .map(|(item, id)| {
            let frames = board.panels[id].frames;
            json!({
                "panel":id,
                "frames":frames,
                "seconds":seconds(board, u64::from(frames)),
                "source_beats":item.source_beats,
            })
        })
        .collect();
    let total: u64 = ids
        .iter()
        .map(|id| u64::from(board.panels[id].frames))
        .sum();
    let mut out = json!({
        "title":breakdown.title,
        "scenes":clip.scenes,
        "panels":panels,
        "total_seconds":seconds(board, total),
        "active_panel":editor.active_page(),
    });
    if let Some(script) = &script {
        let uncovered: Vec<String> = script
            .scenes
            .iter()
            .enumerate()
            .flat_map(|(s, scene)| {
                scene
                    .beats
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| !matches!(b, Beat::Transition(_)))
                    .map(move |(b, _)| script::beat_id(s, b))
            })
            .filter(|id| !used.contains(id))
            .collect();
        out["uncovered_beats"] = json!(uncovered);
    }
    Ok(out)
}

fn estimate_durations(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let layout = layout(editor);
    let scoped = ["panels", "scenes", "scene_names"]
        .iter()
        .any(|k| args.get(*k).is_some());
    let chosen: Vec<PageId> = if scoped {
        selection(board, &layout, args)?
    } else {
        board
            .playing(&layout)
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    };
    if chosen.is_empty() {
        return Err("The storyboard has no panels that play.".into());
    }
    let rates = rates(args)?;
    let found = estimate::estimate(board, &layout, &chosen, &rates);
    let dry_run = args["dry_run"] == true;
    let (ids, frames) = found.changes();
    let mut shortened = Vec::new();
    if !dry_run && !ids.is_empty() {
        editor.edit_storyboard(|b| {
            b.set_frames(&ids, &frames)?;
            shortened = fit_transitions(b);
            Ok(())
        })?;
    }
    let names: std::collections::HashMap<_, _> = editor
        .page_list()
        .iter()
        .map(|m| (m.id, m.name.clone()))
        .collect();
    let secs = |frames: u64| round(seconds(board, frames));
    let panels: Vec<Value> = found
        .panels
        .iter()
        .map(|p| {
            let mut item = json!({
                "panel":p.panel,
                "name":names.get(&p.panel),
                "scene":p.scene,
                "old_frames":p.old_frames,
                "old_seconds":secs(u64::from(p.old_frames)),
                "new_frames":p.new_frames,
                "new_seconds":secs(u64::from(p.new_frames)),
            });
            if let Some(kept) = p.kept {
                item["kept"] = json!(match kept {
                    Kept::Locked => "locked",
                    Kept::NoText => "no_text",
                });
                if let Some(e) = p.estimate {
                    item["estimate_seconds"] = json!(secs(u64::from(e)));
                }
            }
            item
        })
        .collect();
    let scenes: Vec<Value> = found
        .scenes
        .iter()
        .map(|s| json!({"scene":s.scene,"name":s.name,"old_seconds":secs(s.old_frames),"new_seconds":secs(s.new_frames)}))
        .collect();
    Ok(json!({
        "dry_run":dry_run,
        "changed":ids.len(),
        "applied":!dry_run && !ids.is_empty(),
        "panels":panels,
        "scenes":scenes,
        "old_seconds":secs(found.old_frames()),
        "new_seconds":secs(found.new_frames()),
        "transitions_shortened":shortened,
        "rates":rates_json(&rates),
    }))
}

#[cfg(test)]
mod tests {
    use super::super::execute;
    use super::super::tests::{board, call};
    use serde_json::json;
    use std::path::PathBuf;

    const SCRIPT: &str = "Title: The Storm

INT. KITCHEN - NIGHT

Rain on the window.

MIA
(quietly)
Is anyone there?

CUT TO:

EXT. GARDEN - DAWN

Birds scatter from the hedge.
";

    fn script(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sb-breakdown-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("storm.fountain");
        std::fs::write(&path, SCRIPT).unwrap();
        path
    }

    #[test]
    fn scripts_read_as_scenes_and_beats_with_stable_ids_in_pages() {
        let path = script("read");
        let mut e = board();
        let stamp = e.stamp();
        let path = path.to_str().unwrap();
        let out = call(&mut e, "read_storyboard_script", json!({"path":path}));
        assert_eq!(out["title"], "The Storm");
        assert_eq!(
            (out["scene_count"].as_u64(), out["beat_count"].as_u64()),
            (Some(2), Some(4))
        );
        assert_eq!(out["characters"], json!([{"name":"MIA","lines":1}]));
        let kitchen = &out["scenes"][0];
        assert_eq!(kitchen["id"], "s1");
        assert_eq!(kitchen["characters"], json!(["MIA"]));
        let line = &kitchen["beats"][1];
        assert_eq!(line["id"], "s1b2");
        assert_eq!(line["kind"], "dialogue");
        assert_eq!(line["parenthetical"], "(quietly)");
        assert_eq!(line["words"], 3);
        // 3 words at 150 wpm, a pause after the line and the parenthetical.
        assert_eq!(line["estimated_seconds"], 2.2);
        assert_eq!(kitchen["beats"][2]["kind"], "transition");
        assert!(out["next_scene"].is_null());
        // One scene per page.
        let page = call(
            &mut e,
            "read_storyboard_script",
            json!({"path":path,"max_scenes":1}),
        );
        assert_eq!(page["scenes"].as_array().unwrap().len(), 1);
        assert_eq!(page["next_scene"], 1);
        let rest = call(
            &mut e,
            "read_storyboard_script",
            json!({"path":path,"from_scene":1}),
        );
        assert_eq!(rest["scenes"][0]["beats"][0]["id"], "s2b1");
        // Faster speech, read-only, and absolute paths only.
        let fast = call(
            &mut e,
            "read_storyboard_script",
            json!({"path":path,"rates":{"dialogue_wpm":300,"line_pause":0,"parenthetical_pause":0}}),
        );
        assert_eq!(fast["scenes"][0]["beats"][1]["estimated_seconds"], 0.6);
        assert_eq!(e.stamp(), stamp);
        assert!(
            execute(
                &mut e,
                "read_storyboard_script",
                &json!({"path":"storm.fountain"})
            )
            .is_error
        );
        // No storyboard needed.
        let mut design = emulsion_core::project::ProjectEditor::new_project(
            emulsion_core::project::ProjectKind::Design,
            emulsion_core::Document::new(8, 8),
        )
        .unwrap();
        assert!(!execute(&mut design, "read_storyboard_script", &json!({"path":path})).is_error);
    }

    #[test]
    fn breakdowns_build_in_one_step_and_report_coverage() {
        let path = script("build");
        let path = path.to_str().unwrap();
        let mut e = board();
        let breakdown = json!({"title":"The Storm","scenes":[
            {"name":"INT. KITCHEN - NIGHT","panels":[
                {"captions":{"Slugging":"INT. KITCHEN - NIGHT","Action":"Rain on the window."},"size":"wide","seconds":3,"notes":"Establishing","source_beats":["s1b1"]},
                {"captions":{"Dialogue":"MIA (quietly): Is anyone there?"},"camera":"PUSH IN","size":"close_up","source_beats":["s1b2"]}
            ]}
        ]});
        let out = call(
            &mut e,
            "build_storyboard_from_breakdown",
            json!({"breakdown":breakdown,"script":path}),
        );
        let panels = out["panels"].as_array().unwrap();
        assert_eq!(panels.len(), 2);
        assert_eq!(panels[0]["seconds"], 3.);
        assert_eq!(panels[1]["source_beats"], json!(["s1b2"]));
        // Timed from the caption, as the script beat would be.
        let beat = emulsion_io::script::estimated_seconds(
            &emulsion_io::script::read(std::path::Path::new(path))
                .unwrap()
                .scenes[0]
                .beats[1],
            0.,
        );
        assert_eq!(panels[1]["seconds"], (beat * 24.).round() / 24.);
        assert_eq!(out["uncovered_beats"], json!(["s2b1"]));
        let board = e.storyboard().unwrap();
        let id = panels[1]["panel"].as_u64().unwrap();
        let camera = board.caption("Camera").unwrap();
        assert_eq!(board.panels[&id].captions[&camera].text, "PUSH IN");
        assert_eq!(e.page_list().len(), 3);
        assert!(e.undo());
        assert_eq!(e.page_list().len(), 1);
        assert!(e.storyboard().unwrap().caption("Camera").is_none());
        let stamp = e.stamp();
        // Invalid breakdowns change nothing.
        for args in [
            json!({"breakdown":{"scenes":[]}}),
            json!({"breakdown":{"scenes":[{"panels":[{"source_beats":["s9b9"]}]}]},"script":path}),
            json!({"breakdown":{"scenes":[{"panels":[{"source_beats":["beat 1"]}]}]}}),
            json!({"breakdown":{"scenes":[{"panels":[{"seconds":1,"frames":24}]}]}}),
            json!({"breakdown":{"scenes":[{"panels":[{"camera":"PAN","captions":{"Camera":"TILT"}}]}]}}),
            json!({"breakdown":{"scenes":[{"panels":[{"mood":"dark"}]}]}}),
            json!({"breakdown":{"scenes":[{"panels":[{}]}]},"rates":{"dialogue_wpm":5}}),
        ] {
            let result = execute(&mut e, "build_storyboard_from_breakdown", &args);
            assert!(result.is_error, "{args}");
            assert_eq!(e.stamp(), stamp, "{args}");
        }
        let error = execute(
            &mut e,
            "build_storyboard_from_breakdown",
            &json!({"breakdown":{"scenes":[{"panels":[{},{"seconds":1,"frames":24}]}]}}),
        );
        assert!(
            error.content[0]["text"]
                .as_str()
                .unwrap()
                .contains("Scene 1 panel 2")
        );
    }

    #[test]
    fn durations_estimate_as_a_dry_run_then_apply_in_one_step() {
        let mut e = board();
        let added = call(
            &mut e,
            "add_storyboard_panels",
            json!({"start":"scene","group_name":"Hall","panels":[
                {"captions":{"Action":"Mia runs to the window and looks out.","Dialogue":"MIA: Who is out there?"}},
                {"captions":{"Dialogue":"TOM: Nobody."}},
                {"captions":{"Notes":"Hold"}}
            ]}),
        );
        let ids: Vec<u64> = serde_json::from_value(added["panels"].clone()).unwrap();
        call(
            &mut e,
            "set_storyboard_locks",
            json!({"panels":[ids[1]],"locked":true}),
        );
        let stamp = e.stamp();
        let dry = call(
            &mut e,
            "estimate_storyboard_durations",
            json!({"scene_names":["Hall"],"dry_run":true}),
        );
        assert_eq!(e.stamp(), stamp);
        assert_eq!(dry["applied"], false);
        let panels = dry["panels"].as_array().unwrap();
        assert_eq!(panels.len(), 3);
        // 4 s of action, 1.6 s of speech and a 0.5 s pause.
        assert_eq!(panels[0]["new_seconds"], 6.08);
        assert_eq!(panels[1]["kept"], "locked");
        assert_eq!(panels[1]["estimate_seconds"], 1.0);
        assert_eq!(panels[2]["kept"], "no_text");
        assert_eq!(dry["scenes"][0]["name"], "Hall");
        assert_eq!(dry["changed"], 1);
        let applied = call(
            &mut e,
            "estimate_storyboard_durations",
            json!({"panels":[ids[0]],"rates":{"action_wpm":240}}),
        );
        assert_eq!(applied["applied"], true);
        // 2 s of action now.
        assert_eq!(e.storyboard().unwrap().panels[&ids[0]].frames, 98);
        assert!(e.undo());
        assert_eq!(e.storyboard().unwrap().panels[&ids[0]].frames, 48);
        assert!(
            execute(
                &mut e,
                "estimate_storyboard_durations",
                &json!({"rates":{"minimum_seconds":-1}})
            )
            .is_error
        );
    }
}
