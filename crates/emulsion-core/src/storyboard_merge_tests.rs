use super::*;
use crate::command::Slot;
use crate::project::{ProjectEditor, ProjectKind};
use crate::storyboard::{CameraKey, CameraState, Level, SceneCamera};
use crate::timeline::{AudioAsset, AudioClip};
use crate::{Command, Node};

fn layout(p: &ProjectEditor) -> Vec<PageId> {
    p.page_list().iter().map(|m| m.id).collect()
}

/// Panels 1, 2 in scene A and 3, 4 in scene B, 24 frames each, with an FX
/// track holding one sound.
fn base() -> Project {
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
    let blank = p.storyboard().unwrap().blank_panel().unwrap();
    let items = (2..=4)
        .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
        .collect();
    p.insert_panels(Some(1), &blank, items, None).unwrap();
    let ids = layout(&p);
    p.edit_storyboard(|b| {
        b.panels.get_mut(&ids[0]).unwrap().frames = 24;
        b.rename(b.panels[&ids[0]].scene, "A")?;
        b.split(&ids, ids[2], Level::Scene, Some("B"))?;
        let rain = b.timeline.add_asset(sound("Rain"))?;
        b.timeline
            .tracks
            .push(crate::timeline::AudioTrack::new("FX"));
        b.timeline.place(0, clip(rain, 0, 24))
    })
    .unwrap();
    p.snapshot().unwrap()
}

fn sound(name: &str) -> AudioAsset {
    AudioAsset {
        name: name.into(),
        format: "wav".into(),
        duration_ms: 60_000,
        sample_rate: 48_000,
        channels: 2,
        folder: String::new(),
        source: None,
    }
}

fn clip(asset: AssetId, start: u64, frames: u64) -> AudioClip {
    AudioClip {
        asset,
        name: "clip".into(),
        start,
        frames,
        ..AudioClip::default()
    }
}

