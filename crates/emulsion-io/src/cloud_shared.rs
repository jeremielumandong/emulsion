//! Shared storyboards over cloud sync: no server database, only the
//! immutable revisions every device already uploads.
//!
//! Another artist's saves of the same file appear as heads the local file
//! does not include ([`emulsion_cloud::waiting_heads`]). Merging one
//! downloads that head and the common ancestor (verified, then kept in a
//! per-revision cache), the editor merges the boards three ways, and the
//! next save uploads a revision with both heads as parents
//! ([`super::enqueue`] reads the merged head from the board). Nothing is
//! replaced in place: both heads stay in the cloud until a merge revision
//! supersedes them.
use super::*;
use emulsion_cloud::{
    Account, Binding, Collaborator, RemoteRevision, Revision,
    providers::FileProvider,
    store::{self, private_dir},
};

/// This installation's identity: the cloud device ID when cloud sync was
/// ever set up, otherwise a stable ID kept beside the app's data.
pub fn device_id() -> String {
    let cloud = super::store();
    if cloud.root.join("index.json").exists()
        && let Ok(index) = cloud.read()
    {
        return index.device;
    }
    let path = crate::recent::data_dir().join("device-id");
    if let Ok(id) = std::fs::read_to_string(&path)
        && uuid_like(id.trim())
    {
        return id.trim().into();
    }
    let id = emulsion_cloud::id();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &id);
    id
}

