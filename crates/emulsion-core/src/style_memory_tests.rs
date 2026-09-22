//! Regression for large-layer duplication, styling, and deletion allocation lifetimes.
use crate::{
    Command, Document, Node,
    command::Slot,
    styles::{self, LayerStyle},
};
use emulsion_raster::{
    Placement, Raster,
    composite::{CompositeNode, NodeContent},
};
use std::sync::Arc;

fn overlay(node: &CompositeNode) -> Arc<Raster> {
    let NodeContent::StyledGroup { children, .. } = &node.content else {
        panic!("styled layer");
    };
    let NodeContent::Pixels { raster, .. } = &children.last().unwrap().content else {
        panic!("overlay pixels");
    };
    raster.clone()
}

#[test]
fn large_duplicate_overlay_delete_reuses_effect_pixels_and_releases_unretained_tree() {
    // Bigger than the warm-cache byte limit: reuse must come from the live
    // render tree, not from retaining another full-resolution cached image.
    let mut doc = Document::new(4096, 2048);
    let pixels = Arc::new(Raster::from_fn(4096, 2048, [0; 4], |x, y| {
        if x % 31 == 0 || y % 29 == 0 {
            [10000, 20000, 30000, 65535]
        } else {
            [0; 4]
        }
    }));
    let id = Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Texture",
            pixels.clone(),
            Placement::default(),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap()
    .unwrap();
    Command::SetStyles {
        id,
        styles: vec![LayerStyle::ColorOverlay {
            color: [230, 50, 90],
            opacity: 65.,
        }],
    }
    .apply(&mut doc)
    .unwrap();
    let started = std::time::Instant::now();
    let first_tree = doc.composite_tree();
    let cold = started.elapsed();
    let effect = overlay(&first_tree.nodes[0]);
    let weak = Arc::downgrade(&effect);
    let repeated = std::time::Instant::now();
    for _ in 0..12 {
        let copy = Command::DuplicateNode { id }
            .apply(&mut doc)
            .unwrap()
            .unwrap();
        let tree = doc.composite_tree();
        assert_eq!(tree.nodes.len(), 2);
        assert!(Arc::ptr_eq(&effect, &overlay(&tree.nodes[0])));
        assert!(
            Arc::ptr_eq(&effect, &overlay(&tree.nodes[1])),
            "duplicate should reuse identical effect pixels"
        );
        let crate::NodeKind::Raster { raster, .. } = &doc.node(copy).unwrap().kind else {
            panic!("raster");
        };
        assert!(Arc::ptr_eq(raster, &pixels));
        Command::RemoveNode { id: copy }.apply(&mut doc).unwrap();
        drop(tree);
        assert!(Arc::ptr_eq(
            &effect,
            &overlay(&doc.composite_tree().nodes[0])
        ));
    }
    eprintln!(
        "4096×2048 overlay: first render {cold:?}; 12 duplicate/delete cycles {:?}",
        repeated.elapsed()
    );
    drop(first_tree);
    drop(effect);
    assert!(
        weak.upgrade().is_none(),
        "no retained tree or cache keeps the deleted overlay allocation alive"
    );
    styles::evict_for_test(&doc, doc.node(id).unwrap());
}

#[test]
fn integer_translation_reuses_pixels_across_canvas_edge_and_retains_full_shadow() {
    let mut doc = Document::new(32, 32);
    let mut node = Node::raster(
        1,
        "full image",
        Arc::new(Raster::empty(32, 32, [30000, 10000, 0, 65535])),
        Placement::default(),
    );
    node.styles = vec![
        LayerStyle::ColorOverlay {
            color: [0, 255, 0],
            opacity: 100.,
        },
        LayerStyle::DropShadow {
            color: [0; 3],
            opacity: 100.,
            angle: 180.,
            distance: 6.,
            size: 0.,
        },
    ];
    doc.nodes.push(node);
    let initial = styles::render(&doc, &doc.nodes[0]).unwrap();
    if let crate::NodeKind::Raster { placement, .. } = &mut doc.nodes[0].kind {
        placement.x = -16.;
        placement.y = 5.;
    }
    let moved = styles::render(&doc, &doc.nodes[0]).unwrap();
    assert!(Arc::ptr_eq(
        &initial.above[0].raster,
        &moved.above[0].raster
    ));
    assert!(Arc::ptr_eq(
        &initial.below[0].raster,
        &moved.below[0].raster
    ));
    assert_eq!(moved.above[0].rect.x, initial.above[0].rect.x - 16);
    assert_eq!(moved.above[0].rect.y, initial.above[0].rect.y + 5);
    let raster = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
    assert_eq!(raster.get(4, 10), [0, 65535, 0, 65535]);
    assert!(
        raster.get(20, 10)[3] > 0,
        "shadow from full source survives crossing canvas edge"
    );
    let pixels = moved.above[0].raster.clone();
    if let crate::NodeKind::Raster { placement, .. } = &mut doc.nodes[0].kind {
        placement.x = -48.;
    }
    let offcanvas = styles::render(&doc, &doc.nodes[0]).unwrap();
    assert!(Arc::ptr_eq(&pixels, &offcanvas.above[0].raster));
}

