//! Isolated test process: installing an accelerator must not alter unit tests'
//! global compositor state or let an unsupported profile reach acceleration.
use emulsion_raster::blend::{BlendMode, BlendSpace};
use emulsion_raster::composite::{
    BlendingOptions, CompositeNode, CompositeTree, NodeContent, TileAccelerator,
    install_accelerator, render_tile, render_tile_cpu,
};
use emulsion_raster::geom::TileCoord;
use emulsion_raster::tile::{FTile, TILE_PX};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Sentinel(Arc<AtomicUsize>);
impl TileAccelerator for Sentinel {
    fn render_tile(&self, _: &CompositeTree, _: u32, _: TileCoord) -> Option<FTile> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Some(vec![[1.0, 0.0, 0.0, 1.0]; TILE_PX])
    }
}

#[test]
fn new_profile_never_consults_an_installed_accelerator() {
    let calls = Arc::new(AtomicUsize::new(0));
    install_accelerator(Arc::new(Sentinel(calls.clone())));
    let mut tree = CompositeTree {
        width: 1,
        height: 1,
        space: BlendSpace::PhotoshopSrgbV1,
        knockout_background: None,
        nodes: vec![CompositeNode {
            id: 1,
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            blending: BlendingOptions::default(),
            mask: None,
            clip_to: None,
            clip_rect: None,
            content: NodeContent::Fill([0.0, 1.0, 0.0, 1.0]),
        }],
    };
    let tile = TileCoord::new(0, 0);
    assert_eq!(render_tile(&tree, 0, tile), render_tile_cpu(&tree, 0, tile));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    tree.space = BlendSpace::Linear;
    assert_eq!(render_tile(&tree, 0, tile)[0], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    tree.space = BlendSpace::PhotoshopSrgbV1;
    assert_eq!(render_tile(&tree, 0, tile)[0], [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