fn uuid_like(s: &str) -> bool {
    s.len() == 36 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// What the cloud says about a synchronized file, from the last listing.
#[derive(Clone, Debug)]
pub struct Sharing {
    pub binding: Binding,
    /// Heads the local file does not include yet, newest first.
    pub waiting: Vec<RemoteRevision>,
    /// Everyone who saved revisions of the file.
    pub collaborators: Vec<Collaborator>,
    /// Revisions known for the file (listed and queued here).
    pub revisions: usize,
    /// This device's ID, to tell its own saves apart.
    pub device: String,
}

/// Remote and queued revisions of `project`, each once.
fn known_revisions(
    store: &Store,
    project: &str,
    remote: &[RemoteRevision],
) -> Result<Vec<RemoteRevision>> {
    let mut rows: Vec<RemoteRevision> = remote
        .iter()
        .filter(|r| r.revision.project == project)
        .cloned()
        .collect();
    for job in store.read()?.jobs {
        if job.revision.project == project && !rows.iter().any(|r| r.revision.id == job.revision.id)
        {
            rows.push(RemoteRevision {
                remote_id: String::new(),
                revision: job.revision,
            });
        }
    }
    Ok(rows)
}

/// The sharing state of the file at `path` from `remote` (a listing; the
/// remembered one when `None`). `None` when the file is not synchronized.
pub fn sharing(
    store: &Store,
    path: &Path,
    remote: Option<&[RemoteRevision]>,
) -> Result<Option<Sharing>> {
    if !store.root.join("index.json").exists() {
        return Ok(None);
    }
    let Ok(path) = path.canonicalize() else {
        return Ok(None);
    };
    let index = store.read()?;
    let Some(binding) = index.bindings.into_iter().find(|b| b.path == path) else {
        return Ok(None);
    };
    let remembered;
    let remote = match remote {
        Some(r) => r,
        None => {
            remembered = store.remembered(&binding.project)?;
            &remembered
        }
    };
    let rows = known_revisions(store, &binding.project, remote)?;
    Ok(Some(Sharing {
        waiting: emulsion_cloud::waiting_heads(&rows, &binding.project, binding.base.as_deref()),
        collaborators: emulsion_cloud::collaborators(&rows, &binding.project),
        revisions: rows.len(),
        device: index.device,
        binding,
    }))
}

/// Download `remote` into a fresh local copy (verified, unpacked into its
/// own new directory) bound to its cloud file. Returns the copy's path.
pub fn download_copy(
    store: &Store,
    account: &Account,
    provider: &impl FileProvider,
    remote: &RemoteRevision,
) -> Result<PathBuf> {
    let downloads = store.root.join("downloads");
    private_dir(&downloads)?;
    let destination = tempfile::Builder::new()
        .prefix("project-")
        .tempdir_in(&downloads)?;
    let path = download_into(store, provider, remote, destination.path())?;
    store.adopt(&path, account, remote)?;
    let _ = destination.keep();
    Ok(path)
}

/// Download, verify and unpack `remote` into the empty `destination`.
fn download_into(
    store: &Store,
    provider: &impl FileProvider,
    remote: &RemoteRevision,
    destination: &Path,
) -> Result<PathBuf> {
    let mut object = tempfile::NamedTempFile::new_in(&store.root)?;
    provider.download(remote, object.as_file_mut())?;
    let mut payload = tempfile::NamedTempFile::new_in(&store.root)?;
    store::extract_object(object.path(), &remote.revision, payload.as_file_mut())?;
    super::unpack(payload.path(), destination)
}

/// A read-only copy of one revision, downloaded once and kept under
/// `revisions/<id>/` of the cloud folder. Never bound to sync.
pub fn fetch_revision(
    store: &Store,
    provider: &impl FileProvider,
    remote: &RemoteRevision,
) -> Result<PathBuf> {
    remote.revision.validate()?;
    let cache = store.root.join("revisions");
    let dir = cache.join(&remote.revision.id);
    let file = dir.join("artwork").join(&remote.revision.name);
    if file.is_file() {
        return Ok(file);
    }
    private_dir(&cache)?;
    let temp = tempfile::Builder::new()
        .prefix("fetch-")
        .tempdir_in(&cache)?;
    let unpacked = download_into(store, provider, remote, temp.path())?;
    let name = unpacked.file_name().context("Missing filename")?.to_owned();
    let kept = temp.keep();
    if std::fs::rename(&kept, &dir).is_err() {
        // Another window fetched it first.
        let _ = std::fs::remove_dir_all(&kept);
    }
    Ok(dir.join("artwork").join(name))
}

/// Another artist's head, ready to merge into the local file.
#[derive(Clone, Debug)]
pub struct PreparedMerge {
    /// The head to merge, and a local copy of it.
    pub head: Revision,
    pub theirs: PathBuf,
    /// Their common ancestor with the local file, and a copy of it.
    pub base: Revision,
    pub base_path: PathBuf,
    /// Heads still waiting after this one.
    pub waiting: usize,
}

/// List the cloud, pick a waiting head of the file at `path` (`head`, or
/// the newest) and fetch it and the common ancestor. Errors when nothing
/// waits or the revisions share no history.
pub fn prepare_merge(
    store: &Store,
    provider: &impl FileProvider,
    path: &Path,
    head: Option<&str>,
) -> Result<PreparedMerge> {
    let path = path.canonicalize()?;
    let binding = store
        .read()?
        .bindings
        .into_iter()
        .find(|b| b.path == path)
        .context("This file is not synchronized. Sync it to a cloud account first.")?;
    let remote = provider.list()?;
    store.remember_remote(&binding.project, &remote)?;
    let rows = known_revisions(store, &binding.project, &remote)?;
    let waiting = emulsion_cloud::waiting_heads(&rows, &binding.project, binding.base.as_deref());
    let chosen = match head {
        Some(id) => waiting.iter().find(|r| r.revision.id == id),
        None => waiting.first(),
    }
    .context("No other saves are waiting to be merged.")?
    .clone();
    let local = binding
        .base
        .as_deref()
        .context("Save and sync this file once before merging.")?;
    let revisions: Vec<&Revision> = rows.iter().map(|r| &r.revision).collect();
    let base_id = emulsion_cloud::merge_base(revisions.iter().copied(), local, &chosen.revision.id)
        .context(
            "These versions share no history; download the other one as a separate copy instead.",
        )?;
    let base = remote
        .iter()
        .find(|r| r.revision.id == base_id)
        .context("The common version is not in the cloud yet; sync, then try again.")?
        .clone();
    let theirs = fetch_revision(store, provider, &chosen)?;
    let base_path = fetch_revision(store, provider, &base)?;
    Ok(PreparedMerge {
        head: chosen.revision,
        theirs,
        base: base.revision,
        base_path,
        waiting: waiting.len() - 1,
    })
}

#[cfg(test)]
#[path = "cloud_shared_tests.rs"]
mod tests;
