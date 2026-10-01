//! The storyboard's sound library, beside the Timeline: sounds in folders
//! (import, create and rename folders, rename and move sounds, delete
//! unused ones) and a clip preview with a zoomable waveform, draggable in
//! and out points and "Place on track". Sounds are dragged from the list or
//! the preview onto an audio track. Every change is one Undo step through
//! the timeline's audio edit; waveforms come from `emulsion_io::audio`.
use super::storyboard_timeline::{TimelineDrag, frame_label};
use super::*;
use crate::widgets::TrackBounds;
use emulsion_core::timeline::{AudioClip, AudioTrack, audio::AssetId};
use emulsion_io::audio::waveform::{self as wave, State};
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{PopupMenu, PopupMenuItem},
};
use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};

const LIBRARY_W: f32 = 300.;
const PREVIEW_H: f32 = 64.;

pub(crate) struct LibraryUi {
    pub(crate) open: bool,
    pub(crate) selected: Option<AssetId>,
    /// Where imported sounds and new folders go.
    pub(crate) folder: String,
    /// Folders made here that hold no sound yet (folders live on sounds).
    pub(crate) folders: Vec<String>,
    pub(crate) collapsed: HashSet<String>,
    /// Preview zoom: 1 shows the whole sound.
    pub(crate) zoom: f32,
    /// The preview's left edge, in milliseconds.
    pub(crate) view_ms: f64,
    pub(crate) in_ms: u64,
    pub(crate) out_ms: u64,
    pub(crate) bounds: TrackBounds,
    /// A repaint is scheduled while waveforms decode.
    polling: bool,
}

impl Default for LibraryUi {
    fn default() -> Self {
        Self {
            open: false,
            selected: None,
            folder: String::new(),
            folders: Vec::new(),
            collapsed: HashSet::new(),
            zoom: 1.,
            view_ms: 0.,
            in_ms: 0,
            out_ms: 0,
            bounds: Rc::new(Cell::new(None)),
            polling: false,
        }
    }
}

/// A sound (or the previewed part of it) being dragged onto a track.
#[derive(Clone)]
pub(crate) struct DraggedSound {
    pub(crate) asset: AssetId,
    pub(crate) name: String,
    pub(crate) in_ms: u64,
    pub(crate) out_ms: u64,
}

impl Render for DraggedSound {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .px_2()
            .py_1()
            .bg(p.panel)
            .text_color(p.ink)
            .text_size(px(11.))
            .border_1()
            .border_color(p.accent)
            .rounded(px(4.))
            .child(format!("♪ {}", self.name))
    }
}

/// Peaks drawn as a mirrored bar per pixel column; flat while decoding.
pub(crate) fn waveform(peaks: Option<Vec<[f32; 2]>>, color: Hsla) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let (x0, y0) = (bounds.origin.x, bounds.origin.y);
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let mid = h / 2.;
            let Some(peaks) = &peaks else {
                window.paint_quad(fill(
                    Bounds::new(point(x0, y0 + px(mid)), size(px(w), px(1.))),
                    color.opacity(0.5),
                ));
                return;
            };
            let n = peaks.len().max(1);
            let col = w / n as f32;
            for (i, [lo, hi]) in peaks.iter().enumerate() {
                let top = mid - hi.clamp(-1., 1.) * mid * 0.9;
                let bottom = (mid - lo.clamp(-1., 1.) * mid * 0.9).max(top + 1.);
                window.paint_quad(fill(
                    Bounds::new(
                        point(x0 + px(i as f32 * col), y0 + px(top)),
                        size(px(col.max(1.)), px(bottom - top)),
                    ),
                    color.opacity(0.75),
                ));
            }
        },
    )
}

fn valid_folder(name: &str) -> Result<String, String> {
    let name = name.trim().trim_matches('/').to_string();
    if name.is_empty()
        || name.chars().count() > 400
        || name.chars().any(char::is_control)
        || name.split('/').any(|p| p.trim().is_empty() || p == "..")
    {
        return Err("Folder names are 1–400 characters; use / between nested folders.".into());
    }
    Ok(name)
}

