//! Connected accounts and explicit cloud actions; all file/network work is off-frame.
#[path = "cloud_home.rs"]
mod home;
#[path = "cloud_shared_ui.rs"]
mod shared;
use crate::file_prompt::FilePrompts;
use crate::{
    theme,
    workspace::{Screen, Workspace},
};
use emulsion_cloud::{
    Account, Index, Provider, RemoteRevision, Store,
    auth::{self, Config},
    providers,
};
use gpui_kit::{
    component::{
        Disableable, Icon, Sizable,
        button::{Button, ButtonVariants},
        menu::{DropdownMenu, PopupMenuItem},
    },
    *,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default)]
pub(crate) struct CloudUi {
    index: Option<Index>,
    config: Config,
    busy: bool,
    loaded: bool,
    polling: bool,
    note: String,
    cancelled: Option<Arc<AtomicBool>>,
    remote: Vec<(Account, RemoteRevision)>,
    page: usize,
    provider_filter: Option<Provider>,
    history: Option<(Account, String)>,
    connections_open: bool,
    ready: Option<PathBuf>,
    syncing_file: Option<(PathBuf, Provider)>,
    sync_error: Option<(PathBuf, Provider)>,
    syncing_accounts: Vec<Provider>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileSyncStatus {
    Local,
    Syncing,
    Synced,
    Queued,
    Paused,
    Error,
    Disconnected,
    Snapshot,
    Unsaved,
}
impl FileSyncStatus {
    fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Local => t!("cloud.status_local"),
            Self::Syncing => t!("cloud.syncing"),
            Self::Synced => t!("cloud.status_synced"),
            Self::Queued => t!("cloud.status_queued"),
            Self::Paused => t!("cloud.status_paused"),
            Self::Error => t!("cloud.status_error"),
            Self::Disconnected => t!("cloud.status_disconnected"),
            Self::Snapshot => t!("cloud.status_snapshot"),
            Self::Unsaved => t!("cloud.status_unsaved"),
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Local => "hard-drive",
            Self::Syncing => "cloud-sync",
            Self::Synced => "cloud-check",
            Self::Queued | Self::Snapshot => "cloud-upload",
            Self::Paused => "circle-pause",
            Self::Error => "cloud-alert",
            Self::Disconnected => "cloud-off",
            Self::Unsaved => "pencil",
        }
    }
}
impl CloudUi {
    fn file_status(&self, path: &Path, unsaved: bool) -> (Option<Provider>, FileSyncStatus) {
        if let Some((active, provider)) = &self.syncing_file
            && active == path
        {
            return (Some(*provider), FileSyncStatus::Syncing);
        }
        let binding = self
            .index
            .as_ref()
            .and_then(|i| i.bindings.iter().find(|b| b.path == path));
        if unsaved {
            return (binding.map(|b| b.provider), FileSyncStatus::Unsaved);
        }
        if let Some((failed, provider)) = &self.sync_error
            && failed == path
        {
            return (Some(*provider), FileSyncStatus::Error);
        }
        let Some(binding) = binding else {
            return (None, FileSyncStatus::Local);
        };
        let index = self.index.as_ref().unwrap();
        let jobs: Vec<_> = index
            .jobs
            .iter()
            .filter(|j| j.revision.project == binding.project)
            .collect();
        let state = if !index
            .accounts
            .iter()
            .any(|a| a.provider == binding.provider && a.id == binding.account_id)
        {
            FileSyncStatus::Disconnected
        } else if binding.paused {
            FileSyncStatus::Paused
        } else if !jobs.is_empty() && self.syncing_accounts.contains(&binding.provider) {
            FileSyncStatus::Syncing
        } else if jobs.iter().any(|j| j.error.is_some()) {
            FileSyncStatus::Error
        } else if !jobs.is_empty() {
            FileSyncStatus::Queued
        } else if binding.saved_hash.is_some() {
            FileSyncStatus::Synced
        } else {
            FileSyncStatus::Snapshot
        };
        (Some(binding.provider), state)
    }
}

#[derive(Default)]
struct Outcome {
    note: String,
    remote: Option<Vec<(Account, RemoteRevision)>>,
    ready: Option<PathBuf>,
    photos: Vec<PathBuf>,
    catalog: Option<emulsion_io::creative_library::Catalog>,
    /// Sharing states of the open storyboards, after a listing.
    shared: Option<shared::SharedStates>,
}
impl Workspace {
    pub(crate) fn cloud_home_notice(&self) -> Option<AnyElement> {
        (!self.cloud.note.is_empty()).then(|| {
            div()
                .id("home-cloud-status")
                .test_support()
                .text_sm()
                .child(self.cloud.note.clone())
                .into_any_element()
        })
    }

