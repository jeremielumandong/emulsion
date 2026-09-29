//! Includes native command, scene update, frame submission and GPU completion.
//! Excludes OS input dispatch and compositor presentation. No readback in timed frames.
use emulsion_core::{
    Command, Editor,
    diagram::{Builder, Endpoint, Port, Routing, ShapeKind},
};
use emulsion_engine::{Camera, Engine, Gpu, Offscreen, Output, vector::VectorSpace};
use std::time::Instant;
fn stats(v: &mut [f64]) -> (f64, f64) {
    v.sort_by(f64::total_cmp);
    (v[v.len() / 2], v[(v.len() - 1) * 95 / 100])
}
fn main() -> anyhow::Result<()> {
    let count = std::env::args()
        .nth(1)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1000)
        .clamp(2, 10_000);
    let cols = (count as f64).sqrt().ceil() as usize;
    let mut b = Builder::new(
        (cols * 230 + 40) as u32,
        (count.div_ceil(cols) * 130 + 40) as u32,
    )
    .map_err(anyhow::Error::msg)?;
    let mut ids = Vec::new();
    for i in 0..count {
        ids.push(
            b.add_shape(
                ShapeKind::Process,
                [
                    20. + (i % cols) as f64 * 230.,
                    20. + (i / cols) as f64 * 130.,
                    200.,
                    90.,
                ],
                &format!("Service {i}\nRequest processing"),
            )
            .map_err(anyhow::Error::msg)?,
        );
    }
    for p in ids.windows(2) {
        b.connect(
            Endpoint {
                shape: p[0],
                port: Port::East,
            },
            Endpoint {
                shape: p[1],
                port: Port::West,
            },
            "",
            Routing::Straight,
        )
        .map_err(anyhow::Error::msg)?;
    }
    let mut editor = Editor::new(b.finish().map_err(anyhow::Error::msg)?, None);
    anyhow::ensure!(
        emulsion_engine::canvas::diagram_vector_supported(&editor.doc),
        "not eligible for vector rendering"
    );
    let gpu = Gpu::new(emulsion_engine::gpu::instance(), None, None)?;
    let adapter = gpu.adapter.get_info();
    eprintln!("GPU_ADAPTER {adapter:?}");
    let mut engine = Engine::new(
        gpu.clone(),
        &editor.doc,
        None,
        VectorSpace::Srgb,
        true,
        false,
        (1440, 1080),
    )?;
    anyhow::ensure!(
        engine.canvas.rasterized.is_empty(),
        "unexpected vector fallback"
    );
    engine.camera = Camera {
        center: [
            120. + (count / 2 % cols) as f64 * 230.,
            65. + (count / 2 / cols) as f64 * 130.,
        ],
        zoom: 1.,
    };
    let output = Offscreen::new(&gpu, (1440, 1080), wgpu::TextureFormat::Rgba8Unorm);
    engine.render(&output.view, output.format, Output::Raw)?;
    gpu.wait();
    let before_pixels = output.read(&gpu)?;
    let (mut edits, mut reloads, mut frames, mut totals) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..26 {
        let total = Instant::now();
        let t = Instant::now();
        editor.execute(Command::TranslateNode {
            id: ids[count / 2],
            dx: 1.,
            dy: 0.,
        })?;
        let edit = t.elapsed().as_secs_f64() * 1000.;
        let t = Instant::now();
        engine.reload(&editor.doc, None, true)?;
        let reload = t.elapsed().as_secs_f64() * 1000.;
        let t = Instant::now();
        engine.render(&output.view, output.format, Output::Raw)?;
        gpu.wait();
        let frame = t.elapsed().as_secs_f64() * 1000.;
        if i >= 5 {
            edits.push(edit);
            reloads.push(reload);
            frames.push(frame);
            totals.push(total.elapsed().as_secs_f64() * 1000.);
        }
    }
    println!(
        "adapter={:?} shapes={count} vectors={} edit_p50_p95_ms={:?} reload_p50_p95_ms={:?} frame_gpu_complete_p50_p95_ms={:?} command_to_gpu_complete_p50_p95_ms={:?}",
        adapter.name,
        engine.canvas.vector_count(),
        stats(&mut edits),
        stats(&mut reloads),
        stats(&mut frames),
        stats(&mut totals)
    );
    let after_pixels = output.read(&gpu)?;
    anyhow::ensure!(
        before_pixels != after_pixels,
        "movement did not update visible GPU pixels"
    );
    anyhow::ensure!(
        after_pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[0] < 240),
        "empty output"
    );
    Ok(())
}
