//! Find and replace across storyboard captions (Edit › Find and Replace
//! Captions…). Results follow the board live; Replace All is one Undo step
//! and skips locked panels.
use super::*;
use emulsion_core::project::PageId;
use emulsion_core::storyboard::{CaptionId, FindOptions};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::ops::Range;

/// Results listed at once; the count still covers every match.
const MAX_RESULTS: usize = 200;
const CONTEXT_CHARS: usize = 28;

pub(crate) struct CaptionFind {
    editor: WeakEntity<EditorView>,
    query: Entity<InputState>,
    replacement: Entity<InputState>,
    options: FindOptions,
    field: Option<CaptionId>,
    message: Option<String>,
    _subs: Vec<Subscription>,
}

/// A match in context: the text around it on one line, and where the match
/// sits in that text.
fn snippet(text: &str, range: Range<usize>) -> (String, Range<usize>) {
    let flat = |s: &str| s.replace(['\n', '\t'], " ");
    let before: Vec<char> = text[..range.start].chars().collect();
    let skip = before.len().saturating_sub(CONTEXT_CHARS);
    let mut out = if skip > 0 {
        "…".to_string()
    } else {
        String::new()
    };
    out.push_str(&flat(&before[skip..].iter().collect::<String>()));
    let start = out.len();
    out.push_str(&flat(&text[range.clone()]));
    let end = out.len();
    let mut after = text[range.end..].chars();
    out.push_str(&flat(
        &after.by_ref().take(CONTEXT_CHARS).collect::<String>(),
    ));
    if after.next().is_some() {
        out.push('…');
    }
    (out, start..end)
}

impl CaptionFind {
    fn new(editor: Entity<EditorView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Find in captions"));
        let replacement = cx.new(|cx| InputState::new(window, cx).placeholder("Replace with"));
        let subs = vec![
            cx.subscribe(&query, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.message = None;
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &replacement,
                window,
                |this, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.replace_all(cx);
                    }
                },
            ),
            // Undo, edits and panel changes refresh the results.
            cx.observe(&editor, |_, _, cx| cx.notify()),
        ];
        Self {
            editor: editor.downgrade(),
            query,
            replacement,
            options: FindOptions::default(),
            field: None,
            message: None,
            _subs: subs,
        }
    }

    fn replace_all(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().to_string();
        let replacement = self.replacement.read(cx).value().to_string();
        let (field, options) = (self.field, self.options);
        let result = self.editor.update(cx, |editor, cx| {
            editor.replace_captions(&query, &replacement, field, options, cx)
        });
        self.message = Some(match result {
            Ok(Ok((0, 0))) => "No matches to replace.".into(),
            Ok(Ok((replaced, skipped))) => {
                let mut message = format!(
                    "Replaced {replaced} match{}.",
                    if replaced == 1 { "" } else { "es" }
                );
                if skipped > 0 {
                    message.push_str(&format!(
                        " Skipped {skipped} locked panel{}.",
                        if skipped == 1 { "" } else { "s" }
                    ));
                }
                message
            }
            Ok(Err(error)) => error,
            Err(_) => "The storyboard was closed.".into(),
        });
        cx.notify();
    }
}

