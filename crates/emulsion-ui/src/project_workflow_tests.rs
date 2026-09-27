use super::*;
use emulsion_core::{
    NodeKind,
    creation::{CanvasKind, CanvasSpec},
    project::{ProjectEditor, ProjectKind},
};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn design_copy_style_and_bulk_create_are_editable_and_undoable(cx: &mut TestAppContext) {
    let mut doc = Document::new(600, 400);
    let mut add_text = |text: &str, size, y| {
        Command::AddNode {
            node: Box::new(Node::text(
                0,
                text,
                emulsion_core::text::TextSpec {
                    text: text.into(),
                    size,
                    y,
                    ..Default::default()
                },
                600,
                400,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap()
    };
    let source = add_text("Heading", 48., 30.);
    let target = add_text("Hello {{name}}", 20., 130.);
    let (ws, cx) = open(cx, doc.clone());
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc.clone()).unwrap(),
                "Bulk".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![source], Some(source));
            cx.notify();
        });
        window.click("design-position", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-copy-style", cx));
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![target], Some(target));
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-paste-style", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |e, cx| {
            let NodeKind::Text { spec, .. } = &e.editor.doc.node(target).unwrap().kind else {
                panic!()
            };
            assert_eq!(spec.size, 48.);
            assert_eq!(spec.text, "Hello {{name}}");
            assert_eq!(spec.y, 130.);
            e.undo(cx);
            assert_eq!(e.editor.doc, doc);
            e.redo(cx);
        });
        window.click(("design-section", 0usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-bulk-create", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert_eq!(e.editor.page_list().len(), 2);
            let NodeKind::Text { spec, .. } = &e.editor.doc.node(target).unwrap().kind else {
                panic!()
            };
            assert_eq!(spec.text, "Hello Example");
            assert_eq!(spec.size, 48.);
            e.undo(cx);
            assert_eq!(e.editor.page_list().len(), 1);
            let NodeKind::Text { spec, .. } = &e.editor.doc.node(target).unwrap().kind else {
                panic!()
            };
            assert_eq!(spec.text, "Hello {{name}}");
            e.redo(cx);
            assert_eq!(e.editor.page_list().len(), 2);
        })
    });
}

#[gpui_kit::test]
fn design_template_categories_search_and_create_editable_invitations(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Categories".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-library-search", cx));
    cx.simulate_input("Invitation");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find(("design-template", 0usize)).is_none());
        assert!(window.find(("design-template", 100usize)).visible());
        window.click("design-explore-templates", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            window
                .try_find(("design-template-category", 0usize))
                .is_none()
        );
        window.click(("design-template-category", 10usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find(("design-template", 0usize)).is_none());
        for i in 100usize..110 {
            assert!(window.try_find(("design-template", i)).is_some());
        }
        assert!(window.find(("design-template-preview", 100usize)).visible());
        window.click(("design-template", 100usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!((e.editor.doc.width, e.editor.doc.height), (1500, 2100));
        assert_eq!(e.editor.page_list().len(), 2);
        assert!(
            e.editor.doc.nodes.iter().any(
                |n| matches!(&n.kind,NodeKind::Text {spec,..} if spec.text=="Elena\n&\nJonas")
            )
        );
        window.click("design-template-all", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find(("design-template", 0usize)).visible());
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.page_list().len(), 1));
}

