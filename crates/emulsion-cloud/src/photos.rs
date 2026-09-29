//! User-selected imports. Never persist Picker media URLs as library locations.
use crate::{
    Result,
    http::{self, Client},
    now,
    store::{atomic, digest, private_dir},
};
use anyhow::{Context, bail, ensure};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const API: &str = "https://photospicker.googleapis.com/v1";
const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;
pub struct Session {
    pub id: String,
    pub picker_uri: String,
    pub interval: Duration,
    pub timeout: Duration,
}
#[derive(Default)]
pub struct ImportReport {
    pub paths: Vec<PathBuf>,
    pub skipped: usize,
    pub failed: usize,
    pub note: String,
}
pub fn begin(token: &str) -> Result<Session> {
    let value = Client::default().json(
        "POST",
        &format!("{API}/sessions"),
        Some(token),
        Some(&json!({})),
    )?;
    let picker_uri = http::field(&value, "pickerUri")?;
    let url = http::ensure_https(&picker_uri)?;
    ensure!(
        url.host_str() == Some("photos.google.com"),
        "Invalid photo selection URL"
    );
    Ok(Session {
        id: http::field(&value, "id")?,
        picker_uri,
        interval: seconds(&value["pollingConfig"]["pollInterval"], 5.)
            .clamp(Duration::from_secs(1), Duration::from_secs(60)),
        timeout: seconds(&value["pollingConfig"]["timeout"], 600.).min(Duration::from_secs(1800)),
    })
}
fn seconds(value: &Value, fallback: f64) -> Duration {
    let secs = value
        .as_str()
        .and_then(|s| s.strip_suffix('s'))
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s > 0.)
        .unwrap_or(fallback);
    Duration::from_secs_f64(secs.min(1800.))
}
pub fn remove(token: &str, session: &Session) -> Result<()> {
    Client::default()
        .send(
            "DELETE",
            &format!("{API}/sessions/{}", http::segment(&session.id)),
            Some(token),
            &[],
            &[],
        )?
        .success()
}
pub fn import(
    token: &str,
    session: &Session,
    destination: &Path,
    cancelled: &AtomicBool,
    validate: impl Fn(&Path) -> Result<()>,
) -> Result<ImportReport> {
    let result = import_inner(token, session, destination, cancelled, validate);
    let _ = remove(token, session);
    result
}
fn import_inner(
    token: &str,
    session: &Session,
    destination: &Path,
    cancelled: &AtomicBool,
    validate: impl Fn(&Path) -> Result<()>,
) -> Result<ImportReport> {
    private_dir(destination)?;
    let client = Client::default();
    let started = Instant::now();
    loop {
        ensure!(!cancelled.load(Ordering::Relaxed), "Photo import cancelled");
        ensure!(
            started.elapsed() < session.timeout,
            "Photo selection expired; start a new selection"
        );
        let value = client.json(
            "GET",
            &format!("{API}/sessions/{}", http::segment(&session.id)),
            Some(token),
            None,
        )?;
        if value["mediaItemsSet"] == true {
            break;
        }
        let delay = seconds(
            &value["pollingConfig"]["pollInterval"],
            session.interval.as_secs_f64(),
        )
        .clamp(Duration::from_secs(1), Duration::from_secs(60));
        let until = Instant::now() + delay;
        while Instant::now() < until && !cancelled.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let mut report = ImportReport::default();
    let mut cursor = String::new();
    let mut count = 0;
    for _ in 0..100 {
        if cancelled.load(Ordering::Relaxed) {
            report.note = "Cancelled; completed imports were kept.".into();
            return Ok(report);
        }
        let url = http::query(
            &format!("{API}/mediaItems"),
            &[
                ("sessionId", &session.id),
                ("pageSize", "100"),
                ("pageToken", &cursor),
            ],
        );
        let value = match client.json("GET", &url, Some(token), None) {
            Ok(value) => value,
            Err(error) if !report.paths.is_empty() => {
                report.failed += 1;
                report.note = format!(
                    "Import interrupted: {error}. Completed imports were kept; select remaining photos again."
                );
                return Ok(report);
            }
            Err(error) => return Err(error),
        };
        for item in value["mediaItems"]
            .as_array()
            .context("Invalid photo selection response")?
        {
            if cancelled.load(Ordering::Relaxed) {
                report.note = "Cancelled; completed imports were kept.".into();
                return Ok(report);
            }
            count += 1;
            if count > 2000 {
                report.note = "Reached the 2,000 photo import limit. Select remaining photos in a new import.".into();
                return Ok(report);
            }
            let mime = item["mediaFile"]["mimeType"].as_str().unwrap_or("");
            let Some(ext) = extension(mime) else {
                report.skipped += 1;
                continue;
            };
            let result = (|| -> Result<PathBuf> {
                let base = http::field(&item["mediaFile"], "baseUrl")?;
                validate_media_url(&base)?;
                let mut staged = tempfile::Builder::new()
                    .suffix(&format!(".{ext}"))
                    .tempfile_in(destination)?;
                client.download(
                    "GET",
                    &format!("{base}=d"),
                    Some(token),
                    &[],
                    staged.as_file_mut(),
                    MAX_IMAGE_BYTES,
                )?;
                staged.as_file().sync_all()?;
                validate(staged.path())?;
                let (hash, _) = digest(staged.path())?;
                let path = destination.join(format!("{hash}.{ext}"));
                if path.exists() {
                    ensure!(
                        digest(&path)?.0 == hash,
                        "Existing imported file is damaged"
                    );
                } else {
                    staged.persist_noclobber(&path).map_err(|e| e.error)?;
                }
                // URLs and tokens intentionally excluded from provenance.
                let provenance = json!({"version":1,"source":"google_photos","media_id":item["id"],"original_name":item["mediaFile"]["filename"],"sha256":hash,"imported_at":now()});
                atomic(&destination.join(format!("{hash}.source.json")), |f| {
                    serde_json::to_writer(f, &provenance)?;
                    Ok(())
                })?;
                Ok(path)
            })();
            match result {
                Ok(path) => {
                    if !report.paths.contains(&path) {
                        report.paths.push(path);
                    }
                }
                Err(_) => report.failed += 1,
            }
        }
        let next = value["nextPageToken"].as_str().unwrap_or("");
        if next.is_empty() {
            return Ok(report);
        }
        ensure!(next != cursor, "Photo selection repeated a page");
        cursor = next.to_string();
    }
    bail!("Photo selection exceeds page limit")
}
fn validate_media_url(value: &str) -> Result<()> {
    let url = http::ensure_https(value)?;
    let host = url.host_str().unwrap_or("");
    ensure!(
        host == "googleusercontent.com" || host.ends_with(".googleusercontent.com"),
        "Invalid Photos media host"
    );
    Ok(())
}
fn extension(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/tiff" => Some("tiff"),
        "image/heic" | "image/heif" => Some("heic"),
        "image/avif" => Some("avif"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn poll_duration_and_media_validation_are_bounded() {
        assert_eq!(seconds(&json!("2.5s"), 5.), Duration::from_millis(2500));
        assert_eq!(seconds(&json!("NaNs"), 5.), Duration::from_secs(5));
        assert!(validate_media_url("https://lh3.googleusercontent.com/photo").is_ok());
        assert!(validate_media_url("https://lh3.googleusercontent.com.evil.test/photo").is_err());
        assert_eq!(extension("video/mp4"), None);
    }
}
