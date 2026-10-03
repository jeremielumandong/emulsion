//! Reference video on the storyboard Timeline (T5): video tracks with clips
//! drawn as a strip of pictures, moved and trimmed by drag (one Undo step
//! each, through `TimingEdit::Audio`, which carries the whole timeline),
//! import from a track menu or by dropping video files, and the reference
//! picture shown over the Stage and the player, fitted with the clip's
//! opacity or as a picture-in-picture inset (View → Reference Video).
//!
//! Pictures decode on background threads through
//! `emulsion_io::reference_video`: one decode runs at a time, and when it
//! finishes the newest wanted picture is decoded next, so scrubbing never
//! blocks the UI and requests passed over meanwhile are dropped.
use super::view::menu_item;
use super::*;
use crate::file_prompt::FilePrompts;
use emulsion_core::timeline::video::{self as tv, VideoPlacement};
use emulsion_core::timeline::{VideoAsset, VideoClip, VideoTrack};
use emulsion_io::reference_video::decode::{self, Picture};
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::PopupMenu,
};
use std::collections::{HashMap, HashSet, VecDeque};

const ROW_H: f32 = 46.;
/// Longest side of the reference picture over the Stage and player.
const MAX_PICTURE: u32 = 1280;
/// Pictures kept for the clip strips.
const MAX_POSTERS: usize = 400;
/// Pictures decoded ahead while playing.
const AHEAD: u32 = 12;

/// What a reference picture was made from.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PictureKey {
    asset: u64,
    index: u64,
    opacity: u32,
    placement: VideoPlacement,
    size: (u32, u32),
}

/// One picture in a clip's strip.
type PosterKey = (u64, u64, (u32, u32));

pub(crate) struct VideoUi {
    /// How the reference shows over the Stage and player; `None` hides it.
    pub(crate) placement: Option<VideoPlacement>,
    /// The selected video clip (track, index).
    pub(crate) clip: Option<(usize, usize)>,
    /// Each video track's lane, for dragging clips between tracks.
    rows: Rc<RefCell<Vec<Option<Bounds<Pixels>>>>>,
    picture: Option<(PictureKey, Arc<RenderImage>)>,
    /// A picture is decoding.
    busy: bool,
    /// The picture that last failed, so it is not tried again and again.
    failed: Option<PictureKey>,
    posters: HashMap<PosterKey, Arc<RenderImage>>,
    /// Oldest first, for dropping.
    poster_order: VecDeque<PosterKey>,
    /// Strip pictures the last paint wanted, left to right.
    poster_queue: Vec<(PosterKey, PathBuf, f64)>,
    poster_busy: bool,
    poster_failed: HashSet<PosterKey>,
}

impl Default for VideoUi {
    fn default() -> Self {
        Self {
            placement: Some(VideoPlacement::Overlay),
            clip: None,
            rows: Rc::default(),
            picture: None,
            busy: false,
            failed: None,
            posters: HashMap::new(),
            poster_order: VecDeque::new(),
            poster_queue: Vec::new(),
            poster_busy: false,
            poster_failed: HashSet::new(),
        }
    }
}

/// A decoded picture `picture` of a `video`-sized video, placed on a
/// transparent `size` canvas the shape of the panel, as BGRA with
/// `opacity` in its alpha: lay it over the panel's frame.
pub(crate) fn reference_canvas(
    picture: &Picture,
    video: (u32, u32),
    size: (u32, u32),
    placement: VideoPlacement,
    opacity: f32,
) -> Vec<u8> {
    let mut out = vec![0u8; size.0 as usize * size.1 as usize * 4];
    let rect = tv::placement_rect(
        placement,
        f64::from(size.0),
        f64::from(size.1),
        f64::from(video.0),
        f64::from(video.1),
    );
    tv::draw_picture(
        &mut out,
        size,
        &picture.rgba,
        (picture.width, picture.height),
        rect,
        opacity,
    );
    for px in out.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    out
}

/// A picture of `size` (RGBA) as a GPU image.
fn rgba_image(picture: &Picture) -> RenderImage {
    let mut bytes = picture.rgba.clone();
    for px in bytes.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    viewport::bgra_image(picture.width, picture.height, bytes)
}

/// Free the GPU copies of pictures no longer shown.
pub(crate) fn drop_images(images: Vec<Arc<RenderImage>>, cx: &mut Context<EditorView>) {
    if images.is_empty() {
        return;
    }
    cx.defer(move |cx| {
        for handle in cx.windows() {
            let images = images.clone();
            cx.update_window(handle, |_, window, _| {
                for image in images {
                    window.drop_image(image).ok();
                }
            })
            .ok();
        }
    });
}

