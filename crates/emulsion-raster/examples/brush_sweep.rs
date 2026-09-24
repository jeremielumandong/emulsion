//! Headless CPU benchmark of the brush-stroke path across brush sizes.
//!
//! Every input sample is timed twice: the paint step (`point_full` +
//! `render_with_compositor` on the CPU path) and the display step
//! (`render_tile_cpu` + `tile_to_bgra8` over the dirty 256 px tiles of a
//! two-node composite tree). Raw per-point samples are written as JSON so a
//! later step can compute statistics.
//!
//! ```text
//! cargo run --release -p emulsion-raster --example brush_sweep -- 4096 40,120,250,400 3 target/brush-sweep.json
//! ```
//!
//! Positional arguments (all optional): canvas size, comma-separated brush
//! sizes, repeats per size, output JSON path. `BRUSH_SWEEP_CASES` selects a
//! comma list from `dry,grain,wet,smudge` (default `dry,wet`).
use emulsion_raster::{
    BlendMode, CompositeNode, CompositeTree, NodeContent, Placement, Raster, TileCoord,
    composite::{render_tile_cpu_into, tile_to_bgra8},
    paint::{Brush, GrainKind, Ink, Stroke},
    preview,
};
use serde_json::{Map, Value, json};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

const TILE: i32 = 256;

/// Counts heap allocations so each timed step also reports allocation
/// pressure. Two relaxed atomic increments per allocation; negligible next to
/// the pixel work being measured.
struct CountingAlloc;

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: every method forwards to `System` unchanged; the counters are only
// bookkeeping and never influence the returned pointers.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn alloc_snapshot() -> (u64, u64) {
    (
        ALLOC_COUNT.load(Ordering::Relaxed),
        ALLOC_BYTES.load(Ordering::Relaxed),
    )
}

fn alloc_delta(before: (u64, u64)) -> (u64, u64) {
    let now = alloc_snapshot();
    (now.0 - before.0, now.1 - before.1)
}

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

fn case_stroke(case: &str, size: f32, base: Arc<Raster>) -> Stroke {
    let dry = Brush {
        size,
        ..Brush::default()
    };
    let wet = Brush {
        wetness: 0.7,
        ..dry
    };
    let ink = Ink::Color([0.03, 0.12, 0.3, 1.]);
    let (brush, ink) = match case {
        "dry" => (dry, ink),
        "grain" => (
            Brush {
                grain: GrainKind::Paper,
                grain_strength: 0.7,
                ..dry
            },
            ink,
        ),
        "wet" => (wet, ink),
        "smudge" => (wet, Ink::Smudge),
        other => panic!("unknown case {other:?}; expected one of dry,grain,wet,smudge"),
    };
    Stroke::new(base, brush, ink, None)
}

fn revision() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn percentile(sorted: &[f64], p: usize) -> f64 {
    sorted[(sorted.len() * p / 100).min(sorted.len() - 1)]
}

