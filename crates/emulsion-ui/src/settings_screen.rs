//! Settings: the capability ladder. Every tier is optional; each one shows
//! what it unlocks and whether it is available.

use crate::app_state::{self, CliStatus};
use crate::settings_storyboard::matches;
use crate::theme::{self, Palette};
use crate::widgets::{button, chip, label, mono};
use crate::workspace::Workspace;
use emulsion_io::settings::DrawingPace;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::input::Input;
use gpui_kit::component::switch::Switch;
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
    if crate::actions::STORYBOARD_ACTIONS.contains(&action) {
        "Storyboard"
    } else if action.starts_with("Tool")
        || action.starts_with("Flow")
        || matches!(
            action,
            "SwapColors"
                | "DefaultColors"
                | "BrushSmaller"
                | "BrushLarger"
                | "BrushSofter"
                | "BrushHarder"
                | "CommitTool"
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
            | "Print"
            | "Quit"
            | "ShowHome"
            | "ShowSettings"
    ) {
        "Files"
    } else if matches!(
        action,
        "Undo"
            | "Redo"
            | "FillSelection"
            | "FillBackground"
            | "ContentAwareFill"
            | "CutPixels"
            | "CopyPixels"
            | "PastePixels"
            | "PasteInPlace"
            | "ClearPixels"
            | "FreeTransform"
            | "DuplicateTransform"
            | "TransformAgain"
            | "TransformAgainWithCopy"
    ) || action.starts_with("Nudge")
        || action.starts_with("DiagramAdd")
    {
        "Edit"
    } else if action.starts_with("Adjust")
        || action.starts_with("Auto")
        || action.starts_with("Filter")
        || matches!(
            action,
            "RepeatFilter" | "ImageSizeDialog" | "CanvasSizeDialog"
        )
    {
        "Image"
    } else if action.contains("Node")
        || action.contains("Layer")
        || action.starts_with("Blend")
        || action.ends_with("BlendMode")
        || action.starts_with("Opacity")
        || matches!(
            action,
            "NewLayer"
                | "GroupNodes"
                | "Ungroup"
                | "CanvasDelete"
                | "MergeVisible"
                | "BringToFront"
                | "SendToBack"
                | "ToggleClippingMask"
        )
        || ctx == "panel"
    {
        "Layers"
    } else if action.contains("Select")
        || matches!(action, "Deselect" | "Reselect" | "ToggleQuickMask")
    {
        "Selection"
    } else if action.starts_with("Zoom")
        || action.starts_with("Rotate")
        || action.starts_with("FlipView")
        || action.starts_with("Show")
        || matches!(
            action,
            "ResetRotation"
                | "ToggleRulers"
                | "ToggleSnap"
                | "TogglePanels"
                | "ToggleScreenMode"
                | "ToggleDrawMode"
                | "ToggleTheme"
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

/// "ctrl-shift-s" → "Ctrl+Shift+S"; "ctrl--" → "Ctrl+-".
fn pretty_keys(keys: &str) -> String {
    let (mods, key) = match keys.strip_suffix("--") {
        Some(mods) => (mods, "-"),
        None => keys.rsplit_once('-').unwrap_or(("", keys)),
    };
    mods.split('-')
        .filter(|part| !part.is_empty())
        .chain(std::iter::once(key))
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
            "pageup" => "Page Up".to_string(),
            "pagedown" => "Page Down".to_string(),
            other => other.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// "ToggleNodeVisible" → "toggle layer visible".
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

/// A tier's state word in the interface language.
fn on_off(on: bool) -> std::borrow::Cow<'static, str> {
    if on {
        t!("settings.state_on")
    } else {
        t!("settings.state_off")
    }
}

impl Workspace {
    /// The interface language: the system's when it is shipped, or a fixed one.
    fn language_settings(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let saved = crate::i18n::supported(&app_state::settings(cx).language);
        let system = sys_locale::get_locale();
        let system_name = crate::i18n::LANGUAGES
            .iter()
            .find(|(code, _)| *code == crate::i18n::resolve("", system.as_deref()))
            .map_or("English", |(_, name)| *name);
        let mut choices = div().flex().flex_wrap().items_center().gap(px(8.)).child(
            chip(
                "lang-system",
                t!("settings.language_system", name = system_name),
                saved.is_none(),
                p,
            )
            .on_click(cx.listener(|_, _, _, cx| crate::i18n::set_language("", cx))),
        );
        for &(code, name) in crate::i18n::LANGUAGES {
            choices = choices.child(
                chip(
                    SharedString::from(format!("lang-{code}")),
                    name,
                    saved == Some(code),
                    p,
                )
                .on_click(cx.listener(move |_, _, _, cx| crate::i18n::set_language(code, cx))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(40.))
            .py(px(24.))
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("settings.language")),
            )
            .child(
                div()
                    .max_w(px(640.))
                    .text_size(px(13.))
                    .text_color(p.muted)
                    .child(t!("settings.language_body")),
            )
            .child(choices)
    }

    pub(crate) fn settings_screen(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = theme::palette(cx);
        let back_label = match self.back_target() {
            crate::workspace::Screen::Editor => t!("settings.back_document"),
            _ => t!("settings.back_home"),
        };
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
        let body = |text: std::borrow::Cow<'static, str>, p: &Palette| {
            div()
                .max_w(px(640.))
                .text_size(px(13.))
                .text_color(p.muted)
                .child(text)
        };

        let prov = emulsion_assistant::provider::by_id(&s.provider);
        let (cli_on, cli_state, cli_line) = match &cli {
            CliStatus::Checking => (
                false,
                t!("settings.state_checking"),
                t!("settings.cli_looking", name = prov.label).into_owned(),
            ),
            CliStatus::Found { path, version } => (
                true,
                t!("settings.state_on"),
                format!("{} · {}", version, path.display()),
            ),
            CliStatus::Missing => (
                false,
                t!("settings.state_not_found"),
                t!(
                    "settings.cli_missing",
                    name = prov.label,
                    hint = prov.install_hint
                )
                .into_owned(),
            ),
        };
        let installed: Vec<&'static str> = probe.installed.clone();

        let cli_path = self.settings_inputs.as_ref().map(|i| i.0.clone());
        let jev_input = self.settings_inputs.as_ref().map(|i| i.1.clone());

        let query = self.settings_query(window, cx);
        let model_names: String = emulsion_ai::models::MANIFEST
            .iter()
            .map(|m| format!("{} {} ", m.name, m.task.label()))
            .collect();
        // Each section with the words a search finds it by.
        let sections: Vec<(&'static str, String, AnyElement)> = vec![
            (
                "settings-language",
                format!(
                    "{} Language interface translation locale",
                    t!("settings.language")
                ),
                self.language_settings(&p, cx).into_any_element(),
            ),
            (
                "settings-experimental",
                format!(
                    "{} Experimental reuse interface layout performance CPU rendering",
                    t!("settings.experimental")
                ),
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_10()
                    .py_6()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("settings.experimental")),
                    )
                    .child(
                        Switch::new("experimental-layout-reuse")
                            .label(SharedString::from(t!("settings.layout_reuse")))
                            .checked(app_state::layout_reuse_enabled(cx))
                            .on_change(|enabled, window, cx| {
                                app_state::set_layout_reuse_enabled(*enabled, window, cx);
                            }),
                    )
                    .child(
                        div()
                            .max_w(rems(40.))
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("settings.layout_reuse_body")),
                    )
                    .when(
                        app_state::layout_reuse_launch_override().is_some(),
                        |section| {
                            section.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("settings.layout_reuse_override")),
                            )
                        },
                    )
                    .into_any_element(),
            ),
            (
                "settings-updates",
                "Updates check for updates new release version download install automatic".into(),
                self.updates_settings(&p, cx).into_any_element(),
            ),
            (
                "settings-built-in",
                format!(
                    "{} Built in suggestions nodes masks blend modes adjustments OpenRaster planner",
                    t!("settings.tier_builtin")
                ),
                section(&p)
                    .child(tier(
                        0,
                        &t!("settings.tier_builtin"),
                        true,
                        &t!("settings.state_on"),
                        &p,
                    ))
                    .child(body(t!("settings.tier_builtin_body"), &p))
                    .child(div().flex().gap(px(8.)).child(
                        chip("sugg", t!("settings.suggestions"), s.suggestions, &p).on_click(
                            cx.listener(|_, _, _, cx| {
                                app_state::update_settings(cx, |s| s.suggestions = !s.suggestions);
                            }),
                        ),
                    ))
                    .into_any_element(),
            ),
            (
                "settings-local-models",
                format!(
                    "{} Local models ONNX segmentation matte depth fill upscaling lens profiles lensfun {model_names}",
                    t!("settings.tier_models")
                ),
                section(&p)
                    .child(tier(
                        1,
                        &t!("settings.tier_models"),
                        models_on,
                        &on_off(models_on),
                        &p,
                    ))
                    .child(body(t!("settings.tier_models_body"), &p))
                    .child(self.models_list(&p, cx))
                    .into_any_element(),
            ),
            (
                "settings-assistant",
                format!(
                    "{} Coding CLI assistant Claude Codex OpenCode Kimi path model auto-apply drawing pace",
                    t!("settings.tier_cli")
                ),
                section(&p)
                    .child(tier(2, &t!("settings.tier_cli"), cli_on, &cli_state, &p))
                    .child(body(t!("settings.tier_cli_body"), &p))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.))
                            .child(mono(t!("settings.field_assistant"), 10., p.muted))
                            .children(emulsion_assistant::provider::PROVIDERS.iter().map(|pr| {
                                let on = s.provider == pr.id;
                                let here = installed.contains(&pr.id);
                                let id = pr.id;
                                chip(
                                    SharedString::from(format!("prov-{}", pr.id)),
                                    if here {
                                        pr.label.to_string()
                                    } else {
                                        t!("settings.not_installed", name = pr.label).into_owned()
                                    },
                                    on,
                                    &p,
                                )
                                .on_click(cx.listener(
                                    move |_, _, _, cx| {
                                        app_state::update_settings(cx, |s| {
                                            s.provider = id.into();
                                            s.model = None;
                                            s.cli_path = None;
                                        });
                                        app_state::detect_cli(cx);
                                    },
                                ))
                            })),
                    )
                    .child(mono(cli_line, 10.5, if cli_on { p.ink } else { p.accent }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(mono(t!("settings.field_path"), 10., p.muted))
                            .children(cli_path.map(|st| {
                                div()
                                    .w(px(420.))
                                    .border_1()
                                    .border_color(p.line)
                                    .child(Input::new(&st).appearance(false))
                            }))
                            .child(
                                chip("cli-use", t!("settings.use_path"), false, &p).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        let v = this
                                            .settings_inputs
                                            .as_ref()
                                            .map(|i| i.0.read(cx).value().to_string())
                                            .unwrap_or_default();
                                        let v = v.trim().to_string();
                                        app_state::update_settings(cx, |s| {
                                            s.cli_path = (!v.is_empty()).then(|| v.into())
                                        });
                                        app_state::detect_cli(cx);
                                    }),
                                ),
                            )
                            .child(
                                chip("cli-detect", t!("settings.detect_again"), false, &p)
                                    .on_click(cx.listener(|_, _, _, cx| app_state::detect_cli(cx))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(mono(t!("settings.field_model"), 10., p.muted))
                            .children(prov.models.iter().map(|(label, value)| {
                                let on = s.model.as_deref() == *value;
                                let v = value.map(str::to_string);
                                chip(SharedString::from(format!("model-{label}")), *label, on, &p)
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        let v = v.clone();
                                        app_state::update_settings(cx, |s| s.model = v);
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(mono(t!("settings.field_confirm"), 10., p.muted))
                            .child(
                                chip("auto", t!("settings.auto_apply"), s.auto_apply, &p).on_click(
                                    cx.listener(|_, _, _, cx| {
                                        app_state::update_settings(cx, |s| {
                                            s.auto_apply = !s.auto_apply
                                        });
                                    }),
                                ),
                            )
                            .child(
                                chip("auto-all", t!("settings.approve_all"), s.approve_all, &p)
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        app_state::update_settings(cx, |s| {
                                            s.approve_all = !s.approve_all
                                        });
                                    })),
                            )
                            .child(
                                chip(
                                    "show-drawing",
                                    t!("settings.show_drawing"),
                                    s.show_drawing,
                                    &p,
                                )
                                .on_click(cx.listener(
                                    |_, _, _, cx| {
                                        app_state::update_settings(cx, |s| {
                                            s.show_drawing = !s.show_drawing
                                        });
                                    },
                                )),
                            )
                            .child(
                                chip(
                                    "drawing-pace",
                                    match s.drawing_pace {
                                        DrawingPace::Natural => t!("settings.pace_natural"),
                                        DrawingPace::Quick => t!("settings.pace_quick"),
                                    },
                                    s.show_drawing,
                                    &p,
                                )
                                .on_click(cx.listener(
                                    |_, _, _, cx| {
                                        app_state::update_settings(cx, |s| {
                                            s.drawing_pace = match s.drawing_pace {
                                                DrawingPace::Natural => DrawingPace::Quick,
                                                DrawingPace::Quick => DrawingPace::Natural,
                                            }
                                        });
                                    },
                                )),
                            ),
                    )
                    .child(mono(t!("settings.next_session"), 9.5, p.muted))
                    .into_any_element(),
            ),
            (
                "settings-image-generation",
                format!(
                    "{} Image generation Local SD A1111 Forge OpenAI Google API key checkpoint model",
                    t!("settings.tier_image")
                ),
                self.image_settings_panel(&p, cx).into_any_element(),
            ),
            (
                "settings-jev",
                format!(
                    "{} Jev decision model TypeSafe API key",
                    t!("settings.tier_jev")
                ),
                section(&p)
                    .child(tier(
                        3,
                        &t!("settings.tier_jev"),
                        jev.is_some(),
                        &on_off(jev.is_some()),
                        &p,
                    ))
                    .child(body(t!("settings.tier_jev_body"), &p))
                    .child(mono(
                        match &jev {
                            Some((_, "environment")) => t!("settings.jev_key_env"),
                            Some(_) => t!("settings.jev_key_saved"),
                            None => t!("settings.jev_key_unset"),
                        },
                        10.5,
                        p.ink,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .children(jev_input.map(|st| {
                                div()
                                    .w(px(420.))
                                    .border_1()
                                    .border_color(p.line)
                                    .child(Input::new(&st).appearance(false))
                            }))
                            .child(
                                chip("jev-save", t!("settings.save_key"), false, &p).on_click(
                                    cx.listener(|this, _, window, cx| {
                                        let v = this
                                            .settings_inputs
                                            .as_ref()
                                            .map(|i| i.1.read(cx).value().to_string())
                                            .unwrap_or_default();
                                        let v = v.trim().to_string();
                                        if !v.is_empty() {
                                            app_state::update_settings(cx, |s| {
                                                s.jev_api_key = Some(v)
                                            });
                                            if let Some(i) = &this.settings_inputs {
                                                i.1.update(cx, |st, cx| {
                                                    st.set_value("", window, cx)
                                                });
                                            }
                                        }
                                    }),
                                ),
                            )
                            .child(chip("jev-clear", t!("settings.clear"), false, &p).on_click(
                                cx.listener(|_, _, _, cx| {
                                    app_state::update_settings(cx, |s| s.jev_api_key = None);
                                }),
                            ))
                            .when(jev.is_some(), |d| {
                                d.child(
                                    chip("jev-test", t!("settings.test"), false, &p)
                                        .on_click(cx.listener(|this, _, _, cx| this.test_jev(cx))),
                                )
                            }),
                    )
                    .children(
                        self.jev_test
                            .clone()
                            .map(|(msg, err)| mono(msg, 10.5, if err { p.accent } else { p.ink })),
                    )
                    .into_any_element(),
            ),
        ];
        let shortcuts = {
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
            let order = [
                "Tools",
                "Files",
                "Edit",
                "Image",
                "Layers",
                "Selection",
                "View",
                "Documents",
                "Storyboard",
                "Assistant",
            ];
            groups.sort_by_key(|(t, _)| order.iter().position(|o| o == t).unwrap_or(99));
            let mut rows = div().flex().flex_wrap().gap(px(28.)).items_start();
            let whole = matches(&query, "Shortcuts keyboard keymap");
            let mut any = false;
            for (title, items) in groups {
                let items: Vec<_> = items
                    .into_iter()
                    .filter(|(label, keys)| {
                        whole
                            || matches(&query, title)
                            || matches(&query, label)
                            || keys.iter().any(|k| matches(&query, k))
                    })
                    .collect();
                if items.is_empty() {
                    continue;
                }
                any = true;
                let mut col = div().flex().flex_col().gap(px(4.)).w(px(300.)).child(mono(
                    t!(format!("settings.group_{}", title.to_lowercase())).to_uppercase(),
                    9.5,
                    p.muted,
                ));
                for (label, keys) in items {
                    let mut caps = div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .w(px(130.))
                        .flex_none();
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
            any.then(|| {
                section(&p)
                    .child(tier(
                        5,
                        &t!("settings.tier_shortcuts"),
                        overrides > 0,
                        &if overrides > 0 {
                            t!("settings.state_custom")
                        } else {
                            t!("settings.state_default")
                        },
                        &p,
                    ))
                    .child(body(t!("settings.tier_shortcuts_body"), &p))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                chip("km-open", t!("settings.open_keymap"), false, &p).on_click(
                                    cx.listener(|this, _, _, cx| this.open_keymap_file(cx)),
                                ),
                            )
                            .child(
                                chip("km-reload", t!("settings.reload"), false, &p).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        crate::actions::bind(cx);
                                        this.invalidate_probe();
                                        this.keymap_note = Some(
                                            t!(
                                                "settings.reloaded",
                                                count = crate::actions::user_bindings().len()
                                            )
                                            .into(),
                                        );
                                        cx.notify();
                                    }),
                                ),
                            )
                            .children(self.keymap_note.clone().map(|m| mono(m, 10.5, p.ink))),
                    )
                    .child(rows)
            })
        };
        let storyboard = self.storyboard_settings(&p, &query, window, cx);
        let color = self.color_settings(&p, &query, cx);
        let mut found = false;
        let mut screen = div()
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
                    .child(label(t!("settings.eyebrow"), &p))
                    .child(
                        div()
                            .text_size(px(40.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("settings.title")),
                    )
                    .child(body(t!("settings.intro"), &p))
                    .children(self.settings_search_box(&p)),
            );
        for (id, words, section) in sections {
            if matches(&query, &words) {
                found = true;
                screen = screen.child(div().id(id).test_support().child(section));
            }
        }
        for (id, section) in [
            ("settings-color-section", color),
            ("settings-storyboard-section", storyboard),
            (
                "settings-shortcuts",
                shortcuts.map(IntoElement::into_any_element),
            ),
        ] {
            if let Some(section) = section {
                found = true;
                screen = screen.child(div().id(id).test_support().child(section));
            }
        }
        screen
            .when(!found, |screen| {
                screen.child(
                    div()
                        .id("settings-no-results")
                        .test_support()
                        .px(px(40.))
                        .py(px(24.))
                        .child(body("No settings match your search.".into(), &p)),
                )
            })
            .child(
                div().px(px(40.)).py(px(24.)).child(
                    button("done", back_label, false, &p)
                        .on_click(cx.listener(|this, _, window, cx| this.go_back(window, cx))),
                ),
            )
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
                t!("settings.keymap_editing", path = shown)
            } else {
                t!("settings.keymap_file", path = shown)
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
                    t!("settings.checkpoint_placeholder")
                } else {
                    provider.default_model().into()
                })
                .default_value(model)
        });
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("settings.api_key_placeholder"))
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
            .child(mono(t!("settings.field_default"), 10., p.muted))
            .child(
                chip("img-none", t!("settings.image_off"), !on, p).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.save_image_settings(window, cx);
                        app_state::update_settings(cx, |s| s.image_provider.clear());
                        this.image_test = None;
                    },
                )),
            );
        for (id, title, provider) in [
            ("img-a1111", t!("settings.image_local"), Provider::A1111),
            ("img-openai", "OpenAI".into(), Provider::OpenAi),
            ("img-google", "Google".into(), Provider::Google),
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
        let mut panel = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(40.))
            .py(px(24.))
            .border_b_1()
            .border_color(p.line)
            .child(tier(4, &t!("settings.tier_image"), on, &on_off(on), p))
            .child(
                div()
                    .max_w(px(700.))
                    .text_size(px(13.))
                    .text_color(p.muted)
                    .child(t!("settings.tier_image_body")),
            )
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
                .child(mono(t!("settings.a1111_hint"), 10.5, p.muted))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child(mono(t!("settings.address"), 10., p.muted))
                        .child(div().w(px(300.)).child(Input::new(&inputs.address))),
                );
        } else {
            panel = panel
                .child(
                    div()
                        .max_w(px(700.))
                        .text_size(px(12.))
                        .text_color(p.muted)
                        .child(t!("settings.cloud_notice")),
                )
                .child(mono(
                    match key_status {
                        Some("environment") => t!("settings.api_key_env"),
                        Some(_) => t!("settings.api_key_saved"),
                        None => t!("settings.api_key_unset"),
                    },
                    10.,
                    p.muted,
                ))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child(div().w(px(380.)).child(Input::new(&inputs.key)))
                        .child(
                            chip("img-clear-key", t!("settings.clear_saved_key"), false, p)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    app_state::update_settings(cx, |s| match provider {
                                        Provider::OpenAi => s.openai_image_key = None,
                                        Provider::Google => s.google_image_key = None,
                                        Provider::A1111 => {}
                                    });
                                    if let Some(inputs) = &this.image_inputs {
                                        inputs
                                            .key
                                            .update(cx, |key, cx| key.set_value("", window, cx));
                                    }
                                    this.image_test = None;
                                })),
                        ),
                );
        }
        panel
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(mono(
                        if local {
                            t!("settings.checkpoint")
                        } else {
                            t!("settings.model")
                        },
                        10.,
                        p.muted,
                    ))
                    .child(div().w(px(300.)).child(Input::new(&inputs.model)))
                    .child(
                        chip("img-save", t!("settings.save"), false, p).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.save_image_settings(window, cx);
                                this.image_test = Some((t!("settings.saved").into(), false));
                            },
                        )),
                    )
                    .child(
                        chip("img-test", t!("settings.save_test"), false, p).on_click(cx.listener(
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
        self.image_test = Some((t!("settings.checking_connection").into(), false));
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
                            t!("settings.connected", model = m)
                        } else {
                            t!("settings.key_accessible", model = m)
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
                .placeholder(t!("settings.cli_path_placeholder"))
                .default_value(path)
        });
        let b = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("settings.jev_key_placeholder"))
                .masked(true)
        });
        self.settings_inputs = Some((a, b));
    }

    fn test_jev(&mut self, cx: &mut Context<Self>) {
        let Some((key, _)) = app_state::settings(cx).jev_key() else {
            return;
        };
        self.jev_test = Some((t!("settings.asking_jev").into(), false));
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

#[cfg(test)]
mod tests {
    use super::{pretty_keys, shortcut_group};

    #[test]
    fn every_default_shortcut_has_a_heading_and_readable_keys() {
        for (ctx, action, keys) in crate::actions::DEFAULTS {
            assert_ne!(
                shortcut_group(action, ctx),
                "Other",
                "{action} has no heading"
            );
            assert!(!pretty_keys(keys).is_empty());
        }
        assert_eq!(pretty_keys("ctrl--"), "Ctrl+-");
        assert_eq!(pretty_keys("ctrl-alt-shift-w"), "Ctrl+Alt+Shift+W");
        assert_eq!(pretty_keys("alt-shift-["), "Alt+Shift+[");
        assert_eq!(pretty_keys("0"), "0");
        assert_eq!(pretty_keys("pagedown"), "Page Down");
        for action in crate::actions::STORYBOARD_ACTIONS {
            assert_eq!(shortcut_group(action, "workspace"), "Storyboard");
            assert!(
                crate::actions::DEFAULTS.iter().any(|(_, a, _)| a == action),
                "{action} has no default shortcut, so Settings would not list it"
            );
        }
        assert_eq!(shortcut_group("PreviousPanel", "panel"), "Storyboard");
        assert_eq!(shortcut_group("PasteInPlace", "canvas"), "Edit");
        assert_eq!(shortcut_group("Flow30", "photo_canvas"), "Tools");
        assert_eq!(pretty_keys("shift-3"), "Shift+3");
        assert_eq!(pretty_keys("#"), "#");
    }
}
