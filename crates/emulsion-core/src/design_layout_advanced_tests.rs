use super::*;
use crate::{Command, Editor, Node, command::Slot};
use emulsion_raster::{vector::PathStyle, vector_geometry::rectangle};
use std::sync::Arc;

fn fixture(count: usize) -> (Editor, NodeId, Vec<NodeId>) {
    let mut editor = Editor::new(Document::new(800, 600), None);
    let ids = (0..count)
        .map(|i| {
            editor
                .execute(Command::AddNode {
                    node: Box::new(Node::path(
                        0,
                        "Item",
                        Arc::new(rectangle(i as f64 * 100., 0., 50., 40.)),
                        PathStyle {
                            fill: Some([255; 4]),
                            stroke: None,
                            ..Default::default()
                        },
                        800,
                        600,
                    )),
                    slot: Slot::TOP,
                })
                .unwrap()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let group = editor
        .execute(Command::Group {
            ids: ids.clone(),
            name: "Frame".into(),
        })
        .unwrap()
        .unwrap();
    (editor, group, ids)
}
fn frame(flow: Flow) -> Frame {
    Frame {
        flow,
        padding: [0.; 4],
        gap: 0.,
        wrap: false,
        ..Default::default()
    }
}
fn install(editor: &mut Editor, group: NodeId, frame: Frame, size: (f64, f64)) {
    editor.begin("Responsive sizing");
    enable(editor, group, frame, size).unwrap();
    editor.end();
}
fn dimensions(editor: &Editor, id: NodeId) -> (f64, f64) {
    item_dimensions(&editor.doc, id).unwrap()
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 0.02, "{a} != {b}");
}
fn stable(editor: &mut Editor) {
    let before = editor.doc.clone();
    for _ in 0..3 {
        editor
            .execute(Command::SetDesign {
                design: Box::new(editor.doc.design.clone()),
            })
            .unwrap();
    }
    assert_eq!(editor.doc, before);
}

#[test]
fn old_json_defaults_remain_fixed_and_optional() {
    let child: Child = serde_json::from_str(r#"{"absolute":false,"fill_width":true}"#).unwrap();
    assert!(child.fill_width);
    assert!(!child.fill_height);
    assert_eq!(child.aspect_ratio, None);
    assert_eq!(child.max_height, None);
    let f: Frame = serde_json::from_str(r#"{"boundary":1}"#).unwrap();
    assert!(!f.hug_width && !f.hug_height);
    assert_eq!(f.min_width, None);
}

#[test]
fn row_equal_shares_respect_asymmetric_minimum_and_redistribute_caps() {
    let (mut e, g, ids) = fixture(2);
    let mut f = frame(Flow::Row);
    f.children.insert(
        ids[0],
        Child {
            fill_width: true,
            min_width: Some(100.),
            ..Default::default()
        },
    );
    f.children.insert(
        ids[1],
        Child {
            fill_width: true,
            ..Default::default()
        },
    );
    install(&mut e, g, f.clone(), (300., 100.));
    close(dimensions(&e, ids[0]).0, 150.);
    close(dimensions(&e, ids[1]).0, 150.);
    f.children.get_mut(&ids[0]).unwrap().min_width = None;
    f.children.get_mut(&ids[0]).unwrap().max_width = Some(50.);
    let before = e.doc.clone();
    install(&mut e, g, f, (300., 100.));
    close(dimensions(&e, ids[0]).0, 50.);
    close(dimensions(&e, ids[1]).0, 250.);
    stable(&mut e);
    assert!(e.undo());
    assert_eq!(e.doc, before);
}

#[test]
fn column_fill_height_and_cross_axis_ratio_redistribute_unused_space() {
    let (mut e, g, ids) = fixture(2);
    let mut f = frame(Flow::Column);
    f.children.insert(
        ids[0],
        Child {
            fill_height: true,
            max_height: Some(50.),
            ..Default::default()
        },
    );
    f.children.insert(
        ids[1],
        Child {
            fill_height: true,
            ..Default::default()
        },
    );
    install(&mut e, g, f, (100., 300.));
    close(dimensions(&e, ids[0]).1, 50.);
    close(dimensions(&e, ids[1]).1, 250.);
    stable(&mut e);
    let mut f = frame(Flow::Row);
    f.children.insert(
        ids[0],
        Child {
            fill_width: true,
            fill_height: true,
            aspect_ratio: Some(1.),
            ..Default::default()
        },
    );
    f.children.insert(
        ids[1],
        Child {
            fill_width: true,
            ..Default::default()
        },
    );
    install(&mut e, g, f, (400., 50.));
    close(dimensions(&e, ids[0]).0, 50.);
    close(dimensions(&e, ids[1]).0, 350.);
    stable(&mut e);
}

#[test]
fn grid_reserves_equal_height_cells_for_ratio_limited_children() {
    let (mut e, g, ids) = fixture(4);
    let mut f = Frame {
        padding: [10.; 4],
        gap: 10.,
        columns: 2,
        ..frame(Flow::Grid)
    };
    for id in &ids {
        f.children.insert(
            *id,
            Child {
                fill_height: true,
                max_width: Some(70.),
                aspect_ratio: Some(2.),
                ..Default::default()
            },
        );
    }
    install(&mut e, g, f, (300., 200.));
    close(dimensions(&e, ids[0]).1, 35.);
    close(
        measure(&e.doc, ids[2]).unwrap().y - measure(&e.doc, ids[0]).unwrap().y,
        95.,
    );
    stable(&mut e);
}

#[test]
fn content_width_limits_and_height_driven_ratio_are_stable() {
    let (mut e, g, ids) = fixture(1);
    let mut f = Frame {
        hug_width: true,
        min_width: Some(220.),
        max_width: Some(500.),
        ..frame(Flow::Column)
    };
    f.children.insert(
        ids[0],
        Child {
            fill_height: true,
            min_width: Some(60.),
            max_width: Some(140.),
            min_height: Some(30.),
            max_height: Some(70.),
            aspect_ratio: Some(2.),
            ..Default::default()
        },
    );
    install(&mut e, g, f, (300., 200.));
    close(dimensions(&e, g).0, 220.);
    assert_eq!(dimensions(&e, ids[0]), (140., 70.));
    stable(&mut e);
}

#[test]
fn invalid_cycles_ratios_and_locked_children_are_atomic() {
    let (mut e, g, ids) = fixture(1);
    let before = e.doc.clone();
    let mut f = Frame {
        hug_width: true,
        ..frame(Flow::Column)
    };
    f.children.insert(
        ids[0],
        Child {
            fill_width: true,
            ..Default::default()
        },
    );
    assert!(enable(&mut e, g, f.clone(), (200., 200.)).is_err());
    assert_eq!(e.doc, before);
    f.hug_width = false;
    f.children.insert(
        ids[0],
        Child {
            min_width: Some(100.),
            max_width: Some(100.),
            min_height: Some(100.),
            max_height: Some(100.),
            aspect_ratio: Some(2.),
            ..Default::default()
        },
    );
    assert!(enable(&mut e, g, f.clone(), (200., 200.)).is_err());
    assert_eq!(e.doc, before);
    e.execute(Command::SetLayerLocks {
        id: ids[0],
        locks: crate::node::LayerLocks {
            position: true,
            ..Default::default()
        },
    })
    .unwrap();
    let before = e.doc.clone();
    f.children.insert(
        ids[0],
        Child {
            fill_height: true,
            ..Default::default()
        },
    );
    assert!(enable(&mut e, g, f, (200., 200.)).is_err());
    assert_eq!(e.doc, before);
}

#[test]
fn wrapped_rows_use_full_height_per_row_without_shrinking_constraints() {
    let (mut e, g, ids) = fixture(2);
    let mut f = Frame {
        wrap: true,
        gap: 10.,
        ..frame(Flow::Row)
    };
    for id in &ids {
        f.children.insert(
            *id,
            Child {
                fill_height: true,
                min_width: Some(80.),
                ..Default::default()
            },
        );
    }
    install(&mut e, g, f, (100., 60.));
    close(dimensions(&e, g).1, 60.);
    close(
        measure(&e.doc, ids[1]).unwrap().y - measure(&e.doc, ids[0]).unwrap().y,
        70.,
    );
    close(dimensions(&e, ids[1]).1, 60.);
    stable(&mut e);
}

#[test]
fn many_siblings_share_space_in_bounded_passes() {
    let allocation = shares(1024., &vec![(1., 100000.); 511]);
    close(allocation.iter().sum(), 1024.);
    assert!(allocation.iter().all(|x| (*x - allocation[0]).abs() < 1e-9));
}

#[test]
fn row_reserves_height_driven_ratio_width_before_flexible_siblings() {
    let (mut e, g, ids) = fixture(2);
    let mut f = frame(Flow::Row);
    f.children.insert(
        ids[0],
        Child {
            fill_height: true,
            aspect_ratio: Some(1.),
            ..Default::default()
        },
    );
    f.children.insert(
        ids[1],
        Child {
            fill_width: true,
            ..Default::default()
        },
    );
    install(&mut e, g, f, (500., 100.));
    close(dimensions(&e, ids[0]).0, 100.);
    close(dimensions(&e, ids[1]).0, 400.);
    stable(&mut e);
}

#[test]
fn text_rewrap_obeys_height_limit_in_first_command_and_preserves_font() {
    let (mut e, g, _) = fixture(1);
    let id = e
        .execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Paragraph",
                crate::text::TextSpec {
                    text: "Native paragraph text wraps across narrow frames.".into(),
                    size: 20.,
                    width: Some(600.),
                    ..Default::default()
                },
                800,
                600,
            )),
            slot: Slot::top_of(Some(g)),
        })
        .unwrap()
        .unwrap();
    assert!(dimensions(&e, id).1 < 40.);
    let mut f = frame(Flow::Column);
    f.children.insert(
        id,
        Child {
            fill_width: true,
            max_height: Some(40.),
            ..Default::default()
        },
    );
    let before = e.doc.clone();
    install(&mut e, g, f, (100., 400.));
    let NodeKind::Text { spec, .. } = &e.doc.node(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(spec.size, 20.);
    assert_eq!(spec.scale_x, 1.);
    assert_eq!(spec.scale_y, 1.);
    assert_eq!(spec.width, Some(100.));
    assert_eq!(spec.height, Some(40.));
    stable(&mut e);
    assert!(e.undo());
    assert_eq!(e.doc, before);
}

