//! Settings › Storyboard: naming rules and the defaults new storyboards and
//! the board view start from. Every change is checked with
//! `Preferences::validate` before the settings writer saves it; text fields
//! save on Enter or when they lose focus.
use crate::app_state;
use crate::theme::Palette;
use crate::widgets::{chip, mono};
use crate::workspace::Workspace;
use emulsion_core::storyboard::{CaptionPreset, Preferences};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

/// Text fields, by key and label, in screen order.
const FIELDS: [(&str, &str); 18] = [
    ("scene_prefix", "Scene prefix"),
    ("scene_start", "First scene number"),
    ("scene_step", "Scene number step"),
    ("scene_digits", "Scene number digits"),
    ("panel_prefix", "Panel prefix"),
    ("panel_digits", "Panel number digits"),
    ("panel_seconds", "Default panel length · seconds"),
    ("smart_add", "Smart add layers · comma-separated"),
    ("thumbnail_width", "Board thumbnail width · px"),
    (
        "action_safe",
        "Stage action safe area · % of frame (0 hides)",
    ),
    ("title_safe", "Stage title safe area · % of frame (0 hides)"),
    ("fields", "Stage field guide · fields"),
    ("overscan", "Stage overscan · % of frame"),
    ("palette", "Palette · hex colours, comma-separated"),
    ("light_before", "Light table · panels before"),
    ("light_after", "Light table · panels after"),
    ("light_opacity", "Light table opacity · %"),
    (
        "review_author",
        "Your name · review notes, scene claims and cloud saves",
    ),
];

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn parse_palette(value: &str) -> Result<Vec<[u8; 3]>, String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| {
            let digits = c.trim_start_matches('#');
            u32::from_str_radix(digits, 16)
                .ok()
                .filter(|_| digits.len() == 6)
                .map(|v| {
                    let [_, r, g, b] = v.to_be_bytes();
                    [r, g, b]
                })
                .ok_or_else(|| format!("{c} is not a colour; use hex such as #E8A040."))
        })
        .collect()
}

/// Settings screen state: the search box and the Storyboard section.
#[derive(Default)]
pub(crate) struct SettingsUi {
    search: Option<(Entity<InputState>, Subscription)>,
    storyboard: Option<StoryboardInputs>,
}

