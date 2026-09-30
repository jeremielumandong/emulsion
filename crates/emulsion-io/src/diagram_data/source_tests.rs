use super::*;

fn item<'a>(draft: &'a Draft, id: &str) -> &'a Item {
    draft.items.iter().find(|i| i.key == id).unwrap()
}

#[test]
fn every_glyphtide_starter_imports_as_a_valid_editable_document() {
    let examples: serde_json::Value =
        serde_json::from_str(include_str!("glyphtide_examples.json")).unwrap();
    for example in examples.as_array().unwrap() {
        let format = match example["engine"].as_str().unwrap() {
            "mermaid" => Format::Mermaid,
            "d2" => Format::D2,
            "graphviz" => Format::Graphviz,
            _ => unreachable!(),
        };
        let draft = parse(example["code"].as_str().unwrap(), format)
            .unwrap_or_else(|e| panic!("{}: {e}", example["id"]));
        let doc = draft
            .document()
            .unwrap_or_else(|e| panic!("{}: {e}", example["id"]));
        doc.validate().unwrap();
        assert!(doc.diagram.as_ref().is_some_and(|d| !d.shapes.is_empty()));
        assert!(
            doc.nodes
                .iter()
                .all(|n| !matches!(n.kind, NodeKind::Raster { .. }))
        );
    }
}

#[test]
fn dot_keeps_quoted_ids_defaults_attributes_and_edge_chains() {
    let d = parse(
        r#"digraph G {
rankdir=LR;
node [shape=box];
"start here" [label="Start 中文", shape=ellipse];
subgraph cluster_one { review [label="Review"]; }
"start here" -> review -> done [label="next"];
}"#,
        Format::Graphviz,
    )
    .unwrap();
    assert_eq!(item(&d, "start here").label, "Start 中文");
    assert_eq!(item(&d, "start here").kind, ShapeKind::Terminator);
    assert_eq!(item(&d, "review").label, "Review");
    assert_eq!(d.links.len(), 2);
    assert_eq!(d.links[1].label, "next");
    d.document().unwrap().validate().unwrap();
    let d = parse("graph { a -- b }", Format::Graphviz).unwrap();
    assert!(!d.links[0].arrow);
}

#[test]
fn d2_keeps_nested_ids_labels_properties_and_directions() {
    let d = parse("direction: right\nserver: {\napi: API endpoint {shape: rectangle}\ndb: Storage {shape: cylinder}\napi -> db: query\n}\nclient: Browser\nclient -> server.api: request", Format::D2).unwrap();
    assert_eq!(item(&d, "server.api").label, "API endpoint");
    assert_eq!(item(&d, "server.db").kind, ShapeKind::Database);
    assert_eq!(d.links[0].label, "query");
    assert_eq!(d.links[1].target, "server.api");
    d.document().unwrap().validate().unwrap();
    let d = parse(
        "a.shape: cylinder\na.label: Store\na.style.fill: \"#fff\"\na -> b",
        Format::D2,
    )
    .unwrap();
    assert_eq!(d.items.len(), 2);
    assert_eq!(item(&d, "a").kind, ShapeKind::Database);
    assert_eq!(item(&d, "a").label, "Store");
}

#[test]
fn malformed_sources_and_external_d2_imports_fail_atomically() {
    for (format, text) in [
        (Format::Mermaid, "flowchart TD\nA[unclosed --> B"),
        (Format::Mermaid, "classDiagram\nclass X {\nfield"),
        (Format::Mermaid, "unknownDiagram\nA --> B"),
        (Format::Graphviz, "digraph { a -> }"),
        (Format::Graphviz, "digraph { a -> b"),
        (Format::Graphviz, "digraph { a -- b }"),
        (Format::D2, "server: {\nclient"),
        (Format::D2, "x: @remote.d2"),
    ] {
        assert!(parse(text, format).is_err(), "accepted: {text}");
    }
}

#[test]
fn source_metadata_and_bidirectional_arrows_survive_materialization() {
    let dot = parse("digraph { a -> b [dir=both] }", Format::Graphviz).unwrap();
    assert!(dot.links[0].arrow_start);
    let quoted = parse("node: \"Contact team@example.com | help\"", Format::D2).unwrap();
    assert_eq!(quoted.items[0].label, "Contact team@example.com | help");
}

#[test]
fn open_dispatch_imports_sources_and_markdown_as_editable_pages() {
    let directory = tempfile::tempdir().unwrap();
    for (extension, source) in [
        ("MMD", "flowchart TD\nA-->B"),
        ("mermaid", "sequenceDiagram\nA->>B: Hello"),
        ("d2", "a -> b"),
        ("dot", "digraph { a -> b }"),
        ("gv", "graph { a -- b }"),
        (
            "glyphtide",
            r#"{"engine":"mermaid","code":"flowchart TD\nA-->B"}"#,
        ),
        ("json", r#"{"engine":"d2","code":"a -> b"}"#),
        ("csv", "id,label\na,Hello"),
        ("sql", "CREATE TABLE a (id INT);"),
        ("txt", "First\nSecond"),
    ] {
        let path = directory.path().join(format!("example.{extension}"));
        std::fs::write(&path, source).unwrap();
        assert!(crate::is_openable(&path));
        assert!(crate::diagram_import::is_diagram(&path));
        let imported = crate::diagram_import::read(&path).unwrap();
        assert_eq!(imported.project.pages.len(), 1);
        assert!(imported.project.pages[0].doc.diagram.is_some());
        imported.project.validate().unwrap();
    }
    let path = directory.path().join("Diagrams.md");
    std::fs::write(&path, "# Diagrams\n```mermaid\nflowchart TD\nA-->B\n```\n```d2\na -> b\n```\n```dot\ndigraph { a -> b }\n```\n").unwrap();
    let imported = crate::diagram_import::read(&path).unwrap();
    assert_eq!(imported.project.pages.len(), 3);
    assert_eq!(imported.project.pages[2].meta.name, "Diagrams · 3");
    imported.project.validate().unwrap();
    let saved = directory.path().join("Imported.emu");
    crate::project::write(&imported.project, &saved).unwrap();
    let reopened = crate::project::read(&saved).unwrap();
    assert_eq!(reopened.pages.len(), 3);
    for (before, after) in imported.project.pages.iter().zip(reopened.pages) {
        assert_eq!(before.doc.diagram, after.doc.diagram);
    }
    std::fs::write(&path, "```mermaid\nflowchart TD\nA-->B").unwrap();
    assert!(crate::diagram_import::read(&path).is_err());
    let unrelated = directory.path().join("settings.json");
    std::fs::write(&unrelated, r#"{"theme":"dark"}"#).unwrap();
    assert!(!crate::diagram_import::is_diagram(&unrelated));
}
