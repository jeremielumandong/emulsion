//! Conforming a storyboard to an edit: durations, order, transitions and
//! sound from editing software, as one project Undo step (see
//! `crate::storyboard_conform`).
use super::ProjectEditor;
use crate::storyboard_conform::{ConformReport, RateChoice, plan};
use crate::timeline::Edit;
use std::sync::Arc;

impl ProjectEditor {
    /// Plan conforming the storyboard to `edit` and, with `apply`, apply it
    /// as one Undo step. Returns the report either way; a dry run changes
    /// nothing.
    pub fn conform_storyboard(
        &mut self,
        edit: &Edit,
        choice: RateChoice,
        apply: bool,
    ) -> Result<ConformReport, String> {
        let board = self.board()?.clone();
        let layout: Vec<_> = self.layout.iter().map(|m| (m.id, m.name.clone())).collect();
        let plan = plan(&board, &layout, edit, choice)?;
        plan.board.validate(&plan.order)?;
        board.check_locks_kept(&plan.timed)?;
        board.check_locks_kept(&plan.board)?;
        if !apply {
            return Ok(plan.report);
        }
        let order: Vec<_> = layout.iter().map(|(id, _)| *id).collect();
        if plan.order == order && plan.board == *board {
            return Ok(plan.report);
        }
        let size = (plan.board.settings.width, plan.board.settings.height);
        if self
            .layout
            .iter()
            .any(|m| (self.pages[&m.id].doc.width, self.pages[&m.id].doc.height) != size)
        {
            return Err("Storyboard resolution must match every panel.".into());
        }
        let mut layout = Vec::with_capacity(self.layout.len());
        for id in &plan.order {
            let meta = self
                .layout
                .iter()
                .find(|m| m.id == *id)
                .ok_or("Panel does not exist.")?;
            layout.push(meta.clone());
        }
        self.record_pages()?;
        self.layout = layout;
        self.storyboard = Some(Arc::new(plan.board));
        self.collect_pages();
        self.refresh_locks();
        Ok(plan.report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;
    use crate::project::{PageId, ProjectKind};
    use crate::storyboard::{Level, Panel, TransitionKind};
    use crate::timeline::{AudioAsset, AudioClip, AudioTrack, EditClip, EditTransition, FrameRate};

    /// Four panels named A–D of 24 frames: A, B in scene 1; C, D in scene
    /// 2; one sound "Rain" on a track.
    fn board() -> (ProjectEditor, Vec<PageId>) {
        let mut editor =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        let blank = editor.storyboard().unwrap().blank_panel().unwrap();
        let first = editor.active_page();
        editor.rename_page(first, "A".into(), 0.).unwrap();
        let mut ids = vec![first];
        ids.extend(
            editor
                .insert_panels(
                    Some(first),
                    &blank,
                    ["B", "C", "D"]
                        .iter()
                        .map(|n| (n.to_string(), Panel::new(0, 24)))
                        .collect(),
                    None,
                )
                .unwrap(),
        );
        editor
            .edit_storyboard(|b| {
                b.panels.get_mut(&first).unwrap().frames = 24;
                b.split(&ids, ids[2], Level::Scene, Some("2"))?;
                let asset = b.timeline.add_asset(AudioAsset {
                    name: "Rain".into(),
                    format: "wav".into(),
                    duration_ms: 10_000,
                    sample_rate: 48_000,
                    channels: 2,
                    folder: String::new(),
                    source: None,
                })?;
                b.timeline.tracks.push(AudioTrack::new("Sound"));
                b.timeline.place(
                    0,
                    AudioClip {
                        asset,
                        name: "Rain".into(),
                        start: 0,
                        frames: 48,
                        fade_in: 4,
                        ..AudioClip::default()
                    },
                )
            })
            .unwrap();
        (editor, ids)
    }

    fn clip(name: &str, record_in: u64, frames: u64) -> EditClip {
        EditClip {
            name: name.into(),
            source_out: frames,
            record_in,
            record_out: record_in + frames,
            ..EditClip::default()
        }
    }

    fn names(editor: &ProjectEditor) -> Vec<String> {
        editor.page_list().iter().map(|m| m.name.clone()).collect()
    }

    #[test]
    fn reordered_retimed_and_missing_clips_conform_in_one_undo_step() {
        let (mut editor, ids) = board();
        let mut edit = Edit::new("Cut", FrameRate::whole(24));
        // B and A swap, C is dropped, D is longer with a dissolve; an extra
        // clip has no panel. A file named after D's ID also matches.
        edit.video = vec![
            clip("B", 0, 12),
            clip("A", 12, 30),
            EditClip {
                media: "/m/Renamed_p".to_string() + &ids[3].to_string() + ".png",
                transition: Some(EditTransition {
                    kind: TransitionKind::Dissolve,
                    frames: 6,
                }),
                ..clip("Shot 9", 42, 40)
            },
            clip("Title", 82, 10),
        ];
        edit.audio = vec![EditClip {
            gain_db: Some(-6.),
            source_in: 24,
            ..clip("Rain", 12, 36)
        }];
        let before = editor.storyboard().unwrap().clone();
        let report = editor
            .conform_storyboard(&edit, RateChoice::Convert, false)
            .unwrap();
        assert_eq!(editor.storyboard().unwrap(), &before, "dry run");
        assert_eq!(report.matched, 3);
        assert_eq!(report.unmatched, ["Title at 00:00:03:10"]);
        assert_eq!(report.left_out.len(), 1);
        assert_eq!(report.left_out[0].name, "C");
        assert_eq!(report.moved.len(), 1);
        assert_eq!(report.transitions, 1);
        assert_eq!(report.sound_clips, 1);
        assert!(report.summary().contains("3 panels matched"));
        let applied = editor
            .conform_storyboard(&edit, RateChoice::Convert, true)
            .unwrap();
        assert_eq!(applied, report);
        assert_eq!(names(&editor), ["B", "A", "C", "D"]);
        let board = editor.storyboard().unwrap();
        let frames: Vec<_> = editor
            .page_list()
            .iter()
            .map(|m| board.panels[&m.id].frames)
            .collect();
        assert_eq!(frames, [12, 30, 24, 40]);
        assert_eq!(board.panels[&ids[3]].transition.frames, 6);
        // The moved panel joined the scene it landed in.
        assert_eq!(board.panels[&ids[0]].scene, board.panels[&ids[1]].scene);
        let sound = &board.timeline.tracks[0].clips[0];
        assert_eq!((sound.start, sound.frames, sound.offset_ms), (12, 36, 1000));
        assert_eq!(sound.gain_db, -6.);
        assert_eq!(sound.fade_in, 4, "effects of the reused clip stay");
        assert!(editor.undo());
        assert_eq!(names(&editor), ["A", "B", "C", "D"]);
        assert_eq!(editor.storyboard().unwrap(), &before);
    }

    #[test]
    fn other_rates_convert_times_or_keep_frames() {
        let (mut editor, _) = board();
        let mut edit = Edit::new("Cut", FrameRate::whole(30));
        edit.video = ["A", "B", "C", "D"]
            .iter()
            .enumerate()
            .map(|(i, n)| clip(n, i as u64 * 45, 45))
            .collect();
        let report = editor
            .conform_storyboard(&edit, RateChoice::Convert, true)
            .unwrap();
        assert!(report.rate_differs);
        let board = editor.storyboard().unwrap();
        // 1.5 s at 24 fps.
        assert!(board.panels.values().all(|p| p.frames == 36));
        editor
            .conform_storyboard(&edit, RateChoice::Keep, true)
            .unwrap();
        let board = editor.storyboard().unwrap();
        assert!(board.panels.values().all(|p| p.frames == 45));
        assert!(report.moved.is_empty());
    }

    #[test]
    fn an_edit_with_no_known_panel_is_refused_and_locked_moves_too() {
        let (mut editor, ids) = board();
        let mut edit = Edit::new("Cut", FrameRate::whole(24));
        edit.video = vec![clip("Other", 0, 10)];
        assert!(
            editor
                .conform_storyboard(&edit, RateChoice::Convert, true)
                .is_err()
        );
        editor
            .edit_storyboard(|b| {
                b.panels.get_mut(&ids[0]).unwrap().locked = true;
                Ok(())
            })
            .unwrap();
        edit.video = vec![clip("B", 0, 24), clip("C", 24, 24), clip("A", 48, 24)];
        let error = editor
            .conform_storyboard(&edit, RateChoice::Convert, false)
            .unwrap_err();
        assert!(error.contains("locked"), "{error}");
    }
}