#[gpui_kit::test]
fn design_canvas_selects_objects_and_leaves_text_editing(cx: &mut TestAppContext) {
    use crate::editor::Tool;
    use gpui_kit::{MouseButton, MouseDownEvent};
    let mut doc = CanvasSpec {
        width: 600.,
        height: 400.,
        ..Default::default()
    }
    .create()
    .unwrap();
    let shape = Command::AddNode {
        node: Box::new(Node::path(
            0,
            "Shape",
            Arc::new(emulsion_raster::vector_geometry::rectangle(
                30., 280., 100., 60.,
            )),
            emulsion_raster::vector::PathStyle {
                fill: Some([80, 120, 220, 255]),
                stroke: None,
                ..Default::default()
            },
            600,
            400,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap()
    .unwrap();
    let baseline_count = doc.nodes.len();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(900.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                "Selection".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-section", 2usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-text", 0usize), cx));
    cx.run_until_parked();
    let (text, text_point, shape_point, blank) = cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!(e.tool, Tool::Move);
        assert!(window.find("design-select").visible());
        let text = e.selected.unwrap();
        let NodeKind::Text { spec, .. } = &e.editor.doc.node(text).unwrap().kind else {
            panic!()
        };
        let caret = emulsion_core::text::layout(spec).caret(1);
        let p = spec.transform().transform_point2(glam::dvec2(
            caret.x as f64,
            (caret.y + caret.height * 0.5) as f64,
        ));
        (
            text,
            e.doc_to_window((p.x, p.y)).unwrap(),
            e.doc_to_window((80., 310.)).unwrap(),
            e.doc_to_window((560., 350.)).unwrap(),
        )
    });
    cx.simulate_click(shape_point, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).selected, Some(shape)));
    cx.simulate_click(
        text_point,
        gpui_kit::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).selected_layer_ids().len(), 2));
    cx.simulate_event(MouseDownEvent {
        position: text_point,
        button: MouseButton::Left,
        modifiers: Default::default(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_mouse_up(text_point, MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).tool, Tool::Type));
    cx.simulate_input("Edited");
    cx.simulate_click(shape_point, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.tool, Tool::Move);
        assert_eq!(e.selected, Some(shape));
        assert!(e.type_tool.field.is_none());
        assert_eq!(e.editor.doc.nodes.len(), baseline_count + 1);
        let NodeKind::Text { spec, .. } = &e.editor.doc.node(text).unwrap().kind else {
            panic!()
        };
        assert!(spec.text.contains("Edited"));
        assert!(!e.editor.in_transaction());
    });
    cx.simulate_click(blank, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).selected, None);
        view.update(cx, |e, cx| e.set_tool(Tool::Type, cx));
    });
    cx.simulate_click(blank, Default::default());
    cx.simulate_input("New text");
    cx.simulate_keystrokes("ctrl-enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.tool, Tool::Move);
        assert!(e.type_tool.field.is_none());
        assert_eq!(e.editor.doc.nodes.len(), baseline_count + 2);
    });
    cx.update(|window, cx| {
        view.update(cx, |e, cx| e.set_tool(Tool::Type, cx));
        window.click("design-select", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).tool, Tool::Move));
}

#[gpui_kit::test]
fn design_page_remove_is_visible_undoable_and_keeps_the_last_page(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Pages".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|_, cx| view.update(cx, |_, cx| cx.notify()));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            window
                .find(("compact-document", view.entity_id()))
                .visible()
        );
        assert!(
            window
                .find(("compact-document-close", view.entity_id()))
                .visible()
        );
        assert_eq!(view.read(cx).editor.page_list().len(), 1);
    });
    cx.update(|window, cx| window.click("project-page-add", cx));
    cx.run_until_parked();
    let second = cx.update(|window, cx| {
        let second = view.read(cx).editor.active_page();
        assert!(window.find(("project-page-remove", second)).visible());
        window.click(("project-page-remove", second), cx);
        second
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.page_list().len(), 1);
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.page_list().len(), 2);
        assert_eq!(view.read(cx).editor.active_page(), second);
        window.click(("project-page-remove", 1u64), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.active_page(), second);
        window.click(("project-page-remove", second), cx);
        assert_eq!(view.read(cx).editor.page_list().len(), 1);
    });
}

