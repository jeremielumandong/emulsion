//! The window's root: top bar, screen switching, and every file operation.

use crate::actions::*;
use crate::editor::EditorView;
use crate::theme::{self, MONO_FONT, dim};
use crate::widgets::mono;
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Node, NodeKind};
use emulsion_io::recent::{self, Recent};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Home,
    Editor,
    Settings,
}

pub struct Workspace {
    pub screen: Screen,
    pub editor: Option<Entity<EditorView>>,
    pub recents: Vec<Recent>,
    pub thumbs: HashMap<PathBuf, Arc<RenderImage>>,
    pub thumbs_loading: HashSet<PathBuf>,
    pub busy: Option<SharedString>,
    pub error: Option<SharedString>,
    focus: FocusHandle,
    last_title: String,
    closing: bool,
    pub(crate) settings_inputs: Option<(
        Entity<gpui_kit::component::input::InputState>,
        Entity<gpui_kit::component::input::InputState>,
    )>,
    pub(crate) jev_test: Option<(SharedString, bool)>,
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "untitled".into())
}

fn summary(doc: &Document) -> String {
    let n = doc.nodes.len();
    format!("{n} node{}", if n == 1 { "" } else { "s" })
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(this) = weak.upgrade() else {
                return true;
            };
            let (modified, closing) = {
                let ws = this.read(cx);
                (
                    ws.editor
                        .as_ref()
                        .is_some_and(|e| e.read(cx).editor.is_modified()),
                    ws.closing,
                )
            };
            if closing || !modified {
                return true;
            }
            let answer = window.prompt(
                PromptLevel::Warning,
                "Close without saving?",
                Some("Your unsaved changes will be lost."),
                &["Close", "Cancel"],
                cx,
            );
            let weak = weak.clone();
            let handle = window.window_handle();
            cx.spawn(async move |cx| {
                if answer.await == Ok(0) {
                    weak.update(cx, |this, _| this.closing = true).ok();
                    handle
                        .update(cx, |_, window, _| window.remove_window())
                        .ok();
                }
            })
            .detach();
            false
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            screen: Screen::Home,
            editor: None,
            recents: recent::load(),
            thumbs: HashMap::new(),
            thumbs_loading: HashSet::new(),
            busy: None,
            error: None,
            focus,
            last_title: String::new(),
            closing: false,
            settings_inputs: None,
            jev_test: None,
        }
    }

    fn modified(&self, cx: &App) -> bool {
        self.editor
            .as_ref()
            .is_some_and(|e| e.read(cx).editor.is_modified())
    }

    /// Run `then` now, or after the user agrees to drop unsaved changes.
    fn confirm_discard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if !self.modified(cx) {
            then(self, window, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "Discard unsaved changes?",
            Some("The open document has changes that are not saved."),
            &["Discard", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                this.update_in(cx, |this, window, cx| then(this, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub(crate) fn install(
        &mut self,
        doc: Document,
        path: Option<PathBuf>,
        source: Option<PathBuf>,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ed = cx.new(|cx| EditorView::new(doc, path, source, name, cx));
        let focus = ed.read(cx).focus.clone();
        self.editor = Some(ed);
        self.screen = Screen::Editor;
        self.error = None;
        focus.focus(window, cx);
        cx.notify();
    }

    pub fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(window, cx, move |this, window, cx| {
            this.busy = Some(format!("Opening {}…", path.display()).into());
            this.error = None;
            cx.notify();
            cx.spawn_in(window, async move |this, cx| {
                let p = path.clone();
                let result = cx
                    .background_spawn(async move { emulsion_io::open(&p) })
                    .await;
                this.update_in(cx, |this, window, cx| {
                    this.busy = None;
                    match result {
                        Ok(doc) => {
                            let native = emulsion_io::is_native(&path);
                            this.recents = recent::push(&path, summary(&doc));
                            this.install(
                                doc,
                                native.then(|| path.clone()),
                                Some(path.clone()),
                                stem(&path),
                                window,
                                cx,
                            );
                        }
                        Err(e) => {
                            this.error =
                                Some(format!("Could not open {}: {e}", path.display()).into());
                            cx.notify();
                        }
                    }
                })
                .ok();
            })
            .detach();
        });
    }

    fn prompt_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.into_iter().next()
            {
                this.update_in(cx, |this, window, cx| this.open_path(p, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub fn new_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(window, cx, |this, window, cx| {
            let mut doc = Document::new(1920, 1080);
            let bg = Node::new(
                0,
                "Background",
                NodeKind::Fill {
                    rgba: [255, 255, 255, 255],
                },
            );
            let _ = Command::AddNode {
                node: Box::new(bg),
                slot: Slot::TOP,
            }
            .apply(&mut doc);
            this.install(doc, None, None, "untitled".into(), window, cx);
        });
    }

    fn save(&mut self, save_as: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.editor.clone() else {
            return;
        };
        let (path, doc, rev, dir, name) = {
            let e = ed.read(cx);
            let dir = e
                .source
                .as_ref()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("."));
            (
                e.editor.path.clone(),
                e.editor.doc.clone(),
                e.editor.revision,
                dir,
                e.name.clone(),
            )
        };
        match path {
            Some(p) if !save_as => self.write(ed, p, doc, rev, cx),
            _ => {
                let rx = cx.prompt_for_new_path(&dir, Some(&format!("{name}.ora")));
                cx.spawn_in(window, async move |this, cx| {
                    if let Ok(Ok(Some(mut p))) = rx.await {
                        if !emulsion_io::is_native(&p) {
                            p.set_extension("ora");
                        }
                        this.update(cx, |this, cx| this.write(ed, p, doc, rev, cx))
                            .ok();
                    }
                })
                .detach();
            }
        }
    }

    fn write(
        &mut self,
        ed: Entity<EditorView>,
        path: PathBuf,
        doc: Document,
        rev: u64,
        cx: &mut Context<Self>,
    ) {
        ed.update(cx, |e, cx| {
            e.set_status(format!("Saving {}…", path.display()), false, cx)
        });
        cx.spawn(async move |this, cx| {
            let (p, d) = (path.clone(), doc.clone());
            let result = cx
                .background_spawn(async move { emulsion_io::save(&d, &p) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.recents = recent::push(&path, summary(&doc));
                    this.thumbs
                        .remove(&std::fs::canonicalize(&path).unwrap_or(path.clone()));
                    ed.update(cx, |e, cx| {
                        e.editor.mark_saved(path.clone(), rev, doc);
                        e.name = stem(&path);
                        e.source = Some(path.clone());
                        e.set_status(format!("Saved {}", path.display()), false, cx);
                    });
                }
                Err(err) => ed.update(cx, |e, cx| {
                    e.set_status(format!("Save failed: {err}"), true, cx)
                }),
            })
            .ok();
        })
        .detach();
    }

    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.editor.clone() else {
            return;
        };
        let (doc, dir, name) = {
            let e = ed.read(cx);
            let dir = e
                .source
                .as_ref()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("."));
            (e.editor.doc.clone(), dir, e.name.clone())
        };
        let rx = cx.prompt_for_new_path(&dir, Some(&format!("{name}.png")));
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(mut p))) = rx.await else {
                return;
            };
            if emulsion_io::ExportFormat::from_path(&p).is_none() {
                p.set_extension("png");
            }
            ed.update(cx, |e, cx| {
                e.set_status(format!("Exporting {}…", p.display()), false, cx)
            });
            let (q, d) = (p.clone(), doc.clone());
            let opts = emulsion_io::ExportOptions::for_doc(&doc);
            let result = cx
                .background_spawn(async move { emulsion_io::export(&d, &q, opts) })
                .await;
            ed.update(cx, |e, cx| match result {
                Ok(()) => e.set_status(format!("Exported {}", p.display()), false, cx),
                Err(err) => e.set_status(format!("Export failed: {err}"), true, cx),
            });
        })
        .detach();
    }

    fn quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(window, cx, |this, _, cx| {
            this.closing = true;
            cx.quit();
        });
    }

    fn with_editor(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut EditorView, &mut Context<EditorView>),
    ) {
        if let Some(e) = &self.editor {
            e.update(cx, f);
        }
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = theme::palette(cx);
        let has_editor = self.editor.is_some();
        let tab = |id: &'static str, text: &'static str, on: bool, enabled: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .px(px(15.))
                .border_l_1()
                .border_color(p.chrome_line)
                .font_family(MONO_FONT)
                .text_size(px(10.))
                .bg(if on { p.accent } else { transparent_black() })
                .text_color(if on {
                    gpui_kit::white()
                } else if enabled {
                    p.nav_fg
                } else {
                    p.nav_fg.opacity(0.4)
                })
                .when(enabled, |d| d.cursor_pointer())
                .child(text)
        };
        let theme_btn = |id: &'static str, glyph: &'static str, on: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .w(px(30.))
                .h(px(26.))
                .border_1()
                .border_color(if on { p.chrome_fg } else { p.chrome_line })
                .bg(if on { p.chrome_fg } else { transparent_black() })
                .text_color(if on { p.chrome } else { p.nav_fg })
                .font_family(MONO_FONT)
                .text_size(px(11.))
                .cursor_pointer()
                .child(glyph)
        };
        div()
            .flex()
            .flex_none()
            .items_stretch()
            .h(dim::TOP_BAR_H)
            .bg(p.chrome)
            .text_color(p.chrome_fg)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(11.))
                    .px(px(18.))
                    .child(div().size(px(14.)).bg(p.accent))
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Emulsion"),
                    ),
            )
            .child(
                tab(
                    "tab-editor",
                    "EDITOR",
                    self.screen == Screen::Editor,
                    has_editor,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if has_editor {
                        this.screen = Screen::Editor;
                        cx.notify();
                    }
                })),
            )
            .child(
                tab("tab-home", "HOME", self.screen == Screen::Home, true).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.screen = Screen::Home;
                        cx.notify();
                    },
                )),
            )
            .child(
                tab(
                    "tab-settings",
                    "SETTINGS",
                    self.screen == Screen::Settings,
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.screen = Screen::Settings;
                    cx.notify();
                })),
            )
            .child(div().flex_1().border_l_1().border_color(p.chrome_line))
            .child(
                div()
                    .flex()
                    .items_center()
                    .px(px(14.))
                    .border_l_1()
                    .border_color(p.chrome_line)
                    .child(
                        theme_btn("light", "☀", !p.dark).on_click(cx.listener(|_, _, _, cx| {
                            if theme::palette(cx).dark {
                                theme::toggle(cx);
                                cx.refresh_windows();
                            }
                        })),
                    )
                    .child(
                        theme_btn("dark", "☾", p.dark).on_click(cx.listener(|_, _, _, cx| {
                            if !theme::palette(cx).dark {
                                theme::toggle(cx);
                                cx.refresh_windows();
                            }
                        })),
                    ),
            )
    }

    fn banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let p = theme::palette(cx);
        let (msg, err) = match (&self.error, &self.busy) {
            (Some(e), _) => (e.clone(), true),
            (None, Some(b)) => (b.clone(), false),
            _ => return None,
        };
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(10.))
                .px(px(16.))
                .py(px(8.))
                .bg(if err { p.accent } else { p.ink })
                .text_color(gpui_kit::white())
                .child(mono(msg, 11., gpui_kit::white()).flex_1())
                .when(err, |d| {
                    d.child(
                        div()
                            .id("dismiss")
                            .cursor_pointer()
                            .child(mono("dismiss", 10., gpui_kit::white()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.error = None;
                                cx.notify();
                            })),
                    )
                }),
        )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let title = match &self.editor {
            Some(e) => {
                let e = e.read(cx);
                format!(
                    "{}{} — Emulsion",
                    e.name,
                    if e.editor.is_modified() { " •" } else { "" }
                )
            }
            None => "Emulsion".into(),
        };
        if title != self.last_title {
            window.set_window_title(&title);
            self.last_title = title;
        }
        let top = self.top_bar(cx);
        let banner = self.banner(cx);
        let body: AnyElement = match (self.screen, &self.editor) {
            (Screen::Editor, Some(e)) => e.clone().into_any_element(),
            (Screen::Settings, _) => self.settings_screen(window, cx).into_any_element(),
            _ => self.home(window, cx).into_any_element(),
        };
        div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .bg(p.paper)
            .text_color(p.ink)
            .font_family(theme::UI_FONT)
            .text_size(px(13.))
            .on_action(
                cx.listener(|this, _: &NewDocument, window, cx| this.new_document(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Open, window, cx| this.prompt_open(window, cx)))
            .on_action(cx.listener(|this, _: &Save, window, cx| this.save(false, window, cx)))
            .on_action(cx.listener(|this, _: &SaveAs, window, cx| this.save(true, window, cx)))
            .on_action(cx.listener(|this, _: &Export, window, cx| this.export(window, cx)))
            .on_action(cx.listener(|this, _: &Quit, window, cx| this.quit(window, cx)))
            .on_action(cx.listener(|this, _: &ShowHome, _, cx| {
                this.screen = Screen::Home;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowEditor, _, cx| {
                if this.editor.is_some() {
                    this.screen = Screen::Editor;
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|_, _: &ToggleTheme, _, cx| {
                theme::toggle(cx);
                cx.refresh_windows();
            }))
            .on_action(
                cx.listener(|this, _: &Undo, _, cx| this.with_editor(cx, |e, cx| e.undo(cx))),
            )
            .on_action(
                cx.listener(|this, _: &Redo, _, cx| this.with_editor(cx, |e, cx| e.redo(cx))),
            )
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| {
                this.with_editor(cx, |e, cx| e.zoom_step(true, cx))
            }))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| {
                this.with_editor(cx, |e, cx| e.zoom_step(false, cx))
            }))
            .on_action(
                cx.listener(|this, _: &ZoomFit, _, cx| {
                    this.with_editor(cx, |e, cx| e.zoom_fit(cx))
                }),
            )
            .on_action(
                cx.listener(|this, _: &Zoom100, _, cx| {
                    this.with_editor(cx, |e, cx| e.zoom_100(cx))
                }),
            )
            .on_action(cx.listener(|this, _: &RotateCw, _, cx| {
                this.with_editor(cx, |e, cx| e.rotate(15.0, cx))
            }))
            .on_action(cx.listener(|this, _: &RotateCcw, _, cx| {
                this.with_editor(cx, |e, cx| e.rotate(-15.0, cx))
            }))
            .on_action(cx.listener(|this, _: &ResetRotation, _, cx| {
                this.with_editor(cx, |e, cx| e.rotate(0.0, cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleRulers, _, cx| {
                this.with_editor(cx, |e, cx| e.toggle_rulers(cx))
            }))
            .on_action(cx.listener(|this, _: &DeleteNode, _, cx| {
                this.with_editor(cx, |e, cx| e.delete_selected(cx))
            }))
            .on_action(cx.listener(|this, _: &DuplicateNode, _, cx| {
                this.with_editor(cx, |e, cx| e.duplicate_selected(cx))
            }))
            .on_action(cx.listener(|this, _: &GroupNodes, _, cx| {
                this.with_editor(cx, |e, cx| e.group_selected(cx))
            }))
            .on_action(cx.listener(|this, _: &Ungroup, _, cx| {
                this.with_editor(cx, |e, cx| e.ungroup_selected(cx))
            }))
            .on_action(cx.listener(|this, _: &MoveNodeUp, _, cx| {
                this.with_editor(cx, |e, cx| e.shift_selected(true, cx))
            }))
            .on_action(cx.listener(|this, _: &MoveNodeDown, _, cx| {
                this.with_editor(cx, |e, cx| e.shift_selected(false, cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleNodeVisible, _, cx| {
                this.with_editor(cx, |e, cx| e.toggle_selected_visible(cx))
            }))
            .on_action(cx.listener(|this, _: &Ask, window, cx| {
                if let Some(e) = this.editor.clone() {
                    this.screen = Screen::Editor;
                    e.update(cx, |e, cx| e.open_ask(window, cx));
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ShowSettings, _, cx| {
                this.screen = Screen::Settings;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Suggestion1, _, cx| {
                this.with_editor(cx, |e, cx| e.accept_suggestion(0, cx))
            }))
            .on_action(cx.listener(|this, _: &Suggestion2, _, cx| {
                this.with_editor(cx, |e, cx| e.accept_suggestion(1, cx))
            }))
            .on_action(cx.listener(|this, _: &Suggestion3, _, cx| {
                this.with_editor(cx, |e, cx| e.accept_suggestion(2, cx))
            }))
            .on_action(cx.listener(|this, _: &Suggestion4, _, cx| {
                this.with_editor(cx, |e, cx| e.accept_suggestion(3, cx))
            }))
            .child(top)
            .children(banner)
            .child(body)
    }
}
