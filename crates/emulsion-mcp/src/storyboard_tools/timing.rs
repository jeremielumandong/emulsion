//! Animatic timing tools: transitions into panels, panel durations (frames,
//! seconds or timecode), fitting a selection to a duration, roll edits and
//! snapping cuts to markers, audio tracks, clips and markers, and the sound
//! library. describe_storyboard reads all of it back through `panel_json`
//! and `animatic_json`.
use super::{def, layout, panel_id, panel_ids};
use crate::ToolDef;
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::storyboard::{MAX_PANEL_FRAMES, Storyboard, Transition, TransitionKind};
use emulsion_core::timeline::audio::{
    MAX_CLIPS, MAX_GAIN_DB, MAX_MARKERS, MAX_TRACKS, MIN_GAIN_DB,
};
use emulsion_core::timeline::{AudioClip, AudioTrack, Edge, Marker};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Most clips, markers or sounds describe_storyboard lists per list.
const PAGE: usize = 200;
/// Most items one call may change.
const MAX_ITEMS: usize = 200;
/// The longest stretch any length or position may name (24 hours at 120 fps).
const MAX_FRAMES: u64 = 24 * 3600 * 120;
const KINDS: [&str; 7] = [
    "cut",
    "dissolve",
    "wipe",
    "clock",
    "iris",
    "slide",
    "fade_to_color",
];
const EDGES: [&str; 4] = ["left", "right", "top", "bottom"];

pub(super) fn track_number() -> Value {
    json!({"type":"integer","minimum":1,"maximum":MAX_TRACKS,"description":"Audio track number from describe_storyboard (1 = first)."})
}

fn gain(what: &str) -> Value {
    json!({"type":"number","minimum":MIN_GAIN_DB,"maximum":MAX_GAIN_DB,"description":what})
}

/// A length: one of frames, seconds or a timecode.
pub(super) fn length_fields(what: &str) -> Value {
    json!({
        "frames":{"type":"integer","minimum":1,"maximum":MAX_FRAMES,"description":format!("{what} in frames. Use one of frames, seconds or timecode.")},
        "seconds":{"type":"number","minimum":0.001,"maximum":86400,"description":format!("{what} in seconds, rounded to whole frames.")},
        "timecode":{"type":"string","minLength":11,"maxLength":16,"description":format!("{what} as a timecode length, HH:MM:SS:FF.")}
    })
}

/// A point on the timeline: one of at, at_timecode, at_seconds or at_panel.
pub(super) fn at_fields() -> Value {
    json!({
        "at":{"type":"integer","minimum":0,"maximum":MAX_FRAMES,"description":"Timeline frame (0 = start). Use one of at, at_timecode, at_seconds or at_panel."},
        "at_timecode":{"type":"string","minLength":11,"maxLength":16,"description":"Timeline position as HH:MM:SS:FF (HH:MM:SS;FF drop-frame)."},
        "at_seconds":{"type":"number","minimum":0,"maximum":86400},
        "at_panel":panel_id()
    })
}

fn merged(mut a: Value, b: Value) -> Value {
    for (key, value) in b.as_object().unwrap() {
        a[key] = value.clone();
    }
    a
}

