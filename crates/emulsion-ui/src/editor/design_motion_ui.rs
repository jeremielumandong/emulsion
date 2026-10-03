//! Saved resize rules, motion authoring and presentation playback.
use super::*;
use crate::file_prompt::FilePrompts;
use emulsion_core::design_metadata::{self as design, Anchor, Effect, Motion};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
/// Translated display names; `label()` stays the English identifier.
fn preset_label(v: emulsion_core::design_keyframes::Preset) -> SharedString {
    match v {
        emulsion_core::design_keyframes::Preset::FadeIn => {
            t!("editor.design_motion_ui.preset_fade_in")
        }
        emulsion_core::design_keyframes::Preset::SlideUp => {
            t!("editor.design_motion_ui.preset_slide_up")
        }
        emulsion_core::design_keyframes::Preset::Pop => t!("editor.design_motion_ui.preset_pop"),
        emulsion_core::design_keyframes::Preset::Pulse => {
            t!("editor.design_motion_ui.preset_pulse")
        }
        emulsion_core::design_keyframes::Preset::Spin => t!("editor.design_motion_ui.preset_spin"),
        emulsion_core::design_keyframes::Preset::Typewriter => {
            t!("editor.design_motion_ui.preset_typewriter")
        }
    }
    .into()
}
fn anchor_label(v: Anchor) -> SharedString {
    match v {
        Anchor::Start => t!("editor.design_motion_ui.anchor_start"),
        Anchor::Center => t!("editor.design_motion_ui.anchor_center"),
        Anchor::End => t!("editor.design_motion_ui.anchor_end"),
        Anchor::Stretch => t!("editor.design_motion_ui.anchor_stretch"),
        Anchor::Scale => t!("editor.design_motion_ui.anchor_scale"),
    }
    .into()
}
fn effect_label(v: Effect) -> SharedString {
    match v {
        Effect::None => t!("editor.design_motion_ui.effect_none"),
        Effect::Fade => t!("editor.design_motion_ui.effect_fade"),
        Effect::Slide => t!("editor.design_motion_ui.effect_slide"),
        Effect::Zoom => t!("editor.design_motion_ui.effect_zoom"),
    }
    .into()
}
#[derive(Default)]
pub(super) struct MotionUi {
    pub(super) preview: Option<Document>,
    /// Preview scenes must never reuse the authored document's GPU cache.
    pub(super) preview_gpu: Rc<RefCell<crate::viewport_gpu::Status>>,
    pub(super) presenting: bool,
    pub(super) hovered_action: Option<NodeId>,
    pub(super) dragged_action: Option<(NodeId, Point<Pixels>)>,
    pub(super) auto_advance: bool,
    pub(super) fullscreen_window: Option<AnyWindowHandle>,
    pub(super) session: Option<super::design_presentation_ui::PresentationSession>,
    pub(super) transition: Option<super::design_presentation_ui::SlideTransition>,
    pub(super) playing: bool,
    time_ms: u32,
    pub(super) run: u64,
    task: Option<Task<()>>,
}
impl EditorView {
    pub(super) fn presentation_time_ms(&self) -> u32 {
        self.motion.time_ms
    }
    /// Return an isolated GPU scene identity for this evaluated presentation frame.
    /// Repeated paints of a paused frame reuse its compiled scene. A new run or
    /// evaluated time invalidates that scene even when the authored revision is unchanged.
    pub(super) fn presentation_gpu_frame(
        &self,
    ) -> Option<(Document, u64, Rc<RefCell<crate::viewport_gpu::Status>>)> {
        if !self.motion.presenting {
            return None;
        }
        let doc = self.motion.preview.as_ref()?.clone();
        let time = if doc.design.motion.is_empty()
            && doc.design.keyframes.is_empty()
            && doc.design.interactions.is_empty()
        {
            0
        } else {
            self.motion.time_ms
        };
        let key = self.motion.run.wrapping_shl(32)
            ^ u64::from(time)
            ^ self
                .presentation_interaction_generation()
                .wrapping_mul(0x9e3779b97f4a7c15);
        Some((doc, key, self.motion.preview_gpu.clone()))
    }

