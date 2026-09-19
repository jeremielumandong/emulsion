//! The model manifest and downloader.
//!
//! Every local model Emulsion can use is listed here with its files, their
//! sizes and the licence they come under. Models live in
//! `<data dir>/models/<id>/` and are fetched on demand with progress and
//! cancellation; nothing downloads without being asked. Sizes are checked
//! after a download and a sha256 is recorded so later runs can spot a
//! damaged file.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// What a model is for; one task can have several candidate models.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    /// Promptable segmentation (points, box).
    Segment,
    /// Salient-object matte, for Select Subject and Remove Background.
    Matte,
    /// Monocular depth.
    Depth,
    /// Fill masked pixels from their surroundings.
    Inpaint,
    /// Super-resolution.
    Upscale,
}

impl Task {
    pub fn label(self) -> &'static str {
        match self {
            Task::Segment => "segmentation",
            Task::Matte => "subject matte",
            Task::Depth => "depth",
            Task::Inpaint => "fill",
            Task::Upscale => "upscale",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ModelFile {
    pub name: &'static str,
    pub url: &'static str,
    /// Expected size in bytes, from the publisher.
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct ModelSpec {
    /// Directory name and the id used by settings and the assistant.
    pub id: &'static str,
    pub name: &'static str,
    pub task: Task,
    pub files: &'static [ModelFile],
    pub license: &'static str,
    pub note: &'static str,
    /// Preferred model for its task when several are installed.
    pub default: bool,
}

impl ModelSpec {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }
}

/// Every model Emulsion knows how to run.
pub const MANIFEST: &[ModelSpec] = &[
    ModelSpec {
        id: "slimsam",
        name: "SlimSAM 77",
        task: Task::Segment,
        files: &[
            ModelFile {
                name: "vision_encoder.onnx",
                url: "https://huggingface.co/Xenova/slimsam-77-uniform/resolve/main/onnx/vision_encoder.onnx",
                bytes: 23_276_014,
            },
            ModelFile {
                name: "prompt_encoder_mask_decoder.onnx",
                url: "https://huggingface.co/Xenova/slimsam-77-uniform/resolve/main/onnx/prompt_encoder_mask_decoder.onnx",
                bytes: 16_557_892,
            },
        ],
        license: "Apache-2.0",
        note: "Segment Anything pruned to 77 %: click or box a thing to select it. One encode per image, then each click answers in well under a second on a CPU.",
        default: true,
    },
    ModelSpec {
        id: "rmbg14",
        name: "RMBG 1.4 (quantized)",
        task: Task::Matte,
        files: &[ModelFile {
            name: "model_quantized.onnx",
            url: "https://huggingface.co/briaai/RMBG-1.4/resolve/main/onnx/model_quantized.onnx",
            bytes: 44_403_226,
        }],
        license: "bria-rmbg-1.4 (non-commercial use)",
        note: "Bria's salient-object matte for Select Subject and Remove Background. Check the licence before commercial use; ISNet below is Apache-2.0.",
        default: true,
    },
    ModelSpec {
        id: "isnet",
        name: "ISNet general use",
        task: Task::Matte,
        files: &[ModelFile {
            name: "isnet-general-use.onnx",
            url: "https://github.com/danielgatis/rembg/releases/download/v0.0.0/isnet-general-use.onnx",
            bytes: 178_648_008,
        }],
        license: "Apache-2.0",
        note: "The matte model rembg ships. Larger than RMBG, free for any use.",
        default: false,
    },
    ModelSpec {
        id: "depth-anything-v2-small",
        name: "Depth Anything v2 small (quantized)",
        task: Task::Depth,
        files: &[ModelFile {
            name: "model_quantized.onnx",
            url: "https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/main/onnx/model_quantized.onnx",
            bytes: 26_641_268,
        }],
        license: "Apache-2.0",
        note: "Relative depth for depth-of-field masks, fog and depth-aware grading.",
        default: true,
    },
    ModelSpec {
        id: "lama",
        name: "LaMa",
        task: Task::Inpaint,
        files: &[ModelFile {
            name: "lama_fp32.onnx",
            url: "https://huggingface.co/Carve/LaMa-ONNX/resolve/main/lama_fp32.onnx",
            bytes: 208_044_816,
        }],
        license: "Apache-2.0",
        note: "Large-mask inpainting: removes objects and fills holes better than content-aware fill on textures and structure.",
        default: true,
    },
    ModelSpec {
        id: "swin2sr-realworld-x4",
        name: "Swin2SR real-world ×4 (quantized)",
        task: Task::Upscale,
        files: &[ModelFile {
            name: "model_quantized.onnx",
            url: "https://huggingface.co/Xenova/swin2SR-realworld-sr-x4-64-bsrgan-psnr/resolve/main/onnx/model_quantized.onnx",
            bytes: 20_971_520,
        }],
        license: "Apache-2.0",
        note: "4× super-resolution tuned for photos with real-world degradation.",
        default: true,
    },
    ModelSpec {
        id: "real-esrgan-x4",
        name: "Real-ESRGAN ×4",
        task: Task::Upscale,
        files: &[ModelFile {
            name: "real_esrgan_x4.onnx",
            url: "https://huggingface.co/facefusion/models-3.0.0/resolve/main/real_esrgan_x4.onnx",
            bytes: 69_183_432,
        }],
        license: "BSD-3-Clause",
        note: "The well-known ×4 upscaler for photos and renders; larger and slower than Swin2SR, often cleaner on detail.",
        default: false,
    },
    ModelSpec {
        id: "swin2sr-lightweight-x2",
        name: "Swin2SR lightweight ×2",
        task: Task::Upscale,
        files: &[ModelFile {
            name: "model.onnx",
            url: "https://huggingface.co/Xenova/swin2SR-lightweight-x2-64/resolve/main/onnx/model.onnx",
            bytes: 8_388_608,
        }],
        license: "Apache-2.0",
        note: "Fast 2× upscale for clean sources.",
        default: false,
    },
];

pub fn spec(id: &str) -> Option<&'static ModelSpec> {
    MANIFEST.iter().find(|m| m.id == id)
}

/// The installed model preferred for `task`: the default if present, else
/// any installed one.
pub fn installed_for(task: Task) -> Option<&'static ModelSpec> {
    let dir = models_dir();
    let mut any = None;
    for m in MANIFEST.iter().filter(|m| m.task == task) {
        if status_in(&dir, m) == Status::Installed {
            if m.default {
                return Some(m);
            }
            any.get_or_insert(m);
        }
    }
    any
}

