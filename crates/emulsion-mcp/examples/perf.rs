//! Timings of the hot paths on a synthetic 24 MP document:
//! `cargo run --release -p emulsion-mcp --example perf`
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Node};
use emulsion_raster::composite::flatten;
use emulsion_raster::paint::{Brush, Ink, Stroke};
use emulsion_raster::{Adjustment, Placement, Raster, select};
use std::sync::Arc;
use std::time::Instant;

fn timed<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    println!("{name:<44} {:>8.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    r
}

fn main() {
    let (w, h) = (6000u32, 4000u32);
    let rgba: Vec<u8> = timed("synthesise 24 MP RGBA8", || {
        (0..h)
            .flat_map(|y| {
                (0..w).flat_map(move |x| {
                    [(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8, 255]
                })
            })
            .collect()
    });
    let base = timed("Raster::from_srgba8 (import)", || {
        Raster::from_srgba8(w, h, &rgba)
    });
    let base = Arc::new(base);
    let mut doc = Document::new(w, h);
    let add = |d: &mut Document, n: Node| {
        Command::AddNode {
            node: Box::new(n),
            slot: Slot::TOP,
        }
        .apply(d)
        .unwrap();
    };
    add(
        &mut doc,
        Node::raster(0, "photo", base.clone(), Placement::default()),
    );
    add(
        &mut doc,
        Node::adjust(
            0,
            Adjustment::BrightnessContrast {
                brightness: 0.1,
                contrast: 0.2,
            },
        ),
    );
    let mut top = Node::raster(
        0,
        "paint",
        Arc::new(Raster::transparent(w, h)),
        Placement::default(),
    );
    top.opacity = 0.8;
    add(&mut doc, top);
    let tree = timed("composite_tree() (3 nodes)", || doc.composite_tree());
    let flat = timed("flatten level 0 (24 MP)", || flatten(&tree, 0));
    timed("flatten level 2 (1.5 MP, screen-ish)", || flatten(&tree, 2));
    timed("to_srgba8 (export)", || flat.to_srgba8());
    timed("to_srgba16 (export 16-bit)", || flat.to_srgba16());
    timed("Document::clone", || doc.clone());
    let mut b = Brush::default();
    b.size = 80.0;
    let mut s = timed("brush stroke 300 pts size 80 (stamp)", || {
        let mut s = Stroke::new(base.clone(), b, Ink::Color([0.8, 0.2, 0.1, 1.0]), None);
        for i in 0..300 {
            let t = i as f32 / 299.0;
            s.point(200.0 + t * 5000.0, 2000.0 + (t * 20.0).sin() * 800.0);
        }
        s.finish();
        s
    });
    timed("brush stroke (render into layer)", || s.render(&base).0);
    let mut soft = Brush::default();
    soft.size = 80.0;
    soft.hardness = 0.2;
    soft.grain_strength = 0.6;
    let mut s2 = timed("textured soft stroke 300 pts (stamp)", || {
        let mut s = Stroke::new(base.clone(), soft, Ink::Color([0.1, 0.2, 0.8, 1.0]), None);
        for i in 0..300 {
            let t = i as f32 / 299.0;
            s.point(200.0 + t * 5000.0, 1000.0 + (t * 20.0).cos() * 600.0);
        }
        s.finish();
        s
    });
    timed("textured soft stroke (render)", || s2.render(&base).0);
    let sel = timed("select::rect + feather 12 px (full)", || {
        let m = select::rect(w, h, 1000.0, 1000.0, 3000.0, 2000.0);
        select::feather(&m, 12.0)
    });
    let _ = sel;
    timed("select::by_color contiguous (wand)", || {
        select::by_color(&rgba, w, h, 3000, 2000, 32, true)
    });
    timed("filter: Gaussian blur r=5 (24 MP)", || {
        emulsion_filters::apply_stack(
            &base,
            &[emulsion_filters::Filter::GaussianBlur { radius: 5.0 }],
        )
    });
    timed("write_rect 512×512 (stroke commit shape)", || {
        let px = vec![[65535u16, 0, 0, 65535]; 512 * 512];
        base.write_rect(emulsion_raster::IRect::new(1000, 1000, 512, 512), &px)
    });
}
