//! Fountain (fountain.io): scene headings, action, character cues with
//! parentheticals and dialogue, transitions, a title page, and the markup
//! that is not story (notes, boneyard, sections, synopses, page breaks),
//! which is dropped.
use super::{Beat, Script, ScriptScene};

const HEADING_PREFIXES: [&str; 8] = [
    "INT.",
    "EXT.",
    "EST.",
    "INT./EXT.",
    "INT/EXT",
    "I/E",
    "INT ",
    "EXT ",
];

fn is_heading(line: &str) -> bool {
    let upper = line.to_uppercase();
    (line.starts_with('.') && !line.starts_with(".."))
        || HEADING_PREFIXES.iter().any(|p| upper.starts_with(p))
}

fn heading_text(line: &str) -> String {
    let line = line.strip_prefix('.').unwrap_or(line);
    // A trailing scene number such as `#12#` is not part of the heading.
    let line = match (line.rfind('#'), line.find('#')) {
        (Some(end), Some(start)) if end > start && line.ends_with('#') => &line[..start],
        _ => line,
    };
    line.trim().to_string()
}

fn is_transition(line: &str) -> bool {
    (line.starts_with('>') && !line.ends_with('<'))
        || (line.ends_with("TO:")
            && line == line.to_uppercase()
            && !line.contains(char::is_lowercase))
}

/// A character cue: an all-caps line (letters present), or forced with `@`.
fn character(line: &str) -> Option<String> {
    if let Some(name) = line.strip_prefix('@') {
        return Some(name.trim().to_string());
    }
    let name = line.trim_end_matches('^').trim();
    // Extensions such as (V.O.) may be lower case.
    let core = name.split('(').next().unwrap_or(name);
    (core.chars().any(char::is_alphabetic)
        && core == core.to_uppercase()
        && !core.trim().is_empty())
    .then(|| name.to_string())
}

