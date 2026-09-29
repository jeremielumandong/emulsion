//! Home's paged cloud browser: one file per row, with history scoped to that file.
use super::*;
use gpui_kit::component::Selectable;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::menu::PopupMenuItem;
use std::collections::BTreeMap;

const PAGE_SIZE: usize = 48;
struct CloudFile {
    account: Account,
    versions: Vec<RemoteRevision>,
}
impl CloudFile {
    fn latest(&self) -> &RemoteRevision {
        &self.versions[0]
    }
}
fn files(
    rows: &[(Account, RemoteRevision)],
    provider: Option<Provider>,
    query: &str,
) -> Vec<CloudFile> {
    let mut groups: BTreeMap<(Provider, String, String), CloudFile> = BTreeMap::new();
    for (account, revision) in rows {
        if provider.is_some_and(|p| p != account.provider) {
            continue;
        }
        groups
            .entry((
                account.provider,
                account.id.clone(),
                revision.revision.project.clone(),
            ))
            .or_insert_with(|| CloudFile {
                account: account.clone(),
                versions: vec![],
            })
            .versions
            .push(revision.clone());
    }
    let mut files: Vec<_> = groups
        .into_values()
        .filter_map(|mut file| {
            file.versions
                .sort_by_key(|r| std::cmp::Reverse((r.revision.created, r.revision.id.clone())));
            let parents: std::collections::HashSet<_> = file
                .versions
                .iter()
                .filter_map(|r| r.revision.parent.clone())
                .collect();
            if let Some(head) = file
                .versions
                .iter()
                .position(|r| !parents.contains(&r.revision.id))
            {
                file.versions.swap(0, head);
            }
            let revision = &file.latest().revision;
            let matches = revision.name.to_lowercase().contains(query.trim())
                || revision.home.as_ref().is_some_and(|h| {
                    h.name.to_lowercase().contains(query.trim())
                        || h.folder
                            .as_ref()
                            .is_some_and(|f| f.name.to_lowercase().contains(query.trim()))
                });
            matches.then_some(file)
        })
        .collect();
    files.sort_by_key(|file| std::cmp::Reverse(file.latest().revision.created));
    files
}

