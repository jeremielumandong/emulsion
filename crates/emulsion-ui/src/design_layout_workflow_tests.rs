//! Native responsive sizing dialogs, decimal metadata and atomic Undo.
use super::*;
use emulsion_core::{
    NodeId,
    design_layout::{self as layout, Flow, Frame},
    project::{ProjectEditor, ProjectKind},
};
use emulsion_raster::{vector::PathStyle, vector_geometry::rectangle};
use gpui_kit::test::TestWindowExt;

fn fixture() -> (Document, NodeId, NodeId) {
    let mut editor = emulsion_core::Editor::new(Document::new(640, 480), None);
    let child = editor
        .execute(Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Fractional rectangle",
                std::sync::Arc::new(rectangle(20., 30., 40.5, 20.25)),
                PathStyle::default(),
                640,
                480,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let group = editor
        .execute(Command::Group {
            ids: vec![child],
            name: "Responsive frame".into(),
        })
        .unwrap()
        .unwrap();
    editor.begin("Layout fixture");
    layout::enable(
        &mut editor,
        group,
        Frame {
            flow: Flow::Row,
            gap: 7.125,
            padding: [4.25, 5.125, 6.25, 8.125],
            ..Frame::default()
        },
        (320.75, 240.5),
    )
    .unwrap();
    editor.end();
    (editor.doc, group, child)
}
fn input(cx: &mut VisualTestContext, prefix: &'static str, index: usize, value: &str) {
    cx.update(|window, cx| window.click((prefix, index), cx));
    cx.simulate_keystrokes("ctrl-a");
    if value.is_empty() {
        cx.simulate_keystrokes("backspace");
    } else {
        cx.simulate_input(value);
    }
    cx.run_until_parked();
}

#[gpui_kit::test]
fn design_canvas_breakpoints_native_add_edit_remove_validation_and_undo(cx: &mut TestAppContext) {
    let (doc, group, _) = fixture();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1100.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc.clone()).unwrap(),
                "Breakpoints".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |v, cx| {
            v.set_layer_selection(vec![group], Some(group));
            v.design_breakpoints_dialog(window, cx);
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-breakpoint-add", cx));
    cx.run_until_parked();
    input(cx, "design-breakpoint-input", 0, "-1");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, doc);
        assert!(window.find("design-breakpoints-error").visible());
    });
    input(cx, "design-breakpoint-input", 0, "500");
    input(cx, "design-breakpoint-input", 1, "12.5");
    cx.update(|window, cx| {
        window.click(("design-breakpoint-flow", 0usize), cx);
        window.click("ok", cx);
    });
    cx.run_until_parked();
    let saved = cx.update(|_, cx| {
        let v = view.read(cx);
        let frame = &v.editor.doc.design.frames[&group];
        assert_eq!(frame.breakpoints.len(), 1);
        assert_eq!(frame.breakpoints[0].min_width, 500.);
        assert_eq!(frame.breakpoints[0].overrides.flow, Some(Flow::Row));
        assert_eq!(frame.breakpoints[0].overrides.gap, Some(12.5));
        assert_eq!(layout::active_breakpoint(&v.editor.doc, group), Some(500.));
        v.editor.doc.clone()
    });
    cx.update(|window, cx| view.update(cx, |v, cx| v.design_breakpoints_dialog(window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click(("design-breakpoint-remove", 0usize), cx);
        window.click("ok", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            assert!(v.editor.doc.design.frames[&group].breakpoints.is_empty());
            assert!(v.editor.undo());
            assert_eq!(v.editor.doc, saved);
            assert!(v.editor.undo());
            assert_eq!(v.editor.doc, doc);
            v.after_change(cx);
        })
    });
}
#[gpui_kit::test]
fn design_layout_frame_limits_preserve_decimals_cancel_and_undo(cx: &mut TestAppContext) {
    let (doc, group, _) = fixture();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(900.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc.clone()).unwrap(),
                "Responsive".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |v, cx| {
            v.set_layer_selection(vec![group], Some(group));
            v.design_layout_dialog(Flow::Row, window, cx);
        });
        view
    });
    cx.run_until_parked();
    input(cx, "design-layout-input", 8, "140.25");
    cx.update(|window, cx| window.within("design-layout-footer").click("close", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, doc);
        view.update(cx, |v, cx| v.design_layout_dialog(Flow::Row, window, cx));
    });
    cx.run_until_parked();
    input(cx, "design-layout-input", 8, "500.75");
    input(cx, "design-layout-input", 9, "140.25");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, doc);
        assert!(window.find("design-layout-dialog-body").visible());
        assert!(window.find("design-layout-error").visible());
    });
    input(cx, "design-layout-input", 8, "140.25");
    input(cx, "design-layout-input", 9, "500.75");
    cx.update(|window, cx| {
        window.click("design-layout-hug-width", cx);
        window.click("design-layout-hug-height", cx);
        window.click("ok", cx);
    });
    cx.run_until_parked();
    let saved = cx.update(|_, cx| {
        let v = view.read(cx);
        let frame = &v.editor.doc.design.frames[&group];
        assert_eq!(frame.gap, 7.125);
        assert_eq!(frame.padding, [4.25, 5.125, 6.25, 8.125]);
        assert_eq!(frame.min_width, Some(140.25));
        assert_eq!(frame.max_width, Some(500.75));
        assert!(frame.hug_width && frame.hug_height);
        let serialized = serde_json::to_value(&v.editor.doc.design).unwrap();
        let restored: emulsion_core::design_metadata::Design =
            serde_json::from_value(serialized).unwrap();
        assert_eq!(restored, v.editor.doc.design);
        v.editor.doc.clone()
    });
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            assert!(v.editor.undo());
            v.after_change(cx);
            assert_eq!(v.editor.doc, doc);
            assert!(v.editor.redo());
            v.after_change(cx);
            assert_eq!(v.editor.doc, saved);
        })
    });
    cx.update(|window, cx| view.update(cx, |v, cx| v.design_layout_dialog(Flow::Row, window, cx)));
    cx.run_until_parked();
    input(cx, "design-layout-input", 8, "");
    input(cx, "design-layout-input", 9, "");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let frame = &view.read(cx).editor.doc.design.frames[&group];
        assert_eq!(frame.min_width, None);
        assert_eq!(frame.max_width, None);
    });
    // A short viewport keeps the apply/cancel footer outside the scrollable body.
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(900.), gpui_kit::px(560.)));
    cx.update(|window, cx| view.update(cx, |v, cx| v.design_layout_dialog(Flow::Row, window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("ok").visible());
        window.within("design-layout-footer").click("close", cx);
    });
}