#[test]
fn deep_filled_and_intrinsic_frames_reflow_once_per_level_without_drift() {
    for filled in [false, true] {
        let (mut e, mut group, ids) = fixture(1);
        let depth = 12;
        let mut child = ids[0];
        for level in 0..depth {
            if level > 0 {
                child = group;
                group = e
                    .execute(Command::Group {
                        ids: vec![group],
                        name: "Nested".into(),
                    })
                    .unwrap()
                    .unwrap();
            }
            let mut f = Frame {
                hug_height: true,
                hug_width: !filled,
                min_width: Some(1.),
                padding: [1.; 4],
                ..frame(Flow::Column)
            };
            f.children.insert(
                child,
                Child {
                    fill_width: filled,
                    ..Default::default()
                },
            );
            install(&mut e, group, f, (300., 500.));
        }
        let before = e.doc.clone();
        ADVANCED_VISITS.with(|counter| counter.set(0));
        reflow(&mut e.doc).unwrap();
        let visits = ADVANCED_VISITS.with(|counter| counter.get());
        assert!(
            visits <= depth * 2,
            "{visits} visits for {depth} nested frames (fill={filled})"
        );
        assert_eq!(e.doc, before);
        stable(&mut e);
    }
}

#[test]
fn zero_axis_rule_keeps_stroke_and_rejects_impossible_fill_atomically() {
    use emulsion_raster::vector::{Anchor, Path, SubPath};
    let (mut e, g, ids) = fixture(1);
    let style = PathStyle {
        fill: None,
        stroke: Some([20, 30, 40, 255]),
        width: 3.,
        ..Default::default()
    };
    e.execute(Command::SetPath {
        id: ids[0],
        path: Arc::new(Path {
            subpaths: vec![SubPath {
                anchors: vec![Anchor::corner((0., 0.)), Anchor::corner((100., 0.))],
                closed: false,
            }],
        }),
        style,
    })
    .unwrap();
    let f = Frame {
        hug_width: true,
        ..frame(Flow::Column)
    };
    install(&mut e, g, f.clone(), (200., 100.));
    let NodeKind::Path { style: actual, .. } = &e.doc.node(ids[0]).unwrap().kind else {
        panic!()
    };
    close(actual.width as f64, 3.);
    close(dimensions(&e, ids[0]).1, 1.);
    stable(&mut e);
    let before = e.doc.clone();
    let mut f = f;
    f.children.insert(
        ids[0],
        Child {
            fill_height: true,
            ..Default::default()
        },
    );
    let error = enable(&mut e, g, f, (200., 100.)).unwrap_err();
    assert!(error.contains("zero-height"), "{error}");
    assert_eq!(e.doc, before);
}

