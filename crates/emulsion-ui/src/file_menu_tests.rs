//! Workspace-specific File actions, driven through the actual native menus.
use super::*;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size};

fn import_item(cx: &mut VisualTestContext, index: usize) {
    cx.update(|window, cx| {
        window.click("file-menu-button", cx);
        window.within("popup-menu").hover(2usize, cx);
        window.press("right", cx);
        window.within("submenu").click(index, cx);
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn file_open_follows_photo_and_paint_and_preserves_existing_document(cx: &mut TestAppContext) {
    let original = doc(&["Existing artwork"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1440.), px(1000.)));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.refresh();
        ws.read(cx).editor.clone().unwrap()
    });
    for (paint, label, prompt) in [
        (
            false,
            "Open images…",
            "Open images — camera RAW, JPEG, PNG, TIFF, PSD, XCF or OpenRaster",
        ),
        (
            true,
            "Open artwork…",
            "Open artwork — OpenRaster, PSD, XCF or images",
        ),
    ] {
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                if e.draw_mode != paint {
                    e.toggle_draw_mode(cx);
                }
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.click("file-menu-button", cx);
            assert_eq!(
                window.within("popup-menu").find(1usize).label(),
                Some(label)
            );
            window.within("popup-menu").click(1usize, cx);
        });
        cx.run_until_parked();
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|options| {
            assert!(options.files && options.multiple && !options.directories);
            assert_eq!(options.prompt.as_deref(), Some(prompt));
            None
        });
        cx.run_until_parked();
    }
    import_item(cx, 1);
    cx.simulate_path_prompt_response(|options| {
        assert_eq!(options.prompt.as_deref(), Some("Import brushes"));
        None
    });
    cx.run_until_parked();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Artwork.png");
    image::RgbaImage::from_pixel(32, 24, image::Rgba([40, 80, 120, 255]))
        .save(&path)
        .unwrap();
    cx.update(|window, cx| window.press("ctrl-o", cx));
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![path.clone()]));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let workspace = ws.read(cx);
        assert_eq!(workspace.tabs.len(), 2);
        assert_eq!(editor.read(cx).editor.doc, original);
        let opened = workspace.editor.as_ref().unwrap().read(cx);
        assert!(opened.draw_mode);
        assert_eq!(opened.source.as_ref(), Some(&path));
    });
}

#[gpui_kit::test]
fn diagram_file_import_adds_pages_to_current_project_and_can_be_undone(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(800, 600));
    cx.simulate_resize(size(px(1440.), px(1000.)));
    let editor = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, Document::new(800, 600)).unwrap(),
                "Diagram".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    for (index, provider) in [(1, "Visio"), (3, "Lucid"), (4, "Mermaid")] {
        import_item(cx, index);
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|options| {
            assert!(options.prompt.as_deref().unwrap().contains(provider));
            assert!(options.files && !options.multiple && !options.directories);
            None
        });
        cx.run_until_parked();
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Imported.drawio");
    std::fs::write(&path, r#"<mxfile><diagram name="Imported"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="Imported node" vertex="1" parent="1"><mxGeometry x="20" y="20" width="100" height="50" as="geometry"/></mxCell></root></mxGraphModel></diagram></mxfile>"#).unwrap();
    import_item(cx, 2);
    cx.simulate_path_prompt_response(|options| {
        assert!(options.prompt.as_deref().unwrap().contains("draw.io"));
        Some(vec![path.clone()])
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(ws.read(cx).tabs.len(), 2);
        assert_eq!(ws.read(cx).editor.as_ref(), Some(&editor));
        assert_eq!(editor.read(cx).editor.page_list().len(), 2);
        assert!(editor.read(cx).editor.doc.diagram.is_some());
        window.click("project-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.page_list().len(), 1));
    let path = directory.path().join("Sequence.mmd");
    let source = "sequenceDiagram\nparticipant U as User\nU->>App: Hello";
    std::fs::write(&path, source).unwrap();
    import_item(cx, 4);
    cx.simulate_path_prompt_response(|_| Some(vec![path.clone()]));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let imported = editor.read(cx);
        assert_eq!(imported.editor.page_list().len(), 2);
        assert_mermaid_artwork(&imported.editor.doc, source);
        window.click("project-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.page_list().len(), 1));
}

#[gpui_kit::test]
fn design_and_library_file_menus_offer_relevant_imports(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(800, 600));
    cx.simulate_resize(size(px(1440.), px(1000.)));
    cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(800, 600)).unwrap(),
                "Design".into(),
                window,
                cx,
            )
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("file-menu-button", cx);
        assert_eq!(
            window.within("popup-menu").find(1usize).label(),
            Some("Open design or presentation…")
        );
        window.within("popup-menu").hover(2usize, cx);
        window.press("right", cx);
        assert_eq!(
            window.within("submenu").find(0usize).label(),
            Some("Place images or SVG…")
        );
        assert_eq!(
            window.within("submenu").find(1usize).label(),
            Some("Import video or audio…")
        );
        assert_eq!(
            window.within("submenu").find(2usize).label(),
            Some("Import template…")
        );
        window.press("escape", cx);
        window.press("escape", cx);
        window.dispatch_action(Box::new(actions::ShowBatch), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("workspace-file-menu-button", cx);
        assert_eq!(
            window.within("popup-menu").find(1usize).label(),
            Some("Import photo folder…")
        );
        window.within("popup-menu").click(1usize, cx);
    });
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.directories && !options.files);
        None
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("workspace-file-menu-button", cx);
        window.within("popup-menu").click(2usize, cx);
    });
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && options.multiple && !options.directories);
        None
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(ws.read(cx).tabs.len(), 2));
}
