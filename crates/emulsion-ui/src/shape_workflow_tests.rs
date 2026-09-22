//! Shape creation and editing through canvas pointer gestures.
use super::*;
use crate::editor::shapes::{ShapeMode, ShapeOperation};
use crate::editor::{EditorView, PaintKind, Tool};
use emulsion_core::NodeKind;
use emulsion_raster::vector_geometry;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, MouseButton};

fn control(cx: &mut VisualTestContext, id: &'static str) {
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        window.click(id, cx);
    });
    cx.run_until_parked();
}

fn field(cx: &mut VisualTestContext, id: &'static str, value: &str) {
    control(cx, id);
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input(value);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

fn choice(cx: &mut VisualTestContext, id: &'static str, index: usize) {
    control(cx, id);
    for _ in 0..=index {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

fn selected_style(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> emulsion_raster::vector::PathStyle {
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let NodeKind::Path { style, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!("vector shape")
        };
        *style
    })
}

#[gpui_kit::test]
fn native_shape_paint_and_stroke_presets_apply_and_undo_each_edit(cx: &mut TestAppContext) {
    use emulsion_raster::vector::{PathPaint, PatternKind, StrokeAlignment, StrokeCap, StrokeJoin};
    let (editor, cx) = setup(cx);
    drag(&editor, cx, (40., 40.), (140., 120.));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(2600.)));
    control(cx, "sidebar-properties");
    let initial_steps = cx.update(|_, cx| editor.read(cx).editor.history.len());
    choice(cx, "shape-fill-type", 2);
    assert!(matches!(
        selected_style(&editor, cx).fill_paint,
        PathPaint::LinearGradient { .. }
    ));
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.history.len()),
        initial_steps + 1
    );
    field(cx, "shape-fill-angle", "45");
    assert!(matches!(
        selected_style(&editor, cx).fill_paint,
        PathPaint::LinearGradient { angle: 45., .. }
    ));
    choice(cx, "shape-fill-type", 4);
    choice(cx, "shape-fill-pattern", 2);
    field(cx, "shape-fill-size", "23");
    assert!(matches!(
        selected_style(&editor, cx).fill_paint,
        PathPaint::Pattern {
            kind: PatternKind::Dots,
            size: 23.,
            ..
        }
    ));
    choice(cx, "shape-stroke-type", 2);
    field(cx, "shape-stroke-angle", "30");
    field(cx, "shape-stroke-width", "9");
    choice(cx, "shape-stroke-alignment", 2);
    choice(cx, "shape-cap", 2);
    choice(cx, "shape-join", 2);
    choice(cx, "shape-stroke-preset", 1);
    let saved = selected_style(&editor, cx);
    assert_eq!(saved.alignment, StrokeAlignment::Outside);
    assert_eq!(saved.cap, StrokeCap::Square);
    assert_eq!(saved.join, StrokeJoin::Bevel);
    assert_eq!(&saved.dash[..2], &[12., 6.]);
    assert_eq!(saved.dash_count, 2);
    control(cx, "shape-save-stroke");
    cx.update(|_, cx| {
        assert_eq!(
            crate::app_state::settings(cx)
                .shape_stroke_presets
                .last()
                .unwrap()
                .style,
            saved
        )
    });
    choice(cx, "shape-fill-type", 1);
    choice(cx, "shape-stroke-preset", 2);
    field(cx, "shape-stroke-width", "4");
    choice(cx, "shape-stroke-type", 1);
    let before = selected_style(&editor, cx);
    let before_steps = cx.update(|_, cx| editor.read(cx).editor.history.len());
    choice(cx, "shape-stroke-preset", 4);
    let restored = selected_style(&editor, cx);
    assert_eq!(
        restored.fill_paint,
        PathPaint::Solid,
        "stroke preset leaves current fill alone"
    );
    assert_eq!(restored.stroke_paint, saved.stroke_paint);
    assert_eq!(restored.width, saved.width);
    assert_eq!(restored.dash, saved.dash);
    assert_eq!(restored.cap, saved.cap);
    assert_eq!(restored.join, saved.join);
    assert_eq!(restored.alignment, saved.alignment);
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.history.len()),
        before_steps + 1
    );
    cx.update(|window, cx| {
        let focus = editor.read(cx).canvas_focus.clone();
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(selected_style(&editor, cx), before);
}

#[gpui_kit::test]
fn pen_subtracts_closed_triangle_from_selected_shape_in_one_undo_step(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    drag(&editor, cx, (40., 40.), (140., 120.));
    let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.update(|_, cx| editor.update(cx, |e, cx| e.set_tool(Tool::Pen, cx)));
    cx.run_until_parked();
    choice(cx, "pen-path-operation", 3);
    for point in [(70., 60.), (110., 60.), (90., 100.), (70., 60.)] {
        let position = cx.update(|_, cx| editor.read(cx).doc_to_window(point).unwrap());
        cx.simulate_click(position, Modifiers::none());
        cx.run_until_parked();
    }
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), original.nodes.len());
        assert_eq!(e.editor.history.len(), 2);
        let NodeKind::Path { cache, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!("editable shape")
        };
        assert_eq!(cache.get(90, 75)[3], 0, "triangle was subtracted");
        assert_eq!(cache.get(50, 75)[3], 65535, "outside triangle stays filled");
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
        original
    );
}

fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_tool(Tool::Shape, cx);
            e.set_fg([230, 40, 20, 255], cx);
        })
    });
    cx.run_until_parked();
    (editor, cx)
}

fn drag(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    start: (f64, f64),
    end: (f64, f64),
) {
    let start = cx.update(|_, cx| editor.read(cx).doc_to_window(start).unwrap());
    let end = cx.update(|_, cx| editor.read(cx).doc_to_window(end).unwrap());
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

#[gpui_kit::test]
fn fixed_shape_dimensions_and_align_edges_control_the_actual_created_path(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(2200.)));
    control(cx, "sidebar-properties");
    field(cx, "shape-width", "63");
    field(cx, "shape-height", "37");
    control(cx, "shape-fixed-size");
    control(cx, "shape-align-edges");
    drag(&editor, cx, (30.2, 40.2), (160., 140.));
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let NodeKind::Path { path, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!("editable shape expected")
        };
        assert_eq!(vector_geometry::bounds(path), Some((30., 40., 63., 37.)));
        assert_eq!(e.editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn shape_properties_resize_linked_dimensions_and_edit_stroke_width(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    drag(&editor, cx, (40., 40.), (140., 90.));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(2200.)));
    control(cx, "sidebar-properties");
    control(cx, "shape-link-size");
    field(cx, "shape-width", "120");
    control(cx, "shape-stroke-type");
    cx.simulate_keystrokes("down down enter");
    cx.run_until_parked();
    field(cx, "shape-stroke-width", "11");
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let NodeKind::Path { path, style, .. } =
            &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        let b = vector_geometry::bounds(path).unwrap();
        assert!(
            (b.2 - 120.).abs() < 0.01 && (b.3 - 60.).abs() < 0.01,
            "linked resize: {b:?}"
        );
        assert_eq!(style.width, 11.);
    });
}

#[gpui_kit::test]
fn added_path_components_align_through_properties_without_extra_layers(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    drag(&editor, cx, (30., 30.), (80., 70.));
    cx.update(|_, cx| editor.update(cx, |e, _| e.shape_ui.operation = ShapeOperation::Component));
    drag(&editor, cx, (120., 100.), (180., 140.));
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(2200.)));
    control(cx, "sidebar-properties");
    control(cx, "shape-align-top");
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), before.nodes.len());
        let NodeKind::Path { path, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(path.subpaths.len(), 2);
        for subpath in &path.subpaths {
            let bounds = vector_geometry::bounds(&emulsion_raster::vector::Path {
                subpaths: vec![subpath.clone()],
            })
            .unwrap();
            assert!((bounds.1 - 30.).abs() < 0.01, "component top: {bounds:?}");
        }
    });
    // Restore canvas keyboard focus without adding another shape.
    cx.update(|window, cx| {
        let focus = editor.read(cx).canvas_focus.clone();
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
        before
    );
}

