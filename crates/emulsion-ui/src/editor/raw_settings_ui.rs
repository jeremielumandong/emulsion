//! Explicit portable settings workflows; file IO never runs during rendering.
use super::*;
use emulsion_io::raw_settings::{self, RawSettingsGroup};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Disableable, Selectable, Sizable};

impl EditorView {
    #[cfg(test)]
    pub(crate) fn raw_settings_is_busy(&self) -> bool {
        self.raw.settings_busy
    }

    pub(crate) fn raw_settings_file(&mut self, save: bool, sidecar: bool, cx: &mut Context<Self>) {
        if self.raw.is_pending() || self.raw.settings_busy {
            return;
        }
        let Some(raw) = self.editor.doc.raw.clone() else {
            return;
        };
        let doc = self.editor.doc.clone();
        let ticket = self.edit_ticket();
        let group = self.raw.settings_group;
        self.raw.settings_busy = true;
        cx.notify();
        // Native save/open prompts have different response types. Normalize
        // them into one foreground task before running any disk operation.
        let pick = if save {
            let suggested = raw_settings::suggested_sidecar_path(&doc)
                .unwrap_or_else(|_| raw.source.with_extension("emulsion-raw.json"));
            let dir = suggested.parent().unwrap_or(std::path::Path::new("."));
            let name = if sidecar {
                suggested
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            } else {
                "settings.emulsion-preset.json".into()
            };
            let rx = cx.prompt_for_new_path(dir, Some(&name));
            cx.spawn(async move |_, _| rx.await.ok().and_then(|r| r.ok()).flatten())
        } else {
            let rx = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(
                    if sidecar {
                        t!("editor.raw_settings_ui.load_sidecar_prompt")
                    } else {
                        t!("editor.raw_settings_ui.load_preset_prompt")
                    }
                    .into(),
                ),
            });
            cx.spawn(async move |_, _| {
                rx.await
                    .ok()
                    .and_then(|r| r.ok())
                    .flatten()
                    .and_then(|paths| paths.into_iter().next())
            })
        };
        cx.spawn(async move |this, cx| {
            let Some(path) = pick.await else {
                this.update(cx, |this, cx| {
                    this.raw.settings_busy = false;
                    cx.notify();
                })
                .ok();
                return;
            };
            let current = this
                .update(cx, |this, _| {
                    this.edit_is_current(ticket)
                        && this.editor.doc.raw.as_ref() == Some(&raw)
                        && !this.raw.is_pending()
                })
                .unwrap_or(false);
            if !current {
                this.update(cx, |this, cx| {
                    this.raw.settings_busy = false;
                    this.set_status(t!("editor.raw_settings_ui.cancelled_changed"), true, cx);
                })
                .ok();
                return;
            }
            let result = cx
                .background_spawn(async move {
                    if save {
                        if sidecar {
                            raw_settings::save_sidecar(&doc, &path)?;
                        } else {
                            raw_settings::save_document_preset(&doc, &path)?;
                        }
                        Ok::<_, emulsion_io::IoError>(None)
                    } else {
                        let loaded = if sidecar {
                            raw_settings::load_sidecar(&doc, &path)?
                        } else {
                            raw_settings::load_document_preset(&doc, &path, group)?
                        };
                        Ok(Some(raw_settings::merge_settings(
                            doc.raw.as_ref().expect("checked RAW source").params,
                            loaded,
                            group,
                        )))
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.raw.settings_busy = false;
                match result {
                    Ok(Some(params))
                        if this.edit_is_current(ticket)
                            && this.editor.doc.raw.as_ref() == Some(&raw)
                            && !this.raw.is_pending() =>
                    {
                        this.raw_apply_params(params, cx)
                    }
                    Ok(Some(_)) => {
                        this.set_status(t!("editor.raw_settings_ui.not_applied_changed"), true, cx)
                    }
                    Ok(None) => this.set_status(
                        if sidecar {
                            t!("editor.raw_settings_ui.sidecar_saved")
                        } else {
                            t!("editor.raw_settings_ui.preset_saved")
                        },
                        false,
                        cx,
                    ),
                    Err(e) => this.set_status(
                        if save {
                            t!("editor.raw_settings_ui.save_failed", error = e)
                        } else {
                            t!("editor.raw_settings_ui.load_failed", error = e)
                        },
                        true,
                        cx,
                    ),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 0 = apply, 1 = save, 2 = reset only this camera's stored defaults.
    pub(super) fn raw_camera_defaults(&mut self, action: u8, cx: &mut Context<Self>) {
        if action > 2 || self.raw.is_pending() || self.raw.settings_busy {
            return;
        }
        let Some(raw) = self.editor.doc.raw.clone() else {
            return;
        };
        let ticket = self.edit_ticket();
        let source = raw.clone();
        let group = self.raw.settings_group;
        self.raw.settings_busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    match action {
                        0 => raw_settings::camera_defaults(&source.metadata),
                        1 => raw_settings::save_camera_defaults(&source.metadata, source.params)
                            .map(|_| None),
                        _ => raw_settings::reset_camera_defaults(&source.metadata).map(|_| None),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.raw.settings_busy = false;
                match result {
                    Ok(Some(params))
                        if this.edit_is_current(ticket)
                            && this.editor.doc.raw.as_ref() == Some(&raw)
                            && !this.raw.is_pending() =>
                    {
                        this.raw_apply_params(
                            raw_settings::merge_settings(raw.params, params, group),
                            cx,
                        );
                    }
                    Ok(Some(_)) => {
                        this.set_status(t!("editor.raw_settings_ui.defaults_not_applied"), true, cx)
                    }
                    Ok(None) => this.set_status(
                        match action {
                            0 => t!("editor.raw_settings_ui.no_defaults"),
                            1 => t!("editor.raw_settings_ui.defaults_saved"),
                            _ => t!("editor.raw_settings_ui.defaults_reset"),
                        },
                        false,
                        cx,
                    ),
                    Err(e) => this.set_status(
                        t!("editor.raw_settings_ui.defaults_failed", error = e),
                        true,
                        cx,
                    ),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_raw_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let disabled = self.raw.is_pending() || self.raw.settings_busy;
        let mut groups = div().flex().flex_wrap().gap_1();
        for (key, label, group) in [
            (
                "raw-group-all",
                t!("editor.raw_settings_ui.all"),
                RawSettingsGroup::All,
            ),
            (
                "raw-group-wb",
                t!("editor.raw_settings_ui.white_balance"),
                RawSettingsGroup::WhiteBalance,
            ),
            (
                "raw-group-tone",
                t!("editor.raw_settings_ui.tone"),
                RawSettingsGroup::Tone,
            ),
            (
                "raw-group-curve",
                t!("editor.raw_panel.curve"),
                RawSettingsGroup::Curve,
            ),
        ] {
            groups = groups.child(
                Button::new(key)
                    .label(label)
                    .small()
                    .ghost()
                    .selected(self.raw.settings_group == group)
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.raw.settings_group = group;
                        cx.notify();
                    })),
            );
        }
        let mut files = div().flex().flex_wrap().gap_1();
        for (key, label, save, sidecar) in [
            (
                "save-sidecar",
                t!("editor.raw_settings_ui.save_sidecar"),
                true,
                true,
            ),
            (
                "load-sidecar",
                t!("editor.raw_settings_ui.load_sidecar"),
                false,
                true,
            ),
            (
                "save-preset",
                t!("editor.raw_settings_ui.save_preset"),
                true,
                false,
            ),
            (
                "load-preset",
                t!("editor.raw_settings_ui.load_preset"),
                false,
                false,
            ),
        ] {
            files =
                files.child(
                    Button::new(key)
                        .label(label)
                        .small()
                        .ghost()
                        .disabled(disabled)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.raw_settings_file(save, sidecar, cx)
                        })),
                );
        }
        let mut defaults = div().flex().flex_wrap().gap_1();
        for (action, key, label) in [
            (
                0,
                "raw-defaults-apply",
                t!("editor.raw_settings_ui.apply_defaults"),
            ),
            (
                1,
                "raw-defaults-save",
                t!("editor.raw_settings_ui.save_defaults"),
            ),
            (
                2,
                "raw-defaults-reset",
                t!("editor.raw_settings_ui.reset_defaults"),
            ),
        ] {
            defaults = defaults.child(
                Button::new(key)
                    .label(label)
                    .small()
                    .ghost()
                    .disabled(disabled)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.raw_camera_defaults(action, cx)),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("editor.raw_settings_ui.apply_group")),
            )
            .child(groups)
            .child(files)
            .child(
                Button::new("raw-synchronize")
                    .label(t!("editor.raw_settings_ui.synchronize"))
                    .small()
                    .ghost()
                    .disabled(disabled)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::SynchronizeRaw), cx)
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("editor.raw_settings_ui.camera_defaults")),
            )
            .child(defaults)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.raw.settings_busy {
                        t!("editor.raw_settings_ui.busy")
                    } else {
                        t!("editor.raw_settings_ui.format_note")
                    }),
            )
            .into_any_element()
    }
}
