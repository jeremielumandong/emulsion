//! The storyboard as a movie or animated GIF: every frame of the animatic
//! rendered at the export size (panels composited over white, the render
//! area chosen, transitions blended with the shared renderer, burn-in text
//! drawn), seen through each scene's camera and with layer keyframes
//! applied, with the timeline's sound mixed in. Movies go through the shared
//! FFmpeg encoder ([`crate::video_export`]); GIFs through the shared frame
//! export ([`crate::frame_export`]). [`AnimaticRenderer`] is what players
//! use to draw the same pictures.
use super::board;
use crate::video_export::{self, Codec, Encode};
use anyhow::{Context, Result, bail};
use emulsion_core::{
    project::{PageId, Project},
    storyboard::{CameraState, Frame, Storyboard},
    storyboard_animatic::{BurnIn, RenderArea, draw_burn_in},
    storyboard_motion::camera_view,
    timeline::{VideoPlacement, transition, video},
};
use emulsion_raster::IRect;
use glam::{DAffine2, dvec2};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Most frames one movie holds (over 2 hours at 120 fps).
pub const MAX_MOVIE_FRAMES: u64 = 1_000_000;
/// Most frames one GIF holds, as for Design motion GIFs.
pub const MAX_GIF_FRAMES: u64 = 6000;
/// Largest render area, in panel pixels.
const MAX_AREA_PIXELS: i64 = 64 << 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovieFormat {
    /// H.264 in MP4.
    #[default]
    Mp4,
    /// ProRes 422 in MOV.
    Mov,
    /// One PNG per frame in a folder (with the sound as `soundtrack.wav`).
    PngSequence,
}

impl MovieFormat {
    pub const ALL: [Self; 3] = [Self::Mp4, Self::Mov, Self::PngSequence];
    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "H.264 (MP4)",
            Self::Mov => "ProRes 422 (MOV)",
            Self::PngSequence => "PNG image sequence",
        }
    }
    /// The file extension, or `None` for a folder of images.
    pub fn extension(self) -> Option<&'static str> {
        match self {
            Self::Mp4 => Some("mp4"),
            Self::Mov => Some("mov"),
            Self::PngSequence => None,
        }
    }
}

/// Movie export settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MovieOptions {
    pub format: MovieFormat,
    /// Output width in pixels; 0 keeps the render area's own width (at most
    /// 3840). The height follows the render area's shape.
    pub width: u32,
    /// First animatic frame.
    pub start: u64,
    /// Frame after the last; `None` runs to the end.
    pub end: Option<u64>,
    pub area: RenderArea,
    /// Burn-in text, or `None` for clean pictures.
    pub burn_in: Option<BurnIn>,
    /// 1–100.
    pub quality: u8,
    /// Mix the timeline's sound in.
    pub audio: bool,
    /// Draw the timeline's reference video over the panels, fitted
    /// (overlay, with each clip's opacity) or as an inset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_video: Option<VideoPlacement>,
}

impl Default for MovieOptions {
    fn default() -> Self {
        Self {
            format: MovieFormat::Mp4,
            width: 0,
            start: 0,
            end: None,
            area: RenderArea::Camera,
            burn_in: None,
            quality: 80,
            audio: true,
            reference_video: None,
        }
    }
}

/// Animated GIF settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GifOptions {
    /// Output width in pixels (16–1920).
    pub width: u32,
    /// GIF frames per second (1–50); animatic frames are sampled at it.
    pub fps: u32,
    pub start: u64,
    pub end: Option<u64>,
    pub area: RenderArea,
    pub burn_in: Option<BurnIn>,
}

impl Default for GifOptions {
    fn default() -> Self {
        Self {
            width: 640,
            fps: 12,
            start: 0,
            end: None,
            area: RenderArea::Camera,
            burn_in: None,
        }
    }
}

/// What an export wrote.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    /// Frames written.
    pub frames: u64,
    pub seconds: f64,
    pub width: u32,
    pub height: u32,
    /// Whether the export has sound.
    pub audio: bool,
    pub files: Vec<PathBuf>,
}

fn layout(project: &Project) -> Vec<PageId> {
    project.pages.iter().map(|p| p.meta.id).collect()
}

/// Panel `id` as exports draw it: without review layers.
fn doc(project: &Project, id: PageId) -> Result<std::borrow::Cow<'_, emulsion_core::Document>> {
    Ok(emulsion_core::storyboard_review::printable(
        &project
            .pages
            .iter()
            .find(|p| p.meta.id == id)
            .context("Missing panel page")?
            .doc,
    ))
}

