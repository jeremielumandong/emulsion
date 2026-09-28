use super::*;
use emulsion_core::creation::{Background, CanvasKind};
use gpui_kit::test::TestWindowExt;

fn type_field(cx: &mut VisualTestContext, label: &'static str, value: &str) {
    cx.update(|window, cx| {
        window.click(
            gpui_kit::SharedString::from(format!("new-canvas-field-{label}")),
            cx,
        )
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    cx.simulate_input(value);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn new_canvas_dialog_validates_before_creating_and_preserves_existing_tabs(
    cx: &mut TestAppContext,
) {
    let original = doc(&["Photo"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.update(|window, cx| ws.update(cx, |ws, cx| ws.new_document(window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("new-canvas-form").visible());
        assert_eq!(ws.read(cx).tabs.len(), 1);
        window.click(("new-canvas-kind", CanvasKind::Paint as usize), cx);
    });
    cx.run_until_parked();
    type_field(cx, "Name", "Local painting");
    type_field(cx, "Width", "NaN");
    cx.update(|window, cx| window.click("new-canvas-create", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("new-canvas-form").visible());
        assert_eq!(ws.read(cx).tabs.len(), 1);
    });
    type_field(cx, "Width", "64");
    type_field(cx, "Height", "48");
    cx.update(|window, cx| {
        window.click(
            ("new-canvas-background", Background::Transparent as usize),
            cx,
        )
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("new-canvas-save-preset", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("new-canvas-create", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("new-canvas-form").is_none());
        let workspace = ws.read(cx);
        assert_eq!(workspace.tabs.len(), 2);
        assert_eq!(workspace.tabs[0].read(cx).editor.doc, original);
        let editor = workspace.editor.as_ref().unwrap().read(cx);
        assert_eq!(editor.name, "Local painting");
        assert_eq!(
            (editor.editor.doc.width, editor.editor.doc.height),
            (64, 48)
        );
        assert!(editor.draw_mode);
        let settings = &cx.global::<AppSettings>().0;
        assert_eq!(settings.canvas_presets[0].name, "Local painting");
        assert_eq!(
            settings.recent_canvases[0].background,
            Background::Transparent
        );
    });
}

#[gpui_kit::test]
fn cancelling_new_canvas_does_not_change_document_or_saved_presets(cx: &mut TestAppContext) {
    let original = doc(&["Existing"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.update(|window, cx| ws.update(cx, |ws, cx| ws.new_document(window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("new-canvas-cancel", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("new-canvas-form").is_none());
        assert_eq!(ws.read(cx).tabs.len(), 1);
        assert_eq!(
            ws.read(cx).editor.as_ref().unwrap().read(cx).editor.doc,
            original
        );
        assert!(cx.global::<AppSettings>().0.recent_canvases.is_empty());
    });
}

#[gpui_kit::test]
fn new_design_dialog_creates_all_pages_and_bleed(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(32, 24));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1000.)));
    cx.update(|window, cx| ws.update(cx, |ws, cx| ws.new_document(window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("new-canvas-kind", CanvasKind::Design as usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("new-canvas-blank", cx));
    cx.run_until_parked();
    type_field(cx, "Name", "Local campaign");
    type_field(cx, "Width", "80");
    type_field(cx, "Height", "60");
    type_field(cx, "Pages", "3");
    type_field(cx, "Bleed · mm", "3");
    cx.update(|window, cx| window.click("new-canvas-create", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("new-canvas-form").is_none());
        let e = ws.read(cx).editor.as_ref().unwrap().read(cx);
        assert_eq!(
            e.editor.kind(),
            Some(emulsion_core::project::ProjectKind::Design)
        );
        assert_eq!(e.editor.page_list().len(), 3);
        assert!(e.editor.page_list().iter().all(|page| page.bleed_mm == 3.));
        assert!(e.has_unsaved_changes());
        assert_eq!(ws.read(cx).tabs.len(), 2);
    });
}

#[gpui_kit::test]
fn new_document_keeps_home_project_on_first_save_and_later_saves(cx: &mut TestAppContext) {
    use emulsion_io::creative_library as library;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("campaign.ora");
    let original = doc(&["Existing"], None);
    let (ws, cx) = open(cx, original.clone());
    let (catalog, folder) = library::update(&library::root(), |catalog| {
        catalog.add_project_folder("Shared UI test project".into())
    })
    .unwrap();
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.home_state.projects.catalog = catalog;
            ws.home_state.projects.folder = Some(folder);
            ws.new_document(window, cx);
        })
    });
    cx.run_until_parked();
    type_field(cx, "Width", "64");
    type_field(cx, "Height", "48");
    cx.update(|window, cx| {
        assert!(window.find("new-canvas-project").visible());
        window.click("new-canvas-create", cx);
    });
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        assert_eq!(editor.read(cx).home_folder_on_save, Some(Some(folder)));
        assert_eq!(ws.read(cx).tabs[0].read(cx).editor.doc, original);
        ws.update(cx, |ws, cx| ws.write(editor.clone(), path.clone(), cx));
    });
    cx.run_until_parked();
    assert!(path.is_file());
    let catalog = library::load(&library::root()).unwrap();
    assert_eq!(
        catalog
            .projects
            .iter()
            .find(|p| p.path == path)
            .unwrap()
            .folder,
        Some(folder)
    );
    cx.update(|_, cx| {
        assert_eq!(editor.read(cx).home_folder_on_save, None);
        ws.update(cx, |ws, cx| ws.write(editor.clone(), path.clone(), cx));
    });
    cx.run_until_parked();
    let catalog = library::load(&library::root()).unwrap();
    assert_eq!(
        catalog.projects.iter().filter(|p| p.path == path).count(),
        1
    );
    assert_eq!(
        catalog
            .projects
            .iter()
            .find(|p| p.path == path)
            .unwrap()
            .folder,
        Some(folder)
    );
}

