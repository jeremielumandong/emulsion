//! Local CSV-to-design generation. Data is substituted only into authored text.
use crate::{IoError, Result};
use emulsion_core::{
    Command, Document, NodeKind,
    graph::Graph,
    project::{MAX_PAGES, MAX_PROJECT_PIXELS, PageMeta, Project, ProjectKind, ProjectPage},
};
use std::{
    collections::{BTreeSet, HashMap},
    ops::Range,
};

pub const MAX_CSV_BYTES: usize = 2 * 1024 * 1024;
fn error(message: impl Into<String>) -> IoError {
    IoError::Manifest(message.into())
}

fn tokens(text: &str) -> Result<Vec<(Range<usize>, String)>> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(start) = text[cursor..].find("{{").map(|i| i + cursor) {
        let end = text[start + 2..]
            .find("}}")
            .map(|i| i + start + 2)
            .ok_or_else(|| error("Close each data field with }}."))?;
        let key = text[start + 2..end].trim();
        if key.is_empty()
            || key.len() > 200
            || key.contains(['{', '}'])
            || key.chars().any(char::is_control)
        {
            return Err(error(
                "Data fields need a name of 1–200 bytes, such as {{name}}.",
            ));
        }
        out.push((start..end + 2, key.into()));
        cursor = end + 2;
    }
    Ok(out)
}

pub fn fields(doc: &Document) -> Result<Vec<String>> {
    let mut fields = BTreeSet::new();
    for node in &doc.nodes {
        if let NodeKind::Text { spec, .. } = &node.kind {
            fields.extend(tokens(&spec.text)?.into_iter().map(|(_, key)| key));
        }
    }
    if fields.is_empty() {
        return Err(error(
            "Add a field such as {{name}} to a text object first.",
        ));
    }
    if fields.len() > 64 {
        return Err(error("Use at most 64 data fields per design."));
    }
    Ok(fields.into_iter().collect())
}

