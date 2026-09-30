//! Mermaid's family-specific parser, layout and renderer, embedded without a browser.
//! Glyphtide's diagramModel is a text summary, not a rendering model.
use super::*;
use base64::Engine as _;
use emulsion_core::{Node, text::TextSpec, vector_cache::VectorRaster};
use merman::render::HeadlessRenderer;

fn renderer() -> HeadlessRenderer {
    HeadlessRenderer::new()
        .with_diagram_id("emulsion-mermaid")
        .with_site_config(merman::MermaidConfig::from_value(serde_json::json!({
            "securityLevel": "strict",
            "htmlLabels": false,
            "look": "classic",
            "fontFamily": "Geist, sans-serif",
            "flowchart": { "htmlLabels": false },
            "class": { "htmlLabels": false },
            "state": { "htmlLabels": false }
        })))
}

pub(super) fn parse(source: &str) -> Result<Draft> {
    let mut source = source.trim().trim_start_matches('\u{feff}').trim();
    if let Some(body) = source.strip_prefix("```mermaid") {
        source = body
            .trim()
            .strip_suffix("```")
            .ok_or_else(|| error("Unclosed Mermaid code fence."))?
            .trim();
    }
    let parsed = renderer()
        .parse_diagram_sync(source)
        .map_err(|e| error(format!("Mermaid: {e}")))?
        .ok_or_else(|| error("Expected a Mermaid diagram."))?;
    let mut draft = Draft::new();
    draft.node(
        "mermaid",
        &format!("Mermaid {}", parsed.meta.diagram_type),
        ShapeKind::Process,
    )?;
    draft.mermaid_source = Some(source.into());
    draft.warn("Mermaid preserves its diagram layout as scalable vector artwork. Text may be outlined and connections do not reroute; re-import the source to update the diagram.");
    Ok(draft)
}

pub(super) fn svg(source: &str) -> Result<String> {
    let svg = renderer()
        .render_svg_resvg_safe_sync(source)
        .map_err(|e| error(format!("Mermaid: {e}")))?
        .ok_or_else(|| error("Expected a Mermaid diagram."))?;
    if svg.len() > 32 << 20 {
        return Err(error(
            "Rendered Mermaid diagram exceeds 32 MiB. Split the input into smaller diagrams.",
        ));
    }
    Ok(svg)
}