#[test]
fn fractional_semantic_ratio_does_not_jump_on_capture() {
    let (mut e, g, ids) = fixture(1);
    let style = PathStyle {
        fill: Some([255; 4]),
        stroke: None,
        ..Default::default()
    };
    e.execute(Command::SetPath {
        id: ids[0],
        path: Arc::new(rectangle(0., 0., 40.5, 20.25)),
        style,
    })
    .unwrap();
    let (w, h) = dimensions(&e, ids[0]);
    close(w / h, 2.);
    let mut f = frame(Flow::Column);
    f.children.insert(
        ids[0],
        Child {
            aspect_ratio: Some(w / h),
            ..Default::default()
        },
    );
    install(&mut e, g, f, (200., 100.));
    assert_eq!(dimensions(&e, ids[0]), (40.5, 20.25));
    stable(&mut e);
}

#[test]
fn canvas_breakpoints_inherit_base_without_cascade_and_reflow_on_thresholds() {
    let (mut e, g, ids) = fixture(2);
    let mut f = frame(Flow::Column);
    f.gap = 7.;
    f.breakpoints = vec![
        Breakpoint {
            min_width: 900.,
            overrides: FrameOverrides {
                flow: Some(Flow::Grid),
                columns: Some(2),
                ..Default::default()
            },
        },
        Breakpoint {
            min_width: 600.,
            overrides: FrameOverrides {
                flow: Some(Flow::Row),
                gap: Some(25.),
                clip_content: Some(true),
                ..Default::default()
            },
        },
    ];
    let original = e.doc.clone();
    install(&mut e, g, f, (300., 200.));
    assert_eq!(active_breakpoint(&e.doc, g), Some(600.));
    close(
        measure(&e.doc, ids[1]).unwrap().x - measure(&e.doc, ids[0]).unwrap().x,
        75.,
    );
    assert!(effective_frame(&e.doc, g).unwrap().clip_content);
    stable(&mut e);
    let installed = e.doc.clone();
    for (width, flow, gap, clip, threshold) in [
        (950, Flow::Grid, 7., false, Some(900.)),
        (500, Flow::Column, 7., false, None),
        (800, Flow::Row, 25., true, Some(600.)),
    ] {
        e.doc.width = width;
        reflow(&mut e.doc).unwrap();
        let active = effective_frame(&e.doc, g).unwrap();
        assert_eq!(
            (active.flow, active.gap, active.clip_content),
            (flow, gap, clip)
        );
        assert_eq!(active_breakpoint(&e.doc, g), threshold);
        assert_eq!(e.doc.design, installed.design);
        stable(&mut e);
    }
    e.doc = installed;
    assert!(e.undo());
    assert_eq!(e.doc, original);
}

