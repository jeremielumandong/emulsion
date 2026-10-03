//! Final Draft `.fdx`: XML paragraphs typed Scene Heading, Action, Character,
//! Parenthetical, Dialogue and Transition, read with the crate's bounded XML
//! tree. The title comes from the title page when it has one.
use super::{Beat, Script, ScriptScene};
use crate::diagram_import::xml::{self, Xml};
use anyhow::{Result, anyhow, bail};

/// A paragraph's text: its `Text` runs joined (styles split one line into
/// several runs).
fn text(paragraph: &Xml) -> String {
    let runs: String = paragraph
        .children("Text")
        .map(|t| t.text.as_str())
        .collect();
    let runs = if runs.is_empty() {
        paragraph.text.clone()
    } else {
        runs
    };
    runs.trim().to_string()
}

pub(super) fn parse(bytes: &[u8]) -> Result<Script> {
    let source = std::str::from_utf8(bytes).map_err(|_| anyhow!("The script is not UTF-8."))?;
    let root =
        xml::parse(source).map_err(|e| anyhow!("This Final Draft file is not valid XML: {e}"))?;
    if root.name != "FinalDraft" {
        bail!("This is not a Final Draft script.");
    }
    let title = root.child("TitlePage").and_then(|page| {
        page.descendants("Paragraph")
            .map(text)
            .find(|t| !t.is_empty())
    });
    let mut script = Script {
        title,
        scenes: Vec::new(),
    };
    let Some(content) = root.child("Content") else {
        return Ok(script);
    };
    let mut scene = ScriptScene::default();
    let mut cue: Option<(String, Option<String>)> = None;
    for paragraph in content.children("Paragraph") {
        let text = text(paragraph);
        if text.is_empty() {
            continue;
        }
        match paragraph.attr("Type") {
            "Scene Heading" => {
                cue = None;
                if !scene.heading.is_empty() || !scene.beats.is_empty() {
                    script.scenes.push(std::mem::take(&mut scene));
                }
                scene.heading = text;
            }
            "Character" => cue = Some((text, None)),
            "Parenthetical" => {
                if let Some((_, paren)) = &mut cue {
                    *paren = Some(text);
                }
            }
            "Dialogue" => {
                let (character, parenthetical) = cue.take().unwrap_or_default();
                scene.beats.push(Beat::Dialogue {
                    character,
                    parenthetical,
                    text,
                });
            }
            "Transition" => {
                cue = None;
                scene.beats.push(Beat::Transition(text));
            }
            // Action, General, Shot and anything else read as action.
            _ => {
                cue = None;
                scene.beats.push(Beat::Action(text));
            }
        }
    }
    if !scene.heading.is_empty() || !scene.beats.is_empty() {
        script.scenes.push(scene);
    }
    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_become_scenes_and_beats() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
<FinalDraft DocumentType="Script" Version="5">
  <Content>
    <Paragraph Type="Scene Heading"><Text>EXT. ROOF - NIGHT</Text></Paragraph>
    <Paragraph Type="Action"><Text>Wind. Mia </Text><Text Style="Bold">grips</Text><Text> the rail.</Text></Paragraph>
    <Paragraph Type="Character"><Text>MIA</Text></Paragraph>
    <Paragraph Type="Parenthetical"><Text>(to herself)</Text></Paragraph>
    <Paragraph Type="Dialogue"><Text>Don't look down &amp; breathe.</Text></Paragraph>
    <Paragraph Type="Transition"><Text>SMASH CUT TO:</Text></Paragraph>
    <Paragraph Type="Scene Heading"><Text>INT. STAIRS - CONTINUOUS</Text></Paragraph>
  </Content>
  <TitlePage><Content><Paragraph Type="Text"><Text>Vertigo</Text></Paragraph></Content></TitlePage>
</FinalDraft>"#;
        let script = parse(xml.as_bytes()).unwrap();
        assert_eq!(script.title.as_deref(), Some("Vertigo"));
        assert_eq!(script.scenes.len(), 2);
        let roof = &script.scenes[0];
        assert_eq!(roof.heading, "EXT. ROOF - NIGHT");
        assert_eq!(
            roof.beats[0],
            Beat::Action("Wind. Mia grips the rail.".into())
        );
        assert_eq!(
            roof.beats[1],
            Beat::Dialogue {
                character: "MIA".into(),
                parenthetical: Some("(to herself)".into()),
                text: "Don't look down & breathe.".into(),
            }
        );
        assert_eq!(roof.beats[2], Beat::Transition("SMASH CUT TO:".into()));
        assert_eq!(script.scenes[1].heading, "INT. STAIRS - CONTINUOUS");
    }

    #[test]
    fn other_or_broken_xml_is_refused() {
        assert!(parse(b"<svg></svg>").is_err());
        assert!(parse(b"<FinalDraft><Content><Paragraph").is_err());
        assert!(parse(b"<!DOCTYPE x [<!ENTITY a 'b'>]><FinalDraft/>").is_err());
    }
}