    pub(super) fn stop_motion(&mut self, cx: &mut Context<Self>) -> bool {
        let had = self.motion.preview.is_some() || self.motion.playing || self.motion.presenting;
        self.stop_design_video(cx);
        self.finish_presentation_session(cx);
        self.clear_presentation_transition(cx);
        if let Some(handle) = self.motion.fullscreen_window.take() {
            cx.defer(move |cx| {
                cx.update_window(handle, |_, window, _| {
                    if window.is_fullscreen() {
                        window.toggle_fullscreen();
                    }
                })
                .ok();
            });
        }
        self.motion.preview = None;
        self.motion.hovered_action = None;
        self.motion.dragged_action = None;
        // Replace the cell rather than resetting it in place: a retained paint
        // closure can finish with its old scene without repopulating this run's cache.
        self.motion.preview_gpu = Default::default();
        self.motion.playing = false;
        self.motion.presenting = false;
        self.motion.task = None;
        self.motion.run = self.motion.run.wrapping_add(1);
        if had {
            self.seen_rev = u64::MAX;
            self.tree_dirty = emulsion_core::Dirty::All;
            cx.notify();
        }
        had
    }
    pub(crate) fn start_motion(&mut self, presenting: bool, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        self.start_motion_prepared(presenting, cx);
    }
    pub(super) fn start_motion_prepared(&mut self, presenting: bool, cx: &mut Context<Self>) {
        self.exit_responsive_preview(cx);
        self.anim.open = false;
        self.anim.playing = false;
        self.stop_motion(cx);
        let doc = self.editor.doc.clone();
        let duration = doc.design.duration_ms.max(
            if presenting && doc.design.page_transition != design::PageTransition::None {
                doc.design.transition_ms.saturating_add(1)
            } else {
                0
            },
        );
        let fps = doc.design.fps;
        let ticket = self.edit_ticket();
        self.motion.playing = true;
        self.motion.presenting = presenting;
        self.motion.time_ms = 0;
        self.motion.run = self.motion.run.wrapping_add(1);
        let run = self.motion.run;
        if presenting {
            self.fit_pending = true;
            self.status = None;
        }
        if presenting {
            self.begin_presentation_session(cx);
            self.motion.transition = (doc.design.page_transition != design::PageTransition::None)
                .then(|| {
                    super::design_presentation_ui::SlideTransition::new(doc.design.page_transition)
                });
        }
        self.motion.task = Some(cx.spawn(async move |this, cx| {
            if presenting && doc.design.page_transition != design::PageTransition::None {
                let source = design::at_time(&doc, 0).unwrap_or_else(|_| doc.clone());
                let (w, h, pixels) = cx
                    .background_spawn(async move { super::history::doc_thumb(&source, 2048) })
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        if this.motion.run != run || this.edit_ticket() != ticket {
                            return false;
                        }
                        if let Some(transition) = &mut this.motion.transition {
                            transition.image = Some(Arc::new(viewport::bgra_image(w, h, pixels)));
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
            let start = cx.background_executor().now();
            let static_slide = doc.design.motion.is_empty()
                && doc.design.keyframes.is_empty()
                && doc.design.interactions.is_empty();
            let mut last_visual_time = None;
            loop {
                let elapsed = cx
                    .background_executor()
                    .now()
                    .duration_since(start)
                    .as_millis() as u64;
                let done = presenting && elapsed >= u64::from(duration);
                let time = if done {
                    duration - 1
                } else {
                    (elapsed % u64::from(duration)) as u32
                };
                let visual_time = if static_slide {
                    0
                } else {
                    time.min(doc.design.duration_ms - 1)
                };
                let result = if last_visual_time != Some(visual_time) {
                    let source = doc.clone();
                    last_visual_time = Some(visual_time);
                    Some(
                        cx.background_spawn(async move { design::at_time(&source, visual_time) })
                            .await,
                    )
                } else {
                    None
                };
                let more = this
                    .update(cx, |this, cx| {
                        if !this.visible || this.motion.run != run || this.edit_ticket() != ticket {
                            return false;
                        }
                        this.motion.time_ms = time;
                        let transition_changed = this.motion.transition.is_some();
                        if let Some(transition) = &mut this.motion.transition {
                            transition.progress =
                                (elapsed as f32 / doc.design.transition_ms as f32).clamp(0., 1.);
                        }
                        if this
                            .motion
                            .transition
                            .as_ref()
                            .is_some_and(|t| t.progress >= 1.)
                        {
                            this.clear_presentation_transition(cx);
                        }
                        if transition_changed {
                            cx.notify();
                        }
                        let result = result.map(|r| {
                            r.and_then(|preview| {
                                if presenting {
                                    this.apply_presentation_interactions(preview)
                                } else {
                                    Ok(preview)
                                }
                            })
                        });
                        match result {
                            Some(Ok(preview)) => {
                                this.motion.preview = Some(preview);
                                this.motion.time_ms = time;
                                this.seen_rev = u64::MAX;
                                this.tree_dirty = emulsion_core::Dirty::All;
                                this.notify_canvas(cx);
                                cx.notify();
                            }
                            Some(Err(e)) => {
                                this.stop_motion(cx);
                                this.set_status(e, true, cx);
                                return false;
                            }
                            None => {}
                        }
                        if done {
                            this.motion.playing = false;
                            let index = this
                                .editor
                                .page_list()
                                .iter()
                                .position(|p| p.id == this.editor.active_page())
                                .unwrap_or(0);
                            if this.motion.auto_advance
                                && !this.design_video_playing()
                                && index + 1 < this.editor.page_list().len()
                            {
                                let owner = cx.weak_entity();
                                cx.defer(move |cx| {
                                    owner
                                        .update(cx, |this, cx| this.presentation_step(1, cx))
                                        .ok();
                                });
                            }
                            return false;
                        }
                        true
                    })
                    .unwrap_or(false);
                if !more {
                    break;
                }
                let delay = if static_slide
                    && (!presenting
                        || doc.design.page_transition == design::PageTransition::None
                        || elapsed >= u64::from(doc.design.transition_ms))
                {
                    if presenting {
                        u64::from(duration).saturating_sub(elapsed).max(1)
                    } else {
                        u64::from(duration)
                    }
                } else {
                    1000 / u64::from(fps)
                };
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(delay))
                    .await;
            }
        }));
        cx.notify();
    }
    pub(super) fn presentation_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if !self.motion.presenting {
            return;
        }
        let pages = self.editor.page_list();
        let index = pages
            .iter()
            .position(|p| p.id == self.editor.active_page())
            .unwrap_or(0);
        let to = (index as isize + delta).clamp(0, pages.len() as isize - 1) as usize;
        let id = pages[to].id;
        if id == self.editor.active_page() {
            return;
        }
        self.record_presentation_navigation(id);
        let fullscreen = self.motion.fullscreen_window.take();
        let session = self.motion.session.take();
        self.stop_motion(cx);
        // Presentation navigation has no authored edits to finalize and must
        // also work while an authorized assistant host request is running.
        if self.editor.set_active_page(id).is_ok() {
            self.after_change(cx);
            self.start_motion_prepared(true, cx);
        }
        self.motion.fullscreen_window = fullscreen;
        self.motion.session = session;
        cx.refresh_windows();
    }
    pub(super) fn resume_presentation_advance(&mut self, cx: &mut Context<Self>) {
        if self.motion.presenting
            && self.motion.auto_advance
            && !self.motion.playing
            && !self.design_video_playing()
            && self
                .editor
                .page_list()
                .last()
                .is_some_and(|page| page.id != self.editor.active_page())
        {
            self.presentation_step(1, cx);
        }
    }
    pub(super) fn presentation_view(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let clean = window.is_fullscreen();
        let index = self
            .editor
            .page_list()
            .iter()
            .position(|p| p.id == self.editor.active_page())
            .unwrap_or(0)
            + 1;
        div()
            .id("design-presentation")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .bg(rgb(0x000000))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.presentation_key(event, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|this, _: &crate::actions::NudgeLeft, _, cx| {
                this.presentation_step(-1, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::actions::NudgeRight, _, cx| {
                this.presentation_step(1, cx)
            }))
            .child(self.presentation_stage())
            .when_some(
                self.status.clone().filter(|(_, error)| *error && !clean),
                |d, (message, _)| {
                    d.child(
                        div()
                            .id("presentation-player-status")
                            .test_support()
                            .px_3()
                            .py_1()
                            .text_color(p.ink)
                            .child(message),
                    )
                },
            )
            .when(!clean, |d| {
                d.child(
                    div()
                        .id("presentation-controls")
                        .test_support()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .p_2()
                        .child(
                            Button::new("presentation-prev")
                                .label(t!("editor.design_motion_ui.previous"))
                                .small()
                                .ghost()
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.presentation_step(-1, cx)),
                                ),
                        )
                        .child(t!(
                            "editor.design_motion_ui.page_of",
                            index = index,
                            total = self.editor.page_list().len()
                        ))
                        .child(
                            Button::new("presentation-presenter-view")
                                .label(t!("editor.design_motion_ui.presenter_view"))
                                .small()
                                .outline()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_presenter(window, cx)
                                })),
                        )
                        .child(
                            Button::new("presentation-fullscreen")
                                .label(t!("editor.design_motion_ui.fullscreen"))
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_presentation_fullscreen(window, cx)
                                })),
                        )
                        .when(self.design_video_playing(), |d| {
                            d.child(
                                Button::new("presentation-stop-video")
                                    .label(t!("editor.design_motion_ui.stop_video"))
                                    .small()
                                    .outline()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.stop_design_video(cx);
                                        this.resume_presentation_advance(cx);
                                    })),
                            )
                        })
                        .child(
                            Button::new("presentation-auto-advance")
                                .label(if self.motion.auto_advance {
                                    t!("editor.design_motion_ui.auto_advance_on")
                                } else {
                                    t!("editor.design_motion_ui.auto_advance_off")
                                })
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.motion.auto_advance = !this.motion.auto_advance;
                                    this.resume_presentation_advance(cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("presentation-next")
                                .label(t!("editor.design_motion_ui.next"))
                                .small()
                                .ghost()
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.presentation_step(1, cx)),
                                ),
                        )
                        .child(
                            Button::new("presentation-exit")
                                .label(t!("editor.design_motion_ui.exit_presentation"))
                                .small()
                                .outline()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.stop_motion(cx);
                                })),
                        ),
                )
            })
            .into_any_element()
    }
    fn set_resize_anchor(
        &mut self,
        id: NodeId,
        horizontal: bool,
        anchor: Anchor,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let mut settings = self.editor.doc.design.clone();
        let rule = settings.constraints.entry(id).or_default();
        if horizontal {
            rule.horizontal = anchor;
        } else {
            rule.vertical = anchor;
        }
        self.execute(
            Command::SetDesign {
                design: Box::new(settings),
            },
            cx,
        );
    }
    fn motion_timing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let settings = self.editor.doc.design.clone();
        let id = self.selected;
        let motion = id
            .and_then(|id| settings.motion.get(&id))
            .cloned()
            .unwrap_or(Motion {
                end_ms: settings.duration_ms,
                transition_ms: 500.min(settings.duration_ms / 2),
                ..Default::default()
            });
        let values = [
            (settings.duration_ms as f64 / 1000.).to_string(),
            settings.fps.to_string(),
            (motion.start_ms as f64 / 1000.).to_string(),
            (motion.end_ms as f64 / 1000.).to_string(),
            (motion.transition_ms as f64 / 1000.).to_string(),
            motion.offset.0.to_string(),
            motion.offset.1.to_string(),
        ];
        let fields = values.map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let owner = cx.weak_entity();
        let page = self.editor.active_page();
        window.open_dialog(cx, move |dialog, _, _| {
            let inputs = fields.clone();
            let owner = owner.clone();
            let motion = motion.clone();
            dialog
                .title(t!("editor.design_motion_ui.timing_title"))
                .width(px(430.))
                .child(
                    div().flex().flex_col().gap_2().children(
                        [
                            t!("editor.design_motion_ui.page_duration_s"),
                            t!("editor.design_motion_ui.fps"),
                            t!("editor.design_motion_ui.object_start_s"),
                            t!("editor.design_motion_ui.object_end_s"),
                            t!("editor.design_motion_ui.transition_s"),
                            t!("editor.design_motion_ui.slide_x"),
                            t!("editor.design_motion_ui.slide_y"),
                        ]
                        .into_iter()
                        .zip(&fields)
                        .map(|(label, input)| div().child(label).child(Input::new(input))),
                    ),
                )
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_motion_ui.apply_timing"
                )))
                .on_ok(move |_, _, cx| {
                    let values = inputs
                        .each_ref()
                        .map(|i| i.read(cx).value().parse::<f64>().unwrap_or(f64::NAN));
                    if values.iter().any(|v| !v.is_finite() || v.abs() > 1e6)
                        || values[..5].iter().any(|v| *v < 0.)
                    {
                        return false;
                    }
                    owner
                        .update(cx, |this, cx| {
                            if this.editor.active_page() != page {
                                return false;
                            }
                            let mut settings = this.editor.doc.design.clone();
                            settings.duration_ms = (values[0] * 1000.).round() as u32;
                            settings.fps = values[1].round() as u32;
                            if let Some(id) = id {
                                let mut motion = motion.clone();
                                motion.start_ms = (values[2] * 1000.).round() as u32;
                                motion.end_ms = (values[3] * 1000.).round() as u32;
                                motion.transition_ms = (values[4] * 1000.).round() as u32;
                                motion.offset = (values[5], values[6]);
                                settings.motion.insert(id, motion);
                            }
                            match settings.validate(&this.editor.doc) {
                                Ok(()) => {
                                    this.execute(
                                        Command::SetDesign {
                                            design: Box::new(settings),
                                        },
                                        cx,
                                    );
                                    true
                                }
                                Err(e) => {
                                    this.set_status(e, true, cx);
                                    false
                                }
                            }
                        })
                        .unwrap_or(false)
                })
        });
    }
    fn set_motion_effect(
        &mut self,
        id: NodeId,
        enter: bool,
        effect: Effect,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let mut settings = self.editor.doc.design.clone();
        let duration = settings.duration_ms;
        let motion = settings.motion.entry(id).or_insert(Motion {
            end_ms: duration,
            transition_ms: 500.min(duration / 2),
            ..Default::default()
        });
        if enter {
            motion.enter = effect;
        } else {
            motion.exit = effect;
        }
        self.execute(
            Command::SetDesign {
                design: Box::new(settings),
            },
            cx,
        );
    }
    fn export_design_motion(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(project) = self.editor.snapshot() else {
            return;
        };
        let dir = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| ".".into());
        let rx = cx.prompt_save_path(&dir, Some("design-animation.gif"));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else {
                return;
            };
            path.set_extension("gif");
            this.update(cx, |this, cx| {
                this.set_status(t!("editor.design_motion_ui.exporting_animation"), false, cx)
            })
            .ok();
            let result = cx
                .background_spawn(async move {
                    emulsion_io::project_animation::write_gif(&project, &path)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(frames) => this.set_status(
                    t!("editor.design_motion_ui.exported_gif", frames = frames),
                    false,
                    cx,
                ),
                Err(e) => this.set_status(e.to_string(), true, cx),
            })
            .ok();
        })
        .detach();
    }
    fn import_lottie_animation(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ticket = self.edit_ticket();
        let rx = cx.prompt_open_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("editor.design_motion_ui.import_lottie_prompt").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move { emulsion_io::lottie::read(&path) })
                .await;
            this.update(cx, |this, cx| {
                if this.edit_ticket() != ticket {
                    this.set_status(t!("editor.design_motion_ui.page_changed_import"), true, cx);
                    return;
                }
                match result.and_then(|(doc, report)| {
                    emulsion_io::lottie::insert(&mut this.editor, &doc).map(|ids| (ids, report))
                }) {
                    Ok((ids, report)) => {
                        let selected = ids.last().copied();
                        this.set_layer_selection(ids, selected);
                        this.after_change(cx);
                        this.set_tool(Tool::Move, cx);
                        this.set_status(
                            t!(
                                "editor.design_motion_ui.imported_lottie",
                                count = report.nodes,
                                details = report.diagnostics.join(" ")
                            ),
                            false,
                            cx,
                        );
                    }
                    Err(error) => this.set_status(error.to_string(), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }
    fn export_motion_interchange(
        &mut self,
        format: emulsion_io::design_motion_export::Format,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let doc = self.editor.doc.clone();
        let extension = if format == emulsion_io::design_motion_export::Format::AnimatedSvg {
            "svg"
        } else {
            "json"
        };
        let dir = self
            .editor
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let rx = cx.prompt_save_path(&dir, Some(&format!("design-animation.{extension}")));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else {
                return;
            };
            path.set_extension(extension);
            this.update(cx, |this, cx| {
                this.set_status(t!("editor.design_motion_ui.exporting_motion"), false, cx)
            })
            .ok();
            let result = cx
                .background_spawn(async move {
                    emulsion_io::design_motion_export::write(&doc, &path, format)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(report) => this.set_status(
                    if report.format == emulsion_io::design_motion_export::Format::Lottie {
                        t!(
                            "editor.design_motion_ui.exported_lottie",
                            details = report.diagnostics.join(" ")
                        )
                    } else {
                        t!(
                            "editor.design_motion_ui.exported_frames",
                            frames = report.frames,
                            vector = report.vector_frames,
                            raster = report.raster_frames,
                            details = report.diagnostics.join(" ")
                        )
                    },
                    false,
                    cx,
                ),
                Err(e) => this.set_status(e.to_string(), true, cx),
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn design_motion_controls(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.presentation_authoring_controls(cx))
            .child(
                Button::new("design-resize-copy")
                    .label(t!("editor.design_motion_ui.resize_copy"))
                    .outline()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.resize_variant_dialog(window, cx)),
                    ),
            )
            .child(
                Button::new("design-motion-timing")
                    .label(t!("editor.design_motion_ui.duration_timing"))
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| this.motion_timing(window, cx))),
            )
            .child(
                Button::new("design-motion-play")
                    .label(if self.motion.preview.is_some() {
                        t!("editor.design_motion_ui.stop_preview")
                    } else {
                        t!("editor.design_motion_ui.preview_animation")
                    })
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.stop_motion(cx) {
                            this.start_motion(false, cx);
                        }
                    })),
            )
            .child(
                Button::new("design-present")
                    .label(t!("editor.design_motion_ui.present_pages"))
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_motion(true, cx);
                        window.focus(&this.canvas_focus, cx);
                    })),
            )
            .child(
                Button::new("design-export-animated-svg")
                    .label(t!("editor.design_motion_ui.export_svg"))
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.export_motion_interchange(
                            emulsion_io::design_motion_export::Format::AnimatedSvg,
                            cx,
                        )
                    })),
            )
            .child(
                Button::new("design-export-lottie")
                    .label(t!("editor.design_motion_ui.export_lottie"))
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.export_motion_interchange(
                            emulsion_io::design_motion_export::Format::Lottie,
                            cx,
                        )
                    })),
            )
            .child(
                Button::new("design-import-lottie")
                    .label(t!("editor.design_motion_ui.import_lottie"))
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.import_lottie_animation(cx))),
            )
            .child(
                Button::new("design-export-lottie-raster")
                    .label(t!("editor.design_motion_ui.export_lottie_raster"))
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.export_motion_interchange(
                            emulsion_io::design_motion_export::Format::LottieRaster,
                            cx,
                        )
                    })),
            )
            .child(
                Button::new("design-export-motion")
                    .label(t!("editor.design_motion_ui.export_gif"))
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.export_design_motion(cx))),
            );
        if let Some(id) = self.selected {
            let owner = cx.weak_entity();
            panel = panel
                .child(
                    Button::new("design-motion-presets")
                        .label(t!("editor.design_motion_ui.motion_preset"))
                        .outline()
                        .dropdown_menu(move |mut menu, _, _| {
                            for preset in emulsion_core::design_keyframes::Preset::ALL {
                                let owner = owner.clone();
                                menu =
                                    menu.item(PopupMenuItem::new(preset_label(preset)).on_click(
                                        move |_, _, cx| {
                                            owner
                                            .update(cx, |this, cx| {
                                                if !this.prepare_page_action(cx) {
                                                    return;
                                                }
                                                let ids = this.selected_layer_ids();
                                                let end =
                                                    this.editor.doc.design.duration_ms.min(1000);
                                                match emulsion_core::design_keyframes::apply_preset(
                                                    &mut this.editor,
                                                    &ids,
                                                    preset,
                                                    0,
                                                    end,
                                                ) {
                                                    Ok(()) => this.after_change(cx),
                                                    Err(e) => this.set_status(e, true, cx),
                                                }
                                            })
                                            .ok();
                                        },
                                    ));
                            }
                            menu
                        }),
                )
                .child(
                    Button::new("design-retime-motion")
                        .label(t!("editor.design_motion_ui.retime"))
                        .outline()
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.retime_motion_dialog(window, cx)
                            }),
                        ),
                );
            panel = panel.child(
                Button::new("design-property-keyframes")
                    .label(t!("editor.design_motion_ui.property_keyframes"))
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.property_keyframes_dialog(window, cx)
                    })),
            );
            let rule = self
                .editor
                .doc
                .design
                .constraints
                .get(&id)
                .copied()
                .unwrap_or_default();
            let motion = self
                .editor
                .doc
                .design
                .motion
                .get(&id)
                .cloned()
                .unwrap_or_default();
            for (horizontal, anchor) in [(true, rule.horizontal), (false, rule.vertical)] {
                let owner = cx.weak_entity();
                panel = panel.child(
                    Button::new(if horizontal {
                        "design-anchor-x"
                    } else {
                        "design-anchor-y"
                    })
                    .label(if horizontal {
                        t!(
                            "editor.design_motion_ui.horizontal_anchor",
                            anchor = anchor_label(anchor)
                        )
                    } else {
                        t!(
                            "editor.design_motion_ui.vertical_anchor",
                            anchor = anchor_label(anchor)
                        )
                    })
                    .small()
                    .outline()
                    .dropdown_menu(move |mut menu, _, _| {
                        for anchor in Anchor::ALL {
                            let owner = owner.clone();
                            menu = menu.item(PopupMenuItem::new(anchor_label(anchor)).on_click(
                                move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.set_resize_anchor(id, horizontal, anchor, cx)
                                        })
                                        .ok();
                                },
                            ));
                        }
                        menu
                    }),
                );
            }
            panel = panel.child(
                Button::new("design-reflow")
                    .label(if rule.reflow_text {
                        t!("editor.design_motion_ui.text_reflow")
                    } else {
                        t!("editor.design_motion_ui.text_scale")
                    })
                    .small()
                    .outline()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut design = this.editor.doc.design.clone();
                        design.constraints.entry(id).or_default().reflow_text = !rule.reflow_text;
                        this.execute(
                            Command::SetDesign {
                                design: Box::new(design),
                            },
                            cx,
                        );
                    })),
            );
            for (enter, effect) in [(true, motion.enter), (false, motion.exit)] {
                let owner = cx.weak_entity();
                panel = panel.child(
                    Button::new(if enter {
                        "design-enter-effect"
                    } else {
                        "design-exit-effect"
                    })
                    .label(if enter {
                        t!(
                            "editor.design_motion_ui.entrance_effect",
                            effect = effect_label(effect)
                        )
                    } else {
                        t!(
                            "editor.design_motion_ui.exit_effect",
                            effect = effect_label(effect)
                        )
                    })
                    .small()
                    .outline()
                    .dropdown_menu(move |mut menu, _, _| {
                        for effect in Effect::ALL {
                            let owner = owner.clone();
                            menu = menu.item(PopupMenuItem::new(effect_label(effect)).on_click(
                                move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.set_motion_effect(id, enter, effect, cx)
                                        })
                                        .ok();
                                },
                            ));
                        }
                        menu
                    }),
                );
            }
            panel = panel.child(
                Button::new("design-remove-motion")
                    .label(t!("editor.design_motion_ui.remove_motion"))
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut design = this.editor.doc.design.clone();
                        design.motion.remove(&id);
                        this.execute(
                            Command::SetDesign {
                                design: Box::new(design),
                            },
                            cx,
                        );
                    })),
            );
        }
        panel
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(t!("editor.design_motion_ui.empty_hint")),
            )
            .into_any_element()
    }
}

