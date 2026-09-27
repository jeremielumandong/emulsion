//! CSV editing for native charts and tables.
use crate::{IoError, Result};
pub fn rows(csv: &str) -> Result<Vec<Vec<String>>> {
    if csv.len() > 512 * 1024 {
        return Err(IoError::Manifest(
            "Chart data must be no larger than 512 KB.".into(),
        ));
    }
    // Preserve explicitly empty table rows. Diagram CSV deliberately skips them,
    // which would silently remove cells when a table is reopened for editing.
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
        if records.len() > 51 || record.len() > 9 || field.len() > 4000 {
            return Err(error(
                "Use at most 50 data rows, 9 columns and 1000 characters per cell.",
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
    if records.len() > 51
        || records
            .iter()
            .any(|row| row.len() > 9 || row.iter().any(|cell| cell.chars().count() > 1000))
    {
        return Err(error(
            "Use at most 50 data rows, 9 columns and 1000 characters per cell.",
        ));
    }
    Ok(records)
}
pub fn to_csv(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|cell| format!("\"{}\"", cell.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{
        Document, Editor, NodeKind,
        design_charts::{self, Chart, Kind},
        project::{ProjectEditor, ProjectKind},
    };
    use std::io::Cursor;

    #[test]
    fn quoted_multiline_csv_preserves_chart_data() {
        let data = vec![
            vec!["Category".into(), "Revenue, USD".into()],
            vec!["A \"quoted\"\nname".into(), "0.25".into()],
            vec!["日本語".into(), "-0.5".into()],
            vec!["".into(), "".into()],
        ];
        assert_eq!(rows(&to_csv(&data)).unwrap(), data);
        assert!(rows("Category,Value\n\"unclosed,1").is_err());
        assert!(rows("Category,Value\n\"closed\"extra,1").is_err());
        assert!(rows(&"x".repeat(512 * 1024 + 1)).is_err());
    }

    #[test]
    fn charts_roundtrip_native_data_and_export_vector_artwork() {
        for kind in Kind::ALL {
            let mut editor = Editor::new(Document::new(360, 260), None);
            let mut chart = Chart::example(kind);
            chart.size = (320., 220.);
            chart.rows[1][0] = "Edited label".into();
            let id = design_charts::apply(&mut editor, None, chart.clone(), (12., 15.)).unwrap();
            let project = ProjectEditor::new_project(ProjectKind::Design, editor.doc.clone())
                .unwrap()
                .snapshot()
                .unwrap();
            let mut archive = Cursor::new(Vec::new());
            crate::project::write_to(&project, &mut archive).unwrap();
            let reopened = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
            let doc = &reopened.pages[0].doc;
            assert_eq!(doc, &editor.doc);
            assert_eq!(doc.design.charts[&id], chart);
            assert!(doc.nodes.iter().all(|node| matches!(
                node.kind,
                NodeKind::Group { .. } | NodeKind::Path { .. } | NodeKind::Text { .. }
            )));
            let (svg, flattened) = crate::project_export::svg(doc).unwrap();
            assert!(!flattened);
            let svg = String::from_utf8(svg).unwrap();
            assert!(!svg.contains("<image"));
            resvg::usvg::Tree::from_str(&svg, &Default::default()).unwrap();
            let mut reopened = Editor::new(doc.clone(), None);
            chart.rows[1][1] = "80".into();
            design_charts::apply(&mut reopened, Some(id), chart.clone(), (12., 15.)).unwrap();
            assert_eq!(reopened.doc.design.charts[&id], chart);
        }
    }
}
