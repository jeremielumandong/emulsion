//! Apply selected RAW groups to explicitly selected open photos.
use super::*;
use emulsion_core::raw::DevelopParams;
use emulsion_io::raw_settings::{RawSettingsGroup, merge_settings};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme, Disableable};

#[derive(Clone)]
struct Target {
    editor: WeakEntity<EditorView>,
    name: String,
    selected: bool,
    wb_compatible: bool,
    expected: emulsion_core::raw::RawDocument,
    ticket: (u64, u64),
}

struct RawSyncDialog {
    source: DevelopParams,
    targets: Vec<Target>,
    group: RawSettingsGroup,
}

impl RawSyncDialog {
    fn apply(&mut self, cx: &mut Context<Self>) {
        let group = self.group;
        let source = self.source;
        let targets: Vec<_> = self
            .targets
            .iter()
            .filter(|t| {
                t.selected
                    && (t.wb_compatible
                        || matches!(group, RawSettingsGroup::Tone | RawSettingsGroup::Curve))
            })
            .cloned()
            .collect();
        cx.spawn(async move |_, cx| {
            for target in targets {
                let wait = target
                    .editor
                    .update(cx, |editor, cx| {
                        if !editor.edit_is_current(target.ticket)
                            || editor.editor.doc.raw.as_ref() != Some(&target.expected)
                        {
                            editor.set_status(
                                "RAW synchronization skipped because this photo changed.",
                                true,
                                cx,
                            );
                            return None;
                        }
                        if editor.raw.is_pending() || editor.editor.in_transaction() {
                            editor.set_status(
                                "Finish the current edit before synchronizing RAW settings.",
                                true,
                                cx,
                            );
                            return None;
                        }
                        editor.raw_params().map(|current| {
                            editor.raw_apply_params_wait(merge_settings(current, source, group), cx)
                        })
                    })
                    .ok()
                    .flatten();
                if let Some(wait) = wait {
                    let _ = wait.recv().await;
                }
            }
        })
        .detach();
    }
}

impl Render for RawSyncDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = div().flex().flex_col().gap_3()
            .child(div().text_sm().child("Choose settings and open photos. Each photo keeps its original and its own undo history."));
        let mut groups = div().flex().flex_wrap().gap_1();
        for (key, name, group) in [
            ("raw-sync-all", "All", RawSettingsGroup::All),
            (
                "raw-sync-wb",
                "White balance",
                RawSettingsGroup::WhiteBalance,
            ),
            ("raw-sync-tone", "Tone", RawSettingsGroup::Tone),
            ("raw-sync-curve", "Curve", RawSettingsGroup::Curve),
        ] {
            groups = groups.child(
                Button::new(key)
                    .label(name)
                    .small()
                    .ghost()
                    .selected(self.group == group)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.group = group;
                        cx.notify();
                    })),
            );
        }
        content = content.child(groups).child(
            Button::new("raw-sync-select-all")
                .label("Select all photos")
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    for target in &mut this.targets {
                        target.selected = target.wb_compatible
                            || matches!(
                                this.group,
                                RawSettingsGroup::Tone | RawSettingsGroup::Curve
                            );
                    }
                    cx.notify();
                })),
        );
        let mut list = div()
            .id("raw-sync-targets")
            .max_h_64()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2();
        for target in &self.targets {
            let id = target.editor.entity_id();
            let compatible = target.wb_compatible
                || matches!(self.group, RawSettingsGroup::Tone | RawSettingsGroup::Curve);
            let title = if compatible {
                target.name.clone()
            } else {
                format!("{} (sampled WB needs the same camera)", target.name)
            };
            list = list.child(
                Checkbox::new(("raw-sync-photo", id))
                    .label(title)
                    .disabled(!compatible)
                    .checked(target.selected && compatible)
                    .on_click(cx.listener(move |this, selected: &bool, _, cx| {
                        if let Some(target) =
                            this.targets.iter_mut().find(|t| t.editor.entity_id() == id)
                        {
                            target.selected = *selected;
                        }
                        cx.notify();
                    })),
            );
        }
        content
            .child(list)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("raw-sync-cancel")
                            .label("Cancel")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("raw-sync-apply")
                            .label("Synchronize")
                            .primary()
                            .small()
                            .disabled(!self.targets.iter().any(|t| {
                                t.selected
                                    && (t.wb_compatible
                                        || matches!(
                                            self.group,
                                            RawSettingsGroup::Tone | RawSettingsGroup::Curve
                                        ))
                            }))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply(cx);
                                window.close_dialog(cx);
                            })),
                    ),
            )
            .text_color(cx.theme().foreground)
    }
}

impl Workspace {
    pub(crate) fn synchronize_raw(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.style_dialog_open(cx) {
            return;
        }
        let Some(source_editor) = &self.editor else {
            return;
        };
        let source = source_editor.read(cx);
        if source.raw.is_pending() || source.editor.in_transaction() {
            return;
        }
        let Some(source_raw) = source.editor.doc.raw.as_ref() else {
            return;
        };
        let params = source_raw.params;
        let source_name = source.name.clone();
        // Sampled multipliers describe this camera's channels, not a universal color.
        let targets: Vec<_> = self
            .tabs
            .iter()
            .filter(|target| *target != source_editor)
            .filter_map(|target| {
                let view = target.read(cx);
                let raw = view.editor.doc.raw.as_ref()?;
                let wb_compatible = params.wb_override.is_none()
                    || (raw
                        .metadata
                        .make
                        .trim()
                        .eq_ignore_ascii_case(source_raw.metadata.make.trim())
                        && raw
                            .metadata
                            .model
                            .trim()
                            .eq_ignore_ascii_case(source_raw.metadata.model.trim()));
                Some(Target {
                    editor: target.downgrade(),
                    name: view.name.clone(),
                    selected: false,
                    wb_compatible,
                    expected: raw.clone(),
                    ticket: view.edit_ticket(),
                })
            })
            .collect();
        if targets.is_empty() {
            source_editor.update(cx, |editor,cx| editor.set_status(
                "Open another RAW photo to synchronize. Sampled white balance requires the same camera model.", false,cx));
            return;
        }
        let dialog_view = cx.new(|_| RawSyncDialog {
            source: params,
            targets,
            group: RawSettingsGroup::All,
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let submit = dialog_view.clone();
            dialog
                .title(format!("Synchronize from {source_name}"))
                .footer(div())
                .on_ok(move |_, _, cx| {
                    submit.update(cx, |this, cx| this.apply(cx));
                    true
                })
                .child(dialog_view.clone())
        });
    }
}
