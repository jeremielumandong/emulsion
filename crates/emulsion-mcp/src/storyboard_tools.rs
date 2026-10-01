//! Storyboard tools: read the outline, set the board's rules, add and time
//! panels, write captions and shot data, and start scenes, sequences and acts.
//! Board editing (locks, Smart add, moving, joining, renumbering, thumbnail
//! sheets and the panel clipboard) lives in `board`; caption fields,
//! formatting and find/replace in `captions`; stage guides, the palette and
//! importing pictures as panels or layers in `stage`; the project and
//! personal libraries and storyboard templates in `library`; PDF, image
//! and CSV exports in `export`. Drawing, duplicating ("next
//! frame") and deleting panels use the project and editing tools on the active
//! page. Every change is one Undo step in the live project.
mod board;
mod captions;
#[cfg(test)]
mod editing_tests;
mod export;
mod library;
mod stage;
#[cfg(test)]
mod stage_tests;

use crate::project_tools::validate_schema;
use crate::text_tools::{byte_to_char, style_json};
use crate::{ToolDef, ToolResult};
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::storyboard::{CaptionId, FrameRate, Level, Panel, Storyboard};
pub use library::{template_path, templates};
use serde_json::{Value, json};

pub const READ_ONLY: &[&str] = &[
    "describe_storyboard",
    "find_in_storyboard_captions",
    "list_storyboard_library",
    "list_storyboard_templates",
    "list_storyboard_pdf_profiles",
];
pub const DESTRUCTIVE: &[&str] = &[
    "remove_storyboard_caption_field",
    "remove_storyboard_library_item",
];
/// Most panels one call may add.
const MAX_BATCH: usize = 200;
const RATES: [&str; 9] = [
    "23.976", "24", "25", "29.97", "30", "48", "50", "59.94", "60",
];

fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    }
}

fn panel_id() -> Value {
    json!({"type":"integer","minimum":1,"description":"Panel (page) ID from describe_storyboard."})
}

fn panel_ids() -> Value {
    json!({"type":"array","items":panel_id(),"minItems":1,"maxItems":MAX_BATCH})
}

fn group_id() -> Value {
    json!({"type":"integer","minimum":1,"description":"Act, sequence or scene ID from describe_storyboard."})
}

/// Where new panels go: `after` a panel or `at_start`.
fn placement() -> Value {
    json!({
        "after":panel_id(),
        "at_start":{"type":"boolean","description":"Insert before the first panel instead of after `after`."}
    })
}

fn panel_fields() -> Value {
    json!({
        "frames":{"type":"integer","minimum":1,"maximum":emulsion_core::storyboard::MAX_PANEL_FRAMES,"description":"Duration in frames. Use this or seconds."},
        "seconds":{"type":"number","minimum":0.01,"maximum":600,"description":"Duration in seconds, rounded to whole frames."},
        "captions":{"type":"object","additionalProperties":{"type":"string","maxLength":16000},"description":"Caption text by field name from describe_storyboard (e.g. Action, Dialogue, Slugging, Notes). An empty string clears a field; new text replaces the field's formatting."},
        "size":{"enum":["unset","extreme_wide","wide","full","medium","medium_close","close_up","extreme_close","insert"],"description":"Shot size."},
        "angle":{"enum":["unset","eye","high","low","overhead","dutch","pov"],"description":"Camera angle."},
        "status":{"enum":["rough","clean","approved"]},
        "tag":{"type":"integer","minimum":0,"maximum":7,"description":"Colour tag index 0–7."},
        "clear_tag":{"type":"boolean"}
    })
}

