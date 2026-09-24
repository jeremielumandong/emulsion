//! The familiar desktop arrangement must leave usable canvas and layer space.
use super::*;
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn custom_toolbox_drag_add_reorder_and_activation_preserve_document(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1600.), gpui_kit::px(1600.)));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.refresh();
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("compact-layout-trigger", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let source = window.find("toolbox-source-Move").bounds().center();
        let target = window.find("toolbox-selected").bounds().center();
        window.drag(source, target, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).workspace_snapshot().tool_ids, ["Move"]);
        window.click("toolbox-add-Rectangular marquee", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let source = window.find("toolbox-row-Rectangular marquee").bounds();
        let target = window.find("toolbox-row-Move").bounds();
        // Drag the label area, clear of the row's Up/Down/Remove buttons.
        window.drag(
            source.origin + gpui_kit::point(gpui_kit::px(30.), source.size.height / 2.),
            target.origin + gpui_kit::point(gpui_kit::px(30.), target.size.height / 2.),
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(
            editor.read(cx).workspace_snapshot().tool_ids,
            ["Rectangular marquee", "Move"]
        );
        window.click("toolbox-up-Move", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(
            editor.read(cx).workspace_snapshot().tool_ids,
            ["Move", "Rectangular marquee"]
        );
        window.click("workspace-customizer-close", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("custom-tool-Rectangular marquee", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.tool, crate::editor::Tool::Select);
        assert_eq!(editor.tools.select, crate::editor::SelectShape::Rect);
        assert_eq!(editor.editor.doc, original);
        assert_eq!(editor.editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn saved_workspace_default_initializes_next_document_and_can_reset(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["First"], None));
    let second = doc(&["Second"], None);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1600.), gpui_kit::px(1200.)));
    let expected = cx.update(|window, cx| {
        let mut layout = ws
            .read(cx)
            .editor
            .as_ref()
            .unwrap()
            .read(cx)
            .workspace_snapshot();
        layout.tool_ids = vec!["Brush".into(), "Eraser".into()];
        layout.hidden_menu_ids = vec!["image".into()];
        layout.sidebar_width = 400.;
        layout.draw_mode = true;
        let tools = layout
            .toolbar_placements
            .iter_mut()
            .find(|bar| bar.id == "tools")
            .unwrap();
        tools.edge = "floating".into();
        tools.x = 100.;
        tools.y = 140.;
        let settings = &mut cx.global_mut::<AppSettings>().0;
        settings.compact_chrome = true;
        settings.workspace_default = Some(layout.clone());
        ws.update(cx, |ws, cx| {
            ws.install(
                second.clone(),
                None,
                None,
                None,
                "second".into(),
                window,
                cx,
            )
        });
        layout
    });
    cx.run_until_parked();
    let editor = cx.update(|window, cx| {
        let editor = ws.read(cx).editor.clone().unwrap();
        assert_eq!(editor.read(cx).workspace_snapshot(), expected);
        assert!(window.find("custom-tool-Brush").visible());
        assert!(!window.find("image-menu").visible());
        window.click("compact-layout-trigger", cx);
        editor
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("workspace-menu-image", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("image-menu").visible());
        window.click("workspace-restore-default", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).workspace_snapshot(), expected);
        assert!(!window.find("image-menu").visible());
        window.click("layout-reset", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("image-menu").visible());
        assert!(window.try_find("custom-tool-Brush").is_none());
        assert!(editor.read(cx).workspace_snapshot().tool_ids.is_empty());
        assert!(
            editor.read(cx).draw_mode,
            "reset restores this mode's factory layout without leaving it"
        );
        assert_eq!(editor.read(cx).editor.doc, second);
        assert_eq!(editor.read(cx).editor.history.len(), 0);
        // Reset affects the current workspace; a saved default remains available.
        assert_eq!(
            cx.global::<AppSettings>().0.workspace_default.as_ref(),
            Some(&expected)
        );
    });
}

