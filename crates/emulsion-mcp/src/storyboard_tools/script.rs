//! Script and page imports and spelling: a screenplay (Fountain, Final
//! Draft or plain text) laid out as scenes and panels with captions, PDF and
//! Illustrator pages as panels, and the caption spelling check.
use super::{def, insertion, layout, panel_ids, placement};
use crate::ToolDef;
use crate::text_tools::byte_to_char;
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::storyboard::Storyboard;
use emulsion_io::script::{self, storyboard::Split};
use emulsion_io::{pdf_import, spell};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Most misspellings check_storyboard_spelling lists.
const MAX_ISSUES: usize = 300;
/// Distinct words that get suggestions; the rest list none.
const MAX_SUGGESTED_WORDS: usize = 100;

pub(super) fn definitions() -> Vec<ToolDef> {
    let path = |what: &str| json!({"type":"string","minLength":1,"maxLength":4096,"description":format!("Absolute path of the {what}.")});
    let mut script_fields = placement();
    script_fields["path"] = path("script (.fountain, .spmd, .fdx or .txt)");
    script_fields["split"] = json!({"enum":["beat","scene"],"description":"beat (default): one panel per action paragraph or dialogue block; scene: one panel per scene holding all of its action and dialogue."});
    let mut pdf_fields = placement();
    pdf_fields["path"] = path("PDF or Illustrator (.ai, saved with PDF compatibility) file");
    vec![
        def(
            "import_storyboard_script",
            "Lay a screenplay out as storyboard panels: Fountain (.fountain, .spmd), Final Draft (.fdx) or plain text (paragraphs become action). Each scene heading starts a new scene named after it; each action paragraph or dialogue block becomes a panel (or one panel per scene with split \"scene\"), with its text in the Action and Dialogue captions (\"MIA (quietly): line\"), the heading in the first panel's Slugging caption, a first duration from the words, and DISSOLVE/FADE/WIPE transitions as panel transitions. Missing caption fields are added. The scenes go after the scene holding `after` (default: the active panel) or at_start. Returns the script title and the new panel IDs. One Undo step.",
            script_fields,
            &["path"],
        ),
        def(
            "import_storyboard_pdf",
            "Add every page of a PDF or Illustrator file as a new panel, in page order, after a panel (default: the active panel) or at_start, in its scene. Pages arrive as editable vector art fitted to the panel. Needs Poppler (pdftocairo) or MuPDF (mutool) installed. Panels are named after the file (\"Layouts page 2\"). Up to 200 pages. One Undo step.",
            pdf_fields,
            &["path"],
        ),
        def(
            "check_storyboard_spelling",
            "Check the spelling of storyboard captions against the bundled English dictionary and the user's personal word list (Settings › Storyboard). Words in capitals (names, sluglines), with digits or of one letter are skipped. Returns each misspelt word with its panel, caption field, character range and up to `suggestions` corrections. Fix words with replace_in_storyboard_captions (whole_word, match_case). Read-only.",
            json!({
                "panels":panel_ids(),
                "field":{"type":"string","minLength":1,"maxLength":800,"description":"Only this caption field (name from describe_storyboard)."},
                "suggestions":{"type":"integer","minimum":0,"maximum":10,"description":"Corrections per word (default 5)."}
            }),
            &[],
        ),
    ]
}

/// Run a script, page or spelling tool; `None` when `name` is not one.
pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let result = match name {
        "import_storyboard_script" => import_script(editor, board, args),
        "import_storyboard_pdf" => import_pdf(editor, args),
        "check_storyboard_spelling" => check_spelling(editor, board, args),
        _ => return None,
    };
    Some(result)
}

pub(super) fn absolute(args: &Value) -> Result<&Path, String> {
    let text = args["path"].as_str().unwrap();
    let path = Path::new(text);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(format!("Use an absolute path, not '{text}'."))
    }
}

fn import_script(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let path = absolute(args)?;
    let after = insertion(args, editor.active_page())?;
    let script = script::read(path).map_err(|e| format!("{e:#}"))?;
    let split = if args["split"] == "scene" {
        Split::Scene
    } else {
        Split::Beat
    };
    let clip = script::storyboard::panels(&script, board, split).map_err(|e| e.to_string())?;
    let ids = editor.paste_panels(after, &clip)?;
    Ok(json!({
        "title":script.title,
        "scenes":clip.scenes,
        "panels":ids,
        "active_panel":editor.active_page(),
    }))
}

fn import_pdf(editor: &mut ProjectEditor, args: &Value) -> Result<Value, String> {
    let path = absolute(args)?;
    if !pdf_import::is_pdf(path) {
        return Err(format!("'{}' is not a .pdf or .ai file.", path.display()));
    }
    let after = insertion(args, editor.active_page())?;
    let pages =
        pdf_import::pages(path, &AtomicBool::new(false), |_, _| {}).map_err(|e| e.to_string())?;
    let ids = editor.import_panels(after, pages)?;
    let panels: Vec<_> = ids
        .iter()
        .map(|id| {
            let meta = editor.page_list().iter().find(|m| m.id == *id).unwrap();
            json!({"panel":id,"name":meta.name})
        })
        .collect();
    Ok(json!({"panels":panels,"active_panel":editor.active_page()}))
}

