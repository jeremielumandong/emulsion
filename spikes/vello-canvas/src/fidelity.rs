//! Pixel diff of the spike's GPU composite against Emulsion's CPU reference
//! compositor (what the GPUI canvas presents), at 100% and zoomed-out levels.

use crate::compositor::Camera;
use crate::engine::{Engine, Offscreen, Output};
use crate::gpu::Gpu;
use crate::vector::VectorSpace;
use emulsion_core::Document;
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
pub fn gpu_render(engine: &mut Engine, level: u32) -> anyhow::Result<Vec<f32>> {
    let size = level_size(engine.canvas.width, engine.canvas.height, level);
    let target = Offscreen::new(&engine.gpu, size, wgpu::TextureFormat::Rgba32Float);
    engine.screen = size;
    let zoom = 1.0 / (1u32 << level) as f64;
    engine.camera = Camera {
        center: [size.0 as f64 / 2.0 / zoom, size.1 as f64 / 2.0 / zoom],
        zoom,
    };
    engine.render(&target.view, target.format, Output::Raw)?;
    let bytes = target.read(&engine.gpu)?;
    Ok(bytemuck::cast_slice::<u8, f32>(&bytes).to_vec())
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
    use crate::brush::{GpuStroke, test_brush};
    use emulsion_raster::blend::BlendSpace;

    /// A GPU, or `None` to skip. `EMULSION_REQUIRE_GPU_TESTS=1` makes a
    /// missing adapter a failure, as in `emulsion-gpu`.
    fn gpu() -> Option<Arc<Gpu>> {
        match Gpu::new(crate::gpu::instance(), None, None) {
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
                    if gpu.tile_format == crate::gpu::TileFormat::Unorm16 {
                        assert!(d.max_code <= 1, "{space:?} level {level}: {d:?}");
                        if level == 0 {
                            assert!(d.max_linear < 1e-4, "{space:?}: {d:?}");
                        }
                    }
                }
            }
        }
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
            emulsion_raster::paint::Ink::Color(crate::brush::INK),
            None,
        );
        for &(x, y, t) in &points {
            cpu.point_at(x, y, None, Some(t));
        }
        cpu.finish();
        let cpu = cpu.render(&base).0;
        let limit = match gpu.tile_format {
            crate::gpu::TileFormat::Unorm16 => 16,
            crate::gpu::TileFormat::Float16 => 400,
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
