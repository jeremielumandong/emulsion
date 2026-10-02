//! The Storyboard panel inspector: the active panel's name, timing, shot
//! data, tag, lock and captions, with rich caption formatting and the
//! caption field manager. The panel's layers stay in the Layers dock below.
//!
//! Every change lands through `ProjectEditor::edit_storyboard` (the name
//! through `rename_page`) as one Undo step. Text commits on Enter, or when
//! its field loses focus, never per keystroke.
//!
//! Text inputs edit plain text, so a caption's formatting shows in a styled
//! preview under its input; formatting applies to the input's selection.
use super::*;
use emulsion_core::project::PageId;
use emulsion_core::storyboard::{Caption, CaptionId, Panel, Storyboard};
use emulsion_core::text::TextStyle;
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
    input::{Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::ops::Range;

pub(crate) use emulsion_core::storyboard::{
    CAMERA_ANGLES, PANEL_STATUSES, SHOT_SIZES, TAG_PALETTE,
};

/// Commits a text field's value.
type Commit = Box<dyn Fn(&mut EditorView, String, &mut Context<EditorView>)>;
/// One choice of a panel menu.
type PanelEdit = Box<dyn Fn(&mut Panel)>;

/// Character formatting the inspector toggles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaptionFormat {
    Bold,
    Italic,
    Underline,
    Strikethrough,
}

impl CaptionFormat {
    const ALL: [(Self, &'static str, &'static str); 4] = [
        (Self::Bold, "B", "Bold"),
        (Self::Italic, "I", "Italic"),
        (Self::Underline, "U", "Underline"),
        (Self::Strikethrough, "S", "Strikethrough"),
    ];
    fn flag(self, style: &mut TextStyle) -> &mut bool {
        match self {
            Self::Bold => &mut style.bold,
            Self::Italic => &mut style.italic,
            Self::Underline => &mut style.underline,
            Self::Strikethrough => &mut style.strikethrough,
        }
    }
}

#[derive(Default)]
pub(crate) struct StoryboardUi {
    inspector: Option<Inspector>,
    fields_open: bool,
    error: Option<String>,
}

impl StoryboardUi {
    /// The last refusal from a storyboard edit.
    pub(crate) fn error(&self) -> Option<String> {
        self.error.clone()
    }
}

/// Inputs bound to one panel and one caption field layout; either changing
/// rebuilds them.
struct Inspector {
    panel: PageId,
    fields: Vec<(CaptionId, bool)>,
    name: Entity<InputState>,
    frames: Entity<InputState>,
    seconds: Entity<InputState>,
    captions: Vec<(CaptionId, CaptionInput)>,
    field_names: Vec<(CaptionId, Entity<InputState>)>,
    new_field: Entity<InputState>,
    /// The caption input focused last: formatting applies to its selection.
    focused: Option<CaptionId>,
    /// The model value each input last showed, by input.
    shown: HashMap<String, String>,
    _subs: Vec<Subscription>,
}

#[derive(Clone)]
enum CaptionInput {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
}

impl CaptionInput {
    fn value(&self, cx: &App) -> String {
        match self {
            Self::Line(state) => state.read(cx).value().to_string(),
            Self::Area(state) => state.read(cx).value().to_string(),
        }
    }
    fn selected_range(&self, cx: &App) -> Range<usize> {
        match self {
            Self::Line(state) => state.read(cx).selected_range(),
            Self::Area(state) => state.read(cx).selected_range(),
        }
    }
    fn is_focused(&self, window: &Window, cx: &App) -> bool {
        match self {
            Self::Line(state) => state.read(cx).focus_handle(cx).is_focused(window),
            Self::Area(state) => state.read(cx).focus_handle(cx).is_focused(window),
        }
    }
    fn set_value(&self, value: String, window: &mut Window, cx: &mut App) {
        match self {
            Self::Line(state) => state.update(cx, |s, cx| s.set_value(value, window, cx)),
            Self::Area(state) => state.update(cx, |s, cx| s.set_value(value, window, cx)),
        }
    }
}

