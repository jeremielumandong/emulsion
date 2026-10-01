//! Caption tools: caption fields, character formatting and find/replace
//! across captions. Offsets are in characters, not UTF-8 bytes.
use super::{def, field, layout, panel_id};
use crate::text_tools::{StyleEdit, byte_to_char, char_to_byte};
use crate::{ToolDef, ToolResult};
use emulsion_core::project::ProjectEditor;
use emulsion_core::storyboard::{FindOptions, MAX_CAPTION_FIELDS, Storyboard};
use serde_json::{Value, json};
use std::ops::Range;

/// Most matches find_in_storyboard_captions lists.
const MAX_MATCHES: usize = 500;
/// Characters of context on each side of a match.
const CONTEXT: usize = 30;

pub(super) fn definitions() -> Vec<ToolDef> {
    let field_name = json!({"type":"string","minLength":1,"maxLength":800,"description":"Caption field name from describe_storyboard (case-insensitive)."});
    let name = json!({"type":"string","minLength":1,"maxLength":800});
    let flag = json!({"type":"boolean"});
    let position = json!({"type":"integer","minimum":0,"maximum":MAX_CAPTION_FIELDS - 1,"description":"Zero-based place in the field order."});
    let mut search = json!({
        "query":{"type":"string","minLength":1,"maxLength":16000},
        "field":field_name.clone(),
        "match_case":flag.clone(),
        "whole_word":flag.clone()
    });
    let mut format = crate::tools::character_style_properties();
    for (key, value) in [
        ("panel", panel_id()),
        ("field", field_name.clone()),
        (
            "start",
            json!({"type":"integer","minimum":0,"description":"Zero-based character offset."}),
        ),
        (
            "end",
            json!({"type":"integer","minimum":0,"description":"Character offset just past the range."}),
        ),
        (
            "match",
            json!({"type":"string","minLength":1,"maxLength":16000,"description":"Format every occurrence of this exact text instead of a range."}),
        ),
    ] {
        format[key] = value;
    }
    let find = def(
        "find_in_storyboard_captions",
        "Find text in captions across the storyboard, in page then field order. Returns each match's panel, field, character range, a little context and whether the panel is locked (replace skips locked panels). Read-only.",
        search.clone(),
        &["query"],
    );
    search["replacement"] =
        json!({"type":"string","maxLength":16000,"description":"Empty deletes the matches."});
    vec![
        def(
            "add_storyboard_caption_field",
            "Add a caption field to the project (such as Camera, Sound or VFX). `multiline` allows line breaks (default true); `print` includes it in printed and PDF boards (default true). Returns the field ID. One Undo step.",
            json!({"name":name,"multiline":flag,"print":flag,"position":position}),
            &["name"],
        ),
        def(
            "update_storyboard_caption_field",
            "Rename a caption field, change whether it is multi-line or printed, or move it to another place in the field order. Panel captions keep their text. One Undo step.",
            json!({"field":field_name,"name":name,"multiline":flag,"print":flag,"position":position}),
            &["field"],
        ),
        def(
            "remove_storyboard_caption_field",
            "Remove a caption field and its text on every panel. Refused while a locked panel has text in it. One Undo step.",
            json!({"field":field_name}),
            &["field"],
        ),
        def(
            "format_storyboard_caption",
            "Format part of one panel's caption: bold, italic, underline, strikethrough, colour (#RRGGBB), size and other character styles. Address the text by `start`/`end` character offsets or by `match` (every exact occurrence); omit both to format the whole caption. Unmentioned styles are kept; unformatted caption text is size 48 and black. Locked panels refuse it. One Undo step.",
            format,
            &["panel", "field"],
        ),
        find,
        def(
            "replace_in_storyboard_captions",
            "Replace text in captions across the storyboard, keeping formatting around each change, such as a character rename. Locked panels are skipped. Returns how many matches were replaced and how many locked panels were skipped. One Undo step.",
            search,
            &["query", "replacement"],
        ),
    ]
}

