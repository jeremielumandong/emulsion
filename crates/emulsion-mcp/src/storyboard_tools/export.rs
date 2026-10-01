//! Storyboard exports: a PDF board laid out by a profile, panel images named
//! by a pattern, and a CSV of captions and timing. They read the live board
//! and write files; the storyboard itself never changes.
use super::{def, panel_ids};
use crate::ToolDef;
use emulsion_core::{project::ProjectEditor, storyboard::Storyboard};
use emulsion_io::storyboard_export::{
    self as export, Profile, Scope, images,
    profile::{builtin, builtins},
};
use serde_json::{Value, json};
use std::{path::Path, sync::atomic::AtomicBool};

fn scope_fields(mut fields: Value) -> Value {
    fields["panels"] = panel_ids();
    fields["panels"]["description"] =
        json!("Export only these panels, in board order. Omit both panels and scene for all.");
    fields["scene"] = json!({"type":"integer","minimum":1,"description":"Export every panel of this scene (group ID from describe_storyboard)."});
    fields
}

fn path_field(extension: &str) -> Value {
    json!({"type":"string","minLength":2,"maxLength":4096,"description":format!("Absolute path of the .{extension} file to write; an existing file is replaced.")})
}

fn profile_options() -> Value {
    let align = json!({"enum":["left","center","right"]});
    let pt = json!({"type":"number","minimum":4,"maximum":36});
    let header = json!({"type":"string","maxLength":200,"description":"Tokens: {project} {act} {seq} {scene} {panel} {name} {index} {frames} {duration} {timecode} {shot} {angle} {status}; {index:3} pads numbers. Empty hides the line."});
    let page_text = json!({"type":"string","maxLength":200,"description":"Tokens: {project} {page} {pages} {date} and the page's first {act} {seq} {scene}. Empty hides it."});
    json!({
        "type":"object",
        "additionalProperties":false,
        "description":"Options laid over the named profile (or the defaults). Lengths in mm, sizes in points.",
        "properties":{
            "landscape":{"type":"boolean"},
            "paper":{"enum":emulsion_io::printing::Paper::pdf().into_iter().map(|p| p.id).collect::<Vec<_>>()},
            "margin_mm":{"type":"number","minimum":0,"maximum":50},
            "columns":{"type":"integer","minimum":1,"maximum":6},
            "rows":{"type":"integer","minimum":1,"maximum":8},
            "gutter_mm":{"type":"number","minimum":0,"maximum":50},
            "fit":{"enum":["fit","fill"],"description":"fit shows the whole panel; fill crops it to its box."},
            "panel_frame_mm":{"type":"number","minimum":0,"maximum":5},
            "panel_header":header,
            "second_panel_header":header,
            "panel_header_align":align,
            "panel_header_pt":pt,
            "captions":{"enum":["none","below","right","left"]},
            "caption_percent":{"type":"number","minimum":10,"maximum":80,"description":"Share of each panel's box given to captions."},
            "caption_frames":{"type":"boolean"},
            "caption_titles":{"type":"boolean","description":"Start each caption with its field name in bold."},
            "caption_fields":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":200},"description":"Caption fields to print; empty prints the fields marked for printing."},
            "caption_pt":pt,
            "page_header":page_text,
            "page_header_align":align,
            "page_footer":page_text,
            "page_footer_align":align,
            "page_text_pt":pt,
            "logo":{"type":"string","maxLength":4096,"description":"Absolute path of a PNG or JPEG logo for the page header."},
            "logo_align":align,
            "logo_height_mm":{"type":"number","minimum":3,"maximum":40},
            "camera_frame":{"type":"boolean"},
            "safe_areas":{"type":"boolean","description":"Draw the board's action and title safe areas."},
            "camera_frame_mm":{"type":"number","minimum":0.05,"maximum":5},
            "camera_arrow_mm":{"type":"number","minimum":0.05,"maximum":5}
        }
    })
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let names: Vec<_> = builtins().into_iter().map(|p| p.name).collect();
    vec![
        def(
            "list_storyboard_pdf_profiles",
            "List the storyboard PDF layout profiles: the built-in ones and those saved in Emulsion, with all their options.",
            json!({}),
            &[],
        ),
        def(
            "export_storyboard_pdf",
            "Write the storyboard as a PDF board: panels per page, captions with their formatting (left, right or below, optionally framed), scene/panel headers with durations, a page header and footer with page numbers and date, an optional logo, and camera frames. `profile` names a built-in or saved profile; `options` change any of its options for this export. Returns the page count.",
            scope_fields(json!({
                "path":path_field("pdf"),
                "profile":{"type":"string","minLength":1,"maxLength":100,"description":format!("Built-in ({}) or saved profile name. Default: the first built-in.", names.join(", "))},
                "options":profile_options(),
                "title":{"type":"string","maxLength":200,"description":"Project name for the {project} token. Default: Storyboard."}
            })),
            &["path"],
        ),
        def(
            "export_storyboard_images",
            "Write each panel as a PNG or JPEG into a folder (created if needed), named by a pattern such as {seq}_{scene}_{panel}. Tokens: {project} {act} {seq} {scene} {panel} (number in its scene) {name} {index} {frames} {duration} {timecode} {shot} {angle} {status}, and {layer} with per_layer; {index:3} pads numbers with zeros. per_layer writes one image per visible top-level layer of each panel. A pattern that gives two files one name writes nothing. Returns the files.",
            scope_fields(json!({
                "directory":{"type":"string","minLength":2,"maxLength":4096,"description":"Absolute folder path."},
                "pattern":{"type":"string","minLength":1,"maxLength":200},
                "format":{"enum":["png","jpeg"]},
                "per_layer":{"type":"boolean"},
                "jpeg_quality":{"type":"integer","minimum":1,"maximum":100},
                "title":{"type":"string","maxLength":200,"description":"Value of {project}. Default: Storyboard."}
            })),
            &["directory"],
        ),
        def(
            "export_storyboard_csv",
            "Write a CSV with one row per panel: index, act, sequence, scene and panel names, duration in frames and seconds, start/end/duration timecode at the board's frame rate, every caption field as plain text, shot size, angle, status, tag, lock and thumbnail-sheet flag. Quoted as RFC 4180; UTF-8. Returns the row count.",
            scope_fields(json!({"path":path_field("csv")})),
            &["path"],
        ),
    ]
}

