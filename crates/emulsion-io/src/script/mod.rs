//! Screenplays read into a neutral form: scenes of action, dialogue and
//! transitions, from plain text, Fountain or Final Draft (`.fdx`). Any
//! workspace can lay the beats out its own way; Storyboard turns them into
//! panels with captions.
mod fdx;
mod fountain;
pub mod storyboard;

use anyhow::{Context, Result, bail};
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

/// Seconds a beat takes to play, for first timings: about two and a half
/// words a second (dialogue gets half a second of air), never under
/// `minimum`.
pub fn estimated_seconds(beat: &Beat, minimum: f64) -> f64 {
    let words = |s: &str| s.split_whitespace().count() as f64;
    let seconds = match beat {
        Beat::Dialogue { text, .. } => words(text) / 2.5 + 0.5,
        Beat::Action(text) => words(text) / 2.5,
        Beat::Transition(_) => 1.,
    };
    seconds.max(minimum)
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
    }
}
