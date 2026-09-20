//! Canvas Delete edits an unfinished selection before touching document pixels.
use super::*;
use core::prelude::v1::test;

fn view(cx: &mut TestAppContext) -> Entity<EditorView> {
    cx.update(|cx| {
        gpui_kit::init(cx);
        theme::install(cx);
        cx.set_global(crate::app_state::AppSettings(Default::default()));
        let mut doc = Document::new(64, 64);
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Photo",
                Arc::new(Raster::solid(64, 64, [0.2, 0.3, 0.4, 1.])),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        cx.new(|cx| EditorView::new(doc, None, None, None, "test".into(), cx))
    })
}

#[gpui_kit::test]
fn delete_removes_unfinished_selection_points_without_changing_document(cx: &mut TestAppContext) {
    let view = view(cx);
    view.update(cx, |this, cx| {
        for shape in [SelectShape::Polygon, SelectShape::Magnetic] {
            for existing_selection in [false, true] {
                this.editor.doc.selection =
                    existing_selection.then(|| Arc::new(select::rect(64, 64, 0., 0., 32., 32.)));
                this.set_select(shape, cx);
                this.tools.polygon = vec![(4., 4.), (20., 4.), (20., 20.)];
                let before = this.editor.doc.clone();
                let selected = this.selected;
                let history = this.editor.history.len();
                let revision = this.editor.revision;
                for remaining in (0..3).rev() {
                    if shape == SelectShape::Magnetic {
                        this.tools.magnetic_live = vec![(21., 20.), (22., 21.)];
                    }
                    this.delete_canvas_pixels(cx);
                    assert_eq!(this.tools.polygon.len(), remaining);
                    assert!(this.tools.magnetic_live.is_empty());
                    assert_eq!(
                        this.editor.doc, before,
                        "unfinished selection must protect both layers and selected pixels"
                    );
                    assert_eq!(this.selected, selected);
                    assert_eq!(this.editor.history.len(), history);
                    assert_eq!(this.editor.revision, revision);
                }
            }
        }
    });
}

#[gpui_kit::test]
fn delete_without_unfinished_selection_retains_layer_delete_behavior(cx: &mut TestAppContext) {
    let view = view(cx);
    view.update(cx, |this, cx| {
        this.set_select(SelectShape::Polygon, cx);
        let before = this.editor.doc.clone();
        this.delete_canvas_pixels(cx);
        assert!(this.editor.doc.nodes.is_empty());
        this.undo(cx);
        assert_eq!(this.editor.doc, before);
    });
}
