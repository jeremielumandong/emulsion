//! Content-addressed portable media resources shared by native history snapshots.
use crate::{IoError, Result};
use emulsion_core::{
    NodeId,
    design::media::{MAX_LOCAL_ASSET_BYTES, MAX_LOCAL_PAGE_BYTES},
    design_metadata::Design,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    io::{Read, Seek},
    sync::Arc,
};
const MAX_POOL_BYTES: usize = 512 << 20;
#[derive(Default)]
pub(crate) struct MediaPool {
    values: BTreeMap<String, Arc<Vec<u8>>>,
    pointers: HashMap<usize, String>,
    total: usize,
}
impl MediaPool {
    pub(crate) fn detach(&mut self, design: &Design) -> (Design, BTreeMap<NodeId, String>) {
        let mut metadata = design.clone();
        let mut refs = BTreeMap::new();
        for (node, media) in &mut metadata.local_media {
            let pointer = Arc::as_ptr(&media.bytes) as usize;
            let hash = self
                .pointers
                .entry(pointer)
                .or_insert_with(|| {
                    Sha256::digest(media.bytes.as_slice())
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                })
                .clone();
            self.values
                .entry(hash.clone())
                .or_insert_with(|| media.bytes.clone());
            refs.insert(*node, hash);
            media.bytes = Arc::default();
        }
        (metadata, refs)
    }
    pub(crate) fn entries(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        if self
            .values
            .values()
            .try_fold(0usize, |sum, bytes| sum.checked_add(bytes.len()))
            .is_none_or(|sum| sum > MAX_POOL_BYTES)
        {
            return Err(IoError::Manifest(
                "Native media resources exceed 512 MiB. Reduce retained history before saving."
                    .into(),
            ));
        }
        Ok(self
            .values
            .iter()
            .map(|(hash, bytes)| (format!("{prefix}/{hash}.media"), bytes.as_ref().clone()))
            .collect())
    }
    pub(crate) fn restore<R: Read + Seek>(
        &mut self,
        design: &mut Design,
        refs: &BTreeMap<NodeId, String>,
        zip: &mut zip::ZipArchive<R>,
        prefix: &str,
    ) -> Result<()> {
        if refs.len() > 64 {
            return Err(IoError::Manifest(
                "Too many media resource references.".into(),
            ));
        }
        for (node, hash) in refs {
            if hash.len() != 64
                || !hash
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err(IoError::Manifest("Invalid media resource digest.".into()));
            }
            let item = design.local_media.get_mut(node).ok_or_else(|| {
                IoError::Manifest("Media reference has no corresponding object.".into())
            })?;
            if !item.bytes.is_empty() {
                return Err(IoError::Manifest(
                    "Media object has both embedded bytes and a resource reference.".into(),
                ));
            }
            let bytes = if let Some(bytes) = self.values.get(hash) {
                bytes.clone()
            } else {
                let mut entry = zip.by_name(&format!("{prefix}/{hash}.media"))?;
                if entry.size() == 0 || entry.size() > MAX_LOCAL_ASSET_BYTES as u64 {
                    return Err(IoError::Manifest(
                        "Media resource must be nonempty and at most 32 MiB.".into(),
                    ));
                }
                let mut bytes = Vec::new();
                entry
                    .by_ref()
                    .take((MAX_LOCAL_ASSET_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > MAX_LOCAL_ASSET_BYTES
                    || Sha256::digest(&bytes)
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                        != *hash
                {
                    return Err(IoError::Manifest(
                        "Media resource digest or size mismatch.".into(),
                    ));
                }
                if self.total.saturating_add(bytes.len()) > MAX_POOL_BYTES {
                    return Err(IoError::Manifest(
                        "Native media history exceeds 512 MiB.".into(),
                    ));
                }
                self.total += bytes.len();
                let bytes = Arc::new(bytes);
                self.values.insert(hash.clone(), bytes.clone());
                bytes
            };
            item.bytes = bytes;
            item.validate().map_err(IoError::Manifest)?;
        }
        if design
            .local_media
            .values()
            .map(|m| m.bytes.len())
            .sum::<usize>()
            > MAX_LOCAL_PAGE_BYTES
        {
            return Err(IoError::Manifest("Page media exceeds 64 MiB.".into()));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    #[test]
    fn design_media_archive_deduplicates_snapshots_and_restores_shared_bytes() {
        let mut design = Design::default();
        let mut media = emulsion_core::design::media::LocalMedia::from_bytes(
            "tone.wav".into(),
            b"RIFF\0\0\0\0WAVEdata".to_vec(),
        )
        .unwrap();
        media.boundary = 2;
        design.local_media.insert(1, media);
        let mut pool = MediaPool::default();
        let (mut first, refs) = pool.detach(&design);
        design.local_media.get_mut(&1).unwrap().volume = 0.2;
        let (mut second, refs2) = pool.detach(&design);
        assert_eq!(refs, refs2);
        assert!(first.local_media[&1].bytes.is_empty());
        assert_eq!(pool.entries("media").unwrap().len(), 1);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in pool.entries("media").unwrap() {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&bytes).unwrap();
        }
        let bytes = zip.finish().unwrap();
        let mut archive = zip::ZipArchive::new(bytes).unwrap();
        let mut loaded = MediaPool::default();
        loaded
            .restore(&mut first, &refs, &mut archive, "media")
            .unwrap();
        loaded
            .restore(&mut second, &refs2, &mut archive, "media")
            .unwrap();
        assert!(Arc::ptr_eq(
            &first.local_media[&1].bytes,
            &second.local_media[&1].bytes
        ));
        assert_eq!(second, design);
        let mut wrong = refs.clone();
        wrong.insert(1, "../escape".into());
        let mut empty = pool.detach(&design).0;
        assert!(
            loaded
                .restore(&mut empty, &wrong, &mut archive, "media")
                .is_err()
        );
    }
}
