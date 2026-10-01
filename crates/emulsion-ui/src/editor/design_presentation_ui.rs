//! Presenter controls live in a separate window; the audience canvas stays clean.
use super::*;
use emulsion_core::{design_metadata::PageTransition, project::PageId};
use gpui_kit::component::{
    Disableable, Root, Sizable, TitleBar, WindowExt,
    button::{Button, ButtonVariants},
    input::{Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::collections::HashSet;

#[path = "design_interactions_ui.rs"]
mod interactions;

pub(super) struct PresentationSession {
    interactions: emulsion_core::design_interactions::Runtime,
    interaction_source: RefCell<Option<(u64, Document)>>,
    back_stack: Vec<PageId>,
    navigating_back: bool,
    return_page: PageId,
    return_view: View,
    return_selection: Vec<NodeId>,
    return_primary: Option<NodeId>,
    presenter: Option<AnyWindowHandle>,
    presenter_opening: bool,
    instance: u64,
    timer_started: Option<Instant>,
    elapsed: std::time::Duration,
}
pub(super) struct SlideTransition {
    kind: PageTransition,
    pub image: Option<Arc<RenderImage>>,
    pub progress: f32,
}
impl SlideTransition {
    pub fn new(kind: PageTransition) -> Self {
        Self {
            kind,
            image: None,
            progress: 0.,
        }
    }
}
/// Audience fitting uses the actual laid-out viewport, without editor margins
/// or the editor's 100% fit cap. A zero-sized hidden viewport needs no repaint.
pub(super) fn fit_page(width: u32, height: u32, bounds: Bounds<Pixels>) -> Option<View> {
    let cw = f64::from(f32::from(bounds.size.width));
    let ch = f64::from(f32::from(bounds.size.height));
    if width == 0 || height == 0 || !cw.is_finite() || !ch.is_finite() || cw <= 0. || ch <= 0. {
        return None;
    }
    Some(View {
        zoom: (cw / f64::from(width)).min(ch / f64::from(height)),
        center: (f64::from(width) / 2., f64::from(height) / 2.),
        rotation: 0.,
        ..Default::default()
    })
}
impl EditorView {
    pub(crate) fn presentation_runtime_ticket(&self) -> u64 {
        self.motion.run
    }
    pub(crate) fn presentation_active(&self) -> bool {
        self.motion.presenting
    }
    pub(crate) fn presentation_host_action(
        &mut self,
        action: emulsion_mcp::design_motion_tools::HostAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<serde_json::Value, String> {
        use emulsion_mcp::design_motion_tools::{Direction, HostAction};
        if !self.visible {
            return Err(
                "Open this document in the visible editor before controlling presentation".into(),
            );
        }
        match action {
            HostAction::State => (),
            HostAction::Start {
                fullscreen,
                presenter,
                auto_advance,
            } => {
                if self.styles_ui.dialog_for.is_some() || self.raw.is_pending() {
                    return Err(
                        "Finish the current appearance dialog or RAW development first".into(),
                    );
                }
                if !self.motion.presenting {
                    self.finish_pointer_gesture(cx);
                    self.close_text_field(cx);
                    self.finish_shape_color_edit(cx);
                    if self.editor.in_transaction() {
                        return Err("Finish the current edit before presenting".into());
                    }
                    self.start_motion_prepared(true, cx);
                }
                self.motion.auto_advance = auto_advance;
                if window.is_fullscreen() != fullscreen {
                    self.toggle_presentation_fullscreen(window, cx);
                }
                if presenter {
                    self.open_presenter(window, cx);
                }
                window.focus(&self.canvas_focus, cx);
            }
            HostAction::End => {
                self.stop_motion(cx);
            }
            HostAction::Navigate(direction) => {
                if !self.motion.presenting {
                    return Err("Start a presentation before navigating".into());
                }
                let count = self.editor.page_list().len() as isize;
                let delta = match direction {
                    Direction::Next => 1,
                    Direction::Previous => -1,
                    Direction::First => -count,
                    Direction::Last => count,
                };
                self.presentation_step(delta, cx);
            }
            HostAction::Fullscreen(enabled) => {
                if !self.motion.presenting {
                    return Err("Start a presentation before changing audience fullscreen".into());
                }
                if window.is_fullscreen() != enabled {
                    self.toggle_presentation_fullscreen(window, cx);
                }
            }
            HostAction::MediaPlay(node) => self.design_media_host_play(node, window, cx)?,
            HostAction::MediaPause => {
                self.design_media_host_command(emulsion_io::design_media::PlaybackCommand::Pause)?
            }
            HostAction::MediaSeek(position_ms) => {
                self.design_media_host_command(emulsion_io::design_media::PlaybackCommand::Seek {
                    position_ms,
                })?
            }
            HostAction::MediaStop => {
                if !self.motion.presenting {
                    return Err("Start a presentation before controlling media.".into());
                }
                self.stop_design_video(cx);
                self.resume_presentation_advance(cx);
            }
            HostAction::TimerPaused(paused) => self.set_presenter_timer_paused(paused, cx)?,
            HostAction::TimerReset => self.reset_presenter_timer(cx)?,
            HostAction::Trigger(node) => self.trigger_presentation_object(node, window, cx)?,
            HostAction::ResponsivePreview(width) => {
                if let Some(width) = width {
                    self.set_responsive_preview(width, cx)?;
                } else {
                    self.exit_responsive_preview(cx);
                }
                window.focus(&self.canvas_focus, cx);
            }
        }
        cx.notify();
        Ok(serde_json::json!({
            "responsive_preview": self.responsive_preview_state(),
            "media":self.design_media_runtime_state(),
            "presenter_timer":self.presenter_timer_state(cx),
            "presenting": self.motion.presenting,
            "page_id": self.editor.active_page(),
            "page_index": self.editor.page_list().iter().position(|p| p.id == self.editor.active_page()).map(|n|n+1),
            "page_count": self.editor.page_list().len(),
            "fullscreen": self.motion.presenting && window.is_fullscreen(),
            "auto_advance": self.motion.auto_advance,
            "animating": self.motion.playing,
            "video_playing": self.design_video_playing(),
            "presenter_open": self.motion.session.as_ref().is_some_and(|s| s.presenter.is_some()),
            "presenter_opening": self.motion.session.as_ref().is_some_and(|s| s.presenter_opening),
            "open_overlays": self.motion.session.as_ref().map(|s|&s.interactions.open_overlays),
            "component_variants": self.motion.session.as_ref().map(|s|&s.interactions.variants),
        }))
    }
    fn presenter_timer_state(&self, cx: &Context<Self>) -> serde_json::Value {
        self.motion.session.as_ref().map(|session|{
            let elapsed=session.elapsed+session.timer_started.map(|start|cx.background_executor().now().saturating_duration_since(start)).unwrap_or_default();
            serde_json::json!({"elapsed_ms":elapsed.as_millis() as u64,"paused":session.timer_started.is_none()})
        }).unwrap_or(serde_json::Value::Null)
    }
    fn set_presenter_timer_paused(
        &mut self,
        paused: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let session = self
            .motion
            .session
            .as_mut()
            .filter(|_| self.motion.presenting)
            .ok_or("Start a presentation before controlling the presenter timer.")?;
        let now = cx.background_executor().now();
        if paused {
            if let Some(started) = session.timer_started.take() {
                session.elapsed += now.saturating_duration_since(started);
            }
        } else if session.timer_started.is_none() {
            session.timer_started = Some(now);
        }
        cx.notify();
        Ok(())
    }
    fn reset_presenter_timer(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let session = self
            .motion
            .session
            .as_mut()
            .filter(|_| self.motion.presenting)
            .ok_or("Start a presentation before controlling the presenter timer.")?;
        session.elapsed = std::time::Duration::ZERO;
        session.timer_started = Some(cx.background_executor().now());
        cx.notify();
        Ok(())
    }
    pub(crate) fn is_clean_presentation(&self, window: &Window) -> bool {
        self.motion.presenting && window.is_fullscreen()
    }
    pub(super) fn clear_presentation_transition(&mut self, cx: &mut Context<Self>) {
        if let Some(image) = self.motion.transition.take().and_then(|t| t.image) {
            cx.defer(move |cx| {
                for handle in cx.windows() {
                    let image = image.clone();
                    cx.update_window(handle, |_, window, _| {
                        window.drop_image(image).ok();
                    })
                    .ok();
                }
            });
        }
    }
    pub(super) fn begin_presentation_session(&mut self, cx: &mut Context<Self>) {
        if self.motion.session.is_none() {
            self.motion.session = Some(PresentationSession {
                interactions: Default::default(),
                interaction_source: Default::default(),
                back_stack: Vec::new(),
                navigating_back: false,
                return_page: self.editor.active_page(),
                return_view: self.view,
                return_selection: self.selected_layer_ids(),
                return_primary: self.selected,
                presenter: None,
                presenter_opening: false,
                instance: self.motion.run,
                timer_started: Some(cx.background_executor().now()),
                elapsed: std::time::Duration::ZERO,
            });
        }
    }
    pub(super) fn finish_presentation_session(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.motion.session.take() else {
            return;
        };
        if let Some(handle) = session.presenter {
            cx.defer(move |cx| {
                cx.update_window(handle, |_, window, _| window.remove_window())
                    .ok();
            });
        }
        if self.editor.active_page() != session.return_page
            && self.editor.page(session.return_page).is_some()
        {
            self.editor.set_active_page(session.return_page).ok();
            self.sync_page_view(cx);
        }
        self.view = session.return_view;
        self.set_layer_selection(session.return_selection, session.return_primary);
        self.fit_pending = false;
        cx.refresh_windows();
    }
    pub(super) fn toggle_presentation_fullscreen(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.motion.fullscreen_window = (!window.is_fullscreen()).then(|| window.window_handle());
        window.toggle_fullscreen();
        self.fit_pending = true;
        window.focus(&self.canvas_focus, cx);
        cx.refresh_windows();
    }
    /// Capture navigation before editing keymaps or player controls can mutate a page.
    pub(super) fn presentation_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.motion.presenting {
            return false;
        }
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            if self.close_presentation_overlay(cx) {
                window.focus(&self.canvas_focus, cx);
                return true;
            }
            self.stop_motion(cx);
            window.focus(&self.canvas_focus, cx);
            return true;
        }
        if event.keystroke.modifiers.control
            || event.keystroke.modifiers.platform
            || event.keystroke.modifiers.alt
        {
            return true;
        }
        // A focused HTML player owns ordinary keys (space pauses its video).
        // Escape above remains an audience-exit shortcut when delivered to GPUI.
        if self.design_video_playing() {
            return false;
        }
        let delta = match key {
            "left" | "up" | "pageup" => Some(-1),
            "space" if event.keystroke.modifiers.shift => Some(-1),
            "right" | "down" | "pagedown" | "space" => Some(1),
            "home" => Some(-(self.editor.page_list().len() as isize)),
            "end" => Some(self.editor.page_list().len() as isize),
            _ => None,
        };
        if let Some(delta) = delta {
            self.presentation_step(delta, cx);
            return true;
        }
        match key {
            "f" => {
                self.toggle_presentation_fullscreen(window, cx);
                true
            }
            "p" => {
                self.open_presenter(window, cx);
                true
            }
            _ => true,
        }
    }
    pub(super) fn presentation_stage(&self) -> AnyElement {
        let stage = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(rgb(0x000000))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .opacity(if self.motion.transition.is_some() {
                        0.
                    } else {
                        1.
                    })
                    .child(self.canvas_region()),
            );
        stage
            .when_some(self.motion.transition.as_ref(), |d, transition| {
                let t = transition.progress.clamp(0., 1.);
                let t = t * t * (3. - 2. * t);
                let scale = match transition.kind {
                    PageTransition::Zoom => 0.82 + 0.18 * t,
                    PageTransition::ZoomOut => 1.18 - 0.18 * t,
                    _ => 1.,
                };
                let left = match transition.kind {
                    PageTransition::Slide => 1. - t,
                    PageTransition::SlideLeft => t - 1.,
                    _ => (1. - scale) / 2.,
                };
                let top = match transition.kind {
                    PageTransition::SlideUp => 1. - t,
                    PageTransition::SlideDown => t - 1.,
                    _ => (1. - scale) / 2.,
                };
                let opacity = if matches!(
                    transition.kind,
                    PageTransition::Slide
                        | PageTransition::SlideLeft
                        | PageTransition::SlideUp
                        | PageTransition::SlideDown
                ) {
                    1.
                } else {
                    t
                };
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .overflow_hidden()
                        .when_some(transition.image.clone(), |d, image| {
                            d.child(
                                div()
                                    .absolute()
                                    .left(relative(left))
                                    .top(relative(top))
                                    .w(relative(scale))
                                    .h(relative(scale))
                                    .opacity(opacity)
                                    .child(img(image).size_full().object_fit(ObjectFit::Contain)),
                            )
                        }),
                )
            })
            .into_any_element()
    }
    pub(super) fn presentation_authoring_controls(&self, cx: &Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        let transition = self.editor.doc.design.page_transition;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                Button::new("design-interactions")
                    .label("Object interaction…")
                    .outline()
                    .disabled(self.selected.is_none())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_interactions_dialog(window, cx)
                    })),
            )
            .child(
                Button::new("design-speaker-notes")
                    .label("Speaker notes…")
                    .outline()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.speaker_notes_dialog(window, cx)),
                    ),
            )
            .child(
                Button::new("design-page-transition")
                    .label(format!("Page transition: {} ▾", transition.label()))
                    .outline()
                    .dropdown_menu(move |mut menu, _, _| {
                        for value in PageTransition::ALL {
                            let owner = owner.clone();
                            menu = menu.item(PopupMenuItem::new(value.label()).on_click(
                                move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            if !this.prepare_page_action(cx) {
                                                return;
                                            }
                                            let mut design = this.editor.doc.design.clone();
                                            design.page_transition = value;
                                            this.execute(
                                                Command::SetDesign {
                                                    design: Box::new(design),
                                                },
                                                cx,
                                            );
                                        })
                                        .ok();
                                },
                            ));
                        }
                        menu
                    }),
            )
            .child(
                Button::new("design-transition-duration")
                    .label(format!(
                        "Transition: {} ms…",
                        self.editor.doc.design.transition_ms
                    ))
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.presentation_duration_dialog(window, cx)
                    })),
            )
            .child(
                Button::new("design-presenter-view")
                    .label("Open presenter view")
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| this.open_presenter(window, cx))),
            )
            .into_any_element()
    }
    fn speaker_notes_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(12)
                .default_value(self.editor.doc.design.speaker_notes.clone())
        });
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx,move|dialog,_,_| {
            let input=input.clone();let owner=owner.clone();
            dialog.title("Speaker notes").width(px(800.))
                .child(div().flex().flex_col().gap_2().child("Only the presenter window displays these notes. They are saved with this page.")
                    .child(div().id("design-speaker-notes-input").test_support().child(Textarea::new(&input).h(rems(20.)).flex_shrink_0())))
                .footer(div().id("design-speaker-notes-footer").test_support().child(crate::widgets::form_dialog_footer("Save notes")))
                .on_ok(move|_,_,cx| {
                    let notes=input.read(cx).value().to_string();
                    owner.update(cx,|this,cx| {
                        if this.edit_ticket()!=ticket {this.set_status("The page changed. Open speaker notes again.",true,cx);return false;}
                        let mut design=this.editor.doc.design.clone();design.speaker_notes=notes;
                        if let Err(error)=design.validate(&this.editor.doc) {this.set_status(error,true,cx);return false;}
                        this.execute(Command::SetDesign{design:Box::new(design)},cx);true
                    }).unwrap_or(false)
                })
        });
    }
    fn presentation_duration_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(self.editor.doc.design.transition_ms.to_string())
        });
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx, move |dialog, _, _| {
            let input = input.clone();
            let owner = owner.clone();
            dialog
                .title("Page transition duration")
                .width(px(380.))
                .child("Duration · 100–3000 milliseconds")
                .child(Input::new(&input).id("design-transition-duration-input"))
                .footer(crate::widgets::form_dialog_footer("Apply"))
                .on_ok(move |_, _, cx| {
                    let Ok(duration) = input.read(cx).value().parse::<u32>() else {
                        return false;
                    };
                    owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket {
                                return false;
                            }
                            let mut design = this.editor.doc.design.clone();
                            design.transition_ms = duration;
                            if let Err(error) = design.validate(&this.editor.doc) {
                                this.set_status(error, true, cx);
                                return false;
                            }
                            this.execute(
                                Command::SetDesign {
                                    design: Box::new(design),
                                },
                                cx,
                            );
                            true
                        })
                        .unwrap_or(false)
                })
        });
    }
    pub(super) fn open_presenter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.motion.presenting {
            self.start_motion(true, cx);
        }
        if !self.motion.presenting {
            return;
        }
        if let Some(handle) = self.motion.session.as_ref().and_then(|s| s.presenter) {
            cx.update_window(handle, |_, window, _| window.activate_window())
                .ok();
            return;
        }
        if !window.is_fullscreen() {
            self.toggle_presentation_fullscreen(window, cx);
        }
        let Some(session) = self.motion.session.as_mut() else {
            return;
        };
        if session.presenter_opening {
            return;
        }
        session.presenter_opening = true;
        let instance = session.instance;
        let audience = window.window_handle();
        let owner = cx.weak_entity();
        // GPUI draws a newly opened window synchronously. Opening it while the
        // editor is leased would let Presenter::render read that same lease.
        cx.defer(move |cx| {
            let current = owner
                .read_with(cx, |this, _| {
                    this.motion.presenting
                        && this
                            .motion
                            .session
                            .as_ref()
                            .is_some_and(|s| s.instance == instance && s.presenter_opening)
                })
                .unwrap_or(false);
            if !current {
                return;
            }
            let closing = owner.clone();
            let render_owner = owner.clone();
            let bounds = Bounds::centered(None, size(px(1040.), px(760.)), cx);
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Emulsion — Presenter".into()),
                        ..Default::default()
                    }),
                    window_min_size: Some(size(px(640.), px(480.))),
                    ..Default::default()
                },
                move |window, cx| {
                    window.on_window_should_close(cx, move |_, cx| {
                        closing
                            .update(cx, |this, cx| {
                                if this
                                    .motion
                                    .session
                                    .as_ref()
                                    .is_some_and(|s| s.instance == instance)
                                {
                                    if let Some(session) = &mut this.motion.session {
                                        session.presenter = None;
                                    }
                                    this.stop_motion(cx);
                                }
                            })
                            .ok();
                        true
                    });
                    let presenter = cx.new(|cx| Presenter::new(render_owner, audience, window, cx));
                    cx.new(|cx| Root::new(presenter, window, cx))
                },
            );
            match result {
                Ok(handle) => {
                    let kept = owner
                        .update(cx, |this, cx| {
                            if let Some(session) = &mut this.motion.session
                                && session.instance == instance
                            {
                                session.presenter = Some(handle.into());
                                session.presenter_opening = false;
                                cx.notify();
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false);
                    if !kept {
                        cx.update_window(handle.into(), |_, window, _| window.remove_window())
                            .ok();
                    }
                }
                Err(error) => {
                    owner
                        .update(cx, |this, cx| {
                            if let Some(session) = &mut this.motion.session
                                && session.instance == instance
                            {
                                session.presenter_opening = false;
                                this.set_status(
                                    format!("Unable to open presenter view: {error}"),
                                    true,
                                    cx,
                                );
                            }
                        })
                        .ok();
                }
            }
            cx.refresh_windows();
        });
    }
}