fn check_spelling(
    editor: &ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let field = args["field"]
        .as_str()
        .map(|name| super::field(board, name))
        .transpose()?;
    let wanted = super::ids(&args["panels"]);
    if let Some(missing) = wanted.iter().find(|id| !board.panels.contains_key(id)) {
        return Err(format!("Panel {missing} does not exist."));
    }
    let layout: Vec<PageId> = layout(editor)
        .into_iter()
        .filter(|id| wanted.is_empty() || wanted.contains(id))
        .collect();
    let personal = emulsion_io::settings::Settings::load()
        .storyboard
        .spelling_words;
    let limit = args["suggestions"].as_u64().unwrap_or(5) as usize;
    let found: Vec<_> = spell::storyboard(board, &layout, &personal)
        .into_iter()
        .filter(|(_, f, _)| field.is_none_or(|wanted| wanted == *f))
        .collect();
    let mut suggested: HashMap<String, Vec<String>> = HashMap::new();
    let issues: Vec<_> = found
        .iter()
        .take(MAX_ISSUES)
        .map(|(panel, caption, range)| {
            let text = &board.panels[panel].captions[caption].text;
            let word = &text[range.clone()];
            if !suggested.contains_key(word) && suggested.len() < MAX_SUGGESTED_WORDS {
                suggested.insert(word.to_string(), spell::suggestions(word, limit));
            }
            let name = &board
                .captions
                .iter()
                .find(|c| c.id == *caption)
                .unwrap()
                .name;
            json!({
                "panel":panel,
                "field":name,
                "word":word,
                "start":byte_to_char(text, range.start),
                "end":byte_to_char(text, range.end),
                "suggestions":suggested.get(word).cloned().unwrap_or_default(),
                "locked":board.is_locked(*panel),
            })
        })
        .collect();
    Ok(json!({
        "count":found.len(),
        "truncated":found.len() > MAX_ISSUES,
        "issues":issues,
        "personal_words":personal.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::super::execute;
    use super::super::tests::{board, call};
    use emulsion_core::Document;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use serde_json::json;
    use std::path::PathBuf;

    fn dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sb-script-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const SCRIPT: &str = "Title: The Storm

INT. KITCHEN - NIGHT

Rain on the window.

MIA
Is anyone there?

DISSOLVE TO:

EXT. GARDEN - DAWN

Birds.
";

    #[test]
    fn scripts_import_as_scenes_and_panels_in_one_step() {
        let path = dir("fountain").join("storm.fountain");
        std::fs::write(&path, SCRIPT).unwrap();
        let mut e = board();
        let first = e.active_page();
        let out = call(
            &mut e,
            "import_storyboard_script",
            json!({"path":path.to_str().unwrap()}),
        );
        assert_eq!(out["title"], "The Storm");
        assert_eq!(
            out["scenes"],
            json!(["INT. KITCHEN - NIGHT", "EXT. GARDEN - DAWN"])
        );
        assert_eq!(out["panels"].as_array().unwrap().len(), 3);
        assert_eq!(e.page_list().len(), 4);
        assert_eq!(e.page_list()[0].id, first, "lands after the active panel");
        assert!(e.undo());
        assert_eq!(e.page_list().len(), 1);
        // One panel per scene, at the start.
        let out = call(
            &mut e,
            "import_storyboard_script",
            json!({"path":path.to_str().unwrap(),"split":"scene","at_start":true}),
        );
        assert_eq!(out["panels"].as_array().unwrap().len(), 2);
        assert_eq!(e.page_list().last().unwrap().id, first);
        let relative = execute(
            &mut e,
            "import_storyboard_script",
            &json!({"path":"storm.fountain"}),
        );
        assert!(relative.is_error);
    }

    #[test]
    fn spelling_lists_words_with_suggestions() {
        let mut e = board();
        let panel = e.active_page();
        call(
            &mut e,
            "update_storyboard_panel",
            json!({"panel":panel,"captions":{"Action":"Mia runns to the windw.","Dialogue":"MIA: Helo!"}}),
        );
        let out = call(&mut e, "check_storyboard_spelling", json!({}));
        let words: Vec<_> = out["issues"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["word"].as_str().unwrap())
            .collect();
        assert!(words.starts_with(&["runns", "windw"]), "{words:?}");
        let windw = &out["issues"][1];
        assert_eq!(windw["field"], "Action");
        assert_eq!(
            (windw["start"].as_u64(), windw["end"].as_u64()),
            (Some(17), Some(22))
        );
        assert!(
            windw["suggestions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s == "window")
        );
        let dialogue = call(
            &mut e,
            "check_storyboard_spelling",
            json!({"field":"dialogue","suggestions":0}),
        );
        assert_eq!(dialogue["issues"][0]["word"], "Helo");
        assert_eq!(dialogue["issues"][0]["suggestions"], json!([]));
        assert!(
            execute(
                &mut e,
                "check_storyboard_spelling",
                &json!({"panels":[999]})
            )
            .is_error
        );
    }

    #[test]
    fn pdf_pages_become_panels_in_one_step() {
        if emulsion_io::pdf_import::converter().is_none() {
            eprintln!("skipped: no PDF converter on PATH");
            return;
        }
        // A two-page PDF from the project exporter.
        let mut design =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(80, 40)).unwrap();
        design
            .add_page(Document::new(40, 80), "Second".into(), 0.)
            .unwrap();
        let project = design.snapshot().unwrap();
        let ids: Vec<_> = project.pages.iter().map(|p| p.meta.id).collect();
        let dir = dir("pdf");
        let path = dir.join("Layouts.pdf");
        emulsion_io::project_export::write(
            &project,
            &ids,
            emulsion_io::project_export::Format::Pdf,
            false,
            &path,
        )
        .unwrap();
        let mut e = board();
        let out = call(
            &mut e,
            "import_storyboard_pdf",
            json!({"path":path.to_str().unwrap()}),
        );
        let names: Vec<_> = out["panels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Layouts page 1", "Layouts page 2"]);
        assert!(e.undo());
        assert_eq!(e.page_list().len(), 1);
        let png = dir.join("a.png");
        assert!(
            execute(
                &mut e,
                "import_storyboard_pdf",
                &json!({"path":png.to_str().unwrap()})
            )
            .is_error
        );
    }
}
