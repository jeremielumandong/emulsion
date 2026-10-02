//! Change tracking, Compare and review tools: board versions, what changed
//! since a version (or the last save or export), two versions panel by
//! panel, and each panel's review status and notes. Review changes are one
//! Undo step each; versions are not Undo steps.
use super::{def, panel_id, panel_ids};
use crate::ToolDef;
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::storyboard::{ReviewStatus, Storyboard};
use emulsion_core::storyboard_changes::{CompareRow, PanelChange, compare, describe_changes};
use emulsion_core::storyboard_versions::{Baseline, BoardState};
use serde_json::{Value, json};

fn statuses() -> Value {
    json!({"enum":["none","to_do","in_review","approved","needs_changes"]})
}

fn version_id() -> Value {
    json!({"type":"integer","minimum":1,"description":"Board version ID from describe_storyboard_changes `versions`."})
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let point = json!({"enum":["last_save","last_export"],"description":"last_save: the board as last opened or saved; last_export: as last exported or printed in this session."});
    let unchanged = json!({"type":"boolean","description":"Also list panels that did not change. Default false."});
    vec![
        def(
            "create_storyboard_version",
            "Save the whole board as a named version to track changes against and compare: every panel's drawing (recorded in its page history) with the order, names, captions, timing, cameras and layer keys. Not an Undo step; saved with the project. Returns the version ID.",
            json!({"name":{"type":"string","minLength":1,"maxLength":200}}),
            &["name"],
        ),
        def(
            "describe_storyboard_changes",
            "What changed since a board version (`version`), the last save or the last export (`since`); default the newest version, else the last save. Lists panels in board order as new, changed (with `aspects`: drawing, caption, timing, camera, layer_keys, details, name, review — review covers review layers and notes), moved, or deleted (with the old panel ID, which no longer exists). Also lists the board `versions`. Read-only.",
            json!({"version":version_id(),"since":point.clone(),"include_unchanged":unchanged.clone()}),
            &[],
        ),
        def(
            "compare_storyboard_versions",
            "Compare two states of the board panel by panel: `from` (a version, or last_save/last_export) and `to` (default the current board). Panels are matched by ID, then by name. Each row gives both panel IDs, the change, both durations and, for captions that differ, the old and new text with a word diff (same/added/removed runs). Reading a version never changes the open board. Read-only.",
            json!({
                "from_version":version_id(),
                "from":point.clone(),
                "to_version":version_id(),
                "to":{"enum":["current","last_save","last_export"]},
                "include_unchanged":unchanged
            }),
            &[],
        ),
        def(
            "set_storyboard_review_status",
            "Set the review status of panels: none, to_do, in_review, approved or needs_changes. Works on locked panels. One Undo step.",
            json!({"panels":panel_ids(),"status":statuses()}),
            &["panels", "status"],
        ),
        def(
            "add_storyboard_review_note",
            "Add a review note to a panel, signed with `author` (default: the review author name in Settings › Storyboard, else Assistant) and the time. Works on locked panels. Returns the note ID. One Undo step.",
            json!({
                "panel":panel_id(),
                "text":{"type":"string","minLength":1,"maxLength":16000},
                "author":{"type":"string","minLength":1,"maxLength":400}
            }),
            &["panel", "text"],
        ),
        def(
            "resolve_storyboard_review_note",
            "Mark a panel's review note resolved, or open again with resolved false. One Undo step.",
            json!({
                "panel":panel_id(),
                "note":{"type":"integer","minimum":1,"description":"Note ID from list_storyboard_review."},
                "resolved":{"type":"boolean","description":"Default true."}
            }),
            &["panel", "note"],
        ),
        def(
            "list_storyboard_review",
            "List panels with a review status or notes, in board order: status, notes (ID, author, time, text, resolved) and how many review layers each panel has. Filter by `status`, or `open_only` for panels with unresolved notes. Read-only.",
            json!({"status":statuses(),"open_only":{"type":"boolean"}}),
            &[],
        ),
    ]
}

fn status(value: &Value) -> Result<ReviewStatus, String> {
    serde_json::from_value(value.clone()).map_err(|e| e.to_string())
}

