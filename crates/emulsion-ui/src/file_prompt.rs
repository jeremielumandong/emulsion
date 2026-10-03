//! Save and Open path prompts that still work without a system file chooser.
//!
//! On Linux GPUI asks xdg-desktop-portal for its FileChooser; where no portal
//! runs (minimal window managers, sandboxes, containers) the platform prompt
//! fails at once. Every path prompt in the app goes through [`FilePrompts`],
//! which then shows [`PathDialog`] inside the window instead, so Save, Open,
//! imports and exports keep working. The receivers match GPUI's, so callers
//! read answers exactly as before.
use crate::theme;
use futures::channel::oneshot;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Answer to a path prompt, shaped like GPUI's own.
pub type PathAnswer<T> = oneshot::Receiver<anyhow::Result<Option<T>>>;

/// Path prompts with an in-app fallback. Use these instead of
/// `App::prompt_for_new_path` / `App::prompt_for_paths`.
pub trait FilePrompts {
    /// Ask where to save, starting in `directory` with `suggested_name`.
    fn prompt_save_path(
        &mut self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> PathAnswer<PathBuf>;
    /// Ask for existing files or folders to open.
    fn prompt_open_paths(&mut self, options: PathPromptOptions) -> PathAnswer<Vec<PathBuf>>;
}

impl FilePrompts for App {
    fn prompt_save_path(
        &mut self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> PathAnswer<PathBuf> {
        let platform =
            (!in_app_only(self)).then(|| self.prompt_for_new_path(directory, suggested_name));
        let request = Request::Save {
            directory: directory.to_path_buf(),
            name: suggested_name.map(str::to_owned),
        };
        relay(self, platform, request, |paths| paths.into_iter().next())
    }

    fn prompt_open_paths(&mut self, options: PathPromptOptions) -> PathAnswer<Vec<PathBuf>> {
        let platform = (!in_app_only(self)).then(|| self.prompt_for_paths(options.clone()));
        relay(self, platform, Request::Open(options), Some)
    }
}

/// Skip the system chooser and always use the in-app dialog: set by tests,
/// or by `EMULSION_FILE_DIALOG=builtin` for a portal that misbehaves.
pub struct InAppFileDialog;
impl Global for InAppFileDialog {}

fn in_app_only(cx: &App) -> bool {
    cx.has_global::<InAppFileDialog>()
        || std::env::var("EMULSION_FILE_DIALOG").is_ok_and(|v| v.eq_ignore_ascii_case("builtin"))
}

#[derive(Clone)]
enum Request {
    Save {
        directory: PathBuf,
        name: Option<String>,
    },
    Open(PathPromptOptions),
}

/// Pass the platform answer through, or show [`PathDialog`] when the
/// platform could not prompt at all.
fn relay<T: 'static>(
    cx: &mut App,
    platform: Option<PathAnswer<T>>,
    request: Request,
    convert: fn(Vec<PathBuf>) -> Option<T>,
) -> PathAnswer<T> {
    let window = cx.active_window();
    let (tx, rx) = oneshot::channel();
    cx.spawn(async move |cx| {
        let error = match platform {
            Some(platform) => match platform.await {
                Ok(Ok(answer)) => {
                    tx.send(Ok(answer)).ok();
                    return;
                }
                Ok(Err(error)) => error,
                // The platform dropped the prompt; so do we.
                Err(_) => return,
            },
            None => anyhow::anyhow!("in-app file dialog requested"),
        };
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                "system file chooser unavailable, using the in-app path dialog: {error:#}"
            );
        }
        let Some(dialog) = cx.update(|cx| show(window, request, cx)) else {
            tx.send(Err(error)).ok();
            return;
        };
        let picked = dialog.await.ok().flatten();
        tx.send(Ok(picked.and_then(convert))).ok();
    })
    .detach();
    rx
}

/// The dialog waiting to be picked up by the window's prompt builder.
#[derive(Default)]
struct PendingDialog(Option<Entity<PathDialog>>);
impl Global for PendingDialog {}

