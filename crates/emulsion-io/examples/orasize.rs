//! Where the bytes of a saved document go:
//! `cargo run --release -p emulsion-io --example orasize -- photo.ARW out.ora`
fn main() {
    let path = std::env::args().nth(1).expect("image path");
    let out = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "target/orasize.ora".into());
    let doc = emulsion_io::raw::open(std::path::Path::new(&path)).expect("open");
    let t = std::time::Instant::now();
    emulsion_io::save(&doc, std::path::Path::new(&out)).expect("save");
    println!("saved in {:?} → {out}", t.elapsed());
}