#[test]
fn translation_reuse_preserves_phase_and_rejects_changed_mask_scale_and_pattern() {
    let mut doc = Document::new(64, 64);
    let mut node = Node::raster(
        1,
        "image",
        Arc::new(Raster::empty(32, 32, [10000, 20000, 30000, 65535])),
        Placement::at(5.25, 6.),
    );
    node.mask = Some(Arc::new(emulsion_raster::Mask::from_fn(
        32,
        32,
        0,
        |x, _| if x < 16 { 255 } else { 0 },
    )));
    node.styles = vec![LayerStyle::ColorOverlay {
        color: [255, 0, 0],
        opacity: 100.,
    }];
    doc.nodes.push(node);
    let first = styles::render(&doc, &doc.nodes[0]).unwrap();
    if let crate::NodeKind::Raster { placement, .. } = &mut doc.nodes[0].kind {
        placement.x += 4.;
    }
    let integer = styles::render(&doc, &doc.nodes[0]).unwrap();
    assert!(Arc::ptr_eq(
        &first.above[0].raster,
        &integer.above[0].raster
    ));
    if let crate::NodeKind::Raster { placement, .. } = &mut doc.nodes[0].kind {
        placement.x += 0.25;
    }
    let fractional = styles::render(&doc, &doc.nodes[0]).unwrap();
    assert!(!Arc::ptr_eq(
        &integer.above[0].raster,
        &fractional.above[0].raster
    ));
    doc.nodes[0].mask_transform[4] += 3.;
    let mask = styles::render(&doc, &doc.nodes[0]).unwrap();
    assert!(!Arc::ptr_eq(
        &mask.above[0].raster,
        &fractional.above[0].raster
    ));
    if let crate::NodeKind::Raster { placement, .. } = &mut doc.nodes[0].kind {
        placement.scale_x = 1.5;
    }
    let scaled = styles::render(&doc, &doc.nodes[0]).unwrap();
    assert!(!Arc::ptr_eq(&scaled.above[0].raster, &mask.above[0].raster));
    doc.nodes[0].styles = vec![LayerStyle::PatternOverlay {
        from: [0; 3],
        to: [255; 3],
        opacity: 100.,
        scale: 3.,
        angle: 0.,
    }];
    let pattern = styles::render(&doc, &doc.nodes[0]).unwrap();
    if let crate::NodeKind::Raster { placement, .. } = &mut doc.nodes[0].kind {
        placement.x += 1.;
    }
    let pattern_moved = styles::render(&doc, &doc.nodes[0]).unwrap();
    assert!(!Arc::ptr_eq(
        &pattern.above[0].raster,
        &pattern_moved.above[0].raster
    ));
}

#[test]
fn large_image_integer_drag_reuses_the_live_effect_allocation() {
    let mut doc = Document::new(4096, 2048);
    let mut node = Node::raster(
        1,
        "large image",
        Arc::new(Raster::empty(4096, 2048, [20000, 10000, 30000, 65535])),
        Placement::default(),
    );
    node.styles = vec![LayerStyle::ColorOverlay {
        color: [255, 0, 0],
        opacity: 75.,
    }];
    doc.nodes.push(node);
    let start = std::time::Instant::now();
    let first = styles::render(&doc, &doc.nodes[0]).unwrap();
    let cold = start.elapsed();
    let start = std::time::Instant::now();
    for step in 1..=12 {
        if let crate::NodeKind::Raster { placement, .. } = &mut doc.nodes[0].kind {
            placement.x = -(step as f64) * 20.;
            placement.y = step as f64 * 3.;
        }
        let moved = styles::render(&doc, &doc.nodes[0]).unwrap();
        assert!(Arc::ptr_eq(&first.above[0].raster, &moved.above[0].raster));
    }
    eprintln!(
        "4096x2048 overlay cold {:?}; 12 integer moves {:?}",
        cold,
        start.elapsed()
    );
}

#[test]
fn rotated_scaled_image_and_smart_integer_moves_share_pixels() {
    for smart in [false, true] {
        let mut doc = Document::new(64, 64);
        let mut node = Node::raster(
            1,
            "image",
            Arc::new(Raster::empty(24, 18, [20000, 10000, 0, 65535])),
            Placement::default(),
        );
        node.styles = vec![LayerStyle::ColorOverlay {
            color: [0, 255, 0],
            opacity: 100.,
        }];
        doc.nodes.push(node);
        if smart {
            Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
        }
        match &mut doc.nodes[0].kind {
            crate::NodeKind::Raster { placement, .. }
            | crate::NodeKind::Smart { placement, .. } => {
                placement.rotation = 27.;
                placement.scale_x = 1.3;
                placement.scale_y = 0.8;
                placement.x = 3.25;
                placement.y = 4.5;
            }
            _ => panic!(),
        }
        let first = styles::render(&doc, &doc.nodes[0]).unwrap();
        for delta in [-45., 79.] {
            match &mut doc.nodes[0].kind {
                crate::NodeKind::Raster { placement, .. }
                | crate::NodeKind::Smart { placement, .. } => {
                    placement.x += delta;
                    placement.y -= delta;
                }
                _ => panic!(),
            }
            let moved = styles::render(&doc, &doc.nodes[0]).unwrap();
            assert!(
                Arc::ptr_eq(&first.above[0].raster, &moved.above[0].raster),
                "smart={smart} delta={delta}"
            );
        }
    }
}
