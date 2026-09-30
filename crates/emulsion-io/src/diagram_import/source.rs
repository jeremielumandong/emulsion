//! Diagram source files share the paste-data parser and editable document path.
use super::*;
use crate::diagram_data::{self, Format};

pub(super) fn is_glyphtide(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file.take((1 << 20) + 1).read_to_end(&mut bytes).is_err() || bytes.len() > 1 << 20 {
        return false;
    }
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .is_some_and(|v| v["code"].is_string())
}

pub(super) fn supports(extension: &str) -> bool {
    Format::from_extension(extension).is_some()
        || ["md", "markdown", "glyphtide"]
            .iter()
            .any(|e| extension.eq_ignore_ascii_case(e))
}

pub(super) fn read(path: &Path) -> Result<Imported> {
    let mut text = String::new();
    std::fs::File::open(path)?
        .take((1 << 20) + 1)
        .read_to_string(&mut text)?;
    if text.len() > 1 << 20 {
        return Err(error("Diagram source exceeds 1 MiB."));
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .filter(|c| !c.is_control())
        .take(180)
        .collect::<String>();
    if name.trim().is_empty() {
        name = "Imported diagram".into();
    }
    let mut sources = Vec::new();
    if ext == "glyphtide" || ext == "json" {
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| error(format!("Invalid Glyphtide JSON: {e}")))?;
        let format = match value["engine"].as_str().unwrap_or("mermaid") {
            "mermaid" => Format::Mermaid,
            "d2" => Format::D2,
            "graphviz" => Format::Graphviz,
            other => return Err(error(format!("Unknown Glyphtide engine: {other}"))),
        };
        let code = value["code"]
            .as_str()
            .ok_or_else(|| error("Glyphtide JSON needs a code string."))?;
        sources.push((format, code.to_string()));
    } else if ext == "md" || ext == "markdown" {
        let mut fence: Option<(String, Option<Format>, String)> = None;
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some((marker, format, code)) = &mut fence {
                if trimmed == marker {
                    if let Some(format) = format {
                        sources.push((*format, std::mem::take(code)));
                    }
                    fence = None;
                } else {
                    code.push_str(line);
                    code.push('\n');
                }
            } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                let count = trimmed
                    .chars()
                    .take_while(|c| *c == trimmed.chars().next().unwrap())
                    .count();
                let marker = trimmed[..count].to_string();
                let language = trimmed[count..].trim();
                let format = match language {
                    "mermaid" => Some(Format::Mermaid),
                    "d2" => Some(Format::D2),
                    "dot" | "graphviz" => Some(Format::Graphviz),
                    _ => None,
                };
                fence = Some((marker, format, String::new()));
            }
            if sources.len() > 64 {
                return Err(error("Import at most 64 diagram code blocks at a time."));
            }
        }
        if fence
            .as_ref()
            .is_some_and(|(_, format, _)| format.is_some())
        {
            return Err(error("Unclosed diagram code fence."));
        }
        if sources.is_empty() {
            return Err(error(
                "Markdown contains no Mermaid, D2 or Graphviz code blocks.",
            ));
        }
    } else {
        sources.push((
            Format::from_extension(&ext)
                .ok_or_else(|| error("Unknown diagram source extension."))?,
            text,
        ));
    }
    let count = sources.len();
    let mut pages = Vec::new();
    let mut warnings = BTreeSet::new();
    for (i, (format, source)) in sources.into_iter().enumerate() {
        let draft = diagram_data::parse(&source, format)?;
        let doc = draft.document()?;
        warnings.extend(draft.warnings);
        pages.push(ProjectPage {
            meta: PageMeta {
                id: i as u64 + 1,
                name: if count == 1 {
                    name.clone()
                } else {
                    format!("{name} · {}", i + 1)
                },
                bleed_mm: 0.,
            },
            graph: Graph::new(doc.clone(), "Imported diagram source"),
            doc,
        });
    }
    let project = Project {
        kind: ProjectKind::Diagram,
        pages,
        active: 1,
        next_page_id: count as u64 + 1,
    };
    project.validate().map_err(error)?;
    Ok(Imported {
        project,
        warnings: warnings.into_iter().collect(),
    })
}
