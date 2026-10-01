//! Storyboard drawing tools through real canvas input: the gap-closing
//! bucket on pixel and vector layers, the cutter, live selection
//! distortion, the ruler and curvilinear Drawing Assist.
use super::*;
use crate::editor::{EditorView, PaintKind, SelectShape, Tool};
use emulsion_core::NodeKind;
use emulsion_core::distort::DistortKind;
use emulsion_core::drawing_guides::GuideKind;
use emulsion_raster::paint::Brush;
use emulsion_raster::strokes::{Stroke, StrokePoint, StrokeSet};

const RED: [u16; 4] = [65535, 0, 0, 65535];
const WHITE: [u16; 4] = [65535; 4];

fn editor(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    cx.update(|_, cx| ws.read(cx).editor.clone().unwrap())
}

fn node(e: &Entity<EditorView>, cx: &mut VisualTestContext, id: u64) -> NodeKind {
    cx.update(|_, cx| e.read(cx).editor.doc.node(id).unwrap().kind.clone())
}

fn selected(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> u64 {
    cx.update(|_, cx| e.read(cx).selected.unwrap())
}

fn pixel(e: &Entity<EditorView>, cx: &mut VisualTestContext, id: u64, x: u32, y: u32) -> [u16; 4] {
    match node(e, cx, id) {
        NodeKind::Raster { raster, .. } => raster.get(x, y),
        _ => panic!("a pixel layer"),
    }
}

fn red(p: [u16; 4]) -> bool {
    p[0] > 60000 && p[1] < 5000 && p[3] > 60000
}

fn steps(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    cx.update(|_, cx| e.read(cx).editor.history.len())
}

/// Press, move through `pts` and release, in document pixels.
fn gesture(e: &Entity<EditorView>, cx: &mut VisualTestContext, pts: &[(f64, f64)]) {
    let pts: Vec<_> = cx.update(|_, cx| {
        let e = e.read(cx);
        pts.iter().map(|p| e.doc_to_window(*p).unwrap()).collect()
    });
    let none = gpui_kit::Modifiers::none();
    cx.simulate_mouse_down(pts[0], gpui_kit::MouseButton::Left, none);
    for pair in pts.windows(2) {
        for i in 1..=8 {
            cx.simulate_mouse_move(
                pair[0] + (pair[1] - pair[0]) * (i as f32 / 8.0),
                Some(gpui_kit::MouseButton::Left),
                none,
            );
        }
    }
    cx.simulate_mouse_up(*pts.last().unwrap(), gpui_kit::MouseButton::Left, none);
    cx.run_until_parked();
}

fn click(e: &Entity<EditorView>, cx: &mut VisualTestContext, p: (f64, f64)) {
    let p = cx.update(|_, cx| e.read(cx).doc_to_window(p).unwrap());
    cx.simulate_click(p, gpui_kit::Modifiers::none());
    cx.run_until_parked();
}

fn paint(e: &Entity<EditorView>, cx: &mut VisualTestContext, kind: PaintKind, size: f32) {
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_paint(kind, cx);
            e.tools.brush = Brush {
                size,
                hardness: 1.0,
                ..Brush::default()
            };
            e.tools.quick_shape = false;
            e.set_fg([255, 0, 0, 255], cx);
        })
    });
    cx.run_until_parked();
}

/// White with a black square outline from 40 to 120, 2 px thick, with a
/// 4 px opening in its top side.
fn line_art() -> Raster {
    Raster::from_fn(256, 192, [0; 4], |x, y| {
        let inside = (40..120).contains(&x) && (40..120).contains(&y);
        let border = !(42..118).contains(&x) || !(42..118).contains(&y);
        let opening = y < 42 && (78..82).contains(&x);
        if inside && border && !opening {
            [0, 0, 0, 65535]
        } else {
            WHITE
        }
    })
}

