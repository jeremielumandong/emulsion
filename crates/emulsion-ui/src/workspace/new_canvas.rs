//! Native, preset-driven document creation. Nothing edits the current tab until
//! the complete specification validates and the user chooses Create.
use super::*;
use emulsion_core::creation::{Background, CanvasKind, CanvasSpec, Unit, presets};
use gpui_kit::component::Disableable;
use gpui_kit::component::input::{Input, InputEvent, InputState};

struct NewCanvas {
    workspace: WeakEntity<Workspace>,
    spec: CanvasSpec,
    fields: [Entity<InputState>; 4],
    category: String,
    notice: Option<String>,
    submitted: bool,
    _subscriptions: Vec<Subscription>,
}

impl NewCanvas {
    fn new(workspace: WeakEntity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let spec = CanvasSpec::default();
        let fields = [
            spec.name.clone(),
            spec.width.to_string(),
            spec.height.to_string(),
            spec.resolution.to_string(),
        ]
        .map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value)));
        let mut subscriptions = Vec::new();
        for field in &fields {
            subscriptions.push(
                cx.subscribe_in(field, window, |this, _, event, window, cx| match event {
                    InputEvent::Change => {
                        this.notice = None;
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } if this.submit(window, cx) => {
                        window.close_dialog(cx);
                    }
                    _ => {}
                }),
            );
        }
        Self {
            workspace,
            spec,
            fields,
            category: "Screen".into(),
            notice: None,
            submitted: false,
            _subscriptions: subscriptions,
        }
    }

    fn draft(&self, cx: &App) -> Result<CanvasSpec, String> {
        let mut spec = self.spec.clone();
        spec.name = self.fields[0].read(cx).value().trim().to_string();
        let number = |index: usize, label: &str| {
            self.fields[index]
                .read(cx)
                .value()
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("Enter a number for {label}."))
        };
        spec.width = number(1, "width")?;
        spec.height = number(2, "height")?;
        spec.resolution = number(3, "resolution")?;
        spec.validate()?;
        Ok(spec)
    }

    fn show_spec(&mut self, spec: CanvasSpec, window: &mut Window, cx: &mut Context<Self>) {
        let values = [
            spec.name.clone(),
            spec.width.to_string(),
            spec.height.to_string(),
            spec.resolution.to_string(),
        ];
        for (field, value) in self.fields.iter().zip(values) {
            field.update(cx, |field, cx| field.set_value(value, window, cx));
        }
        self.spec = spec;
        self.notice = None;
        cx.notify();
    }

    fn pick_kind(&mut self, kind: CanvasKind, window: &mut Window, cx: &mut Context<Self>) {
        let mut spec = self.draft(cx).unwrap_or_else(|_| self.spec.clone());
        if spec.name == "Untitled photo" || spec.name == "Untitled paint" {
            spec.name = format!("Untitled {}", kind.label().to_lowercase());
        }
        spec.kind = kind;
        spec.background = if kind == CanvasKind::Paint {
            Background::Paper
        } else {
            Background::White
        };
        let preset = &presets(kind)[0];
        preset.apply(&mut spec);
        self.category = preset.category.into();
        self.show_spec(spec, window, cx);
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.submitted {
            return true;
        }
        let result = self
            .draft(cx)
            .and_then(|spec| spec.create().map(|doc| (spec, doc)));
        let (spec, doc) = match result {
            Ok(value) => value,
            Err(error) => {
                self.notice = Some(error);
                cx.notify();
                return false;
            }
        };
        let Some(workspace) = self.workspace.upgrade() else {
            return false;
        };
        crate::app_state::update_settings(cx, |settings| {
            settings.recent_canvases.retain(|old| old != &spec);
            settings.recent_canvases.insert(0, spec.clone());
            settings.recent_canvases.truncate(8);
        });
        self.submitted = true;
        workspace.update(cx, |workspace, cx| {
            workspace.add_tab_then(window, cx, move |workspace, window, cx| {
                workspace.install(doc, None, None, None, spec.name, window, cx);
                if let Some(editor) = &workspace.editor {
                    editor.update(cx, |editor, cx| {
                        if editor.draw_mode != (spec.kind == CanvasKind::Paint) {
                            editor.toggle_draw_mode(cx);
                        }
                    });
                }
            });
        });
        true
    }

    fn save_preset(&mut self, cx: &mut Context<Self>) {
        match self.draft(cx) {
            Ok(spec) => {
                let saved = &crate::app_state::settings(cx).canvas_presets;
                if saved.len() >= 100
                    && !saved
                        .iter()
                        .any(|old| old.name == spec.name && old.kind == spec.kind)
                {
                    self.notice = Some("You have 100 saved presets. Remove one in Saved, or use an existing preset name to replace it.".into());
                    cx.notify();
                    return;
                }
                crate::app_state::update_settings(cx, |settings| {
                    settings
                        .canvas_presets
                        .retain(|old| old.name != spec.name || old.kind != spec.kind);
                    settings.canvas_presets.insert(0, spec);
                });
                self.notice = Some("Preset saved locally using the document name.".into());
            }
            Err(error) => self.notice = Some(error),
        }
        cx.notify();
    }
}