/// The area of each panel an export shows, in panel pixels.
pub fn area_rect(project: &Project, area: RenderArea) -> Result<IRect> {
    let board = board(project)?;
    let artwork = match area {
        RenderArea::AllArtwork => board
            .playing(&layout(project))
            .iter()
            .filter_map(|(id, _)| doc(project, *id).ok())
            .filter_map(|d| emulsion_core::diagram::workspace::content_bounds(&d))
            .map(|r| Frame {
                x: f64::from(r.x),
                y: f64::from(r.y),
                w: f64::from(r.w),
                h: f64::from(r.h),
            })
            .collect(),
        _ => Vec::new(),
    };
    let f = board.render_area(area, artwork);
    let (x, y) = (f.x.floor() as i32, f.y.floor() as i32);
    let rect = IRect::new(
        x,
        y,
        ((f.x + f.w).ceil() as i32 - x).max(1),
        ((f.y + f.h).ceil() as i32 - y).max(1),
    );
    if i64::from(rect.w) * i64::from(rect.h) > MAX_AREA_PIXELS {
        bail!("The artwork spreads too far to render; use the camera or overscan area")
    }
    Ok(rect)
}

/// The export size for `width` (0 = the area's own width, at most 3840),
/// keeping the area's shape; `even` rounds both sides to even numbers, as
/// video codecs need.
pub fn output_size(rect: IRect, width: u32, even: bool) -> (u32, u32) {
    let w = if width == 0 {
        (rect.w as u32).min(3840)
    } else {
        width
    };
    let h = (f64::from(w) * f64::from(rect.h) / f64::from(rect.w))
        .round()
        .max(1.) as u32;
    if even {
        ((w + 1) & !1, (h + 1) & !1)
    } else {
        (w, h.max(1))
    }
}

/// Maps a pixel of an `out`-sized picture of `rect` (panel pixels) to the
/// pixel of a `picture`-sized rendering of the same area that the camera
/// `state` shows there. The camera's frame is the panel's (0–width,
/// 0–height); areas around it move with it. Identity at rest when the
/// sizes match.
pub fn camera_source(
    board: &Storyboard,
    state: CameraState,
    rect: IRect,
    out: (u32, u32),
    picture: (u32, u32),
) -> DAffine2 {
    let origin = dvec2(f64::from(rect.x), f64::from(rect.y));
    let area = dvec2(f64::from(rect.w), f64::from(rect.h));
    let to_area = DAffine2::from_translation(origin)
        * DAffine2::from_scale(area / dvec2(f64::from(out.0), f64::from(out.1)));
    let to_picture = DAffine2::from_scale(dvec2(f64::from(picture.0), f64::from(picture.1)) / area)
        * DAffine2::from_translation(-origin);
    to_picture * board.camera_matrix(state) * to_area
}

/// A rendered panel: which panel, at which frame into it when it is
/// animated, and at what size.
/// With layers in depth, the camera it was seen through as well.
type PictureKey = (PageId, Option<u64>, (u32, u32), Option<[u64; 4]>);

/// Draws animatic frames at a fixed size: the panel pictures (still panels
/// kept for the last few, animated ones made per frame), seen through the
/// scene camera, with transitions and burn-in.
pub struct AnimaticRenderer<'a> {
    project: &'a Project,
    board: &'a Storyboard,
    layout: Vec<PageId>,
    rect: IRect,
    size: (u32, u32),
    panels: Vec<(PictureKey, Arc<Vec<u8>>)>,
    /// Animated panels at their last frames, for a held picture under a
    /// transition.
    animated: Vec<(PictureKey, Arc<Vec<u8>>)>,
    /// Where the reference video shows, if it does.
    reference: Option<VideoPlacement>,
    /// The OpenColorIO output transform, when colour management uses it.
    ocio: Option<crate::color_management::ExportTransform>,
}

/// Panels rendered larger than the export, at most, so a zoomed camera
/// stays sharp.
const MAX_CAMERA_DETAIL: f64 = 4.;

impl<'a> AnimaticRenderer<'a> {
    /// Render `project`'s storyboard showing `rect` of each panel (see
    /// [`area_rect`]) at `size`.
    pub fn new(project: &'a Project, rect: IRect, size: (u32, u32)) -> Result<Self> {
        let board = board(project)?;
        let layout = layout(project);
        board.validate(&layout).map_err(anyhow::Error::msg)?;
        if size.0 == 0 || size.1 == 0 || size.0 > 8192 || size.1 > 8192 {
            bail!("Export pictures are 1–8192 pixels a side")
        }
        Ok(Self {
            project,
            board,
            layout,
            rect,
            size,
            panels: Vec::new(),
            animated: Vec::new(),
            reference: None,
            ocio: crate::color_management::ExportTransform::for_project(Some(project))?,
        })
    }

    /// Draw the timeline's reference video over every frame.
    pub fn with_reference_video(mut self, placement: Option<VideoPlacement>) -> Self {
        self.reference = placement;
        self
    }

