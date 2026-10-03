use super::*;
use crate::command::Slot;
use crate::project::ProjectKind;
use crate::storyboard::{CameraKey, CameraState, Level, SceneCamera};
use crate::timeline::{AudioAsset, AudioClip, AudioTrack};
use crate::{Command, Node};

fn layout(p: &ProjectEditor) -> Vec<PageId> {
    p.page_list().iter().map(|m| m.id).collect()
}

/// Four 24-frame panels in scenes A [1], B [2, 3], C [4]; a sound under
/// the whole board and one in scene C; a camera move in scene B.
fn source() -> (ProjectEditor, Vec<PageId>, GroupId) {
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
    let blank = p.storyboard().unwrap().blank_panel().unwrap();
    let items = (2..=4)
        .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
        .collect();
    p.insert_panels(Some(1), &blank, items, None).unwrap();
    let ids = layout(&p);
    let mut scene_b = 0;
    p.edit_storyboard(|b| {
        b.panels.get_mut(&ids[0]).unwrap().frames = 24;
        b.rename(b.panels[&ids[0]].scene, "A")?;
        scene_b = b.split(&ids, ids[1], Level::Scene, Some("B"))?;
        b.split(&ids, ids[3], Level::Scene, Some("C"))?;
        let rest = b.rest_camera();
        let zoomed = CameraState { zoom: 2., ..rest };
        b.cameras.insert(
            scene_b,
            SceneCamera {
                keys: vec![CameraKey::at(0, rest), CameraKey::at(47, zoomed)],
                shake: None,
            },
        );
        let rain = b.timeline.add_asset(AudioAsset {
            name: "Rain".into(),
            format: "wav".into(),
            duration_ms: 60_000,
            sample_rate: 48_000,
            channels: 2,
            folder: String::new(),
            source: None,
        })?;
        b.timeline.tracks.push(AudioTrack::new("FX"));
        b.timeline.tracks.push(AudioTrack::new("Hits"));
        let clip = |start, frames| AudioClip {
            asset: rain,
            name: "Rain".into(),
            start,
            frames,
            ..AudioClip::default()
        };
        b.timeline.place(0, clip(0, 96))?;
        b.timeline.place(1, clip(80, 10))
    })
    .unwrap();
    (p, ids, scene_b)
}

fn paint(p: &mut ProjectEditor, page: PageId, name: &str) {
    p.set_active_page(page).unwrap();
    p.execute(Command::AddNode {
        node: Box::new(Node::new(
            0,
            name,
            crate::NodeKind::Fill {
                rgba: [200, 10, 10, 255],
            },
        )),
        slot: Slot::TOP,
    })
    .unwrap();
}

fn caption(p: &mut ProjectEditor, page: PageId, text: &str) {
    p.edit_storyboard(|b| {
        let action = b.caption("Action").unwrap();
        b.panels
            .get_mut(&page)
            .unwrap()
            .captions
            .insert(action, text.into());
        Ok(())
    })
    .unwrap();
}

