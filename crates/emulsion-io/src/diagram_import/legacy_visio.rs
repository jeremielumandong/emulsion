//! Local, bounded librevisio conversion for legacy binary drawings/stencils.
use super::*;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub(super) fn read(path: &Path) -> Result<Imported> {
    let path = path.canonicalize()?;
    let size = std::fs::metadata(&path)?.len();
    if size == 0 || size > MAX_FILE {
        return Err(error("Empty or oversized legacy Visio file"));
    }
    let tool = if path
        .extension()
        .is_some_and(|e| e.to_string_lossy().to_ascii_lowercase().starts_with("vss"))
    {
        "vss2xhtml"
    } else {
        "vsd2xhtml"
    };
    let mut child = Command::new(tool)
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            error(format!(
                "Legacy Visio conversion needs librevisio's {tool}: {e}"
            ))
        })?;
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(error("Visio conversion exceeded 15 seconds"));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let bytes = reader
        .join()
        .map_err(|_| error("Visio converter output reader failed"))??;
    if !status.success() || bytes.len() as u64 > MAX_FILE {
        return Err(error(
            "Librevisio could not convert this file, or output exceeded 64 MiB",
        ));
    }
    from_xhtml(
        &String::from_utf8(bytes).map_err(|e| error(e.to_string()))?,
        path.file_stem().and_then(|s| s.to_str()).unwrap_or("Visio"),
    )
}
fn from_xhtml(text: &str, name: &str) -> Result<Imported> {
    let mut reader = quick_xml::Reader::from_str(text);
    let mut start = None;
    let mut depth = 0;
    let mut pages = Vec::new();
    let mut skipped = 0;
    loop {
        let before = reader.buffer_position() as usize;
        match reader.read_event().map_err(|e| error(e.to_string()))? {
            quick_xml::events::Event::Start(e) => {
                if start.is_some() {
                    depth += 1;
                } else if e.local_name().as_ref() == "svg" {
                    start = Some(before);
                    depth = 1;
                }
            }
            quick_xml::events::Event::End(_) => {
                if let Some(from) = start {
                    depth -= 1;
                    if depth == 0 {
                        let fragment = &text[from..reader.buffer_position() as usize];
                        let mut probe = quick_xml::Reader::from_str(fragment);
                        let mut drawn = false;
                        loop {
                            match probe.read_event().map_err(|e| error(e.to_string()))? {
                                quick_xml::events::Event::Start(e)
                                | quick_xml::events::Event::Empty(e)
                                    if matches!(
                                        e.local_name().as_ref(),
                                        "path"
                                            | "rect"
                                            | "circle"
                                            | "ellipse"
                                            | "line"
                                            | "polyline"
                                            | "polygon"
                                            | "image"
                                            | "text"
                                    ) =>
                                {
                                    drawn = true;
                                    break;
                                }
                                quick_xml::events::Event::Eof => break,
                                _ => {}
                            }
                        }
                        if !drawn {
                            skipped += 1;
                            start = None;
                            continue;
                        }
                        if pages.len() >= emulsion_core::project::MAX_PAGES {
                            return Err(error(
                                "Converted Visio file exceeds 100 pages; split the source library",
                            ));
                        }
                        let doc = crate::svg_vectors::document(fragment)?;
                        let doc = attach(doc)?;
                        let id = pages.len() as u64 + 1;
                        pages.push(ProjectPage {
                            meta: PageMeta {
                                id,
                                name: format!("{name} · {id}").chars().take(200).collect(),
                                bleed_mm: 0.,
                            },
                            graph: Graph::new(doc.clone(), "Converted Visio"),
                            doc,
                        });
                        start = None;
                    }
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    if pages.is_empty() {
        return Err(error("No SVG drawings were produced by librevisio"));
    }
    let project = Project {
        kind: ProjectKind::Diagram,
        next_page_id: pages.len() as u64 + 1,
        pages,
        active: 1,
    };
    project.validate().map_err(error)?;
    let mut warnings=vec!["Legacy Visio converted locally with librevisio. Vector artwork is retained; original connector bindings and Visio formulas are not available in SVG output.".into()];
    if skipped > 0 {
        warnings.push(format!(
            "Skipped {skipped} empty converter placeholder pages."
        ));
    }
    Ok(Imported { project, warnings })
}
pub(crate) fn attach(doc: Document) -> Result<Document> {
    use emulsion_core::{Command, Editor, command::Slot};
    let roots = doc.children(None);
    let bounds = roots
        .iter()
        .filter_map(|id| emulsion_core::geometry::node_bounds(&doc, *id))
        .fold(emulsion_raster::IRect::default(), |a, b| a.union(&b));
    let mut e = Editor::new(doc, None);
    let group = diagram::add_shape(
        &mut e,
        ShapeKind::Process,
        [
            bounds.x as f64,
            bounds.y as f64,
            bounds.w.max(1) as f64,
            bounds.h.max(1) as f64,
        ],
        "",
    )
    .map_err(error)?;
    let body = e.doc.diagram.as_ref().unwrap().shapes[&group].body;
    if let NodeKind::Path { path, style, .. } = &e.doc.node(body).unwrap().kind {
        let mut style = *style;
        style.fill = None;
        style.stroke = None;
        e.execute(Command::SetPath {
            id: body,
            path: path.clone(),
            style,
        })
        .map_err(|e| error(e.to_string()))?;
    }
    for root in roots {
        e.execute(Command::MoveNode {
            id: root,
            slot: Slot::top_of(Some(group)),
        })
        .map_err(|e| error(e.to_string()))?;
    }
    Ok(e.doc)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converter_svg_pages_remain_vectors_and_graph_objects() {
        let text = r#"<html><body><svg:svg xmlns:svg="http://www.w3.org/2000/svg" width="80" height="60"><svg:rect width="40" height="20" fill="red"/></svg:svg></body></html>"#;
        let imported = from_xhtml(text, "Legacy").unwrap();
        let doc = &imported.project.pages[0].doc;
        assert_eq!(doc.diagram.as_ref().unwrap().shapes.len(), 1);
        assert!(
            !doc.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Raster { .. }))
        );
        doc.validate().unwrap();
    }
}
