//! A script laid out as storyboard panels: one scene per scene heading, one
//! panel per beat (an action paragraph or a dialogue block) or per scene,
//! captions in the Action, Dialogue and Slugging fields, first timings from
//! the words, and screenplay transitions as panel transitions. The result is
//! a panel clip, so pasting it is the ordinary one-step paste.
use super::{Beat, Script, estimated_seconds};
use anyhow::{Result, bail};
use emulsion_core::project::{ClipPanel, MAX_PAGES, PanelClip};
use emulsion_core::storyboard::{
    CaptionField, FrameRate, MAX_PANEL_FRAMES, Panel, Storyboard, Transition, TransitionKind,
};
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

const ACTION: u64 = 1;
const DIALOGUE: u64 = 2;
const SLUGGING: u64 = 3;

fn fields() -> Vec<CaptionField> {
    [
        (ACTION, "Action"),
        (DIALOGUE, "Dialogue"),
        (SLUGGING, "Slugging"),
    ]
    .into_iter()
    .map(|(id, name)| CaptionField {
        id,
        name: name.into(),
        multiline: id != SLUGGING,
        print: true,
    })
    .collect()
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

fn dialogue_line(character: &str, parenthetical: &Option<String>, text: &str) -> String {
    match parenthetical {
        Some(p) => format!("{character} {p}: {text}"),
        None => format!("{character}: {text}"),
    }
}

/// `script` as panels for `board` (its frame rate, naming, default panel
/// length and blank panel).
pub fn panels(script: &Script, board: &Storyboard, split: Split) -> Result<PanelClip> {
    let rate = board.settings.frame_rate;
    let minimum = rate.frames_to_seconds(u64::from(board.settings.panel_frames));
    let blank = board.blank_panel().map_err(anyhow::Error::msg)?;
    let mut clip = PanelClip {
        frame_rate: rate,
        fields: fields(),
        scenes: Vec::new(),
        whole_scenes: true,
        panels: Vec::new(),
    };
    // A transition line applies to the next panel, often across a scene.
    let mut pending: Option<Transition> = None;
    for scene in &script.scenes {
        let index = clip.scenes.len() as u64;
        clip.scenes.push(if scene.heading.is_empty() {
            board.naming.scene_name(clip.scenes.len())
        } else {
            scene.heading.chars().take(200).collect()
        });
        let before = clip.panels.len();
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
                let beat = PanelText {
                    action: std::mem::take(&mut action),
                    dialogue: std::mem::take(&mut dialogue),
                    seconds,
                };
                push(
                    &mut clip,
                    index,
                    &blank,
                    board,
                    beat,
                    &scene.heading,
                    pending.take(),
                    rate,
                );
                seconds = 0.;
            }
        }
        // A whole scene per panel, or a scene with nothing to show yet.
        if split == Split::Scene || clip.panels.len() == before {
            let beat = PanelText {
                action,
                dialogue,
                seconds: seconds.max(minimum),
            };
            push(
                &mut clip,
                index,
                &blank,
                board,
                beat,
                &scene.heading,
                pending.take(),
                rate,
            );
        }
        if clip.panels.len() > MAX_PAGES {
            bail!("The script makes more than {MAX_PAGES} panels; split it per scene.");
        }
    }
    if clip.panels.is_empty() {
        bail!("The script has no scenes, action or dialogue.");
    }
    Ok(clip)
}

/// What one panel shows.
struct PanelText {
    action: Vec<String>,
    dialogue: Vec<String>,
    seconds: f64,
}

#[allow(clippy::too_many_arguments)]
fn push(
    clip: &mut PanelClip,
    scene: u64,
    blank: &emulsion_core::Document,
    board: &Storyboard,
    beat: PanelText,
    heading: &str,
    transition: Option<Transition>,
    rate: FrameRate,
) {
    let number = clip
        .panels
        .iter()
        .filter(|p| p.panel.scene == scene)
        .count()
        + 1;
    let frames = (rate.seconds_to_frames(beat.seconds) as u32).clamp(1, MAX_PANEL_FRAMES);
    let mut panel = Panel::new(scene, frames);
    let mut caption = |id: u64, lines: Vec<String>| {
        let text: String = lines
            .join("\n")
            .chars()
            .take(emulsion_core::storyboard::MAX_CAPTION_CHARS)
            .collect();
        if !text.is_empty() {
            panel.captions.insert(id, text.into());
        }
    };
    caption(ACTION, beat.action);
    caption(DIALOGUE, beat.dialogue);
    if number == 1 && !heading.is_empty() {
        caption(SLUGGING, vec![heading.chars().take(200).collect()]);
    }
    if let Some(t) = transition {
        panel.transition = Transition {
            frames: t.frames.min(frames),
            ..t
        };
    }
    clip.panels.push(ClipPanel {
        name: board.naming.panel_name(number),
        doc: blank.clone(),
        panel,
    });
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
}
