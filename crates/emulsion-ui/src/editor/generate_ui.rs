//! Generative fill and text-to-image in the editor: a prompt field in the
//! Select tool's options. With a selection, the prompt fills it (the
//! picture around the selection goes to the image server as context);
//! without one, it generates a whole new layer the size of the canvas.

use super::*;
use crate::widgets::tip;
use emulsion_ai::generate::{self, Config, Provider};
use emulsion_ai::jobs::Job;
use emulsion_raster::IRect;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
};

#[derive(Default)]
pub struct GenState {
    prompt: Option<(Entity<InputState>, Subscription)>,
    pub busy: bool,
    pub show_in_taskbar: bool,
    /// F1 choice, retained when its prompt is closed and reopened.
    pub ask_provider: Option<Provider>,
}

/// The image server settings, when one is configured.
pub(crate) fn config(cx: &App) -> Option<Config> {
    let s = crate::app_state::settings(cx);
    let provider = generate::Provider::parse(&s.image_provider)?;
    Some(config_for(provider, cx))
}

pub(crate) fn config_for(provider: Provider, cx: &App) -> Config {
    let s = crate::app_state::settings(cx);
    let (endpoint, model) = match provider {
        Provider::A1111 => (s.image_endpoint.clone(), s.image_model.clone()),
        Provider::OpenAi => (None, s.openai_image_model.clone()),
        Provider::Google => (None, s.google_image_model.clone()),
    };
    Config {
        provider,
        endpoint,
        model,
        api_key: s.image_key(provider.id()).map(|(key, _)| key),
    }
}

