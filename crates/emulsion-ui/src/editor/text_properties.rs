//! Character and paragraph controls share the selected canvas text range.
use super::*;
use emulsion_core::text::{Align, AntiAliasMode};
use emulsion_core::text_effects::{TextPath, TextPathMode, WarpStyle};
use gpui_kit::component::button::Button;
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
            "tracking" => (-50., 500.),
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
        let mut panel=div().id("text-properties").flex().flex_col().gap_2().p_3().border_b_1().border_color(cx.theme().border)
            .child(div().text_sm().child("Character"))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(if self.text_style_range().is_some(){"Selected characters"}else{"Entire text layer"}))
            .child(self.text_field("size","Size (px)"))
            .child(self.text_field("tracking","Letter spacing (px)"))
            .child(self.text_field("baseline","Baseline shift (px)"))
            .child(self.text_choice("antialias","Anti-alias",match spec.anti_alias{AntiAliasMode::Smooth=>0,AntiAliasMode::Crisp=>1,AntiAliasMode::Strong=>2,AntiAliasMode::None=>3},&["Smooth","Crisp","Strong","None"],cx))
            .child(div().mt_2().text_sm().child("Paragraph"))
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
