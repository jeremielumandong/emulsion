use super::*;
use crate::editor::{EditorView, PaintKind, SelectShape, ShapeKind, Tool};
use gpui_kit::Modifiers;
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn created_brush_library_and_saved_brush_remain_available_on_canvas(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Brush);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1600.), gpui_kit::px(1200.)));
    let (library, before) = cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.open_brush_workspace(window, cx));
        (
            editor.read(cx).presets.library.clone().unwrap(),
            editor.read(cx).editor.doc.clone(),
        )
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("new-library", cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Canvas library regression");
    cx.update(|window, cx| window.click("save-brush-name", cx));
    cx.run_until_parked();
    let library_id = cx.update(|window, cx| {
        let id = library
            .read(cx)
            .catalog
            .libraries
            .iter()
            .find(|item| item.name == "Canvas library regression")
            .unwrap()
            .id
            .clone();
        window.click("close-brush-library", cx);
        id
    });
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.toggle_presets(cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            window
                .find(gpui_kit::SharedString::from(format!(
                    "preset-library-{library_id}"
                )))
                .visible()
        );
        assert_eq!(
            editor.read(cx).presets.library_id.as_deref(),
            Some(library_id.as_str())
        );
        assert!(editor.read(cx).presets.set_id.is_none());
        window.click("open-brush-library", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("new-brush", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("studio-done", cx));
    cx.run_until_parked();
    let (brush_id, set_id) = cx.update(|window, cx| {
        assert!(
            window.try_find("brush-studio").is_none(),
            "Studio must complete the save"
        );
        let id = editor
            .read(cx)
            .presets
            .current_id
            .clone()
            .expect("saved brush is active");
        let brush = library.read(cx).catalog.brush(&id).unwrap();
        let set_id = brush.set_id.clone();
        assert_eq!(editor.read(cx).brush(), brush.brush);
        assert_eq!(library.read(cx).catalog.recent.first(), Some(&id));
        assert!(
            library
                .read(cx)
                .catalog
                .sets
                .iter()
                .any(|set| set.id == set_id && set.library_id == library_id)
        );
        window.click("close-brush-library", cx);
        (id, set_id)
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            window
                .find(gpui_kit::SharedString::from(format!("preset-set-{set_id}")))
                .visible()
        );
        assert!(
            window
                .find(gpui_kit::SharedString::from(format!("brush-{brush_id}")))
                .visible()
        );
        assert_eq!(
            editor.read(cx).presets.current_id.as_deref(),
            Some(brush_id.as_str())
        );
        assert_eq!(editor.read(cx).editor.doc, before);
        assert!(editor.read(cx).editor.history.is_empty());
    });
}

fn setup(cx: &mut TestAppContext, tool: Tool) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_tool(tool, cx);
            window.focus(&editor.canvas_focus, cx);
        })
    });
    cx.run_until_parked();
    (editor, cx)
}

#[gpui_kit::test]
fn zoom_shift_changes_indicator_without_moving_and_preserves_click_anchor(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Zoom);
    let at = cx.update(|_, cx| editor.read(cx).doc_to_window((80., 60.)).unwrap());
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    assert!(cx.update(|window, _| window.try_find("zoom-cursor-in").is_some()));
    let shift = Modifiers {
        shift: true,
        ..Modifiers::none()
    };
    cx.simulate_modifiers_change(shift);
    cx.run_until_parked();
    assert!(cx.update(|window, _| window.try_find("zoom-cursor-out").is_some()));
    let zoom = cx.update(|_, cx| editor.read(cx).view.zoom);
    cx.simulate_click(at, shift);
    cx.run_until_parked();
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        assert_eq!(editor.view.zoom, zoom * 0.5);
        let after = editor.doc_to_window((80., 60.)).unwrap();
        assert!((f32::from(after.x - at.x)).abs() < 0.01);
        assert!((f32::from(after.y - at.y)).abs() < 0.01);
    });
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert!(cx.update(|window, _| window.try_find("zoom-cursor-in").is_some()));
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).view.zoom), zoom);
    let alt = Modifiers {
        alt: true,
        ..Modifiers::none()
    };
    cx.simulate_modifiers_change(alt);
    cx.run_until_parked();
    assert!(cx.update(|window, _| window.try_find("zoom-cursor-out").is_some()));
    cx.simulate_click(at, alt);
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).view.zoom), zoom * 0.5);
}