#[gpui_kit::test]
fn design_layout_child_sizing_ratio_validation_stale_and_locked(cx: &mut TestAppContext) {
    let (doc, group, child) = fixture();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(900.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc.clone()).unwrap(),
                "Responsive children".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |v, cx| {
            v.set_layer_selection(vec![child], Some(child));
            v.design_layout_child_dialog(group, child, window, cx);
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("design-layout-child-width", cx);
        window.click("design-layout-child-height", cx);
        window.click("design-layout-child-aspect", cx);
    });
    input(cx, "design-layout-child-input", 0, "60.25");
    input(cx, "design-layout-child-input", 1, "120.5");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    let saved = cx.update(|_, cx| {
        let v = view.read(cx);
        let settings = v.editor.doc.design.frames[&group].children[&child];
        assert!(settings.fill_width && settings.fill_height);
        assert_eq!(settings.aspect_ratio, Some(2.));
        assert_eq!(settings.min_width, Some(60.25));
        assert_eq!(settings.max_width, Some(120.5));
        v.editor.doc.clone()
    });
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            assert!(v.editor.undo());
            v.after_change(cx);
            assert_eq!(v.editor.doc, doc);
            assert!(v.editor.redo());
            v.after_change(cx);
            assert_eq!(v.editor.doc, saved);
        })
    });
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.design_layout_child_dialog(group, child, window, cx)
        })
    });
    cx.run_until_parked();
    input(cx, "design-layout-child-input", 4, "0");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, saved);
        window
            .within("design-layout-child-footer")
            .click("close", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.design_layout_child_dialog(group, child, window, cx)
        })
    });
    cx.run_until_parked();
    input(cx, "design-layout-child-input", 0, "");
    input(cx, "design-layout-child-input", 1, "");
    cx.update(|window, cx| {
        window.click("design-layout-child-aspect", cx);
        window.click("ok", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let settings = view.read(cx).editor.doc.design.frames[&group].children[&child];
        assert_eq!(settings.aspect_ratio, None);
        assert_eq!(settings.min_width, None);
        assert_eq!(settings.max_width, None);
    });
    // A dialog cannot overwrite changes made since it was opened.
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.design_layout_child_dialog(group, child, window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            v.editor
                .execute(Command::Rename {
                    id: child,
                    name: "Renamed while dialog open".into(),
                })
                .unwrap();
            v.after_change(cx);
        })
    });
    let changed = cx.update(|_, cx| view.read(cx).editor.doc.clone());
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, changed);
        window
            .within("design-layout-child-footer")
            .click("close", cx);
    });
    cx.run_until_parked();
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(900.), gpui_kit::px(560.)));
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.design_layout_child_dialog(group, child, window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("ok").visible());
        window
            .within("design-layout-child-footer")
            .click("close", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.editor.doc.node_mut(child).unwrap().locked = true;
            v.design_layout_child_dialog(group, child, window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.try_find("design-layout-child-dialog-body").is_none()));
}

#[gpui_kit::test]
fn design_container_breakpoint_limits_and_child_override_dialogs(cx: &mut TestAppContext) {
    let (doc, group, child) = fixture();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1300.), gpui_kit::px(1200.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc.clone()).unwrap(),
                "Container layout".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |v, cx| {
            v.set_layer_selection(vec![group], Some(group));
            v.design_breakpoints_dialog(window, cx);
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("design-breakpoint-reference", cx);
        window.click("design-breakpoint-add", cx);
    });
    cx.run_until_parked();
    input(cx, "design-breakpoint-input", 0, "500");
    cx.update(|window, cx| window.click(("design-breakpoint-limits", 0usize), cx));
    cx.run_until_parked();
    input(cx, "design-breakpoint-limit", 1, "300");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    let frame_state = cx.update(|window, cx| {
        let e = &view.read(cx).editor;
        assert_eq!(
            e.doc.design.frames[&group].breakpoint_reference,
            layout::BreakpointReference::Container
        );
        assert_eq!(
            layout::effective_frame(&e.doc, group).unwrap().max_width,
            Some(300.)
        );
        let doc = e.doc.clone();
        view.update(cx, |v, cx| {
            v.design_layout_child_at_dialog(group, child, Some(0), window, cx)
        });
        doc
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-layout-child-inherit", cx));
    cx.run_until_parked();
    input(cx, "design-layout-child-input", 0, "75");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |v, _| {
            assert_eq!(
                v.editor.doc.design.frames[&group].breakpoints[0]
                    .overrides
                    .children[&child]
                    .min_width,
                Some(75.)
            );
            v.editor.undo();
            assert_eq!(v.editor.doc, frame_state);
            v.editor.undo();
            assert_eq!(v.editor.doc, doc);
        })
    });
}
