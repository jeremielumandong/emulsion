//! Project-aware tools. The live host owns page switching and asynchronous IO.
use crate::{ToolDef, ToolResult};
use emulsion_core::{creation::CanvasSpec, design::Template, project::ProjectEditor};
use serde_json::{Value, json};

pub const READ_ONLY: &[&str] = &[
    "describe_project",
    "list_design_templates",
    "list_design_data_fields",
    "list_project_design_assets",
];
pub const DESTRUCTIVE: &[&str] = &["delete_project_page"];
pub const IO_TOOLS: &[&str] = &[
    "save_project",
    "export_project",
    "export_template_pack",
    "import_project_pages",
];

fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    }
}
pub fn definitions() -> Vec<ToolDef> {
    let id =
        json!({"type":"integer","minimum":1,"description":"Stable page ID from describe_project."});
    let size = json!({"type":"integer","minimum":1,"maximum":30000});
    let path = json!({"type":"string","minLength":1});
    vec![
        def("generate_diagram_page", "Generate an editable diagram page from bounded text flow, CSV, Mermaid flowchart or SQL schema source. Parsers do not execute SQL or fetch resources. Keeps existing pages; one Undo step.", json!({"source":{"type":"string","minLength":1,"maxLength":1048576},"format":{"enum":["text","csv","mermaid","sql"]},"name":{"type":"string","minLength":1,"maxLength":200}}), &["source","format","name"]),
        def(
            "list_project_design_assets",
            "List reusable component variants and saved style names across every project page. Use stable page IDs to reuse assets on the active page.",
            json!({}),
            &[],
        ),
        def(
            "insert_component_from_page",
            "Import a component definition from another project page and place a linked instance on the active page as one Undo step. Local definitions stay independent between pages; offsets default to 24 px.",
            json!({"page":id,"name":{"type":"string","minLength":1,"maxLength":80},"variant":{"type":"string","minLength":1,"maxLength":80},"x_offset":{"type":"number","minimum":-1000000,"maximum":1000000},"y_offset":{"type":"number","minimum":-1000000,"maximum":1000000}}),
            &["page", "name"],
        ),
        def(
            "apply_style_from_page",
            "Import a saved appearance style from another project page and apply it to active-page nodes in one Undo step. Name collisions use a local independent definition.",
            json!({"page":id,"name":{"type":"string","minLength":1,"maxLength":80},"nodes":{"type":"array","items":id,"minItems":1,"uniqueItems":true}}),
            &["page", "name", "nodes"],
        ),
        def(
            "describe_project",
            "Inspect project type, active page, all page IDs/names/sizes, bleed, and global Undo/Redo availability. Requires the live workspace.",
            json!({}),
            &[],
        ),
        def(
            "list_design_templates",
            "List bundled editable starter templates and their IDs, categories and native sizes.",
            json!({}),
            &[],
        ),
        def(
            "select_project_page",
            "Select an existing page for subsequent object tools. Preserves page content and clears stale UI selections.",
            json!({"page":id}),
            &["page"],
        ),
        def(
            "add_project_page",
            "Add and select a blank editable page; dimensions default to the active page. Undoable.",
            json!({"name":{"type":"string","minLength":1,"maxLength":200},"width":size,"height":size}),
            &["name"],
        ),
        def(
            "add_template_page",
            "Add and select a bundled starter template as an editable page. Use list_design_templates for IDs. Keeps existing pages.",
            json!({"template":{"type":"integer","minimum":0},"width":size,"height":size}),
            &["template"],
        ),
        def(
            "duplicate_project_page",
            "Duplicate and select a page, retaining editable contents. Undoable.",
            json!({"page":id}),
            &["page"],
        ),
        def(
            "delete_project_page",
            "Remove a page, preserving it in project Undo. The last page cannot be removed.",
            json!({"page":id}),
            &["page"],
        ),
        def(
            "set_project_page",
            "Rename a page and optionally set print bleed in millimeters. Undoable.",
            json!({"page":id,"name":{"type":"string","minLength":1,"maxLength":200},"bleed_mm":{"type":"number","minimum":0,"maximum":100}}),
            &["page", "name"],
        ),
        def(
            "move_project_page",
            "Move a page to a zero-based index in project order. Undoable.",
            json!({"page":id,"index":{"type":"integer","minimum":0,"maximum":99}}),
            &["page", "index"],
        ),
        def(
            "resize_design_page",
            "Create a resized copy of the active page using native responsive constraints. Reports overflow and keeps the original page. Undoable.",
            json!({"width":size,"height":size}),
            &["width", "height"],
        ),
        def(
            "list_design_data_fields",
            "List {{field}} placeholders in active page text for CSV bulk generation.",
            json!({}),
            &[],
        ),
        def(
            "generate_design_pages",
            "Generate editable pages from bounded CSV data and active page {{field}} placeholders. Imports the generated pages as one Undo step.",
            json!({"csv":{"type":"string","maxLength":2097152}}),
            &["csv"],
        ),
        def(
            "save_project",
            "Save all editable pages and history atomically as .emu. Requires an open Design/Diagram project. Marks only the saved snapshot clean.",
            json!({"path":path}),
            &["path"],
        ),
        def(
            "export_project",
            "Export selected pages, or all when pages is omitted, to PDF, animated GIF, editable draw.io, or a PNG/JPEG/SVG page archive. Returns any rasterization warnings; source is unchanged.",
            json!({"path":path,"format":{"enum":["pdf","png","jpeg","svg","gif","drawio"]},"pages":{"type":"array","items":id,"minItems":1,"uniqueItems":true},"include_bleed":{"type":"boolean"}}),
            &["path", "format"],
        ),
        def(
            "export_template_pack",
            "Author a shareable .emutemplate or .emustencil file from every editable project page. Kind follows the Design/Diagram project. Does not publish online.",
            json!({"path":path,"name":{"type":"string","minLength":1,"maxLength":200},"author":{"type":"string"},"license":{"type":"string"},"description":{"type":"string"}}),
            &["path", "name"],
        ),
        def(
            "import_project_pages",
            "Import pages from a local .emu, template/stencil pack, draw.io, supported Visio or Lucid file; or a GitHub template/stencil package URL. Data only; no repository scripts. Keeps existing pages and returns compatibility warnings. One Undo step.",
            json!({"source":path}),
            &["source"],
        ),
    ]
}
pub fn is_tool(name: &str) -> bool {
    definitions().iter().any(|d| d.name == name)
}

