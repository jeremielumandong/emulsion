//! Library photos as a timelapse movie or GIF.
use super::*;
use emulsion_io::photo_timelapse::{self, Aspect, Format, Framing, Options, Order};
use gpui_kit::component::{Selectable, WindowExt};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

impl Workspace {
    pub(super) fn library_timelapse_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = self.batch.items.iter().filter(|i| i.selected).count();
        Button::new("library-timelapse")
            .label(t!("library.timelapse.button"))
            .small()
            .ghost()
            .disabled(!(2..=photo_timelapse::MAX_PHOTOS).contains(&count))
            .on_click(cx.listener(|this, _, window, cx| this.library_timelapse_dialog(window, cx)))
            .into_any_element()
    }

    fn library_timelapse_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Library order: what the grid shows.
        let paths = self
            .batch
            .items
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.path.clone())
            .collect::<Vec<_>>();
        if paths.len() < 2 {
            return;
        }
        let folder = self
            .batch
            .out_dir
            .clone()
            .or_else(|| paths[0].parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let view = cx.new(|_| TimelapseDialog {
            paths,
            folder,
            options: Options::default(),
            faces: emulsion_ai::face::detector_available().is_some(),
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(0)),
            note: t!("library.timelapse.hint").into_owned(),
        });
        let cancel = view.read(cx).cancel.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let cancel = cancel.clone();
            dialog
                .title(t!("library.timelapse.title"))
                .width(px(560.))
                .overlay_closable(false)
                .on_close(move |_, _, _| cancel.store(true, Ordering::Relaxed))
                .child(view.clone())
        });
    }
}

struct TimelapseDialog {
    paths: Vec<PathBuf>,
    /// Where the save prompt starts.
    folder: PathBuf,
    options: Options,
    /// The face detector is installed.
    faces: bool,
    busy: bool,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    note: String,
}

impl TimelapseDialog {
    /// Ask for the destination, then export there.
    fn choose_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if self.options.format != Format::Gif && !emulsion_io::ffmpeg::available() {
            self.note = t!("library.timelapse.needs_ffmpeg").into_owned();
            cx.notify();
            return;
        }
        let extension = self.options.format.extension();
        let request = cx.prompt_save_path(&self.folder, Some(&format!("Timelapse.{extension}")));
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut path))) = request.await {
                path.set_extension(extension);
                this.update(cx, |this, cx| this.export_to(path, cx)).ok();
            }
        })
        .detach();
    }

    /// Export to `path` on a worker, showing progress until it ends.
    fn export_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let (paths, options) = (self.paths.clone(), self.options.clone());
        let (cancel, done, total) = (self.cancel.clone(), self.done.clone(), self.total.clone());
        cancel.store(false, Ordering::Relaxed);
        done.store(0, Ordering::Relaxed);
        total.store(options.frames(paths.len()), Ordering::Relaxed);
        self.busy = true;
        self.note = t!("library.timelapse.developing").into_owned();
        cx.notify();
        let task = cx.background_spawn(async move {
            let mut progress = |d: u64, t: u64| {
                done.store(d, Ordering::Relaxed);
                total.store(t, Ordering::Relaxed);
            };
            photo_timelapse::write(&paths, &options, &path, &mut progress, &cancel)
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
                this.note = match result {
                    Ok((report, path)) => {
                        let mut note = t!(
                            "library.timelapse.saved",
                            frames = report.frames,
                            seconds = format!("{:.1}", report.seconds),
                            path = path.display()
                        )
                        .into_owned();
                        if !report.unaligned.is_empty() {
                            note.push(' ');
                            note.push_str(&t!(
                                "library.timelapse.unaligned",
                                count = report.unaligned.len()
                            ));
                        }
                        if let Some(parent) = path.parent() {
                            this.folder = parent.to_path_buf();
                        }
                        note
                    }
                    Err(e) => e.to_string(),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A titled row of mutually exclusive choices.
    fn choices<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        title: impl Into<SharedString>,
        items: Vec<(T, SharedString, bool)>,
        current: T,
        set: fn(&mut Options, T),
        cx: &mut Context<Self>,
    ) -> Div {
        let p = theme::palette(cx);
        let mut row = div().flex().flex_wrap().gap_1();
        for (i, (value, text, enabled)) in items.into_iter().enumerate() {
            row = row.child(
                Button::new((id, i))
                    .label(text)
                    .small()
                    .ghost()
                    .selected(current == value)
                    .disabled(self.busy || !enabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        set(&mut this.options, value);
                        cx.notify();
                    })),
            );
        }
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(div().w(px(120.)).flex_none().child(label(title.into(), &p)))
            .child(row)
    }
}

