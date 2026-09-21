//! The History page and autosave.
//!
//! Branches are columns of commits. Picking a commit compares it with the
//! document as it is now; from there it can be restored, branched from, or
//! (for another branch's commit) merged. A merge that touches the same
//! property on both sides stops and asks, one choice per conflict.
//!
//! Autosave commits the document every few seconds when it changed, so the
//! graph always holds recent work, and writes a recovery copy (document and
//! graph) to the data directory about once a minute. Saving or discarding
//! removes the recovery copy.

use super::*;

impl EditorView {
    pub(crate) fn compact_history(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let count = self.editor.history.len();
        let rows = self
            .editor
            .history
            .steps()
            .enumerate()
            .map(|(index, step)| {
                chip(
                    ("history-step", step.revision_before),
                    step.name.clone(),
                    index == 0,
                    p,
                )
                .w_full()
                .justify_start()
                .aria_selected(index == 0)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.undo_to(index, cx);
                    window.focus(&this.canvas_focus, cx);
                }))
                .test_support()
            });
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        crate::widgets::chip_action(
                            "history-undo",
                            "Undo",
                            false,
                            self.editor.history.can_undo(),
                            p,
                            cx.listener(|this, _, window, cx| {
                                this.undo(cx);
                                window.focus(&this.canvas_focus, cx);
                            }),
                        )
                        .test_support(),
                    )
                    .child(
                        crate::widgets::chip_action(
                            "history-redo",
                            "Redo",
                            false,
                            self.editor.history.can_redo(),
                            p,
                            cx.listener(|this, _, window, cx| {
                                this.redo(cx);
                                window.focus(&this.canvas_focus, cx);
                            }),
                        )
                        .test_support(),
                    )
                    .child(
                        chip("history-versions", "Versions", false, p)
                            .on_click(cx.listener(|this, _, _, cx| this.open_history(cx))),
                    ),
            )
            .children(rows)
            .child(
                chip(
                    "history-initial",
                    if count == 0 {
                        "Current state"
                    } else {
                        "Earlier state"
                    },
                    count == 0,
                    p,
                )
                .justify_start()
                .aria_selected(count == 0)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.undo_to(count, cx);
                    window.focus(&this.canvas_focus, cx);
                }))
                .test_support(),
            )
            .into_any_element()
    }
}

use emulsion_core::graph::{
    CommitId, Conflict, ConflictKey, DiffRow, MAIN, MergeOutcome, Side, compare,
};
use std::collections::HashSet;

pub(crate) const AUTOSAVE_SECS: u64 = 10;
const RECOVERY_SECS: u64 = 60;
const THUMB: u32 = 280;
/// Branch colours after main's ink.
const BRANCH_COLORS: [u32; 5] = [crate::theme::ACCENT, 0x6E8FA8, 0x7A9A5B, 0xB0892F, 0x8C6BB1];

#[derive(Default)]
pub(crate) struct HistoryState {
    pub open: bool,
    /// Commit compared with the current document; None = the branch base.
    pub selected: Option<CommitId>,
    thumbs: HashMap<CommitId, Arc<RenderImage>>,
    loading: HashSet<CommitId>,
    current: Option<(u64, Arc<RenderImage>)>,
    current_loading: Option<u64>,
    pub(crate) new_branch: Option<(Entity<InputState>, Subscription)>,
    pub(crate) merge: Option<PendingMerge>,
    pub(crate) last_autosave: Option<Instant>,
    recovery: Option<PathBuf>,
    recovery_rev: u64,
    recovery_busy: bool,
    last_recovery: Option<Instant>,
}

/// A merge waiting for the person's choices.
pub(crate) struct PendingMerge {
    pub from: String,
    pub conflicts: Vec<Conflict>,
    pub choices: HashMap<ConflictKey, Side>,
}

/// Where crash-recovery copies live.
pub fn recovery_dir() -> PathBuf {
    emulsion_io::recent::data_dir().join("autosave")
}

