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
    let b = Brush {
        size: 80.0,
        ..Default::default()
    };
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
    let soft = Brush {
        size: 80.0,
        hardness: 0.2,
        grain_strength: 0.6,
        ..Default::default()
    };
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
    for name in [
        "Screentone 20%",
        "Screentone 40%",
        "Screentone 60%",
        "Speed lines",
        "Maru pen",
    ] {
        let Some(preset) = emulsion_raster::library::find(name) else {
            continue;
        };
        let mut sb = preset.brush;
        sb.size = 120.0;
        let mut s3 = timed(&format!("{name} 300 pts size 120 (stamp)"), || {
            let mut s = Stroke::new(base.clone(), sb, Ink::Color([0.0, 0.0, 0.0, 1.0]), None);
            for i in 0..300 {
                let t = i as f32 / 299.0;
                s.point(200.0 + t * 5000.0, 3000.0 + (t * 20.0).sin() * 600.0);
            }
            s.finish();
            s
        });
        timed(&format!("{name} (render)"), || s3.render(&base).0);
    }
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
    // ── Whole-process paths ──
    let png = timed("PNG encode 8-bit (export)", || {
        emulsion_io::export::png8(w, h, &rgba).unwrap()
    });
    println!("{:<44} {:>8.1} MB", "  png size", png.len() as f64 / 1e6);
    timed("PNG decode + import (open)", || {
        emulsion_io::import::import_bytes("bench.png", &png).unwrap()
    });
    let dir = std::env::temp_dir().join("emulsion-perf");
    std::fs::create_dir_all(&dir).unwrap();
    let ora = dir.join("bench.ora");
    timed("ORA save (3 nodes, 24 MP)", || {
        emulsion_io::save(&doc, &ora).unwrap()
    });
    println!(
        "{:<44} {:>8.1} MB",
        "  ora size",
        std::fs::metadata(&ora).map(|m| m.len()).unwrap_or(0) as f64 / 1e6
    );
    timed("ORA open", || emulsion_io::open(&ora).unwrap());
    let mut editor = timed("Editor::new", || {
        emulsion_core::Editor::new(doc.clone(), None)
    });
    let photo_id = doc.nodes[0].id;
    timed("execute ReplacePixels (full layer) + commit", || {
        editor
            .execute(Command::ReplacePixels {
                id: photo_id,
                raster: base.clone(),
                dirty: base.bounds(),
                label: "bench".into(),
            })
            .unwrap();
        editor.commit("bench", false)
    });
    let mut styled = doc.clone();
    if let Some(n) = styled.nodes.iter_mut().find(|n| n.name == "paint") {
        n.styles
            .push(emulsion_core::styles::LayerStyle::DropShadow {
                color: [0, 0, 0],
                opacity: 0.6,
                angle: 120.0,
                distance: 12.0,
                size: 20.0,
            });
    }
    let styled_tree = timed("composite_tree() with a drop shadow", || {
        styled.composite_tree()
    });
    timed("flatten level 2 with drop shadow", || {
        flatten(&styled_tree, 2)
    });
    let mut graded = doc.clone();
    for adj in [
        Adjustment::Exposure {
            exposure: 0.3,
            offset: 0.0,
            gamma: 1.1,
        },
        Adjustment::BrightnessContrast {
            brightness: 0.05,
            contrast: 0.15,
        },
    ] {
        add(&mut graded, Node::adjust(0, adj));
    }
    let graded_tree = graded.composite_tree();
    timed("flatten level 2 with 3 adjustment nodes", || {
        flatten(&graded_tree, 2)
    });
    timed("flatten level 0 with 3 adjustment nodes", || {
        flatten(&graded_tree, 0)
    });
    let recipe = emulsion_recipes::cameras::presets()
        .into_iter()
        .next()
        .unwrap();
    let compiled = timed("recipe compile_sized (camera look)", || {
        emulsion_recipes::compile_sized(&recipe, w, h).unwrap()
    });
    let mut re = emulsion_core::Editor::new(doc.clone(), None);
    timed("recipe add_to (into editor)", || {
        emulsion_recipes::store::add_to(&mut re, compiled, Slot::TOP).unwrap()
    });
    let rtree = re.doc.composite_tree();
    timed("flatten level 2 with the recipe", || flatten(&rtree, 2));
    // Each stage alone, to see which adjustments cost the most.
    let (_, stage_nodes) = recipe.compile_for(None, w, h);
    for n in &stage_nodes {
        let mut d1 = doc.clone();
        let mut n1 = n.clone();
        n1.id = 0;
        n1.parent = None;
        add(&mut d1, n1);
        let t1 = d1.composite_tree();
        timed(&format!("  stage alone @L2: {}", n.name), || {
            flatten(&t1, 2)
        });
    }
    let m = select::rect(w, h, 1000.0, 1000.0, 3000.0, 2000.0);
    timed("select::grow 10 px", || select::grow(&m, 10));
    timed("select::transform (rotate 10°)", || {
        select::transform(&m, glam::DAffine2::from_angle(10f64.to_radians()))
    });
    timed("warp::warp (Distort, 24 MP)", || {
        emulsion_raster::warp::warp(
            &*base,
            [
                (100.0, 50.0),
                (5900.0, 0.0),
                (6000.0, 4000.0),
                (0.0, 3900.0),
            ],
            [0u16; 4],
        )
        .unwrap()
    });
    timed("liquify dab size 400", || {
        emulsion_raster::liquify::dab(
            &base,
            &base,
            emulsion_raster::liquify::Mode::Push,
            (3000.0, 2000.0),
            200.0,
            1.0,
            (30.0, 0.0),
        )
    });
    timed(
        "vector path rasterize (30 anchors, 3000×2000, stroke 4)",
        || {
            let mut d = String::from("M 1000 1000");
            for i in 1..30 {
                let t = i as f32 / 29.0;
                d.push_str(&format!(
                    " L {} {}",
                    1000.0 + t * 3000.0,
                    2000.0 + (t * 12.0).sin() * 900.0
                ));
            }
            let path = emulsion_raster::vector::Path::from_svg(&d).unwrap();
            let style = emulsion_raster::vector::PathStyle {
                stroke: Some([255, 0, 0, 255]),
                width: 4.0,
                fill: None,
                ..Default::default()
            };
            path.rasterize(&style, w, h)
        },
    );
    timed("write_rect 512×512 (stroke commit shape)", || {
        let px = vec![[65535u16, 0, 0, 65535]; 512 * 512];
        base.write_rect(emulsion_raster::IRect::new(1000, 1000, 512, 512), &px)
    });
}