/// A video clip moved (to `target` track) or trimmed by the pointer at
/// `frame`. `None` when the change cannot fit.
pub(crate) fn video_clip_edit(
    timeline: &Timeline,
    rate: FrameRate,
    (track, index): (usize, usize),
    part: ClipPart,
    frame: i64,
    target: usize,
) -> Option<Timeline> {
    let mut next = timeline.clone();
    let clips = &timeline.video.get(track)?.clips;
    let clip = clips.get(index)?.clone();
    if clip.locked {
        return None;
    }
    let prev_end = index
        .checked_sub(1)
        .map_or(0, |i| clips[i].end())
        .min(clip.start);
    let next_start = clips.get(index + 1).map_or(u64::MAX, |c| c.start);
    let mut edited = clip.clone();
    match part {
        ClipPart::Start => {
            let (start, offset_ms) = trim_head(
                rate,
                (clip.start, clip.end(), clip.offset_ms),
                prev_end,
                frame,
            );
            edited.start = start;
            edited.offset_ms = offset_ms;
            edited.frames = clip.end() - start;
            if offset_ms >= timeline.videos.get(&clip.asset)?.duration_ms {
                return None;
            }
        }
        ClipPart::End => {
            let room = timeline.video_room(&clip, rate);
            edited.frames = trim_tail(clip.start, room, next_start, frame) - clip.start;
        }
        _ => {
            edited.start = frame.max(0) as u64;
            next.video[track].clips.remove(index);
            next.place_video(target, edited).ok()?;
            return Some(next);
        }
    }
    next.video[track].clips[index] = edited;
    Some(next)
}

impl EditorView {
    // ── Showing the reference ──

    /// The picture size for reference pictures: the panel's shape.
    fn reference_size(&self) -> Option<(u32, u32)> {
        let board = self.editor.storyboard()?;
        Some(decode::fit_size(
            (board.settings.width, board.settings.height),
            (MAX_PICTURE, MAX_PICTURE),
        ))
    }

