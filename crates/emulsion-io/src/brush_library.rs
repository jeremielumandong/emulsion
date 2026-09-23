//! Shared, versioned brush storage for desktop and assistant. Writers serialize
//! through an OS lock and compare revisions before atomically replacing JSON.
//! Assets are immutable SHA-256-addressed PNGs; 32-bit IDs only address the live
//! renderer and are resolved again when the library is loaded.
pub use emulsion_brushes::*;
#[cfg(test)]
use emulsion_raster::paint::Brush;
use emulsion_raster::{library::BrushPreset, paint::textures};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const MAX_MANIFEST: u64 = 32 * 1024 * 1024;
const MAX_ASSET: u64 = 32 * 1024 * 1024;
const MAX_PACKAGE: u64 = 512 * 1024 * 1024;
const MAX_ASSET_PIXELS: u64 = 4096 * 4096;

type RuntimeSources = Mutex<BTreeMap<(PathBuf, u32), PathBuf>>;
fn runtime_sources() -> &'static RuntimeSources {
    static SOURCES: OnceLock<RuntimeSources> = OnceLock::new();
    SOURCES.get_or_init(Default::default)
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid brush library: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Model(#[from] emulsion_brushes::Error),
    #[error("Brush library changed in another window; reload and retry")]
    Conflict,
    #[error("Brush library is being saved by another process; retry shortly")]
    Busy,
    #[error("{0}")]
    Invalid(String),
}
pub type StoreResult<T> = std::result::Result<T, StoreError>;

pub struct LoadReport {
    pub catalog: Catalog,
    pub warnings: Vec<String>,
}
#[derive(Default)]
pub struct ImportReport {
    pub added: Vec<BrushId>,
    pub warnings: Vec<String>,
    pub created_libraries: Vec<LibraryId>,
    pub created_sets: Vec<SetId>,
}

pub fn root() -> PathBuf {
    crate::recent::data_dir()
}
fn manifest(root: &Path) -> PathBuf {
    root.join("brush-library.json")
}
fn asset_path(root: &Path, hash: &str) -> PathBuf {
    root.join("brushes")
        .join("assets")
        .join(format!("{hash}.png"))
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Invalid(error.to_string())
}

