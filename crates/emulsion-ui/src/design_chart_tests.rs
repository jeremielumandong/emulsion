//! Real Design controls: creation, data edits, validation, detach, and undo.
use super::*;
use emulsion_core::{
    NodeKind,
    design_charts::Kind,
    project::{ProjectEditor, ProjectKind},
};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn design_charts_create_edit_validate_detach_and_undo(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Charts".into(),
                window,
                cx,
            );
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-section", 1usize), cx));
    cx.run_until_parked();
    for (index, kind) in Kind::ALL.into_iter().enumerate() {
        cx.update(|window, cx| window.click(("design-chart-add", index), cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("design-chart-data").visible());
            window.click("ok", cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = view.read(cx);
            let id = editor.selected.unwrap();
            assert_eq!(editor.editor.doc.design.charts[&id].kind, kind);
            assert_eq!(editor.editor.doc.design.charts.len(), index + 1);
            assert!(editor.editor.doc.nodes.iter().all(|node| matches!(
                node.kind,
                NodeKind::Group { .. } | NodeKind::Path { .. } | NodeKind::Text { .. }
            )));
        });
    }
    let original = cx.update(|window, cx| {
        let before = view.read(cx).editor.doc.clone();
        window.click("design-chart-edit", cx);
        before
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-chart-input", 1usize), cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("NaN");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        assert!(window.find("design-chart-data").visible());
        window.click(("design-chart-input", 1usize), cx);
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("480");
    cx.update(|window, cx| window.click(("design-chart-input", 0usize), cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Quarterly results");
    cx.update(|window, cx| window.click("design-chart-data", cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Category,Revenue\nCustom label,123.5");
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        let chart = &e.editor.doc.design.charts[&e.selected.unwrap()];
        assert_eq!(chart.title, "Quarterly results");
        assert_eq!(chart.size.0, 480.);
        assert_eq!(chart.rows[1], ["Custom label", "123.5"]);
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        window.click("design-chart-detach", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc.design.charts.len(), 3);
        assert_eq!(e.editor.doc.nodes, original.nodes);
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
}
