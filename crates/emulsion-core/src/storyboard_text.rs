//! Storyboard caption text: plain text with optional character formatting,
//! stored as the text layer model's runs, plus find and replace across
//! captions.
use crate::text::{TextRun, TextSpec, TextStyle};
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Caption text. `runs` are byte ranges of `text` with their character style,
/// exactly as text layers store them; text outside every run is unstyled.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "CaptionRepr")]
pub struct Caption {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<TextRun>,
}

/// Older boards stored captions as plain strings.
#[derive(Deserialize)]
#[serde(untagged)]
enum CaptionRepr {
    Plain(String),
    Rich {
        text: String,
        #[serde(default)]
        runs: Vec<TextRun>,
    },
}

impl From<CaptionRepr> for Caption {
    fn from(repr: CaptionRepr) -> Self {
        match repr {
            CaptionRepr::Plain(text) => text.into(),
            CaptionRepr::Rich { text, runs } => Self { text, runs },
        }
    }
}

impl From<&str> for Caption {
    fn from(text: &str) -> Self {
        text.to_string().into()
    }
}

impl From<String> for Caption {
    fn from(text: String) -> Self {
        Self {
            text,
            runs: Vec::new(),
        }
    }
}

impl Caption {
    fn spec(&self) -> TextSpec {
        TextSpec {
            text: self.text.clone(),
            runs: self.runs.clone(),
            ..TextSpec::default()
        }
    }
    fn take(&mut self, spec: TextSpec) {
        self.text = spec.text;
        self.runs = spec.runs;
    }
    /// The style unformatted caption text has.
    pub fn base_style() -> TextStyle {
        TextSpec::default().base_style()
    }
    pub fn style_at(&self, byte: usize) -> TextStyle {
        self.spec().style_at(byte)
    }
    /// Format a byte range, as text layers do.
    pub fn apply_style(&mut self, range: Range<usize>, edit: impl FnMut(&mut TextStyle)) {
        let mut spec = self.spec();
        spec.apply_style(range, edit);
        self.take(spec);
    }
    /// Replace a byte range, keeping the formatting around it.
    pub fn replace_range(&mut self, range: Range<usize>, replacement: &str) {
        let mut spec = self.spec();
        spec.replace_range(range, replacement);
        self.take(spec);
    }
    pub fn validate(&self) -> Result<(), String> {
        let mut end = 0;
        for run in &self.runs {
            if run.start < end
                || run.start >= run.end
                || run.end > self.text.len()
                || !self.text.is_char_boundary(run.start)
                || !self.text.is_char_boundary(run.end)
            {
                return Err("Caption formatting must cover ordered ranges of its text.".into());
            }
            end = run.end;
        }
        Ok(())
    }
}

/// How `find` matches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
}

/// Non-overlapping byte ranges of `query` in `text`.
pub fn find(text: &str, query: &str, options: FindOptions) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let fold = |c: char| -> Box<dyn Iterator<Item = char>> {
        if options.match_case {
            Box::new(std::iter::once(c))
        } else {
            Box::new(c.to_lowercase())
        }
    };
    let wanted: Vec<char> = query.chars().flat_map(fold).collect();
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut found = Vec::new();
    let mut from = 0;
    for (start, _) in text.char_indices() {
        if start < from {
            continue;
        }
        let mut folded = Vec::with_capacity(wanted.len());
        let mut end = start;
        for (offset, c) in text[start..].char_indices() {
            if folded.len() >= wanted.len() {
                break;
            }
            folded.extend(fold(c));
            end = start + offset + c.len_utf8();
        }
        if folded != wanted {
            continue;
        }
        if options.whole_word
            && (is_word(text[..start].chars().next_back()) || is_word(text[end..].chars().next()))
        {
            continue;
        }
        found.push(start..end);
        from = end;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_strings_still_read_and_rich_captions_round_trip() {
        let old: Caption = serde_json::from_str("\"Hero runs\"").unwrap();
        assert_eq!(old, Caption::from("Hero runs"));
        let mut rich = old.clone();
        rich.apply_style(0..4, |s| s.bold = true);
        assert!(rich.style_at(0).bold && !rich.style_at(5).bold);
        let json = serde_json::to_string(&rich).unwrap();
        assert_eq!(serde_json::from_str::<Caption>(&json).unwrap(), rich);
        assert_eq!(
            serde_json::to_string(&old).unwrap(),
            r#"{"text":"Hero runs"}"#
        );
    }

    #[test]
    fn replacing_keeps_formatting_around_the_change() {
        let mut caption = Caption::from("Mia waves at Mia");
        caption.apply_style(0..3, |s| s.italic = true);
        caption.replace_range(13..16, "Tom");
        assert_eq!(caption.text, "Mia waves at Tom");
        assert!(caption.style_at(1).italic && !caption.style_at(14).italic);
        caption.validate().unwrap();
    }

    #[test]
    fn find_matches_case_words_and_unicode() {
        let text = "Café CAFÉ cafés";
        let any = find(text, "café", FindOptions::default());
        assert_eq!(any.len(), 3);
        assert_eq!(&text[any[1].clone()], "CAFÉ");
        let exact = FindOptions {
            match_case: true,
            ..Default::default()
        };
        assert_eq!(find(text, "Café", exact).len(), 1);
        let words = FindOptions {
            whole_word: true,
            ..Default::default()
        };
        assert_eq!(find(text, "café", words).len(), 2);
        assert!(find(text, "", FindOptions::default()).is_empty());
    }

    #[test]
    fn invalid_runs_are_rejected() {
        let mut caption = Caption::from("é");
        caption.runs.push(TextRun {
            start: 0,
            end: 1,
            style: TextStyle::default(),
        });
        assert!(caption.validate().is_err());
    }
}
