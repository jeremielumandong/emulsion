//! Live workspace lifecycle; tab IDs identify entities, never array positions.
use crate::server::ToolDef;
use emulsion_core::creation::{CanvasKind, CanvasSpec};
use serde::Deserialize;
use serde_json::{Value, json};
pub const NAMES: &[&str] = &[
    "get_workspace_tabs",
    "create_design_project",
    "select_workspace_tab",
    "close_workspace_tab",
    "create_canvas",
    "open_workspace_file",
];
pub const READ_ONLY: &[&str] = &["get_workspace_tabs"];
pub const DESTRUCTIVE: &[&str] = &[
    "create_design_project",
    "select_workspace_tab",
    "close_workspace_tab",
    "create_canvas",
    "open_workspace_file",
];
#[derive(Debug)]
pub enum Action {
    List,
    Create(CanvasSpec),
    Open(FileRequest),
    Select(u64),
    Close(u64),
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRequest {
    pub path: std::path::PathBuf,
    /// Optional Photo/Paint workspace for image documents.
    pub kind: Option<String>,
}
pub enum FileContent {
    Document(Box<emulsion_io::Opened>),
    Project(Box<emulsion_core::project::ProjectEditor>, Vec<String>),
}
pub struct LoadedFile {
    pub path: std::path::PathBuf,
    pub kind: Option<CanvasKind>,
    pub content: FileContent,
}
/// Run on a background executor; no workspace changes or catalog installs.
pub fn load_file(request: FileRequest) -> Result<LoadedFile, String> {
    let path = request.path.canonicalize().map_err(|e| e.to_string())?;
    if !path.is_file() {
        return Err("Choose a local artwork file".into());
    }
    if emulsion_io::photo_develop::is_raw_photo(&path) {
        return Err("Open RAW sources with add_library_photos and select_library_photos, then use develop_library or open_library_photo".into());
    }
    let kind = match request.kind.as_deref() {
        None => None,
        Some("photo") => Some(CanvasKind::Photo),
        Some("paint") => Some(CanvasKind::Paint),
        _ => return Err("kind must be photo or paint for image documents".into()),
    };
    let project = emulsion_io::project::is_project(&path);
    let content = if project
        || emulsion_io::pptx::is_pptx(&path)
        || emulsion_io::diagram_import::is_diagram(&path)
        || emulsion_io::template_pack::is_pack(&path)
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        if kind.is_some() {
            return Err("Omit kind for Design/Diagram project files".into());
        }
        let (project_data, warnings) = crate::project_tools::load_pages(&json!({"source":path}))?;
        FileContent::Project(
            Box::new(emulsion_core::project::ProjectEditor::open(
                project_data,
                project.then(|| path.clone()),
            )?),
            warnings,
        )
    } else {
        FileContent::Document(Box::new(
            emulsion_io::open_full(&path).map_err(|e| e.to_string())?,
        ))
    };
    Ok(LoadedFile {
        path,
        kind,
        content,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tab {
    tab_id: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    kind: String,
    name: String,
    width: u32,
    height: u32,
    #[serde(default = "one")]
    pages: usize,
    #[serde(default)]
    bleed_mm: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateCanvas {
    kind: String,
    name: String,
    width: f64,
    height: f64,
    unit: Option<String>,
    background: Option<String>,
    resolution: Option<f64>,
    depth: Option<u8>,
    #[serde(default = "one")]
    pages: usize,
    #[serde(default)]
    bleed_mm: f64,
}
fn one() -> usize {
    1
}
pub fn parse(name: &str, args: &Value) -> Result<Action, String> {
    match name {
        "get_workspace_tabs" if args.as_object().is_some_and(|a| a.is_empty()) => Ok(Action::List),
        "open_workspace_file" => {
            let request: FileRequest =
                serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            if !request.path.is_absolute() {
                return Err("Supply an absolute local file path".into());
            }
            if request
                .kind
                .as_deref()
                .is_some_and(|k| !matches!(k, "photo" | "paint"))
            {
                return Err("kind must be photo or paint for image documents".into());
            }
            Ok(Action::Open(request))
        }
        "create_canvas" => {
            use emulsion_core::creation::{Background, Unit};
            let a: CreateCanvas =
                serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            let spec = CanvasSpec {
                kind: match a.kind.as_str() {
                    "photo" => CanvasKind::Photo,
                    "paint" => CanvasKind::Paint,
                    "design" => CanvasKind::Design,
                    "diagram" => CanvasKind::Diagram,
                    _ => return Err("kind must be photo, paint, design or diagram".into()),
                },
                unit: match a.unit.as_deref().unwrap_or("pixels") {
                    "pixels" => Unit::Pixels,
                    "millimeters" => Unit::Millimeters,
                    "inches" => Unit::Inches,
                    _ => return Err("unit must be pixels, millimeters or inches".into()),
                },
                background: match a.background.as_deref().unwrap_or("white") {
                    "white" => Background::White,
                    "black" => Background::Black,
                    "transparent" => Background::Transparent,
                    "paper" => Background::Paper,
                    _ => return Err("background must be white, black, transparent or paper".into()),
                },
                name: a.name,
                width: a.width,
                height: a.height,
                resolution: a.resolution.unwrap_or(72.),
                depth: a.depth.unwrap_or(16),
                pages: a.pages,
                bleed_mm: a.bleed_mm,
            };
            spec.validate()?;
            Ok(Action::Create(spec))
        }
        "create_design_project" => {
            let a: Create = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            let kind = match a.kind.as_str() {
                "design" => CanvasKind::Design,
                "diagram" => CanvasKind::Diagram,
                _ => return Err("kind must be design or diagram".into()),
            };
            let spec = CanvasSpec {
                kind,
                name: a.name,
                width: a.width as f64,
                height: a.height as f64,
                pages: a.pages,
                bleed_mm: a.bleed_mm,
                ..Default::default()
            };
            spec.validate()?;
            Ok(Action::Create(spec))
        }
        "select_workspace_tab" | "close_workspace_tab" => {
            let a: Tab = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            if a.tab_id == 0 {
                return Err("Choose a nonzero tab_id returned by get_workspace_tabs".into());
            }
            Ok(if name == "select_workspace_tab" {
                Action::Select(a.tab_id)
            } else {
                Action::Close(a.tab_id)
            })
        }
        _ => Err("Unknown workspace tool or unsupported arguments".into()),
    }
}
pub fn definitions() -> Vec<ToolDef> {
    let def = |name: &str, description: &str, properties: Value, required: &[&str]| ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
    };
    let tab = json!({"tab_id":{"type":"integer","minimum":1}});
    vec![
        def(
            "open_workspace_file",
            "Open an existing local image, layered artwork (.ora/.psd), native .emu project or supported editable diagram/presentation in a workspace tab. Decodes in the background and returns the opened tab ID after installation, preserving unsaved tabs and relay origin. Optional kind photo/paint applies to image documents. RAW sources use add_library_photos/select_library_photos. Template packs open as copies without installing into the catalog. Import warnings are returned.",
            json!({"path":{"type":"string","minLength":1,"description":"Absolute local file path"},"kind":{"type":"string","enum":["photo","paint"]}}),
            &["path"],
        ),
        def(
            "create_canvas",
            "Create a Photo, Paint, Design or Diagram canvas in a new workspace tab, using native size, resolution, background and depth validation. Paint opens drawing tools. Existing unsaved tabs remain open; the originating relay remains bound to its original tab. Multiple pages require Design or Diagram.",
            json!({"kind":{"type":"string","enum":["photo","paint","design","diagram"]},"name":{"type":"string","minLength":1,"maxLength":200},"width":{"type":"number","exclusiveMinimum":0},"height":{"type":"number","exclusiveMinimum":0},"unit":{"type":"string","enum":["pixels","millimeters","inches"],"default":"pixels"},"resolution":{"type":"number","minimum":1,"maximum":9600,"default":72},"depth":{"type":"integer","enum":[8,16],"default":16},"background":{"type":"string","enum":["white","black","transparent","paper"],"default":"white"},"pages":{"type":"integer","minimum":1,"maximum":100,"default":1},"bleed_mm":{"type":"number","minimum":0,"maximum":100}}),
            &["kind", "name", "width", "height"],
        ),
        def(
            NAMES[0],
            "List stable workspace tab IDs, current tab, originating relay tab, project kinds and unsaved state. Selecting another tab never retargets the originating relay.",
            json!({}),
            &[],
        ),
        def(
            NAMES[1],
            "Create a native Design or Diagram project in a new tab. Existing unsaved tabs remain open. The originating MCP relay stays bound to its original document.",
            json!({"kind":{"type":"string","enum":["design","diagram"]},"name":{"type":"string","minLength":1,"maxLength":200},"width":{"type":"integer","minimum":1,"maximum":30000},"height":{"type":"integer","minimum":1,"maximum":30000},"pages":{"type":"integer","minimum":1,"maximum":100,"default":1},"bleed_mm":{"type":"number","minimum":0,"maximum":100}}),
            &["kind", "name", "width", "height"],
        ),
        def(
            NAMES[2],
            "Select a workspace tab by stable ID. Does not redirect this relay's future editing calls.",
            tab.clone(),
            &["tab_id"],
        ),
        def(
            NAMES[3],
            "Close a saved, idle tab. Unsaved changes, active edits and pending work are rejected; save in the owning editor first. Never discards artwork or accepts a force flag.",
            tab,
            &["tab_id"],
        ),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_open_requires_explicit_local_paths_and_image_modes() {
        for args in [
            json!({}),
            json!({"path":"relative.ora"}),
            json!({"path":"https://example.com/a.ora"}),
            json!({"path":std::env::temp_dir().join("a.ora"),"kind":"design"}),
            json!({"path":std::env::temp_dir().join("a.ora"),"discard":true}),
        ] {
            assert!(parse("open_workspace_file", &args).is_err(), "{args}");
        }
        assert!(matches!(
            parse(
                "open_workspace_file",
                &json!({"path":std::env::temp_dir().join("a.ora"),"kind":"paint"})
            )
            .unwrap(),
            Action::Open(_)
        ));
    }
    #[test]
    fn native_canvas_requests_validate_units_limits_and_mode() {
        for kind in ["photo", "paint", "design", "diagram"] {
            let Action::Create(spec) = parse("create_canvas", &json!({"kind":kind,"name":"Test","width":2,"height":1,"unit":"inches","resolution":300})).unwrap() else { panic!() };
            assert_eq!(spec.pixel_size().unwrap(), (600, 300));
        }
        for extra in [
            json!({"pages":2}),
            json!({"depth":32}),
            json!({"width":0}),
            json!({"resolution":0}),
            json!({"unit":"feet"}),
            json!({"background":"unknown"}),
            json!({"width":30000,"height":30000}),
        ] {
            let mut args = json!({"kind":"paint","name":"Test","width":100,"height":100});
            args.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert!(parse("create_canvas", &args).is_err(), "{args}");
        }
    }
    #[test]
    fn workspace_arguments_are_bounded_and_never_allow_discard() {
        assert!(parse("close_workspace_tab", &json!({"tab_id":1,"force":true})).is_err());
        assert!(parse("get_workspace_tabs", &json!({"tab_id":1})).is_err());
        for args in [
            json!({"kind":"photo","name":"x","width":10,"height":10}),
            json!({"kind":"design","name":"x","width":0,"height":10}),
            json!({"kind":"design","name":"x","width":10,"height":10,"pages":emulsion_core::project::MAX_PAGES + 1}),
        ] {
            assert!(parse("create_design_project", &args).is_err());
        }
        assert!(matches!(
            parse(
                "create_design_project",
                &json!({"kind":"diagram","name":"Flow","width":800,"height":600})
            )
            .unwrap(),
            Action::Create(_)
        ));
    }
}