#[gpui_kit::test]
fn design_layout_controls_create_a_persistent_frame_and_undo_as_one_edit(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Layout".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.insert_design_element(emulsion_core::design::Element::Rectangle, cx)
        })
    });
    cx.run_until_parked();
    let original = cx.update(|window, cx| {
        window.click("design-position", cx);
        view.read(cx).editor.doc.clone()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-layout-flow", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-layout-fill", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-layout-hug-height", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.design.frames.len(), 1);
            let frame = e.editor.doc.design.frames.values().next().unwrap();
            assert!(frame.children.values().all(|child| child.fill_width));
            assert!(frame.hug_height);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.redo(cx);
            assert_eq!(e.editor.doc.design.frames.len(), 1);
        })
    });
}

#[gpui_kit::test]
fn design_drawer_templates_text_and_elements_create_editable_objects(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Design".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("design-rail").visible());
        assert!(window.find("design-drawer").visible());
        window.click(("design-template", 0usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.page_list().len(), 2);
        assert_eq!(e.editor.doc.nodes.len(), 10);
        assert!(
            e.editor
                .doc
                .nodes
                .iter()
                .all(|n| !matches!(n.kind, NodeKind::Raster { .. }))
        );
        window.click(("design-section", 2usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-text", 0usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc.nodes.len(), 11);
        window.click(("design-section", 1usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-element", 4usize), cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.nodes.len(), 12);
            assert!(matches!(
                e.editor.doc.node(e.selected.unwrap()).unwrap().kind,
                NodeKind::Path { .. }
            ));
            e.undo(cx);
            assert_eq!(e.editor.doc.nodes.len(), 11);
            e.undo(cx);
            assert_eq!(e.editor.doc.nodes.len(), 10);
            e.undo(cx);
            assert_eq!(e.editor.page_list().len(), 1);
        })
    });
}

#[gpui_kit::test]
fn project_pages_keep_text_editable_and_undo_content_in_chronological_order(
    cx: &mut TestAppContext,
) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    let view = cx.update(|window, cx| {
        let project =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap();
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Campaign".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("project-page-strip").visible());
        view.update(cx, |e, cx| {
            let spec = emulsion_core::text::TextSpec {
                text: "Editable heading".into(),
                x: 80.5,
                y: 100.25,
                size: 30.,
                ..Default::default()
            };
            let node = Node::text(0, "Heading", spec.clone(), 600, 400);
            e.execute(
                Command::AddNode {
                    node: Box::new(node),
                    slot: Slot::TOP,
                },
                cx,
            );
            let id = e.editor.doc.nodes[0].id;
            e.selected = Some(id);
            e.copy_pixels(cx);
            let second = e
                .editor
                .add_page(Document::new(800, 500), "Back".into(), 3.)
                .unwrap();
            e.after_change(cx);
            assert!(
                e.selected.is_none(),
                "page-local layer IDs cannot retain selection"
            );
            e.paste_pixels(cx);
            let NodeKind::Text {
                spec: pasted,
                cache,
            } = &e.editor.doc.nodes[0].kind
            else {
                panic!("text was flattened")
            };
            assert_eq!(pasted.text, spec.text);
            assert_eq!(pasted.size, spec.size);
            assert_eq!(cache.size(), (800, 500));
            assert_ne!(
                pasted.x, spec.x,
                "another page centers pasted text even in the same tab"
            );
            e.select_page(1, cx);
            e.execute(Command::SetOpacity { id, opacity: 0.5 }, cx);
            e.undo(cx);
            assert_eq!(e.editor.active_page(), 1);
            assert_eq!(e.editor.doc.nodes[0].opacity, 1.);
            e.undo(cx);
            assert_eq!(e.editor.active_page(), second);
            assert!(e.editor.doc.nodes.is_empty());
            e.undo(cx);
            assert_eq!(e.editor.page_list().len(), 1);
            e.redo(cx);
            e.redo(cx);
            e.redo(cx);
            assert_eq!(e.editor.page_list().len(), 2);
            assert_eq!(e.editor.active_page(), 1);
            assert_eq!(e.editor.doc.nodes[0].opacity, 0.5);
        });
    });
}