pub(super) fn scene_selection() -> Value {
    json!({
        "panels":panel_ids(),
        "scenes":{"type":"array","items":{"type":"integer","minimum":1},"minItems":1,"maxItems":MAX_ITEMS,"description":"Scene IDs from describe_storyboard; all their playing panels."},
        "scene_names":{"type":"array","items":{"type":"string","maxLength":800},"minItems":1,"maxItems":MAX_ITEMS,"description":"Scene names (case-insensitive)."}
    })
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let name = json!({"type":"string","minLength":1,"maxLength":200});
    let fade = json!({"type":"integer","minimum":0,"maximum":MAX_FRAMES,"description":"Frames."});
    let transition = merged(
        json!({
            "panels":panel_ids(),
            "kind":{"enum":KINDS,"description":"cut removes the transition."},
            "edge":{"enum":EDGES,"description":"wipe and slide: the side the new panel enters from (default left)."},
            "color":{"type":"string","minLength":7,"maxLength":7,"description":"fade_to_color: #RRGGBB (default #000000, black)."}
        }),
        length_fields("Transition length (default half a second)"),
    );
    let mut durations = length_fields("Duration");
    durations["frames"]["maximum"] = json!(MAX_PANEL_FRAMES);
    durations["panel"] = panel_id();
    let mut clip = merged(at_fields(), length_fields("Length on the timeline"));
    for (key, value) in [
        ("name", name.clone()),
        (
            "offset_ms",
            json!({"type":"integer","minimum":0,"maximum":MAX_FRAMES * 1000,"description":"Where in the sound the clip starts, in milliseconds."}),
        ),
        ("gain_db", gain("Clip gain in dB.")),
        ("fade_in", fade.clone()),
        ("fade_out", fade),
    ] {
        clip[key] = value;
    }
    let mut place = clip.clone();
    place["sound"] = json!({"type":"integer","minimum":1,"description":"Sound ID from describe_storyboard `sounds`."});
    place["track"] = track_number();
    let mut update_clip = clip;
    update_clip["track"] = track_number();
    update_clip["clip"] = clip_number();
    update_clip["to_track"] = track_number();
    let mut marker = at_fields();
    marker["name"] = name.clone();
    let mut update_marker = marker.clone();
    update_marker["track"] = track_number();
    update_marker["marker"] = json!({"type":"integer","minimum":1,"maximum":MAX_MARKERS});
    let tolerance = json!({
        "frames":{"type":"integer","minimum":1,"maximum":MAX_PANEL_FRAMES,"description":"Snap cuts within this many frames of a marker (default a quarter second)."},
        "seconds":{"type":"number","minimum":0.001,"maximum":600}
    });
    vec![
        def(
            "set_storyboard_transitions",
            "Set how the animatic enters each chosen panel from the panel before: cut, dissolve, wipe (edge), clock wipe, iris, slide (edge) or fade_to_color (color), lasting frames, seconds or a timecode (default half a second). The transition plays over the first frames of the panel, so timing does not change; it cannot be longer than the panel. The first panel has nothing to come from. Locked panels refuse changes. One Undo step.",
            transition,
            &["panels", "kind"],
        ),
        def(
            "set_storyboard_timing",
            "Set the durations of many panels at once, each in frames, seconds or a timecode length (HH:MM:SS:FF). Transitions longer than a new duration are shortened to fit and listed. Locked panels refuse changes. One Undo step.",
            json!({"panels":{"type":"array","minItems":1,"maxItems":MAX_ITEMS,"items":{"type":"object","properties":durations,"required":["panel"],"additionalProperties":false}}}),
            &["panels"],
        ),
        def(
            "fit_storyboard_timing",
            "Retime: scale the playing panels of a selection (panels, scenes by ID or by name) so together they last a total duration, keeping their proportions, such as fitting a scene to its dialogue. Thumbnail sheets are left out. Locked panels refuse changes. Returns each panel's new duration. One Undo step.",
            merged(scene_selection(), length_fields("Total duration")),
            &[],
        ),
        def(
            "roll_storyboard_cut",
            "Roll edit: move the cut after a playing panel later (positive) or earlier (negative) by frames or seconds; the next playing panel loses or gains the same time, so the total stays. Neither panel goes below one frame; returns the frames actually moved. Locked panels refuse changes. One Undo step.",
            json!({
                "panel":panel_id(),
                "frames":{"type":"number","minimum":-(MAX_PANEL_FRAMES as f64),"maximum":MAX_PANEL_FRAMES,"description":"Whole frames; negative moves the cut earlier."},
                "seconds":{"type":"number","minimum":-600,"maximum":600}
            }),
            &["panel"],
        ),
        def(
            "snap_storyboard_cuts",
            "Move every panel cut that lies near an audio marker (on any track) onto it, within a tolerance. Panels keep at least one frame. Locked panels refuse changes: unlock them or move the markers. Returns the cuts moved. One Undo step.",
            tolerance,
            &[],
        ),
        def(
            "add_storyboard_audio_track",
            "Add an audio track (at most 16) below the others, such as Dialogue, Music or SFX. Returns its track number. One Undo step.",
            json!({"name":name,"volume_db":gain("Track volume in dB (default 0).")}),
            &["name"],
        ),
        def(
            "update_storyboard_audio_track",
            "Rename an audio track, set its volume in dB, mute it, or solo it (while any track is soloed only soloed tracks play). One Undo step.",
            json!({"track":track_number(),"name":name,"volume_db":gain("Track volume in dB."),"muted":{"type":"boolean"},"solo":{"type":"boolean"}}),
            &["track"],
        ),
        def(
            "delete_storyboard_audio_track",
            "Delete an audio track with its clips and markers; the sounds stay in the library. Later tracks move up one number. One Undo step.",
            json!({"track":track_number()}),
            &["track"],
        ),
        def(
            "place_storyboard_sound",
            "Place a library sound on an audio track as a clip, starting at a frame, timecode, second or a panel's first frame (default 0). offset_ms skips into the sound; the length defaults to the rest of the sound. Optional name (default the sound's), gain_db and fade_in/fade_out frames. Clips on one track cannot overlap. Returns the clip's track and number. One Undo step.",
            place,
            &["sound", "track"],
        ),
        def(
            "update_storyboard_audio_clip",
            "Move, trim or change a clip: a new start (at, at_timecode, at_seconds, at_panel), another track (to_track), a new length (frames, seconds, timecode), offset_ms into the sound, name, gain_db or fades. To trim the head, raise offset_ms and the start together; effect keys stay where they are in the sound. Clips on one track cannot overlap. Returns the clip's track and number. One Undo step.",
            update_clip,
            &["track", "clip"],
        ),
        def(
            "delete_storyboard_audio_clips",
            "Delete clips from an audio track by number; the sounds stay in the library. One Undo step.",
            json!({"track":track_number(),"clips":{"type":"array","items":clip_number(),"minItems":1,"maxItems":MAX_ITEMS}}),
            &["track", "clips"],
        ),
        def(
            "add_storyboard_markers",
            "Add named markers to an audio track, each at a frame, timecode, second or a panel's first frame, such as each dialogue line or sound hit. snap_storyboard_cuts moves panel cuts onto them. Markers stay in time order. One Undo step.",
            json!({"track":track_number(),"markers":{"type":"array","minItems":1,"maxItems":MAX_ITEMS,"items":{"type":"object","properties":marker,"required":["name"],"additionalProperties":false}}}),
            &["track", "markers"],
        ),
        def(
            "update_storyboard_marker",
            "Rename a marker or move it to another frame, timecode, second or panel start. Returns its new number. One Undo step.",
            update_marker,
            &["track", "marker"],
        ),
        def(
            "delete_storyboard_markers",
            "Delete markers from an audio track by number. One Undo step.",
            json!({"track":track_number(),"markers":{"type":"array","items":{"type":"integer","minimum":1,"maximum":MAX_MARKERS},"minItems":1,"maxItems":MAX_ITEMS}}),
            &["track", "markers"],
        ),
        def(
            "update_storyboard_sounds",
            "Rename sounds in the library and set their folders (`/`-separated, empty for the top level). Clips keep their own names. One Undo step.",
            json!({"sounds":{"type":"array","minItems":1,"maxItems":MAX_ITEMS,"items":{"type":"object","additionalProperties":false,"required":["sound"],"properties":{
                "sound":{"type":"integer","minimum":1},
                "name":name,
                "folder":{"type":"string","maxLength":400}
            }}}}),
            &["sounds"],
        ),
        def(
            "remove_storyboard_sounds",
            "Delete sounds from the library: the chosen ones, which no clip may use, or without `sounds` every sound no clip uses. Returns the removed IDs. One Undo step.",
            json!({"sounds":{"type":"array","items":{"type":"integer","minimum":1},"minItems":1,"maxItems":MAX_ITEMS}}),
            &[],
        ),
    ]
}

