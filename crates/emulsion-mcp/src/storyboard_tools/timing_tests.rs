use super::execute;
use super::tests::{board, call};
use emulsion_core::project::ProjectEditor;
use emulsion_core::timeline::{AudioAsset, Edge, TransitionKind};
use serde_json::{Value, json};

fn refused(e: &mut ProjectEditor, name: &str, args: Value) -> String {
    let stamp = e.stamp();
    let result = execute(e, name, &args);
    assert!(result.is_error, "{name} {args} should fail");
    assert_eq!(e.stamp(), stamp, "{name} {args} changed the project");
    result.content[0]["text"].as_str().unwrap().to_string()
}

/// Four panels of 24, 12, 36 and 48 frames at 24 fps.
fn four(e: &mut ProjectEditor) -> Vec<u64> {
    call(
        e,
        "add_storyboard_panels",
        json!({"panels":[{"frames":12},{"frames":36},{"frames":48}]}),
    );
    let ids = super::layout(e);
    call(
        e,
        "update_storyboard_panel",
        json!({"panel":ids[0],"frames":24}),
    );
    ids
}

fn frames(e: &ProjectEditor, ids: &[u64]) -> Vec<u32> {
    let board = e.storyboard().unwrap();
    ids.iter().map(|id| board.panels[id].frames).collect()
}

fn sound(e: &mut ProjectEditor, name: &str, ms: u64) -> u64 {
    let mut id = 0;
    e.edit_storyboard(|b| {
        id = b.timeline.add_asset(AudioAsset {
            name: name.into(),
            format: "wav".into(),
            duration_ms: ms,
            sample_rate: 48_000,
            channels: 1,
            folder: String::new(),
            source: None,
        })?;
        Ok(())
    })
    .unwrap();
    id
}

fn lock(e: &mut ProjectEditor, panel: u64, locked: bool) {
    call(
        e,
        "set_storyboard_locks",
        json!({"panels":[panel],"locked":locked}),
    );
}

#[test]
fn transitions_set_read_back_and_undo_in_one_step() {
    let mut e = board();
    let ids = four(&mut e);
    let set = call(
        &mut e,
        "set_storyboard_transitions",
        json!({"panels":[ids[1],ids[2]],"kind":"dissolve","seconds":0.25}),
    );
    assert_eq!(set["transition"]["frames"], 6);
    call(
        &mut e,
        "set_storyboard_transitions",
        json!({"panels":[ids[3]],"kind":"wipe","edge":"top","frames":8}),
    );
    let board = e.storyboard().unwrap();
    assert_eq!(
        board.panels[&ids[1]].transition.kind,
        TransitionKind::Dissolve
    );
    assert_eq!(
        board.panels[&ids[3]].transition.kind,
        TransitionKind::Wipe { from: Edge::Top }
    );
    let outline = call(&mut e, "describe_storyboard", json!({}));
    let panels = &outline["acts"][0]["sequences"][0]["scenes"][0]["panels"];
    assert!(panels[0].get("transition").is_none(), "cuts are not listed");
    assert_eq!(panels[1]["start"], 24);
    assert_eq!(panels[1]["timecode"], "00:00:01:00");
    assert_eq!(panels[1]["transition"]["kind"], "dissolve");
    assert_eq!(panels[3]["transition"]["edge"], "top");
    assert_eq!(outline["animatic"]["frames"], 120);
    assert_eq!(outline["animatic"]["timecode"], "00:00:05:00");
    // Fade to black by default, then back to a cut.
    let fade = call(
        &mut e,
        "set_storyboard_transitions",
        json!({"panels":[ids[2]],"kind":"fade_to_color"}),
    );
    assert_eq!(fade["transition"]["color"], "#000000");
    assert_eq!(fade["transition"]["frames"], 12, "half a second by default");
    call(
        &mut e,
        "set_storyboard_transitions",
        json!({"panels":[ids[2]],"kind":"fade_to_color","color":"#FFFFFF","frames":4}),
    );
    assert_eq!(
        e.storyboard().unwrap().panels[&ids[2]].transition.kind,
        TransitionKind::FadeToColor {
            color: [255, 255, 255]
        }
    );
    call(
        &mut e,
        "set_storyboard_transitions",
        json!({"panels":[ids[2]],"kind":"cut"}),
    );
    assert!(e.storyboard().unwrap().panels[&ids[2]].transition.is_cut());
    // One Undo step restores the white fade.
    assert!(e.undo());
    assert_eq!(e.storyboard().unwrap().panels[&ids[2]].transition.frames, 4);
}

