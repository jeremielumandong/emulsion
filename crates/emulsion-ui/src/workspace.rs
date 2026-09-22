//! The window's root: top bar, screen switching, and every file operation.

use crate::actions::*;
use crate::editor::{EditorView, SaveTarget};
use crate::theme::{self, MONO_FONT, dim};
use crate::widgets::mono;
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Node, NodeKind};
use emulsion_io::recent::{self, Recent};
use gpui_kit::component::WindowExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{Selectable, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod raw_sync;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Home,
    Editor,
    Settings,
    Batch,
    /// Licence and attribution.
    About,
}

pub struct Workspace {
    pub screen: Screen,
    /// The active document; always one of `tabs` when open.
    pub editor: Option<Entity<EditorView>>,
    /// Every open document, in tab order.
    pub tabs: Vec<Entity<EditorView>>,
    pub recents: Vec<Recent>,
    pub(crate) home_state: crate::home::HomeState,
    pub(crate) thumbs: HashMap<PathBuf, crate::home::GalleryThumbnail>,
    pub(crate) thumbs_loading: HashMap<PathBuf, u64>,
    pub(crate) thumb_generation: u64,
    pub busy: Option<SharedString>,
    pub error: Option<SharedString>,
    focus: FocusHandle,
    last_title: String,
    dialog_root_watch: Option<Subscription>,
    closing: bool,
    pub(crate) settings_inputs: Option<(
        Entity<gpui_kit::component::input::InputState>,
        Entity<gpui_kit::component::input::InputState>,
    )>,
    pub(crate) jev_test: Option<(SharedString, bool)>,
    pub(crate) image_inputs: Option<crate::settings_screen::ImageInputs>,
    pub(crate) image_test: Option<(SharedString, bool)>,
    pub(crate) keymap_note: Option<SharedString>,
    /// Filesystem facts the Settings screen shows, refreshed at most every
    /// couple of seconds instead of on every frame.
    pub(crate) probe: Option<(std::time::Instant, crate::settings_screen::Probe)>,
    /// About screen: the full crate list is long, so it unfolds on request.
    pub(crate) about_all_crates: bool,
    pub(crate) model_jobs: crate::settings_models::ModelJobs,
    pub(crate) batch: crate::batch::BatchState,
    /// The landing image, decoded once in the background.
    pub(crate) landing: Option<crate::landing::LandingImages>,
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
    format!("{n} layer{}", if n == 1 { "" } else { "s" })
}

impl Workspace {
    fn refresh_raw_peers(&self, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            let peers = self
                .tabs
                .iter()
                .filter(|other| *other != tab)
                .map(Entity::downgrade)
                .collect();
            tab.update(cx, |editor, _| editor.raw_peers = peers);
        }
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Imported brush tips and grains register into a shared registry;
        // decoding them off the UI thread keeps the first frame quick.
        std::thread::Builder::new()
            .name("brush-textures".into())
            .spawn(|| {
                emulsion_io::brushset::load_textures();
            })
            .ok();
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(this) = weak.upgrade() else {
                return true;
            };
            this.update(cx, |this, cx| this.cancel_style_dialog(window, cx));
            let (modified, closing) = {
                let ws = this.read(cx);
                (
                    ws.editor
                        .as_ref()
                        .is_some_and(|e| e.read(cx).has_unsaved_changes()),
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
        let landing = crate::landing::decode();
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
        // Recent files and recovery copies come from disk; read them off the
        // first frame so the window appears at once.
        cx.spawn(async move |this, cx| {
            let (recents, recovered) = cx
                .background_spawn(async { (recent::load(), find_recovered()) })
                .await;
            this.update(cx, |this, cx| {
                this.recents = recents;
                this.recovered = recovered;
                cx.notify();
            })
            .ok();
        })
        .detach();
        Self {
            screen: Screen::Home,
            editor: None,
            tabs: Vec::new(),
            recents: Vec::new(),
            home_state: Default::default(),
            recovered: Vec::new(),
            thumbs: HashMap::new(),
            thumbs_loading: HashMap::new(),
            thumb_generation: 0,
            busy: None,
            error: None,
            focus,
            last_title: String::new(),
            dialog_root_watch: None,
            closing: false,
            settings_inputs: None,
            jev_test: None,
            image_inputs: None,
            image_test: None,
            keymap_note: None,
            probe: None,
            about_all_crates: false,
            model_jobs: Default::default(),
            batch: Default::default(),
            landing,
            splash: true,
        }
    }

    fn style_dialog_open(&self, cx: &App) -> bool {
        self.editor.as_ref().is_some_and(|editor| {
            let ui = &editor.read(cx).styles_ui;
            ui.dialog_for.is_some()
        })
    }

    /// Navigation cancels the preview before another document can own focus.
    fn cancel_style_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.clone() else {
            return;
        };
        editor.update(cx, |editor, cx| editor.finish_shape_color_edit(cx));
        let visible = editor.read(cx).styles_ui.dialog_for.is_some();
        if visible {
            editor.update(cx, |editor, cx| {
                editor.close_style_dialog(false, cx);
            });
            window.close_all_dialogs(cx);
        }
    }

    /// Does any open document have unsaved changes?
    fn modified(&self, cx: &App) -> bool {
        self.tabs.iter().any(|e| e.read(cx).has_unsaved_changes())
    }

    /// Run `then` at once: opening another document adds a tab, so nothing
    /// is discarded.
    fn add_tab_then(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.cancel_style_dialog(window, cx);
        then(self, window, cx);
    }

    /// Make tab `i` the active document. The tab leaving the screen drops
    /// its rendered tiles; they come back on demand.
    pub fn activate_tab(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.tabs.get(i).cloned() else {
            return;
        };
        self.cancel_style_dialog(window, cx);
        if let Some(old) = &self.editor
            && old != &ed
        {
            old.update(cx, |e, _| e.cache.borrow_mut().clear());
        }
        let focus = {
            let editor = ed.read(cx);
            if editor.history.open {
                editor.focus.clone()
            } else {
                editor.canvas_focus.clone()
            }
        };
        self.editor = Some(ed);
        self.screen = Screen::Editor;
        focus.focus(window, cx);
        cx.notify();
    }