/// Hand the queued [`PathDialog`] to `prompt::install`'s builder.
pub(crate) fn take_pending(cx: &mut App) -> Option<Entity<PathDialog>> {
    cx.try_global::<PendingDialog>()?;
    cx.global_mut::<PendingDialog>().0.take()
}

fn show(
    window: Option<AnyWindowHandle>,
    request: Request,
    cx: &mut App,
) -> Option<oneshot::Receiver<Option<Vec<PathBuf>>>> {
    let open = cx.windows();
    let window = window
        .filter(|w| open.contains(w))
        .or_else(|| cx.active_window())
        .or_else(|| open.first().copied())?;
    // Window prompts render in-app once this builder is installed.
    crate::prompt::install(cx);
    let (tx, rx) = oneshot::channel();
    window
        .update(cx, |_, window, cx| {
            let dialog = cx.new(|cx| PathDialog::new(request, tx, window, cx));
            #[cfg(test)]
            cx.set_global(tests::LastDialog(dialog.downgrade()));
            cx.set_global(PendingDialog(Some(dialog)));
            let title = t!("file_prompt.title").to_string();
            drop(window.prompt(PromptLevel::Info, &title, None, &[title.as_str()], cx));
            cx.remove_global::<PendingDialog>();
            window.refresh();
        })
        .ok()?;
    Some(rx)
}

/// The in-app Save / Open dialog: a path field, a folder listing for Open,
/// inline errors and a Replace confirmation for an existing file.
pub(crate) struct PathDialog {
    request: Request,
    input: Entity<InputState>,
    error: Option<SharedString>,
    replace: Option<PathBuf>,
    listing: Option<(PathBuf, Vec<(PathBuf, bool)>)>,
    selected: Vec<PathBuf>,
    sender: Option<oneshot::Sender<Option<Vec<PathBuf>>>>,
    _input_sub: Subscription,
}

const MAX_LISTED: usize = 300;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