/// FNV-1a over raw bytes; proves an optimisation left every output pixel
/// identical (float tiles are hashed by bit pattern).
fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn hash_tile(h: u64, tile: &[[f32; 4]]) -> u64 {
    tile.iter().fold(h, |h, p| {
        p.iter()
            .fold(h, |h, v| fnv1a(h, &v.to_bits().to_le_bytes()))
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: u32 = args
        .get(1)
        .map_or(4096, |s| s.parse().expect("canvas size"));
    let sizes: Vec<f32> = args
        .get(2)
        .map_or("40,120,250,400", String::as_str)
        .split(',')
        .map(|s| s.trim().parse().expect("brush size"))
        .collect();
    let repeats: usize = args.get(3).map_or(3, |s| s.parse().expect("repeats"));
    let output = args
        .get(4)
        .map_or_else(|| PathBuf::from("target/brush-sweep.json"), PathBuf::from);
    let cases: Vec<String> = std::env::var("BRUSH_SWEEP_CASES")
        .unwrap_or_else(|_| "dry,wet".into())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let mut samples_out: Vec<Value> = vec![];
    let mut checksums: Vec<Value> = vec![];
    // One accumulator for the whole run, as the editor keeps one per render thread.
    let mut tile = Vec::new();
    for case in &cases {
        for &size in &sizes {
            let mut paint_all = vec![];
            let mut display_all = vec![];
            let mut tree_all = vec![];
            let mut checksum = 0xcbf2_9ce4_8422_2325u64;
            for repeat in 0..repeats {
                let base = Arc::new(Raster::solid(n, n, [0.7, 0.4, 0.2, 1.]));
                let inputs = preview::sample_stroke(n / 2, n / 4);
                let mut current = (*base).clone();
                let mut stroke = case_stroke(case, size, base);
                stroke.set_seed(repeat as u64 + 1);
                for (index, sample) in inputs.iter().enumerate() {
                    let allocs_before = alloc_snapshot();
                    let begin = Instant::now();
                    stroke.point_full(
                        sample.x,
                        sample.y,
                        sample.pressure,
                        sample.tilt,
                        Some(sample.time_ms),
                    );
                    let (out, dirty) = stroke.render_with_compositor(&current, None);
                    current = out;
                    let paint_ms = begin.elapsed().as_secs_f64() * 1000.;
                    let (paint_allocs, paint_alloc_bytes) = alloc_delta(allocs_before);

                    let begin = Instant::now();
                    let tree = CompositeTree {
                        width: n,
                        height: n,
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
                    let tree_ms = begin.elapsed().as_secs_f64() * 1000.;

                    let mut dirty_tiles = 0u32;
                    let allocs_before = alloc_snapshot();
                    // Timed per tile so hashing (and dropping) each tile's
                    // output stays outside the measurement, exactly as the
                    // untimed harness dropped it before the next tile.
                    let mut display = std::time::Duration::ZERO;
                    if dirty.w > 0 && dirty.h > 0 {
                        for ty in dirty.y / TILE..=(dirty.bottom() - 1) / TILE {
                            for tx in dirty.x / TILE..=(dirty.right() - 1) / TILE {
                                let begin = Instant::now();
                                render_tile_cpu_into(&tree, 0, TileCoord::new(tx, ty), &mut tile);
                                let bgra = tile_to_bgra8(
                                    &tile,
                                    (tx as i64 * TILE as i64, ty as i64 * TILE as i64),
                                    (n, n),
                                    8,
                                    200,
                                    160,
                                );
                                display += begin.elapsed();
                                dirty_tiles += 1;
                                checksum = fnv1a(hash_tile(checksum, &tile), &bgra);
                            }
                        }
                    }
                    let display_ms = display.as_secs_f64() * 1000.;
                    let (display_allocs, display_alloc_bytes) = alloc_delta(allocs_before);
                    let dirty_px = i64::from(dirty.w.max(0)) * i64::from(dirty.h.max(0));

                    paint_all.push(paint_ms);
                    display_all.push(display_ms);
                    tree_all.push(tree_ms);
                    samples_out.push(json!({
                        "case": case,
                        "size": size,
                        "repeat": repeat,
                        "index": index,
                        "paint_ms": paint_ms,
                        "tree_ms": tree_ms,
                        "display_ms": display_ms,
                        "dirty_tiles": dirty_tiles,
                        "dirty_px": dirty_px,
                        "paint_allocs": paint_allocs,
                        "paint_alloc_bytes": paint_alloc_bytes,
                        "display_allocs": display_allocs,
                        "display_alloc_bytes": display_alloc_bytes,
                    }));
                }
            }
            if paint_all.is_empty() {
                continue;
            }
            paint_all.sort_by(f64::total_cmp);
            display_all.sort_by(f64::total_cmp);
            tree_all.sort_by(f64::total_cmp);
            checksums
                .push(json!({"case": case, "size": size, "checksum": format!("{checksum:016x}")}));
            println!(
                "case={case} size={size} n={} paint p50/p95/max ms={:.3}/{:.3}/{:.3}  display p50/p95/max ms={:.3}/{:.3}/{:.3}  tree p50={:.3}  checksum={checksum:016x}",
                paint_all.len(),
                percentile(&paint_all, 50),
                percentile(&paint_all, 95),
                paint_all[paint_all.len() - 1],
                percentile(&display_all, 50),
                percentile(&display_all, 95),
                display_all[display_all.len() - 1],
                percentile(&tree_all, 50),
            );
        }
    }

    let mut meta = Map::new();
    meta.insert("canvas".into(), json!(n));
    meta.insert("sizes".into(), json!(sizes));
    meta.insert("repeats".into(), json!(repeats));
    meta.insert("cases".into(), json!(cases));
    meta.insert("revision".into(), json!(revision()));
    meta.insert(
        "profile".into(),
        json!(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }),
    );
    let doc =
        json!({ "meta": Value::Object(meta), "checksums": checksums, "samples": samples_out });
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).expect("create output directory");
    }
    std::fs::write(&output, serde_json::to_string(&doc).expect("serialize"))
        .expect("write output JSON");
    println!("wrote {}", output.display());
}
