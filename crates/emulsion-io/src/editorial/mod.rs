//! Editorial interchange: the animatic as an edit for editing software, and
//! edits read back for conforming. Three formats share one neutral
//! [`Edit`]: CMX 3600 EDL ([`edl`]), Final Cut Pro 7 XML ([`xmeml`]) and
//! OpenTimelineIO ([`otio`]). Exports write each panel as a still or a
//! short movie (rendered with the animatic renderer) and copy the sounds
//! and reference videos into a media folder beside the edit, named so a
//! conform finds its panel again even when a clip is renamed
//! ([`emulsion_core::storyboard_conform`]).
use crate::storyboard_export::{self as story, movie};
use crate::video_export::{self, Codec, Encode};
use anyhow::{Context, Result, bail};
use emulsion_core::project::{PageId, Project};
use emulsion_core::storyboard_animatic::RenderArea;
use emulsion_core::storyboard_conform::{panel_media_stem, safe_stem, sound_media_stem};
use emulsion_core::timeline::{Edit, EditClip, EditMarker, EditTransition, FrameRate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub mod edl;
pub mod otio;
#[cfg(test)]
mod tests;
pub mod xmeml;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// CMX 3600 edit decision list.
    #[default]
    Edl,
    /// Final Cut Pro 7 XML (xmeml), also read by Premiere Pro and Resolve.
    Xmeml,
    /// OpenTimelineIO JSON.
    Otio,
}

impl Format {
    pub const ALL: [Self; 3] = [Self::Edl, Self::Xmeml, Self::Otio];
    pub fn extension(self) -> &'static str {
        match self {
            Self::Edl => "edl",
            Self::Xmeml => "xml",
            Self::Otio => "otio",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Edl => "Edit Decision List (CMX 3600 .edl)",
            Self::Xmeml => "Final Cut Pro 7 XML (.xml)",
            Self::Otio => "OpenTimelineIO (.otio)",
        }
    }
    /// The format a file name says.
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
        Self::ALL.into_iter().find(|f| f.extension() == extension)
    }
}

/// What each panel's media is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    /// One PNG per panel, held for its duration.
    #[default]
    Still,
    /// One ProRes 422 movie per panel, with its camera and layer motion,
    /// plus frames held after its end for the next transition. Needs
    /// FFmpeg.
    Movie,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExportOptions {
    pub format: Format,
    pub media: MediaKind,
    /// Media width in pixels; 0 uses the panels' own width (at most 3840).
    pub width: u32,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: Format::Edl,
            media: MediaKind::Still,
            width: 1920,
        }
    }
}

/// What an export wrote.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExportReport {
    pub path: PathBuf,
    pub media_folder: PathBuf,
    /// Panel clips in the edit.
    pub clips: usize,
    pub sound_clips: usize,
    pub files: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

/// The media folder beside an edit file: `<name>_media`.
pub fn media_folder(path: &Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{stem}_media"))
}

/// Where each panel's, sound's and video's media is.
#[derive(Clone, Debug, Default)]
pub struct Media {
    pub panels: BTreeMap<PageId, PathBuf>,
    pub sounds: BTreeMap<u64, PathBuf>,
    pub videos: BTreeMap<u64, PathBuf>,
}

/// The animatic as an edit: panels end to end on V1 from 01:00:00:00 with
/// their transitions, the timeline's sound clips on A1… (gain is the clip's
/// plus its track's), reference video on V2…, and every track's markers.
pub fn to_edit(project: &Project, name: &str, media: &Media) -> Result<Edit> {
    let board = story::board(project)?;
    let rate = board.settings.frame_rate;
    let mut edit = Edit::new(name, rate);
    edit.start = Edit::one_hour(rate);
    let path = |p: Option<&PathBuf>| p.map(|p| p.display().to_string()).unwrap_or_default();
    for entry in story::entries(project)?
        .into_iter()
        .filter(|e| e.length() > 0)
    {
        let frames = entry.length();
        let transition = &entry.panel.transition;
        edit.video.push(EditClip {
            name: entry.name.clone(),
            media: path(media.panels.get(&entry.page)),
            track: 0,
            source_in: 0,
            source_out: frames,
            record_in: entry.start,
            record_out: entry.start + frames,
            transition: (!edit.video.is_empty() && !transition.is_cut()).then_some(
                EditTransition {
                    kind: transition.kind,
                    frames: transition.frames,
                },
            ),
            gain_db: None,
        });
    }
    let timeline = &board.timeline;
    for (t, track) in timeline.tracks.iter().enumerate() {
        for clip in &track.clips {
            let source_in = rate.seconds_to_frames(clip.offset_ms as f64 / 1000.);
            edit.audio.push(EditClip {
                name: clip.name.clone(),
                media: path(media.sounds.get(&clip.asset)),
                track: t,
                source_in,
                source_out: source_in + clip.frames,
                record_in: clip.start,
                record_out: clip.end(),
                transition: None,
                gain_db: Some(clip.gain_db + track.volume_db),
            });
        }
        edit.markers
            .extend(track.markers.iter().map(|m| EditMarker {
                frame: m.frame,
                name: m.name.clone(),
            }));
    }
    for (t, track) in timeline.video.iter().enumerate() {
        for clip in &track.clips {
            let source_in = rate.seconds_to_frames(clip.offset_ms as f64 / 1000.);
            edit.video.push(EditClip {
                name: clip.name.clone(),
                media: path(media.videos.get(&clip.asset)),
                track: t + 1,
                source_in,
                source_out: source_in + clip.frames,
                record_in: clip.start,
                record_out: clip.end(),
                transition: None,
                gain_db: None,
            });
        }
    }
    if edit.video.is_empty() {
        bail!("The storyboard has no panels that play")
    }
    edit.sort();
    Ok(edit)
}

