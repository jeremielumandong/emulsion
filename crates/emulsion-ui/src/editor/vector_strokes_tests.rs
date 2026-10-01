//! Vector drawing through the real editor: the Brush and Eraser on stroke
//! layers, shape tools on vector and pixel layers, the contour editor,
//! stroke tools, pencil retouch, one Undo step per gesture and locks.
use super::*;
use core::prelude::v1::test;
use std::time::Instant;

fn view(cx: &mut TestAppContext) -> Entity<EditorView> {
    cx.update(|cx| {
        gpui_kit::init(cx);
        theme::install(cx);
        cx.set_global(crate::app_state::AppSettings(Default::default()));
        cx.new(|cx| EditorView::new(Document::new(100, 80), None, None, None, "vec".into(), cx))
    })
}

fn pen(pressure: f32) -> Option<PenSample> {
    Some(PenSample {
        pressure,
        tilt: (0., 0.),
        down: true,
        at: Instant::now(),
    })
}

fn line(points: &[(f64, f64)]) -> Stroke {
    Stroke {
        points: points
            .iter()
            .map(|&(x, y)| StrokePoint::new(x, y))
            .collect(),
        ..Stroke::new([0, 0, 0, 255], 4.)
    }
}

/// A vector layer holding `strokes`, selected, with the history cleared.
fn layer(v: &mut EditorView, strokes: Vec<Stroke>, cx: &mut Context<EditorView>) -> NodeId {
    let id = v.new_vector_layer(cx).unwrap();
    if !strokes.is_empty() {
        let set = StrokeSet {
            strokes,
            fills: Vec::new(),
        };
        v.editor
            .execute(Command::SetStrokes {
                id,
                strokes: Arc::new(set),
            })
            .unwrap();
    }
    v.editor.history = Default::default();
    id
}

fn strokes(v: &EditorView, id: NodeId) -> Arc<StrokeSet> {
    match &v.editor.doc.node(id).unwrap().kind {
        NodeKind::Strokes { strokes, .. } => strokes.clone(),
        _ => panic!("not a vector layer"),
    }
}

fn release(v: &mut EditorView, cx: &mut Context<EditorView>) {
    let Some(Drag::Tool(drag)) = v.drag.take() else {
        panic!("no tool drag")
    };
    v.tool_up(drag, cx);
}

/// Press, drag through `path` and release with the Vector tool.
fn gesture(v: &mut EditorView, path: &[(f64, f64)], cx: &mut Context<EditorView>) {
    v.vector_down(path[0], Modifiers::default(), 1, cx);
    for &p in &path[1..] {
        v.vector_move(p, None, cx);
    }
    release(v, cx);
}

#[gpui_kit::test]
fn the_brush_draws_a_pressure_line_on_a_vector_layer_as_one_step(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let id = layer(v, Vec::new(), cx);
        v.set_paint(PaintKind::Brush, cx);
        v.tools.quick_shape = false;
        v.tools.brush.size = 10.;
        v.tools.brush.size_pressure = 1.;
        v.tools.fg = [200, 10, 20, 255];
        assert!(v.vector_brush_down((10., 40.), pen(0.2), cx));
        for (i, x) in [20., 30., 40., 50., 60., 70., 80.].into_iter().enumerate() {
            v.vector_move((x, 40.), pen(0.3 + i as f32 * 0.1), cx);
        }
        // The preview lands at the frame and shows while drawing.
        v.flush_live_stroke(cx);
        assert_eq!(strokes(v, id).strokes.len(), 1);
        release(v, cx);
        let set = strokes(v, id);
        assert_eq!(set.strokes.len(), 1);
        let s = &set.strokes[0];
        assert_eq!((s.color, s.width), ([200, 10, 20, 255], 10.));
        let (first, last) = (s.points[0], *s.points.last().unwrap());
        assert!(first.width < 0.3 && last.width > 0.85, "{first:?} {last:?}");
        assert_eq!(v.editor.history.len(), 1);
        let NodeKind::Strokes { cache, .. } = &v.editor.doc.node(id).unwrap().kind else {
            unreachable!()
        };
        assert!(cache.pixels().get(75, 40)[3] > 60000);
        v.undo(cx);
        assert!(strokes(v, id).strokes.is_empty());
    });
}