/// The state `version` or `point` names; `fallback` when neither does.
fn state(
    editor: &ProjectEditor,
    version: &Value,
    point: &Value,
    fallback: Baseline,
) -> Result<BoardState, String> {
    let baseline = match (version.as_u64(), point.as_str()) {
        (Some(_), Some(_)) => return Err("Name either a version or a point, not both".into()),
        (Some(id), None) => Baseline::Version(id),
        (None, Some("last_export")) => Baseline::LastExport,
        (None, Some("last_save")) => Baseline::LastSave,
        (None, Some("current")) => {
            return editor
                .current_board_state()
                .ok_or_else(|| "Open a storyboard first".into());
        }
        _ => fallback,
    };
    editor.board_state(baseline)
}

fn versions_json(editor: &ProjectEditor) -> Value {
    editor
        .board_versions()
        .iter()
        .map(|v| json!({"id":v.id,"name":v.name,"time":v.time,"panels":v.layout.len()}))
        .collect()
}

fn change_json(c: &PanelChange) -> Value {
    let mut out = json!({"name":c.name,"change":c.kind});
    if let Some(new) = c.new {
        out["panel"] = json!(new);
    }
    if c.old.is_some() && c.old != c.new {
        out["old_panel"] = json!(c.old);
    }
    if !c.aspects.is_empty() {
        out["aspects"] = json!(c.aspects);
    }
    if c.moved {
        out["moved"] = json!(true);
    }
    out
}

fn row_json(row: &CompareRow) -> Value {
    let mut out = change_json(&row.change);
    out["frames"] = json!({"from":row.old_frames,"to":row.new_frames});
    if !row.captions.is_empty() {
        out["captions"] = row
            .captions
            .iter()
            .map(|c| {
                let diff: Vec<_> = c
                    .words
                    .iter()
                    .map(|(op, text)| json!({"op":op,"text":text}))
                    .collect();
                json!({"field":c.field,"from":c.old,"to":c.new,"diff":diff})
            })
            .collect();
    }
    out
}

fn review_json(editor: &ProjectEditor, board: &Storyboard, id: PageId) -> Value {
    let panel = &board.panels[&id];
    let name = editor
        .page_list()
        .iter()
        .find(|m| m.id == id)
        .map_or("", |m| m.name.as_str());
    let layers = editor
        .page(id)
        .map_or(0, |e| e.doc.nodes.iter().filter(|n| n.review).count());
    json!({
        "panel":id,
        "name":name,
        "status":panel.review.status,
        "notes":panel.review.notes,
        "review_layers":layers,
    })
}

