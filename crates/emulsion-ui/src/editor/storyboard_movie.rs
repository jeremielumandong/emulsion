//! File → Export Movie… and Export Animated GIF… for storyboards: format,
//! size, frame range, render area, burn-in and sound, then the export runs
//! off the UI thread from a snapshot with progress and Cancel.
use super::*;
use crate::file_prompt::FilePrompts;
use emulsion_core::project::{PageId, Project};
use emulsion_core::storyboard_animatic::{BurnIn, BurnInPosition, RenderArea};
use emulsion_io::storyboard_export::{
    self as story,
    movie::{self, GifOptions, MovieFormat, MovieOptions},
};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

/// A menu entry: its label and what it sets.
type Choice = (String, Rc<dyn Fn(&mut MovieExport)>);

fn choice(label: impl Into<String>, apply: impl Fn(&mut MovieExport) + 'static) -> Choice {
    (label.into(), Rc::new(apply))
}

/// Which export the dialog makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Movie,
    Gif,
}

/// Frames to export, chosen from the board.
#[derive(Clone, Debug, PartialEq)]
struct Range {
    label: String,
    start: u64,
    end: Option<u64>,
}

/// The movie / GIF export dialog.
pub(crate) struct MovieExport {
    kind: Kind,
    project: Arc<Project>,
    name: String,
    movie: MovieOptions,
    gif: GifOptions,
    range: Range,
    /// Ranges offered: the whole animatic, the selected panels, each scene.
    ranges: Vec<Range>,
    captions: Vec<String>,
    busy: bool,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    message: Option<String>,
}

impl MovieExport {
    fn new(kind: Kind, project: Project, name: String, selection: &[PageId]) -> Self {
        let mut ranges = vec![Range {
            label: "Whole animatic".into(),
            start: 0,
            end: None,
        }];
        let entries = story::entries(&project).unwrap_or_default();
        let span = |chosen: Vec<&story::Entry>| {
            let first = chosen.iter().map(|e| e.start).min()?;
            let last = chosen.iter().map(|e| e.start + e.length()).max()?;
            (last > first).then_some((first, last))
        };
        if let Some((start, end)) = span(
            entries
                .iter()
                .filter(|e| selection.contains(&e.page))
                .collect(),
        ) {
            ranges.push(Range {
                label: format!("Selected panels ({})", selection.len()),
                start,
                end: Some(end),
            });
        }
        let mut scenes: Vec<(u64, String)> = Vec::new();
        for e in &entries {
            if !scenes.iter().any(|(id, _)| *id == e.scene_id) {
                scenes.push((e.scene_id, e.scene.clone()));
            }
        }
        for (id, scene) in scenes {
            if let Some((start, end)) = span(entries.iter().filter(|e| e.scene_id == id).collect())
            {
                ranges.push(Range {
                    label: format!("Scene {scene}"),
                    start,
                    end: Some(end),
                });
            }
        }
        let captions = story::board(&project)
            .map(|b| b.captions.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        Self {
            kind,
            project: Arc::new(project),
            name,
            movie: MovieOptions {
                width: 1280,
                ..MovieOptions::default()
            },
            gif: GifOptions::default(),
            range: ranges[0].clone(),
            ranges,
            captions,
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(0)),
            message: None,
        }
    }

    fn area(&mut self) -> &mut RenderArea {
        match self.kind {
            Kind::Movie => &mut self.movie.area,
            Kind::Gif => &mut self.gif.area,
        }
    }

    fn burn_in(&mut self) -> &mut Option<BurnIn> {
        match self.kind {
            Kind::Movie => &mut self.movie.burn_in,
            Kind::Gif => &mut self.gif.burn_in,
        }
    }