impl Render for TimelapseDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let o = self.options.clone();
        let gif = o.format == Format::Gif;
        let text = |s: &str| SharedString::from(s.to_string());
        let count = self.paths.len();
        let summary = t!(
            "library.timelapse.summary",
            count = count,
            frames = o.frames(count),
            seconds = format!("{:.1}", o.seconds(count))
        );
        let (done, total) = (
            self.done.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed).max(1),
        );
        let percent = done as f32 * 100. / total as f32;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(mono(summary, 12., p.ink))
            .child(self.choices(
                "timelapse-format",
                t!("library.timelapse.format"),
                vec![
                    (Format::Mp4, text("MP4"), true),
                    (Format::Mov, text("MOV"), true),
                    (Format::Gif, text("GIF"), true),
                ],
                o.format,
                |o, v| {
                    o.format = v;
                    if v == Format::Gif {
                        o.width = o.width.min(photo_timelapse::MAX_GIF_WIDTH);
                    }
                },
                cx,
            ))
            .child(self.choices(
                "timelapse-size",
                t!("library.timelapse.size"),
                vec![
                    (640, text("640"), true),
                    (1280, text("720p"), true),
                    (1920, text("1080p"), true),
                    (3840, text("4K"), !gif),
                ],
                o.width,
                |o, v| o.width = v,
                cx,
            ))
            .child(self.choices(
                "timelapse-shape",
                t!("library.timelapse.shape"),
                vec![
                    (
                        Aspect::FirstPhoto,
                        t!("library.timelapse.shape_first").into(),
                        true,
                    ),
                    (Aspect::Landscape, text("16:9"), true),
                    (Aspect::Portrait, text("9:16"), true),
                    (Aspect::Square, text("1:1"), true),
                ],
                o.aspect,
                |o, v| o.aspect = v,
                cx,
            ))
            .child(
                self.choices(
                    "timelapse-fps",
                    t!("library.timelapse.frame_rate"),
                    [6, 12, 24, 30, 60]
                        .into_iter()
                        .map(|v| (v, text(&v.to_string()), true))
                        .collect(),
                    o.fps,
                    |o, v| o.fps = v,
                    cx,
                ),
            )
            .child(
                self.choices(
                    "timelapse-hold",
                    t!("library.timelapse.hold"),
                    [1, 2, 3, 6, 12, 24]
                        .into_iter()
                        .map(|v| (v, text(&v.to_string()), true))
                        .collect(),
                    o.hold,
                    |o, v| o.hold = v,
                    cx,
                ),
            )
            .child(self.choices(
                "timelapse-crossfade",
                t!("library.timelapse.crossfade"),
                vec![
                    (0, t!("library.timelapse.crossfade_off").into(), true),
                    (2, text("2"), true),
                    (4, text("4"), true),
                    (8, text("8"), true),
                    (12, text("12"), true),
                ],
                o.crossfade,
                |o, v| o.crossfade = v,
                cx,
            ))
            .child(self.choices(
                "timelapse-framing",
                t!("library.timelapse.framing"),
                vec![
                    (
                        Framing::Fill,
                        t!("library.timelapse.framing_fill").into(),
                        true,
                    ),
                    (
                        Framing::Fit,
                        t!("library.timelapse.framing_fit").into(),
                        true,
                    ),
                    (
                        Framing::AlignFaces,
                        t!("library.timelapse.framing_faces").into(),
                        self.faces,
                    ),
                ],
                o.framing,
                |o, v| o.framing = v,
                cx,
            ))
            .when(o.framing == Framing::AlignFaces, |d| {
                d.child(self.choices(
                    "timelapse-face-size",
                    t!("library.timelapse.face_size"),
                    vec![
                        (0.12, t!("library.timelapse.face_small").into(), true),
                        (0.2, t!("library.timelapse.face_medium").into(), true),
                        (0.3, t!("library.timelapse.face_large").into(), true),
                    ],
                    o.eye_spacing,
                    |o, v| o.eye_spacing = v,
                    cx,
                ))
            })
            .when(!self.faces, |d| {
                d.child(mono(
                    t!("library.timelapse.faces_needs_model"),
                    10.,
                    p.muted,
                ))
            })
            .child(self.choices(
                "timelapse-order",
                t!("library.timelapse.order"),
                vec![
                    (
                        Order::CaptureTime,
                        t!("library.timelapse.order_capture").into(),
                        true,
                    ),
                    (
                        Order::FileName,
                        t!("library.timelapse.order_name").into(),
                        true,
                    ),
                    (
                        Order::AsGiven,
                        t!("library.timelapse.order_shown").into(),
                        true,
                    ),
                ],
                o.order,
                |o, v| o.order = v,
                cx,
            ))
            .when(self.busy, |d| {
                d.child(mono(
                    t!("library.timelapse.exporting", done = done, total = total),
                    11.,
                    p.muted,
                ))
                .child(
                    Progress::new("timelapse-progress")
                        .accessibility_label(t!("library.timelapse.progress"))
                        .value(percent)
                        .loading(done == 0),
                )
            })
            .child(mono(self.note.clone(), 11., p.muted))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("timelapse-export")
                            .label(t!("library.timelapse.export"))
                            .small()
                            .primary()
                            .disabled(self.busy)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_path(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("timelapse-cancel")
                            .label(if self.busy {
                                t!("library.timelapse.cancel")
                            } else {
                                t!("library.timelapse.close")
                            })
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel.store(true, Ordering::Relaxed);
                                if this.busy {
                                    this.note = t!("library.timelapse.cancelling").into_owned();
                                    cx.notify();
                                } else {
                                    window.close_dialog(cx);
                                }
                            })),
                    ),
            )
    }
}
