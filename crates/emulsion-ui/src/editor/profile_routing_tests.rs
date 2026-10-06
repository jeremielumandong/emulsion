use super::*;
use core::prelude::v1::test;
use emulsion_core::Node;
use emulsion_raster::Placement;
use emulsion_raster::blend::BlendSpace;

fn document() -> Document {
    let mut doc = Document::new(16, 16);
    doc.nodes.push(Node::raster(
        1,
        "Background",
        Arc::new(Raster::solid(16, 16, [1.0, 0.0, 0.0, 1.0])),
        Placement::default(),
    ));
    doc.nodes.push(Node::raster(
        2,
        "Blue",
        Arc::new(Raster::solid(16, 16, [0.0, 0.0, 0.5, 0.5])),
        Placement::default(),
    ));
    doc.next_id = 3;
    doc
}

#[gpui_kit::test]
fn profile_edit_undo_redo_and_before_view_keep_distinct_render_trees(cx: &mut TestAppContext) {
    let view = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::install(cx);
        cx.set_global(crate::app_state::AppSettings(Default::default()));
        cx.new(|cx| EditorView::new(document(), None, None, None, "profiles".into(), cx))
    });
    view.update(cx, |view, cx| {
        view.sync_trees(cx);
        let initial = view.render_gen;
        view.compare = 0.5;
        view.execute(
            Command::SetBlendSpace {
                space: BlendSpace::PhotoshopSrgbV1,
            },
            cx,
        );
        view.sync_trees(cx);
        let photoshop = view.render_gen;
        assert!(photoshop > initial);
        assert_eq!(view.tree.space, BlendSpace::PhotoshopSrgbV1);
        assert_eq!(view.before_tree.as_ref().unwrap().space, BlendSpace::Linear);
        assert!(view.before_active());
        let current = emulsion_raster::composite::render_tile_cpu(
            &view.tree,
            0,
            emulsion_raster::TileCoord::new(0, 0),
        );
        let before = emulsion_raster::composite::render_tile_cpu(
            view.before_tree.as_ref().unwrap(),
            0,
            emulsion_raster::TileCoord::new(0, 0),
        );
        assert_ne!(current[0], before[0]);
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        assert!(matches!(
            *view.gpu_canvas.borrow(),
            crate::viewport_gpu::Status::Refused { retry_at: None, .. }
        ));
        view.undo(cx);
        view.sync_trees(cx);
        assert_eq!(view.tree.space, BlendSpace::Linear);
        assert!(view.render_gen > photoshop);
        assert!(!view.before_active());
        view.redo(cx);
        view.sync_trees(cx);
        assert_eq!(view.tree.space, BlendSpace::PhotoshopSrgbV1);
        assert_eq!(view.before_tree.as_ref().unwrap().space, BlendSpace::Linear);
        view.execute(
            Command::SetBlendSpace {
                space: BlendSpace::Srgb,
            },
            cx,
        );
        view.sync_trees(cx);
        assert_eq!(view.tree.space, BlendSpace::Srgb);
        assert_eq!(view.before_tree.as_ref().unwrap().space, BlendSpace::Linear);
    });
}

#[test]
fn flatten_white_keeps_scoped_photoshop_appearance() {
    let mut doc = document();
    doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    doc.psd_background = Some(1);
    // Root Deep reveals the explicit red stop, even after a green sibling.
    let mut green = Node::raster(
        3,
        "Ordinary backdrop",
        Arc::new(Raster::solid(16, 16, [0.0, 1.0, 0.0, 1.0])),
        Placement::default(),
    );
    green.parent = None;
    doc.nodes.insert(1, green);
    doc.nodes[2].blending.knockout = emulsion_raster::composite::Knockout::Deep;
    let expected = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
    let actual = layer_menu::flatten_on_white(&doc).unwrap();
    assert_eq!(actual.get(0, 0), expected.get(0, 0));
    assert_eq!(actual.get(0, 0)[3], u16::MAX);
    assert_eq!(doc.psd_background, Some(1));
}