#[test]
fn inactive_breakpoint_conflicts_duplicate_widths_and_unknown_fields_reject() {
    let (mut e, g, ids) = fixture(1);
    let mut f = frame(Flow::Column);
    f.children.insert(
        ids[0],
        Child {
            fill_width: true,
            ..Default::default()
        },
    );
    f.breakpoints = vec![Breakpoint {
        min_width: 5000.,
        overrides: FrameOverrides {
            hug_width: Some(true),
            ..Default::default()
        },
    }];
    let before = e.doc.clone();
    assert!(enable(&mut e, g, f.clone(), (300., 200.)).is_err());
    assert_eq!(e.doc, before);
    f.children.clear();
    f.breakpoints.push(f.breakpoints[0].clone());
    assert!(enable(&mut e, g, f.clone(), (300., 200.)).is_err());
    assert_eq!(e.doc, before);
    f.breakpoints.pop();
    f.breakpoints[0].overrides = FrameOverrides {
        gap: Some(f64::NAN),
        ..Default::default()
    };
    assert!(enable(&mut e, g, f, (300., 200.)).is_err());
    assert_eq!(e.doc, before);
    assert!(
        serde_json::from_value::<Breakpoint>(
            serde_json::json!({"min_width":600,"overrides":{"gapp":10}})
        )
        .is_err()
    );
}

