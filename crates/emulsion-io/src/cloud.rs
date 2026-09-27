//! Portable cloud payloads retain native history and fingerprinted RAW originals.
use anyhow::{Context, Result, ensure};
use emulsion_cloud::{Store, store::{MAX_FILE_BYTES, digest, private_dir}};
use emulsion_core::{Document, graph::Graph};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, HashSet}, fs::File, io::{Read, Write}, path::{Path, PathBuf}};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

#[derive(Serialize, Deserialize)]
struct Manifest { version: u32, name: String, kind: String, files: BTreeMap<String, String> }
enum Native { Project(emulsion_core::project::Project), Ora(Box<crate::ora::Opened>) }
impl Native {
    fn read(path: &Path) -> Result<Option<Self>> {
        if crate::project::is_project(path) { Ok(Some(Self::Project(crate::project::read(path)?))) }
        else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ora")) {
            let opened = crate::ora::read_full(path)?; ensure!(opened.history_error.is_none(), "Repair project history before syncing"); Ok(Some(Self::Ora(Box::new(opened))))
        } else { Ok(None) }
    }
    fn map(&mut self, f: &mut impl FnMut(&mut Document) -> Result<()>) -> Result<()> {
        fn graph(graph: &mut Graph, f: &mut impl FnMut(&mut Document) -> Result<()>) -> Result<()> {
            let mut commits = graph.commits().cloned().collect::<Vec<_>>();
            for commit in &mut commits { f(&mut commit.doc)?; }
            *graph = Graph::from_parts(commits, graph.branches().clone(), graph.head().into())?; Ok(())
        }
        match self {
            Self::Project(project) => for page in &mut project.pages { f(&mut page.doc)?; graph(&mut page.graph, f)?; },
            Self::Ora(opened) => { f(&mut opened.doc)?; if let Some(g) = &mut opened.graph { graph(g, f)?; } },
        }
        Ok(())
    }
    fn write(&self, path: &Path) -> Result<()> {
        match self { Self::Project(p) => crate::project::write(p, path)?, Self::Ora(o) => crate::ora::write_full(&o.doc, o.graph.as_ref(), path)? }
        Ok(())
    }
}
pub fn store() -> Store { Store::new(crate::recent::data_dir().join("cloud")) }

/// Called on the local save worker. It never authenticates or uses the network.
pub fn enqueue_saved(path: &Path) -> Result<bool> {
    let store = store();
    // No cloud directory or disk work is needed for users who never enabled sync.
    if !store.root.join("index.json").exists() { return Ok(false); }
    let source = path.canonicalize()?;
    if !store.read()?.bindings.iter().any(|b| b.path == source) { return Ok(false); }
    enqueue(&store, &source)
}
pub fn enqueue(store: &Store, source: &Path) -> Result<bool> {
    private_dir(&store.root)?;
    let temp = tempfile::tempdir_in(&store.root)?;
    let payload = temp.path().join("payload.zip");
    pack(source, &payload)?;
    store.enqueue(source, &payload)
}

