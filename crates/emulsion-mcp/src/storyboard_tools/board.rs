//! Board editing tools: locks, Smart add, moving panels, joining and
//! renumbering groups, thumbnail sheets and the panel clipboard.
use super::{def, group_id, ids, insertion, layout, panel_id, panel_ids, placement};
use crate::ToolDef;
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::storyboard::{GroupId, RenumberScope, Storyboard, ThumbnailGrid};
use serde_json::{Value, json};
use std::collections::HashSet;

pub(super) fn definitions() -> Vec<ToolDef> {
    let flag = json!({"type":"boolean"});
    let scene_ids = json!({"type":"array","items":group_id(),"minItems":1,"maxItems":200});
    let sides = ThumbnailGrid::MAX_CELLS_PER_SIDE;
    let mut copy = placement();
    copy["panels"] = panel_ids();
    copy["scenes"] = scene_ids.clone();
    copy["new_scenes"] = json!({"type":"boolean","description":"Whole scenes come back as new scenes (default true); false puts every panel in the scene it lands in."});
    let mut import = copy.clone();
    import.as_object_mut().unwrap().remove("panels");
    import["path"] = json!({"type":"string","minLength":1,"description":"Absolute path of a saved storyboard .emu file."});
    import["scenes"] = json!({"type":"array","items":{"type":"string","maxLength":800},"minItems":1,"maxItems":200,"description":"Scene names in that file (case-insensitive). Omit to import every panel."});
    vec![
        def(
            "set_storyboard_locks",
            "Lock or unlock panels and whole scenes. Locked panels refuse drawing, data changes, moving and removal until unlocked; a locked scene protects every panel in it. Lock approved panels so later edits cannot touch them. One Undo step.",
            json!({"panels":panel_ids(),"scenes":scene_ids,"locked":flag}),
            &["locked"],
        ),
        def(
            "smart_add_storyboard_panel",
            "Smart add: add the next panel after a panel (default: the active panel), in its scene, with its shot size and angle and copies of its top-level layers named in the Smart add list (set_storyboard_settings smart_add_layers), so the set or background carries over. The new panel becomes active. One Undo step.",
            json!({"after":panel_id()}),
            &[],
        ),
        def(
            "move_storyboard_panels",
            "Move panels, keeping their page order, after a panel (or at_start), across scene boundaries. With `scene` (an ID) or `scene_name`, they join that scene, which must be at or next to where they land; with only a scene they go to its end. Otherwise they join the scene they land in. One Undo step.",
            {
                let mut fields = placement();
                fields["panels"] = panel_ids();
                fields["scene"] = group_id();
                fields["scene_name"] = json!({"type":"string","minLength":1,"maxLength":800});
                fields
            },
            &["panels"],
        ),
        def(
            "join_storyboard_group",
            "Join a scene, sequence or act into the one of the same level just before it; the group disappears and its panels (or scenes, or sequences) join the earlier one. Returns the absorbing group ID. The opposite of start_storyboard_group. One Undo step.",
            json!({"group":group_id()}),
            &["group"],
        ),
        def(
            "renumber_storyboard",
            "Rename scenes and/or panels by the naming rules, for the whole board or only inside chosen acts, sequences or scenes (numbers still count from the start of the board). Locked scenes and panels keep their names. Returns how many names changed. One Undo step.",
            json!({
                "groups":{"type":"array","items":group_id(),"minItems":1,"maxItems":200},
                "scenes":{"type":"boolean","description":"Rename scenes (default true)."},
                "panels":{"type":"boolean","description":"Rename panels (default true)."}
            }),
            &[],
        ),
        def(
            "set_storyboard_thumbnail_sheet",
            "Make a panel a thumbnail sheet: a grid of small camera frames for roughing a sequence on one page, or `clear` it. Sheets do not count towards running time. Returns the cell rectangles (pixels, row order) to draw a thumbnail in each with the drawing tools, then convert_storyboard_thumbnails. One Undo step.",
            json!({
                "panel":panel_id(),
                "columns":{"type":"integer","minimum":1,"maximum":sides},
                "rows":{"type":"integer","minimum":1,"maximum":sides},
                "gap":{"type":"integer","minimum":0,"maximum":10000,"description":"Pixels between cells (default 24)."},
                "margin":{"type":"integer","minimum":0,"maximum":10000,"description":"Pixels around the grid (default 32)."},
                "clear":flag
            }),
            &["panel"],
        ),
        def(
            "convert_storyboard_thumbnails",
            "Convert a thumbnail sheet into one panel per cell, in row order, in the sheet's scene and place; each cell is cropped to its camera frame and scaled to the project resolution with its layers kept editable. The sheet is removed. Returns the new panel IDs. One Undo step.",
            json!({"panel":panel_id()}),
            &["panel"],
        ),
        def(
            "copy_storyboard_panels",
            "Copy panels and/or whole scenes with their layers, timing, captions and shot data, and paste the copies after a panel (default: the last copied panel) or at_start. Copied whole scenes come back as new scenes after the scene they land in. Copies are unlocked. Returns the new panel IDs. One Undo step.",
            copy,
            &[],
        ),
        def(
            "import_storyboard_panels",
            "Import panels from another saved storyboard .emu file, by scene name or all of them, after a panel (default: the active panel) or at_start. Whole scenes come back as new scenes; caption fields are matched by name (missing ones are added), durations keep their time at this frame rate and other resolutions are cropped to the centre and scaled. Returns the new panel IDs. One Undo step.",
            import,
            &["path"],
        ),
    ]
}