#[gpui_kit::test]
fn history_and_paths_docks_work_without_changing_the_document(cx: &mut TestAppContext) {
    let mut original = doc(&["Photo"], None);
    let path_id = Command::AddNode {
        node: Box::new(Node::path(
            0,
            "Triangle",
            Arc::new(emulsion_raster::vector::Path::from_svg("M 10 10 L 80 10 L 20 60 Z").unwrap()),
            Default::default(),
            256,
            192,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut original)
    .unwrap()
    .unwrap();
    let (ws, cx) = open(cx, original.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.execute(
                Command::SetOpacity {
                    id: path_id,
                    opacity: 0.5,
                },
                cx,
            );
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("history-initial", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc, original));
    cx.update(|window, cx| window.click("history-redo", cx));
    cx.run_until_parked();
    let edited = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    assert_ne!(edited, original);
    cx.update(|window, cx| window.click("dock-paths", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("path-row", path_id), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-paths-content").visible());
        assert!(window.find("sidebar-history-content").visible());
        let e = e.read(cx);
        assert_eq!(e.selected, Some(path_id));
        assert_eq!(e.tool, crate::editor::Tool::Pen);
        assert_eq!(e.editor.doc, edited);
    });
    cx.update(|window, cx| window.click("dock-layers", cx));
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find(("row", path_id)).visible()));
}

#[gpui_kit::test]
fn editor_keeps_tools_left_layers_right_and_options_above_canvas(cx: &mut TestAppContext) {
    let (_ws, cx) = open(cx, doc(&["Photo", "Paint", "Details"], None));
    cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = false;
        window.refresh();
    });
    for (width, height) in [(1000., 720.), (800., 600.), (720., 540.)] {
        cx.simulate_resize(gpui_kit::size(gpui_kit::px(width), gpui_kit::px(height)));
        cx.run_until_parked();
        cx.update(|window, _| {
            let rail = window.find("tool-rail").bounds();
            let scroll = window.find("tool-rail-scroll").bounds();
            let swatches = window.find("tool-rail-swatches").bounds();
            assert!(scroll.origin.y >= rail.origin.y);
            assert!(scroll.origin.y + scroll.size.height <= swatches.origin.y + gpui_kit::px(1.));
            assert!(
                swatches.origin.y + swatches.size.height
                    <= rail.origin.y + rail.size.height + gpui_kit::px(1.)
            );
            assert!(window.find("tool-rail-swatches").visible());
            let options = window.find("editor-tool-options").bounds();
            let area = window.find("editor-work-area").bounds();
            let canvas = window.find("editor-canvas-column").bounds();
            let dock = window.find("node-panel").bounds();
            let layers = window.find("sidebar-layers-dock").bounds();
            let inspector = window
                .find((
                    "sidebar-content",
                    crate::editor::SidebarTab::History as usize,
                ))
                .bounds();
            assert!(
                rail.size.width <= gpui_kit::px(64.),
                "tools should use a narrow rail"
            );
            assert!(canvas.origin.x >= rail.origin.x + rail.size.width);
            assert!(dock.origin.x >= canvas.origin.x + canvas.size.width);
            assert!(
                canvas.size.width >= gpui_kit::px(300.),
                "canvas width at {width}×{height}: {canvas:?}"
            );
            assert!(
                canvas.size.height >= gpui_kit::px(300.),
                "canvas height at {width}×{height}: {canvas:?}"
            );
            assert!(options.origin.y + options.size.height <= area.origin.y + gpui_kit::px(1.));
            assert!(
                layers.size.height > inspector.size.height,
                "Layers should be the main dock at {width}×{height}: {layers:?}, {inspector:?}"
            );
            assert!(inspector.origin.y + inspector.size.height <= layers.origin.y);
            assert!(window.find("sidebar-history-content").visible());
            assert!(window.find("dock-layers").visible());
            assert!(window.find("dock-channels").visible());
            assert!(window.find("dock-paths").visible());
            assert!(window.find(("row", 3u64)).visible());
            assert!(window.find("sidebar-properties").visible());
        });
    }
}