#[gpui_kit::test]
fn new_design_template_starts_at_native_size_without_a_blank_page(cx: &mut TestAppContext) {
    let original = doc(&["Existing photo"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1000.)));
    let (catalog, folder) =
        emulsion_io::creative_library::update(&emulsion_io::creative_library::root(), |c| {
            c.add_project_folder("Template campaign".into())
        })
        .unwrap();
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.home_state.projects.catalog = catalog;
            ws.home_state.projects.folder = Some(folder);
            ws.open_new_canvas_kind(CanvasKind::Design, window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("new-template-grid").visible());
        assert!(window.try_find("new-canvas-field-Width").is_none());
        assert!(window.try_find("new-template-design-12").is_none());
        window.click("new-canvas-create", cx);
        assert_eq!(ws.read(cx).tabs.len(), 1);
        window.click("new-template-design-0", cx);
    });
    cx.run_until_parked();
    type_field(cx, "Name", "Campaign cover");
    cx.update(|window, cx| window.click("new-canvas-create", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("new-canvas-form").is_none());
        let w = ws.read(cx);
        assert_eq!(w.tabs.len(), 2);
        assert_eq!(w.tabs[0].read(cx).editor.doc, original);
        let e = w.editor.as_ref().unwrap().read(cx);
        assert_eq!(
            e.editor.kind(),
            Some(emulsion_core::project::ProjectKind::Design)
        );
        assert_eq!(e.editor.page_list().len(), 1);
        assert_eq!(
            (e.editor.doc.width, e.editor.doc.height),
            emulsion_core::design::Template::catalog()
                .next()
                .unwrap()
                .native_size()
        );
        assert!(e.editor.doc.nodes.len() > 1);
        assert_eq!(e.name, "Campaign cover");
        assert_eq!(e.home_folder_on_save, Some(Some(folder)));
        assert!(e.editor.path.is_none());
        assert!(e.has_unsaved_changes());
    });
}

#[gpui_kit::test]
fn new_diagram_template_is_editable_and_cancel_leaves_tabs_unchanged(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(32, 32));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1000.)));
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_new_canvas_kind(CanvasKind::Diagram, window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("new-canvas-field-Width").is_none());
        window.click("new-template-diagram-0", cx);
        window.click("new-canvas-create", cx);
        window.click("new-canvas-cancel", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(ws.read(cx).tabs.len(), 1);
        ws.update(cx, |ws, cx| {
            ws.open_new_canvas_kind(CanvasKind::Diagram, window, cx)
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("new-template-diagram-0", cx);
        window.click("new-canvas-create", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = ws.read(cx).editor.as_ref().unwrap().read(cx);
        assert_eq!(
            e.editor.kind(),
            Some(emulsion_core::project::ProjectKind::Diagram)
        );
        assert_eq!(e.editor.page_list().len(), 1);
        assert!(!e.editor.doc.diagram.as_ref().unwrap().edges.is_empty());
        assert_eq!((e.editor.doc.width, e.editor.doc.height), (960, 640));
        assert_eq!(ws.read(cx).tabs.len(), 2);
    });
}

#[gpui_kit::test]
fn new_saved_template_copies_every_page_without_overwriting_source(cx: &mut TestAppContext) {
    use emulsion_io::creative_library as library;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Two page template.emu");
    let template = emulsion_core::creation::CanvasSpec {
        kind: CanvasKind::Design,
        width: 80.,
        height: 60.,
        pages: 2,
        ..Default::default()
    }
    .create_project()
    .unwrap();
    emulsion_io::project::write(&template.snapshot().unwrap(), &path).unwrap();
    let source_bytes = std::fs::read(&path).unwrap();
    let (catalog, id) = library::update(&library::root(), |c| {
        c.add_asset(path.clone(), library::AssetKind::Template)
    })
    .unwrap();
    let (ws, cx) = open(cx, Document::new(32, 32));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1000.)));
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.home_state.projects.catalog = catalog;
            ws.open_new_canvas_kind(CanvasKind::Design, window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click(
            (
                "new-template-category",
                emulsion_core::design::Template::CATEGORIES.len() + 1,
            ),
            cx,
        )
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click(
            gpui_kit::SharedString::from(format!("new-template-local-{id}")),
            cx,
        )
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("new-canvas-create", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = ws.read(cx).editor.as_ref().unwrap().read(cx);
        assert_eq!(e.editor.page_list().len(), 2);
        assert_eq!((e.editor.doc.width, e.editor.doc.height), (80, 60));
        assert!(e.editor.path.is_none());
        assert!(e.has_unsaved_changes());
    });
    assert_eq!(std::fs::read(path).unwrap(), source_bytes);
}
