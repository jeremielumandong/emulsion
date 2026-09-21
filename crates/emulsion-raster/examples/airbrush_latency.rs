use emulsion_raster::{
    composite::{render_tile, tile_to_bgra8},
    library::library,
    paint::{Ink, Stroke},
    *,
};
use std::{sync::Arc, time::Instant};
fn node(id: u64, content: NodeContent) -> CompositeNode {
    CompositeNode {
        id,
        visible: true,
        opacity: 1.,
        blend: BlendMode::Normal,
        blending: Default::default(),
        mask: None,
        clip_to: None,
        content,
    }
}
fn main() {
    let b = library()
        .into_iter()
        .find(|p| p.name == "Airbrush")
        .unwrap()
        .brush;
    let base = Arc::new(Raster::transparent(1920, 1080));
    let mut current = (*base).clone();
    let mut stroke = Stroke::new(base, b, Ink::Color([0.1, 0.2, 0.3, 1.]), None);
    let mut paint = vec![];
    let mut display = vec![];
    for i in 0..180 {
        let begin = Instant::now();
        stroke.point(200. + i as f32 * 8., 450. + (i as f32 * 0.06).sin() * 180.);
        let (out, dirty) = stroke.render(&current);
        current = out;
        paint.push(begin.elapsed().as_secs_f64() * 1000.);
        let tree = CompositeTree {
            width: 1920,
            height: 1080,
            space: emulsion_raster::blend::BlendSpace::Linear,
            nodes: vec![
                node(1, NodeContent::Fill([1.; 4])),
                node(
                    2,
                    NodeContent::Pixels {
                        raster: Arc::new(current.clone()),
                        placement: Placement::default(),
                    },
                ),
            ],
        };
        let begin = Instant::now();
        if dirty.w > 0 && dirty.h > 0 {
            for ty in dirty.y / 256..=(dirty.bottom() - 1) / 256 {
                for tx in dirty.x / 256..=(dirty.right() - 1) / 256 {
                    let tile = render_tile(&tree, 0, TileCoord::new(tx, ty));
                    std::hint::black_box(tile_to_bgra8(
                        &tile,
                        (tx as i64 * 256, ty as i64 * 256),
                        (1920, 1080),
                        8,
                        200,
                        160,
                    ));
                }
            }
        }
        display.push(begin.elapsed().as_secs_f64() * 1000.);
    }
    for (name, values) in [("point+render", paint), ("composite+bgra", display)] {
        println!(
            "{name}: mean {:.2} max {:.2} ms",
            values.iter().sum::<f64>() / 180.,
            values.into_iter().fold(0., f64::max)
        );
    }
}
