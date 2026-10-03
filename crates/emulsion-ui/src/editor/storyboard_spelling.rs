//! Spell checking for storyboard captions. Text inputs cannot draw
//! decorations, so each caption with misspelt words shows a small "n
//! spelling issues" button under its input; its menu offers corrections,
//! Add to dictionary (the personal list in Settings › Storyboard) and Ignore
//! (for this session). Edit › Check Spelling… walks every caption on the
//! board. Corrections keep the caption's formatting and are one Undo step.
//!
//! The bundled dictionary loads off the UI thread the first time a caption
//! is checked; until then nothing is marked.
use super::storyboard_find::snippet;
use super::*;
use emulsion_core::project::PageId;
use emulsion_core::storyboard::{CaptionId, Preferences};
use emulsion_io::spell;
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use std::collections::HashSet;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

/// Corrections offered per word.
const SUGGESTIONS: usize = 6;
/// Words one caption's menu lists.
const MENU_WORDS: usize = 8;
/// Issues the Check Spelling dialog lists; the count covers all of them.
const MAX_LISTED: usize = 200;

static LOADING: AtomicBool = AtomicBool::new(false);
static LOADED: AtomicBool = AtomicBool::new(false);

/// Words ignored for the rest of this session, lower case.
#[derive(Default)]
struct IgnoredWords(HashSet<String>);
impl Global for IgnoredWords {}

/// Whether captions are checked: Settings › Storyboard › Check spelling.
fn spelling_on(cx: &App) -> bool {
    cx.try_global::<crate::app_state::AppSettings>()
        .is_none_or(|s| s.0.storyboard.check_spelling)
}

/// The personal dictionary plus the words ignored this session.
fn accepted_words(cx: &App) -> Vec<String> {
    let mut words = cx
        .try_global::<crate::app_state::AppSettings>()
        .map(|s| s.0.storyboard.spelling_words.clone())
        .unwrap_or_default();
    if let Some(ignored) = cx.try_global::<IgnoredWords>() {
        words.extend(ignored.0.iter().cloned());
    }
    words
}

/// The misspelt words of `text`, by byte range.
fn issues(text: &str, cx: &App) -> Vec<Range<usize>> {
    spell::misspellings(text, &accepted_words(cx))
}

/// A word's menu: corrections, then Add to dictionary and Ignore.
#[allow(clippy::too_many_arguments)]
fn word_items(
    mut menu: PopupMenu,
    owner: &WeakEntity<EditorView>,
    panel: PageId,
    field: CaptionId,
    range: Range<usize>,
    word: &str,
    locked: bool,
) -> PopupMenu {
    menu = menu.label(format!("“{word}”"));
    let suggestions = spell::suggestions(word, SUGGESTIONS);
    if suggestions.is_empty() {
        menu = menu.label("No suggestions");
    }
    if !locked {
        for suggestion in suggestions {
            let (owner, range, word) = (owner.clone(), range.clone(), word.to_string());
            menu = menu.item(
                PopupMenuItem::new(suggestion.clone()).on_click(move |_, _, cx| {
                    owner
                        .update(cx, |e, cx| {
                            e.replace_caption_word(
                                panel,
                                field,
                                range.clone(),
                                &word,
                                &suggestion,
                                cx,
                            )
                        })
                        .ok();
                }),
            );
        }
    }
    let (add, ignore) = (owner.clone(), owner.clone());
    let (added, ignored) = (word.to_string(), word.to_string());
    menu.item(
        PopupMenuItem::new("Add to dictionary").on_click(move |_, _, cx| {
            add.update(cx, |e, cx| e.add_spelling_word(&added, cx)).ok();
        }),
    )
    .item(PopupMenuItem::new("Ignore").on_click(move |_, _, cx| {
        ignore
            .update(cx, |e, cx| e.ignore_spelling_word(&ignored, cx))
            .ok();
    }))
}