/// An absolute output path with the expected extension.
fn output(args: &Value, key: &str, extension: Option<&str>) -> Result<std::path::PathBuf, String> {
    let path = Path::new(args[key].as_str().unwrap_or_default());
    if !path.is_absolute() {
        return Err(format!("Use an absolute {key}."));
    }
    if let Some(extension) = extension
        && !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(extension))
    {
        return Err(format!("The path must end in .{extension}."));
    }
    if let Some(parent) = path.parent()
        && extension.is_some()
        && !parent.is_dir()
    {
        return Err(format!("The folder {} does not exist.", parent.display()));
    }
    Ok(path.to_path_buf())
}

fn scope(args: &Value, board: &Storyboard) -> Result<Scope, String> {
    match (args.get("panels"), args["scene"].as_u64()) {
        (Some(_), Some(_)) => Err("Use either panels or scene.".into()),
        (Some(ids), None) => Ok(Scope::Panels(super::ids(ids))),
        (None, Some(scene)) if board.scenes.contains_key(&scene) => Ok(Scope::Scene(scene)),
        (None, Some(scene)) => Err(format!("No scene has ID {scene}.")),
        (None, None) => Ok(Scope::All),
    }
}

/// Saved profiles from the app's settings.
fn saved() -> Vec<Profile> {
    emulsion_io::settings::Settings::load().storyboard_pdf_profiles
}