#[test]
fn opacity_dynamics_follow_pressure_tilt_speed_and_fade() {
    let sample = |x: f32, pressure: f32, tilt: f32, speed: f32| Sample {
        x,
        y: 0.,
        pressure,
        tilt: (tilt, 0.),
        speed,
    };
    let brush = Brush::default();
    let off = OpacityDynamics {
        pressure: 0.,
        tilt: 0.,
        speed: 0.,
        fade: 0.,
    };
    let samples = [
        sample(0., 0.2, 0., 0.),
        sample(10., 1., 60., 0.),
        sample(20., 1., 0., 6.),
        sample(130., 1., 0., 0.),
    ];
    let plain = stroke_points(&samples, &brush, &off, true);
    assert!(plain.iter().all(|p| p.opacity == 1. && p.width == 1.));
    let on = OpacityDynamics {
        pressure: 1.,
        tilt: 1.,
        speed: 1.,
        fade: 100.,
    };
    let p = stroke_points(&samples, &brush, &on, true);
    assert!((p[0].opacity - 0.2).abs() < 1e-6, "pressure");
    assert!(p[1].opacity < 0.4, "tilt");
    assert!(p[2].opacity < 0.1, "speed");
    assert_eq!(p[3].opacity, 0., "faded out after 100 px");
}

#[gpui_kit::test]
fn the_eraser_splits_vector_lines_in_one_step(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let id = layer(v, vec![line(&[(0., 20.), (100., 20.)])], cx);
        v.set_paint(PaintKind::Eraser, cx);
        v.tools.brush.size = 10.;
        assert!(v.vector_brush_down((45., 10.), None, cx));
        v.vector_move((45., 30.), None, cx);
        v.vector_move((48., 30.), None, cx);
        release(v, cx);
        let set = strokes(v, id);
        assert_eq!(set.strokes.len(), 2);
        assert!(set.strokes[0].points.last().unwrap().x <= 40. + 1e-9);
        assert!(set.strokes[1].points[0].x >= 50. - 1e-9);
        assert_eq!(v.editor.history.len(), 1);
        // Pixel-only kinds refuse on a vector layer and change nothing.
        v.set_paint(PaintKind::Smudge, cx);
        assert!(v.vector_brush_down((10., 20.), None, cx));
        assert!(v.drag.is_none());
        v.undo(cx);
        assert_eq!(strokes(v, id).strokes.len(), 1);
    });
}

#[gpui_kit::test]
fn shape_tools_draw_editable_strokes_on_vector_layers(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let id = layer(v, Vec::new(), cx);
        v.set_vector_mode(VectorMode::Line, cx);
        assert_eq!(v.tool, Tool::Vector);
        v.vector.width = 6.;
        gesture(v, &[(10., 10.), (50., 13.)], cx);
        v.set_vector_mode(VectorMode::Rectangle, cx);
        v.drag_shift = true;
        gesture(v, &[(10., 30.), (40., 50.)], cx);
        v.drag_shift = false;
        v.set_vector_mode(VectorMode::Ellipse, cx);
        gesture(v, &[(60., 30.), (90., 70.)], cx);
        v.set_vector_mode(VectorMode::Polyline, cx);
        for p in [(5., 70.), (20., 75.), (35., 70.)] {
            v.vector_down(p, Modifiers::default(), 1, cx);
        }
        v.tool_commit(cx);
        let set = strokes(v, id);
        assert_eq!(set.strokes.len(), 4);
        let [l, r, e, p] = [0, 1, 2, 3].map(|i| &set.strokes[i]);
        assert!(!l.closed && l.points.len() == 2 && l.width == 6.);
        assert!(r.closed && r.points.len() == 4);
        let side = |a: &StrokePoint, b: &StrokePoint| (a.x - b.x).hypot(a.y - b.y);
        assert!((side(&r.points[0], &r.points[1]) - side(&r.points[1], &r.points[2])).abs() < 1e-3);
        assert!(e.closed && e.points.len() >= 16);
        assert!(!p.closed && p.points.len() == 3);
        assert_eq!(v.editor.history.len(), 4, "one step per shape");
        // Shift keeps lines to 45°.
        v.set_vector_mode(VectorMode::Line, cx);
        v.drag_shift = true;
        gesture(v, &[(10., 10.), (40., 38.)], cx);
        let snapped = strokes(v, id).strokes[4].points.clone();
        assert!(((snapped[1].x - 10.) - (snapped[1].y - 10.)).abs() < 1e-3);
    });
}

#[gpui_kit::test]
fn shape_tools_paint_pixels_on_bitmap_layers(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let id = v.new_empty_layer(cx).unwrap();
        v.editor.history = Default::default();
        v.set_vector_mode(VectorMode::Line, cx);
        v.vector.width = 6.;
        gesture(v, &[(10., 40.), (90., 40.)], cx);
        let NodeKind::Raster { raster, .. } = &v.editor.doc.node(id).unwrap().kind else {
            panic!("still a pixel layer");
        };
        assert!(raster.get(50, 40)[3] > 30000);
        assert_eq!(raster.get(50, 60)[3], 0);
        assert_eq!(v.editor.history.len(), 1);
        assert!(
            !v.editor
                .doc
                .nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Strokes { .. }))
        );
    });
}

