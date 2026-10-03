//! Reference video tools: import a video file as a clip on a video track
//! (optionally with its sound), move, trim, fade, hide, lock and delete
//! video clips, and the `video` section of describe_storyboard. Positions
//! and lengths take frames, seconds or timecode like the audio tools.
use super::timing::{at_fields, indices, length, length_fields, position, seconds, timecode};
use super::{def, layout};
use crate::ToolDef;
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::storyboard::Storyboard;
use emulsion_core::timeline::VideoClip;
use emulsion_core::timeline::video::{MAX_VIDEO_CLIPS, MAX_VIDEO_TRACKS};
use serde_json::{Value, json};
use std::path::Path;

fn track_number() -> Value {
    json!({"type":"integer","minimum":1,"maximum":MAX_VIDEO_TRACKS,"description":"Video track number from describe_storyboard `video` (1 = first)."})
}

fn clip_number() -> Value {
    json!({"type":"integer","minimum":1,"maximum":MAX_VIDEO_CLIPS,"description":"Clip number on its video track (1 = earliest)."})
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let mut import = at_fields();
    import["path"] = json!({"type":"string","minLength":2,"maxLength":4096,"description":"Absolute path of the video file (.mp4, .mov, .m4v, .mkv, .webm or .avi)."});
    import["track"] = json!({"type":"integer","minimum":1,"maximum":MAX_VIDEO_TRACKS,"description":"Video track number. Default: the first track with room there (a new track when none has)."});
    import["with_audio"] = json!({"type":"boolean","description":"Also import the video's sound as a library sound and line a clip of it up under the video on the first audio track with room. Default false."});
    let mut update = at_fields();
    for (key, value) in length_fields("Length on the timeline").as_object().unwrap() {
        update[key] = value.clone();
    }
    update["track"] = track_number();
    update["clip"] = clip_number();
    update["to_track"] = track_number();
    update["name"] = json!({"type":"string","minLength":1,"maxLength":200});
    update["offset_ms"] = json!({"type":"integer","minimum":0,"description":"In point: where in the video the clip starts, in milliseconds."});
    update["opacity"] = json!({"type":"number","minimum":0,"maximum":1,"description":"How strongly the picture shows over the panels."});
    update["visible"] = json!({"type":"boolean"});
    update["locked"] = json!({"type":"boolean","description":"Locked clips refuse every other change until unlocked."});
    vec![
        def(
            "import_storyboard_video",
            "Import a video file as a reference clip on a video track (at most 4), starting at a frame, timecode, second or a panel's first frame (default 0) and lasting the whole video. The file is copied into the project and saved in the .emu package (up to 2 GiB of video). with_audio also brings in its sound, lined up on an audio track. The person sees it over the Stage and the player; export_storyboard_movie draws it with reference_video. Needs FFmpeg. Video clips stay at their frames when panels are retimed. Returns the clip's track, number, length and the video's size and frame rate. One Undo step.",
            import,
            &["path"],
        ),
        def(
            "update_storyboard_video_clip",
            "Move, trim or change a reference video clip: a new start (at, at_timecode, at_seconds, at_panel), another video track (to_track), a new length (frames, seconds, timecode; at most to the end of the video), offset_ms (in point), name, opacity, visible or locked. To trim the head, raise offset_ms and the start together. Clips on one track cannot overlap. Returns the clip's track and number. One Undo step.",
            update,
            &["track", "clip"],
        ),
        def(
            "delete_storyboard_video_clips",
            "Delete reference video clips from a video track by number; a video no clip uses leaves the project. Locked clips refuse. An emptied track stays; delete_track removes it. One Undo step.",
            json!({"track":track_number(),"clips":{"type":"array","items":clip_number(),"minItems":1,"maxItems":MAX_VIDEO_CLIPS},"delete_track":{"type":"boolean","description":"Remove the track itself (with any clips left on it)."}}),
            &["track", "clips"],
        ),
    ]
}

/// The `video` section of describe_storyboard: tracks with their clips and
/// the videos they show.
pub(super) fn video_json(board: &Storyboard) -> Value {
    let timeline = &board.timeline;
    let tracks: Vec<Value> = timeline
        .video
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let clips: Vec<Value> = track
                .clips
                .iter()
                .enumerate()
                .map(|(n, c)| {
                    json!({
                        "clip":n + 1,
                        "name":c.name,
                        "video":c.asset,
                        "start":c.start,
                        "start_timecode":timecode(board, c.start),
                        "frames":c.frames,
                        "seconds":seconds(board, c.frames),
                        "offset_ms":c.offset_ms,
                        "opacity":c.opacity,
                        "visible":c.visible,
                        "locked":c.locked,
                    })
                })
                .collect();
            json!({"track":i + 1,"name":track.name,"clips":clips})
        })
        .collect();
    let videos: Vec<Value> = timeline
        .videos
        .iter()
        .map(|(id, a)| {
            json!({"video":id,"name":a.name,"format":a.format,"duration_ms":a.duration_ms,"fps":a.fps,"width":a.width,"height":a.height,"has_audio":a.has_audio})
        })
        .collect();
    json!({"tracks":tracks,"videos":videos,"end":timeline.video_end()})
}

