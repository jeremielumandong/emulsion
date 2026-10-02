//! Screenplays read into a neutral form: scenes of action, dialogue and
//! transitions, from plain text, Fountain or Final Draft (`.fdx`). Any
//! workspace can lay the beats out its own way; Storyboard turns them into
//! panels with captions.
mod fdx;
mod fountain;
pub mod storyboard;

use anyhow::{Context, Result, bail};
use emulsion_core::storyboard_estimate::{self as estimate, WordRates};
use std::path::Path;

/// Scripts larger than this are refused.
pub const MAX_BYTES: u64 = 16 << 20;

/// One unit of a scene.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Beat {
    Action(String),
    Dialogue {
        character: String,
        parenthetical: Option<String>,
        text: String,
    },
    Transition(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptScene {
    /// The scene heading (slugline), such as `INT. KITCHEN - NIGHT`; empty
    /// for material before the first heading.
    pub heading: String,
    pub beats: Vec<Beat>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Script {
    pub title: Option<String>,
    pub scenes: Vec<ScriptScene>,
}

impl Script {
    pub fn beat_count(&self) -> usize {
        self.scenes.iter().map(|s| s.beats.len()).sum()
    }
}

/// The formats `read` understands, by file extension.
pub const EXTENSIONS: [&str; 4] = ["fountain", "fdx", "txt", "spmd"];

/// Read a script file. `.fdx` is Final Draft; `.fountain` and `.spmd` are
/// Fountain; anything else is read as Fountain, which treats ordinary prose
/// as action, so plain text works too.
pub fn read(path: &Path) -> Result<Script> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("Cannot read {}", path.display()))?
        .len();
    if size > MAX_BYTES {
        bail!("Scripts are limited to {} MiB.", MAX_BYTES >> 20);
    }
    let bytes = std::fs::read(path)?;
    let fdx = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("fdx"));
    if fdx {
        return fdx::parse(&bytes);
    }
    let text = String::from_utf8(bytes).context("The script is not UTF-8 text.")?;
    Ok(fountain::parse(&text))
}

/// Parse Fountain (or plain) text.
pub fn parse_fountain(text: &str) -> Script {
    fountain::parse(text)
}

/// Parse Final Draft XML.
pub fn parse_fdx(bytes: &[u8]) -> Result<Script> {
    fdx::parse(bytes)
}

/// Seconds a beat takes to play, for first timings, by the storyboard word
/// rates (`WordRates::default`): dialogue at 150 words a minute with a pause
/// after the line and its parenthetical, action at 120, never under
/// `minimum`.
pub fn estimated_seconds(beat: &Beat, minimum: f64) -> f64 {
    beat_seconds(beat, &WordRates::default()).max(minimum)
}

/// Seconds a beat takes at `rates` (no minimum): a dialogue block timed
/// exactly as its Dialogue caption would be.
pub fn beat_seconds(beat: &Beat, rates: &WordRates) -> f64 {
    match beat {
        Beat::Dialogue {
            character,
            parenthetical,
            text,
        } => rates.dialogue_caption_seconds(&dialogue_line(character, parenthetical, text)),
        Beat::Action(text) => rates.action_seconds(estimate::words(text)),
        Beat::Transition(_) => estimate::TRANSITION_SECONDS,
    }
}

/// A dialogue block as one caption line: "MIA (quietly): text".
pub fn dialogue_line(character: &str, parenthetical: &Option<String>, text: &str) -> String {
    match parenthetical {
        Some(p) => format!("{character} {p}: {text}"),
        None => format!("{character}: {text}"),
    }
}

/// The ID read_storyboard_script gives a beat: "s2b5" for the fifth beat of
/// the second scene.
pub fn beat_id(scene: usize, beat: usize) -> String {
    emulsion_core::storyboard_breakdown::beat_id(scene, beat)
}

impl Script {
    /// The beat a beat ID names.
    pub fn beat(&self, id: &str) -> Option<&Beat> {
        let (scene, beat) = emulsion_core::storyboard_breakdown::beat_index(id)?;
        self.scenes.get(scene)?.beats.get(beat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_read_by_extension_and_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("idea.txt");
        std::fs::write(&plain, "A storm rolls in.\n\nThe lights go out.").unwrap();
        let script = read(&plain).unwrap();
        assert_eq!(script.beat_count(), 2);
        let fdx = dir.path().join("pilot.fdx");
        std::fs::write(
            &fdx,
            r#"<?xml version="1.0"?><FinalDraft><Content><Paragraph Type="Scene Heading"><Text>INT. HALL - DAY</Text></Paragraph></Content></FinalDraft>"#,
        )
        .unwrap();
        assert_eq!(read(&fdx).unwrap().scenes[0].heading, "INT. HALL - DAY");
        assert!(read(&dir.path().join("missing.fountain")).is_err());
    }

    #[test]
    fn timings_follow_the_words() {
        let line = Beat::Dialogue {
            character: "MIA".into(),
            parenthetical: None,
            text: "one two three four five six seven eight nine ten".into(),
        };
        assert!((estimated_seconds(&line, 1.) - 4.5).abs() < 1e-9);
        assert_eq!(
            estimated_seconds(&Beat::Transition("CUT TO:".into()), 2.),
            2.
        );
        // 12 action words at 120 words a minute.
        let action =
            Beat::Action("Mia stands at the sink. The back door creaks open behind her.".into());
        assert!((estimated_seconds(&action, 0.) - 6.).abs() < 1e-9);
        let script = parse_fountain("INT. HALL - DAY\n\nRain.\n\nMIA\nHello.\n");
        assert!(matches!(script.beat("s1b2"), Some(Beat::Dialogue { .. })));
        assert_eq!(beat_id(0, 1), "s1b2");
        assert!(script.beat("s2b1").is_none());
    }
}