#[gpui_kit::test]
fn the_contour_editor_selects_moves_scales_reshapes_and_deletes(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let id = layer(
            v,
            vec![
                line(&[(10., 10.), (30., 10.)]),
                line(&[(10., 50.), (30., 50.)]),
            ],
            cx,
        );
        v.set_vector_mode(VectorMode::Contour, cx);
        // Click a line and drag it: only it moves, as one step.
        gesture(v, &[(20., 10.), (25., 15.)], cx);
        assert_eq!(v.vector.selection, vec![0]);
        let set = strokes(v, id);
        assert_eq!(
            (set.strokes[0].points[0].x, set.strokes[0].points[0].y),
            (15., 15.)
        );
        assert_eq!(set.strokes[1].points[0].x, 10.);
        assert_eq!(v.editor.history.len(), 1);
        // A marquee selects both; the bottom-right corner scales them.
        gesture(v, &[(70., 75.), (0., 0.)], cx);
        assert_eq!(v.vector.selection, vec![0, 1]);
        assert_eq!(v.editor.history.len(), 1, "selecting is not an edit");
        gesture(v, &[(35., 50.), (60., 90.)], cx);
        let set = strokes(v, id);
        let scaled = &set.strokes[1].points[1];
        assert!(scaled.x > 50. && set.strokes[1].width > 4., "{scaled:?}");
        assert_eq!(v.editor.history.len(), 2);
        // Drag one point of a selected line.
        let p = set.strokes[0].points[0];
        gesture(v, &[(p.x, p.y), (p.x - 4., p.y)], cx);
        let moved = strokes(v, id).strokes[0].points[0];
        assert!((moved.x - (p.x - 4.)).abs() < 1e-9 && moved.y == p.y);
        assert_eq!(
            strokes(v, id).strokes[0].points[1],
            set.strokes[0].points[1]
        );
        // Delete removes the selection.
        v.vector.selection = vec![1];
        assert!(v.vector_delete(cx));
        assert_eq!(strokes(v, id).strokes.len(), 1);
        assert_eq!(v.editor.history.len(), 4);
        v.undo(cx);
        assert_eq!(strokes(v, id).strokes.len(), 2);
    });
}

#[gpui_kit::test]
fn stroke_tools_smooth_optimize_and_convert_lines(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let zigzag: Vec<(f64, f64)> = (0..=10)
            .map(|i| (f64::from(i) * 8., if i % 2 == 0 { 20. } else { 26. }))
            .collect();
        let straight: Vec<(f64, f64)> = (0..=20).map(|i| (f64::from(i) * 4., 60.)).collect();
        let id = layer(v, vec![line(&zigzag), line(&straight)], cx);
        v.vector.smooth = 1.;
        v.smooth_strokes(cx);
        let set = strokes(v, id);
        assert!((set.strokes[0].points[1].y - 26.).abs() > 1., "smoothed");
        v.vector.optimize = 0.5;
        v.optimize_strokes(cx);
        assert_eq!(strokes(v, id).strokes[1].points.len(), 2);
        // Only the contour selection converts while it has one.
        v.set_vector_mode(VectorMode::Contour, cx);
        v.vector.layer = Some(id);
        v.vector.selection = vec![1];
        v.outline_selected_strokes(cx);
        let set = strokes(v, id);
        assert_eq!((set.strokes.len(), set.fills.len()), (1, 1));
        assert_eq!(v.editor.history.len(), 3);
    });
}

#[gpui_kit::test]
fn pencil_retouch_changes_lines_under_the_brush_in_one_step(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let points: Vec<(f64, f64)> = (0..=10).map(|i| (f64::from(i) * 10., 40.)).collect();
        let id = layer(v, vec![line(&points)], cx);
        v.set_vector_mode(VectorMode::Retouch, cx);
        v.vector.retouch = Retouch::Thicker;
        v.vector.retouch_size = 30.;
        v.vector.retouch_amount = 1.;
        gesture(v, &[(30., 40.), (50., 40.), (60., 40.)], cx);
        let s = &strokes(v, id).strokes[0];
        assert!(s.points[4].width > 1.5, "{:?}", s.points[4]);
        assert_eq!(s.points[10].width, 1.);
        assert_eq!(v.editor.history.len(), 1);
        v.vector.retouch = Retouch::Fainter;
        gesture(v, &[(0., 40.), (5., 40.)], cx);
        assert!(strokes(v, id).strokes[0].points[0].opacity < 1.);
        assert_eq!(v.editor.history.len(), 2);
    });
}

