//! Open any file Emulsion can import and report what came out.
//!
//!     cargo run -p emulsion-io --example open_any -- picture.heic other.xcf
//!
//! Prints size, source depth and layers, or the error a person would see.
//! `--pixel X,Y` also prints that pixel of the first layer as sRGB 8-bit,
//! for checking colour management.
//! Handy for checking a converter (ImageMagick, heif-convert, avifdec,
//! pdftoppm) is wired up on this machine.

fn main() {
    let mut paths: Vec<std::ffi::OsString> = Vec::new();
    let mut pixel: Option<(u32, u32)> = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        if a == "--pixel" {
            let v = args
                .next()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let (x, y) = v.split_once(',').unwrap_or(("0", "0"));
            pixel = Some((x.parse().unwrap_or(0), y.parse().unwrap_or(0)));
        } else {
            paths.push(a);
        }
    }
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
                if let (Some((x, y)), Some(emulsion_core::NodeKind::Raster { raster, .. })) =
                    (pixel, doc.nodes.first().map(|n| &n.kind))
                    && x < raster.width()
                    && y < raster.height()
                {
                    let px = raster.to_srgba8();
                    let i = ((y * raster.width() + x) * 4) as usize;
                    println!("  pixel {x},{y}: sRGB {:?}", &px[i..i + 4]);
                }
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
