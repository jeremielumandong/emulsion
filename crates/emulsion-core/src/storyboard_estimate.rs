//! Panel durations estimated from words: the one word-rate model used for
//! script imports, breakdowns and Timing › Estimate durations. Dialogue is
//! spoken at `dialogue_wpm` with a pause after each line and each
//! parenthetical; action reads at `action_wpm`; a panel never lasts less than
//! `minimum_seconds`. Captions count by field name (Dialogue and Action by
//! default), and a scene lasts as long as its panels together.
use crate::project::PageId;
use crate::storyboard::{GroupId, MAX_PANEL_FRAMES, Panel, Storyboard};
use serde::{Deserialize, Serialize};

/// Seconds a screenplay transition line (CUT TO:) holds a panel on its own.
pub const TRANSITION_SECONDS: f64 = 1.;
/// Most caption field names one rule set counts.
pub const MAX_RATE_FIELDS: usize = 16;

/// How fast words play, and which caption fields hold them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WordRates {
    /// Spoken words per minute.
    pub dialogue_wpm: f64,
    /// Words per minute for action lines: the time the described action
    /// takes on screen.
    pub action_wpm: f64,
    /// Seconds of air after each line of dialogue, for the reaction.
    pub line_pause: f64,
    /// Seconds each parenthetical ("(beat)", "(quietly)") adds.
    pub parenthetical_pause: f64,
    /// The shortest panel an estimate gives.
    pub minimum_seconds: f64,
    /// Caption fields read as dialogue, by name (case-insensitive).
    pub dialogue_fields: Vec<String>,
    /// Caption fields read as action, by name (case-insensitive).
    pub action_fields: Vec<String>,
}

impl Default for WordRates {
    fn default() -> Self {
        Self {
            dialogue_wpm: 150.,
            action_wpm: 120.,
            line_pause: 0.5,
            parenthetical_pause: 0.5,
            minimum_seconds: 1.,
            dialogue_fields: vec!["Dialogue".into()],
            action_fields: vec!["Action".into()],
        }
    }
}

/// Words in `text`: whitespace-separated runs holding a letter or digit, so
/// dashes and ellipses do not count.
pub fn words(text: &str) -> usize {
    text.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count()
}

/// What a dialogue caption holds: lines, spoken words and parentheticals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DialogueCount {
    pub lines: usize,
    pub words: usize,
    pub parentheticals: usize,
}

/// `text` with every "(…)" removed, and how many there were.
pub(crate) fn strip_parentheticals(text: &str) -> (String, usize) {
    let (mut out, mut count, mut depth) = (String::new(), 0, 0usize);
    for c in text.chars() {
        match c {
            '(' => {
                if depth == 0 {
                    count += 1;
                }
                depth += 1;
                out.push(' ');
            }
            ')' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            _ => out.push(c),
        }
    }
    (out, count)
}

/// The speaker before a "NAME:" or "NAME (aside):" prefix, when the line has
/// one: up to four words starting with a capital, without sentence
/// punctuation ("MIA", "McCLANE (V.O.)", "Mr. Lee").
pub(crate) fn speaker(line: &str) -> Option<(&str, &str)> {
    let (name, rest) = line.split_once(':')?;
    let (bare, _) = strip_parentheticals(name);
    let bare = bare.trim();
    let named = bare.chars().next().is_some_and(char::is_uppercase)
        && bare.chars().count() <= 40
        && bare.split_whitespace().count() <= 4
        && !bare.contains([',', '!', '?', ';', '"']);
    named.then_some((name, rest))
}

/// Count a dialogue caption: one line per speaker prefix ("MIA: …"), or one
/// for text with none; a line without a prefix continues the one before.
/// Speaker names are not spoken; parentheticals are counted, not spoken.
pub fn dialogue(text: &str) -> DialogueCount {
    let mut count = DialogueCount::default();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let spoken = match speaker(line) {
            Some((name, rest)) => {
                count.lines += 1;
                count.parentheticals += strip_parentheticals(name).1;
                rest
            }
            None => {
                count.lines = count.lines.max(1);
                line
            }
        };
        let (spoken, asides) = strip_parentheticals(spoken);
        count.parentheticals += asides;
        count.words += words(&spoken);
    }
    count
}

impl WordRates {
    pub const WPM: std::ops::RangeInclusive<f64> = 20.0..=600.;
    pub const PAUSE: std::ops::RangeInclusive<f64> = 0.0..=10.;
    pub const MINIMUM: std::ops::RangeInclusive<f64> = 0.0..=60.;