#[gpui_kit::test]
fn compact_editor_gives_canvas_more_room_and_contains_toolbars(cx: &mut TestAppContext) {
    let (_ws, cx) = open(cx, doc(&["Photo", "Paint", "Details"], None));
    for (width, height) in [(800., 600.), (1000., 720.), (1440., 900.)] {
        cx.simulate_resize(gpui_kit::size(gpui_kit::px(width), gpui_kit::px(height)));
        cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = false;
            window.refresh();
        });
        cx.run_until_parked();
        let legacy_canvas = cx.update(|window, _| window.find("editor-canvas-column").bounds());
        cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = true;
            window.refresh();
        });
        cx.run_until_parked();
        let expanded = cx.update(|window, _| {
            let header = window.find("editor-document-bar").bounds();
            assert!(
                header.size.height >= gpui_kit::px(36.)
                    && header.size.height <= gpui_kit::px(40.),
                "compact header should stay comfortable without becoming a second toolbar: {header:?}"
            );
            let canvas = window.find("editor-canvas-column").bounds();
            assert!(canvas.size.width > legacy_canvas.size.width);
            assert!(canvas.size.height > legacy_canvas.size.height);
            // Photoshop's Essentials: the colour swatches sit at the foot
            // of the Tools panel and in the panel dock, not in their own bar.
            assert!(window.try_find("canvas-toolbar-color").is_none());
            assert!(window.find("tool-rail-swatches").visible());
            assert!(window.find("sidebar-swatches").visible());
            for name in ["tools", "options", "view"] {
                let toolbar = window
                    .find(gpui_kit::SharedString::from(format!(
                        "canvas-toolbar-{name}"
                    )))
                    .bounds();
                assert!(toolbar.size.width > gpui_kit::px(0.));
                assert!(toolbar.size.height > gpui_kit::px(0.));
                assert!(
                    toolbar.origin.x >= canvas.origin.x,
                    "{name} at {width}×{height}"
                );
                assert!(
                    toolbar.origin.y >= canvas.origin.y,
                    "{name} at {width}×{height}"
                );
                assert!(
                    toolbar.right() <= canvas.right() + gpui_kit::px(1.),
                    "{name}: {toolbar:?}, canvas: {canvas:?}"
                );
                assert!(
                    toolbar.bottom() <= canvas.bottom() + gpui_kit::px(1.),
                    "{name}: {toolbar:?}, canvas: {canvas:?}"
                );
            }
            canvas
        });
        cx.update(|window, cx| window.click("sidebar-collapse", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let canvas = window.find("editor-canvas-column").bounds();
            assert!(canvas.size.width > expanded.size.width + gpui_kit::px(100.));
            assert!(window.find("sidebar-expand").visible());
            window.click("sidebar-expand", cx);
        });
        cx.run_until_parked();
        cx.update(|window, _| {
            assert_eq!(window.find("editor-canvas-column").bounds(), expanded);
            assert!(window.find(("row", 3u64)).visible());
        });
    }
}

#[gpui_kit::test]
fn compact_toolbars_restore_and_presets_preserve_document(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(900.)));
    cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.refresh();
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("toolbar-close-tools", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("canvas-toolbar-tools").is_none());
        if window.try_find("compact-layout-trigger").is_some() {
            window.click("compact-layout-trigger", cx);
        }
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("toolbar-toggle-tools", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("canvas-toolbar-tools").visible());
        window.click("layout-preset-minimal", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("canvas-toolbar-tools").visible());
        for name in ["options", "view", "color"] {
            assert!(
                window
                    .try_find(gpui_kit::SharedString::from(format!(
                        "canvas-toolbar-{name}"
                    )))
                    .is_none()
            );
        }
        assert!(window.find("sidebar-expand").visible());
        window.click("layout-reset", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        for name in ["tools", "options", "view"] {
            assert!(
                window
                    .find(gpui_kit::SharedString::from(format!(
                        "canvas-toolbar-{name}"
                    )))
                    .visible()
            );
        }
        assert!(window.try_find("canvas-toolbar-color").is_none());
        assert!(window.find("sidebar-collapse").visible());
        let editor = ws.read(cx).editor.as_ref().unwrap().read(cx);
        assert_eq!(editor.editor.doc, original);
        assert_eq!(editor.editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn compact_tool_grip_docks_with_arrow_keys(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(720.)));
    cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.refresh();
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("toolbar-grip-tools", cx));
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    cx.update(|window, _| {
        let canvas = window.find("editor-canvas-column").bounds();
        let tools = window.find("canvas-toolbar-tools").bounds();
        assert!(tools.origin.x > canvas.center().x);
        assert!(tools.right() <= canvas.right());
    });
    cx.simulate_keystrokes("up");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let canvas = window.find("editor-canvas-column").bounds();
        let tools = window.find("canvas-toolbar-tools").bounds();
        assert!(tools.size.width > tools.size.height);
        assert!(tools.origin.y < canvas.origin.y + gpui_kit::px(20.));
        assert!(tools.right() <= canvas.right());
        assert_eq!(
            ws.read(cx)
                .editor
                .as_ref()
                .unwrap()
                .read(cx)
                .editor
                .history
                .len(),
            0
        );
    });
}