struct StoryboardInputs {
    /// The saved preferences the inputs last showed.
    synced: Preferences,
    fields: Vec<(&'static str, Entity<InputState>)>,
    captions: Vec<Entity<InputState>>,
    new_caption: Entity<InputState>,
    message: Option<(String, bool)>,
    _subs: Vec<Subscription>,
}

/// Whether settings text matches a lowercase search.
pub(crate) fn matches(query: &str, text: &str) -> bool {
    query.is_empty() || text.to_lowercase().contains(query)
}

fn field_value(prefs: &Preferences, key: &str) -> String {
    let naming = &prefs.naming;
    match key {
        "scene_prefix" => naming.scene_prefix.clone(),
        "scene_start" => naming.scene_start.to_string(),
        "scene_step" => naming.scene_step.to_string(),
        "scene_digits" => naming.scene_digits.to_string(),
        "panel_prefix" => naming.panel_prefix.clone(),
        "panel_digits" => naming.panel_digits.to_string(),
        "panel_seconds" => prefs.panel_seconds.to_string(),
        "smart_add" => prefs.smart_add_layers.join(", "),
        "action_safe" => prefs.stage.action_safe.to_string(),
        "title_safe" => prefs.stage.title_safe.to_string(),
        "fields" => prefs.stage.fields.to_string(),
        "overscan" => prefs.stage.overscan.to_string(),
        "palette" => prefs
            .palette
            .iter()
            .map(|c| hex(*c))
            .collect::<Vec<_>>()
            .join(", "),
        "light_before" => prefs.light_table.before.to_string(),
        "light_after" => prefs.light_table.after.to_string(),
        "light_opacity" => (prefs.light_table.opacity * 100.).round().to_string(),
        "review_author" => prefs.review_author.clone(),
        _ => prefs.thumbnail_width.to_string(),
    }
}

fn apply_field(prefs: &mut Preferences, key: &str, label: &str, value: &str) -> Result<(), String> {
    fn number<T: std::str::FromStr>(value: &str, label: &str) -> Result<T, String> {
        value
            .trim()
            .parse()
            .map_err(|_| format!("Enter a whole number for {label}."))
    }
    fn decimal(value: &str, label: &str) -> Result<f64, String> {
        value
            .trim()
            .trim_end_matches('%')
            .parse()
            .map_err(|_| format!("Enter a number for {label}."))
    }
    let naming = &mut prefs.naming;
    match key {
        "scene_prefix" => naming.scene_prefix = value.into(),
        "scene_start" => naming.scene_start = number(value, label)?,
        "scene_step" => naming.scene_step = number(value, label)?,
        "scene_digits" => naming.scene_digits = number(value, label)?,
        "panel_prefix" => naming.panel_prefix = value.into(),
        "panel_digits" => naming.panel_digits = number(value, label)?,
        "panel_seconds" => {
            prefs.panel_seconds = value
                .trim()
                .parse()
                .map_err(|_| "Enter the default panel length in seconds.".to_string())?
        }
        "smart_add" => {
            prefs.smart_add_layers = value
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(String::from)
                .collect()
        }
        "action_safe" => prefs.stage.action_safe = decimal(value, label)?,
        "title_safe" => prefs.stage.title_safe = decimal(value, label)?,
        "fields" => prefs.stage.fields = number(value, label)?,
        "overscan" => prefs.stage.overscan = decimal(value, label)?,
        "palette" => prefs.palette = parse_palette(value)?,
        "light_before" => prefs.light_table.before = number(value, label)?,
        "light_after" => prefs.light_table.after = number(value, label)?,
        "light_opacity" => prefs.light_table.opacity = decimal(value, label)? / 100.,
        "review_author" => prefs.review_author = value.trim().into(),
        _ => prefs.thumbnail_width = number(value, label)?,
    }
    Ok(())
}

/// Words shown in the personal dictionary row; the rest are counted.
const SHOWN_WORDS: usize = 200;

/// The personal dictionary: each word removable, and Clear.
fn spelling_words(words: &[String], p: &Palette, cx: &mut Context<Workspace>) -> AnyElement {
    let mut list = div()
        .id("settings-storyboard-spelling-words")
        .test_support()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(4.))
        .max_w(px(640.))
        .child(
            mono("Personal dictionary", 10., p.muted)
                .w(px(240.))
                .flex_none(),
        );
    if words.is_empty() {
        list = list.child(mono(
            "Empty. Add words from a caption's spelling menu.",
            10.,
            p.muted,
        ));
    }
    for (index, word) in words.iter().take(SHOWN_WORDS).enumerate() {
        let word = word.clone();
        list = list.child(
            chip(
                ("settings-storyboard-spelling-word", index),
                format!("{word} ×"),
                false,
                p,
            )
            .test_support()
            .on_click(cx.listener(move |this, _, _, cx| {
                let word = word.clone();
                this.update_storyboard_preferences(
                    move |p| p.spelling_words.retain(|w| *w != word),
                    cx,
                )
            })),
        );
    }
    if words.len() > SHOWN_WORDS {
        list = list.child(mono(
            format!("and {} more", words.len() - SHOWN_WORDS),
            10.,
            p.muted,
        ));
    }
    if !words.is_empty() {
        list = list.child(
            chip(
                "settings-storyboard-spelling-clear",
                "Clear dictionary",
                false,
                p,
            )
            .test_support()
            .on_click(cx.listener(|this, _, _, cx| {
                this.update_storyboard_preferences(|p| p.spelling_words.clear(), cx)
            })),
        );
    }
    list.into_any_element()
}

