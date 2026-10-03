//! AI image tools on storyboard panels (Edit → AI on the Stage, and AI on
//! selected panels… on the Board): subject masks, background removal,
//! upscale, denoise, expand and generative fill through
//! `emulsion_ai::panels`. The panel size never changes. One panel or many
//! run off the UI thread with the AI progress card and Cancel, report an
//! error per panel, and land as one Undo step.
use super::*;
use crate::widgets::{chip, mono};
use emulsion_ai::jobs::Job;
use emulsion_ai::panels::{self, Area, Backend, MaskOutput, Models, Op, Request, Target};
use emulsion_core::project::PageId;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{PopupMenu, PopupMenuItem},
};
use std::sync::Mutex;

/// The operations the dialog offers, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    SelectSubject,
    SubjectMask,
    RemoveBackground,
    Upscale,
    Denoise,
    Expand,
    Fill,
}

impl Kind {
    const ALL: [Kind; 7] = [
        Kind::SelectSubject,
        Kind::SubjectMask,
        Kind::RemoveBackground,
        Kind::Upscale,
        Kind::Denoise,
        Kind::Expand,
        Kind::Fill,
    ];

    fn label(self) -> &'static str {
        match self {
            Kind::SelectSubject => "Select subject",
            Kind::SubjectMask => "Subject mask",
            Kind::RemoveBackground => "Remove background",
            Kind::Upscale => "Upscale",
            Kind::Denoise => "Denoise",
            Kind::Expand => "Expand",
            Kind::Fill => "Generative fill",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Kind::SelectSubject => "The subject becomes each panel's selection.",
            Kind::SubjectMask => "A layer mask on the layer, around its subject.",
            Kind::RemoveBackground => {
                "A cut-out copy of the layer above it; the original is hidden."
            }
            Kind::Upscale => {
                "A copy of the layer with up to 4× the pixels at the same size on the panel, so it stays sharp when enlarged or under a camera push-in. The panel resolution does not change; the original is hidden."
            }
            Kind::Denoise => "A cleaned copy of the layer; the original is hidden.",
            Kind::Expand => {
                "Shrinks the picture inside the frame and fills the new border; the original is hidden."
            }
            Kind::Fill => "Paints the area into a new layer; the original is kept.",
        }
    }

    fn prompted(self) -> bool {
        matches!(self, Kind::Expand | Kind::Fill)
    }
}

const AMOUNTS: [f32; 3] = [0.1, 0.15, 0.25];

/// The AI dialog: one panel from the Stage, or the Board selection.
pub(crate) struct PanelAiDialog {
    editor: WeakEntity<EditorView>,
    panels: Vec<PageId>,
    pub(crate) kind: Kind,
    layer: Entity<InputState>,
    area_layer: Entity<InputState>,
    prompt: Entity<InputState>,
    pub(crate) area: usize,
    amount: f32,
    /// The configured image provider's name, if any.
    provider: Option<&'static str>,
    job: Option<Arc<Job>>,
    pub(crate) message: Option<String>,
    _subs: Vec<Subscription>,
}

