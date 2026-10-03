//! Paper worksheets: print empty frames for chosen panels or new ones, then
//! read photos or scans of the drawn sheets back onto their panels as
//! layers (one Undo step), with a dry run that only reports.
use super::def;
use super::export::{output, profile, profile_options, scope, scope_fields};
use crate::ToolDef;
use emulsion_core::{project::ProjectEditor, storyboard::Storyboard};
use emulsion_io::storyboard_export::{
    worksheet::{self, Panels, Slot},
    worksheet_scan::{self as scan, Clean, MAX_PHOTOS},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::atomic::AtomicBool};

fn slot_json(slot: Slot) -> Value {
    match slot {
        Slot::Panel(id) => json!({"panel":id}),
        Slot::New(n) => json!({"new_panel":n}),
    }
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let profile_names = emulsion_io::storyboard_export::profile::builtins()
        .into_iter()
        .map(|p| p.name)
        .collect::<Vec<_>>()
        .join(", ");
    let mut print = scope_fields(json!({
        "path":{"type":"string","minLength":2,"maxLength":4096,"description":"Absolute path of the .pdf file to write; an existing file is replaced."},
        "new_panels":{"type":"integer","minimum":1,"maximum":worksheet::MAX_NEW_PANELS,"description":"Print this many empty frames that become new panels when imported, instead of existing panels."},
        "profile":{"type":"string","minLength":1,"maxLength":100,"description":format!("Storyboard PDF profile whose paper, orientation, panels per page (up to {}) and caption placement the sheets use: built-in ({profile_names}) or saved. Default: the first built-in.", worksheet::MAX_FRAMES)},
        "options":profile_options(),
        "title":{"type":"string","maxLength":200,"description":"Name printed in each sheet's header. Default: Storyboard."}
    }));
    print["panels"]["description"] = json!(
        "Print frames for these panels, in board order. Omit panels, scene and new_panels for every panel."
    );
    vec![
        def(
            "export_storyboard_worksheets",
            "Write paper worksheets as a PDF: empty frames at the board's aspect ratio to draw on by hand, each labelled with its scene and panel number (or New panel n) with ruled caption lines and the panel's captions. Every sheet has four corner marks and a QR code naming this storyboard, the sheet and each frame's panel, so import_storyboard_worksheets can bring photos or scans of the drawn sheets back. Returns the pages with their sheet IDs and frames.",
            print,
            &["path"],
        ),
        def(
            "import_storyboard_worksheets",
            "Read photos or scans (JPEG, PNG, HEIC and other formats File → Open reads) of drawn worksheets: find each sheet's code and corner marks, straighten the photo, cut each frame out at the panel resolution and clean it. Each drawing goes onto its panel as a new layer named \"Paper drawing (date)\"; new-panel frames become new panels after `after` (default: the active panel). One Undo step for the whole import. Sheets from another storyboard are refused. For a photo whose code cannot be read, give `layout_profile` (the profile it was printed with) and its frames become new panels. `dry_run` reports what was found without changing anything.",
            json!({
                "paths":{"type":"array","minItems":1,"maxItems":MAX_PHOTOS,"items":{"type":"string","minLength":2,"maxLength":4096},"description":"Absolute paths of the photos or scans."},
                "clean":{"enum":["transparent","white","line_art","photo"],"description":"transparent (default): paper becomes transparent, strokes keep their colour; white: paper becomes even white; line_art: black strokes only; photo: only straightened."},
                "replace":{"type":"boolean","description":"Remove each panel's earlier paper drawing layers instead of adding another. Default false."},
                "after":{"type":"integer","minimum":1,"description":"Panel new panels go after. Default: the active panel."},
                "layout_profile":{"type":"string","minLength":1,"maxLength":100,"description":"Profile a sheet without a readable code was printed with; its frames become new panels."},
                "dry_run":{"type":"boolean","description":"Only report the sheets, frames and target panels found. Default false."}
            }),
            &["paths"],
        ),
    ]
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let cancel = AtomicBool::new(false);
    let result = match name {
        "export_storyboard_worksheets" => (|| {
            let path = output(args, "path", Some("pdf"))?;
            let profile = profile(args)?;
            let panels = match args["new_panels"].as_u64() {
                Some(_) if args.get("panels").is_some() || args.get("scene").is_some() => {
                    return Err("Use new_panels, panels or scene, not several.".into());
                }
                Some(count) => Panels::New(count as u32),
                None => Panels::Existing(scope(args, board)?),
            };
            let project = editor.snapshot().ok_or("Open a storyboard first.")?;
            let title = args["title"].as_str().unwrap_or("Storyboard");
            let codes = worksheet::write_pdf(&project, title, &panels, &profile, &path, &cancel)
                .map_err(|e| e.to_string())?;
            let sheets: Vec<_> = codes
                .iter()
                .map(|c| {
                    json!({
                        "sheet":c.sheet,
                        "frames":c.frames.iter().map(|(slot, _)| slot_json(*slot)).collect::<Vec<_>>(),
                    })
                })
                .collect();
            Ok(json!({"path":path,"pages":codes.len(),"profile":profile.name,"sheets":sheets}))
        })(),
        "import_storyboard_worksheets" => (|| {
            let paths: Vec<PathBuf> = args["paths"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(PathBuf::from)
                .collect();
            if let Some(path) = paths.iter().find(|p| !p.is_absolute()) {
                return Err(format!("Use absolute paths: {}", path.display()));
            }
            let clean: Clean = match args.get("clean") {
                Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string())?,
                None => Clean::default(),
            };
            let layout = match args["layout_profile"].as_str() {
                Some(name) => {
                    let profile = profile(&json!({"profile":name}))?;
                    let project = editor.snapshot().ok_or("Open a storyboard first.")?;
                    Some(worksheet::blank_code(&project, &profile).map_err(|e| e.to_string())?)
                }
                None => None,
            };
            let after = match args["after"].as_u64() {
                Some(id) if editor.page_list().iter().any(|m| m.id == id) => id,
                Some(id) => return Err(format!("No panel has ID {id}.")),
                None => editor.active_page(),
            };
            let size = (board.settings.width, board.settings.height);
            let read = scan::scan_files(
                &paths,
                &board.project_id,
                layout.as_ref(),
                size,
                clean,
                &cancel,
                |_, _| {},
            );
            let mut photos = Vec::new();
            let mut sheets = Vec::new();
            for (path, result) in paths.iter().zip(read) {
                match result {
                    Ok(sheet) => {
                        photos.push(json!({
                            "path":path,
                            "sheet":sheet.code.sheet,
                            "code_read":sheet.code_read,
                            "frames":sheet.frames.iter().map(|f| {
                                let mut v = slot_json(f.slot);
                                v["drawn"] = json!(f.drawn());
                                v["ink"] = json!((f.ink * 1000.).round() / 1000.);
                                v
                            }).collect::<Vec<_>>(),
                        }));
                        sheets.push(sheet);
                    }
                    Err(error) => photos.push(json!({"path":path,"error":error.to_string()})),
                }
            }
            let ids: Vec<_> = editor.page_list().iter().map(|m| m.id).collect();
            let plan = scan::plan(&sheets, &ids);
            let summary = json!({
                "photos":photos,
                "drawings":plan.drawings.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                "new_panels":plan.new_panels.len(),
                "blank_frames":plan.blank,
                "notes":plan.notes,
            });
            if args["dry_run"] == true {
                return Ok(json!({"dry_run":true,"result":summary}));
            }
            if plan.drawings.is_empty() && plan.new_panels.is_empty() {
                return Err(format!("Nothing to import. {summary}"));
            }
            let placed = scan::place(editor, plan, Some(after), args["replace"] == true)?;
            Ok(json!({
                "result":summary,
                "changed_panels":placed.changed,
                "added_panels":placed.added,
            }))
        })(),
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::super::execute;
    use super::super::export::tests::Dir;
    use super::super::tests::{board, call};
    use serde_json::json;

    #[test]
    fn worksheets_print_and_photos_dry_run_without_changing_the_board() {
        let mut e = board();
        call(
            &mut e,
            "add_storyboard_panels",
            json!({"panels":[{"seconds":1}]}),
        );
        let stamp = e.stamp();
        let dir = Dir::new("worksheets");
        let pdf = dir.path().join("sheets.pdf");
        let result = call(
            &mut e,
            "export_storyboard_worksheets",
            json!({"path":pdf,"profile":"1 per page · large"}),
        );
        assert_eq!(result["pages"], 2);
        assert!(result["sheets"][0]["frames"][0]["panel"].is_u64());
        let result = call(
            &mut e,
            "export_storyboard_worksheets",
            json!({"path":pdf,"new_panels":7}),
        );
        assert_eq!(result["sheets"][2]["frames"][0]["new_panel"], 7);
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
        // A photo of nothing: reported, nothing changes.
        let photo = dir.path().join("photo.png");
        image::RgbaImage::from_pixel(300, 200, image::Rgba([240, 240, 240, 255]))
            .save(&photo)
            .unwrap();
        let result = call(
            &mut e,
            "import_storyboard_worksheets",
            json!({"paths":[photo],"dry_run":true}),
        );
        let error = result["result"]["photos"][0]["error"].as_str().unwrap();
        assert!(error.contains("No worksheet code"), "{error}");
        for args in [
            json!({"paths":[photo]}),
            json!({"paths":["photo.png"]}),
            json!({"paths":[photo],"clean":"sepia"}),
            json!({"paths":[photo],"after":999}),
        ] {
            assert!(
                execute(&mut e, "import_storyboard_worksheets", &args).is_error,
                "{args}"
            );
        }
        assert!(
            execute(
                &mut e,
                "export_storyboard_worksheets",
                &json!({"path":pdf,"new_panels":2,"scene":1})
            )
            .is_error
        );
        assert_eq!(e.stamp(), stamp);
    }
}