#[gpui_kit::test]
fn explicit_subtool_shortcuts_reach_the_advertised_tools(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Hand);
    for (keys, shape) in [
        ("shift-m", SelectShape::Ellipse),
        ("shift-l", SelectShape::Polygon),
        ("alt-l", SelectShape::Magnetic),
        ("shift-w", SelectShape::Quick),
    ] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tool, Tool::Select);
            assert_eq!(editor.read(cx).tools.select, shape);
        });
    }
    for (keys, kind) in [
        ("shift-b", PaintKind::Smudge),
        ("shift-j", PaintKind::Liquify),
    ] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| editor.read(cx).tools.paint), kind);
    }
    for (keys, kind) in [("shift-u", ShapeKind::Ellipse), ("u", ShapeKind::Rect)] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tool, Tool::Shape);
            assert_eq!(editor.read(cx).tools.shape, kind);
        });
    }
    cx.simulate_keystrokes("q");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).tool), Tool::Mask);
    cx.simulate_keystrokes("shift-q");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).tool), Tool::Grade);
}

#[gpui_kit::test]
fn brush_option_slider_supports_keyboard_limits_without_editing_pixels(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Brush);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.update(|window, cx| window.click("ToolHardness", cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("home");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).tools.brush.hardness), 0.);
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert!((cx.update(|_, cx| editor.read(cx).tools.brush.hardness) - 0.01).abs() < 0.0001);
    cx.simulate_keystrokes("shift-right");
    cx.run_until_parked();
    assert!((cx.update(|_, cx| editor.read(cx).tools.brush.hardness) - 0.11).abs() < 0.0001);
    cx.simulate_keystrokes("end");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).tools.brush.hardness), 1.);
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
        before
    );
}