#[gpui_kit::test]
fn project_save_reopen_and_recovery_preserve_inactive_pages(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("campaign.emu");
    let recovery = folder.path().join("recovery.emu");
    let (ws, cx) = open(cx, Document::new(32, 24));
    let view = cx.update(|window, cx| {
        let spec = CanvasSpec {
            kind: CanvasKind::Design,
            pages: 2,
            bleed_mm: 3.,
            width: 32.,
            height: 24.,
            ..Default::default()
        };
        let mut session = spec.create_project().unwrap();
        session.rename_page(2, "Back cover".into(), 5.).unwrap();
        emulsion_io::project::write(&session.snapshot().unwrap(), &path).unwrap();
        let stamp = session.stamp();
        session.mark_project_saved(path.clone(), &stamp);
        ws.update(cx, |ws, cx| {
            ws.install_project(session, "Campaign".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.select_page(2, cx);
            let id = e.editor.doc.nodes[0].id;
            e.execute(Command::SetOpacity { id, opacity: 0.25 }, cx);
            e.select_page(1, cx);
            assert!(
                e.has_unsaved_changes(),
                "inactive page edits must prompt on close"
            );
            e.history.recovery = Some(recovery.clone());
            e.history.last_recovery = None;
            e.autosave(cx);
        })
    });
    cx.run_until_parked();
    let recovered = emulsion_io::project::read(&recovery).unwrap();
    assert_eq!(recovered.pages.len(), 2);
    assert_eq!(recovered.pages[1].doc.nodes[0].opacity, 0.25);
    assert_eq!(recovered.pages[1].meta.name, "Back cover");
    assert_eq!(recovered.pages[1].meta.bleed_mm, 5.);
    cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_recovered(recovery.clone(), window, cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = ws.read(cx).editor.as_ref().unwrap().read(cx);
        assert!(e.has_unsaved_changes());
        assert!(e.editor.path.is_none());
        assert_eq!(e.editor.page(2).unwrap().doc.nodes[0].opacity, 0.25);
        assert!(
            recovery.exists(),
            "keep the recovery copy until saved or explicitly discarded"
        );
    });
    // Opening the saved path again selects the existing tab, preserving its edits.
    cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_path(path.clone(), window, cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            ws.read(cx).editor.as_ref().unwrap().entity_id(),
            view.entity_id()
        )
    });
}

#[gpui_kit::test]
fn diagram_drawer_creates_connected_editable_shapes_and_cut_restores_graph(
    cx: &mut TestAppContext,
) {
    use emulsion_core::diagram::{Endpoint, Port};
    let (ws, cx) = open(cx, Document::new(800, 600));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, Document::new(800, 600)).unwrap(),
                "Flowchart".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("diagram-drawer").visible());
        window.click(("diagram-shape", 0usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("diagram-shape", 1usize), cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            let ids = e
                .editor
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .shapes
                .keys()
                .copied()
                .collect::<Vec<_>>();
            assert_eq!(ids.len(), 2);
            e.diagram_connect(
                Endpoint {
                    shape: ids[0],
                    port: Port::East,
                },
                Endpoint {
                    shape: ids[1],
                    port: Port::West,
                },
                cx,
            );
            let model = e.editor.doc.diagram.as_ref().unwrap();
            assert_eq!(model.edges.len(), 1);
            let edge = *model.edges.keys().next().unwrap();
            assert!(
                e.editor
                    .doc
                    .nodes
                    .iter()
                    .all(|n| !matches!(n.kind, NodeKind::Raster { .. }))
            );
            e.set_layer_selection(ids.clone(), Some(ids[0]));
            e.cut_pixels(cx);
            assert!(e.editor.doc.nodes.is_empty());
            e.undo(cx);
            assert!(
                e.editor
                    .doc
                    .diagram
                    .as_ref()
                    .unwrap()
                    .edges
                    .contains_key(&edge)
            );
            e.paste_pixels(cx);
            assert_eq!(e.editor.doc.diagram.as_ref().unwrap().shapes.len(), 4);
            assert_eq!(e.editor.doc.diagram.as_ref().unwrap().edges.len(), 2);
            e.editor.doc.validate().unwrap();
        })
    });
}

