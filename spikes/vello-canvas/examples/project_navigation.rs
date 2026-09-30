//! Offscreen navigation of an unmodified project; includes GPU completion.
use emulsion_engine::{Camera, Engine, Gpu, Offscreen, Output, vector::VectorSpace};
use std::{path::PathBuf, time::Instant};

fn stats(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    serde_json::json!({"p50": values[values.len() / 2], "p95": values[values.len() * 95 / 100]})
}

fn main() -> anyhow::Result<()> {
    let path = PathBuf::from(std::env::args_os().nth(1).expect("project path"));
    let project = emulsion_io::project::read(&path)?;
    let doc = &project
        .pages
        .iter()
        .find(|page| page.meta.id == project.active)
        .expect("active project page")
        .doc;
    let gpu = Gpu::new(emulsion_engine::gpu::instance(), None, None)?;
    let screen = (1600, 1000);
    let mut engine = Engine::new(
        gpu.clone(),
        doc,
        None,
        VectorSpace::Srgb,
        true,
        true,
        screen,
    )?;
    anyhow::ensure!(
        engine.canvas.unsupported.is_empty(),
        "{:?}",
        engine.canvas.unsupported
    );
    let output = Offscreen::new(&gpu, screen, wgpu::TextureFormat::Rgba8Unorm);
    let mut rows = Vec::new();
    for (name, zoom, pan) in [
        ("pan_fit", 0.8, true),
        ("pan_100", 1.0, true),
        ("zoom", 1.0, false),
        ("pan_200", 2.0, true),
    ] {
        let mut elapsed = Vec::new();
        let mut encode = Vec::new();
        let mut render = Vec::new();
        let mut composite = Vec::new();
        for frame in 0..50 {
            let offset = if frame % 2 == 0 { 4.0 } else { 0.0 };
            engine.camera = Camera {
                center: [
                    doc.width as f64 / 2.0 + if pan { offset } else { 0.0 },
                    doc.height as f64 / 2.0,
                ],
                zoom: zoom * if !pan && offset > 0.0 { 1.08 } else { 1.0 },
            };
            let start = Instant::now();
            let times = engine.render(&output.view, output.format, Output::Raw)?;
            gpu.wait();
            if frame >= 10 {
                elapsed.push(start.elapsed().as_secs_f64() * 1000.0);
                encode.push(times.vector_encode_ms);
                render.push(times.vector_render_ms);
                composite.push(times.composite_ms);
            }
        }
        rows.push(serde_json::json!({"case": name, "gpu_complete_ms": stats(elapsed), "vector_encode_ms": stats(encode), "vector_submit_ms": stats(render), "composite_submit_ms": stats(composite)}));
    }
    let mut editor = emulsion_core::Editor::new(doc.clone(), None);
    let id = doc
        .nodes
        .iter()
        .rev()
        .find(|node| node.visible && matches!(node.kind, emulsion_core::NodeKind::Path { .. }))
        .expect("editable path")
        .id;
    let mut edit = Vec::new();
    let mut reload = Vec::new();
    for frame in 0..30 {
        let start = Instant::now();
        editor.execute(emulsion_core::Command::TranslateNode {
            id,
            dx: if frame % 2 == 0 { 2.0 } else { -2.0 },
            dy: 0.0,
        })?;
        let edited = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        engine.reload(&editor.doc, None, true)?;
        let reloaded = start.elapsed().as_secs_f64() * 1000.0;
        engine.render(&output.view, output.format, Output::Raw)?;
        gpu.wait();
        if frame >= 10 {
            edit.push(edited);
            reload.push(reloaded);
        }
    }
    println!(
        "{}",
        serde_json::json!({"adapter": gpu.adapter.get_info().name, "nodes": doc.nodes.len(), "runs": engine.canvas.runs.len(), "ops": engine.canvas.ops.len(), "rasterized": engine.canvas.rasterized, "move_edit_ms": stats(edit), "move_reload_ms": stats(reload), "results": rows})
    );
    Ok(())
}
