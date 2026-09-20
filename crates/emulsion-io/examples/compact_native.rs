//! Save a compact copy and verify every document in its history.
//! cargo run --release -p emulsion-io --example compact_native -- INPUT.ora OUTPUT.ora
use anyhow::{Context, ensure};
use std::{path::PathBuf, time::Instant};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2 || (args.len() == 3 && args[2] == "--verify-only"),
        "usage: compact_native INPUT.ora OUTPUT.ora [--verify-only]"
    );
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let verify_only = args.len() == 3;
    ensure!(
        verify_only || !output.exists(),
        "output already exists; choose a new copy name"
    );
    let started = Instant::now();
    let original = emulsion_io::ora::read_full(&input)?;
    ensure!(
        original.history_error.is_none(),
        "source history is unreadable"
    );
    let old_load = started.elapsed();
    let started = Instant::now();
    if !verify_only {
        emulsion_io::ora::write_full(&original.doc, original.graph.as_ref(), &output)?;
    }
    let save = started.elapsed();
    let started = Instant::now();
    let restored = emulsion_io::ora::read_full(&output)?;
    let new_load = started.elapsed();
    ensure!(
        restored.history_error.is_none(),
        "compact history is unreadable"
    );
    verify_doc(&original.doc, &restored.doc).context("live document differs")?;
    if let Some(graph) = &original.graph {
        let other = restored.graph.as_ref().context("history missing")?;
        ensure!(
            graph.head() == other.head() && graph.len() == other.len(),
            "history differs"
        );
        ensure!(
            graph.branches() == other.branches(),
            "history branches differ"
        );
        for commit in graph.commits() {
            let other = other.commit(commit.id).context("history commit missing")?;
            ensure!(
                commit.parents == other.parents
                    && commit.name == other.name
                    && commit.time == other.time
                    && commit.auto == other.auto
                    && commit.branch == other.branch,
                "history commit differs: {}",
                commit.id
            );
            verify_doc(&commit.doc, &other.doc)
                .with_context(|| format!("history commit {} differs", commit.id))?;
        }
    }
    let old_bytes = std::fs::metadata(input)?.len();
    let new_bytes = std::fs::metadata(&output)?.len();
    println!(
        "Verified artwork and history: {old_bytes} -> {new_bytes} bytes ({:.1}% smaller)",
        100.0 * (1.0 - new_bytes as f64 / old_bytes as f64)
    );
    println!(
        "Load {:.2}s -> {:.2}s; copy: {}",
        old_load.as_secs_f64(),
        new_load.as_secs_f64(),
        output.display()
    );
    if !verify_only {
        println!("Save {:.2}s", save.as_secs_f64());
    }
    Ok(())
}

// Document equality intentionally compares pixel-buffer identities for fast
// editing. Files opened independently need a deep pixel comparison instead.
fn same_plane<P: emulsion_raster::image::Pix>(
    a: &emulsion_raster::image::Plane<P>,
    b: &emulsion_raster::image::Plane<P>,
) -> bool {
    a.width() == b.width()
        && a.height() == b.height()
        && a.fill() == b.fill()
        && a.base_tiles().count() == b.base_tiles().count()
        && a.base_tiles()
            .all(|(coord, pixels)| b.base_tile(*coord).is_some_and(|other| pixels == other))
}
fn verify_doc(a: &emulsion_core::Document, b: &emulsion_core::Document) -> anyhow::Result<()> {
    use emulsion_core::NodeKind;
    let mut b = b.clone();
    ensure!(
        a.source_depth == b.source_depth && a.next_id == b.next_id && a.info == b.info,
        "document metadata differs"
    );
    if let (Some(mask), Some(other)) = (&a.selection, &mut b.selection) {
        ensure!(same_plane(mask, other), "selection differs");
        *other = mask.clone();
    }
    ensure!(a.nodes.len() == b.nodes.len(), "layer count differs");
    for (node, other) in a.nodes.iter().zip(&mut b.nodes) {
        if let (Some(mask), Some(other)) = (&node.mask, &mut other.mask) {
            ensure!(same_plane(mask, other), "layer mask differs");
            *other = mask.clone();
        }
        match (&node.kind, &mut other.kind) {
            (NodeKind::Raster { raster, .. }, NodeKind::Raster { raster: other, .. }) => {
                ensure!(
                    same_plane(raster, other),
                    "raster pixels differ for node {}",
                    node.id
                );
                *other = raster.clone();
            }
            (
                NodeKind::Smart {
                    source,
                    cache,
                    offset,
                    ..
                },
                NodeKind::Smart {
                    source: other,
                    cache: other_cache,
                    offset: other_offset,
                    ..
                },
            ) => {
                ensure!(
                    same_plane(source, other)
                        && same_plane(cache, other_cache)
                        && offset == other_offset,
                    "smart pixels differ for node {}",
                    node.id
                );
                *other = source.clone();
            }
            (NodeKind::Path { cache, .. }, NodeKind::Path { cache: other, .. })
            | (NodeKind::Text { cache, .. }, NodeKind::Text { cache: other, .. }) => {
                ensure!(
                    same_plane(cache, other),
                    "rendered pixels differ for node {}",
                    node.id
                );
            }
            _ => {}
        }
        ensure!(
            node == other,
            "layer metadata or geometry differs for node {}",
            node.id
        );
    }
    ensure!(*a == b, "document settings differ");
    Ok(())
}