pub(super) fn clip_number() -> Value {
    json!({"type":"integer","minimum":1,"maximum":MAX_CLIPS,"description":"Clip number on its track from describe_storyboard (1 = earliest)."})
}

fn hex(color: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

fn parse_hex(text: &str) -> Result<[u8; 3], String> {
    let digits = text
        .strip_prefix('#')
        .filter(|d| d.len() == 6 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| format!("Colours are #RRGGBB, not '{text}'."))?;
    let channel = |i: usize| u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).unwrap();
    Ok([channel(0), channel(1), channel(2)])
}

fn edge_name(edge: Edge) -> &'static str {
    match edge {
        Edge::Left => "left",
        Edge::Right => "right",
        Edge::Top => "top",
        Edge::Bottom => "bottom",
    }
}

pub(super) fn seconds(board: &Storyboard, frames: u64) -> f64 {
    board.settings.frame_rate.frames_to_seconds(frames)
}

pub(super) fn timecode(board: &Storyboard, frame: u64) -> String {
    board.settings.frame_rate.timecode(frame)
}

pub(super) fn transition_json(board: &Storyboard, t: Transition) -> Value {
    let kind = match t.kind {
        TransitionKind::Cut => "cut",
        TransitionKind::Dissolve => "dissolve",
        TransitionKind::Wipe { .. } => "wipe",
        TransitionKind::Clock => "clock",
        TransitionKind::Iris => "iris",
        TransitionKind::Slide { .. } => "slide",
        TransitionKind::FadeToColor { .. } => "fade_to_color",
    };
    let mut out =
        json!({"kind":kind,"frames":t.frames,"seconds":seconds(board, u64::from(t.frames))});
    match t.kind {
        TransitionKind::Wipe { from } | TransitionKind::Slide { from } => {
            out["edge"] = json!(edge_name(from))
        }
        TransitionKind::FadeToColor { color } => out["color"] = json!(hex(color)),
        _ => {}
    }
    out
}

/// Add a panel's place in the animatic to its describe_storyboard entry:
/// `start` frame and `timecode` for playing panels, and its `transition`
/// unless it is a cut.
pub(super) fn panel_json(
    board: &Storyboard,
    starts: &HashMap<PageId, u64>,
    id: PageId,
    entry: &mut Value,
) {
    if let Some(start) = starts.get(&id) {
        entry["start"] = json!(start);
        entry["timecode"] = json!(timecode(board, *start));
    }
    let transition = board.panels[&id].transition;
    if !transition.is_cut() {
        entry["transition"] = transition_json(board, transition);
    }
}

/// One page of a list, from `from`.
pub(super) fn page<T>(items: &[T], from: usize) -> &[T] {
    &items[from.min(items.len())..(from + PAGE).min(items.len())]
}

/// The animatic for describe_storyboard: running time, audio tracks with
/// their clips and markers, and the sound library. Lists longer than one
/// page show `PAGE` items from `audio_from`.
pub(super) fn animatic_json(board: &Storyboard, layout: &[PageId], args: &Value) -> Value {
    let from = args["audio_from"].as_u64().unwrap_or(0) as usize;
    let timeline = &board.timeline;
    let total = board.animatic_frames(layout);
    let mut more = false;
    let tracks: Vec<Value> = timeline
        .tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            more |= track.clips.len() > from + PAGE || track.markers.len() > from + PAGE;
            let clips: Vec<Value> = page(&track.clips, from)
                .iter()
                .enumerate()
                .map(|(n, clip)| {
                    json!({
                        "clip":from + n + 1,
                        "name":clip.name,
                        "sound":clip.asset,
                        "start":clip.start,
                        "start_timecode":timecode(board, clip.start),
                        "frames":clip.frames,
                        "seconds":seconds(board, clip.frames),
                        "offset_ms":clip.offset_ms,
                        "gain_db":clip.gain_db,
                        "fade_in":clip.fade_in,
                        "fade_out":clip.fade_out,
                        "effects":clip.has_effects(),
                    })
                })
                .collect();
            let markers: Vec<Value> = page(&track.markers, from)
                .iter()
                .enumerate()
                .map(|(n, m)| json!({"marker":from + n + 1,"name":m.name,"frame":m.frame,"timecode":timecode(board, m.frame)}))
                .collect();
            json!({
                "track":i + 1,
                "name":track.name,
                "volume_db":track.volume_db,
                "muted":track.muted,
                "solo":track.solo,
                "audible":timeline.audible(i),
                "clip_count":track.clips.len(),
                "clips":clips,
                "marker_count":track.markers.len(),
                "markers":markers,
            })
        })
        .collect();
    let used = uses(board);
    let sounds: Vec<_> = timeline.assets.iter().collect();
    more |= sounds.len() > from + PAGE;
    let sounds: Vec<Value> = page(&sounds, from)
        .iter()
        .map(|(id, asset)| {
            json!({
                "sound":id,
                "name":asset.name,
                "folder":asset.folder,
                "format":asset.format,
                "duration_ms":asset.duration_ms,
                "frames":board.settings.frame_rate.seconds_to_frames(asset.duration_ms as f64 / 1000.),
                "clips":used.get(id).copied().unwrap_or(0),
            })
        })
        .collect();
    json!({
        "frames":total,
        "timecode":timecode(board, total),
        "drop_frame":board.settings.frame_rate.drop_frame(),
        "audio_end":timeline.end(),
        "tracks":tracks,
        "sound_count":timeline.assets.len(),
        "sounds":sounds,
        "audio_from":from,
        "more":more,
    })
}