    pub fn validate(&self) -> Result<(), String> {
        let within = |v: f64, r: &std::ops::RangeInclusive<f64>| v.is_finite() && r.contains(&v);
        if !within(self.dialogue_wpm, &Self::WPM) || !within(self.action_wpm, &Self::WPM) {
            return Err("Word rates are 20–600 words per minute.".into());
        }
        if !within(self.line_pause, &Self::PAUSE) || !within(self.parenthetical_pause, &Self::PAUSE)
        {
            return Err("Pauses are 0–10 seconds.".into());
        }
        if !within(self.minimum_seconds, &Self::MINIMUM) {
            return Err("The minimum panel duration is 0–60 seconds.".into());
        }
        for fields in [&self.dialogue_fields, &self.action_fields] {
            if fields.len() > MAX_RATE_FIELDS
                || fields
                    .iter()
                    .any(|n| n.trim().is_empty() || n.chars().count() > 200)
            {
                return Err(format!(
                    "Count up to {MAX_RATE_FIELDS} caption fields of 1–200 characters each."
                ));
            }
        }
        Ok(())
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Seconds to speak `words` words over `lines` lines with
    /// `parentheticals` asides.
    pub fn dialogue_seconds(&self, words: usize, lines: usize, parentheticals: usize) -> f64 {
        words as f64 * 60. / self.dialogue_wpm
            + lines as f64 * self.line_pause
            + parentheticals as f64 * self.parenthetical_pause
    }

    /// Seconds the action `words` describe.
    pub fn action_seconds(&self, words: usize) -> f64 {
        words as f64 * 60. / self.action_wpm
    }

    /// Seconds for a dialogue caption.
    pub fn dialogue_caption_seconds(&self, text: &str) -> f64 {
        let d = dialogue(text);
        self.dialogue_seconds(d.words, d.lines, d.parentheticals)
    }

    fn counts(names: &[String], field: &str) -> bool {
        names.iter().any(|n| n.trim().eq_ignore_ascii_case(field))
    }

    /// Seconds a panel's counted captions need, before the minimum; `None`
    /// when they hold no words or asides to time.
    pub fn caption_seconds(&self, board: &Storyboard, panel: &Panel) -> Option<f64> {
        let mut seconds = None;
        for field in &board.captions {
            let Some(caption) = panel.captions.get(&field.id) else {
                continue;
            };
            let text = caption.text.as_str();
            let add = if Self::counts(&self.dialogue_fields, &field.name) {
                let d = dialogue(text);
                (d.words + d.parentheticals > 0)
                    .then(|| self.dialogue_seconds(d.words, d.lines, d.parentheticals))
            } else if Self::counts(&self.action_fields, &field.name) {
                let n = words(text);
                (n > 0).then(|| self.action_seconds(n))
            } else {
                None
            };
            if let Some(add) = add {
                seconds = Some(seconds.unwrap_or(0.) + add);
            }
        }
        seconds
    }

    /// Frames for `seconds` at the board's rate, never under the minimum.
    pub fn frames(&self, board: &Storyboard, seconds: f64) -> u32 {
        let rate = board.settings.frame_rate;
        (rate.seconds_to_frames(seconds.max(self.minimum_seconds)) as u32)
            .clamp(1, MAX_PANEL_FRAMES)
    }
}

/// Why a panel keeps its duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    /// Its counted captions are empty.
    NoText,
    /// The panel or its scene is locked.
    Locked,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanelEstimate {
    pub panel: PageId,
    pub scene: GroupId,
    pub old_frames: u32,
    /// The duration the captions ask for, when there is text to time.
    pub estimate: Option<u32>,
    /// The duration after applying: the estimate unless the panel is kept.
    pub new_frames: u32,
    pub kept: Option<Kept>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneEstimate {
    pub scene: GroupId,
    pub name: String,
    pub old_frames: u64,
    pub new_frames: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Estimate {
    pub panels: Vec<PanelEstimate>,
    pub scenes: Vec<SceneEstimate>,
}

impl Estimate {
    /// The panels that change, and their new durations, for
    /// `Storyboard::set_frames`.
    pub fn changes(&self) -> (Vec<PageId>, Vec<u32>) {
        self.panels
            .iter()
            .filter(|p| p.new_frames != p.old_frames)
            .map(|p| (p.panel, p.new_frames))
            .unzip()
    }

    pub fn old_frames(&self) -> u64 {
        self.scenes.iter().map(|s| s.old_frames).sum()
    }

    pub fn new_frames(&self) -> u64 {
        self.scenes.iter().map(|s| s.new_frames).sum()
    }
}

/// Estimate `panels` (any order; thumbnail sheets and unknown panels are
/// left out) from their captions, in page order with scene totals over the
/// estimated panels.
pub fn estimate(
    board: &Storyboard,
    layout: &[PageId],
    panels: &[PageId],
    rates: &WordRates,
) -> Estimate {
    let mut out = Estimate::default();
    for (id, frames) in board.playing(layout) {
        if !panels.contains(&id) {
            continue;
        }
        let panel = &board.panels[&id];
        let estimate = rates
            .caption_seconds(board, panel)
            .map(|s| rates.frames(board, s));
        let kept = if board.is_locked(id) {
            Some(Kept::Locked)
        } else if estimate.is_none() {
            Some(Kept::NoText)
        } else {
            None
        };
        let new_frames = match kept {
            None => estimate.unwrap_or(frames),
            Some(_) => frames,
        };
        match out.scenes.last_mut() {
            Some(s) if s.scene == panel.scene => {
                s.old_frames += u64::from(frames);
                s.new_frames += u64::from(new_frames);
            }
            _ => out.scenes.push(SceneEstimate {
                scene: panel.scene,
                name: board
                    .scenes
                    .get(&panel.scene)
                    .map(|s| s.name.clone())
                    .unwrap_or_default(),
                old_frames: u64::from(frames),
                new_frames: u64::from(new_frames),
            }),
        }
        out.panels.push(PanelEstimate {
            panel: id,
            scene: panel.scene,
            old_frames: frames,
            estimate,
            new_frames,
            kept,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;
    use crate::project::{ProjectEditor, ProjectKind};

    #[test]
    fn known_texts_take_known_times() {
        let r = WordRates::default();
        // 10 words at 150 wpm is 4 s, plus half a second of air.
        let ten = "one two three four five six seven eight nine ten";
        assert!((r.dialogue_caption_seconds(&format!("MIA: {ten}")) - 4.5).abs() < 1e-9);
        // The speaker is not spoken; a parenthetical adds its pause.
        let d = dialogue("MIA (quietly): Is anyone there?\nTOM: No. (beat) Maybe.");
        assert_eq!(
            d,
            DialogueCount {
                lines: 2,
                words: 5,
                parentheticals: 2
            }
        );
        assert!((r.dialogue_seconds(5, 2, 2) - (2. + 1. + 1.)).abs() < 1e-9);
        // A line without a speaker is one line; continuation lines add none.
        assert_eq!(dialogue("Is anyone there?\nHello?").lines, 1);
        // Speakers start with a capital and hold no sentence punctuation;
        // dashes are not words.
        assert_eq!(dialogue("McCLANE (V.O.): Come out — now").words, 3);
        assert_eq!(dialogue("Wait, what: now").words, 3);
        // 12 action words at 120 wpm.
        assert!((r.action_seconds(12) - 6.).abs() < 1e-9);
        assert_eq!(words("A -- B ... c."), 3);
        let fast = WordRates {
            dialogue_wpm: 300.,
            line_pause: 0.,
            ..WordRates::default()
        };
        assert!((fast.dialogue_caption_seconds(ten) - 2.).abs() < 1e-9);
        assert!(WordRates::default().validate().is_ok());
        for bad in [
            WordRates {
                dialogue_wpm: 0.,
                ..WordRates::default()
            },
            WordRates {
                line_pause: f64::NAN,
                ..WordRates::default()
            },
            WordRates {
                action_fields: vec![" ".into()],
                ..WordRates::default()
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn panels_are_estimated_by_field_name_and_summed_per_scene() {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        let blank = p.storyboard().unwrap().blank_panel().unwrap();
        let first = p.active_page();
        let ids = p
            .insert_panels(
                Some(first),
                &blank,
                vec![
                    ("A".into(), crate::storyboard::Panel::new(0, 48)),
                    ("B".into(), crate::storyboard::Panel::new(0, 48)),
                ],
                None,
            )
            .unwrap();
        p.edit_storyboard(|b| {
            let action = b.caption("Action").unwrap();
            let dialogue = b.caption("Dialogue").unwrap();
            let notes = b.caption("Notes").unwrap();
            let one = b.panels.get_mut(&ids[0]).unwrap();
            one.captions
                .insert(action, "Mia runs to the window and looks out.".into());
            one.captions
                .insert(dialogue, "MIA: Who is out there?".into());
            let two = b.panels.get_mut(&ids[1]).unwrap();
            two.captions.insert(notes, "PUSH IN slowly".into());
            Ok(())
        })
        .unwrap();
        let board = p.storyboard().unwrap();
        let layout: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
        let rates = WordRates::default();
        let all = estimate(board, &layout, &layout, &rates);
        assert_eq!(all.panels.len(), 3);
        // 8 action words (4 s) + 4 spoken words (1.6 s) + 0.5 s at 24 fps.
        let one = &all.panels[1];
        assert_eq!(one.estimate, Some(146));
        assert_eq!(one.new_frames, 146);
        // Notes do not count; blank panels keep their length.
        assert_eq!(all.panels[2].kept, Some(Kept::NoText));
        assert_eq!(all.panels[2].new_frames, 48);
        assert_eq!(all.scenes.len(), 1);
        assert_eq!(all.scenes[0].old_frames, 48 * 3);
        assert_eq!(all.scenes[0].new_frames, 48 + 146 + 48);
        assert_eq!(all.changes(), (vec![ids[0]], vec![146]));
        // Notes counted as action when asked; the minimum holds.
        let notes = WordRates {
            action_fields: vec!["notes".into()],
            minimum_seconds: 3.,
            ..WordRates::default()
        };
        let only = estimate(board, &layout, &[ids[1]], &notes);
        assert_eq!(only.panels.len(), 1);
        assert_eq!(only.panels[0].new_frames, 72);
        // Locked panels keep their duration.
        p.edit_storyboard(|b| {
            b.panels.get_mut(&ids[0]).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        let board = p.storyboard().unwrap();
        let locked = estimate(board, &layout, &layout, &rates);
        assert_eq!(locked.panels[1].kept, Some(Kept::Locked));
        assert_eq!(locked.panels[1].estimate, Some(146));
        assert!(locked.changes().0.is_empty());
    }
}