    /// The reference video's picture for frame `n` over `out`, placed in
    /// the camera frame (which may be smaller than the render area).
    fn draw_reference(&self, out: &mut [u8], n: u64) -> Result<()> {
        let Some(placement) = self.reference else {
            return Ok(());
        };
        let timeline = &self.board.timeline;
        let Some(at) = timeline.video_at(n, self.board.settings.frame_rate) else {
            return Ok(());
        };
        let asset = &timeline.videos[&at.asset];
        let source = asset
            .source
            .as_deref()
            .with_context(|| format!("The video “{}” has no file", asset.name))?;
        let (w, h) = self.size;
        let k = (
            f64::from(w) / f64::from(self.rect.w),
            f64::from(h) / f64::from(self.rect.h),
        );
        let (fw, fh) = (
            f64::from(self.board.settings.width),
            f64::from(self.board.settings.height),
        );
        let [x, y, rw, rh] = video::placement_rect(
            placement,
            fw * k.0,
            fh * k.1,
            f64::from(asset.width),
            f64::from(asset.height),
        );
        let rect = [
            x - f64::from(self.rect.x) * k.0,
            y - f64::from(self.rect.y) * k.1,
            rw,
            rh,
        ];
        let size = crate::reference_video::decode::fit_size(
            (asset.width, asset.height),
            (rw.ceil() as u32, rh.ceil() as u32),
        );
        let picture =
            crate::reference_video::decode::picture(source, asset.fps, at.index, size, 24)?;
        video::draw_picture(
            out,
            (w, h),
            &picture.rgba,
            (picture.width, picture.height),
            rect,
            at.opacity,
        );
        Ok(())
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The animatic's length in frames.
    pub fn frames(&self) -> u64 {
        self.board.animatic_frames(&self.layout)
    }

    /// Panel `id` as drawn (no camera, no keyframes) as opaque RGBA8 at the
    /// export size.
    pub fn panel(&mut self, id: PageId) -> Result<Arc<Vec<u8>>> {
        self.picture(id, None, self.size, None)
    }

    /// Panel `id`, `local` frames in when it is animated, as opaque RGBA8
    /// of `rect` at `size`; with `camera`, its layers in depth placed for
    /// that camera (parallax).
    fn picture(
        &mut self,
        id: PageId,
        local: Option<u64>,
        size: (u32, u32),
        camera: Option<CameraState>,
    ) -> Result<Arc<Vec<u8>>> {
        let bits = camera.map(|c| [c.x, c.y, c.zoom, c.rotation].map(f64::to_bits));
        let key = (id, local, size, bits);
        let cache = if local.is_some() || camera.is_some() {
            &mut self.animated
        } else {
            &mut self.panels
        };
        // Most recently used last.
        if let Some(i) = cache.iter().position(|(k, _)| *k == key) {
            let hit = cache.remove(i);
            let picture = hit.1.clone();
            cache.push(hit);
            return Ok(picture);
        }
        let source = doc(self.project, id)?;
        let animated = self
            .board
            .shown_panel(
                id,
                &source,
                local.unwrap_or(0) as f64,
                camera.unwrap_or_else(|| self.board.rest_camera()),
            )
            .map_err(anyhow::Error::msg)?;
        let mut doc = crate::export::develop_document(animated.as_ref().unwrap_or(&source))?;
        if self.rect != IRect::new(0, 0, doc.width as i32, doc.height as i32) {
            emulsion_core::geometry::crop(&mut doc, self.rect, 0.);
        }
        // The export rectangle may expose off-page mask geometry that was
        // cheap at the saved grid. Reject unsafe derived work before rendering.
        doc.validate()?;
        let target = size.0.max(size.1);
        let mut level = 0;
        while (doc.width.max(doc.height) >> (level + 1)) >= target && level < 8 {
            level += 1;
        }
        let raster = emulsion_raster::composite::flatten(&doc.composite_tree(), level);
        let image = image::RgbaImage::from_raw(raster.width(), raster.height(), raster.to_srgba8())
            .context("Invalid rendered panel")?;
        let mut image = image::imageops::resize(
            &image,
            size.0,
            size.1,
            image::imageops::FilterType::Triangle,
        );
        for p in image.pixels_mut() {
            let a = u32::from(p.0[3]);
            for c in &mut p.0[..3] {
                *c = ((u32::from(*c) * a + 255 * (255 - a) + 127) / 255) as u8;
            }
            p.0[3] = 255;
        }
        if let Some(ocio) = &self.ocio {
            ocio.apply_rgba8(&mut image);
        }
        let picture = Arc::new(image.into_raw());
        let (cache, keep) = if local.is_some() {
            (&mut self.animated, 2)
        } else {
            (&mut self.panels, 4)
        };
        if cache.len() >= keep {
            cache.remove(0);
        }
        cache.push((key, picture.clone()));
        Ok(picture)
    }

    /// How much larger than the export panels of `panel`'s scene render, so
    /// the camera's closest zoom stays sharp.
    fn detail(&self, panel: PageId) -> f64 {
        let zoom = self
            .board
            .cameras
            .get(&self.board.panels[&panel].scene)
            .map_or(1., |c| c.keys.iter().fold(1., |z, k| k.zoom.max(z)));
        let side = f64::from(self.size.0.max(self.size.1));
        zoom.clamp(1., MAX_CAMERA_DETAIL).min(8192. / side).max(1.)
    }

    /// Panel `id`, `local` frames in, as the camera shows it at animatic
    /// frame `at`.
    pub(crate) fn view(&mut self, id: PageId, local: u64, at: u64) -> Result<Arc<Vec<u8>>> {
        let animated = !self.board.panels[&id].motion.is_empty();
        let local = animated.then_some(local);
        let state = self.board.camera_at(&self.layout, at as f64);
        if state == self.board.rest_camera() {
            return self.picture(id, local, self.size, None);
        }
        let parallax = self.board.has_parallax(id).then_some(state);
        let k = self.detail(id);
        let picture = (
            (f64::from(self.size.0) * k).round().max(1.) as u32,
            (f64::from(self.size.1) * k).round().max(1.) as u32,
        );
        let source = self.picture(id, local, picture, parallax)?;
        let m = camera_source(self.board, state, self.rect, self.size, picture);
        Ok(Arc::new(camera_view(
            &source,
            picture.0,
            picture.1,
            m,
            self.size.0,
            self.size.1,
            [255; 4],
        )))
    }

    /// Animatic frame `n` as opaque RGBA8 at the export size.
    pub fn frame(&mut self, n: u64, burn_in: Option<&BurnIn>) -> Result<Vec<u8>> {
        let at = self
            .board
            .animatic_frame(&self.layout, n)
            .context("That frame is past the end of the animatic")?;
        let to = self.view(at.panel, at.local, n)?;
        let (w, h) = self.size;
        let mut out = match at.blend {
            Some((from, kind, t)) => {
                // The panel before holds its last frame under the transition.
                let last = u64::from(self.board.panels[&from].frames).saturating_sub(1);
                let from = self.view(from, last, (n - at.local).saturating_sub(1))?;
                transition::blend(kind, t, &from, &to, w, h)
            }
            None => to.as_ref().clone(),
        };
        self.draw_reference(&mut out, n)?;
        if let Some(burn) = burn_in {
            let lines = self.board.burn_in_lines(&self.layout, n, burn);
            draw_burn_in(&mut out, w, h, &lines, burn);
        }
        Ok(out)
    }
}

/// The frames `start..end` of a range, checked against the animatic.
fn range(total: u64, start: u64, end: Option<u64>) -> Result<(u64, u64)> {
    if total == 0 {
        bail!("The storyboard has no panels that play")
    }
    let end = end.unwrap_or(total);
    if start >= end || end > total {
        bail!("Choose frames within 0–{total}, with the start before the end")
    }
    Ok((start, end))
}

impl MovieOptions {
    pub fn validate(&self) -> Result<()> {
        if self.width != 0 && !(16..=video_export::MAX_SIDE).contains(&self.width) {
            bail!("Movie width is 16–{} pixels", video_export::MAX_SIDE)
        }
        if !(1..=100).contains(&self.quality) {
            bail!("Quality is 1–100")
        }
        if let Some(b) = &self.burn_in {
            b.validate().map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}

impl GifOptions {
    pub fn validate(&self) -> Result<()> {
        if !(16..=1920).contains(&self.width) {
            bail!("GIF width is 16–1920 pixels")
        }
        if !(1..=50).contains(&self.fps) {
            bail!("GIF frame rate is 1–50 fps")
        }
        if let Some(b) = &self.burn_in {
            b.validate().map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}

/// Export the animatic as a movie at `path` (a file, or a folder for an
/// image sequence). Checks everything before writing; `progress(done,
/// total)` follows the frames; `cancel` stops it, leaving any existing
/// movie untouched.
pub fn write_movie(
    project: &Project,
    options: &MovieOptions,
    path: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<Report> {
    options.validate()?;
    let board = board(project)?;
    let rate = board.settings.frame_rate;
    let rect = area_rect(project, options.area)?;
    let size = output_size(
        rect,
        options.width,
        options.format != MovieFormat::PngSequence,
    );
    let mut renderer =
        AnimaticRenderer::new(project, rect, size)?.with_reference_video(options.reference_video);
    let (start, end) = range(renderer.frames(), options.start, options.end)?;
    let frames = end - start;
    if frames > MAX_MOVIE_FRAMES {
        bail!("Movies are limited to {MAX_MOVIE_FRAMES} frames; export a shorter range")
    }
    let sound = options.audio && crate::audio::mix::has_sound(&board.timeline, start, end);
    if sound {
        board.timeline.validate().map_err(anyhow::Error::msg)?;
    }
    let temp = tempfile::tempdir()?;
    let wav = temp.path().join("soundtrack.wav");
    if sound {
        crate::audio::mix::write_wav(&board.timeline, rate, start, end, &wav, cancel)?;
    }
    let burn = options.burn_in.as_ref();
    let mut files = Vec::new();
    match options.format.extension() {
        Some(_) => {
            let encode = Encode {
                codec: if options.format == MovieFormat::Mov {
                    Codec::ProRes
                } else {
                    Codec::H264
                },
                width: size.0,
                height: size.1,
                rate,
                quality: options.quality,
                audio: sound.then(|| wav.clone()),
            };
            video_export::encode(
                path,
                &encode,
                frames,
                |n| renderer.frame(start + n, burn),
                progress,
                cancel,
            )?;
            files.push(path.to_path_buf());
        }
        None => {
            std::fs::create_dir_all(path)
                .with_context(|| format!("Cannot create the folder {}", path.display()))?;
            let digits = end.to_string().len().max(5);
            for n in start..end {
                crate::printing::canceled(cancel)?;
                let pixels = renderer.frame(n, burn)?;
                let file = path.join(format!("frame_{n:0digits$}.png"));
                image::RgbaImage::from_raw(size.0, size.1, pixels)
                    .context("Invalid rendered frame")?
                    .save(&file)
                    .with_context(|| format!("Cannot write {}", file.display()))?;
                files.push(file);
                progress(n - start + 1, frames);
            }
            if sound {
                let file = path.join("soundtrack.wav");
                std::fs::copy(&wav, &file)?;
                files.push(file);
            }
        }
    }
    Ok(Report {
        frames,
        seconds: rate.frames_to_seconds(frames),
        width: size.0,
        height: size.1,
        audio: sound,
        files,
    })
}

/// Export the animatic as a looping GIF at `path`, sampled at the GIF's
/// frame rate. Leaves any existing file untouched on failure or cancel.
pub fn write_gif(
    project: &Project,
    options: &GifOptions,
    path: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<Report> {
    options.validate()?;
    let board = board(project)?;
    let rate = board.settings.frame_rate;
    let rect = area_rect(project, options.area)?;
    let size = output_size(rect, options.width, false);
    let mut renderer = AnimaticRenderer::new(project, rect, size)?;
    let (start, end) = range(renderer.frames(), options.start, options.end)?;
    let seconds = rate.frames_to_seconds(end - start);
    let count = ((seconds * f64::from(options.fps)).round() as u64).max(1);
    if count > MAX_GIF_FRAMES {
        bail!(
            "GIFs are limited to {MAX_GIF_FRAMES} frames; lower the frame rate or export a shorter range"
        )
    }
    let burn = options.burn_in.as_ref();
    let delay = image::Delay::from_numer_denom_ms(1000, options.fps);
    let frames = (0..count).map(|i| {
        crate::printing::canceled(cancel).map_err(|e| e.to_string())?;
        let at = (start + rate.seconds_to_frames(i as f64 / f64::from(options.fps))).min(end - 1);
        let pixels = renderer.frame(at, burn).map_err(|e| e.to_string())?;
        progress(i + 1, count);
        image::RgbaImage::from_raw(size.0, size.1, pixels)
            .map(|image| (image, delay))
            .ok_or_else(|| "Invalid rendered frame".to_string())
    });
    crate::frame_export::encode_gif_frames(path, frames, 10).map_err(anyhow::Error::msg)?;
    Ok(Report {
        frames: count,
        seconds,
        width: size.0,
        height: size.1,
        audio: false,
        files: vec![path.to_path_buf()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storyboard_export::tests::project;
    use emulsion_core::timeline::{Transition, TransitionKind};
    use emulsion_core::{Command, Node, NodeKind, command::Slot};
    use image::AnimationDecoder;

    /// The test board with each panel filled with its own colour and a
    /// dissolve into panel 2. Panels: 48, 48, 12 frames at 24 fps.
    fn coloured() -> Project {
        let mut project = project();
        for (page, rgba) in
            project
                .pages
                .iter_mut()
                .zip([[255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255]])
        {
            Command::AddNode {
                node: Box::new(Node::new(0, "Fill", NodeKind::Fill { rgba })),
                slot: Slot::TOP,
            }
            .apply(&mut page.doc)
            .unwrap();
        }
        let second = project.pages[1].meta.id;
        project
            .storyboard
            .as_mut()
            .unwrap()
            .panels
            .get_mut(&second)
            .unwrap()
            .transition = Transition {
            kind: TransitionKind::Dissolve,
            frames: 8,
        };
        project
    }

    #[test]
    fn expanded_panel_crop_rejects_vector_work_without_publishing_frame() {
        let mut project = coloured();
        let page = &mut project.pages[0];
        let id = page.meta.id;
        let fill = page
            .doc
            .nodes
            .iter_mut()
            .find(|node| node.name == "Fill")
            .unwrap();
        fill.vector_mask = Some(emulsion_core::VectorMask {
            path: std::sync::Arc::new(emulsion_raster::vector_geometry::rectangle(
                6_550_000., 0., 50_000., 3_600_000.,
            )),
            transform: [1e-5, 0., 0., 1e-5, 0., 0.],
            properties: emulsion_core::MaskProperties {
                density: 1.,
                feather: 1000.,
            },
            ..Default::default()
        });
        page.doc.validate().unwrap();
        let original = page.doc.clone();
        let mut renderer =
            AnimaticRenderer::new(&project, IRect::new(0, 0, 68, 36), (68, 36)).unwrap();
        for _ in 0..2 {
            let error = renderer.panel(id).unwrap_err();
            assert!(
                error.to_string().contains("native rendering work budget"),
                "{error}"
            );
            assert!(renderer.panels.is_empty());
            assert!(renderer.animated.is_empty());
        }
        assert_eq!(project.pages[0].doc, original);
    }

    #[test]
    fn frames_render_panels_transitions_and_burn_in() {
        let project = coloured();
        let rect = area_rect(&project, RenderArea::Camera).unwrap();
        assert_eq!(rect, IRect::new(0, 0, 64, 36));
        let size = output_size(rect, 32, true);
        assert_eq!(size, (32, 18));
        let mut r = AnimaticRenderer::new(&project, rect, size).unwrap();
        assert_eq!(r.frames(), 108);
        let px = |f: &[u8]| [f[0], f[1], f[2], f[3]];
        assert_eq!(px(&r.frame(0, None).unwrap()), [255, 0, 0, 255]);
        let mid = r.frame(51, None).unwrap();
        assert!(mid[0] > 60 && mid[2] > 60, "dissolving: {:?}", px(&mid));
        assert_eq!(px(&r.frame(60, None).unwrap()), [0, 0, 255, 255]);
        let burn = BurnIn {
            size: 20.,
            ..BurnIn::default()
        };
        let burnt = r.frame(0, Some(&burn)).unwrap();
        let last_row = &burnt[(17 * 32 * 4)..];
        assert!(last_row[0] < 255, "the band darkens the bottom");
        assert!(r.frame(108, None).is_err());
        let over = area_rect(&project, RenderArea::Overscan).unwrap();
        assert!(over.w > 64 && over.x < 0);
        let wide_size = output_size(over, 0, true);
        let mut wide = AnimaticRenderer::new(&project, over, wide_size).unwrap();
        // Fills reach past the camera.
        let frame = wide.frame(0, None).unwrap();
        assert_eq!(frame.len(), (wide_size.0 * wide_size.1 * 4) as usize);
        assert_eq!(px(&frame), [255, 0, 0, 255]);
        // Page-wide fills are not artwork that widens the area.
        assert_eq!(area_rect(&project, RenderArea::AllArtwork).unwrap(), rect);
    }

    #[test]
    fn frames_draw_the_reference_video_over_or_inset() {
        if !crate::ffmpeg::available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = crate::reference_video::test_video::counter(dir.path(), 24, 30);
        let video = crate::reference_video::import(&file, false).unwrap().video;
        let mut project = coloured();
        let board = project.storyboard.as_mut().unwrap();
        let rate = board.settings.frame_rate;
        board
            .timeline
            .import_video(video, None, 0, rate, None)
            .unwrap();
        let rect = area_rect(&project, RenderArea::Camera).unwrap();
        let px = |f: &[u8], x: u32, y: u32| {
            let i = ((y * 64 + x) * 4) as usize;
            [f[i], f[i + 1], f[i + 2]]
        };
        let mut plain = AnimaticRenderer::new(&project, rect, (64, 36)).unwrap();
        assert_eq!(px(&plain.frame(5, None).unwrap(), 32, 18), [255, 0, 0]);
        let mut over = AnimaticRenderer::new(&project, rect, (64, 36))
            .unwrap()
            .with_reference_video(Some(VideoPlacement::Overlay));
        // Picture 5 is grey 50, fitted in the middle; the sides show red.
        let frame = over.frame(5, None).unwrap();
        assert_eq!(px(&frame, 32, 18), [50, 50, 50]);
        assert_eq!(px(&frame, 1, 18), [255, 0, 0]);
        let mut inset = AnimaticRenderer::new(&project, rect, (64, 36))
            .unwrap()
            .with_reference_video(Some(VideoPlacement::PictureInPicture));
        let frame = inset.frame(10, None).unwrap();
        assert_eq!(px(&frame, 58, 31), [100, 100, 100]);
        assert_eq!(px(&frame, 32, 18), [255, 0, 0]);
        // Past the clip, nothing is drawn.
        assert_eq!(px(&over.frame(40, None).unwrap(), 32, 18), [255, 0, 0]);
    }

    /// Add a `w` × 36 raster of `rgba` (16-bit) at the left of page `page`;
    /// returns its id.
    fn strip(project: &mut Project, page: usize, w: u32, rgba: [u16; 4]) -> emulsion_core::NodeId {
        let raster = emulsion_raster::Raster::from_fn(w, 36, [0; 4], |_, _| rgba);
        let doc = &mut project.pages[page].doc;
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Strip",
                Arc::new(raster),
                emulsion_raster::Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(doc)
        .unwrap();
        doc.nodes.last().unwrap().id
    }

    #[test]
    fn frames_show_the_scene_camera() {
        use emulsion_core::storyboard::{CameraKey, SceneCamera};
        // Panel 1: red, with a blue strip over its left half.
        let mut project = coloured();
        strip(&mut project, 0, 32, [0, 0, 65535, 65535]);
        let first = project.pages[0].meta.id;
        let rect = area_rect(&project, RenderArea::Camera).unwrap();
        let px = |f: &[u8], x: u32, y: u32| {
            let i = ((y * 64 + x) * 4) as usize;
            [f[i], f[i + 1], f[i + 2]]
        };
        {
            let mut r = AnimaticRenderer::new(&project, rect, (64, 36)).unwrap();
            let f = r.frame(0, None).unwrap();
            assert_eq!((px(&f, 4, 18), px(&f, 60, 18)), ([0, 0, 255], [255, 0, 0]));
        }
        // Zoom 2 on the right half: the whole picture is red, and the camera
        // spans the scene's second panel too.
        let board = project.storyboard.as_mut().unwrap();
        let scene = board.panels[&first].scene;
        let state = CameraState {
            x: 48.,
            y: 18.,
            zoom: 2.,
            ..board.rest_camera()
        };
        board.cameras.insert(
            scene,
            SceneCamera {
                keys: vec![CameraKey::at(0, state)],
                shake: None,
            },
        );
        let mut r = AnimaticRenderer::new(&project, rect, (64, 36)).unwrap();
        let f = r.frame(0, None).unwrap();
        for x in [1, 32, 62] {
            let [red, _, blue] = px(&f, x, 18);
            assert!(red > 240 && blue < 15, "x {x}: {:?}", px(&f, x, 18));
        }
        // The camera's centre is in panel pixels: the move to the left half
        // shows blue.
        let board = project.storyboard.as_mut().unwrap();
        board
            .cameras
            .get_mut(&scene)
            .unwrap()
            .keys
            .push(CameraKey::at(40, CameraState { x: 16., ..state }));
        let mut r = AnimaticRenderer::new(&project, rect, (64, 36)).unwrap();
        let f = r.frame(40, None).unwrap();
        assert_eq!(px(&f, 32, 18), [0, 0, 255]);
        // Panel 2 holds the last key.
        let f = r.frame(60, None).unwrap();
        assert_eq!(px(&f, 32, 18), [0, 0, 255]);
        // Scene 2 has no camera.
        let f = r.frame(100, None).unwrap();
        assert_eq!(px(&f, 32, 18), [0, 255, 0]);
    }

    #[test]
    fn frames_show_layer_keyframes() {
        use emulsion_core::motion::Easing;
        use emulsion_core::storyboard::{LayerMotion, LayerProperty, MotionKey, PropertyTrack};
        // Panel 3 (frames 96–107): a black strip slides 48 px right.
        let mut project = coloured();
        let id = strip(&mut project, 2, 16, [0, 0, 0, 65535]);
        let third = project.pages[2].meta.id;
        let key = |frame, value| MotionKey {
            frame,
            value,
            easing: Easing::Linear,
            curve: None,
        };
        project
            .storyboard
            .as_mut()
            .unwrap()
            .panels
            .get_mut(&third)
            .unwrap()
            .motion
            .insert(
                id,
                LayerMotion {
                    pivot: None,
                    tracks: vec![PropertyTrack {
                        property: LayerProperty::X,
                        keys: vec![key(0, 0.), key(11, 48.)],
                    }],
                },
            );
        let rect = area_rect(&project, RenderArea::Camera).unwrap();
        let mut r = AnimaticRenderer::new(&project, rect, (64, 36)).unwrap();
        let px = |f: &[u8], x: u32| {
            let i = ((18 * 64 + x) * 4) as usize;
            [f[i], f[i + 1], f[i + 2]]
        };
        let start = r.frame(96, None).unwrap();
        assert_eq!((px(&start, 4), px(&start, 56)), ([0, 0, 0], [0, 255, 0]));
        let end = r.frame(107, None).unwrap();
        assert_eq!((px(&end, 4), px(&end, 56)), ([0, 255, 0], [0, 0, 0]));
        // The page itself is unchanged, and the still picture has no motion.
        assert_eq!(px(&r.panel(third).unwrap(), 4), [0, 0, 0]);
    }

    #[test]
    fn gifs_sample_the_animatic_and_check_options() {
        let project = coloured();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.gif");
        let options = GifOptions {
            width: 32,
            fps: 4,
            start: 24,
            end: Some(72),
            ..Default::default()
        };
        let mut calls = 0;
        let report = write_gif(
            &project,
            &options,
            &path,
            &mut |_, _| calls += 1,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!((report.frames, calls), (8, 8));
        let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
            std::fs::File::open(&path).unwrap(),
        ))
        .unwrap();
        let frames = decoder.into_frames().collect_frames().unwrap();
        assert_eq!(frames.len(), 8);
        assert_eq!(frames[0].delay().numer_denom_ms(), (250, 1));
        assert_eq!(frames[0].buffer().dimensions(), (32, 18));
        let red = frames[0].buffer().get_pixel(4, 4).0;
        let blue = frames[7].buffer().get_pixel(4, 4).0;
        assert!(red[0] > 200 && red[2] < 50, "{red:?}");
        assert!(blue[2] > 200 && blue[0] < 50, "{blue:?}");
        for bad in [
            GifOptions {
                fps: 0,
                ..options.clone()
            },
            GifOptions {
                end: Some(500),
                ..options.clone()
            },
            GifOptions {
                start: 72,
                ..options.clone()
            },
        ] {
            let before = std::fs::read(&path).unwrap();
            assert!(
                write_gif(
                    &project,
                    &bad,
                    &path,
                    &mut |_, _| {},
                    &AtomicBool::new(false)
                )
                .is_err()
            );
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        assert!(
            write_gif(
                &project,
                &options,
                &path,
                &mut |_, _| {},
                &AtomicBool::new(true)
            )
            .is_err()
        );
    }

    #[test]
    fn movies_have_the_expected_frames_size_and_sound() {
        if !crate::audio::test_audio::ffmpeg() {
            return;
        }
        let mut project = coloured();
        let dir = tempfile::tempdir().unwrap();
        let sound = crate::audio::test_audio::tone(dir.path(), "beep.wav", 440, 2.);
        let asset = crate::audio::store::import(&sound, "").unwrap();
        let timeline = &mut project.storyboard.as_mut().unwrap().timeline;
        let id = timeline.add_asset(asset).unwrap();
        timeline
            .tracks
            .push(emulsion_core::timeline::AudioTrack::new("Sound"));
        timeline
            .place(
                0,
                emulsion_core::timeline::AudioClip {
                    asset: id,
                    name: "Beep".into(),
                    start: 0,
                    frames: 48,
                    ..Default::default()
                },
            )
            .unwrap();
        let path = dir.path().join("board.mp4");
        let options = MovieOptions {
            width: 32,
            end: Some(60),
            burn_in: Some(BurnIn::default()),
            ..Default::default()
        };
        let mut last = (0, 0);
        let report = write_movie(
            &project,
            &options,
            &path,
            &mut |d, t| last = (d, t),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(last, (60, 60));
        assert!(report.audio);
        let info = video_export::probe(&path).unwrap();
        assert_eq!((info.frames, info.width, info.height), (60, 32, 18));
        assert!(info.audio);
        assert!((info.seconds - 2.5).abs() < 0.1, "{info:?}");
        // ProRes without sound, from frame 48.
        let mov = dir.path().join("board.mov");
        let options = MovieOptions {
            format: MovieFormat::Mov,
            width: 32,
            start: 48,
            ..Default::default()
        };
        let report = write_movie(
            &project,
            &options,
            &mov,
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(!report.audio, "no clip plays after frame 48");
        let info = video_export::probe(&mov).unwrap();
        assert_eq!((info.frames, info.codec.as_str()), (60, "prores"));
        // An image sequence.
        let seq = dir.path().join("frames");
        let options = MovieOptions {
            format: MovieFormat::PngSequence,
            start: 10,
            end: Some(13),
            ..Default::default()
        };
        let report = write_movie(
            &project,
            &options,
            &seq,
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(report.files.len(), 4, "three frames and the sound");
        assert!(seq.join("frame_00010.png").is_file());
        assert_eq!(
            image::open(seq.join("frame_00012.png")).unwrap().width(),
            64
        );
        let bad = MovieOptions {
            quality: 0,
            ..Default::default()
        };
        let nothing = dir.path().join("nothing.mp4");
        assert!(
            write_movie(
                &project,
                &bad,
                &nothing,
                &mut |_, _| {},
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(!nothing.exists());
    }
}