/// How many clips use each sound.
fn uses(board: &Storyboard) -> HashMap<u64, usize> {
    let mut used = HashMap::new();
    for clip in board.timeline.tracks.iter().flat_map(|t| &t.clips) {
        *used.entry(clip.asset).or_insert(0) += 1;
    }
    used
}

/// At most one of `keys` in `args`.
fn one_of<'a>(args: &'a Value, keys: &[&'a str]) -> Result<Option<&'a str>, String> {
    let given: Vec<_> = keys
        .iter()
        .copied()
        .filter(|k| args.get(*k).is_some())
        .collect();
    match given[..] {
        [] => Ok(None),
        [key] => Ok(Some(key)),
        _ => Err(format!("Use only one of {}.", given.join(", "))),
    }
}

fn parse_timecode(board: &Storyboard, value: &Value) -> Result<u64, String> {
    let text = value.as_str().unwrap_or_default();
    board
        .settings
        .frame_rate
        .parse_timecode(text)
        .ok_or_else(|| format!("'{text}' is not a timecode at this frame rate (HH:MM:SS:FF)."))
}

/// A length from frames, seconds or timecode, if given.
pub(super) fn length(board: &Storyboard, args: &Value) -> Result<Option<u64>, String> {
    let frames = match one_of(args, &["frames", "seconds", "timecode"])? {
        None => return Ok(None),
        Some("frames") => args["frames"].as_u64().unwrap(),
        Some("seconds") => board
            .settings
            .frame_rate
            .seconds_to_frames(args["seconds"].as_f64().unwrap()),
        Some(_) => parse_timecode(board, &args["timecode"])?,
    };
    if frames == 0 {
        return Err("That is shorter than one frame.".into());
    }
    Ok(Some(frames))
}

/// A timeline position from at, at_timecode, at_seconds or at_panel.
pub(super) fn position(
    board: &Storyboard,
    layout: &[PageId],
    args: &Value,
) -> Result<Option<u64>, String> {
    Ok(Some(
        match one_of(args, &["at", "at_timecode", "at_seconds", "at_panel"])? {
            None => return Ok(None),
            Some("at") => args["at"].as_u64().unwrap(),
            Some("at_timecode") => parse_timecode(board, &args["at_timecode"])?,
            Some("at_seconds") => board
                .settings
                .frame_rate
                .seconds_to_frames(args["at_seconds"].as_f64().unwrap()),
            Some(_) => {
                let id = args["at_panel"].as_u64().unwrap();
                board
                    .panel_starts(layout)
                    .into_iter()
                    .find(|(p, _)| *p == id)
                    .map(|(_, start)| start)
                    .ok_or_else(|| format!("Panel {id} does not play in the animatic."))?
            }
        },
    ))
}

/// A track index from its 1-based number.
pub(super) fn track(board: &Storyboard, value: &Value) -> Result<usize, String> {
    let n = value.as_u64().unwrap() as usize;
    if n > board.timeline.tracks.len() {
        return Err(format!(
            "There is no audio track {n}; the board has {}. Add one with add_storyboard_audio_track.",
            board.timeline.tracks.len()
        ));
    }
    Ok(n - 1)
}

/// Distinct 0-based indices from 1-based `numbers`, all below `len`.
pub(super) fn indices(value: &Value, len: usize, what: &str) -> Result<BTreeSet<usize>, String> {
    let mut out = BTreeSet::new();
    for n in super::ids(value) {
        if n as usize > len {
            return Err(format!("There is no {what} {n} on that track."));
        }
        if !out.insert(n as usize - 1) {
            return Err(format!("{what} {n} is listed twice."));
        }
    }
    Ok(out)
}

/// A playing panel; thumbnail sheets do not play.
fn playing(board: &Storyboard, id: PageId) -> Result<(), String> {
    let panel = board
        .panels
        .get(&id)
        .ok_or_else(|| format!("Panel {id} does not exist."))?;
    if panel.thumbnails.is_some() {
        return Err(format!(
            "Panel {id} is a thumbnail sheet and does not play in the animatic."
        ));
    }
    Ok(())
}

/// Shorten transitions that no longer fit their panels. Returns the panels
/// changed.
pub(super) fn fit_transitions(board: &mut Storyboard) -> Vec<PageId> {
    let mut changed = Vec::new();
    for (id, panel) in &mut board.panels {
        if panel.transition.frames > panel.frames {
            panel.transition.frames = panel.frames;
            changed.push(*id);
        }
    }
    changed
}

/// Run a timing tool; `None` when `name` is not one.
pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let layout = layout(editor);
    let result = match name {
        "set_storyboard_transitions" => transitions(editor, board, args),
        "set_storyboard_timing" => set_timing(editor, board, args),
        "fit_storyboard_timing" => fit(editor, board, &layout, args),
        "roll_storyboard_cut" => roll(editor, board, &layout, args),
        "snap_storyboard_cuts" => snap(editor, board, &layout, args),
        "add_storyboard_audio_track" => {
            let count = board.timeline.tracks.len();
            if count >= MAX_TRACKS {
                return Some(Err(format!("Use at most {MAX_TRACKS} audio tracks.")));
            }
            editor
                .edit_storyboard(|b| {
                    let mut track = AudioTrack::new(args["name"].as_str().unwrap().trim());
                    track.volume_db = args["volume_db"].as_f64().unwrap_or(0.) as f32;
                    b.timeline.tracks.push(track);
                    Ok(())
                })
                .map(|()| json!({"track":count + 1}))
        }
        "update_storyboard_audio_track" => track(board, &args["track"]).and_then(|i| {
            editor
                .edit_storyboard(|b| {
                    let t = &mut b.timeline.tracks[i];
                    if let Some(name) = args["name"].as_str() {
                        t.name = name.trim().into();
                    }
                    if let Some(db) = args["volume_db"].as_f64() {
                        t.volume_db = db as f32;
                    }
                    if let Some(muted) = args["muted"].as_bool() {
                        t.muted = muted;
                    }
                    if let Some(solo) = args["solo"].as_bool() {
                        t.solo = solo;
                    }
                    Ok(())
                })
                .map(|()| {
                    let t = &editor.storyboard().unwrap().timeline.tracks[i];
                    json!({"track":i + 1,"name":t.name,"volume_db":t.volume_db,"muted":t.muted,"solo":t.solo})
                })
        }),
        "delete_storyboard_audio_track" => track(board, &args["track"]).and_then(|i| {
            editor
                .edit_storyboard(|b| {
                    b.timeline.tracks.remove(i);
                    Ok(())
                })
                .map(|()| json!({"deleted":i + 1,"tracks":board.timeline.tracks.len() - 1}))
        }),
        "place_storyboard_sound" => place(editor, board, &layout, args),
        "update_storyboard_audio_clip" => update_clip(editor, board, &layout, args),
        "delete_storyboard_audio_clips" => track(board, &args["track"]).and_then(|i| {
            let chosen = indices(&args["clips"], board.timeline.tracks[i].clips.len(), "clip")?;
            editor
                .edit_storyboard(|b| {
                    let mut n = 0;
                    b.timeline.tracks[i].clips.retain(|_| {
                        n += 1;
                        !chosen.contains(&(n - 1))
                    });
                    Ok(())
                })
                .map(|()| json!({"track":i + 1,"deleted":chosen.len()}))
        }),
        "add_storyboard_markers" => add_markers(editor, board, &layout, args),
        "update_storyboard_marker" => update_marker(editor, board, &layout, args),
        "delete_storyboard_markers" => track(board, &args["track"]).and_then(|i| {
            let chosen = indices(
                &args["markers"],
                board.timeline.tracks[i].markers.len(),
                "marker",
            )?;
            editor
                .edit_storyboard(|b| {
                    let mut n = 0;
                    b.timeline.tracks[i].markers.retain(|_| {
                        n += 1;
                        !chosen.contains(&(n - 1))
                    });
                    Ok(())
                })
                .map(|()| json!({"track":i + 1,"deleted":chosen.len()}))
        }),
        "update_storyboard_sounds" => update_sounds(editor, board, args),
        "remove_storyboard_sounds" => remove_sounds(editor, board, args),
        _ => return None,
    };
    Some(result)
}