#[test]
fn invalid_or_locked_transitions_change_nothing() {
    let mut e = board();
    let ids = four(&mut e);
    for args in [
        json!({"panels":[ids[1]],"kind":"spin"}),
        json!({"panels":[ids[1]],"kind":"dissolve","edge":"left"}),
        json!({"panels":[ids[1]],"kind":"wipe","color":"#000000"}),
        json!({"panels":[ids[1]],"kind":"fade_to_color","color":"black"}),
        json!({"panels":[ids[1]],"kind":"dissolve","frames":13}),
        json!({"panels":[ids[1]],"kind":"dissolve","frames":2,"seconds":1}),
        json!({"panels":[ids[1]],"kind":"cut","frames":2}),
        json!({"panels":[ids[1],ids[1]],"kind":"iris"}),
        json!({"panels":[99],"kind":"iris"}),
        json!({"panels":[ids[1]],"kind":"iris","timecode":"bad timecode"}),
        json!({"panels":[],"kind":"iris"}),
    ] {
        refused(&mut e, "set_storyboard_transitions", args);
    }
    lock(&mut e, ids[1], true);
    assert!(
        refused(
            &mut e,
            "set_storyboard_transitions",
            json!({"panels":[ids[1]],"kind":"clock","frames":4}),
        )
        .contains("locked")
    );
}

#[test]
fn durations_set_in_frames_seconds_and_timecode() {
    let mut e = board();
    let ids = four(&mut e);
    call(
        &mut e,
        "set_storyboard_transitions",
        json!({"panels":[ids[2]],"kind":"dissolve","frames":20}),
    );
    let set = call(
        &mut e,
        "set_storyboard_timing",
        json!({"panels":[
            {"panel":ids[0],"frames":30},
            {"panel":ids[1],"seconds":1.5},
            {"panel":ids[2],"timecode":"00:00:00:12"}
        ]}),
    );
    assert_eq!(frames(&e, &ids), [30, 36, 12, 48]);
    assert_eq!(set["transitions_shortened"], json!([ids[2]]));
    assert_eq!(
        e.storyboard().unwrap().panels[&ids[2]].transition.frames,
        12
    );
    assert_eq!(set["total_frames"], 126);
    assert!(e.undo());
    assert_eq!(frames(&e, &ids), [24, 12, 36, 48]);
    assert_eq!(
        e.storyboard().unwrap().panels[&ids[2]].transition.frames,
        20
    );
    for args in [
        json!({"panels":[{"panel":ids[0]}]}),
        json!({"panels":[{"panel":ids[0],"frames":2,"seconds":1}]}),
        json!({"panels":[{"panel":ids[0],"frames":2},{"panel":ids[0],"frames":3}]}),
        json!({"panels":[{"panel":ids[0],"seconds":0.001}]}),
        json!({"panels":[{"panel":ids[0],"timecode":"00:00:01:30"}]}),
        json!({"panels":[{"panel":99,"frames":2}]}),
        json!({"panels":[{"panel":ids[0],"frames":0}]}),
    ] {
        refused(&mut e, "set_storyboard_timing", args);
    }
    lock(&mut e, ids[1], true);
    refused(
        &mut e,
        "set_storyboard_timing",
        json!({"panels":[{"panel":ids[0],"frames":10},{"panel":ids[1],"frames":10}]}),
    );
    assert_eq!(frames(&e, &ids), [24, 12, 36, 48]);
}