#[gpui_kit::test]
fn grouped_tools_remain_clickable_outside_the_scrolling_rail(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = false;
        window.refresh();
    });
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(600.)));
    cx.run_until_parked();
    for _ in 0..2 {
        cx.update(|window, cx| window.click("Rectangular marquee", cx));
        cx.run_until_parked();
    }
    // The active trigger closes the open menu, and can reopen it.
    cx.update(|window, cx| window.click("Rectangular marquee", cx));
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.try_find(("rail-flyout", 1usize)).is_none()));
    cx.update(|window, cx| window.click("Rectangular marquee", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let popup = window.find(("rail-flyout", 1usize));
        assert!(popup.visible());
        let rail = window.find("tool-rail").bounds();
        assert!(
            popup.bounds().origin.x + popup.bounds().size.width
                > rail.origin.x + rail.size.width + gpui_kit::px(100.)
        );
        window.click(("rail-flyout-item", 17usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = ws.read(cx).editor.as_ref().unwrap().read(cx);
        assert_eq!(e.tool, crate::editor::Tool::Select);
        assert_eq!(e.tools.select, crate::editor::SelectShape::Ellipse);
        assert!(window.try_find(("rail-flyout", 1usize)).is_none());
        assert_eq!(e.editor.history.len(), 0);
    });
    cx.update(|window, cx| window.click("Elliptical marquee", cx));
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find(("rail-flyout", 1usize)).visible()));
    cx.simulate_click(
        gpui_kit::point(gpui_kit::px(700.), gpui_kit::px(590.)),
        gpui_kit::Modifiers::none(),
    );
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.try_find(("rail-flyout", 1usize)).is_none()));
}

fn compact(
    cx: &mut TestAppContext,
    original: Document,
    width: f32,
    height: f32,
) -> (
    Entity<Workspace>,
    Entity<crate::editor::EditorView>,
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, original);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(width), gpui_kit::px(height)));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.refresh();
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    (ws, editor, cx)
}

