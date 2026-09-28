use super::*;
fn fixture() -> (Editor, NodeId, NodeId, NodeId) {
    let mut e = Editor::new(Document::new(800, 600), None);
    e.execute(Command::AddNode {
        node: Box::new(Node::new(
            0,
            "Background",
            NodeKind::Fill { rgba: [255; 4] },
        )),
        slot: Slot::TOP,
    })
    .unwrap();
    let a = add_shape(&mut e, ShapeKind::Process, [40., 40., 120., 60.], "Start").unwrap();
    let b = add_shape(
        &mut e,
        ShapeKind::Decision,
        [400., 240., 120., 80.],
        "Ready?",
    )
    .unwrap();
    let edge = connect(
        &mut e,
        Endpoint {
            shape: a,
            port: Port::East,
        },
        Endpoint {
            shape: b,
            port: Port::North,
        },
        "Yes",
        Routing::Orthogonal,
    )
    .unwrap();
    (e, a, b, edge)
}
fn path(e: &Editor, edge: NodeId) -> Arc<Path> {
    let id = e.doc.diagram.as_ref().unwrap().edges[&edge].path;
    let NodeKind::Path { path, .. } = &e.doc.node(id).unwrap().kind else {
        panic!()
    };
    path.clone()
}
#[test]
fn moving_deleting_and_undo_preserve_connections() {
    let (mut e, a, _, edge) = fixture();
    let original = e.doc.clone();
    assert_eq!(
        e.doc.children(None)[1],
        edge,
        "connector must be above the background"
    );
    e.execute(Command::TranslateNode {
        id: a,
        dx: 50.,
        dy: 20.,
    })
    .unwrap();
    assert_eq!(path(&e, edge).subpaths[0].anchors[0].p, (210., 90.));
    e.undo();
    assert_eq!(e.doc, original);
    e.execute(Command::RemoveNode { id: a }).unwrap();
    assert!(!e.doc.diagram.as_ref().unwrap().edges.contains_key(&edge));
    assert!(e.doc.node(edge).is_none());
    e.undo();
    assert_eq!(e.doc, original);
    e.doc.validate().unwrap();
}
#[test]
fn transforming_a_graph_retains_manual_waypoints_and_label_placement() {
    let (mut e, a, b, edge) = fixture();
    let mut diagram = e.doc.diagram.as_deref().unwrap().clone();
    diagram.edges.get_mut(&edge).unwrap().waypoints = vec![(230., 70.), (230., 190.)];
    diagram.edges.get_mut(&edge).unwrap().label_offset = (15., -12.);
    e.execute(Command::SetDiagram {
        diagram: Some(Arc::new(diagram)),
    })
    .unwrap();
    let original = e.doc.clone();
    let label = original.diagram.as_ref().unwrap().edges[&edge].label;
    e.execute(Command::TransformNodes {
        ids: vec![a, b, edge],
        transform: [1., 0., 0., 1., 50., 30.],
    })
    .unwrap();
    let model = e.doc.diagram.as_ref().unwrap();
    assert_eq!(
        model.edges[&edge].waypoints,
        vec![(280., 100.), (280., 220.)]
    );
    assert!((model.edges[&edge].label_offset.0 - 15.).abs() < 0.0001);
    assert!((model.edges[&edge].label_offset.1 + 12.).abs() < 0.0001);
    let (NodeKind::Text { spec: before, .. }, NodeKind::Text { spec: after, .. }) = (
        &original.node(label).unwrap().kind,
        &e.doc.node(label).unwrap().kind,
    ) else {
        panic!()
    };
    assert_eq!((after.x, after.y), (before.x + 50., before.y + 30.));
    e.undo();
    assert_eq!(e.doc, original);
}
#[test]
fn copy_rebinds_endpoints_and_retains_native_labels() {
    let (e, a, b, _) = fixture();
    let fragment = crate::fragment::Fragment::capture(&e.doc, &[a, b]).unwrap();
    assert_eq!(fragment.roots.len(), 3);
    let mut target = Editor::new(Document::new(1000, 800), None);
    add_shape(
        &mut target,
        ShapeKind::Note,
        [700., 500., 120., 60.],
        "Existing",
    )
    .unwrap();
    fragment.paste(&mut target, Slot::TOP, (25., 30.)).unwrap();
    let model = target.doc.diagram.as_ref().unwrap();
    assert_eq!(model.shapes.len(), 3);
    assert_eq!(model.edges.len(), 1);
    let (id, edge) = model.edges.first_key_value().unwrap();
    assert_ne!(edge.source.shape, a);
    assert_eq!(path(&target, *id).subpaths[0].anchors[0].p, (185., 100.));
    assert!(matches!(
        target.doc.node(edge.label).unwrap().kind,
        NodeKind::Text { .. }
    ));
    target.undo();
    assert_eq!(target.doc.diagram.as_ref().unwrap().shapes.len(), 1);
    target.redo();
    target.doc.validate().unwrap();
}
#[test]
fn malformed_diagram_is_rejected_atomically_and_self_loop_routes() {
    let (mut e, a, _, _) = fixture();
    let before = e.doc.clone();
    let mut model = e.doc.diagram.as_deref().unwrap().clone();
    model.edges.values_mut().next().unwrap().source.shape = 99999;
    assert!(
        e.execute(Command::SetDiagram {
            diagram: Some(Arc::new(model))
        })
        .is_err()
    );
    assert_eq!(e.doc, before);
    let edge = connect(
        &mut e,
        Endpoint {
            shape: a,
            port: Port::Auto,
        },
        Endpoint {
            shape: a,
            port: Port::Auto,
        },
        "Retry",
        Routing::Orthogonal,
    )
    .unwrap();
    let p = path(&e, edge);
    assert!(p.subpaths[0].anchors.len() >= 4);
    assert_ne!(
        p.subpaths[0].anchors.first().unwrap().p,
        p.subpaths[0].anchors.last().unwrap().p
    );
    for kind in ShapeKind::ALL {
        let p = kind.path([20., 30., 140., 80.]);
        assert!(!p.subpaths.is_empty());
        assert!(
            p.subpaths
                .iter()
                .flat_map(|s| &s.anchors)
                .all(|a| a.p.0.is_finite() && a.p.1.is_finite())
        );
    }
}
#[test]
fn native_duplicate_and_branch_merge_remap_shape_ids() {
    let (mut e, a, _, _) = fixture();
    let copy = e
        .execute(Command::DuplicateNode { id: a })
        .unwrap()
        .unwrap();
    let model = e.doc.diagram.as_ref().unwrap();
    assert_ne!(model.shapes[&a].label, model.shapes[&copy].label);
    assert_eq!(model.shapes.len(), 3);
    let base = Document::new(800, 600);
    let mut left = Editor::new(base.clone(), None);
    let mut right = Editor::new(base.clone(), None);
    add_shape(&mut left, ShapeKind::Process, [20., 20., 100., 60.], "Left").unwrap();
    add_shape(
        &mut right,
        ShapeKind::Process,
        [220., 20., 100., 60.],
        "Right",
    )
    .unwrap();
    let crate::graph::MergeOutcome::Merged(doc) =
        crate::graph::merge(&base, &left.doc, &right.doc, &HashMap::new()).unwrap()
    else {
        panic!("independent additions should merge")
    };
    assert_eq!(doc.diagram.as_ref().unwrap().shapes.len(), 2);
    doc.validate().unwrap();
}
#[test]
fn locked_connector_prevents_indirect_geometry_changes() {
    let (mut e, a, _, edge) = fixture();
    e.execute(Command::SetLocked {
        id: edge,
        locked: true,
    })
    .unwrap();
    let original = e.doc.clone();
    assert!(
        e.execute(Command::TranslateNode {
            id: a,
            dx: 50.,
            dy: 20.
        })
        .is_err()
    );
    assert_eq!(e.doc, original);
}

