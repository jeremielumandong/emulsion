use super::*;
use emulsion_core::{
    NodeKind,
    project::{ProjectEditor, ProjectKind},
    text::TextSpec,
};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn saved_styles_create_apply_update_reset_detach_and_undo_through_controls(
    cx: &mut TestAppContext,
) {
    let mut doc = Document::new(800, 600);
    let mut ids = Vec::new();
    for (text, x, size) in [("Heading", 30., 48.), ("Own content", 300., 22.)] {
        ids.push(
            Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    text,
                    TextSpec {
                        text: text.into(),
                        x,
                        y: 80.,
                        size,
                        ..Default::default()
                    },
                    800,
                    600,
                )),
                slot: emulsion_core::command::Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap()
            .unwrap(),
        );
    }
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                "Styles".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |editor, cx| {
            editor.set_layer_selection(vec![ids[0]], Some(ids[0]));
            cx.notify();
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-section", 6usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-style-create", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-style-name", cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Titles");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |editor, cx| {
            assert!(editor.editor.doc.design.saved_styles.contains_key("Titles"));
            editor.set_layer_selection(vec![ids[1]], Some(ids[1]));
            cx.notify();
        })
    });
    cx.run_until_parked();
    let before = cx.update(|window, cx| {
        let before = view.read(cx).editor.doc.clone();
        window.click(("design-style-apply", 0usize), cx);
        before
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        let NodeKind::Text { spec, .. } = &e.editor.doc.node(ids[1]).unwrap().kind else {
            panic!()
        };
        assert_eq!(
            (spec.text.as_str(), spec.x, spec.size),
            ("Own content", 300., 48.)
        );
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, before);
        window.click("design-redo", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |editor, cx| {
            editor.execute(
                Command::SetOpacity {
                    id: ids[1],
                    opacity: 0.4,
                },
                cx,
            );
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-style-manage", 0usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.within("popup-menu").click(0usize, cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |editor, cx| {
            assert_eq!(editor.editor.doc.node(ids[0]).unwrap().opacity, 0.4);
            editor.execute(
                Command::SetOpacity {
                    id: ids[1],
                    opacity: 0.7,
                },
                cx,
            );
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-style-reset", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc.node(ids[1]).unwrap().opacity, 0.4);
        window.click("design-style-detach", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let doc = &view.read(cx).editor.doc;
        assert!(!doc.design.style_links.contains_key(&ids[1]));
        assert_eq!(doc.node(ids[1]).unwrap().opacity, 0.4);
    });
}