    /// Uses the cached index: rendering cards never reads files or contacts a provider.
    pub(crate) fn cloud_file_control(&self, path: &Path, cx: &Context<Self>) -> AnyElement {
        self.cloud_file_control_view(path, false, cx)
    }

    pub(crate) fn cloud_file_badge(&self, path: &Path, cx: &Context<Self>) -> AnyElement {
        self.cloud_file_control_view(path, true, cx)
    }

    fn cloud_file_control_view(
        &self,
        path: &Path,
        compact: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let index = self.cloud.index.as_ref();
        let binding = index.and_then(|i| i.bindings.iter().find(|b| b.path == path));
        let accounts = index.map(|i| i.accounts.as_slice()).unwrap_or_default();
        let destinations: Vec<_> = accounts
            .iter()
            .filter(|a| a.provider != Provider::GooglePhotos)
            .filter(|a| binding.is_none_or(|b| b.provider == a.provider && b.account_id == a.id))
            .map(|a| a.provider)
            .collect();
        let unsaved = self.editor.as_ref().is_some_and(|e| {
            let e = e.read(cx);
            e.editor.path.as_deref().or(e.source.as_deref()) == Some(path)
                && (e.editor.is_modified() || e.history.save_busy)
        });
        let (provider, status) = self.cloud.file_status(path, unsaved);
        let status_label = provider.map_or_else(
            || status.label().to_string(),
            |p| format!("{} · {}", p.label(), status.label()),
        );
        let palette = theme::palette(cx);
        let color = match status {
            FileSyncStatus::Synced => if palette.dark {
                rgb(0x66d9a0)
            } else {
                rgb(0x157344)
            }
            .into(),
            FileSyncStatus::Error | FileSyncStatus::Disconnected => if palette.dark {
                rgb(0xffb86b)
            } else {
                rgb(0x9b4e00)
            }
            .into(),
            FileSyncStatus::Syncing => palette.accent,
            _ => palette.muted,
        };
        if compact {
            let menu = self.cloud_file_menu(path.to_path_buf(), cx);
            return Button::new((
                ElementId::from("home-file-sync"),
                path.to_string_lossy().into_owned(),
            ))
            .accessibility_label(t!("cloud.sync_actions_a11y", status = status_label))
            .tooltip(t!("cloud.sync_actions_tooltip", status = status_label))
            .xsmall()
            .ghost()
            .size(px(24.))
            .text_color(color)
            .child(
                div()
                    .id((
                        ElementId::from("home-file-sync-status"),
                        path.to_string_lossy().into_owned(),
                    ))
                    .test_support()
                    .child(
                        Icon::empty()
                            .path(format!("icons/{}.svg", status.icon()))
                            .size(px(14.)),
                    ),
            )
            .dropdown_menu(move |popup, _, _| {
                menu(popup.item(PopupMenuItem::new(status_label.clone()).disabled(true)))
            })
            .into_any_element();
        }
        let status_row = div()
            .id((
                ElementId::from("home-details-sync-status"),
                path.to_string_lossy().into_owned(),
            ))
            .test_support()
            .flex()
            .items_center()
            .gap(px(5.))
            .text_xs()
            .text_color(color)
            .child(
                Icon::empty()
                    .path(format!("icons/{}.svg", status.icon()))
                    .size(px(14.)),
            )
            .child(status_label);
        let label = if destinations.is_empty() {
            if binding.is_some() {
                t!("cloud.reconnect_cloud")
            } else {
                t!("cloud.connect_cloud")
            }
        } else if binding.is_some_and(|b| b.paused) {
            t!("cloud.resume_sync")
        } else if binding.is_some() {
            t!("cloud.sync_now")
        } else if destinations.len() == 1 {
            t!("cloud.sync_to", provider = destinations[0].label())
        } else {
            t!("cloud.sync_to_cloud")
        };
        let button = Button::new((
            ElementId::from("home-details-sync"),
            path.to_string_lossy().into_owned(),
        ))
        .label(label)
        .icon(Icon::empty().path("icons/cloud-upload.svg"))
        .small()
        .outline()
        .tooltip(t!("cloud.sync_tooltip"))
        .disabled(self.cloud.busy || !self.cloud.loaded);
        let path = path.to_path_buf();
        let button = if destinations.len() == 1 {
            let provider = destinations[0];
            button
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.cloud_sync_file(path.clone(), provider, cx)
                }))
                .into_any_element()
        } else if destinations.is_empty() {
            button
                .on_click(cx.listener(|this, _, window, cx| {
                    this.cloud.connections_open = true;
                    this.open_cloud_home(window, cx);
                }))
                .into_any_element()
        } else {
            let owner = cx.weak_entity();
            button
                .dropdown_menu(move |mut menu, _, _| {
                    for provider in &destinations {
                        let provider = *provider;
                        let owner = owner.clone();
                        let path = path.clone();
                        menu = menu.item(
                            PopupMenuItem::new(t!("cloud.sync_to", provider = provider.label()))
                                .on_click(move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.cloud_sync_file(path.clone(), provider, cx)
                                        })
                                        .ok();
                                }),
                        );
                    }
                    menu
                })
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .min_w_0()
            .child(button)
            .child(status_row)
            .into_any_element()
    }

    fn cloud_sync_file(&mut self, path: PathBuf, provider: Provider, cx: &mut Context<Self>) {
        if self.cloud.busy {
            return;
        }
        if let Some(editor) = &self.editor {
            let e = editor.read(cx);
            let open = e.editor.path.as_ref().or(e.source.as_ref());
            let same_file = open.is_some_and(|open| {
                open == &path
                    || open
                        .canonicalize()
                        .ok()
                        .zip(path.canonicalize().ok())
                        .is_some_and(|(a, b)| a == b)
            });
            if same_file && (e.history.save_busy || e.editor.is_modified()) {
                self.cloud.note = t!("cloud.save_before_sync").into_owned();
                cx.notify();
                return;
            }
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.cloud.note = t!(
            "cloud.syncing_file",
            name = name,
            provider = provider.label()
        )
        .into_owned();
        self.cloud.syncing_file = Some((path.clone(), provider));
        if self
            .cloud
            .sync_error
            .as_ref()
            .is_some_and(|(failed, _)| failed == &path)
        {
            self.cloud.sync_error = None;
        }
        let mut rows: Vec<_> = self
            .cloud
            .remote
            .iter()
            .filter(|(a, _)| a.provider != provider)
            .cloned()
            .collect();
        let paths = self.shared_paths(cx);
        self.cloud_task(
            move |store| {
                store.bind(&path, provider)?;
                emulsion_io::cloud::enqueue(&store, &path)?;
                let canonical = path.canonicalize()?;
                let account = store.update(|index| {
                    let binding = index
                        .bindings
                        .iter()
                        .find(|b| b.path == canonical)
                        .ok_or_else(|| anyhow::anyhow!(t!("cloud.err_settings_changed")))?;
                    for job in &mut index.jobs {
                        if job.revision.project == binding.project {
                            job.retry_at = 0;
                        }
                    }
                    index
                        .accounts
                        .iter()
                        .find(|a| a.provider == provider && a.id == binding.account_id)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!(t!("cloud.err_reconnect_account")))
                })?;
                let transfer = providers::connected(&store, &account)
                    .and_then(|files| providers::synchronize(&store, &account, &files));
                let remote = match transfer {
                    Ok(remote) => remote,
                    Err(error) => {
                        // Account lookup/listing can fail before the uploader records a job
                        // error. Keep the selected file's status accurate across restarts.
                        let index = store.read()?;
                        if let Some(binding) = index.bindings.iter().find(|b| b.path == canonical) {
                            for job in index.jobs.iter().filter(|j| {
                                j.revision.project == binding.project && j.error.is_none()
                            }) {
                                store.failed(job, error.to_string())?;
                            }
                        }
                        return Err(error);
                    }
                };
                rows.extend(remote.into_iter().map(|r| (account.clone(), r)));
                let index = store.read()?;
                let binding = index
                    .bindings
                    .iter()
                    .find(|b| b.path == canonical)
                    .ok_or_else(|| anyhow::anyhow!(t!("cloud.err_settings_changed")))?;
                let pending = index
                    .jobs
                    .iter()
                    .any(|j| j.revision.project == binding.project);
                let note = if binding.paused {
                    t!("cloud.sync_paused_file", name = name)
                } else if pending {
                    t!(
                        "cloud.queued_file",
                        name = name,
                        provider = provider.label()
                    )
                } else {
                    t!(
                        "cloud.synced_file",
                        name = name,
                        provider = provider.label()
                    )
                }
                .into_owned();
                Ok(Outcome {
                    note,
                    shared: Some(Self::shared_states(&store, &rows, &paths)),
                    remote: Some(rows),
                    ..Default::default()
                })
            },
            cx,
        );
    }

    fn cloud_task(
        &mut self,
        work: impl FnOnce(Store) -> anyhow::Result<Outcome> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.cloud.busy {
            return;
        }
        self.cloud.busy = true;
        cx.spawn(async move |this, cx| {
            let (result, index, config) = cx
                .background_spawn(async move {
                    let store = emulsion_io::cloud::store();
                    let result = work(store.clone());
                    (result, store.read(), Config::load(&store.root))
                })
                .await;
            this.update(cx, |this, cx| {
                this.cloud.busy = false;
                let syncing = this.cloud.syncing_file.take();
                this.cloud.syncing_accounts.clear();
                this.cloud.cancelled = None;
                this.cloud.loaded = true;
                if let Ok(index) = index {
                    this.cloud.index = Some(index);
                }
                if let Ok(config) = config {
                    this.cloud.config = config;
                }
                if result.is_err()
                    && let Some((path, provider)) = syncing
                {
                    let has_queued_error = this.cloud.index.as_ref().is_some_and(|i| {
                        i.bindings.iter().find(|b| b.path == path).is_some_and(|b| {
                            i.jobs
                                .iter()
                                .any(|j| j.revision.project == b.project && j.error.is_some())
                        })
                    });
                    if !has_queued_error {
                        this.cloud.sync_error = Some((path, provider));
                    }
                }
                match result {
                    Ok(outcome) => {
                        if let Some(catalog) = outcome.catalog
                            && catalog.revision >= this.home_state.projects.catalog.revision
                        {
                            this.home_state.projects.catalog = catalog;
                        }
                        this.cloud.note = outcome.note;
                        if let Some(remote) = outcome.remote {
                            this.cloud.remote = remote;
                        }
                        if let Some(ready) = outcome.ready {
                            this.cloud.ready = Some(ready);
                        }
                        if let Some(states) = outcome.shared {
                            this.apply_shared_states(states, cx);
                        }
                        if !outcome.photos.is_empty() {
                            let dir = outcome.photos[0].parent().unwrap().to_path_buf();
                            this.load_batch(dir, outcome.photos, cx);
                            this.batch.note = Some((this.cloud.note.clone().into(), false));
                            this.refresh_imported_photo_library(cx);
                            this.back_to = if this.editor.is_some() {
                                Screen::Editor
                            } else {
                                Screen::Home
                            };
                            this.screen = Screen::Batch;
                        }
                    }
                    Err(error) => {
                        this.cloud.note = error.to_string();
                        this.shared_check_failed(&error.to_string(), cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn cloud_load(&mut self, cx: &mut Context<Self>) {
        if !self.cloud.loaded {
            self.cloud_task(|_| Ok(Outcome::default()), cx);
        }
        if !self.cloud.polling {
            self.cloud.polling = true;
            // Test executors advance timers eagerly; live network polling belongs to production.
            #[cfg(not(test))]
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(60))
                        .await;
                    if this
                        .update(cx, |this, cx| this.cloud_sync(false, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
    }
    pub(crate) fn cloud_sync(&mut self, retry: bool, cx: &mut Context<Self>) {
        if self.cloud.busy
            || self.cloud.index.as_ref().is_none_or(|i| {
                !i.accounts
                    .iter()
                    .any(|a| a.provider != Provider::GooglePhotos)
            })
        {
            return;
        }
        self.cloud.note = t!("cloud.checking_revisions").into_owned();
        self.cloud.syncing_accounts = self
            .cloud
            .index
            .as_ref()
            .unwrap()
            .accounts
            .iter()
            .filter(|a| a.provider != Provider::GooglePhotos)
            .map(|a| a.provider)
            .collect();
        let paths = self.shared_paths(cx);
        let author = crate::app_state::settings(cx)
            .storyboard
            .review_author
            .trim()
            .to_string();
        self.cloud_task(
            move |store| {
                if retry {
                    store.retry()?;
                }
                // New revisions carry the artist's name from Settings.
                let _ = store.set_author(Some(&author));
                let accounts = store.read()?.accounts;
                let mut rows = vec![];
                let mut errors = emulsion_io::cloud::home::enqueue_changes(&store)?;
                for account in accounts
                    .into_iter()
                    .filter(|a| a.provider != Provider::GooglePhotos)
                {
                    let result = providers::connected(&store, &account)
                        .and_then(|provider| providers::synchronize(&store, &account, &provider));
                    match result {
                        Ok(remote) => rows.extend(remote.into_iter().map(|r| (account.clone(), r))),
                        Err(error) => errors.push(format!("{}: {error}", account.provider.label())),
                    }
                }
                let pending = store.read()?.jobs.len();
                let note = if errors.is_empty() {
                    if pending == 0 {
                        t!("cloud.up_to_date").into_owned()
                    } else {
                        crate::home::recency::plural(
                            pending,
                            "cloud.uploads_queued_one",
                            "cloud.uploads_queued_many",
                        )
                    }
                } else {
                    errors.join(" · ")
                };
                Ok(Outcome {
                    note,
                    shared: Some(Self::shared_states(&store, &rows, &paths)),
                    remote: Some(rows),
                    ..Default::default()
                })
            },
            cx,
        );
    }
    fn cloud_import_config(&mut self, cx: &mut Context<Self>) {
        if self.cloud.busy {
            return;
        }
        let rx = cx.prompt_open_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("cloud.choose_registration").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| {
                    this.cloud_task(
                        move |store| {
                            Config::import(&store, &path)?;
                            Ok(Outcome {
                                note: t!("cloud.registration_saved").into_owned(),
                                ..Default::default()
                            })
                        },
                        cx,
                    )
                })
                .ok();
            }
        })
        .detach();
    }
    fn cloud_connect(&mut self, provider: Provider, cx: &mut Context<Self>) {
        if self.cloud.busy {
            return;
        }
        let Some(client) = self.cloud.config.clients.get(&provider).cloned() else {
            self.cloud.note = t!("cloud.no_registration").into_owned();
            cx.notify();
            return;
        };
        match auth::PendingLogin::start(provider, client) {
            Ok(pending) => {
                self.cloud.cancelled = Some(pending.cancelled.clone());
                cx.open_url(&pending.url);
                self.cloud.note =
                    t!("cloud.finish_sign_in", provider = provider.label()).into_owned();
                self.cloud_task(
                    move |store| {
                        let (mut account, tokens) = pending.finish()?;
                        providers::Files::initialize(&mut account, &tokens.access_token).map_err(
                            |e| {
                                anyhow::anyhow!(t!(
                                    "cloud.err_storage_setup",
                                    provider = provider.label(),
                                    error = e
                                ))
                            },
                        )?;
                        // Replacement never transfers bindings or work to a different account.
                        if let Some(old) = store
                            .read()?
                            .accounts
                            .into_iter()
                            .find(|a| a.provider == provider && a.id != account.id)
                        {
                            auth::forget(&old)?;
                        }
                        account.persistent_credentials = auth::save(&account, &tokens);
                        let note = if account.persistent_credentials {
                            t!("cloud.connected_persistent", account = account.label)
                        } else {
                            t!("cloud.connected_session", account = account.label)
                        }
                        .into_owned();
                        store.connect(account).map_err(|e| {
                            anyhow::anyhow!(t!("cloud.err_saving_connection", error = e))
                        })?;
                        Ok(Outcome {
                            note,
                            remote: Some(vec![]),
                            ..Default::default()
                        })
                    },
                    cx,
                );
            }
            Err(error) => {
                self.cloud.note = error.to_string();
                cx.notify();
            }
        }
    }
    fn cloud_disconnect(&mut self, account: Account, cx: &mut Context<Self>) {
        self.cloud_task(
            move |store| {
                auth::forget(&account)?;
                store.disconnect(account.provider)?;
                Ok(Outcome {
                    note: t!("cloud.disconnected").into_owned(),
                    remote: Some(vec![]),
                    ..Default::default()
                })
            },
            cx,
        );
    }
    fn cloud_pause(&mut self, path: PathBuf, paused: bool, cx: &mut Context<Self>) {
        self.cloud_task(
            move |store| {
                store.set_paused(&path, paused)?;
                Ok(Outcome {
                    note: if paused {
                        t!("cloud.uploads_paused").into_owned()
                    } else {
                        t!("cloud.uploads_resumed").into_owned()
                    },
                    ..Default::default()
                })
            },
            cx,
        );
    }
    fn cloud_download(&mut self, account: Account, remote: RemoteRevision, cx: &mut Context<Self>) {
        self.cloud.note = t!("cloud.downloading", name = remote.revision.name).into_owned();
        self.cloud_task(
            move |store| {
                let provider = providers::connected(&store, &account)?;
                let path = emulsion_io::cloud::shared::download_copy(
                    &store, &account, &provider, &remote,
                )?;
                let catalog = emulsion_io::cloud::home::restore(
                    &emulsion_io::creative_library::root(),
                    &path,
                    &remote.revision,
                )?;
                Ok(Outcome {
                    note: t!("cloud.downloaded").into_owned(),
                    ready: Some(path),
                    catalog: Some(catalog),
                    ..Default::default()
                })
            },
            cx,
        );
    }
    pub(crate) fn cloud_import_photos(&mut self, cx: &mut Context<Self>) {
        if self.cloud.busy {
            return;
        }
        let account = self
            .cloud
            .index
            .as_ref()
            .and_then(|i| {
                i.accounts
                    .iter()
                    .find(|a| a.provider == Provider::GooglePhotos)
            })
            .cloned();
        let Some(account) = account else {
            self.cloud.note = t!("cloud.connect_photos_first").into_owned();
            self.home_state.cloud_files = true;
            self.cloud.connections_open = true;
            self.screen = Screen::Home;
            cx.notify();
            return;
        };
        self.cloud.busy = true;
        self.cloud.note = t!("cloud.opening_photos").into_owned();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.cloud.cancelled = Some(cancelled.clone());
        cx.spawn(async move |this, cx| {
            let start = cx
                .background_spawn(async move {
                    let store = emulsion_io::cloud::store();
                    let token = auth::access(&store, &account)?;
                    let session = emulsion_cloud::photos::begin(&token)?;
                    Ok::<_, anyhow::Error>((token, session))
                })
                .await;
            match start {
                Ok((token, session)) => {
                    this.update(cx, |this, cx| {
                        this.cloud.busy = false;
                        if cancelled.load(Ordering::Relaxed) {
                            this.cloud_task(
                                move |_| {
                                    emulsion_cloud::photos::remove(&token, &session)?;
                                    Ok(Outcome {
                                        note: t!("cloud.photo_import_cancelled").into_owned(),
                                        ..Default::default()
                                    })
                                },
                                cx,
                            );
                            return;
                        }
                        cx.open_url(&session.picker_uri);
                        this.cloud.note = t!("cloud.select_photos").into_owned();
                        this.cloud_task(
                            move |store| {
                                let destination = store.root.join("photo-imports");
                                let report = emulsion_cloud::photos::import(
                                    &token,
                                    &session,
                                    &destination,
                                    &cancelled,
                                    |p| {
                                        emulsion_io::open(p)?;
                                        Ok(())
                                    },
                                )?;
                                emulsion_io::creative_library::update(
                                    &emulsion_io::creative_library::root(),
                                    |catalog| {
                                        for path in &report.paths {
                                            catalog.add_asset(
                                                path.clone(),
                                                emulsion_io::creative_library::AssetKind::Image,
                                            )?;
                                        }
                                        Ok(())
                                    },
                                )?;
                                Ok(Outcome {
                                    note: t!(
                                        "cloud.imported_photos",
                                        count = report.paths.len(),
                                        skipped = report.skipped,
                                        failed = report.failed,
                                        note = report.note
                                    )
                                    .into_owned(),
                                    photos: report.paths,
                                    ..Default::default()
                                })
                            },
                            cx,
                        );
                    })
                    .ok();
                }
                Err(error) => {
                    this.update(cx, |this, cx| {
                        this.cloud.busy = false;
                        this.cloud.cancelled = None;
                        this.cloud.note = error.to_string();
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn cloud_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let busy = self.cloud.busy;
        let accounts = self
            .cloud
            .index
            .as_ref()
            .map(|i| i.accounts.clone())
            .unwrap_or_default();
        let mut panel = div()
            .id("cloud-settings")
            .test_support()
            .px(px(40.))
            .py(px(24.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .border_b_1()
            .border_color(p.line)
            .child(div().text_xl().child(t!("cloud.connections_title")))
            .child(
                div()
                    .text_sm()
                    .text_color(p.muted)
                    .child(t!("cloud.connections_body")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(p.muted)
                    .child(t!("cloud.connections_note")),
            )
            .child(
                div().flex().flex_wrap().gap(px(8.)).child(
                    Button::new("cloud-registration")
                        .label(t!("cloud.import_registration"))
                        .small()
                        .outline()
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.cloud_import_config(cx))),
                ),
            );
        if !self.cloud.note.is_empty() {
            panel = panel.child(div().text_sm().child(self.cloud.note.clone()));
        }
        for (provider_index, provider) in Provider::ALL.into_iter().enumerate() {
            let account = accounts.iter().find(|a| a.provider == provider).cloned();
            let configured = self.cloud.config.clients.contains_key(&provider);
            let label = account
                .as_ref()
                .map(|a| {
                    format!(
                        "{} · {}{}",
                        provider.label(),
                        a.label,
                        if a.persistent_credentials {
                            String::new()
                        } else {
                            format!(" · {}", t!("cloud.session_only"))
                        }
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "{} · {}",
                        provider.label(),
                        if configured {
                            t!("cloud.not_connected")
                        } else {
                            t!("cloud.registration_needed")
                        }
                    )
                });
            let mut row = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(div().min_w(px(210.)).text_sm().child(label));
            row = row.child(
                Button::new(("cloud-connect", provider_index))
                    .label(if account.is_some() {
                        t!("cloud.reconnect")
                    } else {
                        t!("cloud.connect")
                    })
                    .small()
                    .outline()
                    .disabled(busy || !configured)
                    .on_click(cx.listener(move |this, _, _, cx| this.cloud_connect(provider, cx))),
            );
            if let Some(account) = account {
                row = row.child(
                    Button::new(("cloud-disconnect", provider_index))
                        .label(t!("cloud.disconnect"))
                        .small()
                        .outline()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cloud_disconnect(account.clone(), cx)
                        })),
                );
                if provider == Provider::GooglePhotos {
                    row = row.child(
                        Button::new("cloud-photos-import")
                            .label(t!("cloud.import_photos"))
                            .small()
                            .outline()
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.cloud_import_photos(cx))),
                    );
                }
            }
            panel = panel.child(row);
        }
        if let Some(cancelled) = self.cloud.cancelled.clone() {
            panel = panel.child(
                Button::new("cloud-cancel")
                    .label(t!("cloud.cancel"))
                    .small()
                    .outline()
                    .on_click(move |_, _, _| cancelled.store(true, Ordering::Relaxed)),
            );
        }
        if let Some(path) = self.cloud.ready.clone() {
            panel = panel.child(
                Button::new("cloud-open-download")
                    .label(t!("cloud.open_download"))
                    .small()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_path(path.clone(), window, cx)
                    })),
            );
        }
        panel.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt;

    fn drive_account() -> Account {
        Account {
            provider: Provider::GoogleDrive,
            id: "test-account".into(),
            registration: "test-client".into(),
            label: "Test Drive".into(),
            root: "test-folder".into(),
            persistent_credentials: false,
        }
    }

    #[test]
    fn card_status_tracks_durable_queue_and_never_marks_failed_uploads_synced() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path().join("cloud"));
        let path = temp.path().join("photo.png");
        std::fs::write(&path, b"test snapshot").unwrap();
        let path = path.canonicalize().unwrap();
        let account = drive_account();
        store.connect(account.clone()).unwrap();
        let mut ui = CloudUi::default();
        assert_eq!(ui.file_status(&path, false), (None, FileSyncStatus::Local));
        store.bind(&path, account.provider).unwrap();
        store.enqueue(&path, &path).unwrap();
        ui.index = Some(store.read().unwrap());
        assert_eq!(
            ui.file_status(&path, false),
            (Some(Provider::GoogleDrive), FileSyncStatus::Queued)
        );
        let job = store.pending(&account).unwrap().remove(0);
        store.failed(&job, "offline".into()).unwrap();
        ui.index = Some(Store::new(&store.root).read().unwrap());
        assert_eq!(ui.file_status(&path, false).1, FileSyncStatus::Error);
        store.set_paused(&path, true).unwrap();
        ui.index = Some(store.read().unwrap());
        assert_eq!(ui.file_status(&path, false).1, FileSyncStatus::Paused);
        store.set_paused(&path, false).unwrap();
        store.complete(&job).unwrap();
        ui.index = Some(store.read().unwrap());
        assert_eq!(ui.file_status(&path, false).1, FileSyncStatus::Synced);
        assert_eq!(ui.file_status(&path, true).1, FileSyncStatus::Unsaved);
        ui.sync_error = Some((path.clone(), Provider::GoogleDrive));
        assert_eq!(ui.file_status(&path, false).1, FileSyncStatus::Error);
        ui.syncing_file = Some((path.clone(), Provider::GoogleDrive));
        assert_eq!(ui.file_status(&path, false).1, FileSyncStatus::Syncing);
        ui.syncing_file = None;
        ui.sync_error = None;
        store
            .connect(Account {
                id: "different-account".into(),
                ..account
            })
            .unwrap();
        ui.index = Some(store.read().unwrap());
        assert_eq!(ui.file_status(&path, false).1, FileSyncStatus::Disconnected);
    }

    #[gpui_kit::test]
    fn card_sync_is_visible_in_grid_and_list_and_keeps_unsaved_work_local(cx: &mut TestAppContext) {
        let (workspace, cx) = crate::tests::open(cx, emulsion_core::Document::new(32, 32));
        cx.run_until_parked();
        cx.simulate_resize(size(px(1440.), px(1000.)));
        let path = PathBuf::from("/tmp/emulsion-card-sync-unsaved.png");
        cx.update(|window, cx| {
            workspace.update(cx, |this, cx| {
                this.cloud = CloudUi {
                    loaded: true,
                    index: Some(Index {
                        accounts: vec![drive_account()],
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                this.editor.as_ref().unwrap().update(cx, |editor, _| {
                    editor.editor.path = Some(path.clone());
                    editor.history.save_busy = true;
                });
                this.recents = vec![emulsion_io::recent::Recent {
                    path: path.clone(),
                    opened: emulsion_io::recent::now(),
                    summary: String::new(),
                }];
                this.recovered.clear();
                this.set_screen(Screen::Home, window, cx);
            })
        });
        cx.run_until_parked();
        for rows in [false, true] {
            if rows {
                cx.update(|window, cx| window.click("home-list", cx));
                cx.run_until_parked();
            }
            cx.update(|window, cx| {
                let id = (
                    ElementId::from("home-file-sync"),
                    path.to_string_lossy().into_owned(),
                );
                assert!(window.find(id.clone()).visible());
                assert!(
                    window
                        .find((
                            ElementId::from("home-file-sync-status"),
                            path.to_string_lossy().into_owned()
                        ))
                        .visible()
                );
                window.click(id, cx);
            });
            cx.run_until_parked();
            cx.update(|window, cx| window.within("popup-menu").click(1usize, cx));
            cx.run_until_parked();
            cx.update(|window, cx| {
                let this = workspace.read(cx);
                assert_eq!(this.screen, Screen::Home);
                assert!(!this.cloud.busy);
                assert!(this.cloud.note.contains("Save the open file"));
                assert!(window.find("home-cloud-status").visible());
            });
        }
    }

    #[gpui_kit::test]
    fn cloud_home_connections_explain_missing_registrations(cx: &mut TestAppContext) {
        let (workspace, cx) = crate::tests::open(cx, emulsion_core::Document::new(32, 32));
        cx.run_until_parked();
        cx.simulate_resize(size(px(1440.), px(1100.)));
        cx.update(|window, cx| {
            workspace.update(cx, |this, cx| {
                this.cloud = CloudUi {
                    loaded: true,
                    index: Some(Index::default()),
                    ..Default::default()
                };
                {
                    this.cloud.connections_open = true;
                    this.open_cloud_home(window, cx);
                };
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("cloud-settings").visible());
            assert!(window.find("cloud-registration").visible());
            window.click(("cloud-connect", 0usize), cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(!workspace.read(cx).cloud.busy);
            assert!(workspace.read(cx).cloud.cancelled.is_none());
        });
    }

    #[gpui_kit::test]
    fn photos_without_account_routes_to_setup_without_starting_network(cx: &mut TestAppContext) {
        let (workspace, cx) = crate::tests::open(cx, emulsion_core::Document::new(32, 32));
        cx.run_until_parked();
        cx.update(|_, cx| {
            workspace.update(cx, |this, cx| {
                this.cloud.index = Some(Index::default());
                this.cloud_import_photos(cx);
                assert_eq!(this.screen, Screen::Home);
                assert!(this.home_state.cloud_files);
                assert!(!this.cloud.busy);
                assert!(this.cloud.note.contains("Connect Google Photos"));
            })
        });
    }
}
