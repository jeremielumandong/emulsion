//! Measure the retained SVG viewport with an unmodified project.
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let file = std::env::args_os().nth(1).expect("project path");
    let project = emulsion_io::project::read(std::path::Path::new(&file))?;
    let doc = &project
        .pages
        .iter()
        .find(|p| p.meta.id == project.active)
        .unwrap()
        .doc;
    let started = Instant::now();
    let scene = emulsion_io::svg_viewport::SvgViewport::new(doc)?;
    println!("build_ms={:.2}", started.elapsed().as_secs_f64() * 1000.);
    for zoom in [0.8, 1., 2., 4., 8., 16.] {
        let mut times = Vec::new();
        for frame in 0..5 {
            let start = Instant::now();
            let image = scene.render(
                (1600, 1000),
                [
                    zoom,
                    0.,
                    0.,
                    zoom,
                    800. - zoom * (doc.width as f64 / 2. + frame as f64),
                    500. - zoom * doc.height as f64 / 2.,
                ],
            )?;
            std::hint::black_box(image);
            times.push(start.elapsed().as_secs_f64() * 1000.);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "zoom={zoom} median_ms={:.2} max_ms={:.2}",
            times[2], times[4]
        );
    }
    Ok(())
}