fn open(p: &Project) -> ProjectEditor {
    ProjectEditor::open(p.clone(), None).unwrap()
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

fn caption(p: &mut ProjectEditor, page: PageId, field: &str, text: &str) {
    p.edit_storyboard(|b| {
        let id = b.caption(field).unwrap();
        b.panels
            .get_mut(&page)
            .unwrap()
            .captions
            .insert(id, text.into());
        Ok(())
    })
    .unwrap();
}

fn text(p: &Project, page: PageId, field: &str) -> String {
    let b = p.storyboard.as_ref().unwrap();
    b.panels[&page]
        .captions
        .get(&b.caption(field).unwrap())
        .map(|c| c.text.clone())
        .unwrap_or_default()
}

fn has_layer(p: &Project, page: PageId, name: &str) -> bool {
    p.pages
        .iter()
        .find(|pg| pg.meta.id == page)
        .unwrap()
        .doc
        .nodes
        .iter()
        .any(|n| n.name == name)
}

fn ids(p: &Project) -> Vec<PageId> {
    p.pages.iter().map(|pg| pg.meta.id).collect()
}

fn names(p: &Project) -> Vec<&str> {
    p.pages.iter().map(|pg| pg.meta.name.as_str()).collect()
}

#[test]
fn imported_versions_keep_one_current_aid_carrier_and_aid_free_off_head_artwork() {
    use crate::drawing_guides::{DrawingGuides, GuideKind, GuideSet};
    use crate::storyboard_versions::{BoardVersion, version_board};

    let meta = PageMeta {
        id: 2,
        name: "Retired".into(),
        bleed_mm: 0.,
    };
    let mut artwork = Document::new(2, 2);
    let mut graph = Graph::new(artwork.clone(), "First");
    let mut theirs = Storyboard::new(crate::storyboard::Settings::new(2, 2), &[2]);
    let saved_board = version_board(&theirs);
    for n in 0..128 {
        artwork.resolution = 100. + n as f32;
        let commit = graph
            .record(&artwork, format!("Drawing {n}"), false)
            .unwrap();
        theirs.versions.list.push(BoardVersion {
            id: n + 1,
            name: format!("Version {n}"),
            time: n,
            layout: vec![meta.clone()],
            board: saved_board.clone(),
            pages: BTreeMap::from([(2, commit)]),
        });
    }
    // The source has one carrier with large, valid names; most imported
    // versions select another commit. Import must not multiply that payload.
    let guides = DrawingGuides {
        sets: vec![GuideSet {
            name: "Large exact name ".repeat(4096),
            guides: vec![GuideKind::Off],
        }],
        active_set: Some(0),
        ..Default::default()
    };
    guides.validate().unwrap();
    let colors = vec![[12, 34, 56], [78, 90, 12]];
    graph.set_retired_live_aids(colors.clone(), guides.clone());
    let source_head = graph.head_branch().tip;
    let source_branches = graph.branches().clone();
    let source_len = graph.len();
    theirs.versions.retired.insert(2, graph);

    // New carrier, existing carrier with explicit defaults, and an existing
    // carrier with different aids all import the same historical drawings.
    for established in [None, Some(false), Some(true)] {
        let mut board = Storyboard::new(crate::storyboard::Settings::new(2, 2), &[1]);
        let destination_guides = if established == Some(true) {
            DrawingGuides {
                guides: vec![GuideKind::Grid { size: 31. }],
                ..Default::default()
            }
        } else {
            DrawingGuides::default()
        };
        let destination_colors = if established == Some(true) {
            vec![[9, 8, 7]]
        } else {
            Vec::new()
        };
        if established.is_some() {
            let mut destination = Graph::new(Document::new(2, 2), "Existing");
            destination
                .set_retired_live_aids(destination_colors.clone(), destination_guides.clone());
            board.versions.retired.insert(2, destination);
        }
        assert_eq!(
            import_versions(&mut board, &mut [], &theirs, &HashMap::new()).unwrap(),
            128
        );
        let graph = &board.versions.retired[&2];
        let tip = graph.head_branch().tip;
        let (expected_colors, expected_guides) = if established.is_none() {
            (&colors, &guides)
        } else {
            (&destination_colors, &destination_guides)
        };
        assert_eq!(&graph.commit(tip).unwrap().doc.colors, expected_colors);
        assert_eq!(
            &graph.commit(tip).unwrap().doc.drawing_guides,
            expected_guides
        );
        let stored_name_bytes: usize = graph
            .commits()
            .flat_map(|c| &c.doc.drawing_guides.sets)
            .map(|set| set.name.len())
            .sum();
        assert_eq!(
            stored_name_bytes,
            expected_guides
                .sets
                .iter()
                .map(|set| set.name.len())
                .sum::<usize>()
        );
        for commit in graph.commits().filter(|c| c.id != tip) {
            assert!(commit.doc.colors.is_empty());
            assert_eq!(commit.doc.drawing_guides, DrawingGuides::default());
        }
        for (n, version) in board.versions.list.iter().enumerate() {
            let doc = graph.retired_document_at(version.pages[&2]).unwrap();
            assert_eq!(doc.resolution, 100. + n as f32);
            assert_eq!(&doc.colors, expected_colors);
            assert_eq!(&doc.drawing_guides, expected_guides);
        }
    }
    let graph = &theirs.versions.retired[&2];
    assert_eq!(graph.head_branch().tip, source_head);
    assert_eq!(graph.branches(), &source_branches);
    assert_eq!(graph.len(), source_len);

    // A live donor uses its current document, not its stale snapshot aids.
    let mut live = ProjectPage {
        meta: meta.clone(),
        doc: artwork,
        graph: graph.clone(),
    };
    live.doc.colors = vec![[2, 4, 6]];
    live.doc.drawing_guides = DrawingGuides::default();
    let live_sources = HashMap::from([(2, &live)]);
    let mut board = Storyboard::new(crate::storyboard::Settings::new(2, 2), &[1]);
    import_versions(&mut board, &mut [], &theirs, &live_sources).unwrap();
    let graph = &board.versions.retired[&2];
    let tip = &graph.commit(graph.head_branch().tip).unwrap().doc;
    assert_eq!(tip.colors, live.doc.colors);
    assert_eq!(tip.drawing_guides, live.doc.drawing_guides);

    // A current recipient page wins even when its current aids are default
    // and its graph snapshot happens to contain older, nondefault values.
    let mut recipient = ProjectPage {
        meta,
        doc: Document::new(2, 2),
        graph: Graph::new(Document::new(2, 2), "Recipient"),
    };
    recipient
        .graph
        .set_retired_live_aids(vec![[9, 9, 9]], guides);
    let before = recipient.graph.head_branch();
    let mut board = Storyboard::new(crate::storyboard::Settings::new(2, 2), &[2]);
    import_versions(
        &mut board,
        std::slice::from_mut(&mut recipient),
        &theirs,
        &live_sources,
    )
    .unwrap();
    assert!(recipient.doc.colors.is_empty());
    assert_eq!(recipient.doc.drawing_guides, DrawingGuides::default());
    assert_eq!(recipient.graph.head_branch(), before);
    assert!(board.versions.retired.is_empty());
    for commit in recipient.graph.commits().filter(|c| c.id != before.tip) {
        assert!(commit.doc.colors.is_empty());
        assert_eq!(commit.doc.drawing_guides, DrawingGuides::default());
    }
}

fn merge(
    base: &Project,
    ours: &ProjectEditor,
    theirs: &ProjectEditor,
    choices: &[(ConflictKey, Resolution)],
) -> BoardMerge {
    merge_boards(
        base,
        &ours.snapshot().unwrap(),
        &theirs.snapshot().unwrap(),
        &choices.iter().cloned().collect(),
    )
    .unwrap()
}

#[test]
fn changes_on_either_side_both_arrive_without_conflict() {
    let base = base();
    let p = ids(&base);
    let (mut ours, mut theirs) = (open(&base), open(&base));
    caption(&mut ours, p[0], "Action", "Mine");
    ours.edit_storyboard(|b| {
        b.panels.get_mut(&p[1]).unwrap().frames = 36;
        Ok(())
    })
    .unwrap();
    // The same panel changes in different aspects on the other side.
    caption(&mut theirs, p[0], "Dialogue", "Theirs");
    paint(&mut theirs, p[1], "Their line");
    paint(&mut theirs, p[3], "Their background");

    let merged = merge(&base, &ours, &theirs, &[]);
    assert!(
        merged.report.conflicts.is_empty(),
        "{:?}",
        merged.report.conflicts
    );
    let m = &merged.project;
    assert_eq!(ids(m), p);
    assert_eq!(text(m, p[0], "Action"), "Mine");
    assert_eq!(text(m, p[0], "Dialogue"), "Theirs");
    assert_eq!(m.storyboard.as_ref().unwrap().panels[&p[1]].frames, 36);
    assert!(has_layer(m, p[1], "Their line"));
    assert!(has_layer(m, p[3], "Their background"));
    assert_eq!(merged.report.took_theirs, 3);
    // Their side's summary uses change tracking's classification.
    let changed: Vec<_> = merged
        .report
        .theirs
        .iter()
        .filter(|c| c.is_change())
        .map(|c| (c.panel(), c.kind))
        .collect();
    assert_eq!(
        changed,
        [
            (p[0], crate::storyboard_changes::ChangeKind::Changed),
            (p[1], crate::storyboard_changes::ChangeKind::Changed),
            (p[3], crate::storyboard_changes::ChangeKind::Changed),
        ]
    );
}

#[test]
fn a_caption_both_sides_changed_conflicts_and_resolves_three_ways() {
    let base = base();
    let p = ids(&base);
    let (mut ours, mut theirs) = (open(&base), open(&base));
    caption(&mut ours, p[1], "Action", "Mine");
    caption(&mut theirs, p[1], "Action", "Theirs");
    // A different field of the same panel merges cleanly anyway.
    caption(&mut theirs, p[1], "Notes", "Their note");

    let merged = merge(&base, &ours, &theirs, &[]);
    let c = &merged.report.conflicts;
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].key, ConflictKey::Panel(p[1]));
    assert_eq!(c[0].aspects, [Aspect::Caption]);
    assert!(c[0].keep_both);
    assert_eq!(c[0].default, Resolution::Mine);
    assert_eq!(text(&merged.project, p[1], "Action"), "Mine");
    assert_eq!(text(&merged.project, p[1], "Notes"), "Their note");

    let key = ConflictKey::Panel(p[1]);
    let theirs_wins = merge(&base, &ours, &theirs, &[(key.clone(), Resolution::Theirs)]);
    assert_eq!(text(&theirs_wins.project, p[1], "Action"), "Theirs");
    assert_eq!(theirs_wins.report.conflicts[0].chosen, Resolution::Theirs);

    let both = merge(&base, &ours, &theirs, &[(key, Resolution::Both)]);
    let m = &both.project;
    assert_eq!(m.pages.len(), 5);
    let copy = m.pages[2].meta.id;
    assert!(!p.contains(&copy), "the copy is a new panel");
    assert_eq!(names(m)[1..3], ["Panel 2", "Panel 2 (theirs)"]);
    assert_eq!(text(m, p[1], "Action"), "Mine");
    assert_eq!(text(m, copy, "Action"), "Theirs");
    assert!(m.next_page_id > copy);

    // Choices must name a conflict, and Keep both is for panels.
    let none = BTreeMap::from([(ConflictKey::Panel(p[0]), Resolution::Theirs)]);
    assert!(
        merge_boards(
            &base,
            &ours.snapshot().unwrap(),
            &theirs.snapshot().unwrap(),
            &none
        )
        .is_err()
    );
}

