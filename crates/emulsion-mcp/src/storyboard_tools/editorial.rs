//! Editorial interchange: export the animatic as an EDL, XML edit list
//! (xmeml) or OpenTimelineIO edit with per-panel media, and conform the board
//! to an edit made in editing software (durations, order, transitions and
//! sound) as one Undo step, with a dry run first.
use super::def;
use crate::ToolDef;
use emulsion_core::{
    project::ProjectEditor, storyboard::Storyboard, storyboard_conform::RateChoice,
};
use emulsion_io::editorial::{self, ExportOptions, Format, MediaKind};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::atomic::AtomicBool;

pub(super) fn definitions() -> Vec<ToolDef> {
    vec![
        def(
            "export_storyboard_edit",
            "Export the animatic for editing software: a CMX 3600 EDL (.edl), XML edit list (xmeml .xml, read by most editing applications) or OpenTimelineIO (.otio). Panels play end to end on V1 from 01:00:00:00 (drop-frame timecode at 29.97/59.94) with their dissolves and wipes, sound clips on A1… with gain (not in EDLs), reference video on V2 and markers (XML and OTIO). Each panel is written as a PNG still or a ProRes movie (with camera and layer motion; needs FFmpeg) into a `<name>_media` folder beside the edit, with the sounds and videos; clip names are panel names so conform_storyboard_to_edit can match them. Returns the clip count, media files and warnings about what the format cannot hold.",
            json!({
                "path":{"type":"string","minLength":2,"maxLength":4096,"description":"Absolute path of the .edl, .xml or .otio file; an existing file is replaced."},
                "format":{"enum":["edl","xml","otio"],"description":"Default: from the path's extension."},
                "media":{"enum":["still","movie"],"description":"Per-panel media. Default still."},
                "width":{"type":"integer","minimum":16,"maximum":8192,"description":"Media width in pixels. Default 1920."},
                "title":{"type":"string","maxLength":200,"description":"Sequence name. Default: Storyboard."}
            }),
            &["path"],
        ),
        def(
            "conform_storyboard_to_edit",
            "Conform the storyboard to an edit made in editing software (.edl, .xml or .otio): picture clips are matched to panels by name (or by the panel ID in media file names this app exported), then panels take the edit's durations, order (panels the edit moves join the scene they land in) and transitions, and sound clips that play the board's sounds replace the board's sound clips (keeping their fades and effects). Clips with no panel and panels left out are reported; left-out panels keep their place. Use dry_run first to see the report, then apply. One Undo step.",
            json!({
                "path":{"type":"string","minLength":2,"maxLength":4096,"description":"Absolute path of the edit file."},
                "frame_rate":{"enum":["convert","keep"],"description":"When the edit's rate differs from the board's: convert keeps times (default), keep keeps frame counts. EDLs carry no rate and are read at the board's."},
                "dry_run":{"type":"boolean","description":"Report what would change without changing the board."}
            }),
            &["path"],
        ),
    ]
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let path = Path::new(args["path"].as_str().unwrap_or_default());
    let result = match name {
        "export_storyboard_edit" => (|| {
            let format = match args["format"].as_str() {
                Some("edl") => Format::Edl,
                Some("xml") => Format::Xmeml,
                Some("otio") => Format::Otio,
                _ => Format::from_path(path)
                    .ok_or("End the path in .edl, .xml or .otio, or set format.")?,
            };
            let path = super::export::output(args, "path", Some(format.extension()))?;
            let options = ExportOptions {
                format,
                media: if args["media"] == "movie" {
                    MediaKind::Movie
                } else {
                    MediaKind::Still
                },
                width: args["width"].as_u64().unwrap_or(1920) as u32,
            };
            let project = editor.snapshot().ok_or("Open a storyboard first.")?;
            let report = editorial::export(
                &project,
                args["title"].as_str().unwrap_or("Storyboard"),
                &options,
                &path,
                &mut |_, _| {},
                &AtomicBool::new(false),
            )
            .map_err(|e| format!("{e:#}"))?;
            serde_json::to_value(report).map_err(|e| e.to_string())
        })(),
        "conform_storyboard_to_edit" => (|| {
            if !path.is_absolute() {
                return Err("Use an absolute path.".to_string());
            }
            let edit =
                editorial::read(path, board.settings.frame_rate).map_err(|e| format!("{e:#}"))?;
            let choice = if args["frame_rate"] == "keep" {
                RateChoice::Keep
            } else {
                RateChoice::Convert
            };
            let dry_run = args["dry_run"] == true;
            let report = editor.conform_storyboard(&edit, choice, !dry_run)?;
            let mut out = serde_json::to_value(&report).map_err(|e| e.to_string())?;
            out["dry_run"] = json!(dry_run);
            out["summary"] = json!(report.summary());
            Ok(out)
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

    #[test]
    fn an_exported_edit_conforms_back_after_editing() {
        let dir = std::env::temp_dir().join(format!("sb-editorial-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut e = board();
        let first = e.active_page();
        call(
            &mut e,
            "add_storyboard_panels",
            json!({"after":first,"panels":[{"frames":24},{"frames":12}]}),
        );
        let names: Vec<_> = e.page_list().iter().map(|m| m.name.clone()).collect();
        let path = dir.join("Film.edl");
        let out = call(
            &mut e,
            "export_storyboard_edit",
            json!({"path":path.to_str().unwrap(),"width":64}),
        );
        assert_eq!(out["clips"], 3);
        assert_eq!(out["files"].as_array().unwrap().len(), 3);
        // An editor swaps the last two panels and trims the first.
        let text = std::fs::read_to_string(&path).unwrap();
        let mut edit = emulsion_io::editorial::parse(
            &text,
            emulsion_io::editorial::Format::Edl,
            emulsion_core::storyboard::FrameRate::whole(24),
        )
        .unwrap();
        let (a, b, c) = (
            edit.video[0].clone(),
            edit.video[1].clone(),
            edit.video[2].clone(),
        );
        let a_frames = 30;
        edit.video = vec![
            emulsion_core::timeline::EditClip {
                record_out: a_frames,
                ..a
            },
            emulsion_core::timeline::EditClip {
                record_in: a_frames,
                record_out: a_frames + c.frames(),
                ..c.clone()
            },
            emulsion_core::timeline::EditClip {
                record_in: a_frames + c.frames(),
                record_out: a_frames + c.frames() + b.frames(),
                ..b
            },
        ];
        let otio = dir.join("Cut.otio");
        std::fs::write(&otio, emulsion_io::editorial::otio::write(&edit)).unwrap();
        let dry = call(
            &mut e,
            "conform_storyboard_to_edit",
            json!({"path":otio.to_str().unwrap(),"dry_run":true}),
        );
        assert_eq!(dry["matched"], 3);
        assert_eq!(dry["moved"].as_array().unwrap().len(), 1);
        assert_eq!(dry["retimed"].as_array().unwrap().len(), 1);
        assert_eq!(
            e.page_list()
                .iter()
                .map(|m| m.name.clone())
                .collect::<Vec<_>>(),
            names
        );
        call(
            &mut e,
            "conform_storyboard_to_edit",
            json!({"path":otio.to_str().unwrap()}),
        );
        let after: Vec<_> = e.page_list().iter().map(|m| m.name.clone()).collect();
        assert_eq!(
            after,
            [names[0].clone(), names[2].clone(), names[1].clone()]
        );
        assert!(e.undo());
        assert_eq!(
            e.page_list()
                .iter()
                .map(|m| m.name.clone())
                .collect::<Vec<_>>(),
            names
        );
        assert!(
            execute(
                &mut e,
                "conform_storyboard_to_edit",
                &json!({"path":"Cut.otio"})
            )
            .is_error
        );
    }
}