fn naming_fields() -> Value {
    let prefix = json!({"type":"string","maxLength":160});
    let digits = json!({"type":"integer","minimum":0,"maximum":6,"description":"Pad numbers with zeros to this many digits."});
    json!({
        "type":"object",
        "additionalProperties":false,
        "description":"Naming rules used by renumbering, new scenes and new panels. Omitted rules stay.",
        "properties":{
            "scene_prefix":prefix,
            "scene_start":{"type":"integer","minimum":0,"maximum":1000000},
            "scene_step":{"type":"integer","minimum":1,"maximum":1000},
            "scene_digits":digits,
            "panel_prefix":prefix,
            "panel_digits":digits,
            "panels_per_scene":{"type":"boolean","description":"Panel numbers restart in every scene; false numbers them through the project."},
            "insert_letters":{"type":"boolean","description":"A scene started inside another takes its name plus a letter (10 → 10A)."}
        }
    })
}

pub fn definitions() -> Vec<ToolDef> {
    let level = json!({"enum":["scene","sequence","act"]});
    let name = json!({"type":"string","minLength":1,"maxLength":200});
    let mut new_panel = panel_fields();
    new_panel["name"] = name.clone();
    let mut add = placement();
    add["start"] = level.clone();
    add["group_name"] = name.clone();
    add["panels"] = json!({"type":"array","minItems":1,"maxItems":MAX_BATCH,"items":{"type":"object","properties":new_panel,"additionalProperties":false}});
    let mut defs = vec![
        def(
            "describe_storyboard",
            "Read the active storyboard: resolution, frame rate, naming rules, Smart add layers, stage `guides` (action/title safe %, field guide, overscan, with the safe-area, field and overscan `stage_area` rectangles in panel pixels), the colour `palette`, caption fields (with multiline and print flags), running time, the active panel and the outline of acts → sequences → scenes → panels with each panel's duration, captions (plain text, plus `formatting` ranges in characters when a caption has styled text), shot data and lock. Thumbnail sheets list their cell rectangles in pixels. Use describe_document on a selected panel to see its layers.",
            json!({}),
            &[],
        ),
        def(
            "set_storyboard_settings",
            "Set the storyboard frame rate, the default duration of new panels, the naming rules, the Smart add layer list, the Stage guides (action and title safe areas, field guide, overscan) and the colour palette. Existing panel durations stay in frames. One Undo step.",
            json!({
                "frame_rate":{"enum":RATES},
                "panel_frames":{"type":"integer","minimum":1,"maximum":emulsion_core::storyboard::MAX_PANEL_FRAMES},
                "naming":naming_fields(),
                "smart_add_layers":{"type":"array","items":{"type":"string","maxLength":800},"minItems":0,"maxItems":64,"description":"Top-level layer names (case-insensitive) that smart_add_storyboard_panel copies into the next panel, such as Background or Set. An empty list clears it."},
                "guides":stage::guide_fields(),
                "palette":stage::palette_fields()
            }),
            &[],
        ),
        def(
            "add_storyboard_panels",
            "Add blank panels (white background layer at the project resolution) after a panel (default: the active panel), or at the start, each with optional duration, captions and shot data. Unnamed panels are named by the naming rules. `start` begins a new scene, sequence or act with the first new panel, named `group_name`. The first new panel becomes active, ready for drawing tools. Returns the new panel IDs. One Undo step for the whole batch.",
            add,
            &["panels"],
        ),
        def(
            "update_storyboard_panel",
            "Change one panel's duration, captions or shot data. Captions are merged by field name; an empty string clears a field. Locked panels refuse changes. One Undo step.",
            {
                let mut fields = panel_fields();
                fields["panel"] = panel_id();
                fields
            },
            &["panel"],
        ),
        def(
            "start_storyboard_group",
            "Split: start a new scene, sequence or act at a panel; that panel and the rest of its enclosing group move into the new group. Splitting a sequence or act mid-scene also starts a scene there. Returns the new group ID. One Undo step.",
            json!({"panel":panel_id(),"level":level,"name":name}),
            &["panel", "level"],
        ),
        def(
            "rename_storyboard_group",
            "Rename an act, sequence or scene by its group ID from describe_storyboard. One Undo step.",
            json!({"group":group_id(),"name":name}),
            &["group", "name"],
        ),
    ];
    defs.extend(board::definitions());
    defs.extend(captions::definitions());
    defs.extend(stage::definitions());
    defs.extend(library::definitions());
    defs.extend(export::definitions());
    defs
}