/// Remove notes `[[…]]` and boneyard `/*…*/`, which can span lines.
fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let note = rest.find("[[");
        let bone = rest.find("/*");
        let (start, open, close) = match (note, bone) {
            (Some(n), Some(b)) if b < n => (b, "/*", "*/"),
            (Some(n), _) => (n, "[[", "]]"),
            (None, Some(b)) => (b, "/*", "*/"),
            (None, None) => break,
        };
        out.push_str(&rest[..start]);
        match rest[start + open.len()..].find(close) {
            Some(end) => rest = &rest[start + open.len() + end + close.len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Strip Fountain emphasis marks, keeping the words.
fn plain(text: &str) -> String {
    text.replace("***", "")
        .replace("**", "")
        .replace(['*', '_'], "")
        .trim()
        .to_string()
}

pub(super) fn parse(text: &str) -> Script {
    let text = strip_markup(&text.replace("\r\n", "\n").replace('\r', "\n"));
    let mut script = Script::default();
    let mut lines: Vec<&str> = text.lines().collect();
    // Title page: `Key: value` lines before the first blank line.
    const TITLE_KEYS: [&str; 10] = [
        "title",
        "credit",
        "author",
        "authors",
        "source",
        "draft date",
        "date",
        "contact",
        "notes",
        "copyright",
    ];
    if lines.first().is_some_and(|l| {
        l.split_once(':')
            .is_some_and(|(k, _)| TITLE_KEYS.contains(&k.trim().to_lowercase().as_str()))
    }) {
        let end = lines
            .iter()
            .position(|l| l.trim().is_empty())
            .unwrap_or(lines.len());
        let mut key = String::new();
        for line in &lines[..end] {
            if let Some((k, v)) = line
                .split_once(':')
                .filter(|_| !line.starts_with([' ', '\t']))
            {
                key = k.trim().to_lowercase();
                if key == "title" && !v.trim().is_empty() {
                    script.title = Some(plain(v));
                }
            } else if key == "title" && script.title.is_none() {
                script.title = Some(plain(line));
            }
        }
        lines.drain(..end);
    }
    let mut scene = ScriptScene::default();
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut block = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            // Two spaces keep a blank line inside a dialogue block.
            if line == "  " && !block.is_empty() {
                block.push("");
                continue;
            }
            if !block.is_empty() {
                blocks.push(std::mem::take(&mut block));
            }
        } else {
            block.push(line);
        }
    }
    if !block.is_empty() {
        blocks.push(block);
    }
    for block in blocks {
        let first = block[0].trim();
        if first.starts_with('#') || first.starts_with('=') {
            // Sections, synopses and page breaks are outline, not story.
            continue;
        }
        if block.len() == 1 && is_heading(first) {
            if !scene.heading.is_empty() || !scene.beats.is_empty() {
                script.scenes.push(std::mem::take(&mut scene));
            }
            scene.heading = heading_text(first);
            continue;
        }
        if block.len() == 1 && is_transition(first) {
            let text = first.trim_start_matches('>').trim().to_string();
            scene.beats.push(Beat::Transition(plain(&text)));
            continue;
        }
        if block.len() > 1
            && !first.starts_with('!')
            && let Some(name) = character(first)
        {
            let mut parenthetical = None;
            let mut text = Vec::new();
            for line in &block[1..] {
                let line = line.trim();
                if line.starts_with('(') && line.ends_with(')') && text.is_empty() {
                    parenthetical = Some(line.to_string());
                } else if line.starts_with('(') && line.ends_with(')') {
                    // A later parenthetical stays with the words.
                    text.push(line.to_string());
                } else {
                    text.push(plain(line));
                }
            }
            scene.beats.push(Beat::Dialogue {
                character: name,
                parenthetical,
                text: text.join("\n").trim().to_string(),
            });
            continue;
        }
        let action: Vec<String> = block
            .iter()
            .map(|l| {
                let l = l.strip_prefix('!').unwrap_or(l);
                let l = l.trim();
                let l = l
                    .strip_prefix('>')
                    .and_then(|c| c.strip_suffix('<'))
                    .unwrap_or(l);
                plain(l)
            })
            .collect();
        let action = action.join("\n").trim().to_string();
        if !action.is_empty() {
            scene.beats.push(Beat::Action(action));
        }
    }
    if !scene.heading.is_empty() || !scene.beats.is_empty() {
        script.scenes.push(scene);
    }
    script
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Title: **The Long Night**
Author: A. Writer

INT. KITCHEN - NIGHT #1#

Rain hammers the window. *Mia* stands alone.

MIA
(whispering)
Is anyone there?

[[cut this?]] The kettle /* old line */ screams.

TOM (O.S.)
Only me.

CUT TO:

.FLASHBACK

# Act two

= Mia remembers.

>THE END<
";

    #[test]
    fn scenes_dialogue_transitions_and_title() {
        let script = parse(SAMPLE);
        assert_eq!(script.title.as_deref(), Some("The Long Night"));
        assert_eq!(script.scenes.len(), 2);
        let kitchen = &script.scenes[0];
        assert_eq!(kitchen.heading, "INT. KITCHEN - NIGHT");
        assert_eq!(
            kitchen.beats[0],
            Beat::Action("Rain hammers the window. Mia stands alone.".into())
        );
        assert_eq!(
            kitchen.beats[1],
            Beat::Dialogue {
                character: "MIA".into(),
                parenthetical: Some("(whispering)".into()),
                text: "Is anyone there?".into(),
            }
        );
        assert_eq!(
            kitchen.beats[2],
            Beat::Action("The kettle  screams.".into())
        );
        assert!(
            matches!(&kitchen.beats[3], Beat::Dialogue { character, .. } if character == "TOM (O.S.)")
        );
        assert_eq!(kitchen.beats[4], Beat::Transition("CUT TO:".into()));
        let flashback = &script.scenes[1];
        assert_eq!(flashback.heading, "FLASHBACK");
        assert_eq!(flashback.beats, [Beat::Action("THE END".into())]);
    }

    #[test]
    fn plain_prose_is_action_and_forced_elements_work() {
        let script = parse("He runs.\n\nShe follows.\n\n!LOUD NOISE\n\n@McKenzie\nHello.");
        assert_eq!(script.scenes.len(), 1);
        let beats = &script.scenes[0].beats;
        assert_eq!(beats.len(), 4);
        assert_eq!(beats[2], Beat::Action("LOUD NOISE".into()));
        assert!(matches!(&beats[3], Beat::Dialogue { character, .. } if character == "McKenzie"));
    }
}
