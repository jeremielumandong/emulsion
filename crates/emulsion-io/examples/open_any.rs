//! Open any file Emulsion can import and report what came out.
//!
//!     cargo run -p emulsion-io --example open_any -- picture.heic other.xcf
//!
//! Prints size, source depth and layers, or the error a person would see.
//! Handy for checking a converter (ImageMagick, heif-convert, avifdec,
//! pdftoppm) is wired up on this machine.

fn main() {
    let paths: Vec<_> = std::env::args_os().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: open_any <file>...");
        std::process::exit(2);
    }
    let mut failed = false;
    for p in paths {
        let path = std::path::Path::new(&p);
        let t = std::time::Instant::now();
        match emulsion_io::open(path) {
            Ok(doc) => {
                let layers: Vec<String> = doc
                    .nodes
                    .iter()
                    .map(|n| format!("{}{}", n.name, if n.visible { "" } else { " (hidden)" }))
                    .collect();
                println!(
                    "{}: {}×{} {}-bit, {} layer(s) [{}] in {:.0} ms",
                    path.display(),
                    doc.width,
                    doc.height,
                    doc.source_depth,
                    doc.nodes.len(),
                    layers.join(", "),
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
            Err(e) => {
                failed = true;
                println!("{}: {e}", path.display());
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