pub fn validate_args(name: &str, args: &Value) -> Result<(), String> {
    let def = definitions()
        .into_iter()
        .find(|d| d.name == name)
        .ok_or("Unknown project tool")?;
    let object = args.as_object().ok_or("Arguments must be an object")?;
    let props = def.input_schema["properties"].as_object().unwrap();
    if object.keys().any(|key| !props.contains_key(key)) {
        return Err("Unknown project tool argument".into());
    }
    for key in def.input_schema["required"].as_array().unwrap() {
        if !object.contains_key(key.as_str().unwrap()) {
            return Err(format!("Missing argument {key}"));
        }
    }
    for (key, value) in object {
        let schema = &props[key];
        let valid = match schema["type"].as_str() {
            Some("string") => value.as_str().is_some_and(|s| {
                schema["minLength"]
                    .as_u64()
                    .is_none_or(|min| s.chars().count() >= min as usize)
                    && schema["maxLength"]
                        .as_u64()
                        .is_none_or(|max| s.len() <= max as usize)
            }),
            Some("integer") => value.as_u64().is_some_and(|n| {
                schema["minimum"].as_u64().is_none_or(|min| n >= min)
                    && schema["maximum"].as_u64().is_none_or(|max| n <= max)
            }),
            Some("number") => value.as_f64().is_some_and(|n| {
                n.is_finite()
                    && schema["minimum"].as_f64().is_none_or(|min| n >= min)
                    && schema["maximum"].as_f64().is_none_or(|max| n <= max)
            }),
            Some("boolean") => value.is_boolean(),
            Some("array") => value.as_array().is_some_and(|a| {
                !a.is_empty() && a.iter().all(|v| v.as_u64().is_some_and(|n| n > 0))
            }),
            _ => true,
        };
        if !valid
            || schema["enum"]
                .as_array()
                .is_some_and(|items| !items.contains(value))
        {
            return Err(format!("Invalid argument '{key}'"));
        }
    }
    Ok(())
}
pub fn execute(editor: &mut ProjectEditor, name: &str, args: &Value) -> ToolResult {
    match run(editor, name, args) {
        Ok(value) => ToolResult::text(value.to_string()),
        Err(error) => ToolResult::error(error),
    }
}
fn run(editor: &mut ProjectEditor, name: &str, args: &Value) -> Result<Value, String> {
    validate_args(name, args)?;
    if name == "list_design_templates" {
        return Ok(
            json!({"templates":Template::catalog().enumerate().map(|(id,t)| json!({"id":id,"name":t.label(),"category":t.category().map(|i|Template::CATEGORIES[i].label),"size":t.native_size()})).collect::<Vec<_>>()}),
        );
    }
    if name == "describe_project" {
        return Ok(
            json!({"kind":editor.kind(),"active_page":editor.active_page(),"modified":editor.is_modified(),"can_undo":editor.can_undo(),"can_redo":editor.can_redo(),"pages":editor.page_list().iter().map(|p| {let d=&editor.page(p.id).unwrap().doc;json!({"id":p.id,"name":p.name,"bleed_mm":p.bleed_mm,"width":d.width,"height":d.height})}).collect::<Vec<_>>()}),
        );
    }
    if name == "list_project_design_assets" {
        return Ok(json!({"pages":editor.page_list().iter().map(|p| {
            let d=&editor.page(p.id).unwrap().doc.design;
            json!({"page":p.id,"components":d.components,"styles":d.saved_styles.keys().collect::<Vec<_>>()})
        }).collect::<Vec<_>>()}));
    }
    if editor.kind().is_none() {
        return Err("Open a Design or Diagram project first".into());
    }
    if name == "list_design_data_fields" {
        return emulsion_io::design_bulk::fields(&editor.doc)
            .map(|fields| json!({"fields":fields}))
            .map_err(|e| e.to_string());
    }
    if editor.in_transaction() {
        return Err("Finish the current edit before changing project pages".into());
    }
    let page = args["page"].as_u64().unwrap_or(editor.active_page());
    let mut extra = json!({});
    match name {
        "generate_diagram_page" => {
            use emulsion_io::diagram_data::{self, Format};
            let format=match args["format"].as_str().unwrap() {"text"=>Format::Text,"csv"=>Format::Csv,"mermaid"=>Format::Mermaid,_=>Format::Sql};
            let doc=diagram_data::parse(args["source"].as_str().unwrap(),format).and_then(|d|d.document()).map_err(|e|e.to_string())?;
            editor.add_page(doc,args["name"].as_str().unwrap().into(),0.)?;
        }
        "insert_component_from_page" => {
            let source = editor.page(page).ok_or("Unknown source page")?.doc.clone();
            let id = emulsion_core::design_components::import_and_insert(
                editor,
                &source,
                args["name"].as_str().unwrap(),
                args["variant"].as_str().unwrap_or("Default"),
                (
                    args["x_offset"].as_f64().unwrap_or(24.),
                    args["y_offset"].as_f64().unwrap_or(24.),
                ),
            )?;
            extra = json!({"node":id});
        }
        "apply_style_from_page" => {
            let name = args["name"].as_str().unwrap();
            let style = editor
                .page(page)
                .ok_or("Unknown source page")?
                .doc
                .design
                .saved_styles
                .get(name)
                .ok_or("Unknown source style")?
                .clone();
            let ids = args["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_u64().unwrap())
                .collect::<Vec<_>>();
            extra = json!({"name":emulsion_core::design_styles::apply(editor,&ids,name,&style)?});
        }
        "select_project_page" => editor.set_active_page(page)?,
        "add_project_page" | "add_template_page" => {
            let template = if name == "add_template_page" {
                Some(
                    Template::catalog()
                        .nth(
                            usize::try_from(args["template"].as_u64().unwrap())
                                .map_err(|_| "Invalid template")?,
                        )
                        .ok_or("Unknown template ID")?,
                )
            } else {
                None
            };
            let (w, h) =
                template.map_or((editor.doc.width, editor.doc.height), |t| t.native_size());
            let width = args["width"].as_u64().map_or(w, |v| v as u32);
            let height = args["height"].as_u64().map_or(h, |v| v as u32);
            let doc = if let Some(t) = template {
                t.create(width, height)?
            } else {
                CanvasSpec {
                    width: width as f64,
                    height: height as f64,
                    ..Default::default()
                }
                .create()?
            };
            let label = template.map_or_else(|| args["name"].as_str().unwrap(), |t| t.label());
            editor.add_page(doc, label.into(), 0.)?;
        }
        "duplicate_project_page" => {
            editor.duplicate_page(page)?;
        }
        "delete_project_page" => editor.remove_page(page)?,
        "move_project_page" => editor.move_page(page, args["index"].as_u64().unwrap() as usize)?,
        "set_project_page" => {
            let bleed = args["bleed_mm"].as_f64().unwrap_or(
                editor
                    .page_list()
                    .iter()
                    .find(|p| p.id == page)
                    .ok_or("Unknown page")?
                    .bleed_mm,
            );
            editor.rename_page(page, args["name"].as_str().unwrap().into(), bleed)?;
        }
        "resize_design_page" => {
            let width = args["width"].as_u64().unwrap() as u32;
            let height = args["height"].as_u64().unwrap() as u32;
            let resized =
                emulsion_core::design_metadata::resize_variant(&editor.doc, width, height)?;
            extra = json!({"overflow":resized.overflow});
            editor.add_page(resized.doc, format!("{width} × {height} variant"), 0.)?;
        }
        "generate_design_pages" => {
            let generated = emulsion_io::design_bulk::generate(
                &editor.doc,
                args["csv"].as_str().unwrap(),
                emulsion_core::project::MAX_PAGES.saturating_sub(editor.page_list().len()),
            )
            .map_err(|e| e.to_string())?;
            extra = json!({"pages":editor.import_pages(generated)?});
        }
        _ => return Err("This project operation requires the asynchronous live host".into()),
    }
    Ok(json!({"active_page":editor.active_page(),"result":extra}))
}

/// Read and decode off the UI thread. The host revalidates the target before import.
pub fn load_pages(args: &Value) -> Result<(emulsion_core::project::Project, Vec<String>), String> {
    validate_args("import_project_pages", args)?;
    let source = args["source"].as_str().unwrap();
    let path = std::path::Path::new(source);
    if source.starts_with("https://") {
        let pack =
            emulsion_io::template_pack::download_github(source).map_err(|e| e.to_string())?;
        Ok((pack.project, vec![]))
    } else if emulsion_io::template_pack::is_pack(path) {
        let pack = emulsion_io::template_pack::read(path).map_err(|e| e.to_string())?;
        Ok((pack.project, vec![]))
    } else if emulsion_io::project::is_project(path) {
        emulsion_io::project::read(path)
            .map(|p| (p, vec![]))
            .map_err(|e| e.to_string())
    } else {
        emulsion_io::diagram_import::read(path)
            .map(|i| (i.project, i.warnings))
            .map_err(|e| e.to_string())
    }
}

pub fn write_snapshot(
    project: &emulsion_core::project::Project,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    validate_args(name, args)?;
    let path = std::path::Path::new(args["path"].as_str().ok_or("Missing path")?);
    match name {
        "save_project" => {
            if !emulsion_io::project::is_project(path) {
                return Err("Project save path must end in .emu".into());
            }
            emulsion_io::project::write(project, path).map_err(|e| e.to_string())?;
        }
        "export_project" => {
            if args["format"] == "drawio" {
                if args.get("pages").is_some() || args["include_bleed"].as_bool().unwrap_or(false) {return Err("draw.io export uses all pages and does not support print bleed".into());}
                emulsion_io::drawio::write(project,path).map_err(|e|e.to_string())?;
                return Ok(json!({"path":path,"pages":project.pages.len()}));
            }
            if args["format"] == "gif" {
                if args.get("pages").is_some() || args["include_bleed"].as_bool().unwrap_or(false) {
                    return Err("GIF uses all pages and does not support print bleed".into());
                }
                let frames = emulsion_io::project_animation::write_gif(project, path)
                    .map_err(|e| e.to_string())?;
                return Ok(
                    json!({"path":path,"frames":frames,"video":"Embedded videos export their poster, not playback"}),
                );
            }
            use emulsion_io::project_export::{self, Format};
            let format = match args["format"].as_str().unwrap() {
                "pdf" => Format::Pdf,
                "png" => Format::Png,
                "jpeg" => Format::Jpeg,
                _ => Format::Svg,
            };
            let pages = args["pages"].as_array().map_or_else(
                || project.pages.iter().map(|p| p.meta.id).collect(),
                |a| a.iter().map(|v| v.as_u64().unwrap()).collect::<Vec<_>>(),
            );
            let report = project_export::write(
                project,
                &pages,
                format,
                args["include_bleed"].as_bool().unwrap_or(false),
                path,
            )
            .map_err(|e| e.to_string())?;
            return Ok(
                json!({"path":path,"pages":report.pages,"rasterized_pages":report.rasterized_pages}),
            );
        }
        "export_template_pack" => {
            use emulsion_io::template_pack::{self, Kind, Manifest};
            let kind = match project.kind {
                emulsion_core::project::ProjectKind::Design => Kind::Design,
                _ => Kind::Stencil,
            };
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(kind.extension()))
            {
                return Err(format!("Package path must end in .{}", kind.extension()));
            }
            let mut manifest = Manifest::new(kind, args["name"].as_str().unwrap().into());
            manifest.author = args["author"].as_str().unwrap_or_default().into();
            manifest.license = args["license"].as_str().unwrap_or_default().into();
            manifest.description = args["description"].as_str().unwrap_or_default().into();
            template_pack::write(project, &manifest, path).map_err(|e| e.to_string())?;
        }
        _ => return Err("Unknown project output tool".into()),
    }
    Ok(json!({"path":path}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Document, project::ProjectKind};
    #[test]
    fn page_tools_preserve_global_undo_and_validate_before_mutation() {
        let mut editor =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(64, 64)).unwrap();
        let original = editor.active_page();
        assert!(
            !execute(
                &mut editor,
                "add_project_page",
                &json!({"name":"Second","width":80})
            )
            .is_error
        );
        let second = editor.active_page();
        assert_ne!(original, second);
        assert_eq!(editor.doc.width, 80);
        assert!(editor.undo());
        assert_eq!(editor.page_list().len(), 1);
        assert_eq!(editor.active_page(), original);
        assert!(editor.redo());
        assert_eq!(editor.active_page(), second);
        let stamp = editor.stamp();
        for args in [
            json!({"name":"Bad","width":0}),
            json!({"name":"Bad","height":2.5}),
            json!({"name":"Bad","extra":true}),
        ] {
            assert!(execute(&mut editor, "add_project_page", &args).is_error);
            assert_eq!(editor.stamp(), stamp);
        }
        assert!(!execute(&mut editor, "delete_project_page", &json!({"page":second})).is_error);
        assert!(
            execute(
                &mut editor,
                "delete_project_page",
                &json!({"page":original})
            )
            .is_error
        );
        assert!(editor.undo());
        assert_eq!(editor.page_list().len(), 2);
    }
    #[test]
    fn save_roundtrip_keeps_every_page_and_rejects_flattening_extension() {
        let mut editor =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(64, 64)).unwrap();
        editor
            .add_page(Document::new(80, 40), "Second".into(), 2.)
            .unwrap();
        let path =
            std::env::temp_dir().join(format!("emulsion-mcp-project-{}.emu", std::process::id()));
        let project = editor.snapshot().unwrap();
        assert!(
            write_snapshot(
                &project,
                "save_project",
                &json!({"path":path.with_extension("ora")})
            )
            .is_err()
        );
        write_snapshot(&project, "save_project", &json!({"path":path})).unwrap();
        let restored = emulsion_io::project::read(&path).unwrap();
        assert_eq!(restored.pages.len(), 2);
        assert_eq!(restored.active, editor.active_page());
        assert_eq!(restored.pages[1].meta.bleed_mm, 2.);
        std::fs::remove_file(path).unwrap();
    }
}