/// A thumbnail sheet with its camera frames, for describe_storyboard.
pub(super) fn sheet_json(board: &Storyboard, grid: ThumbnailGrid) -> Value {
    let cells: Vec<_> = grid
        .cells(board.settings.width, board.settings.height)
        .into_iter()
        .map(|c| json!({"x":c.x,"y":c.y,"width":c.w,"height":c.h}))
        .collect();
    json!({"columns":grid.columns,"rows":grid.rows,"gap":grid.gap,"margin":grid.margin,"cells":cells})
}

/// Scenes with this name, ignoring case and surrounding spaces.
fn scenes_named(board: &Storyboard, name: &str) -> Vec<GroupId> {
    board
        .scenes
        .iter()
        .filter(|(_, s)| s.name.trim().eq_ignore_ascii_case(name.trim()))
        .map(|(id, _)| *id)
        .collect()
}

/// A scene by ID or by name.
fn scene(board: &Storyboard, args: &Value) -> Result<Option<GroupId>, String> {
    match (args["scene"].as_u64(), args["scene_name"].as_str()) {
        (Some(_), Some(_)) => Err("Use either scene or scene_name".into()),
        (Some(id), None) if board.scenes.contains_key(&id) => Ok(Some(id)),
        (Some(_), None) => Err("No scene has that ID.".into()),
        (None, Some(name)) => {
            let found = scenes_named(board, name);
            match found[..] {
                [id] => Ok(Some(id)),
                [] => Err(format!("No scene is named '{name}'.")),
                _ => Err(format!("Several scenes are named '{name}'; use its ID.")),
            }
        }
        (None, None) => Ok(None),
    }
}

/// Chosen panels plus every panel of the chosen scenes, in page order.
fn selection(board: &Storyboard, layout: &[PageId], args: &Value) -> Result<Vec<PageId>, String> {
    let panels: HashSet<_> = ids(&args["panels"]).into_iter().collect();
    let scenes: HashSet<_> = ids(&args["scenes"]).into_iter().collect();
    if let Some(id) = panels.iter().find(|id| !board.panels.contains_key(id)) {
        return Err(format!("Panel {id} does not exist."));
    }
    if let Some(id) = scenes.iter().find(|id| !board.scenes.contains_key(id)) {
        return Err(format!("No scene has ID {id}."));
    }
    let chosen: Vec<_> = layout
        .iter()
        .copied()
        .filter(|id| panels.contains(id) || scenes.contains(&board.panels[id].scene))
        .collect();
    if chosen.is_empty() {
        return Err("Choose panels or scenes.".into());
    }
    Ok(chosen)
}