#[gpui_kit::test]
fn brush_quick_controls_open_at_pointer_and_change_settings_without_painting(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, Tool::Brush);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(3840.), gpui_kit::px(2160.)));
    cx.run_until_parked();
    let (before, position) = cx.update(|_, cx| {
        let editor = editor.read(cx);
        let canvas = editor.canvas_bounds.get().unwrap();
        (
            editor.editor.doc.clone(),
            canvas.origin + gpui_kit::point(gpui_kit::px(200.), gpui_kit::px(220.)),
        )
    });
    cx.simulate_mouse_down(position, gpui_kit::MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let panel = window.find("brush-quick-controls").bounds();
        assert!((f32::from(panel.origin.x - position.x)).abs() < 80.);
        assert!(panel.right() < window.viewport_size().width / 2.);
        assert!(window.try_find("brush-settings-panel").is_none());
        let size = window.find("QuickBrushSize").bounds();
        let toolbar_size = window.find("ToolSize").bounds();
        assert_ne!(size, toolbar_size);
        window.drag(
            gpui_kit::point(size.origin.x + size.size.width * 0.2, size.center().y),
            gpui_kit::point(size.origin.x + size.size.width * 0.8, size.center().y),
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(editor.read(cx).tools.brush.size > 250.);
        assert!(window.find("brush-quick-controls").visible());
        window.click("QuickBrushHardness", cx);
        assert!(
            !editor.read(cx).has_active_gesture(),
            "popup click must finish slider gesture"
        );
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("home");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| editor.read(cx).tools.brush.hardness), 0.);
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert!((cx.update(|_, cx| editor.read(cx).tools.brush.hardness) - 0.01).abs() < 0.0001);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("brush-quick-controls").is_none());
        let editor = editor.read(cx);
        assert!(editor.canvas_focus.is_focused(window));
        assert!(!editor.has_active_gesture());
        assert_eq!(editor.editor.doc, before);
        assert!(editor.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn brush_quick_controls_stay_with_toolbar_and_do_not_replace_other_tool_menus(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, Tool::Brush);
    let canvas_before = cx.update(|window, cx| {
        let bounds = window.find("editor-canvas-column").bounds();
        window.click("brush-settings", cx);
        bounds
    });
    cx.run_until_parked();
    cx.update(|window, _| {
        assert!(window.find("brush-quick-controls").visible());
        assert!(window.find("QuickBrushOpacity").visible());
        assert!(window.find("QuickBrushFlow").visible());
        assert_eq!(window.find("editor-canvas-column").bounds(), canvas_before);
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let position = cx.update(|_, cx| {
        editor.update(cx, |editor, cx| editor.set_tool(Tool::Move, cx));
        editor.read(cx).canvas_bounds.get().unwrap().center()
    });
    cx.simulate_mouse_down(position, gpui_kit::MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, _| {
        assert!(window.find("popup-menu").visible());
        assert!(window.try_find("brush-quick-controls").is_none());
    });
}

#[gpui_kit::test]
fn brush_memory_buttons_recall_transfer_and_persist_without_editing_pixels(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, Tool::Brush);
    let (before, id) = cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.apply_preset_named("Chalk", cx));
            editor.tools.brush.size = 26.0;
            editor.tools.brush.opacity = 0.4;
        });
        let id = editor.read(cx).presets.current_id.clone().unwrap();
        let before = editor.read(cx).editor.doc.clone();
        window.click("brush-settings", cx);
        (before, id)
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(editor.read(cx).tools.sample_merged);
        window.click("brush-sample-current", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(!editor.read(cx).tools.sample_merged);
        window.click("brush-sample-visible", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("brush-memory-save", 0usize), cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            editor.tools.brush.size = 70.0;
            editor.tools.brush.opacity = 0.8;
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("brush-memory-recall", 0usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).tools.brush.size, 26.0);
        assert_eq!(editor.read(cx).tools.brush.opacity, 0.4);
        window.click("transfer-brush-smudge", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).paint_kind(), PaintKind::Smudge);
        assert_eq!(editor.read(cx).presets.current_id.as_ref(), Some(&id));
        assert_eq!(editor.read(cx).tools.brush.size, 26.0);
        window.click(("brush-memory-save", 1usize), cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.simulate_keystrokes("]");
    cx.run_until_parked();
    let saved = emulsion_io::brush_library::load().unwrap();
    assert_eq!(
        saved.tool_memory("paint", &id).unwrap().marks[0]
            .unwrap()
            .size,
        26.0
    );
    assert_eq!(
        saved.tool_memory("smudge", &id).unwrap().marks[1]
            .unwrap()
            .size,
        26.0
    );
    assert_eq!(saved.tool_memory("smudge", &id).unwrap().brush.size, 31.0);
    cx.update(|_, cx| {
        assert_eq!(editor.read(cx).editor.doc, before);
        assert!(editor.read(cx).editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn brush_studio_cancel_isolated_and_done_updates_the_shared_library(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Brush);
    let (second, library, before, count) = cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.open_brush_workspace(window, cx));
        let library = editor.read(cx).presets.library.clone().unwrap();
        let second = cx.new(|cx| {
            EditorView::new(
                doc(&["Second"], None),
                None,
                None,
                None,
                "second".into(),
                cx,
            )
        });
        second.update(cx, |second, cx| second.toggle_presets(cx));
        assert_eq!(
            second
                .read(cx)
                .presets
                .library
                .as_ref()
                .unwrap()
                .entity_id(),
            library.entity_id()
        );
        let before = editor.read(cx).editor.doc.clone();
        let count = library.read(cx).catalog.brushes.len();
        (second, library, before, count)
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("brush-library-workspace").visible());
        window.click("new-brush", cx);
    });
    cx.run_until_parked();
    let pad = cx.update(|window, cx| {
        assert!(window.find("brush-studio").visible());
        assert_eq!(library.read(cx).catalog.brushes.len(), count);
        window.find("brush-studio-pad").bounds()
    });
    cx.simulate_mouse_down(pad.center(), gpui_kit::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        pad.center() + gpui_kit::point(gpui_kit::px(30.), gpui_kit::px(10.)),
        Some(gpui_kit::MouseButton::Left),
        Modifiers::none(),
    );
    cx.simulate_mouse_up(pad.center(), gpui_kit::MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    // Workspace actions must never reach the hidden artwork while Studio owns
    // the editing surface. Undo/redo belong to its isolated drawing pad.
    cx.dispatch_action(crate::actions::DeleteNode);
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
        before
    );
    cx.dispatch_action(crate::actions::ClearPixels);
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
        before
    );
    cx.dispatch_action(crate::actions::NewLayer);
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
        before
    );
    cx.dispatch_action(crate::actions::Undo);
    cx.dispatch_action(crate::actions::Redo);
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("undo-pad", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_ne!(window.find("redo-pad").disabled(), Some(true));
        window.click("redo-pad", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_ne!(window.find("undo-pad").disabled(), Some(true));
        assert_eq!(editor.read(cx).editor.doc, before);
        assert!(editor.read(cx).editor.history.is_empty());
    });
    cx.update(|window, cx| window.click("studio-cancel", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("brush-studio").is_none());
        assert_eq!(library.read(cx).catalog.brushes.len(), count);
        assert_eq!(editor.read(cx).editor.doc, before);
        window.click("new-brush", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("studio-done", cx);
        assert!(window.find("studio-saving").visible());
        assert_eq!(library.read(cx).catalog.brushes.len(), count);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("brush-studio").is_none());
        assert_eq!(library.read(cx).catalog.brushes.len(), count + 1);
        let other_library = second.read(cx).presets.library.clone().unwrap();
        assert_eq!(other_library.read(cx).catalog, library.read(cx).catalog);
        assert_eq!(editor.read(cx).editor.doc, before);
        assert!(editor.read(cx).editor.history.is_empty());
        window.click("close-brush-library", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("brush-library-workspace").is_none());
        assert!(editor.read(cx).canvas_focus.is_focused(window));
    });
    let id = cx.update(|window, cx| {
        let id = library.read(cx).catalog.brushes.last().unwrap().id.clone();
        second.update(cx, |second, cx| second.apply_brush_id(&id, cx));
        editor.update(cx, |editor, cx| {
            editor.apply_brush_id(&id, cx);
            editor.open_brush_workspace(window, cx);
        });
        id
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("edit-brush", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("Properties", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("studio-value-size", cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("87");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|window, cx| window.click("studio-done", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            library.read(cx).catalog.brush(&id).unwrap().brush.size,
            87.0
        );
        assert_eq!(editor.read(cx).tools.brush.size, 87.0);
        assert_eq!(
            second.read(cx).tools.brush.size,
            87.0,
            "saved Studio edits must reach another document using this brush"
        );
        assert_eq!(editor.read(cx).editor.doc, before);
    });
    assert_eq!(
        emulsion_io::brush_library::load()
            .unwrap()
            .brush(&id)
            .unwrap()
            .brush
            .size,
        87.0
    );
    cx.update(|window, cx| {
        second.update(cx, |second, cx| {
            second.tools.brush.opacity = 0.35;
            second.set_paint(PaintKind::Eraser, cx);
        });
        // Open immediately, before the workspace's catalog observer rerenders.
        // Studio must draft the canonical library revision in this event turn.
        window.click("edit-brush", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("Properties", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("studio-value-size", cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("99");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|window, cx| window.click("studio-done", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            library.read(cx).catalog.brush(&id).unwrap().brush.size,
            99.0
        );
        second.update(cx, |second, cx| {
            assert_eq!(second.paint_kind(), PaintKind::Eraser);
            second.set_paint(PaintKind::Brush, cx);
            assert_eq!(
                second.tools.brush.size, 99.0,
                "inactive slots refresh the edited definition on activation"
            );
            assert_eq!(
                second.tools.brush.opacity, 0.35,
                "slot refresh preserves quick setting overrides"
            );
        })
    });
}

#[gpui_kit::test]
fn save_current_brush_preserves_dual_sources_baselines_and_metadata(cx: &mut TestAppContext) {
    use emulsion_io::brush_library as store;
    use emulsion_raster::paint::Brush;
    let (editor, cx) = setup(cx, Tool::Brush);
    let mut png = std::io::Cursor::new(Vec::new());
    image::GrayImage::from_pixel(2, 2, image::Luma([255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let (hash, runtime) = store::store_texture_asset(&png.into_inner()).unwrap();
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            editor.toggle_presets(cx);
            let library = editor.presets.library.clone().unwrap();
            let mut draft = library.read(cx).catalog.clone();
            let id = draft
                .add_brush(store::USER_SET, "Dual source test", Brush::default())
                .unwrap();
            let definition = draft.brush_mut(&id).unwrap();
            definition.brush.tip = runtime;
            definition.shape_asset = Some(hash.clone());
            definition.secondary = Some(Brush {
                size: 17.0,
                grain_tex: runtime,
                ..Brush::default()
            });
            definition.secondary_grain_asset = Some(hash.clone());
            definition.author.name = "Fixture artist".into();
            draft.create_reset_point(&id).unwrap();
            let source = draft.brush(&id).unwrap().clone();
            library
                .update(cx, |state, cx| state.commit(draft, cx))
                .unwrap();
            editor.apply_brush_id(&id, cx);
            editor.tools.brush.size = 41.0;
            editor.save_preset(cx);
            let copy_id = editor.presets.current_id.as_deref().unwrap();
            assert_ne!(copy_id, id);
            let copy = library.read(cx).catalog.brush(copy_id).unwrap();
            assert_eq!(copy.brush.size, 41.0);
            assert_eq!(copy.secondary, source.secondary);
            assert_eq!(copy.shape_asset, source.shape_asset);
            assert_eq!(copy.secondary_grain_asset, source.secondary_grain_asset);
            assert_eq!(copy.baseline, source.baseline);
            assert_eq!(copy.reset_point, source.reset_point);
            assert_eq!(copy.reset_point_secondary, source.reset_point_secondary);
            assert_eq!(copy.author, source.author);
            assert!(editor.editor.history.is_empty());
        })
    });
}

#[gpui_kit::test]
fn leaving_canvas_focus_releases_temporary_pan(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Brush);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert!(cx.update(|_, cx| editor.read(cx).space_held));
    cx.update(|window, cx| {
        let focus = editor.read(cx).panel_focus.clone();
        window.focus(&focus, cx);
        window.render_frame(cx);
    });
    cx.run_until_parked();
    assert!(!cx.update(|_, cx| editor.read(cx).space_held));
}

#[gpui_kit::test]
fn brush_settings_and_presets_stay_in_sidebar_without_shrinking_canvas(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, Tool::Brush);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(600.)));
    cx.update(|_, cx| crate::app_state::update_settings(cx, |s| s.advanced_tools = true));
    cx.run_until_parked();
    let before = cx.update(|window, cx| {
        assert!(window.try_find("advanced").is_none());
        assert!(window.try_find("brush-settings-panel").is_none());
        let bar = window.find("editor-tool-options").bounds();
        assert!(f32::from(bar.size.height) < 100.);
        let before = (
            window.find("editor-canvas-column").bounds(),
            editor.read(cx).editor.doc.clone(),
        );
        window.click("brush-settings", cx);
        before
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("brush-quick-controls").visible());
        window.within("popup-menu").click(2usize, cx); // All brush settings.
    });
    cx.run_until_parked();
    for tab in [
        "brush-settings-tip",
        "brush-settings-texture",
        "brush-settings-dynamics",
        "brush-settings-drawing",
    ] {
        cx.update(|window, cx| window.click(tab, cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("brush-settings-panel").visible());
            assert_eq!(window.find("editor-canvas-column").bounds(), before.0);
            assert_eq!(editor.read(cx).editor.doc, before.1);
        });
    }
    cx.update(|window, cx| window.click("brush-settings-presets", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("brush-settings-panel").visible());
        let preset = emulsion_raster::library::library()
            .into_iter()
            .filter(|brush| brush.category == emulsion_raster::library::CATEGORIES[0])
            .nth(1)
            .expect("second builtin brush");
        let id = gpui_kit::SharedString::from(format!(
            "brush-brush:builtin:{}:{}",
            preset.category, preset.name
        ));
        let sidebar = ("sidebar-content", editor.read(cx).sidebar_tab as usize);
        let delta =
            window.find(sidebar).bounds().center().y - window.find(id.clone()).bounds().center().y;
        // Library names and set counts vary; scroll the requested brush into view.
        window.scroll(
            sidebar,
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), delta)),
            cx,
        );
        assert!(window.find("brush-presets-panel").visible());
        assert!(window.try_find("preset-close").is_none());
        window.click(id, cx);
    });
    cx.run_until_parked();
    let selected_brush = cx.update(|window, cx| {
        assert!(window.find("brush-settings-panel").visible());
        assert!(editor.read(cx).presets.current.is_some());
        assert_eq!(window.find("editor-canvas-column").bounds(), before.0);
        let brush = editor.read(cx).brush();
        window.scroll(
            ("sidebar-content", editor.read(cx).sidebar_tab as usize),
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(10000.))),
            cx,
        );
        window.click("brush-settings-tip", cx);
        brush
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("brush-presets-panel").is_none());
        assert_eq!(editor.read(cx).brush(), selected_brush);
        assert_eq!(editor.read(cx).editor.doc, before.1);
        window.click("brush-settings-close", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-history-content").visible());
        window.click("presets", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let library = window.find("brush-presets-panel").bounds();
        let dock = window.find("node-panel").bounds();
        assert!(library.origin.x >= dock.origin.x);
        assert!(library.right() <= dock.right());
        assert_eq!(window.find("editor-canvas-column").bounds(), before.0);
        window.click("presets", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-history-content").visible());
        assert_eq!(editor.read(cx).editor.doc, before.1);
        assert!(editor.read(cx).editor.history.is_empty());
    });
}