/// Straight sRGBA8 → BGRA thumbnail of `doc`, at most `max` px a side.
pub(crate) fn doc_thumb(doc: &Document, max: u32) -> (u32, u32, Vec<u8>) {
    let tree = doc.composite_tree();
    let mut level = 0;
    while {
        let (w, h) = level_size(doc.width, doc.height, level);
        w.max(h) > max * 2 && level < 16
    } {
        level += 1;
    }
    let small = emulsion_raster::composite::flatten(&tree, level);
    let img = image::RgbaImage::from_raw(small.width(), small.height(), small.to_srgba8())
        .expect("sized");
    let s = (max as f64 / small.width().max(small.height()) as f64).min(1.0);
    let (w, h) = (
        ((small.width() as f64 * s).round() as u32).max(1),
        ((small.height() as f64 * s).round() as u32).max(1),
    );
    let mut t = image::imageops::thumbnail(&img, w, h).into_raw();
    for px in t.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    (w, h, t)
}

fn ago(secs: u64) -> String {
    emulsion_io::recent::ago(secs)
}

impl EditorView {
    // ── Autosave ────────────────────────────────────────────────────────

    pub(crate) fn start_autosave(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(AUTOSAVE_SECS))
                    .await;
                if this.update(cx, |this, cx| this.autosave(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Commit recent work, and now and then write a recovery copy.
    pub(crate) fn autosave(&mut self, cx: &mut Context<Self>) {
        if self.editor.in_transaction() || self.drag.is_some() {
            return;
        }
        if self.editor.uncommitted() && self.editor.commit("Autosave", true).is_some() {
            self.history.last_autosave = Some(Instant::now());
            cx.notify();
        }
        let due = self
            .history
            .last_recovery
            .is_none_or(|t| t.elapsed().as_secs() >= RECOVERY_SECS);
        if !due
            || self.history.recovery_busy
            || !self.editor.is_modified()
            || self.history.recovery_rev == self.editor.revision
        {
            return;
        }
        let path = self
            .history
            .recovery
            .get_or_insert_with(|| {
                let safe: String = self
                    .name
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .take(48)
                    .collect();
                recovery_dir().join(format!(
                    "{safe}-{}-{}.ora",
                    std::process::id(),
                    emulsion_io::recent::now()
                ))
            })
            .clone();
        let (doc, graph, rev) = (
            self.editor.doc.clone(),
            self.editor.graph.clone(),
            self.editor.revision,
        );
        self.history.recovery_busy = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    std::fs::create_dir_all(recovery_dir())?;
                    emulsion_io::save_full(&doc, &graph, &path).map_err(std::io::Error::other)
                })
                .await;
            this.update(cx, |this, _| {
                this.history.recovery_busy = false;
                this.history.last_recovery = Some(Instant::now());
                match result {
                    Ok(()) => this.history.recovery_rev = rev,
                    Err(e) => tracing::warn!("recovery copy failed: {e}"),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Remove the recovery copy (after a save, or when the work is discarded).
    pub fn discard_recovery(&mut self) {
        if let Some(p) = self.history.recovery.take() {
            let _ = std::fs::remove_file(p);
        }
        self.history.recovery_rev = 0;
    }

    /// Short status text: "autosaved 12s ago".
    pub(crate) fn autosave_note(&self) -> Option<String> {
        let t = self.history.last_autosave?;
        let s = t.elapsed().as_secs();
        Some(if s < 60 {
            format!("autosaved {s}s ago")
        } else {
            format!("autosaved {}m ago", s / 60)
        })
    }

    // ── Branch operations ───────────────────────────────────────────────

    pub fn open_history(&mut self, cx: &mut Context<Self>) {
        self.history.open = true;
        cx.notify();
    }

    pub fn close_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history.open = false;
        self.history.new_branch = None;
        window.focus(&self.canvas_focus, cx);
        cx.notify();
    }

    fn after_graph_change(&mut self, cx: &mut Context<Self>) {
        self.invalidate_pending_edits();
        self.drag = None;
        self.warp = None;
        self.history.selected = None;
        self.after_change(cx);
    }

    pub fn create_branch(&mut self, name: &str, at: Option<CommitId>, cx: &mut Context<Self>) {
        let from = self.editor.graph.head().to_string();
        let r = match at {
            Some(c) if c != self.editor.graph.head_branch().tip => self.editor.branch_at(name, c),
            _ => self.editor.branch(name),
        };
        match r {
            Ok(()) => {
                self.set_status(
                    format!("Now on branch {name}. {from} is unchanged."),
                    false,
                    cx,
                );
                self.after_graph_change(cx);
            }
            Err(e) => self.set_status(e.to_string(), true, cx),
        }
    }

    pub fn switch_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        match self.editor.checkout(name) {
            Ok(()) => {
                self.set_status(format!("Switched to {name}"), false, cx);
                self.after_graph_change(cx);
            }
            Err(e) => self.set_status(e.to_string(), true, cx),
        }
    }

    pub fn delete_branch(&mut self, name: &str, cx: &mut Context<Self>) {
        match self.editor.delete_branch(name) {
            Ok(()) => {
                self.set_status(format!("Deleted branch {name}"), false, cx);
                self.after_graph_change(cx);
            }
            Err(e) => self.set_status(e.to_string(), true, cx),
        }
    }

    /// Merge `from` into the head branch, or stop to ask about conflicts.
    pub fn merge_branch(
        &mut self,
        from: &str,
        choices: HashMap<ConflictKey, Side>,
        cx: &mut Context<Self>,
    ) {
        match self.editor.merge(from, &choices) {
            Ok(MergeOutcome::Merged(_)) => {
                self.history.merge = None;
                let into = self.editor.graph.head().to_string();
                self.set_status(
                    format!("Merged {from} into {into}. One undo step reverts it."),
                    false,
                    cx,
                );
                self.after_graph_change(cx);
            }
            Ok(MergeOutcome::Conflicts(conflicts)) => {
                let n = conflicts.len();
                self.history.merge = Some(PendingMerge {
                    from: from.to_string(),
                    conflicts,
                    choices,
                });
                self.history.open = true;
                self.set_status(
                    format!(
                        "{n} change{} made on both branches. Pick which to keep.",
                        if n == 1 { " was" } else { "s were" }
                    ),
                    false,
                    cx,
                );
                cx.notify();
            }
            Err(e) => self.set_status(e.to_string(), true, cx),
        }
    }

    fn finish_merge(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.history.merge.take() else {
            return;
        };
        if m.conflicts.iter().any(|c| !m.choices.contains_key(&c.key)) {
            self.history.merge = Some(m);
            self.set_status("Pick a side for every change first.", true, cx);
            return;
        }
        self.merge_branch(&m.from, m.choices, cx);
    }

    pub fn restore_commit(&mut self, id: CommitId, cx: &mut Context<Self>) {
        match self.editor.restore(id) {
            Ok(()) => {
                self.set_status("Restored. Undo brings the newer version back.", false, cx);
                self.after_change(cx);
            }
            Err(e) => self.set_status(e.to_string(), true, cx),
        }
    }

    fn start_new_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.history.selected;
        let state =
            cx.new(|cx| InputState::new(window, cx).placeholder("branch name, e.g. warm-grade"));
        state.update(cx, |s, cx| s.focus(window, cx));
        let sub = cx.subscribe_in(&state, window, move |this, st, ev: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                let name = st.read(cx).value().trim().to_string();
                if !name.is_empty() {
                    this.history.new_branch = None;
                    this.create_branch(&name, at, cx);
                }
            }
        });
        self.history.new_branch = Some((state, sub));
        cx.notify();
    }

    // ── Thumbnails ──────────────────────────────────────────────────────

    fn commit_thumb(&mut self, id: CommitId, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        if let Some(t) = self.history.thumbs.get(&id) {
            return Some(t.clone());
        }
        if self.history.loading.insert(id) {
            let doc = self.editor.graph.commit(id)?.doc.clone();
            cx.spawn(async move |this, cx| {
                let (w, h, bgra) = cx
                    .background_spawn(async move { doc_thumb(&doc, THUMB) })
                    .await;
                this.update(cx, |this, cx| {
                    this.history
                        .thumbs
                        .insert(id, Arc::new(viewport::bgra_image(w, h, bgra)));
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        None
    }

    fn current_thumb(&mut self, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        let rev = self.editor.revision;
        let fresh = self
            .history
            .current
            .as_ref()
            .filter(|(r, _)| *r == rev)
            .map(|(_, t)| t.clone());
        if fresh.is_none() && self.history.current_loading != Some(rev) {
            self.history.current_loading = Some(rev);
            let doc = self.editor.doc.clone();
            cx.spawn(async move |this, cx| {
                let (w, h, bgra) = cx
                    .background_spawn(async move { doc_thumb(&doc, THUMB) })
                    .await;
                this.update(cx, |this, cx| {
                    this.history.current = Some((rev, Arc::new(viewport::bgra_image(w, h, bgra))));
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        fresh.or_else(|| self.history.current.as_ref().map(|(_, t)| t.clone()))
    }

    // ── Render ──────────────────────────────────────────────────────────

    fn branch_color(&self, name: &str, p: &Palette) -> Hsla {
        if name == MAIN {
            return p.ink;
        }
        let i = self
            .editor
            .graph
            .branches()
            .keys()
            .filter(|k| k.as_str() != MAIN)
            .position(|k| k == name)
            .unwrap_or(0);
        rgb(BRANCH_COLORS[i % BRANCH_COLORS.len()]).into()
    }

    /// The top-bar badge: current branch and how far ahead of main it is.
    pub(crate) fn branch_badge(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let g = &self.editor.graph;
        let head = g.head().to_string();
        let meta = if head == MAIN {
            let n = g.branches().len() - 1;
            match n {
                0 => "history".to_string(),
                1 => "1 other branch".to_string(),
                n => format!("{n} other branches"),
            }
        } else {
            format!("{} ahead", g.ahead(&head, MAIN))
        };
        let color = self.branch_color(&head, p);
        let accent = p.accent;
        div()
            .id("branch-badge")
            .flex()
            .items_center()
            .gap(px(7.))
            .px(px(9.))
            .py(px(4.))
            .border_1()
            .border_color(p.ink)
            .bg(p.panel)
            .cursor_pointer()
            .hover(move |s| s.border_color(accent))
            .on_click(cx.listener(|this, _, window, cx| {
                if this.history.open {
                    this.close_history(window, cx);
                } else {
                    this.open_history(cx);
                }
            }))
            .child(div().size(px(6.)).rounded_full().bg(color))
            .child(mono(format!("branch / {head}"), 10., p.ink).whitespace_nowrap())
            .child(mono(meta, 10., p.muted).whitespace_nowrap())
    }

    pub(crate) fn history_page(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let g = self.editor.graph.clone();
        let head = g.head().to_string();
        let head_tip = g.head_branch().tip;
        let base = g.head_branch().base;
        let selected = self.history.selected.unwrap_or(base);
        let uncommitted = self.editor.uncommitted();

        // Columns: main first, then the rest by name.
        let mut names: Vec<String> = g.branches().keys().cloned().collect();
        names.sort_by_key(|n| (n != MAIN, n.clone()));
        let mut columns = div()
            .id("branch-columns")
            .flex()
            .gap(px(24.))
            .items_start()
            .overflow_x_scroll()
            .pb(px(14.));
        for (bi, name) in names.iter().enumerate() {
            let b = g.branches()[name];
            let color = self.branch_color(name, p);
            // Commits made on this branch, plus its base when that lives elsewhere.
            let mut ids: Vec<CommitId> = g
                .commits()
                .filter(|c| &c.branch == name)
                .map(|c| c.id)
                .collect();
            if !ids.contains(&b.base) {
                ids.insert(0, b.base);
            }
            let reach = g.ancestors(b.tip);
            ids.retain(|id| reach.contains(id));
            let is_head = *name == head;
            let mut col = div().flex().flex_col().min_w(px(225.)).flex_none().child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mb(px(13.))
                    .child(div().size(px(9.)).flex_none().bg(color))
                    .child(mono(name.clone(), 10.5, p.ink).whitespace_nowrap())
                    .child(div().flex_1())
                    .when(!is_head, |d| {
                        let n = name.clone();
                        d.child(chip(("switch", bi), "switch", false, p).on_click(
                            cx.listener(move |this, _, _, cx| this.switch_branch(&n, cx)),
                        ))
                    })
                    .when(!is_head && name != MAIN, |d| {
                        let n = name.clone();
                        d.child(chip(("delete", bi), "delete", false, p).on_click(
                            cx.listener(move |this, _, _, cx| this.delete_branch(&n, cx)),
                        ))
                    }),
            );
            for (ci, id) in ids.iter().enumerate() {
                let c = g.commit(*id).expect("listed");
                let on = *id == selected;
                let fill = if *id == b.tip && !(is_head && uncommitted) {
                    color
                } else {
                    p.paper
                };
                let who = if c.auto {
                    "auto"
                } else if c.parents.len() > 1 {
                    "merge"
                } else {
                    "you"
                };
                let from_elsewhere = c.branch != *name;
                let meta = if from_elsewhere {
                    format!("{} · from {}", ago(c.time), c.branch)
                } else {
                    format!("{} · {who}", ago(c.time))
                };
                let id = *id;
                let accent = p.accent;
                col = col.child(
                    div()
                        .id(("commit", id))
                        .flex()
                        .gap(px(11.))
                        .cursor_pointer()
                        .when(on, |d| d.bg(accent.opacity(0.08)))
                        .hover(move |s| s.bg(accent.opacity(0.05)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.history.selected = Some(id);
                            cx.notify();
                        }))
                        .child(
                            div()
                                .w(px(11.))
                                .flex_none()
                                .flex()
                                .flex_col()
                                .items_center()
                                .child(
                                    div()
                                        .size(px(9.))
                                        .rounded_full()
                                        .border_2()
                                        .border_color(color)
                                        .bg(fill),
                                )
                                .child(div().flex_1().w(px(1.)).bg(p.line)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .gap(px(3.))
                                .pb(px(16.))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.ink)
                                        .child(c.name.clone()),
                                )
                                .child(mono(meta, 9.5, p.muted)),
                        ),
                );
                let _ = ci;
            }
            if is_head {
                col = col.child(
                    div()
                        .flex()
                        .gap(px(11.))
                        .child(
                            div().w(px(11.)).flex_none().flex().justify_center().child(
                                div()
                                    .size(px(9.))
                                    .rounded_full()
                                    .border_2()
                                    .border_color(color)
                                    .bg(if uncommitted { color } else { p.paper }),
                            ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child("HEAD"),
                                )
                                .child(mono(
                                    if uncommitted {
                                        "uncommitted"
                                    } else {
                                        "up to date"
                                    },
                                    9.5,
                                    p.muted,
                                )),
                        ),
                );
            }
            columns = columns.child(col);
        }

        let new_branch: AnyElement = match &self.history.new_branch {
            Some((state, _)) => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().w(px(260.)).child(Input::new(state)))
                .child(mono("enter to create · esc to cancel", 9.5, p.muted))
                .into_any_element(),
            None => {
                let from = if selected == head_tip || self.history.selected.is_none() {
                    "New branch from here".to_string()
                } else {
                    format!(
                        "New branch from “{}”",
                        g.commit(selected).map(|c| c.name.as_str()).unwrap_or("?")
                    )
                };
                button("new-branch", from, false, p)
                    .on_click(cx.listener(|this, _, window, cx| this.start_new_branch(window, cx)))
                    .into_any_element()
            }
        };

        let left = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(300.))
            .min_h_0()
            .px(px(32.))
            .py(px(30.))
            .border_r_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(label("History · branch anything", p))
                    .child(div().flex_1())
                    .child(chip("back-to-canvas", "back to canvas", false, p).on_click(
                        cx.listener(|this, _, window, cx| this.close_history(window, cx)),
                    )),
            )
            .child(
                div()
                    .mt(px(6.))
                    .mb(px(18.))
                    .text_size(px(30.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.name.clone()),
            )
            .child(div().mb(px(22.)).child(new_branch))
            .child(columns);

        let right = self.compare_pane(selected, p, cx);
        div()
            .id("history-page")
            .flex()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .overflow_y_scroll()
            .bg(p.paper)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    if this.history.new_branch.is_some() {
                        this.history.new_branch = None;
                        cx.notify();
                    } else {
                        this.close_history(window, cx);
                    }
                }
            }))
            .child(left)
            .child(right)
    }

    fn compare_pane(
        &mut self,
        selected: CommitId,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let g = &self.editor.graph;
        let Some(c) = g.commit(selected) else {
            return div().w(px(330.)).into_any_element();
        };
        let (c_name, c_branch, c_time) = (c.name.clone(), c.branch.clone(), c.time);
        let rows: Vec<DiffRow> = compare(&c.doc, &self.editor.doc);
        let head = g.head().to_string();
        // Merging needs a branch whose tip is not already in this branch.
        let mergeable = [c_branch.clone()]
            .into_iter()
            .filter(|b| *b != head)
            .find(|b| {
                g.branch(b)
                    .map(|br| !g.ancestors(g.head_branch().tip).contains(&br.tip))
                    .unwrap_or(false)
            });
        let a = self.commit_thumb(selected, cx);
        let b = self.current_thumb(cx);
        let thumb = |t: Option<Arc<RenderImage>>, border: Hsla| {
            div()
                .flex_1()
                .h(px(140.))
                .border_1()
                .border_color(border)
                .bg(p.stage)
                .overflow_hidden()
                .children(t.map(|t| {
                    img(ImageSource::Render(t))
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                }))
        };
        let mut pane = div()
            .flex()
            .flex_col()
            .w(px(330.))
            .flex_none()
            .px(px(24.))
            .py(px(28.))
            .bg(p.panel)
            .child(div().mb(px(13.)).child(label("Compare two points", p)))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .mb(px(8.))
                    .child(thumb(a, p.ink))
                    .child(thumb(b, p.accent)),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .mb(px(14.))
                    .child(mono(format!("{c_name} · {}", ago(c_time)), 9.5, p.muted))
                    .child(mono("now", 9.5, p.accent)),
            );
        if rows.is_empty() {
            pane = pane.child(mono("identical", 10.5, p.muted));
        }
        for r in rows.iter().take(12) {
            pane = pane.child(
                div()
                    .flex()
                    .justify_between()
                    .gap(px(10.))
                    .py(px(8.))
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        mono(r.label.clone(), 10.5, p.muted)
                            .overflow_hidden()
                            .whitespace_nowrap(),
                    )
                    .child(mono(format!("{} → {}", r.a, r.b), 10.5, p.ink).whitespace_nowrap()),
            );
        }
        if rows.len() > 12 {
            pane = pane.child(mono(format!("+{} more", rows.len() - 12), 9.5, p.muted).pt(px(6.)));
        }
        pane = pane.child(
            div().flex().gap(px(8.)).mt(px(18.)).child(
                button("restore", "Restore this", false, p)
                    .flex_1()
                    .on_click(cx.listener(move |this, _, _, cx| this.restore_commit(selected, cx))),
            ),
        );
        if let Some(from) = mergeable {
            let label_text = format!("Merge {from} into {head}");
            pane = pane.child(button("merge", label_text, true, p).mt(px(10.)).on_click(
                cx.listener(move |this, _, _, cx| this.merge_branch(&from, HashMap::new(), cx)),
            ));
        }
        if let Some(m) = &self.history.merge {
            pane = pane.child(self.conflict_list(m, p, cx));
        }
        pane.into_any_element()
    }

    fn conflict_list(
        &self,
        m: &PendingMerge,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let head = self.editor.graph.head().to_string();
        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .mt(px(22.))
            .pt(px(14.))
            .border_t_1()
            .border_color(p.ink)
            .child(label(format!("Merging {} · both changed", m.from), p));
        for (i, c) in m.conflicts.iter().enumerate() {
            let key = c.key;
            let pick = m.choices.get(&key).copied();
            let side = |id: &'static str, text: String, s: Side| {
                chip((id, i), text, pick == Some(s), p).on_click(cx.listener(
                    move |this, _, _, cx| {
                        if let Some(m) = &mut this.history.merge {
                            m.choices.insert(key, s);
                            cx.notify();
                        }
                    },
                ))
            };
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .child(c.what.clone()),
                    )
                    .child(side("keep-ours", format!("{head}: {}", c.ours), Side::Ours))
                    .child(side(
                        "take-theirs",
                        format!("{}: {}", m.from, c.theirs),
                        Side::Theirs,
                    )),
            );
        }
        let ready = m.conflicts.iter().all(|c| m.choices.contains_key(&c.key));
        list.child(
            div()
                .flex()
                .gap(px(8.))
                .child(
                    button(
                        "finish-merge",
                        if ready {
                            "Finish merge"
                        } else {
                            "Pick a side for each"
                        },
                        ready,
                        p,
                    )
                    .flex_1()
                    .on_click(cx.listener(|this, _, _, cx| this.finish_merge(cx))),
                )
                .child(
                    button("cancel-merge", "Cancel", false, p).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.history.merge = None;
                            cx.notify();
                        },
                    )),
                ),
        )
    }
}
