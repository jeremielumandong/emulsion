//! Paired CPU hot-path measurements. Does not measure UI frames or presentation.
use emulsion_core::{Document, Node, NodeId, document::PanelRow};
use emulsion_raster::{Raster, color};
use rayon::prelude::*;
use std::{collections::HashMap, sync::Arc};
#[path = "../../emulsion-ui/src/viewport/sampling.rs"]
mod sampling;
use serde_json::{Value, json};
use std::{hint::black_box, time::Instant};

fn paired<T: PartialEq>(name: &str, before: impl Fn() -> T, after: impl Fn() -> T) -> Value {
    assert!(before() == after(), "{name}: output differs");
    let mut a = Vec::new();
    let mut b = Vec::new();
    for run in 0..35 {
        for candidate in if run % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let start = Instant::now();
            let output = if candidate { after() } else { before() };
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            black_box(&output);
            if run >= 4 {
                if candidate { &mut b } else { &mut a }.push(elapsed);
            }
        }
    }
    let median = |v: &[f64]| {
        let mut v = v.to_vec();
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let (am, bm) = (median(&a), median(&b));
    eprintln!(
        "{name}: {am:.3} -> {bm:.3} ms ({:.1}% less)",
        (1. - bm / am) * 100.
    );
    json!({"case":name,"exact_output":true,"reference_ms":a,"candidate_ms":b,
        "reference_median_ms":am,"candidate_median_ms":bm,"reduction_pct":(1.-bm/am)*100.})
}

fn main() {
    let mut results = Vec::new();
    for (w, h) in [(1280, 854), (3840, 2160)] {
        for mode in ["opaque", "translucent", "dark"] {
            let raster = Raster::from_fn(w, h, [0; 4], |x, y| {
                let v = ((x * 7919 + y * 31) % 65536) as u16;
                let a = if mode == "translucent" { v } else { u16::MAX };
                if mode == "dark" {
                    [v % 656, v % 310, v % 200, a]
                } else {
                    [v / 2, v / 3, v / 4, a]
                }
            });
            results.push(paired(
                &format!("display/{w}x{h}/{mode}"),
                || {
                    raster.rows_par(4, 0u8, |row, dst| {
                        for (p, o) in row.iter().zip(dst.as_chunks_mut::<4>().0) {
                            *o = color::premul_to_srgba8(color::px_to_f(*p));
                        }
                    })
                },
                || raster.to_srgba8(),
            ));
        }
    }
    for count in [1000, 4000, 10000] {
        let mut doc = Document::new(32, 32);
        let raster = Arc::new(Raster::transparent(1, 1));
        for group in 0..count / 100 {
            let id = (group * 101 + 101) as NodeId;
            for index in 0..100 {
                let mut node = Node::raster(
                    (group * 101 + index + 1) as NodeId,
                    format!("Layer {index}"),
                    raster.clone(),
                    Default::default(),
                );
                node.parent = Some(id);
                doc.nodes.push(node);
            }
            doc.nodes.push(Node::group(id, "Group"));
        }
        let predicate =
            |node: &Node| node.name.to_lowercase().contains("layer 9") && !node.is_group();
        results.push(paired(
            &format!("layer-search/{count}"),
            || {
                fn visit(
                    doc: &Document,
                    parent: Option<NodeId>,
                    depth: usize,
                    out: &mut Vec<PanelRow>,
                ) {
                    for id in doc.children(parent).into_iter().rev() {
                        let node = doc.node(id).unwrap();
                        if node.name.to_lowercase().contains("layer 9") && !node.is_group() {
                            out.push(PanelRow { id, depth });
                        }
                        if node.is_group() {
                            visit(doc, Some(id), depth + 1, out);
                        }
                    }
                }
                let mut out = Vec::new();
                visit(&doc, None, 0, &mut out);
                out
            },
            || doc.filter_panel_rows(predicate),
        ));
    }
    let mut tiles = [HashMap::new(), HashMap::new()];
    for y in 0..6 {
        for x in 0..8 {
            for old in [false, true] {
                if (x + y) % 7 == 0 {
                    continue;
                }
                let data: Vec<u8> = (0..256 * 256 * 4)
                    .map(|i| ((i + x * 13 + y * 7 + old as i32 * 23) % 251) as u8)
                    .collect();
                tiles[old as usize].insert((x, y), data);
            }
        }
    }
    for (w, h) in [(1280, 720), (1920, 1080)] {
        for rotation in [0f64, 33., 90.] {
            let render = |candidate| {
                let mut out = vec![0u8; w * h * 4];
                let (s, c) = (-rotation).to_radians().sin_cos();
                out.par_chunks_mut(w * 4)
                    .enumerate()
                    .for_each(|(row, line)| {
                        let mut sampler = sampling::RowSampler::default();
                        for (col, pixel) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                            let (vx, vy) = (
                                col as f64 + 0.5 - w as f64 / 2.,
                                row as f64 + 0.5 - h as f64 / 2.,
                            );
                            let x = (1024. + (vx * c - vy * s) / 2.).floor() as i64;
                            let y = (768. + (vx * s + vy * c) / 2.).floor() as i64;
                            if !(0..2048).contains(&x) || !(0..1536).contains(&y) {
                                continue;
                            }
                            let old = col < w / 2;
                            let lookup = |tx, ty, old: bool| {
                                tiles[old as usize].get(&(tx, ty)).map(Vec::as_slice)
                            };
                            let bytes = if candidate {
                                sampler.pixel(x, y, old, lookup)
                            } else {
                                lookup((x / 256) as i32, (y / 256) as i32, old).map(|b| {
                                    let i = ((y % 256 * 256 + x % 256) * 4) as usize;
                                    &b[i..i + 4]
                                })
                            };
                            if let Some(bytes) = bytes {
                                pixel.copy_from_slice(bytes);
                            }
                        }
                    });
                out
            };
            results.push(paired(
                &format!("canvas-sampling/{w}x{h}/{rotation}deg"),
                || render(false),
                || render(true),
            ));
        }
    }
    println!(
        "{}",
        json!({"debug_assertions":cfg!(debug_assertions),"results":results})
    );
}