#[test]
fn layout_preserves_locked_positions_and_avoids_placing_shapes_over_them() {
    let (mut e, a, b, _) = fixture();
    let mut model = e.doc.diagram.as_deref().unwrap().clone();
    model.shapes.get_mut(&a).unwrap().layout_locked = true;
    e.execute(Command::SetDiagram {
        diagram: Some(Arc::new(model)),
    })
    .unwrap();
    let before = e.doc.clone();
    arrange(&mut e, Layout::Grid).unwrap();
    let model = e.doc.diagram.as_ref().unwrap();
    let fixed = shape_bounds(&e.doc, &model.shapes[&a]).unwrap();
    assert_eq!(
        fixed,
        shape_bounds(&before, &before.diagram.as_ref().unwrap().shapes[&a]).unwrap()
    );
    let moved = shape_bounds(&e.doc, &model.shapes[&b]).unwrap();
    assert!(
        moved[0] >= fixed[0] + fixed[2]
            || moved[1] >= fixed[1] + fixed[3]
            || moved[0] + moved[2] <= fixed[0]
            || moved[1] + moved[3] <= fixed[1]
    );
    e.undo();
    assert_eq!(e.doc, before);
}

#[test]
fn conditional_fill_restores_original_style_and_reacts_to_data_changes() {
    let (mut e, a, _, _) = fixture();
    let shape = e.doc.diagram.as_ref().unwrap().shapes[&a].clone();
    let NodeKind::Path {
        style: original, ..
    } = e.doc.node(shape.body).unwrap().kind.clone()
    else {
        panic!()
    };
    let mut model = e.doc.diagram.as_deref().unwrap().clone();
    let shape = model.shapes.get_mut(&a).unwrap();
    shape.data.insert("status".into(), "done".into());
    shape.conditions.push(ConditionalFill {
        field: "status".into(),
        equals: "done".into(),
        color: [20, 180, 80, 255],
    });
    e.execute(Command::SetDiagram {
        diagram: Some(Arc::new(model)),
    })
    .unwrap();
    let body = e.doc.diagram.as_ref().unwrap().shapes[&a].body;
    let NodeKind::Path { style, .. } = &e.doc.node(body).unwrap().kind else {
        panic!()
    };
    assert_eq!(style.fill, Some([20, 180, 80, 255]));
    let mut model = e.doc.diagram.as_deref().unwrap().clone();
    model
        .shapes
        .get_mut(&a)
        .unwrap()
        .data
        .insert("status".into(), "review".into());
    e.execute(Command::SetDiagram {
        diagram: Some(Arc::new(model)),
    })
    .unwrap();
    let NodeKind::Path { style, .. } = &e.doc.node(body).unwrap().kind else {
        panic!()
    };
    assert_eq!(*style, original);
    e.undo();
    let NodeKind::Path { style, .. } = &e.doc.node(body).unwrap().kind else {
        panic!()
    };
    assert_eq!(style.fill, Some([20, 180, 80, 255]));
    let mut model = e.doc.diagram.as_deref().unwrap().clone();
    model.shapes.get_mut(&a).unwrap().conditions.clear();
    e.execute(Command::SetDiagram {
        diagram: Some(Arc::new(model)),
    })
    .unwrap();
    let NodeKind::Path { style, .. } = &e.doc.node(body).unwrap().kind else {
        panic!()
    };
    assert_eq!(*style, original);
    assert!(
        e.doc.diagram.as_ref().unwrap().shapes[&a]
            .unconditional_style
            .is_none()
    );
}

