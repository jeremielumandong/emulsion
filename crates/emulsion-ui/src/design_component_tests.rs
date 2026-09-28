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
