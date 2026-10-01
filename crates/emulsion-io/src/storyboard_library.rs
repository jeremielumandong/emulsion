//! The personal storyboard library: drawings shared by every storyboard,
//! kept in the creative library beside templates and stencils. Each drawing
//! is a native ORA file managed under `storyboard-library/`; the catalog holds
//! its name, tags and kind, so the Library workspace's folders, search and
//! sync see it like any other asset.
use crate::creative_library::{self as library, Asset, AssetKind, Catalog};
use crate::{IoError, Result};
use emulsion_core::Document;
use emulsion_core::storyboard_library::{ItemKind, check_name, clean_tags};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

/// The folder, under the creative library root, holding the drawings.
pub const DIR: &str = "storyboard-library";

fn error(message: impl Into<String>) -> IoError {
    IoError::Manifest(message.into())
}

pub fn asset_kind(kind: ItemKind) -> AssetKind {
    match kind {
        ItemKind::Layers => AssetKind::StoryboardLayers,
        ItemKind::Panel => AssetKind::StoryboardPanel,
    }
}

pub fn item_kind(kind: AssetKind) -> Option<ItemKind> {
    match kind {
        AssetKind::StoryboardLayers => Some(ItemKind::Layers),
        AssetKind::StoryboardPanel => Some(ItemKind::Panel),
        _ => None,
    }
}

/// The personal library's items, in catalog order.
pub fn items(catalog: &Catalog) -> impl Iterator<Item = (&Asset, ItemKind)> {
    catalog
        .assets
        .iter()
        .filter_map(|a| item_kind(a.kind).map(|kind| (a, kind)))
}

fn item(catalog: &Catalog, id: u64) -> Result<&Asset> {
    items(catalog)
        .find(|(a, _)| a.id == id)
        .map(|(a, _)| a)
        .ok_or_else(|| error("No personal library item has that ID."))
}

/// A new file name under `dir`, unique across processes.
fn fresh_path(dir: &Path) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join(format!("{time}-{}-{seq}.ora", std::process::id()))
}

/// Add a drawing to the personal library; returns the catalog and its ID.
pub fn add(
    root: &Path,
    name: &str,
    tags: &[String],
    kind: ItemKind,
    doc: &Document,
) -> Result<(Catalog, u64)> {
    let name = name.trim().to_string();
    check_name(&name).map_err(error)?;
    let tags = clean_tags(tags).map_err(error)?;
    let mut doc = doc.clone();
    doc.selection = None;
    doc.raw_originals.clear();
    doc.validate().map_err(|e| error(e.to_string()))?;
    if !doc.nodes.iter().any(|n| n.parent.is_none()) {
        return Err(error("That drawing has no layers."));
    }
    let mut bytes = Cursor::new(Vec::new());
    crate::ora::write_to(&doc, None, &mut bytes)?;
    let dir = root.join(DIR);
    std::fs::create_dir_all(&dir)?;
    let path = fresh_path(&dir);
    crate::write_atomic(&path, |f| {
        f.write_all(bytes.get_ref())?;
        Ok(())
    })?;
    let result = library::update(root, |c| {
        let id = c.add_asset(path.clone(), asset_kind(kind))?;
        let asset = c.assets.iter_mut().find(|a| a.id == id).unwrap();
        asset.name = name;
        asset.tags = tags;
        Ok(id)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&path);
    }
    result
}

/// The drawing of a personal library item.
pub fn load(asset: &Asset) -> Result<Document> {
    if item_kind(asset.kind).is_none() {
        return Err(error("That asset is not a storyboard library item."));
    }
    crate::ora::read(&asset.path)
}

/// Rename an item and, when given, replace its tags.
pub fn rename(root: &Path, id: u64, name: &str, tags: Option<&[String]>) -> Result<Catalog> {
    let name = name.trim().to_string();
    check_name(&name).map_err(error)?;
    let tags = tags.map(clean_tags).transpose().map_err(error)?;
    library::update(root, |c| {
        item(c, id)?;
        let asset = c.assets.iter_mut().find(|a| a.id == id).unwrap();
        asset.name = name;
        if let Some(tags) = tags {
            asset.tags = tags;
        }
        Ok(())
    })
    .map(|(c, _)| c)
}

/// Remove an item; its managed drawing file is deleted with it.
pub fn remove(root: &Path, id: u64) -> Result<Catalog> {
    let (catalog, path) = library::update(root, |c| {
        let path = item(c, id)?.path.clone();
        c.remove_asset(id);
        Ok(path)
    })?;
    let managed = root.join(DIR).canonicalize().ok();
    if managed.is_some_and(|dir| path.starts_with(dir)) {
        let _ = std::fs::remove_file(path);
    }
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Node, NodeKind};

    fn drawing() -> Document {
        let mut doc = Document::new(64, 36);
        doc.nodes.push(Node::new(
            1,
            "Tree",
            NodeKind::Fill {
                rgba: [20, 120, 40, 255],
            },
        ));
        doc.next_id = 2;
        doc
    }

    #[test]
    fn personal_items_round_trip_rename_and_delete_their_files() {
        let root = std::env::temp_dir().join(format!(
            "emulsion-storyboard-library-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let (catalog, id) = add(
            &root,
            " Tree ",
            &["prop".into(), "prop".into()],
            ItemKind::Layers,
            &drawing(),
        )
        .unwrap();
        let (asset, kind) = items(&catalog).next().unwrap();
        assert_eq!((asset.id, kind), (id, ItemKind::Layers));
        assert_eq!(asset.name, "Tree");
        assert_eq!(asset.tags, ["prop"]);
        assert!(
            asset
                .path
                .starts_with(root.join(DIR).canonicalize().unwrap())
        );
        assert_eq!(load(asset).unwrap().nodes, drawing().nodes);
        let path = asset.path.clone();
        // Invalid input changes nothing.
        assert!(add(&root, "", &[], ItemKind::Panel, &drawing()).is_err());
        assert!(add(&root, "Empty", &[], ItemKind::Panel, &Document::new(8, 8)).is_err());
        assert!(rename(&root, id, " ", None).is_err());
        assert!(rename(&root, 999, "x", None).is_err());
        assert_eq!(library::load(&root).unwrap().assets.len(), 1);

        let catalog = rename(&root, id, "Oak", Some(&["set".into()])).unwrap();
        assert_eq!(catalog.assets[0].name, "Oak");
        assert_eq!(catalog.assets[0].tags, ["set"]);
        let catalog = remove(&root, id).unwrap();
        assert!(items(&catalog).next().is_none());
        assert!(!path.exists());
        assert!(remove(&root, id).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
