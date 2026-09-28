use super::*;
use emulsion_core::{
    NodeKind,
    diagram::{Builder, ShapeKind},
    project::{ProjectEditor, ProjectKind},
};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn diagram_inspector_tabs_format_graph_objects_and_fill_is_undoable(cx: &mut TestAppContext) {
    let mut builder = Builder::new(800, 600).unwrap();
    let id = builder
        .add_shape(ShapeKind::Process, [100., 100., 120., 60.], "Task")
        .unwrap();
    let doc = builder.finish().unwrap();
    let body = doc.diagram.as_ref().unwrap().shapes[&id].body;
    let label = doc.diagram.as_ref().unwrap().shapes[&id].label;
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Diagram".into(),
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
    cx.update(|window, cx| {
        assert!(window.find("diagram-inspector").visible());
        window.click(("diagram-fill", 3usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window,cx| {
        let editor=view.read(cx);
        assert!(matches!(&editor.editor.doc.node(body).unwrap().kind,NodeKind::Path{style,..} if style.fill==Some([213,232,212,255])));
        assert_eq!(editor.text_target().unwrap().0,label);
        window.click("project-undo",cx);
    });
    cx.run_until_parked();
    cx.update(|window,cx| {
        assert!(matches!(&view.read(cx).editor.doc.node(body).unwrap().kind,NodeKind::Path{style,..} if style.fill==Some([233,239,251,255])));
        window.click(("diagram-property-tab",1usize),cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("diagram-edit-text").visible());
        window.click(("diagram-property-tab", 2usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find(("diagram-inspector-layout", 0usize)).visible());
        window.click(("diagram-property-tab", 3usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find("diagram-edit-data").visible()));
}
