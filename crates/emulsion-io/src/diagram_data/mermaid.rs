//! Rust port of Glyphtide's frontend/src/lib/diagramModel.js extractors.
//! Extended with quoted-label scanning, attributes, metadata and explicit
//! editable source records for diagram families without a native layout.
use super::syntax::{self, unquote};
use super::*;
use regex::Regex;

const TYPES: &[&str] = &[
    "flowchart",
    "graph",
    "sequencediagram",
    "classdiagram",
    "statediagram",
    "statediagram-v2",
    "erdiagram",
    "gantt",
    "mindmap",
    "pie",
    "journey",
    "quadrantchart",
    "requirementdiagram",
    "gitgraph",
    "c4context",
    "c4container",
    "c4component",
    "c4dynamic",
    "c4deployment",
    "timeline",
    "sankey-beta",
    "xychart-beta",
    "xychart",
    "block-beta",
    "block",
    "packet-beta",
    "packet",
    "kanban",
    "architecture-beta",
    "radar-beta",
    "treemap-beta",
];

pub(super) fn parse(source: &str) -> Result<Draft> {
    let mut draft = Draft::new();
    let mut source = source.trim().trim_start_matches('\u{feff}').trim();
    if source.starts_with("```mermaid") {
        source = source.strip_prefix("```mermaid").unwrap().trim();
        source = source
            .strip_suffix("```")
            .ok_or_else(|| error("Unclosed Mermaid code fence."))?
            .trim();
    }
    if source.starts_with("---") {
        let rest = source.strip_prefix("---").unwrap();
        let end = rest
            .find("\n---")
            .ok_or_else(|| error("Unclosed Mermaid front matter."))?;
        for line in rest[..end].lines().filter(|l| !l.trim().is_empty()) {
            syntax::record(&mut draft, line, "front matter")?;
        }
        source = rest[end + 4..].trim();
        draft.warn("Mermaid front matter is retained as editable data notes; theme and layout settings use Emulsion defaults.");
    }
    let cleaned = source
        .lines()
        .map(|l| syntax::comment(l, "%%"))
        .collect::<Vec<_>>()
        .join("\n");
    let statements = syntax::split(&cleaned, ';')?
        .into_iter()
        .flat_map(str::lines)
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>();
    let header = *statements
        .first()
        .ok_or_else(|| error("Expected a Mermaid diagram header."))?;
    let kind = header
        .split_whitespace()
        .next()
        .unwrap()
        .to_ascii_lowercase();
    if !TYPES.contains(&kind.as_str()) {
        return Err(error(format!("Unknown Mermaid diagram type: {kind}")));
    }
    let body = &statements[1..];
    if header.split_whitespace().any(|s| s == "LR" || s == "RL") {
        draft.layout = Layout::Horizontal;
    }
    if header.split_whitespace().any(|s| s == "RL" || s == "BT") {
        draft.warn("Reverse layout direction is normalized to Emulsion's forward layout.");
    }
    match kind.as_str() {
        "graph" | "flowchart" | "statediagram" | "statediagram-v2" => {
            flow(&mut draft, body, kind.starts_with("state"))?
        }
        "sequencediagram" => sequence(&mut draft, body)?,
        "classdiagram" | "erdiagram" => entities(&mut draft, body, kind == "erdiagram")?,
        "mindmap" => mindmap(&mut draft, &cleaned)?,
        "sankey-beta" => sankey(&mut draft, body)?,
        _ => {
            for line in body {
                syntax::record(&mut draft, line, &kind)?;
            }
            draft.warn("This Mermaid family is imported as editable data notes in source order, not its specialized chart layout.");
        }
    }
    Ok(draft)
}

