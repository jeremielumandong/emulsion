//! Scratch directories shared by a conversation and its CLI processes.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
static OUTSTANDING: AtomicUsize = AtomicUsize::new(0);

/// Keep the workspace until both the conversation and all of its processes end.
#[derive(Clone, Debug)]
pub struct SessionDirectory(Arc<Directory>);

#[derive(Debug)]
struct Directory {
    root: PathBuf,
    path: PathBuf,
}

impl SessionDirectory {
    pub fn create(root: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(root)?;
        let root = root.canonicalize()?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        loop {
            let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("{}-{timestamp}-{serial}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    OUTSTANDING.fetch_add(1, Ordering::SeqCst);
                    return Ok(Self(Arc::new(Directory { root, path })));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.0.path
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let root = self.root.clone();
        let path = self.path.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("assistant-cleanup".into())
            .spawn(move || {
                // Antivirus and recently closed handles can briefly hold files
                // open on Windows. Never block the UI while retrying.
                for attempt in 0..6 {
                    match remove_directory(&root, &path) {
                        Ok(()) => break,
                        Err(error) if attempt == 5 => {
                            tracing::warn!(%error, path = %path.display(), "could not clean up assistant session");
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(100)),
                    }
                }
                OUTSTANDING.fetch_sub(1, Ordering::SeqCst);
            })
        {
            OUTSTANDING.fetch_sub(1, Ordering::SeqCst);
            tracing::warn!(%error, "could not start assistant session cleanup");
        }
    }
}

/// Allow cleanup to finish during shutdown after releasing document leases.
/// Run off the UI thread, and never wait indefinitely for a stuck CLI.
pub fn wait_for_cleanup(timeout: Duration) {
    let start = Instant::now();
    while OUTSTANDING.load(Ordering::SeqCst) != 0 && start.elapsed() < timeout {
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Remove abandoned workspaces from earlier runs, preserving live or unknown
/// owners. Call on a background thread; this may recursively remove many files.
pub fn cleanup_stale(root: &Path) -> io::Result<()> {
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(owner_pid) else {
            continue;
        };
        if process_alive(pid) != Some(false) {
            continue;
        }
        if let Err(error) = remove_directory(&root, &entry.path()) {
            tracing::warn!(%error, path = %entry.path().display(), "could not clean up abandoned assistant session");
        }
    }
    Ok(())
}

/// Recognize only the old PID-entity names and our PID-timestamp-counter names.
fn owner_pid(name: &str) -> Option<u32> {
    let fields: Vec<_> = name.split('-').collect();
    if !(2..=3).contains(&fields.len())
        || fields
            .iter()
            .any(|field| field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    fields[0].parse().ok().filter(|pid| *pid != 0)
}

fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn remove_directory(root: &Path, path: &Path) -> io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    // Refuse junctions and symlinks, as well as renamed/replaced parent paths.
    if !metadata.is_dir() || is_link(&metadata) {
        return Ok(());
    }
    let canonical = path.canonicalize()?;
    if canonical.parent() != Some(root) || root.canonicalize()? != root {
        return Err(io::Error::other(
            "session directory is outside its original root",
        ));
    }
    std::fs::remove_dir_all(canonical)
}

#[cfg(unix)]
fn process_alive(pid: u32) -> Option<bool> {
    let pid = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: signal zero only probes a positive PID; it sends no signal.
    if unsafe { libc::kill(pid, 0) } == 0 {
        Some(true)
    } else if io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Some(false)
    } else {
        None
    }
}

#[cfg(windows)]
fn process_alive(pid: u32) -> Option<bool> {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: PID is a plain integer, returned handles are checked before use,
    // the exit-code pointer is valid, and the handle is closed exactly once.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return (GetLastError() == ERROR_INVALID_PARAMETER).then_some(false);
        }
        let mut exit_code = 0;
        let queried = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        (queried != 0).then_some(exit_code == STILL_ACTIVE as u32)
    }
}

#[cfg(not(any(unix, windows)))]
fn process_alive(_: u32) -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "emulsion-storage-test-{}-{}-{serial}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root.canonicalize().unwrap())
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn wait_until_removed(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while path.exists() {
            assert!(Instant::now() < deadline, "cleanup did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn last_lease_removes_nested_session_files() {
        let root = TestRoot::new();
        let directory = SessionDirectory::create(&root.0).unwrap();
        let path = directory.path().to_path_buf();
        std::fs::create_dir_all(path.join("codex-home/sessions")).unwrap();
        std::fs::write(path.join("codex-home/sessions/turn.jsonl"), "history").unwrap();
        let process_lease = directory.clone();
        drop(directory);
        assert!(path.join("codex-home/sessions/turn.jsonl").exists());
        drop(process_lease);
        wait_until_removed(&path);
    }

    fn dead_pid() -> u32 {
        // Probe rather than assuming an arbitrary PID is absent on the host.
        (1_000_000..1_001_000)
            .find(|pid| process_alive(*pid) == Some(false))
            .expect("a dead PID for the cleanup fixture")
    }

    #[test]
    fn stale_cleanup_preserves_live_and_unrelated_paths() {
        let root = TestRoot::new();
        let dead = dead_pid();
        let old = root.0.join(format!("{dead}-42"));
        let newer = root.0.join(format!("{dead}-123456789-0"));
        let live = root.0.join(format!("{}-17", std::process::id()));
        let unrelated = root.0.join("personal-files");
        let zero = root.0.join("0-42");
        for path in [&old, &newer, &live, &unrelated, &zero] {
            std::fs::create_dir_all(path.join("nested")).unwrap();
            std::fs::write(path.join("nested/file"), "keep or remove").unwrap();
        }
        let regular_file = root.0.join(format!("{dead}-43"));
        std::fs::write(&regular_file, "not a directory").unwrap();
        cleanup_stale(&root.0).unwrap();
        assert!(!old.exists());
        assert!(!newer.exists());
        for path in [&live, &unrelated, &zero] {
            assert!(path.join("nested/file").exists());
        }
        assert!(regular_file.exists());
    }

    #[test]
    fn stale_cleanup_preserves_directory_links() {
        let root = TestRoot::new();
        let outside = TestRoot::new();
        std::fs::write(outside.0.join("keep"), "outside the sessions root").unwrap();
        let link = root.0.join(format!("{}-42", dead_pid()));
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside.0, &link).unwrap();
        #[cfg(windows)]
        {
            if let Err(error) = std::os::windows::fs::symlink_dir(&outside.0, &link) {
                // Windows may require Developer Mode or elevation for symlinks.
                if error.raw_os_error() == Some(1314) {
                    return;
                }
                panic!("could not create fixture symlink: {error}");
            }
        }
        cleanup_stale(&root.0).unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(outside.0.join("keep").exists());
    }

    #[test]
    fn recognizes_only_session_directory_names() {
        assert_eq!(owner_pid("12-34"), Some(12));
        assert_eq!(owner_pid("12-3456789-0"), Some(12));
        for name in ["0-1", "12", "12-", "12-personal", "12-1-2-3", "../12-1"] {
            assert_eq!(owner_pid(name), None, "{name}");
        }
        assert_eq!(process_alive(std::process::id()), Some(true));
    }
}