impl PathDialog {
    fn new(
        request: Request,
        sender: oneshot::Sender<Option<Vec<PathBuf>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial = match &request {
            Request::Save { directory, name } => {
                let dir = if directory.as_os_str().is_empty() {
                    std::env::current_dir().unwrap_or_else(|_| home())
                } else {
                    directory.clone()
                };
                dir.join(name.as_deref().unwrap_or_default())
                    .to_string_lossy()
                    .into_owned()
            }
            Request::Open(_) => with_separator(&home()),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        let sub = cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.error = None;
                this.replace = None;
                this.refresh_listing(cx);
                cx.notify();
            }
        });
        let mut dialog = Self {
            request,
            input,
            error: None,
            replace: None,
            listing: None,
            selected: Vec::new(),
            sender: Some(sender),
            _input_sub: sub,
        };
        dialog.refresh_listing(cx);
        dialog
    }

    fn text(&self, cx: &App) -> String {
        self.input.read(cx).value().trim().to_string()
    }

    /// The typed path, with `~` expanded and relative paths taken from the
    /// prompt's starting folder.
    fn resolve(&self, cx: &App) -> Option<PathBuf> {
        let text = self.text(cx);
        if text.is_empty() {
            return None;
        }
        let path = match text.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
                home().join(rest.trim_start_matches(['/', '\\']))
            }
            _ => PathBuf::from(&text),
        };
        Some(if path.is_absolute() {
            path
        } else {
            match &self.request {
                Request::Save { directory, .. } => directory.join(path),
                Request::Open(_) => home().join(path),
            }
        })
    }

    fn options(&self) -> Option<&PathPromptOptions> {
        match &self.request {
            Request::Open(options) => Some(options),
            Request::Save { .. } => None,
        }
    }

    /// Re-read the folder the Open field points into, when it changed.
    fn refresh_listing(&mut self, cx: &App) {
        let Some(options) = self.options() else {
            return;
        };
        let folders_only = !options.files;
        let Some(path) = self.resolve(cx) else {
            return;
        };
        let dir = if path.is_dir() {
            path
        } else {
            match path.parent() {
                Some(parent) if parent.is_dir() => parent.to_path_buf(),
                _ => return,
            }
        };
        if self.listing.as_ref().is_some_and(|(d, _)| *d == dir) {
            return;
        }
        let mut entries: Vec<(PathBuf, bool)> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                    .map(|e| {
                        let path = e.path();
                        let is_dir = path.is_dir();
                        (path, is_dir)
                    })
                    .filter(|(_, is_dir)| *is_dir || !folders_only)
                    .collect()
            })
            .unwrap_or_default();
        entries.sort_by_cached_key(|(p, is_dir)| {
            (
                !is_dir,
                p.file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default(),
            )
        });
        entries.truncate(MAX_LISTED);
        self.listing = Some((dir, entries));
    }

    fn set_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.error = None;
        self.replace = None;
        self.refresh_listing(cx);
        cx.notify();
    }

    fn pick_entry(
        &mut self,
        path: PathBuf,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if is_dir {
            self.set_text(with_separator(&path), window, cx);
        } else if self.options().is_some_and(|o| o.multiple) {
            if let Some(ix) = self.selected.iter().position(|p| *p == path) {
                self.selected.remove(ix);
            } else {
                self.selected.push(path);
            }
            self.error = None;
            cx.notify();
        } else {
            self.set_text(path.to_string_lossy().into_owned(), window, cx);
        }
    }

    fn fail(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.request.clone() {
            Request::Save { .. } => self.confirm_save(cx),
            Request::Open(options) => self.confirm_open(&options, window, cx),
        }
    }

    fn confirm_save(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.resolve(cx) else {
            return self.fail(t!("file_prompt.empty"), cx);
        };
        if path.is_dir() {
            return self.fail(t!("file_prompt.is_folder"), cx);
        }
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
        if !parent.is_some_and(Path::is_dir) {
            let shown = parent.unwrap_or(&path).display().to_string();
            return self.fail(t!("file_prompt.no_folder", path = shown), cx);
        }
        if path.exists() && self.replace.as_ref() != Some(&path) {
            self.replace = Some(path);
            self.error = None;
            cx.notify();
            return;
        }
        self.answer(Some(vec![path]), cx);
    }

    fn confirm_open(
        &mut self,
        options: &PathPromptOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if options.multiple && !self.selected.is_empty() {
            if let Some(gone) = self.selected.iter().find(|p| !p.exists()) {
                let shown = gone.display().to_string();
                return self.fail(t!("file_prompt.missing", path = shown), cx);
            }
            let selected = std::mem::take(&mut self.selected);
            return self.answer(Some(selected), cx);
        }
        let Some(path) = self.resolve(cx) else {
            return self.fail(t!("file_prompt.empty"), cx);
        };
        if !path.exists() {
            let shown = path.display().to_string();
            return self.fail(t!("file_prompt.missing", path = shown), cx);
        }
        if path.is_dir() {
            if options.directories {
                return self.answer(Some(vec![path]), cx);
            }
            // A folder when files are wanted: go into it.
            return self.set_text(with_separator(&path), window, cx);
        }
        if !options.files {
            return self.fail(t!("file_prompt.need_folder"), cx);
        }
        self.answer(Some(vec![path]), cx);
    }

    fn answer(&mut self, paths: Option<Vec<PathBuf>>, cx: &mut Context<Self>) {
        let chosen = paths.is_some();
        if let Some(sender) = self.sender.take() {
            sender.send(paths).ok();
        }
        cx.emit(PromptResponse(if chosen { 0 } else { 1 }));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "enter" => self.confirm(window, cx),
            "escape" => self.answer(None, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn listing(&self, p: &theme::Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (dir, entries) = self.listing.as_ref()?;
        let parent = dir
            .parent()
            .map(|d| (d.to_path_buf(), true, "..".to_string()));
        let rows = parent
            .into_iter()
            .chain(entries.iter().map(|(path, is_dir)| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let label = if *is_dir {
                    format!("{name}{}", std::path::MAIN_SEPARATOR)
                } else {
                    name
                };
                (path.clone(), *is_dir, label)
            }))
            .enumerate()
            .map(|(ix, (path, is_dir, label))| {
                let selected = self.selected.contains(&path);
                div()
                    .id(("file-prompt-entry", ix))
                    .test_support()
                    .px(px(8.))
                    .py(px(3.))
                    .rounded_sm()
                    .text_size(px(12.))
                    .text_color(if is_dir { p.ink } else { p.muted })
                    .when(selected, |d| d.bg(p.soft_bg).text_color(p.ink))
                    .hover(|d| d.bg(p.soft_bg))
                    .cursor_pointer()
                    .child(if selected {
                        format!("✓ {label}")
                    } else {
                        label
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_entry(path.clone(), is_dir, window, cx)
                    }))
            })
            .collect::<Vec<_>>();
        Some(
            div()
                .id("file-prompt-listing")
                .test_support()
                .flex()
                .flex_col()
                .h(px(220.))
                .overflow_y_scroll()
                .p(px(4.))
                .rounded_md()
                .border_1()
                .border_color(p.line)
                .bg(p.paper)
                .children(rows)
                .into_any_element(),
        )
    }
}

