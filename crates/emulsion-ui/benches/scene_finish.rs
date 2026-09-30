//! Same-process comparison of the legacy sorter, texture grouping alone,
//! production Scene::finish, and two historical candidates. Only sorting is timed.
//! Reproduce the recorded measurements with: python scripts/bench-gpui-scene.py
//! This compiles all compared sorting algorithms at opt-level=3 with the same
//! assertion/overflow settings as the linked GPUI library.
//! The Cargo bench target is also available, but profile differences affect results.

#[path = "support/scene_sort_candidate.rs"]
mod candidate;
#[path = "support/scene_sort_initial.rs"]
mod initial;
#[path = "support/scene_sort_previous.rs"]
mod previous;

#[derive(Default)]
struct SortScratch {
    initial: initial::SortScratch,
    previous: previous::SortScratch,
}

use gpui_kit::{
    AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds, ContentMask, DevicePixels,
    MonochromeSprite, Quad, ScaledPixels, Scene, TileId, TransformationMatrix, point, rgb, size,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering::Relaxed},
    time::Instant,
};

struct MeasuredAllocator;
static TRACK: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && TRACK.load(Relaxed) {
            ALLOCATIONS.fetch_add(1, Relaxed);
            ALLOCATED.fetch_add(layout.size(), Relaxed);
            LIVE.fetch_add(layout.size() as isize, Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if TRACK.load(Relaxed) {
            LIVE.fetch_sub(layout.size() as isize, Relaxed);
        }
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;

fn allocations(
    scene: &mut Scene,
    sorter: Sorter,
    scratch: &mut SortScratch,
) -> (usize, usize, isize) {
    ALLOCATIONS.store(0, Relaxed);
    ALLOCATED.store(0, Relaxed);
    LIVE.store(0, Relaxed);
    TRACK.store(true, Relaxed);
    finish(scene, sorter, scratch);
    TRACK.store(false, Relaxed);
    (
        ALLOCATIONS.load(Relaxed),
        ALLOCATED.load(Relaxed),
        LIVE.load(Relaxed),
    )
}

#[derive(Clone, Copy)]
enum Sorter {
    Original,
    TextureOnly,
    Production,
    Initial,
    Previous,
}

fn finish(scene: &mut Scene, sorter: Sorter, scratch: &mut SortScratch) {
    if matches!(sorter, Sorter::Production) {
        scene.finish();
        return;
    }
    if matches!(sorter, Sorter::Initial) {
        initial::sort_in_drawing_order(scene, &mut scratch.initial);
        return;
    }
    if matches!(sorter, Sorter::Previous) {
        previous::sort_in_drawing_order(scene, &mut scratch.previous);
        return;
    }
    // Exact pre-backport Scene::finish, apart from the separate texture-only arm.
    scene.shadows.sort_by_key(|p| p.order);
    scene.quads.sort_by_key(|p| p.order);
    scene.paths.sort_by_key(|p| p.order);
    scene.underlines.sort_by_key(|p| p.order);
    if matches!(sorter, Sorter::Original) {
        scene
            .monochrome_sprites
            .sort_by_key(|p| (p.order, p.tile.tile_id));
        scene
            .subpixel_sprites
            .sort_by_key(|p| (p.order, p.tile.tile_id));
        scene
            .polychrome_sprites
            .sort_by_key(|p| (p.order, p.tile.tile_id));
    } else {
        scene
            .monochrome_sprites
            .sort_by_key(|p| (p.order, p.tile.texture_id.index, p.tile.tile_id));
        scene
            .subpixel_sprites
            .sort_by_key(|p| (p.order, p.tile.texture_id.index, p.tile.tile_id));
        scene
            .polychrome_sprites
            .sort_by_key(|p| (p.order, p.tile.texture_id.index, p.tile.tile_id));
    }
    scene.surfaces.sort_by_key(|p| p.order);
}

fn seed(name: &str, count: usize, seed: u64) -> Scene {
    let mut scene = Scene::default();
    let mut random = seed;
    for index in 0..count {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        let order = if name.ends_with("sparse") {
            (random >> 32) as u32
        } else if name.ends_with("reverse-ties") {
            ((count - index) / 8) as u32
        } else if name.ends_with("equal") {
            0
        } else if name.ends_with("random") || name.ends_with("full-keys") {
            (random % 64) as u32
        } else if name.ends_with("reverse") {
            (count - index) as u32
        } else {
            (index / 32) as u32
        };
        let bounds = Bounds::new(
            point(ScaledPixels(index as f32 * 12.), ScaledPixels(0.)),
            size(ScaledPixels(10.), ScaledPixels(10.)),
        );
        if name.starts_with("quads") || (name.starts_with("mixed") && index % 2 == 0) {
            scene.quads.push(Quad {
                order,
                bounds,
                content_mask: ContentMask { bounds },
                background: rgb(index as u32).into(),
                ..Default::default()
            });
        } else {
            let textures = if name.starts_with("atlas4") || name.starts_with("mixed") {
                4
            } else {
                1
            };
            scene.monochrome_sprites.push(MonochromeSprite {
                order,
                pad: 0,
                bounds,
                content_mask: ContentMask { bounds },
                color: rgb(index as u32).into(),
                tile: AtlasTile {
                    texture_id: AtlasTextureId {
                        index: if name.ends_with("full-keys") {
                            random as u32
                        } else {
                            index as u32 % textures
                        },
                        kind: AtlasTextureKind::Monochrome,
                    },
                    tile_id: TileId(if name.ends_with("full-keys") {
                        (random >> 32) as u32
                    } else {
                        (index as u32 / textures) % 32
                    }),
                    padding: 0,
                    bounds: Bounds::new(
                        point(DevicePixels(0), DevicePixels(0)),
                        size(DevicePixels(10), DevicePixels(10)),
                    ),
                },
                transformation: TransformationMatrix::unit(),
            });
        }
    }
    if name == "quads-nearly-ordered" {
        for index in (0..count.saturating_sub(32)).step_by(257) {
            scene.quads.swap(index, index + 32);
        }
    }
    if name == "atlas4-grouped" {
        scene
            .monochrome_sprites
            .sort_by_key(|p| (p.order, p.tile.texture_id.index, p.tile.tile_id));
    }
    scene
}

fn reset(scene: &mut Scene, source: &Scene) {
    scene.clear();
    scene.quads.extend_from_slice(&source.quads);
    scene
        .monochrome_sprites
        .extend_from_slice(&source.monochrome_sprites);
}

fn main() {
    // A multiple of five balances both execution order and physical buffers.
    const SAMPLES: usize = 25;
    const FRAMES: usize = 64;
    let seed_value = std::env::var("EMULSION_SCENE_BENCH_SEED")
        .map(|value| value.parse::<u64>().expect("numeric seed"))
        .unwrap_or(0x5eed);
    let counts: Vec<usize> = std::env::var("EMULSION_SCENE_BENCH_SIZES")
        .unwrap_or_else(|_| "32,256,2000,10000".into())
        .split(',')
        .map(|value| value.parse().expect("numeric size"))
        .collect();
    let workloads = [
        "quads-ordered",
        "quads-random",
        "quads-reverse",
        "atlas1-ordered",
        "atlas1-random",
        "atlas4-ordered",
        "atlas4-random",
        "quads-reverse-ties",
        "quads-equal",
        "quads-nearly-ordered",
        "quads-sparse",
        "atlas4-sparse",
        "atlas4-grouped",
        "atlas4-full-keys",
        "mixed-random",
        "mixed-ordered",
    ];
    let selected =
        std::env::var("EMULSION_SCENE_BENCH_WORKLOADS").unwrap_or_else(|_| workloads.join(","));
    let selected: Vec<&str> = selected.split(',').collect();
    assert!(selected.iter().all(|name| workloads.contains(name)));
    println!(
        "Scene sorting only; 25 rotating samples of 64 frames; 5 warmup samples; microseconds/frame"
    );
    println!(
        "workload,count,original_us,texture_only_us,production_us,production_p10_us,production_p90_us,change_percent,original_batches,production_batches,original_allocs,original_alloc_bytes,production_allocs,production_retained_bytes,initial_candidate_us,previous_candidate_us"
    );
    for count in counts {
        for &workload in &selected {
            let source = seed(workload, count, seed_value);
            let mut scenes: [Scene; 5] = Default::default();
            let mut scratch: [SortScratch; 5] = Default::default();
            let sorters = [
                Sorter::Original,
                Sorter::TextureOnly,
                Sorter::Production,
                Sorter::Initial,
                Sorter::Previous,
            ];
            let mut samples: [Vec<f64>; 5] = Default::default();
            for sample in 0..SAMPLES + 5 {
                for offset in 0..5 {
                    let variant = (sample + offset) % 5;
                    // Rotate physical primitive buffers too: otherwise one
                    // algorithm permanently gets a different cache/TLB layout.
                    let scene = &mut scenes[(sample / 5 + variant) % 5];
                    let mut elapsed = std::time::Duration::ZERO;
                    for _ in 0..FRAMES {
                        reset(scene, &source);
                        let start = Instant::now();
                        finish(black_box(scene), sorters[variant], &mut scratch[variant]);
                        elapsed += start.elapsed();
                        black_box(&scene.quads);
                        black_box(&scene.monochrome_sprites);
                    }
                    if sample >= 5 {
                        samples[variant].push(elapsed.as_secs_f64() * 1e6 / FRAMES as f64);
                    }
                }
            }
            // Restore algorithm-indexed results for the independent oracles;
            // rotating buffers above leaves them indexed by physical storage.
            for variant in 0..5 {
                reset(&mut scenes[variant], &source);
                finish(
                    &mut scenes[variant],
                    sorters[variant],
                    &mut scratch[variant],
                );
            }
            let mut reference = Scene::default();
            reset(&mut reference, &source);
            candidate::sort_in_drawing_order(
                &mut reference,
                &mut candidate::SortScratch::default(),
            );
            assert_eq!(
                format!("{:?}", reference.quads),
                format!("{:?}", scenes[2].quads)
            );
            assert_eq!(
                format!("{:?}", reference.monochrome_sprites),
                format!("{:?}", scenes[2].monochrome_sprites)
            );
            // Production preserves sequences already ordered by the legacy
            // key, and groups atlas textures only when it must sort anyway.
            assert_eq!(
                format!("{:?}", scenes[1].quads),
                format!("{:?}", scenes[2].quads)
            );
            assert_eq!(
                format!(
                    "{:?}",
                    scenes[if source
                        .monochrome_sprites
                        .is_sorted_by_key(|p| (p.order, p.tile.tile_id))
                    {
                        0
                    } else {
                        1
                    }]
                    .monochrome_sprites
                ),
                format!("{:?}", scenes[2].monochrome_sprites)
            );
            assert_eq!(
                format!("{:?}", scenes[1].quads),
                format!("{:?}", scenes[3].quads)
            );
            assert_eq!(
                format!("{:?}", scenes[1].monochrome_sprites),
                format!("{:?}", scenes[3].monochrome_sprites)
            );
            assert_eq!(
                format!("{:?}", scenes[2].quads),
                format!("{:?}", scenes[4].quads)
            );
            assert_eq!(
                format!("{:?}", scenes[2].monochrome_sprites),
                format!("{:?}", scenes[4].monochrome_sprites)
            );
            for values in &mut samples {
                values.sort_by(f64::total_cmp);
            }
            let medians = samples.each_ref().map(|values| values[SAMPLES / 2]);
            reset(&mut scenes[0], &source);
            let original_alloc = allocations(&mut scenes[0], Sorter::Original, &mut scratch[0]);
            reset(&mut scenes[2], &source);
            let production_alloc = allocations(&mut scenes[2], Sorter::Production, &mut scratch[2]);
            let mut fresh = Scene::default();
            reset(&mut fresh, &source);
            let retained_bytes =
                allocations(&mut fresh, Sorter::Production, &mut SortScratch::default()).2;
            println!(
                "{workload},{count},{:.3},{:.3},{:.3},{:.3},{:.3},{:.1},{},{},{},{},{},{},{:.3},{:.3}",
                medians[0],
                medians[1],
                medians[2],
                samples[2][SAMPLES / 10],
                samples[2][SAMPLES * 9 / 10],
                (medians[2] / medians[0] - 1.) * 100.,
                scenes[0].batches().count(),
                scenes[2].batches().count(),
                original_alloc.0,
                original_alloc.1,
                production_alloc.0,
                retained_bytes,
                medians[3],
                medians[4],
            );
        }
    }
}
