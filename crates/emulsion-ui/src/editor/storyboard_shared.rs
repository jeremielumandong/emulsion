//! Shared storyboards (K5) in the editor. File → Shared Project… shows the
//! file's cloud sync, the collaborators seen in its revisions, other
//! artists' saves waiting to be merged (Review and merge…), the scene
//! claims with Claim and Release, and Check for changes. Review and merge
//! shows what the other artist changed since the common version (change
//! tracking's marks, with pictures) and every conflict with its choice,
//! then applies the merge as one Undo step and saves, which uploads a
//! revision with both heads as parents. Claims show on the Board and the
//! Timeline, and editing a scene someone else claimed warns without
//! blocking. Network work runs on the workspace's cloud worker.
use super::storyboard_changes::{kind_color, spawn_thumb};
use super::*;
use emulsion_core::project::{PageId, Project};
use emulsion_core::storyboard::GroupId;
use emulsion_core::storyboard_changes::ChangeKind;
use emulsion_core::storyboard_merge::{BoardMergeReport, ConflictKey, Resolution};
use emulsion_core::storyboard_sharing::SceneClaim;
use emulsion_io::cloud::shared::{PreparedMerge, Sharing};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
};
use std::collections::{BTreeMap, HashSet};

const THUMB: u32 = 96;

/// The open storyboard's sharing state, as the last cloud check found it.
#[derive(Default)]
pub(crate) struct SharedUi {
    /// `Some(None)`: checked, and the file is not synchronized.
    pub(crate) state: Option<Option<Sharing>>,
    /// A check started from here is running.
    pub(crate) checking: bool,
    /// The file's sync status as Home shows it (“Google Drive · Synced”).
    pub(crate) sync_label: Option<String>,
    /// Scenes already warned about this session.
    warned: HashSet<GroupId>,
    /// Waiting heads already announced in the status bar.
    announced: HashSet<String>,
    /// This installation's ID, read once.
    device: Option<String>,
}

/// Who claims are made for: the name in Settings and this installation.
fn claimant(cx: &App) -> String {
    crate::app_state::settings(cx)
        .storyboard
        .review_author
        .trim()
        .to_string()
}

fn who(claim: &SceneClaim) -> String {
    format!(
        "{} · {}",
        claim.claimant,
        emulsion_io::recent::ago(claim.time)
    )
}

impl EditorView {
    pub(crate) fn device_id(&mut self) -> String {
        self.shared_ui
            .device
            .get_or_insert_with(emulsion_io::cloud::shared::device_id)
            .clone()
    }

    /// A claim on `scene` by someone other than this artist.
    pub(crate) fn claim_by_other(&self, scene: GroupId, cx: &App) -> Option<SceneClaim> {
        let claim = self.editor.storyboard()?.claim(scene)?;
        let device = self.shared_ui.device.as_deref().unwrap_or_default();
        claim.is_other(&claimant(cx), device).then(|| claim.clone())
    }

    fn active_scene(&self) -> Option<GroupId> {
        let board = self.editor.storyboard()?;
        Some(board.panels.get(&self.editor.active_page())?.scene)
    }

    /// Claim `scenes` for this artist, as one Undo step.
    pub(crate) fn claim_scenes(&mut self, scenes: &[GroupId], cx: &mut Context<Self>) -> bool {
        let name = claimant(cx);
        let device = self.device_id();
        let now = emulsion_core::storyboard_review::now();
        let done = self.edit_board(|b| b.claim_scenes(scenes, &name, &device, now), cx);
        if done {
            self.set_status(
                format!("Claimed {} scene(s) for {name}.", scenes.len()),
                false,
                cx,
            );
        }
        done
    }

    /// Release the claims on `scenes`, as one Undo step.
    pub(crate) fn release_scenes(&mut self, scenes: &[GroupId], cx: &mut Context<Self>) -> bool {
        let now = emulsion_core::storyboard_review::now();
        self.edit_board(|b| b.release_scenes(scenes, now).map(|_| ()), cx)
    }

    /// The scenes of the Board selection, or of the active panel.
    pub(crate) fn selected_scenes(&self) -> Vec<GroupId> {
        let Some(board) = self.editor.storyboard() else {
            return Vec::new();
        };
        let mut panels = self.board_selection();
        if panels.is_empty() {
            panels.push(self.editor.active_page());
        }
        let mut out: Vec<GroupId> = Vec::new();
        for id in panels {
            if let Some(p) = board.panels.get(&id)
                && !out.contains(&p.scene)
            {
                out.push(p.scene);
            }
        }
        out
    }

