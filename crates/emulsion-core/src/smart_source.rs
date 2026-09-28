//! Persistent editable Smart Object source descriptors. Disk IO belongs to emulsion-io.
use crate::{Document, Editor, NodeId, NodeKind, node::SmartEditable};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

pub const MAX_SOURCE_BYTES: usize = 64 << 20;
pub const MAX_SOURCE_DEPTH: usize = 8;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalLink {
    pub path: PathBuf,
    pub sha256: String,
    #[serde(default)]
    pub auto_refresh: bool,
    /// Local source edits have not been explicitly written to the linked file.
    #[serde(default)]
    pub locally_modified: bool,
}
impl ExternalLink {
    pub fn validate(&self) -> Result<(), String> {
        if !self.path.is_absolute()
            || self.path.as_os_str().len() > 4096
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("Invalid Smart Object external source path or fingerprint.".into());
        }
        Ok(())
    }
}
pub fn descriptor(doc: &Document, id: NodeId) -> Result<&SmartEditable, String> {
    match &doc.node(id).ok_or("The source object was removed.")?.kind {
        NodeKind::Smart {
            editable: Some(editable),
            ..
        } => Ok(editable),
        _ => Err("Select a Smart Object with an editable source.".into()),
    }
}
pub fn link(doc: &Document, id: NodeId) -> Option<&ExternalLink> {
    match descriptor(doc, id).ok()? {
        SmartEditable::Document { external, .. } => external.as_ref(),
        _ => None,
    }
}
pub fn ensure_editable(doc: &Document, id: NodeId) -> Result<(), String> {
    let n = doc.node(id).ok_or("The source object was removed.")?;
    let locks = doc.layer_locks(id);
    if doc.locked_ancestor(id).is_some() || locks.pixels || locks.position || locks.transparency {
        return Err("Unlock the Smart Object before editing its source.".into());
    }
    if !matches!(n.kind, NodeKind::Smart { .. }) {
        return Err("Select a Smart Object.".into());
    }
    Ok(())
}
/// The rendered source is a disposable cache; its native archive is retained.
/// Uses the replacement primitive to preserve placement, filters and mask scaling.
pub fn apply(
    editor: &mut Editor,
    id: NodeId,
    archive: Arc<Vec<u8>>,
    external: Option<ExternalLink>,
    rendered: Arc<emulsion_raster::Raster>,
) -> Result<(), String> {
    ensure_editable(&editor.doc, id)?;
    if archive.is_empty() || archive.len() > MAX_SOURCE_BYTES {
        return Err("Smart source archive must be between 1 byte and 64 MiB.".into());
    }
    if let Some(link) = &external {
        link.validate()?;
    }
    let mut trial = Editor::new(editor.doc.clone(), None);
    crate::photo_source::replace(&mut trial, id, rendered)?;
    let NodeKind::Smart { editable, .. } = &mut trial.doc.node_mut(id).unwrap().kind else {
        unreachable!()
    };
    *editable = Some(SmartEditable::Document { archive, external });
    editor.commit_design_document(trial.doc, "Update Smart Object source")
}
pub fn set_link(
    editor: &mut Editor,
    id: NodeId,
    external: Option<ExternalLink>,
) -> Result<(), String> {
    ensure_editable(&editor.doc, id)?;
    if let Some(link) = &external {
        link.validate()?;
    }
    let mut doc = editor.doc.clone();
    match &mut doc.node_mut(id).unwrap().kind {
        NodeKind::Smart {
            editable: Some(SmartEditable::Document { external: link, .. }),
            ..
        } => *link = external,
        _ => return Err("Create an editable source document first.".into()),
    }
    editor.commit_design_document(doc, "Change Smart Object link")
}