impl PanelAiDialog {
    fn new(
        editor: WeakEntity<EditorView>,
        panels: Vec<PageId>,
        kind: Kind,
        layer: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = |text: String, hint: &str, window: &mut Window, cx: &mut Context<Self>| {
            let hint = hint.to_string();
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(hint)
                    .default_value(text)
            })
        };
        let layer = input(layer, "Whole panel", window, cx);
        let area_layer = input(String::new(), "Layer name, e.g. Sky", window, cx);
        let prompt = input(
            String::new(),
            "Describe what to paint, or leave blank for the local model",
            window,
            cx,
        );
        let subs = [&layer, &area_layer, &prompt]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |_, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                })
            })
            .collect();
        let provider = super::generate_ui::config(cx).map(|c| c.provider.label());
        Self {
            editor,
            panels,
            kind,
            layer,
            area_layer,
            prompt,
            area: 0,
            amount: 0.15,
            provider,
            job: None,
            message: None,
            _subs: subs,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_prompt(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt.update(cx, |input, cx| {
            input.set_value(text.to_string(), window, cx)
        });
    }

    #[cfg(test)]
    pub(crate) fn set_layer(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.layer.update(cx, |input, cx| {
            input.set_value(text.to_string(), window, cx)
        });
    }

    /// The request as entered.
    pub(crate) fn request(&self, cx: &App) -> Result<Request, String> {
        let text = |input: &Entity<InputState>| input.read(cx).value().trim().to_string();
        let layer = text(&self.layer);
        let target = if layer.is_empty() {
            Target::Panel
        } else {
            Target::Named(layer)
        };
        let prompt = self
            .provider
            .and_then(|_| Some(text(&self.prompt)).filter(|p| !p.is_empty()));
        let op = match self.kind {
            Kind::SelectSubject => Op::Subject(MaskOutput::Selection),
            Kind::SubjectMask => {
                if target == Target::Panel {
                    return Err("Name the layer to mask.".into());
                }
                Op::Subject(MaskOutput::LayerMask)
            }
            Kind::RemoveBackground => Op::Subject(MaskOutput::CutOut),
            Kind::Upscale => Op::Upscale,
            Kind::Denoise => Op::Denoise,
            Kind::Expand => Op::Expand {
                amount: self.amount,
                prompt,
            },
            Kind::Fill => Op::Fill {
                area: match self.area {
                    1 => {
                        let name = text(&self.area_layer);
                        if name.is_empty() {
                            return Err("Name the layer that marks the area.".into());
                        }
                        Area::Layer(name)
                    }
                    2 => Area::Whole,
                    _ => Area::Selection,
                },
                prompt,
            },
        };
        Ok(Request { op, target })
    }

    pub(crate) fn run(&mut self, cx: &mut Context<Self>) {
        if self.job.as_ref().is_some_and(|j| !j.is_finished()) {
            return;
        }
        let request = match self.request(cx) {
            Ok(request) => request,
            Err(e) => {
                self.message = Some(e);
                cx.notify();
                return;
            }
        };
        let panels = self.panels.clone();
        let started = self
            .editor
            .update(cx, |e, cx| e.run_panel_ai(panels, request, cx))
            .unwrap_or_else(|_| Err("The storyboard was closed.".into()));
        let (job, outcome) = match started {
            Ok(started) => started,
            Err(e) => {
                self.message = Some(e);
                cx.notify();
                return;
            }
        };
        self.job = Some(job.clone());
        self.message = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(150))
                    .await;
                let done = outcome.lock().ok().and_then(|mut o| o.take());
                let alive = this
                    .update(cx, |this, cx| {
                        if let Some(done) = &done {
                            this.message = Some(done.clone());
                        }
                        cx.notify();
                    })
                    .is_ok();
                if done.is_some() || !alive || (job.is_finished() && job.cancelled()) {
                    break;
                }
            }
        })
        .detach();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(job) = &self.job {
            job.cancel();
        }
        cx.notify();
    }
}

