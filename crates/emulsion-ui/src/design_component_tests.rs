//! Native dialogs create, publish variants and insert editable linked artwork.
use super::*;
use emulsion_core::{
    Node, NodeKind,
    command::Slot,
    project::{ProjectEditor, ProjectKind},
    text::TextSpec,
};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn design_components_dialog_create_variant_insert_and_undo(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Components".into(),
                window,
                cx,
            );
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |v, cx| {
            let id = v
                .editor
                .execute(Command::AddNode {
                    node: Box::new(Node::text(
                        0,
                        "Title",
                        TextSpec {
                            text: "Editable source".into(),
                            x: 30.,
                            y: 40.,
                            ..Default::default()
                        },
                        600,
                        400,
                    )),
                    slot: Slot::TOP,
                })
                .unwrap()
                .unwrap();
            v.set_layer_selection(vec![id], Some(id));
            v.design_component_name(false, window, cx);
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-component-name", cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Reusable title");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    let root = cx.update(|window, cx| {
        let root = view.read(cx).selected.unwrap();
        assert_eq!(
            view.read(cx).editor.doc.design.component_links[&root].component,
            "Reusable title"
        );
        view.update(cx, |v, cx| v.design_component_name(true, window, cx));
        root
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-component-name", cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Alternate");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(
            view.read(cx).editor.doc.design.components["Reusable title"]
                .variants
                .len(),
            2
        );
        view.update(cx, |v, cx| v.design_component_library(window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-component-insert", 0usize), cx));
    cx.run_until_parked();
    cx.update(|_,cx|view.update(cx,|v,_|{let inserted=v.selected.unwrap();assert_ne!(inserted,root);assert_eq!(v.editor.doc.design.component_links.len(),2);assert!(v.editor.doc.subtree(inserted).iter().any(|id|matches!(&v.editor.doc.node(*id).unwrap().kind,NodeKind::Text{spec,..} if spec.text=="Editable source")));v.editor.undo();assert_eq!(v.editor.doc.design.component_links.len(),1);assert!(v.editor.doc.node(root).is_some());}));
}

#[gpui_kit::test]
fn design_component_property_dialog_cancel_save_publish_reset_and_undo(cx: &mut TestAppContext) {
    use emulsion_core::design_components as components;
    let mut source = emulsion_core::Editor::new(Document::new(800, 600), None);
    let text = source
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Title",
                TextSpec {
                    text: "Original".into(),
                    x: 40.,
                    y: 60.,
                    ..Default::default()
                },
                800,
                600,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let first = components::create(&mut source, &[text], "Badge").unwrap();
    let second = components::insert(&mut source, "Badge", "Default", (250., 0.)).unwrap();
    // This workflow exercises explicit flags; automatic tracking has its own coverage.
    components::set_auto_overrides(&mut source, second, false).unwrap();
    let local = source.doc.children(Some(second))[0];
    let NodeKind::Text { spec, .. } = &source.doc.node(local).unwrap().kind else {
        panic!()
    };
    let mut spec = (**spec).clone();
    spec.text = "Local words".into();
    source
        .execute(Command::SetText {
            id: local,
            spec: Box::new(spec),
        })
        .unwrap();
    source
        .execute(Command::SetOpacity {
            id: local,
            opacity: 0.4,
        })
        .unwrap();
    let (ws, cx) = open(cx, source.doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(900.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, source.doc.clone()).unwrap(),
                "Overrides".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |v, cx| {
            v.set_layer_selection(vec![local], Some(local));
            v.design_component_overrides(window, cx);
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click(("design-component-override", 0usize), cx);
        window
            .within("design-component-override-footer")
            .click("close", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, source.doc);
        view.update(cx, |v, cx| v.design_component_overrides(window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click(("design-component-override", 0usize), cx);
        window.click(("design-component-override", 3usize), cx);
        window.click("ok", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            let flags = components::overrides_for(&v.editor.doc, second, local);
            assert!(flags.content && flags.opacity);
            let saved = v.editor.doc.clone();
            assert!(v.editor.undo());
            assert_eq!(v.editor.doc, source.doc);
            assert!(v.editor.redo());
            assert_eq!(v.editor.doc, saved);
            let NodeKind::Text { spec, .. } = &v.editor.doc.node(text).unwrap().kind else {
                panic!()
            };
            let mut spec = (**spec).clone();
            spec.text = "Published source".into();
            spec.size = 72.;
            v.editor
                .execute(Command::SetText {
                    id: text,
                    spec: Box::new(spec),
                })
                .unwrap();
            components::update(&mut v.editor, first, None).unwrap();
            let NodeKind::Text { spec, .. } = &v.editor.doc.node(local).unwrap().kind else {
                panic!()
            };
            assert_eq!(spec.text, "Local words");
            assert_eq!(spec.size, 72.);
            assert_eq!(v.editor.doc.node(local).unwrap().opacity, 0.4);
            let before = v.editor.doc.clone();
            components::reset(&mut v.editor, second, None).unwrap();
            assert!(components::overrides_for(&v.editor.doc, second, local).is_empty());
            let NodeKind::Text { spec, .. } = &v.editor.doc.node(local).unwrap().kind else {
                panic!()
            };
            assert_eq!(spec.text, "Published source");
            v.editor.undo();
            assert_eq!(v.editor.doc, before);
            v.after_change(cx);
        })
    });
}
