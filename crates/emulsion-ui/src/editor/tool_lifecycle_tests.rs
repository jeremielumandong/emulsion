use super::*;
use core::prelude::v1::test;
use emulsion_raster::vector::{Anchor, SubPath};

fn view(cx: &mut TestAppContext) -> Entity<EditorView> {
    cx.update(|cx| {
        gpui_kit::init(cx);
        theme::install(cx);
        cx.set_global(crate::app_state::AppSettings(Default::default()));
        cx.new(|cx| EditorView::new(Document::new(64, 64), None, None, None, "tools".into(), cx))
    })
}

fn path() -> SubPath {
    SubPath {
        anchors: vec![Anchor::corner((2., 2.)), Anchor::corner((12., 12.))],
        closed: false,
    }
}

#[gpui_kit::test]
fn premature_enter_keeps_draft_points_and_straighten_escape_is_handled(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        v.set_tool(Tool::Pen, cx);
        v.tools.pen.building = Some(SubPath {
            anchors: vec![Anchor::corner((5., 5.))],
            closed: false,
        });
        v.tool_commit(cx);
        assert_eq!(v.tools.pen.building.as_ref().unwrap().anchors.len(), 1);
        v.set_select(SelectShape::Polygon, cx);
        v.tools.polygon = vec![(5., 5.), (20., 5.)];
        v.tool_commit(cx);
        assert_eq!(v.tools.polygon.len(), 2);
        v.set_tool(Tool::Crop, cx);
        v.tools.straighten = 10.;
        assert!(v.tool_cancel(cx));
        assert_eq!(v.tools.straighten, 0.);
        assert!(v.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn switching_tools_clears_hidden_previews_but_reselecting_preserves_them(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        v.set_select(SelectShape::Polygon, cx);
        v.tools.polygon = vec![(1., 1.), (2., 2.)];
        v.set_select(SelectShape::Polygon, cx);
        v.set_tool(Tool::Select, cx);
        assert_eq!(v.tools.polygon.len(), 2);
        v.set_tool(Tool::Pen, cx);
        assert!(v.tools.polygon.is_empty());
        v.tools.pen.building = Some(path());
        v.set_tool(Tool::Crop, cx);
        assert!(v.tools.pen.building.is_none());
        v.tools.crop = Some((0., 0., 12., 12.));
        v.set_paint(PaintKind::Eraser, cx);
        assert!(v.tools.crop.is_none());
    });
}

#[gpui_kit::test]
fn escape_clears_every_pending_state_and_discards_live_shape(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        v.tools.polygon = vec![(1., 1.)];
        v.tools.crop = Some((0., 0., 12., 12.));
        v.tools.pen.building = Some(path());
        v.tools.pen.selected = Some((0, 0));
        v.tools.straighten = 10.;
        v.drag = Some(Drag::Tool(ToolDrag::Shape {
            start: (1., 1.),
            end: (20., 20.),
            ellipse: false,
        }));
        assert!(v.tool_cancel(cx));
        assert!(v.drag.is_none());
        assert!(v.tools.polygon.is_empty());
        assert!(v.tools.crop.is_none());
        assert!(v.tools.pen.building.is_none());
        assert!(v.tools.pen.selected.is_none());
        assert_eq!(v.tools.straighten, 0.);
        assert!(v.editor.doc.nodes.is_empty());
        assert!(!v.tool_cancel(cx));
    });
}

#[gpui_kit::test]
fn enter_only_commits_the_active_tool(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        v.tool = Tool::Hand;
        v.tools.pen.building = Some(path());
        v.tools.crop = Some((0., 0., 12., 12.));
        v.tool_commit(cx);
        assert!(v.editor.doc.nodes.is_empty());
        assert_eq!(v.editor.doc.width, 64);
        v.tool = Tool::Crop;
        v.tool_commit(cx);
        assert_eq!(v.editor.doc.width, 12);
        assert!(v.editor.doc.nodes.is_empty());
    });
}

#[gpui_kit::test]
fn escape_rolls_back_selection_drag_but_switching_retains_one_undo_step(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        for cancel in [true, false] {
            v.tool = Tool::Select;
            let mask = Arc::new(select::rect(64, 64, 2., 2., 10., 10.));
            v.editor.begin("Move selection");
            v.execute(
                Command::SetSelection {
                    selection: Some(mask.clone()),
                },
                cx,
            );
            v.drag = Some(Drag::Tool(ToolDrag::MoveSelection {
                start: (2., 2.),
                orig: mask,
            }));
            if cancel {
                assert!(v.tool_cancel(cx));
                assert!(v.editor.doc.selection.is_none());
                assert!(!v.editor.undo());
            } else {
                v.set_tool(Tool::Hand, cx);
                assert!(v.editor.doc.selection.is_some());
                assert!(v.editor.undo());
                assert!(v.editor.doc.selection.is_none());
            }
            assert!(!v.editor.in_transaction());
            assert!(v.drag.is_none());
        }
    });
}