#[gpui_kit::test]
fn rectangle_anchor_can_be_edited_with_pen_and_undone_without_rasterizing(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    drag(&editor, cx, (40., 40.), (140., 120.));
    let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.update(|_, cx| editor.update(cx, |e, cx| e.set_tool(Tool::Pen, cx)));
    cx.run_until_parked();
    drag(&editor, cx, (40., 40.), (55., 55.));
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), original.nodes.len());
        let NodeKind::Path { path, .. } = &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
        else {
            panic!("shape remains editable")
        };
        assert_eq!(path.anchor_count(), 4);
        assert!(
            path.subpaths[0]
                .anchors
                .iter()
                .any(|a| (a.p.0 - 55.).abs() < 0.01 && (a.p.1 - 55.).abs() < 0.01)
        );
        assert_eq!(
            e.editor.history.len(),
            2,
            "one shape gesture and one anchor gesture"
        );
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
        original
    );
}

#[gpui_kit::test]
fn shape_boolean_gestures_edit_one_layer_and_undo_restores_original(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    drag(&editor, cx, (30., 30.), (110., 110.));
    let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    for (operation, want) in [
        (ShapeOperation::Add, [true, true, true]),
        (ShapeOperation::Subtract, [true, false, false]),
        (ShapeOperation::Intersect, [false, true, false]),
        (ShapeOperation::Exclude, [true, false, true]),
    ] {
        cx.update(|_, cx| editor.update(cx, |e, _| e.shape_ui.operation = operation));
        drag(&editor, cx, (70., 30.), (150., 110.));
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.editor.doc.nodes.len(), original.nodes.len());
            let NodeKind::Path { cache, .. } =
                &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
            else {
                panic!("boolean operation must preserve an editable path")
            };
            for (x, wanted) in [50, 90, 130].into_iter().zip(want) {
                assert_eq!(
                    cache.get(x, 70)[3] > 32767,
                    wanted,
                    "{operation:?} at x={x}"
                );
            }
        });
        cx.simulate_keystrokes("ctrl-z");
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
            original
        );
    }
}

#[gpui_kit::test]
fn path_and_pixels_modes_keep_their_distinct_editing_semantics(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    for mode in [ShapeMode::Path, ShapeMode::Pixels] {
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.set_tool(Tool::Shape, cx);
                e.shape_ui.mode = mode;
            })
        });
        drag(&editor, cx, (40., 40.), (140., 120.));
        cx.update(|_, cx| {
            let e = editor.read(cx);
            let node = e.editor.doc.node(e.selected.unwrap()).unwrap();
            match (&node.kind, mode) {
                (NodeKind::Path { path, style, cache }, ShapeMode::Path) => {
                    assert_eq!(path.anchor_count(), 4);
                    assert_eq!(style.fill, None);
                    assert_eq!(style.stroke, None);
                    assert_eq!(cache.get(90, 80)[3], 0);
                    assert_eq!(e.tool, Tool::Pen);
                }
                (NodeKind::Raster { raster, .. }, ShapeMode::Pixels) => {
                    assert_eq!(raster.get(90, 80)[3], 65535);
                }
                _ => panic!("unexpected shape mode output"),
            }
        });
        cx.simulate_keystrokes("ctrl-z");
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
            original
        );
    }
}

#[gpui_kit::test]
fn bucket_recolors_vector_fill_without_replacing_geometry_or_adding_layer(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    drag(&editor, cx, (40., 40.), (140., 120.));
    let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_paint(PaintKind::Bucket, cx);
            e.set_fg([20, 180, 70, 255], cx);
        })
    });
    cx.run_until_parked();
    let inside = cx.update(|_, cx| editor.read(cx).doc_to_window((90., 80.)).unwrap());
    cx.simulate_click(inside, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), original.nodes.len());
        let id = e.selected.unwrap();
        let NodeKind::Path { path, style, .. } = &e.editor.doc.node(id).unwrap().kind else {
            panic!("vector shape remains editable")
        };
        let NodeKind::Path { path: before, .. } = &original.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(path, before);
        assert_eq!(style.fill, Some([20, 180, 70, 255]));
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| editor.read(cx).editor.doc.clone()),
        original
    );
}