fn node(draft: &mut Draft, value: &str) -> Result<String> {
    let value = value.trim();
    if value == "[*]" {
        draft.node("[*]", "Start / End", ShapeKind::Terminator)?;
        return Ok("[*]".into());
    }
    let end = value
        .char_indices()
        .find(|(_, c)| !(c.is_alphanumeric() || "_.-".contains(*c)))
        .map(|(i, _)| i)
        .unwrap_or(value.len());
    if end == 0 {
        return Err(error(format!("Invalid Mermaid node: {value}")));
    }
    let id = &value[..end];
    let mut rest = value[end..].trim();
    let mut classes = None;
    if let Some(i) = syntax::outside(rest)?
        .into_iter()
        .find(|&i| rest[i..].starts_with(":::"))
    {
        classes = Some(rest[i + 3..].trim().to_string());
        rest = rest[..i].trim();
        draft.warn("Mermaid classes and styling use Emulsion's default appearance.");
    }
    let mut label = id.to_string();
    let mut kind = ShapeKind::Process;
    if !rest.is_empty() {
        let patterns = [
            ("([", "])", ShapeKind::Terminator),
            ("[[", "]]", ShapeKind::Process),
            ("[(", ")]", ShapeKind::Database),
            ("((", "))", ShapeKind::Terminator),
            ("{{", "}}", ShapeKind::Decision),
            ("[/", "/]", ShapeKind::Data),
            ("[\\", "\\]", ShapeKind::Data),
            ("[/", "\\]", ShapeKind::Data),
            ("[\\", "/]", ShapeKind::Data),
            ("[", "]", ShapeKind::Process),
            ("(", ")", ShapeKind::Terminator),
            ("{", "}", ShapeKind::Decision),
            (">", "]", ShapeKind::Note),
        ];
        let Some((open, close, shape)) = patterns
            .iter()
            .find(|(a, b, _)| rest.starts_with(a) && rest.ends_with(b))
        else {
            return Err(error(format!(
                "Unsupported or incomplete Mermaid node: {value}"
            )));
        };
        if rest.len() < open.len() + close.len() {
            return Err(error("Empty Mermaid node delimiters."));
        }
        label = unquote(&rest[open.len()..rest.len() - close.len()]);
        kind = *shape;
    }
    draft.node(id, &label, kind)?;
    if !rest.is_empty() {
        let item = draft.items.iter_mut().find(|i| i.key == id).unwrap();
        item.label = label;
        item.kind = kind;
        item.data.insert("mermaid_shape".into(), rest.into());
    }
    if let Some(classes) = classes {
        draft
            .items
            .iter_mut()
            .find(|i| i.key == id)
            .unwrap()
            .data
            .insert("mermaid_class".into(), classes);
    }
    Ok(id.into())
}

fn metadata(draft: &mut Draft, line: &str) -> Result<bool> {
    if [
        "accTitle",
        "accDescr",
        "title",
        "style",
        "classDef",
        "class ",
        "linkStyle",
        "click",
        "direction",
    ]
    .iter()
    .any(|p| line.starts_with(p))
    {
        syntax::record(draft, line, "Mermaid directive")?;
        draft.warn("Mermaid directives are retained as editable notes; styling and actions are not applied.");
        return Ok(true);
    }
    Ok(false)
}

