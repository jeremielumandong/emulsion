//! Spell checking with the bundled en_US Hunspell dictionary (see
//! `assets/dictionaries/`), plus the person's own words. The dictionary is
//! parsed once, on first use, off the UI thread where callers can manage it.
use spellbook::Dictionary;
use std::ops::Range;
use std::sync::OnceLock;
use unicode_segmentation::UnicodeSegmentation;

const AFF: &str = include_str!("../../../assets/dictionaries/en_US.aff");
const DIC: &str = include_str!("../../../assets/dictionaries/en_US.dic");

/// The bundled dictionary.
pub fn dictionary() -> &'static Dictionary {
    static DICTIONARY: OnceLock<Dictionary> = OnceLock::new();
    DICTIONARY.get_or_init(|| Dictionary::new(AFF, DIC).expect("bundled dictionary parses"))
}

/// Words a checker leaves alone: all capitals (names, sluglines, character
/// cues), anything with a digit, single letters.
fn ignored(word: &str) -> bool {
    let letters = word.chars().filter(|c| c.is_alphabetic()).count();
    letters < 2
        || word.chars().any(|c| c.is_ascii_digit())
        || (word.chars().any(char::is_uppercase) && !word.chars().any(char::is_lowercase))
}

fn known(word: &str, personal: &[String]) -> bool {
    personal.iter().any(|w| w.eq_ignore_ascii_case(word)) || dictionary().check(word)
}

/// Byte ranges of the misspelt words in `text`.
pub fn misspellings(text: &str, personal: &[String]) -> Vec<Range<usize>> {
    text.split_word_bound_indices()
        .filter(|(_, w)| w.chars().any(char::is_alphabetic))
        .filter_map(|(at, w)| {
            // Curly apostrophes check like straight ones.
            let word = w
                .trim_matches(|c: char| c == '\'' || c == '’')
                .replace('’', "'");
            (!ignored(&word) && !known(&word, personal)).then(|| at..at + w.len())
        })
        .collect()
}

/// Up to `limit` corrections for `word`, best first.
pub fn suggestions(word: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    dictionary().suggest(word, &mut out);
    out.truncate(limit);
    out
}

/// Misspelt words across a storyboard's captions, in page then field order:
/// each panel, caption field and byte range.
pub fn storyboard(
    board: &emulsion_core::storyboard::Storyboard,
    layout: &[emulsion_core::project::PageId],
    personal: &[String],
) -> Vec<(
    emulsion_core::project::PageId,
    emulsion_core::storyboard::CaptionId,
    Range<usize>,
)> {
    let mut out = Vec::new();
    for &panel in layout {
        let Some(data) = board.panels.get(&panel) else {
            continue;
        };
        for field in &board.captions {
            if let Some(caption) = data.captions.get(&field.id) {
                out.extend(
                    misspellings(&caption.text, personal)
                        .into_iter()
                        .map(|range| (panel, field.id, range)),
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storyboard_captions_are_checked_in_page_then_field_order() {
        use emulsion_core::Document;
        use emulsion_core::project::{ProjectEditor, ProjectKind};
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        let panel = p.active_page();
        p.edit_storyboard(|b| {
            let action = b.caption("Action").unwrap();
            let dialogue = b.caption("Dialogue").unwrap();
            let data = b.panels.get_mut(&panel).unwrap();
            data.captions.insert(dialogue, "Helo there.".into());
            data.captions.insert(action, "Mia runns home.".into());
            Ok(())
        })
        .unwrap();
        let board = p.storyboard().unwrap();
        let found: Vec<_> = storyboard(board, &[panel], &[])
            .into_iter()
            .map(|(id, field, range)| {
                (
                    id,
                    field,
                    board.panels[&id].captions[&field].text[range].to_string(),
                )
            })
            .collect();
        let field = |name| board.caption(name).unwrap();
        assert_eq!(
            found,
            [
                (panel, field("Action"), "runns".to_string()),
                (panel, field("Dialogue"), "Helo".to_string())
            ]
        );
        assert_eq!(
            storyboard(board, &[panel], &["runns".into(), "helo".into()]),
            []
        );
    }

    #[test]
    fn misspelt_words_are_found_and_corrected() {
        let text = "Mia runns to the windw. INT. KITCHEN - NIGHT, take 2.";
        let found: Vec<_> = misspellings(text, &[])
            .into_iter()
            .map(|r| &text[r])
            .collect();
        assert_eq!(found, ["runns", "windw"]);
        assert!(suggestions("windw", 5).iter().any(|s| s == "window"));
        // The person's own words are accepted.
        assert!(misspellings("Zorbak waits.", &["zorbak".into()]).is_empty());
        assert_eq!(misspellings("Zorbak waits.", &[]).len(), 1);
        // Contractions and curly apostrophes are words.
        assert!(misspellings("She doesn’t know. Don't go.", &[]).is_empty());
    }
}
