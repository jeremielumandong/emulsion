//! Gap-closing bucket, cutter and distortion through commands and Undo.
use crate::bucket::{BucketOptions, bucket_fill};
use crate::command::Slot;
use crate::cutter::cut_to_new_layer;
use crate::distort::{DistortKind, Distortion, distort_area, distort_command};
use crate::drawing_guides::GuideKind;
use crate::{Command, Document, Editor, Node, NodeKind};
use emulsion_raster::gap_fill::FillMode;
use emulsion_raster::strokes::{Stroke, StrokePoint, StrokeSet};
use emulsion_raster::{IRect, Placement, Raster, color, select};
use std::sync::Arc;

const BLACK: [u8; 4] = [0, 0, 0, 255];
const RED: [u8; 4] = [255, 0, 0, 255];

/// A 60×60 square outline from 10 to 50 drawn as pencil strokes, with a
/// 4 px opening in its top side.
fn outline() -> StrokeSet {
    let pt = |x: f64, y: f64| StrokePoint::new(x, y);
    let stroke = |pts: Vec<StrokePoint>| Stroke {
        points: pts,
        ..Stroke::new(BLACK, 2.0)
    };
    StrokeSet {
        strokes: vec![
            stroke(vec![
                pt(28.0, 11.0),
                pt(11.0, 11.0),
                pt(11.0, 49.0),
                pt(49.0, 49.0),
            ]),
            stroke(vec![pt(49.0, 49.0), pt(49.0, 11.0), pt(32.0, 11.0)]),
        ],
        fills: Vec::new(),
    }
}