#[gpui_kit::test]
fn selection_shortcut_preserves_brush_settings_and_leaves_mask_mode(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        v.set_paint(PaintKind::Brush, cx);
        v.tools.brush.size = 73.;
        v.tools.mask_edit = true;
        v.set_select(SelectShape::Lasso, cx);
        assert!(!v.tools.mask_edit);
        v.set_paint(PaintKind::Brush, cx);
        assert_eq!(v.tools.brush.size, 73.);
    });
}

#[gpui_kit::test]
fn escape_transform_restores_placement_and_preserves_committed_history(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let id = v
            .execute(
                Command::AddNode {
                    node: Box::new(Node::raster(
                        0,
                        "Pixels",
                        Arc::new(Raster::solid(16, 16, [1.; 4])),
                        Placement::default(),
                    )),
                    slot: Slot::TOP,
                },
                cx,
            )
            .unwrap();
        v.editor.begin("Transform");
        v.execute(
            Command::SetPlacement {
                id,
                placement: Placement::at(10., 20.),
            },
            cx,
        );
        v.drag = Some(Drag::Transform(super::super::transform::Grab {
            collective: false,
            current: Placement::default(),
            mask: None,
            id,
            start: Placement::default(),
            handle: super::super::transform::Handle::Corner(0),
            start_doc: (0., 0.),
            size: (16, 16),
        }));
        assert!(v.tool_cancel(cx));
        assert!(v.drag.is_none());
        assert!(!v.editor.in_transaction());
        let NodeKind::Raster { placement, .. } = &v.editor.doc.node(id).unwrap().kind else {
            panic!("raster")
        };
        assert_eq!(*placement, Placement::default());
        assert!(v.editor.undo());
        assert!(
            v.editor.doc.nodes.is_empty(),
            "previously committed Add is still undoable"
        );
    });
}

#[gpui_kit::test]
fn escape_discards_warp_and_distort_previews_without_undoing_edits(cx: &mut TestAppContext) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        let before = v.editor.doc.clone();
        v.warp = Some(super::super::transform::WarpState {
            id: 1,
            cols: 1,
            rows: 1,
            grid: vec![(0., 0.); 4],
        });
        v.drag = Some(Drag::Warp(0));
        assert!(v.tool_cancel(cx));
        assert!(v.warp.is_none());
        assert!(v.drag.is_none());
        assert_eq!(v.editor.doc, before);
        v.drag = Some(Drag::Distort {
            id: 1,
            corner: 0,
            quad: [(0., 0.); 4],
        });
        assert!(v.tool_cancel(cx));
        assert!(v.drag.is_none());
        assert_eq!(v.editor.doc, before);
        assert!(!v.editor.undo());
    });
}

#[test]
fn constrained_shapes_keep_drag_direction_and_equal_sides() {
    for (end, expected) in [
        ((30., 15.), (10., 10., 20., 20.)),
        ((-10., 15.), (-10., 10., 20., 20.)),
        ((30., 5.), (10., -10., 20., 20.)),
        ((-10., 5.), (-10., -10., 20., 20.)),
        ((10., 30.), (10., 10., 20., 20.)),
    ] {
        assert_eq!(shape_rect((10., 10.), end, true), expected);
        assert_eq!(shape_rect((10., 10.), end, false), norm((10., 10.), end));
    }
}

#[gpui_kit::test]
fn shift_shape_preview_and_commit_agree_and_releasing_shift_restores_aspect(
    cx: &mut TestAppContext,
) {
    let v = view(cx);
    v.update(cx, |v, cx| {
        v.tool = Tool::Shape;
        for ellipse in [false, true] {
            for shift in [true, false] {
                v.drag = Some(Drag::Tool(ToolDrag::Shape {
                    start: (10., 10.),
                    end: (30., 15.),
                    ellipse,
                }));
                v.drag_shift = true;
                let constrained = v.overlay(1.).lines[0].0.clone();
                let height = |points: &[(f64, f64)]| {
                    points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max)
                        - points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min)
                };
                assert!((height(&constrained) - 20.).abs() < 1e-9);
                v.drag_shift = shift;
                let preview = v.overlay(1.).lines[0].0.clone();
                assert!((height(&preview) - if shift { 20. } else { 5. }).abs() < 1e-9);
                let Some(Drag::Tool(drag)) = v.drag.take() else {
                    panic!("shape drag")
                };
                v.tool_up(drag, cx);
                let node = v.editor.doc.nodes.last().unwrap();
                assert!(node.mask.is_none());
                let NodeKind::Path { path, .. } = &node.kind else {
                    panic!("editable shape")
                };
                let bounds = emulsion_raster::vector_geometry::bounds(path).unwrap();
                assert_eq!(bounds, (10., 10., 20., if shift { 20. } else { 5. }));
                assert_eq!(
                    path.subpaths[0].anchors.iter().any(|a| a.has_handles()),
                    ellipse
                );
            }
        }
    });
}