fn with_separator(dir: &Path) -> String {
    let mut text = dir.to_string_lossy().into_owned();
    if !text.ends_with(std::path::MAIN_SEPARATOR) {
        text.push(std::path::MAIN_SEPARATOR);
    }
    text
}

impl EventEmitter<PromptResponse> for PathDialog {}

impl Focusable for PathDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for PathDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let open = self.options().cloned();
        let title: SharedString = match &open {
            Some(options) => options
                .prompt
                .clone()
                .unwrap_or_else(|| t!("file_prompt.open_title").into()),
            None => t!("file_prompt.save_title").into(),
        };
        let ok_label: SharedString = match (&open, &self.replace) {
            (Some(_), _) => t!("file_prompt.open").into(),
            (None, Some(_)) => t!("file_prompt.replace").into(),
            (None, None) => t!("file_prompt.save").into(),
        };
        let hint = match &open {
            Some(_) => t!("file_prompt.open_hint"),
            None => t!("file_prompt.save_hint"),
        };
        let replace = self.replace.as_ref().map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            t!("file_prompt.replace_confirm", name = name)
        });
        let selected = (!self.selected.is_empty())
            .then(|| t!("file_prompt.selected", count = self.selected.len()));
        let listing = self.listing(&p, cx);

        div()
            .id("file-prompt")
            .test_support()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "escape") {
                    this.on_key_down(event, window, cx);
                }
            }))
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.45))
            .occlude()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .w(px(520.))
                    .max_w_full()
                    .p(px(20.))
                    .rounded_lg()
                    .border_1()
                    .border_color(p.line)
                    .bg(p.panel)
                    .shadow_lg()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.ink)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(p.muted)
                            .child(format!("{} {hint}", t!("file_prompt.unavailable"))),
                    )
                    .child(Input::new(&self.input).small())
                    .children(listing)
                    .children(
                        selected.map(|s| div().text_size(px(12.)).text_color(p.muted).child(s)),
                    )
                    .children(replace.map(|text| {
                        div()
                            .id("file-prompt-replace")
                            .test_support()
                            .text_size(px(12.))
                            .text_color(p.ink)
                            .child(text)
                    }))
                    .children(self.error.clone().map(|error| {
                        div()
                            .id("file-prompt-error")
                            .test_support()
                            .text_size(px(12.))
                            .text_color(p.accent)
                            .child(error)
                    }))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("file-prompt-cancel")
                                    .label(t!("shell.cancel"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.answer(None, cx);
                                    })),
                            )
                            .child(
                                Button::new("file-prompt-ok")
                                    .label(ok_label)
                                    .when(self.replace.is_some(), |b| b.danger())
                                    .when(self.replace.is_none(), |b| b.primary())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.confirm(window, cx);
                                    })),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
#[path = "file_prompt_tests.rs"]
mod tests;
