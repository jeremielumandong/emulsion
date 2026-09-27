//! Narrow-scope file adapters. Every upload creates an immutable revision.
use crate::{
    Account, Provider, RemoteRevision, Result, Revision, Store, auth,
    http::{self, Client},
    store::{self, HEADER_LIMIT},
};
use anyhow::{Context, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

const DRIVE: &str = "https://www.googleapis.com/drive/v3/files";
const GRAPH: &str = "https://graph.microsoft.com/v1.0";
const CHUNK: usize = 8 * 1024 * 1024; // Also a multiple of Graph's 320 KiB below.
const GRAPH_CHUNK: usize = 10 * 320 * 1024;
const MAX_OBJECTS: usize = 20_000;

pub trait FileProvider {
    fn list(&self) -> Result<Vec<RemoteRevision>>;
    fn upload(&self, revision: &Revision, object: &Path) -> Result<()>;
    fn download(&self, remote: &RemoteRevision, out: &mut File) -> Result<()>;
}
pub struct Files {
    account: Account,
    token: String,
    client: Client,
}
impl Files {
    pub fn new(account: Account, token: String) -> Result<Self> {
        ensure!(
            account.provider != Provider::GooglePhotos,
            "Photos is an import source, not a file provider"
        );
        Ok(Self {
            account,
            token,
            client: Client::default(),
        })
    }
    pub fn initialize(account: &mut Account, token: &str) -> Result<()> {
        let client = Client::default();
        account.root = match account.provider {
            Provider::GoogleDrive => {
                let url = http::query(
                    DRIVE,
                    &[
                        (
                            "q",
                            "trashed = false and mimeType = 'application/vnd.google-apps.folder' and appProperties has { key='emulsion' and value='root-v1' }",
                        ),
                        ("fields", "files(id)"),
                    ],
                );
                let data = client.json("GET", &url, Some(token), None)?;
                if let Some(id) = data["files"]
                    .as_array()
                    .and_then(|v| v.first())
                    .and_then(|v| v["id"].as_str())
                {
                    id.to_string()
                } else {
                    http::field(&client.json("POST", DRIVE, Some(token), Some(&json!({"name":"Emulsion", "mimeType":"application/vnd.google-apps.folder", "appProperties":{"emulsion":"root-v1"}})))?, "id")?
                }
            }
            Provider::Dropbox => String::new(),
            Provider::OneDrive => http::field(
                &client.json(
                    "GET",
                    &format!("{GRAPH}/me/drive/special/approot"),
                    Some(token),
                    None,
                )?,
                "id",
            )?,
            Provider::GooglePhotos => String::new(),
        };
        Ok(())
    }
    fn objects(&self) -> Result<Vec<(String, String)>> {
        let mut out = vec![];
        let mut cursor = String::new();
        for _ in 0..1000 {
            let (value, entries, next) = match self.account.provider {
                Provider::GoogleDrive => {
                    let url = http::query(
                        DRIVE,
                        &[
                            (
                                "q",
                                "trashed = false and appProperties has { key='emulsion' and value='revision-v1' }",
                            ),
                            ("fields", "nextPageToken,files(id,name)"),
                            ("pageSize", "1000"),
                            ("pageToken", &cursor),
                        ],
                    );
                    (
                        self.client.json("GET", &url, Some(&self.token), None)?,
                        "files",
                        "nextPageToken",
                    )
                }
                Provider::Dropbox => {
                    let (url, body) = if cursor.is_empty() {
                        (
                            "https://api.dropboxapi.com/2/files/list_folder",
                            json!({"path":"", "recursive":false, "limit":1000}),
                        )
                    } else {
                        (
                            "https://api.dropboxapi.com/2/files/list_folder/continue",
                            json!({"cursor":cursor}),
                        )
                    };
                    (
                        self.client
                            .json("POST", url, Some(&self.token), Some(&body))?,
                        "entries",
                        "cursor",
                    )
                }
                Provider::OneDrive => {
                    let url = if cursor.is_empty() {
                        format!(
                            "{GRAPH}/me/drive/items/{}/children?$select=id,name,file&$top=200",
                            http::segment(&self.account.root)
                        )
                    } else {
                        validate_graph_url(&cursor)?;
                        cursor.clone()
                    };
                    (
                        self.client.json("GET", &url, Some(&self.token), None)?,
                        "value",
                        "@odata.nextLink",
                    )
                }
                Provider::GooglePhotos => unreachable!(),
            };
            for item in value[entries]
                .as_array()
                .context("Provider returned invalid file listing")?
            {
                let Some(name) = item["name"].as_str() else {
                    continue;
                };
                if !name.ends_with(".emulsion") {
                    continue;
                }
                out.push((http::field(item, "id")?, name.into()));
                ensure!(
                    out.len() <= MAX_OBJECTS,
                    "Cloud revision limit reached; archive old projects before continuing"
                );
            }
            if self.account.provider == Provider::Dropbox && value["has_more"] == false {
                return Ok(out);
            }
            let next = value[next].as_str().unwrap_or("");
            if next.is_empty() {
                return Ok(out);
            }
            ensure!(next != cursor, "Provider repeated a listing cursor");
            cursor = next.to_string();
        }
        bail!("Cloud listing exceeds page limit")
    }
    fn get(&self, remote_id: &str, out: &mut impl Write, max: u64, header: bool) -> Result<()> {
        let range = header.then(|| ("Range", format!("bytes=0-{}", HEADER_LIMIT - 1)));
        let mut headers = range.into_iter().collect::<Vec<_>>();
        let (method, url, bearer) = match self.account.provider {
            Provider::GoogleDrive => (
                "GET",
                format!("{DRIVE}/{}?alt=media", http::segment(remote_id)),
                Some(self.token.as_str()),
            ),
            Provider::Dropbox => {
                headers.push(("Dropbox-API-Arg", json!({"path":remote_id}).to_string()));
                (
                    "POST",
                    "https://content.dropboxapi.com/2/files/download".into(),
                    Some(self.token.as_str()),
                )
            }
            Provider::OneDrive => {
                let value = self.client.json(
                    "GET",
                    &format!("{GRAPH}/me/drive/items/{}", http::segment(remote_id)),
                    Some(&self.token),
                    None,
                )?;
                let url = http::field(&value, "@microsoft.graph.downloadUrl")?;
                http::trusted_download(&url)?;
                ("GET", url, None)
            }
            Provider::GooglePhotos => unreachable!(),
        };
        self.client
            .download(method, &url, bearer, &headers, out, max)?;
        Ok(())
    }
    fn upload_drive(&self, revision: &Revision, path: &Path) -> Result<()> {
        let metadata = json!({"name":revision.object_name(), "parents":[self.account.root], "appProperties":{"emulsion":"revision-v1"}});
        let size = path.metadata()?.len();
        let response = self.client.send(
            "POST",
            "https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable",
            Some(&self.token),
            &[
                ("Content-Type", "application/json".into()),
                ("X-Upload-Content-Type", "application/octet-stream".into()),
                ("X-Upload-Content-Length", size.to_string()),
            ],
            &serde_json::to_vec(&metadata)?,
        )?;
        response.success()?;
        let session = response.header("location")?;
        let url = http::ensure_https(&session)?;
        ensure!(
            url.host_str() == Some("www.googleapis.com"),
            "Invalid Drive upload session host"
        );
        let mut file = File::open(path)?;
        let mut offset = 0;
        while offset < size {
            let bytes = read_chunk(&mut file, (size - offset).min(CHUNK as u64) as usize)?;
            let end = offset + bytes.len() as u64;
            let response = self.client.send(
                "PUT",
                &session,
                Some(&self.token),
                &[
                    ("Content-Type", "application/octet-stream".into()),
                    (
                        "Content-Range",
                        format!("bytes {offset}-{}/{size}", end - 1),
                    ),
                ],
                &bytes,
            )?;
            if end < size {
                ensure!(
                    response.status == 308,
                    "Drive did not acknowledge the upload chunk"
                );
            } else {
                response.success()?;
                ensure!(
                    matches!(response.status, 200 | 201),
                    "Drive has not committed the uploaded revision yet"
                );
            }
            offset = end;
        }
        Ok(())
    }
    fn upload_dropbox(&self, revision: &Revision, path: &Path) -> Result<()> {
        let post = |endpoint: &str, arg: Value, bytes: &[u8]| {
            self.client.send(
                "POST",
                &format!("https://content.dropboxapi.com/2/files/{endpoint}"),
                Some(&self.token),
                &[
                    ("Content-Type", "application/octet-stream".into()),
                    ("Dropbox-API-Arg", arg.to_string()),
                ],
                bytes,
            )
        };
        let value = post("upload_session/start", json!({"close":false}), &[])?.json()?;
        let session = http::field(&value, "session_id")?;
        let size = path.metadata()?.len();
        let mut file = File::open(path)?;
        let mut offset = 0;
        while offset < size {
            let bytes = read_chunk(&mut file, (size - offset).min(CHUNK as u64) as usize)?;
            let end = offset + bytes.len() as u64;
            let cursor = json!({"session_id":session, "offset":offset});
            if end == size {
                post("upload_session/finish", json!({"cursor":cursor, "commit":{"path":format!("/{}", revision.object_name()), "mode":"add", "autorename":false, "strict_conflict":true, "mute":true}}), &bytes)?.success()?;
            } else {
                post(
                    "upload_session/append_v2",
                    json!({"cursor":cursor, "close":false}),
                    &bytes,
                )?
                .success()?;
            }
            offset = end;
        }
        Ok(())
    }
    fn upload_onedrive(&self, revision: &Revision, path: &Path) -> Result<()> {
        let url = format!(
            "{GRAPH}/me/drive/items/{}:/{}:/createUploadSession",
            http::segment(&self.account.root),
            revision.object_name()
        );
        let value = self.client.json("POST", &url, Some(&self.token), Some(&json!({"item":{"@microsoft.graph.conflictBehavior":"fail", "name":revision.object_name()}})))?;
        let session = http::field(&value, "uploadUrl")?;
        http::trusted_download(&session)?;
        let size = path.metadata()?.len();
        let mut file = File::open(path)?;
        let mut offset = 0;
        while offset < size {
            let bytes = read_chunk(&mut file, (size - offset).min(GRAPH_CHUNK as u64) as usize)?;
            let end = offset + bytes.len() as u64;
            // The upload URL is already authorized; Graph explicitly forbids adding a bearer here.
            let response = self.client.send(
                "PUT",
                &session,
                None,
                &[
                    ("Content-Type", "application/octet-stream".into()),
                    (
                        "Content-Range",
                        format!("bytes {offset}-{}/{size}", end - 1),
                    ),
                ],
                &bytes,
            )?;
            response.success()?;
            if end < size {
                ensure!(
                    response.status == 202,
                    "OneDrive did not acknowledge upload chunk"
                );
            } else {
                ensure!(
                    matches!(response.status, 200 | 201),
                    "OneDrive has not committed the uploaded revision yet"
                );
            }
            offset = end;
        }
        Ok(())
    }
}
impl FileProvider for Files {
    fn list(&self) -> Result<Vec<RemoteRevision>> {
        let mut out = Vec::new();
        for (remote_id, name) in self.objects()? {
            let mut header = vec![];
            self.get(&remote_id, &mut header, HEADER_LIMIT as u64, true)?;
            let revision = store::read_header(&mut header.as_slice())?;
            ensure!(
                revision.object_name() == name,
                "Cloud revision name and header disagree"
            );
            if let Some(existing) = out
                .iter()
                .find(|r: &&RemoteRevision| r.revision.id == revision.id)
            {
                ensure!(
                    existing.revision == revision,
                    "Duplicate cloud revision has different content"
                );
                continue;
            }
            out.push(RemoteRevision {
                remote_id,
                revision,
            });
        }
        Ok(out)
    }
    fn upload(&self, revision: &Revision, object: &Path) -> Result<()> {
        revision.validate()?;
        store::verify_object(object, revision)?;
        match self.account.provider {
            Provider::GoogleDrive => self.upload_drive(revision, object),
            Provider::Dropbox => self.upload_dropbox(revision, object),
            Provider::OneDrive => self.upload_onedrive(revision, object),
            Provider::GooglePhotos => unreachable!(),
        }
    }
    fn download(&self, remote: &RemoteRevision, out: &mut File) -> Result<()> {
        remote.revision.validate()?;
        self.get(
            &remote.remote_id,
            out,
            remote.revision.bytes + HEADER_LIMIT as u64,
            false,
        )
    }
}
fn read_chunk(file: &mut File, size: usize) -> Result<Vec<u8>> {
    let mut data = vec![0; size];
    file.read_exact(&mut data)?;
    Ok(data)
}
fn validate_graph_url(value: &str) -> Result<()> {
    let u = http::ensure_https(value)?;
    ensure!(
        u.host_str() == Some("graph.microsoft.com") && u.path().starts_with("/v1.0/"),
        "Invalid Graph pagination URL"
    );
    Ok(())
}

/// Upload existing jobs only; local saves never call a provider.
pub fn synchronize(
    store: &Store,
    account: &Account,
    provider: &impl FileProvider,
) -> Result<Vec<RemoteRevision>> {
    // Serialize transfers across windows/processes without holding the index lock.
    let transfer_lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(store.root.join("transfer.lock"))?;
    transfer_lock
        .try_lock()
        .map_err(|_| anyhow::anyhow!("Cloud sync is already running in another window"))?;
    let mut remote = provider.list()?;
    for job in store.pending(account)? {
        ensure!(
            store
                .read()?
                .accounts
                .iter()
                .any(|a| a.provider == account.provider && a.id == account.id),
            "Account disconnected; pending work was kept"
        );
        if let Some(existing) = remote.iter().find(|r| r.revision.id == job.revision.id) {
            ensure!(
                existing.revision == job.revision,
                "Uploaded revision identity has conflicting metadata"
            );
            // An ambiguous commit is considered complete only after the bytes verify.
            let mut file = tempfile::tempfile_in(&store.root)?;
            provider.download(existing, &mut file)?;
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::Start(0))?;
            let mut named = tempfile::NamedTempFile::new_in(&store.root)?;
            std::io::copy(&mut file, &mut named)?;
            store::verify_object(named.path(), &job.revision)?;
            store.complete(&job)?;
            continue;
        }
        let result = provider.upload(&job.revision, &store.object_path(&job.revision.id)?);
        match result {
            Ok(()) => {
                store.complete(&job)?;
                remote.push(RemoteRevision {
                    remote_id: String::new(),
                    revision: job.revision,
                });
            }
            Err(error) => {
                store.failed(&job, error.to_string())?;
                return Err(error);
            }
        }
    }
    provider.list()
}
pub fn connected(store: &Store, account: &Account) -> Result<Files> {
    Files::new(account.clone(), auth::access(store, account)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Fake {
        entries: RefCell<Vec<(RemoteRevision, Vec<u8>)>>,
        ambiguous: RefCell<bool>,
    }
    impl FileProvider for Fake {
        fn list(&self) -> Result<Vec<RemoteRevision>> {
            Ok(self.entries.borrow().iter().map(|e| e.0.clone()).collect())
        }
        fn upload(&self, r: &Revision, file: &Path) -> Result<()> {
            self.entries.borrow_mut().push((
                RemoteRevision {
                    remote_id: r.id.clone(),
                    revision: r.clone(),
                },
                std::fs::read(file)?,
            ));
            if *self.ambiguous.borrow() {
                *self.ambiguous.borrow_mut() = false;
                bail!("Connection interrupted after commit")
            }
            Ok(())
        }
        fn download(&self, r: &RemoteRevision, out: &mut File) -> Result<()> {
            out.write_all(
                &self
                    .entries
                    .borrow()
                    .iter()
                    .find(|e| e.0.remote_id == r.remote_id)
                    .unwrap()
                    .1,
            )?;
            Ok(())
        }
    }
    #[test]
    fn ambiguous_commit_retry_verifies_existing_bytes_without_duplicate_upload() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("cloud"));
        let account = Account {
            registration: String::new(),
            provider: Provider::Dropbox,
            id: "a".into(),
            label: "Artist".into(),
            root: String::new(),
            persistent_credentials: false,
        };
        store.connect(account.clone()).unwrap();
        let path = dir.path().join("art.ora");
        std::fs::write(&path, b"native artwork").unwrap();
        store.bind(&path, account.provider).unwrap();
        store.enqueue(&path, &path).unwrap();
        let remote = Fake {
            entries: RefCell::new(vec![]),
            ambiguous: RefCell::new(true),
        };
        assert!(synchronize(&store, &account, &remote).is_err());
        assert_eq!(store.read().unwrap().jobs.len(), 1);
        store.retry().unwrap();
        synchronize(&store, &account, &remote).unwrap();
        assert!(store.read().unwrap().jobs.is_empty());
        assert_eq!(remote.entries.borrow().len(), 1);
    }
}