fn flow(draft: &mut Draft, lines: &[&str], state: bool) -> Result<()> {
    let arrow = Regex::new(
        r"^(?:(?:--|==|-\.)\s+(.+?)\s+(?:-->|==>|\.->)|<\|--|--\|>|<[-=.]{2,}>?|[-=.]{2,}[>xo]?)",
    )
    .unwrap();
    let alias = Regex::new(r#"^state\s+"([^"]+)"\s+as\s+(\S+)$"#).unwrap();
    let mut nesting = 0usize;
    for &line in lines {
        if metadata(draft, line)? {
            continue;
        }
        if line.starts_with("subgraph ")
            || (state && line.starts_with("state ") && line.ends_with('{'))
        {
            syntax::record(draft, line, "group")?;
            nesting += 1;
            draft.warn("Subgraphs and composite states are flattened; their declarations are retained as notes.");
            continue;
        }
        if line == "end" || line == "}" {
            nesting = nesting
                .checked_sub(1)
                .ok_or_else(|| error("Unexpected diagram group end."))?;
            continue;
        }
        if state && let Some(m) = alias.captures(line) {
            draft.node(&m[2], &m[1], ShapeKind::Process)?;
            continue;
        }
        let (line, transition) = if state {
            syntax::pair(line, ':')?.unwrap_or((line, ""))
        } else {
            (line, "")
        };
        let positions = syntax::outside(line)?;
        let mut segments = Vec::new();
        let mut edges = Vec::new();
        let mut start = 0;
        let mut skip = 0;
        let mut braces = 0usize;
        for i in positions {
            if i < skip {
                continue;
            }
            if line[i..].starts_with('{') {
                braces += 1;
                continue;
            }
            if line[i..].starts_with('}') {
                braces = braces.saturating_sub(1);
                continue;
            }
            if braces > 0 {
                continue;
            }
            if let Some(captures) = arrow.captures(&line[i..]) {
                let m = captures.get(0).unwrap();
                segments.push(line[start..i].trim());
                let op = m.as_str();
                start = i + m.end();
                let rest = line[start..].trim_start();
                let mut label = captures
                    .get(1)
                    .map_or_else(|| transition.trim().to_string(), |s| unquote(s.as_str()));
                if let Some(after) = rest.strip_prefix('|') {
                    let end = after
                        .find('|')
                        .ok_or_else(|| error("Unclosed Mermaid edge label."))?;
                    label = unquote(&after[..end]);
                    start = line.len() - after[end + 1..].len();
                }
                edges.push((
                    label,
                    op.contains('>') || op.ends_with('x') || op.ends_with('o'),
                    op.starts_with('<') && !op.ends_with('>'),
                    op.starts_with('<') && op.ends_with('>'),
                ));
                skip = start;
            }
        }
        segments.push(line[start..].trim());
        if edges.is_empty() && state && !transition.is_empty() {
            draft.node(line.trim(), &unquote(transition), ShapeKind::Process)?;
            continue;
        }
        let mut groups = Vec::new();
        for segment in segments {
            let ids = syntax::split(segment, '&')?
                .into_iter()
                .map(|s| node(draft, s))
                .collect::<Result<Vec<_>>>()?;
            groups.push(ids);
        }
        for (i, (label, arrow, reverse, both)) in edges.into_iter().enumerate() {
            for a in &groups[i] {
                for b in &groups[i + 1] {
                    if reverse {
                        draft.link(b, a, &label, true)?;
                    } else {
                        draft.link(a, b, &label, arrow)?;
                        draft.links.last_mut().unwrap().arrow_start = both;
                    }
                }
            }
        }
    }
    if nesting != 0 {
        return Err(error("Unclosed diagram group."));
    }
    Ok(())
}

fn sequence(draft: &mut Draft, lines: &[&str]) -> Result<()> {
    let participant = Regex::new(r"^(?:participant|actor)\s+(\w+)(?:\s+as\s+(.+))?$").unwrap();
    let message =
        Regex::new(r"^(\w+)\s*(<<--?>>|--?>>?|--?[x)])\s*[+-]?(\w+)\s*:\s*(.*)$").unwrap();
    draft.layout = Layout::Horizontal;
    draft.warn("Sequence participants and ordered messages become a graph; lifelines, activation and timing use data notes.");
    for &line in lines {
        if let Some(m) = participant.captures(line) {
            draft.node(
                &m[1],
                &unquote(m.get(2).map_or(&m[1], |s| s.as_str())),
                ShapeKind::Process,
            )?;
        } else if let Some(m) = message.captures(line) {
            draft.node(&m[1], &m[1], ShapeKind::Process)?;
            draft.node(&m[3], &m[3], ShapeKind::Process)?;
            draft.link(&m[1], &m[3], &unquote(&m[4]), true)?;
        } else {
            syntax::record(draft, line, "sequence directive")?;
        }
    }
    Ok(())
}

fn entities(draft: &mut Draft, lines: &[&str], er: bool) -> Result<()> {
    let relationship = Regex::new(r#"^([\w~.-]+)\s*(?:"([^"]*)"\s*)?([|o}{*<.>=-]{2,})\s*(?:"([^"]*)"\s*)?([\w~.-]+)\s*(?::\s*(.*))?$"#).unwrap();
    let kind = if er {
        ShapeKind::Entity
    } else {
        ShapeKind::Class
    };
    let mut block: Option<String> = None;
    for &line in lines {
        if line == "}" {
            if block.take().is_none() {
                return Err(error("Unexpected class/entity block end."));
            }
            continue;
        }
        if let Some(id) = &block {
            let item = draft.items.iter_mut().find(|i| &i.key == id).unwrap();
            item.label.push('\n');
            item.label.push_str(line);
            continue;
        }
        if let Some(m) = relationship.captures(line) {
            draft.node(&m[1], &m[1], kind)?;
            draft.node(&m[5], &m[5], kind)?;
            let label = unquote(m.get(6).map_or("", |s| s.as_str()));
            let relation = format!(
                "{}{}{}",
                m.get(2).map_or("", |s| s.as_str()),
                &m[3],
                m.get(4).map_or("", |s| s.as_str())
            );
            let label = if label.is_empty() {
                relation
            } else {
                format!("{label} ({relation})")
            };
            draft.link(&m[1], &m[5], &label, !er)?;
        } else if let Some(declaration) = line.strip_suffix('{') {
            let id = declaration
                .trim()
                .strip_prefix("class ")
                .unwrap_or(declaration.trim());
            draft.node(id, id, kind)?;
            block = Some(id.into());
        } else if let Some(id) = line.strip_prefix("class ") {
            draft.node(id.trim(), id.trim(), kind)?;
        } else if !er && let Some((id, member)) = syntax::pair(line, ':')? {
            draft.node(id.trim(), id.trim(), kind)?;
            let item = draft.items.iter_mut().find(|i| i.key == id.trim()).unwrap();
            item.label.push('\n');
            item.label.push_str(member.trim());
        } else {
            syntax::record(draft, line, "class/entity directive")?;
        }
    }
    if block.is_some() {
        return Err(error("Unclosed class/entity block."));
    }
    draft.warn("Class/ER relationships retain their notation in connection labels; specialized UML and cardinality markers use standard connectors.");
    Ok(())
}

fn mindmap(draft: &mut Draft, text: &str) -> Result<()> {
    let mut parents: Vec<(usize, String)> = Vec::new();
    for line in text
        .lines()
        .skip_while(|l| l.trim().is_empty())
        .skip(1)
        .filter(|l| !l.trim().is_empty())
    {
        let indent = line.len() - line.trim_start().len();
        let value = line.trim();
        let key = format!("mindmap_{}", draft.items.len());
        draft.node(&key, &unquote(value), ShapeKind::Process)?;
        while parents.last().is_some_and(|(n, _)| *n >= indent) {
            parents.pop();
        }
        if let Some((_, parent)) = parents.last() {
            draft.link(parent, &key, "", false)?;
        }
        parents.push((indent, key));
    }
    draft.warn("Mind map indentation becomes editable parent/child connections; labels retain source shape notation.");
    Ok(())
}

fn sankey(draft: &mut Draft, lines: &[&str]) -> Result<()> {
    for row in super::csv_rows(&lines.join("\n"))? {
        if row.len() != 3
            || !row[2]
                .parse::<f64>()
                .is_ok_and(|v| v.is_finite() && v >= 0.)
        {
            return Err(error("Sankey rows need source,target,nonnegative value."));
        }
        draft.node(&row[0], &row[0], ShapeKind::Process)?;
        draft.node(&row[1], &row[1], ShapeKind::Process)?;
        draft.link(&row[0], &row[1], &row[2], true)?;
    }
    draft.warn(
        "Sankey values are retained in connection labels; band widths use standard connectors.",
    );
    Ok(())
}