impl Workspace {
    pub(crate) fn open_cloud_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.home_state.cloud_files = true;
        self.cloud.history = None;
        self.cloud.page = 0;
        self.set_screen(Screen::Home, window, cx);
        if self
            .cloud
            .index
            .as_ref()
            .is_none_or(|i| i.accounts.is_empty())
        {
            self.cloud.connections_open = true;
        }
        self.cloud_sync(false, cx);
    }
    pub(crate) fn cloud_reset_page(&mut self) {
        self.cloud.page = 0;
    }

    pub(crate) fn cloud_home_browser(
        &mut self,
        columns: u16,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let busy = self.cloud.busy;
        let query = self.home_search_query(cx);
        let groups = files(&self.cloud.remote, self.cloud.provider_filter, &query);
        let p = theme::palette(cx);
        let mut root = div()
            .id("home-cloud-browser")
            .test_support()
            .flex()
            .flex_col()
            .gap_3();
        let mut controls =
            crate::widgets::command_bar("cloud-home-toolbar", "Cloud filters and actions");
        for (n, provider) in std::iter::once(None)
            .chain(
                Provider::ALL
                    .into_iter()
                    .filter(|p| *p != Provider::GooglePhotos)
                    .map(Some),
            )
            .enumerate()
        {
            controls = controls.child(
                Button::new(("cloud-filter", n))
                    .label(provider.map_or("All drives", Provider::label))
                    .small()
                    .outline()
                    .selected(self.cloud.provider_filter == provider)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.cloud.provider_filter = provider;
                        this.cloud.history = None;
                        this.cloud.page = 0;
                        cx.notify();
                    })),
            );
        }
        controls = controls
            .child(
                Button::new("cloud-home-refresh")
                    .label(if busy {
                        "Syncing…"
                    } else {
                        "Refresh / retry"
                    })
                    .small()
                    .outline()
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.cloud_sync(true, cx))),
            )
            .child(
                Button::new("cloud-home-accounts")
                    .label(if self.cloud.connections_open {
                        "Hide connections"
                    } else {
                        "Manage connections…"
                    })
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cloud.connections_open = !this.cloud.connections_open;
                        cx.notify();
                    })),
            );
        if let Some(account) = self.cloud.index.as_ref().and_then(|i| {
            i.accounts
                .iter()
                .find(|a| a.provider == Provider::GoogleDrive && !a.root.is_empty())
        }) {
            let url = format!(
                "https://drive.google.com/drive/folders/{}",
                emulsion_cloud::http::segment(&account.root)
            );
            controls = controls.child(
                Button::new("cloud-home-drive-folder")
                    .label("Open Drive folder")
                    .small()
                    .outline()
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            );
        }
        root = root.child(controls);
        if self.cloud.connections_open {
            root = root.child(self.cloud_panel(cx));
        }
        if let Some(path) = self.cloud.ready.clone() {
            root = root.child(
                Button::new("cloud-home-open")
                    .label("Open downloaded copy")
                    .small()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_path(path.clone(), window, cx)
                    })),
            );
        }
        if let Some((account, project)) = self.cloud.history.clone() {
            // History is a separate paged view of one file, never a global list.
            let file = files(&self.cloud.remote, Some(account.provider), "")
                .into_iter()
                .find(|f| f.account.id == account.id && f.latest().revision.project == project);
            root = root.child(
                Button::new("cloud-history-back")
                    .label("← Cloud files")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cloud.history = None;
                        this.cloud.page = 0;
                        cx.notify();
                    })),
            );
            if let Some(file) = file {
                root = root.child(
                    div()
                        .text_lg()
                        .child(format!("{} · Version history", file.latest().revision.name)),
                );
                let heads = emulsion_cloud::heads(&file.versions);
                let total = file.versions.len();
                self.cloud.page = self.cloud.page.min(total.saturating_sub(1) / PAGE_SIZE);
                for (n, version) in file
                    .versions
                    .iter()
                    .enumerate()
                    .skip(self.cloud.page * PAGE_SIZE)
                    .take(PAGE_SIZE)
                {
                    let is_head = heads.iter().any(|h| h.revision.id == version.revision.id);
                    let state = if is_head && heads.len() > 1 {
                        "Conflicting version"
                    } else if is_head {
                        "Latest"
                    } else {
                        "Earlier version"
                    };
                    let account = account.clone();
                    let version = version.clone();
                    root = root.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(format!(
                                "{state} · {} · {}",
                                emulsion_io::recent::ago(version.revision.created),
                                &version.revision.id[..8]
                            ))
                            .child(
                                Button::new(("cloud-history-download", n))
                                    .label("Download copy")
                                    .small()
                                    .outline()
                                    .disabled(busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.cloud_download(account.clone(), version.clone(), cx)
                                    })),
                            ),
                    );
                }
                root = root.child(self.cloud_pages(total, "versions", cx));
            } else {
                root = root.child("Refresh to load this file’s versions.");
            }
        } else {
            let total = groups.len();
            self.cloud.page = self.cloud.page.min(total.saturating_sub(1) / PAGE_SIZE);
            root = root.child(div().text_sm().text_color(p.muted).child(format!(
                "{total} cloud files · Search by file or project name above"
            )));
            let mut grid = div()
                .id("cloud-file-grid")
                .test_support()
                .grid()
                .grid_cols(columns)
                .gap_3();
            for (n, file) in groups
                .iter()
                .enumerate()
                .skip(self.cloud.page * PAGE_SIZE)
                .take(PAGE_SIZE)
            {
                let account = file.account.clone();
                let latest = file.latest().clone();
                let history_account = account.clone();
                let project = latest.revision.project.clone();
                let heads = emulsion_cloud::heads(&file.versions);
                grid =
                    grid.child(
                        div()
                            .id(("cloud-file", n))
                            .test_support()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(p.line)
                            .bg(p.panel)
                            .child(Icon::empty().path("icons/file-image.svg").size(px(28.)))
                            .child(div().text_sm().text_ellipsis().child(
                                latest.revision.home.as_ref().map_or_else(
                                    || latest.revision.name.clone(),
                                    |h| h.name.clone(),
                                ),
                            ))
                            .child(div().text_xs().text_color(p.muted).text_ellipsis().child(
                                format!(
                                        "{} · {}",
                                        latest
                                            .revision
                                            .home
                                            .as_ref()
                                            .and_then(|h| h.folder.as_ref())
                                            .map_or("Unfiled", |f| f.name.as_str()),
                                        latest.revision.name
                                    ),
                            ))
                            .child(div().text_xs().text_color(p.muted).child(format!(
                                "{} · {}",
                                account.provider.label(),
                                emulsion_io::recent::ago(latest.revision.created)
                            )))
                            .child(div().text_xs().child(if heads.len() > 1 {
                                "Conflict · open history to choose a version".to_string()
                            } else {
                                format!("{} saved versions", file.versions.len())
                            }))
                            .child(
                                Button::new(("cloud-file-download", n))
                                    .label("Download copy")
                                    .small()
                                    .outline()
                                    .disabled(busy || heads.len() > 1)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.cloud_download(account.clone(), latest.clone(), cx)
                                    })),
                            )
                            .child(
                                Button::new(("cloud-file-history", n))
                                    .label("Version history…")
                                    .small()
                                    .outline()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.cloud.history =
                                            Some((history_account.clone(), project.clone()));
                                        this.cloud.page = 0;
                                        cx.notify();
                                    })),
                            ),
                    );
            }
            root = root.child(grid);
            if total == 0 {
                let connected = self.cloud.index.as_ref().is_some_and(|index| {
                    index
                        .accounts
                        .iter()
                        .any(|a| a.provider != Provider::GooglePhotos)
                });
                let filtered = !query.trim().is_empty() || self.cloud.provider_filter.is_some();
                let (title, description) = if busy {
                    (
                        "Checking your cloud files",
                        "Your files will appear when the current sync finishes.",
                    )
                } else if !connected {
                    (
                        "Connect a drive",
                        "Connect Google Drive, Dropbox or OneDrive to sync saved work and browse cloud copies.",
                    )
                } else if filtered {
                    (
                        "No matching cloud files",
                        "Try another name or clear the search and drive filter.",
                    )
                } else {
                    (
                        "No cloud files yet",
                        "Sync a saved file from its Home card, or refresh to find work from another device.",
                    )
                };
                let mut empty = crate::widgets::empty_state("cloud", title, description);
                if !busy && !connected {
                    empty = empty.child(
                        Button::new("cloud-empty-connect")
                            .label("Manage connections…")
                            .small()
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cloud.connections_open = true;
                                cx.notify();
                            })),
                    );
                } else if !busy && filtered {
                    empty = empty.child(
                        Button::new("cloud-empty-clear")
                            .label("Clear filters")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cloud.provider_filter = None;
                                this.cloud.page = 0;
                                this.clear_home_search(window, cx);
                                cx.notify();
                            })),
                    );
                } else if !busy {
                    empty = empty.child(
                        Button::new("cloud-empty-local")
                            .label("Browse local files")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.home_state.cloud_files = false;
                                this.home_state.page = 0;
                                cx.notify();
                            })),
                    );
                }
                root = root.child(div().id("cloud-empty").test_support().child(empty));
            }
            if total > PAGE_SIZE {
                root = root.child(self.cloud_pages(total, "files", cx));
            }
        }
        root.into_any_element()
    }
    fn cloud_pages(&self, total: usize, unit: &str, cx: &Context<Self>) -> AnyElement {
        let page = self.cloud.page;
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                Button::new("cloud-page-prev")
                    .label("Previous")
                    .small()
                    .outline()
                    .disabled(page == 0)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cloud.page = this.cloud.page.saturating_sub(1);
                        cx.notify();
                    })),
            )
            .child(format!(
                "{}–{} of {total} {unit}",
                if total == 0 { 0 } else { page * PAGE_SIZE + 1 },
                ((page + 1) * PAGE_SIZE).min(total)
            ))
            .child(
                Button::new("cloud-page-next")
                    .label("Next")
                    .small()
                    .outline()
                    .disabled((page + 1) * PAGE_SIZE >= total)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cloud.page += 1;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
    pub(crate) fn cloud_file_menu(
        &self,
        path: PathBuf,
        cx: &Context<Self>,
    ) -> impl Fn(gpui_kit::component::menu::PopupMenu) -> gpui_kit::component::menu::PopupMenu
    + Clone
    + use<> {
        let binding = self
            .cloud
            .index
            .as_ref()
            .and_then(|i| i.bindings.iter().find(|b| b.path == path))
            .cloned();
        let account = binding
            .as_ref()
            .and_then(|b| {
                self.cloud
                    .index
                    .as_ref()?
                    .accounts
                    .iter()
                    .find(|a| a.provider == b.provider && a.id == b.account_id)
            })
            .cloned();
        let owner = cx.weak_entity();
        let busy = self.cloud.busy || !self.cloud.loaded;
        let destinations: Vec<_> = self
            .cloud
            .index
            .as_ref()
            .into_iter()
            .flat_map(|i| &i.accounts)
            .filter(|a| a.provider != Provider::GooglePhotos)
            .filter(|a| {
                binding
                    .as_ref()
                    .is_none_or(|b| b.provider == a.provider && b.account_id == a.id)
            })
            .map(|a| a.provider)
            .collect();
        move |mut menu| {
            if destinations.is_empty() {
                let connect = owner.clone();
                menu = menu.item(
                    PopupMenuItem::new(if binding.is_some() {
                        "Reconnect cloud…"
                    } else {
                        "Connect cloud…"
                    })
                    .disabled(busy)
                    .on_click(move |_, window, cx| {
                        connect
                            .update(cx, |this, cx| {
                                this.cloud.connections_open = true;
                                this.open_cloud_home(window, cx);
                            })
                            .ok();
                    }),
                );
            } else {
                for provider in &destinations {
                    let provider = *provider;
                    let sync = owner.clone();
                    let sync_path = path.clone();
                    menu = menu.item(
                        PopupMenuItem::new(format!("Sync to {}", provider.label()))
                            .disabled(busy)
                            .on_click(move |_, _, cx| {
                                sync.update(cx, |this, cx| {
                                    this.cloud_sync_file(sync_path.clone(), provider, cx)
                                })
                                .ok();
                            }),
                    );
                }
            }
            let Some(binding) = binding.clone() else {
                return menu;
            };
            let pause = owner.clone();
            let path = path.clone();
            let paused = binding.paused;
            let menu = menu.separator().item(
                PopupMenuItem::new(if paused { "Resume sync" } else { "Pause sync" })
                    .disabled(busy)
                    .on_click(move |_, _, cx| {
                        pause
                            .update(cx, |this, cx| this.cloud_pause(path.clone(), !paused, cx))
                            .ok();
                    }),
            );
            let Some(account) = account.clone() else {
                return menu;
            };
            let owner = owner.clone();
            menu.item(PopupMenuItem::new("Cloud version history…").on_click(
                move |_, window, cx| {
                    owner
                        .update(cx, |this, cx| {
                            this.open_cloud_home(window, cx);
                            this.cloud.history = Some((account.clone(), binding.project.clone()));
                        })
                        .ok();
                },
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt;

    #[gpui_kit::test]
    fn cloud_empty_states_offer_connection_and_filter_recovery(cx: &mut TestAppContext) {
        let (workspace, cx) = crate::tests::open(cx, emulsion_core::Document::new(32, 32));
        cx.run_until_parked();
        cx.simulate_resize(size(px(1440.), px(1000.)));
        cx.update(|window, cx| {
            workspace.update(cx, |this, cx| {
                this.cloud = CloudUi {
                    loaded: true,
                    index: Some(Index::default()),
                    ..Default::default()
                };
                this.home_state.cloud_files = true;
                this.set_screen(Screen::Home, window, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find("cloud-page-next").is_none());
            window.click("cloud-empty-connect", cx);
            assert!(workspace.read(cx).cloud.connections_open);
            workspace.update(cx, |this, cx| {
                this.cloud.connections_open = false;
                this.cloud
                    .index
                    .as_mut()
                    .unwrap()
                    .accounts
                    .push(collection()[0].0.clone());
                this.cloud.provider_filter = Some(Provider::Dropbox);
                cx.notify();
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("cloud-empty-clear", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(workspace.read(cx).cloud.provider_filter, None);
            assert!(window.find("cloud-empty-local").visible());
            workspace.update(cx, |this, cx| {
                this.cloud.busy = true;
                cx.notify();
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find("cloud-empty-local").is_none());
            assert_eq!(window.find("cloud-home-refresh").label(), Some("Syncing…"));
            window.click("cloud-home-refresh", cx);
            assert!(workspace.read(cx).cloud.busy);
        });
    }
    fn collection() -> Vec<(Account, RemoteRevision)> {
        let account = Account {
            provider: Provider::GoogleDrive,
            id: "fixture-account".into(),
            registration: "fixture".into(),
            label: "Fixture".into(),
            root: "fixture".into(),
            persistent_credentials: false,
        };
        (0..1000)
            .flat_map(|n| {
                let account = account.clone();
                (0..3).map(move |v| {
                    (
                        account.clone(),
                        RemoteRevision {
                            remote_id: format!("remote-{n}-{v}"),
                            revision: emulsion_cloud::Revision {
                                project: format!("project-{n:04}"),
                                id: format!("revision-{n:04}-{v}"),
                                parent: (v > 0).then(|| format!("revision-{n:04}-{}", v - 1)),
                                hash: "a".repeat(64),
                                name: format!("photo-{n:04}.jpg"),
                                created: n * 3 + v,
                                device: "test-device".into(),
                                bytes: 10,
                                home: None,
                            },
                        },
                    )
                })
            })
            .collect()
    }
    #[test]
    fn thousands_of_revisions_group_into_searchable_files() {
        let rows = collection();
        let grouped = files(&rows, None, "");
        assert_eq!(grouped.len(), 1000);
        assert!(
            grouped
                .iter()
                .all(|f| f.versions.len() == 3 && f.latest().revision.id.ends_with("-2"))
        );
        assert_eq!(files(&rows, None, "photo-0042").len(), 1);
        let mut named = rows.clone();
        named[128].1.revision.home = Some(emulsion_cloud::HomeMetadata {
            name: "Cover portrait".into(),
            folder: Some(emulsion_cloud::HomeFolder {
                id: emulsion_cloud::id(),
                name: "Incubarity".into(),
            }),
            kind: Some("Photo".into()),
        });
        assert_eq!(files(&named, None, "incubarity").len(), 1);
        assert_eq!(files(&named, None, "cover portrait").len(), 1);
        assert!(files(&rows, Some(Provider::Dropbox), "").is_empty());
        let mut branch = rows[..3].to_vec();
        branch[0].1.revision.created = 99999; // Parent timestamps do not determine the head.
        assert!(
            files(&branch, None, "")[0]
                .latest()
                .revision
                .id
                .ends_with("-2")
        );
    }
    #[gpui_kit::test]
    fn cloud_home_renders_only_one_page_for_a_thousand_files(cx: &mut TestAppContext) {
        let (workspace, cx) = crate::tests::open(cx, emulsion_core::Document::new(32, 32));
        cx.run_until_parked();
        cx.simulate_resize(size(px(1440.), px(1000.)));
        cx.update(|window, cx| {
            workspace.update(cx, |this, cx| {
                this.cloud = CloudUi {
                    loaded: true,
                    remote: collection(),
                    ..Default::default()
                };
                this.home_state.cloud_files = true;
                this.set_screen(Screen::Home, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| {
            assert!(window.find("home-cloud-browser").visible());
            assert!(window.try_find(("cloud-file", 0usize)).is_some());
            assert!(window.try_find(("cloud-file", 47usize)).is_some());
            assert!(window.try_find(("cloud-file", 48usize)).is_none());
            assert!(window.try_find("cloud-settings").is_none());
        });
        cx.update(|_, cx| {
            workspace.update(cx, |this, cx| {
                this.cloud.page = 1;
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| {
            assert!(window.try_find(("cloud-file", 0usize)).is_none());
            assert!(window.try_find(("cloud-file", 48usize)).is_some());
            assert!(window.try_find(("cloud-file", 96usize)).is_none());
        });
    }
}