pub fn pack(source: &Path, destination: &Path) -> Result<()> {
    let source = source.canonicalize()?;
    let temp = tempfile::tempdir_in(destination.parent().context("Missing bundle directory")?)?;
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut hashes = BTreeMap::new();
    let name = source.file_name().context("Missing filename")?.to_string_lossy().into_owned();
    valid_name(&name)?;
    let kind;
    if let Some(mut native) = Native::read(&source)? {
        kind = if crate::project::is_project(&source) { "emu" } else { "ora" };
        let mut mapped: BTreeMap<PathBuf, (String, String)> = BTreeMap::new();
        native.map(&mut |doc| {
            let mut register = |path: &Path, expected: Option<&str>| -> Result<PathBuf> {
                let path = path.canonicalize().with_context(|| format!("Required RAW original is missing: {}", path.display()))?;
                if !mapped.contains_key(&path) {
                    let (hash, _) = digest(&path)?;
                    let ext = safe_extension(&path);
                    let key = format!("sources/{hash}.{ext}");
                    files.insert(key.clone(), path.clone()); hashes.insert(key.clone(), hash.clone());
                    mapped.insert(path.clone(), (key, hash));
                }
                let (key, hash) = &mapped[&path];
                if let Some(expected) = expected { ensure!(expected.eq_ignore_ascii_case(hash), "RAW original does not match its saved fingerprint"); }
                Ok(PathBuf::from(key))
            };
            if let Some(raw) = &mut doc.raw { raw.source = register(&raw.source, Some(&raw.source_sha256))?; }
            for original in &mut doc.raw_originals { *original = register(original, None)?; }
            Ok(())
        })?;
        let file = temp.path().join(format!("project.{kind}")); native.write(&file)?;
        files.insert(format!("document.{kind}"), file);
    } else {
        // Use the normal decoder to reject unsupported or malformed imports.
        crate::open(&source)?;
        kind = "image";
        files.insert(format!("document.{}", safe_extension(&source)), source.clone());
        let sidecar = crate::raw_settings::sidecar_path(&source)?;
        if sidecar.exists() {
            crate::raw_settings::load_sidecar_verified(&sidecar, &crate::raw::source_digest(&source)?)?;
            files.insert("raw-sidecar.json".into(), sidecar);
        }
    }
    let mut total = 0;
    for (name, path) in &files { let (hash, size) = digest(path)?; total += size; ensure!(total <= MAX_FILE_BYTES - 1024 * 1024, "Portable project exceeds the 4 GiB cloud limit"); hashes.insert(name.clone(), hash); }
    let manifest = Manifest { version:1, name, kind:kind.into(), files:hashes };
    ensure!(manifest.files.len() <= 1000, "Too many project dependencies");
    emulsion_cloud::store::atomic(destination, |f| {
        let mut zip = ZipWriter::new(f);
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(true);
        zip.start_file("manifest.json", options)?; zip.write_all(&serde_json::to_vec(&manifest)?)?;
        for (name, path) in &files { zip.start_file(name, options)?; std::io::copy(&mut File::open(path)?, &mut zip)?; }
        zip.finish()?; Ok(())
    })
}

