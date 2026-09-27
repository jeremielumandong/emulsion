//! Handoff shell: real panels and routing at desktop and narrow widths.
use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{ElementId, px, size};

#[gpui_kit::test]
fn shared_dock_groups_preserve_document_and_saved_panel_choices(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1280.), px(900.)));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.refresh();
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let panel = window.find("node-panel").bounds();
        assert_eq!(panel.size.width, px(300.));
        let top = window.find("sidebar-primary-tabs").bounds();
        let colors = window.find("sidebar-color-group").bounds();
        let layers = window.find("sidebar-layers-dock").bounds();
        assert!(top.bottom() <= colors.top() && colors.bottom() <= layers.top());
        for id in [
            "sidebar-properties",
            "sidebar-adjustments",
            "sidebar-history-top",
            "sidebar-assistant",
            "sidebar-more",
            "sidebar-collapse",
        ] {
            let b = window.find(id).bounds();
            assert!(
                b.left() >= panel.left() && b.right() <= panel.right(),
                "{id}: {b:?} exceeds {panel:?}"
            );
        }
        window.click("sidebar-color-tab", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-color-content").visible());
        window.click("dock-paths", cx);
    });
    cx.run_until_parked();
    let saved = cx.update(|_, cx| editor.read(cx).workspace_snapshot());
    assert!(saved.sidebar_color_tab);
    assert_eq!(saved.dock_tab, "paths");
    cx.update(|window, cx| window.click("sidebar-assistant", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-assistant-prompt").visible());
        editor.update(cx, |e, cx| e.apply_workspace_layout(&saved, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-paths-content").visible());
        assert_eq!(editor.read(cx).workspace_snapshot(), saved);
        assert_eq!(editor.read(cx).editor.doc, original);
        assert_eq!(editor.read(cx).editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn narrow_sidebar_reopens_as_overlay_without_consuming_the_canvas(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.refresh();
    });
    cx.simulate_resize(size(px(480.), px(700.)));
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find("sidebar-collapsed").visible()));
    cx.dispatch_action(crate::actions::TogglePanels);
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find("node-panel").visible()));
    cx.dispatch_action(crate::actions::TogglePanels);
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-collapsed").visible());
        window.click("sidebar-expand", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let panel = window.find("node-panel").bounds();
        assert!(panel.left() >= px(0.) && panel.right() <= px(480.));
        assert!(window.find("editor-canvas-column").bounds().size.width > px(280.));
        window.click("dock-channels", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("channels-panel").visible());
        window.click("sidebar-collapse", cx);
        assert_eq!(
            ws.read(cx).editor.as_ref().unwrap().read(cx).editor.doc,
            original
        );
    });
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find("sidebar-collapsed").visible()));
}

#[gpui_kit::test]
fn home_library_navigation_and_narrow_export_settings_stay_reachable(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(size(px(760.), px(800.)));
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.visit_destination(
                crate::workspace::destinations::Destination::Home,
                window,
                cx,
            )
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(window.find("home-library").bounds().size.width, px(56.));
        window.click((ElementId::from("home-destination"), "Library"), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(ws.read(cx).screen, crate::workspace::Screen::Batch);
        assert!(window.find("library-navigation").visible());
        window.click("library-settings-toggle", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let panel = window.find("library-settings-overlay").bounds();
        assert!(panel.left() >= px(0.) && panel.right() <= px(760.));
        assert!(window.find("batch-out").visible());
        window.click("library-settings-close", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click((ElementId::from("library-destination"), "Home"), cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(ws.read(cx).screen, crate::workspace::Screen::Home));
}

#[gpui_kit::test]
fn shared_dock_sections_collapse_and_resize_from_keyboard(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["One", "Two", "Three"], None));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.refresh();
        ws.read(cx).editor.clone().unwrap()
    });
    cx.simulate_resize(size(px(1000.), px(720.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("sidebar-layers-toggle", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(
            window.find("sidebar-layers-dock").bounds().size.height,
            px(32.)
        );
        assert!(
            editor
                .read(cx)
                .workspace_snapshot()
                .sidebar_layers_collapsed
        );
        window.click("dock-layers", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            !editor
                .read(cx)
                .workspace_snapshot()
                .sidebar_layers_collapsed
        );
        assert!(window.find(("row", 1u64)).visible());
        for _ in 0..200 {
            window.focus_next(cx);
            window.render_frame(cx);
            if window.find("sidebar-color-resize").focused() == Some(true) {
                break;
            }
        }
        assert_eq!(window.find("sidebar-color-resize").focused(), Some(true));
        window.press("down", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            editor.read(cx).workspace_snapshot().sidebar_colors_height,
            80.
        );
        assert_eq!(editor.read(cx).editor.history.len(), 0);
    });
}

#[test]
fn pre_shared_dock_workspace_json_keeps_custom_layout_and_defaults_new_sections() {
    let saved: emulsion_io::settings::WorkspaceLayout = serde_json::from_str(
        r#"{
        "draw_mode":true,"sidebar_width":410,"sidebar_tab":"reference",
        "toolbars_overlay":true,"toolbar_placements":[
            {"id":"Tools","edge":"floating","visible":true,"x":91,"y":73,"scale":1.25}
        ]
    }"#,
    )
    .unwrap();
    assert_eq!(saved.sidebar_width, 410.);
    assert_eq!(saved.sidebar_tab, "reference");
    assert_eq!(saved.toolbars_overlay, Some(true));
    assert_eq!(saved.toolbar_placements[0].x, 91.);
    assert_eq!(saved.dock_tab, "layers");
    assert!(
        !saved.sidebar_upper_collapsed
            && !saved.sidebar_layers_collapsed
            && !saved.sidebar_colors_collapsed
    );
    assert_eq!(saved.sidebar_colors_height, 64.);
}