fn editor_with(node: Node) -> (Editor, u64) {
    let mut e = Editor::new(Document::new(60, 60), None);
    let id = e
        .execute(Command::AddNode {
            node: Box::new(node),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    (e, id)
}

fn alpha(e: &Editor, id: u64, x: u32, y: u32) -> u16 {
    match &e.doc.node(id).unwrap().kind {
        NodeKind::Raster { raster, .. } => raster.get(x, y)[3],
        NodeKind::Strokes { strokes, .. } => strokes.rasterize(60, 60).get(x, y)[3],
        _ => panic!("not a layer"),
    }
}

#[test]
fn vector_gap_fill_stays_vector_in_one_undo_step() {
    let (mut e, id) = editor_with(Node::strokes(0, "Ink", Arc::new(outline()), 60, 60));
    let leaky = BucketOptions::default();
    let cmd = bucket_fill(&e.doc, id, (30.0, 30.0), RED, &leaky)
        .unwrap()
        .unwrap();
    let Command::SetStrokes { strokes, .. } = &cmd else {
        panic!("a vector layer fills with a vector fill")
    };
    let rendered = strokes.rasterize(60, 60);
    assert!(
        rendered.get(2, 2)[3] > 0,
        "without gap closing it leaks outside"
    );

    let closing = BucketOptions {
        gap: 6,
        ..Default::default()
    };
    let before = e.history.len();
    e.execute(
        bucket_fill(&e.doc, id, (30.0, 30.0), RED, &closing)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(e.history.len(), before + 1);
    let NodeKind::Strokes { strokes, .. } = &e.doc.node(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(strokes.fills.len(), 1);
    assert_eq!(strokes.fills[0].color, RED);
    assert!(
        strokes.fills[0].outlines.iter().all(|o| o.len() < 60),
        "traced and simplified"
    );
    assert_eq!(alpha(&e, id, 2, 2), 0, "the gap holds");
    assert!(alpha(&e, id, 30, 30) > 60000);
    e.undo();
    let NodeKind::Strokes { strokes, .. } = &e.doc.node(id).unwrap().kind else {
        panic!()
    };
    assert!(strokes.fills.is_empty());
}

#[test]
fn bitmap_fill_modes_and_layer_sampling() {
    // Left half opaque blue on the layer; a red background below it.
    let mut px = vec![[0u16; 4]; 60 * 60];
    for (i, p) in px.iter_mut().enumerate() {
        if i % 60 < 30 {
            *p = color::f_to_px([0.0, 0.0, 1.0, 1.0]);
        }
    }
    let layer = Raster::from_pixels(60, 60, [0; 4], &px);
    let (mut e, id) = editor_with(Node::raster(
        0,
        "Paint",
        Arc::new(layer),
        Placement::default(),
    ));
    let bg = e
        .execute(Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Back",
                Arc::new(Raster::solid(60, 60, [1.0, 0.0, 0.0, 1.0])),
                Placement::default(),
            )),
            slot: Slot {
                parent: None,
                index: 0,
            },
        })
        .unwrap()
        .unwrap();
    assert_ne!(bg, id);
    let green = [0, 255, 0, 255];
    // Sampling all layers sees red on the right; the layer alone sees
    // transparency, and the same area either way here.
    for sample_all in [true, false] {
        let o = BucketOptions {
            sample_all,
            ..Default::default()
        };
        let Some(Command::ReplacePixels { raster, .. }) =
            bucket_fill(&e.doc, id, (45.0, 30.0), green, &o).unwrap()
        else {
            panic!()
        };
        assert!(raster.get(45, 30)[1] > 60000);
        assert_eq!(
            raster.get(10, 30)[1],
            0,
            "the blue half is a different colour"
        );
    }
    // Paint behind fills under the blue too when the area is everything.
    let behind = BucketOptions {
        mode: FillMode::Behind,
        contiguous: false,
        tolerance: 255,
        ..Default::default()
    };
    e.execute(
        bucket_fill(&e.doc, id, (45.0, 30.0), green, &behind)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let NodeKind::Raster { raster, .. } = &e.doc.node(id).unwrap().kind else {
        panic!()
    };
    assert!(raster.get(45, 30)[1] > 60000, "transparent part filled");
    assert_eq!(raster.get(10, 30)[1], 0, "blue stays on top");
    // Unpainted leaves painted pixels alone.
    e.undo();
    let unpainted = BucketOptions {
        mode: FillMode::Unpainted,
        ..behind
    };
    e.execute(
        bucket_fill(&e.doc, id, (45.0, 30.0), green, &unpainted)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let NodeKind::Raster { raster, .. } = &e.doc.node(id).unwrap().kind else {
        panic!()
    };
    assert!(raster.get(45, 30)[1] > 60000);
    assert!(raster.get(10, 30)[2] > 60000 && raster.get(10, 30)[1] == 0);
    assert!(bucket_fill(&e.doc, id, (-1.0, 3.0), green, &unpainted).is_err());
}

#[test]
fn cutter_lifts_pixels_and_strokes_into_a_new_layer() {
    let (mut e, id) = editor_with(Node::raster(
        0,
        "Paint",
        Arc::new(Raster::solid(40, 40, [0.0, 0.0, 1.0, 1.0])),
        Placement::at(10.0, 10.0),
    ));
    let lasso = select::polygon(
        60,
        60,
        &[(20.0f32, 20.0f32), (40.0, 20.0), (40.0, 40.0), (20.0, 40.0)],
    );
    for copy in [false, true] {
        let before = e.history.len();
        let commands = cut_to_new_layer(&e.doc, id, &lasso, copy).unwrap();
        e.begin("Cut");
        let mut new = None;
        for c in commands {
            if let Some(n) = e.execute(c).unwrap() {
                new = Some(n);
            }
        }
        e.end();
        assert_eq!(e.history.len(), before + 1, "one Undo step");
        let new = new.expect("a new layer");
        // The new layer sits just above, at the same place.
        let order = e.doc.children(None);
        assert_eq!(
            order.iter().position(|n| *n == new),
            Some(order.iter().position(|n| *n == id).unwrap() + 1)
        );
        let NodeKind::Raster { placement, .. } = &e.doc.node(new).unwrap().kind else {
            panic!()
        };
        assert_eq!(*placement, Placement::at(10.0, 10.0));
        assert!(
            alpha(&e, new, 20, 20) > 60000,
            "selected pixels lifted (layer 30,30)"
        );
        assert_eq!(alpha(&e, new, 2, 2), 0);
        assert_eq!(alpha(&e, id, 20, 20) == 0, !copy, "cut clears, copy keeps");
        assert!(alpha(&e, id, 2, 2) > 60000);
        e.undo();
        assert_eq!(e.doc.node(new), None);
        assert!(alpha(&e, id, 20, 20) > 60000);
    }
    // Vector strokes split along the lasso.
    let (mut e, id) = editor_with(Node::strokes(0, "Ink", Arc::new(outline()), 60, 60));
    let band = select::rect(60, 60, 0.0, 0.0, 30.0, 60.0);
    e.begin("Cut");
    for c in cut_to_new_layer(&e.doc, id, &band, false).unwrap() {
        e.execute(c).unwrap();
    }
    e.end();
    let new = *e.doc.children(None).last().unwrap();
    let xs = |id| match &e.doc.node(id).unwrap().kind {
        NodeKind::Strokes { strokes, .. } => strokes
            .strokes
            .iter()
            .flat_map(|s| s.points.iter().map(|p| p.x))
            .collect::<Vec<_>>(),
        _ => panic!(),
    };
    assert!(xs(new).iter().all(|x| *x <= 30.01));
    assert!(xs(id).iter().all(|x| *x >= 29.99));
    assert!(
        cut_to_new_layer(&e.doc, id, &select::rect(60, 60, 0.0, 0.0, 5.0, 5.0), false).is_err()
    );
}

#[test]
fn distortion_maps_corners_and_commits_in_place() {
    let source = IRect::new(10, 10, 20, 20);
    let mut d = Distortion::new(source, DistortKind::Perspective);
    assert!(d.is_identity());
    assert_eq!(d.grid.len(), 4);
    let p = d.map((15.0, 25.0)).unwrap();
    assert!((p.0 - 15.0).abs() < 1e-9 && (p.1 - 25.0).abs() < 1e-9);
    // Pull the top-right corner out; the centre (where the diagonals
    // cross) moves towards it.
    d.move_handle(1, (40.0, 5.0));
    let c = d.map((20.0, 20.0)).unwrap();
    assert!(
        (c.0 - 230.0 / 11.0).abs() < 1e-6 && (c.1 - 230.0 / 11.0).abs() < 1e-6,
        "{c:?}"
    );
    let corner = d.map((30.0, 10.0)).unwrap();
    assert!((corner.0 - 40.0).abs() < 1e-9 && (corner.1 - 5.0).abs() < 1e-9);
    let env = Distortion::new(source, DistortKind::Envelope(3));
    assert_eq!(env.grid.len(), 16);
    assert_eq!(env.lines().len(), 8);
    assert_eq!(
        Distortion::new(source, DistortKind::Envelope(99)).cells(),
        8
    );

    let (mut e, id) = editor_with(Node::raster(
        0,
        "Paint",
        Arc::new(Raster::solid(60, 60, [0.0; 4]).write_rect(
            IRect::new(10, 10, 20, 20),
            &vec![color::f_to_px([1.0, 0.0, 0.0, 1.0]); 400],
        )),
        Placement::default(),
    ));
    assert_eq!(distort_area(&e.doc, id, None), Some(source));
    // Move the whole square right by 20 with an envelope.
    let mut env = Distortion::new(source, DistortKind::Envelope(3));
    for q in &mut env.grid {
        q.0 += 20.0;
    }
    let before = e.history.len();
    e.begin("Distort");
    let cmd = distort_command(&e.doc, id, None, &env).unwrap();
    e.preview(cmd.clone()).unwrap();
    e.execute(cmd).unwrap();
    e.end();
    assert_eq!(e.history.len(), before + 1);
    assert_eq!(alpha(&e, id, 15, 15), 0, "moved away");
    assert!(alpha(&e, id, 40, 20) > 60000, "moved here");
    // A selection distorts only its pixels.
    e.undo();
    let left = select::rect(60, 60, 10.0, 10.0, 10.0, 20.0);
    let area = distort_area(&e.doc, id, Some(&left)).unwrap();
    let mut d = Distortion::new(area, DistortKind::Perspective);
    for q in &mut d.grid {
        q.1 += 30.0;
    }
    e.execute(distort_command(&e.doc, id, Some(&left), &d).unwrap())
        .unwrap();
    assert_eq!(alpha(&e, id, 12, 15), 0);
    assert!(alpha(&e, id, 25, 15) > 60000, "unselected half stays");
    assert!(alpha(&e, id, 12, 45) > 60000);
    // Vector points follow the same mapping.
    let (e2, vid) = editor_with(Node::strokes(0, "Ink", Arc::new(outline()), 60, 60));
    let area = distort_area(&e2.doc, vid, None).unwrap();
    let mut d = Distortion::new(area, DistortKind::Perspective);
    for q in &mut d.grid {
        q.0 += 5.0;
    }
    let Command::SetStrokes { strokes, .. } = distort_command(&e2.doc, vid, None, &d).unwrap()
    else {
        panic!()
    };
    assert!((strokes.strokes[0].points[1].x - 16.0).abs() < 1e-6);
}

#[test]
fn drawing_guides_are_kept_by_undo() {
    let (mut e, id) = editor_with(Node::strokes(0, "Ink", Arc::new(outline()), 60, 60));
    e.doc
        .drawing_guides
        .set_primary(GuideKind::Grid { size: 10.0 });
    e.execute(Command::Rename {
        id,
        name: "Renamed".into(),
    })
    .unwrap();
    e.doc.drawing_guides.save_set("Grid").unwrap();
    e.undo();
    assert_eq!(e.doc.drawing_guides.sets.len(), 1);
    e.redo();
    assert_eq!(
        e.doc.drawing_guides.primary(),
        GuideKind::Grid { size: 10.0 }
    );
}
