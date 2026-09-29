//! Rebuildable photo read index. Creative IDs and the shared catalog remain authoritative.
use crate::{
    IoError, Result,
    creative_library::{AssetKind, Catalog},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Clone, Serialize, Deserialize)]
struct Row {
    id: u64,
    path: PathBuf,
    text: String,
    rating: u8,
    label: u8,
    flagged: bool,
    rejected: bool,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Index {
    version: u32,
    pub revision: u64,
    rows: BTreeMap<u64, Row>,
    paths: BTreeMap<PathBuf, u64>,
    ratings: BTreeMap<u8, BTreeSet<u64>>,
    labels: BTreeMap<u8, BTreeSet<u64>>,
    picked: BTreeSet<u64>,
    rejected: BTreeSet<u64>,
}
impl Index {
    pub fn build(catalog: &Catalog) -> Self {
        let mut index = Self {
            version: 1,
            revision: catalog.revision,
            ..Default::default()
        };
        for a in catalog.assets.iter().filter(|a| a.kind == AssetKind::Image) {
            let text = std::iter::once(
                a.path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase(),
            )
            .chain(a.tags.iter().map(|t| t.to_lowercase()))
            .collect::<Vec<_>>()
            .join("\n");
            index.paths.insert(a.path.clone(), a.id);
            index.ratings.entry(a.rating).or_default().insert(a.id);
            index.labels.entry(a.color_label).or_default().insert(a.id);
            if a.flagged {
                index.picked.insert(a.id);
            }
            if a.rejected {
                index.rejected.insert(a.id);
            }
            index.rows.insert(
                a.id,
                Row {
                    id: a.id,
                    path: a.path.clone(),
                    text,
                    rating: a.rating,
                    label: a.color_label,
                    flagged: a.flagged,
                    rejected: a.rejected,
                },
            );
        }
        index
    }
    pub fn search(
        &self,
        query: &str,
        rating: u8,
        label: u8,
        picked: bool,
        rejected: bool,
    ) -> BTreeSet<u64> {
        let query = query.to_lowercase();
        let mut ids: BTreeSet<u64> = if picked {
            self.picked.clone()
        } else if rejected {
            self.rejected.clone()
        } else if label > 0 {
            self.labels.get(&label).cloned().unwrap_or_default()
        } else {
            self.ratings
                .range(rating..)
                .flat_map(|(_, ids)| ids.iter().copied())
                .collect()
        };
        ids.retain(|id| {
            self.rows.get(id).is_some_and(|r| {
                r.rating >= rating
                    && (label == 0 || r.label == label)
                    && (!picked || r.flagged)
                    && (!rejected || r.rejected)
                    && (query.is_empty() || r.text.contains(&query))
            })
        });
        ids
    }
    pub fn id(&self, path: &Path) -> Option<u64> {
        self.paths.get(path).copied()
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        std::fs::create_dir_all(root)?;
        crate::write_atomic(&root.join("photos.index.json"), |file| {
            serde_json::to_writer(file, self).map_err(|e| IoError::Manifest(e.to_string()))
        })
    }
    /// A missing/corrupt/stale derivative is rebuilt; catalog updates never depend on it.
    pub fn load(root: &Path, catalog: &Catalog) -> Self {
        let read = (|| -> Result<Self> {
            let mut bytes = vec![];
            std::fs::File::open(root.join("photos.index.json"))?
                .take(96 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 96 * 1024 * 1024 {
                return Err(IoError::Manifest("Photo index exceeds limit".into()));
            }
            serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))
        })();
        if let Ok(index) = read
            && index.version == 1
            && index.revision == catalog.revision
            && index.rows.len()
                == catalog
                    .assets
                    .iter()
                    .filter(|a| a.kind == AssetKind::Image)
                    .count()
        {
            // Rebuild secondary postings from verified source metadata. Cached rows
            // are never a source of truth for edits, IDs, paths or permissions.
            let expected = Self::build(catalog);
            if index.rows.iter().all(|(id, r)| {
                expected.rows.get(id).is_some_and(|a| {
                    a.path == r.path
                        && a.text == r.text
                        && a.rating == r.rating
                        && a.label == r.label
                        && a.flagged == r.flagged
                        && a.rejected == r.rejected
                })
            }) {
                return expected;
            }
        }
        Self::build(catalog)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_matches_filters_and_survives_stale_or_corrupt_cache() {
        let mut c = Catalog::default();
        for i in 0..1000 {
            let id = c
                .insert_photo_reference(PathBuf::from(format!("/offline/photo-{i}.nef")))
                .unwrap();
            let a = c.assets.last_mut().unwrap();
            assert_eq!(a.id, id);
            a.rating = (i % 6) as u8;
            a.color_label = (i % 6) as u8;
            a.flagged = i % 7 == 0;
            a.tags = vec![
                if i % 2 == 0 {
                    "Family > Summer"
                } else {
                    "Winter"
                }
                .into(),
            ];
        }
        let index = Index::build(&c);
        let found = index.search("summer", 3, 0, false, false);
        let expected = c
            .assets
            .iter()
            .filter(|a| a.rating >= 3 && a.tags[0].contains("Summer"))
            .map(|a| a.id)
            .collect();
        assert_eq!(found, expected);
        let dir = tempfile::tempdir().unwrap();
        index.save(dir.path()).unwrap();
        assert_eq!(
            Index::load(dir.path(), &c).search("summer", 3, 0, false, false),
            found
        );
        c.assets[0].rating = 5;
        c.revision += 1;
        assert!(
            Index::load(dir.path(), &c)
                .search("photo-0", 5, 0, false, false)
                .contains(&c.assets[0].id)
        );
        std::fs::write(dir.path().join("photos.index.json"), b"bad").unwrap();
        assert_eq!(Index::load(dir.path(), &c).len(), 1000);
    }
    #[test]
    #[ignore = "large catalog benchmark"]
    fn hundred_thousand_photo_index() {
        let start = std::time::Instant::now();
        let mut c = Catalog::default();
        for i in 0..100000 {
            c.insert_photo_reference(PathBuf::from(format!("/offline/capture-{i}.nef")))
                .unwrap();
            c.assets.last_mut().unwrap().rating = (i % 6) as u8;
        }
        let index = Index::build(&c);
        let build = start.elapsed();
        let query = std::time::Instant::now();
        let hits = index.search("capture-999", 3, 0, false, false);
        println!(
            "100000 photos: build {:?}, query {:?}, {} hits",
            build,
            query.elapsed(),
            hits.len()
        );
        assert!(!hits.is_empty());
        assert_eq!(index.len(), 100000);
    }
}