/// `folder` with the leading `old` replaced by `new`, when inside `old`.
fn renamed_folder(folder: &str, old: &str, new: &str) -> Option<String> {
    if folder == old {
        Some(new.to_string())
    } else {
        folder
            .strip_prefix(old)
            .and_then(|rest| rest.strip_prefix('/'))
            .map(|rest| format!("{new}/{rest}"))
    }
}

fn menu_item(
    owner: &WeakEntity<EditorView>,
    label: impl Into<SharedString>,
    run: impl Fn(&mut EditorView, &mut Window, &mut Context<EditorView>) + 'static,
) -> PopupMenuItem {
    let owner = owner.clone();
    PopupMenuItem::new(label).on_click(move |_, window, cx| {
        owner.update(cx, |e, cx| run(e, window, cx)).ok();
    })
}

fn duration_text(ms: u64) -> String {
    format!("{}:{:04.1}", ms / 60_000, (ms % 60_000) as f64 / 1000.)
}

impl EditorView {
    /// Waveform peaks of a sound from `start_ms` to `end_ms` in `buckets`
    /// columns, or `None` while it decodes (a repaint is scheduled).
    pub(crate) fn sound_peaks(
        &mut self,
        asset: AssetId,
        (start_ms, end_ms): (f64, f64),
        buckets: usize,
        cx: &mut Context<Self>,
    ) -> Option<Vec<[f32; 2]>> {
        let source = self
            .editor
            .storyboard()?
            .timeline
            .assets
            .get(&asset)?
            .source
            .clone()?;
        match wave::waveform(&source) {
            State::Ready(w) => Some(w.peaks(start_ms, end_ms, buckets.clamp(1, 4096))),
            State::Failed(_) => None,
            State::Pending => {
                if !self.timeline_ui.library.polling {
                    self.timeline_ui.library.polling = true;
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(150))
                            .await;
                        this.update(cx, |this, cx| {
                            this.timeline_ui.library.polling = false;
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                }
                None
            }
        }
    }

    pub(crate) fn library_select_sound(&mut self, asset: AssetId, cx: &mut Context<Self>) {
        let Some(sound) = self
            .editor
            .storyboard()
            .and_then(|b| b.timeline.assets.get(&asset))
        else {
            return;
        };
        let lib = &mut self.timeline_ui.library;
        lib.folder = sound.folder.clone();
        lib.selected = Some(asset);
        lib.in_ms = 0;
        lib.out_ms = sound.duration_ms;
        lib.zoom = 1.;
        lib.view_ms = 0.;
        cx.notify();
    }

    /// The preview's visible span in milliseconds.
    fn preview_span(&self) -> Option<(f64, f64)> {
        let lib = &self.timeline_ui.library;
        let duration = self
            .editor
            .storyboard()?
            .timeline
            .assets
            .get(&lib.selected?)?
            .duration_ms
            .max(1) as f64;
        let len = duration / f64::from(lib.zoom.max(1.));
        let start = lib.view_ms.clamp(0., duration - len);
        Some((start, start + len))
    }

    /// Drag the preview's in or out point to window x `x`.
    pub(crate) fn library_preview_drag(&mut self, out: bool, x: Pixels) {
        let Some((start, end)) = self.preview_span() else {
            return;
        };
        let Some(fraction) = crate::widgets::track_fraction(&self.timeline_ui.library.bounds, x)
        else {
            return;
        };
        let ms = (start + (end - start) * f64::from(fraction)).round() as u64;
        let lib = &mut self.timeline_ui.library;
        if out {
            lib.out_ms = ms.max(lib.in_ms + 1);
        } else {
            lib.in_ms = ms.min(lib.out_ms.saturating_sub(1));
        }
    }

    pub(crate) fn library_zoom(&mut self, factor: f32, cx: &mut Context<Self>) {
        let Some((start, end)) = self.preview_span() else {
            return;
        };
        let lib = &mut self.timeline_ui.library;
        let centre = (start + end) / 2.;
        lib.zoom = (lib.zoom * factor).clamp(1., 256.);
        let len = (end - start) * f64::from(factor.recip());
        lib.view_ms = (centre - len / 2.).max(0.);
        cx.notify();
    }

    /// Place `in_ms`–`out_ms` of a sound on `track` (the selected track or
    /// the first, made if there is none) at `start`, as one Undo step.
    pub(crate) fn timeline_place_sound(
        &mut self,
        asset: AssetId,
        track: Option<usize>,
        start: u64,
        (in_ms, out_ms): (u64, u64),
        cx: &mut Context<Self>,
    ) -> bool {
        let track = track.or(self.timeline_ui.track).unwrap_or(0);
        let mut placed = None;
        let ok = self.timeline_audio_edit(
            |t, rate| {
                let sound = t
                    .assets
                    .get(&asset)
                    .ok_or("That sound is not in the library.")?;
                let out_ms = out_ms.min(sound.duration_ms);
                if in_ms >= out_ms {
                    return Err("Choose an out point after the in point.".into());
                }
                let clip = AudioClip {
                    asset,
                    name: sound.name.clone(),
                    start,
                    frames: rate
                        .seconds_to_frames((out_ms - in_ms) as f64 / 1000.)
                        .max(1),
                    offset_ms: in_ms,
                    gain_db: 0.,
                    fade_in: 0,
                    fade_out: 0,
                };
                if t.tracks.is_empty() {
                    t.tracks.push(AudioTrack::new("Audio 1"));
                }
                t.place(track, clip)?;
                placed = t.tracks[track].clips.iter().position(|c| c.start == start);
                Ok(())
            },
            cx,
        );
        if let Some(index) = placed.filter(|_| ok) {
            self.timeline_ui.track = Some(track);
            self.timeline_ui.clip = Some((track, index));
        }
        ok
    }

    /// A sound dropped on a track lane at window x `x`.
    pub(crate) fn timeline_drop_sound(
        &mut self,
        sound: &DraggedSound,
        track: usize,
        x: Pixels,
        cx: &mut Context<Self>,
    ) {
        let start = self.timeline_frame_at(x).round() as u64;
        self.timeline_place_sound(
            sound.asset,
            Some(track),
            start,
            (sound.in_ms, sound.out_ms),
            cx,
        );
    }

    /// Choose sound files and add them to the library (in the current
    /// folder) as one Undo step.
    pub(crate) fn library_import(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import sounds".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            this.update(cx, |this, cx| this.library_import_files(paths, cx))
                .ok();
        })
        .detach();
    }

    /// Import sound files in the background, then add them.
    pub(crate) fn library_import_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() || paths.is_empty() {
            return;
        }
        let folder = self.timeline_ui.library.folder.clone();
        self.set_status("Importing sounds…", false, cx);
        cx.spawn(async move |this, cx| {
            let imported = cx
                .background_spawn(async move {
                    paths
                        .iter()
                        .map(|p| {
                            emulsion_io::audio::store::import(p, &folder).map_err(|e| e.to_string())
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| this.library_add_assets(imported, cx))
                .ok();
        })
        .detach();
    }

    pub(crate) fn library_add_assets(
        &mut self,
        imported: Vec<Result<emulsion_core::timeline::AudioAsset, String>>,
        cx: &mut Context<Self>,
    ) {
        let mut errors = Vec::new();
        let assets: Vec<_> = imported
            .into_iter()
            .filter_map(|r| r.map_err(|e| errors.push(e)).ok())
            .collect();
        let mut last = None;
        let count = assets.len();
        if count > 0
            && self.timeline_audio_edit(
                |t, _| {
                    for asset in assets {
                        last = Some(t.add_asset(asset)?);
                    }
                    Ok(())
                },
                cx,
            )
        {
            let folder = self.timeline_ui.library.folder.clone();
            self.timeline_ui.library.folders.retain(|f| *f != folder);
            if let Some(id) = last {
                self.library_select_sound(id, cx);
            }
        }
        match errors.first() {
            Some(error) => self.set_status(error.clone(), true, cx),
            None if count > 0 => self.set_status(
                format!(
                    "Imported {count} sound{}.",
                    if count == 1 { "" } else { "s" }
                ),
                false,
                cx,
            ),
            None => {}
        }
    }

    pub(crate) fn library_new_folder(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let parent = self.timeline_ui.library.folder.clone();
        let name = match valid_folder(name) {
            Ok(n) if parent.is_empty() => n,
            Ok(n) => format!("{parent}/{n}"),
            Err(e) => {
                self.set_status(e, true, cx);
                return false;
            }
        };
        let exists = self.library_folders().contains(&name);
        if !exists {
            self.timeline_ui.library.folders.push(name.clone());
        }
        self.timeline_ui.library.folder = name;
        cx.notify();
        true
    }

    pub(crate) fn library_rename_folder(
        &mut self,
        old: &str,
        new: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let new = match valid_folder(new) {
            Ok(n) => n,
            Err(e) => {
                self.set_status(e, true, cx);
                return false;
            }
        };
        let used = self.editor.storyboard().is_some_and(|b| {
            b.timeline
                .assets
                .values()
                .any(|a| renamed_folder(&a.folder, old, &new).is_some())
        });
        if used
            && !self.timeline_audio_edit(
                |t, _| {
                    for asset in t.assets.values_mut() {
                        if let Some(f) = renamed_folder(&asset.folder, old, &new) {
                            asset.folder = f;
                        }
                    }
                    Ok(())
                },
                cx,
            )
        {
            return false;
        }
        let lib = &mut self.timeline_ui.library;
        for folder in lib
            .folders
            .iter_mut()
            .chain(std::iter::once(&mut lib.folder))
        {
            if let Some(f) = renamed_folder(folder, old, &new) {
                *folder = f;
            }
        }
        cx.notify();
        true
    }

    pub(crate) fn library_rename_sound(
        &mut self,
        asset: AssetId,
        name: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = name.trim().to_string();
        self.timeline_audio_edit(
            move |t, _| {
                t.assets
                    .get_mut(&asset)
                    .ok_or("That sound is not in the library.")?
                    .name = name;
                Ok(())
            },
            cx,
        )
    }

    pub(crate) fn library_move_sound(
        &mut self,
        asset: AssetId,
        folder: String,
        cx: &mut Context<Self>,
    ) -> bool {
        self.timeline_audio_edit(
            move |t, _| {
                t.assets
                    .get_mut(&asset)
                    .ok_or("That sound is not in the library.")?
                    .folder = folder;
                Ok(())
            },
            cx,
        )
    }

    /// Delete a sound no clip uses.
    pub(crate) fn library_delete_sound(&mut self, asset: AssetId, cx: &mut Context<Self>) -> bool {
        let ok = self.timeline_audio_edit(
            |t, _| {
                if t.tracks
                    .iter()
                    .any(|tr| tr.clips.iter().any(|c| c.asset == asset))
                {
                    return Err("A clip uses this sound. Delete its clips first.".into());
                }
                t.assets
                    .remove(&asset)
                    .ok_or("That sound is not in the library.")?;
                Ok(())
            },
            cx,
        );
        if ok && self.timeline_ui.library.selected == Some(asset) {
            self.timeline_ui.library.selected = None;
        }
        ok
    }

    pub(crate) fn library_delete_unused(&mut self, cx: &mut Context<Self>) {
        let mut removed = 0;
        self.timeline_audio_edit(
            |t, _| {
                removed = t.remove_unused_assets();
                Ok(())
            },
            cx,
        );
        if self.timeline_ui.library.selected.is_some_and(|id| {
            self.editor
                .storyboard()
                .is_some_and(|b| !b.timeline.assets.contains_key(&id))
        }) {
            self.timeline_ui.library.selected = None;
        }
        self.set_status(
            format!(
                "Removed {removed} unused sound{}.",
                if removed == 1 { "" } else { "s" }
            ),
            false,
            cx,
        );
    }

    /// Every folder: those holding sounds, their parents, and new ones.
    pub(crate) fn library_folders(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .editor
            .storyboard()
            .map(|b| {
                b.timeline
                    .assets
                    .values()
                    .map(|a| a.folder.clone())
                    .collect()
            })
            .unwrap_or_default();
        out.extend(self.timeline_ui.library.folders.iter().cloned());
        let mut all = Vec::new();
        for folder in out.into_iter().filter(|f| !f.is_empty()) {
            let parts: Vec<_> = folder.split('/').collect();
            for n in 1..=parts.len() {
                all.push(parts[..n].join("/"));
            }
        }
        all.sort();
        all.dedup();
        all
    }

    #[allow(clippy::too_many_arguments)]
    fn library_sound_menu(
        menu: PopupMenu,
        owner: WeakEntity<Self>,
        asset: AssetId,
        name: String,
        folders: Vec<String>,
        used: bool,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let move_owner = owner.clone();
        menu.item(menu_item(
            &owner,
            "Rename sound…",
            move |e, window, cx| {
                e.timeline_text_dialog(
                    "Rename sound",
                    "Name",
                    name.clone(),
                    "Rename",
                    move |this, text, cx| this.library_rename_sound(asset, &text, cx),
                    window,
                    cx,
                )
            },
        ))
        .submenu("Move to folder", window, cx, move |mut sub, _, _| {
            sub = sub.item(menu_item(&move_owner, "Top level", move |e, _, cx| {
                e.library_move_sound(asset, String::new(), cx);
            }));
            for folder in &folders {
                let f = folder.clone();
                sub = sub.item(menu_item(&move_owner, folder.clone(), move |e, _, cx| {
                    e.library_move_sound(asset, f.clone(), cx);
                }));
            }
            sub
        })
        .item(menu_item(&owner, "Place at playhead", move |e, _, cx| {
            let start = e.transport.frame;
            let len = e
                .editor
                .storyboard()
                .and_then(|b| b.timeline.assets.get(&asset))
                .map_or(0, |a| a.duration_ms);
            e.timeline_place_sound(asset, None, start, (0, len), cx);
        }))
        .separator()
        .item(
            menu_item(
                &owner,
                if used {
                    "Delete (used by clips)"
                } else {
                    "Delete sound"
                },
                move |e, _, cx| {
                    e.library_delete_sound(asset, cx);
                },
            )
            .disabled(used),
        )
    }

    /// The library panel beside the Timeline's tracks.
    pub(crate) fn timeline_library(
        &mut self,
        p: &Palette,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(board) = self.editor.storyboard() else {
            return div().into_any_element();
        };
        let used: HashSet<AssetId> = board
            .timeline
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.asset))
            .collect();
        let mut by_folder: BTreeMap<String, Vec<(AssetId, String, u64)>> = BTreeMap::new();
        for (id, a) in &board.timeline.assets {
            by_folder.entry(a.folder.clone()).or_default().push((
                *id,
                a.name.clone(),
                a.duration_ms,
            ));
        }
        let folders = self.library_folders();
        let lib = &self.timeline_ui.library;
        let current = lib.folder.clone();
        let selected = lib.selected;
        let owner = cx.weak_entity();
        let mut list = div()
            .id("timeline-library-list")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .text_size(px(11.));
        let sound_row =
            |id: AssetId, name: String, ms: u64, depth: usize, cx: &mut Context<Self>| {
                let menu_owner = owner.clone();
                let menu_folders = folders.clone();
                let menu_name = name.clone();
                let is_used = used.contains(&id);
                let dragged = DraggedSound {
                    asset: id,
                    name: name.clone(),
                    in_ms: 0,
                    out_ms: ms,
                };
                div()
                    .id(("library-sound", id))
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl(px(8. + depth as f32 * 12.))
                    .pr_2()
                    .py(px(2.))
                    .when(selected == Some(id), |r| {
                        r.bg(p.soft_bg).text_color(p.accent)
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.library_select_sound(id, cx)))
                    .on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
                    .context_menu(move |menu, window, cx| {
                        Self::library_sound_menu(
                            menu,
                            menu_owner.clone(),
                            id,
                            menu_name.clone(),
                            menu_folders.clone(),
                            is_used,
                            window,
                            cx,
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(format!("♪ {name}")),
                    )
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(px(10.))
                            .text_color(p.muted)
                            .child(duration_text(ms)),
                    )
            };
        for (id, name, ms) in by_folder.get("").cloned().unwrap_or_default() {
            list = list.child(sound_row(id, name, ms, 0, cx));
        }
        for folder in &folders {
            // Hidden inside a collapsed parent.
            let hidden = self
                .timeline_ui
                .library
                .collapsed
                .iter()
                .any(|c| folder.starts_with(&format!("{c}/")));
            if hidden {
                continue;
            }
            let depth = folder.matches('/').count();
            let collapsed = self.timeline_ui.library.collapsed.contains(folder);
            let label = folder.rsplit('/').next().unwrap_or(folder).to_string();
            let f = folder.clone();
            let menu_f = folder.clone();
            let menu_owner = owner.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("library-folder-{folder}")))
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl(px(4. + depth as f32 * 12.))
                    .py(px(2.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(current == *folder, |r| r.text_color(p.accent))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                        let lib = &mut this.timeline_ui.library;
                        if lib.folder == f && e.click_count() == 1 && !lib.collapsed.remove(&f) {
                            lib.collapsed.insert(f.clone());
                        }
                        lib.folder = f.clone();
                        cx.notify();
                    }))
                    .context_menu(move |menu, _, _| {
                        let old = menu_f.clone();
                        menu.item(menu_item(
                            &menu_owner,
                            "Rename folder…",
                            move |e, window, cx| {
                                let old = old.clone();
                                let current = old.rsplit('/').next().unwrap_or(&old).to_string();
                                e.timeline_text_dialog(
                                    "Rename folder",
                                    "Name",
                                    current,
                                    "Rename",
                                    move |this, text, cx| {
                                        let parent =
                                            old.rsplit_once('/').map(|(p, _)| p.to_string());
                                        let new = match parent {
                                            Some(parent) => format!("{parent}/{}", text.trim()),
                                            None => text,
                                        };
                                        this.library_rename_folder(&old, &new, cx)
                                    },
                                    window,
                                    cx,
                                )
                            },
                        ))
                    })
                    .child(if collapsed { "▸" } else { "▾" })
                    .child(format!("{label}/")),
            );
            if !collapsed {
                for (id, name, ms) in by_folder.get(folder).cloned().unwrap_or_default() {
                    list = list.child(sound_row(id, name, ms, depth + 1, cx));
                }
            }
        }
        if board.timeline.assets.is_empty() {
            list = list.child(
                div()
                    .p_2()
                    .text_color(p.muted)
                    .child("Import sounds, then drag them onto an audio track."),
            );
        }
        let preview = self.library_preview(p, cx);
        let folder_label = if current.is_empty() {
            "Top level".to_string()
        } else {
            current.clone()
        };
        div()
            .id("timeline-library")
            .test_support()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(LIBRARY_W))
            .border_l_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        Button::new("library-import")
                            .label("Import…")
                            .tooltip("Add sound files to the library")
                            .xsmall()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| this.library_import(cx))),
                    )
                    .child(
                        Button::new("library-new-folder")
                            .label("New folder")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.timeline_text_dialog(
                                    "New folder",
                                    "Name (inside the current folder)",
                                    String::new(),
                                    "Create",
                                    |this, text, cx| this.library_new_folder(&text, cx),
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new("library-top-level")
                            .label("Top level")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.timeline_ui.library.folder.clear();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("library-delete-unused")
                            .label("Delete unused")
                            .tooltip("Remove sounds no clip uses")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.library_delete_unused(cx))),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(p.muted)
                            .child(format!("Into: {folder_label}")),
                    ),
            )
            .child(list)
            .children(preview)
            .into_any_element()
    }

    /// The clip preview: zoomable waveform with in and out points.
    fn library_preview(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let asset_id = self.timeline_ui.library.selected?;
        let rate = self.timeline_rate();
        let (name, _duration) = self
            .editor
            .storyboard()?
            .timeline
            .assets
            .get(&asset_id)
            .map(|a| (a.name.clone(), a.duration_ms))?;
        let (start, end) = self.preview_span()?;
        let width = self
            .timeline_ui
            .library
            .bounds
            .get()
            .map_or(LIBRARY_W - 16., |b| f32::from(b.size.width));
        let peaks = self.sound_peaks(asset_id, (start, end), width.round() as usize, cx);
        let lib = &self.timeline_ui.library;
        let (in_ms, out_ms) = (lib.in_ms, lib.out_ms);
        let frac = |ms: u64| ((ms as f64 - start) / (end - start)).clamp(0., 1.) as f32;
        let bounds = lib.bounds.clone();
        let point_handle = |out: bool, cx: &mut Context<Self>| {
            let at = frac(if out { out_ms } else { in_ms });
            div()
                .id(if out {
                    "library-preview-out"
                } else {
                    "library-preview-in"
                })
                .test_support()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(at))
                .ml(px(-4.))
                .w(px(8.))
                .cursor(CursorStyle::ResizeLeftRight)
                .child(
                    div()
                        .absolute()
                        .left(px(3.))
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(p.accent),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        this.timeline_begin(
                            TimelineDrag::PreviewPoint { out },
                            e.position,
                            e.modifiers,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }),
                )
        };
        let frames = rate.seconds_to_frames(out_ms.saturating_sub(in_ms) as f64 / 1000.);
        let dragged = DraggedSound {
            asset: asset_id,
            name: name.clone(),
            in_ms,
            out_ms,
        };
        Some(
            div()
                .id("library-preview")
                .test_support()
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .border_t_1()
                .border_color(p.line)
                .text_size(px(10.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(name),
                        )
                        .child(
                            Button::new("library-zoom-out")
                                .label("−")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| this.library_zoom(0.5, cx))),
                        )
                        .child(
                            Button::new("library-zoom-in")
                                .label("+")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| this.library_zoom(2., cx))),
                        ),
                )
                .child(
                    div()
                        .id("library-preview-wave")
                        .relative()
                        .h(px(PREVIEW_H))
                        .bg(p.soft_bg)
                        .overflow_hidden()
                        .on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
                        .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                            let dy = f32::from(e.delta.pixel_delta(px(16.)).y);
                            if e.modifiers.secondary() {
                                this.library_zoom(if dy > 0. { 1.25 } else { 0.8 }, cx);
                            } else if let Some((start, end)) = this.preview_span() {
                                let lib = &mut this.timeline_ui.library;
                                lib.view_ms =
                                    (start - f64::from(dy) / 200. * (end - start)).max(0.);
                                cx.notify();
                            }
                        }))
                        .child(
                            canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
                                .absolute()
                                .size_full(),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(relative(frac(in_ms)))
                                .w(relative((frac(out_ms) - frac(in_ms)).max(0.)))
                                .bg(p.accent.opacity(0.12)),
                        )
                        .child(waveform(peaks, rgb(0x3E7CB1).into()).absolute().size_full())
                        .child(point_handle(false, cx))
                        .child(point_handle(true, cx)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .font_family(MONO_FONT)
                                .text_color(p.muted)
                                .child(format!(
                                    "In {} · Out {} · {}",
                                    duration_text(in_ms),
                                    duration_text(out_ms),
                                    frame_label(rate, frames, true)
                                )),
                        )
                        .child(
                            Button::new("library-place")
                                .label("Place on track")
                                .tooltip(
                                    "Place the in–out part at the playhead on the selected track",
                                )
                                .xsmall()
                                .outline()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let start = this.transport.frame;
                                    this.timeline_place_sound(
                                        asset_id,
                                        None,
                                        start,
                                        (in_ms, out_ms),
                                        cx,
                                    );
                                })),
                        ),
                )
                .into_any_element(),
        )
    }
}
