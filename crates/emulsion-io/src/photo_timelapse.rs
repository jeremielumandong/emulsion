//! Library photos as a timelapse: each photo developed with its own Library
//! settings, framed into a fixed-size picture (cropped to fill, fitted with
//! black bars, or turned, scaled and moved so the eyes of the most prominent
//! face land in the same place every frame), held for a number of frames and
//! optionally cross-faded into the next. Movies go through the shared FFmpeg
//! encoder ([`crate::video_export`]); GIFs through the shared frame export
//! ([`crate::frame_export`]). Photos are developed one at a time as the
//! movie is written, so long sequences never sit in memory.
use crate::video_export::{self, Codec, Encode};
use anyhow::{Context, Result, bail};
use emulsion_core::timeline::FrameRate;
use glam::{DAffine2, DVec2, dvec2};
use image::{Delay, RgbaImage};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Most photos one timelapse holds.
pub const MAX_PHOTOS: usize = 10_000;
/// Most frames one GIF holds, as for storyboard and Design motion GIFs.
pub const MAX_GIF_FRAMES: u64 = 6000;
/// Widest GIF.
pub const MAX_GIF_WIDTH: u32 = 1920;
/// Where the midpoint between the eyes lands, as a fraction of the frame.
const EYE_CENTRE: [f64; 2] = [0.5, 0.42];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// H.264 in MP4.
    #[default]
    Mp4,
    /// ProRes 422 in MOV.
    Mov,
    /// Looping animated GIF.
    Gif,
}

impl Format {
    pub const ALL: [Self; 3] = [Self::Mp4, Self::Mov, Self::Gif];
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            Self::Gif => "gif",
        }
    }
}

/// The shape of the frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aspect {
    /// The first photo's own shape.
    #[default]
    FirstPhoto,
    Landscape,
    Portrait,
    Square,
}

impl Aspect {
    pub const ALL: [Self; 4] = [
        Self::FirstPhoto,
        Self::Landscape,
        Self::Portrait,
        Self::Square,
    ];
    /// Height over width, or `None` to follow the first photo.
    fn ratio(self) -> Option<f64> {
        match self {
            Self::FirstPhoto => None,
            Self::Landscape => Some(9. / 16.),
            Self::Portrait => Some(16. / 9.),
            Self::Square => Some(1.),
        }
    }
}

/// How each photo sits in the frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Framing {
    /// Scale to cover the frame, cropping the overflow.
    #[default]
    Fill,
    /// Scale to fit inside the frame, with black bars.
    Fit,
    /// Turn, scale and move each photo so the eyes of its most prominent face
    /// line up; photos without a face are filled. Needs the face detector.
    AlignFaces,
}

/// The order photos play in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    /// Capture time from EXIF, then file name for photos without one.
    #[default]
    CaptureTime,
    /// File name.
    FileName,
    /// The order given.
    AsGiven,
}

/// Timelapse settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub format: Format,
    /// Output width in pixels; the height follows `aspect`.
    pub width: u32,
    pub aspect: Aspect,
    /// Frames per second, 1–60.
    pub fps: u32,
    /// Frames each photo is held for, 1–600.
    pub hold: u32,
    /// Frames of cross-fade between photos, 0–120.
    pub crossfade: u32,
    pub framing: Framing,
    /// With [`Framing::AlignFaces`], the distance between the eyes as a
    /// fraction of the frame width (0.05–0.6).
    pub eye_spacing: f64,
    pub order: Order,
    /// 1–100, for movies.
    pub quality: u8,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            format: Format::Mp4,
            width: 1920,
            aspect: Aspect::FirstPhoto,
            fps: 24,
            hold: 3,
            crossfade: 0,
            framing: Framing::Fill,
            eye_spacing: 0.2,
            order: Order::CaptureTime,
            quality: 80,
        }
    }
}

