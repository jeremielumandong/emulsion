use super::*;
use ::core::prelude::v1::test;
use emulsion_core::{
    Node,
    command::Slot,
    design_interactions::{Action, OverlayOperation},
    project::{ProjectEditor, ProjectKind},
};
use gpui_kit::test::TestWindowExt;

fn fixture() -> (Document, NodeId, NodeId) {
    let mut editor = emulsion_core::Editor::new(Document::new(400, 300), None);
    let mut ids = Vec::new();
    for x in [20., 150.] {
        ids.push(
            editor
                .execute(Command::AddNode {
                    node: Box::new(Node::path(
                        0,
                        "Interactive",
                        Arc::new(emulsion_raster::vector_geometry::rectangle(
                            x, 30., 80., 60.,
                        )),
                        emulsion_raster::vector::PathStyle {
                            fill: Some([255; 4]),
                            stroke: None,
                            ..Default::default()
                        },
                        400,
                        300,
                    )),
                    slot: Slot::TOP,
                })
                .unwrap()
                .unwrap(),
        );
    }
    let overlay = editor
        .execute(Command::Group {
            ids: vec![ids[1]],
            name: "Modal".into(),
        })
        .unwrap()
        .unwrap();
    (editor.doc, ids[0], overlay)
}

#[gpui_kit::test]
fn interaction_dialog_authors_overlay_and_canvas_click_escape_preserve_source(
    cx: &mut TestAppContext,
) {
    let (doc, button, overlay) = fixture();
    let (workspace, cx) = crate::tests::open(cx, doc.clone());
    cx.simulate_resize(size(px(1200.), px(1000.)));
    let view = cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc.clone()).unwrap(),
                "Interactive".into(),
                window,
                cx,
            )
        });
        let view = workspace.read(cx).editor.clone().unwrap();
        view.update(cx, |v, cx| {
            v.set_layer_selection(vec![button], Some(button));
            v.design_interactions_dialog(window, cx);
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("design-interaction-add", cx);
        window.click(("design-interaction-type", 0usize), cx);
        window.within("popup-menu").click(4usize, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    let authored = cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            assert_eq!(
                v.editor.doc.design.interactions[&button],
                vec![Action::Overlay {
                    target: overlay,
                    operation: OverlayOperation::Show
                }]
            );
            assert!(v.editor.doc.design.overlays.contains(&overlay));
            let authored = v.editor.doc.clone();
            assert!(v.editor.undo());
            assert_eq!(v.editor.doc, doc);
            assert!(v.editor.redo());
            v.after_change(cx);
            v.start_motion(true, cx);
            window.focus(&v.canvas_focus, cx);
            authored
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let offset = {
            let v = view.read(cx);
            assert!(
                !v.motion
                    .preview
                    .as_ref()
                    .unwrap()
                    .node(overlay)
                    .unwrap()
                    .visible
            );
            let bounds = v.canvas_bounds().unwrap();
            let p = v.view.doc_to_screen((40., 50.), &bounds);
            point(
                px(p.0 as f32) - bounds.origin.x,
                px(p.1 as f32) - bounds.origin.y,
            )
        };
        window.click_at("canvas", offset, cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let v = view.read(cx);
        assert!(
            v.motion
                .preview
                .as_ref()
                .unwrap()
                .node(overlay)
                .unwrap()
                .visible
        );
        assert_eq!(v.editor.doc, authored);
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let v = view.read(cx);
        assert!(v.motion.presenting);
        assert!(
            !v.motion
                .preview
                .as_ref()
                .unwrap()
                .node(overlay)
                .unwrap()
                .visible
        );
        assert_eq!(v.editor.doc, authored);
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let v = view.read(cx);
        assert!(!v.motion.presenting);
        assert_eq!(v.editor.doc, authored);
    });
}

#[gpui_kit::test]
fn slide_actions_back_stack_and_host_execution_preserve_project_history(cx: &mut TestAppContext) {
    let (doc, button, _) = fixture();
    let (workspace, cx) = crate::tests::open(cx, doc.clone());
    let view = cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                "Slides".into(),
                window,
                cx,
            )
        });
        workspace.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    let (first, second, stamp) = cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            let first = v.editor.active_page();
            let second = v.editor.duplicate_page(first).unwrap();
            emulsion_core::design_interactions::author(
                &mut v.editor,
                button,
                vec![Action::Back],
                None,
            )
            .unwrap();
            v.editor.set_active_page(first).unwrap();
            emulsion_core::design_interactions::author(
                &mut v.editor,
                button,
                vec![Action::Slide { page: second }],
                None,
            )
            .unwrap();
            v.after_change(cx);
            let stamp = v.editor.stamp();
            v.start_motion(true, cx);
            window.focus(&v.canvas_focus, cx);
            (first, second, stamp)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.trigger_presentation_object(button, window, cx).unwrap()
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            assert_eq!(v.editor.active_page(), second);
            v.trigger_presentation_object(button, window, cx).unwrap();
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            assert_eq!(v.editor.active_page(), first);
            assert_eq!(v.editor.stamp(), stamp);
            v.stop_motion(cx);
            assert_eq!(v.editor.stamp(), stamp);
        })
    });
}

#[gpui_kit::test]
fn component_variant_click_remains_preview_only_across_animation_ticks(cx: &mut TestAppContext) {
    let (doc, member, group) = fixture();
    let mut editor = emulsion_core::Editor::new(doc, None);
    let trigger = editor.doc.children(Some(group))[0];
    let component =
        emulsion_core::design_components::create(&mut editor, &[member], "Control").unwrap();
    editor
        .execute(Command::SetOpacity {
            id: member,
            opacity: 0.4,
        })
        .unwrap();
    emulsion_core::design_components::update(&mut editor, component, Some("Dim")).unwrap();
    emulsion_core::design_components::reset(&mut editor, component, Some("Default")).unwrap();
    emulsion_core::design_interactions::author(
        &mut editor,
        trigger,
        vec![Action::Variant {
            target: component,
            variant: "Dim".into(),
        }],
        None,
    )
    .unwrap();
    let authored = editor.doc.clone();
    let (workspace, cx) = crate::tests::open(cx, authored.clone());
    let view = cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.install_project(
                ProjectEditor::new_project(ProjectKind::Design, authored.clone()).unwrap(),
                "Variant preview".into(),
                window,
                cx,
            )
        });
        let view = workspace.read(cx).editor.clone().unwrap();
        view.update(cx, |v, cx| {
            v.start_motion(true, cx);
            window.focus(&v.canvas_focus, cx);
        });
        view
    });
    cx.run_until_parked();
    let stamp = cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            let stamp = v.editor.stamp();
            v.presentation_host_action(
                emulsion_mcp::design_motion_tools::HostAction::Trigger(trigger),
                window,
                cx,
            )
            .unwrap();
            stamp
        })
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let v = view.read(cx);
        let preview = v.motion.preview.as_ref().unwrap();
        let child = preview.children(Some(component))[0];
        assert_eq!(preview.node(child).unwrap().opacity, 0.4);
        assert_eq!(v.editor.doc, authored);
        assert_eq!(v.editor.stamp(), stamp);
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let v = view.read(cx);
        assert!(!v.motion.presenting);
        assert_eq!(v.editor.doc, authored);
        assert_eq!(v.editor.stamp(), stamp);
    });
}
