//! Photo-specific catalog records; source bytes stay outside the catalog.
use crate::{
    IoError, Result,
    creative_library::{Asset, AssetKind, Catalog},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SmartRule {
    pub minimum_rating: u8,
    pub color_label: u8,
    pub flagged: bool,
    pub rejected: bool,
    pub raw_only: bool,
    pub keyword: String,
}
impl SmartRule {
    pub fn matches(&self, a: &Asset) -> bool {
        a.kind == AssetKind::Image
            && a.rating >= self.minimum_rating
            && (self.color_label == 0 || a.color_label == self.color_label)
            && (!self.flagged || a.flagged)
            && (!self.rejected || a.rejected)
            && (!self.raw_only || crate::photo_develop::is_raw_photo(&a.path))
            && (self.keyword.is_empty()
                || a.tags
                    .iter()
                    .any(|k| k.to_lowercase().contains(&self.keyword.to_lowercase()))
                || a.name.to_lowercase().contains(&self.keyword.to_lowercase()))
    }
    pub fn validate(&self) -> Result<()> {
        if self.minimum_rating > 5
            || self.color_label > 5
            || self.keyword.len() > 200
            || self.flagged && self.rejected
        {
            return Err(IoError::Manifest("Invalid smart collection rule".into()));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhotoRecords {
    pub smart: BTreeMap<u64, SmartRule>,
    /// Key is the top photo; members include that photo.
    pub stacks: BTreeMap<u64, Vec<u64>>,
    pub fingerprints: BTreeMap<PathBuf, String>,
}
impl PhotoRecords {
    pub fn validate(&self, catalog: &Catalog) -> Result<()> {
        if self.smart.len() > 500 || self.stacks.len() > 10000 || self.fingerprints.len() > 100000 {
            return Err(IoError::Manifest("Photo catalog limits exceeded".into()));
        }
        for (id, rule) in &self.smart {
            rule.validate()?;
            if !catalog.collections.iter().any(|c| c.id == *id) {
                return Err(IoError::Manifest(
                    "Smart collection missing catalog entry".into(),
                ));
            }
        }
        let asset_ids: HashSet<_> = catalog.assets.iter().map(|a| a.id).collect();
        let mut seen = HashSet::new();
        for (top, members) in &self.stacks {
            if members.len() < 2
                || !members.contains(top)
                || members
                    .iter()
                    .any(|id| !seen.insert(id) || !asset_ids.contains(id))
            {
                return Err(IoError::Manifest("Invalid photo stack".into()));
            }
        }
        Ok(())
    }
}
pub fn stack(c: &mut Catalog, paths: &[PathBuf]) -> Result<()> {
    let ids: Vec<_> = paths
        .iter()
        .map(|p| {
            c.assets
                .iter()
                .find(|a| &a.path == p)
                .map(|a| a.id)
                .ok_or_else(|| IoError::Manifest("Photo is not in catalog".into()))
        })
        .collect::<Result<_>>()?;
    if ids.len() < 2 || ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err(IoError::Manifest(
            "Select at least two distinct photos".into(),
        ));
    }
    c.photos
        .stacks
        .retain(|_, members| !members.iter().any(|id| ids.contains(id)));
    c.photos.stacks.insert(ids[0], ids);
    Ok(())
}
pub fn unstack(c: &mut Catalog, paths: &[PathBuf]) {
    let ids: Vec<_> = c
        .assets
        .iter()
        .filter(|a| paths.contains(&a.path))
        .map(|a| a.id)
        .collect();
    c.photos
        .stacks
        .retain(|_, members| !members.iter().any(|id| ids.contains(id)));
}
pub fn import(c: &mut Catalog, paths: &[PathBuf], deduplicate: bool) -> Result<Vec<PathBuf>> {
    if deduplicate {
        for a in c
            .assets
            .iter()
            .filter(|a| a.kind == AssetKind::Image && a.path.is_file())
        {
            c.photos
                .fingerprints
                .insert(a.path.clone(), crate::raw::source_digest(&a.path)?);
        }
    }
    let mut imported=Vec::new();
    let mut paths_seen:HashSet<_>=c.assets.iter().map(|a|a.path.clone()).collect();
    let mut hashes:HashSet<_>=c.photos.fingerprints.values().cloned().collect();
    for path in paths {
        let path=path.canonicalize()?;let digest=crate::raw::source_digest(&path)?;
        if deduplicate&&!paths_seen.contains(&path)&&hashes.contains(&digest){continue;}
        if !paths_seen.contains(&path){if crate::photo_develop::supported(&path){c.insert_photo_reference(path.clone())?;}else{c.add_asset(path.clone(),AssetKind::Image)?;}}
        paths_seen.insert(path.clone());hashes.insert(digest.clone());c.photos.fingerprints.insert(path.clone(),digest);imported.push(path);
    }
    c.validate()?;
    Ok(imported)
}
pub fn relink(c: &mut Catalog, old: &Path, new: &Path) -> Result<()> {
    let new = new.canonicalize()?;
    let digest = crate::raw::source_digest(&new)?;
    let expected = c
        .photos
        .fingerprints
        .get(old)
        .cloned()
        .or_else(|| crate::raw::source_digest(old).ok())
        .ok_or_else(|| {
            IoError::Manifest(
                "No stored fingerprint for this missing original; import it once before moving it"
                    .into(),
            )
        })?;
    if digest != expected {
        return Err(IoError::Manifest(
            "Replacement does not match the original fingerprint".into(),
        ));
    }
    if c.assets.iter().any(|a| a.path == new && a.path != old) {
        return Err(IoError::Manifest(
            "Replacement already has a catalog entry".into(),
        ));
    }
    let index = c
        .assets
        .iter()
        .position(|a| a.path == old)
        .ok_or_else(|| IoError::Manifest("Unknown photo".into()))?;
    use crate::photo_files::Change;
    use sha2::{Digest, Sha256};
    let mut changes = vec![];
    let mut rebound = vec![];
    for asset in &c.assets {
        if !crate::photo_develop::is_virtual(&asset.path) {
            continue;
        }
        let mut reference = crate::photo_develop::reference(&asset.path)?;
        if reference.source != old {
            continue;
        }
        if reference.source_sha256 != digest {
            return Err(IoError::Manifest(
                "Virtual copy fingerprint differs from original".into(),
            ));
        }
        let before = std::fs::read(&asset.path)?;
        let previous_hash: String = Sha256::digest(&before)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        reference.source = new.clone();
        let after =
            serde_json::to_vec_pretty(&reference).map_err(|e| IoError::Manifest(e.to_string()))?;
        let next_hash: String = Sha256::digest(&after)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let sidecar = crate::raw_settings::sidecar_path(&asset.path)?;
        if sidecar.try_exists()? {
            changes.push(Change {
                before: Some(std::fs::read(&sidecar)?),
                after: crate::raw_settings::rebind_bytes(&sidecar, &previous_hash, &next_hash)?,
                path: sidecar,
            });
        }
        changes.push(Change {
            path: asset.path.clone(),
            before: Some(before),
            after,
        });
        rebound.push((asset.path.clone(), next_hash));
    }
    // Validate every virtual recipe before touching any file.
    crate::raw_settings::relink_sidecar(old, &new, &digest)?;
    crate::photo_files::apply(&changes)?;
    for (path, hash) in rebound {
        c.photos.fingerprints.insert(path, hash);
    }
    c.assets[index].path = new.clone();
    c.photos.fingerprints.remove(old);
    c.photos.fingerprints.insert(new, digest);
    Ok(())
}
/// A consistent catalog backup; never overwrites an existing destination.
pub fn backup(catalog: &Catalog, path: &Path) -> Result<()> {
    if path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("emulibrary"))
    {
        return crate::photo_backup::save(catalog, path);
    }
    catalog.validate()?;
    let bytes = serde_json::to_vec_pretty(catalog).map_err(|e| IoError::Manifest(e.to_string()))?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

pub fn restore(catalog: &mut Catalog, path: &Path, backup_directory: &Path) -> Result<()> {
    if path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("emulibrary"))
    {
        std::fs::create_dir_all(backup_directory)?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| IoError::Manifest(e.to_string()))?
            .as_nanos();
        backup(
            catalog,
            &backup_directory.join(format!("before-restore-{stamp}.json")),
        )?;
        let mut restored = crate::photo_backup::restore(path, backup_directory)?;
        restored.revision = catalog.revision;
        *catalog = restored;
        return Ok(());
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((96 << 20) + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 96 << 20 {
        return Err(IoError::Manifest("Catalog backup too large".into()));
    }
    let mut restored: Catalog =
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    restored.validate()?;
    std::fs::create_dir_all(backup_directory)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| IoError::Manifest(e.to_string()))?
        .as_nanos();
    backup(
        catalog,
        &backup_directory.join(format!("before-restore-{stamp}.json")),
    )?;
    restored.revision = catalog.revision;
    *catalog = restored;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deduplication_smart_collections_stacks_relink_and_backups_preserve_sources() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.png");
        let duplicate = dir.path().join("duplicate.png");
        let b = dir.path().join("b.png");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&duplicate, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let mut c = Catalog::default();
        assert_eq!(
            import(&mut c, &[a.clone(), duplicate, b.clone()], true)
                .unwrap()
                .len(),
            2
        );
        c.assets[0].rating = 5;
        let rule = SmartRule {
            minimum_rating: 4,
            ..Default::default()
        };
        assert!(rule.matches(&c.assets[0]));
        assert!(!rule.matches(&c.assets[1]));
        let id = c.add_collection("Stars".into(), vec![]).unwrap();
        c.photos.smart.insert(id, rule);
        stack(&mut c, &[a.clone(), b.clone()]).unwrap();
        c.validate().unwrap();
        assert_eq!(c.photos.stacks.len(), 1);
        unstack(&mut c, std::slice::from_ref(&a));
        assert!(c.photos.stacks.is_empty());
        assert!(relink(&mut c, &a, &b).is_err());
        let moved = dir.path().join("moved.png");
        let params = emulsion_core::raw::DevelopParams {
            exposure: 1.2,
            ..Default::default()
        };
        let digest = crate::raw::source_digest(&a).unwrap();
        crate::raw_settings::save_photo_settings(&a, &digest, params).unwrap();
        crate::raw_settings::save_snapshot(&a, &digest, "Before move", params).unwrap();
        std::fs::rename(&a, &moved).unwrap();
        relink(&mut c, &a, &moved).unwrap();
        assert_eq!(c.assets[0].path, moved);
        assert_eq!(
            crate::raw_settings::adjacent_settings(&moved, &digest).unwrap(),
            params
        );
        assert_eq!(
            crate::raw_settings::photo_history(&moved, &digest)
                .unwrap()
                .1["Before move"],
            params
        );
        let file = dir.path().join("backup.json");
        backup(&c, &file).unwrap();
        assert!(backup(&c, &file).is_err());
        let mut newer = c.clone();
        newer.assets[0].rating = 1;
        restore(&mut newer, &file, &dir.path().join("backups")).unwrap();
        assert_eq!(newer, c);
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
    }
}

/// Undo metadata only while the recorded result still matches the catalog.
pub fn undo_metadata(
    catalog: &mut crate::creative_library::Catalog,
    changes: &[(
        crate::creative_library::Asset,
        crate::creative_library::Asset,
    )],
) -> crate::Result<()> {
    for (_, expected) in changes {
        if catalog.assets.iter().find(|a| a.id == expected.id) != Some(expected) {
            return Err(crate::IoError::Manifest(
                "Photo metadata changed outside this undo history".into(),
            ));
        }
    }
    for (before, _) in changes {
        if let Some(asset) = catalog.assets.iter_mut().find(|a| a.id == before.id) {
            *asset = before.clone();
        }
    }
    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct RootRelinkReport {
    pub relinked: usize,
    pub metadata_only: usize,
    pub skipped: Vec<(PathBuf, String)>,
}
/// Keep successful mappings and report every skipped reference. Source bytes
/// are never changed. Known originals must match their stored fingerprints.
pub fn relink_root(c: &mut Catalog, old_root: &Path, new_root: &Path) -> Result<RootRelinkReport> {
    if !old_root.is_absolute() || !new_root.is_dir() {
        return Err(IoError::Manifest(
            "Use an absolute old root and an existing replacement folder".into(),
        ));
    }
    let new_root = new_root.canonicalize()?;
    let paths: Vec<_> = c
        .assets
        .iter()
        .filter(|a| a.kind == AssetKind::Image && a.path.starts_with(old_root))
        .map(|a| a.path.clone())
        .collect();
    let mut report = RootRelinkReport {
        relinked: 0,
        metadata_only: 0,
        skipped: vec![],
    };
    for old in paths {
        let new = new_root.join(old.strip_prefix(old_root).unwrap());
        let result = (|| -> Result<bool> {
            if c.photos.fingerprints.contains_key(&old) || old.is_file() {
                relink(c, &old, &new)?;
                return Ok(false);
            }
            // An offline imported record has no proven pixel identity. Only bind
            // metadata when there are no existing development or virtual-copy edits.
            if crate::raw_settings::sidecar_path(&old)?.exists()
                || c.assets
                    .iter()
                    .filter(|a| crate::photo_develop::is_virtual(&a.path))
                    .any(|a| {
                        crate::photo_develop::reference(&a.path).is_ok_and(|r| r.source == old)
                    })
            {
                return Err(IoError::Manifest("Unverified original has saved edits; relink it individually after verifying identity".into()));
            }
            let new = new.canonicalize()?;
            if !new.is_file()
                || !crate::photo_develop::supported(&new)
                || c.assets.iter().any(|a| a.path == new && a.path != old)
            {
                return Err(IoError::Manifest(
                    "Missing, unsupported or duplicate replacement".into(),
                ));
            }
            let digest = crate::raw::source_digest(&new)?;
            let asset = c.assets.iter_mut().find(|a| a.path == old).unwrap();
            asset.path = new.clone();
            c.photos.fingerprints.insert(new, digest);
            Ok(true)
        })();
        match result {
            Ok(metadata_only) => {
                report.relinked += 1;
                report.metadata_only += usize::from(metadata_only);
            }
            Err(e) => report.skipped.push((old, e.to_string())),
        }
    }
    c.validate()?;
    Ok(report)
}

#[cfg(test)]
mod root_relink_tests {
    use super::*;
    #[test]
    fn root_mapping_preserves_metadata_and_rejects_wrong_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        let new = dir.path().join("new");
        std::fs::create_dir(&new).unwrap();
        let photo = new.join("photo.png");
        std::fs::write(
            &photo,
            crate::export::png8(1, 1, &[80, 90, 100, 255]).unwrap(),
        )
        .unwrap();
        let mut c = Catalog::default();
        c.add_photo_reference(old.join("photo.png")).unwrap();
        c.assets[0].rating = 5;
        let report = relink_root(&mut c, &old, &new).unwrap();
        assert_eq!(report.relinked, 1);
        assert_eq!(report.metadata_only, 1);
        assert_eq!(c.assets[0].path, photo);
        assert_eq!(c.assets[0].rating, 5);
        let mismatch = dir.path().join("wrong");
        std::fs::create_dir(&mismatch).unwrap();
        std::fs::write(
            mismatch.join("photo.png"),
            crate::export::png8(1, 1, &[0, 0, 0, 255]).unwrap(),
        )
        .unwrap();
        let report = relink_root(&mut c, &new, &mismatch).unwrap();
        assert_eq!(report.relinked, 0);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(c.assets[0].path, photo);
    }
}
