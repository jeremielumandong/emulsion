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
    /// The landing image, decoded once in the background.
    pub(crate) landing: Option<Arc<RenderImage>>,
    /// Recovery copies left by an earlier session that did not close cleanly.
    pub(crate) recovered: Vec<(PathBuf, u64)>,
    /// Launch splash, until the timer or the first click or key.
    pub(crate) splash: bool,
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
                    weak.update(cx, |this, cx| {
                        this.closing = true;
                        if let Some(ed) = &this.editor {
                            ed.update(cx, |e, _| e.discard_recovery());
                        }
                    })
                    .ok();
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
        // Decoded up front (a few milliseconds) so the splash never shows without it.
        let landing = crate::landing::decode().map(Arc::new);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1400))
                .await;
            this.update(cx, |this, cx| {
                this.splash = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
        Self {
            screen: Screen::Home,
            editor: None,
            recents: recent::load(),
            recovered: find_recovered(),
            thumbs: HashMap::new(),
            thumbs_loading: HashSet::new(),
            busy: None,
            error: None,
            focus,
            last_title: String::new(),
            closing: false,
            settings_inputs: None,
            jev_test: None,
            landing,
            splash: true,
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

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn install(
        &mut self,
        doc: Document,
        graph: Option<emulsion_core::graph::Graph>,
        path: Option<PathBuf>,
        source: Option<PathBuf>,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(old) = &self.editor {
            old.update(cx, |e, _| e.discard_recovery());
        }
        let ed = cx.new(|cx| EditorView::new(doc, graph, path, source, name, cx));
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
                    .background_spawn(async move { emulsion_io::open_full(&p) })
                    .await;
                this.update_in(cx, |this, window, cx| {
                    this.busy = None;
                    match result {
                        Ok(opened) => {
                            let native = emulsion_io::is_native(&path);
                            let (doc, graph, broken) = (opened.doc, opened.graph, opened.history_error);
                            this.recents = recent::push(&path, summary(&doc));
                            this.install(
                                doc,
                                graph,
                                native.then(|| path.clone()),
                                Some(path.clone()),
                                stem(&path),
                                window,
                                cx,
                            );
                            if let (Some(err), Some(ed)) = (broken, &this.editor) {
                                ed.update(cx, |e, cx| {
                                    e.set_status(
                                        format!("The file's history could not be read, so it starts fresh ({err})."),
                                        true,
                                        cx,
                                    )
                                });
                            }
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

    /// Reopen a recovery copy as an untitled document, with its history.
    pub(crate) fn open_recovered(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_discard(window, cx, move |this, window, cx| {
            this.busy = Some("Recovering…".into());
            cx.notify();
            cx.spawn_in(window, async move |this, cx| {
                let p = path.clone();
                let result = cx
                    .background_spawn(async move { emulsion_io::ora::read_full(&p) })
                    .await;
                this.update_in(cx, |this, window, cx| {
                    this.busy = None;
                    match result {
                        Ok(o) => {
                            let name = recovered_name(&path);
                            this.install(o.doc, o.graph, None, None, name, window, cx);
                            let _ = std::fs::remove_file(&path);
                            this.recovered.retain(|(q, _)| *q != path);
                            if let Some(ed) = &this.editor {
                                ed.update(cx, |e, cx| {
                                    e.set_status("Recovered. Save it to keep it.", false, cx)
                                });
                            }
                        }
                        Err(e) => {
                            this.error =
                                Some(format!("Could not recover {}: {e}", path.display()).into());
                            cx.notify();
                        }
                    }
                })
                .ok();
            })
            .detach();
        });
    }

    pub(crate) fn discard_recovered(&mut self, path: &Path, cx: &mut Context<Self>) {
        let _ = std::fs::remove_file(path);
        self.recovered.retain(|(q, _)| q != path);
        cx.notify();
    }

    /// Open the bundled landing image as a new document to edit.
    pub(crate) fn open_landing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(window, cx, |this, window, cx| {
            this.busy = Some("Opening the landing image…".into());
            cx.notify();
            cx.spawn_in(window, async move |this, cx| {
                let result = cx
                    .background_spawn(async {
                        emulsion_io::import::import_bytes(
                            crate::landing::LANDING_NAME,
                            crate::landing::LANDING_JPG,
                        )
                    })
                    .await;
                this.update_in(cx, |this, window, cx| {
                    this.busy = None;
                    match result {
                        Ok(doc) => this.install(
                            doc,
                            None,
                            None,
                            None,
                            crate::landing::LANDING_NAME.into(),
                            window,
                            cx,
                        ),
                        Err(e) => {
                            this.error =
                                Some(format!("Could not open the landing image: {e}").into());
                            cx.notify();
                        }
                    }
                })
                .ok();
            })
            .detach();
        });
    }

    fn splash_view(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = theme::palette(cx);
        let bg: AnyElement = match &self.landing {
            Some(img_) => img(ImageSource::Render(img_.clone()))
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div().size_full().bg(p.chrome).into_any_element(),
        };
        div()
            .id("splash")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(p.chrome)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.splash = false;
                    cx.notify();
                }),
            )
            .child(bg)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(p.chrome.opacity(0.85), 0.),
                        linear_color_stop(p.chrome.opacity(0.0), 0.6),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .left(px(48.))
                    .bottom(px(48.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .child(div().size(px(22.)).bg(p.accent))
                            .child(
                                div()
                                    .text_size(px(40.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(gpui_kit::white())
                                    .child("Emulsion"),
                            ),
                    )
                    .child(mono(
                        format!("{} · every edit, still undoable", env!("CARGO_PKG_VERSION")),
                        11.,
                        gpui_kit::white().opacity(0.8),
                    )),
            )
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
            this.install(doc, None, None, None, "untitled".into(), window, cx);
        });
    }

    fn save(&mut self, save_as: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.editor.clone() else {
            return;
        };
        let (path, dir, name) = {
            let e = ed.read(cx);
            let dir = e
                .source
                .as_ref()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("."));
            (e.editor.path.clone(), dir, e.name.clone())
        };
        match path {
            Some(p) if !save_as => self.write(ed, p, cx),
            _ => {
                let rx = cx.prompt_for_new_path(&dir, Some(&format!("{name}.ora")));
                cx.spawn_in(window, async move |this, cx| {
                    if let Ok(Ok(Some(mut p))) = rx.await {
                        if !emulsion_io::is_native(&p) {
                            p.set_extension("ora");
                        }
                        this.update(cx, |this, cx| this.write(ed, p, cx)).ok();
                    }
                })
                .detach();
            }
        }
    }

    /// Save the document as it is now, with its history, committing first
    /// so the file's newest commit is exactly what was written.
    fn write(&mut self, ed: Entity<EditorView>, path: PathBuf, cx: &mut Context<Self>) {
        let (doc, rev, graph) = ed.update(cx, |e, cx| {
            e.editor.commit("Saved", false);
            e.set_status(format!("Saving {}…", path.display()), false, cx);
            (
                e.editor.doc.clone(),
                e.editor.revision,
                e.editor.graph.clone(),
            )
        });
        cx.spawn(async move |this, cx| {
            let (p, d) = (path.clone(), doc.clone());
            let result = cx
                .background_spawn(async move { emulsion_io::save_full(&d, &graph, &p) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.recents = recent::push(&path, summary(&doc));
                    this.thumbs
                        .remove(&std::fs::canonicalize(&path).unwrap_or(path.clone()));
                    ed.update(cx, |e, cx| {
                        e.editor.mark_saved(path.clone(), rev);
                        e.discard_recovery();
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
            let file = p
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            ed.update(cx, |e, cx| {
                e.editor.commit(format!("Exported {file}"), false);
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
            if let Some(ed) = &this.editor {
                ed.update(cx, |e, _| e.discard_recovery());
            }
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
                            theme::set_dark(false, cx);
                            cx.refresh_windows();
                        })),
                    )
                    .child(
                        theme_btn("dark", "☾", p.dark).on_click(cx.listener(|_, _, _, cx| {
                            theme::set_dark(true, cx);
                            cx.refresh_windows();
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
                this.with_editor(cx, |e, cx| {
                    if !e.tool_cancel(cx) {
                        e.rotate(0.0, cx)
                    }
                })
            }))
            .on_action(cx.listener(|this, _: &ToggleRulers, _, cx| {
                this.with_editor(cx, |e, cx| e.toggle_rulers(cx))
            }))
            .on_action(cx.listener(|this, _: &DeleteNode, _, cx| {
                this.with_editor(cx, |e, cx| {
                    if !(e.tool == crate::editor::Tool::Pen && e.pen_delete(cx)) {
                        e.delete_selected(cx)
                    }
                })
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
            .on_action(cx.listener(|this, _: &ToolPen, _, cx| {
                this.with_editor(cx, |e, cx| e.set_tool(crate::editor::Tool::Pen, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolType, _, cx| {
                this.with_editor(cx, |e, cx| e.set_tool(crate::editor::Tool::Type, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolHand, _, cx| {
                this.with_editor(cx, |e, cx| e.set_tool(crate::editor::Tool::Hand, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolMove, _, cx| {
                this.with_editor(cx, |e, cx| e.set_tool(crate::editor::Tool::Move, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolMarquee, _, cx| {
                this.with_editor(cx, |e, cx| {
                    let next = if e.tool == crate::editor::Tool::Select
                        && e.select_shape() == crate::editor::SelectShape::Rect
                    {
                        crate::editor::SelectShape::Ellipse
                    } else {
                        crate::editor::SelectShape::Rect
                    };
                    e.set_select(next, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &ToolLasso, _, cx| {
                this.with_editor(cx, |e, cx| {
                    let next = if e.tool == crate::editor::Tool::Select
                        && e.select_shape() == crate::editor::SelectShape::Lasso
                    {
                        crate::editor::SelectShape::Polygon
                    } else {
                        crate::editor::SelectShape::Lasso
                    };
                    e.set_select(next, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &ToolWand, _, cx| {
                this.with_editor(cx, |e, cx| {
                    e.set_select(crate::editor::SelectShape::Wand, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &ToolBrush, _, cx| {
                this.with_editor(cx, |e, cx| e.set_paint(crate::editor::PaintKind::Brush, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolEraser, _, cx| {
                this.with_editor(cx, |e, cx| {
                    e.set_paint(crate::editor::PaintKind::Eraser, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &ToolBucket, _, cx| {
                this.with_editor(cx, |e, cx| {
                    e.set_paint(crate::editor::PaintKind::Bucket, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &ToolGradient, _, cx| {
                this.with_editor(cx, |e, cx| {
                    e.set_paint(crate::editor::PaintKind::Gradient, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &ToolHeal, _, cx| {
                this.with_editor(cx, |e, cx| e.set_tool(crate::editor::Tool::Heal, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolClone, _, cx| {
                this.with_editor(cx, |e, cx| e.set_tool(crate::editor::Tool::Clone, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolCrop, _, cx| {
                this.with_editor(cx, |e, cx| e.set_tool(crate::editor::Tool::Crop, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolShape, _, cx| {
                this.with_editor(cx, |e, cx| e.set_tool(crate::editor::Tool::Shape, cx))
            }))
            .on_action(cx.listener(|this, _: &SwapColors, _, cx| {
                this.with_editor(cx, |e, cx| e.swap_colors(cx))
            }))
            .on_action(cx.listener(|this, _: &DefaultColors, _, cx| {
                this.with_editor(cx, |e, cx| e.default_colors(cx))
            }))
            .on_action(cx.listener(|this, _: &BrushSmaller, _, cx| {
                this.with_editor(cx, |e, cx| e.brush_size(false, cx))
            }))
            .on_action(cx.listener(|this, _: &BrushLarger, _, cx| {
                this.with_editor(cx, |e, cx| e.brush_size(true, cx))
            }))
            .on_action(cx.listener(|this, _: &CommitTool, _, cx| {
                this.with_editor(cx, |e, cx| e.tool_commit(cx))
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.with_editor(cx, |e, cx| e.select_all(cx))
            }))
            .on_action(
                cx.listener(|this, _: &Deselect, _, cx| {
                    this.with_editor(cx, |e, cx| e.deselect(cx))
                }),
            )
            .on_action(cx.listener(|this, _: &InvertSelection, _, cx| {
                this.with_editor(cx, |e, cx| e.invert_selection(cx))
            }))
            .on_action(cx.listener(|this, _: &FillSelection, _, cx| {
                this.with_editor(cx, |e, cx| e.fill_selection(cx))
            }))
            .on_action(cx.listener(|this, _: &ContentAwareFill, _, cx| {
                this.with_editor(cx, |e, cx| e.content_aware_fill(cx))
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
            .relative()
            .on_key_down(cx.listener(|this, _: &KeyDownEvent, _, cx| {
                if this.splash {
                    this.splash = false;
                    cx.notify();
                }
            }))
            .child(top)
            .children(banner)
            .child(body)
            .when(self.splash, |d| d.child(self.splash_view(cx)))
    }
}

/// Recovery copies from other sessions, newest first.
fn find_recovered() -> Vec<(PathBuf, u64)> {
    let me = format!("-{}-", std::process::id());
    let Ok(dir) = std::fs::read_dir(crate::editor::recovery_dir()) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, u64)> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "ora")
                && !p
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().contains(&me))
        })
        .map(|p| {
            let t = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            (p, t)
        })
        .collect();
    out.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    out
}

/// "campaign_hero" from "campaign_hero-1234-1700000000.ora".
pub(crate) fn recovered_name(p: &Path) -> String {
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut parts: Vec<&str> = stem.rsplitn(3, '-').collect();
    parts.reverse();
    if parts.len() == 3 {
        parts[0].to_string()
    } else {
        stem
    }
}