fn transitions(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let ids: Vec<PageId> = super::ids(&args["panels"]);
    let kind_name = args["kind"].as_str().unwrap();
    let edge = match args["edge"].as_str() {
        Some(_) if !matches!(kind_name, "wipe" | "slide") => {
            return Err("edge is only for wipe and slide.".into());
        }
        Some("right") => Edge::Right,
        Some("top") => Edge::Top,
        Some("bottom") => Edge::Bottom,
        _ => Edge::Left,
    };
    let color = match args["color"].as_str() {
        Some(_) if kind_name != "fade_to_color" => {
            return Err("color is only for fade_to_color.".into());
        }
        Some(text) => parse_hex(text)?,
        None => [0, 0, 0],
    };
    let kind = match kind_name {
        "cut" => TransitionKind::Cut,
        "dissolve" => TransitionKind::Dissolve,
        "wipe" => TransitionKind::Wipe { from: edge },
        "clock" => TransitionKind::Clock,
        "iris" => TransitionKind::Iris,
        "slide" => TransitionKind::Slide { from: edge },
        _ => TransitionKind::FadeToColor { color },
    };
    let frames = match (kind, length(board, args)?) {
        (TransitionKind::Cut, Some(_)) => return Err("A cut has no length.".into()),
        (TransitionKind::Cut, None) => 0,
        (_, Some(frames)) => frames,
        (_, None) => board.settings.frame_rate.seconds_to_frames(0.5).max(1),
    };
    let mut seen = HashSet::new();
    for id in &ids {
        playing(board, *id)?;
        if !seen.insert(*id) {
            return Err(format!("Panel {id} is listed twice."));
        }
        let panel_frames = board.panels[id].frames;
        if frames > u64::from(panel_frames) {
            return Err(format!(
                "Panel {id} lasts only {panel_frames} frames; a transition cannot be longer than its panel."
            ));
        }
    }
    let transition = Transition {
        kind,
        frames: frames as u32,
    };
    editor.edit_storyboard(|b| {
        for id in &ids {
            b.panels.get_mut(id).unwrap().transition = transition;
        }
        Ok(())
    })?;
    Ok(json!({"panels":ids,"transition":transition_json(board, transition)}))
}

