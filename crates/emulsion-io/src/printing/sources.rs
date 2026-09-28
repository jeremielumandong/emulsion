//! Immutable full-resolution photo and timestamped storyboard sources.
use super::*;
use emulsion_core::{Command, Document, Node, command::Slot};
use std::{
    path::{Path, PathBuf},
    process::{Command as Process, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhotoInput {
    pub path: PathBuf,
    #[serde(default)]
    pub params: Option<emulsion_core::raw::DevelopParams>,
    #[serde(default)]
    pub expected_digest: Option<String>,
}
pub fn photo_document(input: &PhotoInput) -> Result<Document> {
    if let Some(params) = input.params {
        let source = if let Some(digest) = &input.expected_digest {
            crate::photo_develop::PhotoSource::load_verified(&input.path, digest)?
        } else {
            crate::photo_develop::PhotoSource::load(&input.path)?
        };
        let raster = source.develop_with(&params)?;
        let mut doc = raster_document(raster)?;
        doc.raw_originals
            .push(crate::photo_develop::original_path(&input.path)?);
        Ok(doc)
    } else {
        if let Some(digest) = &input.expected_digest {
            if !crate::raw::source_digest(&input.path)?.eq_ignore_ascii_case(digest) {
                bail!("Photo changed; refresh the Library before printing")
            }
        }
        Ok(crate::photo_develop::open_saved(&input.path)?)
    }
}
fn raster_document(raster: emulsion_raster::Raster) -> Result<Document> {
    let mut doc = Document::new(raster.width(), raster.height());
    doc.resolution = 300.;
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Print snapshot",
            Arc::new(raster),
            Default::default(),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)?;
    Ok(doc)
}
pub fn photos(inputs: &[PhotoInput], cancel: &AtomicBool) -> Result<Vec<Source>> {
    if inputs.is_empty() || inputs.len() > 200 {
        bail!("Select 1–200 photos for a print job")
    }
    let mut result = vec![];
    let mut bytes = 0;
    for input in inputs {
        canceled(cancel)?;
        let name = input
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut source = prepare_sources(vec![(name, photo_document(input)?)], cancel)?.remove(0);
        source.original_paths.push(input.path.clone());
        bytes += source.svg.len();
        if bytes > 512 * 1024 * 1024 {
            bail!("Print snapshots exceed 512 MiB; select fewer photos")
        }
        result.push(source);
    }
    Ok(result)
}
/// Millisecond timestamps, in authored order; repeats are allowed deliberately.
pub fn timestamps(text: &str) -> Result<Vec<u32>> {
    let times = text
        .split(',')
        .map(|v| {
            v.trim()
                .parse::<u32>()
                .context("Use comma-separated millisecond timestamps, for example 0, 1000, 2500")
        })
        .collect::<Result<Vec<_>>>()?;
    validate_times(&times)?;
    Ok(times)
}
fn validate_times(times: &[u32]) -> Result<()> {
    if times.is_empty() || times.len() > 100 || times.iter().any(|&t| t > 24 * 60 * 60 * 1000) {
        bail!("Select 1–100 frames within 24 hours")
    }
    Ok(())
}
fn frame_name(name: &str, t: u32) -> String {
    format!(
        "{name} · {:02}:{:02}:{:02}.{:03}",
        t / 3_600_000,
        (t / 60_000) % 60,
        (t / 1000) % 60,
        t % 1000
    )
}
pub fn animation(
    name: &str,
    doc: &Document,
    times: &[u32],
    cancel: &AtomicBool,
) -> Result<Vec<Source>> {
    validate_times(times)?;
    if times.iter().any(|&t| t >= doc.design.duration_ms) {
        bail!(
            "Animation frame is outside the page duration ({} ms)",
            doc.design.duration_ms
        )
    }
    let mut result = vec![];
    let mut bytes = 0;
    for &time in times {
        canceled(cancel)?;
        let frame =
            emulsion_core::design_metadata::at_time(doc, time).map_err(anyhow::Error::msg)?;
        let source = prepare_sources(vec![(frame_name(name, time), frame)], cancel)?.remove(0);
        bytes += source.svg.len();
        if bytes > 512 * 1024 * 1024 {
            bail!("Storyboard exceeds 512 MiB; choose fewer frames")
        }
        result.push(source);
    }
    Ok(result)
}
/// Local video only. FFmpeg is discovered from PATH; no shell or network input.
pub fn video(path: &Path, times: &[u32], cancel: &AtomicBool) -> Result<Vec<Source>> {
    validate_times(times)?;
    let path = path
        .canonicalize()
        .context("Choose an existing local video file")?;
    if !path.is_file() {
        bail!("Choose a local video file")
    }
    let dir = tempfile::tempdir()?;
    let mut result = vec![];
    let mut bytes = 0;
    for &time in times {
        canceled(cancel)?;
        let output = dir.path().join("frame.png");
        if output.exists() {
            std::fs::remove_file(&output)?;
        }
        let mut command = Process::new("ffmpeg");
        command.args(["-nostdin","-v","error","-y","-protocol_whitelist","file,pipe","-ss"])
            .arg(format!("{:.3}",f64::from(time)/1000.)).arg("-i").arg(&path)
            .args(["-map","0:v:0","-frames:v","1","-vf","scale=w='min(4096,iw)':h='min(4096,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2","-threads","1"])
            .arg(&output).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .context("Video storyboard requires FFmpeg on PATH (Windows, macOS or Linux)")?;
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() || !output.is_file() {
                    bail!(
                        "Cannot decode video at {time} ms; check the timestamp and installed codec"
                    )
                }
                break;
            }
            if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(30) {
                let _ = child.kill();
                let _ = child.wait();
                bail!("Video frame extraction canceled or timed out")
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let raster = crate::import::decode(&output)?.raster;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let mut source = prepare_sources(
            vec![(frame_name(&name, time), raster_document(raster)?)],
            cancel,
        )?
        .remove(0);
        source.original_paths.push(path.clone());
        bytes += source.svg.len();
        if bytes > 512 * 1024 * 1024 {
            bail!("Storyboard exceeds 512 MiB; choose fewer frames")
        }
        result.push(source);
    }
    Ok(result)
}

pub fn frames(name: &str, doc: &Document, ids: &[u64], cancel: &AtomicBool) -> Result<Vec<Source>> {
    if ids.is_empty() || ids.len() > 200 {
        bail!("Select 1–200 Design frames")
    }
    let mut result = vec![];
    let mut bytes = 0;
    for &id in ids {
        canceled(cancel)?;
        if !doc.design.frames.contains_key(&id) {
            bail!("Node {id} is not a Design layout frame")
        }
        let node = doc.node(id).context("Missing Design frame")?;
        let (frame, _) = crate::selection_export::prepare(
            doc,
            &[id],
            &crate::selection_export::Options {
                bounds: crate::selection_export::Bounds::Frame,
                ..Default::default()
            },
        )?;
        let source =
            prepare_sources(vec![(format!("{name} · {}", node.name), frame)], cancel)?.remove(0);
        bytes += source.svg.len();
        if bytes > 512 * 1024 * 1024 {
            bail!("Frames exceed 512 MiB; choose fewer frames")
        }
        result.push(source);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_photo_drafts_are_printed_without_saving_and_preserve_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        image::RgbaImage::from_pixel(20, 10, image::Rgba([60, 60, 60, 255]))
            .save(&path)
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        let params = emulsion_core::raw::DevelopParams {
            exposure: 1.,
            ..Default::default()
        };
        let input = PhotoInput {
            path: path.clone(),
            params: Some(params),
            expected_digest: Some(crate::raw::source_digest(&path).unwrap()),
        };
        let source = photos(&[input.clone()], &AtomicBool::new(false))
            .unwrap()
            .remove(0);
        assert_eq!(source.name, "photo.png");
        assert_eq!(source.width, 20);
        assert!(source.original_paths.contains(&path));
        let doc = source.document.unwrap();
        let pixels = emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8();
        assert!(pixels[0] > 60);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let mut changed = input;
        changed.expected_digest = Some("0".repeat(64));
        assert!(photos(&[changed], &AtomicBool::new(false)).is_err());
    }
    #[test]
    fn timestamps_are_bounded_and_animation_order_is_explicit() {
        assert_eq!(timestamps("2000, 0, 1000").unwrap(), vec![2000, 0, 1000]);
        for value in ["", "-1", "0,", "90000000"] {
            assert!(timestamps(value).is_err());
        }
        let doc = Document::new(20, 20);
        let duration = doc.design.duration_ms;
        let frames = animation("Page", &doc, &[1, 0], &AtomicBool::new(false)).unwrap();
        assert!(frames[0].name.ends_with("00:00:00.001"));
        assert!(frames[1].name.ends_with("00:00:00.000"));
        assert!(animation("Page", &doc, &[duration], &AtomicBool::new(false)).is_err());
        assert!(animation("Page", &doc, &[0], &AtomicBool::new(true)).is_err());
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    #[test]
    fn responsive_frames_print_separately_at_their_physical_size() {
        use emulsion_core::{
            Editor,
            design_layout::{self, Child, Frame},
        };
        let mut e = Editor::new(Document::new(400, 300), None);
        e.doc.resolution = 100.;
        let child = e
            .execute(Command::AddNode {
                node: Box::new(Node::path(
                    0,
                    "Red",
                    Arc::new(emulsion_raster::vector_geometry::rectangle(
                        20., 20., 140., 50.,
                    )),
                    emulsion_raster::vector::PathStyle {
                        fill: Some([255, 0, 0, 255]),
                        stroke: None,
                        ..Default::default()
                    },
                    400,
                    300,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let group = e
            .execute(Command::Group {
                ids: vec![child],
                name: "Card frame".into(),
            })
            .unwrap()
            .unwrap();
        let mut frame = Frame {
            padding: [0.; 4],
            ..Default::default()
        };
        frame.children.insert(
            child,
            Child {
                absolute: true,
                ..Default::default()
            },
        );
        e.begin("Frame");
        design_layout::enable(&mut e, group, frame, (80., 80.)).unwrap();
        e.end();
        let sources = frames("Design", &e.doc, &[group], &AtomicBool::new(false)).unwrap();
        assert_eq!((sources[0].width, sources[0].height), (80, 80));
        assert!(sources[0].name.contains("Card frame"));
        assert!((sources[0].physical_size().unwrap().0 - 20.32).abs() < 0.001);
        assert_eq!((e.doc.width, e.doc.height), (400, 300));
        assert!(frames("Design", &e.doc, &[child], &AtomicBool::new(false)).is_err());
    }
}

#[cfg(test)]
mod video_tests {
    use super::*;
    #[test]
    fn local_video_frames_are_timestamped_bounded_and_do_not_upscale() {
        if Process::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            eprintln!(
                "FFmpeg unavailable; local video integration requires the optional system decoder"
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frames.mkv");
        assert!(
            Process::new("ffmpeg")
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=duration=2:size=64x48:rate=2",
                    "-c:v",
                    "ffv1",
                    "-threads",
                    "1"
                ])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let frames = video(&path, &[1000, 0], &AtomicBool::new(false)).unwrap();
        assert_eq!((frames[0].width, frames[0].height), (64, 48));
        assert!(frames[0].name.ends_with("00:00:01.000"));
        assert!(frames[1].name.ends_with("00:00:00.000"));
        let pixels = |i: usize| {
            emulsion_raster::composite::flatten(
                &frames[i].document.as_ref().unwrap().composite_tree(),
                0,
            )
            .to_srgba8()
        };
        assert_ne!(pixels(0), pixels(1));
        assert!(video(&path, &[3000], &AtomicBool::new(false)).is_err());
        assert!(video(&path, &[0], &AtomicBool::new(true)).is_err());
    }
}
