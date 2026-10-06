//! Fitting must not publish a partly cropped or resized projective drawing.
use super::*;
use crate::{Command, Document, Node, NodeKind, SmartPlacement};
use emulsion_raster::projective::Projective2;
use emulsion_raster::{Mask, Placement, Raster};
use std::sync::Arc;
fn projected_with_effect(width: u32, height: u32) -> Document {
    let mut doc = Document::new(width, height);
    let mut node = Node::smart(
        1,
        "Projected drawing",
        Arc::new(Raster::solid(2, 2, [1.; 4])),
        vec![],
        Placement::default(),
    );
    let NodeKind::Smart { placement, .. } = &mut node.kind else {
        panic!()
    };
    *placement = SmartPlacement::Projective(Projective2::IDENTITY);
    node.styles.push(crate::styles::LayerStyle::DropShadow {
        color: [0; 3],
        opacity: 100.,
        angle: 0.,
        distance: 0.,
        size: 1.,
    });
    doc.nodes.push(node);
    doc.next_id = 2;
    doc.selection = Some(Arc::new(Mask::white(width, height)));
    doc.validate().unwrap();
    doc
}
#[test]
fn fitting_propagates_resize_resource_refusal_after_safe_crop_and_preserves_input() {
    let doc = projected_with_effect(29_994, 2);
    let before = doc.clone();
    let rect = centred_frame(&doc, 29_995, 1);
    assert_ne!(rect, IRect::new(0, 0, doc.width as i32, doc.height as i32));
    let cropped = fit_document(&doc, rect, rect.w as u32, rect.h as u32).unwrap();
    assert_eq!(
        (cropped.width, cropped.height),
        (rect.w as u32, rect.h as u32)
    );
    let error = fit_document(&doc, rect, 29_995, 1).unwrap_err();
    assert!(
        error.to_string().contains("padded effect canvas"),
        "{error}"
    );
    assert_eq!(doc, before);
    assert!(Arc::ptr_eq(
        doc.selection.as_ref().unwrap(),
        before.selection.as_ref().unwrap()
    ));
    let success = fit_to_frame(&doc, 29_993, 1).unwrap();
    assert_eq!((success.width, success.height), (29_993, 1));
    let NodeKind::Smart { source: old, .. } = &doc.nodes[0].kind else {
        panic!()
    };
    let NodeKind::Smart { source: new, .. } = &success.nodes[0].kind else {
        panic!()
    };
    assert!(Arc::ptr_eq(old, new));
}
#[test]
fn failed_later_panel_fit_publishes_no_earlier_document_or_project_history() {
    use crate::project::{ProjectEditor, ProjectKind};
    let mut target =
        ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(29_995, 1)).unwrap();
    target
        .execute(Command::AddNode {
            node: Box::new(Node::new(
                0,
                "Redo target",
                NodeKind::Fill { rgba: [255; 4] },
            )),
            slot: crate::command::Slot::TOP,
        })
        .unwrap();
    assert!(target.undo());
    assert!(target.can_redo());
    let stamp = target.stamp();
    let snapshot = target.snapshot().unwrap();
    let before = target.doc.clone();
    let history = target.history.len();
    let revision = target.revision;
    let safe = Document::new(4, 4);
    let error = target
        .import_panels(
            None,
            vec![
                ("Fits first".into(), safe),
                ("Must refuse".into(), projected_with_effect(29_994, 1)),
            ],
        )
        .unwrap_err();
    assert!(error.contains("padded effect canvas"), "{error}");
    assert_eq!(target.stamp(), stamp);
    assert_eq!(target.doc, before);
    assert_eq!(target.history.len(), history);
    assert_eq!(target.revision, revision);
    assert!(target.can_redo());
    let after = target.snapshot().unwrap();
    assert_eq!(after.next_page_id, snapshot.next_page_id);
    assert_eq!(after.active, snapshot.active);
    assert_eq!(after.pages.len(), snapshot.pages.len());
    assert!(target.redo());
    assert_eq!(target.doc.nodes[0].name, "Redo target");
}
#[test]
fn invalid_fit_dimensions_are_errors_before_aspect_division() {
    let doc = Document::new(4, 4);
    assert!(fit_to_frame(&doc, 0, 4).is_err());
    assert!(fit_to_frame(&doc, 4, 0).is_err());
    assert!(fit_document(&doc, IRect::new(0, 0, 4, 4), 30_001, 1).is_err());
    assert!(fit_to_frame(&Document::new(0, 4), 4, 4).is_err());
}

#[test]
fn fitting_propagates_crop_refusal_before_following_resize() {
    let doc = projected_with_effect(4, 4);
    let before = doc.clone();
    // The requested final canvas is safe; the intervening crop enlargement
    // crosses the existing padded-effect limit and must stop the fit.
    let error = fit_document(&doc, IRect::new(0, 0, 30_000, 1), 4, 4).unwrap_err();
    assert!(
        error.to_string().contains("padded effect canvas"),
        "{error}"
    );
    assert_eq!(doc, before);
}
