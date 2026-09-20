//! Settings: the capability ladder. Every tier is optional; each one shows
//! what it unlocks and whether it is available.

use crate::app_state::{self, CliStatus};
use crate::theme::{self, Palette};
use crate::widgets::{button, chip, label, mono};
use crate::workspace::Workspace;
use gpui_kit::component::input::Input;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

pub(crate) struct ImageInputs {
    provider: emulsion_ai::generate::Provider,
    address: Entity<gpui_kit::component::input::InputState>,
    model: Entity<gpui_kit::component::input::InputState>,
    key: Entity<gpui_kit::component::input::InputState>,
}

/// A heading and its (action label, key caps) rows.
type ShortcutGroup = (&'static str, Vec<(String, Vec<String>)>);

/// Which heading a shortcut sits under on the Settings screen.
fn shortcut_group(action: &str, ctx: &str) -> &'static str {
    if action.starts_with("Tool")
        || matches!(
            action,
            "SwapColors" | "DefaultColors" | "BrushSmaller" | "BrushLarger" | "CommitTool"
        )
    {
        "Tools"
    } else if matches!(
        action,
        "NewDocument"
            | "Open"
            | "Save"
            | "SaveAs"
            | "Export"
            | "Quit"
            | "ShowHome"
            | "ShowSettings"
            | "ImageSizeDialog"
            | "CanvasSizeDialog"
    ) {
        "Files"
    } else if matches!(
        action,
        "Undo"
            | "Redo"
            | "FillSelection"
            | "ContentAwareFill"
            | "CutPixels"
            | "CopyPixels"
            | "PastePixels"
            | "ClearPixels"
            | "FreeTransform"
    ) || action.starts_with("Nudge")
    {
        "Edit"
    } else if action.contains("Select") || action == "Deselect" {
        "Selection"
    } else if action.contains("Node")
        || matches!(action, "GroupNodes" | "Ungroup" | "CanvasDelete")
        || ctx == "panel"
    {
        "Layers"
    } else if action.starts_with("Zoom")
        || action.starts_with("Rotate")
        || matches!(
            action,
            "ResetRotation" | "ToggleRulers" | "ToggleTheme" | "ShowEditor"
        )
    {
        "View"
    } else if action.ends_with("Tab") {
        "Documents"
    } else if action == "Ask" || action.starts_with("Suggestion") {
        "Assistant"
    } else {
        "Other"
    }
}

