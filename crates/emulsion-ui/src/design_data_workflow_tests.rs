//! CSV binding and generation through native controls.
use super::*;
use emulsion_core::{
    Node, NodeKind,
    command::Slot,
    project::{ProjectEditor, ProjectKind},
    text::TextSpec,
};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn design_data_native_binding_and_record_set_dialog(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(1000.)));
    let (view, id) = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "CSV designs".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        let id = view.update(cx, |v, cx| {
            let id = v
                .editor
                .execute(Command::AddNode {
                    node: Box::new(Node::text(
                        0,
                        "Name",
                        TextSpec {
                            text: "Original".into(),
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
            v.design_data_binding_dialog(window, cx);
            id
        });
        (view, id)
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(view.read(cx).editor.doc.design.data_bindings.is_empty());
        assert!(window.find("design-data-error").visible());
        window.click("design-data-column", cx);
    });
    cx.simulate_input("name");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    let bound = cx.update(|window, cx| {
        assert_eq!(
            view.read(cx).editor.doc.design.data_bindings[&id].column(),
            "name"
        );
        let doc = view.read(cx).editor.doc.clone();
        view.update(cx, |v, cx| {
            v.design_bulk_dialog(Some(("name\nAlice\nBob".into(), ".".into())), window, cx);
        });
        doc
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|_,cx|view.update(cx,|v,_| {
        assert_eq!(v.editor.page_list().len(),3);
        let pages=v.editor.page_list();
        for (page,name) in [(pages[1].id,"Alice"),(pages[2].id,"Bob")] {
            assert!(matches!(&v.editor.page(page).unwrap().doc.node(id).unwrap().kind,NodeKind::Text{spec,..} if spec.text==name));
        }
        v.editor.undo();assert_eq!(v.editor.page_list().len(),1);assert_eq!(v.editor.doc,bound);
        v.editor.undo();assert!(v.editor.doc.design.data_bindings.is_empty());
    }));
}
