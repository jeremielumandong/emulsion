//! Pixel diff of the spike's GPU composite against Emulsion's CPU reference
//! compositor (what the GPUI canvas presents), at 100% and zoomed-out levels.

use emulsion_core::Document;
use emulsion_engine::compositor::Camera;
use emulsion_engine::engine::{Engine, Offscreen, Output};
use emulsion_engine::gpu::Gpu;
use emulsion_engine::vector::VectorSpace;
use emulsion_raster::composite::{level_size, render_tile_cpu, tiles_at};
use emulsion_raster::{TILE, TileCoord, color};
use rayon::prelude::*;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct Diff {
    pub label: String,
    pub pixels: usize,
    /// Premultiplied linear, 0–1.
    pub max_linear: f32,
    pub mean_linear: f64,
    /// 8-bit sRGB display codes over mid grey.
    pub max_code: u8,
    pub over_1: usize,
    pub over_3: usize,
    /// Pixels on an edge in the CPU image (3×3 range above 24 codes), where
    /// two rasterizers legitimately disagree on partial coverage.
    pub edges: usize,
    /// Off-edge pixels more than 3 codes from every CPU pixel within one
    /// pixel: colour and blending errors rather than rasterization.
    pub interior_over_3: usize,
}

impl Diff {
    pub fn row(&self) -> String {
        format!(
            "| {} | {} | {:.2e} | {:.2e} | {} | {:.3}% | {:.3}% | {:.2}% | {:.3}% |",
            self.label,
            self.pixels,
            self.max_linear,
            self.mean_linear,
            self.max_code,
            100.0 * self.over_1 as f64 / self.pixels.max(1) as f64,
            100.0 * self.over_3 as f64 / self.pixels.max(1) as f64,
            100.0 * self.edges as f64 / self.pixels.max(1) as f64,
            100.0 * self.interior_over_3 as f64 / self.pixels.max(1) as f64,
        )
    }
}

pub const HEADER: &str = "| Case | Pixels | Max linear | Mean linear | Max 8-bit code | >1 code | >3 codes | Edge px | >3 codes off-edge |\n|---|---|---|---|---|---|---|---|---|";

fn display(p: &[f32]) -> [u8; 3] {
    let bg = 0.214; // sRGB 128
    let a = p[3].clamp(0.0, 1.0);
    [0, 1, 2].map(|i| color::linear_to_srgb8(p[i] + bg * (1.0 - a)))
}

pub fn compare(
    label: &str,
    width: usize,
    gpu: &[f32],
    cpu: &[f32],
    heat: Option<&mut Vec<u8>>,
) -> Diff {
    let mut d = Diff {
        label: label.into(),
        pixels: gpu.len() / 4,
        ..Default::default()
    };
    let mut sum = 0.0f64;
    let mut heat_out = Vec::with_capacity(d.pixels);
    for (g, c) in gpu.as_chunks::<4>().0.iter().zip(cpu.as_chunks::<4>().0) {
        let mut m = 0.0f32;
        for i in 0..4 {
            m = m.max((g[i] - c[i]).abs());
        }
        sum += m as f64;
        d.max_linear = d.max_linear.max(m);
        let (dg, dc) = (display(g), display(c));
        let code = (0..3).map(|i| dg[i].abs_diff(dc[i])).max().unwrap_or(0);
        d.max_code = d.max_code.max(code);
        d.over_1 += usize::from(code > 1);
        d.over_3 += usize::from(code > 3);
    }
    d.mean_linear = sum / d.pixels.max(1) as f64;
    let (dg, dc): (Vec<[u8; 3]>, Vec<[u8; 3]>) = (
        gpu.as_chunks::<4>().0.iter().map(|p| display(p)).collect(),
        cpu.as_chunks::<4>().0.iter().map(|p| display(p)).collect(),
    );
    let height = d.pixels / width.max(1);
    for y in 0..height {
        for x in 0..width {
            let g = dg[y * width + x];
            let mut best = u8::MAX;
            let (mut lo, mut hi) = ([u8::MAX; 3], [0u8; 3]);
            for (ox, oy) in (-1i64..=1).flat_map(|oy| (-1i64..=1).map(move |ox| (ox, oy))) {
                let (cx, cy) = (x as i64 + ox, y as i64 + oy);
                if cx < 0 || cy < 0 || cx >= width as i64 || cy >= height as i64 {
                    continue;
                }
                let c = dc[cy as usize * width + cx as usize];
                best = best.min((0..3).map(|i| g[i].abs_diff(c[i])).max().unwrap_or(0));
                for i in 0..3 {
                    lo[i] = lo[i].min(c[i]);
                    hi[i] = hi[i].max(c[i]);
                }
            }
            let edge = (0..3).any(|i| hi[i] - lo[i] > 24);
            d.edges += usize::from(edge);
            let best = if edge { 0 } else { best };
            d.interior_over_3 += usize::from(best > 3);
            heat_out.push(best.saturating_mul(32));
        }
    }
    if let Some(h) = heat {
        *h = heat_out;
    }
    d
}