/// `destination` must be a newly created empty directory owned by this import.
/// The caller deletes the directory on failure; no user file is overwritten.
pub fn unpack(payload: &Path, destination: &Path) -> Result<PathBuf> {
    ensure!(destination.is_dir() && std::fs::read_dir(destination)?.next().is_none(), "Cloud download needs an empty destination directory");
    let mut zip = ZipArchive::new(File::open(payload)?)?;
    ensure!(zip.len() <= 1001, "Too many cloud bundle entries");
    let mut seen = HashSet::new(); let mut total = 0u64;
    for n in 0..zip.len() {
        let entry = zip.by_index(n)?;
        ensure!(entry.enclosed_name().is_some() && !entry.is_dir() && seen.insert(entry.name().to_owned()), "Unsafe or duplicate cloud bundle entry");
        total = total.checked_add(entry.size()).context("Bundle size overflow")?;
        ensure!(total <= MAX_FILE_BYTES, "Cloud bundle exceeds decoded size limit");
    }
    let manifest: Manifest = {
        let file = zip.by_name("manifest.json")?; ensure!(file.size() <= 256 * 1024, "Bundle manifest is too large"); serde_json::from_reader(file)?
    };
    ensure!(manifest.version == 1 && matches!(manifest.kind.as_str(), "emu" | "ora" | "image"), "Unsupported cloud bundle version or kind");
    valid_name(&manifest.name)?;
    ensure!(manifest.files.len() + 1 == zip.len(), "Unexpected cloud bundle entries");
    for (key, expected) in &manifest.files {
        valid_entry(key)?;
        ensure!(expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()), "Invalid dependency fingerprint");
        let path = destination.join(key);
        if let Some(parent) = path.parent() { private_dir(parent)?; }
        let mut entry = zip.by_name(key)?;
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        let copied = std::io::copy(&mut entry.by_ref().take(MAX_FILE_BYTES + 1), &mut out)?;
        ensure!(copied <= MAX_FILE_BYTES, "Expanded bundle entry exceeds limit"); out.sync_all()?;
        ensure!(&digest(&path)?.0 == expected, "Cloud dependency failed integrity verification");
    }
    let documents = manifest.files.keys().filter(|key| key.starts_with("document.")).collect::<Vec<_>>();
    ensure!(documents.len() == 1, "Bundle must contain exactly one document");
    let file = destination.join(documents[0]);
    let output = destination.join(&manifest.name);
    ensure!(!output.exists(), "Cloud filename collides with bundle data");
    if manifest.kind != "image" {
        ensure!(documents[0].as_str() == format!("document.{}", manifest.kind), "Bundle document type mismatch");
        let mut native = Native::read(&file)?.context("Invalid native bundle")?;
        native.map(&mut |doc| {
            let resolve = |path: &Path| -> Result<PathBuf> {
                let key = path.to_str().context("Invalid RAW reference")?; valid_entry(key)?;
                ensure!(key.starts_with("sources/") && manifest.files.contains_key(key), "RAW source missing from cloud bundle");
                Ok(destination.join(key))
            };
            if let Some(raw) = &mut doc.raw {
                let key = raw.source.to_str().context("Invalid RAW source")?;
                ensure!(manifest.files.get(key).is_some_and(|h| h.eq_ignore_ascii_case(&raw.source_sha256)), "RAW source fingerprint mismatch");
                raw.source = resolve(&raw.source)?;
            }
            for source in &mut doc.raw_originals { *source = resolve(source)?; }
            Ok(())
        })?;
        native.write(&output)?;
    } else {
        ensure!(Path::new(&manifest.name).extension() == file.extension(), "Image filename type mismatch");
        std::fs::rename(&file, &output)?;
        if manifest.files.contains_key("raw-sidecar.json") { std::fs::rename(destination.join("raw-sidecar.json"), crate::raw_settings::sidecar_path(&output)?)?; }
        crate::open(&output)?;
    }
    Ok(output)
}
fn valid_name(name: &str) -> Result<()> { ensure!(!name.is_empty() && name.len() <= 240 && !name.contains(['/', '\\', ':']) && !name.chars().any(char::is_control) && name != "." && name != "..", "Invalid portable filename"); Ok(()) }
fn valid_entry(name: &str) -> Result<()> {
    if let Some(source) = name.strip_prefix("sources/") { valid_name(source)?; } else { valid_name(name)?; ensure!(name.starts_with("document.") || name == "raw-sidecar.json", "Unexpected bundle entry"); }
    Ok(())
}
fn safe_extension(path: &Path) -> String { path.extension().and_then(|s| s.to_str()).filter(|s| s.len() <= 12 && s.bytes().all(|b| b.is_ascii_alphanumeric())).unwrap_or("bin").to_ascii_lowercase() }

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn native_roundtrip_retains_history_and_rejects_nonempty_destination() {
        let dir = tempfile::tempdir().unwrap(); let source = dir.path().join("art.ora");
        let doc = Document::new(2, 2); let graph = Graph::new(doc.clone(), "Original");
        crate::ora::write_full(&doc, Some(&graph), &source).unwrap(); let payload = dir.path().join("bundle.zip"); pack(&source, &payload).unwrap();
        let out = dir.path().join("download"); std::fs::create_dir(&out).unwrap(); let opened = unpack(&payload, &out).unwrap();
        let native = crate::ora::read_full(&opened).unwrap(); assert!(native.history_error.is_none()); assert_eq!(native.graph.unwrap().len(), graph.len());
        assert!(unpack(&payload, &out).is_err());
    }
    #[test] fn entry_paths_cannot_escape_destination() {
        for key in ["../a", "sources/../../a", "/absolute", "sources/C:\\x", "settings.json", "document.ora/evil"] { assert!(valid_entry(key).is_err(), "{key}"); }
    }
}