    /// Warn once per scene when work lands in a scene someone else claimed.
    pub(crate) fn warn_claimed_scene(&mut self, cx: &mut Context<Self>) {
        let Some(scene) = self.active_scene() else {
            return;
        };
        if self.shared_ui.warned.contains(&scene) || !self.editor.is_modified() {
            return;
        }
        if let Some(claim) = self.claim_by_other(scene, cx) {
            self.shared_ui.warned.insert(scene);
            let name = self.editor.storyboard().unwrap().scenes[&scene]
                .name
                .clone();
            self.set_status(
                format!(
                    "Scene {name} is claimed by {}. Claims are advisory: your edit stays, but tell them.",
                    who(&claim)
                ),
                true,
                cx,
            );
        }
    }

    /// A badge for a claimed scene on the Board and the Timeline.
    pub(crate) fn claim_badge(&self, scene: GroupId, p: &Palette, cx: &App) -> Option<AnyElement> {
        let claim = self.editor.storyboard()?.claim(scene)?.clone();
        let other = self.claim_by_other(scene, cx).is_some();
        Some(
            div()
                .id(("storyboard-claim", scene))
                .test_support()
                .px_1()
                .rounded(px(3.))
                .bg(if other { p.accent } else { p.ink.opacity(0.6) })
                .text_color(p.panel)
                .text_size(px(10.))
                .child(format!("Claimed · {}", claim.claimant))
                .into_any_element(),
        )
    }

    /// The Stage banner for a panel in a scene someone else claimed.
    pub(crate) fn claim_banner(&self, p: &Palette, cx: &App) -> Option<AnyElement> {
        let scene = self.active_scene()?;
        let claim = self.claim_by_other(scene, cx)?;
        let name = self.editor.storyboard()?.scenes.get(&scene)?.name.clone();
        Some(
            div()
                .id("storyboard-claim-banner")
                .test_support()
                .absolute()
                .top_2()
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .px_3()
                        .py_1()
                        .rounded(px(6.))
                        .bg(p.panel.opacity(0.95))
                        .border_1()
                        .border_color(p.accent)
                        .text_size(px(12.))
                        .text_color(p.ink)
                        .child(format!(
                            "Scene {name} is claimed by {}. You can still draw here.",
                            who(&claim)
                        )),
                )
                .into_any_element(),
        )
    }

    /// The workspace's latest sharing state for this file.
    pub(crate) fn set_shared_state(&mut self, state: Option<Sharing>, cx: &mut Context<Self>) {
        let checking = std::mem::take(&mut self.shared_ui.checking);
        let fresh: Vec<String> = state
            .iter()
            .flat_map(|s| &s.waiting)
            .filter(|r| !self.shared_ui.announced.contains(&r.revision.id))
            .map(|r| r.revision.id.clone())
            .collect();
        match &state {
            Some(s) if !fresh.is_empty() => {
                let by = s.waiting[0]
                    .revision
                    .author
                    .clone()
                    .unwrap_or_else(|| "Another artist".into());
                self.set_status(
                    format!(
                        "{by} saved changes to this storyboard. File → Shared Project… → Review and merge."
                    ),
                    false,
                    cx,
                );
            }
            Some(_) if checking => self.set_status("No new changes from others.", false, cx),
            None if checking => self.set_status(
                "This storyboard is not synced. Sync it from its Home card to share it.",
                true,
                cx,
            ),
            _ => {}
        }
        self.shared_ui.announced.extend(fresh);
        self.shared_ui.state = Some(state);
        cx.notify();
    }

    fn workspace(&self) -> Option<WeakEntity<crate::workspace::Workspace>> {
        self.library_workspace.clone()
    }

    /// File → Shared Project…
    pub(crate) fn shared_project_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<SharedDialog>> {
        if self.editor.storyboard().is_none() {
            self.set_status("Shared projects are for storyboards.", false, cx);
            return None;
        }
        self.device_id();
        let editor = cx.entity();
        let view = cx.new(|cx| {
            cx.observe(&editor, |_, _, cx| cx.notify()).detach();
            SharedDialog {
                editor: editor.downgrade(),
            }
        });
        let shown = view.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Shared project")
                .width(px(620.))
                .child(shown.clone())
        });
        Some(view)
    }

    /// Open the review of a fetched head (see `Workspace::review_shared_merge`).
    pub(crate) fn open_merge_review(
        &mut self,
        prepared: PreparedMerge,
        base: Project,
        theirs: Project,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<MergeReview> {
        let editor = cx.entity().downgrade();
        let ours = self.editor.snapshot();
        let view = cx.new(|cx| {
            let mut v = MergeReview {
                editor,
                prepared,
                base: Arc::new(base),
                theirs: Arc::new(theirs),
                report: None,
                choices: BTreeMap::new(),
                applied: None,
                thumbs: HashMap::new(),
                loading: HashSet::new(),
                generation: 0,
            };
            v.plan(ours, cx);
            v
        });
        let shown = view.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            crate::dialog_actions::with_actions(dialog, &shown, window, cx)
                .title("Review and merge")
                .width(px(680.))
        });
        view
    }

    /// Merge `theirs` into the board as one Undo step, then save, which
    /// uploads a revision with both heads as parents.
    pub(crate) fn apply_shared_merge(
        &mut self,
        base: &Project,
        theirs: &Project,
        choices: &BTreeMap<ConflictKey, Resolution>,
        revision: &str,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        if self.history.save_busy {
            return Err("Wait for the current save to finish.".into());
        }
        if !self.prepare_page_action(cx) {
            return Err("Finish the current edit first.".into());
        }
        let report = self
            .editor
            .merge_board(base, theirs, choices, Some(revision))
            .inspect_err(|error| self.set_status(error.clone(), true, cx))?;
        self.after_change(cx);
        let text = format!(
            "Merged: {} panels took their changes, {} conflict(s) resolved; the board has {} panels. One Undo step.",
            report.took_theirs,
            report.conflicts.len(),
            report.panels
        );
        self.set_status(text.clone(), false, cx);
        if let (Some(path), Some(ws)) = (self.editor.path.clone(), self.workspace()) {
            let me = cx.entity();
            cx.defer(move |cx| {
                ws.update(cx, |ws, cx| ws.write(me, path, cx)).ok();
            });
        }
        Ok(text)
    }
}