fn set_timing(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let mut ids = Vec::new();
    let mut frames = Vec::new();
    for item in args["panels"].as_array().unwrap() {
        let id = item["panel"].as_u64().unwrap();
        playing(board, id)?;
        if ids.contains(&id) {
            return Err(format!("Panel {id} is listed twice."));
        }
        let f = length(board, item)?
            .ok_or_else(|| format!("Give panel {id} a duration in frames, seconds or timecode."))?;
        if f > u64::from(MAX_PANEL_FRAMES) {
            return Err(format!(
                "Panel durations are at most {MAX_PANEL_FRAMES} frames."
            ));
        }
        ids.push(id);
        frames.push(f as u32);
    }
    let mut shortened = Vec::new();
    editor.edit_storyboard(|b| {
        b.set_frames(&ids, &frames)?;
        shortened = fit_transitions(b);
        Ok(())
    })?;
    Ok(timing_result(editor, &ids, shortened))
}

fn timing_result(editor: &ProjectEditor, ids: &[PageId], shortened: Vec<PageId>) -> Value {
    let board = editor.storyboard().unwrap();
    let panels: Vec<_> = ids
        .iter()
        .map(|id| {
            let f = board.panels[id].frames;
            json!({"panel":id,"frames":f,"seconds":seconds(board, u64::from(f))})
        })
        .collect();
    let total = board.animatic_frames(&layout(editor));
    json!({
        "panels":panels,
        "transitions_shortened":shortened,
        "total_frames":total,
        "total_timecode":timecode(board, total),
    })
}

/// Playing panels chosen by `panels`, `scenes` and `scene_names`, in page
/// order.
pub(super) fn selection(
    board: &Storyboard,
    layout: &[PageId],
    args: &Value,
) -> Result<Vec<PageId>, String> {
    let panels: HashSet<_> = super::ids(&args["panels"]).into_iter().collect();
    let mut scenes: HashSet<_> = super::ids(&args["scenes"]).into_iter().collect();
    for id in &panels {
        playing(board, *id)?;
    }
    if let Some(id) = scenes.iter().find(|id| !board.scenes.contains_key(id)) {
        return Err(format!("No scene has ID {id}."));
    }
    for name in args["scene_names"].as_array().into_iter().flatten() {
        let name = name.as_str().unwrap().trim();
        let found: Vec<_> = board
            .scenes
            .iter()
            .filter(|(_, s)| s.name.trim().eq_ignore_ascii_case(name))
            .map(|(id, _)| *id)
            .collect();
        if found.is_empty() {
            return Err(format!("No scene is named '{name}'."));
        }
        scenes.extend(found);
    }
    let chosen: Vec<_> = board
        .playing(layout)
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| panels.contains(id) || scenes.contains(&board.panels[id].scene))
        .collect();
    if chosen.is_empty() {
        return Err("Choose panels or scenes that play in the animatic.".into());
    }
    Ok(chosen)
}

fn fit(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    layout: &[PageId],
    args: &Value,
) -> Result<Value, String> {
    let ids = selection(board, layout, args)?;
    let total =
        length(board, args)?.ok_or("Give the total duration in frames, seconds or timecode.")?;
    let mut shortened = Vec::new();
    editor.edit_storyboard(|b| {
        b.retime(&ids, total)?;
        shortened = fit_transitions(b);
        Ok(())
    })?;
    let mut out = timing_result(editor, &ids, shortened);
    out["selection_frames"] = json!(total);
    Ok(out)
}

fn roll(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    layout: &[PageId],
    args: &Value,
) -> Result<Value, String> {
    let id = args["panel"].as_u64().unwrap();
    playing(board, id)?;
    let delta = match one_of(args, &["frames", "seconds"])? {
        Some("frames") => {
            let f = args["frames"].as_f64().unwrap();
            if f.fract() != 0. {
                return Err("frames must be a whole number.".into());
            }
            f as i64
        }
        Some(_) => {
            let s = args["seconds"].as_f64().unwrap();
            let f = board.settings.frame_rate.seconds_to_frames(s.abs()) as i64;
            if s < 0. { -f } else { f }
        }
        None => return Err("Give the frames or seconds to move the cut by.".into()),
    };
    if delta == 0 {
        return Err("Move the cut by at least one frame.".into());
    }
    let order = board.playing(layout);
    let index = order.iter().position(|(p, _)| *p == id).unwrap();
    let Some((next, _)) = order.get(index + 1).copied() else {
        return Err(format!(
            "Panel {id} is the last panel; there is no cut after it."
        ));
    };
    let mut applied = 0;
    let mut shortened = Vec::new();
    editor.edit_storyboard(|b| {
        applied = b.roll(layout, id, delta)?;
        shortened = fit_transitions(b);
        Ok(())
    })?;
    let mut out = timing_result(editor, &[id, next], shortened);
    out["moved"] = json!(applied);
    Ok(out)
}

