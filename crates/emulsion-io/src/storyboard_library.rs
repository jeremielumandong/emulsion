//! The personal storyboard library: drawings shared by every storyboard,
//! kept in the creative library beside templates and stencils. Each item is
//! a file managed under `storyboard-library/`; the catalog holds its name,
//! tags and kind, so the Library workspace's folders, search and sync see it
//! like any other asset. A plain drawing is a native ORA file. An animated
//! panel item or a scene item is a small package (`.emsb`): `item.json`
//! with its timing, keyframes, comps and camera, and one ORA drawing per
//! panel under `drawings/`. Items saved before animation stay ORA files.
use crate::creative_library::{self as library, Asset, AssetKind, Catalog};
use crate::{IoError, Result};
use emulsion_core::Document;
use emulsion_core::storyboard_library::{
    ItemAnimation, ItemKind, LibraryItem, MAX_SCENE_PANELS, check_name, clean_tags,
};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

/// The folder, under the creative library root, holding the drawings.
pub const DIR: &str = "storyboard-library";

fn error(message: impl Into<String>) -> IoError {
    IoError::Manifest(message.into())
}

pub fn asset_kind(kind: ItemKind) -> AssetKind {
    match kind {
        ItemKind::Layers => AssetKind::StoryboardLayers,
        ItemKind::Panel => AssetKind::StoryboardPanel,
        ItemKind::Scene => AssetKind::StoryboardScene,
    }
}

