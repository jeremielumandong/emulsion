//! Character and paragraph controls share the selected canvas text range.
use super::*;
use emulsion_core::text::{Align, AntiAliasMode};
use emulsion_core::text_effects::{TextPath, TextPathMode, WarpStyle};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable};

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
        div()
            .id("design-text-size")
            .test_support()
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx);
                cx.stop_propagation();
            })
            .w(px(46.))
            .child(
                Styled::h(
                    Input::new(&input).id("design-text-size-input").small(),
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
            self.type_tool.error =
                Some(format!("Enter a value from {} to {}.", limits.0, limits.1));
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
        labels: &[&str],
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
            self.type_tool.error = Some("Inside text requires a closed path.".into());
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
                    .label("Attach to path")
                    .dropdown_menu(move |mut menu, _, _| {
                        if paths.is_empty() {
                            return menu
                                .item(PopupMenuItem::new("Draw a path first").disabled(true));
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
        let owner = cx.weak_entity();
        let font = Button::new("photo-character-font")
            .label(if style.font.is_empty() {
                "Default font".into()
            } else {
                style.font.clone()
            })
            .small()
            .outline()
            .w_full()
            .justify_start()
            .dropdown_menu(move |mut menu, _, _| {
                for font in emulsion_core::text::font_families() {
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new(font.clone()).on_click(move |_, _, cx| {
                        owner
                            .update(cx, |this, cx| {
                                this.close_text_field(cx);
                                this.restyle_text(|spec| spec.font = font.clone(), cx);
                            })
                            .ok();
                    }));
                }
                menu
            });
        let fields = div()
            .grid()
            .grid_cols(2)
            .gap_1()
            .child(self.photo_text_field("size", "Size · px", cx))
            .child(self.photo_text_field("leading", "Leading ×", cx))
            .child(self.photo_text_field("tracking", "Tracking", cx))
            .child(self.photo_text_field("baseline", "Baseline", cx));
        let styles = div().flex().gap_1().children(
            [
                (0, "Bold", "bold", style.bold),
                (1, "Italic", "italic", style.italic),
                (2, "Underline", "underline", style.underline),
                (3, "Strikethrough", "strikethrough", style.strikethrough),
            ]
            .into_iter()
            .map(|(i, title, icon, on)| {
                Button::new(match i {
                    0 => "photo-text-bold",
                    1 => "photo-text-italic",
                    2 => "text-underline",
                    _ => "text-strikethrough",
                })
                .accessibility_label(title)
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
                        (Align::Left, "Left", "align-left"),
                        (Align::Center, "Center", "align-center"),
                        (Align::Right, "Right", "align-right"),
                        (Align::Justify, "Justify", "align-justify"),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (align, title, icon))| {
                        Button::new(("photo-text-align", i))
                            .accessibility_label(title)
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
                    .child(self.photo_text_field("width", "Width", cx))
                    .child(self.photo_text_field("height", "Height", cx)),
            )
            .child(
                Button::new("text-paragraph-format")
                    .label("Lists & paragraph spacing…")
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
                            "Bullets",
                            emulsion_core::text::ListStyle::Bullet,
                        ),
                        (
                            "text-list-numbered",
                            "Numbered",
                            emulsion_core::text::ListStyle::Numbered,
                        ),
                        (
                            "text-list-none",
                            "No list",
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
                "Orientation",
                usize::from(spec.vertical),
                &["Horizontal", "Vertical"],
                cx,
            ))
            .child(self.text_choice(
                "warp",
                "Warp",
                match spec.warp.style {
                    WarpStyle::None => 0,
                    WarpStyle::Arc => 1,
                    WarpStyle::Bulge => 2,
                    WarpStyle::Flag => 3,
                },
                &["None", "Arc", "Bulge", "Flag"],
                cx,
            ));
        if spec.warp.style != WarpStyle::None {
            options = options
                .child(self.text_field("bend", "Bend (%)"))
                .child(self.text_field("warp-horizontal", "Horizontal (%)"))
                .child(self.text_field("warp-vertical", "Vertical (%)"));
        }
        options = options.child(self.text_path_choice(cx));
        if let Some(path) = &spec.text_path {
            options = options
                .child(self.text_choice(
                    "path-mode",
                    "Layout",
                    usize::from(path.mode == TextPathMode::Inside),
                    &["Along path", "Inside path"],
                    cx,
                ))
                .child(
                    Button::new("text-path-flip")
                        .label(if path.flip {
                            "Flip back"
                        } else {
                            "Flip across path"
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
                .child(self.text_field("path-offset", "Path offset (px)"))
                .child(self.text_field("path-inset", "Inset (px)"))
                .child(
                    Button::new("text-detach-path")
                        .label("Detach from path")
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
                    "Selected characters"
                } else {
                    "Text layer"
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
                            .child("Color")
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
                "Anti-alias",
                match spec.anti_alias {
                    AntiAliasMode::Smooth => 0,
                    AntiAliasMode::Crisp => 1,
                    AntiAliasMode::Strong => 2,
                    AntiAliasMode::None => 3,
                },
                &["Smooth", "Crisp", "Strong", "None"],
                cx,
            ))
            .child(self.photo_section("photo-paragraph", "Paragraph", paragraph, &p, cx))
            .child(self.photo_section(
                "photo-type-options",
                "Type options",
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
                        "Underline ✓"
                    } else {
                        "Underline"
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
                        "Strikethrough ✓"
                    } else {
                        "Strikethrough"
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
                    .label("Lists and paragraph spacing…")
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
                    .label("Bullets")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_text_list(emulsion_core::text::ListStyle::Bullet, cx)
                    })),
            )
            .child(
                Button::new("text-list-numbered")
                    .small()
                    .label("Numbered")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_text_list(emulsion_core::text::ListStyle::Numbered, cx)
                    })),
            )
            .child(
                Button::new("text-list-none")
                    .small()
                    .label("Remove list")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_text_list(emulsion_core::text::ListStyle::None, cx)
                    })),
            );
        let mut panel=div().id("text-properties").flex().flex_col().gap_2().p_3().border_b_1().border_color(cx.theme().border)
            .child(div().text_sm().child("Character"))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(if self.text_style_range().is_some(){"Selected characters"}else{"Entire text layer"}))
            .child(decorations)
            .child(self.text_field("size","Size (px)"))
            .child(self.text_field("tracking","Letter spacing (px)"))
            .child(self.text_field("baseline","Baseline shift (px)"))
            .child(self.text_choice("antialias","Anti-alias",match spec.anti_alias{AntiAliasMode::Smooth=>0,AntiAliasMode::Crisp=>1,AntiAliasMode::Strong=>2,AntiAliasMode::None=>3},&["Smooth","Crisp","Strong","None"],cx))
            .child(div().mt_2().text_sm().child("Paragraph"))
            .child(lists)
            .child(self.text_choice("orientation","Orientation",usize::from(spec.vertical),&["Horizontal","Vertical"],cx))
            .child(self.text_choice("align","Alignment",match spec.align{Align::Left=>0,Align::Center=>1,Align::Right=>2,Align::Justify=>3},&["Left","Center","Right","Justify"],cx))
            .child(self.text_field("leading","Line height (multiple)"))
            .child(self.text_field("width","Frame width (px)"))
            .child(self.text_field("height","Frame height (px)"))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Drag on empty canvas to draw a frame. Drag its lower-right handle to resize."))
            .child(div().mt_2().text_sm().child("Warp text"))
            .child(self.text_choice("warp","Style",match spec.warp.style{WarpStyle::None=>0,WarpStyle::Arc=>1,WarpStyle::Bulge=>2,WarpStyle::Flag=>3},&["None","Arc","Bulge","Flag"],cx));
        if spec.warp.style != WarpStyle::None {
            panel = panel
                .child(self.text_field("bend", "Bend (%)"))
                .child(self.text_field("warp-horizontal", "Horizontal distortion (%)"))
                .child(self.text_field("warp-vertical", "Vertical distortion (%)"));
        }
        panel = panel
            .child(div().mt_2().text_sm().child("Path text"))
            .child(self.text_path_choice(cx));
        if let Some(path) = &spec.text_path {
            panel = panel
                .child(self.text_choice(
                    "path-mode",
                    "Layout",
                    usize::from(path.mode == TextPathMode::Inside),
                    &["Along path", "Inside path"],
                    cx,
                ))
                .child(self.text_field("path-offset", "Path offset (px)"))
                .child(self.text_field("path-inset", "Inset (px)"))
                .child(
                    Button::new("text-path-flip")
                        .small()
                        .label(if path.flip {
                            "Flip back"
                        } else {
                            "Flip across path"
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
                        .label("Detach text from path")
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
