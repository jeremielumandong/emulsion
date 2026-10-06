//! Page-aware diagram interchange and generation; graph edits use diagram_tools.
use crate::{ToolDef, ToolResult};
use emulsion_core::{
    diagram,
    project::{ProjectEditor, ProjectKind},
};
use serde_json::{Value, json};
use std::path::Path;
pub fn is_tool(name: &str) -> bool {
    matches!(
        name,
        "install_diagram_stencil_pack"
            | "create_diagram_link"
            | "open_diagram_link"
            | "import_diagram"
            | "save_document_stencils"
            | "export_diagram"
            | "generate_diagram"
            | "quick_create_diagram"
            | "insert_diagram_template"
            | "insert_diagram_pack_entry"
    )
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("{key} must be a string"))
}
pub fn execute(editor: &mut ProjectEditor, name: &str, args: &Value) -> ToolResult {
    if name == "install_diagram_stencil_pack" {
        return install_stencil_pack(args);
    }
    match run(editor, name, args) {
        Ok(v) => ToolResult::text(v.to_string()),
        Err(e) => ToolResult::error(e),
    }
}
pub fn install_stencil_pack(args: &Value) -> ToolResult {
    let result = (|| -> Result<Value, String> {
        let map = args.as_object().ok_or("Arguments must be an object")?;
        if map.keys().any(|k| k != "path" && k != "pack") {
            return Err("Only path or pack is accepted".into());
        }
        let (pack, warnings) = match (args.get("path"), args.get("pack")) {
            (Some(_), None) => {
                emulsion_io::template_pack::read_stencil_source(Path::new(text(args, "path")?))
            }
            (None, Some(_)) => emulsion_io::diagram_packs::build(text(args, "pack")?),
            _ => return Err("Provide exactly one of path or pack".into()),
        }
        .map_err(|e| e.to_string())?;
        let count = pack.project.pages.len();
        let (_, id) =
            emulsion_io::template_pack::install(&emulsion_io::creative_library::root(), pack)
                .map_err(|e| e.to_string())?;
        Ok(json!({"pack":id,"entries":count,"warnings":warnings}))
    })();
    match result {
        Ok(v) => ToolResult::text(v.to_string()),
        Err(error) => ToolResult::error(error),
    }
}

pub fn validate_args(name: &str, args: &Value) -> Result<(), String> {
    let def = definitions()
        .into_iter()
        .find(|d| d.name == name)
        .ok_or("Unknown diagram tool")?;
    let object = args.as_object().ok_or("Arguments must be an object")?;
    let props = def.input_schema["properties"].as_object().unwrap();
    for (key, value) in object {
        let schema = props
            .get(key)
            .ok_or_else(|| format!("Unknown argument {key}"))?;
        let valid = match schema["type"].as_str() {
            Some("string") => value.is_string(),
            Some("boolean") => value.is_boolean(),
            Some("integer") => value.as_u64().is_some_and(|v| v > 0),
            _ => true,
        };
        if !valid
            || schema["enum"]
                .as_array()
                .is_some_and(|items| !items.contains(value))
        {
            return Err(format!("Invalid argument {key}"));
        }
    }
    Ok(())
}
pub fn load_import(args: &Value) -> Result<emulsion_io::drawio::Imported, String> {
    validate_args("import_diagram", args)?;
    match (args.get("path"), args.get("xml")) {
        (Some(path), None) => {
            let path = Path::new(path.as_str().ok_or("path must be a string")?);
            if emulsion_io::template_pack::is_pack(path) {
                let pack = emulsion_io::template_pack::read(path).map_err(|e| e.to_string())?;
                Ok(emulsion_io::drawio::Imported {
                    project: pack.project,
                    warnings: Vec::new(),
                })
            } else {
                emulsion_io::diagram_import::read(path).map_err(|e| e.to_string())
            }
        }
        (None, Some(xml)) => {
            emulsion_io::drawio::from_xml(xml.as_str().ok_or("xml must be a string")?)
                .map_err(|e| e.to_string())
        }
        _ => Err("Provide exactly one of path or xml".into()),
    }
}
pub fn save_stencil_snapshot(
    project: &emulsion_core::project::Project,
    args: &Value,
) -> ToolResult {
    if project.kind != ProjectKind::Diagram {
        return ToolResult::error("Open a Diagram project first");
    }
    let result = validate_args("save_document_stencils", args).and_then(|_| {
        emulsion_io::document_stencils::save(
            &emulsion_io::creative_library::root(),
            project,
            args["name"].as_str().unwrap_or("Diagram"),
        )
        .map_err(|e| e.to_string())
    });
    match result {
        Ok(ids) => ToolResult::text(json!({"packs":ids}).to_string()),
        Err(e) => ToolResult::error(e),
    }
}