pub fn item_kind(kind: AssetKind) -> Option<ItemKind> {
    match kind {
        AssetKind::StoryboardLayers => Some(ItemKind::Layers),
        AssetKind::StoryboardPanel => Some(ItemKind::Panel),
        AssetKind::StoryboardScene => Some(ItemKind::Scene),
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

/// The package entry holding an animated item's data.
const PACKAGE_ENTRY: &str = "item.json";
/// Largest package entry read.
const MAX_ENTRY: u64 = 1 << 30;

#[derive(Serialize, Deserialize)]
struct Package {
    kind: ItemKind,
    animation: ItemAnimation,
}

fn drawing_entry(n: usize) -> String {
    format!("drawings/{n}.ora")
}

/// A new file name under `dir`, unique across processes.
fn fresh_path(dir: &Path, extension: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join(format!("{time}-{}-{seq}.{extension}", std::process::id()))
}

/// Add a drawing to the personal library; returns the catalog and its ID.
pub fn add(
    root: &Path,
    name: &str,
    tags: &[String],
    kind: ItemKind,
    doc: &Document,
) -> Result<(Catalog, u64)> {
    add_item(root, name, tags, &LibraryItem::drawing(kind, doc.clone()))
}

/// Add an item made by `LibraryItem::drawing` or a capture
/// (`ProjectEditor::capture_panel_item`, `capture_scene_item`); returns the
/// catalog and its ID. Items with animation are saved as packages.
pub fn add_item(
    root: &Path,
    name: &str,
    tags: &[String],
    item: &LibraryItem,
) -> Result<(Catalog, u64)> {
    let name = name.trim().to_string();
    check_name(&name).map_err(error)?;
    let tags = clean_tags(tags).map_err(error)?;
    let mut item = item.clone();
    for doc in std::iter::once(&mut item.doc).chain(&mut item.more) {
        let doc = Arc::make_mut(doc);
        doc.selection = None;
        doc.raw_originals.clear();
    }
    item.name = name.clone();
    item.validate_content().map_err(error)?;
    let mut bytes = Cursor::new(Vec::new());
    let extension = match &item.animation {
        None => {
            crate::ora::write_to(&item.doc, None, &mut bytes)?;
            "ora"
        }
        Some(animation) => {
            write_package(&item, animation, &mut bytes)?;
            "emsb"
        }
    };
    let dir = root.join(DIR);
    std::fs::create_dir_all(&dir)?;
    let path = fresh_path(&dir, extension);
    crate::write_atomic(&path, |f| {
        f.write_all(bytes.get_ref())?;
        Ok(())
    })?;
    let result = library::update(root, |c| {
        let id = c.add_asset(path.clone(), asset_kind(item.kind))?;
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

fn write_package<W: Write + Seek>(
    item: &LibraryItem,
    animation: &ItemAnimation,
    writer: W,
) -> Result<()> {
    let package = Package {
        kind: item.kind,
        animation: animation.clone(),
    };
    let json = serde_json::to_vec(&package).map_err(|e| error(e.to_string()))?;
    let mut zip = ZipWriter::new(writer);
    zip.start_file(PACKAGE_ENTRY, SimpleFileOptions::default())?;
    zip.write_all(&json)?;
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (n, doc) in item.drawings().enumerate() {
        let mut bytes = Cursor::new(Vec::new());
        crate::ora::write_to(doc, None, &mut bytes)?;
        zip.start_file(drawing_entry(n), stored.large_file(true))?;
        zip.write_all(bytes.get_ref())?;
    }
    zip.finish()?.flush()?;
    Ok(())
}

/// An item package's kind, animation and drawings.
fn read_package<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
) -> Result<(ItemKind, ItemAnimation, Vec<Document>)> {
    let json = crate::ora::read_entry(zip, PACKAGE_ENTRY, MAX_ENTRY)?;
    let package: Package = serde_json::from_slice(&json).map_err(|e| error(e.to_string()))?;
    let count = package.animation.panels.len();
    if count == 0 || count > MAX_SCENE_PANELS {
        return Err(error("That library item has no panels."));
    }
    let mut docs = Vec::new();
    for n in 0..count {
        let bytes = crate::ora::read_entry(zip, &drawing_entry(n), MAX_ENTRY)?;
        docs.push(crate::ora::read_from(Cursor::new(bytes))?.doc);
    }
    Ok((package.kind, package.animation, docs))
}

/// A personal library item, ready to place with `ProjectEditor::place_item`.
pub fn load_item(asset: &Asset) -> Result<LibraryItem> {
    let kind = item_kind(asset.kind)
        .ok_or_else(|| error("That asset is not a storyboard library item."))?;
    let file = std::fs::File::open(&asset.path)?;
    let mut zip = ZipArchive::new(std::io::BufReader::new(file))?;
    let mut item = if zip.by_name(PACKAGE_ENTRY).is_ok() {
        let (saved, animation, mut docs) = read_package(&mut zip)?;
        if saved != kind {
            return Err(error("That library item does not match its kind."));
        }
        let mut item = LibraryItem::drawing(kind, docs.remove(0));
        item.animation = Some(animation);
        item.more = docs.into_iter().map(Arc::new).collect();
        item
    } else {
        LibraryItem::drawing(kind, crate::ora::read(&asset.path)?)
    };
    item.id = asset.id;
    item.name = asset.name.clone();
    item.tags = asset.tags.clone();
    item.validate_content().map_err(error)?;
    Ok(item)
}

/// The drawing of a personal library item (a scene item's first panel).
pub fn load(asset: &Asset) -> Result<Document> {
    load_item(asset).map(|item| Arc::unwrap_or_clone(item.doc))
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

    #[test]
    fn animated_and_scene_items_round_trip_as_packages() {
        use emulsion_core::command::Slot;
        use emulsion_core::motion::Easing;
        use emulsion_core::storyboard::{LayerMotion, LayerProperty, MotionKey, PropertyTrack};
        use emulsion_core::storyboard_library::Placed;
        use emulsion_core::{Command, Node, NodeKind};
        let root = std::env::temp_dir().join(format!(
            "emulsion-storyboard-library-animated-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let mut board = emulsion_core::creation::CanvasSpec {
            name: "Board".into(),
            kind: emulsion_core::creation::CanvasKind::Storyboard,
            width: 64.,
            height: 36.,
            pages: 2,
            ..Default::default()
        }
        .create_project()
        .unwrap();
        board.set_active_page(1).unwrap();
        let hero = board
            .execute(Command::AddNode {
                node: Box::new(Node::new(0, "Hero", NodeKind::Fill { rgba: [9; 4] })),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        board
            .edit_storyboard(|b| {
                let key = |frame, value| MotionKey {
                    frame,
                    value,
                    easing: Easing::Linear,
                    curve: None,
                };
                b.panels.get_mut(&1).unwrap().motion.insert(
                    hero,
                    LayerMotion {
                        pivot: None,
                        tracks: vec![PropertyTrack {
                            property: LayerProperty::X,
                            keys: vec![key(0, -10.), key(8, 0.)],
                        }],
                    },
                );
                Ok(())
            })
            .unwrap();
        let panel = board.capture_panel_item(1).unwrap();
        let scene = board
            .capture_scene_item(board.storyboard().unwrap().panels[&1].scene)
            .unwrap();
        let (_, panel_id) = add_item(&root, "Slide", &[], &panel).unwrap();
        let (catalog, scene_id) = add_item(&root, "Opening", &["intro".into()], &scene).unwrap();
        let asset = |id| catalog.assets.iter().find(|a| a.id == id).unwrap();
        assert_eq!(asset(scene_id).kind, AssetKind::StoryboardScene);
        assert_eq!(
            asset(scene_id).path.extension().unwrap(),
            "emsb",
            "animated items are packages"
        );
        let back = load_item(asset(panel_id)).unwrap();
        assert_eq!(back.animation, panel.animation);
        let back = load_item(asset(scene_id)).unwrap();
        assert_eq!((back.kind, back.more.len()), (ItemKind::Scene, 1));
        assert_eq!(back.animation, scene.animation);
        assert_eq!(back.tags, ["intro"]);
        assert_eq!(load(asset(scene_id)).unwrap().nodes, scene.doc.nodes);
        // Another storyboard places the scene, one Undo step.
        let mut other = emulsion_core::project::ProjectEditor::new_project(
            emulsion_core::project::ProjectKind::Storyboard,
            Document::new(64, 36),
        )
        .unwrap();
        let Placed::Scene { panels, .. } = other.place_item(&back).unwrap() else {
            panic!()
        };
        assert_eq!(other.page_list().len(), 3);
        assert!(
            other.storyboard().unwrap().panels[&panels[0]]
                .motion
                .contains_key(&hero)
        );
        assert!(other.undo());
        assert_eq!(other.page_list().len(), 1);
        // A mismatched item is refused and leaves nothing behind.
        let mut broken = scene.clone();
        broken.more.clear();
        assert!(add_item(&root, "Broken", &[], &broken).is_err());
        assert_eq!(items(&library::load(&root).unwrap()).count(), 2);
        remove(&root, scene_id).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