fn video_track(board: &Storyboard, value: &Value) -> Result<usize, String> {
    let n = value.as_u64().unwrap_or(0) as usize;
    if n == 0 || n > board.timeline.video.len() {
        return Err(format!(
            "There is no video track {n}; the board has {}. import_storyboard_video makes one.",
            board.timeline.video.len()
        ));
    }
    Ok(n - 1)
}

fn clip_result(editor: &ProjectEditor, track: usize, index: usize) -> Value {
    let board = editor.storyboard().unwrap();
    let clip = &board.timeline.video[track].clips[index];
    json!({
        "track":track + 1,
        "clip":index + 1,
        "name":clip.name,
        "start":clip.start,
        "start_timecode":timecode(board, clip.start),
        "frames":clip.frames,
        "end":clip.end(),
    })
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let layout = layout(editor);
    Some(match name {
        "import_storyboard_video" => import(editor, board, &layout, args),
        "update_storyboard_video_clip" => update(editor, board, &layout, args),
        "delete_storyboard_video_clips" => delete(editor, board, args),
        _ => return None,
    })
}

fn import(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    layout: &[PageId],
    args: &Value,
) -> Result<Value, String> {
    let path = Path::new(args["path"].as_str().unwrap_or_default());
    if !path.is_absolute() {
        return Err("Use an absolute path.".into());
    }
    let start = position(board, layout, args)?.unwrap_or(0);
    let track = match args.get("track") {
        Some(value) => {
            let n = value.as_u64().unwrap_or(0) as usize;
            if n > board.timeline.video.len() + 1 {
                return Err(format!(
                    "There is no video track {n}; the board has {}.",
                    board.timeline.video.len()
                ));
            }
            Some(n - 1)
        }
        None => None,
    };
    let imported = emulsion_io::reference_video::import(path, args["with_audio"] == true)
        .map_err(|e| e.to_string())?;
    let rate = board.settings.frame_rate;
    let video = imported.video.clone();
    let with_sound = imported.sound.is_some();
    let mut at = (0, 0);
    editor.edit_storyboard(|b| {
        let mut timeline = b.timeline.clone();
        if let Some(t) = track
            && t == timeline.video.len()
        {
            timeline
                .video
                .push(emulsion_core::timeline::VideoTrack::new(&format!(
                    "Video {}",
                    t + 1
                )));
        }
        at = timeline.import_video(imported.video, imported.sound, start, rate, track)?;
        timeline.validate()?;
        b.timeline = timeline;
        Ok(())
    })?;
    let mut out = clip_result(editor, at.0, at.1);
    let id = editor.storyboard().unwrap().timeline.video[at.0].clips[at.1].asset;
    out["video"] = json!({"video":id,"duration_ms":video.duration_ms,"fps":video.fps,"width":video.width,"height":video.height,"has_audio":video.has_audio});
    out["sound_imported"] = json!(with_sound);
    Ok(out)
}

/// The clip at `track`/`clip` (1-based numbers in `args`), refusing locked
/// clips unless `unlocking`.
fn chosen(board: &Storyboard, args: &Value) -> Result<(usize, usize, VideoClip), String> {
    let t = video_track(board, &args["track"])?;
    let n = args["clip"].as_u64().unwrap_or(0) as usize;
    let clip = n
        .checked_sub(1)
        .and_then(|i| board.timeline.video[t].clips.get(i))
        .cloned()
        .ok_or_else(|| format!("There is no clip {n} on video track {}.", t + 1))?;
    Ok((t, n - 1, clip))
}

fn update(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    layout: &[PageId],
    args: &Value,
) -> Result<Value, String> {
    let (from, index, mut clip) = chosen(board, args)?;
    let to = match args.get("to_track") {
        Some(value) => video_track(board, value)?,
        None => from,
    };
    if let Some(locked) = args["locked"].as_bool() {
        clip.locked = locked;
    }
    let others = args
        .as_object()
        .unwrap()
        .keys()
        .any(|k| !matches!(k.as_str(), "track" | "clip" | "locked"));
    if clip.locked && others {
        return Err("That clip is locked; unlock it (locked false) to change it.".into());
    }
    let asset = board
        .timeline
        .videos
        .get(&clip.asset)
        .ok_or("That clip's video is missing.")?;
    if let Some(start) = position(board, layout, args)? {
        clip.start = start;
    }
    if let Some(offset) = args["offset_ms"].as_u64() {
        if offset >= asset.duration_ms {
            return Err(format!(
                "The video lasts {} ms; offset_ms must be inside it.",
                asset.duration_ms
            ));
        }
        clip.offset_ms = offset;
    }
    if let Some(frames) = length(board, args)? {
        clip.frames = frames;
    }
    let room = board.timeline.video_room(&clip, board.settings.frame_rate);
    if clip.frames > room {
        return Err(format!(
            "From that in point the video has {room} frames left; the clip cannot be longer."
        ));
    }
    if let Some(name) = args["name"].as_str() {
        clip.name = name.trim().into();
    }
    if let Some(opacity) = args["opacity"].as_f64() {
        clip.opacity = opacity as f32;
    }
    if let Some(visible) = args["visible"].as_bool() {
        clip.visible = visible;
    }
    let mut landed = (to, 0);
    editor.edit_storyboard(|b| {
        b.timeline.video[from].clips.remove(index);
        landed.1 = b.timeline.place_video(to, clip)?;
        Ok(())
    })?;
    Ok(clip_result(editor, landed.0, landed.1))
}

