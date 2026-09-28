//! Content-addressed native source documents, shared across history snapshots.
use crate::{IoError, Result};
use emulsion_core::{node::SmartEditable, smart_source::MAX_SOURCE_BYTES};
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
    sync::Arc,
};
#[derive(Default)]
pub(crate) struct SourcePool(BTreeMap<String, Arc<Vec<u8>>>);
impl SourcePool {
    pub fn reference(&mut self, editable: &Option<SmartEditable>) -> Option<String> {
        if let Some(SmartEditable::Document { archive, .. }) = editable {
            let key = self
                .0
                .iter()
                .find(|(_, v)| Arc::ptr_eq(v, archive))
                .map(|(k, _)| k.clone())
                .unwrap_or_else(|| crate::smart_source::fingerprint(archive.as_slice()));
            self.0.insert(key.clone(), archive.clone());
            Some(key)
        } else {
            None
        }
    }
    pub fn stripped(editable: &Option<SmartEditable>) -> Option<SmartEditable> {
        let mut v = editable.clone();
        if let Some(SmartEditable::Document { archive, .. }) = &mut v {
            *archive = Arc::new(Vec::new());
        }
        v
    }
    pub fn entries(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        if self
            .0
            .values()
            .any(|b| b.is_empty() || b.len() > MAX_SOURCE_BYTES)
            || self.0.values().map(|b| b.len()).sum::<usize>() > 512 << 20
        {
            return Err(IoError::Manifest(
                "Smart source resources exceed bounded archive limits.".into(),
            ));
        }
        Ok(self
            .0
            .iter()
            .map(|(k, v)| (format!("{prefix}/{k}.ora"), v.as_ref().clone()))
            .collect())
    }
    pub fn restore<R: Read + Seek>(
        &mut self,
        mut editable: Option<SmartEditable>,
        reference: Option<String>,
        zip: &mut zip::ZipArchive<R>,
        prefix: &str,
    ) -> Result<Option<SmartEditable>> {
        match (&mut editable, reference) {
            (Some(SmartEditable::Document { archive, .. }), Some(hash)) => {
                if hash.len() != 64
                    || !hash.bytes().all(|c| c.is_ascii_hexdigit())
                    || !archive.is_empty()
                {
                    return Err(IoError::Manifest("Invalid Smart source reference.".into()));
                }
                if let Some(v) = self.0.get(&hash) {
                    *archive = v.clone();
                } else {
                    let mut entry = zip.by_name(&format!("{prefix}/{hash}.ora"))?;
                    if entry.size() > MAX_SOURCE_BYTES as u64
                        || self.0.values().map(|b| b.len() as u64).sum::<u64>() + entry.size()
                            > 512 << 20
                    {
                        return Err(IoError::Manifest(
                            "Smart source archive exceeds limits.".into(),
                        ));
                    }
                    let mut bytes = Vec::new();
                    entry
                        .by_ref()
                        .take((MAX_SOURCE_BYTES + 1) as u64)
                        .read_to_end(&mut bytes)?;
                    if bytes.is_empty()
                        || bytes.len() > MAX_SOURCE_BYTES
                        || crate::smart_source::fingerprint(&bytes) != hash
                    {
                        return Err(IoError::Manifest(
                            "Smart source resource digest mismatch.".into(),
                        ));
                    }
                    *archive = Arc::new(bytes);
                    self.0.insert(hash, archive.clone());
                }
            }
            (Some(SmartEditable::Document { .. }), None) | (_, Some(_)) => {
                return Err(IoError::Manifest(
                    "Missing or mismatched Smart source reference.".into(),
                ));
            }
            _ => {}
        }
        Ok(editable)
    }
}