fn snap(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    layout: &[PageId],
    args: &Value,
) -> Result<Value, String> {
    if board.timeline.marker_frames().is_empty() {
        return Err(
            "There are no markers to snap to; add them with add_storyboard_markers.".into(),
        );
    }
    let rate = board.settings.frame_rate;
    let tolerance = match one_of(args, &["frames", "seconds"])? {
        Some("frames") => args["frames"].as_u64().unwrap(),
        Some(_) => rate
            .seconds_to_frames(args["seconds"].as_f64().unwrap())
            .max(1),
        None => rate.seconds_to_frames(0.25).max(1),
    };
    let before = board.panel_starts(layout);
    let mut shortened = Vec::new();
    editor.edit_storyboard(|b| {
        b.snap_to_markers(layout, tolerance)?;
        shortened = fit_transitions(b);
        Ok(())
    })?;
    let after = editor.storyboard().unwrap().panel_starts(layout);
    let mut moved: Vec<Value> = before
        .iter()
        .zip(&after)
        .skip(1)
        .filter(|(a, b)| a.1 != b.1)
        .map(|(a, b)| json!({"panel":a.0,"from":a.1,"to":b.1}))
        .collect();
    // The end of the last panel can snap too.
    let (old_end, new_end) = (board.animatic_frames(layout), {
        let b = editor.storyboard().unwrap();
        b.animatic_frames(layout)
    });
    if old_end != new_end {
        moved.push(json!({"end":true,"from":old_end,"to":new_end}));
    }
    Ok(
        json!({"tolerance_frames":tolerance,"cuts_moved":moved,"transitions_shortened":shortened,"total_frames":new_end}),
    )
}

/// Apply clip fields (except the track) onto `clip`.
fn apply_clip(
    board: &Storyboard,
    layout: &[PageId],
    clip: &mut AudioClip,
    args: &Value,
    new: bool,
) -> Result<(), String> {
    let asset = board
        .timeline
        .assets
        .get(&clip.asset)
        .ok_or("That sound is not in the library.")?;
    if let Some(start) = position(board, layout, args)? {
        clip.start = start;
    }
    if let Some(offset) = args["offset_ms"].as_u64() {
        if offset >= asset.duration_ms.max(1) {
            return Err(format!(
                "The sound lasts {} ms; offset_ms must be inside it.",
                asset.duration_ms
            ));
        }
        if !new && offset != clip.offset_ms {
            // Effect keys stay where they are in the sound, as on the Timeline.
            let frames = |ms: u64| {
                board
                    .settings
                    .frame_rate
                    .seconds_to_frames(ms as f64 / 1000.)
            };
            clip.shift_effect_keys(frames(offset) as i64 - frames(clip.offset_ms) as i64);
        }
        clip.offset_ms = offset;
    }
    match length(board, args)? {
        Some(frames) => clip.frames = frames,
        None if new => {
            let left = asset.duration_ms.saturating_sub(clip.offset_ms);
            clip.frames = board
                .settings
                .frame_rate
                .seconds_to_frames(left as f64 / 1000.)
                .max(1);
        }
        None => {}
    }
    if let Some(name) = args["name"].as_str() {
        clip.name = name.trim().into();
    }
    if let Some(db) = args["gain_db"].as_f64() {
        clip.gain_db = db as f32;
    }
    if let Some(f) = args["fade_in"].as_u64() {
        clip.fade_in = f;
    }
    if let Some(f) = args["fade_out"].as_u64() {
        clip.fade_out = f;
    }
    if clip.fade_in + clip.fade_out > clip.frames {
        return Err(format!(
            "Fades ({} + {} frames) must fit inside the clip's {} frames.",
            clip.fade_in, clip.fade_out, clip.frames
        ));
    }
    Ok(())
}

/// Where a clip landed: its track and 1-based number.
fn clip_result(editor: &ProjectEditor, track: usize, start: u64) -> Value {
    let board = editor.storyboard().unwrap();
    let clips = &board.timeline.tracks[track].clips;
    let n = clips.iter().position(|c| c.start == start).unwrap();
    let clip = &clips[n];
    json!({
        "track":track + 1,
        "clip":n + 1,
        "name":clip.name,
        "start":clip.start,
        "start_timecode":timecode(board, clip.start),
        "frames":clip.frames,
        "end":clip.end(),
    })
}

fn place(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    layout: &[PageId],
    args: &Value,
) -> Result<Value, String> {
    let t = track(board, &args["track"])?;
    let id = args["sound"].as_u64().unwrap();
    let asset =
        board.timeline.assets.get(&id).ok_or_else(|| {
            format!("No sound has ID {id}; describe_storyboard lists the library.")
        })?;
    let mut clip = AudioClip {
        asset: id,
        name: asset.name.clone(),
        start: 0,
        frames: 1,
        ..AudioClip::default()
    };
    apply_clip(board, layout, &mut clip, args, true)?;
    let start = clip.start;
    editor.edit_storyboard(|b| b.timeline.place(t, clip))?;
    Ok(clip_result(editor, t, start))
}