/// Replace `caption`'s text with `text`, keeping the formatting of the text
/// on either side of what changed.
pub(crate) fn set_caption_text(caption: &mut Caption, text: &str) {
    let old = caption.text.as_str();
    let prefix: usize = old
        .chars()
        .zip(text.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let room = old.len().min(text.len()) - prefix;
    let mut suffix = 0;
    for (a, b) in old.chars().rev().zip(text.chars().rev()) {
        if a != b || suffix + a.len_utf8() > room {
            break;
        }
        suffix += a.len_utf8();
    }
    let replacement = text[prefix..text.len() - suffix].to_string();
    caption.replace_range(prefix..old.len() - suffix, &replacement);
}

fn hsla_of([r, g, b, a]: [u8; 4]) -> Hsla {
    Rgba {
        r: f32::from(r) / 255.,
        g: f32::from(g) / 255.,
        b: f32::from(b) / 255.,
        a: f32::from(a) / 255.,
    }
    .into()
}

/// A caption with its formatting. Unformatted text, and text in the default
/// caption colour, uses the interface's text colour.
pub(crate) fn caption_text(caption: &Caption) -> StyledText {
    let base = Caption::base_style();
    let highlights: Vec<_> = caption
        .runs
        .iter()
        .map(|run| {
            let s = &run.style;
            let style = HighlightStyle {
                color: (s.color != base.color).then(|| hsla_of(s.color)),
                font_weight: s.bold.then_some(FontWeight::BOLD),
                font_style: s.italic.then_some(FontStyle::Italic),
                underline: s.underline.then(|| UnderlineStyle {
                    thickness: px(1.),
                    ..Default::default()
                }),
                strikethrough: s.strikethrough.then(|| StrikethroughStyle {
                    thickness: px(1.),
                    ..Default::default()
                }),
                ..Default::default()
            };
            (run.start..run.end, style)
        })
        .collect();
    StyledText::new(caption.text.clone()).with_highlights(highlights)
}

fn label_of<T: PartialEq>(table: &[(T, &'static str)], value: T) -> &'static str {
    table
        .iter()
        .find(|(v, _)| *v == value)
        .map_or("", |(_, l)| l)
}

/// Frames as seconds at the board's frame rate.
fn seconds(board: &Storyboard, frames: u32) -> f64 {
    board.settings.frame_rate.frames_to_ms(frames) / 1000.
}

impl EditorView {
    /// Apply a storyboard change as one Undo step, reporting refusals.
    pub(crate) fn edit_board(
        &mut self,
        edit: impl FnOnce(&mut Storyboard) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.editor.edit_storyboard(edit) {
            Ok(()) => {
                self.storyboard_ui.error = None;
                self.after_change(cx);
                true
            }
            Err(error) => {
                self.storyboard_ui.error = Some(error.clone());
                self.set_status(error, true, cx);
                false
            }
        }
    }

    fn edit_panel(&mut self, panel: PageId, edit: impl FnOnce(&mut Panel), cx: &mut Context<Self>) {
        self.edit_board(
            |board| {
                edit(
                    board
                        .panels
                        .get_mut(&panel)
                        .ok_or("Panel does not exist.")?,
                );
                Ok(())
            },
            cx,
        );
    }

    fn commit_panel_name(&mut self, panel: PageId, name: String, cx: &mut Context<Self>) {
        let Some(meta) = self.editor.page_list().iter().find(|m| m.id == panel) else {
            return;
        };
        if meta.name == name.trim() {
            return;
        }
        if self.editor.storyboard().is_some_and(|b| b.is_locked(panel)) {
            self.set_status("That panel is locked. Unlock it to change it.", true, cx);
            return;
        }
        let bleed = meta.bleed_mm;
        match self.editor.rename_page(panel, name, bleed) {
            Ok(()) => {
                self.storyboard_ui.error = None;
                self.after_change(cx);
            }
            Err(error) => {
                self.storyboard_ui.error = Some(error.clone());
                self.set_status(error, true, cx);
            }
        }
        // Show the stored name again after a refusal or trimming.
        self.storyboard_ui.inspector = None;
    }

    fn commit_duration(
        &mut self,
        panel: PageId,
        text: &str,
        in_seconds: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(rate) = self.editor.storyboard().map(|b| b.settings.frame_rate) else {
            return;
        };
        let fps = rate.fps();
        let value = text.trim().parse::<f64>().ok().filter(|v| v.is_finite());
        let frames = value.map(|v| {
            if in_seconds {
                (v * fps).round()
            } else {
                v.round()
            }
        });
        match frames {
            Some(frames) if frames >= 1. && frames <= f64::from(u32::MAX) => {
                self.edit_panel(panel, |p| p.frames = frames as u32, cx)
            }
            _ => {
                let error = format!("Enter a duration of at least one frame ({}).", rate.label());
                self.storyboard_ui.error = Some(error.clone());
                self.set_status(error, true, cx);
            }
        }
        // Show both fields from the stored duration.
        self.storyboard_ui.inspector = None;
        cx.notify();
    }

    fn commit_caption(
        &mut self,
        panel: PageId,
        field: CaptionId,
        text: String,
        cx: &mut Context<Self>,
    ) {
        self.edit_caption(panel, field, &text, |_| {}, cx);
    }

    /// Set a caption's text and then change it further, as one Undo step.
    fn edit_caption(
        &mut self,
        panel: PageId,
        field: CaptionId,
        text: &str,
        edit: impl FnOnce(&mut Caption),
        cx: &mut Context<Self>,
    ) {
        self.edit_board(
            |board| {
                let p = board
                    .panels
                    .get_mut(&panel)
                    .ok_or("Panel does not exist.")?;
                let mut caption = p.captions.get(&field).cloned().unwrap_or_default();
                set_caption_text(&mut caption, text);
                edit(&mut caption);
                if caption.text.is_empty() {
                    p.captions.remove(&field);
                } else {
                    p.captions.insert(field, caption);
                }
                Ok(())
            },
            cx,
        );
    }

    /// The focused caption, its input text and its selection (the whole
    /// caption when nothing is selected).
    fn caption_target(&self, cx: &App) -> Option<(PageId, CaptionId, String, Range<usize>)> {
        let inspector = self.storyboard_ui.inspector.as_ref()?;
        let field = inspector.focused?;
        let input = &inspector.captions.iter().find(|(id, _)| *id == field)?.1;
        let text = input.value(cx);
        let mut range = input.selected_range(cx);
        range.end = range.end.min(text.len());
        range.start = range.start.min(range.end);
        if range.is_empty() {
            range = 0..text.len();
        }
        Some((inspector.panel, field, text, range))
    }

    /// Toggle bold, italic, underline or strikethrough on the selection.
    pub(crate) fn format_caption(&mut self, format: CaptionFormat, cx: &mut Context<Self>) {
        let Some((panel, field, text, range)) = self.caption_target(cx) else {
            self.set_status("Select caption text to format.", false, cx);
            return;
        };
        self.edit_caption(
            panel,
            field,
            &text,
            |caption| {
                let on = !*format.flag(&mut caption.style_at(range.start));
                caption.apply_style(range, |style| *format.flag(style) = on);
            },
            cx,
        );
    }

    /// Colour the selection with the app's colour picker.
    pub(crate) fn open_caption_color(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((panel, field, text, range)) = self.caption_target(cx) else {
            self.set_status("Select caption text to colour.", false, cx);
            return;
        };
        let initial = self
            .editor
            .storyboard()
            .and_then(|b| b.panels.get(&panel)?.captions.get(&field).cloned())
            .map_or(Caption::base_style().color, |c| {
                c.style_at(range.start).color
            });
        self.open_color_dialog(
            "caption",
            "Caption colour",
            initial,
            move |this, color, cx| {
                let range = range.clone();
                this.edit_caption(
                    panel,
                    field,
                    &text,
                    |c| c.apply_style(range, |s| s.color = color),
                    cx,
                )
            },
            window,
            cx,
        );
    }

    fn add_caption_field(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self
            .storyboard_ui
            .inspector
            .as_ref()
            .map(|i| i.new_field.clone())
        else {
            return;
        };
        let name = input.read(cx).value().to_string();
        if self.edit_board(|b| b.add_caption_field(&name, true, true).map(|_| ()), cx) {
            self.storyboard_ui.inspector = None;
        }
    }

    fn rename_caption_field(&mut self, field: CaptionId, name: String, cx: &mut Context<Self>) {
        let unchanged = self
            .editor
            .storyboard()
            .and_then(|b| b.captions.iter().find(|c| c.id == field))
            .is_none_or(|c| c.name == name.trim());
        if !unchanged {
            // `edit_storyboard` validates names: non-empty and unique.
            self.edit_board(
                |b| {
                    let caption = b.captions.iter_mut().find(|c| c.id == field);
                    caption.ok_or("No caption field has that ID.")?.name = name.trim().into();
                    Ok(())
                },
                cx,
            );
        }
        self.storyboard_ui.inspector = None;
        cx.notify();
    }

    /// A caption field change; the inputs follow the new field layout.
    fn edit_caption_fields(
        &mut self,
        edit: impl FnOnce(&mut Storyboard) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        if self.edit_board(edit, cx) {
            self.storyboard_ui.inspector = None;
        }
    }

    /// Build or refresh the inputs for the active panel.
    fn sync_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(board) = self.editor.storyboard() else {
            self.storyboard_ui.inspector = None;
            return;
        };
        let panel = self.editor.active_page();
        let Some(data) = board.panels.get(&panel) else {
            self.storyboard_ui.inspector = None;
            return;
        };
        let fields: Vec<_> = board.captions.iter().map(|c| (c.id, c.multiline)).collect();
        let name = self
            .editor
            .page_list()
            .iter()
            .find(|m| m.id == panel)
            .map(|m| m.name.clone())
            .unwrap_or_default();
        let frames = data.frames.to_string();
        let secs = format!("{:.2}", seconds(board, data.frames));
        let texts: Vec<String> = fields
            .iter()
            .map(|(id, _)| {
                data.captions
                    .get(id)
                    .map(|c| c.text.clone())
                    .unwrap_or_default()
            })
            .collect();
        let field_names: Vec<String> = board.captions.iter().map(|c| c.name.clone()).collect();
        let stale = self
            .storyboard_ui
            .inspector
            .as_ref()
            .is_none_or(|i| i.panel != panel || i.fields != fields);
        if stale {
            self.storyboard_ui.inspector =
                Some(self.build_inspector(panel, &fields, [&name, &frames, &secs], window, cx));
        }
        let Some(inspector) = &mut self.storyboard_ui.inspector else {
            return;
        };
        let line = |state: &Entity<InputState>| CaptionInput::Line(state.clone());
        let targets = [
            ("name".to_string(), line(&inspector.name), name),
            ("frames".to_string(), line(&inspector.frames), frames),
            ("seconds".to_string(), line(&inspector.seconds), secs),
        ]
        .into_iter()
        .chain(
            inspector
                .captions
                .iter()
                .zip(texts)
                .map(|((id, input), text)| (format!("caption-{id}"), input.clone(), text)),
        )
        .chain(
            inspector
                .field_names
                .iter()
                .zip(field_names)
                .map(|((id, input), name)| (format!("field-{id}"), line(input), name)),
        )
        .collect::<Vec<_>>();
        // Show values the model changed to (Undo, another panel's edit),
        // but never replace text being typed: focus may already have moved
        // on by the time a field's Blur commits it.
        for (key, input, value) in targets {
            if inspector.shown.get(&key) == Some(&value) {
                continue;
            }
            if !input.is_focused(window, cx) {
                input.set_value(value.clone(), window, cx);
            }
            inspector.shown.insert(key, value);
        }
    }

    fn build_inspector(
        &mut self,
        panel: PageId,
        fields: &[(CaptionId, bool)],
        [name, frames, secs]: [&str; 3],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Inspector {
        let mut subs = Vec::new();
        let commit_on =
            |event: &InputEvent| matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur);
        let line = |value: &str,
                    subs: &mut Vec<Subscription>,
                    window: &mut Window,
                    cx: &mut Context<Self>,
                    commit: Commit| {
            let input = cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()));
            subs.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event: &InputEvent, _, cx| {
                    if commit_on(event) {
                        commit(this, input.read(cx).value().to_string(), cx);
                    }
                },
            ));
            input
        };
        let name = line(
            name,
            &mut subs,
            window,
            cx,
            Box::new(move |this, value, cx| this.commit_panel_name(panel, value, cx)),
        );
        let frames = line(
            frames,
            &mut subs,
            window,
            cx,
            Box::new(move |this, value, cx| this.commit_duration(panel, &value, false, cx)),
        );
        let seconds = line(
            secs,
            &mut subs,
            window,
            cx,
            Box::new(move |this, value, cx| this.commit_duration(panel, &value, true, cx)),
        );
        let mut captions = Vec::new();
        for &(field, multiline) in fields {
            let focus = move |this: &mut Self, event: &InputEvent| {
                if matches!(event, InputEvent::Focus)
                    && let Some(inspector) = &mut this.storyboard_ui.inspector
                {
                    inspector.focused = Some(field);
                }
            };
            let input = if multiline {
                let state = cx.new(|cx| TextareaState::new(window, cx).rows(3));
                subs.push(cx.subscribe_in(
                    &state,
                    window,
                    move |this, state, event: &InputEvent, _, cx| {
                        focus(this, event);
                        if matches!(
                            event,
                            InputEvent::Blur
                                | InputEvent::PressEnter {
                                    secondary: true,
                                    ..
                                }
                        ) {
                            this.commit_caption(
                                panel,
                                field,
                                state.read(cx).value().to_string(),
                                cx,
                            );
                        }
                    },
                ));
                CaptionInput::Area(state)
            } else {
                let state = cx.new(|cx| InputState::new(window, cx));
                subs.push(cx.subscribe_in(
                    &state,
                    window,
                    move |this, state, event: &InputEvent, _, cx| {
                        focus(this, event);
                        if commit_on(event) {
                            this.commit_caption(
                                panel,
                                field,
                                state.read(cx).value().to_string(),
                                cx,
                            );
                        }
                    },
                ));
                CaptionInput::Line(state)
            };
            captions.push((field, input));
        }
        let field_names = fields
            .iter()
            .map(|&(field, _)| {
                let input = line(
                    "",
                    &mut subs,
                    window,
                    cx,
                    Box::new(move |this, value, cx| this.rename_caption_field(field, value, cx)),
                );
                (field, input)
            })
            .collect();
        let new_field = cx.new(|cx| InputState::new(window, cx).placeholder("New field name"));
        subs.push(
            cx.subscribe_in(&new_field, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.add_caption_field(cx);
                }
            }),
        );
        Inspector {
            panel,
            fields: fields.to_vec(),
            name,
            frames,
            seconds,
            captions,
            field_names,
            new_field,
            focused: None,
            shown: HashMap::new(),
            _subs: subs,
        }
    }

    /// The Panel tab of the sidebar.
    pub(super) fn storyboard_inspector(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_inspector(window, cx);
        let root = div()
            .id("storyboard-inspector")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(12.))
            .text_size(px(12.));
        let (Some(board), Some(inspector)) = (
            self.editor.storyboard(),
            self.storyboard_ui.inspector.as_ref(),
        ) else {
            return root
                .child(mono(
                    "Open a storyboard to inspect its panels.",
                    11.,
                    p.muted,
                ))
                .into_any_element();
        };
        let panel = inspector.panel;
        let data = &board.panels[&panel];
        let locked = board.is_locked(panel);
        let scene = board.scenes.get(&data.scene);
        let scene_locked = scene.is_some_and(|s| s.locked);
        let rate = board.settings.frame_rate;
        let heading = |text: &'static str| label(text, p).pt(px(4.));
        let field = |id: &'static str, title: &'static str, input: AnyElement| {
            div()
                .id(id)
                .test_support()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(mono(title, 10., p.muted))
                .child(input)
        };
        let mut root = root.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(label("Panel", p))
                .child(div().flex_1())
                .child(mono(
                    format!(
                        "{} · {}",
                        scene.map_or("", |s| s.name.as_str()),
                        rate.label()
                    ),
                    10.,
                    p.muted,
                )),
        );
        if locked {
            root = root.child(
                div()
                    .id("storyboard-locked")
                    .test_support()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .p(px(8.))
                    .rounded(px(4.))
                    .bg(p.soft_bg)
                    .child(div().flex_1().min_w_0().child(if scene_locked {
                        "This panel's scene is locked; its fields are read-only."
                    } else {
                        "This panel is locked; its fields are read-only."
                    }))
                    .child(
                        Button::new("storyboard-unlock")
                            .label("Unlock")
                            .flex_none()
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.edit_board(
                                    |b| {
                                        let p = b
                                            .panels
                                            .get_mut(&panel)
                                            .ok_or("Panel does not exist.")?;
                                        p.locked = false;
                                        let scene = p.scene;
                                        if let Some(scene) = b.scenes.get_mut(&scene) {
                                            scene.locked = false;
                                        }
                                        Ok(())
                                    },
                                    cx,
                                );
                            })),
                    ),
            );
        }
        root = root
            .child(field(
                "storyboard-name",
                "Name",
                Input::new(&inspector.name)
                    .small()
                    .readonly(locked)
                    .into_any_element(),
            ))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        field(
                            "storyboard-frames",
                            "Frames",
                            Input::new(&inspector.frames)
                                .small()
                                .readonly(locked)
                                .into_any_element(),
                        )
                        .flex_1(),
                    )
                    .child(
                        field(
                            "storyboard-seconds",
                            "Seconds",
                            Input::new(&inspector.seconds)
                                .small()
                                .readonly(locked)
                                .into_any_element(),
                        )
                        .flex_1(),
                    ),
            )
            .child(mono(
                format!(
                    "{} frames · {:.2} s at {}",
                    data.frames,
                    seconds(board, data.frames),
                    rate.label()
                ),
                10.,
                p.muted,
            ));
        let owner = cx.weak_entity();
        let menu = |id: &'static str,
                    title: &'static str,
                    current: &'static str,
                    items: Vec<(&'static str, bool, PanelEdit)>| {
            let owner = owner.clone();
            let items = Rc::new(items);
            field(
                id,
                title,
                Button::new(SharedString::from(format!("{id}-menu")))
                    .label(current)
                    .small()
                    .outline()
                    .w_full()
                    .disabled(locked)
                    .dropdown_menu(move |mut menu, _, _| {
                        for (index, (label, on, _)) in items.iter().enumerate() {
                            let owner = owner.clone();
                            let items = items.clone();
                            menu = menu.item(PopupMenuItem::new(*label).checked(*on).on_click(
                                move |_, _, cx| {
                                    let items = items.clone();
                                    owner
                                        .update(cx, |this, cx| {
                                            this.edit_panel(panel, |p| (items[index].2)(p), cx)
                                        })
                                        .ok();
                                },
                            ));
                        }
                        menu
                    })
                    .into_any_element(),
            )
            .flex_1()
        };
        root = root
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(menu(
                        "storyboard-shot",
                        "Shot size",
                        label_of(&SHOT_SIZES, data.size),
                        SHOT_SIZES
                            .iter()
                            .map(|&(v, l)| {
                                (
                                    l,
                                    v == data.size,
                                    Box::new(move |p: &mut Panel| p.size = v) as PanelEdit,
                                )
                            })
                            .collect(),
                    ))
                    .child(menu(
                        "storyboard-angle",
                        "Camera angle",
                        label_of(&CAMERA_ANGLES, data.angle),
                        CAMERA_ANGLES
                            .iter()
                            .map(|&(v, l)| {
                                (
                                    l,
                                    v == data.angle,
                                    Box::new(move |p: &mut Panel| p.angle = v) as PanelEdit,
                                )
                            })
                            .collect(),
                    )),
            )
            .child(menu(
                "storyboard-status",
                "Status",
                label_of(&PANEL_STATUSES, data.status),
                PANEL_STATUSES
                    .iter()
                    .map(|&(v, l)| {
                        (
                            l,
                            v == data.status,
                            Box::new(move |p: &mut Panel| p.status = v) as PanelEdit,
                        )
                    })
                    .collect(),
            ));
        let mut tags = div().flex().flex_wrap().items_center().gap(px(4.)).child(
            chip("storyboard-tag-none", "None", data.tag.is_none(), p)
                .test_support()
                .when(!locked, |c| {
                    c.on_click(cx.listener(move |this, _, _, cx| {
                        this.edit_panel(panel, |p| p.tag = None, cx)
                    }))
                }),
        );
        for (index, (name, color)) in TAG_PALETTE.iter().enumerate() {
            let tag = index as u8;
            let on = data.tag == Some(tag);
            tags = tags.child(
                div()
                    .id(("storyboard-tag", index))
                    .test_support()
                    .role(Role::Button)
                    .aria_label(format!("{name} tag"))
                    .size(px(20.))
                    .rounded_full()
                    .bg(rgb(*color))
                    .border_2()
                    .border_color(if on { p.ink } else { transparent_black() })
                    .when(!locked, |d| {
                        d.cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.edit_panel(panel, |p| p.tag = Some(tag), cx)
                            }))
                    }),
            );
        }
        root = root.child(field(
            "storyboard-tags",
            "Tag colour",
            tags.into_any_element(),
        ));
        let lock_chip =
            |id: &'static str, text: &'static str, on: bool| chip(id, text, on, p).test_support();
        root = root.child(
            div()
                .flex()
                .gap(px(6.))
                .child(
                    lock_chip("storyboard-lock-panel", "Lock panel", data.locked).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.edit_panel(panel, |p| p.locked = !p.locked, cx)
                        }),
                    ),
                )
                .child(
                    lock_chip("storyboard-lock-scene", "Lock scene", scene_locked).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.edit_board(
                                |b| {
                                    let scene =
                                        b.panels.get(&panel).ok_or("Panel does not exist.")?.scene;
                                    let scene =
                                        b.scenes.get_mut(&scene).ok_or("Scene does not exist.")?;
                                    scene.locked = !scene.locked;
                                    Ok(())
                                },
                                cx,
                            );
                        }),
                    ),
                ),
        );
        // Captions, each with its formatting shown under the input.
        root = root.child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(heading("Captions"))
                .child(div().flex_1())
                .children(CaptionFormat::ALL.iter().map(|&(format, glyph, name)| {
                    Button::new(SharedString::from(format!(
                        "storyboard-format-{}",
                        name.to_lowercase()
                    )))
                    .label(glyph)
                    .accessibility_label(name)
                    .tooltip(format!("{name} the selected caption text"))
                    .xsmall()
                    .ghost()
                    .disabled(locked)
                    .on_click(cx.listener(move |this, _, _, cx| this.format_caption(format, cx)))
                }))
                .child(
                    Button::new("storyboard-format-color")
                        .label("A")
                        .accessibility_label("Colour")
                        .tooltip("Colour the selected caption text")
                        .xsmall()
                        .ghost()
                        .disabled(locked)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_caption_color(window, cx)),
                        ),
                ),
        );
        for ((field_id, input), caption_field) in inspector.captions.iter().zip(&board.captions) {
            let caption = data.captions.get(field_id);
            let editor = match input {
                CaptionInput::Line(state) => Input::new(state)
                    .small()
                    .readonly(locked)
                    .into_any_element(),
                CaptionInput::Area(state) => Textarea::new(state)
                    .h(rems(4.5))
                    .readonly(locked)
                    .into_any_element(),
            };
            root = root.child(
                div()
                    .id(("storyboard-caption", *field_id as usize))
                    .test_support()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(mono(caption_field.name.clone(), 10., p.muted))
                    .child(editor)
                    .children(caption.filter(|c| !c.runs.is_empty()).map(|caption| {
                        div()
                            .id(("storyboard-caption-preview", *field_id as usize))
                            .test_support()
                            .px(px(6.))
                            .py(px(4.))
                            .rounded(px(3.))
                            .bg(p.soft_bg)
                            .text_color(p.ink)
                            .child(caption_text(caption))
                    })),
            );
        }
        root = root.child(
            Button::new("storyboard-fields-toggle")
                .label(if self.storyboard_ui.fields_open {
                    "Caption fields ▴"
                } else {
                    "Caption fields…"
                })
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.storyboard_ui.fields_open = !this.storyboard_ui.fields_open;
                    cx.notify();
                })),
        );
        if self.storyboard_ui.fields_open {
            root = root.child(self.caption_field_manager(board, inspector, p, cx));
        }
        root = root
            .child(self.layer_animation_section(panel, locked, p, cx))
            .child(self.layer_comps_section(panel, locked, p, cx));
        root.children(self.storyboard_ui.error.clone().map(|error| {
            div()
                .id("storyboard-error")
                .test_support()
                .aria_label(error.clone())
                .text_color(p.accent)
                .child(error)
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(mono(
                    "This panel's layers are in the Layers dock.",
                    10.,
                    p.muted,
                ))
                .child(
                    Button::new("storyboard-show-layers")
                        .label("Layers")
                        .xsmall()
                        .outline()
                        .on_click(
                            cx.listener(|this, _, window, cx| this.show_layers_panel(window, cx)),
                        ),
                ),
        )
        .into_any_element()
    }

    /// Add, rename, reorder and remove caption fields; multi-line and print.
    fn caption_field_manager(
        &self,
        board: &Storyboard,
        inspector: &Inspector,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let count = board.captions.len();
        let mut list = div()
            .id("storyboard-fields")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(8.))
            .rounded(px(4.))
            .border_1()
            .border_color(p.line);
        for (index, ((id, input), caption)) in inspector
            .field_names
            .iter()
            .zip(&board.captions)
            .enumerate()
        {
            let id = *id;
            let (multiline, print) = (caption.multiline, caption.print);
            let key = id as usize;
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().id(("storyboard-field-name", key)).test_support().child(Input::new(input).small()))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(4.))
                            .child(chip(("storyboard-field-multiline", key), "Multi-line", multiline, p).test_support().on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.edit_caption_fields(
                                        |b| {
                                            let c = b.captions.iter_mut().find(|c| c.id == id);
                                            c.ok_or("No caption field has that ID.")?.multiline = !multiline;
                                            Ok(())
                                        },
                                        cx,
                                    )
                                }),
                            ))
                            .child(chip(("storyboard-field-print", key), "Print", print, p).test_support().on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.edit_caption_fields(
                                        |b| {
                                            let c = b.captions.iter_mut().find(|c| c.id == id);
                                            c.ok_or("No caption field has that ID.")?.print = !print;
                                            Ok(())
                                        },
                                        cx,
                                    )
                                },
                            )))
                            .child(
                                Button::new(("storyboard-field-up", key))
                                    .label("↑")
                                    .accessibility_label("Move field up")
                                    .xsmall()
                                    .ghost()
                                    .disabled(index == 0)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.edit_caption_fields(|b| b.move_caption_field(id, index - 1), cx)
                                    })),
                            )
                            .child(
                                Button::new(("storyboard-field-down", key))
                                    .label("↓")
                                    .accessibility_label("Move field down")
                                    .xsmall()
                                    .ghost()
                                    .disabled(index + 1 == count)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.edit_caption_fields(|b| b.move_caption_field(id, index + 1), cx)
                                    })),
                            )
                            .child(
                                Button::new(("storyboard-field-remove", key))
                                    .label("Remove")
                                    .tooltip("Remove the field and its text on every panel (Undo restores it)")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.edit_caption_fields(|b| b.remove_caption_field(id), cx)
                                    })),
                            ),
                    ),
            );
        }
        list.child(
            div()
                .flex()
                .gap(px(6.))
                .child(
                    div()
                        .id("storyboard-new-field")
                        .test_support()
                        .flex_1()
                        .child(Input::new(&inspector.new_field).small()),
                )
                .child(
                    Button::new("storyboard-add-field")
                        .label("Add field")
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| this.add_caption_field(cx))),
                ),
        )
    }
}

#[cfg(test)]
mod tests;
