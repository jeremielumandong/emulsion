//! Visible connected-object moves; reports edit, scene update, patch and zoom costs.
use emulsion_core::{
    Command, Editor,
    diagram::{Builder, Endpoint, Port, Routing, ShapeKind},
};
use emulsion_io::svg_viewport::{SvgViewport, changed_bounds};
use std::time::Instant;
fn stats(values: &mut [f64]) -> (f64, f64) {
    values.sort_by(f64::total_cmp);
    (
        values[values.len() / 2],
        values[(values.len() - 1) * 95 / 100],
    )
}
fn main() -> anyhow::Result<()> {
    let count = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(2, emulsion_core::diagram::MAX_SHAPES);
    let columns = (count as f64).sqrt().ceil() as usize;
    let mut b = Builder::new(
        (columns * 230 + 40) as u32,
        (count.div_ceil(columns) * 130 + 40) as u32,
    )
    .map_err(anyhow::Error::msg)?;
    let mut ids = Vec::new();
    for i in 0..count {
        ids.push(
            b.add_shape(
                ShapeKind::Process,
                [
                    20. + (i % columns) as f64 * 230.,
                    20. + (i / columns) as f64 * 130.,
                    200.,
                    90.,
                ],
                &format!("Service {i}\nRequest processing"),
            )
            .map_err(anyhow::Error::msg)?,
        );
    }
    for pair in ids.windows(2) {
        b.connect(
            Endpoint {
                shape: pair[0],
                port: Port::East,
            },
            Endpoint {
                shape: pair[1],
                port: Port::West,
            },
            "",
            Routing::Straight,
        )
        .map_err(anyhow::Error::msg)?;
    }
    let nested = std::env::args().any(|arg| arg == "--nested");
    let mut doc = b.finish().map_err(anyhow::Error::msg)?;
    if nested {
        let root = doc.alloc_id();
        for node in &mut doc.nodes {
            if node.parent.is_none() && !matches!(node.kind, emulsion_core::NodeKind::Fill { .. }) {
                node.parent = Some(root);
            }
        }
        doc.nodes
            .push(emulsion_core::Node::group(root, "Architecture"));
        doc.normalize();
        doc.validate()?;
    }
    let mut e = Editor::new(doc, None);
    let center = (
        120. + (count / 2 % columns) as f64 * 230.,
        65. + (count / 2 / columns) as f64 * 130.,
    );
    let matrix = [1., 0., 0., 1., 720. - center.0, 540. - center.1];
    let start = Instant::now();
    let mut scene = SvgViewport::new(&e.doc)?;
    let cold = start.elapsed().as_secs_f64() * 1000.;
    let mut pixels = scene.render((1440, 1080), matrix)?;
    let (mut edits, mut scenes, mut patches, mut rebuilt) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..21 {
        let before = e.doc.clone();
        let start = Instant::now();
        e.execute(Command::TranslateNode {
            id: ids[count / 2],
            dx: 1.,
            dy: 0.,
        })?;
        edits.push(start.elapsed().as_secs_f64() * 1000.);
        let start = Instant::now();
        let next = SvgViewport::updated(&e.doc, Some(&scene))?;
        scenes.push(start.elapsed().as_secs_f64() * 1000.);
        rebuilt.push(next.rebuilt_layers);
        let start = Instant::now();
        if let Some(dirty) = changed_bounds(&before, &e.doc) {
            next.render_update((1440, 1080), matrix, dirty, &mut pixels)?;
        } else {
            pixels = next.render((1440, 1080), matrix)?;
        }
        patches.push(start.elapsed().as_secs_f64() * 1000.);
        scene = next;
    }
    let mut zoom = Vec::new();
    for scale in [0.5, 1., 4., 16., 64.] {
        let start = Instant::now();
        scene.render(
            (1440, 1080),
            [
                scale,
                0.,
                0.,
                scale,
                720. - center.0 * scale,
                540. - center.1 * scale,
            ],
        )?;
        zoom.push(serde_json::json!({"scale":scale,"ms":start.elapsed().as_secs_f64()*1000.}));
    }
    println!(
        "{}",
        serde_json::json!({"shapes":count,"nested":nested,"connectors":count-1,"cold_scene_ms":cold,"edit_p50_p95_ms":stats(&mut edits),"scene_p50_p95_ms":stats(&mut scenes),"patch_p50_p95_ms":stats(&mut patches),"max_rebuilt_layers":rebuilt.into_iter().max(),"zoom":zoom})
    );
    Ok(())
}