fn author(args: &Value) -> String {
    args["author"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            let name = emulsion_io::settings::Settings::load()
                .storyboard
                .review_author;
            if name.trim().is_empty() {
                "Assistant".into()
            } else {
                name
            }
        })
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let result = match name {
        "create_storyboard_version" => editor
            .create_board_version(args["name"].as_str().unwrap())
            .map(|id| json!({"version":id,"versions":versions_json(editor)})),
        "describe_storyboard_changes" => (|| {
            let fallback = editor
                .board_versions()
                .last()
                .map_or(Baseline::LastSave, |v| Baseline::Version(v.id));
            let old = state(editor, &args["version"], &args["since"], fallback)?;
            let now = editor
                .current_board_state()
                .ok_or("Open a storyboard first")?;
            let all = args["include_unchanged"] == true;
            let changes: Vec<_> = describe_changes(&old, &now)
                .iter()
                .filter(|c| all || c.is_change())
                .map(change_json)
                .collect();
            Ok(json!({"since":old.label,"changes":changes,"versions":versions_json(editor)}))
        })(),
        "compare_storyboard_versions" => (|| {
            let current = editor
                .current_board_state()
                .ok_or("Open a storyboard first")?;
            let from = state(
                editor,
                &args["from_version"],
                &args["from"],
                Baseline::LastSave,
            )?;
            let to = match (args["to_version"].is_null(), args["to"].as_str()) {
                (true, None | Some("current")) => current,
                _ => state(editor, &args["to_version"], &args["to"], Baseline::LastSave)?,
            };
            let all = args["include_unchanged"] == true;
            let rows: Vec<_> = compare(&from, &to)
                .iter()
                .filter(|r| all || r.change.is_change())
                .map(row_json)
                .collect();
            Ok(json!({"from":from.label,"to":to.label,"panels":rows}))
        })(),
        "set_storyboard_review_status" => (|| {
            let status = status(&args["status"])?;
            let panels = super::ids(&args["panels"]);
            editor.edit_storyboard(|b| {
                for id in &panels {
                    b.set_review_status(*id, status)?;
                }
                Ok(())
            })?;
            Ok(json!({"panels":panels,"status":status}))
        })(),
        "add_storyboard_review_note" => (|| {
            let panel = args["panel"].as_u64().unwrap();
            let author = author(args);
            let mut id = 0;
            editor.edit_storyboard(|b| {
                id = b.add_review_note(
                    panel,
                    &author,
                    args["text"].as_str().unwrap(),
                    emulsion_core::storyboard_review::now(),
                )?;
                Ok(())
            })?;
            Ok(json!({"panel":panel,"note":id,"author":author}))
        })(),
        "resolve_storyboard_review_note" => (|| {
            let panel = args["panel"].as_u64().unwrap();
            let resolved = args["resolved"].as_bool().unwrap_or(true);
            editor.edit_storyboard(|b| {
                b.resolve_review_note(panel, args["note"].as_u64().unwrap(), resolved)
            })?;
            Ok(json!({"panel":panel,"note":args["note"],"resolved":resolved}))
        })(),
        "list_storyboard_review" => (|| {
            let wanted = args.get("status").map(status).transpose()?;
            let open_only = args["open_only"] == true;
            let panels: Vec<_> = super::layout(editor)
                .into_iter()
                .filter(|id| {
                    let review = &board.panels[id].review;
                    wanted.is_none_or(|s| review.status == s)
                        && (!open_only || review.open_notes().next().is_some())
                        && (wanted.is_some() || !review.is_empty())
                })
                .map(|id| review_json(editor, board, id))
                .collect();
            Ok(json!({"panels":panels}))
        })(),
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{board, call};
    use emulsion_core::{Command, Node, NodeKind, command::Slot};
    use serde_json::json;

    #[test]
    fn changes_since_a_version_and_a_comparison_read_the_history() {
        let mut e = board();
        call(&mut e, "add_storyboard_panels", json!({"panels":[{},{}]}));
        let v = call(
            &mut e,
            "create_storyboard_version",
            json!({"name":"Pass 1"}),
        )["version"]
            .clone();
        let layout: Vec<u64> = e.page_list().iter().map(|m| m.id).collect();
        call(
            &mut e,
            "update_storyboard_panel",
            json!({"panel":layout[1],"captions":{"Action":"Mia runs"},"frames":10}),
        );
        e.set_active_page(layout[2]).unwrap();
        e.execute(Command::AddNode {
            node: Box::new(Node::new(0, "Ink", NodeKind::Fill { rgba: [0; 4] })),
            slot: Slot::TOP,
        })
        .unwrap();
        let changes = call(&mut e, "describe_storyboard_changes", json!({"version":v}));
        assert_eq!(changes["since"], "Pass 1");
        let list = changes["changes"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["aspects"], json!(["caption", "timing"]));
        assert_eq!(list[1]["aspects"], json!(["drawing"]));
        let compared = call(
            &mut e,
            "compare_storyboard_versions",
            json!({"from_version":v}),
        );
        let row = &compared["panels"][0];
        assert_eq!(row["frames"], json!({"from":48,"to":10}));
        assert_eq!(
            row["captions"][0]["diff"][0],
            json!({"op":"added","text":"Mia runs"})
        );
        // Reading versions changed nothing.
        assert!(e.can_undo());
        assert_eq!(e.page_list().len(), 3);
    }

    #[test]
    fn review_status_and_notes_are_one_undo_step_each() {
        let mut e = board();
        call(&mut e, "add_storyboard_panels", json!({"panels":[{}]}));
        call(
            &mut e,
            "set_storyboard_review_status",
            json!({"panels":[1],"status":"needs_changes"}),
        );
        let note = call(
            &mut e,
            "add_storyboard_review_note",
            json!({"panel":1,"text":"Bigger eyes","author":"Ana"}),
        )["note"]
            .clone();
        call(
            &mut e,
            "resolve_storyboard_review_note",
            json!({"panel":1,"note":note}),
        );
        let listed = call(&mut e, "list_storyboard_review", json!({}));
        assert_eq!(listed["panels"][0]["status"], "needs_changes");
        assert_eq!(listed["panels"][0]["notes"][0]["resolved"], true);
        let open = call(&mut e, "list_storyboard_review", json!({"open_only":true}));
        assert!(open["panels"].as_array().unwrap().is_empty());
        let review = |e: &emulsion_core::project::ProjectEditor| {
            e.storyboard().unwrap().panels[&1].review.clone()
        };
        assert!(e.undo());
        assert!(!review(&e).notes[0].resolved);
        assert!(e.undo());
        assert!(review(&e).notes.is_empty());
        assert!(e.undo());
        assert!(review(&e).is_empty());
    }
}