impl Render for CaptionFind {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let query = self.query.read(cx).value().to_string();
        let root = div()
            .id("caption-find")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(px(12.));
        let Some(editor) = self.editor.upgrade() else {
            return root.child("The storyboard was closed.");
        };
        let view = editor.read(cx);
        let Some(board) = view.editor.storyboard() else {
            return root.child("This document is not a storyboard.");
        };
        let pages = view.editor.page_list();
        let layout: Vec<PageId> = pages.iter().map(|m| m.id).collect();
        let matches = board.find(&layout, &query, self.field, self.options);
        let fields: Vec<(CaptionId, String)> = board
            .captions
            .iter()
            .map(|c| (c.id, c.name.clone()))
            .collect();
        let field_name = |id: CaptionId| {
            fields
                .iter()
                .find(|(f, _)| *f == id)
                .map_or(String::new(), |(_, n)| n.clone())
        };
        let mut panels: Vec<PageId> = matches.iter().map(|m| m.0).collect();
        panels.dedup();
        let owner = cx.weak_entity();
        let field_menu = Button::new("caption-find-field")
            .label(self.field.map_or("All fields".into(), field_name))
            .small()
            .outline()
            .dropdown_menu({
                let fields = fields.clone();
                let current = self.field;
                move |mut menu, _, _| {
                    let all = std::iter::once((None, "All fields".to_string()))
                        .chain(fields.iter().map(|(id, name)| (Some(*id), name.clone())));
                    for (field, name) in all {
                        let owner = owner.clone();
                        menu =
                            menu.item(PopupMenuItem::new(name).checked(field == current).on_click(
                                move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.field = field;
                                            this.message = None;
                                            cx.notify();
                                        })
                                        .ok();
                                },
                            ));
                    }
                    menu
                }
            });
        let mut results = div()
            .id("caption-find-results")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_h(px(280.))
            .overflow_y_scroll();
        for (index, (panel, field, range)) in matches.iter().take(MAX_RESULTS).enumerate() {
            let (panel, field) = (*panel, *field);
            let Some(caption) = board
                .panels
                .get(&panel)
                .and_then(|p| p.captions.get(&field))
            else {
                continue;
            };
            let (text, at) = snippet(&caption.text, range.clone());
            let name = pages
                .iter()
                .find(|m| m.id == panel)
                .map_or(String::new(), |m| m.name.clone());
            let locked = board.is_locked(panel);
            let editor = self.editor.clone();
            results = results.child(
                div()
                    .id(("caption-find-result", index))
                    .test_support()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(6.))
                    .py(px(3.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .when(panel == view.editor.active_page(), |d| d.bg(p.soft_bg))
                    .hover(|d| d.bg(p.soft_bg))
                    .child(div().w(px(96.)).flex_none().truncate().child(name))
                    .child(mono(field_name(field), 10., p.muted).w(px(80.)).flex_none())
                    .child(div().flex_1().min_w_0().truncate().child(
                        StyledText::new(text).with_highlights([(
                            at,
                            HighlightStyle {
                                font_weight: Some(FontWeight::BOLD),
                                background_color: Some(p.accent.opacity(0.25)),
                                ..Default::default()
                            },
                        )]),
                    ))
                    .when(locked, |d| d.child(mono("locked", 9.5, p.muted)))
                    .on_click(move |_, _, cx| {
                        editor.update(cx, |e, cx| e.select_page(panel, cx)).ok();
                    }),
            );
        }
        let summary = if query.is_empty() {
            "Type to search every caption.".to_string()
        } else {
            format!(
                "{} match{} in {} panel{}",
                matches.len(),
                if matches.len() == 1 { "" } else { "es" },
                panels.len(),
                if panels.len() == 1 { "" } else { "s" }
            )
        };
        let toggle =
            |id: &'static str, text: &'static str, on: bool| chip(id, text, on, &p).test_support();
        root.child(
            div()
                .id("caption-find-query")
                .test_support()
                .child(Input::new(&self.query)),
        )
        .child(
            div()
                .id("caption-find-replacement")
                .test_support()
                .child(Input::new(&self.replacement)),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .child(
                    toggle("caption-find-case", "Match case", self.options.match_case).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.options.match_case = !this.options.match_case;
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    toggle("caption-find-word", "Whole word", self.options.whole_word).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.options.whole_word = !this.options.whole_word;
                            cx.notify();
                        }),
                    ),
                )
                .child(field_menu)
                .child(div().flex_1())
                .child(
                    Button::new("caption-find-replace-all")
                        .label("Replace All")
                        .small()
                        .primary()
                        .on_click(cx.listener(|this, _, _, cx| this.replace_all(cx))),
                ),
        )
        .child(mono(summary, 10., p.muted))
        .child(results)
        .children(self.message.clone().map(|message| {
            div()
                .id("caption-find-message")
                .test_support()
                .aria_label(message.clone())
                .text_color(p.ink)
                .child(message)
        }))
    }
}

impl EditorView {
    /// Edit › Find and Replace Captions… for storyboards.
    pub(crate) fn open_caption_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            self.set_status("Find and replace works on storyboard captions.", false, cx);
            return;
        }
        let editor = cx.entity();
        let find = cx.new(|cx| CaptionFind::new(editor, window, cx));
        let query = find.read(cx).query.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Find and replace captions")
                .width(px(640.))
                .child(find.clone())
                .footer(
                    div().flex().justify_end().child(
                        Button::new("caption-find-close")
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
        });
        query.update(cx, |query, cx| query.focus(window, cx));
    }

    /// Replace every match on unlocked panels as one Undo step. Returns the
    /// replacements made and the locked panels skipped.
    pub(crate) fn replace_captions(
        &mut self,
        query: &str,
        replacement: &str,
        field: Option<CaptionId>,
        options: FindOptions,
        cx: &mut Context<Self>,
    ) -> Result<(usize, usize), String> {
        if query.is_empty() {
            return Err("Enter text to find.".into());
        }
        let layout: Vec<PageId> = self.editor.page_list().iter().map(|m| m.id).collect();
        let mut counts = (0, 0);
        if self.edit_board(
            |board| {
                counts = board.replace_all(&layout, query, replacement, field, options);
                Ok(())
            },
            cx,
        ) {
            Ok(counts)
        } else {
            Err(self.storyboard_ui.error().unwrap_or_default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::snippet;

    #[test]
    fn storyboard_find_snippets_keep_the_match_in_context() {
        let (text, at) = snippet("Mia waves\nat Tom", 13..16);
        assert_eq!(text, "Mia waves at Tom");
        assert_eq!(&text[at], "Tom");
        let long = "x".repeat(40) + "hit" + &"y".repeat(40);
        let (text, at) = snippet(&long, 40..43);
        assert!(text.starts_with('…') && text.ends_with('…'));
        assert_eq!(&text[at], "hit");
    }
}