fn run(editor: &mut ProjectEditor, name: &str, args: &Value) -> Result<Value, String> {
    if editor.kind() != Some(ProjectKind::Diagram) {
        return Err("Open a Diagram project first".into());
    }
    if editor.in_transaction() {
        return Err("Finish the current edit first".into());
    }
    validate_args(name, args)?;
    match name {
        "create_diagram_link" => {
            let path = editor
                .path
                .as_ref()
                .ok_or("Save the project before copying a diagram link")?;
            let nodes: Vec<u64> = args
                .get("nodes")
                .map(|v| serde_json::from_value(v.clone()).map_err(|e| e.to_string()))
                .transpose()?
                .unwrap_or_default();
            let view: Option<[f64; 4]> = args
                .get("view")
                .map(|v| serde_json::from_value(v.clone()).map_err(|e| e.to_string()))
                .transpose()?;
            let link = diagram::workspace::Link {
                project: Some(path.to_string_lossy().into()),
                page: editor.active_page(),
                nodes,
                view,
            };
            link.check_project(editor)?;
            Ok(json!({"link":link.encode()?}))
        }
        "open_diagram_link" => {
            let link = diagram::workspace::Link::decode(text(args, "link")?)?;
            link.check_project(editor)?;
            editor.set_active_page(link.page)?;
            Ok(json!({"page":link.page,"nodes":link.nodes,"view":link.view}))
        }
        "insert_diagram_pack_entry" => {
            let project = emulsion_io::project::read(Path::new(text(args, "path")?))
                .map_err(|e| e.to_string())?;
            let page = args["page"].as_u64().unwrap_or(1);
            let page = project
                .pages
                .get((page - 1) as usize)
                .ok_or("Stencil page does not exist")?;
            let roots: Vec<_> = page
                .doc
                .nodes
                .iter()
                .filter(|n| {
                    n.parent.is_none() && !matches!(n.kind, emulsion_core::NodeKind::Fill { .. })
                })
                .map(|n| n.id)
                .collect();
            let fragment = emulsion_core::fragment::Fragment::capture(&page.doc, &roots)?;
            let bounds = roots
                .iter()
                .map(|id| emulsion_core::geometry::node_bounds(&page.doc, *id))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
                .into_iter()
                .flatten()
                .fold(emulsion_raster::IRect::default(), |a, b| a.union(&b));
            let center = match args.get("center") {
                None => (editor.doc.width as f64 / 2., editor.doc.height as f64 / 2.),
                Some(value) => {
                    let values = value
                        .as_array()
                        .filter(|v| v.len() == 2)
                        .ok_or("center must be [x,y]")?;
                    let x = values[0].as_f64().ok_or("Invalid center x")?;
                    let y = values[1].as_f64().ok_or("Invalid center y")?;
                    if !x.is_finite() || !y.is_finite() || x.abs() > 1e6 || y.abs() > 1e6 {
                        return Err("Center must be finite and within 1e6 document pixels".into());
                    }
                    (x, y)
                }
            };
            let offset = (
                center.0 - bounds.x as f64 - bounds.w as f64 / 2.,
                center.1 - bounds.y as f64 - bounds.h as f64 / 2.,
            );
            let nodes = fragment.paste(editor, emulsion_core::command::Slot::TOP, offset)?;
            Ok(json!({"nodes":nodes}))
        }
        "insert_diagram_template" => {
            let id = text(args, "template")?;
            let template = *emulsion_core::diagram_library::TEMPLATES
                .iter()
                .find(|t| t.id == id)
                .ok_or("Unknown diagram template")?;
            let page = template.insert(editor)?;
            Ok(json!({"page":page,"template":template.id}))
        }
        "save_document_stencils" => {
            let project = editor.snapshot().ok_or("No project")?;
            let ids = emulsion_io::document_stencils::save(
                &emulsion_io::creative_library::root(),
                &project,
                args["name"].as_str().unwrap_or("Diagram"),
            )
            .map_err(|e| e.to_string())?;
            Ok(json!({"packs":ids}))
        }
        "import_diagram" => {
            let imported = load_import(args)?;
            let captured = (args["save_stencils"] == true).then(|| imported.project.clone());
            let pages = editor.import_pages(imported.project)?;
            let mut warnings = imported.warnings;
            let packs = if let Some(project) = captured {
                match emulsion_io::document_stencils::save(
                    &emulsion_io::creative_library::root(),
                    &project,
                    "Diagram",
                ) {
                    Ok(ids) => ids,
                    Err(e) => {
                        warnings.push(format!(
                            "Diagram imported; stencil library could not be saved: {e}"
                        ));
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            Ok(json!({"pages":pages,"warnings":warnings,"stencil_packs":packs}))
        }
        "export_diagram" => {
            let project = editor.snapshot().ok_or("No project")?;
            if let Some(path) = args.get("path") {
                let path = Path::new(path.as_str().unwrap());
                if path.exists() && args["overwrite"] != true {
                    return Err("Destination exists; set overwrite=true to replace it".into());
                }
                emulsion_io::drawio::write(&project, path).map_err(|e| e.to_string())?;
                Ok(json!({"path":path,"pages":project.pages.len()}))
            } else {
                Ok(json!({"xml":emulsion_io::drawio::to_xml(&project).map_err(|e|e.to_string())?}))
            }
        }
        "generate_diagram" => {
            let format = match text(args, "format")? {
                "text" => emulsion_io::diagram_data::Format::Text,
                "csv" => emulsion_io::diagram_data::Format::Csv,
                "mermaid" => emulsion_io::diagram_data::Format::Mermaid,
                "d2" => emulsion_io::diagram_data::Format::D2,
                "graphviz" => emulsion_io::diagram_data::Format::Graphviz,
                "sql" => emulsion_io::diagram_data::Format::Sql,
                _ => return Err("Unknown data format".into()),
            };
            let draft = emulsion_io::diagram_data::parse(text(args, "text")?, format)
                .map_err(|e| e.to_string())?;
            if args["refresh"] == true {
                let commands = draft
                    .refresh_commands(&editor.doc)
                    .map_err(|e| e.to_string())?;
                editor.begin("Refresh diagram data");
                for command in commands {
                    if let Err(e) = editor.execute(command) {
                        editor.cancel();
                        return Err(e.to_string());
                    }
                }
                editor.end();
                Ok(json!({"refreshed":true}))
            } else {
                let doc = draft.document().map_err(|e| e.to_string())?;
                let page = editor.add_page(
                    doc,
                    args["name"].as_str().unwrap_or("Generated diagram").into(),
                    0.,
                )?;
                Ok(json!({"page":page,"warnings":draft.warnings}))
            }
        }
        "quick_create_diagram" => {
            let source = args["source"].as_u64().ok_or("source must be a node ID")?;
            let direction =
                serde_json::from_value(args["direction"].clone()).map_err(|e| e.to_string())?;
            let kind = serde_json::from_value(args["kind"].clone()).map_err(|e| e.to_string())?;
            let node = diagram::quick_create(editor, source, direction, kind)?;
            Ok(json!({"node":node}))
        }
        _ => Err("Unknown diagram project tool".into()),
    }
}
pub(crate) fn definitions() -> Vec<ToolDef> {
    let string = json!({"type":"string"});
    let boolean = json!({"type":"boolean"});
    let def = |name: &str, description: &str, properties: Value, required: &[&str]| ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    };
    vec![
        def(
            "create_diagram_link",
            "Copy a local saved-project reference to the current selection or view. View is [center_x,center_y,zoom,rotation].",
            json!({"nodes":{"type":"array","maxItems":1000,"items":{"type":"integer","minimum":1}},"view":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"number"}}}),
            &[],
        ),
        def(
            "open_diagram_link",
            "Navigate to a page, selection and view in the already-open matching project. Does not load external files or execute URLs.",
            json!({"link":string}),
            &["link"],
        ),
        def(
            "insert_diagram_pack_entry",
            "Place an installed stencil entry onto the active diagram, preserving editable artwork and connections. Use a path returned by list_diagram_stencil_packs; page is one-based (default 1). Optional center [x,y] defaults to the canvas center. One undo step.",
            json!({"path":string,"page":{"type":"integer","minimum":1},"center":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}}}),
            &["path"],
        ),
        def(
            "install_diagram_stencil_pack",
            "Install a local .emustencil, draw.io XML/library, supported Visio file or SVG folder into the reusable offline stencil catalog. Returns compatibility warnings.",
            json!({"path":{"type":"string"},"pack":{"type":"string","description":"Bundled pack ID from list_diagram_stencil_packs"}}),
            &[],
        ),
        def(
            "insert_diagram_template",
            "Add an offline editable diagram template as a new page. Discover IDs with list_diagram_library. One undo step.",
            json!({"template":{"type":"string"}}),
            &["template"],
        ),
        def(
            "save_document_stencils",
            "Save shapes from all project pages as permanent offline stencil packs. Preserves editable artwork, deduplicates repeat imports, and leaves the document unchanged.",
            json!({"name":{"type":"string","maxLength":150}}),
            &[],
        ),
        def(
            "import_diagram",
            "Add pages from local Mermaid, D2, Graphviz, Markdown code blocks, Glyphtide JSON, CSV, SQL, text, draw.io, supported Visio/Lucid or native stencil/template packs; alternatively supply draw.io XML. Returns compatibility warnings. One undo step.",
            json!({"path":string,"xml":string,"save_stencils":{"type":"boolean","default":false}}),
            &[],
        ),
        def(
            "export_diagram",
            "Export every project page as editable draw.io. Omit path to return XML; existing destinations require overwrite=true. Unsupported artwork fails explicitly. Use export_project for PDF or image archives and export_template_pack for portable stencils.",
            json!({"path":string,"overwrite":boolean}),
            &[],
        ),
        def(
            "generate_diagram",
            "Generate a new editable page from text, CSV, Mermaid, D2, Graphviz DOT or SQL schema. Returns compatibility warnings. Mermaid preserves its layout and source as scalable vector artwork; connections do not reroute. refresh=true updates native data-linked graphs on the current page in one undo step; re-import Mermaid source to update it.",
            json!({"format":{"type":"string","enum":["text","csv","mermaid","d2","graphviz","sql"]},"text":string,"name":string,"refresh":boolean}),
            &["format", "text"],
        ),
        def(
            "quick_create_diagram",
            "Add a connected neighboring shape in one undo step.",
            json!({"source":{"type":"integer","minimum":1},"direction":{"type":"string","enum":["north","east","south","west"]},"kind":{"type":"string","enum":["process","decision","terminator","data","document","database","note","class","entity","container","swimlane","cloud"]}}),
            &["source", "direction", "kind"],
        ),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::Document;
    fn call(e: &mut ProjectEditor, name: &str, args: Value) -> Value {
        let r = execute(e, name, &args);
        assert!(!r.is_error, "{name}: {:?}", r.content);
        serde_json::from_str(r.content[0]["text"].as_str().unwrap()).unwrap()
    }
    #[test]
    fn diagram_links_require_matching_saved_project_and_select_page() {
        let mut e = project();
        assert!(execute(&mut e, "create_diagram_link", &json!({})).is_error);
        e.path = Some(std::path::PathBuf::from("/tmp/diagram-link-test.emu"));
        let page = e.active_page();
        let link = call(&mut e, "create_diagram_link", json!({"view":[120,240,2,0]}))["link"]
            .as_str()
            .unwrap()
            .to_owned();
        let result = call(&mut e, "open_diagram_link", json!({"link":link}));
        assert_eq!(result["page"], page);
        assert_eq!(result["view"], json!([120., 240., 2., 0.]));
        e.path = Some(std::path::PathBuf::from("/tmp/other.emu"));
        assert!(execute(&mut e, "open_diagram_link", &json!({"link":link})).is_error);
        assert!(crate::tools::is_read_only("create_diagram_link"));
    }
    fn project() -> ProjectEditor {
        ProjectEditor::new_project(ProjectKind::Diagram, Document::new(800, 600)).unwrap()
    }
    #[test]
    fn source_engines_return_compatibility_notes_and_one_undo_step() {
        for (format, source) in [("d2", "a -> b"), ("graphviz", "digraph { a -> b }")] {
            let mut editor = project();
            let before = editor.doc.clone();
            let result = call(
                &mut editor,
                "generate_diagram",
                json!({"format":format,"text":source}),
            );
            assert!(!result["warnings"].as_array().unwrap().is_empty());
            assert_eq!(editor.page_list().len(), 2);
            assert_eq!(editor.doc.diagram.as_ref().unwrap().shapes.len(), 2);
            assert_eq!(editor.doc.diagram.as_ref().unwrap().edges.len(), 1);
            assert!(editor.undo());
            assert_eq!(editor.page_list().len(), 1);
            assert_eq!(editor.doc, before);
        }
    }
    #[test]
    fn mermaid_generation_preserves_vector_artwork_source_and_one_undo_step() {
        use emulsion_core::NodeKind;
        for source in [
            "flowchart LR\n A[Start] --> B[Finish]",
            "sequenceDiagram\nA->>B: Hello",
        ] {
            let mut editor = project();
            let before = editor.doc.clone();
            let result = call(
                &mut editor,
                "generate_diagram",
                json!({"format":"mermaid","text":source}),
            );
            assert!(result["warnings"].as_array().unwrap().iter().any(|w| {
                w.as_str()
                    .is_some_and(|w| w.contains("connections do not reroute"))
            }));
            assert_eq!(editor.page_list().len(), 2);
            editor.doc.validate().unwrap();
            let graph = editor.doc.diagram.as_ref().unwrap();
            assert_eq!(graph.shapes.len(), 1);
            assert!(graph.edges.is_empty());
            assert_eq!(
                graph.shapes.values().next().unwrap().data["source_format"],
                "mermaid"
            );
            assert!(editor.doc.nodes.iter().any(|n| n.visible
                && match &n.kind {
                    NodeKind::Path { path, style, .. } =>
                        !path.is_empty() && (style.fill.is_some() || style.stroke.is_some()),
                    NodeKind::Smart { .. } => true,
                    _ => false,
                }));
            assert!(
                !editor
                    .doc
                    .nodes
                    .iter()
                    .any(|n| matches!(n.kind, NodeKind::Raster { .. }))
            );
            let recovered: String = editor
                .doc
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
            let generated = editor.doc.clone();
            let stamp = editor.stamp();
            assert!(
                execute(
                    &mut editor,
                    "generate_diagram",
                    &json!({
                        "format":"mermaid", "text":source, "refresh":true,
                    })
                )
                .is_error
            );
            assert_eq!(editor.stamp(), stamp);
            assert_eq!(editor.doc, generated);
            assert!(editor.undo());
            assert_eq!(editor.page_list().len(), 1);
            assert_eq!(editor.doc, before);
            assert!(editor.redo());
            assert_eq!(editor.page_list().len(), 2);
            assert_eq!(editor.doc, generated);
        }
    }
    #[test]
    fn diagram_project_mcp_generation_import_export_undo() {
        let mut e = project();
        call(
            &mut e,
            "generate_diagram",
            json!({"format":"text","text":"Start -> Finish"}),
        );
        assert_eq!(e.page_list().len(), 2);
        assert_eq!(e.doc.diagram.as_ref().unwrap().shapes.len(), 2);
        assert_eq!(e.doc.diagram.as_ref().unwrap().edges.len(), 1);
        let generated = e.doc.clone();
        let source = *e
            .doc
            .diagram
            .as_ref()
            .unwrap()
            .shapes
            .keys()
            .next()
            .unwrap();
        call(
            &mut e,
            "quick_create_diagram",
            json!({"source":source,"direction":"south","kind":"class"}),
        );
        assert_eq!(e.doc.diagram.as_ref().unwrap().shapes.len(), 3);
        assert_eq!(e.doc.diagram.as_ref().unwrap().edges.len(), 2);
        assert!(e.undo());
        assert_eq!(e.doc.diagram.as_ref().unwrap().shapes.len(), 2);
        assert_eq!(e.doc, generated);
        let xml = call(&mut e, "export_diagram", json!({}));
        let mut target = project();
        let imported = call(&mut target, "import_diagram", json!({"xml":xml["xml"]}));
        assert_eq!(imported["stencil_packs"], json!([]));
        assert_eq!(target.page_list().len(), 3);
        target.undo();
        assert_eq!(target.page_list().len(), 1);
        target.redo();
        assert_eq!(target.page_list().len(), 3);
        let before = target.stamp();
        assert!(execute(&mut target, "import_diagram", &json!({"xml":"<broken>"})).is_error);
        assert_eq!(target.stamp(), before);
        assert!(
            execute(
                &mut target,
                "generate_diagram",
                &json!({"format":"text","text":"A -> B","refresh":"true"})
            )
            .is_error
        );
        for name in [
            "save_document_stencils",
            "import_diagram",
            "export_diagram",
            "generate_diagram",
            "quick_create_diagram",
        ] {
            assert_eq!(
                crate::tools::definitions()
                    .iter()
                    .filter(|d| d.name == name)
                    .count(),
                1
            );
            assert!(crate::tools::uses_native_history(name));
        }
    }
    #[test]
    fn diagram_pack_entry_preserves_connections_and_undo() {
        let doc = emulsion_core::diagram_library::TEMPLATES[0]
            .build()
            .unwrap();
        let source = ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap();
        let path =
            std::env::temp_dir().join(format!("emulsion-stencil-mcp-{}.emu", std::process::id()));
        emulsion_io::project::write(&source.snapshot().unwrap(), &path).unwrap();
        let mut e = project();
        let before = e.doc.clone();
        let result = call(
            &mut e,
            "insert_diagram_pack_entry",
            json!({"path":path,"center":[400.,300.]}),
        );
        assert!(!result["nodes"].as_array().unwrap().is_empty());
        assert_eq!(e.page_list().len(), 1);
        assert_eq!(
            e.doc.diagram.as_ref().unwrap().edges.len(),
            source.doc.diagram.as_ref().unwrap().edges.len()
        );
        e.doc.validate().unwrap();
        let stamp = e.stamp();
        assert!(
            execute(
                &mut e,
                "insert_diagram_pack_entry",
                &json!({"path":path,"page":0})
            )
            .is_error
        );
        assert_eq!(e.stamp(), stamp);
        assert!(e.undo());
        assert_eq!(e.doc, before);
        std::fs::remove_file(path).unwrap();
        assert!(crate::tools::uses_native_history(
            "insert_diagram_pack_entry"
        ));
    }

    #[test]
    fn template_insertion_adds_one_undoable_page() {
        let mut e = project();
        call(
            &mut e,
            "insert_diagram_template",
            json!({"template":"swimlanes"}),
        );
        assert_eq!(e.page_list().len(), 2);
        assert_eq!(e.doc.diagram.as_ref().unwrap().shapes.len(), 6);
        e.doc.validate().unwrap();
        assert!(e.undo());
        assert_eq!(e.page_list().len(), 1);
    }

    #[test]
    fn every_library_template_is_available_to_project_mcp() {
        for template in emulsion_core::diagram_library::TEMPLATES {
            let mut e = project();
            let before = e.doc.clone();
            call(
                &mut e,
                "insert_diagram_template",
                json!({"template":template.id}),
            );
            assert_eq!(e.page_list().len(), 2, "{}", template.id);
            e.doc.validate().unwrap();
            assert_eq!(
                e.doc.diagram.as_ref().unwrap().shapes.len(),
                template
                    .build()
                    .unwrap()
                    .diagram
                    .as_ref()
                    .unwrap()
                    .shapes
                    .len()
            );
            assert!(e.undo());
            assert_eq!(e.page_list().len(), 1);
            assert_eq!(e.doc, before);
            assert!(e.redo());
            assert_eq!(e.page_list().len(), 2);
        }
    }
}
