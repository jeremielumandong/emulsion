//! One searchable, document-scoped font picker for every text control.
use super::*;
use emulsion_core::{design_fonts::EmbeddedFont, text::TextSpec};
use gpui_kit::component::Sizable;
use std::{collections::BTreeMap, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq)]
struct FontChoice {
    family: String,
    label: String,
    /// Where the font comes from, in the interface language.
    source: String,
}

fn retire_previews(previews: &mut HashMap<String, Arc<RenderImage>>, cx: &mut App) {
    let images = std::mem::take(previews);
    if !images.is_empty() {
        // The current window is absent from App while rendering/dispatching.
        // Defer retirement so all of its uploaded atlas entries are released.
        cx.defer(move |cx| {
            for image in images.into_values() {
                cx.drop_image(image, None);
            }
        });
    }
}

fn choices(installed: Vec<String>, embedded: &BTreeMap<String, EmbeddedFont>) -> Vec<FontChoice> {
    let mut families = installed;
    families.extend(embedded.keys().cloned());
    families.extend(["Geist".into(), "Geist Mono".into()]);
    families.retain(|font| !font.starts_with("EmulsionFont-") || embedded.contains_key(font));
    families.sort();
    families.dedup();
    let mut result: Vec<_> = families
        .into_iter()
        .filter(|family| !family.is_empty())
        .map(|family| FontChoice {
            label: embedded
                .get(&family)
                .map_or_else(|| family.clone(), |font| font.family().to_string()),
            source: if embedded.contains_key(&family) {
                t!("editor.font_picker.embedded")
            } else if matches!(family.as_str(), "Geist" | "Geist Mono") {
                t!("editor.font_picker.bundled")
            } else {
                t!("editor.font_picker.installed")
            }
            .into_owned(),
            family,
        })
        .collect();
    result.sort_by_key(|choice| (choice.label.to_lowercase(), choice.family.clone()));
    result.insert(
        0,
        FontChoice {
            family: String::new(),
            label: t!("editor.font_picker.default_font").into_owned(),
            source: t!("editor.font_picker.system").into_owned(),
        },
    );
    result
}

fn matching(choices: &[FontChoice], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    choices
        .iter()
        .enumerate()
        .filter(|(_, choice)| {
            let searchable = format!("{} {}", choice.label, choice.source).to_lowercase();
            query
                .split_whitespace()
                .all(|word| searchable.contains(word))
        })
        .map(|(index, _)| index)
        .collect()
}

pub(super) struct FontPickerState {
    search: Entity<InputState>,
    _subscription: Subscription,
    choices: Vec<FontChoice>,
    visible: Vec<usize>,
    active: usize,
    current: String,
    sample: String,
    bold: bool,
    italic: bool,
    previews: HashMap<String, Arc<RenderImage>>,
    preview_color: [u8; 4],
    scroll: UniformListScrollHandle,
    anchor: Point<Pixels>,
    trigger_bounds: Option<Bounds<Pixels>>,
    target: Option<NodeId>,
    page: emulsion_core::project::PageId,
    revision: u64,
    range: Option<Range<usize>>,
}

/// Render with the document's own shaper, including private embedded aliases.
/// Dimensions and font size are raster pixels; callers can display at half size
/// for a sharp 2× sample. GPUI's system font registry cannot resolve these aliases.
#[allow(clippy::too_many_arguments)]
pub(super) fn font_preview(
    font: &str,
    sample: &str,
    bold: bool,
    italic: bool,
    color: [u8; 4],
    width: u32,
    height: u32,
    size: f32,
) -> Arc<RenderImage> {
    let spec = TextSpec {
        text: sample.into(),
        font: font.into(),
        bold,
        italic,
        color,
        size,
        x: 2.,
        y: 0.,
        ..Default::default()
    };
    let mut bytes = emulsion_core::text::rasterize(&spec, width, height).to_srgba8();
    for pixel in bytes.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Arc::new(crate::viewport::bgra_image(width, height, bytes))
}

