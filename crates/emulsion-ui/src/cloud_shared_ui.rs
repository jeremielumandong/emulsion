//! The workspace side of shared storyboards: after each cloud check the
//! open storyboards learn which other artists' saves wait to be merged
//! (and the listing is remembered for MCP and for opening offline), Check
//! for changes runs a sync on request, and Review and merge fetches the
//! other head and the common version on the cloud worker before the
//! editor shows the review. See `editor/storyboard_shared.rs`.
use super::*;
use crate::editor::EditorView;
use emulsion_io::cloud::shared::{self, Sharing};

pub(super) type SharedStates = Vec<(PathBuf, Option<Sharing>)>;

impl Workspace {
    /// Saved storyboards open in tabs, by file.
    pub(super) fn shared_paths(&self, cx: &App) -> Vec<PathBuf> {
        self.tabs
            .iter()
            .filter_map(|tab| {
                let e = tab.read(cx);
                e.editor.storyboard()?;
                e.editor.path.clone()
            })
            .collect()
    }

    /// On the cloud worker: remember the listing of each open storyboard's
    /// project and work out its sharing state.
    pub(super) fn shared_states(
        store: &Store,
        rows: &[(Account, RemoteRevision)],
        paths: &[PathBuf],
    ) -> SharedStates {
        let Ok(index) = store.read() else {
            return Vec::new();
        };
        for path in paths {
            let Ok(canonical) = path.canonicalize() else {
                continue;
            };
            let Some(binding) = index.bindings.iter().find(|b| b.path == canonical) else {
                continue;
            };
            let listing: Vec<RemoteRevision> = rows
                .iter()
                .filter(|(a, r)| {
                    a.provider == binding.provider
                        && a.id == binding.account_id
                        && r.revision.project == binding.project
                })
                .map(|(_, r)| r.clone())
                .collect();
            if !listing.is_empty() {
                let _ = store.remember_remote(&binding.project, &listing);
            }
        }
        paths
            .iter()
            .map(|p| (p.clone(), shared::sharing(store, p, None).ok().flatten()))
            .collect()
    }

    pub(super) fn apply_shared_states(&mut self, states: SharedStates, cx: &mut Context<Self>) {
        for (path, state) in states {
            // The same status Home cards show for the file.
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            for tab in self.tabs.clone() {
                let (same, unsaved) = {
                    let e = tab.read(cx);
                    (
                        e.editor.path.as_ref() == Some(&path),
                        e.editor.is_modified() || e.history.save_busy,
                    )
                };
                if same {
                    let (provider, status) = self.cloud.file_status(&canonical, unsaved);
                    let label = provider.map(|p| format!("{} · {}", p.label(), status.label()));
                    let state = state.clone();
                    tab.update(cx, |e, cx| {
                        e.shared_ui.sync_label = label;
                        e.set_shared_state(state, cx)
                    });
                }
            }
        }
    }

    /// A check started from a storyboard failed: say so there.
    pub(super) fn shared_check_failed(&mut self, error: &str, cx: &mut Context<Self>) {
        for tab in self.tabs.clone() {
            tab.update(cx, |e, cx| {
                if std::mem::take(&mut e.shared_ui.checking) {
                    e.set_status(format!("Could not check for changes: {error}"), true, cx);
                }
            });
        }
    }

    fn storyboard_binding(
        &self,
        path: &Path,
    ) -> Option<(emulsion_cloud::Binding, Option<Account>)> {
        let index = self.cloud.index.as_ref()?;
        let canonical = path.canonicalize().ok()?;
        let binding = index.bindings.iter().find(|b| b.path == canonical)?.clone();
        let account = index
            .accounts
            .iter()
            .find(|a| a.provider == binding.provider && a.id == binding.account_id)
            .cloned();
        Some((binding, account))
    }

    /// Check for changes: sync, then report other artists' saves.
    pub(crate) fn check_shared_changes(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.clone() else {
            return;
        };
        let (path, storyboard) = {
            let e = editor.read(cx);
            (e.editor.path.clone(), e.editor.storyboard().is_some())
        };
        let say = |text: &str, cx: &mut Context<Self>| {
            editor.update(cx, |e, cx| e.set_status(text.to_string(), true, cx));
        };
        if !storyboard {
            return say("Check for changes is for shared storyboards.", cx);
        }
        let Some(path) = path else {
            return say(
                "Save this storyboard and sync it from its Home card to share it.",
                cx,
            );
        };
        if self.cloud.busy {
            return say("Cloud sync is busy; try again in a moment.", cx);
        }
        editor.update(cx, |e, _| e.shared_ui.checking = true);
        if self.storyboard_binding(&path).is_none() {
            editor.update(cx, |e, cx| e.set_shared_state(None, cx));
            return;
        }
        self.cloud_sync(true, cx);
    }

    /// When a synced storyboard opens, look for other artists' saves.
    pub(crate) fn shared_check_on_open(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.editor.as_ref().and_then(|e| {
            let e = e.read(cx);
            e.editor.storyboard()?;
            e.editor.path.clone()
        }) else {
            return;
        };
        if self.storyboard_binding(&path).is_some() {
            self.cloud_sync(false, cx);
        }
    }

    /// Review and merge…: fetch `head` (or the newest waiting save) and
    /// the common version on a worker, then open the review.
    pub(crate) fn review_shared_merge(
        &mut self,
        editor: Entity<EditorView>,
        head: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (path, busy): (Option<PathBuf>, bool) = {
            let e = editor.read(cx);
            (e.editor.path.clone(), e.history.save_busy)
        };
        let fail = |text: &str, cx: &mut Context<Self>| {
            editor.update(cx, |e, cx| e.set_status(text.to_string(), true, cx));
        };
        let Some(path) = path else {
            return fail("Save this storyboard first.", cx);
        };
        if busy {
            return fail("Wait for the current save to finish.", cx);
        }
        let Some((_, Some(account))) = self.storyboard_binding(&path) else {
            return fail("Reconnect this storyboard's cloud account to merge.", cx);
        };
        let job = emulsion_ai::jobs::Job::new();
        job.set_stage("fetching the other save");
        editor.update(cx, |e, cx| {
            e.watch_job(job.clone(), "Fetching the other artist's save", cx)
        });
        let target = editor.downgrade();
        cx.spawn_in(window, async move |_, cx| {
            let result = cx
                .background_spawn(async move {
                    let result = (|| -> anyhow::Result<_> {
                        let store = emulsion_io::cloud::store();
                        let provider = providers::connected(&store, &account)?;
                        let prepared =
                            shared::prepare_merge(&store, &provider, &path, head.as_deref())?;
                        let base = emulsion_io::project::read(&prepared.base_path)?;
                        let theirs = emulsion_io::project::read(&prepared.theirs)?;
                        Ok((prepared, base, theirs))
                    })();
                    job.finish();
                    result.map_err(|e| e.to_string())
                })
                .await;
            target
                .update_in(cx, |e, window, cx| match result {
                    Ok((prepared, base, theirs)) => {
                        e.open_merge_review(prepared, base, theirs, window, cx);
                    }
                    Err(error) => {
                        e.set_status(format!("Could not fetch the other save: {error}"), true, cx)
                    }
                })
                .ok();
        })
        .detach();
    }
}