pub fn is_tool(name: &str) -> bool {
    definitions().iter().any(|d| d.name == name)
}

pub fn validate_args(name: &str, args: &Value) -> Result<(), String> {
    let def = definitions()
        .into_iter()
        .find(|d| d.name == name)
        .ok_or("Unknown storyboard tool")?;
    validate_schema(&def.input_schema, args)
}

pub fn execute(editor: &mut ProjectEditor, name: &str, args: &Value) -> ToolResult {
    match run(editor, name, args) {
        Ok(value) => ToolResult::text(value.to_string()),
        Err(error) => ToolResult::error(error),
    }
}

fn level(value: &Value) -> Level {
    match value.as_str() {
        Some("act") => Level::Act,
        Some("sequence") => Level::Sequence,
        _ => Level::Scene,
    }
}

fn rate(label: &str) -> FrameRate {
    match label {
        "23.976" => FrameRate::ntsc(24),
        "29.97" => FrameRate::ntsc(30),
        "59.94" => FrameRate::ntsc(60),
        whole => FrameRate::whole(whole.parse().unwrap_or(24)),
    }
}

fn rate_label(rate: FrameRate) -> String {
    RATES
        .iter()
        .find(|label| self::rate(label) == rate)
        .map_or_else(|| format!("{:.3}", rate.fps()), |label| label.to_string())
}

fn layout(editor: &ProjectEditor) -> Vec<PageId> {
    editor.page_list().iter().map(|m| m.id).collect()
}

/// The positive IDs in an optional array argument.
fn ids(value: &Value) -> Vec<u64> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .collect()
}

/// The panel new panels go after: `after`, nothing for `at_start`, otherwise
/// `default`.
fn insertion(args: &Value, default: PageId) -> Result<Option<PageId>, String> {
    match (args["at_start"] == true, args["after"].as_u64()) {
        (true, Some(_)) => Err("Use either after or at_start".into()),
        (true, None) => Ok(None),
        (false, after) => Ok(Some(after.unwrap_or(default))),
    }
}

/// A caption field by name, case-insensitive.
fn field(board: &Storyboard, name: &str) -> Result<CaptionId, String> {
    board.caption(name).ok_or_else(|| {
        let names: Vec<_> = board.captions.iter().map(|c| c.name.as_str()).collect();
        format!(
            "Unknown caption field '{name}'. Fields: {}",
            names.join(", ")
        )
    })
}

/// Apply validated panel fields from `args` onto `panel`.
fn apply(board: &Storyboard, panel: &mut Panel, args: &Value) -> Result<(), String> {
    if let Some(frames) = args["frames"].as_u64() {
        panel.frames = frames as u32;
    }
    if let Some(seconds) = args["seconds"].as_f64() {
        panel.frames = (seconds * board.settings.frame_rate.fps()).round().max(1.) as u32;
    }
    if let Some(captions) = args["captions"].as_object() {
        for (name, text) in captions {
            let id = field(board, name)?;
            match text.as_str().unwrap_or_default() {
                "" => {
                    panel.captions.remove(&id);
                }
                // Unchanged text keeps its formatting.
                text if panel.captions.get(&id).is_some_and(|c| c.text == text) => {}
                text => {
                    panel.captions.insert(id, text.into());
                }
            }
        }
    }
    for key in ["size", "angle", "status"] {
        if let Some(value) = args.get(key) {
            match key {
                "size" => {
                    panel.size = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?
                }
                "angle" => {
                    panel.angle =
                        serde_json::from_value(value.clone()).map_err(|e| e.to_string())?
                }
                _ => {
                    panel.status =
                        serde_json::from_value(value.clone()).map_err(|e| e.to_string())?
                }
            }
        }
    }
    if let Some(tag) = args["tag"].as_u64() {
        panel.tag = Some(tag as u8);
    }
    if args["clear_tag"] == true {
        panel.tag = None;
    }
    Ok(())
}

