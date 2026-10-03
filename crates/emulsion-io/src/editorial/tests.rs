use super::*;
use emulsion_core::project::ProjectEditor;
use emulsion_core::storyboard_conform::RateChoice;
use emulsion_core::timeline::{Edge, Transition, TransitionKind};

/// An edit at 29.97 drop frame long enough to cross dropped numbers: three
/// panels with a dissolve and a wipe, sound on two tracks, a marker and a
/// reference video on V2.
fn sample() -> Edit {
    let rate = FrameRate::ntsc(30);
    let mut edit = Edit::new("Storm & Sea", rate);
    edit.start = Edit::one_hour(rate);
    let clip = |name: &str, media: &str, record_in: u64, frames: u64| EditClip {
        name: name.into(),
        media: media.into(),
        track: 0,
        source_in: 0,
        source_out: frames,
        record_in,
        record_out: record_in + frames,
        transition: None,
        gain_db: None,
    };
    edit.video = vec![
        clip("Panel 1", "/m/Panel_1_p1.png", 0, 1900),
        EditClip {
            transition: Some(EditTransition {
                kind: TransitionKind::Dissolve,
                frames: 12,
            }),
            ..clip("Panel 2", "/m/my media/Panel_2_p2.png", 1900, 48)
        },
        EditClip {
            transition: Some(EditTransition {
                kind: TransitionKind::Wipe { from: Edge::Left },
                frames: 8,
            }),
            ..clip("Panel 3", "/m/Panel_3_p3.png", 1948, 20)
        },
        EditClip {
            track: 1,
            source_in: 30,
            source_out: 90,
            ..clip("Live action", "/m/Live_v9.mp4", 10, 60)
        },
    ];
    edit.audio = vec![
        EditClip {
            source_in: 15,
            source_out: 75,
            gain_db: Some(-6.),
            ..clip("Rain", "/m/Rain_s4.wav", 100, 60)
        },
        EditClip {
            track: 1,
            gain_db: Some(0.),
            ..clip("Thunder", "/m/Thunder_s5.wav", 1800, 100)
        },
    ];
    edit.markers = vec![EditMarker {
        frame: 1850,
        name: "Hit".into(),
    }];
    edit.sort();
    edit
}

#[test]
fn every_format_round_trips_clips_timing_and_drop_frame() {
    let edit = sample();
    for format in Format::ALL {
        let (text, warnings) = write_string(&edit, format, (1920, 1080));
        let back = parse(&text, format, FrameRate::whole(24)).unwrap();
        assert_eq!(back.rate, edit.rate, "{format:?}");
        assert_eq!(back.start, edit.start, "{format:?}");
        assert_eq!(back.markers, edit.markers, "{format:?}");
        let mut expected = edit.clone();
        if format == Format::Edl {
            // One picture track, no levels.
            assert_eq!(warnings.len(), 2, "{warnings:?}");
            expected.video.retain(|c| c.track == 0);
            for c in &mut expected.audio {
                c.gain_db = None;
            }
        } else {
            assert!(warnings.is_empty());
        }
        assert_eq!(back.video, expected.video, "{format:?}");
        assert_eq!(back.audio, expected.audio, "{format:?}");
    }
}

#[test]
fn edl_text_follows_cmx_3600() {
    let (text, _) = write_string(&sample(), Format::Edl, (1920, 1080));
    assert!(text.contains("FCM: DROP FRAME"));
    let lines: Vec<_> = text.lines().collect();
    let rate = FrameRate::ntsc(30);
    let tc = |f: u64| rate.timecode(f);
    let hour = Edit::one_hour(rate);
    assert_eq!(
        lines[3],
        format!(
            "001  AX       V     C        {} {} {} {}",
            tc(0),
            tc(1900),
            tc(hour),
            tc(hour + 1900)
        )
    );
    assert_eq!(tc(hour), "01:00:00;00");
    assert!(lines.contains(&"* FROM CLIP NAME: Panel 1"));
    assert!(
        lines.contains(
            &format!(
                "002  AX       V     D    012 {} {} {} {}",
                tc(0),
                tc(48),
                tc(hour + 1900),
                tc(hour + 1948)
            )
            .as_str()
        )
    );
    assert!(text.contains("W001 008"));
    assert!(text.contains("A2    C"));
    assert!(text.contains(&format!("* LOC: {} RED     Hit", tc(hour + 1850))));
    // A 23.976 board reads a drop-frame EDL at 29.97.
    let other = parse(&text, Format::Edl, FrameRate::ntsc(24)).unwrap();
    assert_eq!(other.rate, FrameRate::ntsc(30));
    // Non-drop numbering at 29.97 counts frames plainly.
    let ndf = "TITLE: x\nFCM: NON-DROP FRAME\n001  AX V C 00:00:00:00 00:01:00:00 01:00:00:00 01:01:00:00\n";
    let edit = parse(ndf, Format::Edl, FrameRate::ntsc(30)).unwrap();
    assert_eq!(edit.video[0].record_out, 1800);
    assert_eq!(edit.video[0].name, "AX");
}