/// The edit as text in `format`, with warnings about what the format
/// cannot hold.
pub fn write_string(edit: &Edit, format: Format, size: (u32, u32)) -> (String, Vec<String>) {
    match format {
        Format::Edl => edl::write(edit),
        Format::Xmeml => (xmeml::write(edit, size), Vec::new()),
        Format::Otio => (otio::write(edit), Vec::new()),
    }
}

/// Read an edit from text. EDLs carry no frame rate, so they are read at
/// `rate` (at 29.97 when they say drop frame and `rate` has none).
pub fn parse(text: &str, format: Format, rate: FrameRate) -> Result<Edit> {
    let edit = match format {
        Format::Edl => edl::read(text, rate)?,
        Format::Xmeml => xmeml::read(text)?,
        Format::Otio => otio::read(text)?,
    };
    edit.validate().map_err(anyhow::Error::msg)?;
    if edit.video.is_empty() && edit.audio.is_empty() {
        bail!("The edit has no clips")
    }
    Ok(edit)
}

/// Read an edit file, its format from its extension.
pub fn read(path: &Path, rate: FrameRate) -> Result<Edit> {
    let format = Format::from_path(path)
        .context("Choose an EDL (.edl), Final Cut XML (.xml) or OpenTimelineIO (.otio) file")?;
    let size = std::fs::metadata(path)
        .with_context(|| format!("Cannot read {}", path.display()))?
        .len();
    if size > 32 << 20 {
        bail!("Edit files are limited to 32 MB")
    }
    let bytes = std::fs::read(path).with_context(|| format!("Cannot read {}", path.display()))?;
    // EDLs from older systems are often Latin-1.
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|e| e.into_bytes().iter().map(|&b| char::from(b)).collect());
    parse(text.trim_start_matches('\u{feff}'), format, rate)
}