fn csv_rows(csv: &str) -> Result<Vec<Vec<String>>> {
    // Explicitly empty values still generate a page; blank physical lines do not.
    let error = |message: &str| IoError::Manifest(message.into());
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut started = false;
    let mut chars = csv.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' if field.is_empty() && !closed => {
                    quoted = true;
                    started = true;
                }
                ',' => {
                    record.push(std::mem::take(&mut field));
                    closed = false;
                    started = true;
                }
                '\r' | '\n' => {
                    if c == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    if started {
                        record.push(std::mem::take(&mut field));
                        records.push(std::mem::take(&mut record));
                    }
                    started = false;
                    closed = false;
                }
                _ if closed => {
                    return Err(error("Unexpected characters after a quoted CSV field."));
                }
                '"' => return Err(error("Quote inside an unquoted CSV field.")),
                _ => {
                    field.push(c);
                    started = true;
                }
            }
        }
        if records.len() > MAX_PAGES + 1 || record.len() > 64 || field.len() > 4096 {
            return Err(error(
                "Use at most 100 data rows, 64 columns and 4096 bytes per value.",
            ));
        }
    }
    if quoted {
        return Err(error("Unterminated quoted CSV field."));
    }
    if started {
        record.push(field);
        records.push(record);
    }
    if records.len() > MAX_PAGES + 1
        || records
            .iter()
            .any(|row| row.len() > 64 || row.iter().any(|cell| cell.len() > 4096))
    {
        return Err(error(
            "Use at most 100 data rows, 64 columns and 4096 bytes per value.",
        ));
    }
    Ok(records)
}
pub fn generate(source: &Document, csv: &str, max_pages: usize) -> Result<Project> {
    if csv.len() > MAX_CSV_BYTES {
        return Err(error("CSV files must be no larger than 2 MB."));
    }
    source.validate()?;
    let required = fields(source)?;
    let rows = csv_rows(csv)?;
    let header = rows
        .first()
        .ok_or_else(|| error("CSV needs column names and at least one data row."))?;
    let headers: HashMap<_, _> = header
        .iter()
        .enumerate()
        .map(|(i, h)| (h.trim(), i))
        .collect();
    if header.is_empty()
        || header.len() > 64
        || headers.len() != header.len()
        || headers.contains_key("")
    {
        return Err(error("CSV needs 1–64 unique, nonempty column names."));
    }
    for name in required {
        if !headers.contains_key(name.as_str()) {
            return Err(error(format!("CSV is missing the {name} column.")));
        }
    }
    let count = rows.len() - 1;
    if count == 0 || count > max_pages.min(MAX_PAGES) {
        return Err(error(format!(
            "Choose 1–{} data rows to fit this project.",
            max_pages.min(MAX_PAGES)
        )));
    }
    if count as u64 * u64::from(source.width) * u64::from(source.height) > MAX_PROJECT_PIXELS {
        return Err(error("Generated designs exceed the total page area limit."));
    }
    let templates: Vec<_> = source
        .nodes
        .iter()
        .filter_map(|node| {
            if let NodeKind::Text { spec, .. } = &node.kind {
                Some((node.id, spec.clone()))
            } else {
                None
            }
        })
        .map(|(id, spec)| tokens(&spec.text).map(|fields| (id, spec, fields)))
        .collect::<Result<_>>()?;
    let mut pages = Vec::with_capacity(count);
    for (index, row) in rows.iter().skip(1).enumerate() {
        if row.len() != header.len() || row.iter().any(|v| v.len() > 4096) {
            return Err(error(format!(
                "Row {} must have {} columns with at most 4096 bytes per value.",
                index + 2,
                header.len()
            )));
        }
        let mut doc = source.clone();
        for (id, original, fields) in &templates {
            if fields.is_empty() {
                continue;
            }
            let removed: usize = fields
                .iter()
                .map(|(range, _)| original.text[range.clone()].chars().count())
                .sum();
            let inserted: usize = fields
                .iter()
                .map(|(_, key)| row[headers[key.as_str()]].chars().count())
                .sum();
            if original.text.chars().count() - removed + inserted > emulsion_core::text::MAX_CHARS {
                return Err(error(format!(
                    "Row {} exceeds the text length limit.",
                    index + 2
                )));
            }
            let mut spec = (**original).clone();
            for (range, key) in fields.iter().rev() {
                let replacement = &row[headers[key.as_str()]];
                let style = spec.style_at(range.start);
                spec.replace_range(range.clone(), replacement);
                spec.apply_style(range.start..range.start + replacement.len(), |s| {
                    *s = style.clone()
                });
            }
            if spec.text.chars().count() > emulsion_core::text::MAX_CHARS {
                return Err(error(format!(
                    "Row {} exceeds the text length limit.",
                    index + 2
                )));
            }
            Command::SetText {
                id: *id,
                spec: Box::new(spec),
            }
            .apply(&mut doc)
            .map_err(|e| error(e.to_string()))?;
        }
        let id = index as u64 + 1;
        let name = format!("Data {id}");
        pages.push(ProjectPage {
            meta: PageMeta {
                id,
                name: name.clone(),
                bleed_mm: 0.,
            },
            graph: Graph::new(doc.clone(), name),
            doc,
        });
    }
    let project = Project {
        kind: ProjectKind::Design,
        pages,
        active: 1,
        next_page_id: count as u64 + 1,
    };
    project.validate().map_err(error)?;
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Node, command::Slot, project::ProjectEditor, text::TextSpec};
    fn source() -> (Document, u64) {
        let mut doc = Document::new(300, 200);
        let mut spec = TextSpec {
            text: "Hello {{name}} — {{offer}}".into(),
            size: 20.,
            ..Default::default()
        };
        spec.apply_style(6..14, |s| s.bold = true);
        let id = Command::AddNode {
            node: Box::new(Node::text(0, "Greeting", spec, 300, 200)),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        (doc, id)
    }
    #[test]
    fn bulk_pages_preserve_source_rich_text_and_undo_as_one_batch() {
        let (doc, id) = source();
        let batch = generate(
            &doc,
            "name,offer\r\n\"Zoë, 李\",\"10%\nToday\"\r\nSam,\"{{literal}}\"",
            99,
        )
        .unwrap();
        let NodeKind::Text { spec, .. } = &batch.pages[0].doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text, "Hello Zoë, 李 — 10%\nToday");
        assert!(spec.style_at(6).bold);
        let NodeKind::Text { spec, .. } = &batch.pages[1].doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text, "Hello Sam — {{literal}}");
        let mut editor = ProjectEditor::new_project(ProjectKind::Design, doc.clone()).unwrap();
        editor.import_pages(batch).unwrap();
        assert_eq!(editor.page_list().len(), 3);
        editor.undo();
        assert_eq!(editor.page_list().len(), 1);
        assert_eq!(editor.doc, doc);
        editor.redo();
        assert_eq!(editor.page_list().len(), 3);
    }
    #[test]
    fn invalid_data_or_protected_fields_reject_the_whole_batch() {
        let (mut doc, id) = source();
        for csv in [
            "name,name\na,b",
            "name\na",
            "name,offer\na",
            "name,offer\n\"broken",
        ] {
            assert!(generate(&doc, csv, 99).is_err(), "{csv}");
        }
        assert!(generate(&doc, "name,offer\na,b\nc,d", 1).is_err());
        Command::SetLocked { id, locked: true }
            .apply(&mut doc)
            .unwrap();
        let before = doc.clone();
        assert!(generate(&doc, "name,offer\na,b", 99).is_err());
        assert_eq!(doc, before);
    }
    #[test]
    fn empty_records_generate_pages_and_oversize_expansion_is_rejected() {
        let (doc, id) = source();
        for csv in ["name,offer\n,\n", "name,offer\n\"\",\"\"\n"] {
            let batch = generate(&doc, csv, 99).unwrap();
            assert_eq!(batch.pages.len(), 1);
            let NodeKind::Text { spec, .. } = &batch.pages[0].doc.node(id).unwrap().kind else {
                panic!()
            };
            assert_eq!(spec.text, "Hello  — ");
        }
        let mut expanded = doc.clone();
        let NodeKind::Text { spec, .. } = &doc.node(id).unwrap().kind else {
            panic!()
        };
        let mut spec = (**spec).clone();
        spec.text = "{{name}} ".repeat(1000);
        Command::SetText {
            id,
            spec: Box::new(spec),
        }
        .apply(&mut expanded)
        .unwrap();
        let before = expanded.clone();
        assert!(generate(&expanded, &format!("name\n{}", "x".repeat(4096)), 99).is_err());
        assert_eq!(expanded, before);
        assert!(
            generate(
                &doc,
                &format!("name,offer\n\"{}\",ok", "x".repeat(4097)),
                99
            )
            .is_err()
        );
        assert!(generate(&doc, "name,offer\n,", 0).is_err());
        assert!(generate(&doc, &"x".repeat(MAX_CSV_BYTES + 1), 99).is_err());
    }
}
