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
            assert!(window.find("design-chart-grid").visible());
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
        assert!(window.find("design-chart-grid").visible());
        window.click(("design-chart-input", 1usize), cx);
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("480");
    cx.update(|window, cx| window.click(("design-chart-input", 0usize), cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Quarterly results");
    cx.update(|window, cx| window.click("design-chart-csv-mode", cx));
    cx.run_until_parked();
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

#[gpui_kit::test]
fn chart_grid_edits_structure_converts_and_preserves_cancel_undo_and_save(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Data grid".into(),
                window,
                cx,
            );
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-section", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-chart-add", 0usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    let original = cx.update(|window, cx| {
        let doc = view.read(cx).editor.doc.clone();
        window.click("design-chart-edit-selection", cx);
        doc
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-chart-cell", 10usize), cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("125.5");
    cx.update(|window, cx| window.click("design-chart-add-row", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-chart-add-column", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-chart-remove-row", 2usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-chart-kind", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    let changed = cx.update(|window, cx| {
        let editor = view.read(cx);
        let id = editor.selected.unwrap();
        assert!(
            original.design.charts.contains_key(&id),
            "conversion preserves group identity"
        );
        let chart = &editor.editor.doc.design.charts[&id];
        assert_eq!(chart.kind, Kind::Line);
        assert_eq!(
            chart.rows,
            vec![
                vec!["Category", "Value", "Series 2"],
                vec!["First", "125.5", "0"],
                vec!["Third", "20", "0"],
                vec!["", "0", "0"],
            ]
        );
        let changed = editor.editor.doc.clone();
        window.click("design-undo", cx);
        changed
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        window.click("design-redo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, changed);
        window.click("design-chart-edit", cx);
    });
    cx.run_until_parked();
    // Pie cannot silently discard a second series. Keep the dialog and draft.
    cx.update(|window, cx| window.click(("design-chart-kind", 2usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, changed);
        assert!(window.find("design-chart-error").visible());
        window.click(("design-chart-remove-column", 2usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    let pie = cx.update(|window, cx| {
        let doc = view.read(cx).editor.doc.clone();
        let chart = doc.design.charts.values().next().unwrap();
        assert_eq!(chart.kind, Kind::Pie);
        assert_eq!(chart.rows[1], ["First", "125.5"]);
        let project = ProjectEditor::new_project(ProjectKind::Design, doc.clone())
            .unwrap()
            .snapshot()
            .unwrap();
        let mut archive = std::io::Cursor::new(Vec::new());
        emulsion_io::project::write_to(&project, &mut archive).unwrap();
        let restored =
            emulsion_io::project::read_from(std::io::Cursor::new(archive.into_inner())).unwrap();
        assert_eq!(restored.pages[0].doc, doc);
        window.click("design-chart-edit", cx);
        doc
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-chart-add-row", cx));
    cx.run_until_parked();
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(640.), gpui_kit::px(720.)));
    cx.run_until_parked();
    cx.update(|window, _| {
        for id in ["design-chart-grid", "ok"] {
            let control = window.find(id);
            assert!(
                control.visible(),
                "{id} remains reachable in a small window"
            );
            let bounds = control.bounds();
            assert!(bounds.origin.x >= gpui_kit::px(0.));
            assert!(bounds.right() <= gpui_kit::px(640.), "{id}: {bounds:?}");
            assert!(bounds.bottom() <= gpui_kit::px(720.), "{id}: {bounds:?}");
        }
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, pie));
}

#[gpui_kit::test]
fn chart_grid_csv_switching_preserves_quotes_unicode_and_rejects_bad_data(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Table data".into(),
                window,
                cx,
            );
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-section", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-chart-add", 3usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-chart-csv-mode", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-chart-data", cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Label,Notes\n\"broken,1");
    cx.update(|window, cx| window.click("design-chart-grid-mode", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("design-chart-data").visible());
        assert!(window.find("design-chart-error").visible());
        assert!(view.read(cx).editor.doc.design.charts.is_empty());
        window.click("design-chart-data", cx);
    });
    cx.simulate_keystrokes("ctrl-a");
    let rows = vec![
        vec!["Label".into(), "Notes".into()],
        vec![
            "日本語".into(),
            "Quoted \"text\", with comma\nand newline".into(),
        ],
        vec!["".into(), "".into()],
    ];
    cx.simulate_input(&emulsion_io::design_charts::to_csv(&rows));
    cx.update(|window, cx| window.click("design-chart-grid-mode", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("design-chart-grid").visible());
        window.click("design-chart-csv-mode", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-chart-grid-mode", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = view.read(cx);
        let chart = editor.editor.doc.design.charts.values().next().unwrap();
        assert_eq!(chart.kind, Kind::Table);
        assert_eq!(chart.rows, rows);
    });
}

#[gpui_kit::test]
fn native_chart_axis_and_merge_controls_apply_without_losing_covered_cells(
    cx: &mut TestAppContext,
) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1100.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, Document::new(600, 400)).unwrap(),
                "Advanced charts".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|w, cx| w.click(("design-section", 1usize), cx));
    cx.run_until_parked();
    cx.update(|w, cx| w.click(("design-chart-add", 4usize), cx));
    cx.run_until_parked();
    cx.update(|w, cx| w.click(("design-chart-axis", 4usize), cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("500");
    cx.update(|w, cx| w.click(("design-chart-axis", 5usize), cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("100");
    cx.update(|w, cx| w.click("ok", cx));
    cx.run_until_parked();
    cx.update(|w, cx| {
        assert!(view.read(cx).editor.doc.design.charts.is_empty());
        assert!(w.find("design-chart-error").visible());
        w.click(("design-chart-axis", 4usize), cx);
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("0");
    cx.update(|w, cx| w.click("ok", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        let c = &e.editor.doc.design.charts[&e.selected.unwrap()];
        assert_eq!(c.kind, Kind::Area);
        assert_eq!(c.y_axis.max, Some(100.));
    });
    cx.update(|w, cx| w.click(("design-chart-add", 3usize), cx));
    cx.run_until_parked();
    cx.update(|w, cx| w.click("design-table-merge", cx));
    cx.run_until_parked();
    cx.update(|w, cx| w.click("ok", cx));
    cx.run_until_parked();
    let merged = cx.update(|_, cx| {
        let e = view.read(cx);
        let c = &e.editor.doc.design.charts[&e.selected.unwrap()];
        assert_eq!(c.merges.len(), 1);
        e.editor.doc.clone()
    });
    cx.update(|w, cx| w.click("design-chart-edit", cx));
    cx.run_until_parked();
    cx.update(|w, cx| w.click("design-table-unmerge", cx));
    cx.run_until_parked();
    cx.update(|w, cx| w.click("ok", cx));
    cx.run_until_parked();
    cx.update(|w, cx| {
        let e = view.read(cx);
        let id = e.selected.unwrap();
        assert!(e.editor.doc.design.charts[&id].merges.is_empty());
        assert_eq!(
            e.editor.doc.design.charts[&id].rows,
            merged.design.charts[&id].rows
        );
        w.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, merged));
}
