use super::*;
use emulsion_raster::{
    Placement, Raster,
    vector::{GradientStop, PathPaint},
};
fn editor() -> Editor {
    Editor::new(Document::new(200, 200), None)
}
fn add(e: &mut Editor, path: Path) -> NodeId {
    e.execute(Command::AddNode {
        node: Box::new(Node::path(
            0,
            "Vector",
            Arc::new(path),
            PathStyle {
                fill: Some([100, 50, 0, 255]),
                stroke: Some([0, 0, 0, 255]),
                width: 4.,
                ..Default::default()
            },
            200,
            200,
        )),
        slot: Slot::TOP,
    })
    .unwrap()
    .unwrap()
}
#[test]
fn native_vector_point_join_split_and_boolean_are_atomic_and_editable() {
    let mut e = editor();
    let id = add(
        &mut e,
        Path::from_svg("M10 10 C20 0 30 0 40 10 L60 10 M60 10 L80 20").unwrap(),
    );
    let original = e.doc.clone();
    join(&mut e, id, 0, 1).unwrap();
    let NodeKind::Path { path, .. } = &e.doc.node(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(path.subpaths.len(), 1);
    assert_eq!(path.subpaths[0].anchors[0].h_out, (20., 0.));
    assert!(e.undo());
    assert_eq!(e.doc, original);
    split(&mut e, id, 0, 1).unwrap();
    assert!(e.undo());
    assert_eq!(e.doc, original);
    point(&mut e, id, 0, 0, (12., 13.), None, None, false).unwrap();
    let NodeKind::Path { path, .. } = &e.doc.node(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(path.subpaths[0].anchors[0].h_out, (22., 3.));
    assert!(e.undo());
    let a = add(&mut e, vector_geometry::rectangle(0., 0., 50., 50.));
    let b = add(&mut e, vector_geometry::rectangle(25., 0., 50., 50.));
    let before = e.doc.clone();
    combine(&mut e, &[a, b], Combine::Union).unwrap();
    assert!(e.doc.node(b).is_none());
    e.undo();
    assert_eq!(e.doc, before);
    e.execute(Command::SetLocked {
        id: b,
        locked: true,
    })
    .unwrap();
    let before = e.doc.clone();
    assert!(combine(&mut e, &[a, b], Combine::Component).is_err());
    assert_eq!(e.doc, before);
}
#[test]
fn gradients_outline_and_matching_keep_identity_and_original_source() {
    let mut e = editor();
    let a = add(&mut e, vector_geometry::rectangle(20., 20., 50., 50.));
    let b = add(&mut e, vector_geometry::rectangle(90., 20., 50., 50.));
    assert_eq!(matching(&e.doc, a, MatchProperty::Fill).unwrap().len(), 2);
    let stops = [
        GradientStop {
            offset: 0.,
            color: [255, 0, 0, 255],
        },
        GradientStop {
            offset: 0.4,
            color: [0, 255, 0, 255],
        },
        GradientStop {
            offset: 1.,
            color: [0, 0, 255, 255],
        },
    ];
    let before = e.doc.clone();
    gradient(&mut e, a, false, &stops, false, 20.).unwrap();
    assert_eq!(matching(&e.doc, a, MatchProperty::Fill).unwrap(), vec![a]);
    let fragment = crate::fragment::Fragment::capture(&e.doc, &[a]).unwrap();
    let NodeKind::Path { style, .. } = &fragment.nodes[0].kind else {
        panic!()
    };
    assert_eq!(style.fill_paint.gradient_stops([0; 4]).unwrap(), stops);
    assert!(e.undo());
    assert_eq!(e.doc, before);
    let source = e.doc.node(a).unwrap().clone();
    let outline = outline_stroke(&mut e, a).unwrap();
    assert_eq!(*e.doc.node(a).unwrap(), source);
    assert!(matches!(
        e.doc.node(outline).unwrap().kind,
        NodeKind::Path { .. }
    ));
    e.undo();
    assert_eq!(e.doc, before);
    assert!(e.doc.node(b).is_some());
    let bad = [stops[2], stops[0]];
    assert!(gradient(&mut e, a, false, &bad, false, 0.).is_err());
    assert_eq!(e.doc, before);
    let _: PathPaint = PathPaint::from_stops(&stops, true, 0.).unwrap();
}
#[test]
fn projective_and_mesh_warps_validate_folds_and_undo_exactly() {
    let mut e = editor();
    let id = add(&mut e, vector_geometry::rectangle(10., 10., 40., 40.));
    let before = e.doc.clone();
    let corners = [(10., 10.), (100., 20.), (70., 90.), (20., 80.)];
    perspective(&mut e, id, corners, 0.25).unwrap();
    let NodeKind::Path { path, .. } = &e.doc.node(id).unwrap().kind else {
        panic!()
    };
    for (a, b) in path.subpaths[0].anchors.iter().take(4).zip(corners) {
        assert!((a.p.0 - b.0).abs() < 1e-6 && (a.p.1 - b.1).abs() < 1e-6);
    }
    e.undo();
    assert_eq!(e.doc, before);
    assert!(
        mesh(
            &mut e,
            id,
            2,
            2,
            &[(0., 0.), (50., 50.), (0., 50.), (50., 0.)],
            0.25
        )
        .is_err()
    );
    assert_eq!(e.doc, before);
    skew(&mut e, id, 20., 0., (10., 10.)).unwrap();
    e.undo();
    assert_eq!(e.doc, before);
}
#[test]
fn bitmap_trace_preserves_holes_placement_source_and_undo() {
    let mut e = editor();
    let mut bytes = vec![255; 16 * 16 * 4];
    for y in 2..14 {
        for x in 2..14 {
            if !(6..10).contains(&x) || !(6..10).contains(&y) {
                let i = (y * 16 + x) * 4;
                bytes[i..i + 3].fill(0);
            }
        }
    }
    let id = e
        .execute(Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Bitmap",
                Arc::new(Raster::from_srgba8(16, 16, &bytes)),
                Placement::at(20., 30.),
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    let before = e.doc.clone();
    let preview = trace::preview(&e.doc, id, Default::default()).unwrap();
    assert_eq!(preview.subpaths.len(), 2);
    let raster = preview.rasterize(
        &PathStyle {
            fill: Some([0, 0, 0, 255]),
            stroke: None,
            ..Default::default()
        },
        200,
        200,
    );
    assert_eq!(raster.get(28, 38)[3], 0);
    assert_eq!(raster.get(23, 33)[3], 65535);
    let new = trace::apply(&mut e, id, Default::default()).unwrap();
    assert!(e.doc.node(new).is_some());
    assert_eq!(e.doc.node(id), before.node(id));
    e.undo();
    assert_eq!(e.doc, before);
    assert!(
        trace::preview(
            &e.doc,
            id,
            trace::Options {
                resolution: 10000,
                ..Default::default()
            }
        )
        .is_err()
    );
}
