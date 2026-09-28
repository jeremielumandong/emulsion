//! Reproducible command latency fixture; not a rendering or frame latency test.
use emulsion_core::{
    Command, Editor,
    diagram::{Builder, Endpoint, Port, Routing, ShapeKind},
};
use std::time::Instant;
fn main() -> Result<(), String> {
    let count = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1000)
        .clamp(2, 1000);
    let started = Instant::now();
    let mut builder = Builder::new(8000, 8000)?;
    let mut ids = Vec::new();
    for i in 0..count {
        ids.push(builder.add_shape(
            ShapeKind::Process,
            [
                40. + (i % 25) as f64 * 220.,
                40. + (i / 25) as f64 * 140.,
                140.,
                70.,
            ],
            &format!("Step {i}"),
        )?);
    }
    for pair in ids.windows(2) {
        builder.connect(
            Endpoint {
                shape: pair[0],
                port: Port::Auto,
            },
            Endpoint {
                shape: pair[1],
                port: Port::Auto,
            },
            "",
            Routing::Orthogonal,
        )?;
    }
    let doc = builder.finish()?;
    println!(
        "build {count} shapes / {} edges: {:.3} ms",
        count - 1,
        started.elapsed().as_secs_f64() * 1000.
    );
    for phase in ["clone", "validate", "composite_tree", "synchronize"] {
        let mut measured=Vec::new();
        for _ in 0..21 {
            let mut next=doc.clone();let start=Instant::now();
            match phase {
                "clone"=>{std::hint::black_box(doc.clone());},
                "composite_tree"=>{std::hint::black_box(doc.composite_tree());},
                "validate"=>doc.validate().map_err(|e|e.to_string())?,
                _=>emulsion_core::diagram::synchronize(&doc,&mut next)?,
            }
            measured.push(start.elapsed().as_secs_f64()*1000.);
        }
        measured.sort_by(f64::total_cmp);
        println!("{phase} p50 {:.3} ms; p95 {:.3} ms",measured[10],measured[19]);
    }
    let mut editor = Editor::new(doc, None);
    let mut times = Vec::new();
    for i in 0..20 {
        let start = Instant::now();
        editor
            .execute(Command::TranslateNode {
                id: ids[count / 2],
                dx: if i % 2 == 0 { 1. } else { -1. },
                dy: 0.,
            })
            .map_err(|e| e.to_string())?;
        times.push(start.elapsed().as_secs_f64() * 1000.);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "move shape p50 {:.3} ms; p95 {:.3} ms",
        times[10], times[18]
    );
    editor.doc.validate().map_err(|e| e.to_string())?;
    Ok(())
}