#[path = "design_keyframes_ui.rs"]
mod keyframe_ui;

#[cfg(test)]
mod lottie_workflow_tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui::TestAppContext;
    #[gpui_kit::test]
    fn design_lottie_native_picker_import_export_cancel_stale_and_undo(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.json");
        let mut source = Document::new(160, 100);
        let id = source.alloc_id();
        source.nodes.push(emulsion_core::Node::path(
            id,
            "Editable square",
            Arc::new(emulsion_raster::vector_geometry::rectangle(
                10., 10., 20., 20.,
            )),
            Default::default(),
            160,
            100,
        ));
        std::fs::write(&path, emulsion_io::lottie::encode(&source).unwrap().0).unwrap();
        let original = Document::new(320, 200);
        let (workspace, cx) = crate::tests::open(cx, original.clone());
        let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
        cx.update(|_, cx| view.update(cx, |this, cx| this.import_lottie_animation(cx)));
        cx.run_until_parked();
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|_| None);
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
        cx.update(|_, cx| view.update(cx, |this, cx| this.import_lottie_animation(cx)));
        cx.run_until_parked();
        cx.simulate_path_prompt_response(|_| Some(vec![path.clone()]));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(!view.read(cx).editor.doc.nodes.is_empty());
            assert_eq!(view.read(cx).editor.doc.width, 320);
        });
        let output = dir.path().join("vectors.json");
        cx.update(|_, cx| {
            view.update(cx, |this, cx| {
                this.export_motion_interchange(
                    emulsion_io::design_motion_export::Format::Lottie,
                    cx,
                )
            })
        });
        cx.run_until_parked();
        assert!(cx.did_prompt_for_new_path());
        cx.simulate_new_path_selection(|_| Some(output.clone()));
        cx.run_until_parked();
        assert!(emulsion_io::lottie::is_lottie_path(&output));
        cx.update(|_, cx| {
            view.update(cx, |this, cx| {
                this.undo(cx);
                assert_eq!(this.editor.doc, original);
                this.import_lottie_animation(cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |this, cx| {
                let mut design = this.editor.doc.design.clone();
                design.duration_ms += 100;
                this.editor
                    .execute(emulsion_core::Command::SetDesign {
                        design: Box::new(design),
                    })
                    .unwrap();
                this.after_change(cx);
            })
        });
        cx.simulate_path_prompt_response(|_| Some(vec![path.clone()]));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).editor.doc.nodes.is_empty()));
    }
}
