//! Settings: the capability ladder. Every tier is optional; each one shows
//! what it unlocks and whether it is available.

use crate::app_state::{self, CliStatus};
use crate::theme::{self, Palette};
use crate::widgets::{button, chip, label, mono};
use crate::workspace::Workspace;
use gpui_kit::component::input::Input;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

const MODELS: [(&str, Option<&str>); 4] = [
    ("CLI default", None),
    ("sonnet", Some("sonnet")),
    ("opus", Some("opus")),
    ("haiku", Some("haiku")),
];

fn tier(n: u8, title: &str, on: bool, state: &str, p: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(26.))
                .border_1()
                .border_color(if on { p.accent } else { p.line })
                .bg(if on { p.accent } else { transparent_black() })
                .text_color(if on { gpui_kit::white() } else { p.muted })
                .font_family(theme::MONO_FONT)
                .text_size(px(11.))
                .child(n.to_string()),
        )
        .child(
            div()
                .text_size(px(17.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_string()),
        )
        .child(mono(
            state.to_uppercase(),
            9.5,
            if on { p.accent } else { p.muted },
        ))
}

impl Workspace {
    pub(crate) fn settings_screen(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = theme::palette(cx);
        self.ensure_settings_inputs(window, cx);
        let s = app_state::settings(cx).clone();
        let cli = app_state::cli(cx);
        let jev = s.jev_key();
        let section = |p: &Palette| {
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .px(px(40.))
                .py(px(24.))
                .border_b_1()
                .border_color(p.line)
        };
        let body = |t: &str, p: &Palette| {
            div()
                .max_w(px(640.))
                .text_size(px(13.))
                .text_color(p.muted)
                .child(t.to_string())
        };

        let (cli_on, cli_state, cli_line) = match &cli {
            CliStatus::Checking => (false, "checking", "Looking for Claude Code…".to_string()),
            CliStatus::Found { path, version } => {
                (true, "on", format!("{} · {}", version, path.display()))
            }
            CliStatus::Missing => (
                false,
                "not found",
                format!(
                    "Claude Code was not found. Install it with: {}",
                    emulsion_assistant::provider::default_provider().install_hint
                ),
            ),
        };

        let cli_path = self.settings_inputs.as_ref().map(|i| i.0.clone());
        let jev_input = self.settings_inputs.as_ref().map(|i| i.1.clone());

        div()
            .id("settings")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .px(px(40.))
                    .pt(px(44.))
                    .pb(px(24.))
                    .border_b_1()
                    .border_color(p.line)
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(label("Settings · capability ladder", &p))
                    .child(div().text_size(px(40.)).font_weight(FontWeight::SEMIBOLD).child("Every tier is optional."))
                    .child(body("Emulsion is a complete editor with none of these. Each tier you add makes it better, and anything that needs a missing tier falls back or stays hidden.", &p)),
            )
            .child(
                section(&p)
                    .child(tier(0, "Built in", true, "on", &p))
                    .child(body("Nodes, masks, blend modes, adjustments, OpenRaster and image formats, the offline request planner behind Ctrl+K, and suggestions from image statistics.", &p))
                    .child(
                        div().flex().gap(px(8.)).child(chip("sugg", "suggestions", s.suggestions, &p).on_click(cx.listener(|_, _, _, cx| {
                            app_state::update_settings(cx, |s| s.suggestions = !s.suggestions);
                        }))),
                    ),
            )
            .child(
                section(&p)
                    .child(tier(1, "Local models", false, "not yet", &p))
                    .child(body("Segmentation, background removal, fill and upscaling on your GPU. These arrive in a later release; nothing to set up yet.", &p)),
            )
            .child(
                section(&p)
                    .child(tier(2, "Coding CLI assistant", cli_on, cli_state, &p))
                    .child(body("Multi-step requests from Ctrl+K go to Claude Code, which can only use Emulsion's tools. Every change it proposes is shown as an Apply / Skip card unless you turn on auto-apply.", &p))
                    .child(mono(cli_line, 10.5, if cli_on { p.ink } else { p.accent }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(mono("path", 10., p.muted))
                            .children(cli_path.map(|st| div().w(px(420.)).border_1().border_color(p.line).child(Input::new(&st).appearance(false))))
                            .child(chip("cli-use", "use path", false, &p).on_click(cx.listener(|this, _, _, cx| {
                                let v = this.settings_inputs.as_ref().map(|i| i.0.read(cx).value().to_string()).unwrap_or_default();
                                let v = v.trim().to_string();
                                app_state::update_settings(cx, |s| s.cli_path = (!v.is_empty()).then(|| v.into()));
                                app_state::detect_cli(cx);
                            })))
                            .child(chip("cli-detect", "detect again", false, &p).on_click(cx.listener(|_, _, _, cx| app_state::detect_cli(cx)))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(mono("model", 10., p.muted))
                            .children(MODELS.iter().map(|(label, value)| {
                                let on = s.model.as_deref() == *value;
                                let v = value.map(str::to_string);
                                chip(SharedString::from(format!("model-{label}")), *label, on, &p).on_click(cx.listener(move |_, _, _, cx| {
                                    let v = v.clone();
                                    app_state::update_settings(cx, |s| s.model = v);
                                }))
                            })),
                    )
                    .child(
                        div().flex().items_center().gap(px(8.)).child(mono("confirm", 10., p.muted)).child(
                            chip("auto", "auto-apply non-destructive changes", s.auto_apply, &p).on_click(cx.listener(|_, _, _, cx| {
                                app_state::update_settings(cx, |s| s.auto_apply = !s.auto_apply);
                            })),
                        ),
                    )
                    .child(mono(
                        "The model picks up these settings the next time a document starts an assistant session.",
                        9.5,
                        p.muted,
                    )),
            )
            .child(
                section(&p)
                    .child(tier(3, "Jev decision model", jev.is_some(), if jev.is_some() { "on" } else { "off" }, &p))
                    .child(body("TypeSafe's Jev answers small typed questions with calibrated confidence. With a key, Ctrl+K requests are planned by Jev, which copes with looser phrasing than the offline planner, and only text leaves the machine: the request and node names, never pixels.", &p))
                    .child(mono(
                        match &jev {
                            Some((_, "environment")) => "key: from TYPESAFE_API_KEY".to_string(),
                            Some(_) => "key: saved in settings".to_string(),
                            None => "key: not set".to_string(),
                        },
                        10.5,
                        p.ink,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .children(jev_input.map(|st| div().w(px(420.)).border_1().border_color(p.line).child(Input::new(&st).appearance(false))))
                            .child(chip("jev-save", "save key", false, &p).on_click(cx.listener(|this, _, window, cx| {
                                let v = this.settings_inputs.as_ref().map(|i| i.1.read(cx).value().to_string()).unwrap_or_default();
                                let v = v.trim().to_string();
                                if !v.is_empty() {
                                    app_state::update_settings(cx, |s| s.jev_api_key = Some(v));
                                    if let Some(i) = &this.settings_inputs {
                                        i.1.update(cx, |st, cx| st.set_value("", window, cx));
                                    }
                                }
                            })))
                            .child(chip("jev-clear", "clear", false, &p).on_click(cx.listener(|_, _, _, cx| {
                                app_state::update_settings(cx, |s| s.jev_api_key = None);
                            })))
                            .when(jev.is_some(), |d| {
                                d.child(chip("jev-test", "test", false, &p).on_click(cx.listener(|this, _, _, cx| this.test_jev(cx))))
                            }),
                    )
                    .children(self.jev_test.clone().map(|(msg, err)| mono(msg, 10.5, if err { p.accent } else { p.ink }))),
            )
            .child(div().px(px(40.)).py(px(24.)).child(button("done", "Back to the editor", false, &p).on_click(cx.listener(|this, _, _, cx| {
                this.screen = if this.editor.is_some() { crate::workspace::Screen::Editor } else { crate::workspace::Screen::Home };
                cx.notify();
            }))))
    }

    fn ensure_settings_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_inputs.is_some() {
            return;
        }
        use gpui_kit::component::input::InputState;
        let path = app_state::settings(cx)
            .cli_path
            .clone()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let a = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("search PATH and common locations")
                .default_value(path)
        });
        let b = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("paste a TypeSafe API key")
                .masked(true)
        });
        self.settings_inputs = Some((a, b));
    }

    fn test_jev(&mut self, cx: &mut Context<Self>) {
        let Some((key, _)) = app_state::settings(cx).jev_key() else {
            return;
        };
        self.jev_test = Some(("Asking Jev…".into(), false));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move { crate::assistant::test_jev(key) })
                .await;
            this.update(cx, |this, cx| {
                this.jev_test = Some(match r {
                    Ok(m) => (m.into(), false),
                    Err(e) => (e.into(), true),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
