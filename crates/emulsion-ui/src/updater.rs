//! In-app updates: a launch-time check of the latest GitHub release, a
//! verified background download, and installation when Emulsion restarts.
//! Unsaved work is never discarded: restarting goes through the normal quit
//! confirmation, and the installer or new AppImage starts only after exit.

use crate::app_state;
use crate::theme::Palette;
use crate::widgets::{chip, mono};
use crate::workspace::Workspace;
use emulsion_io::update::{self, Install, Release};
use gpui_kit::*;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading {
        release: Release,
        done: u64,
        total: u64,
    },
    /// Downloaded and verified. An AppImage is already replaced; an
    /// installer waits for exit.
    Ready {
        release: Release,
        file: PathBuf,
    },
    Failed(String),
}

struct Updater {
    status: Status,
    install: Install,
    cancel: Arc<AtomicBool>,
}
impl Global for Updater {}

/// What to start once the application has quit.
static AFTER_EXIT: Mutex<Option<PathBuf>> = Mutex::new(None);

fn state(cx: &mut App) -> &mut Updater {
    if !cx.has_global::<Updater>() {
        cx.set_global(Updater {
            status: Status::Idle,
            install: Install::detect(),
            cancel: Arc::new(AtomicBool::new(false)),
        });
    }
    cx.global_mut::<Updater>()
}

fn set_status(cx: &mut App, status: Status) {
    state(cx).status = status;
    cx.refresh_windows();
}

pub fn status(cx: &App) -> Status {
    cx.try_global::<Updater>()
        .map_or(Status::Idle, |u| u.status.clone())
}

/// Whether this copy can download and install a release itself.
fn installable(cx: &mut App, release: &Release) -> bool {
    state(cx).install.asset_name(release.version()).is_some()
}

/// Check once per launch when automatic updates are on. Development builds
/// never check on their own.
pub fn start(cx: &mut App) {
    if cfg!(debug_assertions) || !app_state::settings(cx).auto_update {
        return;
    }
    if matches!(status(cx), Status::Idle) {
        check(cx, true);
    }
}

/// Ask GitHub for the latest release; with `download`, fetch it when newer.
pub fn check(cx: &mut App, download: bool) {
    if matches!(
        status(cx),
        Status::Checking | Status::Downloading { .. } | Status::Ready { .. }
    ) {
        return;
    }
    set_status(cx, Status::Checking);
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async { update::latest_release() })
            .await;
        cx.update(|cx| match result {
            Ok(release) if release.is_newer_than(update::CURRENT_VERSION) => {
                let fetch = download && installable(cx, &release);
                set_status(cx, Status::Available(release));
                if fetch {
                    start_download(cx);
                }
            }
            Ok(_) => set_status(cx, Status::UpToDate),
            Err(error) => {
                tracing::warn!(%error, "update check failed");
                set_status(cx, Status::Failed(format!("Update check failed: {error}")));
            }
        });
    })
    .detach();
}

/// Download the available release, verify it and stage it for install.
pub fn start_download(cx: &mut App) {
    let Status::Available(release) = status(cx) else {
        return;
    };
    let install = state(cx).install.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    state(cx).cancel = cancel.clone();
    set_status(
        cx,
        Status::Downloading {
            release: release.clone(),
            done: 0,
            total: 0,
        },
    );
    let (tx, rx) = async_channel::unbounded::<(u64, u64)>();
    let task = {
        let release = release.clone();
        cx.background_spawn(async move {
            let file = update::download(
                &release,
                &install,
                &|done, total| {
                    let _ = tx.try_send((done, total));
                },
                &cancel,
            )?;
            if let Install::AppImage(target) = &install {
                update::replace_appimage(&file, target)?;
                return Ok(target.clone());
            }
            Ok::<_, update::UpdateError>(file)
        })
    };
    cx.spawn(async move |cx| {
        // The sender is dropped when the download finishes.
        while let Ok((done, total)) = rx.recv().await {
            cx.update(|cx| {
                if let Status::Downloading { release, .. } = status(cx) {
                    set_status(
                        cx,
                        Status::Downloading {
                            release,
                            done,
                            total,
                        },
                    );
                }
            });
        }
        let result = task.await;
        cx.update(|cx| match result {
            Ok(file) => set_status(cx, Status::Ready { release, file }),
            Err(update::UpdateError::Cancelled) => set_status(cx, Status::Available(release)),
            Err(error) => {
                tracing::warn!(%error, "update download failed");
                set_status(cx, Status::Failed(format!("Update failed: {error}")));
            }
        });
    })
    .detach();
}

pub fn cancel_download(cx: &mut App) {
    state(cx).cancel.store(true, Ordering::Relaxed);
}

/// Start whatever the update left for after exit: the relaunched AppImage or
/// the Windows installer. Called by the binary once the app has quit.
pub fn run_after_exit() {
    let Some(program) = AFTER_EXIT.lock().take() else {
        return;
    };
    if let Err(error) = std::process::Command::new(&program)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        tracing::warn!(%error, program = %program.display(), "could not start the update");
    }
}

