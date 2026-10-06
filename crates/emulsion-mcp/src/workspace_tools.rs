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
    "create_storyboard_from_template",
];
pub const READ_ONLY: &[&str] = &["get_workspace_tabs"];
pub const DESTRUCTIVE: &[&str] = &[
    "create_design_project",
    "select_workspace_tab",
    "close_workspace_tab",
    "create_canvas",
    "open_workspace_file",
    "create_storyboard_from_template",
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
    /// Open the project as an unsaved copy with this name: a new storyboard
    /// from a template.
    #[serde(skip)]
    pub copy_as: Option<String>,
}
pub enum FileContent {
    Document(Box<emulsion_io::Opened>),
    Project(Box<emulsion_core::project::ProjectEditor>, Vec<String>),
}
pub struct LoadedFile {
    pub path: std::path::PathBuf,
    /// The tab name of an unsaved copy.
    pub copy_as: Option<String>,
    pub kind: Option<CanvasKind>,
    pub content: FileContent,
    /// Evidence from the same PSD/PSB decode, never a follow-up import.
    pub psd_report: Option<emulsion_io::psd::ReadReport>,
}
/// Run on a background executor; no workspace changes or catalog installs.
pub fn load_file(request: FileRequest) -> Result<LoadedFile, String> {
    let path = request.path.canonicalize().map_err(|e| e.to_string())?;
    if let Some(name) = request.copy_as {
        // A copy has no path, so saving asks where; history starts fresh.
        let opened = emulsion_io::project::read_with_report(&path).map_err(|e| e.to_string())?;
        let session = emulsion_core::project::ProjectEditor::open(opened.project, None)?;
        return Ok(LoadedFile {
            path,
            copy_as: Some(name),
            kind: Some(CanvasKind::Storyboard),
            content: FileContent::Project(Box::new(session), opened.report.warnings()),
            psd_report: None,
        });
    }
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
    let (content, psd_report) = if project
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
        (
            FileContent::Project(
                Box::new(emulsion_core::project::ProjectEditor::open(
                    project_data,
                    project.then(|| path.clone()),
                )?),
                warnings,
            ),
            None,
        )
    } else {
        let (opened, report) =
            emulsion_io::open_full_with_report(&path).map_err(|e| e.to_string())?;
        (FileContent::Document(Box::new(opened)), report)
    };
    Ok(LoadedFile {
        path,
        copy_as: None,
        kind,
        content,
        psd_report,
    })
}
/// Stable machine-readable evidence for a newly installed PSD/PSB document.
/// `current_appearance_only` bounds every comparison; no gamma preference or
/// future-edit equivalence is inferred.
pub fn psd_report_value(report: emulsion_io::psd::ReadReport) -> Value {
    use emulsion_io::psd::ImportProfileDecision;
    let (decision, ambiguous) = match report.profile_decision {
        ImportProfileDecision::NotCompared => ("not_compared", None),
        ImportProfileDecision::UniquePhotoshopSrgbV1 => ("photoshop_srgb_v1_selected", Some(false)),
        ImportProfileDecision::LegacyMatch { ambiguous } => ("legacy_srgb_match", Some(ambiguous)),
        ImportProfileDecision::SameCurrentAppearance => ("same_current_appearance", None),
        ImportProfileDecision::SavedAppearance => ("saved_appearance", None),
    };
    json!({"decision":decision,"ambiguous":ambiguous,"background_preserved":report.background_preserved,"current_appearance_only":true})
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FromTemplate {
    template: u64,
    name: String,
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
                    "storyboard" => CanvasKind::Storyboard,
                    _ => {
                        return Err(
                            "kind must be photo, paint, design, diagram or storyboard".into()
                        );
                    }
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
                "storyboard" => CanvasKind::Storyboard,
                _ => return Err("kind must be design, diagram or storyboard".into()),
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
        "create_storyboard_from_template" => {
            let a: FromTemplate =
                serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            let name = a.name.trim().to_string();
            if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
                return Err("Enter a name of 1–200 characters".into());
            }
            let path = crate::storyboard_tools::template_path(
                &emulsion_io::creative_library::root(),
                a.template,
            )?;
            Ok(Action::Open(FileRequest {
                path,
                kind: None,
                copy_as: Some(name),
            }))
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
            "Create a Photo, Paint, Design, Diagram or Storyboard canvas in a new workspace tab, using native size, resolution, background and depth validation. Paint opens drawing tools. Existing unsaved tabs remain open; the originating relay remains bound to its original tab. Multiple pages require Design or Diagram.",
            json!({"kind":{"type":"string","enum":["photo","paint","design","diagram","storyboard"]},"name":{"type":"string","minLength":1,"maxLength":200},"width":{"type":"number","exclusiveMinimum":0},"height":{"type":"number","exclusiveMinimum":0},"unit":{"type":"string","enum":["pixels","millimeters","inches"],"default":"pixels"},"resolution":{"type":"number","minimum":1,"maximum":9600,"default":72},"depth":{"type":"integer","enum":[8,16],"default":16},"background":{"type":"string","enum":["white","black","transparent","paper"],"default":"white"},"pages":{"type":"integer","minimum":1,"maximum":100,"default":1},"bleed_mm":{"type":"number","minimum":0,"maximum":100}}),
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
            "Create a native Design, Diagram or Storyboard project in a new tab. A storyboard's pages are its panels, all at the given resolution; then use the storyboard tools. Existing unsaved tabs remain open. The originating MCP relay stays bound to its original document.",
            json!({"kind":{"type":"string","enum":["design","diagram","storyboard"]},"name":{"type":"string","minLength":1,"maxLength":200},"width":{"type":"integer","minimum":1,"maximum":30000},"height":{"type":"integer","minimum":1,"maximum":30000},"pages":{"type":"integer","minimum":1,"maximum":100,"default":1},"bleed_mm":{"type":"number","minimum":0,"maximum":100}}),
            &["kind", "name", "width", "height"],
        ),
        def(
            "create_storyboard_from_template",
            "Start a new storyboard from an installed storyboard template (list_storyboard_templates): a new tab holding an unsaved copy with the template's resolution, frame rate, caption fields, naming, Smart add layers, stage guides, palette, library and panels, with fresh history. The originating MCP relay stays bound to its original document.",
            json!({"template":{"type":"integer","minimum":1},"name":{"type":"string","minLength":1,"maxLength":200}}),
            &["template", "name"],
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
    struct TestDirectory(std::path::PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "emulsion-project-recovery-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn native_recovery_fixture(missing: bool) -> emulsion_core::project::Project {
        use emulsion_core::{
            Document,
            project::{ProjectEditor, ProjectKind},
            storyboard::Panel,
        };
        let mut editor =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        let blank = editor.storyboard().unwrap().blank_panel().unwrap();
        let retired = editor
            .insert_panels(
                Some(1),
                &blank,
                vec![("Retired".into(), Panel::new(0, 24))],
                None,
            )
            .unwrap()[0];
        editor.create_board_version("Before removal").unwrap();
        editor.set_active_page(1).unwrap();
        editor.remove_page(retired).unwrap();
        let mut project = editor.snapshot().unwrap();
        if missing {
            project
                .storyboard
                .as_mut()
                .unwrap()
                .versions
                .retired
                .clear();
        }
        project
    }

    #[test]
    fn native_pages_workspace_open_and_template_copy_return_the_same_read_report() {
        let dir = TestDirectory::new();
        for missing in [false, true] {
            let path = dir.0.join(format!("source-{missing}.emu"));
            emulsion_io::project::write(&native_recovery_fixture(missing), &path).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            let opened = emulsion_io::project::read_with_report(&path).unwrap();
            let warnings = opened.report.warnings();
            assert_eq!(warnings.len(), usize::from(missing));
            let from_reader =
                emulsion_io::project::read_from_with_report(std::io::Cursor::new(&bytes)).unwrap();
            assert_eq!(from_reader.report, opened.report);
            let (pages, notes) = crate::project_tools::load_pages(&json!({"source":path})).unwrap();
            assert_eq!(notes, warnings);
            assert_eq!(pages.pages.len(), 1);
            assert_eq!(pages.storyboard.as_ref().unwrap().versions.list.len(), 1);
            for copy in [false, true] {
                let loaded = load_file(FileRequest {
                    path: path.clone(),
                    kind: None,
                    copy_as: copy.then(|| "Copy".into()),
                })
                .unwrap();
                let FileContent::Project(session, notes) = loaded.content else {
                    panic!("expected project")
                };
                assert_eq!(notes, warnings);
                assert_eq!(session.path, (!copy).then(|| path.canonicalize().unwrap()));
                assert_eq!(session.board_versions().len(), 1);
            }
            if missing {
                for result in [
                    emulsion_io::project::read(&path),
                    emulsion_io::project::read_from(std::io::Cursor::new(&bytes)),
                ] {
                    let error = result.err().expect("bare APIs cannot lose a report");
                    let emulsion_io::IoError::ProjectRecoveryRequired { report } = error else {
                        panic!("expected typed recovery requirement")
                    };
                    assert_eq!(report, opened.report);
                }
            } else {
                assert!(emulsion_io::project::read(&path).is_ok());
                assert!(emulsion_io::project::read_from(std::io::Cursor::new(&bytes)).is_ok());
            }
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn secondary_cloud_extract_and_catalog_routes_still_reject_recovery_needed_content() {
        let dir = TestDirectory::new();
        let path = dir.0.join("strict-source.emu");
        emulsion_io::project::write(&native_recovery_fixture(true), &path).unwrap();
        let report = emulsion_io::project::read_with_report(&path)
            .unwrap()
            .report;
        assert!(!report.is_empty());
        assert!(matches!(
            emulsion_io::storyboard_extract::read_extract(&path),
            Err(emulsion_io::IoError::ProjectRecoveryRequired { .. })
        ));
        let destination = dir.0.join("cloud.zip");
        let error = emulsion_io::cloud::pack(&path, &destination).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<emulsion_io::IoError>(),
            Some(emulsion_io::IoError::ProjectRecoveryRequired { .. })
        ));
        assert!(!destination.exists());
        let catalog = dir.0.join("catalog");
        let result = crate::creative_catalog_tools::execute(
            &catalog,
            "install_design_template",
            &json!({"path":path}),
        );
        assert!(result.is_error);
        assert!(
            !catalog.exists(),
            "a strict failure must not install the source"
        );
    }

    #[test]
    fn corrupt_native_inputs_never_produce_installable_pages_or_copies() {
        let dir = TestDirectory::new();
        let path = dir.0.join("corrupt.emu");
        std::fs::write(&path, b"not an archive").unwrap();
        assert!(crate::project_tools::load_pages(&json!({"source":path})).is_err());
        for copy in [false, true] {
            assert!(
                load_file(FileRequest {
                    path: path.clone(),
                    kind: None,
                    copy_as: copy.then(|| "Copy".into()),
                })
                .is_err()
            );
        }
    }

    #[test]
    fn psd_file_loader_carries_same_import_report_and_preserves_document() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../emulsion-io/tests/fixtures/psd/blending/knockout-deep-nested-pt.psd");
        let (expected, report) = emulsion_io::open_full_with_report(&path).unwrap();
        let expected_path = path.canonicalize().unwrap();
        let loaded = load_file(FileRequest {
            path,
            kind: None,
            copy_as: None,
        })
        .unwrap();
        assert_eq!(loaded.psd_report, report);
        assert_eq!(loaded.path, expected_path);
        let FileContent::Document(opened) = loaded.content else {
            panic!("PSD must open as a document");
        };
        crate::document_contents::assert_document_contents(
            &opened.doc,
            &expected.doc,
            "workspace file loader",
        );
        assert!(opened.history_error.is_none());
        assert!(opened.graph.is_none());
    }

    #[test]
    fn report_json_keeps_uncompared_and_ambiguous_evidence_distinct() {
        use emulsion_io::psd::{ImportProfileDecision, ReadReport};
        for (decision, expected, ambiguous) in [
            (
                ImportProfileDecision::NotCompared,
                "not_compared",
                Value::Null,
            ),
            (
                ImportProfileDecision::UniquePhotoshopSrgbV1,
                "photoshop_srgb_v1_selected",
                json!(false),
            ),
            (
                ImportProfileDecision::LegacyMatch { ambiguous: false },
                "legacy_srgb_match",
                json!(false),
            ),
            (
                ImportProfileDecision::LegacyMatch { ambiguous: true },
                "legacy_srgb_match",
                json!(true),
            ),
            (
                ImportProfileDecision::SameCurrentAppearance,
                "same_current_appearance",
                Value::Null,
            ),
            (
                ImportProfileDecision::SavedAppearance,
                "saved_appearance",
                Value::Null,
            ),
        ] {
            let value = psd_report_value(ReadReport {
                profile_decision: decision,
                background_preserved: true,
            });
            assert_eq!(value["decision"], expected);
            assert_eq!(value["ambiguous"], ambiguous);
            assert_eq!(value["background_preserved"], true);
            assert_eq!(value["current_appearance_only"], true);
        }
    }

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
        for kind in ["photo", "paint", "design", "diagram", "storyboard"] {
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
    fn storyboard_templates_need_a_known_template_and_a_name() {
        for args in [
            json!({"template":1}),
            json!({"template":1,"name":" "}),
            json!({"template":1,"name":"x","path":"/tmp/a.emu"}),
            json!({"template":u64::MAX,"name":"Pilot"}),
        ] {
            assert!(
                parse("create_storyboard_from_template", &args).is_err(),
                "{args}"
            );
        }
        // open_workspace_file never opens an unsaved copy.
        let Action::Open(request) = parse(
            "open_workspace_file",
            &json!({"path":std::env::temp_dir().join("a.emu")}),
        )
        .unwrap() else {
            panic!()
        };
        assert!(request.copy_as.is_none());
        assert!(
            parse(
                "open_workspace_file",
                &json!({"path":std::env::temp_dir().join("a.emu"),"copy_as":"x"})
            )
            .is_err()
        );
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
        let Action::Create(spec) = parse(
            "create_design_project",
            &json!({"kind":"storyboard","name":"Pilot","width":1920,"height":1080,"pages":4}),
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            spec.create_project()
                .unwrap()
                .storyboard()
                .unwrap()
                .panels
                .len(),
            4
        );
    }
}