/// "ctrl-shift-s" → "Ctrl+Shift+S".
fn pretty_keys(keys: &str) -> String {
    keys.split('-')
        .map(|part| match part {
            "ctrl" => "Ctrl".to_string(),
            "shift" => "Shift".to_string(),
            "alt" => "Alt".to_string(),
            "cmd" => "Cmd".to_string(),
            "enter" => "Enter".to_string(),
            "escape" => "Esc".to_string(),
            "backspace" => "Backspace".to_string(),
            "delete" => "Delete".to_string(),
            "tab" => "Tab".to_string(),
            other => other.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// "ToggleNodeVisible" → "toggle node visible".
fn humanize(action: &str) -> String {
    let mut out = String::new();
    for (i, c) in action.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// What the Settings screen needs from disk: model files, CLI binaries on
/// PATH, the lens database and the keymap file.
#[derive(Clone)]
pub(crate) struct Probe {
    pub models_on: bool,
    pub statuses: Vec<emulsion_ai::models::Status>,
    pub installed: Vec<&'static str>,
    pub lens_on: bool,
    pub bindings: Vec<(String, String, String)>,
    pub overrides: usize,
}

impl Probe {
    fn read() -> Probe {
        let statuses: Vec<emulsion_ai::models::Status> = emulsion_ai::models::MANIFEST
            .iter()
            .map(emulsion_ai::models::status)
            .collect();
        Probe {
            models_on: statuses.contains(&emulsion_ai::models::Status::Installed),
            statuses,
            installed: emulsion_assistant::provider::installed()
                .into_iter()
                .map(|p| p.id)
                .collect(),
            lens_on: emulsion_io::lensfun::installed(),
            bindings: crate::actions::effective(),
            overrides: crate::actions::user_bindings().len(),
        }
    }
}

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
                .text_color(if on { p.accent_fg } else { p.muted })
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
        self.ensure_image_inputs(window, cx);
        let s = app_state::settings(cx).clone();
        let cli = app_state::cli(cx);
        let probe = self.probe(cx);
        let models_on = probe.models_on;
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

        let prov = emulsion_assistant::provider::by_id(&s.provider);
        let (cli_on, cli_state, cli_line) = match &cli {
            CliStatus::Checking => (false, "checking", format!("Looking for {}…", prov.label)),
            CliStatus::Found { path, version } => {
                (true, "on", format!("{} · {}", version, path.display()))
            }
            CliStatus::Missing => (
                false,
                "not found",
                format!(
                    "{} was not found. Install it with: {}",
                    prov.label, prov.install_hint
                ),
            ),
        };
        let installed: Vec<&'static str> = probe.installed.clone();

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
                    .child(tier(1, "Local models", models_on, if models_on { "on" } else { "off" }, &p))
                    .child(body("Segmentation, subject mattes, depth, fill and upscaling that run on this machine with ONNX Runtime. Install what you want; each task's tools appear once its model is here. Select Subject and Remove Background need a matte model, the AI quick select needs SlimSAM.", &p))
                    .child(self.models_list(&p, cx)),
            )
            .child(
                section(&p)
                    .child(tier(2, "Coding CLI assistant", cli_on, cli_state, &p))
                    .child(body("Multi-step requests from Ctrl+K go to a coding CLI that can only use Emulsion's tools. Every change it proposes is shown as an Apply / Skip card unless you turn on auto-apply. Claude Code asks before each tool; Codex, OpenCode and Kimi run one process per request and Emulsion holds their changes for you instead.", &p))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .child(mono("assistant", 10., p.muted))
                            .children(emulsion_assistant::provider::PROVIDERS.iter().map(|pr| {
                                let on = s.provider == pr.id;
                                let here = installed.contains(&pr.id);
                                let id = pr.id;
                                chip(
                                    SharedString::from(format!("prov-{}", pr.id)),
                                    if here { pr.label.to_string() } else { format!("{} (not installed)", pr.label) },
                                    on,
                                    &p,
                                )
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    app_state::update_settings(cx, |s| {
                                        s.provider = id.into();
                                        s.model = None;
                                        s.cli_path = None;
                                    });
                                    app_state::detect_cli(cx);
                                }))
                            })),
                    )
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
                            .children(prov.models.iter().map(|(label, value)| {
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
                        )
                        .child(
                            chip("auto-all", "apply everything without asking", s.approve_all, &p).on_click(cx.listener(|_, _, _, cx| {
                                app_state::update_settings(cx, |s| s.approve_all = !s.approve_all);
                            })),
                        )
                        .child(
                            chip("show-drawing", "show the assistant drawing live", s.show_drawing, &p).on_click(cx.listener(|_, _, _, cx| {
                                app_state::update_settings(cx, |s| s.show_drawing = !s.show_drawing);
                            })),
                        ),
                    )
                    .child(mono(
                        "The model picks up these settings the next time a document starts an assistant session.",
                        9.5,
                        p.muted,
                    )),
            )
            .child(self.image_settings_panel(&p, cx))
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
            .child({
                let eff = probe.bindings.clone();
                let overrides = probe.overrides;
                // One line per action, keys as key caps, grouped by purpose.
                let mut groups: Vec<ShortcutGroup> = Vec::new();
                for (ctx, action, keys) in &eff {
                    let title = shortcut_group(action, ctx);
                    let g = match groups.iter_mut().find(|(t, _)| *t == title) {
                        Some(g) => g,
                        None => {
                            groups.push((title, Vec::new()));
                            groups.last_mut().unwrap()
                        }
                    };
                    let label = humanize(action);
                    match g.1.iter_mut().find(|(a, _)| *a == label) {
                        Some((_, ks)) => ks.push(pretty_keys(keys)),
                        None => g.1.push((label, vec![pretty_keys(keys)])),
                    }
                }
                let order = ["Tools", "Files", "Edit", "Selection", "Layers", "View", "Documents", "Assistant"];
                groups.sort_by_key(|(t, _)| order.iter().position(|o| o == t).unwrap_or(99));
                let mut rows = div().flex().flex_wrap().gap(px(28.)).items_start();
                for (title, items) in groups {
                    let mut col = div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .w(px(300.))
                        .child(mono(title.to_uppercase(), 9.5, p.muted));
                    for (label, keys) in items {
                        let mut caps = div().flex().items_center().gap(px(4.)).w(px(130.)).flex_none();
                        for (i, k) in keys.iter().enumerate() {
                            if i > 0 {
                                caps = caps.child(mono("/", 9.5, p.muted));
                            }
                            caps = caps.child(
                                div()
                                    .px(px(5.))
                                    .py(px(1.))
                                    .border_1()
                                    .border_color(p.line)
                                    .bg(p.soft_bg)
                                    .font_family(crate::theme::MONO_FONT)
                                    .text_size(px(10.))
                                    .text_color(p.ink)
                                    .child(k.clone()),
                            );
                        }
                        col = col.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(caps)
                                .child(div().text_size(px(12.)).text_color(p.ink).child(label)),
                        );
                    }
                    rows = rows.child(col);
                }
                section(&p)
                    .child(tier(5, "Shortcuts", overrides > 0, if overrides > 0 { "custom" } else { "default" }, &p))
                    .child(body("Every shortcut, as it works right now. To change one, open the keymap file, uncomment a line and set its keys, then reload. Bare letters work while the canvas has focus; modifier shortcuts work anywhere.", &p))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(chip("km-open", "open keymap file", false, &p).on_click(cx.listener(|this, _, _, cx| this.open_keymap_file(cx))))
                            .child(chip("km-reload", "reload", false, &p).on_click(cx.listener(|this, _, _, cx| {
                                crate::actions::bind(cx);
                                this.invalidate_probe();
                                this.keymap_note = Some(format!("Reloaded: {} custom binding(s).", crate::actions::user_bindings().len()).into());
                                cx.notify();
                            })))
                            .children(self.keymap_note.clone().map(|m| mono(m, 10.5, p.ink))),
                    )
                    .child(rows)
            })
            .child(div().px(px(40.)).py(px(24.)).child(button("done", "Back to the editor", false, &p).on_click(cx.listener(|this, _, _, cx| {
                this.screen = if this.editor.is_some() { crate::workspace::Screen::Editor } else { crate::workspace::Screen::Home };
                cx.notify();
            }))))
    }

    /// Write the keymap template if there is no file yet, then hand it to
    /// the system's editor.
    fn open_keymap_file(&mut self, cx: &mut Context<Self>) {
        let path = crate::actions::keymap_path();
        if !path.exists() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, crate::actions::keymap_template());
        }
        let shown = path.display().to_string();
        let opened = std::process::Command::new("xdg-open")
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok();
        self.keymap_note = Some(
            if opened {
                format!("Editing {shown}; press reload when saved.")
            } else {
                format!("Keymap file: {shown}")
            }
            .into(),
        );
        cx.notify();
    }

    /// Refresh the cached filesystem facts when they are stale.
    pub(crate) fn probe(&mut self, _cx: &App) -> Probe {
        if let Some((t, p)) = &self.probe
            && t.elapsed() < std::time::Duration::from_millis(1500)
        {
            return p.clone();
        }
        let p = Probe::read();
        self.probe = Some((std::time::Instant::now(), p.clone()));
        p
    }

    /// Bump the cache after a change the screen made itself.
    pub(crate) fn invalidate_probe(&mut self) {
        self.probe = None;
    }

    fn ensure_image_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.image_inputs.is_some() {
            return;
        }
        use emulsion_ai::generate::Provider;
        use gpui_kit::component::input::InputState;
        let s = app_state::settings(cx);
        let provider = Provider::parse(&s.image_provider).unwrap_or(Provider::A1111);
        let endpoint = s.image_endpoint.clone().unwrap_or_default();
        let model = match provider {
            Provider::A1111 => s.image_model.clone(),
            Provider::OpenAi => s.openai_image_model.clone(),
            Provider::Google => s.google_image_model.clone(),
        }
        .unwrap_or_default();
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(Provider::A1111.default_endpoint())
                .default_value(endpoint)
        });
        let model = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(if provider == Provider::A1111 {
                    "server's current checkpoint"
                } else {
                    provider.default_model()
                })
                .default_value(model)
        });
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste API key to save or replace")
                .masked(true)
        });
        self.image_inputs = Some(ImageInputs {
            provider,
            address,
            model,
            key,
        });
    }

    fn save_image_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(inputs) = &self.image_inputs else {
            return;
        };
        let provider = inputs.provider;
        let endpoint = inputs.address.read(cx).value().trim().to_string();
        let model = inputs.model.read(cx).value().trim().to_string();
        let key = inputs.key.read(cx).value().trim().to_string();
        let key_input = inputs.key.clone();
        app_state::update_settings(cx, move |s| {
            let model = (!model.is_empty()).then_some(model);
            match provider {
                emulsion_ai::generate::Provider::A1111 => {
                    s.image_endpoint = (!endpoint.is_empty()).then_some(endpoint);
                    s.image_model = model;
                }
                emulsion_ai::generate::Provider::OpenAi => {
                    s.openai_image_model = model;
                    if !key.is_empty() {
                        s.openai_image_key = Some(key);
                    }
                }
                emulsion_ai::generate::Provider::Google => {
                    s.google_image_model = model;
                    if !key.is_empty() {
                        s.google_image_key = Some(key);
                    }
                }
            }
        });
        key_input.update(cx, |input, cx| input.set_value("", window, cx));
        self.image_test = None;
    }

    fn image_settings_panel(&mut self, p: &Palette, cx: &mut Context<Self>) -> Div {
        use emulsion_ai::generate::Provider;
        let s = app_state::settings(cx);
        let selected = Provider::parse(&s.image_provider);
        let on = selected.is_some();
        let mut choices = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(mono("default", 10., p.muted))
            .child(
                chip("img-none", "Off", !on, p).on_click(cx.listener(|this, _, window, cx| {
                    this.save_image_settings(window, cx);
                    app_state::update_settings(cx, |s| s.image_provider.clear());
                    this.image_test = None;
                })),
            );
        for (id, title, provider) in [
            ("img-a1111", "Local SD", Provider::A1111),
            ("img-openai", "OpenAI", Provider::OpenAi),
            ("img-google", "Google", Provider::Google),
        ] {
            choices = choices.child(
                chip(id, title, selected == Some(provider), p)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.save_image_settings(window, cx);
                        app_state::update_settings(cx, |s| s.image_provider = provider.id().into());
                        this.image_inputs = None;
                        this.image_test = None;
                        cx.notify();
                    }))
                    .test_support(),
            );
        }
        let mut panel = div().flex().flex_col().gap(px(10.)).px(px(40.)).py(px(24.))
            .border_b_1().border_color(p.line)
            .child(tier(4, "Image generation", on, if on { "on" } else { "off" }, p))
            .child(div().max_w(px(700.)).text_size(px(13.)).text_color(p.muted)
                .child("Use Ctrl-K to choose Assistant, Local SD, OpenAI, or Google. Image modes create a new layer, or fill the selected area. The Select tool uses the default provider below."))
            .child(choices);
        let Some(provider) = selected else {
            return panel;
        };
        let Some(inputs) = &self.image_inputs else {
            return panel;
        };
        let local = provider == Provider::A1111;
        let key_status = app_state::settings(cx)
            .image_key(provider.id())
            .map(|(_, source)| source);
        if local {
            panel = panel
                .child(mono(
                    "Start A1111 / Forge with --api. Enter the base URL below.",
                    10.5,
                    p.muted,
                ))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child(mono("Address", 10., p.muted))
                        .child(div().w(px(300.)).child(Input::new(&inputs.address))),
                );
        } else {
            panel = panel
                .child(div().max_w(px(700.)).text_size(px(12.)).text_color(p.muted)
                    .child("Cloud generation sends your prompt and, for fills, the selected canvas context to this provider. API usage is billed separately from chat or coding subscriptions."))
                .child(mono(match key_status {
                    Some("environment") => "API key: from environment",
                    Some(_) => "API key: saved",
                    None => "API key: not set",
                }, 10., p.muted))
                .child(div().flex().flex_wrap().items_center().gap(px(8.))
                    .child(div().w(px(380.)).child(Input::new(&inputs.key)))
                    .child(chip("img-clear-key", "Clear saved key", false, p)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            app_state::update_settings(cx, |s| match provider {
                                Provider::OpenAi => s.openai_image_key = None,
                                Provider::Google => s.google_image_key = None,
                                Provider::A1111 => {},
                            });
                            if let Some(inputs) = &this.image_inputs {
                                inputs.key.update(cx, |key, cx| key.set_value("", window, cx));
                            }
                            this.image_test = None;
                        }))));
        }
        panel
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(mono(
                        if local { "Checkpoint" } else { "Model" },
                        10.,
                        p.muted,
                    ))
                    .child(div().w(px(300.)).child(Input::new(&inputs.model)))
                    .child(chip("img-save", "Save", false, p).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.save_image_settings(window, cx);
                            this.image_test = Some(("Settings saved".into(), false));
                        },
                    )))
                    .child(
                        chip("img-test", "Save & test", false, p).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.save_image_settings(window, cx);
                                this.test_image_server(cx);
                            },
                        )),
                    ),
            )
            .children(
                self.image_test
                    .clone()
                    .map(|(msg, err)| mono(msg, 10.5, if err { p.accent } else { p.ink })),
            )
    }

    fn test_image_server(&mut self, cx: &mut Context<Self>) {
        let Some(cfg) = crate::editor::generate_ui::config(cx) else {
            return;
        };
        let provider = cfg.provider;
        self.image_test = Some(("Checking connection and model access…".into(), false));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move { emulsion_ai::generate::reachable(&cfg) })
                .await;
            this.update(cx, |this, cx| {
                if app_state::settings(cx).image_provider != provider.id() {
                    return;
                }
                this.image_test = Some(match r {
                    Ok(m) => (
                        if provider == emulsion_ai::generate::Provider::A1111 {
                            format!("Connected · {m}")
                        } else {
                            format!(
                                "Key and model accessible · {m}. Generation quota is not checked."
                            )
                        }
                        .into(),
                        false,
                    ),
                    Err(e) => (e.to_string().into(), true),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
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