impl EditorView {
    pub(super) fn font_label(&self, family: &str) -> String {
        if family.is_empty() {
            t!("editor.font_picker.default_font").into_owned()
        } else {
            self.editor
                .doc
                .design
                .fonts
                .get(family)
                .map(|font| {
                    t!("editor.font_picker.embedded_label", family = font.family()).into_owned()
                })
                .unwrap_or_else(|| family.into())
        }
    }

    pub(super) fn toggle_font_picker(
        &mut self,
        bounds: Option<Bounds<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu == Some(Menu::Font) {
            self.dismiss_font_picker(window, cx);
            return;
        }
        let target = self.text_target();
        if self.selected_layer_ids().len() > 1
            || target
                .as_ref()
                .is_some_and(|(id, _)| self.editor.doc.locked_ancestor(*id).is_some())
            || (target.is_none() && self.tool != Tool::Type)
        {
            return;
        }
        // Preserve a selected character range before the search input takes
        // focus, and finish typing so choosing a font is its own undo step.
        let range = self.text_style_range();
        self.close_text_field(cx);
        let spec = target
            .as_ref()
            .map_or(&self.type_tool.spec, |(_, spec)| spec.as_ref());
        let style = spec.style_at(range.as_ref().map_or(0, |range| range.start));
        let sample = range
            .as_ref()
            .and_then(|range| spec.text.get(range.clone()))
            .unwrap_or(&spec.text)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let sample: String = sample.graphemes(true).take(36).collect();
        let sample = if sample.is_empty() {
            "Aa Bb The quick brown fox 0123".into()
        } else {
            sample
        };
        let choices = choices(
            emulsion_core::text::font_families(),
            &self.editor.doc.design.fonts,
        );
        let visible = matching(&choices, "");
        let active = choices
            .iter()
            .position(|choice| choice.family == style.font)
            .unwrap_or(0);
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("editor.font_picker.search_placeholder"))
        });
        let subscription = cx.subscribe(&search, |this, input, event, cx| {
            if matches!(event, InputEvent::Change)
                && let Some(state) = this.type_tool.font_picker.as_mut()
            {
                state.visible = matching(&state.choices, input.read(cx).value().as_ref());
                state.active = 0;
                state.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
        });
        let scroll = UniformListScrollHandle::new();
        scroll.scroll_to_item(active, ScrollStrategy::Nearest);
        self.type_tool.font_picker = Some(FontPickerState {
            search: search.clone(),
            _subscription: subscription,
            choices,
            visible,
            active,
            current: style.font,
            sample,
            bold: style.bold,
            italic: style.italic,
            previews: HashMap::new(),
            preview_color: [0; 4],
            scroll,
            anchor: bounds.map_or(window.mouse_position(), |bounds| {
                point(bounds.left(), bounds.bottom() + px(4.))
            }),
            trigger_bounds: bounds,
            target: target.map(|(id, _)| id),
            page: self.editor.active_page(),
            revision: self.editor.revision,
            range,
        });
        self.menu = Some(Menu::Font);
        search.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn dismiss_font_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_font_picker(cx);
        window.focus(&self.canvas_focus, cx);
        cx.notify();
    }

    pub(super) fn close_font_picker(&mut self, cx: &mut Context<Self>) {
        if self.menu == Some(Menu::Font) {
            self.menu = None;
        }
        if let Some(mut state) = self.type_tool.font_picker.take() {
            retire_previews(&mut state.previews, cx);
        }
    }

    fn font_picker_target_is_current(&self, state: &FontPickerState) -> bool {
        state.page == self.editor.active_page()
            && state.revision == self.editor.revision
            && state.target == self.text_target().map(|(id, _)| id)
            && self.selected_layer_ids().len() <= 1
            && state
                .target
                .is_none_or(|id| self.editor.doc.locked_ancestor(id).is_none())
            && (state.target.is_some() || self.tool == Tool::Type)
    }

    fn choose_font(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut state) = self.type_tool.font_picker.take() else {
            return;
        };
        if self.font_picker_target_is_current(&state)
            && !self.editor.in_transaction()
            && let Some(choice) = state
                .visible
                .get(index)
                .and_then(|index| state.choices.get(*index))
            && (!choice.family.starts_with("EmulsionFont-")
                || self.editor.doc.design.fonts.contains_key(&choice.family))
        {
            self.type_tool.selection = state.target.zip(state.range);
            self.restyle_text(|spec| spec.font.clone_from(&choice.family), cx);
        }
        retire_previews(&mut state.previews, cx);
        self.dismiss_font_picker(window, cx);
    }

    pub(super) fn font_picker(
        &mut self,
        p: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.menu != Some(Menu::Font) {
            self.close_font_picker(cx);
            return None;
        }
        if self
            .type_tool
            .font_picker
            .as_ref()
            .is_some_and(|state| !self.font_picker_target_is_current(state))
            || self.previewing()
            || self.history.open
        {
            self.close_font_picker(cx);
            return None;
        }
        let state = self.type_tool.font_picker.as_mut()?;
        let rgb = p.ink.to_rgb();
        let color = [rgb.r, rgb.g, rgb.b, rgb.a].map(|v| (v * 255.).round() as u8);
        if state.preview_color != color {
            retire_previews(&mut state.previews, cx);
            state.preview_color = color;
        }
        let search = state.search.clone();
        let focus = search.read(cx).focus_handle(cx);
        let count = state.visible.len();
        let scroll = state.scroll.clone();
        let at = state.anchor;
        let (ink, muted, panel, line, accent) = (p.ink, p.muted, p.panel, p.line, p.accent);
        let height = (count.clamp(1, 5) as f32 * 64.)
            .min((f32::from(window.viewport_size().height) - 118.).max(40.));
        let list = uniform_list(
            "font-picker-results",
            count,
            cx.processor(move |this, range: Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        let state = this.type_tool.font_picker.as_mut()?;
                        let choice = state.choices.get(*state.visible.get(index)?)?.clone();
                        // Only visible rows are rasterized. Bound the session cache
                        // for machines with very large installed-font collections.
                        if state.previews.len() >= 128 {
                            retire_previews(&mut state.previews, cx);
                        }
                        let preview = state
                            .previews
                            .entry(choice.family.clone())
                            .or_insert_with(|| {
                                font_preview(
                                    &choice.family,
                                    &state.sample,
                                    state.bold,
                                    state.italic,
                                    color,
                                    600,
                                    64,
                                    40.,
                                )
                            })
                            .clone();
                        let selected = state.current == choice.family;
                        let active = state.active == index;
                        Some(
                            div()
                                .id(("font-row", index))
                                .test_support()
                                .role(Role::ListItem)
                                .aria_label(if selected {
                                    t!(
                                        "editor.font_picker.row_selected",
                                        label = choice.label,
                                        source = choice.source
                                    )
                                } else {
                                    t!(
                                        "editor.font_picker.row",
                                        label = choice.label,
                                        source = choice.source
                                    )
                                })
                                .h(px(64.))
                                .w_full()
                                .min_w_0()
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .justify_center()
                                .px_3()
                                .gap_1()
                                .cursor_pointer()
                                .when(active, |row| row.bg(accent.opacity(0.12)))
                                .hover(move |row| row.bg(accent.opacity(0.18)))
                                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.choose_font(index, window, cx)
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .text_size(px(11.))
                                        .child(
                                            div().flex_1().min_w_0().truncate().child(choice.label),
                                        )
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_color(muted)
                                                .child(choice.source),
                                        )
                                        .when(selected, |row| {
                                            row.child(div().text_color(accent).child("✓"))
                                        }),
                                )
                                .child(
                                    img(ImageSource::Render(preview))
                                        .w(px(300.))
                                        .h(px(32.))
                                        .flex_none(),
                                ),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&scroll)
        .h(px(height));
        Some(
            deferred(
                anchored().position(at).snap_to_window().child(
                    div()
                        .id("font-picker")
                        .test_support()
                        .role(Role::Dialog)
                        .aria_label(t!("editor.font_picker.choose_font"))
                        .occlude()
                        .track_focus(&focus)
                        .w(px(340.).min((window.viewport_size().width - px(16.)).max(px(0.))))
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .overflow_hidden()
                        .rounded_lg()
                        .border_1()
                        .border_color(line)
                        .shadow_lg()
                        .bg(panel)
                        .text_color(ink)
                        .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            match event.keystroke.key.as_str() {
                                "escape" | "tab" => this.dismiss_font_picker(window, cx),
                                "enter" => {
                                    let Some(state) = this.type_tool.font_picker.as_ref() else {
                                        return;
                                    };
                                    if state.visible.is_empty() {
                                        cx.stop_propagation();
                                        return;
                                    }
                                    this.choose_font(state.active, window, cx);
                                }
                                "up" | "down" => {
                                    let Some(state) = this.type_tool.font_picker.as_mut() else {
                                        return;
                                    };
                                    let last = state.visible.len().saturating_sub(1);
                                    state.active = if event.keystroke.key == "up" {
                                        state.active.saturating_sub(1)
                                    } else {
                                        (state.active + 1).min(last)
                                    };
                                    state
                                        .scroll
                                        .scroll_to_item(state.active, ScrollStrategy::Nearest);
                                    cx.notify();
                                }
                                _ => return,
                            }
                            cx.stop_propagation();
                            window.prevent_default();
                        }))
                        .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            // Let the trigger's click close an already-open picker
                            // instead of dismissing on down and reopening on click.
                            if this.type_tool.font_picker.as_ref().is_some_and(|state| {
                                state
                                    .trigger_bounds
                                    .is_some_and(|bounds| bounds.contains(&event.position))
                            }) {
                                return;
                            }
                            if this.menu == Some(Menu::Font) {
                                this.close_font_picker(cx);
                                cx.notify();
                            }
                        }))
                        .child(
                            div().p_2().child(
                                Input::new(&search)
                                    .id("font-search")
                                    .aria_label(t!("editor.font_picker.search_fonts"))
                                    .small(),
                            ),
                        )
                        .when(count == 0, |picker| {
                            picker.child(
                                div()
                                    .id("font-picker-empty")
                                    .test_support()
                                    .p_4()
                                    .text_size(px(12.))
                                    .text_color(muted)
                                    .child(t!("editor.font_picker.no_match")),
                            )
                        })
                        .when(count > 0, |picker| picker.child(list))
                        .child(
                            div()
                                .px_3()
                                .py_2()
                                .border_t_1()
                                .border_color(line)
                                .text_size(px(10.))
                                .text_color(muted)
                                .child(SharedString::from(t!(
                                    "editor.font_picker.footer",
                                    count = count
                                ))),
                        ),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use emulsion_core::command::Slot;
    use gpui_kit::test::TestWindowExt;

    fn setup(
        cx: &mut TestAppContext,
        design: bool,
    ) -> (Entity<EditorView>, &mut VisualTestContext) {
        let mut doc = Document::new(600, 400);
        let id = Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Text",
                TextSpec {
                    text: "Hello world".into(),
                    font: "Geist".into(),
                    x: 30.,
                    y: 40.,
                    size: 24.,
                    ..Default::default()
                },
                600,
                400,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        let (workspace, cx) = crate::tests::open(cx, doc.clone());
        cx.simulate_resize(size(px(1280.), px(900.)));
        let editor = cx.update(|window, cx| {
            if design {
                workspace.update(cx, |workspace, cx| {
                    workspace.install_project(
                        emulsion_core::project::ProjectEditor::new_project(
                            emulsion_core::project::ProjectKind::Design,
                            doc,
                        )
                        .unwrap(),
                        "Font discovery".into(),
                        window,
                        cx,
                    )
                });
            }
            let editor = workspace.read(cx).editor.clone().unwrap();
            editor.update(cx, |editor, cx| {
                editor.set_layer_selection(vec![id], Some(id));
                editor.set_tool(if design { Tool::Move } else { Tool::Type }, cx);
                if !design {
                    editor.select_sidebar(SidebarTab::Properties, cx);
                }
                window.focus(&editor.canvas_focus, cx);
            });
            editor
        });
        cx.run_until_parked();
        if design {
            click(cx, "design-drawer-close");
        }
        (editor, cx)
    }

    fn spec(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> TextSpec {
        cx.update(|_, cx| (*editor.read(cx).text_target().unwrap().1).clone())
    }

    fn click(cx: &mut VisualTestContext, id: &'static str) {
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            window.click(id, cx);
        });
        cx.run_until_parked();
    }

    #[test]
    fn font_catalog_is_document_scoped_searchable_and_deduplicated() {
        let font =
            EmbeddedFont::from_bytes(include_bytes!("../../../../assets/fonts/Geist.ttf").to_vec())
                .unwrap();
        let embedded = BTreeMap::from([(font.alias().to_string(), font.clone())]);
        let choices = choices(
            vec![
                "EmulsionFont-other-document".into(),
                "Geist".into(),
                "Geist".into(),
                "Serif Family".into(),
            ],
            &embedded,
        );
        assert_eq!(
            choices
                .iter()
                .filter(|choice| choice.family == "Geist")
                .count(),
            1
        );
        assert!(
            !choices
                .iter()
                .any(|choice| choice.family == "EmulsionFont-other-document")
        );
        assert_eq!(matching(&choices, "  gEiSt   eMbEdDeD ").len(), 1);
        assert_eq!(
            choices[matching(&choices, "embedded")[0]].family,
            font.alias()
        );
        assert_eq!(matching(&choices, "  ").len(), choices.len());
        assert!(matching(&choices, "unavailable font").is_empty());
        assert_eq!(choices[0].family, "");
    }

    #[test]
    fn embedded_font_samples_render_the_same_face_as_canvas_text() {
        let font = EmbeddedFont::from_bytes(
            include_bytes!("../../../../assets/fonts/GeistMono.ttf").to_vec(),
        )
        .unwrap();
        let preview = font_preview(
            font.alias(),
            "Hello 0123",
            false,
            false,
            [20, 40, 80, 255],
            600,
            64,
            40.,
        );
        let installed = font_preview(
            "Geist Mono",
            "Hello 0123",
            false,
            false,
            [20, 40, 80, 255],
            600,
            64,
            40.,
        );
        let other = font_preview(
            "Geist",
            "Hello 0123",
            false,
            false,
            [20, 40, 80, 255],
            600,
            64,
            40.,
        );
        assert!(
            preview
                .as_bytes(0)
                .unwrap()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0)
        );
        assert_eq!(preview.as_bytes(0), installed.as_bytes(0));
        assert_ne!(preview.as_bytes(0), other.as_bytes(0));
    }

    #[gpui_kit::test]
    fn searchable_fonts_preserve_live_text_range_and_have_one_step_undo(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, false);
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                assert!(editor.try_edit_text_at((40., 50.), 1, window, cx));
                let field = editor.type_tool.field.as_mut().unwrap();
                field.anchor = 6;
                field.cursor = 11;
            })
        });
        let original = spec(&editor, cx);
        click(cx, "type-font");
        cx.update(|window, cx| {
            assert!(window.find("font-picker").visible());
            let state = editor.read(cx).type_tool.font_picker.as_ref().unwrap();
            assert_eq!(state.range, Some(6..11));
            assert_eq!(state.sample, "world");
            assert!(state.search.read(cx).focus_handle(cx).is_focused(window));
        });
        cx.simulate_input("geist mono");
        cx.run_until_parked();
        assert_eq!(
            spec(&editor, cx),
            original,
            "searching never changes canvas text"
        );
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let updated = spec(&editor, cx);
        assert_eq!(updated.style_at(0).font, "Geist");
        assert_eq!(updated.style_at(6).font, "Geist Mono");
        assert_eq!(updated.text, original.text);
        cx.simulate_keystrokes("ctrl-z");
        cx.run_until_parked();
        assert_eq!(spec(&editor, cx), original);
        cx.simulate_keystrokes("ctrl-shift-z");
        cx.run_until_parked();
        assert_eq!(spec(&editor, cx), updated);
    }

    #[gpui_kit::test]
    fn searchable_fonts_support_no_results_escape_and_reopening(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, false);
        let original = spec(&editor, cx);
        click(cx, "photo-character-font");
        click(cx, "photo-character-font");
        cx.update(|_, cx| assert!(editor.read(cx).type_tool.font_picker.is_none()));
        click(cx, "photo-character-font");
        let preview = cx.update(|window, cx| {
            editor
                .read(cx)
                .type_tool
                .font_picker
                .as_ref()
                .unwrap()
                .previews
                .values()
                .find(|image| window.has_image_atlas_entry(image))
                .unwrap()
                .clone()
        });
        cx.simulate_input("no such font 839461");
        cx.run_until_parked();
        cx.update(|window, _| assert!(window.find("font-picker-empty").visible()));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).menu, Some(Menu::Font)));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, _| assert!(!window.has_image_atlas_entry(&preview)));
        assert_eq!(spec(&editor, cx), original);
        click(cx, "photo-character-font");
        cx.simulate_input("geist");
        cx.run_until_parked();
        cx.simulate_keystrokes("down enter");
        cx.run_until_parked();
        assert_eq!(spec(&editor, cx).font, "Geist Mono");
    }

    #[gpui_kit::test]
    fn compact_overflow_font_picker_searches_and_selects_by_pointer(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, false);
        cx.update(|_, cx| {
            cx.global_mut::<crate::app_state::AppSettings>()
                .0
                .compact_chrome = true;
            editor.update(cx, |_, cx| cx.notify());
        });
        cx.run_until_parked();
        click(cx, "tool-options-more");
        click(cx, "type-font");
        cx.simulate_input("geist mono");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("font-picker").visible());
            window.click(("font-row", 0usize), cx);
        });
        cx.run_until_parked();
        assert_eq!(spec(&editor, cx).font, "Geist Mono");
        cx.update(|_, cx| assert!(editor.read(cx).type_tool.font_picker.is_none()));
    }

    #[gpui_kit::test]
    fn design_font_picker_fits_narrow_window_and_rejects_locked_or_stale_targets(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx, true);
        let original = spec(&editor, cx);
        cx.simulate_resize(size(px(600.), px(500.)));
        cx.run_until_parked();
        click(cx, "design-text-font");
        cx.update(|window, _| {
            let bounds = window.find("font-picker").bounds();
            assert!(bounds.left() >= px(0.) && bounds.right() <= px(600.));
            assert!(bounds.top() >= px(0.) && bounds.bottom() <= px(500.));
        });
        cx.simulate_input("geist mono");
        cx.run_until_parked();
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let id = editor.selected.unwrap();
                editor.execute(Command::SetLocked { id, locked: true }, cx);
                editor.choose_font(0, window, cx);
            })
        });
        assert_eq!(spec(&editor, cx), original);
        click(cx, "design-text-font");
        cx.update(|_, cx| assert!(editor.read(cx).type_tool.font_picker.is_none()));
        cx.update(|_, cx| editor.update(cx, |editor, cx| editor.undo(cx)));
        cx.run_until_parked();
        click(cx, "design-text-font");
        cx.simulate_input("geist mono");
        cx.run_until_parked();
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_layer_selection(Vec::new(), None);
                editor.choose_font(0, window, cx);
                let id = editor.editor.doc.nodes[0].id;
                editor.set_layer_selection(vec![id], Some(id));
            })
        });
        assert_eq!(spec(&editor, cx), original);
    }
}