    /// The frames, size and length the export will have, or why it cannot
    /// run.
    fn summary(&self) -> anyhow::Result<String> {
        let board = story::board(&self.project)?;
        let rate = board.settings.frame_rate;
        let (area, width, even) = match self.kind {
            Kind::Movie => (
                self.movie.area,
                self.movie.width,
                self.movie.format != MovieFormat::PngSequence,
            ),
            Kind::Gif => (self.gif.area, self.gif.width, false),
        };
        let rect = movie::area_rect(&self.project, area)?;
        let (w, h) = movie::output_size(rect, width, even);
        let layout: Vec<_> = self.project.pages.iter().map(|p| p.meta.id).collect();
        let total = board.animatic_frames(&layout);
        let end = self.range.end.unwrap_or(total);
        if total == 0 || self.range.start >= end {
            anyhow::bail!("The storyboard has no panels that play")
        }
        let frames = end - self.range.start;
        let seconds = rate.frames_to_seconds(frames);
        Ok(format!(
            "{w} × {h} · {} ({frames} frames, {seconds:.1} s)",
            rate.timecode(frames)
        ))
    }

    /// Ask for the destination, then export there.
    fn choose_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.kind == Kind::Movie && self.movie.format == MovieFormat::PngSequence;
        if folder {
            let request = cx.prompt_open_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some("Export the frames into this folder".into()),
            });
            cx.spawn_in(window, async move |this, cx| {
                if let Ok(Ok(Some(paths))) = request.await
                    && let Some(dir) = paths.into_iter().next()
                {
                    this.update(cx, |this, cx| this.export_to(dir, cx)).ok();
                }
            })
            .detach();
            return;
        }
        let extension = match self.kind {
            Kind::Movie => self.movie.format.extension().unwrap_or("mp4"),
            Kind::Gif => "gif",
        };
        let request = cx.prompt_save_path(
            &std::env::current_dir().unwrap_or_default(),
            Some(&format!("{}.{extension}", self.name)),
        );
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut path))) = request.await {
                path.set_extension(extension);
                this.update(cx, |this, cx| this.export_to(path, cx)).ok();
            }
        })
        .detach();
    }

    /// Export to `path` on a worker, showing progress until it ends.
    pub(crate) fn export_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let (start, end) = (self.range.start, self.range.end);
        let (kind, project) = (self.kind, self.project.clone());
        let (mut movie, mut gif) = (self.movie.clone(), self.gif.clone());
        (movie.start, movie.end, gif.start, gif.end) = (start, end, start, end);
        let (cancel, done, total) = (self.cancel.clone(), self.done.clone(), self.total.clone());
        cancel.store(false, Ordering::Relaxed);
        done.store(0, Ordering::Relaxed);
        total.store(0, Ordering::Relaxed);
        self.busy = true;
        self.message = Some("Exporting…".into());
        cx.notify();
        let task = cx.background_spawn(async move {
            let mut progress = |d: u64, t: u64| {
                done.store(d, Ordering::Relaxed);
                total.store(t, Ordering::Relaxed);
            };
            match kind {
                Kind::Movie => movie::write_movie(&project, &movie, &path, &mut progress, &cancel),
                Kind::Gif => movie::write_gif(&project, &gif, &path, &mut progress, &cancel),
            }
            .map(|report| (report, path))
        });
        // Repaint the progress while the worker runs.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let busy = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.busy
                    })
                    .unwrap_or(false);
                if !busy {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                this.message = Some(match result {
                    Ok((report, path)) => format!(
                        "Exported {} frames ({:.1} s{}) to {}",
                        report.frames,
                        report.seconds,
                        if report.audio { ", with sound" } else { "" },
                        path.display()
                    ),
                    Err(e) => format!("Export failed: {e}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn menu(
        &self,
        id: &'static str,
        label: String,
        choices: Vec<Choice>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let owner = cx.weak_entity();
        Button::new(id)
            .label(label)
            .small()
            .outline()
            .dropdown_caret(true)
            .disabled(self.busy)
            .dropdown_menu(move |mut menu, _, _| {
                for (name, apply) in &choices {
                    let (owner, apply) = (owner.clone(), apply.clone());
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(move |_, _, cx| {
                        owner
                            .update(cx, |this, cx| {
                                apply(this);
                                cx.notify();
                            })
                            .ok();
                    }));
                }
                menu
            })
            .into_any_element()
    }

    fn burn_in_label(burn: &Option<BurnIn>) -> String {
        let Some(b) = burn else {
            return "No burn-in".into();
        };
        let mut parts = Vec::new();
        if b.scene {
            parts.push("scene".to_string());
        }
        if b.panel {
            parts.push("panel".into());
        }
        if b.timecode {
            parts.push("timecode".into());
        }
        if let Some(c) = &b.caption {
            parts.push(c.to_lowercase());
        }
        let place = match b.position {
            BurnInPosition::Top => "top",
            BurnInPosition::Bottom => "bottom",
        };
        format!("Burn-in: {} · {place}", parts.join(", "))
    }

    fn option_rows(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        if self.kind == Kind::Movie {
            rows.push(
                self.menu(
                    "storyboard-movie-format",
                    self.movie.format.label().into(),
                    MovieFormat::ALL
                        .into_iter()
                        .map(|f| choice(f.label(), move |s| s.movie.format = f))
                        .collect(),
                    cx,
                ),
            );
        }
        let (width, widths): (u32, &[u32]) = match self.kind {
            Kind::Movie => (self.movie.width, &[0, 3840, 1920, 1280, 960, 640]),
            Kind::Gif => (self.gif.width, &[1280, 960, 640, 480, 320]),
        };
        let width_label = |w: u32| {
            if w == 0 {
                "Full size".to_string()
            } else {
                format!("{w} px wide")
            }
        };
        rows.push(
            self.menu(
                "storyboard-movie-size",
                width_label(width),
                widths
                    .iter()
                    .map(|&w| {
                        choice(width_label(w), move |s| match s.kind {
                            Kind::Movie => s.movie.width = w,
                            Kind::Gif => s.gif.width = w,
                        })
                    })
                    .collect(),
                cx,
            ),
        );
        rows.push(
            self.menu(
                "storyboard-movie-range",
                self.range.label.clone(),
                self.ranges
                    .iter()
                    .map(|r| {
                        let r2 = r.clone();
                        choice(r.label.clone(), move |s| s.range = r2.clone())
                    })
                    .collect(),
                cx,
            ),
        );
        let area = match self.kind {
            Kind::Movie => self.movie.area,
            Kind::Gif => self.gif.area,
        };
        let area_label = |a: RenderArea| match a {
            RenderArea::Camera => "Camera frame",
            RenderArea::Overscan => "With overscan",
            RenderArea::AllArtwork => "All artwork",
        };
        rows.push(
            self.menu(
                "storyboard-movie-area",
                area_label(area).into(),
                [
                    RenderArea::Camera,
                    RenderArea::Overscan,
                    RenderArea::AllArtwork,
                ]
                .into_iter()
                .map(|a| choice(area_label(a), move |s| *s.area() = a))
                .collect(),
                cx,
            ),
        );
        let burn = match self.kind {
            Kind::Movie => &self.movie.burn_in,
            Kind::Gif => &self.gif.burn_in,
        };
        let mut burns = vec![
            choice("No burn-in", |s| *s.burn_in() = None),
            choice("Timecode", |s| {
                *s.burn_in() = Some(BurnIn {
                    scene: false,
                    panel: false,
                    ..BurnIn::default()
                })
            }),
            choice("Scene, panel and timecode", |s| {
                let position = s.burn_in().as_ref().map(|b| b.position).unwrap_or_default();
                *s.burn_in() = Some(BurnIn {
                    position,
                    ..BurnIn::default()
                })
            }),
        ];
        for name in &self.captions {
            let field = name.clone();
            burns.push(choice(
                format!("Scene, panel, timecode and {}", name.to_lowercase()),
                move |s| {
                    *s.burn_in() = Some(BurnIn {
                        caption: Some(field.clone()),
                        ..BurnIn::default()
                    })
                },
            ));
        }
        if burn.is_some() {
            burns.push(choice("Text at the top", |s| {
                if let Some(b) = s.burn_in() {
                    b.position = BurnInPosition::Top;
                }
            }));
            burns.push(choice("Text at the bottom", |s| {
                if let Some(b) = s.burn_in() {
                    b.position = BurnInPosition::Bottom;
                }
            }));
        }
        rows.push(self.menu(
            "storyboard-movie-burn-in",
            Self::burn_in_label(burn),
            burns,
            cx,
        ));
        match self.kind {
            Kind::Movie => {
                if self.movie.format != MovieFormat::PngSequence {
                    let quality = |q: u8| match q {
                        0..=60 => "Draft quality",
                        61..=89 => "Good quality",
                        _ => "Best quality",
                    };
                    rows.push(
                        self.menu(
                            "storyboard-movie-quality",
                            quality(self.movie.quality).into(),
                            [50u8, 80, 95]
                                .into_iter()
                                .map(|q| choice(quality(q), move |s| s.movie.quality = q))
                                .collect(),
                            cx,
                        ),
                    );
                }
                rows.push(self.menu(
                    "storyboard-movie-audio",
                    if self.movie.audio {
                        "With sound".into()
                    } else {
                        "No sound".into()
                    },
                    vec![
                        choice("With sound", |s| s.movie.audio = true),
                        choice("No sound", |s| s.movie.audio = false),
                    ],
                    cx,
                ));
                // Only offered when the board has reference video.
                if story::board(&self.project).is_ok_and(|b| !b.timeline.video.is_empty()) {
                    use emulsion_core::timeline::VideoPlacement;
                    let label = |v: Option<VideoPlacement>| match v {
                        None => "No reference video",
                        Some(VideoPlacement::Overlay) => "Reference video over the panels",
                        Some(VideoPlacement::PictureInPicture) => "Reference video inset",
                    };
                    rows.push(
                        self.menu(
                            "storyboard-movie-reference",
                            label(self.movie.reference_video).into(),
                            [
                                None,
                                Some(VideoPlacement::Overlay),
                                Some(VideoPlacement::PictureInPicture),
                            ]
                            .into_iter()
                            .map(|v| choice(label(v), move |s| s.movie.reference_video = v))
                            .collect(),
                            cx,
                        ),
                    );
                }
            }
            Kind::Gif => rows.push(
                self.menu(
                    "storyboard-movie-fps",
                    format!("{} fps", self.gif.fps),
                    [6u32, 8, 10, 12, 15, 24]
                        .into_iter()
                        .map(|f| choice(format!("{f} fps"), move |s| s.gif.fps = f))
                        .collect(),
                    cx,
                ),
            ),
        }
        rows
    }
}

impl Render for MovieExport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let summary = self.summary();
        div()
            .id("storyboard-movie-export")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .text_color(p.ink)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .children(self.option_rows(cx)),
            )
            .child(
                div()
                    .id("storyboard-movie-summary")
                    .test_support()
                    .text_color(if summary.is_ok() {
                        p.muted
                    } else {
                        rgb(0xc98535).into()
                    })
                    .child(match &summary {
                        Ok(s) => s.clone(),
                        Err(e) => e.to_string(),
                    }),
            )
            .when(
                self.kind == Kind::Movie && self.movie.format != MovieFormat::PngSequence,
                |d| {
                    d.child(div().text_color(p.muted).child(
                        "Movies are encoded with FFmpeg, which must be installed and on PATH.",
                    ))
                },
            )
    }
}