/// File → Shared Project….
pub(crate) struct SharedDialog {
    editor: WeakEntity<EditorView>,
}

impl SharedDialog {
    fn check(&mut self, cx: &mut Context<Self>) {
        if let Some(ws) = self.editor.upgrade().and_then(|e| e.read(cx).workspace()) {
            ws.update(cx, |ws, cx| ws.check_shared_changes(cx)).ok();
        }
    }

    fn review(&mut self, head: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        if let Some(ws) = editor.read(cx).workspace() {
            window.close_dialog(cx);
            ws.update(cx, |ws, cx| {
                ws.review_shared_merge(editor, Some(head), window, cx)
            })
            .ok();
        }
    }
}

impl Render for SharedDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let Some(editor) = self.editor.upgrade() else {
            return div();
        };
        let e = editor.read(cx);
        let board = e.editor.storyboard();
        let layout: Vec<PageId> = e.editor.page_list().iter().map(|m| m.id).collect();
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .text_color(p.ink);

        // Sync binding.
        let sync = match &e.shared_ui.state {
            _ if e.editor.path.is_none() => "Save this storyboard, then sync it from its Home card to share it.".to_string(),
            None => "Not checked yet. Choose Check for changes.".into(),
            Some(None) => "Not synced. Use Sync to… on its Home card: every save then becomes a cloud revision others can merge.".into(),
            Some(Some(s)) => format!(
                "{} · {} revisions",
                e.shared_ui
                    .sync_label
                    .clone()
                    .unwrap_or_else(|| s.binding.provider.label().to_string()),
                s.revisions,
            ),
        };
        body = body.child(
            div()
                .id("storyboard-shared-sync")
                .test_support()
                .whitespace_normal()
                .child(sync),
        );
        if let Some(Some(s)) = &e.shared_ui.state {
            let people: Vec<String> = s
                .collaborators
                .iter()
                .map(|c| {
                    let name = c.author.clone().unwrap_or_else(|| {
                        format!("Device {}", &c.device[..8.min(c.device.len())])
                    });
                    let me = if c.device == s.device {
                        " (this computer)"
                    } else {
                        ""
                    };
                    format!(
                        "{name}{me} · {} saves · {}",
                        c.revisions,
                        emulsion_io::recent::ago(c.last)
                    )
                })
                .collect();
            body = body.child(mono("Collaborators", 10., p.muted)).child(
                div()
                    .id("storyboard-shared-people")
                    .test_support()
                    .flex()
                    .flex_col()
                    .children(people),
            );
            body = body.child(mono("Waiting to merge", 10., p.muted));
            if s.waiting.is_empty() {
                body = body.child(
                    div()
                        .text_color(p.muted)
                        .child("Nothing: this file includes every save."),
                );
            }
            for head in &s.waiting {
                let r = &head.revision;
                let id = r.id.clone();
                body = body.child(
                    div()
                        .id(SharedString::from(format!(
                            "storyboard-shared-head-{}",
                            r.id
                        )))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(format!(
                            "{} · saved {}",
                            r.author.clone().unwrap_or_else(|| "Another artist".into()),
                            emulsion_io::recent::ago(r.created)
                        ))
                        .child(
                            Button::new(SharedString::from(format!(
                                "storyboard-shared-review-{}",
                                r.id
                            )))
                            .label("Review and merge…")
                            .small()
                            .primary()
                            .on_click(cx.listener(
                                move |this, _, window, cx| this.review(id.clone(), window, cx),
                            )),
                        ),
                );
            }
        }

        // Claims.
        body = body.child(mono("Scene claims", 10., p.muted));
        let claims: Vec<SceneClaim> = board
            .map(|b| b.active_claims(&layout).into_iter().cloned().collect())
            .unwrap_or_default();
        if claims.is_empty() {
            body = body.child(div().text_color(p.muted).child("No scene is claimed."));
        }
        for claim in claims {
            let scene = claim.scene;
            let name = board
                .and_then(|b| b.scenes.get(&scene))
                .map_or_else(String::new, |s| s.name.clone());
            body = body.child(
                div()
                    .id(SharedString::from(format!(
                        "storyboard-shared-claim-{scene}"
                    )))
                    .test_support()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(format!("Scene {name} · {}", who(&claim)))
                    .child(
                        Button::new(SharedString::from(format!(
                            "storyboard-shared-release-{scene}"
                        )))
                        .label("Release")
                        .xsmall()
                        .outline()
                        .on_click({
                            let editor = self.editor.clone();
                            move |_, _, cx| {
                                editor
                                    .update(cx, |e, cx| {
                                        e.release_scenes(&[scene], cx);
                                    })
                                    .ok();
                            }
                        }),
                    ),
            );
        }
        let scenes = e.selected_scenes();
        let named = claimant(cx);
        let can_claim = !named.is_empty() && !scenes.is_empty();
        let checking = e.shared_ui.checking;
        body.child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .when(named.is_empty(), |d| {
                    d.child(
                        div()
                            .text_color(p.muted)
                            .child("Set your name in Settings › Storyboard to claim scenes."),
                    )
                })
                .child(
                    Button::new("storyboard-shared-claim")
                        .label(format!(
                            "Claim selected scene{}",
                            if scenes.len() == 1 { "" } else { "s" }
                        ))
                        .small()
                        .disabled(!can_claim)
                        .on_click({
                            let editor = self.editor.clone();
                            move |_, _, cx| {
                                editor
                                    .update(cx, |e, cx| {
                                        let scenes = e.selected_scenes();
                                        e.claim_scenes(&scenes, cx);
                                    })
                                    .ok();
                            }
                        }),
                )
                .child(
                    Button::new("storyboard-shared-check")
                        .label(if checking {
                            "Checking…"
                        } else {
                            "Check for changes"
                        })
                        .small()
                        .disabled(checking)
                        .on_click(cx.listener(|this, _, _, cx| this.check(cx))),
                )
                .child(
                    Button::new("storyboard-shared-close")
                        .label("Close")
                        .small()
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
        )
    }
}