fn describe(editor: &ProjectEditor, board: &Storyboard) -> Value {
    let layout = layout(editor);
    let fps = board.settings.frame_rate.fps();
    let captions = |panel: &Panel| -> Value {
        board
            .captions
            .iter()
            .filter_map(|field| {
                panel
                    .captions
                    .get(&field.id)
                    .map(|caption| (field.name.clone(), json!(caption.text)))
            })
            .collect::<serde_json::Map<_, _>>()
            .into()
    };
    // Styled ranges in characters, only for captions that have any.
    let formatting = |panel: &Panel| -> serde_json::Map<String, Value> {
        board
            .captions
            .iter()
            .filter_map(|field| {
                let caption = panel.captions.get(&field.id)?;
                let runs: Vec<_> = caption
                    .runs
                    .iter()
                    .map(|run| {
                        json!({
                            "start":byte_to_char(&caption.text, run.start),
                            "end":byte_to_char(&caption.text, run.end),
                            "style":style_json(&run.style),
                        })
                    })
                    .collect();
                (!runs.is_empty()).then(|| (field.name.clone(), runs.into()))
            })
            .collect()
    };
    let mut acts: Vec<Value> = Vec::new();
    for scene in board.outline(&layout) {
        if acts.last().is_none_or(|a| a["id"] != scene.act) {
            acts.push(json!({"id":scene.act,"name":board.acts[&scene.act].name,"sequences":[]}));
        }
        let sequences = acts.last_mut().unwrap()["sequences"]
            .as_array_mut()
            .unwrap();
        if sequences.last().is_none_or(|s| s["id"] != scene.sequence) {
            sequences.push(json!({"id":scene.sequence,"name":board.sequences[&scene.sequence].name,"scenes":[]}));
        }
        let panels: Vec<Value> = scene
            .panels
            .iter()
            .map(|id| {
                let panel = &board.panels[id];
                let meta = editor.page_list().iter().find(|m| m.id == *id).unwrap();
                let mut entry = json!({
                    "panel":id,
                    "name":meta.name,
                    "frames":panel.frames,
                    "seconds":f64::from(panel.frames) / fps,
                    "captions":captions(panel),
                    "size":panel.size,
                    "angle":panel.angle,
                    "status":panel.status,
                    "tag":panel.tag,
                    "locked":panel.locked,
                    "layers":editor.page(*id).map_or(0, |e| e.doc.nodes.len()),
                });
                let formatting = formatting(panel);
                if !formatting.is_empty() {
                    entry["formatting"] = formatting.into();
                }
                if let Some(grid) = panel.thumbnails {
                    entry["thumbnail_sheet"] = board::sheet_json(board, grid);
                }
                entry
            })
            .collect();
        let group = &board.scenes[&scene.scene];
        sequences.last_mut().unwrap()["scenes"]
            .as_array_mut()
            .unwrap()
            .push(
                json!({"id":scene.scene,"name":group.name,"locked":group.locked,"panels":panels}),
            );
    }
    json!({
        "width":board.settings.width,
        "height":board.settings.height,
        "frame_rate":rate_label(board.settings.frame_rate),
        "panel_frames":board.settings.panel_frames,
        "naming":board.naming,
        "smart_add_layers":board.smart_add_layers,
        "guides":stage::guides_json(board),
        "palette":stage::palette_json(board),
        "caption_fields":board.captions,
        "total_frames":board.total_frames(),
        "total_seconds":board.total_frames() as f64 / fps,
        "active_panel":editor.active_page(),
        "can_undo":editor.can_undo(),
        "acts":acts,
    })
}