impl EditorView {
    pub(crate) fn ensure_gen_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.generate.prompt.is_some() {
            return;
        }
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Describe the fill, or leave blank to remove the selection")
        });
        let sub = cx.subscribe_in(&state, window, |this, _st, ev: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.generate_from_prompt(cx);
            }
        });
        self.generate.prompt = Some((state, sub));
    }

    fn prompt_text(&self, cx: &App) -> String {
        self.generate
            .prompt
            .as_ref()
            .map(|(st, _)| st.read(cx).value().trim().to_string())
            .unwrap_or_default()
    }

    /// Fill the selection from the prompt, or generate a full-canvas layer.
    pub(crate) fn generate_from_prompt(&mut self, cx: &mut Context<Self>) {
        let prompt = self.prompt_text(cx);
        let Some(cfg) = config(cx) else {
            self.set_status(
                "No image server is set up: choose one under Settings › Image generation.",
                true,
                cx,
            );
            return;
        };
        if let Err(error) = self.generate_text(prompt, cfg, cx) {
            self.set_status(error, true, cx);
        }
    }

    /// Shared by F1 and the Select tool. Validation leaves the input intact.
    pub(crate) fn generate_text(
        &mut self,
        prompt: String,
        cfg: Config,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let prompt = prompt.trim().to_string();
        let removing = prompt.is_empty();
        let sel = self.editor.doc.selection.clone();
        if removing && sel.is_none() {
            return Err("Select an object to remove, or type what to generate.".into());
        }
        if sel.as_ref().is_some_and(|mask| {
            emulsion_raster::select::bounds(mask)
                .intersect(&IRect::new(
                    0,
                    0,
                    self.editor.doc.width as i32,
                    self.editor.doc.height as i32,
                ))
                .is_empty()
        }) {
            return Err("Select an area of the image to fill.".into());
        }
        let prompt = if removing {
            "Remove the object inside the masked area. Fill the area with a natural continuation of the surrounding background, matching its lighting, texture and perspective. Preserve the rest of the image.".to_string()
        } else {
            prompt
        };
        if self.generate.busy {
            return Err("Still generating the last request.".into());
        }
        if self.assistant.running || self.editor.in_transaction() {
            return Err("Finish the current edit before generating an image.".into());
        }
        if self.ai.job.as_ref().is_some_and(|job| !job.is_finished()) {
            return Err("Wait for the current image task to finish.".into());
        }
        cfg.validate().map_err(|error| error.to_string())?;
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        // The displayed tree may lag edits while rendering catches up.
        let source = sel.as_ref().map(|_| self.editor.doc.clone());
        let job = Job::new();
        self.watch_job(job.clone(), "Generating an image", cx);
        let j = job.clone();
        let slot = self.insertion_slot();
        let label = if removing {
            "Generative removal".to_string()
        } else {
            format!("Generated: {}", short_prompt(&prompt))
        };
        let model_id = cfg.model_id();
        self.generate.busy = true;
        self.set_status(
            if sel.is_some() {
                "Generating the fill…"
            } else {
                "Generating a new layer…"
            },
            false,
            cx,
        );
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    match &sel {
                        Some(s) => {
                            let img = emulsion_raster::composite::flatten(
                                &source.expect("fill document snapshot").composite_tree(),
                                0,
                            );
                            generate::fill(&cfg, &img, s, &prompt, None, &j)
                        }
                        None => generate::text_to_image(&cfg, &prompt, None, w, h, &j)
                            .map(|r| (r, IRect::new(0, 0, w as i32, h as i32))),
                    }
                })
                .await;
            // A brush/slider edit may have started while the provider worked.
            // Let it commit first so generation keeps its own undo step.
            if r.is_ok() {
                loop {
                    let waiting = this.update(cx, |this, _| {
                        this.editor.in_transaction() && !job.cancelled()
                    });
                    if !matches!(waiting, Ok(true)) {
                        break;
                    }
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(50))
                        .await;
                }
            }
            job.finish();
            this.update(cx, |this, cx| {
                this.generate.busy = false;
                if job.cancelled() {
                    this.set_status("Image generation cancelled.", false, cx);
                    return;
                }
                match r {
                    Ok((layer, reg)) => {
                        let node = Node::raster(
                            0,
                            label,
                            Arc::new(layer),
                            Placement::at(reg.x as f64, reg.y as f64),
                        )
                        .from_model(&model_id);
                        if let Some(id) = this.execute(
                            Command::AddNode {
                                node: Box::new(node),
                                slot,
                            },
                            cx,
                        ) {
                            this.set_layer_selection(vec![id], Some(id));
                            this.set_status(
                                "Generated into a new layer. Hide it to compare.",
                                false,
                                cx,
                            );
                        }
                    }
                    Err(e) => this.set_status(format!("Generate: {e}"), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        Ok(())
    }

    /// The prompt field and its button for the options bar.
    pub(crate) fn generate_row(&mut self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut v = Vec::new();
        if self.generate.show_in_taskbar {
            return v;
        }
        let Some((st, _)) = &self.generate.prompt else {
            return v;
        };
        v.push(self.group("generate", p));
        v.push(
            div()
                .w(px(300.))
                .border_1()
                .border_color(p.line)
                .child(Input::new(st).appearance(false).bordered(false))
                .into_any_element(),
        );
        let has_sel = self.editor.doc.selection.is_some();
        let busy = self.generate.busy;
        v.push(
            tip(
                chip(
                    "gen-go",
                    if busy {
                        "generating…"
                    } else if has_sel {
                        "fill selection"
                    } else {
                        "new layer"
                    },
                    busy,
                    p,
                )
                .on_click(cx.listener(|this, _, _, cx| this.generate_from_prompt(cx))),
                "Ask the image server to paint this. Set the server under Settings › Image generation.",
            )
            .into_any_element(),
        );
        v
    }

    pub(super) fn open_generative_fill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_gen_prompt(window, cx);
        self.generate.show_in_taskbar = true;
        if let Some((state, _)) = &self.generate.prompt {
            state.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    pub(super) fn generation_taskbar_actions(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.generate.show_in_taskbar || self.tool != Tool::Select {
            return Vec::new();
        }
        let Some((state, _)) = &self.generate.prompt else {
            return Vec::new();
        };
        vec![
            div()
                .w_80()
                .child(Input::new(state).small())
                .into_any_element(),
            Button::new("context-generate-submit")
                .small()
                .primary()
                .label(if self.generate.busy {
                    "Generating…"
                } else {
                    "Generate"
                })
                .disabled(self.generate.busy)
                .on_click(cx.listener(|this, _, _, cx| this.generate_from_prompt(cx)))
                .into_any_element(),
            Button::new("context-generate-cancel")
                .small()
                .ghost()
                .label("Cancel")
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.generate.busy {
                        this.cancel_ai(cx);
                    }
                    this.generate.show_in_taskbar = false;
                    window.focus(&this.canvas_focus, cx);
                    cx.notify();
                }))
                .into_any_element(),
        ]
    }
}

fn short_prompt(p: &str) -> String {
    let s: String = p.chars().take(40).collect();
    if s.len() < p.len() {
        format!("{s}…")
    } else {
        s
    }
}