#[test]
fn panels_added_on_both_sides_are_kept_beside_their_neighbours() {
    let base = base();
    let p = ids(&base);
    let (mut ours, mut theirs) = (open(&base), open(&base));
    let blank = base.storyboard.as_ref().unwrap().blank_panel().unwrap();
    // Both allocate the same next page ID for different panels.
    let mine = ours
        .insert_panels(
            Some(p[0]),
            &blank,
            vec![("Mine new".into(), Panel::new(0, 12))],
            None,
        )
        .unwrap()[0];
    let yours = theirs
        .insert_panels(
            Some(p[2]),
            &blank,
            vec![
                ("Theirs new A".into(), Panel::new(0, 12)),
                ("Theirs new B".into(), Panel::new(0, 12)),
            ],
            None,
        )
        .unwrap();
    assert_eq!(mine, yours[0]);

    let merged = merge(&base, &ours, &theirs, &[]);
    assert!(merged.report.conflicts.is_empty());
    // The second has an ID this copy never used.
    assert_eq!(merged.report.renumbered, 1);
    let m = &merged.project;
    assert_eq!(
        names(m),
        [
            "Page 1",
            "Mine new",
            "Panel 2",
            "Panel 3",
            "Theirs new A",
            "Theirs new B",
            "Panel 4"
        ]
    );
    let all = ids(m);
    let unique: HashSet<_> = all.iter().collect();
    assert_eq!(unique.len(), all.len());
    assert_eq!(all[1], mine);
    // Their panels joined scene B, like the panel before them.
    let board = m.storyboard.as_ref().unwrap();
    assert_eq!(board.panels[&all[4]].scene, board.panels[&p[2]].scene);
    m.validate().unwrap();
}