#[test]
fn xmeml_reads_fcp_transitions_with_open_ends() {
    // FCP writes -1 for clip ends inside a centred transition.
    let text = r#"<?xml version="1.0"?><!DOCTYPE xmeml><xmeml version="4"><sequence><name>S</name>
<rate><timebase>25</timebase><ntsc>FALSE</ntsc></rate><media><video><track>
<clipitem id="a"><name>A</name><start>0</start><end>-1</end><in>0</in><out>50</out><file id="f1"><pathurl>file://localhost/Volumes/x/A%20B.png</pathurl></file></clipitem>
<transitionitem><start>45</start><end>55</end><alignment>center</alignment><effect><name>Cross Dissolve</name></effect></transitionitem>
<clipitem id="b"><name>B</name><start>-1</start><end>100</end><in>0</in><out>50</out><file id="f1"/></clipitem>
</track></video></media></sequence></xmeml>"#;
    let edit = parse(text, Format::Xmeml, FrameRate::whole(24)).unwrap();
    assert_eq!(edit.rate, FrameRate::whole(25));
    assert_eq!(edit.video.len(), 2);
    assert_eq!((edit.video[0].record_in, edit.video[0].record_out), (0, 50));
    assert_eq!(edit.video[0].media, "/Volumes/x/A B.png");
    assert_eq!(edit.video[1].media, "/Volumes/x/A B.png");
    assert_eq!(edit.video[1].transition.unwrap().frames, 5);
    assert!(
        parse(
            "<!DOCTYPE x [<!ENTITY a 'b'>]><xmeml/>",
            Format::Xmeml,
            FrameRate::whole(24)
        )
        .is_err()
    );
}

#[test]
fn urls_and_rates() {
    assert_eq!(file_url("/a b/c#.png"), "file:///a%20b/c%23.png");
    assert_eq!(url_path("file:///a%20b/c%23.png"), "/a b/c#.png");
    assert_eq!(url_path("file:///C:/x.png"), "C:/x.png");
    assert_eq!(rate_from_fps(23.976).unwrap(), FrameRate::ntsc(24));
    assert_eq!(rate_from_fps(29.97002997).unwrap(), FrameRate::ntsc(30));
    assert_eq!(rate_from_fps(25.).unwrap(), FrameRate::whole(25));
    assert!(rate_from_fps(0.).is_err());
}

/// The test board (3 panels: 48, 48, 12 frames at 24 fps) with a dissolve
/// into panel 2.
fn board() -> Project {
    let mut project = crate::storyboard_export::tests::project();
    let second = project.pages[1].meta.id;
    project
        .storyboard
        .as_mut()
        .unwrap()
        .panels
        .get_mut(&second)
        .unwrap()
        .transition = Transition {
        kind: TransitionKind::Iris,
        frames: 6,
    };
    project
}

#[test]
fn exports_write_media_and_conform_back_without_changes() {
    let dir = tempfile::tempdir().unwrap();
    let project = board();
    for format in Format::ALL {
        let path = dir.path().join(format!("Film.{}", format.extension()));
        let options = ExportOptions {
            format,
            media: MediaKind::Still,
            width: 64,
        };
        let mut calls = 0;
        let report = export(
            &project,
            "Film",
            &options,
            &path,
            &mut |_, _| calls += 1,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(report.clips, 3);
        assert_eq!(report.media_folder, dir.path().join("Film_media"));
        assert!(report.files.iter().all(|f| f.is_file()));
        let edit = read(&path, FrameRate::whole(24)).unwrap();
        assert_eq!(edit.video.len(), 3);
        assert!(
            edit.video[0]
                .media
                .ends_with(&format!("Panel_1_p{}.png", project.pages[0].meta.id))
        );
        let mut editor = ProjectEditor::open(project.clone(), None).unwrap();
        let before = editor.storyboard().unwrap().clone();
        let conform = editor
            .conform_storyboard(&edit, RateChoice::Convert, true)
            .unwrap();
        assert_eq!(conform.matched, 3, "{format:?}");
        assert!(conform.is_noop(), "{format:?}: {conform:?}");
        // The iris came back as a dissolve in every format but OTIO, and
        // the board keeps its own kind.
        assert_eq!(editor.storyboard().unwrap(), &before, "{format:?}");
    }
}