/// The CPU reference for the whole document at `level`, premultiplied linear.
pub fn cpu_reference(doc: &Document, level: u32) -> (Vec<f32>, (u32, u32)) {
    let tree = doc.composite_tree();
    let (w, h) = level_size(doc.width, doc.height, level);
    let (tx, ty) = tiles_at(doc.width, doc.height, level);
    let coords: Vec<TileCoord> = (0..ty)
        .flat_map(|y| (0..tx).map(move |x| TileCoord::new(x, y)))
        .collect();
    let tiles: Vec<(TileCoord, Vec<[f32; 4]>)> = coords
        .into_par_iter()
        .map(|c| (c, render_tile_cpu(&tree, level, c)))
        .collect();
    let mut out = vec![0.0f32; w as usize * h as usize * 4];
    let t = TILE as usize;
    for (c, tile) in tiles {
        for y in 0..t {
            let gy = c.y as usize * t + y;
            if gy >= h as usize {
                break;
            }
            for x in 0..t {
                let gx = c.x as usize * t + x;
                if gx >= w as usize {
                    break;
                }
                let i = (gy * w as usize + gx) * 4;
                out[i..i + 4].copy_from_slice(&tile[y * t + x]);
            }
        }
    }
    (out, (w, h))
}

/// Render `engine`'s document at `level` into a raw target and read it back.
/// Screen-sized chunks, each submitted and awaited on its own. One draw over
/// a whole 4K document with hundreds of ops per pixel outlasts i915's
/// preemption timeout and hangs the GPU.
const CHUNK: u32 = 512;

/// Render `engine`'s document at `level` into raw targets and read it back.
pub fn gpu_render(engine: &mut Engine, level: u32) -> anyhow::Result<Vec<f32>> {
    let size = level_size(engine.canvas.width, engine.canvas.height, level);
    let target = Offscreen::new(
        &engine.gpu,
        (CHUNK, CHUNK),
        wgpu::TextureFormat::Rgba32Float,
    );
    engine.screen = (CHUNK, CHUNK);
    let zoom = 1.0 / (1u32 << level) as f64;
    let mut out = vec![0.0f32; size.0 as usize * size.1 as usize * 4];
    for cy in (0..size.1).step_by(CHUNK as usize) {
        for cx in (0..size.0).step_by(CHUNK as usize) {
            // The chunk's top-left level pixel sits at the screen origin.
            engine.camera = Camera {
                center: [
                    (cx + CHUNK / 2) as f64 / zoom,
                    (cy + CHUNK / 2) as f64 / zoom,
                ],
                zoom,
            };
            engine.render(&target.view, target.format, Output::Raw)?;
            let bytes = target.read(&engine.gpu)?;
            let px: &[f32] = bytemuck::cast_slice(&bytes);
            let w = CHUNK.min(size.0 - cx) as usize;
            for y in 0..CHUNK.min(size.1 - cy) as usize {
                let src = &px[y * CHUNK as usize * 4..][..w * 4];
                let dst = ((cy as usize + y) * size.0 as usize + cx as usize) * 4;
                out[dst..dst + w * 4].copy_from_slice(src);
            }
        }
    }
    Ok(out)
}

fn save_png(path: &Path, size: (u32, u32), data: &[f32]) -> anyhow::Result<()> {
    let rgb: Vec<u8> = data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| display(p))
        .collect();
    image::save_buffer(path, &rgb, size.0, size.1, image::ColorType::Rgb8)?;
    Ok(())
}