#[gpui_kit::test]
fn bucket_closes_gaps_on_pixel_layers_and_stays_vector_on_stroke_layers(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Ink"], Some(line_art())));
    let e = editor(&ws, cx);
    let id = selected(&e, cx);
    paint(&e, cx, PaintKind::Bucket, 32.0);
    click(&e, cx, (80., 80.));
    assert_eq!(
        pixel(&e, cx, id, 10, 10),
        RED,
        "without gap closing the fill leaks"
    );
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.undo(cx);
            e.tools.drawing.bucket.gap = 6;
        })
    });
    let before = steps(&e, cx);
    click(&e, cx, (80., 80.));
    assert_eq!(steps(&e, cx), before + 1);
    assert_eq!(pixel(&e, cx, id, 80, 80), RED);
    assert_eq!(
        pixel(&e, cx, id, 43, 43),
        RED,
        "the fill still meets the lines"
    );
    assert_eq!(pixel(&e, cx, id, 10, 10), WHITE, "the 4 px opening holds");

    // On a vector stroke layer the fill is a vector fill under the strokes.
    let pt = StrokePoint::new;
    let set = StrokeSet {
        strokes: vec![Stroke {
            points: vec![
                pt(150., 40.),
                pt(230., 40.),
                pt(230., 120.),
                pt(150., 120.),
                pt(150., 46.),
            ],
            ..Stroke::new([0, 0, 255, 255], 3.)
        }],
        fills: Vec::new(),
    };
    let vid = cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let id = e
                .execute(
                    Command::AddNode {
                        node: Box::new(Node::strokes(0, "Pencil", Arc::new(set), 256, 192)),
                        slot: Slot::TOP,
                    },
                    cx,
                )
                .unwrap();
            e.set_layer_selection(vec![id], Some(id));
            e.tools.drawing.bucket.sample_all = false;
            id
        })
    });
    let before = steps(&e, cx);
    click(&e, cx, (190., 80.));
    assert_eq!(steps(&e, cx), before + 1);
    let NodeKind::Strokes { strokes, .. } = node(&e, cx, vid) else {
        panic!("still a vector layer")
    };
    assert_eq!(strokes.fills.len(), 1);
    assert_eq!(strokes.fills[0].color, [255, 0, 0, 255]);
    let drawn = strokes.rasterize(256, 192);
    assert!(drawn.get(190, 80)[3] > 60000);
    assert_eq!(
        drawn.get(190, 20)[3],
        0,
        "the gap in the pencil outline held"
    );
    assert_eq!(cx.update(|_, cx| e.read(cx).selected), Some(vid));
}

#[gpui_kit::test]
fn lasso_cutter_lifts_the_selection_into_a_new_layer(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Paint"], None));
    let e = editor(&ws, cx);
    let id = selected(&e, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_tool(Tool::Select, cx);
            e.set_select(SelectShape::Lasso, cx);
        })
    });
    gesture(
        &e,
        cx,
        &[
            (20., 20.),
            (100., 20.),
            (100., 100.),
            (20., 100.),
            (20., 21.),
        ],
    );
    assert!(cx.update(|_, cx| e.read(cx).editor.doc.selection.is_some()));
    let before = steps(&e, cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.cut_to_new_layer(false, cx)));
    assert_eq!(steps(&e, cx), before + 1, "one Undo step");
    let new = selected(&e, cx);
    assert_ne!(new, id);
    assert!(pixel(&e, cx, new, 60, 60)[3] > 60000, "lifted");
    assert_eq!(pixel(&e, cx, new, 150, 150)[3], 0);
    assert_eq!(pixel(&e, cx, id, 60, 60)[3], 0, "cut out of the source");
    assert!(pixel(&e, cx, id, 150, 150)[3] > 60000);
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    assert!(cx.update(|_, cx| e.read(cx).editor.doc.node(new).is_none()));
    assert!(pixel(&e, cx, id, 60, 60)[3] > 60000);
    // Copy keeps the source.
    cx.update(|_, cx| e.update(cx, |e, cx| e.cut_to_new_layer(true, cx)));
    assert!(pixel(&e, cx, id, 60, 60)[3] > 60000);
    let copy = selected(&e, cx);
    assert!(pixel(&e, cx, copy, 60, 60)[3] > 60000);
}