#[test]
fn deletions_apply_unless_the_other_side_changed_the_panel() {
    let base = base();
    let p = ids(&base);
    let (mut ours, mut theirs) = (open(&base), open(&base));
    // Theirs deletes an untouched panel; deleting a changed one conflicts.
    theirs.remove_pages(&[p[1]]).unwrap();
    caption(&mut ours, p[3], "Action", "Kept work");
    theirs.remove_pages(&[p[3]]).unwrap();
    // Mine deletes a panel theirs changed.
    ours.remove_pages(&[p[2]]).unwrap();
    caption(&mut theirs, p[2], "Action", "Their work");

    let merged = merge(&base, &ours, &theirs, &[]);
    let kinds: Vec<_> = merged
        .report
        .conflicts
        .iter()
        .map(|c| (c.key.clone(), c.detail.as_str(), c.keep_both))
        .collect();
    assert_eq!(
        kinds,
        [
            (
                ConflictKey::Panel(p[2]),
                "Deleted here, changed in theirs",
                false
            ),
            (
                ConflictKey::Panel(p[3]),
                "Changed here, deleted in theirs",
                false
            ),
        ]
    );
    // Default keeps mine: panel 3 stays deleted, panel 4 stays changed.
    assert_eq!(ids(&merged.project), [p[0], p[3]]);
    assert_eq!(text(&merged.project, p[3], "Action"), "Kept work");

    let theirs_way = merge(
        &base,
        &ours,
        &theirs,
        &[
            (ConflictKey::Panel(p[2]), Resolution::Theirs),
            (ConflictKey::Panel(p[3]), Resolution::Theirs),
        ],
    );
    assert_eq!(ids(&theirs_way.project), [p[0], p[2]]);
    assert_eq!(text(&theirs_way.project, p[2], "Action"), "Their work");
}