impl Render for PanelAiDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let busy = self.job.as_ref().is_some_and(|j| !j.is_finished());
        let kinds = Kind::ALL.into_iter().map(|kind| {
            chip(
                SharedString::from(format!("panel-ai-{kind:?}")),
                kind.label(),
                self.kind == kind,
                &p,
            )
            .test_support()
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.job.as_ref().is_none_or(|j| j.is_finished()) {
                    this.kind = kind;
                    this.message = None;
                    cx.notify();
                }
            }))
        });
        let scope = match self.panels.len() {
            1 => "1 panel".to_string(),
            n => format!("{n} selected panels"),
        };
        let prompt_note = match self.provider {
            Some(provider) => format!(
                "The prompt and the panel's picture go to {provider} (Settings › Image generation). Leave it blank to fill from the surroundings with the local fill model."
            ),
            None => "Prompts need an image provider, set under Settings › Image generation. Without one, the local fill model fills from the surroundings; nothing leaves this computer.".to_string(),
        };
        div()
            .id("panel-ai-dialog")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .text_color(p.ink)
            .child(mono(format!("Runs on {scope}."), 10.5, p.muted))
            .child(div().flex().flex_wrap().gap(px(6.)).children(kinds))
            .child(mono(self.kind.blurb(), 10.5, p.muted))
            .child("Layer (blank for the whole panel)")
            .child(Input::new(&self.layer).small().disabled(busy))
            .when(self.kind == Kind::Expand, |d| {
                d.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .children(AMOUNTS.into_iter().map(|a| {
                            chip(
                                SharedString::from(format!(
                                    "panel-ai-amount-{}",
                                    (a * 100.) as u32
                                )),
                                format!("{:.0} % each side", a * 100.),
                                (self.amount - a).abs() < 1e-3,
                                &p,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.amount = a;
                                    cx.notify();
                                },
                            ))
                        })),
                )
            })
            .when(self.kind == Kind::Fill, |d| {
                let areas = ["Each panel's selection", "Pixels of a layer", "Whole frame"];
                d.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .children(areas.into_iter().enumerate().map(|(i, label)| {
                            chip(
                                SharedString::from(format!("panel-ai-area-{i}")),
                                label,
                                self.area == i,
                                &p,
                            )
                            .test_support()
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.area = i;
                                    cx.notify();
                                },
                            ))
                        })),
                )
                .when(self.area == 1, |d| {
                    d.child(Input::new(&self.area_layer).small().disabled(busy))
                })
            })
            .when(self.kind.prompted(), |d| {
                d.when(self.provider.is_some(), |d| {
                    d.child("Prompt")
                        .child(Input::new(&self.prompt).small().disabled(busy))
                })
                .child(
                    div()
                        .id("panel-ai-prompt-note")
                        .test_support()
                        .whitespace_normal()
                        .text_color(p.muted)
                        .child(prompt_note),
                )
            })
            .when_some(
                self.job
                    .as_ref()
                    .filter(|_| busy)
                    .map(|j| j.summary())
                    .or(self.message.clone()),
                |d, m| {
                    d.child(
                        div()
                            .id("panel-ai-message")
                            .test_support()
                            .whitespace_normal()
                            .child(m),
                    )
                },
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .when(busy, |d| {
                        d.child(
                            Button::new("panel-ai-cancel")
                                .label("Cancel")
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                        )
                    })
                    .child(
                        Button::new("panel-ai-close")
                            .label("Close")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("panel-ai-run")
                            .label("Run")
                            .small()
                            .primary()
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.run(cx))),
                    ),
            )
    }
}

/// Where a finished run leaves its report for the dialog.
pub(crate) type Outcome = Arc<Mutex<Option<String>>>;

