use super::*;
use emulsion_core::{
    NodeKind,
    creation::{CanvasKind, CanvasSpec},
    project::{ProjectEditor, ProjectKind},
};
use gpui_kit::test::TestWindowExt;

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
        assert_eq!(e.editor.doc.nodes.len(), 5);
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
        assert_eq!(view.read(cx).editor.doc.nodes.len(), 6);
        window.click(("design-section", 1usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-element", 4usize), cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.nodes.len(), 7);
            assert!(matches!(
                e.editor.doc.node(e.selected.unwrap()).unwrap().kind,
                NodeKind::Path { .. }
            ));
            e.undo(cx);
            assert_eq!(e.editor.doc.nodes.len(), 6);
            e.undo(cx);
            assert_eq!(e.editor.doc.nodes.len(), 5);
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
    cx.update(|window, cx| window.click(("design-section", 9usize), cx));
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
    cx.update(|window, cx| window.click(("design-section", 9usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-resize-copy", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.page_list().len(), 2);
        assert!(window.try_find("ok").is_none());
        window.click("design-resize-copy", cx);
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