pub(super) fn document(source: &str) -> Result<Document> {
    let svg = svg(source)?;
    let tree = resvg::usvg::Tree::from_str(&svg, &crate::svg_vectors::options())
        .map_err(|e| error(format!("Mermaid SVG: {e}")))?;
    let (w, h) = (
        tree.size().width().ceil() as u32,
        tree.size().height().ceil() as u32,
    );
    crate::import::check_size(w, h)?;
    let mut builder = diagram::Builder::new(w, h).map_err(error)?;
    let root = builder
        .add_shape(ShapeKind::Process, [0., 0., w as f64, h as f64], "")
        .map_err(error)?;
    let mut doc = builder.finish().map_err(error)?;
    doc.node_mut(root).unwrap().name = "Mermaid diagram".into();
    let graph = Arc::make_mut(doc.diagram.as_mut().unwrap());
    let shape = graph.shapes.get_mut(&root).unwrap();
    shape.data.insert("source_format".into(), "mermaid".into());
    let body = shape.body;
    if let NodeKind::Path { path, style, cache } = &mut doc.node_mut(body).unwrap().kind {
        style.fill = None;
        style.stroke = None;
        *cache = VectorRaster::path(path.clone(), *style, w, h);
    }
    let mut notes = BTreeSet::new();
    crate::drawio::images::insert(
        &mut doc,
        root,
        [0., 0., w as f64, h as f64],
        false,
        &format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&svg)
        ),
        &mut 16_777_216,
        &mut notes,
    )?;
    // Keep the exact source in bounded, hidden native text records, including
    // when SVG normalization converts all visible labels to editable outlines.
    let mut chars = source.chars().peekable();
    let mut index = 0;
    while chars.peek().is_some() {
        let text: String = chars.by_ref().take(16_000).collect();
        let id = doc.alloc_id();
        let mut node = Node::text(
            id,
            format!("Mermaid source {index:04}"),
            TextSpec {
                text,
                ..Default::default()
            },
            w,
            h,
        );
        node.parent = Some(root);
        node.visible = false;
        doc.nodes.push(node);
        index += 1;
    }
    doc.normalize();
    doc.validate().map_err(|e| error(e.to_string()))?;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SEQUENCE: &str = "sequenceDiagram\n    autonumber\n    actor U as Visitor\n    participant SPA as Web application\n    participant API as Service\n    rect rgb(224,247,250)\n    Note over U,API: Registration stage\n    U->>SPA: Open registration\n    SPA->>API: Submit profile\n    API->>API: Validate profile\n    Note over API: First line<br/>Second line\n    API-->>SPA: Accepted\n    end\n    alt Approved\n    SPA-->>U: Welcome\n    else Rejected\n    SPA-->>U: Try again\n    end";

    fn elements(svg: &str, tag: &str, class: &str) -> Vec<BTreeMap<String, String>> {
        let mut reader = quick_xml::Reader::from_str(svg);
        let mut out = Vec::new();
        loop {
            match reader.read_event().unwrap() {
                quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)
                    if e.local_name().as_ref() == tag =>
                {
                    let attrs = e
                        .attributes()
                        .map(|a| {
                            let a = a.unwrap();
                            (
                                a.key.as_ref().to_string(),
                                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                    .unwrap()
                                    .into_owned(),
                            )
                        })
                        .collect::<BTreeMap<_, _>>();
                    if attrs
                        .get("class")
                        .is_some_and(|v| v.split_whitespace().any(|v| v == class))
                    {
                        out.push(attrs);
                    }
                }
                quick_xml::events::Event::Eof => break,
                _ => {}
            }
        }
        out
    }

    #[test]
    fn sequence_has_lifelines_ordered_messages_notes_and_sections() {
        let svg = svg(SEQUENCE).unwrap();
        assert_eq!(elements(&svg, "line", "actor-line").len(), 3);
        let messages = elements(&svg, "text", "messageText");
        assert_eq!(messages.len(), 6);
        let ys: Vec<f64> = messages.iter().map(|a| a["y"].parse().unwrap()).collect();
        assert!(
            ys.windows(2).all(|p| p[0] < p[1]),
            "messages must follow time vertically: {ys:?}"
        );
        assert_eq!(elements(&svg, "text", "sequenceNumber").len(), 6);
        assert!(!elements(&svg, "rect", "note").is_empty());
        assert!(
            svg.contains("224,247,250") || svg.contains("224, 247, 250") || svg.contains("#e0f7fa")
        );
        assert!(svg.contains("Approved") && svg.contains("Rejected"));
        assert!(
            !svg.contains("&lt;br"),
            "line breaks must be rendered, not printed"
        );
    }

    #[test]
    fn flowchart_retains_direction_subgraphs_shapes_and_styles() {
        let source = "flowchart RL\nsubgraph Backend\n A[(Storage)] --> B{Ready?}\nend\nB -->|Yes| C([Done])\nstyle A fill:#ff0000\n";
        let svg = svg(source).unwrap();
        assert!(svg.contains("Backend") && svg.contains("Storage") && svg.contains("Yes"));
        assert!(!elements(&svg, "g", "cluster").is_empty());
        assert!(svg.contains("#ff0000"));
        assert!(parse(source).unwrap().is_mermaid());
    }

    #[test]
    fn source_fence_front_matter_unicode_and_native_roundtrip_are_preserved() {
        let source = format!("---\ntitle: 流程\n---\n{SEQUENCE}");
        let draft = parse(&format!("```mermaid\n{source}\n```")).unwrap();
        let doc = draft.document().unwrap();
        doc.validate().unwrap();
        assert!(
            doc.nodes.len() > 4,
            "diagram should contain its rendered artwork"
        );
        assert!(
            !doc.nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Raster { .. }))
        );
        let recovered: String = doc
            .nodes
            .iter()
            .filter_map(|n| match &n.kind {
                NodeKind::Text { spec, .. }
                    if !n.visible && n.name.starts_with("Mermaid source ") =>
                {
                    Some(spec.text.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(recovered, source);
        let project = emulsion_core::project::ProjectEditor::new_project(
            emulsion_core::project::ProjectKind::Diagram,
            doc.clone(),
        )
        .unwrap()
        .snapshot()
        .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sequence.emu");
        crate::project::write(&project, &path).unwrap();
        let reopened = crate::project::read(&path).unwrap();
        assert_eq!(
            crate::project_export::svg(&reopened.pages[0].doc).unwrap(),
            crate::project_export::svg(&doc).unwrap()
        );
        let (_, flattened) = crate::project_export::svg(&doc).unwrap();
        assert!(!flattened);
        assert!(draft.refresh_commands(&doc).is_err());
    }

    #[test]
    fn malformed_and_unsupported_diagrams_never_turn_into_source_notes() {
        for source in [
            "notADiagram\nA",
            "pie\nnot valid data",
            "sequenceDiagram\nA->>B: Hello\nend",
            "flowchart TD\nA[unclosed --> B",
        ] {
            assert!(parse(source).is_err(), "accepted {source}");
        }
    }
}
