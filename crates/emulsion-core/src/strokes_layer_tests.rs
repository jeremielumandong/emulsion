//! Vector stroke layers through commands, history, geometry and paste.
use crate::command::Slot;
use crate::{Command, Document, Editor, Node, NodeKind};
use emulsion_raster::strokes::{Stroke, StrokePoint, StrokeSet};
use std::sync::Arc;

fn set(x: f64) -> Arc<StrokeSet> {
    Arc::new(StrokeSet {
        strokes: vec![Stroke {
            points: vec![StrokePoint::new(x, 20.), StrokePoint::new(x + 30., 20.)],
            ..Stroke::new([0, 0, 0, 255], 4.)
        }],
        fills: Vec::new(),
    })
}

fn layer(editor: &mut Editor) -> u64 {
    let (w, h) = (editor.doc.width, editor.doc.height);
    editor
        .execute(Command::AddNode {
            node: Box::new(Node::strokes(0, "Pencil", set(10.), w, h)),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap()
}

fn strokes(editor: &Editor, id: u64) -> Arc<StrokeSet> {
    let NodeKind::Strokes { strokes, cache } = &editor.doc.node(id).unwrap().kind else {
        panic!("not a stroke layer")
    };
    assert_eq!(cache.size(), (editor.doc.width, editor.doc.height));
    strokes.clone()
}

#[test]
fn stroke_layers_draw_edit_and_undo() {
    let mut editor = Editor::new(Document::new(64, 40), None);
    let id = layer(&mut editor);
    let NodeKind::Strokes { cache, .. } = &editor.doc.node(id).unwrap().kind else {
        unreachable!()
    };
    assert!(cache.pixels().get(20, 20)[3] > 60000);
    editor
        .execute(Command::SetStrokes {
            id,
            strokes: set(30.),
        })
        .unwrap();
    assert_eq!(strokes(&editor, id).strokes[0].points[0].x, 30.);
    assert_eq!(
        Command::SetStrokes {
            id,
            strokes: set(0.)
        }
        .label(),
        "Edit drawing"
    );
    assert!(editor.undo());
    assert_eq!(strokes(&editor, id).strokes[0].points[0].x, 10.);
    // Invalid strokes and the wrong layer kind change nothing.
    let mut bad = (*set(0.)).clone();
    bad.strokes[0].points[0].x = f64::NAN;
    assert!(
        editor
            .execute(Command::SetStrokes {
                id,
                strokes: Arc::new(bad)
            })
            .is_err()
    );
    let fill = editor
        .execute(Command::AddNode {
            node: Box::new(Node::new(0, "Paper", NodeKind::Fill { rgba: [255; 4] })),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    assert!(
        editor
            .execute(Command::SetStrokes {
                id: fill,
                strokes: set(0.)
            })
            .is_err()
    );
    editor.doc.validate().unwrap();
}

#[test]
fn stroke_layers_move_scale_and_paste_with_their_document() {
    let mut editor = Editor::new(Document::new(64, 40), None);
    let id = layer(&mut editor);
    crate::geometry::translate_node(&mut editor.doc, id, 5., 2.).unwrap();
    let moved = strokes(&editor, id);
    assert_eq!(
        (moved.strokes[0].points[0].x, moved.strokes[0].points[0].y),
        (15., 22.)
    );
    editor
        .execute(Command::ImageSize {
            width: 128,
            height: 80,
        })
        .unwrap();
    let scaled = strokes(&editor, id);
    assert_eq!(scaled.strokes[0].width, 8.);
    assert_eq!(scaled.strokes[0].points[0].x, 30.);
    let fragment = crate::fragment::Fragment::capture(&editor.doc, &[id]).unwrap();
    let mut target = Editor::new(Document::new(200, 100), None);
    let pasted = fragment.paste(&mut target, Slot::TOP, (0., 0.)).unwrap();
    assert_eq!(strokes(&target, pasted[0]).strokes[0].points[0].x, 30.);
}

#[test]
fn documents_reject_invalid_stroke_layers() {
    let mut doc = Document::new(16, 16);
    let mut bad = (*set(0.)).clone();
    bad.strokes[0].width = -1.;
    doc.nodes
        .push(Node::strokes(1, "Bad", Arc::new(bad), 16, 16));
    assert!(doc.validate().is_err());
}
