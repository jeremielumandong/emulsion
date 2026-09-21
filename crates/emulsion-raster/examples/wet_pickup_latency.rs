use emulsion_raster::{
    Raster,
    library::library,
    paint::{Ink, Stroke},
};
use std::{sync::Arc, time::Instant};
fn main() {
    for cached in [false, true] {
        for name in ["Flat bristle", "Round oil", "Impasto"] {
            let b = library()
                .into_iter()
                .find(|p| p.name == name)
                .unwrap()
                .brush;
            let base = Arc::new(Raster::solid(1920, 1080, [1.; 4]));
            let mut current = (*base).clone();
            let mut stroke = Stroke::new(base, b, Ink::Color([0.1, 0.2, 0.3, 1.]), None);
            let tree = emulsion_raster::CompositeTree {
                width: 1920,
                height: 1080,
                space: emulsion_raster::blend::BlendSpace::Linear,
                nodes: vec![emulsion_raster::CompositeNode {
                    id: 1,
                    visible: true,
                    opacity: 1.,
                    blend: emulsion_raster::BlendMode::Normal,
                    mask: None,
                    clip_to: None,
                    content: emulsion_raster::NodeContent::Fill([1.; 4]),
                }],
            };
            let tree = Arc::new(tree);
            if cached {
                let sampler = emulsion_raster::composite::PixelSampler::new(tree);
                stroke.set_backdrop(Arc::new(move |x, y| sampler.get(x, y)));
            } else {
                stroke.set_backdrop(Arc::new(move |x, y| {
                    emulsion_raster::composite::region(
                        &tree,
                        emulsion_raster::IRect::new(x, y, 1, 1),
                    )[0]
                }));
            }
            let mut points = vec![];
            let mut renders = vec![];
            for i in 0..30 {
                let x = 200. + i as f32 * 8.;
                let y = 450. + (i as f32 * 0.06).sin() * 180.;
                let start = Instant::now();
                stroke.point(x, y);
                points.push(start.elapsed().as_secs_f64() * 1000.);
                let start = Instant::now();
                current = stroke.render(&current).0;
                renders.push(start.elapsed().as_secs_f64() * 1000.);
            }
            let mean = |x: &[f64]| x.iter().sum::<f64>() / x.len() as f64;
            let max = |x: &[f64]| x.iter().copied().fold(0., f64::max);
            println!(
                "cached={cached} {name} {} px: point mean {:.2} max {:.2} ms; render mean {:.2} max {:.2} ms",
                b.size,
                mean(&points),
                max(&points),
                mean(&renders),
                max(&renders)
            );
        }
    }
}