impl Options {
    pub fn validate(&self) -> Result<()> {
        let max_width = if self.format == Format::Gif {
            MAX_GIF_WIDTH
        } else {
            video_export::MAX_SIDE
        };
        if !(16..=max_width).contains(&self.width) {
            bail!("The width is 16–{max_width} pixels")
        }
        if !(1..=60).contains(&self.fps) {
            bail!("The frame rate is 1–60 frames per second")
        }
        if !(1..=600).contains(&self.hold) {
            bail!("Each photo is held for 1–600 frames")
        }
        if self.crossfade > 120 {
            bail!("Cross-fades are 0–120 frames")
        }
        if !(0.05..=0.6).contains(&self.eye_spacing) {
            bail!("Eye spacing is 5–60% of the frame width")
        }
        if !(1..=100).contains(&self.quality) {
            bail!("Quality is 1–100")
        }
        Ok(())
    }

    /// Frames in a timelapse of `photos` photos.
    pub fn frames(&self, photos: usize) -> u64 {
        let photos = photos as u64;
        photos * u64::from(self.hold) + photos.saturating_sub(1) * u64::from(self.crossfade)
    }

    /// Length in seconds of a timelapse of `photos` photos.
    pub fn seconds(&self, photos: usize) -> f64 {
        self.frames(photos) as f64 / f64::from(self.fps.max(1))
    }
}

/// What was written.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub frames: u64,
    pub seconds: f64,
    pub width: u32,
    pub height: u32,
    /// Photos framed with [`Framing::Fill`] because no face was found.
    pub unaligned: Vec<PathBuf>,
}