#[test]
fn fit_roll_and_snap_retime_the_animatic() {
    let mut e = board();
    let ids = four(&mut e);
    let street = call(
        &mut e,
        "start_storyboard_group",
        json!({"panel":ids[2],"level":"scene","name":"Street"}),
    )["group"]
        .as_u64()
        .unwrap();
    // Fit the first scene (24 + 12) to 3 seconds, keeping proportions.
    let fit = call(
        &mut e,
        "fit_storyboard_timing",
        json!({"panels":[ids[0],ids[1]],"seconds":3}),
    );
    assert_eq!(fit["selection_frames"], 72);
    assert_eq!(frames(&e, &ids), [48, 24, 36, 48]);
    call(
        &mut e,
        "fit_storyboard_timing",
        json!({"scene_names":["street"],"timecode":"00:00:02:00"}),
    );
    assert_eq!(frames(&e, &ids)[2..], [21, 27]);
    assert!(e.undo());
    assert_eq!(frames(&e, &ids)[2..], [36, 48]);
    // Roll the cut after the first panel earlier by half a second.
    let rolled = call(
        &mut e,
        "roll_storyboard_cut",
        json!({"panel":ids[0],"seconds":-0.5}),
    );
    assert_eq!(rolled["moved"], -12);
    assert_eq!(frames(&e, &ids), [36, 36, 36, 48]);
    let clamped = call(
        &mut e,
        "roll_storyboard_cut",
        json!({"panel":ids[1],"frames":100}),
    );
    assert_eq!(clamped["moved"], 35);
    assert!(e.undo());
    assert_eq!(frames(&e, &ids), [36, 36, 36, 48]);
    // Snap cuts to dialogue markers.
    call(
        &mut e,
        "add_storyboard_audio_track",
        json!({"name":"Dialogue"}),
    );
    call(
        &mut e,
        "add_storyboard_markers",
        json!({"track":1,"markers":[{"name":"Line 2","at":40},{"name":"Line 1","at_timecode":"00:00:01:09"}]}),
    );
    let snapped = call(&mut e, "snap_storyboard_cuts", json!({"frames":6}));
    assert_eq!(snapped["cuts_moved"].as_array().unwrap().len(), 1);
    assert_eq!(frames(&e, &ids), [33, 39, 36, 48]);
    assert!(e.undo());
    assert_eq!(frames(&e, &ids), [36, 36, 36, 48]);
    for (name, args) in [
        ("fit_storyboard_timing", json!({"panels":[ids[0]]})),
        ("fit_storyboard_timing", json!({"seconds":2})),
        (
            "fit_storyboard_timing",
            json!({"scenes":[street],"frames":1}),
        ),
        (
            "fit_storyboard_timing",
            json!({"scene_names":["Nowhere"],"frames":10}),
        ),
        ("fit_storyboard_timing", json!({"scenes":[999],"frames":10})),
        ("roll_storyboard_cut", json!({"panel":ids[3],"frames":2})),
        ("roll_storyboard_cut", json!({"panel":ids[0],"frames":1.5})),
        ("roll_storyboard_cut", json!({"panel":ids[0],"frames":0})),
        ("roll_storyboard_cut", json!({"panel":ids[0]})),
        ("snap_storyboard_cuts", json!({"frames":2,"seconds":1})),
    ] {
        refused(&mut e, name, args);
    }
    // Locked panels refuse every timing edit.
    lock(&mut e, ids[1], true);
    refused(
        &mut e,
        "fit_storyboard_timing",
        json!({"panels":[ids[0],ids[1]],"frames":100}),
    );
    refused(
        &mut e,
        "roll_storyboard_cut",
        json!({"panel":ids[0],"frames":2}),
    );
    refused(&mut e, "snap_storyboard_cuts", json!({"frames":6}));
    lock(&mut e, ids[1], false);
    let mut empty = board();
    refused(&mut empty, "snap_storyboard_cuts", json!({}));
}