#[gpui_kit::test]
fn selection_distort_previews_live_and_commits_as_one_step(cx: &mut TestAppContext) {
    let art = Raster::from_fn(256, 192, [0; 4], |x, y| {
        if (40..80).contains(&x) && (40..80).contains(&y) {
            RED
        } else {
            [0; 4]
        }
    });
    let (ws, cx) = open(cx, doc(&["Paint"], Some(art)));
    let e = editor(&ws, cx);
    let id = selected(&e, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_tool(Tool::Select, cx);
            e.execute(
                Command::SetSelection {
                    selection: Some(Arc::new(emulsion_raster::select::rect(
                        256, 192, 30., 30., 60., 60.,
                    ))),
                },
                cx,
            );
            e.start_distort(DistortKind::Envelope(3), cx);
        })
    });
    let before = steps(&e, cx);
    // Drag the lattice's right-hand point (90, 50) (index 7) out to the right.
    assert_eq!(pixel(&e, cx, id, 85, 58), [0; 4]);
    gesture(&e, cx, &[(90., 50.), (150., 50.)]);
    assert!(cx.update(|_, cx| e.read(cx).editor.in_transaction()));
    assert!(
        red(pixel(&e, cx, id, 85, 58)),
        "the preview shows the stretch"
    );
    cx.update(|_, cx| e.update(cx, |e, cx| e.apply_distort(cx)));
    assert!(!cx.update(|_, cx| e.read(cx).editor.in_transaction()));
    assert_eq!(steps(&e, cx), before + 1, "one Undo step");
    assert!(red(pixel(&e, cx, id, 85, 58)));
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    assert_eq!(pixel(&e, cx, id, 85, 58), [0; 4]);
    // Cancel leaves the layer alone.
    cx.update(|_, cx| e.update(cx, |e, cx| e.start_distort(DistortKind::Perspective, cx)));
    gesture(&e, cx, &[(80., 80.), (120., 120.)]);
    cx.update(|_, cx| e.update(cx, |e, cx| e.cancel_distort(cx)));
    assert_eq!(pixel(&e, cx, id, 100, 100), [0; 4]);
    assert!(!cx.update(|_, cx| e.read(cx).editor.in_transaction()));
}

#[gpui_kit::test]
fn brush_strokes_started_near_the_ruler_follow_its_edge(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Paint"], Some(Raster::solid(256, 192, [1.0; 4]))));
    let e = editor(&ws, cx);
    let id = selected(&e, cx);
    paint(&e, cx, PaintKind::Brush, 4.0);
    cx.update(|_, cx| e.update(cx, |e, cx| e.toggle_ruler(cx)));
    // The ruler runs across the middle at y = 96; start just below it and
    // wander off downwards.
    gesture(&e, cx, &[(80., 100.), (130., 110.), (180., 130.)]);
    assert!(red(pixel(&e, cx, id, 150, 96)), "drawn along the edge");
    assert_eq!(
        pixel(&e, cx, id, 180, 130),
        WHITE,
        "not where the hand went"
    );
    // Its ends drag like guide handles.
    gesture(&e, cx, &[(192., 96.), (192., 150.)]);
    let r = cx.update(|_, cx| e.read(cx).editor.doc.drawing_guides.ruler.unwrap());
    assert!((r.b.1 - 150.).abs() < 1.0);
    // A stroke far from the ruler is free.
    gesture(&e, cx, &[(20., 20.), (60., 40.)]);
    assert!(red(pixel(&e, cx, id, 60, 40)));
}

#[gpui_kit::test]
fn curvilinear_assist_bends_strokes_and_guide_sets_switch(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Paint"], Some(Raster::solid(256, 192, [1.0; 4]))));
    let e = editor(&ws, cx);
    let id = selected(&e, cx);
    paint(&e, cx, PaintKind::Brush, 4.0);
    let guide = GuideKind::Curvilinear {
        center: (128., 96.),
        radius: 150.,
        five: false,
    };
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            e.editor.doc.drawing_guides.set_primary(guide.clone());
            e.tools.guide.assist = true;
        })
    });
    gesture(&e, cx, &[(128., 60.), (184., 60.), (240., 60.)]);
    // The stroke follows the arc through the side vanishing points.
    let emulsion_core::drawing_guides::AssistCurve::Circle { center, radius } =
        guide.curves((128., 60.))[0]
    else {
        panic!("off the horizon the guide is an arc")
    };
    let arc_y = |x: f64| center.1 - (radius * radius - (x - center.0).powi(2)).sqrt();
    assert!(arc_y(240.) > 70.0, "the arc bends away from the horizon");
    for x in [200u32, 225] {
        let y = arc_y(x as f64);
        assert!(red(pixel(&e, cx, id, x, y as u32)), "on the arc at x = {x}");
        assert_eq!(
            pixel(&e, cx, id, x, 60),
            WHITE,
            "not along the straight drag"
        );
    }
    // Save the guide as a set, show another, and switch back.
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            let g = &mut e.editor.doc.drawing_guides;
            g.save_set("Fish-eye").unwrap();
            g.set_primary(GuideKind::Grid { size: 16. });
            assert!(g.switch_to(0));
        })
    });
    let primary = cx.update(|_, cx| e.read(cx).editor.doc.drawing_guides.primary());
    assert_eq!(primary, guide);
    // Undo does not take the guides away.
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    let sets = cx.update(|_, cx| e.read(cx).editor.doc.drawing_guides.sets.len());
    assert_eq!(sets, 1);
}
