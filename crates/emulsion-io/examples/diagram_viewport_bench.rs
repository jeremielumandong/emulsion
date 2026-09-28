//! Measures vector scene preparation and physical viewport rendering during moves.
use emulsion_core::{
    Command, Editor,
    diagram::{Builder, ShapeKind},
};
use emulsion_io::svg_viewport::SvgViewport;
use std::time::Instant;
fn main() -> anyhow::Result<()> {
    let count = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(2, 1000);
    let mut b = Builder::new(2400, 1800).map_err(anyhow::Error::msg)?;
    let mut ids = Vec::new();
    for i in 0..count {
        ids.push(
            b.add_shape(
                ShapeKind::Process,
                [
                    20. + (i % 10) as f64 * 230.,
                    20. + (i / 10) as f64 * 130.,
                    200.,
                    90.,
                ],
                &format!("Service {i}\nRequest processing"),
            )
            .map_err(anyhow::Error::msg)?,
        );
    }
    let doc = b.finish().map_err(anyhow::Error::msg)?;
    let mut e = Editor::new(doc, None);
    let mut prepare = Vec::new();
    let mut render = Vec::new();
    let start = Instant::now();
    let initial = SvgViewport::new(&e.doc)?;
    let cold = start.elapsed().as_secs_f64() * 1000.;
    let mut pixels = initial.render((1440, 1080), [0.6, 0., 0., 0.6, 0., 0.])?;
    for _ in 0..21 {
        let before = e.doc.clone();
        e.execute(Command::TranslateNode {
            id: ids[count / 2],
            dx: 1.,
            dy: 0.,
        })?;
        let start = Instant::now();
        let scene = SvgViewport::new(&e.doc)?;
        prepare.push(start.elapsed().as_secs_f64() * 1000.);
        let start = Instant::now();
        let dirty = emulsion_io::svg_viewport::changed_bounds(&before, &e.doc).unwrap();
        scene.render_update((1440, 1080), [0.6, 0., 0., 0.6, 0., 0.], dirty, &mut pixels)?;
        render.push(start.elapsed().as_secs_f64() * 1000.);
    }
    prepare.remove(0);
    render.remove(0);
    prepare.sort_by(f64::total_cmp);
    render.sort_by(f64::total_cmp);
    println!(
        "shapes={count} cold_scene_ms={cold:.2} moving_scene_p50_ms={:.2} moving_scene_p95_ms={:.2} viewport_p50_ms={:.2} viewport_p95_ms={:.2}",
        prepare[10], prepare[18], render[10], render[18]
    );
    Ok(())
}
