//! Character and paragraph controls share the selected canvas text range.
use super::*;
use emulsion_core::text::{Align, AntiAliasMode};
use emulsion_core::text_effects::{TextPath, TextPathMode, WarpStyle};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable};

pub(super) struct TextFields {
    target: Option<NodeId>,
    inputs: HashMap<&'static str, Entity<InputState>>,
    _subs: Vec<Subscription>,
}
fn number(value: f32) -> String {
    format!("{value:.2}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

impl EditorView {
    pub(super) fn design_text_size_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.text_fields_sync(window, cx);
        let input = self.type_tool.properties.as_ref().unwrap().inputs["size"].clone();
        let focus = input.read(cx).focus_handle(cx);
        let disabled = self.is_design()
            && self.text_target().is_some_and(|(id, _)| {
                self.editor.doc.locked_ancestor(id).is_some()
                    || self.editor.doc.layer_locks(id).pixels
            });
        div()
            .id("design-text-size")
            .test_support()
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if !disabled {
                    window.focus(&focus, cx);
                }
                cx.stop_propagation();
            })
            .w(px(46.))
            .child(
                Styled::h(
                    Input::new(&input)
                        .id("design-text-size-input")
                        .small()
                        .disabled(disabled),
                    px(24.),
                )
                .text_size(px(11.)),
            )
            .into_any_element()
    }
    fn text_fields_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.text_target().map(|(id, _)| id);
        let spec = self
            .text_target()
            .map(|(_, s)| (*s).clone())
            .unwrap_or_else(|| self.type_tool.spec.clone());
        let style = spec.style_at(self.text_style_range().map_or(0, |r| r.start));
        let values = [
            ("size", number(style.size)),
            ("leading", number(spec.line_height)),
            ("tracking", number(style.letter_spacing)),
            ("baseline", number(style.baseline)),
            ("width", spec.width.map(number).unwrap_or_default()),
            ("height", spec.height.map(number).unwrap_or_default()),
            ("bend", number(spec.warp.bend)),
            ("warp-horizontal", number(spec.warp.horizontal)),
            ("warp-vertical", number(spec.warp.vertical)),
            (
                "path-offset",
                number(spec.text_path.as_ref().map_or(0., |p| p.offset)),
            ),
            (
                "path-inset",
                number(spec.text_path.as_ref().map_or(0., |p| p.inset)),
            ),
        ];
        if self
            .type_tool
            .properties
            .as_ref()
            .is_none_or(|f| f.target != target)
        {
            let mut inputs = HashMap::new();
            let mut subs = Vec::new();
            for (key, value) in &values {
                let key = *key;
                let input = cx.new(|cx| InputState::new(window, cx).default_value(value.clone()));
                subs.push(cx.subscribe_in(
                    &input,
                    window,
                    move |this, input, event: &InputEvent, _, cx| {
                        if this.text_target().map(|(id, _)| id) == target
                            && matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur)
                        {
                            this.apply_text_field(key, input.read(cx).value().as_ref(), cx);
                        }
                    },
                ));
                inputs.insert(key, input);
            }
            self.type_tool.properties = Some(TextFields {
                target,
                inputs,
                _subs: subs,
            });
        }
        if let Some(fields) = &self.type_tool.properties {
            for (key, value) in values {
                let input = &fields.inputs[key];
                if !input.read(cx).focus_handle(cx).is_focused(window)
                    && input.read(cx).value().as_ref() != value
                {
                    input.update(cx, |input, cx| input.set_value(value, window, cx));
                }
            }
        }
    }
    fn apply_text_field(&mut self, key: &str, value: &str, cx: &mut Context<Self>) {
        // A canvas text session owns a long-lived transaction. End it before a
        // Properties edit so each committed field is independently undoable;
        // close_text_field retains the selected character range.
        self.close_text_field(cx);
        if matches!(key, "width" | "height") && value.trim().is_empty() {
            self.restyle_text(
                |s| {
                    if key == "width" {
                        s.width = None
                    } else {
                        s.height = None
                    }
                },
                cx,
            );
            return;
        }
        let limits = match key {
            "size" => (1., 4000.),
            "leading" => (0.5, 4.),
            "tracking" => (-4000., 4000.),
            "width" | "height" => (1., 30000.),
            "bend" | "warp-horizontal" | "warp-vertical" => (-100., 100.),
            _ => (-30000., 30000.),
        };
        let Some(value) = value
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite() && *v >= limits.0 && *v <= limits.1)
        else {
            self.type_tool.error = Some(
                t!(
                    "editor.text_properties.enter_range",
                    min = limits.0,
                    max = limits.1
                )
                .into_owned(),
            );
            cx.notify();
            return;
        };
        self.type_tool.error = None;
        if key == "baseline" {
            if let Some((id, spec)) = self.text_target() {
                let mut updated = (*spec).clone();
                updated.apply_style(self.text_style_range().unwrap_or(0..spec.text.len()), |s| {
                    s.baseline = value
                });
                if updated != *spec {
                    self.execute(
                        Command::SetText {
                            id,
                            spec: Box::new(updated),
                        },
                        cx,
                    );
                }
            }
            return;
        }
        self.restyle_text(
            |s| match key {
                "size" => s.size = value,
                "leading" => s.line_height = value,
                "tracking" => s.letter_spacing = value,
                "width" => s.width = Some(value),
                "height" => s.height = Some(value),
                "bend" => s.warp.bend = value,
                "warp-horizontal" => s.warp.horizontal = value,
                "warp-vertical" => s.warp.vertical = value,
                "path-offset" => {
                    if let Some(p) = &mut s.text_path {
                        p.offset = value;
                    }
                }
                "path-inset" => {
                    if let Some(p) = &mut s.text_path {
                        p.inset = value;
                    }
                }
                _ => {}
            },
            cx,
        );
    }
    fn text_field(&self, key: &'static str, title: &str) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().text_xs().child(title.to_string()))
            .child(
                div()
                    .id(SharedString::from(format!("text-{key}")))
                    .w_24()
                    .test_support()
                    .child(
                        Input::new(&self.type_tool.properties.as_ref().unwrap().inputs[key])
                            .small(),
                    ),
            )
            .into_any_element()
    }
    fn text_choice(
        &self,
        key: &'static str,
        title: &str,
        current: usize,
        labels: &[std::borrow::Cow<'static, str>],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editor = cx.entity().downgrade();
        let labels: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
        let caption = format!(
            "{title}: {}",
            labels.get(current).map_or("", String::as_str)
        );
        div()
            .id(SharedString::from(format!("text-{key}")))
            .test_support()
            .child(
                Button::new(SharedString::from(format!("text-{key}-button")))
                    .small()
                    .label(caption)
                    .dropdown_menu(move |mut menu, _, _| {
                        for (index, label) in labels.iter().enumerate() {
                            let weak = editor.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label.clone())
                                    .checked(index == current)
                                    .on_click(move |_, _, cx| {
                                        if let Some(editor) = weak.upgrade() {
                                            editor.update(cx, |this, cx| {
                                                this.choose_text_option(key, index, cx)
                                            });
                                        }
                                    }),
                            );
                        }
                        menu
                    }),
            )
            .into_any_element()
    }
    fn choose_text_option(&mut self, key: &str, index: usize, cx: &mut Context<Self>) {
        if key == "path-mode"
            && index == 1
            && self.text_target().is_some_and(|(_, spec)| {
                spec.text_path.as_ref().is_some_and(|path| {
                    !path
                        .path
                        .subpaths
                        .iter()
                        .any(|sub| sub.closed && sub.anchors.len() >= 3)
                })
            })
        {
            self.type_tool.error = Some(t!("editor.text_properties.closed_path").into());
            cx.notify();
            return;
        }
        self.type_tool.error = None;
        self.restyle_text(
            |s| match key {
                "align" => {
                    s.align = [Align::Left, Align::Center, Align::Right, Align::Justify][index]
                }
                "orientation" => s.vertical = index == 1,
                "antialias" => {
                    s.anti_alias = [
                        AntiAliasMode::Smooth,
                        AntiAliasMode::Crisp,
                        AntiAliasMode::Strong,
                        AntiAliasMode::None,
                    ][index]
                }
                "warp" => {
                    s.warp.style = [
                        WarpStyle::None,
                        WarpStyle::Arc,
                        WarpStyle::Bulge,
                        WarpStyle::Flag,
                    ][index]
                }
                "path-mode" => {
                    if let Some(p) = &mut s.text_path {
                        p.mode = if index == 0 {
                            TextPathMode::Follow
                        } else {
                            TextPathMode::Inside
                        };
                    }
                }
                _ => {}
            },
            cx,
        );
    }
    fn text_path_choice(&self, cx: &mut Context<Self>) -> AnyElement {
        let paths: Vec<_> = self
            .editor
            .doc
            .nodes
            .iter()
            .filter_map(|n| {
                if let NodeKind::Path { path, .. } = &n.kind {
                    Some((n.name.clone(), path.clone()))
                } else {
                    None
                }
            })
            .collect();
        let editor = cx.entity().downgrade();
        div()
            .id("text-attach-path")
            .test_support()
            .child(
                Button::new("text-attach-path-button")
                    .small()
                    .label(t!("editor.text_properties.attach_path"))
                    .dropdown_menu(move |mut menu, _, _| {
                        if paths.is_empty() {
                            return menu.item(
                                PopupMenuItem::new(t!("editor.text_properties.draw_path_first"))
                                    .disabled(true),
                            );
                        }
                        for (name, path) in &paths {
                            let weak = editor.clone();
                            let path = path.clone();
                            menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                                move |_, _, cx| {
                                    if let Some(editor) = weak.upgrade() {
                                        editor.update(cx, |this, cx| {
                                            this.restyle_text(
                                                |s| {
                                                    let mut local = (*path).clone();
                                                    let inverse = s.transform().inverse();
                                                    local.transform(inverse);
                                                    s.text_path = Some(TextPath {
                                                        path: local,
                                                        mode: TextPathMode::Follow,
                                                        offset: 0.,
                                                        flip: false,
                                                        inset: 0.,
                                                    });
                                                },
                                                cx,
                                            );
                                        });
                                    }
                                },
                            ));
                        }
                        menu
                    }),
            )
            .into_any_element()
    }
    fn photo_text_field(&self, key: &'static str, title: &str, cx: &App) -> AnyElement {
        let input = &self.type_tool.properties.as_ref().unwrap().inputs[key];
        let focus = input.read(cx).focus_handle(cx);
        div()
            .id(SharedString::from(format!("text-{key}")))
            .test_support()
            .min_w_0()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx)
            })
            .child(
                Input::new(input)
                    .aria_label(title.to_string())
                    .small()
                    .h(px(26.))
                    .prefix(div().text_size(px(11.)).child(title.to_string()))
                    .font_family(MONO_FONT)
                    .text_size(px(11.))
                    .text_align(TextAlign::Right),
            )
            .into_any_element()
    }

    fn photo_character(
        &mut self,
        spec: &emulsion_core::text::TextSpec,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::palette(cx);
        let style = spec.style_at(self.text_style_range().map_or(0, |r| r.start));
        let font_bounds = TrackBounds::default();
        let anchor = font_bounds.clone();
        let locked = self
            .text_target()
            .is_some_and(|(id, _)| self.editor.doc.locked_ancestor(id).is_some());
        let font = Button::new("photo-character-font")
            .label(self.font_label(&style.font))
            .small()
            .outline()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .justify_start()
            .relative()
            .disabled(locked)
            .tooltip(t!("design.direct.font_search"))
            .child(
                canvas(
                    move |bounds, _, _| font_bounds.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.toggle_font_picker(anchor.get(), window, cx)
            }));
        let fields = div()
            .grid()
            .grid_cols(2)
            .gap_1()
            .child(self.photo_text_field("size", &t!("editor.text_properties.size"), cx))
            .child(self.photo_text_field("leading", &t!("editor.text_properties.leading"), cx))
            .child(self.photo_text_field("tracking", &t!("editor.text_properties.tracking"), cx))
            .child(self.photo_text_field("baseline", &t!("editor.text_properties.baseline"), cx));
        let styles = div().flex().gap_1().children(
            [
                (0, t!("editor.text_properties.bold"), "bold", style.bold),
                (
                    1,
                    t!("editor.text_properties.italic"),
                    "italic",
                    style.italic,
                ),
                (
                    2,
                    t!("editor.text_properties.underline"),
                    "underline",
                    style.underline,
                ),
                (
                    3,
                    t!("editor.text_properties.strikethrough"),
                    "strikethrough",
                    style.strikethrough,
                ),
            ]
            .into_iter()
            .map(|(i, title, icon, on)| {
                Button::new(match i {
                    0 => "photo-text-bold",
                    1 => "photo-text-italic",
                    2 => "text-underline",
                    _ => "text-strikethrough",
                })
                .accessibility_label(title.clone())
                .tooltip(title)
                .small()
                .outline()
                .flex_1()
                .when(on, |b| b.bg(p.accent.opacity(0.18)).text_color(p.accent))
                .child(rail::tool_icon(icon).size(px(13.)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.close_text_field(cx);
                    this.restyle_text(
                        |spec| match i {
                            0 => spec.bold = !on,
                            1 => spec.italic = !on,
                            2 => spec.underline = !on,
                            _ => spec.strikethrough = !on,
                        },
                        cx,
                    );
                }))
            }),
        );
        let paragraph = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div().flex().gap_1().children(
                    [
                        (
                            Align::Left,
                            t!("editor.text_properties.align_left"),
                            "align-left",
                        ),
                        (
                            Align::Center,
                            t!("editor.text_properties.align_center"),
                            "align-center",
                        ),
                        (
                            Align::Right,
                            t!("editor.text_properties.align_right"),
                            "align-right",
                        ),
                        (
                            Align::Justify,
                            t!("editor.text_properties.align_justify"),
                            "align-justify",
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (align, title, icon))| {
                        Button::new(("photo-text-align", i))
                            .accessibility_label(title.clone())
                            .tooltip(title)
                            .small()
                            .outline()
                            .flex_1()
                            .when(spec.align == align, |b| b.bg(p.soft_bg))
                            .child(rail::tool_icon(icon).size(px(13.)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.close_text_field(cx);
                                this.restyle_text(|s| s.align = align, cx);
                            }))
                    }),
                ),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_1()
                    .child(self.photo_text_field("width", &t!("editor.text_properties.width"), cx))
                    .child(self.photo_text_field(
                        "height",
                        &t!("editor.text_properties.height"),
                        cx,
                    )),
            )
            .child(
                Button::new("text-paragraph-format")
                    .label(t!("editor.text_properties.lists_spacing"))
                    .small()
                    .outline()
                    .w_full()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.show_paragraph_format(window, cx)),
                    ),
            )
            .child(
                div().flex().gap_1().children(
                    [
                        (
                            "text-list-bullet",
                            t!("editor.text_properties.bullets"),
                            emulsion_core::text::ListStyle::Bullet,
                        ),
                        (
                            "text-list-numbered",
                            t!("editor.text_properties.numbered"),
                            emulsion_core::text::ListStyle::Numbered,
                        ),
                        (
                            "text-list-none",
                            t!("editor.text_properties.no_list"),
                            emulsion_core::text::ListStyle::None,
                        ),
                    ]
                    .into_iter()
                    .map(|(id, label, style)| {
                        Button::new(id).label(label).small().ghost().on_click(
                            cx.listener(move |this, _, _, cx| this.set_text_list(style, cx)),
                        )
                    }),
                ),
            )
            .into_any_element();
        let mut options = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.text_choice(
                "orientation",
                &t!("editor.text_properties.orientation"),
                usize::from(spec.vertical),
                &[
                    t!("editor.text_properties.horizontal"),
                    t!("editor.text_properties.vertical"),
                ],
                cx,
            ))
            .child(self.text_choice(
                "warp",
                &t!("editor.text_properties.warp"),
                match spec.warp.style {
                    WarpStyle::None => 0,
                    WarpStyle::Arc => 1,
                    WarpStyle::Bulge => 2,
                    WarpStyle::Flag => 3,
                },
                &[
                    t!("editor.text_properties.none"),
                    t!("editor.text_properties.warp_arc"),
                    t!("editor.text_properties.warp_bulge"),
                    t!("editor.text_properties.warp_flag"),
                ],
                cx,
            ));
        if spec.warp.style != WarpStyle::None {
            options = options
                .child(self.text_field("bend", &t!("editor.text_properties.bend")))
                .child(self.text_field(
                    "warp-horizontal",
                    &t!("editor.text_properties.warp_horizontal"),
                ))
                .child(
                    self.text_field("warp-vertical", &t!("editor.text_properties.warp_vertical")),
                );
        }
        options = options.child(self.text_path_choice(cx));
        if let Some(path) = &spec.text_path {
            options = options
                .child(self.text_choice(
                    "path-mode",
                    &t!("editor.text_properties.layout"),
                    usize::from(path.mode == TextPathMode::Inside),
                    &[
                        t!("editor.text_properties.along_path"),
                        t!("editor.text_properties.inside_path"),
                    ],
                    cx,
                ))
                .child(
                    Button::new("text-path-flip")
                        .label(if path.flip {
                            t!("editor.text_properties.flip_back")
                        } else {
                            t!("editor.text_properties.flip_across")
                        })
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.restyle_text(
                                |s| {
                                    if let Some(path) = &mut s.text_path {
                                        path.flip = !path.flip;
                                    }
                                },
                                cx,
                            )
                        })),
                )
                .child(self.text_field("path-offset", &t!("editor.text_properties.path_offset")))
                .child(self.text_field("path-inset", &t!("editor.text_properties.inset")))
                .child(
                    Button::new("text-detach-path")
                        .label(t!("editor.text_properties.detach"))
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.restyle_text(|s| s.text_path = None, cx)
                        })),
                );
        }
        div()
            .id("text-properties")
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(mono(
                if self.text_style_range().is_some() {
                    t!("editor.text_properties.selected_chars")
                } else {
                    t!("editor.text_properties.text_layer")
                },
                10.,
                p.muted,
            ))
            .child(font)
            .child(fields)
            .child(styles)
            .child(
                Button::new("photo-text-colour")
                    .small()
                    .ghost()
                    .justify_start()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(t!("editor.text_properties.color"))
                            .child(div().size(px(12.)).rounded(px(3.)).bg(rgb(((style.color[0]
                                as u32)
                                << 16)
                                | ((style.color[1] as u32) << 8)
                                | style.color[2] as u32)))
                            .child(mono(
                                format!(
                                    "#{:02X}{:02X}{:02X}",
                                    style.color[0], style.color[1], style.color[2]
                                ),
                                10.5,
                                p.ink,
                            )),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.open_text_colour(window, cx))),
            )
            .child(self.text_choice(
                "antialias",
                &t!("editor.text_properties.antialias"),
                match spec.anti_alias {
                    AntiAliasMode::Smooth => 0,
                    AntiAliasMode::Crisp => 1,
                    AntiAliasMode::Strong => 2,
                    AntiAliasMode::None => 3,
                },
                &[
                    t!("editor.text_properties.aa_smooth"),
                    t!("editor.text_properties.aa_crisp"),
                    t!("editor.text_properties.aa_strong"),
                    t!("editor.text_properties.none"),
                ],
                cx,
            ))
            .child(self.photo_section(
                "photo-paragraph",
                &t!("editor.text_properties.paragraph"),
                paragraph,
                &p,
                cx,
            ))
            .child(self.photo_section(
                "photo-type-options",
                &t!("editor.text_properties.type_options"),
                options.into_any_element(),
                &p,
                cx,
            ))
            .children(
                self.type_tool
                    .error
                    .as_ref()
                    .map(|error| div().text_xs().text_color(p.accent).child(error.clone())),
            )
            .into_any_element()
    }

    pub(super) fn text_properties(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.tool != Tool::Type && self.text_target().is_none() {
            return None;
        }
        self.text_fields_sync(window, cx);
        let spec = self
            .text_target()
            .map(|(_, s)| (*s).clone())
            .unwrap_or_else(|| self.type_tool.spec.clone());
        let style = spec.style_at(self.text_style_range().map_or(0, |r| r.start));
        if self.shared_panel_mode() {
            return Some(self.photo_character(&spec, window, cx));
        }
        let decorations = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                Button::new("text-underline")
                    .small()
                    .label(if style.underline {
                        t!("editor.text_properties.underline_on")
                    } else {
                        t!("editor.text_properties.underline")
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_text_field(cx);
                        this.restyle_text(|s| s.underline = !s.underline, cx);
                    })),
            )
            .child(
                Button::new("text-strikethrough")
                    .small()
                    .label(if style.strikethrough {
                        t!("editor.text_properties.strikethrough_on")
                    } else {
                        t!("editor.text_properties.strikethrough")
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_text_field(cx);
                        this.restyle_text(|s| s.strikethrough = !s.strikethrough, cx);
                    })),
            );
        let lists = div()
            .child(
                Button::new("text-paragraph-format")
                    .small()
                    .label(t!("editor.text_properties.lists_spacing_long"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.show_paragraph_format(window, cx)),
                    ),
            )
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                Button::new("text-list-bullet")
                    .small()
                    .label(t!("editor.text_properties.bullets"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_text_list(emulsion_core::text::ListStyle::Bullet, cx)
                    })),
            )
            .child(
                Button::new("text-list-numbered")
                    .small()
                    .label(t!("editor.text_properties.numbered"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_text_list(emulsion_core::text::ListStyle::Numbered, cx)
                    })),
            )
            .child(
                Button::new("text-list-none")
                    .small()
                    .label(t!("editor.text_properties.remove_list"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_text_list(emulsion_core::text::ListStyle::None, cx)
                    })),
            );
        let mut panel = div()
            .id("text-properties")
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_sm()
                    .child(t!("editor.text_properties.character")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.text_style_range().is_some() {
                        t!("editor.text_properties.selected_chars")
                    } else {
                        t!("editor.text_properties.entire_layer")
                    }),
            )
            .child(decorations)
            .child(self.text_field("size", &t!("editor.text_properties.size_px")))
            .child(self.text_field("tracking", &t!("editor.text_properties.letter_spacing")))
            .child(self.text_field("baseline", &t!("editor.text_properties.baseline_shift")))
            .child(self.text_choice(
                "antialias",
                &t!("editor.text_properties.antialias"),
                match spec.anti_alias {
                    AntiAliasMode::Smooth => 0,
                    AntiAliasMode::Crisp => 1,
                    AntiAliasMode::Strong => 2,
                    AntiAliasMode::None => 3,
                },
                &[
                    t!("editor.text_properties.aa_smooth"),
                    t!("editor.text_properties.aa_crisp"),
                    t!("editor.text_properties.aa_strong"),
                    t!("editor.text_properties.none"),
                ],
                cx,
            ))
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .child(t!("editor.text_properties.paragraph")),
            )
            .child(lists)
            .child(self.text_choice(
                "orientation",
                &t!("editor.text_properties.orientation"),
                usize::from(spec.vertical),
                &[
                    t!("editor.text_properties.horizontal"),
                    t!("editor.text_properties.vertical"),
                ],
                cx,
            ))
            .child(self.text_choice(
                "align",
                &t!("editor.text_properties.alignment"),
                match spec.align {
                    Align::Left => 0,
                    Align::Center => 1,
                    Align::Right => 2,
                    Align::Justify => 3,
                },
                &[
                    t!("editor.text_properties.align_left"),
                    t!("editor.text_properties.align_center"),
                    t!("editor.text_properties.align_right"),
                    t!("editor.text_properties.align_justify"),
                ],
                cx,
            ))
            .child(self.text_field("leading", &t!("editor.text_properties.line_height")))
            .child(self.text_field("width", &t!("editor.text_properties.frame_width")))
            .child(self.text_field("height", &t!("editor.text_properties.frame_height")))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("editor.text_properties.frame_hint")),
            )
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .child(t!("editor.text_properties.warp_text")),
            )
            .child(self.text_choice(
                "warp",
                &t!("editor.text_properties.warp_style"),
                match spec.warp.style {
                    WarpStyle::None => 0,
                    WarpStyle::Arc => 1,
                    WarpStyle::Bulge => 2,
                    WarpStyle::Flag => 3,
                },
                &[
                    t!("editor.text_properties.none"),
                    t!("editor.text_properties.warp_arc"),
                    t!("editor.text_properties.warp_bulge"),
                    t!("editor.text_properties.warp_flag"),
                ],
                cx,
            ));
        if spec.warp.style != WarpStyle::None {
            panel = panel
                .child(self.text_field("bend", &t!("editor.text_properties.bend")))
                .child(self.text_field(
                    "warp-horizontal",
                    &t!("editor.text_properties.horizontal_distortion"),
                ))
                .child(self.text_field(
                    "warp-vertical",
                    &t!("editor.text_properties.vertical_distortion"),
                ));
        }
        panel = panel
            .child(
                div()
                    .mt_2()
                    .text_sm()
                    .child(t!("editor.text_properties.path_text")),
            )
            .child(self.text_path_choice(cx));
        if let Some(path) = &spec.text_path {
            panel = panel
                .child(self.text_choice(
                    "path-mode",
                    &t!("editor.text_properties.layout"),
                    usize::from(path.mode == TextPathMode::Inside),
                    &[
                        t!("editor.text_properties.along_path"),
                        t!("editor.text_properties.inside_path"),
                    ],
                    cx,
                ))
                .child(self.text_field("path-offset", &t!("editor.text_properties.path_offset")))
                .child(self.text_field("path-inset", &t!("editor.text_properties.inset")))
                .child(
                    Button::new("text-path-flip")
                        .small()
                        .label(if path.flip {
                            t!("editor.text_properties.flip_back")
                        } else {
                            t!("editor.text_properties.flip_across")
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.restyle_text(
                                |s| {
                                    if let Some(p) = &mut s.text_path {
                                        p.flip = !p.flip
                                    }
                                },
                                cx,
                            )
                        })),
                )
                .child(
                    Button::new("text-detach-path")
                        .small()
                        .label(t!("editor.text_properties.detach_text"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.restyle_text(|s| s.text_path = None, cx)
                        })),
                );
        }
        if let Some(error) = &self.type_tool.error {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        Some(panel.into_any_element())
    }
}