/// Where models live: `$EMULSION_MODELS_DIR`, else `<data dir>/models`.
pub fn models_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("EMULSION_MODELS_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    data_dir().join("models")
}

fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(d).join("emulsion");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/emulsion")
}

pub fn model_dir(m: &ModelSpec) -> PathBuf {
    models_dir().join(m.id)
}

pub fn file_path(m: &ModelSpec, f: &ModelFile) -> PathBuf {
    model_dir(m).join(f.name)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Installed,
    Missing,
    /// Some files present, or a file with the wrong size.
    Partial,
}

pub fn status(m: &ModelSpec) -> Status {
    status_in(&models_dir(), m)
}

fn status_in(dir: &Path, m: &ModelSpec) -> Status {
    let mut have = 0;
    for f in m.files {
        match fs::metadata(dir.join(m.id).join(f.name)) {
            Ok(md) if md.len() > 0 => {
                if size_ok(md.len(), f.bytes) {
                    have += 1;
                } else {
                    return Status::Partial;
                }
            }
            _ => {}
        }
    }
    if have == m.files.len() {
        Status::Installed
    } else if have == 0 {
        Status::Missing
    } else {
        Status::Partial
    }
}

/// Publishers occasionally re-export a model; accept a few percent drift
/// but not a truncated file.
fn size_ok(actual: u64, expected: u64) -> bool {
    let lo = expected - expected / 20;
    let hi = expected + expected / 20;
    (lo..=hi).contains(&actual)
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("download cancelled")]
    Cancelled,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("fetching {url}: {msg}")]
    Http { url: String, msg: String },
    #[error("{name} is {got} bytes, expected about {want}")]
    Size { name: String, got: u64, want: u64 },
}

/// Bytes done and total, for a progress bar.
pub type Progress<'a> = &'a (dyn Fn(u64, u64) + Sync);