/// Run a board tool; `None` when `name` is not one.
pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let result = match name {
        "set_storyboard_locks" => set_locks(editor, board, args),
        "smart_add_storyboard_panel" => {
            let after = args["after"].as_u64().unwrap_or(editor.active_page());
            editor.smart_add_panel(after).map(|id| {
                let wanted: HashSet<_> = board
                    .smart_add_layers
                    .iter()
                    .map(|n| n.to_lowercase())
                    .collect();
                let carried: Vec<_> = editor.page(id).map_or(Vec::new(), |page| {
                    page.doc
                        .nodes
                        .iter()
                        .filter(|n| {
                            n.parent.is_none() && wanted.contains(&n.name.trim().to_lowercase())
                        })
                        .map(|n| n.name.clone())
                        .collect()
                });
                json!({"panel":id,"carried_layers":carried,"active_panel":editor.active_page()})
            })
        }
        "move_storyboard_panels" => move_panels(editor, board, args),
        "join_storyboard_group" => {
            let group = args["group"].as_u64().unwrap();
            if board.scenes.get(&group).is_some_and(|s| s.locked) {
                return Some(Err("That scene is locked. Unlock it before joining.".into()));
            }
            let layout = layout(editor);
            let mut into = 0;
            editor
                .edit_storyboard(|b| {
                    into = b.join(&layout, group)?;
                    Ok(())
                })
                .map(|()| json!({"group":into}))
        }
        "renumber_storyboard" => {
            let groups = ids(&args["groups"]);
            let scope = if groups.is_empty() {
                RenumberScope::All
            } else {
                RenumberScope::Groups(groups)
            };
            editor
                .renumber(
                    &scope,
                    args["scenes"].as_bool().unwrap_or(true),
                    args["panels"].as_bool().unwrap_or(true),
                )
                .map(|renamed| json!({"renamed":renamed}))
        }
        "set_storyboard_thumbnail_sheet" => set_sheet(editor, board, args),
        "convert_storyboard_thumbnails" => editor
            .convert_thumbnails(args["panel"].as_u64().unwrap())
            .map(|ids| json!({"panels":ids,"active_panel":editor.active_page()})),
        "copy_storyboard_panels" => (|| {
            let chosen = selection(board, &layout(editor), args)?;
            let mut clip = editor.copy_panels(&chosen)?;
            clip.whole_scenes &= args["new_scenes"] != false;
            let after = insertion(args, *chosen.last().unwrap())?;
            let pasted = editor.paste_panels(after, &clip)?;
            Ok(json!({"panels":pasted,"active_panel":editor.active_page()}))
        })(),
        "import_storyboard_panels" => import(editor, args),
        _ => return None,
    };
    Some(result)
}

fn set_locks(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let locked = args["locked"].as_bool().unwrap();
    let (panels, scenes) = (ids(&args["panels"]), ids(&args["scenes"]));
    if panels.is_empty() && scenes.is_empty() {
        return Err("Choose panels or scenes to lock or unlock.".into());
    }
    for id in &panels {
        let panel = board
            .panels
            .get(id)
            .ok_or_else(|| format!("Panel {id} does not exist."))?;
        if panel.locked != locked && board.scenes[&panel.scene].locked {
            return Err(format!(
                "Panel {id} is in a locked scene; unlock the scene first."
            ));
        }
    }
    editor.edit_storyboard(|b| {
        for id in &scenes {
            b.scenes
                .get_mut(id)
                .ok_or_else(|| format!("No scene has ID {id}."))?
                .locked = locked;
        }
        for id in &panels {
            b.panels.get_mut(id).unwrap().locked = locked;
        }
        Ok(())
    })?;
    Ok(json!({"panels":panels,"scenes":scenes,"locked":locked}))
}