#[test]
fn extracts_carry_the_scenes_cameras_sound_and_a_record() {
    let (mut p, ids, scene_b) = source();
    p.create_board_version("Before the hand-off").unwrap();
    let project = p.snapshot().unwrap();
    let board = p.storyboard().unwrap();
    assert_eq!(board.project_id.len(), 36);
    // Scenes must be neighbours; sequences expand to their scenes.
    let a = board.panels[&ids[0]].scene;
    let c = board.panels[&ids[3]].scene;
    assert!(extract_scenes(&project, &[a, c], "Film", 0).is_err());
    assert!(extract_scenes(&project, &[999], "Film", 0).is_err());
    let sequence = board.scenes[&a].sequence;
    assert_eq!(
        range_scenes(board, &ids, &[sequence]).unwrap(),
        [a, scene_b, c]
    );

    let extract = extract_scenes(&project, &[scene_b], "Film", 1_700_000_000).unwrap();
    let theirs = extract.storyboard.as_ref().unwrap();
    let record = theirs.extract.as_ref().unwrap();
    assert_eq!(record.source_project, board.project_id);
    assert_ne!(theirs.project_id, board.project_id);
    assert_eq!(record.scenes, [scene_b]);
    assert_eq!(record.after, Some(ids[0]));
    assert_eq!((record.start_frame, record.frames), (24, 48));
    assert_eq!(record.extracted_at, 1_700_000_000);
    assert_eq!(
        record.panels.iter().map(|p| p.id).collect::<Vec<_>>(),
        [ids[1], ids[2]]
    );
    assert_eq!(extract.pages.len(), 2);
    assert!(theirs.versions.is_empty(), "board versions stay behind");
    assert_eq!(extract.next_page_id, project.next_page_id);
    assert_eq!(theirs.scenes.len(), 1);
    assert_eq!(theirs.scenes[&scene_b].name, "B");
    assert_eq!(theirs.cameras.len(), 1);
    // The board-long sound is cut to the range; the one in scene C stays.
    let fx = &theirs.timeline.tracks[0].clips;
    assert_eq!((fx[0].start, fx[0].frames, fx[0].offset_ms), (0, 48, 1000));
    assert!(theirs.timeline.tracks[1].clips.is_empty());
    assert_eq!(theirs.timeline.tracks[1].name, "Hits");
    extract.validate().unwrap();
}

#[test]
fn an_edited_extract_merges_back_in_one_step_and_ripples_the_rest() {
    let (mut p, ids, scene_b) = source();
    let extract = extract_scenes(&p.snapshot().unwrap(), &[scene_b], "Film", 0).unwrap();
    let mut artist = ProjectEditor::open(extract, None).unwrap();
    artist.create_board_version("Artist's draft").unwrap();
    // Longer panel, new drawing, a new panel at the end of the scene.
    artist
        .edit_storyboard(|b| {
            b.panels.get_mut(&ids[1]).unwrap().frames = 48;
            Ok(())
        })
        .unwrap();
    paint(&mut artist, ids[2], "Clean line");
    let blank = artist.storyboard().unwrap().blank_panel().unwrap();
    let added = artist
        .insert_panels(
            Some(ids[2]),
            &blank,
            vec![("Panel 3A".into(), Panel::new(0, 12))],
            None,
        )
        .unwrap()[0];
    let theirs = artist.snapshot().unwrap();

    let report = p.plan_merge(&theirs).unwrap();
    assert!(report.same_project);
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
    assert_eq!((report.panels_here, report.panels_there), (2, 3));
    assert_eq!((report.frames_here, report.frames_there), (48, 84));
    let before = p.stamp();
    let summary = p.merge_extract(&theirs, &MergeOptions::default()).unwrap();
    assert_eq!(summary.took_theirs, 3);
    assert_eq!(summary.frames_delta, 36);
    let after = layout(&p);
    assert_eq!(after.len(), 5);
    assert_eq!(after[0], ids[0]);
    assert_eq!(after[4], ids[3]);
    let board = p.storyboard().unwrap();
    assert_eq!(board.panels[&after[1]].frames, 48);
    assert_eq!(board.panels[&after[3]].frames, 12);
    assert_eq!(board.panels[&after[1]].scene, scene_b, "the scene is kept");
    assert_eq!(board.panels[&after[3]].scene, scene_b);
    assert!(board.cameras.contains_key(&scene_b));
    assert!(
        p.page(after[2])
            .unwrap()
            .doc
            .nodes
            .iter()
            .any(|n| n.name == "Clean line")
    );
    assert_ne!(added, 0);
    // Sound after the range moved with it; the range's sound came back.
    let hits = &board.timeline.tracks[1].clips;
    assert_eq!(hits[0].start, 80 + 36);
    let fx: Vec<_> = board.timeline.tracks[0]
        .clips
        .iter()
        .map(|c| (c.start, c.frames))
        .collect();
    assert_eq!(fx, [(0, 24), (24, 48), (108, 24)]);
    board.validate(&after).unwrap();
    assert!(
        board.versions.is_empty(),
        "the extract's versions stay there"
    );
    // One Undo step puts everything back.
    assert!(p.undo());
    assert_eq!(p.stamp(), before);
    assert_eq!(layout(&p), ids);
}