#[gpui_kit::test]
fn brush_samples_coalesce_and_pointer_up_flushes_before_undo(cx: &mut TestAppContext) {
    use emulsion_raster::paint::{Brush, Ink, Stroke};
    let v = view(cx);
    v.update(cx, |v, cx| {
        let base = Arc::new(Raster::solid(64, 64, [0.; 4]));
        let id = v
            .execute(
                Command::AddNode {
                    node: Box::new(Node::raster(0, "Paint", base.clone(), Placement::default())),
                    slot: Slot::TOP,
                },
                cx,
            )
            .unwrap();
        v.canvas_bounds.set(Some(Bounds::new(
            point(px(0.), px(0.)),
            size(px(64.), px(64.)),
        )));
        v.view = viewport::View {
            zoom: 1.,
            center: (32., 32.),
            rotation: 0.,
        };
        let brush = Brush {
            size: 6.,
            stabilizer: 0.,
            taper_end: 0.,
            ..Default::default()
        };
        v.editor.begin("Brush stroke");
        v.drag = Some(Drag::Tool(ToolDrag::Stroke {
            id,
            stroke: Box::new(Stroke::new(base, brush, Ink::Color([0., 0., 0., 1.]), None)),
            to_local: glam::DAffine2::IDENTITY,
            heal: false,
            label: "Brush stroke",
            mask: false,
            mask_raster: None,
        }));
        let rev = v.editor.revision;
        for x in [8., 16., 24.] {
            v.tool_move(point(px(x), px(24.)), cx);
        }
        assert_eq!(
            v.editor.revision, rev,
            "input samples must not compose separate previews"
        );
        v.flush_live_stroke(cx);
        assert_eq!(v.editor.revision, rev + 1);
        let NodeKind::Raster { raster, .. } = &v.editor.doc.node(id).unwrap().kind else {
            panic!("raster")
        };
        assert!(raster.get(16, 24)[3] > 0, "intermediate samples preserved");
        v.flush_live_stroke(cx);
        assert_eq!(v.editor.revision, rev + 1, "no duplicate preview");
        v.tool_move(point(px(40.), px(24.)), cx);
        let Some(Drag::Tool(drag)) = v.drag.take() else {
            panic!("stroke")
        };
        v.tool_up(drag, cx);
        let NodeKind::Raster { raster, .. } = &v.editor.doc.node(id).unwrap().kind else {
            panic!("raster")
        };
        assert!(
            raster.get(39, 24)[3] > 0,
            "pointer-up must publish pending samples even without taper"
        );
        assert!(!v.editor.in_transaction());
        assert!(v.editor.undo());
        let NodeKind::Raster { raster, .. } = &v.editor.doc.node(id).unwrap().kind else {
            panic!("raster")
        };
        assert_eq!(raster.get(16, 24)[3], 0);
        assert_eq!(raster.get(39, 24)[3], 0);
        let base = raster.clone();
        v.editor.begin("Brush stroke");
        v.drag = Some(Drag::Tool(ToolDrag::Stroke {
            id,
            stroke: Box::new(Stroke::new(
                base,
                Brush::default(),
                Ink::Color([0., 0., 0., 1.]),
                None,
            )),
            to_local: glam::DAffine2::IDENTITY,
            heal: false,
            label: "Brush stroke",
            mask: false,
            mask_raster: None,
        }));
        v.tool_move(point(px(16.), px(24.)), cx);
        assert!(v.tool_cancel(cx));
        v.flush_live_stroke(cx);
        let NodeKind::Raster { raster, .. } = &v.editor.doc.node(id).unwrap().kind else {
            panic!("raster")
        };
        assert_eq!(
            raster.get(16, 24)[3],
            0,
            "canceled pending samples must never appear"
        );
    });
}

#[gpui_kit::test]
fn choosing_a_medium_activates_its_brush_and_preserves_active_adjustments(cx: &mut TestAppContext) {
    use emulsion_raster::library;
    let v = view(cx);
    v.update(cx, |v, cx| {
        v.set_paint(PaintKind::Brush, cx);
        for category in ["Oil", "Ink", "Airbrush", "Eraser", "Smudge", "Chalk"] {
            let expected = library::library()
                .into_iter()
                .find(|p| p.category == category)
                .unwrap();
            v.select_brush_category(category, cx);
            assert_eq!(v.presets.category.as_deref(), Some(category));
            assert_eq!(v.presets.current.as_deref(), Some(expected.name.as_str()));
            assert_eq!(v.tools.brush, expected.brush.sanitized());
            assert_eq!(
                v.tools.paint,
                match category {
                    "Eraser" => PaintKind::Eraser,
                    "Smudge" => PaintKind::Smudge,
                    _ => PaintKind::Brush,
                }
            );
        }
        v.select_brush_category("Airbrush", cx);
        assert_eq!(v.tools.brush.hardness, 0.);
        assert_eq!(v.tools.brush.flow, 0.08);
        v.apply_preset_named("Fine spray", cx);
        v.tools.brush.size = 73.;
        v.select_brush_category("Airbrush", cx);
        assert_eq!(v.presets.current.as_deref(), Some("Fine spray"));
        assert_eq!(
            v.tools.brush.size, 73.,
            "reselecting must preserve adjustments"
        );
    });
}