fn move_panels(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let moving = ids(&args["panels"]);
    let scene = scene(board, args)?;
    if let Some(id) = moving.iter().find(|id| board.is_locked(**id)) {
        return Err(format!("Panel {id} is locked. Unlock it to move it."));
    }
    let rest: Vec<_> = layout(editor)
        .into_iter()
        .filter(|id| !moving.contains(id))
        .collect();
    let after = match (args["at_start"] == true, args["after"].as_u64(), scene) {
        (true, Some(_), _) => return Err("Use either after or at_start".into()),
        (true, None, _) => None,
        (false, Some(after), _) => Some(after),
        // Only a scene: go to its end.
        (false, None, Some(scene)) => Some(
            *rest
                .iter()
                .rfind(|id| board.panels[id].scene == scene)
                .ok_or("Give after or at_start: the scene has no other panels.")?,
        ),
        (false, None, None) => return Err("Give after, at_start or a scene.".into()),
    };
    let to = match after {
        None => 0,
        Some(after) if moving.contains(&after) => {
            return Err("after must be a panel that is not moving.".into());
        }
        Some(after) => {
            rest.iter()
                .position(|id| *id == after)
                .ok_or("Panel does not exist.")?
                + 1
        }
    };
    editor.move_panels(&moving, to, scene)?;
    let board = editor.storyboard().unwrap();
    let scenes: Vec<_> = moving.iter().map(|id| board.panels[id].scene).collect();
    Ok(json!({"panels":moving,"scenes":scenes}))
}

fn set_sheet(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let id = args["panel"].as_u64().unwrap();
    let grid = if args["clear"] == true {
        if ["columns", "rows", "gap", "margin"]
            .iter()
            .any(|key| args.get(key).is_some())
        {
            return Err("clear takes no grid settings".into());
        }
        None
    } else {
        let (Some(columns), Some(rows)) = (args["columns"].as_u64(), args["rows"].as_u64()) else {
            return Err("Give columns and rows, or clear".into());
        };
        let defaults = ThumbnailGrid::new(columns as u8, rows as u8);
        Some(ThumbnailGrid {
            gap: args["gap"].as_u64().map_or(defaults.gap, |v| v as u32),
            margin: args["margin"]
                .as_u64()
                .map_or(defaults.margin, |v| v as u32),
            ..defaults
        })
    };
    editor.edit_storyboard(|b| {
        b.panels
            .get_mut(&id)
            .ok_or("Panel does not exist.")?
            .thumbnails = grid;
        Ok(())
    })?;
    Ok(match grid {
        Some(grid) => json!({"panel":id,"thumbnail_sheet":sheet_json(board, grid)}),
        None => json!({"panel":id,"thumbnail_sheet":null}),
    })
}

fn import(editor: &mut ProjectEditor, args: &Value) -> Result<Value, String> {
    let path = std::path::Path::new(args["path"].as_str().unwrap());
    if !path.is_absolute() {
        return Err("Use an absolute path to a storyboard .emu file.".into());
    }
    let project = emulsion_io::project::read(path).map_err(|e| e.to_string())?;
    let source = ProjectEditor::open(project, None)?;
    let board = source
        .storyboard()
        .ok_or("That file is not a storyboard project.")?;
    let order = layout(&source);
    let chosen: Vec<_> = match args["scenes"].as_array() {
        None => order,
        Some(names) => {
            let mut scenes = HashSet::new();
            for name in names.iter().filter_map(Value::as_str) {
                let found = scenes_named(board, name);
                if found.is_empty() {
                    let names: Vec<_> = board.scenes.values().map(|s| s.name.as_str()).collect();
                    return Err(format!(
                        "No scene is named '{name}'. Scenes: {}",
                        names.join(", ")
                    ));
                }
                scenes.extend(found);
            }
            order
                .into_iter()
                .filter(|id| scenes.contains(&board.panels[id].scene))
                .collect()
        }
    };
    let mut clip = source.copy_panels(&chosen)?;
    clip.whole_scenes &= args["new_scenes"] != false;
    let after = insertion(args, editor.active_page())?;
    let pasted = editor.paste_panels(after, &clip)?;
    Ok(json!({"panels":pasted,"scenes":clip.scenes,"active_panel":editor.active_page()}))
}