/// Review and merge: their changes since the common version and the
/// conflicts, each with a choice, then Apply.
pub(crate) struct MergeReview {
    editor: WeakEntity<EditorView>,
    pub(crate) prepared: PreparedMerge,
    base: Arc<Project>,
    theirs: Arc<Project>,
    pub(crate) report: Option<Result<BoardMergeReport, String>>,
    pub(crate) choices: BTreeMap<ConflictKey, Resolution>,
    pub(crate) applied: Option<String>,
    thumbs: HashMap<PageId, Arc<RenderImage>>,
    loading: HashSet<PageId>,
    generation: u64,
}

impl MergeReview {
    /// Plan the merge against the board as it is now, off the UI thread.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let ours = self
            .editor
            .upgrade()
            .and_then(|e| e.read(cx).editor.snapshot());
        self.plan(ours, cx);
    }

    fn plan(&mut self, ours: Option<Project>, cx: &mut Context<Self>) {
        let Some(ours) = ours else {
            return;
        };
        let (base, theirs, choices) =
            (self.base.clone(), self.theirs.clone(), self.choices.clone());
        self.generation += 1;
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let report = cx
                .background_spawn(async move {
                    emulsion_core::storyboard_merge::merge_boards(&base, &ours, &theirs, &choices)
                        .map(|m| m.report)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.report = Some(report);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn choose(&mut self, key: ConflictKey, choice: Resolution, cx: &mut Context<Self>) {
        self.choices.insert(key, choice);
        self.refresh(cx);
    }

    /// Merge with the choices made and save; true when it landed.
    pub(crate) fn apply(&mut self, cx: &mut Context<Self>) -> bool {
        let (base, theirs, choices) =
            (self.base.clone(), self.theirs.clone(), self.choices.clone());
        let revision = self.prepared.head.id.clone();
        match self.editor.update(cx, |e, cx| {
            e.apply_shared_merge(&base, &theirs, &choices, &revision, cx)
        }) {
            Ok(Ok(text)) => {
                self.applied = Some(text);
                cx.notify();
                true
            }
            Ok(Err(error)) => {
                self.report = Some(Err(error));
                cx.notify();
                false
            }
            Err(_) => false,
        }
    }

    fn thumb(&mut self, panel: PageId, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        if let Some(image) = self.thumbs.get(&panel) {
            return Some(image.clone());
        }
        if self.loading.insert(panel) {
            let doc = self
                .theirs
                .pages
                .iter()
                .chain(&self.base.pages)
                .find(|p| p.meta.id == panel)?
                .doc
                .clone();
            spawn_thumb(doc, THUMB, cx, move |this: &mut Self, image, cx| {
                match image {
                    Ok(image) => {
                        this.thumbs.insert(panel, image);
                    }
                    Err(error) => this.report = Some(Err(error)),
                }
                cx.notify();
            });
        }
        None
    }
}

impl Render for MergeReview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let head = &self.prepared.head;
        let by = head
            .author
            .clone()
            .unwrap_or_else(|| "Another artist".into());
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(p.ink);
        body = body.child(div().whitespace_normal().child(format!(
            "{by} saved this storyboard {}. Their changes since the version you both started from:",
            emulsion_io::recent::ago(head.created)
        )));
        if let Some(text) = &self.applied {
            return div()
                .id("storyboard-shared-applied")
                .test_support()
                .whitespace_normal()
                .child(text.clone());
        }
        let report = self.report.clone();
        match report {
            None => body = body.child(mono("Comparing…", 10.5, p.muted)),
            Some(Err(error)) => {
                body = body.child(
                    div()
                        .id("storyboard-shared-error")
                        .test_support()
                        .whitespace_normal()
                        .text_color(p.accent)
                        .child(error),
                )
            }
            Some(Ok(report)) => {
                let changes: Vec<_> = report
                    .theirs
                    .iter()
                    .filter(|c| c.is_change())
                    .cloned()
                    .collect();
                let mut list = div()
                    .id("storyboard-shared-changes")
                    .test_support()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .max_h(px(240.))
                    .overflow_y_scroll();
                if changes.is_empty() {
                    list = list.child(
                        div()
                            .text_color(p.muted)
                            .child("No panel changes (board data only)."),
                    );
                }
                for change in changes {
                    let panel = change.panel();
                    let image = self.thumb(panel, cx);
                    let color = kind_color(change.kind);
                    list = list.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(64.))
                                    .h(px(36.))
                                    .bg(gpui_kit::white())
                                    .border_1()
                                    .border_color(color)
                                    .overflow_hidden()
                                    .children(image.map(|i| {
                                        img(i).size_full().object_fit(ObjectFit::Contain)
                                    })),
                            )
                            .child(div().w(px(160.)).child(change.name.clone()))
                            .child(
                                div()
                                    .text_color(if change.kind == ChangeKind::Deleted {
                                        p.muted
                                    } else {
                                        color
                                    })
                                    .child(change.summary()),
                            ),
                    );
                }
                body = body.child(list).child(mono(
                    format!(
                        "After the merge: {} panels. {} conflict(s).",
                        report.panels,
                        report.conflicts.len()
                    ),
                    10.,
                    p.muted,
                ));
                for conflict in &report.conflicts {
                    let key = conflict.key.clone();
                    let chip_id = |what: &str| {
                        SharedString::from(format!("storyboard-shared-{what}-{}", key.key()))
                    };
                    let choice = |r: Resolution| {
                        let key = key.clone();
                        cx.listener(move |this: &mut Self, _, _, cx| {
                            this.choose(key.clone(), r, cx)
                        })
                    };
                    let mut options = div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            chip(
                                chip_id("mine"),
                                "Keep mine",
                                conflict.chosen == Resolution::Mine,
                                &p,
                            )
                            .test_support()
                            .on_click(choice(Resolution::Mine)),
                        )
                        .child(
                            chip(
                                chip_id("theirs"),
                                "Take theirs",
                                conflict.chosen == Resolution::Theirs,
                                &p,
                            )
                            .test_support()
                            .on_click(choice(Resolution::Theirs)),
                        );
                    if conflict.keep_both {
                        options = options.child(
                            chip(
                                chip_id("both"),
                                "Keep both",
                                conflict.chosen == Resolution::Both,
                                &p,
                            )
                            .test_support()
                            .on_click(choice(Resolution::Both)),
                        );
                    }
                    body = body.child(
                        div()
                            .id(SharedString::from(format!(
                                "storyboard-shared-conflict-{}",
                                key.key()
                            )))
                            .test_support()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(conflict.what.clone())
                                    .child(mono(conflict.detail.clone(), 10., p.muted)),
                            )
                            .child(options),
                    );
                }
            }
        }
        div()
            .id("storyboard-shared-review")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(body)
            .child(mono(
                "Apply merges into the open board as one Undo step and saves; the upload names both saves as its parents. Both versions stay in the cloud.",
                10.,
                p.muted,
            ))
    }
}