fn run(editor: &mut ProjectEditor, name: &str, args: &Value) -> Result<Value, String> {
    validate_args(name, args)?;
    let board = editor
        .storyboard()
        .ok_or("Open a Storyboard project first (create_design_project with kind \"storyboard\").")?
        .clone();
    if name == "describe_storyboard" {
        return Ok(describe(editor, &board));
    }
    if !READ_ONLY.contains(&name) && editor.in_transaction() {
        return Err("Finish the current edit before changing the storyboard".into());
    }
    match name {
        "set_storyboard_settings" => {
            editor.edit_storyboard(|b| {
                if let Some(label) = args["frame_rate"].as_str() {
                    b.settings.frame_rate = rate(label);
                }
                if let Some(frames) = args["panel_frames"].as_u64() {
                    b.settings.panel_frames = frames as u32;
                }
                if let Some(rules) = args["naming"].as_object() {
                    let mut naming = serde_json::to_value(&b.naming).map_err(|e| e.to_string())?;
                    for (key, value) in rules {
                        naming[key] = value.clone();
                    }
                    b.naming = serde_json::from_value(naming).map_err(|e| e.to_string())?;
                }
                if let Some(layers) = args["smart_add_layers"].as_array() {
                    b.smart_add_layers = layers
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|name| name.trim().to_string())
                        .collect();
                }
                stage::apply_settings(b, args)
            })?;
            let board = editor.storyboard().unwrap();
            Ok(json!({
                "frame_rate":rate_label(board.settings.frame_rate),
                "naming":board.naming,
                "smart_add_layers":board.smart_add_layers,
                "guides":stage::guides_json(board),
                "palette":stage::palette_json(board),
            }))
        }
        "add_storyboard_panels" => {
            let after = insertion(args, editor.active_page())?;
            let items = args["panels"].as_array().unwrap();
            let mut panels = Vec::new();
            for item in items {
                let mut panel = Panel::new(0, board.settings.panel_frames);
                apply(&board, &mut panel, item)?;
                let name = item["name"].as_str().map_or_else(
                    || {
                        board
                            .naming
                            .panel_name(editor.page_list().len() + panels.len() + 1)
                    },
                    str::to_string,
                );
                panels.push((name, panel));
            }
            let start = args
                .get("start")
                .map(|value| (level(value), args["group_name"].as_str()));
            if start.is_none() && args.get("group_name").is_some() {
                return Err("group_name needs start".into());
            }
            let ids = editor.insert_panels(after, &board.blank_panel()?, panels, start)?;
            Ok(json!({"panels":ids,"active_panel":editor.active_page()}))
        }
        "update_storyboard_panel" => {
            let id = args["panel"].as_u64().unwrap();
            editor.edit_storyboard(|b| {
                let mut panel = b.panels.get(&id).ok_or("Panel does not exist")?.clone();
                apply(b, &mut panel, args)?;
                b.panels.insert(id, panel);
                Ok(())
            })?;
            let panel = &editor.storyboard().unwrap().panels[&id];
            Ok(json!({"panel":id,"frames":panel.frames}))
        }
        "start_storyboard_group" => {
            let layout = layout(editor);
            let mut group = 0;
            editor.edit_storyboard(|b| {
                group = b.split(
                    &layout,
                    args["panel"].as_u64().unwrap(),
                    level(&args["level"]),
                    args["name"].as_str(),
                )?;
                Ok(())
            })?;
            Ok(json!({"group":group}))
        }
        "rename_storyboard_group" => {
            editor.edit_storyboard(|b| {
                b.rename(
                    args["group"].as_u64().unwrap(),
                    args["name"].as_str().unwrap(),
                )
            })?;
            Ok(json!({"group":args["group"]}))
        }
        _ => board::run(editor, &board, name, args)
            .or_else(|| captions::run(editor, &board, name, args))
            .or_else(|| stage::run(editor, &board, name, args))
            .or_else(|| library::run(editor, &board, name, args))
            .or_else(|| export::run(editor, &board, name, args))
            .unwrap_or_else(|| Err("Unknown storyboard tool".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Document, project::ProjectKind};

    pub(super) fn call(editor: &mut ProjectEditor, name: &str, args: Value) -> Value {
        let result = execute(editor, name, &args);
        assert!(!result.is_error, "{name}: {}", result.content[0]["text"]);
        serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap()
    }

    pub(super) fn board() -> ProjectEditor {
        ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap()
    }

    #[test]
    fn a_scenario_becomes_scenes_panels_captions_and_timing() {
        let mut e = board();
        call(
            &mut e,
            "set_storyboard_settings",
            json!({"frame_rate":"25"}),
        );
        let added = call(
            &mut e,
            "add_storyboard_panels",
            json!({"after":1,"start":"scene","group_name":"2 · Kitchen","panels":[
                {"seconds":2,"size":"wide","captions":{"Action":"Mia enters","Slugging":"INT. KITCHEN - DAY"}},
                {"frames":12,"size":"close_up","angle":"low","captions":{"dialogue":"Who's there?"}}
            ]}),
        );
        let ids: Vec<u64> = serde_json::from_value(added["panels"].clone()).unwrap();
        assert_eq!(added["active_panel"], ids[0]);
        let outline = call(&mut e, "describe_storyboard", json!({}));
        assert_eq!(outline["frame_rate"], "25");
        let scenes = &outline["acts"][0]["sequences"][0]["scenes"];
        assert_eq!(scenes[1]["name"], "2 · Kitchen");
        let first = &scenes[1]["panels"][0];
        assert_eq!(first["frames"], 50);
        assert_eq!(first["size"], "wide");
        assert_eq!(first["captions"]["Slugging"], "INT. KITCHEN - DAY");
        assert_eq!(first["layers"], 1);
        assert_eq!(
            scenes[1]["panels"][1]["captions"]["Dialogue"],
            "Who's there?"
        );
        assert_eq!(outline["total_frames"], 48 + 50 + 12);
        // One Undo step removes the whole batch.
        assert!(e.undo());
        assert_eq!(e.page_list().len(), 1);
    }

    #[test]
    fn panels_update_and_groups_split_and_rename_as_single_steps() {
        let mut e = board();
        call(
            &mut e,
            "add_storyboard_panels",
            json!({"panels":[{},{},{}]}),
        );
        call(
            &mut e,
            "update_storyboard_panel",
            json!({"panel":1,"captions":{"Notes":"Keep eyeline"},"tag":3,"status":"clean"}),
        );
        call(
            &mut e,
            "update_storyboard_panel",
            json!({"panel":1,"captions":{"Notes":""},"clear_tag":true}),
        );
        let panel = &e.storyboard().unwrap().panels[&1];
        assert!(panel.captions.is_empty() && panel.tag.is_none());
        let layout: Vec<_> = e.page_list().iter().map(|m| m.id).collect();
        let group = call(
            &mut e,
            "start_storyboard_group",
            json!({"panel":layout[2],"level":"sequence","name":"Chase"}),
        )["group"]
            .as_u64()
            .unwrap();
        call(
            &mut e,
            "rename_storyboard_group",
            json!({"group":group,"name":"The chase"}),
        );
        let outline = call(&mut e, "describe_storyboard", json!({}));
        assert_eq!(outline["acts"][0]["sequences"][1]["name"], "The chase");
        assert!(e.undo());
        assert_eq!(e.storyboard().unwrap().sequences[&group].name, "Chase");
    }

    #[test]
    fn next_frames_copy_and_move_characters_between_panels() {
        let mut e = board();
        let drawn = crate::exec::execute(
            &mut e,
            "draw_shape",
            &json!({"shape":"ellipse","name":"Hero","x":10,"y":8,"width":12,"height":20,"mode":"shape"}),
        );
        assert!(!drawn.is_error, "{}", drawn.content[0]["text"]);
        let hero = e.doc.nodes.iter().find(|n| n.name == "Hero").unwrap().id;
        call(
            &mut e,
            "update_storyboard_panel",
            json!({"panel":1,"captions":{"Action":"Hero waits"}}),
        );
        // Next frame: duplicate the panel, then move the character in the copy.
        let project = |e: &mut ProjectEditor, name: &str, args: Value| {
            let result = crate::project_tools::execute(e, name, &args);
            assert!(!result.is_error, "{name}: {}", result.content[0]["text"]);
        };
        project(&mut e, "duplicate_project_page", json!({"page":1}));
        let next = e.active_page();
        assert_ne!(next, 1);
        let moved = crate::exec::execute(
            &mut e,
            "translate_node",
            &json!({"node":hero,"dx":10,"dy":0}),
        );
        assert!(!moved.is_error, "{}", moved.content[0]["text"]);
        let board = e.storyboard().unwrap();
        assert_eq!(board.panels[&next], board.panels[&1]);
        // Carry the character into a fresh panel.
        let added = call(
            &mut e,
            "add_storyboard_panels",
            json!({"after":next,"panels":[{"size":"close_up"}]}),
        );
        let fresh = added["panels"][0].as_u64().unwrap();
        project(
            &mut e,
            "copy_page_nodes",
            json!({"from":1,"nodes":[hero],"to":fresh,"dx":5,"dy":5}),
        );
        assert_eq!(e.active_page(), fresh);
        assert!(e.doc.nodes.iter().any(|n| n.name == "Hero"));
        let order: Vec<_> = e.page_list().iter().map(|m| m.id).collect();
        assert_eq!(order, [1, next, fresh]);
        e.snapshot().unwrap().validate().unwrap();
    }

    #[test]
    fn invalid_calls_change_nothing() {
        let mut e = board();
        let stamp = e.stamp();
        for (name, args) in [
            ("add_storyboard_panels", json!({"panels":[]})),
            (
                "add_storyboard_panels",
                json!({"panels":[{"captions":{"Mood":"x"}}]}),
            ),
            ("add_storyboard_panels", json!({"panels":[{"size":"huge"}]})),
            (
                "add_storyboard_panels",
                json!({"panels":[{}],"after":1,"at_start":true}),
            ),
            (
                "add_storyboard_panels",
                json!({"panels":[{}],"group_name":"x"}),
            ),
            ("add_storyboard_panels", json!({"panels":[{"extra":1}]})),
            ("update_storyboard_panel", json!({"panel":99})),
            ("update_storyboard_panel", json!({"panel":1,"frames":0})),
            ("start_storyboard_group", json!({"panel":1,"level":"scene"})),
            ("rename_storyboard_group", json!({"group":1,"name":"x"})),
            ("set_storyboard_settings", json!({"frame_rate":"12"})),
        ] {
            assert!(execute(&mut e, name, &args).is_error, "{name} {args}");
            assert_eq!(e.stamp(), stamp, "{name}");
        }
        let mut design =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(8, 8)).unwrap();
        assert!(execute(&mut design, "describe_storyboard", &json!({})).is_error);
    }

    #[test]
    fn every_tool_is_registered_once_and_read_only_tools_are_known() {
        let names: Vec<_> = crate::tools::definitions()
            .into_iter()
            .map(|d| d.name)
            .collect();
        for def in definitions() {
            assert_eq!(
                names.iter().filter(|n| **n == def.name).count(),
                1,
                "{}",
                def.name
            );
            assert_eq!(
                crate::tools::is_read_only(&def.name),
                READ_ONLY.contains(&def.name.as_str())
            );
            assert_eq!(
                crate::tools::uses_native_history(&def.name),
                !READ_ONLY.contains(&def.name.as_str())
            );
        }
    }
}