#[test]
fn conflicts_are_reported_and_resolved_either_way() {
    let (mut p, ids, scene_b) = source();
    let extract = extract_scenes(&p.snapshot().unwrap(), &[scene_b], "Film", 0).unwrap();
    let mut artist = ProjectEditor::open(extract, None).unwrap();
    // Both sides change panel 2; the extract deletes panel 3.
    caption(&mut artist, ids[1], "Theirs");
    artist.remove_pages(&[ids[2]]).unwrap();
    let theirs = artist.snapshot().unwrap();
    caption(&mut p, ids[1], "Mine");
    // A panel added here inside scene B.
    let blank = p.storyboard().unwrap().blank_panel().unwrap();
    let mine_new = p
        .insert_panels(
            Some(ids[2]),
            &blank,
            vec![("Mine".into(), Panel::new(0, 24))],
            None,
        )
        .unwrap()[0];

    let report = p.plan_merge(&theirs).unwrap();
    let kinds: Vec<_> = report
        .conflicts
        .iter()
        .map(|c| (c.panel, c.kind, c.default))
        .collect();
    assert_eq!(
        kinds,
        [
            (ids[1], ConflictKind::ChangedHere, Resolution::Theirs),
            (ids[2], ConflictKind::DeletedThere, Resolution::Theirs),
            (mine_new, ConflictKind::AddedHere, Resolution::Mine),
        ]
    );
    assert!(report.conflicts[0].changed_there);
    let action = p.storyboard().unwrap().caption("Action").unwrap();
    let text = |p: &ProjectEditor, id: PageId| {
        p.storyboard().unwrap().panels[&id].captions[&action]
            .text
            .clone()
    };

    // Defaults: theirs for panel 2, panel 3 deleted, my new panel kept.
    p.merge_extract(&theirs, &MergeOptions::default()).unwrap();
    let merged = layout(&p);
    assert_eq!(merged.len(), 4);
    assert_eq!(text(&p, merged[1]), "Theirs");
    assert_eq!(p.page_list()[2].name, "Mine");
    assert!(p.undo());

    // The other way round.
    let options = MergeOptions {
        resolutions: BTreeMap::from([
            (ids[1], Resolution::Mine),
            (ids[2], Resolution::Mine),
            (mine_new, Resolution::Theirs),
        ]),
        allow_other_project: false,
    };
    p.merge_extract(&theirs, &options).unwrap();
    let merged = layout(&p);
    assert_eq!(merged.len(), 4);
    assert_eq!(text(&p, merged[1]), "Mine");
    let names: Vec<_> = p.page_list().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Page 1", "Panel 2", "Panel 3", "Panel 4"]);
    assert!(p.undo());

    // A resolution for a panel without a conflict is a mistake.
    let wrong = MergeOptions {
        resolutions: BTreeMap::from([(ids[0], Resolution::Mine)]),
        allow_other_project: false,
    };
    assert!(p.merge_extract(&theirs, &wrong).is_err());
}

#[test]
fn extracts_of_another_project_are_refused_unless_merged_anyway() {
    let (p, _, scene_b) = source();
    let extract = extract_scenes(&p.snapshot().unwrap(), &[scene_b], "Film", 0).unwrap();
    let (mut other, ids, _) = source();
    assert_ne!(
        other.storyboard().unwrap().project_id,
        p.storyboard().unwrap().project_id
    );
    let report = other.plan_merge(&extract).unwrap();
    assert!(!report.same_project);
    let stamp = other.stamp();
    let error = other
        .merge_extract(&extract, &MergeOptions::default())
        .unwrap_err();
    assert!(error.contains("another project"), "{error}");
    assert_eq!(other.stamp(), stamp);
    let anyway = MergeOptions {
        allow_other_project: true,
        ..Default::default()
    };
    other.merge_extract(&extract, &anyway).unwrap();
    assert_eq!(layout(&other).len(), ids.len());
    // Not an extract at all.
    let plain = p.snapshot().unwrap();
    assert!(other.plan_merge(&plain).is_err());
}
