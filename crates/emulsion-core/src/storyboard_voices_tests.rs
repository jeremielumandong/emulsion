use crate::storyboard::{Settings, Storyboard};
use crate::storyboard_voices::*;
use crate::timeline::{AudioAsset, AudioClip, AudioTrack};

fn board() -> Storyboard {
    let mut b = Storyboard::new(Settings::new(64, 36), &[1, 2, 3]);
    let dialogue = b.dialogue_field().unwrap();
    let mut say = |panel: u64, text: &str| {
        b.panels
            .get_mut(&panel)
            .unwrap()
            .captions
            .insert(dialogue, text.into());
    };
    say(1, "MIA (quietly): Is anyone there?\nTOM (O.S.): Only me.");
    say(3, "Mia (V.O.): I knew it.");
    b
}

fn take(ms: u64) -> AudioAsset {
    AudioAsset {
        name: "take".into(),
        format: "wav".into(),
        duration_ms: ms,
        sample_rate: 22_050,
        channels: 1,
        folder: String::new(),
        source: None,
    }
}

const LAYOUT: [u64; 3] = [1, 2, 3];

#[test]
fn dialogue_captions_become_lines_with_characters() {
    let lines = dialogue_lines(
        "MIA (quietly): Is anyone *there*?\n  and then (beat) more.\nTOM (O.S.) (CONT'D): Only me.\n\nDR. LEE: Hm.",
    );
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].character, "MIA");
    assert_eq!(lines[0].parenthetical.as_deref(), Some("(quietly)"));
    assert_eq!(
        spoken_text(&lines[0].text),
        "Is anyone there? and then more."
    );
    // Screenplay extensions belong to the cue, not the delivery.
    assert_eq!(lines[1].character, "TOM");
    assert_eq!(lines[1].parenthetical, None);
    assert_eq!(lines[2].character, "DR. LEE");
    // No cue: one line by nobody in particular; sentences are not cues.
    let plain = dialogue_lines("Well, look: nothing here.");
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].character, "");
    assert!(dialogue_lines("MIA: (sighs)").is_empty());
    assert_eq!(character_key("Mia  (V.O.)"), "MIA");

    let b = board();
    assert_eq!(b.characters(&LAYOUT), ["MIA", "TOM"]);
    let plan = b.scratch_plan(&LAYOUT, &[1, 3]);
    assert_eq!(plan.len(), 3);
    assert_eq!((plan[2].panel, plan[2].line), (3, 0));
    // Mia cast once, in any spelling.
    assert_eq!(plan[2].cast_index, plan[0].cast_index);
    assert_eq!(plan[1].cast_index, 1);
    assert!(plan.iter().all(|l| l.voice.is_none()));
}

#[test]
fn the_cast_persists_and_old_boards_load_without_one() {
    let mut b = board();
    let json = serde_json::to_string(&b).unwrap();
    assert!(!json.contains("voices"), "an empty cast is not saved");
    let back: Storyboard = serde_json::from_str(&json).unwrap();
    assert!(back.voices.is_empty());

    b.voices.voices.insert(
        "MIA".into(),
        Voice {
            rate: 1.25,
            pitch: 60,
            ..Voice::new(VoiceEngine::Espeak {
                voice: "en-gb+f2".into(),
            })
        },
    );
    b.voices.voices.insert(
        "TOM".into(),
        Voice::new(VoiceEngine::Piper {
            model: "en_US-ryan-medium.onnx".into(),
            speaker: Some(2),
        }),
    );
    b.validate(&LAYOUT).unwrap();
    let back: Storyboard = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
    assert_eq!(back.voices, b.voices);
    assert_eq!(
        back.scratch_plan(&LAYOUT, &[3])[0]
            .voice
            .as_ref()
            .unwrap()
            .rate,
        1.25
    );
    // Option-like or odd voice names are refused.
    b.voices.voices.get_mut("MIA").unwrap().engine = VoiceEngine::Espeak {
        voice: "--stdout".into(),
    };
    assert!(b.validate(&LAYOUT).is_err());
}