/// Diff one document. Raster compositing is isolated by recompiling with
/// Vello off (vector nodes then composite from their CPU caches).
pub fn run(
    gpu: &Arc<Gpu>,
    doc: &Document,
    name: &str,
    out: Option<&Path>,
    levels: &[u32],
) -> anyhow::Result<Vec<Diff>> {
    let mut diffs = Vec::new();
    let configs = [
        ("raster, direct", None, false),
        ("raster, via tile cache", None, true),
        ("Vello, sRGB-encoded", Some(VectorSpace::Srgb), true),
        ("Vello, linear 8-bit", Some(VectorSpace::Linear), true),
    ];
    let mut unsupported_noted = false;
    for (label, space, cache) in configs {
        let has_vectors = doc.nodes.iter().any(|n| {
            matches!(
                n.kind,
                emulsion_core::NodeKind::Path { .. } | emulsion_core::NodeKind::Text { .. }
            )
        });
        if space.is_some() && !has_vectors {
            continue;
        }
        let mut engine = Engine::new(
            gpu.clone(),
            doc,
            None,
            space.unwrap_or(VectorSpace::Srgb),
            space.is_some(),
            cache,
            (64, 64),
        )?;
        if !unsupported_noted {
            for u in &engine.canvas.unsupported {
                println!("  unsupported in {name}: {u}");
            }
            unsupported_noted = true;
        }
        // Vector targets are screen-resolution vectors, not mips: compare at 100% only.
        let levels: &[u32] = if space.is_some() { &[0] } else { levels };
        for &level in levels {
            let (cpu, size) = cpu_reference(doc, level);
            let gpu_px = gpu_render(&mut engine, level)?;
            // SPIKE_PROBE=x,y;x,y prints both renders at those pixels.
            if let Ok(probe) = std::env::var("SPIKE_PROBE") {
                for xy in probe.split(';') {
                    if let Some((x, y)) = xy.split_once(',')
                        && let (Ok(x), Ok(y)) = (x.parse::<usize>(), y.parse::<usize>())
                        && x < size.0 as usize
                        && y < size.1 as usize
                    {
                        let i = (y * size.0 as usize + x) * 4;
                        println!(
                            "  probe {x},{y} {label}: gpu {:?} cpu {:?}",
                            &gpu_px[i..i + 4],
                            &cpu[i..i + 4]
                        );
                    }
                }
            }
            let mut heat = Vec::new();
            let d = compare(
                &format!("{name} · {label} · level {level}"),
                size.0 as usize,
                &gpu_px,
                &cpu,
                Some(&mut heat),
            );
            println!("{}", d.row());
            if let Some(dir) = out {
                std::fs::create_dir_all(dir)?;
                let stem = format!(
                    "{}-{}-l{level}",
                    name,
                    label
                        .split([' ', ',', '(', ')'])
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("-")
                );
                save_png(&dir.join(format!("{stem}-gpu.png")), size, &gpu_px)?;
                if space.is_none() {
                    save_png(&dir.join(format!("{name}-l{level}-cpu.png")), size, &cpu)?;
                }
                image::save_buffer(
                    dir.join(format!("{stem}-diff.png")),
                    &heat,
                    size.0,
                    size.1,
                    image::ColorType::L8,
                )?;
            }
            diffs.push(d);
        }
    }
    Ok(diffs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_engine::brush::{GpuStroke, test_brush};
    use emulsion_raster::blend::BlendSpace;

    /// A GPU, or `None` to skip. `EMULSION_REQUIRE_GPU_TESTS=1` makes a
    /// missing adapter a failure, as in `emulsion-gpu`.
    fn gpu() -> Option<Arc<Gpu>> {
        match Gpu::new(emulsion_engine::gpu::instance(), None, None) {
            Ok(gpu) => Some(gpu),
            Err(error) => {
                assert!(
                    std::env::var("EMULSION_REQUIRE_GPU_TESTS").as_deref() != Ok("1"),
                    "GPU tests required: {error:#}"
                );
                eprintln!("Skipping GPU checks: {error:#}");
                None
            }
        }
    }

    #[test]
    fn raster_composite_matches_cpu_reference() {
        let Some(gpu) = gpu() else { return };
        for space in [BlendSpace::Linear, BlendSpace::Srgb] {
            let doc = crate::testdocs::fidelity(space);
            for cache in [false, true] {
                let mut engine = Engine::new(
                    gpu.clone(),
                    &doc,
                    None,
                    VectorSpace::Srgb,
                    false,
                    cache,
                    (64, 64),
                )
                .unwrap();
                assert!(engine.canvas.unsupported.is_empty());
                assert_eq!(engine.cache.is_some(), cache);
                for level in [0, 1, 2] {
                    let (cpu, size) = cpu_reference(&doc, level);
                    let gpu_px = gpu_render(&mut engine, level).unwrap();
                    let d = compare("test", size.0 as usize, &gpu_px, &cpu, None);
                    if gpu.tile_format == emulsion_engine::gpu::TileFormat::Unorm16 {
                        assert!(d.max_code <= 1, "{space:?} level {level}: {d:?}");
                        if level == 0 {
                            assert!(d.max_linear < 1e-4, "{space:?}: {d:?}");
                        }
                    }
                }
            }
        }
    }

    /// `Engine::reload` rebuilds the program against the atlas already on the
    /// GPU. Tiles the edit left alone must be re-acquired by `Arc` identity, so
    /// the atlas must not grow, the composite cache must not be invalidated,
    /// and the result must still match the CPU.
    ///
    /// Layers that are baked -- masked, or placed off the document grid -- are
    /// re-flattened on every reload into a fresh `Raster`, so their tiles do
    /// get new slots and are reported dirty. That is why the no-invalidation
    /// half of this test uses a document without them.
    #[test]
    fn reload_reuses_the_atlas_and_keeps_parity() {
        let Some(gpu) = gpu() else { return };

        // No baked layers: a reload must be a complete no-op for the cache.
        let mut plain = Document::new(700, 520);
        crate::bench::add_paint_layer(&mut plain);
        let mut engine = Engine::new(
            gpu.clone(),
            &plain,
            None,
            VectorSpace::Srgb,
            false,
            true,
            (64, 64),
        )
        .unwrap();
        let (tiles, pages) = (engine.atlas.used(), engine.atlas.pages());
        for _ in 0..3 {
            engine.reload(&plain, None, false).unwrap();
            assert_eq!(
                (engine.atlas.used(), engine.atlas.pages()),
                (tiles, pages),
                "reload allocated atlas slots for tiles it already had"
            );
            assert!(
                engine.canvas.dirty.is_empty(),
                "reload of an unchanged document invalidated {} cache rect(s)",
                engine.canvas.dirty.len()
            );
        }

        // With masks and placements, reload must still composite correctly.
        let doc = crate::testdocs::fidelity(BlendSpace::Linear);
        let mut engine = Engine::new(
            gpu.clone(),
            &doc,
            None,
            VectorSpace::Srgb,
            false,
            true,
            (64, 64),
        )
        .unwrap();
        let before = engine.atlas.used();
        engine.reload(&doc, None, false).unwrap();
        assert_eq!(
            engine.atlas.used(),
            before,
            "reload leaked atlas slots across a rebuild"
        );
        let (cpu, size) = cpu_reference(&doc, 0);
        let gpu_px = gpu_render(&mut engine, 0).unwrap();
        let d = compare("reload", size.0 as usize, &gpu_px, &cpu, None);
        if gpu.tile_format == emulsion_engine::gpu::TileFormat::Unorm16 {
            assert!(d.max_code <= 1, "after reload: {d:?}");
        }
    }

    /// What the app does when you paint: the stroke is rasterised on the CPU,
    /// committed with ReplacePixels, and the canvas reloads the new document.
    /// The composite must actually change.
    #[test]
    fn reload_shows_pixels_committed_to_the_document() {
        let Some(gpu) = gpu() else { return };
        let mut doc = Document::new(700, 520);
        let paint = crate::bench::add_paint_layer(&mut doc);
        let mut engine = Engine::new(
            gpu.clone(),
            &doc,
            Some(paint),
            VectorSpace::Srgb,
            false,
            true,
            (64, 64),
        )
        .unwrap();
        // Fill the composite cache first, so a stale cache would hide the edit.
        let before = gpu_render(&mut engine, 0).unwrap();

        // Paint a stroke across the middle, exactly as the editor does.
        let base = match &doc.node(paint).unwrap().kind {
            emulsion_core::NodeKind::Raster { raster, .. } => raster.clone(),
            other => panic!("paint layer is not a raster: {other:?}"),
        };
        let brush = test_brush(120.0);
        let mut stroke = emulsion_raster::paint::Stroke::new(
            base,
            brush,
            emulsion_raster::paint::Ink::Color(emulsion_engine::brush::INK),
            None,
        );
        for i in 0..200 {
            let t = i as f32 / 200.0;
            stroke.point_at(40.0 + 620.0 * t, 260.0, None, Some(i as f64));
        }
        stroke.finish();
        let current = match &doc.node(paint).unwrap().kind {
            emulsion_core::NodeKind::Raster { raster, .. } => raster.clone(),
            _ => unreachable!(),
        };
        let painted = stroke.render(&current).0;
        emulsion_core::Command::ReplacePixels {
            id: paint,
            raster: Arc::new(painted),
            dirty: emulsion_raster::IRect::new(0, 0, 700, 520),
            label: "Paint".into(),
        }
        .apply(&mut doc)
        .expect("commit stroke");

        engine.reload(&doc, Some(paint), false).unwrap();
        let after = gpu_render(&mut engine, 0).unwrap();
        let changed = before
            .iter()
            .zip(&after)
            .filter(|(a, b)| (*a - *b).abs() > 1e-4)
            .count();
        assert!(
            changed > 0,
            "reload after ReplacePixels showed no change: the committed stroke is invisible"
        );
    }

    /// The same commit-and-reload cycle on the real 4K test document, which
    /// unlike a bare paint layer has masked and placed layers that compile to
    /// baked sources.
    #[test]
    fn reload_shows_pixels_on_the_layered_document() {
        let Some(gpu) = gpu() else { return };
        let mut doc = crate::testdocs::layers_4k();
        // Paint into the topmost plain raster layer, as selecting it would.
        let target = doc
            .nodes
            .iter()
            .rev()
            .find(|n| matches!(n.kind, emulsion_core::NodeKind::Raster { .. }))
            .map(|n| n.id)
            .expect("a raster layer");
        let mut engine = Engine::new(
            gpu.clone(),
            &doc,
            Some(target),
            VectorSpace::Srgb,
            true,
            true,
            (256, 256),
        )
        .unwrap();
        let before = gpu_render(&mut engine, 0).unwrap();

        let current = match &doc.node(target).unwrap().kind {
            emulsion_core::NodeKind::Raster { raster, .. } => raster.clone(),
            _ => unreachable!(),
        };
        let brush = test_brush(300.0);
        let mut stroke = emulsion_raster::paint::Stroke::new(
            current.clone(),
            brush,
            emulsion_raster::paint::Ink::Color(emulsion_engine::brush::INK),
            None,
        );
        for i in 0..300 {
            let t = i as f32 / 300.0;
            stroke.point_at(200.0 + 3400.0 * t, 1080.0, None, Some(i as f64));
        }
        stroke.finish();
        let painted = stroke.render(&current).0;
        emulsion_core::Command::ReplacePixels {
            id: target,
            raster: Arc::new(painted),
            dirty: emulsion_raster::IRect::new(0, 0, 3840, 2160),
            label: "Paint".into(),
        }
        .apply(&mut doc)
        .expect("commit stroke");

        engine.reload(&doc, Some(target), true).unwrap();
        let after = gpu_render(&mut engine, 0).unwrap();
        let changed = before
            .iter()
            .zip(&after)
            .filter(|(a, b)| (*a - *b).abs() > 1e-4)
            .count();
        assert!(
            changed > 0,
            "reload on the layered document showed no change: committed stroke invisible"
        );
    }

    /// Painting on a non-pixel layer makes the editor add a raster layer, so
    /// the reload is a *structural* change, not just new pixels. The
    /// compositor holds the serialised op program, so a reload that forgets to
    /// re-upload it invalidates the cache correctly and still draws the old
    /// document -- which is exactly what "the brush does nothing" looks like.
    #[test]
    fn reload_shows_a_layer_added_after_compile() {
        let Some(gpu) = gpu() else { return };
        let mut doc = Document::new(400, 300);
        // A background so the composite is not empty to begin with.
        crate::bench::add_paint_layer(&mut doc);
        let mut engine = Engine::new(
            gpu.clone(),
            &doc,
            None,
            VectorSpace::Srgb,
            true,
            true,
            (128, 128),
        )
        .unwrap();
        let before = gpu_render(&mut engine, 0).unwrap();
        let ops_before = engine.canvas.ops.len();

        // Add an opaque layer covering the document, as painting on a fill
        // layer would.
        let raster = emulsion_raster::Raster::solid(400, 300, [0.6, 0.12, 0.12, 1.0]);
        emulsion_core::Command::AddNode {
            node: Box::new(emulsion_core::Node::raster(
                0,
                "Added",
                Arc::new(raster),
                Default::default(),
            )),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .expect("add layer");

        engine.reload(&doc, None, true).unwrap();
        assert!(
            engine.canvas.ops.len() > ops_before,
            "the new layer did not reach the program"
        );
        let after = gpu_render(&mut engine, 0).unwrap();
        let changed = before
            .iter()
            .zip(&after)
            .filter(|(a, b)| (*a - *b).abs() > 1e-4)
            .count();
        assert!(
            changed > 0,
            "a layer added after compile is invisible: the compositor is still \
             running the old op program"
        );
    }

    /// What the pen and type tools do: add a vector node after the engine was
    /// built. The vector layer holds one encoded Vello fragment per object and
    /// the R-tree that culls them, so a reload that does not rebuild it draws
    /// the old set of paths and text -- new ones simply never appear.
    #[test]
    fn reload_shows_a_vector_node_added_after_compile() {
        let Some(gpu) = gpu() else { return };
        let mut doc = crate::testdocs::vectors(4, 1);
        let mut engine = Engine::new(
            gpu.clone(),
            &doc,
            None,
            VectorSpace::Srgb,
            true,
            true,
            (256, 256),
        )
        .unwrap();
        let before = gpu_render(&mut engine, 0).unwrap();
        let objects_before = engine.vectors.objects.len();

        // A big opaque path across the middle, as drawing with the pen would.
        let corner = |x: f64, y: f64| emulsion_raster::vector::Anchor {
            p: (x, y),
            h_in: (x, y),
            h_out: (x, y),
            smooth: false,
        };
        let path = emulsion_raster::vector::Path {
            subpaths: vec![emulsion_raster::vector::SubPath {
                anchors: vec![
                    corner(100.0, 1000.0),
                    corner(3700.0, 1000.0),
                    corner(3700.0, 1400.0),
                    corner(100.0, 1400.0),
                ],
                closed: true,
            }],
        };
        emulsion_core::Command::AddNode {
            node: Box::new(emulsion_core::Node::path(
                0,
                "Pen path",
                Arc::new(path),
                emulsion_raster::vector::PathStyle::default(),
                3840,
                2160,
            )),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .expect("add path");

        engine.reload(&doc, None, true).unwrap();
        assert!(
            engine.vectors.objects.len() > objects_before,
            "the new path did not reach the vector layer"
        );
        let after = gpu_render(&mut engine, 0).unwrap();
        let changed = before
            .iter()
            .zip(&after)
            .filter(|(a, b)| (*a - *b).abs() > 1e-4)
            .count();
        assert!(
            changed > 0,
            "a vector node added after compile is invisible: the vector layer \
             is still the one built at compile time"
        );
    }

    /// Where reload time goes on a realistic document. Not an assertion about
    /// speed; run with --nocapture to see the split.
    #[test]
    fn reload_cost_breakdown() {
        let Some(gpu) = gpu() else { return };
        let mut doc = crate::testdocs::layers_4k();
        // Add vector content, as the pen and type tools do.
        for i in 0..3 {
            let corner = |x: f64, y: f64| emulsion_raster::vector::Anchor {
                p: (x, y),
                h_in: (x, y),
                h_out: (x, y),
                smooth: false,
            };
            let x = 200.0 + 400.0 * i as f64;
            let path = emulsion_raster::vector::Path {
                subpaths: vec![emulsion_raster::vector::SubPath {
                    anchors: vec![
                        corner(x, 600.0),
                        corner(x + 300.0, 600.0),
                        corner(x + 300.0, 900.0),
                        corner(x, 900.0),
                    ],
                    closed: true,
                }],
            };
            emulsion_core::Command::AddNode {
                node: Box::new(emulsion_core::Node::path(
                    0,
                    format!("Rect {i}"),
                    Arc::new(path),
                    emulsion_raster::vector::PathStyle::default(),
                    3840,
                    2160,
                )),
                slot: emulsion_core::command::Slot::TOP,
            }
            .apply(&mut doc)
            .expect("add path");
        }
        let mut engine = Engine::new(
            gpu.clone(),
            &doc,
            None,
            VectorSpace::Srgb,
            true,
            true,
            (1600, 1000),
        )
        .unwrap();

        let mut whole = Vec::new();
        for _ in 0..5 {
            let t = std::time::Instant::now();
            engine.reload(&doc, None, true).unwrap();
            whole.push(t.elapsed().as_secs_f64() * 1000.0);
        }

        // The case that matters: a stroke committed with ReplacePixels, as the
        // editor does on every frame of a drag.
        // A layer that compiles to a direct source, which is what an ordinary
        // pixel layer does and what the editor paints into.
        let target = engine
            .canvas
            .sources
            .iter()
            .find_map(|s| s.node)
            .expect("a direct pixel source");
        let brush = test_brush(300.0);
        let mut edited = Vec::new();
        let (mut changed_tiles, mut total_tiles) = (0usize, 0usize);
        for pass in 0..5 {
            let current = match &doc.node(target).unwrap().kind {
                emulsion_core::NodeKind::Raster { raster, .. } => raster.clone(),
                _ => unreachable!(),
            };
            let mut stroke = emulsion_raster::paint::Stroke::new(
                current.clone(),
                brush,
                emulsion_raster::paint::Ink::Color(emulsion_engine::brush::INK),
                None,
            );
            let y = 400.0 + 120.0 * pass as f32;
            for i in 0..60 {
                let t = i as f32 / 60.0;
                stroke.point_at(300.0 + 1200.0 * t, y, None, Some(i as f64));
            }
            stroke.finish();
            let painted = stroke.render(&current).0;
            emulsion_core::Command::ReplacePixels {
                id: target,
                raster: Arc::new(painted),
                dirty: emulsion_raster::IRect::new(0, 0, 3840, 2160),
                label: "Paint".into(),
            }
            .apply(&mut doc)
            .expect("commit");
            // How many of this layer's tiles actually changed identity?
            let after_ptrs: std::collections::HashSet<usize> = match &doc.node(target).unwrap().kind
            {
                emulsion_core::NodeKind::Raster { raster, .. } => raster
                    .base_tiles()
                    .map(|(_, t)| t.as_ptr() as usize)
                    .collect(),
                _ => unreachable!(),
            };
            let before_ptrs: std::collections::HashSet<usize> = current
                .base_tiles()
                .map(|(_, t)| t.as_ptr() as usize)
                .collect();
            changed_tiles = after_ptrs.difference(&before_ptrs).count();
            total_tiles = after_ptrs.len();
            let t = std::time::Instant::now();
            engine.reload(&doc, None, true).unwrap();
            edited.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        // Isolate the vector re-encode.
        let mut vec_only = Vec::new();
        for _ in 0..5 {
            let t = std::time::Instant::now();
            let v = emulsion_engine::vector::VectorLayer::new(
                gpu.clone(),
                &engine.canvas,
                engine.vectors.space,
            )
            .unwrap();
            vec_only.push(t.elapsed().as_secs_f64() * 1000.0);
            drop(v);
        }
        // Split the edited-reload cost.
        let mut tree_ms = Vec::new();
        for _ in 0..5 {
            let t = std::time::Instant::now();
            let tree = doc.composite_tree();
            tree_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            std::hint::black_box(&tree);
        }
        let mut fresh_ms = Vec::new();
        for _ in 0..3 {
            let t = std::time::Instant::now();
            let c = emulsion_engine::Canvas::compile(&doc, &gpu, None, 64, true).unwrap();
            fresh_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            drop(c);
        }

        // The incremental path the spike used: replace one source's raster and
        // upload only the tiles whose identity changed.
        let mut inc_ms = Vec::new();
        if let Some(src) = engine
            .canvas
            .sources
            .iter()
            .position(|s| s.raster.base_tiles().count() == total_tiles)
        {
            for pass in 0..5 {
                let current = match &doc.node(target).unwrap().kind {
                    emulsion_core::NodeKind::Raster { raster, .. } => raster.clone(),
                    _ => unreachable!(),
                };
                let mut stroke = emulsion_raster::paint::Stroke::new(
                    current.clone(),
                    brush,
                    emulsion_raster::paint::Ink::Color(emulsion_engine::brush::INK),
                    None,
                );
                let y = 1200.0 + 80.0 * pass as f32;
                for i in 0..60 {
                    let t = i as f32 / 60.0;
                    stroke.point_at(300.0 + 1200.0 * t, y, None, Some(i as f64));
                }
                stroke.finish();
                let painted = Arc::new(stroke.render(&current).0);
                let t = std::time::Instant::now();
                let (canvas, atlas) = (&mut engine.canvas, &mut engine.atlas);
                canvas
                    .replace_raster(&gpu.queue, atlas, src, painted, None)
                    .unwrap();
                inc_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            }
        }

        let med = |mut v: Vec<f64>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        println!(
            "reload unchanged {:.2} ms; after a stroke commit {:.2} ms; \
             vector re-encode alone {:.1} ms; \
             composite_tree {:.1} ms; fresh compile {:.1} ms; \
             incremental replace_raster {:.2} ms; \
             {} of {} layer tiles changed identity; \
             {} sources, {} ops, {} vector objects",
            med(whole),
            med(edited),
            med(vec_only),
            med(tree_ms),
            med(fresh_ms),
            if inc_ms.is_empty() {
                f64::NAN
            } else {
                med(inc_ms)
            },
            changed_tiles,
            total_tiles,
            engine.canvas.sources.len(),
            engine.canvas.ops.len(),
            engine.vectors.objects.len(),
        );
    }

    #[test]
    fn gpu_dabs_match_cpu_stroke() {
        let Some(gpu) = gpu() else { return };
        let mut doc = Document::new(700, 520);
        let paint = crate::bench::add_paint_layer(&mut doc);
        let mut engine = Engine::new(
            gpu.clone(),
            &doc,
            Some(paint),
            VectorSpace::Srgb,
            false,
            true,
            (64, 64),
        )
        .unwrap();
        let source = engine.canvas.paint.expect("paint source");
        // Fill the composite cache before painting so the stroke must invalidate it.
        gpu_render(&mut engine, 0).unwrap();
        let brush = test_brush(120.0);
        let points: Vec<(f32, f32, f64)> = (0..400)
            .map(|i| {
                let t = i as f32 / 400.0;
                (40.0 + 620.0 * t, 260.0 + 180.0 * (t * 9.0).sin(), i as f64)
            })
            .collect();
        let mut stroke = GpuStroke::begin(source, brush);
        for chunk in points.chunks(37) {
            for &(x, y, _) in chunk {
                stroke.point(x, y);
            }
            let (b, c, a, e) = engine.brush_parts();
            stroke.render(b, c, a, e).unwrap();
            engine.flush();
        }
        let (_, _, atlas, encoder) = engine.brush_parts();
        let readback = stroke.finish(atlas, encoder);
        engine.flush();
        readback.map();
        gpu.wait();
        assert!(readback.is_ready());
        let raster = readback
            .complete(&gpu, &mut engine.canvas, &mut engine.atlas, source)
            .unwrap();
        let base = Arc::new(emulsion_raster::Raster::transparent(700, 520));
        let mut cpu = emulsion_raster::paint::Stroke::new(
            base.clone(),
            brush,
            emulsion_raster::paint::Ink::Color(emulsion_engine::brush::INK),
            None,
        );
        for &(x, y, t) in &points {
            cpu.point_at(x, y, None, Some(t));
        }
        cpu.finish();
        let cpu = cpu.render(&base).0;
        // Each dab rounds to 16 bits on the GPU but accumulates in f32 on the
        // CPU; drivers differ (lavapipe 5, Apple M1 15). 32/65535 is still
        // under one 8-bit code.
        let limit = match gpu.tile_format {
            emulsion_engine::gpu::TileFormat::Unorm16 => 32,
            emulsion_engine::gpu::TileFormat::Float16 => 400,
        };
        let mut max = 0;
        for y in 0..520 {
            for x in 0..700 {
                let (a, b) = (raster.get(x, y), cpu.get(x, y));
                for i in 0..4 {
                    max = max.max(a[i].abs_diff(b[i]));
                }
            }
        }
        assert!(max <= limit, "max channel difference {max}/65535");
        // The screen shows the painted layer: cached tiles were invalidated.
        if let Some(emulsion_core::NodeKind::Raster { raster: r, .. }) =
            doc.node_mut(paint).map(|n| &mut n.kind)
        {
            *r = raster.clone();
        }
        let (cpu_px, size) = cpu_reference(&doc, 0);
        let gpu_px = gpu_render(&mut engine, 0).unwrap();
        let d = compare("after stroke", size.0 as usize, &gpu_px, &cpu_px, None);
        assert!(d.max_code <= 1, "{d:?}");
    }
}