#[gpui_kit::test]
fn workspace_destinations_preserve_open_projects_and_start_the_right_editor(
    cx: &mut TestAppContext,
) {
    use crate::workspace::destinations::Destination;
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Campaign".into(),
                window,
                cx,
            );
            let project = ws.editor.clone().unwrap();
            let pages = project.read(cx).editor.page_list().len();
            ws.visit_destination(Destination::Photo, window, cx);
            assert_eq!(ws.destination(cx), Some(Destination::Photo));
            ws.visit_destination(Destination::Design, window, cx);
            assert_eq!(ws.editor.as_ref().unwrap().entity_id(), project.entity_id());
            assert_eq!(project.read(cx).editor.page_list().len(), pages);
            ws.start_destination(Destination::Diagram, window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("new-canvas-form").visible());
        window.click("new-canvas-create", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let ws = ws.read(cx);
        assert_eq!(ws.destination(cx), Some(Destination::Diagram));
        assert_eq!(ws.tabs.len(), 3);
        assert_eq!(
            ws.editor.as_ref().unwrap().read(cx).editor.kind(),
            Some(ProjectKind::Diagram)
        );
    });
}

#[gpui_kit::test]
fn design_motion_preview_and_presentation_leave_saved_objects_unchanged(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(400, 300));
    let view = cx.update(|window, cx| {
        let mut doc = emulsion_core::design::Template::Announcement
            .create(400, 300)
            .unwrap();
        let id = doc
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Text { .. }))
            .unwrap()
            .id;
        doc.design
            .motion
            .insert(id, emulsion_core::design_metadata::Motion::default());
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                "Motion".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    let original = cx.update(|_, cx| view.read(cx).editor.doc.clone());
    cx.update(|window, cx| window.click("design-animate", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-motion-play", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(e.previewing());
            assert_eq!(e.editor.doc, original);
            assert!(e.tool_cancel(cx));
            assert!(!e.previewing());
            e.start_motion(true, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("design-presentation").visible());
        window.click("presentation-exit", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("design-presentation").is_none());
        assert!(window.find("design-rail").visible());
        assert_eq!(view.read(cx).editor.doc, original);
    });
}

#[gpui_kit::test]
fn diagram_text_generation_installs_a_complete_page_and_undo_restores_project(
    cx: &mut TestAppContext,
) {
    let (ws, cx) = open(cx, Document::new(800, 600));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, Document::new(800, 600)).unwrap(),
                "Data".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("diagram-generate", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(0usize, cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert_eq!(e.editor.page_list().len(), 2);
            let model = e.editor.doc.diagram.as_ref().unwrap();
            assert_eq!(model.shapes.len(), 3);
            assert_eq!(model.edges.len(), 2);
            e.undo(cx);
            assert_eq!(e.editor.page_list().len(), 1);
            assert!(e.editor.doc.diagram.is_none());
        })
    });
}

#[gpui_kit::test]
fn resize_form_submits_and_cancel_keeps_the_project(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(400, 300));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(400, 300)).unwrap(),
                "Resize".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-resize", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.page_list().len(), 2);
        assert!(window.try_find("ok").is_none());
        window.click("design-resize", cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.page_list().len(), 2);
        assert!(window.try_find("ok").is_none());
        view.update(cx, |e, cx| e.undo(cx));
        assert_eq!(view.read(cx).editor.page_list().len(), 1);
    });
}

