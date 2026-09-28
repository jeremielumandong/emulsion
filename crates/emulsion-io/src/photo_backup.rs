//! Portable, checksummed photo catalogs. Restore extracts into a new directory;
//! it never overwrites an original or trusts an archive member's filesystem path.
use crate::{
    IoError, Result,
    creative_library::{AssetKind, Catalog},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_TOTAL: u64 = 64 * 1024 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    kind: String,
    source: PathBuf,
    sha256: String,
    size: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    catalog: Catalog,
    entries: Vec<Entry>,
}
fn bad(s: impl Into<String>) -> IoError {
    IoError::Manifest(s.into())
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn save(catalog: &Catalog, path: &Path) -> Result<()> {
    catalog.validate()?;
    let mut sources = BTreeMap::<PathBuf, String>::new();
    for asset in &catalog.assets {
        if asset.kind == AssetKind::Image {
            sources.insert(asset.path.clone(), "photo".into());
            if crate::photo_develop::is_virtual(&asset.path) {
                sources.insert(
                    crate::photo_develop::reference(&asset.path)?.source,
                    "photo".into(),
                );
            }
        }
    }
    for source in sources.clone().keys() {
        let sidecar = crate::raw_settings::sidecar_path(source)?;
        if sidecar.try_exists()? {
            sources.insert(sidecar, "sidecar".into());
        }
    }
    for (kind, dir) in [
        ("mask", crate::recent::data_dir().join("develop-masks")),
        ("edits", crate::recent::data_dir().join("develop-edits")),
        ("profile", crate::camera_profiles::directory()),
        ("preset", crate::lightroom_presets::library_dir()),
    ] {
        if dir.exists() {
            for file in std::fs::read_dir(dir)? {
                let p = file?.path();
                if p.is_file() {
                    sources.insert(p, kind.into());
                }
            }
        }
    }
    if sources.len() > 250000 {
        return Err(bad("Too many backup resources"));
    }
    let temp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    let mut archive = zip::ZipWriter::new(temp.reopen()?);
    let mut total = 0;
    let mut entries = vec![];
    for (source, kind) in sources {
        let size = std::fs::metadata(&source)?.len();
        total += size;
        if size > 2 * 1024 * 1024 * 1024 || total > MAX_TOTAL {
            return Err(bad(
                "Backup exceeds the 2 GiB per file / 64 GiB total limit",
            ));
        }
        archive.start_file(
            format!("resources/{}", entries.len()),
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .large_file(true),
        )?;
        let mut file = std::fs::File::open(&source)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 65536];
        let mut count = 0;
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > size {
                return Err(bad("Source changed during backup; retry"));
            }
            hasher.update(&buffer[..n]);
            archive.write_all(&buffer[..n])?;
        }
        if count != size {
            return Err(bad("Source changed during backup; retry"));
        }
        entries.push(Entry {
            kind,
            source,
            size,
            sha256: hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        });
    }
    archive.start_file("manifest.json", zip::write::SimpleFileOptions::default())?;
    let manifest = Manifest {
        format: "emulsion-photo-backup".into(),
        version: 1,
        catalog: catalog.clone(),
        entries,
    };
    let bytes = serde_json::to_vec(&manifest).map_err(|e| bad(e.to_string()))?;
    if bytes.len() > 96 << 20 {
        return Err(bad("Backup manifest exceeds 96 MiB"));
    }
    archive.write_all(&bytes)?;
    archive.finish()?.sync_all()?;
    temp.persist_noclobber(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn restore(path: &Path, destination: &Path) -> Result<Catalog> {
    std::fs::create_dir_all(destination)?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    if zip.len() > 250001 {
        return Err(bad("Backup has too many members"));
    }
    let mut bytes = Vec::new();
    zip.by_name("manifest.json")?
        .take((96 << 20) + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 96 << 20 {
        return Err(bad("Backup manifest too large"));
    }
    let mut manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| bad(e.to_string()))?;
    if manifest.format != "emulsion-photo-backup"
        || manifest.version != 1
        || manifest.entries.len() + 1 != zip.len()
    {
        return Err(bad("Invalid photo backup"));
    }
    manifest.catalog.validate()?;
    let temp = tempfile::Builder::new()
        .prefix("restored-photos-")
        .tempdir_in(destination)?;
    let mut mapped = BTreeMap::new();
    let mut resources = vec![];
    let mut total = 0u64;
    for (i, entry) in manifest.entries.iter().enumerate() {
        if !["photo", "sidecar", "mask", "preset", "edits", "profile"].contains(&entry.kind.as_str())
            || entry.size > 2 * 1024 * 1024 * 1024
        {
            return Err(bad("Invalid backup resource"));
        }
        total = total
            .checked_add(entry.size)
            .ok_or_else(|| bad("Backup size overflow"))?;
        if total > MAX_TOTAL {
            return Err(bad("Backup exceeds 64 GiB"));
        }
        let extension = entry
            .source
            .extension()
            .unwrap_or_default()
            .to_string_lossy();
        if extension.len() > 20 || !extension.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(bad("Invalid resource extension"));
        }
        let out = temp.path().join(format!("{i}.{extension}"));
        let mut output = std::fs::File::create(&out)?;
        let mut input = zip.by_name(&format!("resources/{i}"))?;
        if input.size() != entry.size {
            return Err(bad("Resource size mismatch"));
        }
        let mut hasher = Sha256::new();
        let mut buffer = [0; 65536];
        let mut count = 0;
        loop {
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > entry.size {
                return Err(bad("Resource expands beyond recorded size"));
            }
            hasher.update(&buffer[..n]);
            output.write_all(&buffer[..n])?;
        }
        let digest: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if digest != entry.sha256 || count != entry.size {
            return Err(bad("Backup checksum mismatch"));
        }
        output.sync_all()?;
        if entry.kind == "photo" {
            if mapped.insert(entry.source.clone(), out.clone()).is_some() {
                return Err(bad("Duplicate photo in backup"));
            }
        }
        resources.push(out);
    }
    // Sidecars are located relative to the relocated originals, including virtual copies.
    for (i, e) in manifest
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == "sidecar")
    {
        let original = mapped
            .iter()
            .find(|(old, _)| crate::raw_settings::sidecar_path(old).is_ok_and(|s| s == e.source))
            .ok_or_else(|| bad("Orphan sidecar in backup"))?;
        std::fs::rename(
            &resources[i],
            crate::raw_settings::sidecar_path(original.1)?,
        )?;
    }
    for (old, path) in &mapped {
        if crate::photo_develop::is_virtual(path) {
            let before = std::fs::read(path)?;
            let mut reference = crate::photo_develop::reference(path)?;
            reference.source = mapped
                .get(&reference.source)
                .ok_or_else(|| bad("Missing virtual-copy original"))?
                .clone();
            let after = serde_json::to_vec_pretty(&reference).map_err(|e| bad(e.to_string()))?;
            let sidecar = crate::raw_settings::sidecar_path(path)?;
            if sidecar.exists() {
                let rebound =
                    crate::raw_settings::rebind_bytes(&sidecar, &hash(&before), &hash(&after))?;
                std::fs::write(sidecar, rebound)?;
            }
            std::fs::write(path, after)?;
        }
        let _ = old;
    }
    // Check resource conflicts before publishing any global asset.
    let mut globals = vec![];
    for (i, e) in manifest
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == "mask" || e.kind == "preset" || e.kind == "edits" || e.kind=="profile")
    {
        let name = e
            .source
            .file_name()
            .ok_or_else(|| bad("Missing resource name"))?;
        let target = if e.kind == "mask" {
            crate::recent::data_dir().join("develop-masks").join(name)
        } else if e.kind=="profile" {
            crate::camera_profiles::directory().join(name)
        } else if e.kind=="edits" {
            crate::recent::data_dir().join("develop-edits").join(name)
        } else {
            crate::lightroom_presets::library_dir().join(name)
        };
        if e.kind == "mask" && name.to_string_lossy() != format!("{}.png", e.sha256) {
            return Err(bad("Invalid mask identity"));
        }
        if e.kind=="profile" {
            if name.to_string_lossy()!=format!("{}.dcp",e.sha256){return Err(bad("Invalid profile identity"));}
            crate::camera_profiles::read(&resources[i])?;
        }
        if e.kind=="edits" {
            if name.to_string_lossy()!=format!("{}.json",e.sha256){return Err(bad("Invalid local edit identity"));}
            let edits:emulsion_core::develop_edits::LocalEdits=serde_json::from_slice(&std::fs::read(&resources[i])?).map_err(|e|bad(e.to_string()))?;
            edits.validate().map_err(bad)?;
        }
        if e.kind == "preset" {
            crate::lightroom_presets::load(&resources[i], Default::default())?;
        }
        if target.exists() {
            if crate::raw::source_digest(&target)? != e.sha256 {
                return Err(bad(
                    "Existing mask/preset differs from backup; restore stopped",
                ));
            }
        } else {
            globals.push((resources[i].clone(), target));
        }
    }
    for asset in &mut manifest.catalog.assets {
        if let Some(path) = mapped.get(&asset.path) {
            asset.path = path.clone();
        }
    }
    manifest.catalog.photos.fingerprints = manifest
        .catalog
        .assets
        .iter()
        .filter(|a| a.kind == AssetKind::Image)
        .map(|a| Ok((a.path.clone(), crate::raw::source_digest(&a.path)?)))
        .collect::<Result<_>>()?;
    manifest.catalog.validate()?;
    for (source, target) in globals {
        std::fs::create_dir_all(target.parent().unwrap())?;
        let mut input = std::fs::File::open(source)?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        std::io::copy(&mut input, &mut output)?;
        output.sync_all()?;
    }
    let _ = temp.keep();
    Ok(manifest.catalog)
}