#[test]
fn one_sided_reorders_win_and_two_sided_reorders_are_a_choice() {
    let base = base();
    let p = ids(&base);
    let (mut ours, mut theirs) = (open(&base), open(&base));
    theirs.move_pages(&[p[3]], 0).unwrap();
    let blank = base.storyboard.as_ref().unwrap().blank_panel().unwrap();
    let mine = ours
        .insert_panels(
            Some(p[1]),
            &blank,
            vec![("Mine new".into(), Panel::new(0, 12))],
            None,
        )
        .unwrap()[0];
    let merged = merge(&base, &ours, &theirs, &[]);
    assert!(merged.report.conflicts.is_empty());
    assert_eq!(ids(&merged.project), [p[3], p[0], p[1], mine, p[2]]);

    ours.move_pages(&[p[0]], 4).unwrap();
    let merged = merge(&base, &ours, &theirs, &[]);
    assert_eq!(merged.report.conflicts[0].key, ConflictKey::Order);
    assert_eq!(ids(&merged.project), layout(&ours));
    let merged = merge(
        &base,
        &ours,
        &theirs,
        &[(ConflictKey::Order, Resolution::Theirs)],
    );
    assert_eq!(ids(&merged.project), [p[3], p[0], p[1], mine, p[2]]);
}