#[gpui_kit::test]
fn creative_pack_export_form_open_install_and_stencil_placement(cx: &mut TestAppContext) {
    use emulsion_core::diagram::{Builder, ShapeKind};
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("workflow.emustencil");
    let mut builder = Builder::new(400, 300).unwrap();
    builder
        .add_shape(
            ShapeKind::Process,
            [40., 50., 160., 70.],
            "Editable stencil",
        )
        .unwrap();
    let doc = builder.finish().unwrap();
    let (ws, cx) = open(cx, Document::new(400, 300));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc.clone()).unwrap(),
                "Workflow pack".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("creative-export-pack", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    let pack = emulsion_io::template_pack::read(&path).unwrap();
    let mut stencil = doc.clone();
    stencil
        .nodes
        .retain(|n| !(n.parent.is_none() && matches!(n.kind, NodeKind::Fill { .. })));
    assert_eq!(pack.project.pages[0].doc, stencil);
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, doc);
        ws.update(cx, |ws, cx| ws.open_path(path.clone(), window, cx));
    });
    cx.run_until_parked();
    let placed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let catalog =
        emulsion_io::creative_library::load(&emulsion_io::creative_library::root()).unwrap();
    let asset = catalog
        .assets
        .iter()
        .find(|a| a.name == "Workflow pack")
        .unwrap();
    cx.update(|window, cx| {
        assert_eq!(placed.read(cx).editor.doc, stencil);
        window.click(
            (
                gpui_kit::ElementId::from("stencil-pack-item"),
                format!("{}-0", asset.id),
            ),
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|_,cx|placed.update(cx,|e,cx|{
        assert_eq!(e.editor.doc.diagram.as_ref().unwrap().shapes.len(),2);
        assert_eq!(e.editor.doc.nodes.iter().filter(|n|matches!(&n.kind,NodeKind::Text{spec,..} if spec.text=="Editable stencil")).count(),2);
        e.undo(cx);
        assert_eq!(e.editor.doc, stencil);
    }));
}

#[gpui_kit::test]
fn design_handoff_layout_and_native_actions(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Design".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    for compact in [false, true] {
        for width in [800., 1000., 1600.] {
            cx.simulate_resize(gpui_kit::size(gpui_kit::px(width), gpui_kit::px(1000.)));
            cx.update(|window, cx| {
                cx.global_mut::<AppSettings>().0.compact_chrome = compact;
                window.refresh();
            });
            cx.run_until_parked();
            cx.update(|window, _| {
                assert_eq!(
                    window.find("design-rail").bounds().size.width,
                    gpui_kit::px(68.)
                );
                assert_eq!(
                    window.find("design-drawer").bounds().size.width,
                    gpui_kit::px(250.)
                );
                assert_eq!(
                    window.find("design-drawer-heading").bounds().size.height,
                    gpui_kit::px(38.)
                );
                let bar = window.find("design-canvas-toolbar").bounds();
                assert_eq!(bar.size.height, gpui_kit::px(38.));
                let pages = window.find("project-page-strip").bounds();
                assert_eq!(pages.size.height, gpui_kit::px(88.));
                assert_eq!(pages.origin.x, bar.origin.x);
                assert_eq!(pages.size.width, bar.size.width);
                for id in [0usize, 1, 2, 3, 7, 6, 8] {
                    let bounds = window.find(("design-section", id)).bounds();
                    assert_eq!(
                        bounds.size,
                        gpui_kit::size(gpui_kit::px(56.), gpui_kit::px(48.))
                    );
                }
                for id in ["design-position", "design-animate", "design-resize"] {
                    let bounds = window.find(id).bounds();
                    assert!(bar.contains(&bounds.origin), "{id} starts outside header");
                    assert!(
                        bar.contains(&bounds.bottom_right()),
                        "{id} ends outside header"
                    );
                }
            });
        }
    }
    cx.update(|window, cx| window.click(("design-format", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-template", 0usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!((e.editor.doc.width, e.editor.doc.height), (1080, 1920));
        assert_eq!(e.editor.page_list().len(), 2);
        assert!(
            e.editor
                .doc
                .nodes
                .iter()
                .all(|n| !matches!(n.kind, NodeKind::Raster { .. }))
        );
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.page_list().len(), 1);
        assert_eq!(view.read(cx).editor.doc.width, 600);
        window.click(("design-section", 2usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-type-pair", 0usize), cx));
    cx.run_until_parked();
    let original = cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), 3);
        assert_eq!(
            e.editor
                .doc
                .nodes
                .iter()
                .filter(|n| matches!(n.kind, NodeKind::Text { .. }))
                .count(),
            2
        );
        assert!(e.editor.doc.node(e.selected.unwrap()).unwrap().is_group());
        let original = e.editor.doc.clone();
        window.click("design-position", cx);
        original
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-align", 0usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!(
            emulsion_core::geometry::node_bounds(&e.editor.doc, e.selected.unwrap())
                .unwrap()
                .x,
            0
        );
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(view.read(cx).editor.doc.nodes.is_empty());
        window.click("design-redo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        window.click("design-drawer-close", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("design-drawer").is_none());
        window.click(("design-section", 1usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-open-tools", cx));
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find("tool-rail").visible()));
}

#[gpui_kit::test]
fn design_frame_fit_controls_preserve_embedded_pixels_and_undo(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1600.), gpui_kit::px(1400.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Frame".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-section", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-open-frames", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-frame", 0usize), cx));
    cx.run_until_parked();
    let pixels = Arc::new(emulsion_raster::Raster::solid(
        400,
        100,
        [0.2, 0.3, 0.4, 1.],
    ));
    let (image, original) = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            let group = e.selected.unwrap();
            let image = emulsion_core::design::place_in_frame(&mut e.editor, group, pixels.clone())
                .unwrap();
            e.after_change(cx);
            (image, e.editor.doc.clone())
        })
    });
    cx.run_until_parked();
    for (control, index) in [
        ("design-frame-fit", 1usize),
        ("design-frame-fit", 2),
        ("design-frame-focus", 5),
    ] {
        cx.update(|window, cx| window.click((control, index), cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let e = view.read(cx);
            let NodeKind::Raster { raster, placement } = &e.editor.doc.node(image).unwrap().kind
            else {
                panic!()
            };
            assert!(Arc::ptr_eq(raster, &pixels));
            assert_eq!(
                e.editor.doc.node(image).unwrap().clip_to,
                original.node(image).unwrap().clip_to
            );
            if control == "design-frame-fit" && index == 1 {
                assert_eq!(placement.scale_x, 0.3);
            }
            if control == "design-frame-fit" && index == 2 {
                assert_ne!(placement.scale_x, placement.scale_y);
            }
            assert_ne!(e.editor.doc, original);
            let folder = tempfile::tempdir().unwrap();
            let path = folder.path().join("fitted-frame.emu");
            emulsion_io::project::write(&e.editor.snapshot().unwrap(), &path).unwrap();
            let mut loaded = emulsion_io::project::read(&path).unwrap();
            let NodeKind::Raster {
                raster: restored,
                placement: restored_placement,
            } = &mut loaded.pages[0].doc.node_mut(image).unwrap().kind
            else {
                panic!()
            };
            assert_eq!(restored_placement, placement);
            assert_eq!(
                (restored.width(), restored.height()),
                (raster.width(), raster.height())
            );
            for y in 0..raster.height() {
                for x in 0..raster.width() {
                    assert_eq!(restored.get(x, y), raster.get(x, y));
                }
            }
            // Document equality intentionally compares raster Arc identity.
            // Having compared every pixel, normalize only that allocation.
            *restored = raster.clone();
            assert_eq!(loaded.pages[0].doc, e.editor.doc);
            window.click("design-undo", cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
    }
}

#[gpui_kit::test]
fn design_selection_toolbar_edits_native_text_and_preserves_undo(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(900.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Type".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-section", 2usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-text", 0usize), cx));
    cx.run_until_parked();
    let original = cx.update(|window, cx| {
        assert!(window.find("design-selection-toolbar").visible());
        assert!(window.try_find("node-panel").is_none());
        let before = view.read(cx).editor.doc.clone();
        window.click("design-text-bold", cx);
        before
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_ne!(view.read(cx).editor.doc, original);
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        window.click("design-text-size", cx);
    });
    cx.run_until_parked();
    cx.update(|window, _| assert_eq!(window.find("design-text-size").focused(), Some(true)));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("42");
    cx.run_until_parked();
    cx.update(|window, _| assert_eq!(window.find("design-text-size-input").value(), Some("42")));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        let NodeKind::Text { spec, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(spec.style_at(0).size, 42.);
        window.click("design-text-align", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.within("popup-menu").click(1usize, cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        let NodeKind::Text { spec, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(spec.align, emulsion_core::text::Align::Center);
        window.click("design-text-properties", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-properties-content").visible());
        window.click("design-inspector-toggle", cx);
    });
    cx.run_until_parked();
    let before_zoom = cx.update(|window, cx| {
        let z = view.read(cx).view.zoom;
        window.click("design-zoom-in", cx);
        z
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(e.view.zoom > before_zoom);
            e.undo(cx);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
        })
    });
}

#[gpui_kit::test]
fn project_chrome_keeps_canvas_actions_inside_narrow_and_wide_windows(cx: &mut TestAppContext) {
    use gpui_kit::{px, size};
    let (ws, cx) = open(cx, Document::new(600, 400));
    for kind in [ProjectKind::Design, ProjectKind::Diagram] {
        let view = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(
                    ProjectEditor::new_project(kind, Document::new(600, 400)).unwrap(),
                    "Fidelity".into(),
                    window,
                    cx,
                )
            });
            ws.read(cx).editor.clone().unwrap()
        });
        for dark in [false, true] {
            for width in [480., 800., 1440.] {
                cx.simulate_resize(size(px(width), px(900.)));
                cx.update(|_, cx| crate::theme::set_dark(dark, cx));
                cx.run_until_parked();
                cx.update(|window, cx| {
                    let is_design = kind == ProjectKind::Design;
                    let column = window.find("editor-canvas-column").bounds();
                    let bar = window
                        .find(if is_design {
                            "design-canvas-toolbar"
                        } else {
                            "diagram-canvas-toolbar"
                        })
                        .bounds();
                    assert_eq!(bar.size.height, px(38.));
                    assert_eq!(
                        window.find("editor-status-strip").bounds().size.height,
                        px(24.)
                    );
                    assert_eq!(
                        window.find("project-page-strip").bounds().size.height,
                        px(if is_design { 88. } else { 32. })
                    );
                    assert!(
                        column.size.width >= px(280.),
                        "{kind:?} at {width}: {column:?}"
                    );
                    assert!(column.right() <= px(width));
                    assert!(window.try_find("ask-ai-hint").is_none());
                    assert!(!view.read(cx).rulers);
                    for id in if is_design {
                        ["design-position", "design-animate", "design-resize"]
                    } else {
                        [
                            "diagram-canvas-connect",
                            "diagram-canvas-layout",
                            "diagram-canvas-fit",
                        ]
                    } {
                        let b = window.find(id).bounds();
                        assert!(
                            b.left() >= bar.left() && b.right() <= bar.right(),
                            "{id} at {width}: {b:?} outside {bar:?}"
                        );
                    }
                });
            }
        }
        if kind == ProjectKind::Diagram {
            cx.simulate_resize(size(px(1440.), px(1000.)));
            cx.run_until_parked();
            cx.update(|window, cx| window.click(("diagram-shape", 1usize), cx));
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert_eq!(
                    view.read(cx)
                        .editor
                        .doc
                        .diagram
                        .as_ref()
                        .unwrap()
                        .shapes
                        .len(),
                    1
                );
                window.click("project-undo", cx);
            });
            cx.run_until_parked();
            cx.update(|_, cx| assert!(view.read(cx).editor.doc.nodes.is_empty()));
        }
    }
}