#[test]
fn audio_tracks_clips_and_markers_edit_as_single_steps() {
    let mut e = board();
    let ids = four(&mut e);
    let rain = sound(&mut e, "Rain", 2000);
    let line = sound(&mut e, "Line", 1000);
    assert_eq!(
        call(
            &mut e,
            "add_storyboard_audio_track",
            json!({"name":"Dialogue"})
        )["track"],
        1
    );
    call(
        &mut e,
        "add_storyboard_audio_track",
        json!({"name":"FX","volume_db":-6}),
    );
    let updated = call(
        &mut e,
        "update_storyboard_audio_track",
        json!({"track":2,"name":"Ambience","solo":true,"muted":false}),
    );
    assert_eq!(updated["name"], "Ambience");
    // The whole sound by default, starting at a panel.
    let placed = call(
        &mut e,
        "place_storyboard_sound",
        json!({"sound":line,"track":1,"at_panel":ids[1],"gain_db":-3,"fade_out":4}),
    );
    assert_eq!(
        (placed["start"].clone(), placed["frames"].clone()),
        (json!(24), json!(24))
    );
    call(
        &mut e,
        "place_storyboard_sound",
        json!({"sound":rain,"track":2,"at":0,"offset_ms":500,"seconds":1,"name":"Rain loop"}),
    );
    let first = call(
        &mut e,
        "place_storyboard_sound",
        json!({"sound":line,"track":1,"at_timecode":"00:00:00:00","frames":12}),
    );
    assert_eq!(first["clip"], 1, "clips stay in time order");
    // Overlaps are refused.
    refused(
        &mut e,
        "place_storyboard_sound",
        json!({"sound":line,"track":1,"at":30}),
    );
    // Move the line clip to the FX track and trim it.
    let moved = call(
        &mut e,
        "update_storyboard_audio_clip",
        json!({"track":1,"clip":2,"to_track":2,"at":48,"frames":10,"offset_ms":100,"name":"Mia line"}),
    );
    assert_eq!(
        (moved["track"].clone(), moved["clip"].clone()),
        (json!(2), json!(2))
    );
    let board = e.storyboard().unwrap();
    assert_eq!(board.timeline.tracks[0].clips.len(), 1);
    let clip = &board.timeline.tracks[1].clips[1];
    assert_eq!(
        (clip.start, clip.frames, clip.offset_ms, clip.name.as_str()),
        (48, 10, 100, "Mia line")
    );
    assert!(e.undo());
    assert_eq!(e.storyboard().unwrap().timeline.tracks[0].clips.len(), 2);
    // Markers stay in time order.
    call(
        &mut e,
        "add_storyboard_markers",
        json!({"track":1,"markers":[{"name":"B","at":30},{"name":"A","at_seconds":0.5}]}),
    );
    let marker = call(
        &mut e,
        "update_storyboard_marker",
        json!({"track":1,"marker":1,"at":40,"name":"A2"}),
    );
    assert_eq!(marker["marker"], 2);
    let describe = call(&mut e, "describe_storyboard", json!({}));
    let tracks = &describe["animatic"]["tracks"];
    assert_eq!(tracks[0]["markers"][1]["name"], "A2");
    assert_eq!(tracks[0]["clips"][1]["gain_db"], -3.);
    assert_eq!(tracks[1]["solo"], true);
    assert_eq!(tracks[0]["audible"], false, "another track is soloed");
    assert_eq!(tracks[1]["clips"][0]["name"], "Rain loop");
    assert_eq!(describe["animatic"]["sounds"][1]["clips"], 2);
    call(
        &mut e,
        "delete_storyboard_markers",
        json!({"track":1,"markers":[1]}),
    );
    call(
        &mut e,
        "delete_storyboard_audio_clips",
        json!({"track":1,"clips":[1,2]}),
    );
    assert!(e.storyboard().unwrap().timeline.tracks[0].clips.is_empty());
    assert!(e.undo());
    assert_eq!(e.storyboard().unwrap().timeline.tracks[0].clips.len(), 2);
    call(&mut e, "delete_storyboard_audio_track", json!({"track":1}));
    assert_eq!(e.storyboard().unwrap().timeline.tracks.len(), 1);
    assert!(e.undo());
    assert_eq!(e.storyboard().unwrap().timeline.tracks.len(), 2);
    for (name, args) in [
        ("add_storyboard_audio_track", json!({"name":" "})),
        (
            "add_storyboard_audio_track",
            json!({"name":"x","volume_db":30}),
        ),
        (
            "update_storyboard_audio_track",
            json!({"track":3,"muted":true}),
        ),
        ("delete_storyboard_audio_track", json!({"track":5})),
        ("place_storyboard_sound", json!({"sound":99,"track":1})),
        ("place_storyboard_sound", json!({"sound":rain,"track":3})),
        (
            "place_storyboard_sound",
            json!({"sound":rain,"track":1,"at":200,"offset_ms":2000}),
        ),
        (
            "place_storyboard_sound",
            json!({"sound":rain,"track":1,"at":200,"frames":4,"fade_in":3,"fade_out":3}),
        ),
        (
            "place_storyboard_sound",
            json!({"sound":rain,"track":1,"at":200,"at_panel":ids[0]}),
        ),
        (
            "update_storyboard_audio_clip",
            json!({"track":1,"clip":9,"at":0}),
        ),
        (
            "update_storyboard_audio_clip",
            json!({"track":1,"clip":1,"at":30}),
        ),
        (
            "delete_storyboard_audio_clips",
            json!({"track":1,"clips":[1,1]}),
        ),
        (
            "delete_storyboard_audio_clips",
            json!({"track":1,"clips":[7]}),
        ),
        (
            "add_storyboard_markers",
            json!({"track":1,"markers":[{"name":"x"}]}),
        ),
        (
            "add_storyboard_markers",
            json!({"track":1,"markers":[{"name":"x","at_panel":99}]}),
        ),
        (
            "update_storyboard_marker",
            json!({"track":1,"marker":5,"name":"x"}),
        ),
        (
            "delete_storyboard_markers",
            json!({"track":2,"markers":[1]}),
        ),
    ] {
        refused(&mut e, name, args);
    }
    // Locks protect panels, not sound: audio still edits.
    lock(&mut e, ids[0], true);
    call(
        &mut e,
        "update_storyboard_audio_track",
        json!({"track":1,"volume_db":-2}),
    );
}