impl Workspace {
    /// Finish a staged update: quit (asking about unsaved work as usual) and
    /// start the new version or its installer, or open the macOS disk image.
    fn apply_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Status::Ready { file, .. } = status(cx) else {
            return;
        };
        if matches!(state(cx).install, Install::MacApp(_)) {
            // Finder mounts the image; dragging Emulsion to Applications
            // replaces this copy.
            let _ = std::process::Command::new("open").arg(&file).spawn();
            return;
        }
        *AFTER_EXIT.lock() = Some(file);
        self.quit(window, cx);
    }

    /// A top-bar notice while an update is available or ready.
    pub(crate) fn update_notice(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let text = match status(cx) {
            Status::Available(r) => format!("v{} available", r.version()),
            Status::Downloading { release, .. } => format!("downloading v{}…", release.version()),
            Status::Ready { release, .. } => format!("restart to update to v{}", release.version()),
            _ => return None,
        };
        Some(
            div()
                .id("update-notice")
                .flex()
                .items_center()
                .px(px(12.))
                .border_l_1()
                .border_color(p.chrome_line)
                .bg(p.accent)
                .text_color(p.accent_fg)
                .font_family(crate::theme::MONO_FONT)
                .text_size(px(10.))
                .cursor_pointer()
                .child(text)
                .on_click(cx.listener(|this, _, window, cx| {
                    if matches!(status(cx), Status::Ready { .. }) {
                        this.apply_update(window, cx);
                    } else {
                        this.cancel_style_dialog(window, cx);
                        this.set_screen(crate::workspace::Screen::Settings, window, cx);
                        cx.notify();
                    }
                }))
                .into_any_element(),
        )
    }

    /// The Updates section of the Settings screen.
    pub(crate) fn updates_settings(&mut self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let auto = app_state::settings(cx).auto_update;
        let status = status(cx);
        let install = state(cx).install.clone();
        let line = match &status {
            Status::Idle => format!("Emulsion {} · not checked yet", update::CURRENT_VERSION),
            Status::Checking => "Looking for a new release…".into(),
            Status::UpToDate => format!(
                "Emulsion {} is the latest release.",
                update::CURRENT_VERSION
            ),
            Status::Available(r) if install.asset_name(r.version()).is_some() => {
                format!("Emulsion {} is available.", r.version())
            }
            Status::Available(r) => format!(
                "Emulsion {} is available. This copy was not installed from a release package, so download it from the release page.",
                r.version()
            ),
            Status::Downloading {
                release,
                done,
                total,
            } => format!(
                "Downloading Emulsion {} · {} of {} MB",
                release.version(),
                done / 1_000_000,
                total / 1_000_000
            ),
            Status::Ready { release, .. } => match install {
                Install::AppImage(_) => format!(
                    "Emulsion {} is installed and starts the next time you open Emulsion.",
                    release.version()
                ),
                Install::MacApp(_) => format!(
                    "Emulsion {} is downloaded and verified. Open the disk image and drag Emulsion to Applications.",
                    release.version()
                ),
                _ => format!(
                    "Emulsion {} is downloaded and verified. Its installer runs when you restart.",
                    release.version()
                ),
            },
            Status::Failed(message) => message.clone(),
        };
        let release_page = match &status {
            Status::Available(r)
            | Status::Downloading { release: r, .. }
            | Status::Ready { release: r, .. } => Some(r.html_url.clone()),
            _ => None,
        };
        let mut actions = div().flex().flex_wrap().items_center().gap(px(8.)).child(
            chip("update-auto", "check and download automatically", auto, p).on_click(cx.listener(
                |_, _, _, cx| {
                    app_state::update_settings(cx, |s| s.auto_update = !s.auto_update);
                },
            )),
        );
        actions = match &status {
            Status::Available(r) if install.asset_name(r.version()).is_some() => actions.child(
                chip("update-download", "download and install", true, p)
                    .on_click(cx.listener(|_, _, _, cx| start_download(cx))),
            ),
            Status::Downloading { .. } => actions.child(
                chip("update-cancel", "cancel", false, p)
                    .on_click(cx.listener(|_, _, _, cx| cancel_download(cx))),
            ),
            Status::Ready { .. } => actions.child(
                chip(
                    "update-apply",
                    if matches!(install, Install::MacApp(_)) {
                        "open disk image"
                    } else {
                        "restart now"
                    },
                    true,
                    p,
                )
                .on_click(cx.listener(|this, _, window, cx| this.apply_update(window, cx))),
            ),
            Status::Checking => actions,
            _ => actions.child(
                chip("update-check", "check now", false, p)
                    .on_click(cx.listener(|_, _, _, cx| check(cx, false))),
            ),
        };
        if let Some(url) = release_page {
            actions = actions.child(
                chip("update-notes", "release notes", false, p)
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(40.))
            .py(px(24.))
            .border_b_1()
            .border_color(p.line)
            .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child("Updates"))
            .child(
                div()
                    .max_w(px(640.))
                    .text_size(px(13.))
                    .text_color(p.muted)
                    .child(format!(
                        "New versions come from the GitHub releases of {}. Downloads are checked against their published SHA-256 checksum before anything is replaced, and nothing is installed until you restart.",
                        update::REPOSITORY
                    )),
            )
            .child(mono(
                line,
                10.5,
                if matches!(status, Status::Failed(_)) { p.accent } else { p.ink },
            ))
            .child(actions)
    }
}
