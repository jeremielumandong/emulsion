//! Drive the same appearance controls, dialogs and Undo used in the editor.
use super::*;
use emulsion_core::{
    NodeKind,
    project::{ProjectEditor, ProjectKind},
    text::TextSpec,
    text_effects::WarpStyle,
};
use emulsion_raster::{vector::PathStyle, vector_geometry};
use gpui_kit::test::TestWindowExt;

fn input(cx: &mut VisualTestContext, index: usize, value: &str) {
    cx.update(|window, cx| window.click(("design-appearance-input", index), cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input(value);
    cx.run_until_parked();
}
fn open_appearance(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        if window.try_find("design-appearance-controls").is_none() {
            window.click("design-direct-effects", cx);
        }
    });
    cx.run_until_parked();
}
#[gpui_kit::test]
fn appearance_text_dialogs_validate_edit_and_undo_native_objects(cx: &mut TestAppContext) {
    let mut doc = Document::new(600, 400);
    let id = Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Heading",
            TextSpec {
                text: "First line\nSecond line".into(),
                size: 32.,
                x: 60.,
                y: 80.,
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
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc.clone()).unwrap(),
                "Appearance".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![id], Some(id));
            cx.notify();
        });
        view
    });
    cx.run_until_parked();
    open_appearance(cx);
    cx.update(|window, cx| {
        assert!(window.find("design-appearance-controls").visible());
        assert!(window.find("design-appearance-shadow").visible());
        assert!(window.find("design-appearance-outline").visible());
        window.click("design-appearance-spacing", cx);
    });
    cx.run_until_parked();
    input(cx, 0, "NaN");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, doc);
        assert!(window.find("design-appearance-form").visible());
    });
    input(cx, 0, "3.5");
    input(cx, 1, "1.8");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let NodeKind::Text { spec, .. } = &view.read(cx).editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.style_at(0).letter_spacing, 3.5);
        assert_eq!(spec.line_height, 1.8);
        assert_eq!(spec.text, "First line\nSecond line");
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    open_appearance(cx);
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, doc);
        window.click("design-appearance-curve", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-appearance-warp", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.within("popup-menu").click(1usize, cx));
    cx.run_until_parked();
    input(cx, 0, "55");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let NodeKind::Text { spec, .. } = &view.read(cx).editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.warp.style, WarpStyle::Arc);
        assert_eq!(spec.warp.bend, 55.);
        assert!(emulsion_core::text::vector_paths(spec).is_some());
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    open_appearance(cx);
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, doc);
        window.click("design-appearance-background", cx);
    });
    cx.run_until_parked();
    input(cx, 0, "24");
    input(cx, 1, "10");
    input(cx, 2, "6");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        let selected = e.selected.unwrap();
        assert_ne!(selected, id);
        assert!(e.editor.doc.node(selected).unwrap().is_group());
        let children = e.editor.doc.children(Some(selected));
        assert_eq!(children.len(), 2);
        assert_eq!(children[1], id);
        assert!(matches!(
            e.editor.doc.node(children[0]).unwrap().kind,
            NodeKind::Path { .. }
        ));
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, doc));
}

#[gpui_kit::test]
fn appearance_shape_dialogs_edit_stroke_corners_and_opacity(cx: &mut TestAppContext) {
    let mut doc = Document::new(600, 400);
    let id = Command::AddNode {
        node: Box::new(Node::path(
            0,
            "Card",
            Arc::new(vector_geometry::rectangle(50., 60., 240., 160.)),
            PathStyle {
                fill: Some([30, 80, 240, 255]),
                stroke: Some([0, 0, 0, 255]),
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
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc.clone()).unwrap(),
                "Shape appearance".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![id], Some(id));
            cx.notify();
        });
        view
    });
    cx.run_until_parked();
    for (button, value) in [
        ("design-appearance-stroke", "9"),
        ("design-appearance-corners", "25"),
        ("design-appearance-opacity", "42"),
    ] {
        open_appearance(cx);
        cx.update(|window, cx| window.click(button, cx));
        cx.run_until_parked();
        input(cx, 0, value);
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let node = view.read(cx).editor.doc.node(id).unwrap();
            let NodeKind::Path { path, style, .. } = &node.kind else {
                panic!()
            };
            match button {
                "design-appearance-stroke" => assert_eq!(style.width, 9.),
                "design-appearance-corners" => assert_eq!(path.subpaths[0].anchors.len(), 8),
                _ => assert!((node.opacity - 0.42).abs() < 0.001),
            }
            window.click("design-undo", cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, doc));
    }
}
