//! Handoff shell: real panels and routing at desktop and narrow widths.
use super::*;
use crate::editor::SidebarTab;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{ElementId, px, size};

#[gpui_kit::test]
fn photo_shortcuts_open_switch_close_and_dock_without_editing_the_photo(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1280.), px(900.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for compact in [false, true] {
        cx.simulate_resize(size(px(1280.), px(900.)));
        cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = compact;
            window.refresh();
        });
        cx.run_until_parked();
        for (i, tab) in [
            SidebarTab::Properties,
            SidebarTab::BrushSettings,
            SidebarTab::History,
            SidebarTab::Character,
            SidebarTab::Assistant,
        ]
        .into_iter()
        .enumerate()
        {
            cx.update(|window, cx| window.click(("photo-shortcut", i), cx));
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(window.find("photo-shortcut-panel").visible());
                assert!(editor.read(cx).sidebar_layout.flyout_tab == tab);
                assert!(editor.read(cx).sidebar_tab != tab);
                assert!(window.try_find("sidebar-return-from-flyout").is_none());
                let strip = window.find("photo-shortcut-strip").bounds();
                let panel = window.find("photo-shortcut-panel").bounds();
                assert!(panel.right() <= strip.left());
                assert!(panel.left() >= px(0.));
                assert_eq!(editor.read(cx).editor.doc, original);
            });
        }
        cx.update(|window, cx| window.click("photo-shortcut-close", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find("photo-shortcut-panel").is_none());
            window.click(("photo-shortcut", 0usize), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("photo-shortcut-dock", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find("photo-shortcut-panel").is_none());
            assert!(window.find("sidebar-properties-content").visible());
            window.click("photo-shortcut-toggle-dock", cx);
        });
        cx.run_until_parked();
        cx.simulate_resize(size(px(480.), px(800.)));
        cx.run_until_parked();
        cx.update(|window, cx| window.click(("photo-shortcut", 2usize), cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let panel = window.find("photo-shortcut-panel").bounds();
            assert!(panel.left() >= px(0.) && panel.right() <= px(480.));
            assert_eq!(editor.read(cx).editor.doc, original);
            assert_eq!(editor.read(cx).editor.history.len(), 0);
        });
    }
}

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
        assert_eq!(window.find("home-library").bounds().size.width, px(220.));
        window.click((ElementId::from("home-start"), "Library"), cx);
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

#[gpui_kit::test]
fn paint_brush_panel_and_shelf_share_one_host(cx: &mut TestAppContext) {
    let original = doc(&["Paint"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1440.), px(1000.)));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        let editor = ws.read(cx).editor.clone().unwrap();
        editor.update(cx, |editor, cx| {
            editor.toggle_draw_mode(cx);
            editor.set_tool(crate::editor::Tool::Brush, cx);
            editor.show_sidebar_tab(SidebarTab::Properties, cx);
        });
        window.refresh();
        editor
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-properties-content").visible());
        assert!(window.try_find("brush-summary").is_none());
        assert!(window.try_find("brush-settings-panel").is_none());
        assert!(window.try_find(("shelf-brush", 4usize)).is_none());
        window.click("brush-gallery-toggle", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("photo-brushes-content").visible());
        assert!(window.try_find("brush-summary").is_none());
        assert!(window.try_find("brush-gallery").is_none());
        window.click("brush-gallery-toggle", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            window
                .within("photo-shortcut-panel")
                .find("photo-brushes-content")
                .visible()
        );
        assert!(window.try_find("brush-settings-close").is_none());
        window.click("photo-shortcut-dock", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("photo-shortcut-panel").is_none());
        assert!(window.find("photo-brushes-content").visible());
        assert_eq!(editor.read(cx).editor.doc, original);
        assert!(editor.read(cx).editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn header_and_file_export_share_dialog_formats_and_cancellation(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1440.), px(1000.)));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.refresh();
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    let before = cx.update(|window, cx| {
        let bounds = window.find("document-tab-bar").bounds();
        window.click("file-menu-button", cx);
        bounds
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.within("popup-menu").click(8usize, cx));
    cx.run_until_parked();
    assert!(!cx.did_prompt_for_new_path());
    cx.update(|window, cx| {
        assert!(window.find("export-dialog-body").visible());
        window.click(("export-fmt", 1usize), cx); // JPEG
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).export_prefs.ext, "jpg");
        assert_eq!(window.find("document-tab-bar").bounds(), before);
        window.click("export-cancel", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(!editor.read(cx).export_prefs.open);
        assert!(window.try_find("export-dialog-body").is_none());
        window.click("export", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).export_prefs.ext, "jpg");
        assert!(window.find(("export-fmt", 1usize)).visible());
        window.click("export-go", cx);
    });
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("export-dialog-body").is_none());
        assert_eq!(editor.read(cx).editor.doc, original);
        assert!(editor.read(cx).editor.history.is_empty());
        window.click("export", cx);
    });
    cx.run_until_parked();
    cx.simulate_resize(size(px(480.), px(600.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("export-more", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let confirm = window.find("export-go");
        assert!(confirm.visible());
        assert!(confirm.bounds().right() <= px(480.) && confirm.bounds().bottom() <= px(600.));
        window.press("escape", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert!(!editor.read(cx).export_prefs.open));
}

#[gpui_kit::test]
fn project_header_export_keeps_page_formats_inside_common_dialog(cx: &mut TestAppContext) {
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    let original = doc(&["Design"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1440.), px(1000.)));
    let editor = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, original.clone()).unwrap(),
                "Design export".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("project-export-pages", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("export-dialog-body").visible());
        window.click("project-export-options", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.within("popup-menu").click(13usize, cx)); // Current page SVG.
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("export-dialog-body").is_none());
        assert!(!editor.read(cx).export_prefs.open);
        assert_eq!(editor.read(cx).editor.doc, original);
    });
}

#[gpui_kit::test]
fn photo_brush_fields_are_live_bounded_and_do_not_edit_the_document(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1280.), px(1000.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| window.click(("photo-shortcut", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("photo-brush-spacing", cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("35");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!((editor.read(cx).tools.brush.spacing - 0.35).abs() < 0.001);
        assert!(window.find("PhotoBrushSize").visible());
        assert!(window.find("PhotoBrushFlow").visible());
        window.click("photo-brush-roundness", cx);
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("35");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).tools.brush.roundness, 0.35);
        window.click("photo-brush-spacing", cx);
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("NaN");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!((editor.read(cx).tools.brush.spacing - 0.35).abs() < 0.001);
        assert_eq!(editor.read(cx).editor.doc, original);
        assert!(editor.read(cx).editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn photo_properties_fill_uses_its_own_track_and_one_undo_step(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1440.), px(1100.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| window.click(("photo-shortcut", 0usize), cx));
    cx.run_until_parked();
    let track = gpui_kit::SharedString::from(format!("PhotoFillOpacity({id})"));
    let at = cx.update(|window, _| window.find(track.clone()).bounds().center());
    cx.simulate_mouse_down(at, gpui_kit::MouseButton::Left, Default::default());
    cx.simulate_mouse_up(at, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert!((editor.editor.doc.node(id).unwrap().blending.fill_opacity - 0.5).abs() < 0.03);
        assert_eq!(editor.editor.history.len(), 1);
        assert_eq!(editor.editor.doc.node(id).unwrap().opacity, 1.);
    });
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.undo(cx)));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
}