#[gpui_kit::test]
fn locked_layers_and_panels_refuse_vector_edits(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let id = layer(v, vec![line(&[(0., 20.), (100., 20.)])], cx);
        v.editor
            .execute(Command::SetLocked { id, locked: true })
            .unwrap();
        v.editor.history = Default::default();
        v.set_paint(PaintKind::Brush, cx);
        assert!(v.vector_brush_down((10., 10.), None, cx));
        assert!(v.drag.is_none());
        v.set_vector_mode(VectorMode::Line, cx);
        gesture(v, &[(10., 10.), (50., 10.)], cx);
        v.smooth_strokes(cx);
        assert!(v.status.as_ref().is_some_and(|(_, error)| *error));
        assert_eq!(strokes(v, id).strokes.len(), 1);
        assert!(v.editor.history.is_empty());
        // A pixel lock refuses too.
        v.editor
            .execute(Command::SetLocked { id, locked: false })
            .unwrap();
        let mut locks = v.editor.doc.node(id).unwrap().locks;
        locks.pixels = true;
        v.editor
            .execute(Command::SetLayerLocks { id, locks })
            .unwrap();
        v.editor.history = Default::default();
        v.set_vector_mode(VectorMode::Retouch, cx);
        v.vector_down((50., 20.), Modifiers::default(), 1, cx);
        assert!(v.drag.is_none() && v.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn locked_storyboard_panels_refuse_vector_drawing(cx: &mut TestAppContext) {
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    let v = view(cx);
    v.update(cx, |v, cx| {
        v.editor =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        let id = layer(v, vec![line(&[(0., 20.), (60., 20.)])], cx);
        let page = v.editor.active_page();
        v.editor
            .edit_storyboard(|b| {
                b.panels.get_mut(&page).unwrap().locked = true;
                Ok(())
            })
            .unwrap();
        assert!(v.editor.is_read_only());
        v.set_paint(PaintKind::Brush, cx);
        assert!(v.vector_brush_down((10., 10.), None, cx));
        assert!(v.drag.is_none());
        v.set_vector_mode(VectorMode::Rectangle, cx);
        gesture(v, &[(5., 5.), (30., 30.)], cx);
        assert_eq!(strokes(v, id).strokes.len(), 1);
        assert!(v.status.as_ref().is_some_and(|(_, error)| *error));
    });
}

#[gpui_kit::test]
fn new_vector_layers_join_above_the_selection(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let pixels = v.new_empty_layer(cx).unwrap();
        let id = v.new_vector_layer(cx).unwrap();
        assert_eq!(v.selected, Some(id));
        assert_eq!(v.editor.doc.node(id).unwrap().name, "Vector 1");
        let top = v.editor.doc.children(None);
        assert_eq!(
            top.iter().position(|n| *n == id),
            Some(top.iter().position(|n| *n == pixels).unwrap() + 1)
        );
        assert!(v.vector_layer().is_some());
    });
}

#[gpui_kit::test]
fn vector_strokes_started_near_the_ruler_follow_its_edge(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let id = layer(v, Vec::new(), cx);
        v.set_paint(PaintKind::Brush, cx);
        v.tools.quick_shape = false;
        v.tools.brush.size = 4.;
        // The ruler runs across the middle at y = 40; start just below it
        // and wander off downwards.
        v.toggle_ruler(cx);
        assert!(v.vector_brush_down((20., 43.), pen(1.), cx));
        // tool_move snaps each point through the assist before the drag.
        for p in [(40., 50.), (60., 58.), (80., 66.)] {
            let p = v.assist_point(p);
            v.vector_move(p, pen(1.), cx);
        }
        release(v, cx);
        let set = strokes(v, id);
        let ys: Vec<_> = set.strokes[0].points.iter().map(|p| p.y).collect();
        assert!(ys.iter().all(|y| (y - 40.).abs() < 1.5), "{ys:?}");
        // The next stroke far from the ruler is free.
        assert!(v.vector_brush_down((10., 5.), pen(1.), cx));
        let p = v.assist_point((60., 20.));
        v.vector_move(p, pen(1.), cx);
        release(v, cx);
        let last = strokes(v, id).strokes[1].points.last().copied().unwrap();
        assert!(last.y > 15., "{last:?}");
    });
}