struct Presenter {
    owner: WeakEntity<EditorView>,
    audience: AnyWindowHandle,
    focus: FocusHandle,
    images: HashMap<PageId, (u64, Arc<RenderImage>)>,
    loading: HashSet<PageId>,
    _watch: Subscription,
    _timer: Task<()>,
}
impl Presenter {
    fn new(
        owner: WeakEntity<EditorView>,
        audience: AnyWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let watch = cx.observe(&owner.upgrade().expect("presenter owner"), |_, _, cx| {
            cx.notify()
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let timer = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        Self {
            owner,
            audience,
            focus,
            images: HashMap::new(),
            loading: HashSet::new(),
            _watch: watch,
            _timer: timer,
        }
    }
    fn preview(
        &mut self,
        id: PageId,
        revision: u64,
        doc: Document,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        if let Some((old, image)) = self.images.get(&id)
            && *old == revision
        {
            return Some(image.clone());
        }
        if self.loading.insert(id) {
            cx.spawn(async move |this, cx| {
                let (w, h, pixels) = cx
                    .background_spawn(async move { super::history::doc_thumb(&doc, 800) })
                    .await;
                this.update(cx, |this, cx| {
                    this.loading.remove(&id);
                    this.images
                        .insert(id, (revision, Arc::new(viewport::bgra_image(w, h, pixels))));
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        self.images.get(&id).map(|(_, image)| image.clone())
    }
    fn change(&self, delta: isize, cx: &mut Context<Self>) {
        self.owner
            .update(cx, |this, cx| this.presentation_step(delta, cx))
            .ok();
    }
}
impl Render for Presenter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            window.remove_window();
            return div().into_any_element();
        };
        let editor = owner.read(cx);
        if !editor.motion.presenting {
            window.remove_window();
            return div().into_any_element();
        }
        let list = editor.editor.page_list();
        let id = editor.editor.active_page();
        let index = list.iter().position(|page| page.id == id).unwrap_or(0);
        let count = list.len();
        let title = list
            .get(index)
            .map(|page| page.name.clone())
            .unwrap_or_default();
        let notes = editor.editor.doc.design.speaker_notes.clone();
        let error = editor
            .status
            .clone()
            .filter(|(_, error)| *error)
            .map(|(message, _)| message);
        let active = (id, editor.editor.revision, editor.editor.doc.clone());
        let next = list.get(index + 1).and_then(|page| {
            editor
                .editor
                .page(page.id)
                .map(|e| (page.id, e.revision, e.doc.clone()))
        });
        let now = cx.background_executor().now();
        let elapsed = editor
            .motion
            .session
            .as_ref()
            .map(|session| {
                session.elapsed
                    + session
                        .timer_started
                        .map_or(std::time::Duration::ZERO, |started| {
                            now.saturating_duration_since(started)
                        })
            })
            .unwrap_or_default()
            .as_secs();
        let paused = editor
            .motion
            .session
            .as_ref()
            .is_some_and(|session| session.timer_started.is_none());
        let auto = editor.motion.auto_advance;
        let video = editor.design_video_playing();
        let current = self.preview(active.0, active.1, active.2, cx);
        let next = next.and_then(|(id, rev, doc)| self.preview(id, rev, doc, cx));
        let palette = theme::palette(cx);
        let preview_height = (f32::from(window.viewport_size().height) * 0.30).clamp(110., 240.);
        let preview = |id: &'static str, title: &'static str, image: Option<Arc<RenderImage>>| {
            div()
                .id(id)
                .test_support()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(palette.muted)
                        .child(title),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h(px(preview_height))
                        .bg(rgb(0x000000))
                        .overflow_hidden()
                        .when_some(image, |d, image| {
                            d.child(img(image).size_full().object_fit(ObjectFit::Contain))
                        }),
                )
        };
        div()
            .id("design-presenter-window")
            .test_support()
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.paper)
            .text_color(palette.ink)
            .font_family(theme::UI_FONT)
            .text_size(px(14.))
            .p_4()
            .gap_3()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let delta = match event.keystroke.key.as_str() {
                    "left" | "up" | "pageup" => Some(-1),
                    "right" | "down" | "pagedown" | "space" => Some(1),
                    "home" => Some(-100),
                    "end" => Some(100),
                    _ => None,
                };
                if let Some(delta) = delta {
                    this.change(delta, cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" {
                    this.owner
                        .update(cx, |this, cx| {
                            this.stop_motion(cx);
                        })
                        .ok();
                    cx.stop_propagation();
                }
            }))
            .child(
                TitleBar::new()
                    .child("Presenter")
                    .on_close_window(cx.listener(|this, _, _, cx| {
                        this.owner
                            .update(cx, |this, cx| {
                                this.stop_motion(cx);
                            })
                            .ok();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(format!("Page {} / {count} · {title}", index + 1))
                    .child(
                        div()
                            .id("presenter-timer")
                            .test_support()
                            .font_family(MONO_FONT)
                            .child(format!("{:02}:{:02}", elapsed / 60, elapsed % 60)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(preview("presenter-current-slide", "Current slide", current))
                    .child(preview("presenter-next-slide", "Next slide", next)),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(palette.muted)
                    .child("Speaker notes — visible only here"),
            )
            .child(
                div()
                    .id("presenter-speaker-notes")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .bg(palette.panel)
                    .child(if notes.is_empty() {
                        "No notes for this page.".into()
                    } else {
                        notes
                    }),
            )
            .when_some(error, |d, error| {
                d.child(
                    div()
                        .id("presenter-playback-error")
                        .test_support()
                        .text_color(palette.ink)
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("presenter-prev")
                            .label("Previous")
                            .outline()
                            .disabled(index == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.change(-1, cx))),
                    )
                    .child(
                        Button::new("presenter-next")
                            .label("Next")
                            .outline()
                            .disabled(index + 1 >= count)
                            .on_click(cx.listener(|this, _, _, cx| this.change(1, cx))),
                    )
                    .child(
                        Button::new("presenter-auto")
                            .label(if auto {
                                "Auto advance: on"
                            } else {
                                "Auto advance: off"
                            })
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.owner
                                    .update(cx, |this, cx| {
                                        this.motion.auto_advance = !this.motion.auto_advance;
                                        this.resume_presentation_advance(cx);
                                        cx.notify();
                                    })
                                    .ok();
                            })),
                    )
                    .when(video, |d| {
                        d.child(
                            Button::new("presenter-stop-video")
                                .label("Stop video")
                                .small()
                                .outline()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.owner
                                        .update(cx, |this, cx| {
                                            this.stop_design_video(cx);
                                            this.resume_presentation_advance(cx);
                                            cx.notify();
                                        })
                                        .ok();
                                })),
                        )
                    })
                    .child(
                        Button::new("presenter-timer-pause")
                            .label(if paused {
                                "Resume timer"
                            } else {
                                "Pause timer"
                            })
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.owner
                                    .update(cx, |this, cx| {
                                        if let Some(session) = &mut this.motion.session {
                                            let now = cx.background_executor().now();
                                            if let Some(started) = session.timer_started.take() {
                                                session.elapsed +=
                                                    now.saturating_duration_since(started);
                                            } else {
                                                session.timer_started = Some(now);
                                            }
                                        }
                                        cx.notify();
                                    })
                                    .ok();
                            })),
                    )
                    .child(
                        Button::new("presenter-timer-reset")
                            .label("Reset timer")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.owner
                                    .update(cx, |this, cx| {
                                        if let Some(session) = &mut this.motion.session {
                                            session.elapsed = std::time::Duration::ZERO;
                                            session.timer_started =
                                                Some(cx.background_executor().now());
                                        }
                                        cx.notify();
                                    })
                                    .ok();
                            })),
                    )
                    .child(
                        Button::new("presenter-audience")
                            .label("Show audience")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.update_window(this.audience, |_, window, _| {
                                    window.activate_window()
                                })
                                .ok();
                            })),
                    )
                    .child(
                        Button::new("presenter-exit")
                            .label("End presentation")
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.owner
                                    .update(cx, |this, cx| {
                                        this.stop_motion(cx);
                                    })
                                    .ok();
                            })),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use gpui_kit::test::TestWindowExt;

    #[gpui_kit::test]
    fn presentation_host_runs_during_assistant_and_restores_without_edits(cx: &mut TestAppContext) {
        use emulsion_mcp::design_motion_tools::{Direction, HostAction};
        let (workspace, cx) = crate::tests::open(cx, Document::new(400, 300));
        let view = cx.update(|window, cx| {
            let mut project =
                ProjectEditor::new_project(ProjectKind::Design, Document::new(400, 300)).unwrap();
            let first = project.active_page();
            project
                .add_page(Document::new(600, 400), "Second".into(), 0.)
                .unwrap();
            project.set_active_page(first).unwrap();
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(project, "Host slides".into(), window, cx)
            });
            workspace.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        let (first, stamp, saved_view) = cx.update(|window, cx| {
            view.update(cx, |this, cx| {
                this.assistant.running = true;
                this.view = View {
                    zoom: 1.7,
                    center: (123., 98.),
                    rotation: 12.,
                    ..Default::default()
                };
                this.fit_pending = false;
                let original = (this.editor.active_page(), this.editor.stamp(), this.view);
                assert!(
                    this.presentation_host_action(
                        HostAction::Navigate(Direction::Next),
                        window,
                        cx
                    )
                    .is_err()
                );
                let result = this
                    .presentation_host_action(
                        HostAction::Start {
                            fullscreen: true,
                            presenter: false,
                            auto_advance: false,
                        },
                        window,
                        cx,
                    )
                    .unwrap();
                assert_eq!(result["presenting"], true);
                assert_eq!(result["presenter_timer"]["paused"], false);
                let state = this
                    .presentation_host_action(HostAction::TimerPaused(true), window, cx)
                    .unwrap();
                assert_eq!(state["presenter_timer"]["paused"], true);
                let elapsed = state["presenter_timer"]["elapsed_ms"].clone();
                assert_eq!(
                    this.presentation_host_action(HostAction::TimerPaused(true), window, cx)
                        .unwrap()["presenter_timer"]["elapsed_ms"],
                    elapsed
                );
                let reset = this
                    .presentation_host_action(HostAction::TimerReset, window, cx)
                    .unwrap();
                assert_eq!(reset["presenter_timer"]["elapsed_ms"], 0);
                assert_eq!(reset["presenter_timer"]["paused"], false);
                assert!(
                    this.presentation_host_action(HostAction::MediaPause, window, cx)
                        .is_err()
                );
                assert!(window.is_fullscreen());
                original
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |this, cx| {
                this.presentation_host_action(HostAction::Navigate(Direction::Last), window, cx)
                    .unwrap();
                assert_ne!(this.editor.active_page(), first);
                let last = this.editor.active_page();
                let state = this
                    .presentation_host_action(HostAction::State, window, cx)
                    .unwrap();
                assert_eq!(state["page_index"], 2);
                assert_eq!(state["page_count"], 2);
                assert_eq!(state["fullscreen"], true);
                this.presentation_host_action(HostAction::Navigate(Direction::Next), window, cx)
                    .unwrap();
                assert_eq!(this.editor.active_page(), last);
                this.presentation_host_action(
                    HostAction::Navigate(Direction::Previous),
                    window,
                    cx,
                )
                .unwrap();
                assert_eq!(this.editor.active_page(), first);
                this.presentation_host_action(HostAction::Navigate(Direction::Next), window, cx)
                    .unwrap();
                assert_eq!(this.editor.active_page(), last);
                this.presentation_host_action(HostAction::Navigate(Direction::First), window, cx)
                    .unwrap();
                assert_eq!(this.editor.active_page(), first);
                this.presentation_host_action(HostAction::Fullscreen(false), window, cx)
                    .unwrap();
                assert!(!window.is_fullscreen());
                this.presentation_host_action(HostAction::Fullscreen(true), window, cx)
                    .unwrap();
                assert!(window.is_fullscreen());
                this.presentation_host_action(HostAction::Navigate(Direction::Last), window, cx)
                    .unwrap();
                this.presentation_host_action(
                    HostAction::Start {
                        fullscreen: false,
                        presenter: false,
                        auto_advance: false,
                    },
                    window,
                    cx,
                )
                .unwrap();
                assert!(!window.is_fullscreen());
                this.presentation_host_action(HostAction::End, window, cx)
                    .unwrap();
                assert_eq!(this.editor.active_page(), first);
                assert_eq!(this.editor.stamp(), stamp);
                assert_eq!(this.view, saved_view);
                assert!(!this.presentation_active());
                this.assistant.running = false;
            })
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn fullscreen_audience_hides_chrome_blocks_edits_and_restores_start_page(
        cx: &mut TestAppContext,
    ) {
        let (workspace, cx) = crate::tests::open(cx, Document::new(400, 300));
        let view = cx.update(|window, cx| {
            let mut project = ProjectEditor::new_project(
                ProjectKind::Design,
                emulsion_core::design::Template::Announcement
                    .create(400, 300)
                    .unwrap(),
            )
            .unwrap();
            let first = project.active_page();
            project
                .add_page(Document::new(600, 400), "Second".into(), 0.)
                .unwrap();
            project.set_active_page(first).unwrap();
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(project, "Slides".into(), window, cx)
            });
            workspace.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        let (original, first, original_selection, original_primary, original_view) =
            cx.update(|window, cx| {
                view.update(cx, |this, cx| {
                    let stamp = this.editor.stamp();
                    let first = this.editor.active_page();
                    let selected: Vec<_> =
                        this.editor.doc.nodes.iter().take(2).map(|n| n.id).collect();
                    let primary = selected.last().copied();
                    this.set_layer_selection(selected.clone(), primary);
                    let original_view = View {
                        zoom: 1.7,
                        center: (103., 82.),
                        rotation: 12.,
                        ..Default::default()
                    };
                    this.view = original_view;
                    this.fit_pending = false;
                    this.start_motion(true, cx);
                    window.focus(&this.canvas_focus, cx);
                    (stamp, first, selected, primary, original_view)
                })
            });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("presentation-fullscreen", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.is_fullscreen());
            assert!(window.find("design-presentation").visible());
            for id in [
                "presentation-controls",
                "design-rail",
                "presentation-exit",
                "presentation-fullscreen",
            ] {
                assert!(window.try_find(id).is_none(), "audience leaked {id}");
            }
            assert!(view.read(cx).is_clean_presentation(window));
        });
        for viewport in [size(px(1600.), px(900.)), size(px(1000.), px(1200.))] {
            cx.simulate_resize(viewport);
            cx.run_until_parked();
            cx.update(|window, cx| {
                let editor = view.read(cx);
                let bounds = window.find("canvas").bounds();
                let expected = (f64::from(f32::from(bounds.size.width)) / 400.)
                    .min(f64::from(f32::from(bounds.size.height)) / 300.);
                assert!(expected > 1., "small slides must upscale in audience mode");
                assert!((editor.view.zoom - expected).abs() < 1e-8);
                assert_eq!(editor.view.center, (200., 150.));
                assert_eq!(editor.view.rotation, 0.);
                assert!(!editor.fit_pending);
                let top_left = editor.view.doc_to_screen((0., 0.), &bounds);
                let bottom_right = editor.view.doc_to_screen((400., 300.), &bounds);
                let displayed = (bottom_right.0 - top_left.0, bottom_right.1 - top_left.1);
                assert!(
                    (displayed.0 - f64::from(f32::from(bounds.size.width))).abs() < 1e-6
                        || (displayed.1 - f64::from(f32::from(bounds.size.height))).abs() < 1e-6,
                    "audience fit must have no editing margin"
                );
            });
        }
        cx.simulate_keystrokes("right");
        cx.run_until_parked();
        cx.update(|_, cx| assert_ne!(view.read(cx).editor.active_page(), first));
        cx.simulate_keystrokes("ctrl-z delete backspace ctrl-n ctrl-v");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.stamp(), original));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(!window.is_fullscreen());
            assert!(window.find("design-rail").visible());
            let editor = view.read(cx);
            assert_eq!(editor.editor.active_page(), first);
            assert_eq!(editor.editor.stamp(), original);
            assert_eq!(editor.selected_layer_ids(), original_selection);
            assert_eq!(editor.selected, original_primary);
            assert_eq!(editor.view, original_view);
            assert!(!editor.fit_pending);
            assert!(!editor.motion.presenting);
        });
        cx.update(|window, cx| {
            view.update(cx, |this, cx| {
                this.start_motion(true, cx);
                window.focus(&this.canvas_focus, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("presentation-exit", cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).view, original_view));
    }

    #[gpui_kit::test]
    fn speaker_notes_cancel_is_nonmutating_and_presenter_keeps_notes_off_audience(
        cx: &mut TestAppContext,
    ) {
        let mut doc = Document::new(400, 300);
        doc.design.speaker_notes = "Private speaker notes".into();
        let original = doc.clone();
        let (workspace, cx) = crate::tests::open(cx, doc.clone());
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                    "Notes".into(),
                    window,
                    cx,
                )
            });
            workspace.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        cx.update(|window, cx| view.update(cx, |this, cx| this.speaker_notes_dialog(window, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window
                .within("design-speaker-notes-footer")
                .click("close", cx)
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).editor.doc, original);
            window.click("design-animate", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-speaker-notes", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-speaker-notes-input", cx));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        cx.simulate_input("Updated speaker notes\nSecond talking point");
        cx.run_until_parked();
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |this, cx| {
                assert_eq!(
                    this.editor.doc.design.speaker_notes,
                    "Updated speaker notes\nSecond talking point"
                );
                this.undo(cx);
                assert_eq!(this.editor.doc, original);
                this.redo(cx);
                assert_eq!(
                    this.editor.doc.design.speaker_notes,
                    "Updated speaker notes\nSecond talking point"
                );
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-page-transition", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(2usize, cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(
                view.read(cx).editor.doc.design.page_transition,
                PageTransition::Slide
            );
            window.click("design-transition-duration", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-transition-duration-input", cx));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        cx.simulate_input("700");
        cx.run_until_parked();
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        let original = cx.update(|_, cx| {
            view.update(cx, |this, cx| {
                assert_eq!(this.editor.doc.design.transition_ms, 700);
                this.undo(cx);
                assert_eq!(this.editor.doc.design.transition_ms, 400);
                assert_eq!(
                    this.editor.doc.design.page_transition,
                    PageTransition::Slide
                );
                this.undo(cx);
                assert_eq!(this.editor.doc.design.page_transition, PageTransition::None);
                assert_eq!(
                    this.editor.doc.design.speaker_notes,
                    "Updated speaker notes\nSecond talking point"
                );
                this.redo(cx);
                this.redo(cx);
                assert_eq!(this.editor.doc.design.transition_ms, 700);
                assert_eq!(
                    this.editor.doc.design.page_transition,
                    PageTransition::Slide
                );
                this.editor.doc.clone()
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |this, cx| {
                assert_eq!(this.editor.doc, original);
                this.open_presenter(window, cx);
            })
        });
        cx.run_until_parked();
        let presenter = cx.update(|_, cx| {
            view.read(cx)
                .motion
                .session
                .as_ref()
                .unwrap()
                .presenter
                .unwrap()
        });
        cx.update(|window, cx| {
            assert!(window.is_fullscreen());
            assert!(window.try_find("presenter-speaker-notes").is_none());
            cx.update_window(presenter, |_, window, cx| {
                window.render_frame(cx);
                assert!(window.find("presenter-speaker-notes").visible());
                assert!(window.find("presenter-current-slide").visible());
                assert!(window.find("presenter-next-slide").visible());
                assert!(window.find("presenter-timer").visible());
                window.click("presenter-exit", cx);
            })
            .unwrap();
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(!window.is_fullscreen());
            assert!(!view.read(cx).motion.presenting);
            assert_eq!(view.read(cx).editor.doc, original);
            assert!(!cx.windows().contains(&presenter));
        });
        let windows_before = cx.update(|_, cx| cx.windows().len());
        cx.update(|window, cx| {
            view.update(cx, |this, cx| {
                this.open_presenter(window, cx);
                this.stop_motion(cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(
                cx.windows().len(),
                windows_before,
                "a cancelled presenter request must not open a late window"
            );
            assert!(!window.is_fullscreen());
            assert!(view.read(cx).motion.session.is_none());
            assert_eq!(view.read(cx).editor.doc, original);
        });
    }
    #[gpui_kit::test]
    fn presentation_gpu_cache_is_separate_and_replaced_between_runs(cx: &mut TestAppContext) {
        let (workspace, cx) = crate::tests::open(cx, Document::new(400, 300));
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, Document::new(400, 300))
                        .unwrap(),
                    "GPU slides".into(),
                    window,
                    cx,
                )
            });
            workspace.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        cx.update(|_, cx| view.update(cx, |this, cx| this.start_motion(true, cx)));
        cx.run_until_parked();
        let (static_key, static_generation) = cx.update(|_, cx| {
            let editor = view.read(cx);
            (
                editor.presentation_gpu_frame().unwrap().1,
                editor.render_gen,
            )
        });
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(4));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = view.read(cx);
            assert_eq!(editor.presentation_gpu_frame().unwrap().1, static_key);
            assert_eq!(
                editor.render_gen, static_generation,
                "static slides must not rebuild their rendering tree as the page timer advances"
            );
        });
        let (first_key, first_cache) = cx.update(|_, cx| {
            view.update(cx, |this, cx| {
                let (doc, key, status) = this
                    .presentation_gpu_frame()
                    .expect("evaluated presentation");
                assert_eq!(doc, this.editor.doc);
                assert!(!Rc::ptr_eq(&status, &this.gpu_canvas));
                assert_eq!(this.presentation_gpu_frame().unwrap().1, key);
                this.stop_motion(cx);
                assert!(this.presentation_gpu_frame().is_none());
                assert!(!Rc::ptr_eq(&status, &this.motion.preview_gpu));
                this.start_motion(true, cx);
                (key, status)
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |this, cx| {
                let (_, key, status) = this.presentation_gpu_frame().unwrap();
                assert_ne!(key, first_key);
                assert!(!Rc::ptr_eq(&status, &first_cache));
                assert!(!Rc::ptr_eq(&status, &this.gpu_canvas));
                this.stop_motion(cx);
            })
        });
    }
}

#[cfg(test)]
#[path = "design_interaction_workflow_tests.rs"]
mod interaction_tests;
