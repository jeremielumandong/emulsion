//! OpenTimelineIO (.otio JSON): a Timeline whose Stack holds the panel
//! track, any reference video tracks and the sound tracks, as Clips with
//! ExternalReference media, Gaps and Transitions, plus the markers on the
//! panel track. Transitions start at their cuts (`in_offset` 0), so the
//! outgoing clip's media covers `out_offset`. Emulsion's transition kind
//! and clip gain travel in `metadata.emulsion`.
use anyhow::{Context, Result, bail};
use emulsion_core::timeline::{
    Edit, EditClip, EditMarker, EditTransition, FrameRate, TransitionKind,
};
use serde_json::{Value, json};

fn time(frames: u64, rate: f64) -> Value {
    json!({"OTIO_SCHEMA":"RationalTime.1","rate":rate,"value":frames as f64})
}

fn range(start: u64, frames: u64, rate: f64) -> Value {
    json!({"OTIO_SCHEMA":"TimeRange.1","start_time":time(start,rate),"duration":time(frames,rate)})
}

fn gap(frames: u64, rate: f64) -> Value {
    json!({"OTIO_SCHEMA":"Gap.1","name":"","source_range":range(0,frames,rate),"effects":[],"markers":[],"enabled":true,"metadata":{}})
}

fn clip(c: &EditClip, rate: f64) -> Value {
    let reference = if c.media.is_empty() {
        json!({"OTIO_SCHEMA":"MissingReference.1","name":"","available_range":null,"available_image_bounds":null,"metadata":{}})
    } else {
        json!({"OTIO_SCHEMA":"ExternalReference.1","name":"","target_url":super::file_url(&c.media),"available_range":null,"available_image_bounds":null,"metadata":{}})
    };
    let mut metadata = json!({});
    if let Some(db) = c.gain_db {
        metadata["emulsion"] = json!({"gain_db":db});
    }
    json!({
        "OTIO_SCHEMA":"Clip.2",
        "name":c.name,
        "source_range":range(c.source_in,c.frames(),rate),
        "media_references":{"DEFAULT_MEDIA":reference},
        "active_media_reference_key":"DEFAULT_MEDIA",
        "effects":[],
        "markers":[],
        "enabled":true,
        "metadata":metadata
    })
}

fn transition(t: EditTransition, rate: f64) -> Value {
    let kind = if t.kind == TransitionKind::Dissolve {
        "SMPTE_Dissolve"
    } else {
        "Custom_Transition"
    };
    let label = emulsion_core::timeline::Transition {
        kind: t.kind,
        frames: t.frames,
    }
    .label();
    json!({
        "OTIO_SCHEMA":"Transition.1",
        "name":label,
        "transition_type":kind,
        "in_offset":time(0,rate),
        "out_offset":time(u64::from(t.frames),rate),
        "metadata":{"emulsion":{"transition":t.kind}}
    })
}

/// One track's children: clips, with gaps where nothing plays.
fn track(name: &str, kind: &str, clips: &[&EditClip], rate: f64, markers: Value) -> Value {
    let mut children = Vec::new();
    let mut at = 0;
    for c in clips {
        if c.record_in > at {
            children.push(gap(c.record_in - at, rate));
        } else if let Some(t) = c
            .transition
            .filter(|t| t.frames > 0 && !children.is_empty())
        {
            children.push(transition(t, rate));
        }
        children.push(clip(c, rate));
        at = c.record_out.max(at);
    }
    json!({
        "OTIO_SCHEMA":"Track.1",
        "name":name,
        "kind":kind,
        "children":children,
        "source_range":null,
        "effects":[],
        "markers":markers,
        "enabled":true,
        "metadata":{}
    })
}

/// Write `edit` as an OpenTimelineIO timeline.
pub fn write(edit: &Edit) -> String {
    let rate = edit.rate.fps();
    let mut tracks = Vec::new();
    let video = edit.video.iter().map(|c| c.track + 1).max().unwrap_or(0);
    for t in 0..video {
        let clips: Vec<_> = edit.video.iter().filter(|c| c.track == t).collect();
        let markers = if t == 0 {
            edit.markers
                .iter()
                .map(|m| json!({"OTIO_SCHEMA":"Marker.2","name":m.name,"marked_range":range(m.frame,0,rate),"color":"RED","comment":"","metadata":{}}))
                .collect()
        } else {
            Vec::new()
        };
        let name = if t == 0 {
            "Panels".to_string()
        } else {
            format!("Reference {t}")
        };
        tracks.push(track(&name, "Video", &clips, rate, Value::Array(markers)));
    }
    let audio = edit.audio.iter().map(|c| c.track + 1).max().unwrap_or(0);
    for t in 0..audio {
        let clips: Vec<_> = edit.audio.iter().filter(|c| c.track == t).collect();
        tracks.push(track(
            &format!("A{}", t + 1),
            "Audio",
            &clips,
            rate,
            json!([]),
        ));
    }
    let timeline = json!({
        "OTIO_SCHEMA":"Timeline.1",
        "name":edit.name,
        "global_start_time":time(edit.start,rate),
        "metadata":{"emulsion":{"rate":edit.rate}},
        "tracks":{
            "OTIO_SCHEMA":"Stack.1",
            "name":"tracks",
            "children":tracks,
            "source_range":null,
            "effects":[],
            "markers":[],
            "enabled":true,
            "metadata":{}
        }
    });
    serde_json::to_string_pretty(&timeline).unwrap_or_default() + "\n"
}