#[gpui_kit::test]
fn photo_and_draw_modes_each_remember_their_own_workspace(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (_ws, editor, cx) = compact(cx, original.clone(), 1440., 900.);
    cx.update(|window, cx| {
        assert!(window.find("mode-photo").visible());
        assert!(window.try_find("canvas-toolbar-dock").is_none());
        assert!(window.find("canvas-toolbar-options").visible());
        window.click("mode-draw", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(editor.read(cx).draw_mode);
        assert!(window.find("canvas-toolbar-dock").visible());
        assert!(window.find("canvas-toolbar-brushes").visible());
        assert!(window.find("dock-paint").visible());
        assert!(window.try_find("canvas-toolbar-options").is_none());
        window.click("toolbar-close-tools", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("canvas-toolbar-tools").is_none());
        window.click("mode-photo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(!editor.read(cx).draw_mode);
        assert!(
            window.find("canvas-toolbar-tools").visible(),
            "photo keeps its tools"
        );
        assert!(window.try_find("canvas-toolbar-dock").is_none());
        window.click("mode-draw", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            window.try_find("canvas-toolbar-tools").is_none(),
            "draw restores the toolbox it was left with"
        );
        assert!(window.find("canvas-toolbar-dock").visible());
        let settings = &cx.global::<AppSettings>().0;
        assert!(settings.photo_workspace.is_some());
        assert_eq!(editor.read(cx).editor.doc, original);
        assert_eq!(editor.read(cx).editor.history.len(), 0);
    });
    cx.dispatch_action(crate::actions::ToggleDrawMode);
    cx.run_until_parked();
    cx.update(|_, cx| assert!(!editor.read(cx).draw_mode, "Ctrl+Alt+Shift+D switches too"));
}

#[gpui_kit::test]
fn rapid_mode_switches_persist_the_latest_workspace_and_settings(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (_ws, editor, cx) = compact(cx, original.clone(), 1440., 900.);
    let expected = cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            for _ in 0..3 {
                editor.toggle_draw_mode(cx);
            }
            assert!(editor.draw_mode);
            assert_eq!(editor.editor.doc, original);
            assert_eq!(editor.editor.history.len(), 0);
        });
        // A later, unrelated preference must not be overwritten by a queued
        // workspace snapshot when a slow disk finally catches up.
        crate::app_state::update_settings(cx, |settings| settings.layers_height = 360.);
        crate::app_state::settings(cx).clone()
    });
    cx.run_until_parked();
    assert_eq!(emulsion_io::settings::Settings::load(), expected);
    cx.update(|window, cx| {
        assert!(window.find("canvas-toolbar-dock").visible());
        assert!(editor.read(cx).draw_mode);
        assert_eq!(editor.read(cx).editor.doc, original);
        assert_eq!(editor.read(cx).editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn toolbars_scale_and_dock_from_the_customizer(cx: &mut TestAppContext) {
    let (_ws, editor, cx) = compact(cx, doc(&["Photo"], None), 1440., 1000.);
    let before = cx.update(|window, cx| {
        window.click("compact-layout-trigger", cx);
        window.find("canvas-toolbar-tools").bounds()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("toolbar-scale-tools-XL", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let after = window.find("canvas-toolbar-tools").bounds();
        assert!(
            after.size.width > before.size.width * 1.3,
            "XL tools: {before:?} -> {after:?}"
        );
        assert_eq!(
            editor.read(cx).workspace_snapshot().toolbar_placements[0].scale,
            1.5
        );
        window.click("toolbar-dock-tools-dock-right", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let canvas = window.find("editor-canvas-column").bounds();
        let tools = window.find("canvas-toolbar-tools").bounds();
        assert!(tools.origin.x > canvas.center().x);
        window.click("toolbar-dock-tools-float-over-the-canvas", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let layout = editor.read(cx).workspace_snapshot();
        assert_eq!(layout.toolbar_placements[0].edge, "floating");
        window.click("toolbar-scale-all-S", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let layout = editor.read(cx).workspace_snapshot();
        assert!(
            layout
                .toolbar_placements
                .iter()
                .all(|bar| bar.scale == 0.85)
        );
        let mut restored = layout.clone();
        editor.update(cx, |e, cx| e.apply_workspace_layout(&restored, cx));
        restored = editor.read(cx).workspace_snapshot();
        assert_eq!(restored, layout);
    });
}

#[gpui_kit::test]
fn brush_gallery_and_project_colours_pick_in_one_click(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (_ws, editor, cx) = compact(cx, original.clone(), 1440., 1000.);
    cx.update(|window, cx| {
        // Tests share one data directory; keep this library in memory only so
        // recording the picked brush as recent cannot race other tests' saves.
        crate::editor::shared_library(cx).update(cx, |library, _| {
            library.error = Some("read-only for this test".into())
        });
        editor.update(cx, |e, _| {
            e.editor.doc.colors = vec![[10, 200, 30], [1, 2, 3]]
        });
        window.click("mode-draw", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find(("shelf-brush", 0usize)).visible());
        window.click("brush-gallery-toggle", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("brush-gallery").visible());
        window.click(("gallery-brush", 0usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(editor.read(cx).presets.current_id.is_some());
        window.click("gallery-close", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("brush-gallery").is_none());
        window.click(("project-color", 0usize), cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.tools.fg, [10, 200, 30, 255]);
        assert_eq!(e.editor.history.len(), 0);
        let mut expected = original.clone();
        expected.colors = e.editor.doc.colors.clone();
        assert_eq!(e.editor.doc, expected);
    });
}

#[gpui_kit::test]
fn photo_mode_matches_photoshop_essentials_layout(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (_ws, editor, cx) = compact(cx, original.clone(), 1440., 900.);
    cx.update(|window, cx| {
        // Menu bar in Photoshop's order.
        let menus: Vec<_> = [
            "file", "edit", "image", "layer", "select", "filter", "view", "window",
        ]
        .into_iter()
        .map(|id| {
            window
                .find(gpui_kit::SharedString::from(format!("{id}-menu-button")))
                .bounds()
                .origin
                .x
        })
        .collect();
        assert!(menus.windows(2).all(|pair| pair[0] < pair[1]), "{menus:?}");
        // Tools docked left and the options bar across the top, both beside
        // the canvas rather than over it.
        let column = window.find("editor-canvas-column").bounds();
        let canvas = window.find("canvas").bounds();
        let tools = window.find("canvas-toolbar-tools").bounds();
        let options = window.find("canvas-toolbar-options").bounds();
        assert!(
            tools.right() <= canvas.origin.x + gpui_kit::px(1.),
            "{tools:?} {canvas:?}"
        );
        assert!(
            options.bottom() <= canvas.origin.y + gpui_kit::px(1.),
            "{options:?} {canvas:?}"
        );
        assert!(
            tools.size.height > column.size.height * 0.7,
            "full-height tools"
        );
        assert!(
            options.size.width > column.size.width * 0.95,
            "full-width options"
        );
        // Foreground and background colours at the foot of the tools.
        let swatches = window.find("tool-rail-swatches").bounds();
        assert!(swatches.origin.y > window.find("tool-rail").bounds().bottom() - gpui_kit::px(1.));
        window.click("compact-layout-trigger", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("toolbar-placement-over", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let canvas = window.find("canvas").bounds();
        let tools = window.find("canvas-toolbar-tools").bounds();
        assert!(
            tools.origin.x >= canvas.origin.x,
            "overlay floats over the canvas"
        );
        let layout = editor.read(cx).workspace_snapshot();
        assert_eq!(layout.toolbars_overlay, Some(true));
        assert_eq!(editor.read(cx).editor.doc, original);
        assert_eq!(editor.read(cx).editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn photo_tabs_sit_above_the_canvas_and_panels_open_from_window_menu(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (_ws, editor, cx) = compact(cx, original.clone(), 1440., 900.);
    cx.update(|window, cx| {
        // Photoshop: menus in the header, document tabs under the options
        // bar, between the Tools panel and the panel dock.
        let header = window.find("editor-document-bar").bounds();
        let tab_bar = window.find("document-tab-bar").bounds();
        let tabs = window.find("compact-document-tabs").bounds();
        let options = window.find("canvas-toolbar-options").bounds();
        let tools = window.find("canvas-toolbar-tools").bounds();
        let canvas = window.find("canvas").bounds();
        assert!(tabs.origin.y >= tab_bar.origin.y && tabs.bottom() <= tab_bar.bottom());
        assert!(tab_bar.origin.y >= header.bottom());
        assert!(tab_bar.origin.y >= options.bottom() - gpui_kit::px(1.));
        assert!(tab_bar.bottom() <= canvas.origin.y + gpui_kit::px(1.));
        assert!(tab_bar.origin.x >= tools.right() - gpui_kit::px(1.));
        window.click("dock-channels", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).dock_tab, crate::editor::DockTab::Channels);
        window.click("sidebar-collapse", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("sidebar-rail-dock", 2usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            window.find("sidebar-collapse").visible(),
            "the collapsed strip reopens the dock"
        );
        assert_eq!(editor.read(cx).dock_tab, crate::editor::DockTab::Paths);
        window.click("window-menu-button", cx);
    });
    cx.run_until_parked();
    // Window ▸ Layers.
    let point = cx.update(|window, _| window.within("popup-menu").find(21usize).bounds().center());
    cx.simulate_click(point, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(editor.read(cx).dock_tab, crate::editor::DockTab::Layers);
        assert_eq!(editor.read(cx).editor.doc, original);
    });
    // Draw mode keeps the tabs in the header, where Procreate-style
    // overlays leave the canvas edge-to-edge.
    cx.update(|window, cx| window.click("mode-draw", cx));
    cx.run_until_parked();
    cx.update(|window, _| {
        assert!(window.try_find("document-tab-bar").is_none());
        let header = window.find("editor-document-bar").bounds();
        let tabs = window.find("compact-document-tabs").bounds();
        assert!(tabs.bottom() <= header.bottom() + gpui_kit::px(1.));
    });
}

#[gpui_kit::test]
fn tools_panel_has_quick_mask_and_a_double_column_toggle(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (_ws, editor, cx) = compact(cx, original.clone(), 1440., 900.);
    let single = cx.update(|window, cx| {
        // Photoshop: the Quick Mask button sits under the colour swatches.
        let swatches = window.find("fg-swatch").bounds();
        let quick = window.find("quick-mask-toggle").bounds();
        assert!(quick.origin.y >= swatches.bottom() - gpui_kit::px(1.));
        window.click("quick-mask-toggle", cx);
        window.find("tool-rail").bounds()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(editor.read(cx).tools.quick_mask);
        window.click("quick-mask-toggle", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(!editor.read(cx).tools.quick_mask);
        window.click("tool-columns-toggle", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let double = window.find("tool-rail").bounds();
        assert!(
            double.size.width > single.size.width * 1.6,
            "two columns: {single:?} -> {double:?}"
        );
        assert!(double.size.height < single.size.height * 0.7);
        let layout = editor.read(cx).workspace_snapshot();
        assert_eq!(layout.tool_columns, 2);
        let mut factory = layout.clone();
        factory.tool_columns = 1;
        editor.update(cx, |e, cx| e.apply_workspace_layout(&factory, cx));
        assert_eq!(editor.read(cx).workspace_snapshot().tool_columns, 1);
        editor.update(cx, |e, cx| e.apply_workspace_layout(&layout, cx));
        assert_eq!(editor.read(cx).workspace_snapshot().tool_columns, 2);
        assert_eq!(editor.read(cx).editor.doc, original);
    });
}