#[test]
fn board_data_merges_part_by_part() {
    let base = base();
    let p = ids(&base);
    let (mut ours, mut theirs) = (open(&base), open(&base));
    let b = base.storyboard.as_ref().unwrap();
    let (scene_a, scene_b) = (b.panels[&p[0]].scene, b.panels[&p[2]].scene);
    ours.edit_storyboard(|b| {
        b.rename(scene_a, "Opening")?;
        // A new sound and clip here …
        let thunder = b.timeline.add_asset(sound("Thunder"))?;
        b.timeline.place(0, clip(thunder, 30, 10))?;
        b.add_review_note(p[0], "Maya", "Bigger", 10).map(|_| ())
    })
    .unwrap();
    theirs
        .edit_storyboard(|b| {
            let rest = b.rest_camera();
            b.cameras.insert(
                scene_b,
                SceneCamera {
                    keys: vec![
                        CameraKey::at(0, rest),
                        CameraKey::at(40, CameraState { zoom: 2., ..rest }),
                    ],
                    shake: None,
                },
            );
            // … and another there, which got the same asset ID.
            let birds = b.timeline.add_asset(sound("Birds"))?;
            b.timeline.place(0, clip(birds, 60, 10))?;
            b.add_review_note(p[0], "Ravi", "Darker", 20).map(|_| ())?;
            b.claim_scenes(&[scene_b], "Ravi", "dev-b", 30)
        })
        .unwrap();
    ours.create_board_version("Mine v1").unwrap();
    theirs.create_board_version("Theirs v1").unwrap();

    let merged = merge(&base, &ours, &theirs, &[]);
    assert!(
        merged.report.conflicts.is_empty(),
        "{:?}",
        merged.report.conflicts
    );
    let m = merged.project.storyboard.as_ref().unwrap();
    assert_eq!(m.scenes[&scene_a].name, "Opening");
    assert!(m.cameras.contains_key(&scene_b));
    let clips: Vec<_> = m.timeline.tracks[0]
        .clips
        .iter()
        .map(|c| (m.timeline.assets[&c.asset].name.as_str(), c.start))
        .collect();
    assert_eq!(clips, [("Rain", 0), ("Thunder", 30), ("Birds", 60)]);
    let notes: Vec<_> = m.panels[&p[0]]
        .review
        .notes
        .iter()
        .map(|n| (n.author.as_str(), n.id))
        .collect();
    assert_eq!(notes, [("Maya", 1), ("Ravi", 2)]);
    assert_eq!(m.claim(scene_b).unwrap().claimant, "Ravi");
    let versions: Vec<_> = m.versions.list.iter().map(|v| v.name.as_str()).collect();
    assert!(versions.contains(&"Mine v1") && versions.contains(&"Theirs v1"));
    assert_eq!(merged.report.versions_added, 1);
    // Their version reads back with its drawings.
    let mut e = open(&merged.project);
    let id = e
        .board_versions()
        .iter()
        .find(|v| v.name == "Theirs v1")
        .unwrap()
        .id;
    let state = e
        .board_state(crate::storyboard_versions::Baseline::Version(id))
        .unwrap();
    assert_eq!(state.docs.len(), 4);
    assert!(e.undo() || !e.can_undo());

    // Overlapping clips on one track cannot both stay.
    let (mut ours, mut theirs) = (open(&base), open(&base));
    for (side, start) in [(&mut ours, 30), (&mut theirs, 34)] {
        side.edit_storyboard(|b| b.timeline.place(0, clip(1, start, 10)))
            .unwrap();
    }
    let merged = merge(&base, &ours, &theirs, &[]);
    assert_eq!(
        merged.report.conflicts[0].key,
        ConflictKey::AudioTrack("FX".into())
    );
    let starts = |m: &BoardMerge| -> Vec<u64> {
        m.project.storyboard.as_ref().unwrap().timeline.tracks[0]
            .clips
            .iter()
            .map(|c| c.start)
            .collect()
    };
    assert_eq!(starts(&merged), [0, 30]);
    let merged = merge(
        &base,
        &ours,
        &theirs,
        &[(ConflictKey::AudioTrack("FX".into()), Resolution::Theirs)],
    );
    assert_eq!(starts(&merged), [0, 34]);

    // Renaming one scene two ways is a group conflict.
    let (mut ours, mut theirs) = (open(&base), open(&base));
    ours.edit_storyboard(|b| b.rename(scene_b, "Chase"))
        .unwrap();
    theirs
        .edit_storyboard(|b| b.rename(scene_b, "Escape"))
        .unwrap();
    let merged = merge(
        &base,
        &ours,
        &theirs,
        &[(ConflictKey::Group(scene_b), Resolution::Theirs)],
    );
    assert_eq!(merged.report.conflicts[0].key, ConflictKey::Group(scene_b));
    assert_eq!(
        merged.project.storyboard.as_ref().unwrap().scenes[&scene_b].name,
        "Escape"
    );
}

#[test]
fn merging_is_deterministic() {
    let base = base();
    let p = ids(&base);
    let (mut ours, mut theirs) = (open(&base), open(&base));
    let blank = base.storyboard.as_ref().unwrap().blank_panel().unwrap();
    ours.insert_panels(
        Some(p[3]),
        &blank,
        vec![("A".into(), Panel::new(0, 5))],
        None,
    )
    .unwrap();
    theirs
        .insert_panels(
            Some(p[3]),
            &blank,
            vec![("B".into(), Panel::new(0, 6))],
            None,
        )
        .unwrap();
    caption(&mut ours, p[0], "Action", "x");
    caption(&mut theirs, p[0], "Action", "y");
    let one = merge(&base, &ours, &theirs, &[]);
    let two = merge(&base, &ours, &theirs, &[]);
    assert_eq!(ids(&one.project), ids(&two.project));
    assert_eq!(names(&one.project), names(&two.project));
    assert_eq!(one.project.storyboard, two.project.storyboard);
    assert_eq!(one.report, two.report);
    assert_eq!(names(&one.project)[4..], ["A", "B"]);
}