fn message(result: ToolResult) -> String {
    result.content[0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn options(args: &Value) -> FindOptions {
    FindOptions {
        match_case: args["match_case"] == true,
        whole_word: args["whole_word"] == true,
    }
}

/// Run a caption tool; `None` when `name` is not one.
pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let result = match name {
        "add_storyboard_caption_field" => (|| {
            let mut id = 0;
            editor.edit_storyboard(|b| {
                id = b.add_caption_field(
                    args["name"].as_str().unwrap(),
                    args["multiline"].as_bool().unwrap_or(true),
                    args["print"].as_bool().unwrap_or(true),
                )?;
                match args["position"].as_u64() {
                    Some(index) => b.move_caption_field(id, index as usize),
                    None => Ok(()),
                }
            })?;
            Ok(json!({"field":id,"caption_fields":editor.storyboard().unwrap().captions}))
        })(),
        "update_storyboard_caption_field" => (|| {
            let id = field(board, args["field"].as_str().unwrap())?;
            if ["name", "multiline", "print", "position"]
                .iter()
                .all(|key| args.get(key).is_none())
            {
                return Err("Give a new name, multiline, print or position.".into());
            }
            editor.edit_storyboard(|b| {
                let caption = b.captions.iter_mut().find(|c| c.id == id).unwrap();
                if let Some(name) = args["name"].as_str() {
                    caption.name = name.trim().into();
                }
                if let Some(multiline) = args["multiline"].as_bool() {
                    caption.multiline = multiline;
                }
                if let Some(print) = args["print"].as_bool() {
                    caption.print = print;
                }
                match args["position"].as_u64() {
                    Some(index) => b.move_caption_field(id, index as usize),
                    None => Ok(()),
                }
            })?;
            Ok(json!({"field":id,"caption_fields":editor.storyboard().unwrap().captions}))
        })(),
        "remove_storyboard_caption_field" => (|| {
            let id = field(board, args["field"].as_str().unwrap())?;
            editor.edit_storyboard(|b| b.remove_caption_field(id))?;
            Ok(json!({"removed":id,"caption_fields":editor.storyboard().unwrap().captions}))
        })(),
        "format_storyboard_caption" => format(editor, board, args),
        "find_in_storyboard_captions" => find(editor, board, args),
        "replace_in_storyboard_captions" => (|| {
            let field = args["field"]
                .as_str()
                .map(|name| field(board, name))
                .transpose()?;
            let layout = layout(editor);
            let mut counts = (0, 0);
            editor.edit_storyboard(|b| {
                counts = b.replace_all(
                    &layout,
                    args["query"].as_str().unwrap(),
                    args["replacement"].as_str().unwrap(),
                    field,
                    options(args),
                );
                Ok(())
            })?;
            Ok(json!({"replaced":counts.0,"locked_panels_skipped":counts.1}))
        })(),
        _ => return None,
    };
    Some(result)
}

fn format(editor: &mut ProjectEditor, board: &Storyboard, args: &Value) -> Result<Value, String> {
    let style = StyleEdit::parse(args).map_err(message)?;
    if style.is_empty() {
        return Err("Give at least one character style to change.".into());
    }
    let id = args["panel"].as_u64().unwrap();
    let name = args["field"].as_str().unwrap();
    let field = field(board, name)?;
    let text = &board
        .panels
        .get(&id)
        .ok_or("Panel does not exist.")?
        .captions
        .get(&field)
        .ok_or_else(|| format!("That panel has no {name} caption."))?
        .text;
    let offset = |key: &str| {
        char_to_byte(text, args[key].as_u64().unwrap() as usize)
            .ok_or_else(|| format!("{key} is past the end of the caption."))
    };
    let ranges: Vec<Range<usize>> = match (args.get("start"), args.get("end"), args.get("match")) {
        (None, None, None) => std::iter::once(0..text.len()).collect(),
        (Some(_), Some(_), None) => {
            let range = offset("start")?..offset("end")?;
            if range.is_empty() {
                return Err("start must be less than end.".into());
            }
            vec![range]
        }
        (None, None, Some(wanted)) => {
            let exact = FindOptions {
                match_case: true,
                whole_word: false,
            };
            let found: Vec<_> = board
                .find(&[id], wanted.as_str().unwrap(), Some(field), exact)
                .into_iter()
                .map(|(_, _, range)| range)
                .collect();
            if found.is_empty() {
                return Err(format!(
                    "'{}' is not in that caption.",
                    wanted.as_str().unwrap()
                ));
            }
            found
        }
        _ => return Err("Give start and end, or match, or neither.".into()),
    };
    let chars: Vec<_> = ranges
        .iter()
        .map(|r| json!({"start":byte_to_char(text, r.start),"end":byte_to_char(text, r.end)}))
        .collect();
    editor.edit_storyboard(|b| {
        let caption = b
            .panels
            .get_mut(&id)
            .unwrap()
            .captions
            .get_mut(&field)
            .unwrap();
        for range in ranges {
            caption.apply_style(range, |s| style.apply(s));
        }
        Ok(())
    })?;
    Ok(json!({"panel":id,"field":name,"ranges":chars}))
}

fn find(editor: &ProjectEditor, board: &Storyboard, args: &Value) -> Result<Value, String> {
    let field = args["field"]
        .as_str()
        .map(|name| field(board, name))
        .transpose()?;
    let found = board.find(
        &layout(editor),
        args["query"].as_str().unwrap(),
        field,
        options(args),
    );
    let matches: Vec<_> = found
        .iter()
        .take(MAX_MATCHES)
        .map(|(panel, caption, range)| {
            let text = &board.panels[panel].captions[caption].text;
            let from = text[..range.start]
                .char_indices()
                .rev()
                .nth(CONTEXT - 1)
                .map_or(0, |(byte, _)| byte);
            let to = text[range.end..]
                .char_indices()
                .nth(CONTEXT)
                .map_or(text.len(), |(byte, _)| range.end + byte);
            let name = &board
                .captions
                .iter()
                .find(|c| c.id == *caption)
                .unwrap()
                .name;
            json!({
                "panel":panel,
                "field":name,
                "start":byte_to_char(text, range.start),
                "end":byte_to_char(text, range.end),
                "context":&text[from..to],
                "locked":board.is_locked(*panel),
            })
        })
        .collect();
    Ok(json!({"count":found.len(),"truncated":found.len() > MAX_MATCHES,"matches":matches}))
}
