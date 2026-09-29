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
            chart.rows[1][0] = if kind == Kind::Scatter {
                "42"
            } else {
                "Edited label"
            }
            .into();
            chart.y_axis.min = Some(0.);
            chart.y_axis.max = Some(120.);
            chart.y_axis.label = "Measured value".into();
            if kind == Kind::Table {
                chart.merges.push(design_charts::Merge {
                    row: 0,
                    column: 0,
                    rows: 1,
                    columns: 2,
                });
            }
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

#[cfg(test)]
mod text_formatting_tests {
    use emulsion_core::{
        Command, Document, Editor, Node,
        command::Slot,
        project::{ProjectEditor, ProjectKind},
        text::{ListStyle, TextSpec, apply_list},
    };
    use std::io::Cursor;
    #[test]
    fn decorated_lists_roundtrip_and_export_vector_svg_and_pdf() {
        let mut editor = Editor::new(Document::new(400, 300), None);
        let mut spec = TextSpec {
            text: "Native list\nSecond item".into(),
            size: 24.,
            x: 20.,
            y: 20.,
            width: Some(240.),
            height: Some(120.),
            underline: true,
            ..Default::default()
        };
        spec = apply_list(&spec, ListStyle::Bullet).unwrap();
        let start = spec.text.find("Second").unwrap();
        spec.apply_style(start..start + 6, |s| {
            s.strikethrough = true;
            s.color = [220, 30, 60, 255];
        });
        editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(0, "Decorated list", spec, 400, 300)),
                slot: Slot::TOP,
            })
            .unwrap();
        let project = ProjectEditor::new_project(ProjectKind::Design, editor.doc.clone())
            .unwrap()
            .snapshot()
            .unwrap();
        let mut archive = Cursor::new(Vec::new());
        crate::project::write_to(&project, &mut archive).unwrap();
        let read = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
        assert_eq!(read.pages[0].doc, editor.doc);
        let (svg, flat) = crate::project_export::svg(&editor.doc).unwrap();
        assert!(!flat);
        let source = String::from_utf8(svg).unwrap();
        assert!(!source.contains("<image"));
        assert!(source.contains("text-frame-"));
        let mut plain = editor.doc.clone();
        let emulsion_core::NodeKind::Text { spec, .. } = &plain.nodes[0].kind else {
            panic!()
        };
        let mut spec = (**spec).clone();
        spec.underline = false;
        spec.strikethrough = false;
        for run in &mut spec.runs {
            run.style.underline = false;
            run.style.strikethrough = false;
        }
        Command::SetText {
            id: plain.nodes[0].id,
            spec: Box::new(spec),
        }
        .apply(&mut plain)
        .unwrap();
        let plain_svg = String::from_utf8(crate::project_export::svg(&plain).unwrap().0).unwrap();
        assert!(
            source.matches("<path").count() > plain_svg.matches("<path").count(),
            "decoration contours are exported"
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("native-list.pdf");
        let report = crate::project_export::write(
            &project,
            &[project.pages[0].meta.id],
            crate::project_export::Format::Pdf,
            false,
            &path,
        )
        .unwrap();
        assert!(report.rasterized_pages.is_empty());
        assert!(std::fs::read(path).unwrap().starts_with(b"%PDF-"));
    }
}