fn update_clip(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    layout: &[PageId],
    args: &Value,
) -> Result<Value, String> {
    let from = track(board, &args["track"])?;
    let to = match args.get("to_track") {
        Some(value) => track(board, value)?,
        None => from,
    };
    let n = args["clip"].as_u64().unwrap() as usize;
    let mut clip = board.timeline.tracks[from]
        .clips
        .get(n - 1)
        .cloned()
        .ok_or_else(|| format!("There is no clip {n} on track {}.", from + 1))?;
    apply_clip(board, layout, &mut clip, args, false)?;
    let start = clip.start;
    editor.edit_storyboard(|b| {
        b.timeline.tracks[from].clips.remove(n - 1);
        b.timeline.place(to, clip)
    })?;
    Ok(clip_result(editor, to, start))
}

/// Insert a marker keeping the track in time order; returns its index.
fn insert_marker(track: &mut AudioTrack, marker: Marker) -> usize {
    let at = track.markers.partition_point(|m| m.frame <= marker.frame);
    track.markers.insert(at, marker);
    at
}

fn add_markers(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    layout: &[PageId],
    args: &Value,
) -> Result<Value, String> {
    let t = track(board, &args["track"])?;
    let mut markers = Vec::new();
    for item in args["markers"].as_array().unwrap() {
        let frame = position(board, layout, item)?
            .ok_or("Give each marker a position: at, at_timecode, at_seconds or at_panel.")?;
        markers.push(Marker {
            frame,
            name: item["name"].as_str().unwrap().trim().into(),
        });
    }
    editor.edit_storyboard(|b| {
        for marker in markers {
            insert_marker(&mut b.timeline.tracks[t], marker);
        }
        Ok(())
    })?;
    Ok(markers_result(editor, t))
}

fn markers_result(editor: &ProjectEditor, t: usize) -> Value {
    let board = editor.storyboard().unwrap();
    let markers: Vec<_> = board.timeline.tracks[t]
        .markers
        .iter()
        .take(PAGE)
        .enumerate()
        .map(|(n, m)| json!({"marker":n + 1,"name":m.name,"frame":m.frame,"timecode":timecode(board, m.frame)}))
        .collect();
    json!({"track":t + 1,"marker_count":board.timeline.tracks[t].markers.len(),"markers":markers})
}

fn update_marker(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    layout: &[PageId],
    args: &Value,
) -> Result<Value, String> {
    let t = track(board, &args["track"])?;
    let n = args["marker"].as_u64().unwrap() as usize;
    let mut marker = board.timeline.tracks[t]
        .markers
        .get(n - 1)
        .cloned()
        .ok_or_else(|| format!("There is no marker {n} on track {}.", t + 1))?;
    if let Some(frame) = position(board, layout, args)? {
        marker.frame = frame;
    }
    if let Some(name) = args["name"].as_str() {
        marker.name = name.trim().into();
    }
    let mut at = 0;
    editor.edit_storyboard(|b| {
        let track = &mut b.timeline.tracks[t];
        track.markers.remove(n - 1);
        at = insert_marker(track, marker);
        Ok(())
    })?;
    let board = editor.storyboard().unwrap();
    let m = &board.timeline.tracks[t].markers[at];
    Ok(
        json!({"track":t + 1,"marker":at + 1,"name":m.name,"frame":m.frame,"timecode":timecode(board, m.frame)}),
    )
}

fn update_sounds(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let items = args["sounds"].as_array().unwrap();
    let mut seen = HashSet::new();
    for item in items {
        let id = item["sound"].as_u64().unwrap();
        if !board.timeline.assets.contains_key(&id) {
            return Err(format!("No sound has ID {id}."));
        }
        if !seen.insert(id) {
            return Err(format!("Sound {id} is listed twice."));
        }
    }
    editor.edit_storyboard(|b| {
        for item in items {
            let asset = b
                .timeline
                .assets
                .get_mut(&item["sound"].as_u64().unwrap())
                .unwrap();
            if let Some(name) = item["name"].as_str() {
                asset.name = name.trim().into();
            }
            if let Some(folder) = item["folder"].as_str() {
                asset.folder = folder
                    .split('/')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join("/");
            }
        }
        Ok(())
    })?;
    let board = editor.storyboard().unwrap();
    let sounds: Vec<_> = seen
        .iter()
        .map(|id| {
            let a = &board.timeline.assets[id];
            json!({"sound":id,"name":a.name,"folder":a.folder})
        })
        .collect();
    Ok(json!({"sounds":sounds}))
}

fn remove_sounds(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let used = uses(board);
    let chosen: Vec<u64> = match args.get("sounds") {
        Some(value) => {
            let ids = super::ids(value);
            for id in &ids {
                if !board.timeline.assets.contains_key(id) {
                    return Err(format!("No sound has ID {id}."));
                }
                if let Some(n) = used.get(id) {
                    return Err(format!(
                        "Sound {id} is used by {n} clip(s); delete those clips first."
                    ));
                }
            }
            ids
        }
        None => board
            .timeline
            .assets
            .keys()
            .copied()
            .filter(|id| !used.contains_key(id))
            .collect(),
    };
    editor.edit_storyboard(|b| {
        for id in &chosen {
            b.timeline.assets.remove(id);
        }
        Ok(())
    })?;
    Ok(json!({"removed":chosen,"sound_count":editor.storyboard().unwrap().timeline.assets.len()}))
}