impl EditorView {
    /// Open the AI dialog on the Board selection (`board`) or the active
    /// panel, starting at `kind`.
    pub(crate) fn storyboard_ai_dialog(
        &mut self,
        kind: Kind,
        board: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<PanelAiDialog>> {
        self.editor.storyboard()?;
        let panels = if board {
            self.board_selection()
        } else {
            vec![self.editor.active_page()]
        };
        let layer = if board {
            String::new()
        } else {
            self.selected
                .and_then(|id| self.editor.doc.node(id))
                .map(|n| n.name.clone())
                .unwrap_or_default()
        };
        let editor = cx.entity().downgrade();
        let view = cx.new(|cx| PanelAiDialog::new(editor, panels, kind, layer, window, cx));
        let shown = view.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("AI on panels")
                .width(px(560.))
                .child(shown.clone())
        });
        Some(view)
    }

    /// Edit menu entries on a storyboard.
    pub(super) fn storyboard_ai_menu_items(menu: PopupMenu, owner: &WeakEntity<Self>) -> PopupMenu {
        let item = |label: &str, kind: Option<Kind>| {
            let owner = owner.clone();
            PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
                owner
                    .update(cx, |e, cx| match kind {
                        Some(kind) => {
                            e.storyboard_ai_dialog(kind, e.board_open(), window, cx);
                        }
                        None => e.select_subject(cx),
                    })
                    .ok();
            })
        };
        menu.separator()
            .item(item("Select Subject", None))
            .item(item("Remove Background…", Some(Kind::RemoveBackground)))
            .item(item("Expand Panel Image…", Some(Kind::Expand)))
            .item(item("Upscale Layer…", Some(Kind::Upscale)))
            .item(item("Generative Fill…", Some(Kind::Fill)))
            .item(item("AI on Panels…", Some(Kind::SelectSubject)))
    }

    /// Run `request` on the active panel straight away, on the selected
    /// layer when there is one (Paint's Upscale and Expand on a panel).
    pub(crate) fn panel_ai_now(&mut self, op: Op, cx: &mut Context<Self>) {
        let target = self
            .selected
            .filter(|id| self.editor.doc.node(*id).is_some())
            .map_or(Target::Panel, Target::Layer);
        let panels = vec![self.editor.active_page()];
        if let Err(e) = self.run_panel_ai(panels, Request { op, target }, cx) {
            self.set_status(e, true, cx);
        }
    }

    /// Run `request` on `panels` with the installed models and the image
    /// provider from Settings.
    pub(crate) fn run_panel_ai(
        &mut self,
        panels: Vec<PageId>,
        request: Request,
        cx: &mut Context<Self>,
    ) -> Result<(Arc<Job>, Outcome), String> {
        let backend = Models {
            image: super::generate_ui::config(cx),
        };
        self.run_panel_ai_with(panels, request, Arc::new(backend), cx)
    }

    /// Run `request` on `panels` off the UI thread and commit the panels
    /// that succeeded as one Undo step. The report also goes to the status
    /// bar.
    pub(crate) fn run_panel_ai_with(
        &mut self,
        panels: Vec<PageId>,
        request: Request,
        backend: Arc<dyn Backend + Send>,
        cx: &mut Context<Self>,
    ) -> Result<(Arc<Job>, Outcome), String> {
        if self.ai.job.as_ref().is_some_and(|j| !j.is_finished()) {
            return Err("Wait for the current AI task to finish.".into());
        }
        if !self.prepare_page_action(cx) {
            return Err("Finish the current edit first.".into());
        }
        let request = Request {
            op: request.op.normalized(),
            ..request
        };
        backend.ready(&request.op)?;
        let (inputs, skipped) = panels::gather(&self.editor, &panels)?;
        let label = request.op.label();
        let job = Job::new();
        self.watch_job(job.clone(), label, cx);
        let outcome: Outcome = Default::default();
        let (worker, slot, handle) = (job.clone(), outcome.clone(), job.clone());
        cx.spawn(async move |this, cx| {
            let work = inputs.clone();
            let report = cx
                .background_spawn(
                    async move { panels::run(&work, &request, backend.as_ref(), &worker) },
                )
                .await;
            // Let a brush stroke that started meanwhile finish first.
            loop {
                let busy = this.update(cx, |this, _| this.editor.in_transaction());
                if !matches!(busy, Ok(true)) {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
            }
            this.update(cx, |this, cx| {
                let message = match report
                    .and_then(|r| panels::commit(&mut this.editor, &inputs, r, skipped, label))
                {
                    Ok(applied) => {
                        let line = applied.summary(label);
                        if !applied.done.is_empty() {
                            this.after_change(cx);
                        }
                        this.set_status(line.clone(), !applied.failed.is_empty(), cx);
                        let mut text = line;
                        for (_, name, why) in &applied.failed {
                            text.push_str(&format!("\n{name}: {why}"));
                        }
                        text
                    }
                    Err(e) => {
                        this.set_status(format!("{label}: {e}"), true, cx);
                        e
                    }
                };
                job.finish();
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(message);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        Ok((handle, outcome))
    }
}

#[cfg(test)]
#[path = "storyboard_ai_tests.rs"]
mod tests;