    /// Keep the reference picture on the playhead: decode it in the
    /// background when it changes. Called whenever the Stage or Board
    /// renders.
    pub(crate) fn reference_video_sync(&mut self, cx: &mut Context<Self>) {
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let rate = board.settings.frame_rate;
        let timeline = &board.timeline;
        // Selections that no longer exist (after Undo).
        let ui = &mut self.timeline_ui.video;
        ui.clip = ui
            .clip
            .filter(|(t, c)| timeline.video.get(*t).is_some_and(|t| *c < t.clips.len()));
        let wanted = ui
            .placement
            .zip(self.reference_size())
            .and_then(|(placement, size)| {
                let at = timeline.video_at(self.transport.frame, rate)?;
                let asset = timeline.videos.get(&at.asset)?;
                let key = PictureKey {
                    asset: at.asset,
                    index: at.index,
                    opacity: at.opacity.to_bits(),
                    placement,
                    size,
                };
                Some((key, asset.clone(), at.opacity))
            });
        let Some((key, asset, opacity)) = wanted else {
            if let Some((_, image)) = self.timeline_ui.video.picture.take() {
                drop_images(vec![image], cx);
                cx.notify();
            }
            return;
        };
        let ui = &mut self.timeline_ui.video;
        if ui.picture.as_ref().is_some_and(|(k, _)| *k == key) || ui.busy || ui.failed == Some(key)
        {
            return;
        }
        let Some(source) = asset.source.clone() else {
            return;
        };
        ui.busy = true;
        let ahead = if self.transport.playing { AHEAD } else { 0 };
        cx.spawn(async move |this, cx| {
            let made = cx
                .background_spawn(async move {
                    let video = (asset.width, asset.height);
                    let (w, h) = key.size;
                    let [_, _, rw, rh] = tv::placement_rect(
                        key.placement,
                        f64::from(w),
                        f64::from(h),
                        f64::from(video.0),
                        f64::from(video.1),
                    );
                    let fit = decode::fit_size(video, (rw.ceil() as u32, rh.ceil() as u32));
                    let picture = decode::picture(&source, asset.fps, key.index, fit, ahead)
                        .map_err(|e| format!("{e:#}"))?;
                    let bytes = reference_canvas(&picture, video, key.size, key.placement, opacity);
                    Ok::<_, String>(Arc::new(viewport::bgra_image(w, h, bytes)))
                })
                .await;
            this.update(cx, |this, cx| {
                let ui = &mut this.timeline_ui.video;
                ui.busy = false;
                match made {
                    Ok(image) => {
                        if let Some((_, old)) = ui.picture.replace((key, image)) {
                            drop_images(vec![old], cx);
                        }
                    }
                    Err(error) => {
                        ui.failed = Some(key);
                        this.set_status(format!("Reference video: {error}"), true, cx);
                    }
                }
                // The playhead may have moved on meanwhile.
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The reference picture to draw over the Stage's frame, while the
    /// player is not covering it.
    pub(crate) fn reference_video_stage(&self) -> Option<Arc<RenderImage>> {
        if self.player.showing || self.board_open() {
            return None;
        }
        self.reference_video_image()
    }

    fn reference_video_image(&self) -> Option<Arc<RenderImage>> {
        self.timeline_ui.video.placement?;
        self.timeline_ui
            .video
            .picture
            .as_ref()
            .map(|(_, image)| image.clone())
    }

    /// The reference picture over the player's picture (both have the
    /// panel's shape, so they line up).
    pub(crate) fn reference_video_player(&self) -> Option<AnyElement> {
        let image = self.reference_video_image()?;
        Some(
            img(image)
                .id("storyboard-player-reference")
                .absolute()
                .inset_0()
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
        )
    }

    pub(crate) fn set_reference_video(
        &mut self,
        placement: Option<VideoPlacement>,
        cx: &mut Context<Self>,
    ) {
        self.timeline_ui.video.placement = placement;
        self.timeline_ui.video.failed = None;
        cx.notify();
    }

    /// View menu entries: how the reference video shows.
    pub(crate) fn reference_video_view_items(
        menu: PopupMenu,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let current = editor.read(cx).timeline_ui.video.placement;
        let owner = editor.downgrade();
        menu.submenu("Reference Video", window, cx, move |mut menu, _, _| {
            for (label, placement) in [
                ("Hidden", None),
                ("Overlay", Some(VideoPlacement::Overlay)),
                ("Picture in Picture", Some(VideoPlacement::PictureInPicture)),
            ] {
                menu = menu.item(
                    menu_item(&owner, label, move |e, _, cx| {
                        e.set_reference_video(placement, cx)
                    })
                    .checked(current == placement),
                );
            }
            menu
        })
    }

    // ── Import ──

    /// Choose a video file and place it at the playhead, on `track` or the
    /// first video track with room; `with_audio` also brings in its sound.
    pub(crate) fn timeline_import_video(
        &mut self,
        track: Option<usize>,
        with_audio: bool,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_open_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import video".into()),
        });
        let at = self.transport.frame;
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.timeline_import_video_files(paths, at, track, with_audio, cx)
            })
            .ok();
        })
        .detach();
    }

    /// Import video files in the background, then place them one after
    /// another from frame `at`, as one Undo step.
    pub(crate) fn timeline_import_video_files(
        &mut self,
        paths: Vec<PathBuf>,
        at: u64,
        track: Option<usize>,
        with_audio: bool,
        cx: &mut Context<Self>,
    ) {
        if self.editor.storyboard().is_none() || paths.is_empty() {
            return;
        }
        self.set_status("Importing video…", false, cx);
        cx.spawn(async move |this, cx| {
            let imported = cx
                .background_spawn(async move {
                    paths
                        .iter()
                        .map(|p| {
                            emulsion_io::reference_video::import(p, with_audio)
                                .map_err(|e| format!("{e:#}"))
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                this.timeline_place_videos(imported, at, track, cx)
            })
            .ok();
        })
        .detach();
    }

    fn timeline_place_videos(
        &mut self,
        imported: Vec<Result<emulsion_io::reference_video::Imported, String>>,
        at: u64,
        track: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let mut errors = Vec::new();
        let videos: Vec<_> = imported
            .into_iter()
            .filter_map(|r| r.map_err(|e| errors.push(e)).ok())
            .collect();
        let count = videos.len();
        let mut last = None;
        if count > 0 {
            self.timeline_audio_edit(
                |t, rate| {
                    let mut start = at;
                    for v in videos {
                        if let Some(track) = track
                            && track == t.video.len()
                        {
                            t.video
                                .push(VideoTrack::new(&format!("Video {}", track + 1)));
                        }
                        let placed = t.import_video(v.video, v.sound, start, rate, track)?;
                        start = t.video[placed.0].clips[placed.1].end();
                        last = Some(placed);
                    }
                    Ok(())
                },
                cx,
            );
        }
        if let Some(clip) = last {
            self.timeline_ui.video.clip = Some(clip);
            self.timeline_ui.clip = None;
            self.timeline_ui.marker = None;
            self.set_status(
                format!(
                    "Imported {count} video{}.",
                    if count == 1 { "" } else { "s" }
                ),
                false,
                cx,
            );
        }
        if let Some(error) = errors.first() {
            self.set_status(error.clone(), true, cx);
        }
    }

    /// Video files dropped on a video lane at window x `x`.
    fn timeline_drop_videos(
        &mut self,
        paths: &ExternalPaths,
        track: Option<usize>,
        x: Pixels,
        cx: &mut Context<Self>,
    ) {
        let videos: Vec<PathBuf> = paths
            .paths()
            .iter()
            .filter(|p| emulsion_io::reference_video::format_of(p).is_some())
            .cloned()
            .collect();
        if videos.is_empty() {
            self.set_status(
                format!(
                    "Drop a video file ({}).",
                    emulsion_io::reference_video::EXTENSIONS.join(", ")
                ),
                true,
                cx,
            );
            return;
        }
        let at = self.timeline_frame_at(x).round() as u64;
        self.timeline_import_video_files(videos, at, track, false, cx);
    }

    // ── Editing ──

    /// Change the video clip `at` as one Undo step. Locked clips refuse
    /// everything but unlocking.
    fn timeline_video_clip_edit(
        &mut self,
        (track, index): (usize, usize),
        edit: impl FnOnce(&mut VideoClip) + 'static,
        cx: &mut Context<Self>,
    ) -> bool {
        self.timeline_audio_edit(
            move |t, _| {
                let clip = t
                    .video
                    .get_mut(track)
                    .and_then(|t| t.clips.get_mut(index))
                    .ok_or("That clip no longer exists.")?;
                let was_locked = clip.locked;
                edit(clip);
                if was_locked && clip.locked {
                    return Err("That clip is locked. Unlock it to change it.".into());
                }
                Ok(())
            },
            cx,
        )
    }

    pub(crate) fn timeline_delete_video_clip(
        &mut self,
        (track, index): (usize, usize),
        cx: &mut Context<Self>,
    ) {
        if self.timeline_audio_edit(
            |t, _| {
                let clips = &mut t
                    .video
                    .get_mut(track)
                    .ok_or("No video track has that index.")?
                    .clips;
                match clips.get(index) {
                    None => return Err("That clip no longer exists.".into()),
                    Some(c) if c.locked => {
                        return Err("That clip is locked. Unlock it to delete it.".into());
                    }
                    Some(_) => clips.remove(index),
                };
                t.remove_unused_videos();
                Ok(())
            },
            cx,
        ) {
            self.timeline_ui.video.clip = None;
        }
    }

    /// Delete the selected video clip, if one is selected and no audio clip
    /// or marker was picked since. Returns whether it was.
    pub(crate) fn timeline_delete_selected_video(&mut self, cx: &mut Context<Self>) -> bool {
        let ui = &self.timeline_ui;
        let Some(clip) = ui
            .video
            .clip
            .filter(|_| ui.clip.is_none() && ui.marker.is_none())
        else {
            return false;
        };
        self.timeline_delete_video_clip(clip, cx);
        true
    }

    fn timeline_delete_video_track(&mut self, track: usize, cx: &mut Context<Self>) {
        if self.timeline_audio_edit(
            |t, _| {
                let clips = &t
                    .video
                    .get(track)
                    .ok_or("No video track has that index.")?
                    .clips;
                if clips.iter().any(|c| c.locked) {
                    return Err("The track holds a locked clip. Unlock it first.".into());
                }
                t.video.remove(track);
                t.remove_unused_videos();
                Ok(())
            },
            cx,
        ) {
            self.timeline_ui.video.clip = None;
        }
    }

    // ── Drags ──

    /// What dragging part `part` of video clip `index` on `track` to the
    /// pointer does: the edit to preview and the overlay text.
    pub(super) fn timeline_video_move(
        &self,
        (track, index): (usize, usize),
        part: ClipPart,
        travel: f64,
        y: Pixels,
        modifiers: Modifiers,
    ) -> Option<(TimingEdit, String)> {
        let board = self.editor.storyboard()?;
        let rate = board.settings.frame_rate;
        let timeline = &board.timeline;
        let clip = timeline.video.get(track)?.clips.get(index)?;
        let shift = |anchor: u64| (anchor as f64 + travel).round() as i64;
        let mut target = track;
        let at = match part {
            ClipPart::Start => self.timeline_snap(shift(clip.start), Some(clip.start), modifiers),
            ClipPart::End => self.timeline_snap(shift(clip.end()), Some(clip.end()), modifiers),
            _ => {
                let start = shift(clip.start);
                let snapped = self.timeline_snap(start, Some(clip.start), modifiers);
                target = self.timeline_video_track_at(y).unwrap_or(track);
                if snapped != start {
                    snapped
                } else {
                    let frames = clip.frames as i64;
                    self.timeline_snap(start + frames, Some(clip.end()), modifiers) - frames
                }
            }
        };
        let next = video_clip_edit(timeline, rate, (track, index), part, at, target)?;
        let text = match part {
            ClipPart::Start | ClipPart::End => format!(
                "Clip {}",
                duration_label(rate, next.video[track].clips[index].frames)
            ),
            _ => format!("Start {}", frame_label(rate, at.max(0) as u64, true)),
        };
        Some((TimingEdit::Audio(next), text))
    }

    fn timeline_video_track_at(&self, y: Pixels) -> Option<usize> {
        self.timeline_ui
            .video
            .rows
            .borrow()
            .iter()
            .position(|b| b.is_some_and(|b| y >= b.origin.y && y < b.origin.y + b.size.height))
    }

    /// Where a dragged video clip landed, to keep it selected.
    pub(super) fn timeline_video_landing(
        &self,
        drag: &TimelineDrag,
        edit: &TimingEdit,
    ) -> Option<(usize, usize)> {
        let (TimelineDrag::Video { .. }, TimingEdit::Audio(next)) = (drag, edit) else {
            return None;
        };
        let board = self.editor.storyboard()?;
        let old: Vec<_> = board.timeline.video.iter().map(|t| &t.clips).collect();
        next.video.iter().enumerate().find_map(|(t, track)| {
            track
                .clips
                .iter()
                .position(|c| old.get(t).is_none_or(|o| !o.contains(c)))
                .map(|c| (t, c))
        })
    }

    fn timeline_video_begin(
        &mut self,
        at: (usize, usize),
        part: ClipPart,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.timeline_ui.video.clip = Some(at);
        self.timeline_ui.clip = None;
        self.timeline_ui.marker = None;
        let locked = self
            .editor
            .storyboard()
            .and_then(|b| b.timeline.video.get(at.0)?.clips.get(at.1))
            .is_some_and(|c| c.locked);
        if locked {
            self.set_status("That clip is locked. Unlock it to move it.", true, cx);
            cx.notify();
            return;
        }
        let drag = TimelineDrag::Video {
            track: at.0,
            index: at.1,
            part,
        };
        self.timeline_begin(drag, e.position, e.modifiers, window, cx);
    }

    // ── Rows ──

    fn timeline_video_track_menu(
        menu: PopupMenu,
        owner: WeakEntity<Self>,
        track: Option<usize>,
        name: String,
    ) -> PopupMenu {
        let mut menu = menu
            .item(menu_item(&owner, "Import video…", move |e, _, cx| {
                e.timeline_import_video(track, false, cx)
            }))
            .item(menu_item(
                &owner,
                "Import video with its sound…",
                move |e, _, cx| e.timeline_import_video(track, true, cx),
            ));
        if let Some(track) = track {
            menu = menu
                .separator()
                .item(menu_item(
                    &owner,
                    "Rename track…",
                    move |e, window, cx| {
                        e.timeline_text_dialog(
                            "Rename track",
                            "Name",
                            name.clone(),
                            "Rename",
                            move |this, text, cx| {
                                let name = text.trim().to_string();
                                this.timeline_audio_edit(
                                    move |t, _| {
                                        t.video
                                            .get_mut(track)
                                            .ok_or("No video track has that index.")?
                                            .name = name;
                                        Ok(())
                                    },
                                    cx,
                                )
                            },
                            window,
                            cx,
                        )
                    },
                ))
                .item(menu_item(&owner, "Delete track", move |e, _, cx| {
                    e.timeline_delete_video_track(track, cx)
                }));
        }
        menu
    }

    fn timeline_video_clip_menu(
        menu: PopupMenu,
        owner: WeakEntity<Self>,
        at: (usize, usize),
        clip: VideoClip,
    ) -> PopupMenu {
        let name = clip.name.clone();
        let opacity = clip.opacity;
        menu.item(menu_item(&owner, "Rename clip…", move |e, window, cx| {
            e.timeline_text_dialog(
                "Rename clip",
                "Name",
                name.clone(),
                "Rename",
                move |this, text, cx| {
                    let name = text.trim().to_string();
                    this.timeline_video_clip_edit(at, move |c| c.name = name, cx)
                },
                window,
                cx,
            )
        }))
        .item(menu_item(
            &owner,
            format!("Opacity… ({:.0}%)", opacity * 100.),
            move |e, window, cx| {
                e.timeline_text_dialog(
                    "Clip opacity",
                    "Opacity in percent (0–100)",
                    format!("{:.0}", opacity * 100.),
                    "Set",
                    move |this, text, cx| match text
                        .trim()
                        .trim_end_matches('%')
                        .trim()
                        .parse::<f32>()
                    {
                        Ok(v) if (0. ..=100.).contains(&v) => {
                            this.timeline_video_clip_edit(at, move |c| c.opacity = v / 100., cx)
                        }
                        _ => {
                            this.set_status("Type the opacity in percent, 0–100.", true, cx);
                            false
                        }
                    },
                    window,
                    cx,
                )
            },
        ))
        .item(menu_item(
            &owner,
            if clip.visible { "Hide" } else { "Show" },
            move |e, _, cx| {
                e.timeline_video_clip_edit(at, |c| c.visible = !c.visible, cx);
            },
        ))
        .item(menu_item(
            &owner,
            if clip.locked { "Unlock" } else { "Lock" },
            move |e, _, cx| {
                e.timeline_video_clip_edit(at, |c| c.locked = !c.locked, cx);
            },
        ))
        .separator()
        .item(menu_item(&owner, "Delete clip", move |e, _, cx| {
            e.timeline_delete_video_clip(at, cx)
        }))
    }

    /// The video track rows (or, with none, a row to import or drop video
    /// into), above the audio tracks.
    pub(super) fn timeline_video_rows(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(timeline) = self.timeline_board().map(|b| b.timeline.clone()) else {
            return Vec::new();
        };
        self.timeline_ui.video.poster_queue.clear();
        let owner = cx.weak_entity();
        let count = timeline.video.len().max(1);
        {
            let mut rows = self.timeline_ui.video.rows.borrow_mut();
            rows.resize(timeline.video.len(), None);
            rows.truncate(timeline.video.len());
        }
        let mut out = Vec::new();
        for t in 0..count {
            let track = timeline.video.get(t);
            let index = track.map(|_| t);
            let rows_cell = self.timeline_ui.video.rows.clone();
            let mut lane = Self::timeline_lane(("timeline-video", t), ROW_H)
                .test_support()
                .border_b_1()
                .border_color(p.line)
                .on_scroll_wheel(
                    cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.timeline_wheel(e, cx)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, e: &MouseDownEvent, window, cx| {
                        this.timeline_ui.video.clip = None;
                        this.timeline_begin(
                            TimelineDrag::Scrub,
                            e.position,
                            e.modifiers,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }),
                )
                .drag_over::<ExternalPaths>(|s, _, _, _| s.bg(gpui_kit::black().opacity(0.06)))
                .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
                    let x = window.mouse_position().x;
                    this.timeline_drop_videos(paths, index, x, cx);
                }))
                .child(
                    canvas(
                        move |bounds, _, _| {
                            if let Some(slot) = rows_cell.borrow_mut().get_mut(t) {
                                *slot = Some(bounds);
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                );
            match track {
                Some(track) => {
                    for (c, clip) in track.clips.iter().enumerate() {
                        if let Some(el) = self.timeline_video_clip(t, c, clip, &timeline, p, cx) {
                            lane = lane.child(el);
                        }
                    }
                }
                None => {
                    lane = lane.child(
                        div()
                            .absolute()
                            .left_2()
                            .top(px(14.))
                            .text_size(px(10.))
                            .text_color(p.muted)
                            .whitespace_nowrap()
                            .child("Drop a video file here, or right-click Video to import one"),
                    );
                }
            }
            lane = lane.children(self.timeline_playhead_line(p));
            let name = track.map_or("Video".to_string(), |t| t.name.clone());
            let menu_owner = owner.clone();
            let menu_name = name.clone();
            let header = Self::timeline_header(p, ROW_H)
                .id(("timeline-video-header", t))
                .test_support()
                .flex()
                .items_center()
                .gap_1()
                .context_menu(move |menu, _, _| {
                    Self::timeline_video_track_menu(
                        menu,
                        menu_owner.clone(),
                        index,
                        menu_name.clone(),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(11.))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_color(if track.is_some() { p.ink } else { p.muted })
                        .child(name),
                )
                .child(
                    Button::new(("timeline-video-import", t))
                        .label("Import…")
                        .tooltip("Import a video file at the playhead")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.timeline_import_video(index, false, cx)
                        })),
                );
            out.push(
                div()
                    .flex()
                    .flex_none()
                    .child(header)
                    .child(lane)
                    .into_any_element(),
            );
        }
        self.timeline_posters(cx);
        out
    }

    /// One video clip: a strip of its pictures, trimmed at the ends.
    fn timeline_video_clip(
        &mut self,
        t: usize,
        c: usize,
        clip: &VideoClip,
        timeline: &Timeline,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (zoom, scroll) = (self.timeline_ui.zoom, self.timeline_ui.scroll);
        let width = self.timeline_lane_width();
        let x = clip.start as f32 * zoom - scroll;
        let w = clip.frames as f32 * zoom;
        if x + w < 0. || x > width {
            return None;
        }
        let rate = self.timeline_rate();
        let asset = timeline.videos.get(&clip.asset)?;
        let chosen = self.timeline_ui.video.clip == Some((t, c));
        let strip = self.timeline_strip(clip, asset, x, w, width, rate);
        let fill: Hsla = rgb(0x8E5BB5).into();
        let owner = cx.weak_entity();
        let menu_clip = clip.clone();
        let handle = |part: ClipPart, id: String| {
            div()
                .id(SharedString::from(id))
                .test_support()
                .absolute()
                .top_0()
                .bottom_0()
                .w(px(7.))
                .cursor(CursorStyle::ResizeLeftRight)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        this.timeline_video_begin((t, c), part, e, window, cx);
                        cx.stop_propagation();
                    }),
                )
        };
        let mut label = clip.name.clone();
        if clip.opacity < 1. {
            label.push_str(&format!(" · {:.0}%", clip.opacity * 100.));
        }
        if !clip.visible {
            label.push_str(" · hidden");
        }
        if clip.locked {
            label.push_str(" · locked");
        }
        Some(
            div()
                .id(SharedString::from(format!("timeline-video-clip-{t}-{c}")))
                .test_support()
                .absolute()
                .left(px(x))
                .top(px(3.))
                .w(px(w.max(2.)))
                .h(px(ROW_H - 7.))
                .rounded(px(3.))
                .overflow_hidden()
                .bg(fill.opacity(0.28))
                .border_1()
                .border_color(if chosen { p.accent } else { fill.opacity(0.8) })
                .when(!clip.visible, |d| d.opacity(0.5))
                .cursor(if clip.locked {
                    CursorStyle::Arrow
                } else {
                    CursorStyle::OpenHand
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        this.timeline_video_begin((t, c), ClipPart::Body, e, window, cx);
                        cx.stop_propagation();
                    }),
                )
                .context_menu(move |menu, _, _| {
                    Self::timeline_video_clip_menu(menu, owner.clone(), (t, c), menu_clip.clone())
                })
                .children(strip.into_iter().map(|(left, tw, image)| {
                    img(image)
                        .absolute()
                        .left(px(left))
                        .top_0()
                        .w(px(tw))
                        .h_full()
                        .object_fit(ObjectFit::Cover)
                }))
                .child(
                    div()
                        .absolute()
                        .left(px(4.))
                        .bottom(px(1.))
                        .px_1()
                        .rounded(px(2.))
                        .bg(gpui_kit::black().opacity(0.45))
                        .text_size(px(9.5))
                        .text_color(gpui_kit::white())
                        .whitespace_nowrap()
                        .child(label),
                )
                .child(handle(ClipPart::Start, format!("timeline-video-start-{t}-{c}")).left_0())
                .child(handle(ClipPart::End, format!("timeline-video-end-{t}-{c}")).right_0())
                .into_any_element(),
        )
    }

    /// The strip pictures of a clip at `x` (`w` wide) in a lane `width`
    /// wide: their left edges in the clip, widths and images. Pictures not
    /// decoded yet are queued.
    fn timeline_strip(
        &mut self,
        clip: &VideoClip,
        asset: &VideoAsset,
        x: f32,
        w: f32,
        width: f32,
        rate: FrameRate,
    ) -> Vec<(f32, f32, Arc<RenderImage>)> {
        let Some(source) = asset.source.clone() else {
            return Vec::new();
        };
        let h = ROW_H - 9.;
        let tw = (h * asset.width as f32 / asset.height.max(1) as f32).clamp(16., 160.);
        let size = decode::fit_size((asset.width, asset.height), (tw as u32 * 2, h as u32 * 2));
        let zoom = self.timeline_ui.zoom;
        let ui = &mut self.timeline_ui.video;
        let mut out = Vec::new();
        let mut left = 0.;
        while left < w {
            let on_screen = x + left + tw >= 0. && x + left <= width;
            if on_screen {
                let frame = clip.start + (left / zoom) as u64;
                let index = asset.frame_index(clip.source_ms(frame, rate));
                let key = (clip.asset, index, size);
                match ui.posters.get(&key) {
                    Some(image) => out.push((left, tw, image.clone())),
                    None if !ui.poster_failed.contains(&key) => {
                        ui.poster_queue.push((key, source.clone(), asset.fps))
                    }
                    None => {}
                }
            }
            left += tw;
        }
        out
    }

    /// Decode the next strip picture the timeline wants, one at a time.
    fn timeline_posters(&mut self, cx: &mut Context<Self>) {
        let ui = &mut self.timeline_ui.video;
        if ui.poster_busy {
            return;
        }
        let Some((key, source, fps)) = ui.poster_queue.first().cloned() else {
            return;
        };
        ui.poster_busy = true;
        cx.spawn(async move |this, cx| {
            let made = cx
                .background_spawn(async move {
                    decode::picture(&source, fps, key.1, key.2, 0).map(|p| Arc::new(rgba_image(&p)))
                })
                .await;
            this.update(cx, |this, cx| {
                let ui = &mut this.timeline_ui.video;
                ui.poster_busy = false;
                match made {
                    Ok(image) => {
                        ui.posters.insert(key, image);
                        ui.poster_order.push_back(key);
                        let mut dropped = Vec::new();
                        while ui.poster_order.len() > MAX_POSTERS {
                            if let Some(old) = ui.poster_order.pop_front()
                                && let Some(image) = ui.posters.remove(&old)
                            {
                                dropped.push(image);
                            }
                        }
                        drop_images(dropped, cx);
                    }
                    Err(_) => {
                        ui.poster_failed.insert(key);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::timeline::{FrameRate, VideoAsset};

    fn timeline() -> Timeline {
        let mut t = Timeline::default();
        let asset = VideoAsset {
            name: "Ref".into(),
            format: "mp4".into(),
            duration_ms: 2000,
            fps: 24.,
            width: 64,
            height: 48,
            has_audio: false,
            source: None,
        };
        t.import_video(asset.clone(), None, 10, FrameRate::whole(24), None)
            .unwrap();
        t.import_video(asset, None, 100, FrameRate::whole(24), Some(0))
            .unwrap();
        t
    }

    #[test]
    fn video_clips_move_and_trim_within_their_media_and_neighbours() {
        let rate = FrameRate::whole(24);
        let t = timeline();
        // Moving onto the next clip is refused; to another track is fine.
        assert!(video_clip_edit(&t, rate, (0, 0), ClipPart::Body, 90, 0).is_none());
        let moved = video_clip_edit(&t, rate, (0, 0), ClipPart::Body, 0, 0).unwrap();
        assert_eq!(moved.video[0].clips[0].start, 0);
        // Trimming the head keeps the picture in place.
        let trimmed = video_clip_edit(&t, rate, (0, 0), ClipPart::Start, 22, 0).unwrap();
        let clip = &trimmed.video[0].clips[0];
        assert_eq!((clip.start, clip.frames, clip.offset_ms), (22, 36, 500));
        // The tail stops at the end of the video and before the next clip.
        let tail = video_clip_edit(&t, rate, (0, 0), ClipPart::End, 500, 0).unwrap();
        assert_eq!(tail.video[0].clips[0].end(), 58);
        // Locked clips do not move.
        let mut locked = t.clone();
        locked.video[0].clips[0].locked = true;
        assert!(video_clip_edit(&locked, rate, (0, 0), ClipPart::Body, 0, 0).is_none());
    }

    #[test]
    fn canvas_places_the_picture_with_opacity() {
        let picture = Picture {
            width: 2,
            height: 2,
            rgba: [10, 20, 30, 255].repeat(4),
        };
        let out = reference_canvas(&picture, (2, 2), (4, 2), VideoPlacement::Overlay, 0.5);
        // Fitted in the middle, letterboxed at the sides; BGRA.
        assert_eq!(&out[..4], &[0, 0, 0, 0]);
        assert_eq!(&out[4..8], &[30, 20, 10, 128]);
    }

    /// A 30-picture clip whose picture `n` is grey `n * 8`.
    fn counter(dir: &std::path::Path) -> PathBuf {
        let path = dir.join("Ref take.mkv");
        let status = emulsion_io::ffmpeg::command("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("nullsrc=size=32x18:rate=24,format=gbrp,geq=r='N*8':g='N*8':b='N*8'")
            .args(["-frames:v", "30", "-c:v", "ffv1", "-pix_fmt", "gbrp"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        path
    }

    #[gpui_kit::test]
    fn videos_import_show_drag_and_delete_as_undo_steps(cx: &mut TestAppContext) {
        use crate::tests::open;
        use emulsion_core::project::{ProjectEditor, ProjectKind};
        use gpui_kit::test::TestWindowExt;
        if !emulsion_io::ffmpeg::available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = counter(dir.path());
        let (ws, cx) = open(cx, Document::new(64, 36));
        cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
        let mut project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        project
            .edit_storyboard(|b| {
                b.settings.frame_rate = FrameRate::whole(24);
                Ok(())
            })
            .unwrap();
        let e = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(project, "Board".into(), window, cx)
            });
            ws.read(cx).editor.clone().unwrap()
        });
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.timeline_ui.open = true;
                e.timeline_ui.zoom = 4.;
                e.timeline_import_video_files(vec![file.clone()], 6, None, false, cx);
            })
        });
        cx.run_until_parked();
        let video = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                e.read(cx)
                    .editor
                    .storyboard()
                    .unwrap()
                    .timeline
                    .video
                    .clone()
            })
        };
        let clips = video(cx);
        assert_eq!(clips[0].clips[0].start, 6);
        assert_eq!(clips[0].clips[0].name, "Ref take");
        // On the Timeline, and the picture under the playhead over the Stage.
        cx.update(|_, cx| e.update(cx, |e, cx| e.timeline_seek(16, cx)));
        for _ in 0..3 {
            cx.update(|window, cx| window.render_frame(cx));
            cx.run_until_parked();
        }
        cx.update(|window, cx| {
            assert!(window.find("timeline-video-clip-0-0").visible());
            let shown = e
                .read(cx)
                .timeline_ui
                .video
                .picture
                .as_ref()
                .map(|(k, _)| k.index);
            assert_eq!(shown, Some(10), "frame 16 is picture 10 of a clip at 6");
            assert!(e.read(cx).reference_video_stage().is_some());
        });
        // Hidden from the View menu, nothing shows.
        cx.update(|_, cx| e.update(cx, |e, cx| e.set_reference_video(None, cx)));
        cx.update(|_, cx| assert!(e.read(cx).reference_video_stage().is_none()));
        // A drag moves the clip by the pointer's travel: one Undo step.
        let from = point(px(400.), px(0.));
        cx.update(|window, cx| {
            e.update(cx, |e, cx| {
                let drag = TimelineDrag::Video {
                    track: 0,
                    index: 0,
                    part: ClipPart::Body,
                };
                let to = point(from.x + px(40.), from.y);
                e.timeline_begin(drag, from, Modifiers::secondary_key(), window, cx);
                e.timeline_move(to, Modifiers::secondary_key(), cx);
                e.timeline_end(cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(video(cx)[0].clips[0].start, 16);
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert_eq!(video(cx)[0].clips[0].start, 6);
        // Locked clips refuse deletion; unlocked, Delete removes the clip
        // and its video.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.timeline_video_clip_edit((0, 0), |c| c.locked = true, cx);
                e.timeline_ui.video.clip = Some((0, 0));
                e.timeline_delete_selected(cx);
            })
        });
        assert_eq!(video(cx)[0].clips.len(), 1);
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.timeline_video_clip_edit((0, 0), |c| c.locked = false, cx);
                e.timeline_ui.video.clip = Some((0, 0));
                e.timeline_delete_selected(cx);
            })
        });
        let timeline = cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().timeline.clone());
        assert!(timeline.video[0].clips.is_empty() && timeline.videos.is_empty());
    }
}