fn rational(value: &Value) -> Option<(f64, f64)> {
    let rate = value["rate"].as_f64()?;
    let v = value["value"].as_f64()?;
    (rate > 0. && rate.is_finite() && v.is_finite()).then_some((v, rate))
}

/// Frames at `rate` in an OTIO time, which may use another rate.
fn frames(value: &Value, rate: FrameRate) -> Option<u64> {
    let (v, r) = rational(value)?;
    Some((v / r * rate.fps()).round().max(0.) as u64)
}

fn schema(value: &Value) -> &str {
    value["OTIO_SCHEMA"]
        .as_str()
        .and_then(|s| s.split('.').next())
        .unwrap_or_default()
}

/// Read the first Timeline in an OTIO file (a bare Timeline or a
/// SerializableCollection holding one).
pub fn read(text: &str) -> Result<Edit> {
    let root: Value = serde_json::from_str(text).context("This is not an OpenTimelineIO file")?;
    let timeline = if schema(&root) == "Timeline" {
        &root
    } else {
        root["children"]
            .as_array()
            .and_then(|c| c.iter().find(|v| schema(v) == "Timeline"))
            .context("The file holds no timeline")?
    };
    let stack = &timeline["tracks"];
    let tracks = stack["children"]
        .as_array()
        .context("The timeline has no tracks")?;
    // The rate: our own metadata, else the first clip's or the start time's.
    let rate =
        serde_json::from_value::<FrameRate>(timeline["metadata"]["emulsion"]["rate"].clone())
            .ok()
            .filter(|r| r.validate().is_ok())
            .map(Ok)
            .or_else(|| {
                let mut found = rational(&timeline["global_start_time"]).map(|(_, r)| r);
                for track in tracks {
                    for child in track["children"].as_array().into_iter().flatten() {
                        if found.is_none() {
                            found = rational(&child["source_range"]["duration"]).map(|(_, r)| r);
                        }
                    }
                }
                found.map(super::rate_from_fps)
            })
            .context("The timeline has no frame rate")??;
    let mut edit = Edit::new(timeline["name"].as_str().unwrap_or_default(), rate);
    edit.start = frames(&timeline["global_start_time"], rate).unwrap_or(0);
    let (mut video_index, mut audio_index) = (0, 0);
    for track in tracks {
        if schema(track) != "Track" || track["enabled"] == false {
            continue;
        }
        let audio = track["kind"].as_str() == Some("Audio");
        let index = if audio {
            audio_index += 1;
            audio_index - 1
        } else {
            video_index += 1;
            video_index - 1
        };
        let mut at = 0u64;
        let mut pending: Option<EditTransition> = None;
        for child in track["children"].as_array().into_iter().flatten() {
            match schema(child) {
                "Transition" => {
                    let into = frames(&child["out_offset"], rate).unwrap_or(0) as u32;
                    let kind = serde_json::from_value::<TransitionKind>(
                        child["metadata"]["emulsion"]["transition"].clone(),
                    )
                    .unwrap_or(TransitionKind::Dissolve);
                    pending = (into > 0).then_some(EditTransition { kind, frames: into });
                }
                "Gap" => {
                    at += frames(&child["source_range"]["duration"], rate).unwrap_or(0);
                    pending = None;
                }
                "Clip" => {
                    let range = &child["source_range"];
                    let length = frames(&range["duration"], rate).unwrap_or(0);
                    let source_in = frames(&range["start_time"], rate).unwrap_or(0);
                    let reference = child["media_references"]
                        .get(
                            child["active_media_reference_key"]
                                .as_str()
                                .unwrap_or("DEFAULT_MEDIA"),
                        )
                        .unwrap_or(&child["media_reference"]);
                    let media = reference["target_url"]
                        .as_str()
                        .map(super::url_path)
                        .unwrap_or_default();
                    if length > 0 && child["enabled"] != false {
                        let clip = EditClip {
                            name: child["name"].as_str().unwrap_or_default().into(),
                            media,
                            track: index,
                            source_in,
                            source_out: source_in + length,
                            record_in: at,
                            record_out: at + length,
                            transition: if audio { None } else { pending.take() },
                            gain_db: child["metadata"]["emulsion"]["gain_db"]
                                .as_f64()
                                .map(|g| g as f32),
                        };
                        if audio {
                            edit.audio.push(clip);
                        } else {
                            edit.video.push(clip);
                        }
                    }
                    pending = None;
                    at += length;
                }
                // Nested stacks and other items take their length.
                _ => {
                    at += frames(&child["source_range"]["duration"], rate).unwrap_or(0);
                    pending = None;
                }
            }
        }
        if !audio && index == 0 {
            for marker in track["markers"].as_array().into_iter().flatten() {
                if let Some(frame) = frames(&marker["marked_range"]["start_time"], rate) {
                    edit.markers.push(EditMarker {
                        frame,
                        name: marker["name"].as_str().unwrap_or_default().into(),
                    });
                }
            }
        }
    }
    if edit.video.is_empty() && edit.audio.is_empty() {
        bail!("The timeline has no clips")
    }
    edit.sort();
    Ok(edit)
}
