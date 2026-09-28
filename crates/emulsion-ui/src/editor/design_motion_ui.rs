//! Saved resize rules, motion authoring and presentation playback.
use super::*;
use emulsion_core::design_metadata::{self as design, Anchor, Effect, Motion};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
#[derive(Default)]
pub(super) struct MotionUi {
    pub(super) preview: Option<Document>,
    /// Preview scenes must never reuse the authored document's GPU cache.
    pub(super) preview_gpu: Rc<RefCell<crate::viewport_gpu::Status>>,
    pub(super) presenting: bool,
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
                                .label("Previous")
                                .small()
                                .ghost()
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.presentation_step(-1, cx)),
                                ),
                        )
                        .child(format!("Page {index} / {}", self.editor.page_list().len()))
                        .child(
                            Button::new("presentation-presenter-view")
                                .label("Presenter view")
                                .small()
                                .outline()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_presenter(window, cx)
                                })),
                        )
                        .child(
                            Button::new("presentation-fullscreen")
                                .label("Fullscreen")
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_presentation_fullscreen(window, cx)
                                })),
                        )
                        .when(self.design_video_playing(), |d| {
                            d.child(
                                Button::new("presentation-stop-video")
                                    .label("Stop video")
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
                                    "Auto advance: on"
                                } else {
                                    "Auto advance: off"
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
                                .label("Next")
                                .small()
                                .ghost()
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.presentation_step(1, cx)),
                                ),
                        )
                        .child(
                            Button::new("presentation-exit")
                                .label("Exit presentation · Esc")
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
    pub(super) fn resize_variant_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let source = self.editor.doc.clone();
        let fields = [source.width.to_string(), source.height.to_string()]
            .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let owner = cx.weak_entity();
        let page = self.editor.active_page();
        let revision = self.editor.revision;
        window.open_dialog(cx,move|dialog,_,_|{let inputs=fields.clone();let source=source.clone();let owner=owner.clone();dialog.title("Copy and resize page").width(px(380.)).child(div().flex().flex_col().gap_2().child("Width · px").child(Input::new(&fields[0])).child("Height · px").child(Input::new(&fields[1])).child("The new page uses each object's anchors. The original stays editable.")).footer(crate::widgets::form_dialog_footer("Create variant"))
.on_ok(move|_,_,cx|{
            let sizes=inputs.each_ref().map(|i|i.read(cx).value().parse::<u32>().unwrap_or(0));
            owner.update(cx,|this,cx|{
                if this.editor.active_page()!=page||this.editor.revision!=revision{this.set_status("The source page changed. Open resize again.",true,cx);return false;}
                match design::resize_variant(&source,sizes[0],sizes[1]).and_then(|resized|this.editor.add_page(resized.doc,format!("{} × {} variant",sizes[0],sizes[1]),0.).map(|_|resized.overflow)){
                    Ok(overflow)=>{this.after_change(cx);this.set_layer_selection(overflow.clone(),overflow.first().copied());this.set_status(if overflow.is_empty(){"Created resized page with editable objects.".into()}else{format!("Created variant. {} object(s) extend outside the page and are selected for review.",overflow.len())},!overflow.is_empty(),cx);true},Err(e)=>{this.set_status(e,true,cx);false}
                }
            }).unwrap_or(false)
        })});
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
                .title("Page and object timing")
                .width(px(430.))
                .child(
                    div().flex().flex_col().gap_2().children(
                        [
                            "Page duration · seconds",
                            "Frames per second",
                            "Selected object starts · seconds",
                            "Selected object ends · seconds",
                            "Entrance / exit duration · seconds",
                            "Slide X · pixels",
                            "Slide Y · pixels",
                        ]
                        .into_iter()
                        .zip(&fields)
                        .map(|(label, input)| div().child(label).child(Input::new(input))),
                    ),
                )
                .footer(crate::widgets::form_dialog_footer("Apply timing"))
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
        let rx = cx.prompt_for_new_path(&dir, Some("design-animation.gif"));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else {
                return;
            };
            path.set_extension("gif");
            this.update(cx, |this, cx| {
                this.set_status("Exporting page animation…", false, cx)
            })
            .ok();
            let result = cx
                .background_spawn(async move {
                    emulsion_io::project_animation::write_gif(&project, &path)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(frames) => this.set_status(
                    format!("Exported {frames} animation frames. GIF output fits within 800 px."),
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
                    .label("Copy and resize page…")
                    .outline()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.resize_variant_dialog(window, cx)),
                    ),
            )
            .child(
                Button::new("design-motion-timing")
                    .label("Duration and timing…")
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| this.motion_timing(window, cx))),
            )
            .child(
                Button::new("design-motion-play")
                    .label(if self.motion.preview.is_some() {
                        "Stop preview"
                    } else {
                        "Preview animation"
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
                    .label("Present pages")
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_motion(true, cx);
                        window.focus(&this.canvas_focus, cx);
                    })),
            )
            .child(
                Button::new("design-export-motion")
                    .label("Export animation GIF…")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.export_design_motion(cx))),
            );
        if let Some(id) = self.selected {
            panel = panel.child(
                Button::new("design-property-keyframes")
                    .label("Property keyframes…")
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
                    .label(format!(
                        "{} anchor: {} ▾",
                        if horizontal { "Horizontal" } else { "Vertical" },
                        anchor.label()
                    ))
                    .small()
                    .outline()
                    .dropdown_menu(move |mut menu, _, _| {
                        for anchor in Anchor::ALL {
                            let owner = owner.clone();
                            menu = menu.item(PopupMenuItem::new(anchor.label()).on_click(
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
                        "Text resize: reflow"
                    } else {
                        "Text resize: scale"
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
                    .label(format!(
                        "{}: {} ▾",
                        if enter { "Entrance" } else { "Exit" },
                        effect.label()
                    ))
                    .small()
                    .outline()
                    .dropdown_menu(move |mut menu, _, _| {
                        for effect in Effect::ALL {
                            let owner = owner.clone();
                            menu = menu.item(PopupMenuItem::new(effect.label()).on_click(
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
                    .label("Remove object animation")
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
        panel.child(div().text_size(px(11.)).text_color(p.muted).child("Select an object for anchors and entrance/exit effects. Timing and resize rules are saved with the page.")).into_any_element()
    }
}

#[path = "design_keyframes_ui.rs"]
mod keyframe_ui;