/// Export the animatic as an edit at `path` with each panel's media, the
/// sounds and reference videos in [`media_folder`]. `progress(done, total)`
/// counts panels.
pub fn export(
    project: &Project,
    name: &str,
    options: &ExportOptions,
    path: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<ExportReport> {
    if options.width != 0 && !(16..=video_export::MAX_SIDE).contains(&options.width) {
        bail!("Media width is 16–{} pixels", video_export::MAX_SIDE)
    }
    let board = story::board(project)?;
    let rate = board.settings.frame_rate;
    let entries: Vec<_> = story::entries(project)?
        .into_iter()
        .filter(|e| e.length() > 0)
        .collect();
    if entries.is_empty() {
        bail!("The storyboard has no panels that play")
    }
    if options.media == MediaKind::Movie && !crate::ffmpeg::available() {
        bail!("Movie media needs FFmpeg; install it or export stills")
    }
    let folder = media_folder(path);
    std::fs::create_dir_all(&folder)
        .with_context(|| format!("Cannot create the folder {}", folder.display()))?;
    let rect = movie::area_rect(project, RenderArea::Camera)?;
    let size = movie::output_size(rect, options.width, options.media == MediaKind::Movie);
    let mut renderer = movie::AnimaticRenderer::new(project, rect, size)?;
    let mut media = Media::default();
    let mut files = Vec::new();
    let mut warnings = Vec::new();
    let total = entries.len() as u64;
    for (i, entry) in entries.iter().enumerate() {
        crate::printing::canceled(cancel)?;
        let stem = panel_media_stem(&entry.name, entry.page);
        let file = match options.media {
            MediaKind::Still => {
                let file = folder.join(format!("{stem}.png"));
                let pixels = renderer.panel(entry.page)?;
                image::RgbaImage::from_raw(size.0, size.1, pixels.as_ref().clone())
                    .context("Invalid rendered panel")?
                    .save(&file)
                    .with_context(|| format!("Cannot write {}", file.display()))?;
                file
            }
            MediaKind::Movie => {
                let file = folder.join(format!("{stem}.mov"));
                // Frames held after the end, for the next panel's
                // transition, which plays over this one's tail.
                let handle = entries
                    .get(i + 1)
                    .map_or(0, |n| u64::from(n.panel.transition.frames));
                let frames = entry.length();
                let encode = Encode {
                    codec: Codec::ProRes,
                    width: size.0,
                    height: size.1,
                    rate,
                    quality: 60,
                    audio: None,
                };
                video_export::encode(
                    &file,
                    &encode,
                    frames + handle,
                    |n| {
                        let local = n.min(frames - 1);
                        Ok(renderer
                            .view(entry.page, local, entry.start + local)?
                            .as_ref()
                            .clone())
                    },
                    &mut |_, _| {},
                    cancel,
                )?;
                file
            }
        };
        media.panels.insert(entry.page, file.clone());
        files.push(file);
        progress(i as u64 + 1, total);
    }
    let timeline = &board.timeline;
    let used_sounds: std::collections::BTreeSet<u64> = timeline
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.asset))
        .collect();
    for id in used_sounds {
        let asset = &timeline.assets[&id];
        let file = folder.join(format!(
            "{}.{}",
            sound_media_stem(&asset.name, id),
            asset.format
        ));
        match copy_media(asset.source.as_deref(), &file) {
            Ok(()) => {
                media.sounds.insert(id, file.clone());
                files.push(file);
            }
            Err(e) => warnings.push(format!("The sound “{}” was not copied: {e}", asset.name)),
        }
    }
    if options.format != Format::Edl {
        let used: std::collections::BTreeSet<u64> = timeline
            .video
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.asset))
            .collect();
        for id in used {
            let asset = &timeline.videos[&id];
            let file = folder.join(format!("{}_v{id}.{}", safe_stem(&asset.name), asset.format));
            match copy_media(asset.source.as_deref(), &file) {
                Ok(()) => {
                    media.videos.insert(id, file.clone());
                    files.push(file);
                }
                Err(e) => warnings.push(format!("The video “{}” was not copied: {e}", asset.name)),
            }
        }
    }
    let edit = to_edit(project, name, &media)?;
    let (text, more) = write_string(&edit, options.format, size);
    warnings.extend(more);
    std::fs::write(path, text).with_context(|| format!("Cannot write {}", path.display()))?;
    Ok(ExportReport {
        path: path.to_path_buf(),
        media_folder: folder,
        clips: entries.len(),
        sound_clips: edit.audio.len(),
        files,
        warnings,
    })
}

fn copy_media(source: Option<&Path>, to: &Path) -> Result<()> {
    let source = source.context("its file is not available")?;
    std::fs::copy(source, to).with_context(|| format!("cannot write {}", to.display()))?;
    Ok(())
}

/// A `file://` URL for an absolute path, with unsafe bytes escaped.
pub(crate) fn file_url(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let path = path.replace('\\', "/");
    let mut out = String::from("file://");
    if !path.starts_with('/') {
        out.push('/');
    }
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~:".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The path a `file://` URL (or a plain path) names.
pub(crate) fn url_path(url: &str) -> String {
    let Some(rest) = url
        .strip_prefix("file://localhost")
        .or_else(|| url.strip_prefix("file://"))
    else {
        return url.to_string();
    };
    let decoded = crate::drawio::percent_decode(rest).unwrap_or_else(|_| rest.to_string());
    // file:///C:/x → C:/x
    match decoded.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => decoded[1..].to_string(),
        _ => decoded,
    }
}

/// The frame rate nearest `fps`: NTSC when it is just below a whole rate.
pub(crate) fn rate_from_fps(fps: f64) -> Result<FrameRate> {
    if !fps.is_finite() || !(1. ..=120.).contains(&fps) {
        bail!("The edit's frame rate {fps} is not supported")
    }
    let whole = fps.round();
    let rate = if (fps - whole).abs() < 0.005 {
        FrameRate::whole(whole as u32)
    } else {
        FrameRate::ntsc((fps * 1.001).round() as u32)
    };
    rate.validate().map_err(anyhow::Error::msg)?;
    Ok(rate)
}
