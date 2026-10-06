//! Bounded off-thread Smart Object source IO. Opening never writes linked files.
use crate::{IoError, Result};
use emulsion_core::{
    Document, Editor, Node, NodeId, NodeKind,
    node::SmartEditable,
    smart_source::{ExternalLink, MAX_SOURCE_BYTES},
};
use emulsion_raster::{Placement, composite::flatten};
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};
fn error(s: impl Into<String>) -> IoError {
    IoError::Manifest(s.into())
}
pub fn fingerprint(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    // Reject special files before opening: opening a FIFO can wait for a writer.
    let before = std::fs::metadata(path)?;
    if !before.is_file() || before.len() > MAX_SOURCE_BYTES as u64 {
        return Err(error(
            "Linked source must be a regular file of at most 64 MiB.",
        ));
    }
    let file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_SOURCE_BYTES as u64 {
        return Err(error(
            "Linked source must be a regular file of at most 64 MiB.",
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(error("Linked source grew beyond 64 MiB."));
    }
    Ok(bytes)
}
fn check(doc: &Document) -> Result<()> {
    doc.validate()?;
    if u64::from(doc.width) * u64::from(doc.height) > 64_000_000 || doc.nodes.len() > 2000 {
        return Err(error(
            "A live Smart source supports up to 64 MP and 2,000 layers.",
        ));
    }
    Ok(())
}
struct SourceWriter(Cursor<Vec<u8>>);
impl std::io::Write for SourceWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.position().saturating_add(bytes.len() as u64) > MAX_SOURCE_BYTES as u64 {
            return Err(std::io::Error::other("Native Smart source exceeds 64 MiB."));
        }
        std::io::Write::write(&mut self.0, bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl std::io::Seek for SourceWriter {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let old = self.0.position();
        let position = std::io::Seek::seek(&mut self.0, pos)?;
        if position > MAX_SOURCE_BYTES as u64 {
            self.0.set_position(old);
            return Err(std::io::Error::other("Native Smart source exceeds 64 MiB."));
        }
        Ok(position)
    }
}
pub fn encode(doc: &Document) -> Result<Arc<Vec<u8>>> {
    check(doc)?;
    let mut bytes = SourceWriter(Cursor::new(Vec::new()));
    crate::ora::write_to(doc, None, &mut bytes)?;
    Ok(Arc::new(bytes.0.into_inner()))
}
pub fn open(doc: &Document, id: NodeId) -> Result<Document> {
    let node = doc
        .node(id)
        .ok_or_else(|| error("Smart Object was removed."))?;
    let NodeKind::Smart {
        source, editable, ..
    } = &node.kind
    else {
        return Err(error("Select a Smart Object."));
    };
    let mut result = match editable {
        Some(SmartEditable::Document { archive, .. }) => {
            crate::ora::read_from(Cursor::new(archive.as_slice()))?.doc
        }
        Some(SmartEditable::Svg { xml }) => {
            let imported = crate::svg::import(xml)?;
            if !imported.skipped.is_empty() {
                return Err(error(format!(
                    "SVG has unsupported editable elements: {}. Retained original remains unchanged.",
                    imported.skipped.join(", ")
                )));
            }
            imported.doc
        }
        _ => {
            let mut d = Document::new(source.width(), source.height());
            let kind = match editable {
                Some(SmartEditable::Text { spec }) => NodeKind::Text {
                    spec: spec.clone(),
                    cache: emulsion_core::vector_cache::VectorRaster::text(
                        spec.clone(),
                        d.width,
                        d.height,
                    ),
                },
                Some(SmartEditable::Path { path, style }) => NodeKind::Path {
                    path: path.clone(),
                    style: *style,
                    cache: emulsion_core::vector_cache::VectorRaster::path(
                        path.clone(),
                        *style,
                        d.width,
                        d.height,
                    ),
                },
                _ => NodeKind::Raster {
                    raster: source.clone(),
                    placement: Placement::default(),
                },
            };
            d.nodes.push(Node::new(1, node.name.clone(), kind));
            d.next_id = 2;
            d.design.fonts = doc.design.fonts.clone();
            d
        }
    };
    // Only a native source archive has its own document compositing identity.
    if !matches!(editable, Some(SmartEditable::Document { .. })) {
        result.blend_space = doc.blend_space;
        result.psd_background = None;
    }
    result.selection = None;
    check(&result)?;
    Ok(result)
}
/// Full persisted source structure comparison; selection is an editor-only aid.
/// Document equality alone omits metadata that an explicit source edit can change.
pub fn same_document_contents(a: &Document, b: &Document) -> bool {
    let (mut a, mut b) = (a.clone(), b.clone());
    a.selection = None;
    b.selection = None;
    a == b
        && a.source_depth == b.source_depth
        && a.next_id == b.next_id
        && a.info == b.info
        && a.colors == b.colors
        && a.drawing_guides == b.drawing_guides
}

fn ensure_source_replacement(editor: &Editor, id: NodeId) -> Result<()> {
    editor
        .doc
        .node(id)
        .ok_or_else(|| error("Smart Object was removed."))?
        .require_affine_capability("Smart source replacement")?;
    Ok(())
}

pub fn apply(editor: &mut Editor, id: NodeId, source: &Document) -> Result<()> {
    ensure_source_replacement(editor, id)?;
    emulsion_core::smart_source::ensure_editable(&editor.doc, id).map_err(error)?;
    if matches!(
        &editor.doc.node(id).unwrap().kind,
        NodeKind::Smart { editable: None, .. }
    ) {
        let original = open(&editor.doc, id)?;
        if same_document_contents(source, &original) {
            return Ok(());
        }
    }
    apply_changed(editor, id, source)
}

/// Apply a source edit already established against a captured source-session
/// baseline. Parent layer names/fonts may change while the editor is open, so
/// regenerating that baseline here could incorrectly erase a real child edit.
pub fn apply_changed(editor: &mut Editor, id: NodeId, source: &Document) -> Result<()> {
    ensure_source_replacement(editor, id)?;
    emulsion_core::smart_source::ensure_editable(&editor.doc, id).map_err(error)?;
    let archive = encode(source)?;
    if matches!(emulsion_core::smart_source::descriptor(&editor.doc,id), Ok(SmartEditable::Document{archive: old,..}) if old == &archive)
    {
        return Ok(());
    }
    let mut external = emulsion_core::smart_source::link(&editor.doc, id).cloned();
    if let Some(l) = &mut external {
        l.locally_modified = true;
    }
    let rendered = Arc::new(flatten(&source.try_composite_tree()?, 0));
    emulsion_core::smart_source::apply(editor, id, archive, external, rendered).map_err(error)
}
fn decode(path: &Path, bytes: &[u8]) -> Result<Document> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let doc = match ext.as_str() {
        "ora" => crate::ora::read_from(Cursor::new(bytes))?.doc,
        "emu" => {
            let p = crate::project::read_from(Cursor::new(bytes))?;
            if p.pages.len() != 1 {
                return Err(error(
                    "Link a single-page native source; export the desired page to .ora first.",
                ));
            }
            p.pages.into_iter().next().unwrap().doc
        }
        "svg" => {
            let text = std::str::from_utf8(bytes).map_err(|e| error(e.to_string()))?;
            let imported = crate::svg::import(text)?;
            if !imported.skipped.is_empty() {
                return Err(error(
                    "The SVG contains unsupported native editing elements. Link a rendered image or a native .ora source instead.",
                ));
            }
            imported.doc
        }
        _ => crate::import::import_bytes(
            &path.file_name().unwrap_or_default().to_string_lossy(),
            bytes,
        )?,
    };
    check(&doc)?;
    Ok(doc)
}
/// Explicit relink discards embedded local edits in one undoable step. No file writes.
pub fn relink(editor: &mut Editor, id: NodeId, path: &Path, auto_refresh: bool) -> Result<()> {
    ensure_source_replacement(editor, id)?;
    emulsion_core::smart_source::ensure_editable(&editor.doc, id).map_err(error)?;
    let path = path.canonicalize()?;
    let bytes = read_bounded(&path)?;
    let source = decode(&path, &bytes)?;
    let archive = encode(&source)?;
    let rendered = Arc::new(flatten(&source.try_composite_tree()?, 0));
    let external = ExternalLink {
        path,
        sha256: fingerprint(&bytes),
        auto_refresh,
        locally_modified: false,
    };
    emulsion_core::smart_source::apply(editor, id, archive, Some(external), rendered).map_err(error)
}
/// Returns false for unchanged source. Local edits require explicit discard=true.
pub fn refresh(editor: &mut Editor, id: NodeId, discard_local: bool) -> Result<bool> {
    ensure_source_replacement(editor, id)?;
    let link = emulsion_core::smart_source::link(&editor.doc, id)
        .cloned()
        .ok_or_else(|| error("This Smart Object is not externally linked."))?;
    let bytes = read_bounded(&link.path)?;
    if fingerprint(&bytes) == link.sha256 {
        return Ok(false);
    }
    if link.locally_modified && !discard_local {
        return Err(error(
            "The linked file and embedded source both changed. Save the local source separately or explicitly discard local edits before refresh.",
        ));
    }
    let source = decode(&link.path, &bytes)?;
    let archive = encode(&source)?;
    let rendered = Arc::new(flatten(&source.try_composite_tree()?, 0));
    let external = ExternalLink {
        sha256: fingerprint(&bytes),
        locally_modified: false,
        ..link
    };
    emulsion_core::smart_source::apply(editor, id, archive, Some(external), rendered)
        .map_err(error)?;
    Ok(true)
}
/// Native Save As creates a new layered source; refusing existing paths prevents
/// accidental overwrite. Explicit write_linked handles acknowledged existing files.
pub fn save_as(editor: &mut Editor, id: NodeId, path: &Path) -> Result<()> {
    ensure_source_replacement(editor, id)?;
    emulsion_core::smart_source::ensure_editable(&editor.doc, id).map_err(error)?;
    if !path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("ora"))
    {
        return Err(error(
            "Save an editable Smart source with the .ora extension.",
        ));
    }
    let source = open(&editor.doc, id)?;
    let archive = encode(&source)?;
    let raster = Arc::new(flatten(&source.try_composite_tree()?, 0));
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Err(error) = file.write_all(&archive).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error.into());
    }
    let link = ExternalLink {
        path: path.canonicalize()?,
        sha256: fingerprint(&archive),
        auto_refresh: false,
        locally_modified: false,
    };
    emulsion_core::smart_source::apply(editor, id, archive, Some(link), raster).map_err(error)
}
pub fn write_linked(editor: &mut Editor, id: NodeId) -> Result<()> {
    emulsion_core::smart_source::ensure_editable(&editor.doc, id).map_err(error)?;
    let mut link = emulsion_core::smart_source::link(&editor.doc, id)
        .cloned()
        .ok_or_else(|| error("Save Source As or link a file first."))?;
    let source = open(&editor.doc, id)?;
    let current = read_bounded(&link.path)?;
    if fingerprint(&current) != link.sha256 {
        return Err(error(
            "The external source changed. Refresh or save a separate source before writing.",
        ));
    }
    // Only native .ora writes can preserve all source layers. Other formats
    // remain protected; users can export separately through the source tab.
    if !link
        .path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("ora"))
    {
        return Err(error(
            "Writing layered sources requires a .ora link. Use Save Source As; the original image remains protected.",
        ));
    }
    let archive = encode(&source)?;
    // Recheck after potentially expensive native encoding, immediately before replacement.
    if fingerprint(&read_bounded(&link.path)?) != link.sha256 {
        return Err(error(
            "The linked file changed during source encoding. Nothing was written.",
        ));
    }
    crate::write_atomic(&link.path, |file| {
        use std::io::Write;
        file.write_all(&archive)?;
        Ok(())
    })?;
    link.sha256 = fingerprint(&archive);
    link.locally_modified = false;
    emulsion_core::smart_source::set_link(editor, id, Some(link)).map_err(error)
}

#[cfg(test)]
#[path = "smart_source_tests.rs"]
mod tests;