impl Workspace {
    /// The lowercase search text, creating the search box on first use.
    pub(crate) fn settings_query(&mut self, window: &mut Window, cx: &mut Context<Self>) -> String {
        let (search, _) = self.settings_ui.search.get_or_insert_with(|| {
            let search = cx
                .new(|cx| InputState::new(window, cx).placeholder("Search settings and shortcuts"));
            let sub = cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            });
            (search, sub)
        });
        search.read(cx).value().trim().to_lowercase()
    }

    pub(crate) fn settings_search_box(&self, p: &Palette) -> Option<AnyElement> {
        let (search, _) = self.settings_ui.search.as_ref()?;
        Some(
            div()
                .id("settings-search")
                .test_support()
                .w(px(420.))
                .border_1()
                .border_color(p.line)
                .child(Input::new(search).cleanable(true))
                .into_any_element(),
        )
    }

    fn ensure_storyboard_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = app_state::settings(cx).storyboard.clone();
        let rebuild = self
            .settings_ui
            .storyboard
            .as_ref()
            .is_none_or(|i| i.captions.len() != saved.captions.len());
        if rebuild {
            let message = self.settings_ui.storyboard.take().and_then(|i| i.message);
            let mut subs = Vec::new();
            let mut input = |value: String,
                             subs: &mut Vec<Subscription>,
                             cx: &mut Context<Self>| {
                let state = cx.new(|cx| InputState::new(window, cx).default_value(value));
                subs.push(
                    cx.subscribe(&state, |this, _, event: &InputEvent, cx| match event {
                        InputEvent::PressEnter { .. } | InputEvent::Blur => {
                            this.update_storyboard_preferences(|_| {}, cx)
                        }
                        // The naming example follows typing.
                        InputEvent::Change => cx.notify(),
                        InputEvent::Focus => {}
                    }),
                );
                state
            };
            let fields = FIELDS
                .iter()
                .map(|&(key, _)| (key, input(field_value(&saved, key), &mut subs, cx)))
                .collect();
            let captions = saved
                .captions
                .iter()
                .map(|c| input(c.name.clone(), &mut subs, cx))
                .collect();
            let new_caption =
                cx.new(|cx| InputState::new(window, cx).placeholder("New caption field"));
            subs.push(cx.subscribe_in(
                &new_caption,
                window,
                |this, input, event: &InputEvent, window, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. }) {
                        return;
                    }
                    let name = input.read(cx).value().trim().to_string();
                    let count = app_state::settings(cx).storyboard.captions.len();
                    this.update_storyboard_preferences(
                        |prefs| {
                            prefs.captions.push(CaptionPreset {
                                name,
                                multiline: true,
                                print: true,
                            })
                        },
                        cx,
                    );
                    if app_state::settings(cx).storyboard.captions.len() > count {
                        input.update(cx, |input, cx| input.set_value("", window, cx));
                    }
                },
            ));
            self.settings_ui.storyboard = Some(StoryboardInputs {
                synced: saved.clone(),
                fields,
                captions,
                new_caption,
                message,
                _subs: subs,
            });
            return;
        }
        let Some(inputs) = &mut self.settings_ui.storyboard else {
            return;
        };
        if inputs.synced == saved {
            return;
        }
        // Saved elsewhere, or saved after trimming: show what is stored,
        // except in the field being typed in.
        inputs.synced = saved.clone();
        let values = inputs
            .fields
            .iter()
            .map(|(key, state)| (state.clone(), field_value(&saved, key)))
            .chain(
                inputs
                    .captions
                    .iter()
                    .zip(&saved.captions)
                    .map(|(state, c)| (state.clone(), c.name.clone())),
            )
            .collect::<Vec<_>>();
        for (state, value) in values {
            if !state.read(cx).focus_handle(cx).is_focused(window)
                && state.read(cx).value() != value
            {
                state.update(cx, |state, cx| state.set_value(value, window, cx));
            }
        }
    }

    /// The saved preferences with the section's typed values.
    fn storyboard_draft(&self, cx: &App) -> Result<Preferences, String> {
        let mut prefs = app_state::settings(cx).storyboard.clone();
        let Some(inputs) = &self.settings_ui.storyboard else {
            return Ok(prefs);
        };
        for ((key, state), (_, label)) in inputs.fields.iter().zip(FIELDS) {
            apply_field(&mut prefs, key, label, &state.read(cx).value())?;
        }
        for (preset, state) in prefs.captions.iter_mut().zip(&inputs.captions) {
            preset.name = state.read(cx).value().trim().to_string();
        }
        Ok(prefs)
    }

    /// Apply the typed values and `edit`, validate, then save. Invalid
    /// preferences are never saved; the section shows why.
    pub(crate) fn update_storyboard_preferences(
        &mut self,
        edit: impl FnOnce(&mut Preferences),
        cx: &mut Context<Self>,
    ) {
        let result = self.storyboard_draft(cx).and_then(|mut prefs| {
            edit(&mut prefs);
            prefs.validate().map(|()| prefs)
        });
        let message = match result {
            Ok(prefs) if prefs == app_state::settings(cx).storyboard => None,
            Ok(prefs) => {
                app_state::update_settings(cx, |s| s.storyboard = prefs);
                Some(("Storyboard preferences saved.".to_string(), false))
            }
            Err(error) => Some((error, true)),
        };
        if let Some(inputs) = &mut self.settings_ui.storyboard {
            inputs.message = message;
        }
        cx.notify();
    }

    /// The Storyboard section, with only the rows matching `query` unless
    /// the section itself matches. `None` when nothing matches.
    pub(crate) fn storyboard_settings(
        &mut self,
        p: &Palette,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.ensure_storyboard_inputs(window, cx);
        let saved = app_state::settings(cx).storyboard.clone();
        let draft = self.storyboard_draft(cx);
        let inputs = self.settings_ui.storyboard.as_ref()?;
        let title = "Storyboard preferences naming defaults board stage light table palette";
        let whole = matches(query, title);
        let shown = |text: &str| whole || matches(query, text);
        let row = |label: SharedString, content: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(mono(label, 10., p.muted).w(px(240.)).flex_none())
                .child(content)
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        for ((key, state), (_, label)) in inputs.fields.iter().zip(FIELDS) {
            if shown(label) {
                rows.push(
                    row(
                        label.into(),
                        div()
                            .id(SharedString::from(format!("settings-storyboard-{key}")))
                            .test_support()
                            .w(px(300.))
                            .border_1()
                            .border_color(p.line)
                            .child(Input::new(state).appearance(false))
                            .into_any_element(),
                    )
                    .into_any_element(),
                );
            }
        }
        let naming = draft.as_ref().map_or(&saved.naming, |d| &d.naming);
        let example = format!("{} / {}", naming.scene_name(0), naming.panel_name(1));
        if shown("Naming example scene panel") {
            rows.insert(
                0,
                row(
                    "Naming example".into(),
                    div()
                        .id("settings-storyboard-example")
                        .test_support()
                        .aria_label(example.clone())
                        .text_size(px(13.))
                        .child(example)
                        .into_any_element(),
                )
                .into_any_element(),
            );
        }
        for (id, label, on, toggle) in [
            (
                "settings-storyboard-per-scene",
                "Panel numbers restart in each scene",
                saved.naming.panels_per_scene,
                (|p: &mut Preferences| p.naming.panels_per_scene = !p.naming.panels_per_scene)
                    as fn(&mut Preferences),
            ),
            (
                "settings-storyboard-letters",
                "Inserted scenes take letters (10 → 10A)",
                saved.naming.insert_letters,
                |p| p.naming.insert_letters = !p.naming.insert_letters,
            ),
            (
                "settings-storyboard-board-captions",
                "Show captions on the board",
                saved.show_captions_on_board,
                |p| p.show_captions_on_board = !p.show_captions_on_board,
            ),
            (
                "settings-storyboard-field-guide",
                "New storyboards show the field guide",
                saved.stage.field_guide,
                |p| p.stage.field_guide = !p.stage.field_guide,
            ),
            (
                "settings-storyboard-light-table",
                "Light table on the Stage",
                saved.light_table.enabled,
                |p| p.light_table.enabled = !p.light_table.enabled,
            ),
            (
                "settings-storyboard-light-tint",
                "Tint light table panels (earlier red, later blue)",
                saved.light_table.tint,
                |p| p.light_table.tint = !p.light_table.tint,
            ),
            (
                "settings-storyboard-spelling",
                "Check spelling in captions",
                saved.check_spelling,
                |p| p.check_spelling = !p.check_spelling,
            ),
            (
                "settings-storyboard-review-thumbnails",
                "Hide review layers in thumbnails",
                saved.hide_review_in_thumbnails,
                |p| p.hide_review_in_thumbnails = !p.hide_review_in_thumbnails,
            ),
        ] {
            if shown(label) {
                rows.push(
                    chip(id, label, on, p)
                        .test_support()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.update_storyboard_preferences(toggle, cx)
                        }))
                        .into_any_element(),
                );
            }
        }
        if shown("Audio input device microphone recording") {
            rows.push(crate::settings_audio_input::audio_input_row(
                saved.audio_input.clone(),
                p,
                cx,
            ));
        }
        if shown("Scratch voices engine text-to-speech Piper eSpeak NG voices folder") {
            rows.push(crate::settings_voices::voices_rows(
                saved.voice_engine,
                saved.piper_voices.clone(),
                p,
                cx,
            ));
        }
        if shown("Palette swatches colours default reset") {
            rows.push(
                div()
                    .id("settings-storyboard-palette-swatches")
                    .test_support()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(4.))
                    .children(saved.palette.iter().map(|&[r, g, b]| {
                        div()
                            .size(px(16.))
                            .border_1()
                            .border_color(p.line)
                            .bg(rgb(u32::from_be_bytes([0, r, g, b])))
                    }))
                    .child(
                        chip(
                            "settings-storyboard-palette-reset",
                            "Default palette",
                            false,
                            p,
                        )
                        .test_support()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.update_storyboard_preferences(
                                |p| {
                                    p.palette =
                                        emulsion_core::storyboard_stage::DEFAULT_PALETTE.to_vec()
                                },
                                cx,
                            )
                        })),
                    )
                    .into_any_element(),
            );
        }
        if shown("Personal dictionary spelling words") {
            rows.push(spelling_words(&saved.spelling_words, p, cx));
        }
        if shown("Default caption fields multi-line print") {
            let count = saved.captions.len();
            let mut list = div()
                .id("settings-storyboard-captions")
                .test_support()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(mono("DEFAULT CAPTION FIELDS", 9.5, p.muted));
            for (index, (preset, state)) in saved.captions.iter().zip(&inputs.captions).enumerate()
            {
                let (multiline, print) = (preset.multiline, preset.print);
                let edit = |f: fn(&mut CaptionPreset)| {
                    cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                        this.update_storyboard_preferences(
                            |prefs| {
                                if let Some(preset) = prefs.captions.get_mut(index) {
                                    f(preset)
                                }
                            },
                            cx,
                        )
                    })
                };
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .id(("settings-storyboard-caption", index))
                                .test_support()
                                .w(px(220.))
                                .border_1()
                                .border_color(p.line)
                                .child(Input::new(state).appearance(false)),
                        )
                        .child(
                            chip(
                                ("settings-storyboard-caption-multiline", index),
                                "multi-line",
                                multiline,
                                p,
                            )
                            .test_support()
                            .on_click(edit(|c| c.multiline = !c.multiline)),
                        )
                        .child(
                            chip(
                                ("settings-storyboard-caption-print", index),
                                "print",
                                print,
                                p,
                            )
                            .test_support()
                            .on_click(edit(|c| c.print = !c.print)),
                        )
                        .when(index > 0, |d| {
                            d.child(
                                chip(("settings-storyboard-caption-up", index), "↑", false, p)
                                    .test_support()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.update_storyboard_preferences(
                                            |p| p.captions.swap(index - 1, index),
                                            cx,
                                        )
                                    })),
                            )
                        })
                        .when(index + 1 < count, |d| {
                            d.child(
                                chip(("settings-storyboard-caption-down", index), "↓", false, p)
                                    .test_support()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.update_storyboard_preferences(
                                            |p| p.captions.swap(index, index + 1),
                                            cx,
                                        )
                                    })),
                            )
                        })
                        .child(
                            chip(
                                ("settings-storyboard-caption-remove", index),
                                "remove",
                                false,
                                p,
                            )
                            .test_support()
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.update_storyboard_preferences(
                                        |p| {
                                            p.captions.remove(index);
                                        },
                                        cx,
                                    )
                                },
                            )),
                        ),
                );
            }
            list = list.child(
                div()
                    .id("settings-storyboard-new-caption")
                    .test_support()
                    .w(px(220.))
                    .border_1()
                    .border_color(p.line)
                    .child(Input::new(&inputs.new_caption).appearance(false)),
            );
            rows.push(list.into_any_element());
        }
        if rows.is_empty() {
            return None;
        }
        Some(
            div()
                .id("settings-storyboard")
                .test_support()
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
                        .child("Storyboard"),
                )
                .child(
                    div()
                        .max_w(px(640.))
                        .text_size(px(13.))
                        .text_color(p.muted)
                        .child("How scenes and panels are named, and what new storyboards, the board view and the Stage start with. Open storyboards keep their own naming, caption fields, Stage guides and palette; Apply storyboard preferences on the Board copies these to one. The light table settings apply to every storyboard."),
                )
                .children(rows)
                .children(inputs.message.clone().map(|(message, error)| {
                    div()
                        .id("settings-storyboard-message")
                        .test_support()
                        .aria_label(message.clone())
                        .child(mono(message, 10.5, if error { p.accent } else { p.ink }))
                }))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::actions;
    use crate::tests::open;
    use ::core::prelude::v1::test;
    use emulsion_core::Document;
    use emulsion_core::creation::CanvasKind;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::*;

    fn settle(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
        cx.run_until_parked();
    }

    /// Replace a Settings field's text and press Enter.
    fn type_field(cx: &mut VisualTestContext, id: impl Into<ElementId>, value: &str) {
        let id = id.into();
        cx.update(|window, cx| window.click(id, cx));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        cx.simulate_input(value);
        cx.simulate_keystrokes("enter");
        settle(cx);
    }

    fn field(key: &str) -> SharedString {
        format!("settings-storyboard-{key}").into()
    }

    fn label(cx: &mut VisualTestContext, id: &'static str) -> String {
        cx.update(|window, _| window.find(id).label().unwrap_or_default().to_string())
    }

    #[gpui_kit::test]
    fn storyboard_preferences_validate_save_and_start_new_storyboards(cx: &mut TestAppContext) {
        let (ws, cx) = open(cx, Document::new(32, 32));
        cx.simulate_resize(size(px(1600.), px(4000.)));
        cx.update(|window, _| window.activate_window());
        cx.update(|window, cx| window.dispatch_action(Box::new(actions::ShowSettings), cx));
        settle(cx);
        // Searching brings the section to the top.
        cx.update(|window, cx| window.click("settings-search", cx));
        cx.simulate_input("storyboard");
        settle(cx);
        assert_eq!(label(cx, "settings-storyboard-example"), "1 / Panel 1");
        type_field(cx, field("scene_prefix"), "SC");
        type_field(cx, field("scene_start"), "10");
        type_field(cx, field("scene_digits"), "3");
        assert_eq!(label(cx, "settings-storyboard-example"), "SC010 / Panel 1");
        cx.update(|_, cx| {
            let naming = &crate::app_state::settings(cx).storyboard.naming;
            assert_eq!(
                (naming.scene_prefix.as_str(), naming.scene_start),
                ("SC", 10)
            );
        });
        // Invalid values are explained and never saved.
        type_field(cx, field("thumbnail_width"), "10");
        assert!(label(cx, "settings-storyboard-message").contains("96–480"));
        cx.update(|_, cx| {
            assert_eq!(
                crate::app_state::settings(cx).storyboard.thumbnail_width,
                200
            )
        });
        type_field(cx, field("thumbnail_width"), "240");
        // Stage, palette and light table defaults.
        type_field(cx, field("overscan"), "20");
        type_field(cx, field("fields"), "16");
        type_field(cx, field("palette"), "#000000, #E8A040");
        type_field(cx, field("light_opacity"), "500");
        assert!(label(cx, "settings-storyboard-message").contains("5–100%"));
        type_field(cx, field("light_opacity"), "50");
        type_field(cx, field("light_before"), "2");
        cx.update(|window, cx| window.click("settings-storyboard-light-table", cx));
        settle(cx);
        cx.update(|_, cx| {
            let saved = &crate::app_state::settings(cx).storyboard;
            assert_eq!((saved.stage.overscan, saved.stage.fields), (20., 16));
            assert_eq!(saved.palette, [[0, 0, 0], [232, 160, 64]]);
            let table = &saved.light_table;
            assert!(table.enabled);
            assert_eq!((table.before, table.opacity), (2, 0.5));
        });
        type_field(cx, field("panel_seconds"), "3");
        type_field(cx, field("smart_add"), "Background, Characters");
        cx.update(|window, cx| window.click(("settings-storyboard-caption-remove", 3usize), cx));
        settle(cx);
        type_field(cx, "settings-storyboard-new-caption", "Camera");
        cx.update(|window, cx| window.click("settings-storyboard-board-captions", cx));
        settle(cx);
        let saved = cx.update(|_, cx| crate::app_state::settings(cx).storyboard.clone());
        saved.validate().unwrap();
        assert_eq!(saved.thumbnail_width, 240);
        assert_eq!(saved.panel_seconds, 3.);
        assert_eq!(saved.smart_add_layers, ["Background", "Characters"]);
        assert!(!saved.show_captions_on_board);
        let names: Vec<_> = saved.captions.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Action", "Dialogue", "Slugging", "Camera"]);

        // New storyboards start from these preferences.
        cx.update(|window, cx| window.dispatch_action(Box::new(actions::ShowEditor), cx));
        settle(cx);
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.new_document(window, cx)));
        settle(cx);
        cx.update(|window, cx| {
            window.click(("new-canvas-kind", CanvasKind::Storyboard as usize), cx)
        });
        settle(cx);
        cx.update(|window, cx| window.click("new-canvas-create", cx));
        settle(cx);
        cx.update(|_, cx| {
            let workspace = ws.read(cx);
            assert_eq!(workspace.tabs.len(), 2);
            let editor = workspace.editor.as_ref().unwrap().read(cx);
            let board = editor.editor.storyboard().expect("a storyboard");
            let names: Vec<_> = board.captions.iter().map(|c| c.name.as_str()).collect();
            assert_eq!(names, ["Action", "Dialogue", "Slugging", "Camera"]);
            assert_eq!(board.settings.panel_frames, 72);
            assert_eq!(board.smart_add_layers, ["Background", "Characters"]);
            assert_eq!(board.stage.overscan, 20.);
            assert_eq!(board.palette, [[0, 0, 0], [232, 160, 64]]);
            let scene = board.scenes.values().next().unwrap();
            assert_eq!(scene.name, "SC010");
        });
    }

    #[gpui_kit::test]
    fn settings_search_filters_sections_and_rows(cx: &mut TestAppContext) {
        let (_ws, cx) = open(cx, Document::new(32, 32));
        cx.simulate_resize(size(px(1600.), px(4000.)));
        cx.update(|window, _| window.activate_window());
        cx.update(|window, cx| window.dispatch_action(Box::new(actions::ShowSettings), cx));
        settle(cx);
        let present = |cx: &mut VisualTestContext, id: &'static str| {
            cx.update(|window, _| window.try_find(id).is_some())
        };
        for id in [
            "settings-jev",
            "settings-shortcuts",
            "settings-storyboard-section",
        ] {
            assert!(present(cx, id), "{id} shows without a search");
        }
        let search = |cx: &mut VisualTestContext, text: &str| {
            cx.update(|window, cx| window.click("settings-search", cx));
            cx.simulate_keystrokes(if cfg!(target_os = "macos") {
                "cmd-a"
            } else {
                "ctrl-a"
            });
            if text.is_empty() {
                cx.simulate_keystrokes("backspace");
            } else {
                cx.simulate_input(text);
            }
            settle(cx);
        };
        // A row match shows that row of its section only.
        search(cx, "thumbnail");
        assert!(present(cx, "settings-storyboard-thumbnail_width"));
        assert!(!present(cx, "settings-storyboard-scene_prefix"));
        assert!(!present(cx, "settings-jev"));
        assert!(!present(cx, "settings-shortcuts"));
        // A section match shows the whole section.
        search(cx, "storyboard");
        assert!(present(cx, "settings-storyboard-scene_prefix"));
        assert!(present(cx, "settings-storyboard-captions"));
        search(cx, "typesafe");
        assert!(present(cx, "settings-jev"));
        assert!(!present(cx, "settings-storyboard-section"));
        // Shortcuts match by action or keys.
        search(cx, "free transform");
        assert!(present(cx, "settings-shortcuts"));
        assert!(!present(cx, "settings-experimental"));
        search(cx, "no such setting");
        assert!(present(cx, "settings-no-results"));
        search(cx, "");
        assert!(present(cx, "settings-experimental"));
        assert!(present(cx, "settings-storyboard-section"));
    }
}