/// Put `paths` in the order `order` asks for.
pub fn ordered(paths: &[PathBuf], order: Order) -> Vec<PathBuf> {
    let mut paths = paths.to_vec();
    match order {
        Order::AsGiven => {}
        Order::FileName => paths.sort_by_key(|p| file_name(p)),
        Order::CaptureTime => {
            let mut keyed: Vec<_> = paths
                .into_par_iter()
                .map(|p| (taken(&p), file_name(&p), p))
                .collect();
            // Photos with a capture time first, then the rest by name.
            keyed.sort_by(|a, b| (a.0.is_none(), &a.0, &a.1).cmp(&(b.0.is_none(), &b.0, &b.1)));
            paths = keyed.into_iter().map(|(_, _, p)| p).collect();
        }
    }
    paths
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// EXIF `DateTimeOriginal` ("YYYY:MM:DD HH:MM:SS" sorts as text).
fn taken(path: &Path) -> Option<String> {
    let source = if crate::photo_develop::is_virtual(path) {
        crate::photo_develop::original_path(path).ok()?
    } else {
        path.to_path_buf()
    };
    crate::exif::read(&source)
        .map(|info| info.taken.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Write `paths` (in `options.order`) as a timelapse at `path`.
/// `progress(done, total)` counts frames. Setting `cancel` stops the export
/// and leaves any existing file untouched.
pub fn write(
    paths: &[PathBuf],
    options: &Options,
    path: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<Report> {
    options.validate()?;
    if paths.len() < 2 {
        bail!("Choose at least two photos for a timelapse")
    }
    if paths.len() > MAX_PHOTOS {
        bail!("Timelapses are limited to {MAX_PHOTOS} photos")
    }
    let paths = ordered(paths, options.order);
    let mut frames = Frames::new(paths, options.clone(), develop, cancel)?;
    write_frames(&mut frames, options, path, progress, cancel)
}

/// Encode what `frames` renders; split from [`write`] so tests can feed
/// pictures without developing files.
fn write_frames<'a>(
    frames: &mut Frames<'a>,
    options: &Options,
    path: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<Report> {
    let (width, height) = frames.size;
    let total = options.frames(frames.paths.len());
    match options.format {
        Format::Mp4 | Format::Mov => {
            let encode = Encode {
                codec: if options.format == Format::Mov {
                    Codec::ProRes
                } else {
                    Codec::H264
                },
                width,
                height,
                rate: FrameRate::whole(options.fps),
                quality: options.quality,
                audio: None,
            };
            video_export::encode(path, &encode, total, |n| frames.frame(n), progress, cancel)?;
        }
        Format::Gif => {
            // One GIF frame per held photo, with a long delay, keeps files small.
            let segments = frames.segments();
            if segments.len() as u64 > MAX_GIF_FRAMES {
                bail!(
                    "GIFs are limited to {MAX_GIF_FRAMES} frames; use fewer photos, a shorter cross-fade or a movie"
                )
            }
            let mut done = 0;
            let fps = options.fps;
            let images = segments.into_iter().map(|(first, count)| {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Export canceled".to_string());
                }
                let pixels = frames.frame(first).map_err(|e| e.to_string())?;
                done += count;
                progress(done, total);
                let image = RgbaImage::from_raw(width, height, pixels)
                    .ok_or_else(|| "Invalid rendered frame".to_string())?;
                Ok((image, Delay::from_numer_denom_ms(1000 * count as u32, fps)))
            });
            crate::frame_export::encode_gif_frames(path, images, 10).map_err(anyhow::Error::msg)?;
        }
    }
    Ok(Report {
        frames: total,
        seconds: options.seconds(frames.paths.len()),
        width,
        height,
        unaligned: frames.unaligned.clone(),
    })
}

/// A photo developed with its Library settings, with the eyes of its most
/// prominent face when asked for.
struct Developed {
    image: RgbaImage,
    eyes: Option<[DVec2; 2]>,
}

type Develop = fn(&Path, bool, &AtomicBool) -> Result<Developed>;

fn develop(path: &Path, faces: bool, cancel: &AtomicBool) -> Result<Developed> {
    let source = crate::photo_develop::PhotoSource::load(path)
        .with_context(|| format!("Cannot open {}", path.display()))?;
    let params = crate::raw_settings::adjacent_settings(path, &source.source_sha256)?;
    let raster = source
        .develop_with_cancel(&params, cancel)
        .with_context(|| format!("Cannot develop {}", path.display()))?;
    let eyes = if faces {
        let job = emulsion_ai::jobs::Job::new();
        emulsion_ai::face::detect(&raster, &job)
            .map_err(|e| anyhow::anyhow!("Cannot find faces in {}: {e}", path.display()))?
            .first()
            .map(|face| {
                let [a, b] = [face.landmarks[0], face.landmarks[1]];
                [
                    dvec2(a[0].into(), a[1].into()),
                    dvec2(b[0].into(), b[1].into()),
                ]
            })
    } else {
        None
    };
    let image = RgbaImage::from_raw(raster.width(), raster.height(), raster.to_srgba8())
        .context("Invalid developed photo")?;
    Ok(Developed { image, eyes })
}

/// Renders timelapse frames in order, keeping only the two photos a
/// cross-fade needs.
struct Frames<'a> {
    paths: Vec<PathBuf>,
    options: Options,
    size: (u32, u32),
    develop: Develop,
    cancel: &'a AtomicBool,
    /// Framed photos as (index, RGBA), most recent last.
    cache: Vec<(usize, Vec<u8>)>,
    /// The first photo, developed while sizing the frame.
    first: Option<Developed>,
    unaligned: Vec<PathBuf>,
}

impl<'a> Frames<'a> {
    fn new(
        paths: Vec<PathBuf>,
        options: Options,
        develop: Develop,
        cancel: &'a AtomicBool,
    ) -> Result<Self> {
        let faces = options.framing == Framing::AlignFaces;
        if faces && emulsion_ai::face::detector_available().is_none() {
            bail!("Aligning faces needs the face detector; install it in Settings → Models")
        }
        let first = develop(&paths[0], faces, cancel)?;
        let even = options.format != Format::Gif;
        let size = frame_size(
            options.width,
            options
                .aspect
                .ratio()
                .unwrap_or(f64::from(first.image.height()) / f64::from(first.image.width())),
            even,
        );
        Ok(Self {
            paths,
            options,
            size,
            develop,
            cancel,
            cache: Vec::new(),
            first: Some(first),
            unaligned: Vec::new(),
        })
    }

    /// Runs of identical frames as (first frame, count): each held photo,
    /// then each cross-fade frame on its own.
    fn segments(&self) -> Vec<(u64, u64)> {
        let (hold, fade) = (
            u64::from(self.options.hold),
            u64::from(self.options.crossfade),
        );
        let mut out = Vec::new();
        let mut at = 0;
        for i in 0..self.paths.len() {
            out.push((at, hold));
            at += hold;
            if i + 1 < self.paths.len() {
                for _ in 0..fade {
                    out.push((at, 1));
                    at += 1;
                }
            }
        }
        out
    }

    /// Frame `n` as straight RGBA8.
    fn frame(&mut self, n: u64) -> Result<Vec<u8>> {
        let (hold, fade) = (
            u64::from(self.options.hold),
            u64::from(self.options.crossfade),
        );
        let span = hold + fade;
        let photo = ((n / span) as usize).min(self.paths.len() - 1);
        let offset = n - photo as u64 * span;
        if offset < hold || photo + 1 == self.paths.len() {
            return Ok(self.photo(photo)?.to_vec());
        }
        let t = (offset - hold + 1) as f64 / (fade + 1) as f64;
        let from = self.photo(photo)?.to_vec();
        let to = self.photo(photo + 1)?;
        Ok(blend(&from, to, t))
    }

    /// Photo `i` framed at the output size.
    fn photo(&mut self, i: usize) -> Result<&[u8]> {
        if let Some(at) = self.cache.iter().position(|(j, _)| *j == i) {
            return Ok(&self.cache[at].1);
        }
        if self.cancel.load(Ordering::Relaxed) {
            bail!("Export canceled")
        }
        let faces = self.options.framing == Framing::AlignFaces;
        let developed = match (i, self.first.take()) {
            (0, Some(first)) => first,
            (_, first) => {
                self.first = first;
                (self.develop)(&self.paths[i], faces, self.cancel)?
            }
        };
        let source = (developed.image.width(), developed.image.height());
        let transform = match (self.options.framing, developed.eyes) {
            (Framing::Fit, _) => fit(source, self.size, false),
            (Framing::AlignFaces, Some(eyes)) => {
                align_eyes(eyes, self.size, self.options.eye_spacing)
            }
            (Framing::AlignFaces, None) => {
                self.unaligned.push(self.paths[i].clone());
                fit(source, self.size, true)
            }
            (Framing::Fill, _) => fit(source, self.size, true),
        };
        let pixels = warp(&developed.image, transform, self.size);
        if self.cache.len() == 2 {
            self.cache.remove(0);
        }
        self.cache.push((i, pixels));
        Ok(&self.cache.last().unwrap().1)
    }
}

/// `width` by `width × ratio`, in even numbers for movies.
fn frame_size(width: u32, ratio: f64, even: bool) -> (u32, u32) {
    let height = (f64::from(width) * ratio).round().max(2.) as u32;
    if even {
        (width & !1, (height & !1).min(video_export::MAX_SIDE))
    } else {
        (width, height.min(video_export::MAX_SIDE))
    }
}

/// Source → frame: centred and scaled to cover (`fill`) or fit inside.
fn fit(source: (u32, u32), size: (u32, u32), fill: bool) -> DAffine2 {
    let (sw, sh) = (f64::from(source.0), f64::from(source.1));
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let scale = if fill {
        (w / sw).max(h / sh)
    } else {
        (w / sw).min(h / sh)
    };
    DAffine2::from_scale_angle_translation(
        DVec2::splat(scale),
        0.,
        dvec2((w - sw * scale) / 2., (h - sh * scale) / 2.),
    )
}

/// Source → frame: the similarity that puts the left and right eye (in
/// image order) level, `spacing × width` apart, centred on [`EYE_CENTRE`].
fn align_eyes(eyes: [DVec2; 2], size: (u32, u32), spacing: f64) -> DAffine2 {
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let centre = dvec2(EYE_CENTRE[0] * w, EYE_CENTRE[1] * h);
    let half = dvec2(spacing * w / 2., 0.);
    let [a, b] = eyes;
    let (to_a, to_b) = (centre - half, centre + half);
    let from = b - a;
    let to = to_b - to_a;
    if from.length() < 1e-6 {
        return DAffine2::from_translation(centre - a);
    }
    let scale = to.length() / from.length();
    let angle = to.y.atan2(to.x) - from.y.atan2(from.x);
    let turn = DAffine2::from_scale_angle_translation(DVec2::splat(scale), angle, DVec2::ZERO);
    DAffine2::from_translation(to_a - turn.transform_point2(a)) * turn
}

/// Draw `image` through `transform` onto a black frame of `size`.
/// Shrinking photos are first resized close to their final scale so the
/// bilinear lookup does not alias.
fn warp(image: &RgbaImage, transform: DAffine2, size: (u32, u32)) -> Vec<u8> {
    let scale = transform.matrix2.determinant().abs().sqrt();
    let (image, transform) = if scale < 0.75 {
        let (w, h) = (
            ((f64::from(image.width()) * scale).round() as u32).max(1),
            ((f64::from(image.height()) * scale).round() as u32).max(1),
        );
        let small = image::imageops::resize(image, w, h, image::imageops::FilterType::Triangle);
        let back = DAffine2::from_scale(dvec2(
            f64::from(image.width()) / f64::from(w),
            f64::from(image.height()) / f64::from(h),
        ));
        (std::borrow::Cow::Owned(small), transform * back)
    } else {
        (std::borrow::Cow::Borrowed(image), transform)
    };
    let inverse = transform.inverse();
    let (w, h) = (image.width() as i64, image.height() as i64);
    let raw = image.as_raw();
    let texel = |x: i64, y: i64| -> [f64; 4] {
        if x < 0 || y < 0 || x >= w || y >= h {
            return [0., 0., 0., 255.];
        }
        let i = ((y * w + x) * 4) as usize;
        let p = &raw[i..i + 4];
        // Composite over black.
        let a = f64::from(p[3]) / 255.;
        [
            f64::from(p[0]) * a,
            f64::from(p[1]) * a,
            f64::from(p[2]) * a,
            255.,
        ]
    };
    let mut out = vec![0u8; size.0 as usize * size.1 as usize * 4];
    out.par_chunks_mut(size.0 as usize * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..size.0 as usize {
                let p = inverse.transform_point2(dvec2(x as f64 + 0.5, y as f64 + 0.5))
                    - DVec2::splat(0.5);
                let (x0, y0) = (p.x.floor(), p.y.floor());
                let (fx, fy) = (p.x - x0, p.y - y0);
                let (x0, y0) = (x0 as i64, y0 as i64);
                let (a, b, c, d) = (
                    texel(x0, y0),
                    texel(x0 + 1, y0),
                    texel(x0, y0 + 1),
                    texel(x0 + 1, y0 + 1),
                );
                for ch in 0..4 {
                    let top = a[ch] + (b[ch] - a[ch]) * fx;
                    let bottom = c[ch] + (d[ch] - c[ch]) * fx;
                    row[x * 4 + ch] = (top + (bottom - top) * fy).round().clamp(0., 255.) as u8;
                }
            }
        });
    out
}

/// `from` faded `t` (0–1) of the way to `to`.
fn blend(from: &[u8], to: &[u8], t: f64) -> Vec<u8> {
    from.iter()
        .zip(to)
        .map(|(&a, &b)| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{AnimationDecoder, Rgba};

    /// Test photos are named `<r>-<g>-<b>-<w>x<h>[-eyes]`: a flat colour of
    /// that size, with eyes at a quarter and three quarters across the
    /// middle when asked for.
    fn fake(path: &Path, _faces: bool, _: &AtomicBool) -> Result<Developed> {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let parts: Vec<_> = name.split('-').collect();
        let rgb: Vec<u8> = parts[..3].iter().map(|s| s.parse().unwrap()).collect();
        let (w, h) = parts[3].split_once('x').unwrap();
        let (w, h): (u32, u32) = (w.parse().unwrap(), h.parse().unwrap());
        let eyes = (parts.get(4) == Some(&"eyes")).then(|| {
            let y = f64::from(h) / 2.;
            [dvec2(f64::from(w) / 4., y), dvec2(f64::from(w) * 0.75, y)]
        });
        Ok(Developed {
            image: RgbaImage::from_pixel(w, h, Rgba([rgb[0], rgb[1], rgb[2], 255])),
            eyes,
        })
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    fn frames<'a>(names: &[&str], options: &Options, cancel: &'a AtomicBool) -> Frames<'a> {
        Frames::new(paths(names), options.clone(), fake, cancel).unwrap()
    }

    fn pixel(frame: &[u8], size: (u32, u32), x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size.0 + x) * 4) as usize;
        frame[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn counts_held_and_faded_frames() {
        let options = Options {
            hold: 4,
            crossfade: 2,
            ..Options::default()
        };
        assert_eq!(options.frames(3), 3 * 4 + 2 * 2);
        assert_eq!(options.frames(1), 4);
        assert!((options.seconds(3) - 16. / 24.).abs() < 1e-9);
    }

    #[test]
    fn rejects_out_of_range_options() {
        for bad in [
            Options {
                fps: 0,
                ..Options::default()
            },
            Options {
                hold: 0,
                ..Options::default()
            },
            Options {
                crossfade: 500,
                ..Options::default()
            },
            Options {
                format: Format::Gif,
                width: 3840,
                ..Options::default()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        assert!(Options::default().validate().is_ok());
    }

    #[test]
    fn sizes_from_first_photo_in_even_numbers() {
        let cancel = AtomicBool::new(false);
        let options = Options {
            width: 101,
            ..Options::default()
        };
        let f = frames(&["0-0-0-300x201", "0-0-0-10x10"], &options, &cancel);
        assert_eq!(f.size, (100, 68));
        let square = Options {
            aspect: Aspect::Square,
            width: 64,
            ..Options::default()
        };
        assert_eq!(frames(&["0-0-0-300x201"], &square, &cancel).size, (64, 64));
    }

    #[test]
    fn holds_each_photo_then_cross_fades() {
        let cancel = AtomicBool::new(false);
        let options = Options {
            width: 16,
            aspect: Aspect::Square,
            hold: 2,
            crossfade: 1,
            ..Options::default()
        };
        let mut f = frames(&["200-0-0-8x8", "0-0-200-8x8"], &options, &cancel);
        let size = f.size;
        let colours: Vec<_> = (0..options.frames(2))
            .map(|n| pixel(&f.frame(n).unwrap(), size, 8, 8))
            .collect();
        assert_eq!(
            colours,
            [
                [200, 0, 0, 255],
                [200, 0, 0, 255],
                [100, 0, 100, 255],
                [0, 0, 200, 255],
                [0, 0, 200, 255],
            ]
        );
        assert_eq!(f.segments(), [(0, 2), (2, 1), (3, 2)]);
    }

    #[test]
    fn fit_letterboxes_and_fill_crops() {
        let cancel = AtomicBool::new(false);
        let fit = Options {
            width: 40,
            aspect: Aspect::Square,
            framing: Framing::Fit,
            ..Options::default()
        };
        // A wide photo in a square frame: bars above and below.
        let mut f = frames(&["0-255-0-80x40"], &fit, &cancel);
        let frame = f.frame(0).unwrap();
        assert_eq!(pixel(&frame, f.size, 20, 2), [0, 0, 0, 255]);
        assert_eq!(pixel(&frame, f.size, 20, 20), [0, 255, 0, 255]);
        let fill = Options {
            framing: Framing::Fill,
            ..fit
        };
        let mut f = frames(&["0-255-0-80x40"], &fill, &cancel);
        let frame = f.frame(0).unwrap();
        assert_eq!(pixel(&frame, f.size, 20, 2), [0, 255, 0, 255]);
        assert_eq!(pixel(&frame, f.size, 0, 39), [0, 255, 0, 255]);
    }

    #[test]
    fn puts_eyes_level_at_the_same_place() {
        let size = (200, 100);
        // Tilted eyes, far from the target.
        let eyes = [dvec2(10., 80.), dvec2(50., 40.)];
        let m = align_eyes(eyes, size, 0.2);
        let a = m.transform_point2(eyes[0]);
        let b = m.transform_point2(eyes[1]);
        assert!((a - dvec2(80., 42.)).length() < 1e-9, "{a}");
        assert!((b - dvec2(120., 42.)).length() < 1e-9, "{b}");
    }

    #[test]
    fn photos_without_faces_are_filled_and_reported() {
        let cancel = AtomicBool::new(false);
        let options = Options {
            width: 32,
            aspect: Aspect::Square,
            framing: Framing::AlignFaces,
            ..Options::default()
        };
        // Bypass the detector check: build the frames by hand.
        let mut f = Frames {
            paths: paths(&["9-9-9-64x64-eyes", "9-9-9-64x64"]),
            options,
            size: (32, 32),
            develop: fake,
            cancel: &cancel,
            cache: Vec::new(),
            first: None,
            unaligned: Vec::new(),
        };
        f.photo(0).unwrap();
        f.photo(1).unwrap();
        assert_eq!(f.unaligned, paths(&["9-9-9-64x64"]));
    }

    #[test]
    fn keeps_two_photos_cached() {
        let cancel = AtomicBool::new(false);
        let options = Options {
            width: 8,
            aspect: Aspect::Square,
            ..Options::default()
        };
        let mut f = frames(&["1-1-1-4x4", "2-2-2-4x4", "3-3-3-4x4"], &options, &cancel);
        for i in 0..3 {
            f.photo(i).unwrap();
        }
        assert_eq!(f.cache.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [1, 2]);
    }

    #[test]
    fn orders_by_name_or_as_given() {
        let given = paths(&["b.jpg", "C.jpg", "a.jpg"]);
        assert_eq!(ordered(&given, Order::AsGiven), given);
        assert_eq!(
            ordered(&given, Order::FileName),
            paths(&["a.jpg", "b.jpg", "C.jpg"])
        );
        // No EXIF anywhere: capture time falls back to the name.
        assert_eq!(
            ordered(&given, Order::CaptureTime),
            paths(&["a.jpg", "b.jpg", "C.jpg"])
        );
    }

    #[test]
    fn writes_a_looping_gif_with_held_delays() {
        let cancel = AtomicBool::new(false);
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("lapse.gif");
        let options = Options {
            format: Format::Gif,
            width: 24,
            aspect: Aspect::Square,
            fps: 10,
            hold: 5,
            crossfade: 2,
            ..Options::default()
        };
        let mut f = frames(&["255-0-0-8x8", "0-0-255-8x8"], &options, &cancel);
        let mut seen = 0;
        let report = write_frames(&mut f, &options, &out, &mut |d, _| seen = d, &cancel).unwrap();
        assert_eq!((report.frames, report.width, report.height), (12, 24, 24));
        assert_eq!(seen, 12);
        let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
            std::fs::File::open(&out).unwrap(),
        ))
        .unwrap();
        let delays: Vec<_> = decoder
            .into_frames()
            .map(|f| f.unwrap().delay().numer_denom_ms())
            .map(|(n, d)| n / d)
            .collect();
        assert_eq!(delays, [500, 100, 100, 500]);
    }

    #[test]
    fn writes_an_mp4() {
        if !crate::ffmpeg::available() {
            return;
        }
        let cancel = AtomicBool::new(false);
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("lapse.mp4");
        let options = Options {
            width: 64,
            aspect: Aspect::Landscape,
            fps: 12,
            hold: 2,
            crossfade: 1,
            ..Options::default()
        };
        let mut f = frames(
            &["255-0-0-40x30", "0-255-0-40x30", "0-0-255-40x30"],
            &options,
            &cancel,
        );
        let report = write_frames(&mut f, &options, &out, &mut |_, _| {}, &cancel).unwrap();
        assert_eq!((report.width, report.height), (64, 36));
        let info = video_export::probe(&out).unwrap();
        assert_eq!((info.frames, info.width, info.height), (8, 64, 36));
    }

    #[test]
    fn develops_real_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut list = Vec::new();
        for (i, colour) in [[255u8, 0, 0], [0, 0, 255]].into_iter().enumerate() {
            let path = dir.path().join(format!("shot{i}.png"));
            image::RgbImage::from_pixel(40, 30, image::Rgb(colour))
                .save(&path)
                .unwrap();
            list.push(path);
        }
        let out = dir.path().join("lapse.gif");
        let options = Options {
            format: Format::Gif,
            width: 40,
            hold: 1,
            ..Options::default()
        };
        let report = write(
            &list,
            &options,
            &out,
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!((report.frames, report.width, report.height), (2, 40, 30));
        assert!(report.unaligned.is_empty());
    }
}
