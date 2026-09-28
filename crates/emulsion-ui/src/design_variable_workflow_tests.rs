//! Variable dialogs edit native objects and share document history.
use super::*;
use emulsion_core::{Node, NodeKind, command::Slot, project::{ProjectEditor, ProjectKind}, text::TextSpec};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn design_variable_dialog_create_bind_edit_cancel_and_undo(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(1000.)));
    let (view, id) = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| ws.install_project(ProjectEditor::new_project(ProjectKind::Design, Document::new(600,400)).unwrap(), "Variables".into(), window, cx));
        let view = ws.read(cx).editor.clone().unwrap();
        let id = view.update(cx, |v, cx| {
            let id = v.editor.execute(Command::AddNode { node: Box::new(Node::text(0,"Title", TextSpec { text: "Native text".into(), ..Default::default() },600,400)), slot: Slot::TOP }).unwrap().unwrap();
            v.set_layer_selection(vec![id], Some(id));
            v.design_variable_dialog(None, window, cx);
            id
        });
        (view, id)
    });
    cx.run_until_parked();
    cx.update(|window,cx| window.click("design-variable-name",cx));
    cx.simulate_input("Accent");
    cx.update(|window,cx| window.click("design-variable-value",cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("#FF3300");
    cx.update(|window,cx| window.click("ok",cx));
    cx.run_until_parked();
    cx.update(|window,cx| view.update(cx,|v,cx| v.design_variable_bind_dialog("Accent".into(),window,cx)));
    cx.run_until_parked();
    cx.update(|window,cx| window.click("ok",cx));
    cx.run_until_parked();
    let bound = cx.update(|window,cx| {
        let doc = view.read(cx).editor.doc.clone();
        assert_eq!(doc.design.variable_bindings[&id].len(),1);
        assert!(matches!(&doc.node(id).unwrap().kind, NodeKind::Text { spec,.. } if spec.color == [255,51,0,255]));
        view.update(cx,|v,cx| v.design_variable_dialog(Some("Accent".into()),window,cx));
        doc
    });
    cx.run_until_parked();
    cx.update(|window,cx| window.click("design-variable-value",cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("invalid");
    cx.update(|window,cx| window.click("ok",cx));
    cx.run_until_parked();
    cx.update(|_,cx| assert_eq!(view.read(cx).editor.doc,bound));
    cx.update(|window,cx| window.click("close",cx));
    cx.run_until_parked();
    cx.update(|window,cx| view.update(cx,|v,cx| v.design_variable_dialog(Some("Accent".into()),window,cx)));
    cx.run_until_parked();
    cx.update(|window,cx| window.click("design-variable-value",cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("#00AAFF");
    cx.update(|window,cx| window.click("ok",cx));
    cx.run_until_parked();
    cx.update(|_,cx| view.update(cx,|v,_| {
        assert!(matches!(&v.editor.doc.node(id).unwrap().kind,NodeKind::Text { spec,.. } if spec.color == [0,170,255,255]));
        v.editor.undo();
        assert_eq!(v.editor.doc,bound);
    }));
}
