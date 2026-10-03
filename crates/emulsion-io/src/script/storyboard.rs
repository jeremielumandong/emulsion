//! A script laid out as storyboard panels: one scene per scene heading, one
//! panel per beat (an action paragraph or a dialogue block) or per scene,
//! captions in the Action, Dialogue and Slugging fields, first timings from
//! the words, and screenplay transitions as panel transitions. The result is
//! a panel clip, so pasting it is the ordinary one-step paste.
use super::{Beat, Script, dialogue_line, estimated_seconds};
use anyhow::Result;
use emulsion_core::project::PanelClip;
use emulsion_core::storyboard::{FrameRate, Storyboard, Transition, TransitionKind};
use emulsion_core::storyboard_breakdown::{ClipBuilder, SLUGGING};
use emulsion_core::timeline::Edge;

/// How a script is cut into panels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Split {
    /// One panel per action paragraph or dialogue block.
    #[default]
    Beat,
    /// One panel per scene, holding all of its action and dialogue.
    Scene,
}

/// The transition a screenplay transition line asks for, if any.
fn transition(line: &str, rate: FrameRate) -> Option<Transition> {
    let upper = line.to_uppercase();
    let frames = |seconds: f64| rate.seconds_to_frames(seconds).max(1) as u32;
    let kind = if upper.contains("DISSOLVE") {
        (TransitionKind::Dissolve, frames(0.5))
    } else if upper.contains("FADE") {
        (TransitionKind::FadeToColor { color: [0, 0, 0] }, frames(1.))
    } else if upper.contains("WIPE") {
        (TransitionKind::Wipe { from: Edge::Left }, frames(0.5))
    } else {
        return None;
    };
    Some(Transition {
        kind: kind.0,
        frames: kind.1,
    })
}

/// `script` as panels for `board` (its frame rate, naming, default panel
/// length and blank panel).
pub fn panels(script: &Script, board: &Storyboard, split: Split) -> Result<PanelClip> {
    let rate = board.settings.frame_rate;
    let minimum = rate.frames_to_seconds(u64::from(board.settings.panel_frames));
    let mut clip = ClipBuilder::new(board).map_err(anyhow::Error::msg)?;
    // A transition line applies to the next panel, often across a scene.
    let mut pending: Option<Transition> = None;
    for scene in &script.scenes {
        clip.scene(&scene.heading);
        let (mut action, mut dialogue, mut seconds) = (Vec::new(), Vec::new(), 0.);
        for beat in &scene.beats {
            match beat {
                Beat::Transition(line) => {
                    if let Some(t) = transition(line, rate) {
                        pending = Some(t);
                    }
                    continue;
                }
                Beat::Action(text) => action.push(text.clone()),
                Beat::Dialogue {
                    character,
                    parenthetical,
                    text,
                } => dialogue.push(dialogue_line(character, parenthetical, text)),
            }
            seconds += estimated_seconds(beat, minimum);
            if split == Split::Beat {
                let text = (std::mem::take(&mut action), std::mem::take(&mut dialogue));
                push(
                    &mut clip,
                    text,
                    seconds,
                    &scene.heading,
                    pending.take(),
                    rate,
                )?;
                seconds = 0.;
            }
        }
        // A whole scene per panel, or a scene with nothing to show yet.
        if split == Split::Scene || clip.scene_panels() == 0 {
            let seconds = seconds.max(minimum);
            push(
                &mut clip,
                (action, dialogue),
                seconds,
                &scene.heading,
                pending.take(),
                rate,
            )?;
        }
    }
    clip.finish()
        .map_err(|_| anyhow::anyhow!("The script has no scenes, action or dialogue."))
}