impl crate::dialog_actions::DialogActions for MovieExport {
    /// Progress and Close / Export, pinned in the dialog footer.
    fn render_actions(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let summary = self.summary();
        let progress = if self.busy {
            let (d, t) = (
                self.done.load(Ordering::Relaxed),
                self.total.load(Ordering::Relaxed),
            );
            Some(if t == 0 {
                "Preparing…".to_string()
            } else {
                format!("Frame {d} of {t} ({}%)", d * 100 / t.max(1))
            })
        } else {
            None
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .when_some(progress.or(self.message.clone()), |d, m| {
                d.child(div().id("storyboard-movie-message").test_support().child(m))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .when(self.busy, |d| {
                        d.child(
                            Button::new("storyboard-movie-cancel")
                                .label("Cancel export")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel.store(true, Ordering::Relaxed);
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        Button::new("storyboard-movie-close")
                            .label("Close")
                            .disabled(self.busy)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("storyboard-movie-export-button")
                            .label("Export…")
                            .primary()
                            .disabled(self.busy || summary.is_err())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_path(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl EditorView {
    /// File → Export Movie… or Export Animated GIF… on a storyboard.
    pub(crate) fn storyboard_movie_dialog(
        &mut self,
        kind: Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.storyboard_snapshot(cx) else {
            return;
        };
        let (name, selection) = (self.name.clone(), self.board_selection());
        let saved = cx
            .try_global::<crate::app_state::AppSettings>()
            .map(|s| s.0.storyboard_burn_in.clone())
            .unwrap_or_default();
        let view = cx.new(|_| {
            let mut view = MovieExport::new(kind, project, name, &selection);
            // Burn-in starts as the player last showed it.
            for burn in [&mut view.movie.burn_in, &mut view.gif.burn_in]
                .into_iter()
                .flatten()
            {
                *burn = saved.clone();
            }
            view
        });
        let title = match kind {
            Kind::Movie => "Export movie",
            Kind::Gif => "Export animated GIF",
        };
        window.open_dialog(cx, move |dialog, window, cx| {
            crate::dialog_actions::with_actions(dialog, &view, window, cx)
                .title(title)
                .width(px(600.))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use gpui_kit::test::TestWindowExt;

    fn project() -> Project {
        let mut editor =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        let blank = editor.storyboard().unwrap().blank_panel().unwrap();
        let first = editor.active_page();
        editor
            .insert_panels(
                Some(first),
                &blank,
                vec![(
                    "Panel 2".into(),
                    emulsion_core::storyboard::Panel::new(0, 24),
                )],
                None,
            )
            .unwrap();
        editor.snapshot().unwrap()
    }

    #[gpui_kit::test]
    fn gif_export_shows_the_summary_and_writes_the_file(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
        });
        let board = project();
        let second = board.pages[1].meta.id;
        let (view, cx) =
            cx.add_window_view(|_, _| MovieExport::new(Kind::Gif, board, "Film".into(), &[second]));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("film.gif");
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-movie-export").visible());
            assert!(window.find("storyboard-movie-summary").visible());
            view.update(cx, |v, cx| {
                assert_eq!(v.ranges.len(), 3, "whole, selection and one scene");
                let summary = v.summary().unwrap();
                assert!(summary.starts_with("640 × 360"), "{summary}");
                assert!(summary.contains("72 frames"), "{summary}");
                v.range = v.ranges[1].clone();
                assert!(v.summary().unwrap().contains("24 frames"));
                v.gif.width = 320;
                v.gif.burn_in = Some(BurnIn::default());
                v.export_to(path.clone(), cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            let message = view.read(cx).message.clone().unwrap();
            assert!(message.starts_with("Exported 12 frames"), "{message}");
        });
        assert!(path.is_file());
    }

    #[gpui_kit::test]
    fn the_file_menu_opens_movie_and_gif_exports(cx: &mut TestAppContext) {
        let (ws, cx) = crate::tests::open(cx, Document::new(32, 18));
        cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
        let board = ProjectEditor::open(project(), None).unwrap();
        let editor = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(board, "Film".into(), window, cx)
            });
            ws.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        for kind in [Kind::Movie, Kind::Gif] {
            cx.update(|window, cx| {
                editor.update(cx, |e, cx| e.storyboard_movie_dialog(kind, window, cx))
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.render_frame(cx);
                assert!(window.find("storyboard-movie-export").visible());
                window.close_dialog(cx);
            });
        }
    }
}
