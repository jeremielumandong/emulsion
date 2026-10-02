//! Settings › Storyboard › Audio input device (T13): the microphone the
//! Timeline and the Panel Timer record from, by name, or the system
//! default. The device list is read when first shown and again on
//! **Refresh**, never in the background.
use crate::playback::recorder::input_devices;
use crate::theme::Palette;
use crate::widgets::{chip, mono};
use crate::workspace::Workspace;
use gpui_kit::component::{
    Sizable,
    button::Button,
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use parking_lot::Mutex;

/// The devices last listed, or why listing failed.
static DEVICES: Mutex<Option<Result<Vec<String>, String>>> = Mutex::new(None);

fn devices(refresh: bool) -> Result<Vec<String>, String> {
    let mut cached = DEVICES.lock();
    if refresh || cached.is_none() {
        *cached = Some(input_devices());
    }
    cached.clone().unwrap_or(Ok(Vec::new()))
}

/// What the row says about the device list.
fn listing_note(listed: &Option<Result<Vec<String>, String>>, current: Option<&str>) -> String {
    match listed {
        None => "Open the list or press Refresh to find inputs.".into(),
        Some(Err(error)) => error.clone(),
        Some(Ok(names)) if names.is_empty() => {
            "No microphone found. Connect one and press Refresh.".into()
        }
        Some(Ok(names)) => match current {
            Some(name) if !names.iter().any(|n| n == name) => {
                format!("“{name}” is not connected; recording will say so until it is.")
            }
            _ => format!(
                "{} input{} found.",
                names.len(),
                if names.len() == 1 { "" } else { "s" }
            ),
        },
    }
}

/// The settings row: the chosen input, the list and Refresh.
pub(crate) fn audio_input_row(
    current: Option<String>,
    p: &Palette,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let owner = cx.weak_entity();
    let note = listing_note(&DEVICES.lock().clone(), current.as_deref());
    let label = current
        .clone()
        .unwrap_or_else(|| "System default".to_string());
    let chosen = current.clone();
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(
            mono("Audio input device", 10., p.muted)
                .w(px(240.))
                .flex_none(),
        )
        .child(
            Button::new("settings-storyboard-audio-input")
                .label(format!("{label} ▾"))
                .small()
                .outline()
                .dropdown_menu(move |menu, _, _| {
                    let pick = |text: String, value: Option<String>| {
                        let owner = owner.clone();
                        PopupMenuItem::new(text).checked(value == chosen).on_click(
                            move |_, _, cx| {
                                let value = value.clone();
                                owner
                                    .update(cx, |w, cx| {
                                        w.update_storyboard_preferences(
                                            |prefs| prefs.audio_input = value,
                                            cx,
                                        )
                                    })
                                    .ok();
                            },
                        )
                    };
                    let mut menu = menu.item(pick("System default".into(), None));
                    match devices(false) {
                        Ok(names) => {
                            for name in names {
                                menu = menu.item(pick(name.clone(), Some(name)));
                            }
                        }
                        Err(error) => menu = menu.item(PopupMenuItem::new(error).disabled(true)),
                    }
                    menu
                }),
        )
        .child(
            chip("settings-storyboard-audio-refresh", "Refresh", false, p)
                .test_support()
                .on_click(cx.listener(|_, _, _, cx| {
                    devices(true).ok();
                    cx.notify();
                })),
        )
        .child(
            div()
                .id("settings-storyboard-audio-note")
                .test_support()
                .text_size(px(11.))
                .text_color(p.muted)
                .when(note.contains("not connected"), |d| d.text_color(p.accent))
                .child(note),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    #[test]
    fn the_note_explains_the_list() {
        assert!(listing_note(&None, None).contains("Refresh"));
        assert_eq!(
            listing_note(&Some(Err("No access.".into())), None),
            "No access."
        );
        assert!(listing_note(&Some(Ok(Vec::new())), None).contains("No microphone"));
        let two = Some(Ok(vec!["Built-in".to_string(), "USB".to_string()]));
        assert_eq!(listing_note(&two, Some("USB")), "2 inputs found.");
        assert!(listing_note(&two, Some("Headset")).contains("not connected"));
    }
}