#[test]
fn the_sound_library_renames_files_and_removes_unused_sounds() {
    let mut e = board();
    let rain = sound(&mut e, "Rain", 2000);
    let wind = sound(&mut e, "Wind", 2000);
    let door = sound(&mut e, "Door", 500);
    call(&mut e, "add_storyboard_audio_track", json!({"name":"FX"}));
    call(
        &mut e,
        "place_storyboard_sound",
        json!({"sound":rain,"track":1}),
    );
    call(
        &mut e,
        "update_storyboard_sounds",
        json!({"sounds":[{"sound":rain,"name":"Heavy rain","folder":" Ambience / Weather /"},{"sound":door,"folder":"Foley"}]}),
    );
    let assets = &e.storyboard().unwrap().timeline.assets;
    assert_eq!(assets[&rain].name, "Heavy rain");
    assert_eq!(assets[&rain].folder, "Ambience/Weather");
    assert_eq!(assets[&door].folder, "Foley");
    assert!(refused(&mut e, "remove_storyboard_sounds", json!({"sounds":[rain]})).contains("used"));
    for args in [
        json!({"sounds":[{"sound":99,"name":"x"}]}),
        json!({"sounds":[{"sound":rain,"name":""}]}),
        json!({"sounds":[{"sound":rain,"folder":"../up"}]}),
        json!({"sounds":[{"sound":rain},{"sound":rain}]}),
    ] {
        refused(&mut e, "update_storyboard_sounds", args);
    }
    call(&mut e, "remove_storyboard_sounds", json!({"sounds":[door]}));
    let removed = call(&mut e, "remove_storyboard_sounds", json!({}));
    assert_eq!(removed["removed"], json!([wind]));
    assert_eq!(removed["sound_count"], 1);
    assert!(e.undo());
    assert!(e.storyboard().unwrap().timeline.assets.contains_key(&wind));
    assert!(!e.storyboard().unwrap().timeline.assets.contains_key(&door));
}

#[test]
fn long_audio_lists_are_paged() {
    let mut e = board();
    let tick = sound(&mut e, "Tick", 40);
    call(
        &mut e,
        "add_storyboard_audio_track",
        json!({"name":"Clicks"}),
    );
    e.edit_storyboard(|b| {
        for i in 0..250 {
            b.timeline.place(
                0,
                emulsion_core::timeline::AudioClip {
                    asset: tick,
                    name: "Tick".into(),
                    start: i * 2,
                    frames: 1,
                    offset_ms: 0,
                    gain_db: 0.,
                    fade_in: 0,
                    fade_out: 0,
                },
            )?;
        }
        Ok(())
    })
    .unwrap();
    let first = call(&mut e, "describe_storyboard", json!({}));
    let animatic = &first["animatic"];
    assert_eq!(animatic["tracks"][0]["clip_count"], 250);
    assert_eq!(
        animatic["tracks"][0]["clips"].as_array().unwrap().len(),
        200
    );
    assert_eq!(animatic["more"], true);
    let next = call(&mut e, "describe_storyboard", json!({"audio_from":200}));
    let clips = &next["animatic"]["tracks"][0]["clips"];
    assert_eq!(clips.as_array().unwrap().len(), 50);
    assert_eq!(clips[0]["clip"], 201);
    assert_eq!(next["animatic"]["more"], false);
}