#[test]
fn content_width_breakpoint_reference_stays_canvas_based_and_nested_cycles_validate() {
    let (mut e, inner, ids) = fixture(1);
    let mut f = frame(Flow::Column);
    f.breakpoints = vec![Breakpoint {
        min_width: 600.,
        overrides: FrameOverrides {
            hug_width: Some(true),
            ..Default::default()
        },
    }];
    install(&mut e, inner, f, (300., 100.));
    assert_eq!(active_breakpoint(&e.doc, inner), Some(600.));
    close(dimensions(&e, inner).0, 50.);
    stable(&mut e);
    let outer = e
        .execute(Command::Group {
            ids: vec![inner],
            name: "Outer".into(),
        })
        .unwrap()
        .unwrap();
    let mut f = frame(Flow::Column);
    f.children.insert(
        inner,
        Child {
            fill_width: true,
            ..Default::default()
        },
    );
    let before = e.doc.clone();
    assert!(enable(&mut e, outer, f, (500., 300.)).is_err());
    assert_eq!(e.doc, before);
    assert_eq!(dimensions(&e, ids[0]).0, 50.);
}

#[test]
fn breakpoint_schema_roundtrip_keeps_native_ids_and_old_defaults() {
    let (mut e, g, _) = fixture(1);
    let mut f = frame(Flow::Column);
    f.clip_content = true;
    f.breakpoints = vec![Breakpoint {
        min_width: 600.5,
        overrides: FrameOverrides {
            padding: Some([1., 2., 3., 4.]),
            clip_content: Some(false),
            ..Default::default()
        },
    }];
    install(&mut e, g, f, (300., 200.));
    let value = serde_json::to_value(&e.doc.design).unwrap();
    let restored: crate::design_metadata::Design = serde_json::from_value(value).unwrap();
    assert_eq!(restored, e.doc.design);
    let old: Frame = serde_json::from_str("{}").unwrap();
    assert!(!old.clip_content && old.breakpoints.is_empty());
}

#[test]
fn inactive_breakpoint_does_not_change_legacy_stroked_geometry() {
    let (mut e, g, ids) = fixture(2);
    e.execute(Command::SetPath {
        id: ids[0],
        path: Arc::new(rectangle(0.25, 0.5, 40.5, 20.25)),
        style: PathStyle {
            stroke: Some([255; 4]),
            width: 5.,
            ..Default::default()
        },
    })
    .unwrap();
    install(&mut e, g, frame(Flow::Row), (300., 200.));
    let before = e.doc.clone();
    let mut design = e.doc.design.clone();
    design
        .frames
        .get_mut(&g)
        .unwrap()
        .breakpoints
        .push(Breakpoint {
            min_width: 5000.,
            overrides: FrameOverrides {
                hug_width: Some(true),
                ..Default::default()
            },
        });
    e.execute(Command::SetDesign {
        design: Box::new(design),
    })
    .unwrap();
    let mut after = e.doc.clone();
    after.design = before.design.clone();
    assert_eq!(after, before);
    stable(&mut e);
}