fn read_bounded(path: &Path, max: u64) -> StoreResult<Vec<u8>> {
    let f = fs::File::open(path)?;
    if f.metadata()?.len() > max {
        return Err(invalid(format!(
            "{} exceeds the size limit",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    f.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(invalid("File exceeds the size limit"));
    }
    Ok(bytes)
}

/// Corruption is reported without renaming or overwriting the source. A future
/// save cannot silently replace it because commit reloads under the writer lock.
pub fn load() -> StoreResult<Catalog> {
    Ok(load_with_report()?.catalog)
}
pub fn load_with_report() -> StoreResult<LoadReport> {
    load_from_with_report(&root())
}
pub fn load_from(root: &Path) -> StoreResult<Catalog> {
    Ok(load_from_with_report(root)?.catalog)
}
pub fn load_from_with_report(root: &Path) -> StoreResult<LoadReport> {
    let mut catalog = if manifest(root).exists() {
        serde_json::from_slice::<Catalog>(&read_bounded(&manifest(root), MAX_MANIFEST)?)?
    } else {
        let legacy = root.join("brush-presets.json");
        if legacy.exists() {
            Catalog::migrate_legacy(serde_json::from_slice::<Vec<BrushPreset>>(&read_bounded(
                &legacy,
                MAX_MANIFEST,
            )?)?)
        } else {
            Catalog::builtin()
        }
    };
    catalog.validate()?;
    catalog.backfill_builtins();
    catalog.validate()?;
    let mut warnings = Vec::new();
    hydrate(root, &mut catalog, &mut warnings)?;
    Ok(LoadReport { catalog, warnings })
}

struct WriterLock {
    _file: fs::File,
}
impl WriterLock {
    fn acquire(root: &Path) -> StoreResult<Self> {
        fs::create_dir_all(root)?;
        let path = root.join("brush-library.lock");
        // Keep this inode in place: removing a lock file allows two processes
        // to lock different files at the same path. The OS releases the lock
        // when the handle closes, including process termination.
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(fs::TryLockError::WouldBlock) => Err(StoreError::Busy),
            Err(fs::TryLockError::Error(e)) => Err(e.into()),
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> StoreResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "tmp-{}",
        emulsion_brushes::new_id("write").replace(':', "-")
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

pub fn commit(expected_revision: u64, draft: &Catalog) -> StoreResult<Catalog> {
    commit_to(&root(), expected_revision, draft)
}
pub fn commit_to(root: &Path, expected_revision: u64, draft: &Catalog) -> StoreResult<Catalog> {
    draft.validate()?;
    let _lock = WriterLock::acquire(root)?;
    let current = load_from(root)?;
    if current.revision != expected_revision || draft.revision != expected_revision {
        return Err(StoreError::Conflict);
    }
    let mut next = draft.clone();
    // Promote legacy texture IDs to durable asset references on the first write.
    promote_assets(root, &mut next)?;
    next.revision = expected_revision
        .checked_add(1)
        .ok_or_else(|| invalid("Library revision exhausted"))?;
    next.validate()?;
    let bytes = serde_json::to_vec_pretty(&next)?;
    if bytes.len() as u64 > MAX_MANIFEST {
        return Err(invalid("Brush catalog exceeds 32 MiB"));
    }
    atomic_write(&manifest(root), &bytes)?;
    Ok(next)
}

/// Decode only bounded PNG images. Imported settings can never request a huge
/// raster allocation merely by including a tiny compressed image header.
fn check_png(bytes: &[u8]) -> StoreResult<()> {
    if bytes.len() as u64 > MAX_ASSET {
        return Err(invalid("Brush texture exceeds 32 MiB"));
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(invalid("Brush texture must be PNG"));
    }
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(invalid)?;
    let (w, h) = reader.into_dimensions().map_err(invalid)?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_ASSET_PIXELS {
        return Err(invalid("Brush texture exceeds 16 megapixels"));
    }
    Ok(())
}

/// Resolve hash to a runtime ID, disambiguating collisions instead of confusing
/// two different textures. Legacy FNV IDs are only reused when unambiguous.
fn register(bytes: &[u8], hash: &str) -> StoreResult<u32> {
    check_png(bytes)?;
    static IDS: OnceLock<Mutex<BTreeMap<u32, String>>> = OnceLock::new();
    let mut ids = IDS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some((&id, _)) = ids.iter().find(|(_, known)| known.as_str() == hash) {
        return Ok(id);
    }
    let legacy_id = textures::id_for(bytes);
    let mut id = legacy_id;
    while ids.get(&id).is_some_and(|known| known != hash) {
        id = id.wrapping_add(1).max(1);
    }
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(invalid)?
        .to_rgba8();
    let has_alpha = image.pixels().any(|p| p.0[3] < 255);
    let gray: Vec<_> = image
        .pixels()
        .map(|p| {
            if has_alpha {
                p.0[3]
            } else {
                ((u32::from(p.0[0]) * 54 + u32::from(p.0[1]) * 183 + u32::from(p.0[2]) * 19) / 256)
                    as u8
            }
        })
        .collect();
    let texture = textures::Texture::from_gray8(image.width(), image.height(), &gray)
        .ok_or_else(|| invalid("Empty brush texture"))?;
    textures::register(id, texture);
    ids.insert(id, hash.into());
    Ok(id)
}

pub fn store_texture_asset(bytes: &[u8]) -> StoreResult<(String, u32)> {
    store_texture_asset_to(&root(), bytes)
}
pub fn store_texture_asset_to(root: &Path, bytes: &[u8]) -> StoreResult<(String, u32)> {
    let hash = digest(bytes);
    let id = register(bytes, &hash)?;
    let path = asset_path(root, &hash);
    if !path.exists() {
        atomic_write(&path, bytes)?;
    } else if digest(&read_bounded(&path, MAX_ASSET)?) != hash {
        return Err(invalid("Stored texture hash mismatch"));
    }
    runtime_sources()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert((root.to_path_buf(), id), path);
    Ok((hash, id))
}

fn hydrate(root: &Path, catalog: &mut Catalog, warnings: &mut Vec<String>) -> StoreResult<()> {
    let mut cached: BTreeMap<String, Option<u32>> = BTreeMap::new();
    let mut resolve = |asset: &Option<String>, old: u32| -> u32 {
        let key = asset.clone().unwrap_or_else(|| format!("legacy:{old}"));
        if old == 0 && asset.is_none() {
            return 0;
        }
        if let Some(id) = cached.get(&key) {
            return id.unwrap_or(0);
        }
        let path = asset
            .as_ref()
            .map(|hash| asset_path(root, hash))
            .unwrap_or_else(|| {
                root.join("brushes")
                    .join("textures")
                    .join(format!("{old}.png"))
            });
        let result = read_bounded(&path, MAX_ASSET).and_then(|bytes| {
            let hash = digest(&bytes);
            if asset.as_ref().is_some_and(|expected| expected != &hash) {
                return Err(invalid("Texture hash mismatch"));
            }
            register(&bytes, &hash)
        });
        match result {
            Ok(id) => {
                runtime_sources()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert((root.to_path_buf(), id), path.clone());
                cached.insert(key, Some(id));
                id
            }
            Err(e) => {
                warnings.push(format!("{}: {e}", path.display()));
                cached.insert(key, None);
                0
            }
        }
    };
    for b in &mut catalog.brushes {
        // Preserve unresolved legacy IDs so the source can be restored later.
        // Missing durable assets use the engine fallback, while keeping hashes.
        for (brush, shape, grain) in [
            (&mut b.brush, &b.shape_asset, &b.grain_asset),
            (
                &mut b.baseline,
                &b.baseline_shape_asset,
                &b.baseline_grain_asset,
            ),
        ] {
            let tip = resolve(shape, brush.tip);
            let grain_id = resolve(grain, brush.grain_tex);
            if shape.is_some() || tip != 0 {
                brush.tip = tip;
            }
            if grain.is_some() || grain_id != 0 {
                brush.grain_tex = grain_id;
            }
        }
        if let Some(point) = &mut b.reset_point {
            let tip = resolve(&b.reset_point_shape_asset, point.tip);
            let grain = resolve(&b.reset_point_grain_asset, point.grain_tex);
            if b.reset_point_shape_asset.is_some() || tip != 0 {
                point.tip = tip;
            }
            if b.reset_point_grain_asset.is_some() || grain != 0 {
                point.grain_tex = grain;
            }
        }
        for (component, shape, grain) in [
            (
                &mut b.secondary,
                &b.secondary_shape_asset,
                &b.secondary_grain_asset,
            ),
            (
                &mut b.baseline_secondary,
                &b.baseline_secondary_shape_asset,
                &b.baseline_secondary_grain_asset,
            ),
            (
                &mut b.reset_point_secondary,
                &b.reset_point_secondary_shape_asset,
                &b.reset_point_secondary_grain_asset,
            ),
        ] {
            if let Some(brush) = component {
                let tip = resolve(shape, brush.tip);
                let grain_id = resolve(grain, brush.grain_tex);
                if shape.is_some() || tip != 0 {
                    brush.tip = tip;
                }
                if grain.is_some() || grain_id != 0 {
                    brush.grain_tex = grain_id;
                }
            }
        }
    }
    for memory in catalog.tool_memories.values_mut() {
        if let Some(b) = catalog.brushes.iter().find(|b| b.id == memory.brush_id) {
            memory.brush.tip = b.brush.tip;
            memory.brush.grain_tex = b.brush.grain_tex;
        }
    }
    Ok(())
}

fn promote_assets(root: &Path, catalog: &mut Catalog) -> StoreResult<()> {
    for b in &mut catalog.brushes {
        for (brush, shape, grain) in [
            (&mut b.brush, &mut b.shape_asset, &mut b.grain_asset),
            (
                &mut b.baseline,
                &mut b.baseline_shape_asset,
                &mut b.baseline_grain_asset,
            ),
        ] {
            promote_one(root, brush.tip, shape)?;
            promote_one(root, brush.grain_tex, grain)?;
        }
        if let Some(point) = &mut b.reset_point {
            promote_one(root, point.tip, &mut b.reset_point_shape_asset)?;
            promote_one(root, point.grain_tex, &mut b.reset_point_grain_asset)?;
        }
        for (component, shape, grain) in [
            (
                &mut b.secondary,
                &mut b.secondary_shape_asset,
                &mut b.secondary_grain_asset,
            ),
            (
                &mut b.baseline_secondary,
                &mut b.baseline_secondary_shape_asset,
                &mut b.baseline_secondary_grain_asset,
            ),
            (
                &mut b.reset_point_secondary,
                &mut b.reset_point_secondary_shape_asset,
                &mut b.reset_point_secondary_grain_asset,
            ),
        ] {
            if let Some(brush) = component {
                promote_one(root, brush.tip, shape)?;
                promote_one(root, brush.grain_tex, grain)?;
            }
        }
    }
    Ok(())
}
fn promote_one(root: &Path, id: u32, asset: &mut Option<String>) -> StoreResult<()> {
    if id == 0 || asset.is_some() {
        return Ok(());
    }
    let mapped = runtime_sources()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&(root.to_path_buf(), id))
        .cloned();
    let path = mapped.filter(|path| path.exists()).unwrap_or_else(|| {
        root.join("brushes")
            .join("textures")
            .join(format!("{id}.png"))
    });
    if path.exists() {
        *asset = Some(store_texture_asset_to(root, &read_bounded(&path, MAX_ASSET)?)?.0);
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Package {
    format: String,
    version: u32,
    catalog: Catalog,
    #[serde(default)]
    scope: ExportScope,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportScope {
    #[default]
    Brushes,
    Set(String),
    Library(String),
}

fn validate_portable_sources(catalog: &Catalog) -> StoreResult<()> {
    for b in &catalog.brushes {
        let sources = [
            (Some(b.brush), &b.shape_asset, &b.grain_asset),
            (
                Some(b.baseline),
                &b.baseline_shape_asset,
                &b.baseline_grain_asset,
            ),
            (
                b.reset_point,
                &b.reset_point_shape_asset,
                &b.reset_point_grain_asset,
            ),
            (
                b.secondary,
                &b.secondary_shape_asset,
                &b.secondary_grain_asset,
            ),
            (
                b.baseline_secondary,
                &b.baseline_secondary_shape_asset,
                &b.baseline_secondary_grain_asset,
            ),
            (
                b.reset_point_secondary,
                &b.reset_point_secondary_shape_asset,
                &b.reset_point_secondary_grain_asset,
            ),
        ];
        for (brush, shape, grain) in sources {
            if let Some(brush) = brush
                && ((brush.tip != 0 && shape.is_none())
                    || (brush.grain_tex != 0 && grain.is_none()))
            {
                return Err(invalid(format!("{} has a missing source texture", b.name)));
            }
        }
    }
    Ok(())
}

pub fn export_package(path: &Path, catalog: &Catalog, brush_ids: &[String]) -> StoreResult<()> {
    export_package_from(&root(), path, catalog, brush_ids)
}
pub fn export_package_from(
    root: &Path,
    path: &Path,
    catalog: &Catalog,
    brush_ids: &[String],
) -> StoreResult<()> {
    export_package_scoped_from(root, path, catalog, brush_ids, ExportScope::Brushes)
}
pub fn export_package_scoped(
    path: &Path,
    catalog: &Catalog,
    brush_ids: &[String],
    scope: ExportScope,
) -> StoreResult<()> {
    export_package_scoped_from(&root(), path, catalog, brush_ids, scope)
}
pub fn export_package_scoped_from(
    root: &Path,
    path: &Path,
    catalog: &Catalog,
    brush_ids: &[String],
    scope: ExportScope,
) -> StoreResult<()> {
    let mut selected = catalog.clone();
    match &scope {
        ExportScope::Brushes => {
            if brush_ids.is_empty() {
                return Err(invalid("Select at least one brush to export"));
            }
            selected.brushes.retain(|b| brush_ids.contains(&b.id));
            if selected.brushes.len() != brush_ids.iter().collect::<BTreeSet<_>>().len() {
                return Err(invalid("Export contains unknown brush IDs"));
            }
            selected
                .sets
                .retain(|s| selected.brushes.iter().any(|b| b.set_id == s.id));
        }
        ExportScope::Set(id) => {
            if !selected.sets.iter().any(|s| s.id == *id) {
                return Err(invalid("Export set was not found"));
            }
            selected.sets.retain(|s| s.id == *id);
            selected.brushes.retain(|b| b.set_id == *id);
        }
        ExportScope::Library(id) => {
            if !selected.libraries.iter().any(|l| l.id == *id) {
                return Err(invalid("Export library was not found"));
            }
            selected.sets.retain(|s| s.library_id == *id);
            selected
                .brushes
                .retain(|b| selected.sets.iter().any(|s| s.id == b.set_id));
        }
    }
    selected.libraries.retain(|l| {
        selected.sets.iter().any(|s| s.library_id == l.id)
            || matches!(&scope,ExportScope::Library(id) if l.id == *id)
    });
    selected.pinned.clear();
    selected.recent.clear();
    selected.tool_memories.clear();
    selected.revision = 0;
    promote_assets(root, &mut selected)?;
    selected.validate()?;
    validate_portable_sources(&selected)?;
    let mut assets = BTreeSet::new();
    for b in &selected.brushes {
        assets.extend(b.asset_refs().cloned());
    }
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer
        .start_file("manifest.json", options)
        .map_err(invalid)?;
    let manifest_bytes = serde_json::to_vec(&Package {
        format: "emulsion-brushes".into(),
        version: 1,
        catalog: selected,
        scope,
    })?;
    if manifest_bytes.len() as u64 > MAX_MANIFEST {
        return Err(invalid("Brush package manifest exceeds 32 MiB"));
    }
    writer.write_all(&manifest_bytes)?;
    let mut total = 0;
    for hash in assets {
        let bytes = read_bounded(&asset_path(root, &hash), MAX_ASSET)?;
        if digest(&bytes) != hash {
            return Err(invalid("Texture hash mismatch"));
        }
        total += bytes.len() as u64;
        if total > MAX_PACKAGE {
            return Err(invalid("Brush package exceeds 512 MiB"));
        }
        writer
            .start_file(format!("assets/{hash}.png"), options)
            .map_err(invalid)?;
        writer.write_all(&bytes)?;
    }
    let bytes = writer.finish().map_err(invalid)?.into_inner();
    atomic_write(path, &bytes)
}

fn import_package(
    root: &Path,
    path: &Path,
    draft: &mut Catalog,
    target_set: &str,
) -> StoreResult<Vec<BrushId>> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path)?).map_err(invalid)?;
    if zip.len() > 10000 {
        return Err(invalid("Too many brush package entries"));
    }
    let mut bytes = Vec::new();
    {
        let f = zip.by_name("manifest.json").map_err(invalid)?;
        if f.size() > MAX_MANIFEST {
            return Err(invalid("Package manifest exceeds size limit"));
        }
        f.take(MAX_MANIFEST + 1).read_to_end(&mut bytes)?;
    }
    let mut package: Package = serde_json::from_slice(&bytes)?;
    if package.format != "emulsion-brushes" || package.version != 1 {
        return Err(invalid("Unsupported brush package version"));
    }
    package.catalog.validate()?;
    match &package.scope {
        ExportScope::Set(id)
            if package.catalog.sets.len() != 1 || package.catalog.sets[0].id != *id =>
        {
            return Err(invalid("Package set scope does not match its catalog"));
        }
        ExportScope::Library(id)
            if package.catalog.libraries.len() != 1 || package.catalog.libraries[0].id != *id =>
        {
            return Err(invalid("Package library scope does not match its catalog"));
        }
        _ => {}
    }
    validate_portable_sources(&package.catalog)?;
    let mut total = 0u64;
    let mut assets = BTreeSet::new();
    for b in &package.catalog.brushes {
        assets.extend(b.asset_refs().cloned());
    }
    for hash in assets {
        let f = zip
            .by_name(&format!("assets/{hash}.png"))
            .map_err(invalid)?;
        total = total.saturating_add(f.size());
        if f.size() > MAX_ASSET || total > MAX_PACKAGE {
            return Err(invalid("Brush package exceeds size limits"));
        }
        let mut bytes = Vec::new();
        f.take(MAX_ASSET + 1).read_to_end(&mut bytes)?;
        if digest(&bytes) != hash {
            return Err(invalid("Package texture hash mismatch"));
        }
        store_texture_asset_to(root, &bytes)?;
    }
    hydrate(root, &mut package.catalog, &mut Vec::new())?;
    let mut destination_sets = BTreeMap::new();
    match &package.scope {
        ExportScope::Brushes => {}
        ExportScope::Set(_) => {
            let destination_library = draft
                .sets
                .iter()
                .find(|s| s.id == target_set)
                .ok_or_else(|| invalid("Import destination set was not found"))?
                .library_id
                .clone();
            for set in &package.catalog.sets {
                destination_sets.insert(
                    set.id.clone(),
                    draft.create_set(&destination_library, &set.name)?,
                );
            }
        }
        ExportScope::Library(_) => {
            let mut libraries = BTreeMap::new();
            for library in &package.catalog.libraries {
                libraries.insert(library.id.clone(), draft.create_library(&library.name)?);
            }
            for set in &package.catalog.sets {
                destination_sets.insert(
                    set.id.clone(),
                    draft.create_set(&libraries[&set.library_id], &set.name)?,
                );
            }
        }
    }
    let mut added = Vec::new();
    for mut b in package.catalog.brushes {
        b.id = new_id("brush");
        b.builtin = false;
        b.set_id = destination_sets
            .get(&b.set_id)
            .cloned()
            .unwrap_or_else(|| target_set.into());
        added.push(b.id.clone());
        draft.brushes.push(b);
    }
    Ok(added)
}

pub fn import_paths(
    draft: &mut Catalog,
    paths: &[PathBuf],
    target_set: &str,
) -> StoreResult<ImportReport> {
    import_paths_to(&root(), draft, paths, target_set)
}
pub fn import_paths_to(
    root: &Path,
    draft: &mut Catalog,
    paths: &[PathBuf],
    target_set: &str,
) -> StoreResult<ImportReport> {
    if !draft.sets.iter().any(|s| s.id == target_set) {
        return Err(invalid("Import destination set does not exist"));
    }
    let mut next = draft.clone();
    let mut report = ImportReport::default();
    for path in paths {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("embrushes"))
        {
            report
                .added
                .extend(import_package(root, path, &mut next, target_set)?);
            continue;
        }
        let extension = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        let imported = if extension == "abr" {
            crate::abr::import(path)
        } else {
            crate::brushset::import(path)
        }
        .map_err(invalid)?;
        let file_label = path.file_stem().unwrap_or_default().to_string_lossy();
        let mut import_sets = BTreeMap::new();
        let import_library = if extension == "brushlibrary" {
            report.warnings.push("Library set names were recovered from archive folders; proprietary ordering metadata is not interpreted".into());
            Some(next.create_library(&file_label)?)
        } else if extension == "brushset" {
            let library_id = next
                .sets
                .iter()
                .find(|s| s.id == target_set)
                .expect("validated destination")
                .library_id
                .clone();
            let set_id = next.create_set(&library_id, &file_label)?;
            import_sets.insert(file_label.to_string(), set_id);
            Some(library_id)
        } else {
            None
        };
        // Preserve the external source verbatim alongside the converted model.
        // Future importers can revisit unsupported properties without asking the
        // artist to locate the original archive again.
        let original = read_bounded(path, MAX_PACKAGE)?;
        let source_hash = digest(&original);
        let source_path = root
            .join("brushes")
            .join("sources")
            .join(format!("{source_hash}.archive"));
        if !source_path.exists() {
            atomic_write(&source_path, &original)?;
        }
        for b in imported {
            report.warnings.extend(
                b.warnings
                    .iter()
                    .map(|warning| format!("{} / {}: {warning}", path.display(), b.preset.name)),
            );
            let destination = if let Some(library_id) = &import_library {
                let label = if extension == "brushlibrary" {
                    b.source_set.as_deref().unwrap_or("Imported")
                } else {
                    &file_label
                };
                if let Some(id) = import_sets.get(label) {
                    id.clone()
                } else {
                    let id = next.create_set(library_id, label)?;
                    import_sets.insert(label.to_owned(), id.clone());
                    id
                }
            } else {
                target_set.to_owned()
            };
            let id = next.add_brush(&destination, &b.preset.name, b.preset.brush)?;
            let definition = next.brush_mut(&id).expect("just inserted");
            definition.note = b.preset.note;
            definition.author.source = format!(
                "{} (SHA-256 {source_hash})",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            if let Some(png) = b.shape_png {
                let (hash, runtime) = store_texture_asset_to(root, &png)?;
                definition.shape_asset = Some(hash.clone());
                definition.baseline_shape_asset = Some(hash);
                definition.brush.tip = runtime;
                definition.baseline.tip = runtime;
            }
            if let Some(png) = b.grain_png {
                let (hash, runtime) = store_texture_asset_to(root, &png)?;
                definition.grain_asset = Some(hash.clone());
                definition.baseline_grain_asset = Some(hash);
                definition.brush.grain_tex = runtime;
                definition.baseline.grain_tex = runtime;
            }
            report.added.push(id);
        }
    }
    next.validate()?;
    report.created_libraries = next
        .libraries
        .iter()
        .filter(|library| !draft.libraries.iter().any(|old| old.id == library.id))
        .map(|library| library.id.clone())
        .collect();
    report.created_sets = next
        .sets
        .iter()
        .filter(|set| !draft.sets.iter().any(|old| old.id == set.id))
        .map(|set| set.id.clone())
        .collect();
    *draft = next;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(new_id("emulsion-library-test").replace(':', "-"));
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn png() -> Vec<u8> {
        let image = image::GrayImage::from_pixel(4, 4, image::Luma([128]));
        let mut cursor = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .unwrap();
        cursor.into_inner()
    }
    #[test]
    fn existing_catalog_receives_new_builtins_without_overwriting_edits() {
        let root = temp();
        let mut old = Catalog::builtin();
        let missing = old.brushes.pop().unwrap();
        let existing = old.brushes.first_mut().unwrap();
        existing.name = "Artist renamed this".into();
        existing.brush.size = 99.;
        let existing_id = existing.id.clone();
        old.create_reset_point(&existing_id).unwrap();
        let original = old.brush(&existing_id).unwrap().clone();
        old.revision = 12;
        let bytes = serde_json::to_vec(&old).unwrap();
        fs::write(manifest(&root), &bytes).unwrap();
        let loaded = load_from(&root).unwrap();
        assert_eq!(loaded.brush(&existing_id).unwrap(), &original);
        assert!(loaded.brush(&missing.id).is_some());
        assert_eq!(loaded.revision, 12);
        assert_eq!(fs::read(manifest(&root)).unwrap(), bytes);
        assert_eq!(loaded.brushes.last().unwrap().id, missing.id);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn migration_commit_conflicts_and_corruption_are_safe() {
        let root = temp();
        let legacy = vec![
            BrushPreset {
                name: "Legacy".into(),
                category: "Imported".into(),
                note: String::new(),
                brush: Brush::default()
            };
            401
        ];
        fs::write(
            root.join("brush-presets.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        let first = load_from(&root).unwrap();
        assert_eq!(first.brushes.iter().filter(|b| !b.builtin).count(), 401);
        let saved = commit_to(&root, 0, &first).unwrap();
        assert_eq!(saved.revision, 1);
        assert!(root.join("brush-presets.json").exists());
        assert!(matches!(
            commit_to(&root, 0, &first),
            Err(StoreError::Conflict)
        ));
        assert_eq!(load_from(&root).unwrap(), saved);
        fs::write(manifest(&root), b"broken").unwrap();
        assert!(commit_to(&root, 1, &saved).is_err());
        assert_eq!(fs::read(manifest(&root)).unwrap(), b"broken");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_package_roundtrips_assets_baselines_and_metadata() {
        let source = temp();
        let destination = temp();
        let mut c = Catalog::builtin();
        let (hash, runtime) = store_texture_asset_to(&source, &png()).unwrap();
        let id = c
            .add_brush(
                USER_SET,
                "Textured",
                Brush {
                    tip: runtime,
                    ..Brush::default()
                },
            )
            .unwrap();
        let b = c.brush_mut(&id).unwrap();
        b.shape_asset = Some(hash.clone());
        b.baseline_shape_asset = Some(hash.clone());
        b.author.name = "Artist".into();
        b.secondary = Some(Brush {
            tip: runtime,
            size: 13.,
            ..Brush::default()
        });
        b.secondary_shape_asset = Some(hash.clone());
        c.create_reset_point(&id).unwrap();
        c.brush_mut(&id).unwrap().brush.size = 90.;
        let package = source.join("brush.embrushes");
        export_package_from(&source, &package, &c, std::slice::from_ref(&id)).unwrap();
        let mut target = Catalog::builtin();
        let report = import_paths_to(&destination, &mut target, &[package], USER_SET).unwrap();
        let imported = target.brush(&report.added[0]).unwrap();
        assert_ne!(imported.id, id);
        assert_eq!(imported.author.name, "Artist");
        assert_eq!(imported.shape_asset, Some(hash));
        assert_eq!(imported.brush.size, 90.);
        assert_eq!(imported.reset_point, c.brush(&id).unwrap().reset_point);
        assert_eq!(imported.baseline, c.brush(&id).unwrap().baseline);
        assert_eq!(imported.secondary, c.brush(&id).unwrap().secondary);
        assert_eq!(
            imported.reset_point_secondary_shape_asset,
            c.brush(&id).unwrap().reset_point_secondary_shape_asset
        );
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(destination).unwrap();
    }
    #[test]
    fn failed_import_does_not_mutate_draft() {
        let root = temp();
        let mut c = Catalog::builtin();
        let before = c.clone();
        assert!(import_paths_to(&root, &mut c, &[root.join("missing.brush")], USER_SET).is_err());
        assert_eq!(c, before);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn legacy_runtime_remap_still_promotes_original_texture() {
        let root = temp();
        let old_id = 987654321;
        let textures_dir = root.join("brushes").join("textures");
        fs::create_dir_all(&textures_dir).unwrap();
        fs::write(textures_dir.join(format!("{old_id}.png")), png()).unwrap();
        let legacy = vec![BrushPreset {
            name: "Legacy asset".into(),
            category: "Mine".into(),
            note: String::new(),
            brush: Brush {
                tip: old_id,
                ..Brush::default()
            },
        }];
        fs::write(
            root.join("brush-presets.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        let loaded = load_from(&root).unwrap();
        assert_ne!(loaded.brush("brush:legacy:0").unwrap().brush.tip, old_id);
        let saved = commit_to(&root, 0, &loaded).unwrap();
        assert!(saved.brush("brush:legacy:0").unwrap().shape_asset.is_some());
        export_package_from(
            &root,
            &root.join("recovered.embrushes"),
            &saved,
            &["brush:legacy:0".into()],
        )
        .unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn saving_current_runtime_brush_promotes_content_addressed_assets() {
        let root = temp();
        let (hash, runtime) = store_texture_asset_to(&root, &png()).unwrap();
        let mut catalog = Catalog::builtin();
        let id = catalog
            .add_brush(
                USER_SET,
                "Saved current",
                Brush {
                    tip: runtime,
                    grain_tex: runtime,
                    ..Brush::default()
                },
            )
            .unwrap();
        let saved = commit_to(&root, 0, &catalog).unwrap();
        let brush = saved.brush(&id).unwrap();
        assert_eq!(brush.shape_asset.as_deref(), Some(hash.as_str()));
        assert_eq!(brush.baseline_grain_asset.as_deref(), Some(hash.as_str()));
        export_package_from(
            &root,
            &root.join("current.embrushes"),
            &saved,
            std::slice::from_ref(&id),
        )
        .unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn scoped_packages_restore_ordered_hierarchy_and_empty_sets() {
        let root = temp();
        let mut original = Catalog::builtin();
        let library = original.create_library("Artist library").unwrap();
        let empty = original.create_set(&library, "Empty first").unwrap();
        let full = original.create_set(&library, "Ink second").unwrap();
        original.add_brush(&full, "Z", Brush::default()).unwrap();
        original.add_brush(&full, "A", Brush::default()).unwrap();
        let file = root.join("library.embrushes");
        export_package_scoped_from(&root, &file, &original, &[], ExportScope::Library(library))
            .unwrap();
        let mut imported = Catalog::builtin();
        let report = import_paths_to(&root, &mut imported, &[file], USER_SET).unwrap();
        assert_eq!(report.added.len(), 2);
        let library = imported
            .libraries
            .iter()
            .find(|l| l.name == "Artist library")
            .unwrap();
        let sets: Vec<_> = imported
            .sets
            .iter()
            .filter(|s| s.library_id == library.id)
            .collect();
        assert_eq!(
            sets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            vec!["Empty first", "Ink second"]
        );
        assert_ne!(sets[0].id, empty);
        assert_eq!(
            imported
                .brushes
                .iter()
                .filter(|b| b.set_id == sets[1].id)
                .map(|b| b.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Z", "A"]
        );
        let file = root.join("set.embrushes");
        export_package_scoped_from(&root, &file, &original, &[], ExportScope::Set(full)).unwrap();
        let report = import_paths_to(&root, &mut imported, &[file], USER_SET).unwrap();
        assert_eq!(report.added.len(), 2);
        let set = imported.brush(&report.added[0]).unwrap().set_id.clone();
        assert_eq!(
            imported
                .sets
                .iter()
                .find(|s| s.id == set)
                .unwrap()
                .library_id,
            USER_LIBRARY
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn nested_brushlibrary_preserves_set_names_and_original_archive() {
        fn zip(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (name, bytes) in entries {
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(&bytes).unwrap();
            }
            writer.finish().unwrap().into_inner()
        }
        let xml = b"<?xml version=\"1.0\"?><plist version=\"1.0\"><dict/></plist>".to_vec();
        let single = zip(vec![("Brush.archive", xml.clone())]);
        let set = zip(vec![("Wet brush/Brush.archive", xml)]);
        let library = zip(vec![
            ("Set One/Ink.brush", single),
            ("Set Two/Wet.brushset", set),
        ]);
        let root = temp();
        let file = root.join("My library.brushlibrary");
        fs::write(&file, &library).unwrap();
        let mut catalog = Catalog::builtin();
        let report = import_paths_to(&root, &mut catalog, &[file], USER_SET).unwrap();
        assert_eq!(report.added.len(), 2);
        let library = catalog
            .libraries
            .iter()
            .find(|l| l.name == "My library")
            .unwrap();
        let names: Vec<_> = catalog
            .sets
            .iter()
            .filter(|s| s.library_id == library.id)
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, vec!["Set One", "Wet"]);
        assert!(!report.warnings.is_empty());
        assert_eq!(
            fs::read_dir(root.join("brushes/sources")).unwrap().count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn writer_lock_survives_stale_file_and_releases_with_owner() {
        let root = temp();
        fs::write(root.join("brush-library.lock"), b"old process").unwrap();
        let lock = WriterLock::acquire(&root).unwrap();
        assert!(matches!(WriterLock::acquire(&root), Err(StoreError::Busy)));
        drop(lock);
        let next = WriterLock::acquire(&root).unwrap();
        assert!(root.join("brush-library.lock").exists());
        drop(next);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_legacy_reset_sources_can_be_restored_then_exported() {
        let root = temp();
        let bytes = png();
        let runtime = textures::id_for(&bytes);
        let mut catalog = Catalog::builtin();
        let id = catalog
            .add_brush(
                USER_SET,
                "Recoverable",
                Brush {
                    tip: runtime,
                    ..Brush::default()
                },
            )
            .unwrap();
        catalog.create_reset_point(&id).unwrap();
        let mut warnings = Vec::new();
        hydrate(&root, &mut catalog, &mut warnings).unwrap();
        assert!(!warnings.is_empty());
        assert_eq!(
            catalog.brush(&id).unwrap().reset_point.unwrap().tip,
            runtime
        );
        let source = root.join("brushes").join("textures");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join(format!("{runtime}.png")), bytes).unwrap();
        let saved = commit_to(&root, 0, &catalog).unwrap();
        assert!(saved.brush(&id).unwrap().reset_point_shape_asset.is_some());
        export_package_from(&root, &root.join("recovered.embrushes"), &saved, &[id]).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn portable_packages_reject_unresolved_reset_source_ids() {
        let root = temp();
        let mut catalog = Catalog::builtin();
        let id = catalog
            .add_brush(USER_SET, "Missing reset source", Brush::default())
            .unwrap();
        catalog.brush_mut(&id).unwrap().reset_point = Some(Brush {
            tip: 4321,
            ..Brush::default()
        });
        let path = root.join("invalid.embrushes");
        assert!(export_package_from(&root, &path, &catalog, &[id]).is_err());
        assert!(!path.exists());
        // Construct an invalid package independently of the validating exporter.
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            &serde_json::to_vec(&Package {
                format: "emulsion-brushes".into(),
                version: 1,
                catalog,
                scope: ExportScope::Brushes,
            })
            .unwrap(),
        )
        .unwrap();
        zip.finish().unwrap();
        let mut target = Catalog::builtin();
        let original = target.clone();
        assert!(import_paths_to(&root, &mut target, &[path], USER_SET).is_err());
        assert_eq!(target, original);
        fs::remove_dir_all(root).unwrap();
    }
}
