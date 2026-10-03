//! Storyboard sound import and animatic exports: bring a sound file into the
//! board's sound library, and write the animatic as a movie (H.264 MP4,
//! ProRes MOV or a PNG sequence, with the timeline's sound) or an animated
//! GIF, with the render area and burn-in chosen. Exports read the live board
//! and never change it.
use super::def;
use crate::ToolDef;
use emulsion_core::{
    project::ProjectEditor,
    storyboard::Storyboard,
    storyboard_animatic::{BurnIn, RenderArea},
};
use emulsion_io::storyboard_export::movie::{self, GifOptions, MovieFormat, MovieOptions};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn range_fields(mut fields: Value) -> Value {
    fields["start_frame"] = json!({"type":"integer","minimum":0,"description":"First animatic frame (see describe_storyboard `start`). Default 0."});
    fields["end_frame"] = json!({"type":"integer","minimum":1,"description":"Frame after the last one to export. Default: the end of the animatic."});
    fields["render_area"] = json!({"enum":["camera","overscan","all_artwork"],"description":"camera shows each panel as drawn (default); overscan adds the board's overscan margin; all_artwork widens to everything drawn on any panel."});
    fields["burn_in"] = json!({
        "type":"object",
        "additionalProperties":false,
        "description":"Draw text over the pictures. Omit for clean pictures.",
        "properties":{
            "timecode":{"type":"boolean","description":"Default true."},
            "scene":{"type":"boolean","description":"Default true."},
            "panel":{"type":"boolean","description":"Default true."},
            "caption":{"type":"string","maxLength":200,"description":"A caption field to show, such as Dialogue."},
            "position":{"enum":["top","bottom"]},
            "size":{"type":"number","minimum":1,"maximum":20,"description":"Text height as % of the frame height. Default 4."}
        }
    });
    fields
}

pub(super) fn definitions() -> Vec<ToolDef> {
    vec![
        def(
            "import_storyboard_sound",
            "Import a sound file (WAV, MP3, M4A, AAC, FLAC, OGG, Opus or AIFF) into the storyboard's sound library. The file is copied into the project, so later changes to it do not matter; it is saved inside the .emu package. Needs FFmpeg. Returns the sound ID (for place_storyboard_sound), name, folder and duration. One Undo step.",
            json!({
                "path":{"type":"string","minLength":2,"maxLength":4096,"description":"Absolute path of the sound file."},
                "folder":{"type":"string","maxLength":400,"description":"Library folder, `/`-separated, such as Dialogue/Scene 1. Default: the top level."},
                "name":{"type":"string","minLength":1,"maxLength":200,"description":"Name in the library. Default: the file name."}
            }),
            &["path"],
        ),
        def(
            "export_storyboard_movie",
            "Export the animatic (panels end to end, with their transitions) as a movie with the timeline's sound mixed in: H.264 MP4 (.mp4), ProRes 422 MOV (.mov), or a folder of PNG frames named frame_00000.png plus soundtrack.wav. Choose a frame range, output width, render area and burn-in text. Needs FFmpeg for MP4 and MOV. An existing file is replaced only when the export succeeds. Returns frames, seconds, size and whether it has sound.",
            range_fields(json!({
                "path":{"type":"string","minLength":2,"maxLength":4096,"description":"Absolute path: a .mp4 or .mov file, or a folder for png_sequence."},
                "format":{"enum":["mp4","mov","png_sequence"],"description":"Default: from the path's extension."},
                "width":{"type":"integer","minimum":16,"maximum":8192,"description":"Output width in pixels; the height follows the render area. Default: the render area's width (at most 3840)."},
                "quality":{"type":"integer","minimum":1,"maximum":100,"description":"Default 80."},
                "audio":{"type":"boolean","description":"Include the timeline's sound. Default true."},
                "reference_video":{"enum":["none","overlay","picture_in_picture"],"description":"Draw the timeline's reference video: overlay fits it over the frame with each clip's opacity; picture_in_picture insets it at the bottom right. Default none."}
            })),
            &["path"],
        ),
        def(
            "export_storyboard_gif",
            "Export the animatic as a looping animated GIF, sampled at the GIF frame rate, with transitions, render area and optional burn-in. Returns frames and size.",
            range_fields(json!({
                "path":{"type":"string","minLength":2,"maxLength":4096,"description":"Absolute path of the .gif file."},
                "width":{"type":"integer","minimum":16,"maximum":1920,"description":"Default 640."},
                "fps":{"type":"integer","minimum":1,"maximum":50,"description":"GIF frames per second. Default 12."}
            })),
            &["path"],
        ),
    ]
}

