//! Build a 6000×4000 multi-node document, save it as ORA, and time the
//! renders the viewport performs.
//!
//! cargo run --release -p emulsion-io --example sample -- OUT.ora

use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Node, NodeKind};
use emulsion_raster::composite::{flatten, render_tile, tiles_at};
use emulsion_raster::{Adjustment, BlendMode, Mask, Placement, Raster, TileCoord};
use rayon::prelude::*;
use std::sync::Arc;
use std::time::Instant;

fn add(d: &mut Document, n: Node) -> u64 {
    Command::AddNode {
        node: Box::new(n),
        slot: Slot::TOP,
    }
    .apply(d)
    .unwrap()
    .unwrap()
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "sample.ora".into());
    let (w, h) = (6000u32, 4000u32);
    let t = Instant::now();
    let mut d = Document::new(w, h);

    // Sky-to-ground gradient from 8-bit values, like an imported photo.
    let sky: Vec<u8> = (0..h)
        .into_par_iter()
        .flat_map_iter(|y| {
            let t = y as f32 / h as f32;
            (0..w).flat_map(move |x| {
                let n = ((x * 7 + y * 13) % 17) as f32; // a little grain
                let r = (40.0 + 180.0 * t + n) as u8;
                let g = (90.0 + 120.0 * t + n) as u8;
                let b = (200.0 - 120.0 * t + n) as u8;
                [r, g, b, 255]
            })
        })
        .collect();
    add(
        &mut d,
        Node::raster(
            0,
            "Base · sky",
            Arc::new(Raster::from_srgba8(w, h, &sky)),
            Placement::default(),
        ),
    );

    // A soft disc, masked on its right half.
    let r = 1100.0f32;
    let disc: Vec<u8> = (0..2400u32)
        .flat_map(|y| {
            (0..2400u32).flat_map(move |x| {
                let (dx, dy) = (x as f32 - 1200.0, y as f32 - 1200.0);
                let a = ((r - (dx * dx + dy * dy).sqrt()) / 40.0).clamp(0.0, 1.0);
                [240, 180, 90, (a * 255.0) as u8]
            })
        })
        .collect();
    let mut sun = Node::raster(
        0,
        "Sun",
        Arc::new(Raster::from_srgba8(2400, 2400, &disc)),
        Placement::at(3300.0, 400.0),
    );
    sun.blend = BlendMode::Screen;
    sun.mask = Some(Arc::new(Mask::from_fn(2400, 2400, 255, |x, _| {
        if x < 1600 {
            255
        } else {
            ((2400 - x) * 255 / 800) as u8
        }
    })));
    let sun = add(&mut d, sun);

    // A rotated, scaled card clipped to the sun.
    let card: Vec<u8> = (0..800u32)
        .flat_map(|y| (0..1200u32).flat_map(move |x| [(x / 5) as u8, 30, (y / 4) as u8, 255]))
        .collect();
    let mut card = Node::raster(
        0,
        "Card",
        Arc::new(Raster::from_srgba8(1200, 800, &card)),
        Placement::default(),
    );
    if let NodeKind::Raster { placement, .. } = &mut card.kind {
        *placement = Placement {
            x: 3500.0,
            y: 900.0,
            scale_x: 1.4,
            scale_y: 1.4,
            rotation: 18.0,
            flip_x: false,
            flip_y: false,
        };
    }
    card.blend = BlendMode::Multiply;
    card.opacity = 0.7;
    let card = add(&mut d, card);
    Command::SetClip {
        id: card,
        clip_to: Some(sun),
    }
    .apply(&mut d)
    .unwrap();
    let g = Command::Group {
        ids: vec![sun, card],
        name: "Sun group".into(),
    }
    .apply(&mut d)
    .unwrap()
    .unwrap();
    Command::SetOpacity {
        id: g,
        opacity: 0.9,
    }
    .apply(&mut d)
    .unwrap();

    add(
        &mut d,
        Node::adjust(
            0,
            Adjustment::Exposure {
                exposure: 0.3,
                offset: 0.0,
                gamma: 1.0,
            },
        ),
    );
    add(
        &mut d,
        Node::adjust(
            0,
            Adjustment::HueSaturation {
                hue: 0.0,
                saturation: 15.0,
                lightness: 0.0,
            },
        ),
    );
    add(
        &mut d,
        Node::adjust(
            0,
            Adjustment::WhiteBalance {
                temperature: 20.0,
                tint: 0.0,
            },
        ),
    );
    println!(
        "built {}×{} with {} nodes in {:?}",
        w,
        h,
        d.nodes.len(),
        t.elapsed()
    );

    let tree = d.composite_tree();
    for (label, level, tiles) in [
        ("fit (≈1440 px wide, level 2)", 2u32, None),
        ("100% · 1440×900 view, level 0", 0u32, Some((6, 4))),
    ] {
        let (tx, ty) = tiles.unwrap_or_else(|| tiles_at(w, h, level));
        let coords: Vec<TileCoord> = (0..ty)
            .flat_map(|y| (0..tx).map(move |x| TileCoord::new(x + 5, y + 3)))
            .collect();
        let coords: Vec<TileCoord> = if tiles.is_none() {
            (0..ty)
                .flat_map(|y| (0..tx).map(move |x| TileCoord::new(x, y)))
                .collect()
        } else {
            coords
        };
        // Warm the mip caches once, as the app does after the first frame.
        coords.par_iter().for_each(|c| {
            render_tile(&tree, level, *c);
        });
        let t = Instant::now();
        coords.par_iter().for_each(|c| {
            render_tile(&tree, level, *c);
        });
        let e = t.elapsed();
        println!(
            "{label}: {} tiles in {:?} ({:.1} ms/tile/core-share)",
            coords.len(),
            e,
            e.as_secs_f64() * 1000.0 / coords.len() as f64
        );
    }
    let t = Instant::now();
    let _ = flatten(&tree, 0);
    println!("full 24 MP flatten: {:?}", t.elapsed());

    let t = Instant::now();
    emulsion_io::save(&d, std::path::Path::new(&out)).unwrap();
    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    println!(
        "saved {out} ({:.1} MB) in {:?}",
        size as f64 / 1e6,
        t.elapsed()
    );
    let t = Instant::now();
    let back = emulsion_io::open(std::path::Path::new(&out)).unwrap();
    println!("reopened {} nodes in {:?}", back.nodes.len(), t.elapsed());
}