#[test]
fn bulk_builder_and_layout_preserve_graph_and_undo_atomically() {
    let mut builder = Builder::new(1800, 1200).unwrap();
    let ids = (0..80)
        .map(|i| {
            builder
                .add_shape(
                    ShapeKind::Process,
                    [
                        40. + (i % 10) as f64 * 170.,
                        40. + (i / 10) as f64 * 110.,
                        120.,
                        60.,
                    ],
                    &format!("Step {i}"),
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    for pair in ids.windows(2) {
        builder
            .connect(
                Endpoint {
                    shape: pair[0],
                    port: Port::Auto,
                },
                Endpoint {
                    shape: pair[1],
                    port: Port::Auto,
                },
                "",
                Routing::Orthogonal,
            )
            .unwrap();
    }
    let doc = builder.finish().unwrap();
    let mut editor = Editor::new(doc.clone(), None);
    arrange(&mut editor, Layout::Grid).unwrap();
    editor.doc.validate().unwrap();
    assert_eq!(editor.doc.diagram.as_ref().unwrap().edges.len(), 79);
    for e in editor.doc.diagram.as_ref().unwrap().edges.values() {
        let NodeKind::Path { path, .. } = &editor.doc.node(e.path).unwrap().kind else {
            panic!()
        };
        assert!(path.subpaths[0].anchors.len() >= 2);
    }
    editor.undo();
    assert_eq!(editor.doc, doc);
}

#[test]
fn quick_create_skips_occupied_space_and_is_one_undo_step() {
    let (mut e, a, _, _) = fixture();
    add_shape(&mut e, ShapeKind::Note, [220., 40., 120., 60.], "Occupied").unwrap();
    let original = e.doc.clone();
    let id = quick_create(&mut e, a, Port::East, ShapeKind::Decision).unwrap();
    let model = e.doc.diagram.as_ref().unwrap();
    let bounds = shape_bounds(&e.doc, &model.shapes[&id]).unwrap();
    assert_eq!(bounds, [400., 40., 120., 60.]);
    assert!(
        model
            .edges
            .values()
            .any(|edge| edge.source.shape == a && edge.target.shape == id)
    );
    e.undo();
    assert_eq!(e.doc, original);
    assert!(quick_create(&mut e, a, Port::Auto, ShapeKind::Process).is_err());
    assert_eq!(e.doc, original);
}

#[test]
fn arrows_follow_line_color_and_keep_both_endpoint_directions() {
    let (mut e, _, _, edge) = fixture();
    let mut model = e.doc.diagram.as_deref().unwrap().clone();
    model.edges.get_mut(&edge).unwrap().arrow_start = true;
    e.execute(Command::SetDiagram {
        diagram: Some(Arc::new(model)),
    })
    .unwrap();
    let edge = &e.doc.diagram.as_ref().unwrap().edges[&edge];
    let (path_id, arrow_id) = (edge.path, edge.arrow);
    let NodeKind::Path { path, style, .. } = &e.doc.node(path_id).unwrap().kind else {
        panic!()
    };
    let mut style = *style;
    style.stroke = Some([180, 30, 50, 200]);
    style.dash_count = 2;
    style.dash = [8., 4., 0., 0., 0., 0.];
    e.execute(Command::SetPath {
        id: path_id,
        path: path.clone(),
        style,
    })
    .unwrap();
    let NodeKind::Path { path, style, .. } = &e.doc.node(arrow_id).unwrap().kind else {
        panic!()
    };
    assert_eq!(path.subpaths.len(), 2);
    assert_eq!(style.fill, Some([180, 30, 50, 200]));
    assert_eq!(style.dash_count, 0);
}

#[test]
fn graph_edits_and_history_accounting_never_force_cpu_vector_pixels() {
    let (mut editor, a, _, _) = fixture();
    let unrendered = |doc: &Document| {
        doc.nodes.iter().all(|n| match &n.kind {
            NodeKind::Path { cache, .. } | NodeKind::Text { cache, .. } => !cache.is_rendered(),
            _ => true,
        })
    };
    assert!(unrendered(&editor.doc));
    for dx in [12., -3., 4.] {
        editor
            .execute(Command::TranslateNode { id: a, dx, dy: 0. })
            .unwrap();
    }
    assert!(editor.doc.buffers().is_empty());
    assert!(unrendered(&editor.doc));
    assert!(editor.history.steps().all(|step| unrendered(&step.before)));
    // A CPU-rendered cache still contributes its real allocations.
    let NodeKind::Text { cache, .. } = &editor
        .doc
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Text { .. }))
        .unwrap()
        .kind
    else {
        panic!()
    };
    cache.pixels();
    assert!(!editor.doc.buffers().is_empty());
    editor.undo();
    editor.redo();
    editor.doc.validate().unwrap();
}

#[test]
fn rounded_and_jumped_routes_transform_semantic_bends_without_decorative_anchors() {
    for (radius,jump) in [(6.,JumpStyle::None),(0.,JumpStyle::Arc),(6.,JumpStyle::Arc)] {
        let (mut e,a,b,id)=fixture();
        let mut model=e.doc.diagram.as_deref().unwrap().clone();
        let edge=model.edges.get_mut(&id).unwrap();edge.corner_radius=radius;edge.jump_style=jump;edge.waypoints=vec![(230.,70.),(230.,190.)];
        e.execute(Command::SetDiagram{diagram:Some(Arc::new(model))}).unwrap();
        let before=e.doc.clone();
        e.execute(Command::TransformNodes{ids:vec![a,b,id],transform:[1.,0.,0.,1.,50.,30.]}).unwrap();
        assert_eq!(e.doc.diagram.as_ref().unwrap().edges[&id].waypoints,vec![(280.,100.),(280.,220.)]);
        e.undo();assert_eq!(e.doc,before);
        e.execute(Command::TranslateNode{id,dx:25.,dy:12.}).unwrap();
        assert_eq!(e.doc.diagram.as_ref().unwrap().edges[&id].waypoints,vec![(255.,82.),(255.,202.)]);
        e.undo();assert_eq!(e.doc,before);
    }
}
