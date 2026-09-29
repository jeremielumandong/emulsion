//! Editable, bounded Lottie interchange. Unsupported features are diagnosed by name.
use crate::{IoError, Result};
use emulsion_core::{Document, Editor, NodeId, command::Slot, fragment::Fragment};
use serde::Serialize;
use serde_json::Value;
use std::{io::Read, path::Path};
#[path = "lottie_export.rs"]
mod export;
#[path = "lottie_import.rs"]
mod import;
#[cfg(test)]
#[path = "lottie_tests.rs"]
mod tests;
pub const MAX_BYTES: usize = 64 << 20;
#[derive(Default, Debug, Serialize)]
pub struct Report {
    pub nodes: usize,
    pub animated_nodes: usize,
    pub diagnostics: Vec<String>,
}
impl Report {
    fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        if self.diagnostics.len() < 256 && !self.diagnostics.contains(&message) {
            self.diagnostics.push(message);
        }
    }
}
fn error(s: impl Into<String>) -> IoError {
    IoError::Unsupported(format!("Lottie: {}", s.into()))
}
pub fn is_lottie(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_BYTES {
        return false;
    }
    serde_json::from_slice::<Value>(bytes).is_ok_and(|v| {
        v["layers"].is_array()
            && v["fr"].is_number()
            && v["w"].is_number()
            && v["h"].is_number()
            && v["op"].is_number()
    })
}
/// Bounded file sniff for the shared JSON/Lucid open-file route.
pub fn is_lottie_path(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .is_ok()
        && is_lottie(&bytes)
}
pub fn read(path: &Path) -> Result<(Document, Report)> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    decode(&bytes)
}
pub fn decode(bytes: &[u8]) -> Result<(Document, Report)> {
    if bytes.len() > MAX_BYTES {
        return Err(error("JSON exceeds 64 MiB"));
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|e| error(e.to_string()))?;
    import::decode(&value)
}
pub fn encode(doc: &Document) -> Result<(Vec<u8>, Report)> {
    export::encode(doc)
}
/// Place editable animation as one native Undo operation. Page timing extends as required.
pub fn insert(editor: &mut Editor, doc: &Document) -> Result<Vec<NodeId>> {
    let roots: Vec<_> = doc
        .nodes
        .iter()
        .filter(|n| n.parent.is_none())
        .map(|n| n.id)
        .collect();
    let mut fragment = Fragment::capture(doc, &roots).map_err(error)?;
    // Capture normally makes selected roots visible; imported hidden layers must stay hidden.
    for n in &mut fragment.nodes {
        n.visible = doc.node(n.id).is_some_and(|n| n.visible);
    }
    fragment.paste(editor, Slot::TOP, (0., 0.)).map_err(error)
}