/// The named profile with `options` laid over it, validated.
fn profile(args: &Value) -> Result<Profile, String> {
    let base = match args["profile"].as_str() {
        None => builtins().remove(0),
        Some(name) => builtin(name)
            .or_else(|| {
                saved()
                    .into_iter()
                    .find(|p| p.name.eq_ignore_ascii_case(name))
            })
            .ok_or_else(|| {
                let names: Vec<_> = builtins()
                    .into_iter()
                    .chain(saved())
                    .map(|p| p.name)
                    .collect();
                format!("Unknown profile '{name}'. Profiles: {}", names.join(", "))
            })?,
    };
    let mut value = serde_json::to_value(&base).map_err(|e| e.to_string())?;
    if let Some(options) = args["options"].as_object() {
        for (key, option) in options {
            if key == "paper" {
                let id = option.as_str().unwrap_or_default();
                let paper = emulsion_io::printing::Paper::pdf()
                    .into_iter()
                    .find(|p| p.id == id)
                    .ok_or("Unknown paper")?;
                value["paper"] = serde_json::to_value(paper).map_err(|e| e.to_string())?;
            } else {
                value[key] = option.clone();
            }
        }
    }
    let profile: Profile = serde_json::from_value(value).map_err(|e| e.to_string())?;
    profile.validate().map_err(|e| e.to_string())?;
    Ok(profile)
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let title = args["title"].as_str().unwrap_or("Storyboard");
    let cancel = AtomicBool::new(false);
    let result = match name {
        "list_storyboard_pdf_profiles" => Ok(json!({
            "built_in":builtins(),
            "saved":saved(),
        })),
        "export_storyboard_pdf" => (|| {
            let path = output(args, "path", Some("pdf"))?;
            let profile = profile(args)?;
            let scope = scope(args, board)?;
            let project = editor.snapshot().ok_or("Open a storyboard first.")?;
            let pages = export::sheet::write_pdf(&project, title, &scope, &profile, &path, &cancel)
                .map_err(|e| e.to_string())?;
            Ok(json!({"path":path,"pages":pages,"profile":profile.name}))
        })(),
        "export_storyboard_images" => (|| {
            let directory = output(args, "directory", None)?;
            let mut options = images::Options {
                per_layer: args["per_layer"] == true,
                ..Default::default()
            };
            if let Some(pattern) = args["pattern"].as_str() {
                options.pattern = pattern.into();
            }
            if args["format"] == "jpeg" {
                options.format = images::Format::Jpeg;
            }
            if let Some(quality) = args["jpeg_quality"].as_u64() {
                options.jpeg_quality = quality as u8;
            }
            options.validate().map_err(|e| e.to_string())?;
            let scope = scope(args, board)?;
            let project = editor.snapshot().ok_or("Open a storyboard first.")?;
            let files = images::write(&project, title, &scope, &options, &directory, &cancel)
                .map_err(|e| e.to_string())?;
            Ok(json!({"directory":directory,"files":files}))
        })(),
        "export_storyboard_csv" => (|| {
            let path = output(args, "path", Some("csv"))?;
            let scope = scope(args, board)?;
            let project = editor.snapshot().ok_or("Open a storyboard first.")?;
            let rows = export::csv::write(&project, &scope, &path).map_err(|e| e.to_string())?;
            Ok(json!({"path":path,"rows":rows}))
        })(),
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::super::execute;
    use super::super::tests::{board, call};
    use serde_json::json;
    use std::path::{Path, PathBuf};

    /// A fresh folder, removed when dropped.
    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("sb-export-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn exports_write_pdf_images_and_csv_without_changing_the_board() {
        let mut e = board();
        call(
            &mut e,
            "add_storyboard_panels",
            json!({"panels":[{"captions":{"Action":"Mia, \"runs\""}},{"seconds":1}]}),
        );
        let stamp = e.stamp();
        let dir = Dir::new("ok");
        let pdf = dir.path().join("board.pdf");
        let result = call(
            &mut e,
            "export_storyboard_pdf",
            json!({"path":pdf,"profile":"1 per page · large","options":{"captions":"right","paper":"Letter","page_footer":"{page}/{pages}"}}),
        );
        assert_eq!(result["pages"], 3);
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
        let frames = dir.path().join("frames");
        let result = call(
            &mut e,
            "export_storyboard_images",
            json!({"directory":frames,"pattern":"{scene}_{index:3}","format":"jpeg"}),
        );
        assert_eq!(result["files"].as_array().unwrap().len(), 3);
        assert!(frames.join("1_001.jpg").is_file());
        let csv = dir.path().join("board.csv");
        let result = call(
            &mut e,
            "export_storyboard_csv",
            json!({"path":csv,"panels":[2]}),
        );
        assert_eq!(result["rows"], 1);
        let text = std::fs::read_to_string(&csv).unwrap();
        assert!(text.contains("\"Mia, \"\"runs\"\"\""), "{text}");
        let profiles = call(&mut e, "list_storyboard_pdf_profiles", json!({}));
        assert_eq!(profiles["built_in"].as_array().unwrap().len(), 3);
        assert_eq!(e.stamp(), stamp, "exports never change the board");
    }

    #[test]
    fn invalid_exports_write_nothing_and_change_nothing() {
        let mut e = board();
        let stamp = e.stamp();
        let dir = Dir::new("invalid");
        let pdf = dir.path().join("x.pdf");
        let csv = dir.path().join("x.csv");
        let images = dir.path().join("images");
        for (name, args) in [
            ("export_storyboard_pdf", json!({"path":"board.pdf"})),
            (
                "export_storyboard_pdf",
                json!({"path":dir.path().join("x.txt")}),
            ),
            (
                "export_storyboard_pdf",
                json!({"path":dir.path().join("missing/x.pdf")}),
            ),
            (
                "export_storyboard_pdf",
                json!({"path":pdf,"profile":"Nope"}),
            ),
            (
                "export_storyboard_pdf",
                json!({"path":pdf,"options":{"rows":0}}),
            ),
            (
                "export_storyboard_pdf",
                json!({"path":pdf,"options":{"colour":1}}),
            ),
            (
                "export_storyboard_pdf",
                json!({"path":pdf,"options":{"panel_header":"{page}"}}),
            ),
            (
                "export_storyboard_pdf",
                json!({"path":pdf,"options":{"logo":"logo.png"}}),
            ),
            ("export_storyboard_pdf", json!({"path":pdf,"scene":99})),
            ("export_storyboard_pdf", json!({"path":pdf,"panels":[99]})),
            (
                "export_storyboard_pdf",
                json!({"path":pdf,"panels":[1],"scene":1}),
            ),
            ("export_storyboard_images", json!({"directory":"frames"})),
            (
                "export_storyboard_images",
                json!({"directory":images,"pattern":"{nope}"}),
            ),
            (
                "export_storyboard_images",
                json!({"directory":images,"pattern":"{layer}"}),
            ),
            (
                "export_storyboard_images",
                json!({"directory":images,"format":"gif"}),
            ),
            (
                "export_storyboard_csv",
                json!({"path":dir.path().join("x.pdf")}),
            ),
            ("export_storyboard_csv", json!({"path":csv,"panels":[]})),
        ] {
            assert!(execute(&mut e, name, &args).is_error, "{name} {args}");
            assert_eq!(e.stamp(), stamp, "{name}");
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