fn field(label: &'static str, input: &Entity<InputState>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .flex_1()
        .min_w_0()
        .child(label)
        .child(
            div()
                .id(SharedString::from(format!("new-canvas-field-{label}")))
                .test_support()
                .child(Input::new(input).small()),
        )
}

impl Render for NewCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let draft = self.draft(cx);
        let valid = draft.is_ok();
        let size = draft.as_ref().ok().and_then(|s| s.pixel_size().ok());
        let (preview_w, preview_h) = size
            .map(|(w, h)| {
                let scale = 72. / (w.max(h) as f32);
                (w as f32 * scale, h as f32 * scale)
            })
            .unwrap_or((72., 48.));
        let catalog = presets(self.spec.kind);
        let mut categories = Vec::new();
        for preset in catalog {
            if !categories.contains(&preset.category) {
                categories.push(preset.category);
            }
        }
        let settings = crate::app_state::settings(cx);
        let radius = settings.corners.radius();
        let saved = settings
            .canvas_presets
            .iter()
            .filter(|s| s.kind == self.spec.kind)
            .cloned()
            .collect::<Vec<_>>();
        let recent = settings
            .recent_canvases
            .iter()
            .filter(|s| s.kind == self.spec.kind)
            .take(4)
            .cloned()
            .collect::<Vec<_>>();
        if !saved.is_empty() {
            categories.push("Saved");
        }
        let message = self.notice.clone().or_else(|| draft.as_ref().err().cloned()).unwrap_or_else(|| {
            let bytes = draft.as_ref().unwrap().layer_bytes().unwrap_or(0);
            format!("{:.1} MiB per full RGBA16 layer. Masks, history and render caches use additional memory.", bytes as f64 / 1_048_576.)
        });
        let preview_color = self.spec.background.rgba().unwrap_or([210, 210, 210, 255]);
        let [r, g, b, _] = preview_color;
        let preview_color = rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
        div()
            .id("new-canvas-form")
            .test_support()
            .flex()
            .flex_col()
            .gap_4()
            .text_size(px(12.))
            .text_color(p.ink)
            .child(
                div()
                    .id("new-canvas-scroll")
                    .max_h((window.viewport_size().height - px(230.)).max(px(150.)))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_4()
                            .child(
                                div()
                                    .w(px(130.))
                                    .flex_none()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(div().text_color(p.muted).child("Document type"))
                                    .children(CanvasKind::ALL.map(|kind| {
                                        Button::new(("new-canvas-kind", kind as usize))
                                            .label(kind.label())
                                            .small()
                                            .ghost()
                                            .selected(self.spec.kind == kind)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.pick_kind(kind, window, cx)
                                            }))
                                    }))
                                    .child(div().mt_3().text_color(p.muted).child("Recent sizes"))
                                    .children(recent.into_iter().enumerate().map(
                                        |(index, spec)| {
                                            Button::new(("new-canvas-recent", index))
                                                .label(spec.name.clone())
                                                .small()
                                                .ghost()
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.show_spec(spec.clone(), window, cx)
                                                    },
                                                ))
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(220.))
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(div().flex().flex_wrap().gap_1().children(
                                        categories.into_iter().enumerate().map(
                                            |(index, category)| {
                                                Button::new(("new-canvas-category", index))
                                                    .label(category)
                                                    .xsmall()
                                                    .ghost()
                                                    .selected(self.category == category)
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.category = category.into();
                                                        cx.notify();
                                                    }))
                                            },
                                        ),
                                    ))
                                    .child(
                                        div().flex().flex_wrap().gap_2().children(
                                            catalog
                                                .iter()
                                                .enumerate()
                                                .filter(|(_, preset)| {
                                                    preset.category == self.category
                                                })
                                                .map(|(index, preset)| {
                                                    let selected = draft.as_ref().is_ok_and(|s| {
                                                        s.width == preset.width
                                                            && s.height == preset.height
                                                            && s.unit == preset.unit
                                                            && s.resolution == preset.resolution
                                                    });
                                                    Button::new(("new-canvas-preset", index))
                                                        .label(format!(
                                                            "{}\n{} × {} {}",
                                                            preset.name,
                                                            preset.width,
                                                            preset.height,
                                                            preset.unit.label()
                                                        ))
                                                        .h(px(76.))
                                                        .w(px(132.))
                                                        .small()
                                                        .outline()
                                                        .selected(selected)
                                                        .on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                let mut spec =
                                                                    this.draft(cx).unwrap_or_else(
                                                                        |_| this.spec.clone(),
                                                                    );
                                                                // Keep a typed name even if a size field is temporarily invalid.
                                                                spec.name = this.fields[0]
                                                                    .read(cx)
                                                                    .value()
                                                                    .to_string();
                                                                preset.apply(&mut spec);
                                                                this.show_spec(spec, window, cx);
                                                            },
                                                        ))
                                                }),
                                        ),
                                    )
                                    .when(self.category == "Saved", |panel| {
                                        panel.children(saved.into_iter().enumerate().map(
                                            |(index, spec)| {
                                                let remove_name = spec.name.clone();
                                                let remove_kind = spec.kind;
                                                div().flex().gap_1().child(Button::new(("new-canvas-saved", index))
                                                    .label(spec.name.clone())
                                                    .small()
                                                    .outline()
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.show_spec(spec.clone(), window, cx)
                                                        },
                                                    )))
                                                    .child(Button::new(("new-canvas-remove-preset", index))
                                                        .label("×").tooltip("Remove saved preset").small().ghost()
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            crate::app_state::update_settings(cx, |settings| {
                                                                settings.canvas_presets.retain(|old| old.name != remove_name || old.kind != remove_kind);
                                                            });
                                                            if !crate::app_state::settings(cx).canvas_presets.iter().any(|s| s.kind == this.spec.kind) {
                                                                this.category = presets(this.spec.kind)[0].category.into();
                                                            }
                                                            cx.notify();
                                                        })))
                                            },
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .w(px(260.))
                                    .flex_none()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .h(px(88.))
                                            .rounded(px(radius))
                                            .bg(p.soft_bg)
                                            .child(
                                                div()
                                                    .w(px(preview_w))
                                                    .h(px(preview_h))
                                                    .bg(preview_color)
                                                    .border_1()
                                                    .border_color(p.line),
                                            ),
                                    )
                                    .child(field("Name", &self.fields[0]))
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .child(field("Width", &self.fields[1]))
                                            .child(field("Height", &self.fields[2])),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap_1()
                                            .children(Unit::ALL.map(|unit| {
                                                Button::new(("new-canvas-unit", unit as usize))
                                                    .label(unit.label())
                                                    .xsmall()
                                                    .ghost()
                                                    .selected(self.spec.unit == unit)
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            match this.draft(cx).and_then(
                                                                |mut s| {
                                                                    s.convert_unit(unit).map(|_| s)
                                                                },
                                                            ) {
                                                                Ok(spec) => {
                                                                    this.show_spec(spec, window, cx)
                                                                }
                                                                Err(error) => {
                                                                    this.notice = Some(error);
                                                                    cx.notify();
                                                                }
                                                            }
                                                        },
                                                    ))
                                            }))
                                            .child(
                                                Button::new("new-canvas-orientation")
                                                    .label("Swap ↔")
                                                    .xsmall()
                                                    .ghost()
                                                    .tooltip("Swap width and height")
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            let w = this.fields[1]
                                                                .read(cx)
                                                                .value()
                                                                .to_string();
                                                            let h = this.fields[2]
                                                                .read(cx)
                                                                .value()
                                                                .to_string();
                                                            this.fields[1].update(
                                                                cx,
                                                                |input, cx| {
                                                                    input.set_value(h, window, cx)
                                                                },
                                                            );
                                                            this.fields[2].update(
                                                                cx,
                                                                |input, cx| {
                                                                    input.set_value(w, window, cx)
                                                                },
                                                            );
                                                            cx.notify();
                                                        },
                                                    )),
                                            ),
                                    )
                                    .child(field("Resolution · ppi", &self.fields[3]))
                                    .child(div().flex().gap_1().children([8, 16].map(|depth| {
                                        Button::new(("new-canvas-depth", depth as usize))
                                            .label(format!("RGB · {depth}-bit"))
                                            .xsmall()
                                            .ghost()
                                            .selected(self.spec.depth == depth)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.spec.depth = depth;
                                                cx.notify();
                                            }))
                                    })))
                                    .child(div().text_color(p.muted).child("Background"))
                                    .child(div().flex().flex_wrap().gap_1().children(
                                        Background::ALL.map(|background| {
                                            Button::new((
                                                "new-canvas-background",
                                                background as usize,
                                            ))
                                            .label(background.label())
                                            .xsmall()
                                            .ghost()
                                            .selected(self.spec.background == background)
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    this.spec.background = background;
                                                    cx.notify();
                                                }),
                                            )
                                        }),
                                    )),
                            ),
                    ),
            )
            .child(
                div()
                    .id("new-canvas-message")
                    .test_support()
                    .text_color(p.muted)
                    .child(message),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("new-canvas-save-preset")
                            .label("Save preset")
                            .small()
                            .ghost()
                            .disabled(!valid)
                            .on_click(cx.listener(|this, _, _, cx| this.save_preset(cx))),
                    )
                    .child(
                        div().flex_1().text_color(p.muted).child(
                            size.map(|(w, h)| format!("{w} × {h} px"))
                                .unwrap_or_default(),
                        ),
                    )
                    .child(
                        Button::new("new-canvas-cancel")
                            .label("Cancel")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("new-canvas-create")
                            .label("Create")
                            .small()
                            .primary()
                            .disabled(!valid)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.submit(window, cx) {
                                    window.close_dialog(cx);
                                }
                            })),
                    ),
            )
    }
}

impl Workspace {
    pub(super) fn open_new_canvas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_style_dialog(window, cx);
        let workspace = cx.weak_entity();
        let view = cx.new(|cx| NewCanvas::new(workspace, window, cx));
        window.open_dialog(cx, move |dialog, window, _| {
            let submit = view.clone();
            dialog
                .title("New document")
                .width(px(880.).min(window.viewport_size().width - px(32.)))
                .overlay_closable(false)
                .footer(div())
                .child(view.clone())
                .on_ok(move |_, window, cx| submit.update(cx, |view, cx| view.submit(window, cx)))
        });
    }
}
