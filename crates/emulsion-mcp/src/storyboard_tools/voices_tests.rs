use super::execute;
use super::tests::{board, call};
use emulsion_core::storyboard_voices::{SCRATCH_TRACK, VoiceEngine};
use emulsion_core::timeline::AudioTrack;
use serde_json::json;

/// A board whose first panel says two lines.
fn with_dialogue() -> emulsion_core::project::ProjectEditor {
    let mut e = board();
    let first = e.page_list()[0].id;
    call(
        &mut e,
        "update_storyboard_panel",
        json!({"panel":first,"captions":{"Dialogue":"MIA (quietly): Is anyone there?\nTOM: Only me."}}),
    );
    e
}

#[test]
fn the_cast_is_set_listed_and_undone_in_one_step() {
    let mut e = with_dialogue();
    let set = call(
        &mut e,
        "set_storyboard_voice_cast",
        json!({"voices":[
            {"character":"Mia","engine":"espeak","voice":"en-gb+f2","rate":1.2,"pitch":60},
            {"character":"TOM","engine":"piper","model":"en_US-ryan-medium.onnx","speaker":1}
        ]}),
    );
    assert_eq!(set["cast"]["MIA"]["voice"], "en-gb+f2");
    assert_eq!(set["cast"]["TOM"]["speaker"], 1);
    let board = e.storyboard().unwrap();
    assert_eq!(
        board.voices.voice("mia").unwrap().engine,
        VoiceEngine::Espeak {
            voice: "en-gb+f2".into()
        }
    );
    let listed = call(&mut e, "list_storyboard_voices", json!({}));
    let characters = listed["characters"].as_array().unwrap();
    assert_eq!(characters[0]["character"], "MIA");
    assert_eq!(characters[0]["cast"], true);
    assert_eq!(
        characters[1]["voice"]["label"],
        "Piper en_US-ryan-medium (speaker 1)"
    );
    assert!(listed["engines"]["espeak"].is_boolean());
    // Removing, and one Undo step for the first cast.
    call(
        &mut e,
        "set_storyboard_voice_cast",
        json!({"voices":[{"character":"tom","remove":true}]}),
    );
    assert!(e.storyboard().unwrap().voices.voice("TOM").is_none());
    e.undo();
    e.undo();
    assert!(e.storyboard().unwrap().voices.is_empty());
    let bad = execute(
        &mut e,
        "set_storyboard_voice_cast",
        &json!({"voices":[{"character":"MIA","engine":"espeak"}]}),
    );
    assert!(bad.is_error);
}

#[test]
fn scratch_dialogue_is_generated_replaced_and_enhanced() {
    if !emulsion_io::voices::espeak_available() || !emulsion_io::ffmpeg::available() {
        eprintln!("skipped: needs eSpeak NG and FFmpeg");
        return;
    }
    let mut e = with_dialogue();
    call(
        &mut e,
        "set_storyboard_voice_cast",
        json!({"voices":[
            {"character":"MIA","engine":"espeak","voice":"en-us+f2"},
            {"character":"TOM","engine":"espeak","voice":"en-us+m3"}
        ]}),
    );
    // A recording of the user's own, already on a track.
    e.edit_storyboard(|b| {
        b.timeline.tracks.push(AudioTrack::new("Voice 1"));
        Ok(())
    })
    .unwrap();
    let done = call(&mut e, "generate_storyboard_scratch_dialogue", json!({}));
    assert_eq!(done["lines"], 2);
    let board = e.storyboard().unwrap();
    let track = board
        .timeline
        .tracks
        .iter()
        .position(|t| t.name == SCRATCH_TRACK)
        .unwrap();
    let clips = &board.timeline.tracks[track].clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].start, 0);
    assert_eq!(clips[1].start, clips[0].end());
    assert!(clips[0].name.starts_with("MIA: "));
    // The panel grew to fit both lines (or already did).
    let first = e.page_list()[0].id;
    assert!(u64::from(board.panels[&first].frames) >= clips[1].end());
    // Generating again replaces the takes, as one Undo step.
    let again = call(
        &mut e,
        "generate_storyboard_scratch_dialogue",
        json!({"panels":[first],"extend_panels":false}),
    );
    assert_eq!(again["replaced"], 2);
    let board = e.storyboard().unwrap();
    assert_eq!(board.timeline.assets.len(), 2);
    assert_eq!(board.voices.lines.len(), 2);
    e.undo();
    assert_eq!(e.storyboard().unwrap().voices.lines.len(), 2);
    e.undo();
    assert!(e.storyboard().unwrap().voices.lines.is_empty());
    e.redo();

    // Enhance the first line: a new sound, the original kept.
    let track_number = track + 1;
    let before = e.storyboard().unwrap().timeline.tracks[track].clips[0].asset;
    let enhanced = call(
        &mut e,
        "enhance_storyboard_dialogue_clip",
        json!({"track":track_number,"clip":1}),
    );
    assert_eq!(enhanced["original_sound"], before);
    let board = e.storyboard().unwrap();
    let now = board.timeline.tracks[track].clips[0].asset;
    assert_ne!(now, before);
    assert!(board.timeline.assets.contains_key(&before));
    assert!(enhanced["name"].as_str().unwrap().ends_with("(enhanced)"));
    let copy = call(
        &mut e,
        "enhance_storyboard_dialogue_clip",
        json!({"track":track_number,"clip":2,"new_track":true}),
    );
    let board = e.storyboard().unwrap();
    let at = copy["track"].as_u64().unwrap() as usize - 1;
    assert_eq!(board.timeline.tracks[at].name, "Enhanced dialogue");
    assert_eq!(board.timeline.tracks[track].clips.len(), 2);
}