/// Add a panel showing `(action, dialogue)` lines to the current scene; the
/// scene's first panel carries the heading.
fn push(
    clip: &mut ClipBuilder,
    (action, dialogue): (Vec<String>, Vec<String>),
    seconds: f64,
    heading: &str,
    transition: Option<Transition>,
    rate: FrameRate,
) -> Result<()> {
    let heading = if clip.scene_panels() == 0 {
        heading
    } else {
        ""
    };
    let (action, dialogue) = (action.join("\n"), dialogue.join("\n"));
    let captions = [
        ("Action", action.as_str()),
        ("Dialogue", dialogue.as_str()),
        (SLUGGING, heading),
    ];
    let frames = rate.seconds_to_frames(seconds) as u32;
    let panel = clip.panel(frames, &captions).map_err(anyhow::Error::msg)?;
    if let Some(t) = transition {
        panel.transition = Transition {
            frames: t.frames.min(panel.frames),
            ..t
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::Document;
    use emulsion_core::project::{ProjectEditor, ProjectKind};

    const SCRIPT: &str = "INT. KITCHEN - NIGHT

Rain on the window.

MIA
(quietly)
Is anyone there?

DISSOLVE TO:

EXT. GARDEN - DAWN

Birds.
";

    fn editor() -> ProjectEditor {
        ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap()
    }

    #[test]
    fn beats_become_panels_with_captions_timing_and_transitions() {
        let script = crate::script::parse_fountain(SCRIPT);
        let mut p = editor();
        let clip = panels(&script, p.storyboard().unwrap(), Split::Beat).unwrap();
        assert_eq!(clip.scenes, ["INT. KITCHEN - NIGHT", "EXT. GARDEN - DAWN"]);
        assert_eq!(clip.panels.len(), 3);
        let ids = p.paste_panels(Some(1), &clip).unwrap();
        let board = p.storyboard().unwrap();
        let caption = |id, field: &str| {
            board.panels[&id]
                .captions
                .get(&board.caption(field).unwrap())
                .map(|c| c.text.clone())
        };
        assert_eq!(
            caption(ids[0], "Slugging").as_deref(),
            Some("INT. KITCHEN - NIGHT")
        );
        assert_eq!(
            caption(ids[0], "Action").as_deref(),
            Some("Rain on the window.")
        );
        assert_eq!(
            caption(ids[1], "Dialogue").as_deref(),
            Some("MIA (quietly): Is anyone there?")
        );
        assert_eq!(caption(ids[1], "Slugging"), None);
        // The dissolve before the garden scene enters its first panel.
        assert_eq!(
            board.panels[&ids[2]].transition.kind,
            TransitionKind::Dissolve
        );
        assert_eq!(
            board.scenes[&board.panels[&ids[2]].scene].name,
            "EXT. GARDEN - DAWN"
        );
        // Pasting is one step.
        assert!(p.undo());
        assert_eq!(p.page_list().len(), 1);
    }

    #[test]
    fn whole_scenes_make_one_panel_each() {
        let script = crate::script::parse_fountain(SCRIPT);
        let p = editor();
        let clip = panels(&script, p.storyboard().unwrap(), Split::Scene).unwrap();
        assert_eq!(clip.panels.len(), 2);
        let first = &clip.panels[0].panel;
        assert_eq!(first.captions.len(), 3);
        // Longer than either beat alone.
        assert!(first.frames > p.storyboard().unwrap().settings.panel_frames);
        assert!(panels(&Script::default(), p.storyboard().unwrap(), Split::Beat).is_err());
    }

    #[test]
    fn imported_timings_match_the_caption_estimate() {
        // One word-rate model: re-estimating imported panels from their
        // captions gives the durations the import chose.
        let script = crate::script::parse_fountain(
            "INT. HALL - DAY\n\nMia walks the long corridor, counting the doors under her breath.\n\nMIA\n(whispering)\nSeven. Eight. Nine. Where is the tenth one?\n\nTOM\nRight behind you.\n",
        );
        let mut p = editor();
        let clip = panels(&script, p.storyboard().unwrap(), Split::Beat).unwrap();
        let ids = p.paste_panels(Some(1), &clip).unwrap();
        let board = p.storyboard().unwrap();
        let layout: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
        let rates = emulsion_core::storyboard_estimate::WordRates {
            minimum_seconds: board
                .settings
                .frame_rate
                .frames_to_seconds(u64::from(board.settings.panel_frames)),
            ..Default::default()
        };
        let found = emulsion_core::storyboard_estimate::estimate(board, &layout, &ids, &rates);
        assert_eq!(found.panels.len(), 3);
        for p in &found.panels {
            assert_eq!(p.estimate, Some(p.old_frames), "panel {}", p.panel);
        }
        assert!(found.changes().0.is_empty());
    }
}