#[test]
fn applying_a_merge_is_one_undo_step() {
    let base = base();
    let p = ids(&base);
    let (mut ours, mut theirs) = (open(&base), open(&base));
    paint(&mut ours, p[0], "Mine");
    caption(&mut ours, p[1], "Action", "Mine");
    paint(&mut theirs, p[1], "Theirs");
    paint(&mut theirs, p[2], "Theirs");
    theirs.remove_pages(&[p[3]]).unwrap();
    let blank = base.storyboard.as_ref().unwrap().blank_panel().unwrap();
    theirs
        .insert_panels(
            Some(p[0]),
            &blank,
            vec![("New".into(), Panel::new(0, 8))],
            None,
        )
        .unwrap();
    let theirs = theirs.snapshot().unwrap();
    let revision = "0a0a0a0a-0b0b-4c0c-8d0d-0e0e0e0e0e0e";

    let before = ours.stamp();
    let report = ours
        .merge_board(&base, &theirs, &BTreeMap::new(), Some(revision))
        .unwrap();
    assert!(report.conflicts.is_empty());
    let merged = ours.stamp();
    let after = ours.snapshot().unwrap();
    assert_eq!(names(&after), ["Page 1", "New", "Panel 2", "Panel 3"]);
    assert!(has_layer(&after, p[0], "Mine"));
    assert!(has_layer(&after, p[1], "Theirs"));
    assert!(has_layer(&after, p[2], "Theirs"));
    assert_eq!(text(&after, p[1], "Action"), "Mine");
    assert_eq!(
        after
            .storyboard
            .as_ref()
            .unwrap()
            .sharing
            .merged_revision
            .as_deref(),
        Some(revision)
    );
    assert!(ours.is_modified());

    assert!(ours.undo());
    assert_eq!(
        ours.stamp(),
        before,
        "one Undo puts layout, board and drawings back"
    );
    let undone = ours.snapshot().unwrap();
    assert!(!has_layer(&undone, p[1], "Theirs"));
    assert!(has_layer(&undone, p[0], "Mine"), "earlier edits stay");
    assert!(ours.redo());
    assert_eq!(ours.stamp(), merged);
    assert!(has_layer(&ours.snapshot().unwrap(), p[2], "Theirs"));
    // Undo again, then the earlier edits undo normally.
    assert!(ours.undo());
    assert!(ours.undo());
    assert!(text(&ours.snapshot().unwrap(), p[1], "Action").is_empty());
}

#[test]
fn locked_panels_are_not_changed_by_a_merge() {
    let base = base();
    let p = ids(&base);
    let (mut ours, mut theirs) = (open(&base), open(&base));
    ours.edit_storyboard(|b| {
        b.panels.get_mut(&p[0]).unwrap().locked = true;
        Ok(())
    })
    .unwrap();
    paint(&mut theirs, p[0], "Theirs");
    let stamp = ours.stamp();
    let error = ours
        .merge_board(&base, &theirs.snapshot().unwrap(), &BTreeMap::new(), None)
        .unwrap_err();
    assert!(error.contains("locked"), "{error}");
    assert_eq!(ours.stamp(), stamp);
}

#[test]
fn shared_primitives_follow_the_three_way_rule() {
    assert_eq!(pick(Some(&1), Some(&1), Some(&2)), Pick::Theirs);
    assert_eq!(pick(Some(&1), Some(&3), Some(&1)), Pick::Ours);
    assert_eq!(pick(Some(&1), Some(&3), Some(&3)), Pick::Ours);
    assert_eq!(pick(Some(&1), Some(&3), Some(&2)), Pick::Conflict);
    assert_eq!(pick(None, None, Some(&2)), Pick::Theirs);
    assert_eq!(edit(Some(&1), None), Edit::Deleted);
    assert_eq!(edit::<i32>(None, None), Edit::Absent);
    assert_eq!(
        merge_order(&[1, 2, 3], &[1, 9, 2, 8], |_| true),
        [1, 9, 2, 3, 8]
    );
    assert_eq!(merge_order(&[1, 5, 2], &[1, 6, 2], |_| true), [1, 5, 6, 2]);
    assert_eq!(merge_order(&[1, 2, 3], &[7, 1], |i| i != 2), [7, 1, 3]);
    assert!(reordered(&[1, 2, 3], &[2, 1, 3], &[1, 2]));
    assert!(!reordered(&[1, 2, 3], &[1, 5, 3], &[1, 3]));
    for key in [
        ConflictKey::Panel(4),
        ConflictKey::Order,
        ConflictKey::AudioTrack("FX: hits".into()),
        ConflictKey::Board("settings".into()),
    ] {
        assert_eq!(ConflictKey::parse(&key.key()), Some(key));
    }
    assert_eq!(ConflictKey::parse("nope:1"), None);
}
