//! Connected accounts and explicit cloud actions; all file/network work is off-frame.
use crate::{
    theme,
    workspace::{Screen, Workspace},
};
use emulsion_cloud::{
    Account, Index, Provider, RemoteRevision, Store,
    auth::{self, Config},
    providers::{self, FileProvider},
    store,
};
use gpui_kit::{
    component::{Disableable, Sizable, button::Button},
    *,
};
use std::{
    path::PathBuf,
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
    ready: Option<PathBuf>,
}

#[derive(Default)]
struct Outcome {
    note: String,
    remote: Option<Vec<(Account, RemoteRevision)>>,
    ready: Option<PathBuf>,
    photos: Vec<PathBuf>,
}
impl Workspace {
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
                this.cloud.cancelled = None;
                this.cloud.loaded = true;
                if let Ok(index) = index {
                    this.cloud.index = Some(index);
                }
                if let Ok(config) = config {
                    this.cloud.config = config;
                }
                match result {
                    Ok(outcome) => {
                        this.cloud.note = outcome.note;
                        if let Some(remote) = outcome.remote {
                            this.cloud.remote = remote;
                            this.cloud.page = 0;
                        }
                        if let Some(ready) = outcome.ready {
                            this.cloud.ready = Some(ready);
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
                    Err(error) => this.cloud.note = error.to_string(),
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
        self.cloud.note = "Checking cloud revisions…".into();
        self.cloud_task(
            move |store| {
                if retry {
                    store.retry()?;
                }
                let accounts = store.read()?.accounts;
                let mut rows = vec![];
                let mut errors = vec![];
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
                    format!("Cloud checked · {pending} pending revision(s).")
                } else {
                    errors.join(" · ")
                };
                Ok(Outcome {
                    note,
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
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose OAuth registration JSON".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await && let Some(path) = paths.into_iter().next() {
                this.update(cx, |this, cx| this.cloud_task(move |store| {
                    Config::import(&store, &path)?;
                    Ok(Outcome { note:"Provider registration saved on this device. Choose Connect to sign in.".into(), ..Default::default() })
                }, cx)).ok();
            }
        }).detach();
    }
    fn cloud_connect(&mut self, provider: Provider, cx: &mut Context<Self>) {
        if self.cloud.busy {
            return;
        }
        let Some(client) = self.cloud.config.clients.get(&provider).cloned() else {
            self.cloud.note = "This build has no provider registration. Import a Desktop OAuth registration JSON; see the cloud setup guide.".into();
            cx.notify();
            return;
        };
        match auth::PendingLogin::start(provider, client) {
            Ok(pending) => {
                self.cloud.cancelled = Some(pending.cancelled.clone());
                cx.open_url(&pending.url);
                self.cloud.note = format!("Finish {} sign-in in your browser…", provider.label());
                self.cloud_task(move |store| {
                    let (mut account, tokens) = pending.finish()?;
                    providers::Files::initialize(&mut account, &tokens.access_token)
                        .map_err(|e| anyhow::anyhow!("{} storage setup: {e}", provider.label()))?;
                    // Replacement never transfers bindings or work to a different account.
                    if let Some(old) = store.read()?.accounts.into_iter().find(|a| a.provider == provider && a.id != account.id) { auth::forget(&old)?; }
                    account.persistent_credentials = auth::save(&account, &tokens);
                    let note = if account.persistent_credentials { format!("Connected {}. Choose a saved file to sync.", account.label) } else { format!("Connected {} for this session. The OS credential store is unavailable; reconnect after restarting.", account.label) };
                    store.connect(account)
                        .map_err(|e| anyhow::anyhow!("Saving connection on this device: {e}"))?;
                    Ok(Outcome { note, remote:Some(vec![]), ..Default::default() })
                }, cx);
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
                    note: "Disconnected. Local artwork and pending revisions were kept.".into(),
                    remote: Some(vec![]),
                    ..Default::default()
                })
            },
            cx,
        );
    }
    fn cloud_bind_current(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else {
            self.cloud.note = "Open and save a project or image first.".into();
            cx.notify();
            return;
        };
        let e = editor.read(cx);
        if e.history.save_busy || e.editor.is_modified() {
            self.cloud.note = "Save the current edits before enabling cloud sync.".into();
            cx.notify();
            return;
        }
        let Some(path) = e.editor.path.clone().or_else(|| e.source.clone()) else {
            self.cloud.note = "Save this document to a file first.".into();
            cx.notify();
            return;
        };
        self.cloud_task(
            move |store| {
                store.bind(&path, provider)?;
                emulsion_io::cloud::enqueue(&store, &path)?;
                Ok(Outcome {
                    note: "Cloud sync enabled. Saved versions are queued; Sync now uploads them."
                        .into(),
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
                        "Uploads paused; local saves continue to queue.".into()
                    } else {
                        "Uploads resumed.".into()
                    },
                    ..Default::default()
                })
            },
            cx,
        );
    }
    fn cloud_download(&mut self, account: Account, remote: RemoteRevision, cx: &mut Context<Self>) {
        self.cloud.note = format!("Downloading {}…", remote.revision.name);
        self.cloud_task(
            move |store| {
                let provider = providers::connected(&store, &account)?;
                let mut object = tempfile::NamedTempFile::new_in(&store.root)?;
                provider.download(&remote, object.as_file_mut())?;
                let mut payload = tempfile::NamedTempFile::new_in(&store.root)?;
                store::extract_object(object.path(), &remote.revision, payload.as_file_mut())?;
                let downloads = store.root.join("downloads");
                store::private_dir(&downloads)?;
                let destination = tempfile::Builder::new()
                    .prefix("project-")
                    .tempdir_in(downloads)?;
                let path = emulsion_io::cloud::unpack(payload.path(), destination.path())?;
                store.adopt(&path, &account, &remote)?;
                let _ = destination.keep();
                Ok(Outcome {
                    note: "Downloaded and verified a separate local copy. Open it below.".into(),
                    ready: Some(path),
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
            self.cloud.note = "Connect Google Photos in Settings before importing photos.".into();
            self.screen = Screen::Settings;
            cx.notify();
            return;
        };
        self.cloud.busy = true;
        self.cloud.note = "Opening Google Photos selection…".into();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.cloud.cancelled = Some(cancelled.clone());
        cx.spawn(async move |this, cx| {
            let start = cx.background_spawn(async move {
                let store = emulsion_io::cloud::store();
                let token = auth::access(&store, &account)?;
                let session = emulsion_cloud::photos::begin(&token)?;
                Ok::<_, anyhow::Error>((token, session))
            }).await;
            match start {
                Ok((token, session)) => {
                    this.update(cx, |this, cx| {
                        this.cloud.busy = false;
                        if cancelled.load(Ordering::Relaxed) {
                            this.cloud_task(move |_| {
                                emulsion_cloud::photos::remove(&token, &session)?;
                                Ok(Outcome { note:"Photo import cancelled.".into(), ..Default::default() })
                            }, cx);
                            return;
                        }
                        cx.open_url(&session.picker_uri);
                        this.cloud.note = "Select photos in Google Photos. Imports are stored locally for editing; location metadata may be omitted.".into();
                        this.cloud_task(move |store| {
                            let destination = store.root.join("photo-imports");
                            let report = emulsion_cloud::photos::import(&token, &session, &destination, &cancelled, |p| { emulsion_io::open(p)?; Ok(()) })?;
                            emulsion_io::creative_library::update(&emulsion_io::creative_library::root(), |catalog| {
                                for path in &report.paths { catalog.add_asset(path.clone(), emulsion_io::creative_library::AssetKind::Image)?; }
                                Ok(())
                            })?;
                            Ok(Outcome { note:format!("Imported {} photo(s); {} unsupported, {} failed. {}", report.paths.len(), report.skipped, report.failed, report.note), photos:report.paths, ..Default::default() })
                        }, cx);
                    }).ok();
                }
                Err(error) => { this.update(cx, |this, cx| { this.cloud.busy = false; this.cloud.cancelled = None; this.cloud.note = error.to_string(); cx.notify(); }).ok(); }
            }
        }).detach();
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
        let mut panel = div().id("cloud-settings").test_support().px(px(40.)).py(px(24.)).flex().flex_col().gap(px(12.)).border_b_1().border_color(p.line)
            .child(div().text_xl().child("Cloud projects and photos"))
            .child(div().text_sm().text_color(p.muted).child("Choose which saved projects to sync. Files stay editable offline. Google Photos imports only the photos you select."))
            .child(div().text_sm().text_color(p.muted).child("Enabling sync uploads the saved file and its required originals to the selected provider. Photo imports stay on this device until you choose to sync or export them."))
            .child(div().flex().flex_wrap().gap(px(8.))
                .child(Button::new("cloud-registration").label("Import app registration…").small().outline().disabled(busy).on_click(cx.listener(|this, _, _, cx| this.cloud_import_config(cx))))
                .child(Button::new("cloud-refresh").label("Sync now / retry").small().outline().disabled(busy || accounts.is_empty()).on_click(cx.listener(|this, _, _, cx| this.cloud_sync(true, cx)))));
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
                            ""
                        } else {
                            " · session only"
                        }
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "{} · {}",
                        provider.label(),
                        if configured {
                            "Not connected"
                        } else {
                            "App registration needed"
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
                        "Reconnect"
                    } else {
                        "Connect"
                    })
                    .small()
                    .outline()
                    .disabled(busy || !configured)
                    .on_click(cx.listener(move |this, _, _, cx| this.cloud_connect(provider, cx))),
            );
            if let Some(account) = account {
                row = row.child(
                    Button::new(("cloud-disconnect", provider_index))
                        .label("Disconnect")
                        .small()
                        .outline()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cloud_disconnect(account.clone(), cx)
                        })),
                );
                if provider != Provider::GooglePhotos {
                    row = row.child(
                        Button::new(("cloud-bind", provider_index))
                            .label("Sync current file")
                            .small()
                            .outline()
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.cloud_bind_current(provider, cx)
                            })),
                    );
                } else {
                    row = row.child(
                        Button::new("cloud-photos-import")
                            .label("Import selected photos…")
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
                    .label("Cancel")
                    .small()
                    .outline()
                    .on_click(move |_, _, _| cancelled.store(true, Ordering::Relaxed)),
            );
        }
        if let Some(path) = self.cloud.ready.clone() {
            panel = panel.child(
                Button::new("cloud-open-download")
                    .label("Open downloaded copy")
                    .small()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_path(path.clone(), window, cx)
                    })),
            );
        }
        if let Some(index) = &self.cloud.index {
            for (n, binding) in index.bindings.iter().enumerate().take(100) {
                let jobs = index
                    .jobs
                    .iter()
                    .filter(|j| j.revision.project == binding.project)
                    .collect::<Vec<_>>();
                let state = if !accounts
                    .iter()
                    .any(|a| a.provider == binding.provider && a.id == binding.account_id)
                {
                    "Account disconnected".into()
                } else if binding.paused {
                    "Paused".to_string()
                } else if let Some(error) = jobs.iter().find_map(|j| j.error.as_ref()) {
                    error.clone()
                } else if binding.saved_hash.is_none() {
                    "Snapshot needed".into()
                } else if jobs.is_empty() {
                    "Synced".into()
                } else {
                    format!("{} queued", jobs.len())
                };
                let path = binding.path.clone();
                let paused = binding.paused;
                panel = panel.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child(div().text_sm().child(format!(
                                "{} · {} · {state}",
                                binding
                                    .path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy(),
                                binding.provider.label()
                            )))
                        .child(
                            Button::new(("cloud-pause", n))
                                .label(if paused { "Resume" } else { "Pause" })
                                .small()
                                .outline()
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.cloud_pause(path.clone(), !paused, cx)
                                })),
                        ),
                );
            }
        }
        if !self.cloud.remote.is_empty() {
            panel = panel.child(div().text_lg().child("Cloud versions"));
            let start = self.cloud.page * 20;
            for (n, (account, remote)) in self.cloud.remote.iter().enumerate().skip(start).take(20)
            {
                let account = account.clone();
                let remote = remote.clone();
                let same_project = self
                    .cloud
                    .remote
                    .iter()
                    .filter(|(a, r)| {
                        a.provider == account.provider
                            && a.id == account.id
                            && r.revision.project == remote.revision.project
                    })
                    .map(|(_, r)| r.clone())
                    .collect::<Vec<_>>();
                let heads = emulsion_cloud::heads(&same_project);
                let state = if heads.iter().any(|h| h.revision.id == remote.revision.id) {
                    if heads.len() > 1 {
                        "Conflict · both versions preserved"
                    } else {
                        "Latest"
                    }
                } else {
                    "Earlier version"
                };
                panel = panel.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child(div().text_sm().child(format!(
                            "{} · {} · {state} · {} · device {} · {}",
                            remote.revision.name,
                            account.provider.label(),
                            emulsion_io::recent::ago(remote.revision.created),
                            &remote.revision.device[..8],
                            &remote.revision.id[..8]
                        )))
                        .child(
                            Button::new(("cloud-download", n))
                                .label("Download copy")
                                .small()
                                .outline()
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.cloud_download(account.clone(), remote.clone(), cx)
                                })),
                        ),
                );
            }
            panel = panel.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        Button::new("cloud-prev")
                            .label("Previous")
                            .small()
                            .outline()
                            .disabled(self.cloud.page == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cloud.page = this.cloud.page.saturating_sub(1);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("cloud-next")
                            .label("Next")
                            .small()
                            .outline()
                            .disabled(start + 20 >= self.cloud.remote.len())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cloud.page += 1;
                                cx.notify();
                            })),
                    ),
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

    #[gpui_kit::test]
    fn cloud_settings_explain_missing_registrations_and_keep_connect_inactive(
        cx: &mut TestAppContext,
    ) {
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
                this.set_screen(Screen::Settings, window, cx);
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
                assert_eq!(this.screen, Screen::Settings);
                assert!(!this.cloud.busy);
                assert!(this.cloud.note.contains("Connect Google Photos"));
            })
        });
    }
}