impl EditorView {
    /// Whether captions can be checked now. The first call starts loading
    /// the dictionary off the UI thread and redraws when it is ready.
    pub(crate) fn spelling_ready(&self, cx: &mut Context<Self>) -> bool {
        if !spelling_on(cx) {
            return false;
        }
        if LOADED.load(Ordering::Acquire) {
            return true;
        }
        if !LOADING.swap(true, Ordering::AcqRel) {
            cx.spawn(async move |this, cx| {
                cx.background_spawn(async {
                    spell::dictionary();
                })
                .await;
                LOADED.store(true, Ordering::Release);
                this.update(cx, |this, cx| {
                    this.notify_sidebar(cx);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        false
    }

    /// The "n spelling issues" button under a caption input, when its text
    /// has misspelt words.
    pub(super) fn caption_spelling(
        &self,
        panel: PageId,
        field: CaptionId,
        text: Option<&str>,
        locked: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let text = text.filter(|t| !t.is_empty())?;
        if !self.spelling_ready(cx) {
            return None;
        }
        let found = issues(text, cx);
        if found.is_empty() {
            return None;
        }
        let count = found.len();
        let mut seen = HashSet::new();
        let words: Vec<(Range<usize>, String)> = found
            .into_iter()
            .map(|r| (r.clone(), text[r].to_string()))
            .filter(|(_, w)| seen.insert(w.clone()))
            .take(MENU_WORDS)
            .collect();
        let owner = cx.weak_entity();
        Some(
            Button::new(SharedString::from(format!("storyboard-spelling-{field}")))
                .label(format!(
                    "{count} spelling issue{}",
                    if count == 1 { "" } else { "s" }
                ))
                .text_color(p.accent)
                .xsmall()
                .ghost()
                .dropdown_menu(move |mut menu, _, _| {
                    menu = menu.scrollable(true).max_h(px(420.));
                    for (index, (range, word)) in words.iter().enumerate() {
                        if index > 0 {
                            menu = menu.separator();
                        }
                        menu = word_items(menu, &owner, panel, field, range.clone(), word, locked);
                    }
                    menu
                })
                .into_any_element(),
        )
    }

    /// Replace one misspelt word, keeping the caption's formatting, as one
    /// Undo step. Refused when the caption no longer has `word` there.
    pub(crate) fn replace_caption_word(
        &mut self,
        panel: PageId,
        field: CaptionId,
        range: Range<usize>,
        word: &str,
        replacement: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        self.edit_board(
            |board| {
                if board.is_locked(panel) {
                    return Err("That panel is locked. Unlock it to correct it.".into());
                }
                let caption = board
                    .panels
                    .get_mut(&panel)
                    .and_then(|p| p.captions.get_mut(&field))
                    .filter(|c| c.text.get(range.clone()) == Some(word))
                    .ok_or("The caption changed; check its spelling again.")?;
                caption.replace_range(range, replacement);
                Ok(())
            },
            cx,
        )
    }

    /// Add `word` to the personal dictionary in Settings › Storyboard.
    pub(crate) fn add_spelling_word(&mut self, word: &str, cx: &mut Context<Self>) {
        let word = word.trim().to_string();
        let Some(settings) = cx.try_global::<crate::app_state::AppSettings>() else {
            return;
        };
        let words = &settings.0.storyboard.spelling_words;
        if words.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
            return;
        }
        if words.len() >= Preferences::MAX_SPELLING_WORDS {
            self.set_status(
                "The personal dictionary is full. Remove words in Settings › Storyboard.",
                true,
                cx,
            );
            return;
        }
        crate::app_state::update_settings(cx, |s| s.storyboard.spelling_words.push(word.clone()));
        self.set_status(format!("Added “{word}” to your dictionary."), false, cx);
        self.notify_sidebar(cx);
    }

    /// Accept `word` until Emulsion quits.
    pub(crate) fn ignore_spelling_word(&mut self, word: &str, cx: &mut Context<Self>) {
        cx.default_global::<IgnoredWords>()
            .0
            .insert(word.to_lowercase());
        self.notify_sidebar(cx);
        cx.notify();
    }

    /// Edit › Check Spelling… for storyboards.
    pub(crate) fn open_spell_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            self.set_status("Check Spelling works on storyboard captions.", false, cx);
            return;
        }
        if !spelling_on(cx) {
            self.set_status(
                "Spell checking is off. Turn it on in Settings › Storyboard.",
                false,
                cx,
            );
            return;
        }
        // Start loading the dictionary; the dialog redraws when it is ready.
        self.spelling_ready(cx);
        let editor = cx.entity();
        let check = cx.new(|cx| SpellCheck {
            _sub: cx.observe(&editor, |_, _, cx| cx.notify()),
            editor: editor.downgrade(),
        });
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Check spelling")
                .width(px(640.))
                .child(check.clone())
                .footer(
                    div().flex().justify_end().child(
                        Button::new("spell-check-close")
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
        });
    }
}

/// The Check Spelling dialog: every misspelt caption word on the board, in
/// page then field order; a row selects its panel, Fix corrects it.
struct SpellCheck {
    editor: WeakEntity<EditorView>,
    _sub: Subscription,
}

impl Render for SpellCheck {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let root = div()
            .id("spell-check")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(px(12.));
        let Some(editor) = self.editor.upgrade() else {
            return root.child("The storyboard was closed.");
        };
        if !LOADED.load(Ordering::Acquire) {
            return root.child(mono("Loading the dictionary…", 10.5, p.muted));
        }
        let accepted = accepted_words(cx);
        let view = editor.read(cx);
        let Some(board) = view.editor.storyboard() else {
            return root.child("This document is not a storyboard.");
        };
        let pages = view.editor.page_list();
        let layout: Vec<PageId> = pages.iter().map(|m| m.id).collect();
        let found = spell::storyboard(board, &layout, &accepted);
        let mut results = div()
            .id("spell-check-results")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_h(px(320.))
            .overflow_y_scroll();
        for (index, (panel, field, range)) in found.iter().take(MAX_LISTED).enumerate() {
            let (panel, field) = (*panel, *field);
            let text = &board.panels[&panel].captions[&field].text;
            let word = text[range.clone()].to_string();
            let (context, at) = snippet(text, range.clone());
            let name = pages
                .iter()
                .find(|m| m.id == panel)
                .map_or(String::new(), |m| m.name.clone());
            let field_name = board
                .captions
                .iter()
                .find(|c| c.id == field)
                .map_or(String::new(), |c| c.name.clone());
            let locked = board.is_locked(panel);
            let owner = self.editor.clone();
            let range = range.clone();
            let select = self.editor.clone();
            results = results.child(
                div()
                    .id(("spell-check-result", index))
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
                    .child(mono(field_name, 10., p.muted).w(px(80.)).flex_none())
                    .child(div().flex_1().min_w_0().truncate().child(
                        StyledText::new(context).with_highlights([(
                            at,
                            HighlightStyle {
                                underline: Some(UnderlineStyle {
                                    thickness: px(1.),
                                    color: Some(p.accent),
                                    wavy: true,
                                }),
                                ..Default::default()
                            },
                        )]),
                    ))
                    .when(locked, |d| d.child(mono("locked", 9.5, p.muted)))
                    .child(
                        Button::new(("spell-check-fix", index))
                            .label("Fix")
                            .xsmall()
                            .outline()
                            .dropdown_menu(move |menu, _, _| {
                                word_items(menu, &owner, panel, field, range.clone(), &word, locked)
                            }),
                    )
                    .on_click(move |_, _, cx| {
                        select.update(cx, |e, cx| e.select_page(panel, cx)).ok();
                    }),
            );
        }
        let summary = match found.len() {
            0 => "No spelling issues in the captions.".to_string(),
            1 => "1 spelling issue".to_string(),
            n => format!("{n} spelling issues"),
        };
        root.child(
            div()
                .id("spell-check-summary")
                .test_support()
                .aria_label(summary.clone())
                .child(mono(summary, 10.5, p.muted)),
        )
        .child(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::open;
    use core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_core::storyboard::Caption;
    use gpui_kit::test::TestWindowExt;

    fn settle(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn misspelt_caption_words_are_corrected_added_and_ignored(cx: &mut TestAppContext) {
        let (ws, cx) = open(cx, Document::new(64, 36));
        cx.simulate_resize(size(px(1600.), px(2400.)));
        let mut project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(80, 40)).unwrap();
        let panel = project.active_page();
        let action = project.storyboard().unwrap().caption("Action").unwrap();
        project
            .edit_storyboard(|b| {
                let mut caption = Caption::from("Mia runns to the windw. Zorbak waits.");
                caption.apply_style(0..3, |s| s.bold = true);
                let data = b.panels.get_mut(&panel).unwrap();
                data.captions.insert(action, caption);
                Ok(())
            })
            .unwrap();
        let e = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(project, "Board".into(), window, cx)
            });
            ws.read(cx).editor.clone().unwrap()
        });
        // The dictionary loads off the UI thread.
        cx.update(|_, cx| e.update(cx, |e, cx| e.spelling_ready(cx)));
        settle(cx);
        assert!(cx.update(|_, cx| e.update(cx, |e, cx| e.spelling_ready(cx))));
        let caption = |cx: &mut VisualTestContext| -> Caption {
            cx.update(|_, cx| {
                e.read(cx).editor.storyboard().unwrap().panels[&panel].captions[&action].clone()
            })
        };
        let found = |cx: &mut VisualTestContext| -> Vec<String> {
            let text = caption(cx).text;
            cx.update(|_, cx| {
                issues(&text, cx)
                    .into_iter()
                    .map(|r| text[r].to_string())
                    .collect()
            })
        };
        assert_eq!(found(cx), ["runns", "windw", "Zorbak"]);
        // The inspector shows how many under the caption.
        settle(cx);
        let indicator = SharedString::from(format!("storyboard-spelling-{action}"));
        cx.update(|window, _| assert!(window.find(indicator.clone()).visible()));
        // A correction is one Undo step and keeps the formatting.
        assert!(cx.update(|_, cx| e.update(cx, |e, cx| {
            e.replace_caption_word(panel, action, 17..22, "windw", "window", cx)
        })));
        let fixed = caption(cx);
        assert_eq!(fixed.text, "Mia runns to the window. Zorbak waits.");
        assert!(fixed.style_at(0).bold);
        // A stale range is refused.
        assert!(!cx.update(|_, cx| e.update(cx, |e, cx| {
            e.replace_caption_word(panel, action, 17..22, "windw", "window", cx)
        })));
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert_eq!(caption(cx).text, "Mia runns to the windw. Zorbak waits.");
        // Add to dictionary saves the word in Settings; Ignore lasts the
        // session.
        cx.update(|_, cx| e.update(cx, |e, cx| e.add_spelling_word("Zorbak", cx)));
        let saved = cx.update(|_, cx| {
            crate::app_state::settings(cx)
                .storyboard
                .spelling_words
                .clone()
        });
        assert_eq!(saved, ["Zorbak"]);
        cx.update(|_, cx| e.update(cx, |e, cx| e.ignore_spelling_word("runns", cx)));
        assert_eq!(found(cx), ["windw"]);
        // Check Spelling lists the board.
        cx.update(|window, cx| e.update(cx, |e, cx| e.open_spell_check(window, cx)));
        settle(cx);
        cx.update(|window, _| {
            assert_eq!(
                window.find("spell-check-summary").label(),
                Some("1 spelling issue")
            );
            window.find(("spell-check-result", 0usize));
        });
        // Turning checking off hides the issues.
        cx.update(|_, cx| {
            crate::app_state::update_settings(cx, |s| s.storyboard.check_spelling = false)
        });
        assert!(!cx.update(|_, cx| e.update(cx, |e, cx| e.spelling_ready(cx))));
    }

    #[gpui_kit::test]
    fn settings_turn_checking_off_and_edit_the_personal_dictionary(cx: &mut TestAppContext) {
        let (_ws, cx) = open(cx, Document::new(32, 32));
        cx.simulate_resize(size(px(1600.), px(4000.)));
        cx.update(|window, _| window.activate_window());
        cx.update(|_, cx| {
            crate::app_state::update_settings(cx, |s| {
                s.storyboard.spelling_words = vec!["Zorbak".into(), "Mialand".into()]
            })
        });
        cx.update(|window, cx| window.dispatch_action(Box::new(crate::actions::ShowSettings), cx));
        settle(cx);
        // Searching brings the rows to the top.
        cx.update(|window, cx| window.click("settings-search", cx));
        cx.simulate_input("spelling");
        settle(cx);
        let saved = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| crate::app_state::settings(cx).storyboard.clone())
        };
        cx.update(|window, cx| window.click("settings-storyboard-spelling", cx));
        settle(cx);
        assert!(!saved(cx).check_spelling);
        cx.update(|window, cx| window.click(("settings-storyboard-spelling-word", 0usize), cx));
        settle(cx);
        assert_eq!(saved(cx).spelling_words, ["Mialand"]);
        cx.update(|window, cx| window.click("settings-storyboard-spelling-clear", cx));
        settle(cx);
        assert!(saved(cx).spelling_words.is_empty());
    }
}
