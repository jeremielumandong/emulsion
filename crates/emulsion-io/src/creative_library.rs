//! Local assets, templates, collections and brand kits. References stay local;
//! placed media and saved templates are embedded in their native projects.
use crate::{IoError, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_BYTES: u64 = 8 << 20;
fn error(message: impl Into<String>) -> IoError {
    IoError::Manifest(message.into())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Image,
    Template,
    Stencil,
    Logo,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    pub kind: AssetKind,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub attribution: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub rating: u8,
    #[serde(default)]
    pub flagged: bool,
    #[serde(default)]
    pub variants: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Brand {
    pub id: u64,
    pub name: String,
    pub font: String,
    pub colors: Vec<[u8; 4]>,
    #[serde(default)]
    pub logos: Vec<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    pub id: u64,
    pub name: String,
    pub assets: Vec<u64>,
}
/// Home organizes references to local files. Trashing a reference never deletes
/// its source; restoring it preserves its name and folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    pub kind: Option<emulsion_core::creation::CanvasKind>,
    pub folder: Option<u64>,
    pub trashed: bool,
    pub opened: u64,
    pub summary: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectFolder {
    pub id: u64,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub version: u32,
    pub revision: u64,
    pub next_id: u64,
    pub assets: Vec<Asset>,
    pub brands: Vec<Brand>,
    pub collections: Vec<Collection>,
    pub projects: Vec<ProjectRecord>,
    pub folders: Vec<ProjectFolder>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            next_id: 1,
            assets: Vec::new(),
            brands: Vec::new(),
            collections: Vec::new(),
            projects: Vec::new(),
            folders: Vec::new(),
        }
    }
}
fn label(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 200 && !value.chars().any(char::is_control)
}
impl Catalog {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.assets.len() > 10_000
            || self.brands.len() > 100
            || self.collections.len() > 500
            || self.projects.len() > 20_000
            || self.folders.len() > 500
        {
            return Err(error("Unsupported or oversized creative library."));
        }
        let mut ids = HashSet::new();
        for id in self
            .assets
            .iter()
            .map(|a| a.id)
            .chain(self.brands.iter().map(|b| b.id))
            .chain(self.collections.iter().map(|c| c.id))
            .chain(self.projects.iter().map(|p| p.id))
            .chain(self.folders.iter().map(|f| f.id))
        {
            if id == 0 || id >= self.next_id || !ids.insert(id) {
                return Err(error("Invalid creative library IDs."));
            }
        }
        if self.next_id == u64::MAX {
            return Err(error("Library ID limit reached."));
        }
        let mut paths = HashSet::new();
        for folder in &self.folders {
            if !label(&folder.name) {
                return Err(error("Invalid project folder name."));
            }
        }
        for project in &self.projects {
            if !label(&project.name)
                || !project.path.is_absolute()
                || !paths.insert(&project.path)
                || project.summary.len() > 4000
                || project
                    .folder
                    .is_some_and(|id| !self.folders.iter().any(|f| f.id == id))
            {
                return Err(error("Invalid project reference or folder."));
            }
        }
        for asset in &self.assets {
            if !label(&asset.name)
                || asset.path.as_os_str().is_empty()
                || asset.rating > 5
                || asset.tags.len() > 50
                || asset.tags.iter().any(|s| !label(s))
                || asset.attribution.len() > 4000
                || asset.license.len() > 4000
                || asset.variants.len() > 100
                || asset.variants.iter().any(|v| !label(v))
            {
                return Err(error("Invalid asset metadata."));
            }
        }
        for brand in &self.brands {
            if !label(&brand.name)
                || !label(&brand.font)
                || brand.colors.is_empty()
                || brand.colors.len() > 32
                || brand.logos.len() > 50
                || brand.logos.iter().any(|id| {
                    !self
                        .assets
                        .iter()
                        .any(|a| a.id == *id && a.kind == AssetKind::Logo)
                })
            {
                return Err(error("Invalid brand kit or logo references."));
            }
        }
        for collection in &self.collections {
            if !label(&collection.name)
                || collection.assets.len() > 10_000
                || collection
                    .assets
                    .iter()
                    .any(|id| !self.assets.iter().any(|a| a.id == *id))
            {
                return Err(error("Invalid collection."));
            }
        }
        Ok(())
    }
    pub fn remember_project(
        &mut self,
        recent: &crate::recent::Recent,
        kind: Option<emulsion_core::creation::CanvasKind>,
    ) -> Result<u64> {
        let path = recent.path.canonicalize()?;
        if !path.is_file() || recent.summary.len() > 4000 {
            return Err(error("Choose a local project file."));
        }
        if let Some(project) = self.projects.iter_mut().find(|p| p.path == path) {
            if recent.opened >= project.opened {
                project.opened = recent.opened;
                project.summary = recent.summary.clone();
            }
            if kind.is_some() {
                project.kind = kind;
            }
            return Ok(project.id);
        }
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if !label(&name) || self.projects.len() >= 20_000 || self.next_id >= u64::MAX - 1 {
            return Err(error("Project name or library limit exceeded."));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.projects.push(ProjectRecord {
            id,
            path,
            name,
            kind,
            folder: None,
            trashed: false,
            opened: recent.opened,
            summary: recent.summary.clone(),
        });
        Ok(id)
    }
    pub fn add_project_folder(&mut self, name: String) -> Result<u64> {
        if !label(&name) || self.folders.len() >= 500 || self.next_id >= u64::MAX - 1 {
            return Err(error(
                "Choose a folder name; at most 500 folders are supported.",
            ));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.folders.push(ProjectFolder { id, name });
        Ok(id)
    }
    pub fn remove_project_folder(&mut self, id: u64) {
        self.folders.retain(|f| f.id != id);
        for p in &mut self.projects {
            if p.folder == Some(id) {
                p.folder = None;
            }
        }
    }
    pub fn add_asset(&mut self, path: PathBuf, kind: AssetKind) -> Result<u64> {
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(error("Choose a local file."));
        }
        if let Some(asset) = self
            .assets
            .iter()
            .find(|a| a.path == path && a.kind == kind)
        {
            return Ok(asset.id);
        }
        let name: String = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take(200)
            .collect();
        if !label(&name) || self.assets.len() >= 10_000 {
            return Err(error("Invalid asset name or library asset limit reached."));
        }
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .filter(|id| *id < u64::MAX)
            .ok_or_else(|| error("Library ID limit reached"))?;
        self.assets.push(Asset {
            id,
            path,
            name,
            kind,
            tags: Vec::new(),
            attribution: String::new(),
            license: String::new(),
            rating: 0,
            flagged: false,
            variants: Vec::new(),
        });
        self.validate()?;
        Ok(id)
    }
    pub fn add_brand(&mut self, name: String, font: String, colors: Vec<[u8; 4]>) -> Result<u64> {
        if !label(&name)
            || !label(&font)
            || colors.is_empty()
            || colors.len() > 32
            || self.brands.len() >= 100
        {
            return Err(error(
                "Invalid brand name, font, palette, or brand limit reached.",
            ));
        }
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .filter(|id| *id < u64::MAX)
            .ok_or_else(|| error("Library ID limit reached"))?;
        self.brands.push(Brand {
            id,
            name,
            font,
            colors,
            logos: Vec::new(),
        });
        self.validate()?;
        Ok(id)
    }
    pub fn remove_asset(&mut self, id: u64) {
        self.assets.retain(|a| a.id != id);
        for brand in &mut self.brands {
            brand.logos.retain(|asset| *asset != id);
        }
        for collection in &mut self.collections {
            collection.assets.retain(|asset| *asset != id);
        }
    }
    pub fn add_collection(&mut self, name: String, mut assets: Vec<u64>) -> Result<u64> {
        assets.sort_unstable();
        assets.dedup();
        if !label(&name)
            || self.collections.len() >= 500
            || assets.len() > 10_000
            || assets
                .iter()
                .any(|id| !self.assets.iter().any(|a| a.id == *id))
        {
            return Err(error(
                "Choose a collection name and existing library assets.",
            ));
        }
        let id = self.next_id;
        let next = id
            .checked_add(1)
            .filter(|id| *id < u64::MAX)
            .ok_or_else(|| error("Library ID limit reached"))?;
        self.collections.push(Collection { id, name, assets });
        self.next_id = next;
        Ok(id)
    }
}
pub fn root() -> PathBuf {
    crate::recent::data_dir().join("creative-library")
}
pub fn load(root: &Path) -> Result<Catalog> {
    let path = root.join("catalog.json");
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Catalog::default()),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(error("Creative library exceeds 8 MiB."));
    }
    let catalog: Catalog = serde_json::from_slice(&bytes)
        .map_err(|e| error(format!("Invalid creative library: {e}")))?;
    catalog.validate()?;
    Ok(catalog)
}
/// Reload under the OS lock so windows cannot overwrite each other's changes.
pub fn update<T>(
    root: &Path,
    edit: impl FnOnce(&mut Catalog) -> Result<T>,
) -> Result<(Catalog, T)> {
    fs::create_dir_all(root)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("catalog.lock"))?;
    lock.try_lock()
        .map_err(|e| error(format!("Creative library is busy: {e}")))?;
    let mut catalog = load(root)?;
    let result = edit(&mut catalog)?;
    catalog.validate()?;
    catalog.revision = catalog
        .revision
        .checked_add(1)
        .ok_or_else(|| error("Library revision limit reached"))?;
    let bytes = serde_json::to_vec_pretty(&catalog).map_err(|e| error(e.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(error("Creative library exceeds 8 MiB."));
    }
    crate::write_atomic(&root.join("catalog.json"), |file| {
        file.write_all(&bytes)?;
        Ok(())
    })?;
    Ok((catalog, result))
}
#[derive(Serialize, Deserialize)]
struct BrandFile {
    version: u32,
    name: String,
    font: String,
    colors: Vec<[u8; 4]>,
}
pub fn export_brand(brand: &Brand, path: &Path) -> Result<()> {
    let file = BrandFile {
        version: 1,
        name: brand.name.clone(),
        font: brand.font.clone(),
        colors: brand.colors.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&file).map_err(|e| error(e.to_string()))?;
    crate::write_atomic(path, |file| {
        file.write_all(&bytes)?;
        Ok(())
    })
}
pub fn import_brand(root: &Path, path: &Path) -> Result<Catalog> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(error("Brand kit is too large."));
    }
    let brand: BrandFile = serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
    if brand.version != 1 {
        return Err(error("Unsupported brand kit version."));
    }
    update(root, |c| c.add_brand(brand.name, brand.font, brand.colors)).map(|(c, _)| c)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_updates_preserve_other_windows_and_bad_edits_are_atomic() {
        let root =
            std::env::temp_dir().join(format!("emulsion-creative-library-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let image = root.join("Asset.png");
        fs::write(&image, b"fixture").unwrap();
        let (first, id) = update(&root, |c| c.add_asset(image.clone(), AssetKind::Image)).unwrap();
        assert_eq!(first.assets.len(), 1);
        let (second, brand) = update(&root, |c| {
            c.add_brand(
                "Studio".into(),
                "Geist".into(),
                vec![[20, 30, 40, 255], [230, 70, 40, 255]],
            )
        })
        .unwrap();
        assert!(second.assets.iter().any(|a| a.id == id));
        let before = fs::read(root.join("catalog.json")).unwrap();
        assert!(
            update(&root, |c| {
                c.brands[0].logos.push(999);
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(root.join("catalog.json")).unwrap(), before);
        let file = root.join("studio.brand.json");
        export_brand(second.brands.iter().find(|b| b.id == brand).unwrap(), &file).unwrap();
        assert_eq!(import_brand(&root, &file).unwrap().brands.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod project_tests {
    use super::*;
    #[test]
    fn folder_trash_restore_and_reload_preserve_local_files_and_atomicity() {
        let root =
            std::env::temp_dir().join(format!("emulsion-home-library-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("campaign.emu");
        fs::write(&path, b"untouched original").unwrap();
        let recent = crate::recent::Recent {
            path: path.clone(),
            opened: 123,
            summary: "3 pages".into(),
        };
        let (_, id) = update(&root, |c| {
            c.remember_project(&recent, Some(emulsion_core::creation::CanvasKind::Design))
        })
        .unwrap();
        let (catalog, folder) = update(&root, |c| {
            let folder = c.add_project_folder("Campaigns".into())?;
            let p = c.projects.iter_mut().find(|p| p.id == id).unwrap();
            p.name = "Autumn launch".into();
            p.folder = Some(folder);
            p.trashed = true;
            Ok(folder)
        })
        .unwrap();
        assert_eq!(catalog.projects.len(), 1);
        let before = fs::read(root.join("catalog.json")).unwrap();
        assert!(
            update(&root, |c| {
                c.projects[0].folder = Some(9999);
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(root.join("catalog.json")).unwrap(), before);
        let (restored, _) = update(&root, |c| {
            c.remember_project(&recent, None)?;
            c.projects[0].trashed = false;
            c.remove_project_folder(folder);
            Ok(())
        })
        .unwrap();
        assert_eq!(restored.projects[0].name, "Autumn launch");
        assert!(restored.projects[0].folder.is_none());
        assert_eq!(load(&root).unwrap(), restored);
        assert_eq!(fs::read(path).unwrap(), b"untouched original");
        fs::remove_dir_all(root).unwrap();
    }
}
