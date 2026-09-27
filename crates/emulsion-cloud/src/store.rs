use crate::{Account, Binding, Provider, RemoteRevision, Result, Revision, id, now};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}};

pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const HEADER_LIMIT: usize = 64 * 1024;
const MAGIC: &[u8; 8] = b"EMUCLOUD";
const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub revision: Revision,
    pub provider: Provider,
    pub account_id: String,
    pub attempts: u32,
    pub retry_at: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Index {
    pub version: u32,
    pub device: String,
    pub accounts: Vec<Account>,
    pub bindings: Vec<Binding>,
    pub jobs: Vec<Job>,
}
impl Default for Index {
    fn default() -> Self { Self { version: 1, device: id(), accounts: vec![], bindings: vec![], jobs: vec![] } }
}

#[derive(Clone)]
pub struct Store { pub root: PathBuf }
impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self { Self { root: root.into() } }
    pub fn read(&self) -> Result<Index> { self.update(|index| Ok(index.clone())) }
    pub fn update<T>(&self, change: impl FnOnce(&mut Index) -> Result<T>) -> Result<T> {
        private_dir(&self.root)?;
        let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(self.root.join("index.lock"))?;
        lock.lock()?;
        let path = self.root.join("index.json");
        let mut index: Index = match File::open(&path) {
            Ok(f) => { ensure!(f.metadata()?.len() <= MAX_INDEX_BYTES, "Cloud index is too large"); serde_json::from_reader(f).context("Cannot read cloud index; existing state was preserved")? },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Index::default(),
            Err(e) => return Err(e.into()),
        };
        ensure!(index.version == 1, "Unsupported cloud index version");
        uuid::Uuid::parse_str(&index.device)?;
        let value = change(&mut index)?;
        let bytes = serde_json::to_vec(&index)?;
        ensure!(bytes.len() as u64 <= MAX_INDEX_BYTES, "Cloud index is full");
        atomic(&path, |f| { f.write_all(&bytes)?; Ok(()) })?;
        Ok(value)
    }
    pub fn connect(&self, account: Account) -> Result<()> {
        self.update(|i| { i.accounts.retain(|a| a.provider != account.provider); i.accounts.push(account); Ok(()) })
    }
    pub fn disconnect(&self, provider: Provider) -> Result<()> {
        self.update(|i| { i.accounts.retain(|a| a.provider != provider); Ok(()) })
    }
    pub fn bind(&self, path: &Path, provider: Provider) -> Result<()> {
        ensure!(provider != Provider::GooglePhotos, "Google Photos supports import only");
        let path = path.canonicalize()?;
        self.update(|i| {
            let account = i.accounts.iter().find(|a| a.provider == provider).context("Connect this provider first")?;
            if let Some(existing) = i.bindings.iter_mut().find(|b| b.path == path) {
                ensure!(existing.provider == provider && existing.account_id == account.id, "This file is bound to another cloud account. Download a separate copy or stop its existing sync first.");
                existing.paused = false;
            } else {
                i.bindings.push(Binding { path, provider, account_id: account.id.clone(), project: id(), base: None, saved_hash: None, paused: false });
            }
            Ok(())
        })
    }
    pub fn set_paused(&self, path: &Path, paused: bool) -> Result<()> {
        let path = path.canonicalize()?;
        self.update(|i| { i.bindings.iter_mut().find(|b| b.path == path).context("File is not synchronized")?.paused = paused; Ok(()) })
    }
    pub fn object_path(&self, revision_id: &str) -> Result<PathBuf> {
        uuid::Uuid::parse_str(revision_id)?;
        Ok(self.root.join("outbox").join(format!("{revision_id}.emulsion")))
    }
    /// Caller holds the completed save generation stable until this returns.
    /// The payload can be a native portability bundle; `source` is its local binding.
    pub fn enqueue(&self, source: &Path, payload: &Path) -> Result<bool> {
        let source = source.canonicalize()?;
        let Some(binding) = self.read()?.bindings.into_iter().find(|b| b.path == source) else { return Ok(false) };
        let (hash, bytes) = digest(payload)?;
        ensure!(bytes > 0 && bytes <= MAX_FILE_BYTES, "Cloud file exceeds the 4 GiB transfer limit");
        self.update(|i| {
            let b = i.bindings.iter_mut().find(|b| b.path == source && b.project == binding.project).context("Cloud binding changed during snapshot")?;
            if b.saved_hash.as_ref() == Some(&hash) { return Ok(false); }
            ensure!(i.jobs.len() < 10_000, "Cloud queue is full; retry or pause sync");
            let revision = Revision { project: b.project.clone(), id: id(), parent: b.base.clone(), hash, name: source.file_name().context("Missing filename")?.to_string_lossy().into_owned(), created: now(), device: i.device.clone(), bytes };
            revision.validate()?;
            let path = self.object_path(&revision.id)?;
            private_dir(path.parent().unwrap())?;
            atomic(&path, |f| {
                write_header(f, &revision)?;
                let mut input = File::open(payload)?;
                std::io::copy(&mut input, f)?;
                Ok(())
            })?;
            // Check the captured bytes, not just the source before copying.
            verify_object(&path, &revision)?;
            b.base = Some(revision.id.clone());
            b.saved_hash = Some(revision.hash.clone());
            i.jobs.push(Job { revision, provider: b.provider, account_id: b.account_id.clone(), attempts: 0, retry_at: 0, error: None });
            Ok(true)
        })
    }
    pub fn pending(&self, account: &Account) -> Result<Vec<Job>> {
        let i = self.read()?;
        ensure!(i.accounts.iter().any(|a| a.provider == account.provider && a.id == account.id), "Account disconnected");
        Ok(i.jobs.into_iter().filter(|j| j.provider == account.provider && j.account_id == account.id && j.retry_at <= now() && i.bindings.iter().any(|b| b.project == j.revision.project && b.account_id == account.id && b.provider == account.provider && !b.paused)).collect())
    }
    pub fn complete(&self, job: &Job) -> Result<()> {
        self.update(|i| { i.jobs.retain(|j| j.revision.id != job.revision.id); Ok(()) })?;
        let _ = fs::remove_file(self.object_path(&job.revision.id)?);
        Ok(())
    }
    pub fn failed(&self, job: &Job, error: String) -> Result<()> {
        self.update(|i| {
            if let Some(j) = i.jobs.iter_mut().find(|j| j.revision.id == job.revision.id) {
                j.attempts = j.attempts.saturating_add(1);
                j.retry_at = now() + (15u64.saturating_mul(1u64 << j.attempts.min(8))).min(3600);
                j.error = Some(error.chars().take(400).collect());
            }
            Ok(())
        })
    }
    pub fn retry(&self) -> Result<()> { self.update(|i| { for j in &mut i.jobs { j.retry_at = 0; } Ok(()) }) }
    pub fn adopt(&self, path: &Path, account: &Account, remote: &RemoteRevision) -> Result<()> {
        let path = path.canonicalize()?;
        self.update(|i| {
            ensure!(!i.bindings.iter().any(|b| b.path == path), "Destination already synchronized");
            i.bindings.push(Binding { path, provider: account.provider, account_id: account.id.clone(), project: remote.revision.project.clone(), base: Some(remote.revision.id.clone()), saved_hash: None, paused: false });
            Ok(())
        })
    }
}

pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(path, fs::Permissions::from_mode(0o700))?; }
    Ok(())
}
pub fn atomic(path: &Path, write: impl FnOnce(&mut File) -> Result<()>) -> Result<()> {
    let parent = path.parent().context("Missing destination directory")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    write(temp.as_file_mut())?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)] File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn digest(path: &Path) -> Result<(String, u64)> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut size = 0;
    let mut buf = [0; 128 * 1024];
    loop { let n = f.read(&mut buf)?; if n == 0 { break; } size += n as u64; ensure!(size <= MAX_FILE_BYTES, "File exceeds transfer limit"); h.update(&buf[..n]); }
    Ok((hex(&h.finalize()), size))
}
pub fn write_header(out: &mut impl Write, revision: &Revision) -> Result<()> {
    revision.validate()?;
    let bytes = serde_json::to_vec(revision)?;
    ensure!(bytes.len() + 12 < HEADER_LIMIT, "Revision header too large");
    out.write_all(MAGIC)?;
    out.write_all(&(bytes.len() as u32).to_be_bytes())?;
    out.write_all(&bytes)?;
    Ok(())
}
pub fn read_header(input: &mut impl Read) -> Result<Revision> {
    let mut prefix = [0; 12];
    input.read_exact(&mut prefix)?;
    ensure!(&prefix[..8] == MAGIC, "Not an Emulsion cloud object");
    let len = u32::from_be_bytes(prefix[8..].try_into().unwrap()) as usize;
    ensure!(len > 0 && len + 12 < HEADER_LIMIT, "Invalid cloud header size");
    let mut bytes = vec![0; len]; input.read_exact(&mut bytes)?;
    let revision: Revision = serde_json::from_slice(&bytes)?;
    revision.validate()?;
    Ok(revision)
}
pub fn verify_object(path: &Path, expected: &Revision) -> Result<()> {
    extract_object(path, expected, &mut std::io::sink())
}
pub fn extract_object(path: &Path, expected: &Revision, out: &mut impl Write) -> Result<()> {
    let mut input = File::open(path)?;
    ensure!(&read_header(&mut input)? == expected, "Cloud revision metadata changed");
    let mut hash = Sha256::new(); let mut total = 0u64; let mut buf = [0; 128 * 1024];
    loop { let n = input.read(&mut buf)?; if n == 0 { break; } total += n as u64; ensure!(total <= expected.bytes, "Cloud payload exceeds declared size"); hash.update(&buf[..n]); out.write_all(&buf[..n])?; }
    ensure!(total == expected.bytes && hex(&hash.finalize()) == expected.hash, "Cloud file failed integrity verification");
    Ok(())
}

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, Store, PathBuf, Account) {
        let dir = tempfile::tempdir().unwrap(); let store = Store::new(dir.path().join("cloud"));
        let file = dir.path().join("art.ora"); fs::write(&file, b"first").unwrap();
        let account = Account { provider: Provider::GoogleDrive, id: "account-a".into(), label: "Artist".into(), root: "folder".into(), persistent_credentials: false };
        store.connect(account.clone()).unwrap(); store.bind(&file, account.provider).unwrap(); (dir, store, file, account)
    }
    #[test] fn restart_preserves_snapshots_and_parent_chain() {
        let (_dir, store, file, account) = setup();
        assert!(store.enqueue(&file, &file).unwrap()); assert!(!store.enqueue(&file, &file).unwrap());
        fs::write(&file, b"second").unwrap(); store.enqueue(&file, &file).unwrap();
        let loaded = Store::new(&store.root); let jobs = loaded.pending(&account).unwrap(); assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[1].revision.parent, Some(jobs[0].revision.id.clone()));
        let mut first = vec![]; extract_object(&store.object_path(&jobs[0].revision.id).unwrap(), &jobs[0].revision, &mut first).unwrap(); assert_eq!(first, b"first");
    }
    #[test] fn account_switch_and_pause_never_send_old_work() {
        let (_dir, store, file, mut account) = setup(); store.enqueue(&file, &file).unwrap();
        store.set_paused(&file, true).unwrap(); assert!(store.pending(&account).unwrap().is_empty());
        store.set_paused(&file, false).unwrap(); account.id = "account-b".into(); store.connect(account.clone()).unwrap();
        assert!(store.pending(&account).unwrap().is_empty()); assert!(store.bind(&file, account.provider).is_err()); assert_eq!(store.read().unwrap().jobs.len(), 1);
    }
    #[test] fn corruption_and_traversal_are_rejected() {
        let (_dir, store, file, account) = setup(); store.enqueue(&file, &file).unwrap(); let j = &store.pending(&account).unwrap()[0];
        let p = store.object_path(&j.revision.id).unwrap(); OpenOptions::new().append(true).open(&p).unwrap().write_all(b"tampered").unwrap();
        assert!(verify_object(&p, &j.revision).is_err()); assert!(store.object_path("../../oops").is_err());
        let mut rev = j.revision.clone(); rev.name = "../art.ora".into(); assert!(rev.validate().is_err());
    }
    #[test] fn concurrent_children_are_both_heads() {
        let (_dir, store, file, account) = setup(); store.enqueue(&file, &file).unwrap(); let base = store.pending(&account).unwrap()[0].revision.clone();
        let mut left = base.clone(); left.parent = Some(base.id.clone()); left.id = id();
        let mut right = left.clone(); right.id = id();
        let rows = [base, left, right].map(|revision| RemoteRevision { remote_id: id(), revision }); assert_eq!(crate::heads(&rows).len(), 2);
    }
}