/// Fetch every missing file of `m`, reporting progress; a set `cancel`
/// flag stops it and leaves no partial file behind.
pub fn download(
    m: &ModelSpec,
    progress: Progress,
    cancel: &AtomicBool,
) -> Result<(), DownloadError> {
    let dir = model_dir(m);
    fs::create_dir_all(&dir)?;
    let total = m.total_bytes();
    let mut done: u64 = 0;
    for f in m.files {
        let dest = dir.join(f.name);
        if fs::metadata(&dest).is_ok_and(|md| size_ok(md.len(), f.bytes)) {
            done += f.bytes;
            progress(done, total);
            continue;
        }
        let part = dir.join(format!("{}.part", f.name));
        let result = fetch(f, &part, done, total, progress, cancel);
        if let Err(e) = result {
            let _ = fs::remove_file(&part);
            return Err(e);
        }
        let got = fs::metadata(&part)?.len();
        if !size_ok(got, f.bytes) {
            let _ = fs::remove_file(&part);
            return Err(DownloadError::Size {
                name: f.name.into(),
                got,
                want: f.bytes,
            });
        }
        fs::rename(&part, &dest)?;
        done += f.bytes;
        progress(done, total);
    }
    record_hashes(m)?;
    Ok(())
}

fn fetch(
    f: &ModelFile,
    part: &Path,
    base: u64,
    total: u64,
    progress: Progress,
    cancel: &AtomicBool,
) -> Result<(), DownloadError> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(std::time::Duration::from_secs(30)))
            .timeout_recv_body(Some(std::time::Duration::from_secs(600)))
            .max_redirects(10)
            .build(),
    );
    let resp = agent
        .get(f.url)
        .header("User-Agent", "emulsion")
        .call()
        .map_err(|e| DownloadError::Http {
            url: f.url.into(),
            msg: e.to_string(),
        })?;
    let mut body = resp.into_body().into_reader();
    let mut out = std::io::BufWriter::new(fs::File::create(part)?);
    let mut buf = vec![0u8; 256 * 1024];
    let mut got: u64 = 0;
    let mut last_report = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let n = body.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        got += n as u64;
        if got - last_report >= 1 << 20 {
            last_report = got;
            progress(base + got.min(f.bytes), total);
        }
    }
    out.flush()?;
    Ok(())
}

/// sha256 of each file, written beside them as `sha256.json`.
fn record_hashes(m: &ModelSpec) -> std::io::Result<()> {
    use sha2::Digest;
    let mut map = serde_json::Map::new();
    for f in m.files {
        let mut file = fs::File::open(file_path(m, f))?;
        let mut hasher = sha2::Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let hex: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        map.insert(f.name.into(), serde_json::Value::String(hex));
    }
    fs::write(
        model_dir(m).join("sha256.json"),
        serde_json::to_vec_pretty(&serde_json::Value::Object(map)).unwrap_or_default(),
    )
}

/// Delete a model's files.
pub fn remove(m: &ModelSpec) -> std::io::Result<()> {
    let dir = model_dir(m);
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// Human-readable size.
pub fn human_bytes(b: u64) -> String {
    if b >= 1 << 30 {
        format!("{:.1} GB", b as f64 / (1u64 << 30) as f64)
    } else if b >= 1 << 20 {
        format!("{:.0} MB", b as f64 / (1u64 << 20) as f64)
    } else {
        format!("{:.0} kB", b as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_consistent() {
        let mut ids: Vec<&str> = MANIFEST.iter().map(|m| m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), MANIFEST.len(), "duplicate model id");
        for t in [
            Task::Segment,
            Task::Matte,
            Task::Depth,
            Task::Inpaint,
            Task::Upscale,
        ] {
            assert_eq!(
                MANIFEST.iter().filter(|m| m.task == t && m.default).count(),
                1,
                "exactly one default for {t:?}"
            );
        }
        for m in MANIFEST {
            assert!(
                m.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "{}",
                m.id
            );
            for f in m.files {
                assert!(f.url.starts_with("https://"), "{}", f.url);
                assert!(f.bytes > 1_000_000, "{}", f.name);
            }
        }
    }

    #[test]
    fn status_reads_the_directory() {
        let tmp = std::env::temp_dir().join(format!("emulsion-models-{}", std::process::id()));
        let m = spec("slimsam").unwrap();
        assert_eq!(status_in(&tmp, m), Status::Missing);
        let d = tmp.join(m.id);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(m.files[0].name), vec![0u8; 10]).unwrap();
        assert_eq!(
            status_in(&tmp, m),
            Status::Partial,
            "wrong size counts as partial"
        );
        for f in m.files {
            let file = fs::File::create(d.join(f.name)).unwrap();
            file.set_len(f.bytes).unwrap();
        }
        assert_eq!(status_in(&tmp, m), Status::Installed);
        fs::remove_dir_all(&tmp).unwrap();
        assert_eq!(human_bytes(44_403_226), "42 MB");
    }
}