/// Range, render area and burn-in shared by both exports.
fn common(args: &Value) -> Result<(u64, Option<u64>, RenderArea, Option<BurnIn>), String> {
    let area = match args["render_area"].as_str() {
        None | Some("camera") => RenderArea::Camera,
        Some("overscan") => RenderArea::Overscan,
        Some("all_artwork") => RenderArea::AllArtwork,
        Some(other) => return Err(format!("Unknown render area '{other}'.")),
    };
    let burn_in = match args.get("burn_in") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let burn: BurnIn = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            burn.validate()?;
            Some(burn)
        }
    };
    Ok((
        args["start_frame"].as_u64().unwrap_or(0),
        args["end_frame"].as_u64(),
        area,
        burn_in,
    ))
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    _board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let cancel = AtomicBool::new(false);
    let result = match name {
        "import_storyboard_sound" => (|| {
            let path = Path::new(args["path"].as_str().unwrap_or_default());
            if !path.is_absolute() {
                return Err("Use an absolute path.".to_string());
            }
            let mut asset = emulsion_io::audio::store::import(
                path,
                args["folder"].as_str().unwrap_or_default(),
            )
            .map_err(|e| e.to_string())?;
            if let Some(name) = args["name"].as_str() {
                asset.name = name.trim().into();
            }
            let summary = json!({"name":asset.name,"folder":asset.folder,"duration_ms":asset.duration_ms,"sample_rate":asset.sample_rate,"channels":asset.channels});
            let mut id = 0;
            editor.edit_storyboard(|b| {
                let mut timeline = b.timeline.clone();
                id = timeline.add_asset(asset)?;
                timeline.validate()?;
                b.timeline = timeline;
                Ok(())
            })?;
            let mut out = summary;
            out["sound"] = json!(id);
            Ok(out)
        })(),
        "export_storyboard_movie" => (|| {
            let path = Path::new(args["path"].as_str().unwrap_or_default());
            if !path.is_absolute() {
                return Err("Use an absolute path.".to_string());
            }
            let extension = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase());
            let format = match (args["format"].as_str(), extension.as_deref()) {
                (Some("mp4"), _) | (None, Some("mp4")) => MovieFormat::Mp4,
                (Some("mov"), _) | (None, Some("mov")) => MovieFormat::Mov,
                (Some("png_sequence"), _) => MovieFormat::PngSequence,
                (None, _) => {
                    return Err("End the path in .mp4 or .mov, or set format.".to_string());
                }
                (Some(other), _) => return Err(format!("Unknown format '{other}'.")),
            };
            if format.extension().is_some() {
                super::export::output(args, "path", format.extension())?;
            }
            let (start, end, area, burn_in) = common(args)?;
            let options = MovieOptions {
                format,
                width: args["width"].as_u64().unwrap_or(0) as u32,
                start,
                end,
                area,
                burn_in,
                quality: args["quality"].as_u64().unwrap_or(80).min(255) as u8,
                audio: args["audio"] != false,
                reference_video: match args["reference_video"].as_str() {
                    Some("overlay") => Some(emulsion_core::timeline::VideoPlacement::Overlay),
                    Some("picture_in_picture") => {
                        Some(emulsion_core::timeline::VideoPlacement::PictureInPicture)
                    }
                    _ => None,
                },
            };
            let project = editor.snapshot().ok_or("Open a storyboard first.")?;
            let report = movie::write_movie(&project, &options, path, &mut |_, _| {}, &cancel)
                .map_err(|e| e.to_string())?;
            let mut out = json!({"path":path,"frames":report.frames,"seconds":report.seconds,"width":report.width,"height":report.height,"audio":report.audio});
            if format == MovieFormat::PngSequence {
                out["files"] = json!(report.files.len());
            }
            Ok(out)
        })(),
        "export_storyboard_gif" => (|| {
            let path = super::export::output(args, "path", Some("gif"))?;
            let (start, end, area, burn_in) = common(args)?;
            let options = GifOptions {
                width: args["width"].as_u64().unwrap_or(640) as u32,
                fps: args["fps"].as_u64().unwrap_or(12) as u32,
                start,
                end,
                area,
                burn_in,
            };
            let project = editor.snapshot().ok_or("Open a storyboard first.")?;
            let report = movie::write_gif(&project, &options, &path, &mut |_, _| {}, &cancel)
                .map_err(|e| e.to_string())?;
            Ok(
                json!({"path":path,"frames":report.frames,"seconds":report.seconds,"width":report.width,"height":report.height}),
            )
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
    use std::process::{Command, Stdio};

    /// A fresh folder, removed when dropped.
    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("sb-animatic-{tag}-{}", std::process::id()));
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

    fn ffmpeg() -> bool {
        let ok = emulsion_io::ffmpeg::available();
        if !ok {
            eprintln!("FFmpeg unavailable; skipping");
        }
        ok
    }

    #[test]
    fn sounds_import_and_the_animatic_exports_as_movie_and_gif() {
        let mut e = board();
        call(
            &mut e,
            "add_storyboard_panels",
            json!({"panels":[{"frames":12}]}),
        );
        let dir = Dir::new("ok");
        let gif = dir.path().join("board.gif");
        let stamp = e.stamp();
        let result = call(
            &mut e,
            "export_storyboard_gif",
            json!({"path":gif,"width":32,"fps":6,"burn_in":{"size":10}}),
        );
        assert_eq!(result["frames"], 15, "60 frames at 24 fps is 2.5 s");
        assert_eq!(result["height"], 18);
        assert!(gif.is_file());
        assert_eq!(e.stamp(), stamp, "exports never change the board");
        if !ffmpeg() {
            return;
        }
        let wav = dir.path().join("Line 1.wav");
        assert!(
            Command::new("ffmpeg")
                .args(["-nostdin", "-v", "error", "-f", "lavfi", "-i"])
                .arg("sine=frequency=330:duration=1")
                .arg(&wav)
                .stdin(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        let sound = call(
            &mut e,
            "import_storyboard_sound",
            json!({"path":wav,"folder":"Dialogue"}),
        );
        assert_eq!(sound["name"], "Line 1");
        assert_eq!(sound["folder"], "Dialogue");
        let id = sound["sound"].as_u64().unwrap();
        let timeline = &e.storyboard().unwrap().timeline;
        assert!(timeline.assets[&id].source.as_ref().unwrap().is_file());
        assert!(e.undo());
        assert!(e.storyboard().unwrap().timeline.assets.is_empty());
        assert!(e.redo());
        let mp4 = dir.path().join("board.mp4");
        let result = call(
            &mut e,
            "export_storyboard_movie",
            json!({"path":mp4,"width":64,"start_frame":12,"end_frame":36,"render_area":"overscan"}),
        );
        assert_eq!(result["frames"], 24);
        assert_eq!(result["audio"], false, "no clip is placed");
        let info = emulsion_io::video_export::probe(&mp4).unwrap();
        assert_eq!(info.frames, 24);
        assert_eq!(info.width, 64);
    }

    #[test]
    fn invalid_imports_and_exports_change_nothing() {
        let mut e = board();
        let stamp = e.stamp();
        let dir = Dir::new("invalid");
        let text = dir.path().join("notes.txt");
        std::fs::write(&text, "hi").unwrap();
        let mp4 = dir.path().join("x.mp4");
        let gif = dir.path().join("x.gif");
        for (name, args) in [
            ("import_storyboard_sound", json!({"path":"line.wav"})),
            ("import_storyboard_sound", json!({"path":text})),
            (
                "import_storyboard_sound",
                json!({"path":dir.path().join("missing.wav")}),
            ),
            ("export_storyboard_movie", json!({"path":"x.mp4"})),
            (
                "export_storyboard_movie",
                json!({"path":dir.path().join("x.avi")}),
            ),
            (
                "export_storyboard_movie",
                json!({"path":mp4,"format":"mov"}),
            ),
            (
                "export_storyboard_movie",
                json!({"path":dir.path().join("missing/x.mp4")}),
            ),
            (
                "export_storyboard_movie",
                json!({"path":mp4,"start_frame":40,"end_frame":20}),
            ),
            (
                "export_storyboard_movie",
                json!({"path":mp4,"end_frame":100000}),
            ),
            (
                "export_storyboard_movie",
                json!({"path":mp4,"burn_in":{"size":50}}),
            ),
            ("export_storyboard_gif", json!({"path":mp4})),
            ("export_storyboard_gif", json!({"path":gif,"fps":0})),
            (
                "export_storyboard_gif",
                json!({"path":gif,"render_area":"stage"}),
            ),
        ] {
            assert!(execute(&mut e, name, &args).is_error, "{name} {args}");
            assert_eq!(e.stamp(), stamp, "{name}");
        }
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["notes.txt"]);
    }
}