fn delete(editor: &mut ProjectEditor, board: &Storyboard, args: &Value) -> Result<Value, String> {
    let t = video_track(board, &args["track"])?;
    let clips = &board.timeline.video[t].clips;
    let chosen = indices(&args["clips"], clips.len(), "clip")?;
    if chosen.iter().any(|i| clips[*i].locked) {
        return Err("A chosen clip is locked; unlock it first.".into());
    }
    let drop_track = args["delete_track"] == true;
    if drop_track && clips.iter().any(|c| c.locked) {
        return Err("The track holds a locked clip; unlock it first.".into());
    }
    let mut removed = 0;
    editor.edit_storyboard(|b| {
        let mut n = 0;
        b.timeline.video[t].clips.retain(|_| {
            n += 1;
            !chosen.contains(&(n - 1))
        });
        if drop_track {
            b.timeline.video.remove(t);
        }
        removed = b.timeline.remove_unused_videos();
        Ok(())
    })?;
    Ok(
        json!({"track":t + 1,"deleted":chosen.len(),"videos_removed":removed,"track_deleted":drop_track}),
    )
}

#[cfg(test)]
mod tests {
    use super::super::tests::{board, call};
    use serde_json::json;

    #[test]
    fn video_clips_import_move_trim_and_delete() {
        if !emulsion_io::ffmpeg::available() {
            eprintln!("FFmpeg unavailable; skipping");
            return;
        }
        let dir = std::env::temp_dir().join(format!("emulsion-mcp-video-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ref.mp4");
        let status = emulsion_io::ffmpeg::command("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc=size=64x48:rate=24:duration=2")
            .args(["-f", "lavfi", "-i", "sine=duration=2"])
            .args(["-c:v", "mpeg4", "-c:a", "aac", "-shortest"])
            .arg(&file)
            .status()
            .unwrap();
        assert!(status.success());
        let mut e = board();
        let out = call(
            &mut e,
            "import_storyboard_video",
            json!({"path":file,"at":12,"with_audio":true}),
        );
        assert_eq!(
            (out["track"].clone(), out["clip"].clone()),
            (json!(1), json!(1))
        );
        assert!(out["frames"].as_u64().unwrap() >= 47);
        assert_eq!(out["sound_imported"], true);
        let timeline = &e.storyboard().unwrap().timeline;
        assert_eq!(timeline.tracks[0].clips[0].start, 12);
        let described = call(&mut e, "describe_storyboard", json!({}));
        assert_eq!(described["video"]["tracks"][0]["clips"][0]["start"], 12);
        let out = call(
            &mut e,
            "update_storyboard_video_clip",
            json!({"track":1,"clip":1,"at":0,"frames":24,"offset_ms":500,"opacity":0.5}),
        );
        assert_eq!(
            (out["start"].clone(), out["frames"].clone()),
            (json!(0), json!(24))
        );
        let clip = &e.storyboard().unwrap().timeline.video[0].clips[0];
        assert_eq!((clip.offset_ms, clip.opacity), (500, 0.5));
        // Longer than the rest of the video is refused.
        let long = super::super::execute(
            &mut e,
            "update_storyboard_video_clip",
            &json!({"track":1,"clip":1,"frames":400}),
        );
        assert!(long.is_error);
        call(
            &mut e,
            "update_storyboard_video_clip",
            json!({"track":1,"clip":1,"locked":true}),
        );
        let locked = super::super::execute(
            &mut e,
            "delete_storyboard_video_clips",
            &json!({"track":1,"clips":[1]}),
        );
        assert!(locked.is_error);
        call(
            &mut e,
            "update_storyboard_video_clip",
            json!({"track":1,"clip":1,"locked":false}),
        );
        let out = call(
            &mut e,
            "delete_storyboard_video_clips",
            json!({"track":1,"clips":[1],"delete_track":true}),
        );
        assert_eq!(out["videos_removed"], 1);
        assert!(e.storyboard().unwrap().timeline.video.is_empty());
        assert!(e.can_undo());
        std::fs::remove_dir_all(dir).ok();
    }
}
