use super::execute;
use super::tests::{board, call};
use emulsion_core::project::ProjectEditor;
use emulsion_core::timeline::{AudioAsset, AudioClip, AudioTrack, ClipParam};
use serde_json::json;

/// A board with one 48-frame clip on track 1.
fn with_clip() -> ProjectEditor {
    let mut e = board();
    e.edit_storyboard(|b| {
        let asset = b.timeline.add_asset(AudioAsset {
            name: "Line".into(),
            format: "wav".into(),
            duration_ms: 4000,
            sample_rate: 48_000,
            channels: 1,
            folder: String::new(),
            source: None,
        })?;
        b.timeline.tracks.push(AudioTrack::new("Dialogue"));
        b.timeline.place(
            0,
            AudioClip {
                asset,
                name: "Line".into(),
                start: 12,
                frames: 48,
                ..AudioClip::default()
            },
        )
    })
    .unwrap();
    e
}

fn clip(e: &ProjectEditor) -> &AudioClip {
    &e.storyboard().unwrap().timeline.tracks[0].clips[0]
}

#[test]
fn envelope_and_eq_keys_set_read_back_and_undo_in_single_steps() {
    let mut e = with_clip();
    let set = call(
        &mut e,
        "set_storyboard_clip_effect_keys",
        json!({"track":1,"clip":1,"param":"envelope","keys":[
            {"frame":0,"db":0},
            {"seconds":1,"db":-12,"easing":"ease_in"}
        ]}),
    );
    assert_eq!(set["envelope"][1]["frame"], 24);
    assert_eq!(set["envelope"][1]["db"], -12.);
    assert_eq!(set["envelope"][0]["easing"], "linear");
    assert!((clip(&e).param_at(ClipParam::Envelope, 12.) + 6.).abs() < 1e-4);
    // Replacing a key on the same frame.
    call(
        &mut e,
        "set_storyboard_clip_effect_keys",
        json!({"track":1,"clip":1,"param":"envelope","keys":[{"frame":24,"db":-6}]}),
    );
    assert_eq!(clip(&e).envelope.len(), 2);
    assert!(e.undo());
    assert_eq!(clip(&e).envelope[1].db, -12.);

    call(
        &mut e,
        "set_storyboard_clip_eq",
        json!({"track":1,"clip":1,"low_db":6,"high_db":-3}),
    );
    call(
        &mut e,
        "set_storyboard_clip_effect_keys",
        json!({"track":1,"clip":1,"param":"mid","keys":[{"frame":0,"db":-24},{"frame":10,"db":24}]}),
    );
    let read = call(
        &mut e,
        "describe_storyboard_clip_effects",
        json!({"track":1,"clip":1,"frames":[5]}),
    );
    assert_eq!(read["eq"]["on"], true);
    assert_eq!(read["eq"]["low"]["db"], 6.);
    assert_eq!(read["eq"]["mid"]["keys"].as_array().unwrap().len(), 2);
    assert_eq!(read["eq"]["mid"]["hz"], 1000.);
    assert_eq!(read["values"][0]["mid"], 0.);
    assert_eq!(read["values"][0]["high"], -3.);
    let described = call(&mut e, "describe_storyboard", json!({}));
    assert_eq!(
        described["animatic"]["tracks"][0]["clips"][0]["effects"],
        true
    );

    // Fixed gains replace a band's keys; deleting keys one by one or all.
    call(
        &mut e,
        "set_storyboard_clip_eq",
        json!({"track":1,"clip":1,"mid_db":2}),
    );
    assert!(clip(&e).eq.mid.keys.is_empty());
    call(
        &mut e,
        "delete_storyboard_clip_effect_keys",
        json!({"track":1,"clip":1,"param":"envelope","frames":[0]}),
    );
    assert_eq!(clip(&e).envelope.len(), 1);
    call(
        &mut e,
        "delete_storyboard_clip_effect_keys",
        json!({"track":1,"clip":1,"param":"envelope"}),
    );
    assert!(clip(&e).envelope.is_empty());
    call(
        &mut e,
        "set_storyboard_clip_eq",
        json!({"track":1,"clip":1,"low_db":0,"mid_db":0,"high_db":0}),
    );
    assert!(!clip(&e).has_effects());
}

#[test]
fn bad_effect_requests_change_nothing() {
    let mut e = with_clip();
    let stamp = e.stamp();
    for (name, args) in [
        (
            "set_storyboard_clip_effect_keys",
            json!({"track":1,"clip":1,"param":"low","keys":[{"frame":0,"db":-40}]}),
        ),
        (
            "set_storyboard_clip_effect_keys",
            json!({"track":1,"clip":1,"param":"envelope","keys":[{"db":0}]}),
        ),
        (
            "set_storyboard_clip_effect_keys",
            json!({"track":1,"clip":2,"param":"envelope","keys":[{"frame":0,"db":0}]}),
        ),
        (
            "delete_storyboard_clip_effect_keys",
            json!({"track":1,"clip":1,"param":"envelope","frames":[3]}),
        ),
        (
            "set_storyboard_clip_eq",
            json!({"track":2,"clip":1,"low_db":3}),
        ),
    ] {
        let result = execute(&mut e, name, &args);
        assert!(result.is_error, "{name} {args}");
    }
    assert_eq!(e.stamp(), stamp);
}

#[test]
fn trimming_the_head_keeps_effect_keys_on_the_sound() {
    let mut e = with_clip();
    call(
        &mut e,
        "set_storyboard_clip_effect_keys",
        json!({"track":1,"clip":1,"param":"envelope","keys":[
            {"frame":0,"db":0},
            {"frame":24,"db":-12}
        ]}),
    );
    // One second off the head: the start and the offset move together.
    call(
        &mut e,
        "update_storyboard_audio_clip",
        json!({"track":1,"clip":1,"at":36,"offset_ms":1000,"frames":24}),
    );
    let clip = clip(&e);
    assert_eq!((clip.start, clip.offset_ms), (36, 1000));
    assert_eq!(clip.envelope[0].frame, 0);
    assert!((clip.param_at(ClipParam::Envelope, 0.) + 12.).abs() < 1e-4);
}
