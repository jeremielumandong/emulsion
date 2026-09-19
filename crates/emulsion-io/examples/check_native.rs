//! Read-only check of a native project's artwork and saved history.
//! cargo run --release -p emulsion-io --example check_native -- FILE.ora

use emulsion_core::NodeKind;

fn main() -> anyhow::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: check_native FILE.ora"))?;
    let start = std::time::Instant::now();
    let opened = emulsion_io::ora::read_full(std::path::Path::new(&path))?;
    let paths = opened
        .doc
        .nodes
        .iter()
        .filter(|node| matches!(node.kind, NodeKind::Path { .. }))
        .count();
    println!(
        "Opened {}×{}: {} nodes, {paths} editable paths, {} history commits in {:.2}s",
        opened.doc.width,
        opened.doc.height,
        opened.doc.nodes.len(),
        opened.graph.as_ref().map_or(0, |graph| graph.len()),
        start.elapsed().as_secs_f64()
    );
    if let Some(error) = opened.history_error {
        anyhow::bail!("Artwork opened, but history did not: {error}");
    }
    Ok(())
}
