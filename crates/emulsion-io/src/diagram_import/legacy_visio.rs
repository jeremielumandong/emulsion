//! Local, bounded librevisio conversion for legacy binary drawings/stencils.
use super::*;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub(super) fn read(path: &Path) -> Result<Imported> {
    let path = path.canonicalize()?;
    let size = std::fs::metadata(&path)?.len();
    if size == 0 || size > 256 << 20 {
        return Err(error("Empty or oversized legacy Visio file"));
    }
    let stencil=path.file_name().is_some_and(|n|n.to_string_lossy().to_ascii_lowercase().contains(".vss"));
    let tools=if stencil {["vss2xhtml","vsd2xhtml"]}else{["vsd2xhtml","vss2xhtml"]};
    let mut failures=Vec::new();
    for tool in tools {
        match convert(&path,tool) {
            Ok(mut imported)=>{if !failures.is_empty(){imported.warnings.push(format!("Recovered Visio content with {tool} after the other converter could not read it."));}return Ok(imported);}
            Err(e)=>failures.push(format!("{tool}: {e}")),
        }
    }
    Err(error(failures.join("; ")))
}
fn convert(path:&Path,tool:&str)->Result<Imported>{
    let output=tempfile::tempfile()?;
    let mut child = Command::new(tool)
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(output.try_clone()?))
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            error(format!(
                "Legacy Visio conversion needs librevisio's {tool}: {e}"
            ))
        })?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(60) || output.metadata()?.len()>512<<20 {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error("Visio conversion exceeded 60 seconds or 512 MiB"));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() || output.metadata()?.len()>512<<20 {
        return Err(error("Librevisio failed or exceeded the 512 MiB output limit"));
    }
    use std::io::{Seek,SeekFrom};
    let mut output=output;output.seek(SeekFrom::Start(0))?;
    from_stream(std::io::BufReader::new(output),path.file_stem().and_then(|s|s.to_str()).unwrap_or("Visio"))
}
/// Bound transient conversion memory by the largest entry, not the whole library.
fn from_stream(reader:impl std::io::BufRead,name:&str)->Result<Imported>{
    use quick_xml::{Reader,Writer,events::Event};
    let mut reader=Reader::from_reader(reader);let mut buffer=Vec::new();
    let mut writer:Option<Writer<Vec<u8>>>=None;let mut depth=0usize;let mut oversized=false;
    let mut pages=Vec::new();let mut notes=BTreeSet::new();let mut entries=0usize;let mut empty_entries=0usize;
    loop {
        let event=reader.read_event_into(&mut buffer).map_err(|e|error(e.to_string()))?;
        match &event {
            Event::Start(e)=>{if writer.is_some(){depth+=1;}else if e.local_name().as_ref()=="svg"{writer=Some(Writer::new(Vec::new()));depth=1;oversized=false;}},
            Event::End(_)=>{if writer.is_some(){depth-=1;}},
            Event::Eof=>break,
            _=>{}
        }
        if let Some(w)=&mut writer {
            if w.get_ref().len()>64<<20{oversized=true;}
            if !oversized{w.write_event(event)?;}
            if depth==0 {
                entries+=1;let fragment=writer.take().unwrap().into_inner();
                let result=if oversized{Err(error("Converted entry exceeds 64 MiB"))}else{
                    String::from_utf8(fragment).map_err(|e|error(e.to_string())).and_then(|xml|from_xhtml(&xml,name))
                };
                match result {
                    Ok(imported)=>{
                        for mut page in imported.project.pages {
                            if pages.len()>=emulsion_core::project::MAX_PAGES{return Err(error("Converted library exceeds 4096 pages"));}
                            page.meta.id=pages.len() as u64+1;page.meta.name=format!("{name} · {entries}").chars().take(200).collect();pages.push(page);
                        }
                        notes.extend(imported.warnings);
                    }
                    Err(e)=>{if e.to_string().contains("No SVG drawings"){empty_entries+=1;}else{notes.insert(format!("Converter entry {entries} contains no usable drawing: {e}"));}}
                }
            }
        }
        buffer.clear();
    }
    if pages.is_empty(){return Err(error("No SVG drawings were produced by librevisio"));}
    if empty_entries>0{notes.insert(format!("Skipped {empty_entries} empty converter placeholder entries."));}
    let project=Project{kind:ProjectKind::Diagram,active:1,next_page_id:pages.len() as u64+1,pages};
    project.validate().map_err(error)?;
    Ok(Imported{project,warnings:notes.into_iter().collect()})
}

fn from_xhtml(text: &str, name: &str) -> Result<Imported> {
    let mut start = None;
    let mut depth = 0;
    let mut pages = Vec::new();
    let mut skipped = 0;
    let mut fitted = 0;
    let mut rejected=Vec::new();
    let sanitized=text.chars().filter(|c|!c.is_control() || matches!(c,'\n'|'\r'|'\t')).collect::<String>();
    let text=sanitized.as_str();
    let mut reader=quick_xml::Reader::from_str(text);
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
                                "Converted Visio file exceeds the project page limit; split the source library",
                            ));
                        }
                        let doc = crate::svg_vectors::document(fragment).or_else(|_| {
                            fitted+=1;crate::svg_vectors::fitted_document(fragment)
                        }).and_then(attach);
                        let doc=match doc {Ok(doc)=>doc,Err(e)=>{rejected.push(format!("Converter entry {} could not be recovered: {e}",pages.len()+skipped+rejected.len()+1));start=None;continue;}};
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
        return Err(error(if rejected.is_empty() { "No SVG drawings were produced by librevisio".into() } else { rejected.join("; ") }));
    }
    let project = Project {
        kind: ProjectKind::Diagram,
        next_page_id: pages.len() as u64 + 1,
        pages,
        active: 1,
    };
    project.validate().map_err(error)?;
    let mut warnings=vec!["Legacy Visio converted locally with librevisio. Vector artwork is retained; original connector bindings and Visio formulas are not available in SVG output.".into()];
    warnings.extend(rejected);
    if fitted>0 {warnings.push(format!("Recovered {fitted} converter page dimensions from vector bounds."));}
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
    fn streaming_converter_keeps_valid_entries_and_reports_empty_placeholders() {
        let text=r#"<html><body><svg:svg xmlns:svg="http://www.w3.org/2000/svg" width="80" height="60"><svg:rect width="40" height="20" fill="red"/></svg:svg><svg:svg xmlns:svg="http://www.w3.org/2000/svg" width="80" height="60"></svg:svg><svg:svg xmlns:svg="http://www.w3.org/2000/svg" width="80" height="60"><svg:circle cx="20" cy="20" r="10"/></svg:svg></body></html>"#;
        let imported=from_stream(std::io::Cursor::new(text.as_bytes()),"Library").unwrap();
        assert_eq!(imported.project.pages.len(),2);
        assert!(imported.warnings.iter().any(|w|w.contains("empty converter")));
        imported.project.validate().unwrap();
    }
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