    pub fn next_tab(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.tabs.len();
        if n < 2 {
            return;
        }
        let cur = self.active_tab().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(n as isize) as usize;
        self.activate_tab(next, window, cx);
    }

    pub fn active_tab(&self) -> Option<usize> {
        let e = self.editor.as_ref()?;
        self.tabs.iter().position(|t| t == e)
    }

    /// Close tab `i`, asking first when it has unsaved changes.
    pub fn close_tab(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.tabs.get(i).cloned() else {
            return;
        };
        self.cancel_style_dialog(window, cx);
        let dirty = ed.read(cx).has_unsaved_changes();
        let name = ed.read(cx).name.clone();
        let finish = move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            let Some(i) = this.tabs.iter().position(|t| t == &ed) else {
                return;
            };
            ed.update(cx, |e, _| e.discard_recovery());
            this.tabs.remove(i);
            this.refresh_raw_peers(cx);
            if this.editor.as_ref() == Some(&ed) {
                this.editor = None;
                if this.tabs.is_empty() {
                    this.screen = Screen::Home;
                } else {
                    let j = i.min(this.tabs.len() - 1);
                    this.activate_tab(j, window, cx);
                }
            }
            cx.notify();
        };
        if !dirty {
            finish(self, window, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Close {name} without saving?"),
            Some("Its changes will be lost."),
            &["Close", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                this.update_in(cx, |this, window, cx| finish(this, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// The row of document tabs above the editor.
    /// One tab per open document, even a single one, so it can always be
    /// closed; closing the last returns to Home.
    fn tab_strip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.tabs.is_empty() {
            return None;
        }
        let p = crate::theme::palette(cx);
        let active = self.active_tab();
        let mut row = div()
            .flex()
            .flex_none()
            .items_end()
            .gap(px(2.))
            .px(px(8.))
            .pt(if crate::app_state::settings(cx).compact_chrome {
                px(1.)
            } else {
                px(4.)
            })
            .border_b_1()
            .border_color(p.line)
            .bg(p.chrome)
            .font_family(crate::theme::MONO_FONT)
            .text_size(px(11.));
        for (i, ed) in self.tabs.iter().enumerate() {
            let (name, dirty) = {
                let e = ed.read(cx);
                (e.name.clone(), e.has_unsaved_changes())
            };
            let on = active == Some(i);
            let (ink, paper, line, chrome_fg) = (p.ink, p.paper, p.line, p.chrome_fg);
            row = row.child(
                div()
                    .id(("doc-tab", i))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(5.))
                    .border_1()
                    .border_b_0()
                    .border_color(if on { ink } else { line })
                    .bg(if on { paper } else { transparent_black() })
                    .text_color(if on { ink } else { chrome_fg })
                    .cursor_pointer()
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.activate_tab(i, window, cx)),
                    )
                    .child(format!("{name}{}", if dirty { " •" } else { "" }))
                    .child(crate::widgets::tip(
                        div()
                            .id(("doc-tab-close", i))
                            .px(px(3.))
                            .text_color(chrome_fg)
                            .hover(move |s| s.text_color(ink))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_tab(i, window, cx)
                            }))
                            .child("×"),
                        "Close this document (Ctrl-W); the last one closed returns to Home",
                    )),
            );
        }
        Some(row.into_any_element())
    }

    fn compact_app_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let workspace = cx.entity().downgrade();
        let has_editor = self.editor.is_some();
        let is_home = self.screen == Screen::Home;
        let home_rows = self.home_uses_rows();
        Button::new("compact-app-menu")
            .label("E")
            .tooltip("Emulsion menu")
            .xsmall()
            .ghost()
            .rounded_none()
            .text_color(p.ink)
            .dropdown_menu(move |mut menu, _, _| {
                menu = menu
                    .menu("New document…", Box::new(NewDocument))
                    .menu("Open…", Box::new(Open))
                    .separator();
                if has_editor {
                    let workspace = workspace.clone();
                    menu =
                        menu.item(PopupMenuItem::new("Editor").on_click(move |_, window, cx| {
                            workspace
                                .update(cx, |this, cx| {
                                    if let Some(active) = this.active_tab() {
                                        this.activate_tab(active, window, cx);
                                    }
                                })
                                .ok();
                        }));
                }
                for (label, screen) in [
                    ("Home", Screen::Home),
                    ("Batch", Screen::Batch),
                    ("Settings", Screen::Settings),
                    ("About", Screen::About),
                ] {
                    let workspace = workspace.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        workspace
                            .update(cx, |this, cx| {
                                this.cancel_style_dialog(window, cx);
                                this.screen = screen;
                                if screen == Screen::Batch {
                                    this.refresh_batch_recipes(cx);
                                }
                                cx.notify();
                            })
                            .ok();
                    }));
                }
                menu = menu.separator().item(
                    PopupMenuItem::new("Toggle theme").on_click(|_, _, cx| theme::toggle(cx)),
                );
                if is_home {
                    menu = menu.separator();
                    for (label, rows) in [("Grid view", false), ("Rows view", true)] {
                        let workspace = workspace.clone();
                        menu = menu.item(
                            PopupMenuItem::new(label)
                                .checked(home_rows == rows)
                                .on_click(move |_, _, cx| {
                                    workspace
                                        .update(cx, |this, cx| this.set_home_rows(rows, cx))
                                        .ok();
                                }),
                        );
                    }
                }
                menu
            })
            .into_any_element()
    }

    fn compact_theme_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        div()
            .id("compact-theme-controls")
            .test_support()
            .flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("compact-theme-light")
                    .label("☀")
                    .tooltip("Use light theme")
                    .xsmall()
                    .ghost()
                    .rounded_none()
                    .selected(!p.dark && !theme::following_omarchy(cx))
                    .on_click(|_, _, cx| theme::set_dark(false, cx)),
            )
            .child(
                Button::new("compact-theme-dark")
                    .label("☾")
                    .tooltip("Use dark theme")
                    .xsmall()
                    .ghost()
                    .rounded_none()
                    .selected(p.dark && !theme::following_omarchy(cx))
                    .on_click(|_, _, cx| theme::set_dark(true, cx)),
            )
            .into_any_element()
    }

    fn compact_page_header(
        &self,
        navigation: AnyElement,
        theme_controls: AnyElement,
        label: &'static str,
    ) -> AnyElement {
        div()
            .id("compact-page-header")
            .test_support()
            .flex()
            .items_center()
            .w_full()
            .min_w_0()
            .h(rems(2.25))
            .px_2()
            .gap_2()
            .child(navigation)
            .child(
                div()
                    .text_size(rems(0.75))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label),
            )
            .child(
                div()
                    .id("compact-page-window-drag")
                    .test_support()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(theme_controls)
            .into_any_element()
    }

    /// Navigation and document tabs share the compact editor's single header.
    fn compact_tabs(&self, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let p = theme::palette(cx);
        let app_menu = self.compact_app_menu(cx);
        let home = Button::new("compact-home")
            .label("← Home")
            .tooltip("Back to Home (Esc)")
            .xsmall()
            .outline()
            .on_click(cx.listener(|this, _, window, cx| {
                this.cancel_style_dialog(window, cx);
                this.screen = Screen::Home;
                cx.notify();
            }));
        let navigation = div()
            .flex()
            .items_center()
            .gap_1()
            .child(app_menu)
            .child(home);
        let mut tabs = div()
            .id("compact-document-tabs")
            .flex()
            .items_center()
            .max_w(rems(30.))
            .overflow_x_scroll()
            .gap_1();
        for (i, editor) in self.tabs.iter().enumerate() {
            let view = editor.read(cx);
            let name = view.name.clone();
            let dirty = view.has_unsaved_changes();
            let active = self.editor.as_ref() == Some(editor);
            let id = editor.entity_id();
            tabs = tabs.child(
                div()
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .max_w(rems(9.375))
                    .border_b_1()
                    .border_color(if active { p.accent } else { p.line })
                    .bg(if active { p.panel } else { p.paper })
                    .child(
                        Button::new(("compact-document", id))
                            .label(format!("{name}{}", if dirty { " •" } else { "" }))
                            .tooltip(name)
                            .xsmall()
                            .ghost()
                            .rounded_none()
                            .selected(active)
                            .flex_1()
                            .min_w_0()
                            .text_color(if active { p.ink } else { p.muted })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.activate_tab(i, window, cx)
                            })),
                    )
                    .child(
                        Button::new(("compact-document-close", id))
                            .label("×")
                            .tooltip("Close document (Ctrl-W)")
                            .xsmall()
                            .ghost()
                            .rounded_none()
                            .text_color(p.muted)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_tab(i, window, cx)
                            })),
                    ),
            );
        }
        let tabs = div()
            .flex()
            .items_center()
            .min_w_0()
            .flex_shrink_1()
            .gap_1()
            .child(tabs)
            .child(
                Button::new("compact-new-document")
                    .label("+")
                    .tooltip("New document (Ctrl-N)")
                    .xsmall()
                    .ghost()
                    .rounded_none()
                    .text_color(p.muted)
                    .on_click(cx.listener(|this, _, window, cx| this.new_document(window, cx))),
            )
            .when(self.tabs.len() > 3, |row| {
                let workspace = cx.entity().downgrade();
                let count = self.tabs.len();
                row.child(
                    Button::new("compact-all-documents")
                        .label(format!("{count} ⌄"))
                        .tooltip("All open documents")
                        .xsmall()
                        .outline()
                        .dropdown_menu(move |mut menu, _, cx| {
                            let Some(workspace) = workspace.upgrade() else {
                                return menu;
                            };
                            let tabs = workspace.read(cx).tabs.clone();
                            for (i, editor) in tabs.iter().enumerate() {
                                let view = editor.read(cx);
                                let dimensions =
                                    format!("{}×{}", view.editor.doc.width, view.editor.doc.height);
                                let label = format!(
                                    "{}{}  {}",
                                    view.name,
                                    if view.has_unsaved_changes() {
                                        " •"
                                    } else {
                                        ""
                                    },
                                    dimensions
                                );
                                let workspace = workspace.downgrade();
                                menu = menu.item(PopupMenuItem::new(label).on_click(
                                    move |_, window, cx| {
                                        workspace
                                            .update(cx, |this, cx| this.activate_tab(i, window, cx))
                                            .ok();
                                    },
                                ));
                            }
                            menu
                        }),
                )
            })
            .into_any_element();
        (navigation.into_any_element(), tabs)
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
        self.cancel_style_dialog(window, cx);
        // Already open? Switch to it rather than opening twice.
        if let Some(p) = &path
            && let Some(i) = self
                .tabs
                .iter()
                .position(|t| t.read(cx).editor.path.as_ref() == Some(p))
        {
            self.error = None;
            self.activate_tab(i, window, cx);
            return;
        }
        if let Some(old) = &self.editor {
            old.update(cx, |e, _| e.cache.borrow_mut().clear());
        }
        let ed = cx.new(|cx| EditorView::new(doc, graph, path, source, name, cx));
        let focus = ed.read(cx).canvas_focus.clone();
        self.tabs.push(ed.clone());
        self.refresh_raw_peers(cx);
        self.editor = Some(ed);
        self.screen = Screen::Editor;
        self.error = None;
        focus.focus(window, cx);
        cx.notify();
    }

    pub fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.add_tab_then(window, cx, move |this, window, cx| {
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
        self.add_tab_then(window, cx, move |this, window, cx| {
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
        self.add_tab_then(window, cx, |this, window, cx| {
            this.busy = Some("Opening the landing image…".into());
            cx.notify();
            cx.spawn_in(window, async move |this, cx| {
                let result = cx
                    .background_spawn(async {
                        emulsion_io::import::import_bytes(
                            crate::landing::LANDING_NAME,
                            crate::landing::LANDING_PNG,
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

    fn splash_view(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = theme::palette(cx);
        let viewport = window.viewport_size();
        let aspect = f32::from(viewport.width) / f32::from(viewport.height).max(1.0);
        let bg: AnyElement = match self
            .landing
            .as_ref()
            .map(|images| images.for_aspect(aspect))
        {
            Some((image, (x, y))) => img(ImageSource::Render(image))
                .size_full()
                .object_fit(ObjectFit::Cover)
                .object_position(x, y)
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
                                    .text_color(p.chrome_fg)
                                    .child("Emulsion"),
                            ),
                    )
                    .child(mono(
                        format!("{} · every edit, still undoable", env!("CARGO_PKG_VERSION")),
                        11.,
                        p.chrome_fg.opacity(0.8),
                    )),
            )
    }

    fn prompt_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_style_dialog(window, cx);
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            // Several files open as several tabs.
            multiple: true,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                for p in paths {
                    this.update_in(cx, |this, window, cx| this.open_path(p, window, cx))
                        .ok();
                }
            }
        })
        .detach();
    }

    /// A new 1920×1080 document on a white background.
    pub fn new_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_document_with(Some([255, 255, 255, 255]), window, cx);
    }

    /// A new 1920×1080 document: `background` fills a Background layer;
    /// None gives a single empty, transparent layer (Photoshop's
    /// "Background contents: Transparent").
    pub fn new_document_with(
        &mut self,
        background: Option<[u8; 4]>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_tab_then(window, cx, move |this, window, cx| {
            let mut doc = Document::new(1920, 1080);
            let first = match background {
                Some(rgba) => Node::new(0, "Background", NodeKind::Fill { rgba }),
                None => Node::raster(
                    0,
                    "Layer 1",
                    Arc::new(emulsion_raster::Raster::transparent(doc.width, doc.height)),
                    Default::default(),
                ),
            };
            let _ = Command::AddNode {
                node: Box::new(first),
                slot: Slot::TOP,
            }
            .apply(&mut doc);
            this.install(doc, None, None, None, "untitled".into(), window, cx);
        });
    }

    fn save(&mut self, save_as: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.style_dialog_open(cx) {
            return;
        }
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
        if !save_as && path.is_none() {
            let e = ed.read(cx);
            if emulsion_io::raw_settings::sidecar_only(&e.editor.doc)
                && e.editor.graph.commits().count() == 1
                && e.editor.graph.branches().len() == 1
                && let Ok(path) = emulsion_io::raw_settings::suggested_sidecar_path(&e.editor.doc)
            {
                self.write_target(ed, SaveTarget::Sidecar(path), cx);
                return;
            }
        }
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

    /// Save current work and existing versions without creating a checkpoint.
    /// Writes to one document never overlap: a Save requested while one runs
    /// is remembered (only the newest) and written when the current finishes.
    pub(crate) fn write(&mut self, ed: Entity<EditorView>, path: PathBuf, cx: &mut Context<Self>) {
        self.write_target(ed, SaveTarget::Project(path), cx);
    }

    fn write_target(&mut self, ed: Entity<EditorView>, target: SaveTarget, cx: &mut Context<Self>) {
        let path = target.path().to_path_buf();
        let sidecar = matches!(target, SaveTarget::Sidecar(_));
        let Some((doc, rev, graph)) = ed.update(cx, |e, cx| {
            if e.raw.is_pending() {
                e.set_status(
                    "RAW development is still running. Save when the preview finishes updating.",
                    false,
                    cx,
                );
                return None;
            }
            if e.history.save_busy {
                e.history.save_queued = Some(target.clone());
                return None;
            }
            // A queued sidecar request must not discard edits made since it
            // was requested, or replace a newer native-project save.
            if sidecar
                && (e.editor.path.is_some()
                    || !emulsion_io::raw_settings::sidecar_only(&e.editor.doc)
                    || e.editor.graph.commits().count() != 1
                    || e.editor.graph.branches().len() != 1
                    || emulsion_io::raw_settings::suggested_sidecar_path(&e.editor.doc)
                        .ok()
                        .as_ref()
                        != Some(&path))
            {
                e.set_status(
                    "This document needs a project file. Use Save as to preserve all edits.",
                    true,
                    cx,
                );
                return None;
            }
            e.history.save_busy = true;
            e.set_status(format!("Saving {}…", path.display()), false, cx);
            Some((
                e.editor.doc.clone(),
                e.editor.revision,
                e.editor.graph.clone(),
            ))
        }) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let (p, d) = (path.clone(), doc.clone());
            let result = cx
                .background_spawn(async move {
                    if sidecar {
                        emulsion_io::raw_settings::save_sidecar(&d, &p)
                    } else {
                        emulsion_io::save_full(&d, &graph, &p)
                    }
                })
                .await;
            let queued = ed.update(cx, |e, _| {
                e.history.save_busy = false;
                e.history.save_queued.take()
            });
            this.update(cx, |this, cx| match result {
                Ok(()) => {
                    let recent_path = if sidecar {
                        &doc.raw.as_ref().unwrap().source
                    } else {
                        &path
                    };
                    this.recents = recent::push(recent_path, summary(&doc));
                    this.invalidate_thumbnail(
                        &std::fs::canonicalize(recent_path).unwrap_or(recent_path.clone()),
                    );
                    ed.update(cx, |e, cx| {
                        if sidecar {
                            e.editor.mark_sidecar_saved(rev);
                        } else {
                            e.editor.mark_saved(path.clone(), rev);
                            e.name = stem(&path);
                            e.source = Some(path.clone());
                        }
                        if e.editor.revision == rev {
                            e.discard_recovery();
                        }
                        e.set_status(format!("Saved {}", path.display()), false, cx);
                    });
                }
                Err(err) => ed.update(cx, |e, cx| {
                    e.set_status(format!("Save failed: {err}"), true, cx)
                }),
            })
            .ok();
            if let Some(next) = queued {
                this.update(cx, |this, cx| this.write_target(ed, next, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.style_dialog_open(cx) {
            return;
        }
        let Some(ed) = self.editor.clone() else {
            return;
        };
        if ed.read(cx).raw.is_pending() {
            ed.update(cx, |e, cx| {
                e.set_status(
                    "RAW development is still running. Export when the preview finishes updating.",
                    false,
                    cx,
                )
            });
            return;
        }
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
        let prefs = ed.read(cx).export_prefs;
        let rx = cx.prompt_for_new_path(&dir, Some(&format!("{name}.{}", prefs.ext)));
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(mut p))) = rx.await else {
                return;
            };
            if emulsion_io::ExportFormat::from_path(&p).is_none() {
                p.set_extension(prefs.ext);
            }
            ed.update(cx, |e, cx| {
                e.export_prefs.open = false;
                cx.notify();
            });
            let flattened_psd = emulsion_io::ExportFormat::from_path(&p)
                == Some(emulsion_io::ExportFormat::Psd)
                && emulsion_io::psd::needs_appearance_fallback(&doc);
            ed.update(cx, |e, cx| {
                e.set_status(format!("Exporting {}…", p.display()), false, cx)
            });
            let (q, d) = (p.clone(), doc.clone());
            let mut opts = emulsion_io::ExportOptions::for_doc(&doc);
            opts.depth = if prefs.depth16 { 16 } else { 8 };
            opts.jpeg_quality = prefs.quality;
            let result = cx
                .background_spawn(async move {
                    emulsion_io::export::export_with_workflow(&d, &q, opts, prefs.workflow())
                })
                .await;
            ed.update(cx, |e, cx| match result {
                Ok(()) => {
                    let note = if flattened_psd {
                        " — flattened PSD appearance; save ORA to keep editable effects"
                    } else {
                        ""
                    };
                    e.set_status(format!("Exported {}{note}", p.display()), false, cx)
                }
                Err(err) => e.set_status(format!("Export failed: {err}"), true, cx),
            });
        })
        .detach();
    }

    fn quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_style_dialog(window, cx);
        self.confirm_discard(window, cx, |this, _, cx| {
            for ed in &this.tabs {
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
        if !self.style_dialog_open(cx)
            && let Some(e) = &self.editor
        {
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
                .px(px(10.))
                .border_l_1()
                .border_color(p.chrome_line)
                .font_family(MONO_FONT)
                .text_size(px(10.))
                .bg(if on { p.accent } else { transparent_black() })
                .text_color(if on {
                    p.accent_fg
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
            .h(if self.screen == Screen::Editor || crate::app_state::settings(cx).compact_chrome {
                dim::TOP_BAR_H_COMPACT
            } else {
                dim::TOP_BAR_H
            })
            .bg(p.chrome)
            .text_color(p.chrome_fg)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .px(px(12.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Emulsion"),
                    ),
            )
            .child(
                tab(
                    "tab-editor",
                    "Editor",
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
                tab("tab-home", "Home", self.screen == Screen::Home, true).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.cancel_style_dialog(window, cx);
                        this.screen = Screen::Home;
                        cx.notify();
                    },
                )),
            )
            .child(
                tab("tab-batch", "Batch", self.screen == Screen::Batch, true).on_click(
                    cx.listener(|this, _, window, cx| {
                        this.cancel_style_dialog(window, cx);
                        this.screen = Screen::Batch;
                        this.refresh_batch_recipes(cx);
                    }),
                ),
            )
            .child(
                tab(
                    "tab-settings",
                    "Settings",
                    self.screen == Screen::Settings,
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.cancel_style_dialog(window, cx);
                    this.screen = Screen::Settings;
                    cx.notify();
                })),
            )
            .child(
                tab("tab-about", "About", self.screen == Screen::About, true).on_click(
                    cx.listener(|this, _, window, cx| {
                        this.cancel_style_dialog(window, cx);
                        this.screen = Screen::About;
                        cx.notify();
                    }),
                ),
            )
            .child(div().flex_1().border_l_1().border_color(p.chrome_line))
            .child(
                div()
                    .flex()
                    .items_center()
                    .px(px(14.))
                    .border_l_1()
                    .border_color(p.chrome_line)
                    .map(|d| {
                        #[cfg(target_os = "linux")]
                        let d = {
                            // A quiet theme control, not a social button: the
                            // Omarchy mark, and the theme's name while it is on.
                            let on = theme::following_omarchy(cx);
                            let name = theme::omarchy_theme_name();
                            let label: SharedString = match (&name, on) {
                                (Some(n), true) => format!("◆ {n}").into(),
                                _ => "◆ omarchy".into(),
                            };
                            let tip: SharedString = match &name {
                                Some(n) if on => format!(
                                    "Colours follow your Omarchy theme ({n}), live. Click for Emulsion's own light or dark palette."
                                )
                                .into(),
                                Some(n) => format!(
                                    "Use the colours of your Omarchy theme ({n}) and follow it when it changes"
                                )
                                .into(),
                                None => "Omarchy theme not found; Emulsion keeps its own palette".into(),
                            };
                            d.child(crate::widgets::tip(
                                theme_btn("omarchy", "", on)
                                    .w_auto()
                                    .px(px(10.))
                                    .text_size(px(10.))
                                    .when(!on, |d| d.border_color(transparent_black()))
                                    .child(label)
                                    .on_click(cx.listener(|_, _, _, cx| theme::follow_omarchy(cx))),
                                tip,
                            ))
                        };
                        d
                    })
                    .child(
                        theme_btn("light", "☀", !p.dark && !theme::following_omarchy(cx)).on_click(
                            cx.listener(|_, _, _, cx| {
                                theme::set_dark(false, cx);
                                cx.refresh_windows();
                            }),
                        ),
                    )
                    .child(
                        theme_btn("dark", "☾", p.dark && !theme::following_omarchy(cx)).on_click(
                            cx.listener(|_, _, _, cx| {
                                theme::set_dark(true, cx);
                                cx.refresh_windows();
                            }),
                        ),
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
                .border_l_4()
                .border_color(p.accent)
                .bg(if err { p.accent } else { p.chrome })
                .text_color(if err { p.accent_fg } else { p.chrome_fg })
                .child(mono(msg, 11., if err { p.accent_fg } else { p.chrome_fg }).flex_1())
                .when(err, |d| {
                    d.child(
                        div()
                            .id("dismiss")
                            .cursor_pointer()
                            .child(mono("dismiss", 10., p.accent_fg))
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
        // Dialog state belongs to Root, while this view owns its overlay layer.
        // Root notifications must invalidate the cached Workspace as well.
        if self.dialog_root_watch.is_none()
            && let Some(Some(root)) = window.root::<gpui_kit::component::Root>()
        {
            self.dialog_root_watch = Some(cx.observe(&root, |_, _, cx| cx.notify()));
        }
        let p = theme::palette(cx);
        let title = match &self.editor {
            Some(e) => {
                let e = e.read(cx);
                format!(
                    "{}{} — Emulsion",
                    e.name,
                    if e.has_unsaved_changes() { " •" } else { "" }
                )
            }
            None => "Emulsion".into(),
        };
        if title != self.last_title {
            window.set_window_title(&title);
            self.last_title = title;
        }
        let title_bar = gpui_kit::component::TitleBar::new()
            .on_close_window(|_, window, cx| {
                window.dispatch_action(Box::new(Quit), cx);
            })
            .child(
                div().flex().items_center().gap(px(10.)).pl(px(4.)).child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.last_title.trim_end_matches(" — Emulsion").to_string()),
                ),
            );
        let compact = crate::app_state::settings(cx).compact_chrome;
        let compact_editor = compact && self.screen == Screen::Editor && self.editor.is_some();
        let compact_page = compact && !compact_editor;
        let top = if compact_editor {
            let (navigation, tabs) = self.compact_tabs(cx);
            let theme_controls = self.compact_theme_controls(cx);
            let editor = self.editor.as_ref().unwrap().clone();
            let header = editor.update(cx, |editor, cx| {
                editor.compact_header(navigation, tabs, theme_controls, &p, window, cx)
            });
            gpui_kit::component::TitleBar::new()
                .draggable(false)
                .h(rems(2.25))
                .when(!cfg!(target_os = "macos"), |bar| bar.pl_1())
                .bg(p.paper)
                .border_color(p.line)
                .on_close_window(|_, window, cx| window.dispatch_action(Box::new(Quit), cx))
                .child(header)
                .into_any_element()
        } else if compact_page {
            let navigation = self.compact_app_menu(cx);
            let theme_controls = self.compact_theme_controls(cx);
            let header = if self.screen == Screen::Home {
                self.home_header(navigation, theme_controls, window, cx)
            } else {
                let label = match self.screen {
                    Screen::Batch => "Batch",
                    Screen::Settings => "Settings",
                    Screen::About => "About",
                    Screen::Editor => "Editor",
                    Screen::Home => "Home",
                };
                self.compact_page_header(navigation, theme_controls, label)
            };
            gpui_kit::component::TitleBar::new()
                .draggable(false)
                .h(rems(2.25))
                .when(!cfg!(target_os = "macos"), |bar| bar.pl_1())
                .bg(p.paper)
                .border_color(p.line)
                .on_close_window(|_, window, cx| window.dispatch_action(Box::new(Quit), cx))
                .child(header)
                .into_any_element()
        } else {
            self.top_bar(cx).into_any_element()
        };
        let banner = self.banner(cx);
        let body: AnyElement = match (self.screen, &self.editor) {
            (Screen::Editor, Some(e)) => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .when(!compact_editor, |body| body.children(self.tab_strip(cx)))
                .child(e.clone())
                .into_any_element(),
            (Screen::Settings, _) => self.settings_screen(window, cx).into_any_element(),
            (Screen::About, _) => self.about_screen(window, cx).into_any_element(),
            (Screen::Batch, _) => self.batch_screen(window, cx).into_any_element(),
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
            .on_action(
                cx.listener(|this, _: &SynchronizeRaw, window, cx| {
                    this.synchronize_raw(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &Quit, window, cx| this.quit(window, cx)))
            .on_action(cx.listener(|this, _: &ShowHome, window, cx| {
                this.cancel_style_dialog(window, cx);
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
                        if e.editor.doc.selection.is_some() {
                            e.deselect(cx)
                        } else {
                            e.rotate(0.0, cx)
                        }
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
            .on_action(cx.listener(|this, _: &NewLayer, _, cx| {
                this.with_editor(cx, |e, cx| {
                    e.new_empty_layer(cx);
                })
            }))
            .on_action(cx.listener(|this, _: &DuplicateNode, _, cx| {
                this.with_editor(cx, |e, cx| e.duplicate_selected(cx))
            }))
            .on_action(cx.listener(|this, _: &CopyPixels, _, cx| {
                this.with_editor(cx, |e, cx| e.copy_pixels(cx))
            }))
            .on_action(cx.listener(|this, _: &CutPixels, _, cx| {
                this.with_editor(cx, |e, cx| e.cut_pixels(cx))
            }))
            .on_action(cx.listener(|this, _: &PastePixels, _, cx| {
                this.with_editor(cx, |e, cx| e.paste_pixels(cx))
            }))
            .on_action(cx.listener(|this, _: &ClearPixels, _, cx| {
                this.with_editor(cx, |e, cx| e.clear_pixels(cx))
            }))
            .on_action(cx.listener(|this, _: &CanvasDelete, _, cx| {
                this.with_editor(cx, |e, cx| e.delete_canvas_pixels(cx))
            }))
            .on_action(cx.listener(|this, _: &FreeTransform, window, cx| {
                this.with_editor(cx, |e, cx| {
                    e.transform_pixels(cx);
                    window.focus(&e.canvas_focus, cx);
                })
            }))
            .on_action(cx.listener(|this, _: &TransformScale, window, cx| {
                this.with_editor(cx, |e, cx| {
                    e.begin_transform_action("scale", cx);
                    window.focus(&e.canvas_focus, cx);
                })
            }))
            .on_action(cx.listener(|this, _: &TransformRotate, window, cx| {
                this.with_editor(cx, |e, cx| {
                    e.begin_transform_action("rotate", cx);
                    window.focus(&e.canvas_focus, cx);
                })
            }))
            .on_action(cx.listener(|this, _: &TransformDistort, window, cx| {
                this.with_editor(cx, |e, cx| {
                    e.begin_transform_action("distort", cx);
                    window.focus(&e.canvas_focus, cx);
                })
            }))
            .on_action(cx.listener(|this, _: &TransformWarp, window, cx| {
                this.with_editor(cx, |e, cx| {
                    e.begin_transform_action("warp", cx);
                    window.focus(&e.canvas_focus, cx);
                })
            }))
            .on_action(cx.listener(|this, _: &RotateLayer180, _, cx| {
                this.with_editor(cx, |e, cx| e.rotate_transform_selection(180., cx))
            }))
            .on_action(cx.listener(|this, _: &RotateLayer90Cw, _, cx| {
                this.with_editor(cx, |e, cx| e.rotate_transform_selection(90., cx))
            }))
            .on_action(cx.listener(|this, _: &RotateLayer90Ccw, _, cx| {
                this.with_editor(cx, |e, cx| e.rotate_transform_selection(-90., cx))
            }))
            .on_action(cx.listener(|this, _: &FlipLayerHorizontal, _, cx| {
                this.with_editor(cx, |e, cx| e.flip_transform_selection(true, cx))
            }))
            .on_action(cx.listener(|this, _: &FlipLayerVertical, _, cx| {
                this.with_editor(cx, |e, cx| e.flip_transform_selection(false, cx))
            }))
            .on_action(cx.listener(|this, _: &NudgeLeft, _, cx| {
                this.with_editor(cx, |e, cx| e.nudge_selected(-1.0, 0.0, cx))
            }))
            .on_action(cx.listener(|this, _: &NudgeRight, _, cx| {
                this.with_editor(cx, |e, cx| e.nudge_selected(1.0, 0.0, cx))
            }))
            .on_action(cx.listener(|this, _: &NudgeUp, _, cx| {
                this.with_editor(cx, |e, cx| e.nudge_selected(0.0, -1.0, cx))
            }))
            .on_action(cx.listener(|this, _: &NudgeDown, _, cx| {
                this.with_editor(cx, |e, cx| e.nudge_selected(0.0, 1.0, cx))
            }))
            .on_action(cx.listener(|this, _: &NudgeLeftLarge, _, cx| {
                this.with_editor(cx, |e, cx| e.nudge_selected(-10.0, 0.0, cx))
            }))
            .on_action(cx.listener(|this, _: &NudgeRightLarge, _, cx| {
                this.with_editor(cx, |e, cx| e.nudge_selected(10.0, 0.0, cx))
            }))
            .on_action(cx.listener(|this, _: &NudgeUpLarge, _, cx| {
                this.with_editor(cx, |e, cx| e.nudge_selected(0.0, -10.0, cx))
            }))
            .on_action(cx.listener(|this, _: &NudgeDownLarge, _, cx| {
                this.with_editor(cx, |e, cx| e.nudge_selected(0.0, 10.0, cx))
            }))
            .on_action(cx.listener(|this, _: &GroupNodes, _, cx| {
                this.with_editor(cx, |e, cx| e.group_selected(cx))
            }))
            .on_action(cx.listener(|this, _: &Ungroup, _, cx| {
                this.with_editor(cx, |e, cx| e.ungroup_selected(cx))
            }))
            .on_action(cx.listener(|this, _: &RenameLayer, window, cx| {
                this.with_editor(cx, |e, cx| e.rename_layer(window, cx))
            }))
            .on_action(cx.listener(|this, _: &MergeLayers, _, cx| {
                this.with_editor(cx, |e, cx| e.merge_layers(false, cx))
            }))
            .on_action(cx.listener(|this, _: &MergeVisible, _, cx| {
                this.with_editor(cx, |e, cx| e.merge_layers(true, cx))
            }))
            .on_action(cx.listener(|this, _: &FlattenImage, _, cx| {
                this.with_editor(cx, |e, cx| e.flatten_image(cx))
            }))
            .on_action(cx.listener(|this, _: &LinkLayers, _, cx| {
                this.with_editor(cx, |e, cx| e.set_layer_links(true, cx))
            }))
            .on_action(cx.listener(|this, _: &UnlinkLayers, _, cx| {
                this.with_editor(cx, |e, cx| e.set_layer_links(false, cx))
            }))
            .on_action(cx.listener(|this, _: &CopyLayerStyle, _, cx| {
                this.with_editor(cx, |e, cx| e.copy_layer_style(cx))
            }))
            .on_action(cx.listener(|this, _: &PasteLayerStyle, _, cx| {
                this.with_editor(cx, |e, cx| e.paste_layer_style(cx))
            }))
            .on_action(cx.listener(|this, _: &ApplyLayerMask, _, cx| {
                this.with_editor(cx, |e, cx| e.apply_layer_mask(cx))
            }))
            .on_action(cx.listener(|this, _: &MoveNodeUp, _, cx| {
                this.with_editor(cx, |e, cx| e.shift_selected(true, cx))
            }))
            .on_action(cx.listener(|this, _: &MoveNodeDown, _, cx| {
                this.with_editor(cx, |e, cx| e.shift_selected(false, cx))
            }))
            .on_action(cx.listener(|this, _: &NextBlendMode, _, cx| {
                this.with_editor(cx, |e, cx| e.cycle_blend_mode(true, cx))
            }))
            .on_action(cx.listener(|this, _: &PreviousBlendMode, _, cx| {
                this.with_editor(cx, |e, cx| e.cycle_blend_mode(false, cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleNodeVisible, _, cx| {
                this.with_editor(cx, |e, cx| e.toggle_selected_visible(cx))
            }))
            .on_action(cx.listener(|this, _: &Ask, window, cx| {
                if this.style_dialog_open(cx) {
                    return;
                }
                if let Some(e) = this.editor.clone() {
                    this.screen = Screen::Editor;
                    e.update(cx, |e, cx| e.open_ask(window, cx));
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ToolPen, _, cx| {
                this.with_editor(cx, |e, cx| e.set_pen_mode(crate::editor::PenMode::Pen, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolType, _, cx| {
                this.with_editor(cx, |e, cx| e.set_type_mode(false, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolVerticalType, _, cx| {
                this.with_editor(cx, |e, cx| e.set_type_mode(true, cx))
            }))
            .on_action(cx.listener(|this, _: &ConvertToSmartObject, _, cx| {
                this.with_editor(cx, |e, cx| e.convert_smart(cx))
            }))
            .on_action(cx.listener(|this, _: &ConvertSmartToLayers, _, cx| {
                this.with_editor(cx, |e, cx| e.convert_smart_to_layers(cx))
            }))
            .on_action(cx.listener(|this, _: &RasterizeLayer, _, cx| {
                this.with_editor(cx, |e, cx| e.rasterize_layer(cx))
            }))
            .on_action(cx.listener(|this, _: &ToolHand, _, cx| {
                this.with_editor(cx, |e, cx| e.set_hand_mode(false, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolRotateView, _, cx| {
                this.with_editor(cx, |e, cx| e.set_hand_mode(true, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolMove, _, cx| {
                this.with_editor(cx, |e, cx| e.set_tool(crate::editor::Tool::Move, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolMarquee, _, cx| {
                this.with_editor(cx, |e, cx| {
                    e.set_select(crate::editor::SelectShape::Rect, cx)
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
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.next_tab(1, window, cx)))
            .on_action(cx.listener(|this, _: &PrevTab, window, cx| this.next_tab(-1, window, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                if let Some(i) = this.active_tab() {
                    this.close_tab(i, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToolEyedropper, _, cx| {
                this.with_editor(cx, |e, cx| e.set_tool(crate::editor::Tool::Eyedropper, cx))
            }))
            .on_action(cx.listener(|this, _: &ToolZoom, _, cx| {
                this.with_editor(cx, |e, cx| e.set_tool(crate::editor::Tool::Zoom, cx))
            }))
            .on_action(cx.listener(|this, _: &ImageSizeDialog, window, cx| {
                if this.style_dialog_open(cx) {
                    return;
                }
                if let Some(e) = &this.editor {
                    e.update(cx, |e, cx| {
                        e.open_size_panel(crate::editor::SizeMode::Image, window, cx)
                    });
                }
            }))
            .on_action(cx.listener(|this, _: &CanvasSizeDialog, window, cx| {
                if this.style_dialog_open(cx) {
                    return;
                }
                if let Some(e) = &this.editor {
                    e.update(cx, |e, cx| {
                        e.open_size_panel(crate::editor::SizeMode::Canvas, window, cx)
                    });
                }
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
            .on_action(cx.listener(|this, _: &ShowSettings, window, cx| {
                this.cancel_style_dialog(window, cx);
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
            .children((!compact).then_some(title_bar))
            .child(top)
            .children(banner)
            .child(body)
            .when(self.splash, |d| d.child(self.splash_view(window, cx)))
            .children(gpui_kit::component::Root::render_dialog_layer(window, cx))
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

#[cfg(test)]
mod compact_tests {
    use super::*;
    use crate::app_state::{AppSettings, Capabilities, CliStatus};
    use core::prelude::v1::test;
    use emulsion_io::settings::Settings;
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[gpui_kit::test]
    fn compact_document_tabs_switch_and_close_through_pointer_input(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
            theme::install(cx);
            crate::actions::bind(cx);
            cx.set_global(AppSettings(Settings {
                compact_chrome: true,
                ..Settings::default()
            }));
            cx.set_global(Capabilities {
                cli: CliStatus::Missing,
            });
        });
        let slot = Rc::new(RefCell::new(None));
        let installed = slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.splash = false;
                for name in ["First", "Second"] {
                    workspace.install(
                        Document::new(64, 64),
                        None,
                        None,
                        None,
                        name.into(),
                        window,
                        cx,
                    );
                }
            });
            *installed.borrow_mut() = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let workspace = slot.borrow().clone().unwrap();
        cx.run_until_parked();
        cx.update(|window, _| {
            assert!(window.find("editor-document-bar").bounds().size.height >= px(36.));
            assert!(window.find("compact-tab-leading-drag").bounds().size.width >= px(24.));
            assert!(window.find("compact-window-drag").bounds().size.width >= px(64.));
        });
        cx.update(|window, cx| window.click("compact-app-menu", cx));
        cx.run_until_parked();
        cx.update(|window, _| assert!(window.find("popup-menu").visible()));
        cx.update(|window, cx| window.press("escape", cx));
        cx.run_until_parked();
        let first = cx.update(|_, cx| workspace.read(cx).tabs[0].clone());
        cx.update(|window, cx| window.click(("compact-document", first.entity_id()), cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(workspace.read(cx).editor.as_ref(), Some(&first)));

        cx.update(|window, cx| window.click("compact-home", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let workspace = workspace.read(cx);
            assert_eq!(workspace.screen, Screen::Home);
            assert_eq!(workspace.editor.as_ref(), Some(&first));
        });
        cx.update(|window, _| {
            assert!(window.find("home-brand").visible());
            assert!(window.find("home-header-filters").visible());
            assert!(window.find("home-window-drag").bounds().size.width >= px(48.));
        });
        cx.update(|window, cx| window.click("home-filter-today", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.click("home-header-search-button", cx));
        cx.run_until_parked();
        cx.update(|window, _| {
            assert!(window.find("home-search-container").bounds().size.width <= px(320.));
        });
        cx.update(|window, cx| window.press("escape", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.click("compact-theme-light", cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(!theme::palette(cx).dark));
        cx.update(|window, cx| window.click("compact-theme-dark", cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(theme::palette(cx).dark));
        cx.update(|window, cx| window.click("compact-app-menu", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(3usize, cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let workspace = workspace.read(cx);
            assert_eq!(workspace.screen, Screen::Editor);
            assert_eq!(workspace.editor.as_ref(), Some(&first));
        });

        cx.update(|window, cx| window.click("compact-app-menu", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(5usize, cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(workspace.read(cx).screen, Screen::Batch);
            assert!(window.find("compact-page-header").bounds().size.height >= px(36.));
            assert!(window.find("compact-page-window-drag").bounds().size.width >= px(64.));
        });
        cx.update(|window, cx| window.click("compact-app-menu", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(3usize, cx));
        cx.run_until_parked();

        cx.update(|window, cx| window.click(("compact-document-close", first.entity_id()), cx));
        cx.run_until_parked();
        let last = cx.update(|_, cx| {
            let workspace = workspace.read(cx);
            assert_eq!(workspace.tabs.len(), 1);
            assert_eq!(workspace.editor.as_ref(), workspace.tabs.first());
            workspace.tabs[0].clone()
        });
        cx.update(|window, cx| window.click(("compact-document-close", last.entity_id()), cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let workspace = workspace.read(cx);
            assert!(workspace.tabs.is_empty());
            assert!(workspace.editor.is_none());
            assert_eq!(workspace.screen, Screen::Home);
        });
    }
}
