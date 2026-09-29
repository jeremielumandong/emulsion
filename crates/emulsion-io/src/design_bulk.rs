//! Local CSV record sets with editable text and image bindings.
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

fn page_fields(doc: &Document) -> Result<BTreeSet<String>> {
    let mut fields = BTreeSet::new();
    for node in &doc.nodes {
        if let Some(binding) = doc.design.data_bindings.get(&node.id) {
            fields.insert(binding.column().to_string());
        } else if let NodeKind::Text { spec, .. } = &node.kind {
            fields.extend(tokens(&spec.text)?.into_iter().map(|(_, key)| key));
        }
    }
    if fields.len() > 64 {
        return Err(error("Use at most 64 data columns per design."));
    }
    Ok(fields)
}
pub fn fields(doc: &Document) -> Result<Vec<String>> {
    let fields = page_fields(doc)?;
    if fields.is_empty() {
        return Err(error(
            "Add {{column}} text or bind a text/image object to a CSV column first.",
        ));
    }
    Ok(fields.into_iter().collect())
}
pub fn project_fields(project: &Project, pages: &[u64]) -> Result<Vec<String>> {
    let mut fields = BTreeSet::new();
    for id in pages {
        let page = project
            .pages
            .iter()
            .find(|p| p.meta.id == *id)
            .ok_or_else(|| error("Unknown template page"))?;
        fields.extend(page_fields(&page.doc)?);
    }
    if fields.is_empty() || fields.len() > 64 {
        return Err(error(
            "Template pages need 1–64 CSV fields or saved data bindings.",
        ));
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
/// Generate one page per record, preserving the legacy inline-field workflow.
pub fn generate(source: &Document, csv: &str, max_pages: usize) -> Result<Project> {
    generate_sources(&[(1, "Data".into(), source)], csv, max_pages, None)
}
/// Each CSV record produces an ordered set of selected template pages.
pub fn generate_pages(
    source: &Project,
    template_pages: &[u64],
    csv: &str,
    max_pages: usize,
    base_dir: Option<&std::path::Path>,
) -> Result<Project> {
    source.validate().map_err(error)?;
    if template_pages.is_empty()
        || template_pages
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != template_pages.len()
    {
        return Err(error("Choose unique template pages."));
    }
    let sources = template_pages
        .iter()
        .map(|id| {
            source
                .pages
                .iter()
                .find(|p| p.meta.id == *id)
                .map(|p| (*id, p.meta.name.clone(), &p.doc))
                .ok_or_else(|| error("Unknown template page"))
        })
        .collect::<Result<Vec<_>>>()?;
    generate_sources(&sources, csv, max_pages, base_dir)
}
fn generate_sources(
    sources: &[(u64, String, &Document)],
    csv: &str,
    max_pages: usize,
    base_dir: Option<&std::path::Path>,
) -> Result<Project> {
    if csv.len() > MAX_CSV_BYTES {
        return Err(error("CSV files must be no larger than 2 MB."));
    }
    let mut required = BTreeSet::new();
    for (_, _, doc) in sources {
        doc.validate()?;
        required.extend(page_fields(doc)?);
    }
    if required.is_empty() || required.len() > 64 {
        return Err(error(
            "Template pages need 1–64 CSV fields or saved data bindings.",
        ));
    }
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
    let records = rows.len() - 1;
    let count = records.saturating_mul(sources.len());
    if records == 0 || count > max_pages.min(MAX_PAGES) {
        return Err(error(format!(
            "The record sets need {count} pages; {} pages fit this project.",
            max_pages.min(MAX_PAGES)
        )));
    }
    let area = sources
        .iter()
        .map(|(_, _, doc)| u64::from(doc.width) * u64::from(doc.height))
        .sum::<u64>();
    if records as u64 * area > MAX_PROJECT_PIXELS {
        return Err(error("Generated designs exceed the total page area limit."));
    }
    let mut pages = Vec::with_capacity(count);
    let mut images = HashMap::new();
    let mut pixels = 0u64;
    for (record, row) in rows.iter().skip(1).enumerate() {
        if row.len() != header.len() {
            return Err(error(format!(
                "CSV row {} must have {} columns.",
                record + 2,
                header.len()
            )));
        }
        let remap = sources
            .iter()
            .enumerate()
            .map(|(offset, (id, _, _))| (*id, (record * sources.len() + offset + 1) as u64))
            .collect();
        for (_, name, source) in sources {
            let mut doc = (*source).clone();
            for node in &source.nodes {
                if let Some(binding) = source.design.data_bindings.get(&node.id) {
                    let value = &row[headers[binding.column()]];
                    match binding {
                        emulsion_core::design_data::Binding::Text { .. } => {
                            let NodeKind::Text { spec, .. } = &node.kind else {
                                unreachable!()
                            };
                            let mut spec = (**spec).clone();
                            let style = spec.style_at(0);
                            spec.replace_range(0..spec.text.len(), value);
                            spec.apply_style(0..spec.text.len(), |s| *s = style.clone());
                            Command::SetText {
                                id: node.id,
                                spec: Box::new(spec),
                            }
                            .apply(&mut doc)
                            .map_err(|e| error(format!("Row {}: {e}", record + 2)))?;
                        }
                        emulsion_core::design_data::Binding::Image { fit, focus, .. } => {
                            replace_image(
                                &mut doc,
                                node.id,
                                value,
                                *fit,
                                *focus,
                                base_dir,
                                &mut images,
                                &mut pixels,
                            )
                            .map_err(|e| {
                                error(format!(
                                    "Row {}, column {}: {e}",
                                    record + 2,
                                    binding.column()
                                ))
                            })?;
                        }
                    }
                } else if let NodeKind::Text { spec: original, .. } = &node.kind {
                    let fields = tokens(&original.text)?;
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
                    if original.text.chars().count() - removed + inserted
                        > emulsion_core::text::MAX_CHARS
                    {
                        return Err(error(format!(
                            "Row {} exceeds the text length limit.",
                            record + 2
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
                    Command::SetText {
                        id: node.id,
                        spec: Box::new(spec),
                    }
                    .apply(&mut doc)
                    .map_err(|e| error(e.to_string()))?;
                }
            }
            doc.design.remap_pages(&remap);
            let id = pages.len() as u64 + 1;
            let name = if sources.len() == 1 {
                format!("Data {}", record + 1)
            } else {
                format!(
                    "{} · {}",
                    name.chars().take(175).collect::<String>(),
                    record + 1
                )
            };
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
#[allow(clippy::too_many_arguments)] // Binding geometry plus the shared bounded decode cache.
fn replace_image(
    doc: &mut Document,
    id: u64,
    value: &str,
    fit: emulsion_core::design_data::Fit,
    focus: [f64; 2],
    base_dir: Option<&std::path::Path>,
    images: &mut HashMap<std::path::PathBuf, std::sync::Arc<emulsion_raster::Raster>>,
    pixels: &mut u64,
) -> Result<()> {
    use std::{path::Path, sync::Arc};
    if value.trim().is_empty() || value.contains("://") {
        return Err(error("Image cells must contain a local image path."));
    }
    let path = Path::new(value.trim());
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.unwrap_or_else(|| Path::new(".")).join(path)
    }
    .canonicalize()?;
    if !images.contains_key(&path) {
        let decoded = crate::import::decode(&path)?;
        *pixels += u64::from(decoded.raster.width()) * u64::from(decoded.raster.height());
        if *pixels > 32_000_000 {
            return Err(error(
                "Unique bulk images exceed 32 million decoded pixels. Split the batch.",
            ));
        }
        images.insert(path.clone(), Arc::new(decoded.raster));
    }
    let raster = images[&path].clone();
    let node = doc.node(id).ok_or_else(|| error("Missing image binding"))?;
    let NodeKind::Raster {
        raster: old,
        placement: previous,
    } = &node.kind
    else {
        return Err(error("Bind a raster image."));
    };
    let (w, h) = (
        f64::from(old.width()) * previous.scale_x,
        f64::from(old.height()) * previous.scale_y,
    );
    let (rw, rh) = (f64::from(raster.width()), f64::from(raster.height()));
    let (sx, sy) = (w / rw, h / rh);
    let (sx, sy) = match fit {
        emulsion_core::design_data::Fit::Cover => (sx.max(sy), sx.max(sy)),
        emulsion_core::design_data::Fit::Contain => (sx.min(sy), sx.min(sy)),
        emulsion_core::design_data::Fit::Stretch => (sx, sy),
    };
    let mut placement = *previous;
    placement.scale_x = sx;
    placement.scale_y = sy;
    let cover = fit == emulsion_core::design_data::Fit::Cover;
    let adjusted_focus = [
        if previous.flip_x {
            1. - focus[0]
        } else {
            focus[0]
        },
        if previous.flip_y {
            1. - focus[1]
        } else {
            focus[1]
        },
    ];
    let offset = |span: f64, size: f64, f: f64| {
        if cover {
            (span / 2. - size * f).clamp((span - size).min(0.), 0.)
        } else {
            (span - size) / 2.
        }
    };
    let (ox, oy) = (
        offset(w, rw * sx, adjusted_focus[0]),
        offset(h, rh * sy, adjusted_focus[1]),
    );
    let delta = glam::DAffine2::from_angle(previous.rotation.to_radians()).transform_vector2(
        glam::dvec2(ox + (rw * sx - w) / 2., oy + (rh * sy - h) / 2.),
    );
    placement.x = previous.x + w / 2. + delta.x - rw * sx / 2.;
    placement.y = previous.y + h / 2. + delta.y - rh * sy / 2.;
    let mut mask = node
        .mask
        .as_ref()
        .map(|m| {
            emulsion_core::geometry::resize_layer_mask(m, raster.width(), raster.height())
                .map(Arc::new)
                .map_err(error)
        })
        .transpose()?;
    if cover
        && emulsion_core::design::frame_parts(doc, id).is_none()
        && (rw * sx > w + 1e-8 || rh * sy > h + 1e-8)
    {
        let mut left = -ox / sx;
        let mut top = -oy / sy;
        let cw = w / sx;
        let ch = h / sy;
        if previous.flip_x {
            left = rw - left - cw;
        }
        if previous.flip_y {
            top = rh - top - ch;
        }
        mask = Some(Arc::new(emulsion_raster::Mask::from_fn(
            raster.width(),
            raster.height(),
            0,
            |x, y| {
                let coverage = ((f64::from(x) + 1.).min(left + cw) - f64::from(x).max(left))
                    .clamp(0., 1.)
                    * ((f64::from(y) + 1.).min(top + ch) - f64::from(y).max(top)).clamp(0., 1.);
                (f64::from(mask.as_ref().map_or(255, |m| m.get(x, y))) * coverage).round() as u8
            },
        )));
    }
    Command::ReplaceContent {
        id,
        raster,
        mask,
        placement,
        label: "Replace bound image".into(),
    }
    .apply(doc)
    .map_err(|e| error(e.to_string()))?;
    if emulsion_core::design::frame_parts(doc, id).is_some() {
        emulsion_core::design::media::fit_frame_image(doc, id, fit.native(), focus)
            .map_err(error)?
            .apply(doc)
            .map_err(|e| error(e.to_string()))?;
    }
    Ok(())
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
    #[test]
    fn data_record_sets_remap_links_before_and_after_import_and_roundtrip_bindings() {
        use emulsion_core::{design_data::Binding, design_interactions::Action};
        let (mut first, id) = source();
        first.design.data_bindings.insert(
            id,
            Binding::Text {
                column: "name".into(),
            },
        );
        first
            .design
            .interactions
            .insert(id, vec![Action::Slide { page: 2 }]);
        let second = Document::new(300, 200);
        let mut templates = ProjectEditor::new_project(ProjectKind::Design, first.clone()).unwrap();
        templates.add_page(second, "Back".into(), 0.).unwrap();
        let snapshot = templates.snapshot().unwrap();
        let generated = generate_pages(&snapshot, &[1, 2], "name\nAlice\nBob", 98, None).unwrap();
        assert_eq!(generated.pages.len(), 4);
        for (index, name) in [(0, "Alice"), (2, "Bob")] {
            let doc = &generated.pages[index].doc;
            let NodeKind::Text { spec, .. } = &doc.node(id).unwrap().kind else {
                panic!()
            };
            assert_eq!(spec.text, name);
            assert_eq!(
                doc.design.interactions[&id],
                vec![Action::Slide {
                    page: index as u64 + 2
                }]
            );
            assert_eq!(
                doc.design.data_bindings[&id],
                Binding::Text {
                    column: "name".into()
                }
            );
        }
        let mut target =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(300, 200)).unwrap();
        let imported = target.import_pages(generated).unwrap();
        for index in [0, 2] {
            let page = target.page(imported[index]).unwrap();
            assert_eq!(
                page.doc.design.interactions[&id],
                vec![Action::Slide {
                    page: imported[index + 1]
                }]
            );
            // A checked-out native history root must retain the rewritten destination too.
            for commit in page.graph.commits() {
                assert_eq!(
                    commit.doc.design.interactions[&id],
                    vec![Action::Slide {
                        page: imported[index + 1]
                    }]
                );
            }
        }
        target.undo();
        assert_eq!(target.page_list().len(), 1);
        target.redo();
        assert_eq!(target.page_list().len(), 5);
        // Serialize metadata independently of IO history to check stable binding schema.
        let data = serde_json::to_vec(&target.doc.design).unwrap();
        let roundtrip: emulsion_core::design_metadata::Design =
            serde_json::from_slice(&data).unwrap();
        assert_eq!(roundtrip, target.doc.design);
        assert!(generate_pages(&snapshot, &[1, 1], "name\nA", 99, None).is_err());
        assert!(generate_pages(&snapshot, &[1, 2], "name\nA\nB", 3, None).is_err());
    }
    #[test]
    fn data_images_keep_editable_geometry_crop_and_source_on_late_failure() {
        use emulsion_core::design_data::{Binding, Fit};
        use emulsion_raster::Placement;
        use std::sync::Arc;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wide.png");
        image::RgbaImage::from_pixel(20, 10, image::Rgba([230, 40, 80, 255]))
            .save(&path)
            .unwrap();
        let mut doc = Document::new(300, 200);
        let mut placement = Placement::at(30., 40.);
        placement.flip_x = true;
        placement.rotation = 30.;
        let id = Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Photo",
                Arc::new(emulsion_raster::Raster::from_srgba8(10, 10, &[255; 400])),
                placement,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        doc.design.data_bindings.insert(
            id,
            Binding::Image {
                column: "photo".into(),
                fit: Fit::Cover,
                focus: [0.25, 0.5],
            },
        );
        let before = doc.clone();
        let project = ProjectEditor::new_project(ProjectKind::Design, doc.clone())
            .unwrap()
            .snapshot()
            .unwrap();
        let generated = generate_pages(
            &project,
            &[1],
            "photo\nwide.png",
            99,
            Some(directory.path()),
        )
        .unwrap();
        let node = generated.pages[0].doc.node(id).unwrap();
        let NodeKind::Raster { raster, placement } = &node.kind else {
            panic!()
        };
        assert_eq!((raster.width(), raster.height()), (20, 10));
        assert!(placement.flip_x);
        assert_eq!(placement.rotation, 30.);
        let mask = node.mask.as_ref().unwrap();
        assert_eq!(mask.get(0, 5), 255);
        assert_eq!(mask.get(19, 5), 0);
        assert!(
            generate_pages(
                &project,
                &[1],
                "photo\nwide.png\nmissing.png",
                99,
                Some(directory.path())
            )
            .is_err()
        );
        assert_eq!(project.pages[0].doc, before);
        assert!(
            generate_pages(
                &project,
                &[1],
                "photo\nhttps://example.com/image.png",
                99,
                None
            )
            .is_err()
        );
    }
}