impl crate::dialog_actions::DialogActions for MergeReview {
    /// Close / Apply and save, pinned in the dialog footer.
    fn render_actions(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let ready = matches!(self.report, Some(Ok(_)));
        div()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("storyboard-shared-review-close")
                            .label("Close")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("storyboard-shared-apply")
                            .label("Apply and save")
                            .small()
                            .primary()
                            .disabled(!ready)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply(cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::open;
    use core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_core::storyboard::{Level, Panel};
    use gpui_kit::test::TestWindowExt;

    /// Panels 1 | 2, 3 in two scenes.
    fn storyboard_editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        let (ws, cx) = open(cx, Document::new(64, 36));
        cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
        let mut project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        let blank = project.storyboard().unwrap().blank_panel().unwrap();
        let items = (2..=3)
            .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
            .collect();
        project.insert_panels(Some(1), &blank, items, None).unwrap();
        let ids: Vec<_> = project.page_list().iter().map(|m| m.id).collect();
        project
            .edit_storyboard(|b| {
                b.split(&ids, ids[1], Level::Scene, Some("Chase"))
                    .map(|_| ())
            })
            .unwrap();
        let editor = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(project, "Board".into(), window, cx)
            });
            ws.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        (editor, cx)
    }

    fn layout(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<PageId> {
        cx.update(|_, cx| e.read(cx).editor.page_list().iter().map(|m| m.id).collect())
    }

    fn revision(author: &str) -> emulsion_cloud::Revision {
        emulsion_cloud::Revision {
            project: emulsion_cloud::id(),
            id: emulsion_cloud::id(),
            parent: None,
            hash: "a".repeat(64),
            name: "board.emu".into(),
            created: emulsion_cloud::now(),
            device: emulsion_cloud::id(),
            bytes: 1,
            home: None,
            merged: None,
            author: Some(author.into()),
        }
    }

    #[gpui_kit::test]
    fn another_artists_save_is_reviewed_and_merged_as_one_undo_step(cx: &mut TestAppContext) {
        let (e, cx) = storyboard_editor(cx);
        let ids = layout(&e, cx);
        let base = cx.update(|_, cx| e.read(cx).editor.snapshot().unwrap());
        // Ravi lengthens panel 2 and shortens panel 3; here panel 3 changes too.
        let mut ravi = ProjectEditor::open(base.clone(), None).unwrap();
        ravi.edit_storyboard(|b| {
            b.panels.get_mut(&ids[1]).unwrap().frames = 48;
            b.panels.get_mut(&ids[2]).unwrap().frames = 6;
            Ok(())
        })
        .unwrap();
        let theirs = ravi.snapshot().unwrap();
        cx.update(|_, cx| {
            e.update(cx, |e, _| {
                e.editor
                    .edit_storyboard(|b| {
                        b.panels.get_mut(&ids[2]).unwrap().frames = 30;
                        Ok(())
                    })
                    .unwrap()
            })
        });
        let before = cx.update(|_, cx| e.read(cx).editor.stamp());
        let prepared = PreparedMerge {
            head: revision("Ravi"),
            theirs: PathBuf::new(),
            base: revision("Maya"),
            base_path: PathBuf::new(),
            waiting: 0,
        };
        let review = cx.update(|window, cx| {
            e.update(cx, |e, cx| {
                e.open_merge_review(prepared, base, theirs, window, cx)
            })
        });
        cx.run_until_parked();
        let key = ConflictKey::Panel(ids[2]);
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-shared-changes").visible());
            let report = review.read(cx).report.clone().unwrap().unwrap();
            assert_eq!(report.conflicts.len(), 1);
            assert_eq!(report.conflicts[0].key, key);
            window.click(
                SharedString::from(format!("storyboard-shared-theirs-{}", key.key())),
                cx,
            );
        });
        cx.run_until_parked();
        let frames = |cx: &mut VisualTestContext, id: PageId| {
            cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().panels[&id].frames)
        };
        cx.update(|window, cx| {
            assert_eq!(review.read(cx).choices[&key], Resolution::Theirs);
            assert!(review.update(cx, |r, cx| r.apply(cx)));
            window.render_frame(cx);
            assert!(window.find("storyboard-shared-applied").visible());
        });
        assert_eq!((frames(cx, ids[1]), frames(cx, ids[2])), (48, 6));
        assert_eq!(layout(&e, cx), ids);
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert_eq!(
            cx.update(|_, cx| e.read(cx).editor.stamp()),
            before,
            "one Undo step"
        );
        assert_eq!(frames(cx, ids[2]), 30);
    }

    #[gpui_kit::test]
    fn scene_claims_show_warn_and_release_from_the_shared_dialog(cx: &mut TestAppContext) {
        let (e, cx) = storyboard_editor(cx);
        let ids = layout(&e, cx);
        cx.update(|_, cx| {
            crate::app_state::update_settings(cx, |s| s.storyboard.review_author = "Maya".into())
        });
        let scene =
            cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().panels[&ids[1]].scene);
        // Ravi claimed the Chase scene on his computer.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.edit_board(|b| b.claim_scenes(&[scene], "Ravi", "ravi-device", 10), cx);
                e.editor.set_active_page(ids[1]).unwrap();
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-claim-banner").visible());
        });
        // An edit in his scene warns, once, and still lands.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                assert!(e.edit_board(
                    |b| {
                        b.panels.get_mut(&ids[1]).unwrap().frames = 12;
                        Ok(())
                    },
                    cx
                ));
            })
        });
        let status = cx.update(|_, cx| e.read(cx).status.clone().unwrap().0.to_string());
        assert!(status.contains("claimed by Ravi"), "{status}");

        let dialog = cx
            .update(|window, cx| e.update(cx, |e, cx| e.shared_project_dialog(window, cx)))
            .unwrap();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(
                window
                    .find(SharedString::from(format!(
                        "storyboard-shared-claim-{scene}"
                    )))
                    .visible()
            );
            window.click(
                SharedString::from(format!("storyboard-shared-release-{scene}")),
                cx,
            );
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let board = e.read(cx).editor.storyboard().unwrap();
            assert!(board.claim(scene).is_none());
            assert!(board.sharing.claims[0].released);
            let _ = dialog.read(cx);
        });
        // Maya claims the first scene for herself: no warning there.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.editor.set_active_page(ids[0]).unwrap();
                let scenes = e.selected_scenes();
                assert!(e.claim_scenes(&scenes, cx));
                assert!(e.claim_badge(scenes[0], &theme::palette(cx), cx).is_some());
                assert!(e.claim_by_other(scenes[0], cx).is_none());
            })
        });
    }
}
