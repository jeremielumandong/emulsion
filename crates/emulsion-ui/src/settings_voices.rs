//! Settings › Storyboard › Scratch voices (AI8): which text-to-speech engine
//! speaks scratch dialogue (Auto, Piper or eSpeak NG) and the folder of
//! downloaded Piper voices. Both engines run on this computer; the row says
//! which are installed and how many Piper voices the folder holds, checked
//! when Settings is shown.
use crate::file_prompt::FilePrompts;
use crate::theme::Palette;
use crate::widgets::{chip, mono};
use crate::workspace::Workspace;
use emulsion_core::storyboard_voices::EngineChoice;
use emulsion_io::voices;
use gpui_kit::component::{
    Sizable,
    button::Button,
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use parking_lot::Mutex;
use std::path::Path;

/// What was found: for the folder, whether Piper and eSpeak NG run, and
/// how many Piper voices the folder holds. Checked when first shown, when
/// the folder changes and on Refresh, never on every repaint.
type Found = (Option<String>, bool, bool, usize);
static FOUND: Mutex<Option<Found>> = Mutex::new(None);

fn found(folder: Option<&str>, refresh: bool) -> Found {
    let mut cached = FOUND.lock();
    match &*cached {
        Some(f) if !refresh && f.0.as_deref() == folder => f.clone(),
        _ => {
            let f = (
                folder.map(str::to_string),
                voices::piper_available(),
                voices::espeak_available(),
                folder.map_or(0, |f| voices::piper_models(Path::new(f)).len()),
            );
            *cached = Some(f.clone());
            f
        }
    }
}

const CHOICES: [(EngineChoice, &str); 3] = [
    (EngineChoice::Auto, "Automatic (Piper, else eSpeak NG)"),
    (EngineChoice::Piper, "Piper"),
    (EngineChoice::Espeak, "eSpeak NG"),
];

/// What the row says about the engines.
fn engines_note(piper: bool, espeak: bool, models: usize, folder: Option<&str>) -> String {
    let installed = match (piper, espeak) {
        (true, true) => "Piper and eSpeak NG are installed.",
        (true, false) => "Piper is installed.",
        (false, true) => "eSpeak NG is installed.",
        (false, false) => voices::MISSING,
    };
    let folder = match (folder, models) {
        (None, _) if piper => " Choose the folder of your Piper voices.",
        (None, _) => "",
        (Some(_), 0) => " The Piper voices folder has no voice (.onnx with its .onnx.json).",
        (Some(_), 1) => " 1 Piper voice found.",
        (Some(_), n) => return format!("{installed} {n} Piper voices found."),
    };
    format!("{installed}{folder}")
}

/// The settings rows: the engine and the Piper voices folder.
pub(crate) fn voices_rows(
    choice: EngineChoice,
    folder: Option<String>,
    p: &Palette,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let owner = cx.weak_entity();
    let (_, piper, espeak, models) = found(folder.as_deref(), false);
    let note = engines_note(piper, espeak, models, folder.as_deref());
    let label = CHOICES
        .iter()
        .find(|(c, _)| *c == choice)
        .map_or("", |(_, l)| l);
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    mono("Scratch voices · engine", 10., p.muted)
                        .w(px(240.))
                        .flex_none(),
                )
                .child(
                    Button::new("settings-storyboard-voice-engine")
                        .label(format!("{label} ▾"))
                        .small()
                        .outline()
                        .dropdown_menu(move |mut menu, _, _| {
                            for (value, text) in CHOICES {
                                let owner = owner.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(text).checked(value == choice).on_click(
                                        move |_, _, cx| {
                                            owner
                                                .update(cx, |w, cx| {
                                                    w.update_storyboard_preferences(
                                                        |prefs| prefs.voice_engine = value,
                                                        cx,
                                                    )
                                                })
                                                .ok();
                                        },
                                    ),
                                );
                            }
                            menu
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    mono("Scratch voices · Piper voices folder", 10., p.muted)
                        .w(px(240.))
                        .flex_none(),
                )
                .child(
                    div()
                        .id("settings-storyboard-piper-folder")
                        .test_support()
                        .max_w(px(320.))
                        .truncate()
                        .text_size(px(12.))
                        .child(folder.clone().unwrap_or_else(|| "Not chosen".into())),
                )
                .child(
                    chip("settings-storyboard-piper-choose", "Choose…", false, p)
                        .test_support()
                        .on_click(cx.listener(|_, _, _, cx| {
                            let rx = cx.prompt_open_paths(PathPromptOptions {
                                files: false,
                                directories: true,
                                multiple: false,
                                prompt: Some("Choose the folder of your Piper voices".into()),
                            });
                            cx.spawn(async move |this, cx| {
                                let Ok(Ok(Some(paths))) = rx.await else {
                                    return;
                                };
                                let Some(path) = paths.into_iter().next() else {
                                    return;
                                };
                                this.update(cx, |w, cx| {
                                    w.update_storyboard_preferences(
                                        |prefs| {
                                            prefs.piper_voices =
                                                Some(path.to_string_lossy().into_owned())
                                        },
                                        cx,
                                    )
                                })
                                .ok();
                            })
                            .detach();
                        })),
                )
                .when(folder.is_some(), |d| {
                    d.child(
                        chip("settings-storyboard-piper-clear", "Clear", false, p)
                            .test_support()
                            .on_click(cx.listener(|w, _, _, cx| {
                                w.update_storyboard_preferences(|p| p.piper_voices = None, cx)
                            })),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    chip("settings-storyboard-voices-refresh", "Refresh", false, p)
                        .test_support()
                        .on_click(cx.listener(move |_, _, _, cx| {
                            found(None, true);
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id("settings-storyboard-voices-note")
                        .test_support()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(note),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    #[test]
    fn the_note_says_what_is_installed_and_found() {
        assert_eq!(engines_note(false, false, 0, None), voices::MISSING);
        assert_eq!(
            engines_note(false, true, 0, None),
            "eSpeak NG is installed."
        );
        assert!(engines_note(true, false, 0, None).contains("Choose the folder"));
        assert!(engines_note(true, true, 0, Some("/v")).contains("has no voice"));
        assert!(engines_note(true, true, 3, Some("/v")).ends_with("3 Piper voices found."));
    }
}