#[test]
fn takes_land_per_panel_and_regenerating_replaces_only_scratch_clips() {
    let mut b = board();
    // The user's own recording on the scratch track's spot.
    let own = b.timeline.add_asset(take(1000)).unwrap();
    b.timeline.tracks.push(AudioTrack::new("Voice 1"));
    b.timeline
        .place(
            0,
            AudioClip {
                asset: own,
                name: "Recording 1".into(),
                start: 0,
                frames: 24,
                ..Default::default()
            },
        )
        .unwrap();
    let plan = b.scratch_plan(&LAYOUT, &LAYOUT);
    // 1.5 s, 1 s and 3 s at 24 fps: panel 1 needs 60 frames of its 48.
    let takes: Vec<_> = plan
        .into_iter()
        .zip([1500, 1000, 3000])
        .map(|(l, ms)| (l, take(ms)))
        .collect();
    let report = b
        .apply_scratch(&LAYOUT, &LAYOUT, takes.clone(), true)
        .unwrap();
    b.validate(&LAYOUT).unwrap();
    assert_eq!((report.placed, report.removed), (3, 0));
    assert_eq!(report.extended, [1, 3]);
    assert_eq!(b.panels[&1].frames, 60);
    let track = b
        .timeline
        .tracks
        .iter()
        .position(|t| t.name == SCRATCH_TRACK)
        .unwrap();
    let clips = &b.timeline.tracks[track].clips;
    // Back to back in panel 1, then from panel 3's start (60 + 48).
    let spans: Vec<_> = clips.iter().map(|c| (c.start, c.frames)).collect();
    assert_eq!(spans, [(0, 36), (36, 24), (108, 72)]);
    assert_eq!(clips[0].name, "MIA: Is anyone there?");
    let sound = &b.timeline.assets[&clips[0].asset];
    assert_eq!(sound.folder, SCRATCH_FOLDER);
    assert!(b.is_scratch(clips[0].asset) && !b.is_scratch(own));

    // Regenerate panel 3 only: its old take goes, the rest stays.
    let plan3 = b.scratch_plan(&LAYOUT, &[3]);
    let report = b
        .apply_scratch(&LAYOUT, &[3], vec![(plan3[0].clone(), take(500))], false)
        .unwrap();
    assert_eq!((report.placed, report.removed), (1, 1));
    assert_eq!(b.voices.lines.len(), 3);
    assert_eq!(
        b.timeline.assets.len(),
        4,
        "the replaced take left the library"
    );
    // Regenerating everything never touches the recording.
    b.apply_scratch(&LAYOUT, &LAYOUT, Vec::new(), false)
        .unwrap();
    assert!(b.voices.lines.is_empty());
    assert_eq!(b.timeline.tracks[0].clips[0].asset, own);
    assert_eq!(b.timeline.assets.keys().copied().collect::<Vec<_>>(), [own]);
    b.validate(&LAYOUT).unwrap();
}

#[test]
fn locked_panels_keep_their_length_and_overflow_moves_to_another_track() {
    let mut b = board();
    b.panels.get_mut(&1).unwrap().locked = true;
    let takes: Vec<_> = b
        .scratch_plan(&LAYOUT, &[1])
        .into_iter()
        .map(|l| (l, take(3000)))
        .collect();
    let report = b.apply_scratch(&LAYOUT, &[1], takes, true).unwrap();
    assert_eq!(report.locked, [1]);
    assert_eq!(b.panels[&1].frames, 48);
    // Without extending, a line running into the next panel's line goes
    // on "Scratch dialogue 2".
    let mut b = board();
    let takes: Vec<_> = b
        .scratch_plan(&LAYOUT, &LAYOUT)
        .into_iter()
        .map(|l| (l, take(3000)))
        .collect();
    b.apply_scratch(&LAYOUT, &LAYOUT, takes, false).unwrap();
    let names: Vec<_> = b.timeline.tracks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, [SCRATCH_TRACK, "Scratch dialogue 2"]);
    b.validate(&LAYOUT).unwrap();
}

#[test]
fn regenerated_lines_and_enhanced_clips_keep_the_originals() {
    let mut b = board();
    let plan = b.scratch_plan(&LAYOUT, &[1]);
    let takes = plan.into_iter().map(|l| (l, take(1000))).collect();
    b.apply_scratch(&LAYOUT, &[1], takes, false).unwrap();
    let old = b.timeline.tracks[0].clips[0].asset;
    let slower = Delivery {
        rate: 0.8,
        emphasis: Emphasis::Strong,
        ..Delivery::default()
    };
    // A longer take stops at the next clip.
    let new = b.replace_scratch_take(old, take(5000), slower).unwrap();
    let clip = &b.timeline.tracks[0].clips[0];
    assert_eq!((clip.asset, clip.frames), (new, 24));
    assert!(b.timeline.assets.contains_key(&old));
    assert_eq!(b.voices.lines[&new].delivery, slower);
    assert_eq!(b.timeline.assets[&new].folder, SCRATCH_FOLDER);
    b.validate(&LAYOUT).unwrap();

    // Enhance in place: the clip plays the new sound, the old one stays.
    let at = b.place_enhanced((0, 0), take(1000), false).unwrap();
    assert_eq!(at, (0, 0));
    let enhanced = b.timeline.tracks[0].clips[0].asset;
    assert_ne!(enhanced, new);
    assert!(b.timeline.assets.contains_key(&new));
    assert!(b.timeline.assets[&enhanced].name.ends_with("(enhanced)"));
    assert!(b.is_scratch(enhanced));
    // Or as a new clip on the Enhanced dialogue track.
    let at = b.place_enhanced((0, 1), take(1000), true).unwrap();
    assert_eq!(b.timeline.tracks[at.0].name, ENHANCED_TRACK);
    assert_eq!(b.timeline.tracks[0].clips.len(), 2);
    b.validate(&LAYOUT).unwrap();
    assert!(
        b.replace_scratch_take(9999, take(10), Delivery::default())
            .is_err()
    );
}

#[test]
fn deliveries_stay_in_range() {
    let voice = Voice {
        rate: 1.8,
        pitch: 90,
        ..Voice::new(VoiceEngine::Espeak { voice: "en".into() })
    };
    let d = Delivery {
        rate: 1.5,
        pitch: 20,
        ..Delivery::default()
    };
    assert_eq!(d.applied(&voice), (2.0, 99));
    assert!(Delivery { rate: 3., ..d }.validate().is_err());
    assert_eq!(line_name("", "  "), "Scratch line");
    assert!(line_name("MIA", &"word ".repeat(40)).ends_with('…'));
}
