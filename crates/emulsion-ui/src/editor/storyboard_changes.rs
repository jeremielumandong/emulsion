//! Change tracking in the editor: board versions, marks on the Board and
//! Timeline for panels that are new, changed or moved since a version (or
//! the last save or export), the Changes list with Next/Previous, and the
//! Save Board Version dialog. Comparing runs off the UI thread; reading an
//! old version never touches the open project.
use super::*;
use emulsion_core::project::{PageId, ProjectStamp};
use emulsion_core::storyboard_changes::{ChangeKind, PanelChange, describe_changes};
use emulsion_core::storyboard_versions::{Baseline, BoardState};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use std::collections::HashSet;

pub(crate) fn kind_color(kind: ChangeKind) -> Hsla {
    rgb(match kind {
        ChangeKind::New => 0x30A46C,
        ChangeKind::Changed => 0xF76B15,
        ChangeKind::Moved => 0x0090FF,
        ChangeKind::Deleted => 0xE5484D,
        ChangeKind::Unchanged => 0x8B8D98,
    })
    .into()
}

type Key = (Baseline, ProjectStamp, u64);

/// The changes against one baseline.
pub(crate) struct Computed {
    pub(crate) label: String,
    /// Every panel of both states, aligned.
    pub(crate) changes: Vec<PanelChange>,
    by_panel: HashMap<PageId, usize>,
    /// The baseline state, for deleted panels' pictures.
    pub(crate) old: Arc<BoardState>,
}

impl Computed {
    fn new(old: BoardState, now: &BoardState) -> Self {
        let changes = describe_changes(&old, now);
        let by_panel = changes
            .iter()
            .enumerate()
            .filter_map(|(i, c)| Some((c.new?, i)))
            .collect();
        Self {
            label: old.label.clone(),
            changes,
            by_panel,
            old: Arc::new(old),
        }
    }
    pub(crate) fn of(&self, panel: PageId) -> Option<&PanelChange> {
        self.by_panel.get(&panel).map(|i| &self.changes[*i])
    }
}

/// What the Board and Timeline mark, and the comparison behind it.
#[derive(Default)]
pub(crate) struct ChangeMarks {
    pub(crate) baseline: Option<Baseline>,
    pub(crate) show: bool,
    computed: Option<(Key, Arc<Computed>)>,
    pending: Option<Key>,
    pub(crate) error: Option<String>,
    /// What this render's marks show (see `refresh_change_marks`).
    shown: Option<Arc<Computed>>,
}

/// A baseline's name in menus.
fn baseline_label(editor: &emulsion_core::project::ProjectEditor, baseline: Baseline) -> String {
    match baseline {
        Baseline::LastSave => "Last save".into(),
        Baseline::LastExport => "Last export".into(),
        Baseline::Version(id) => editor
            .board_versions()
            .iter()
            .find(|v| v.id == id)
            .map_or("Deleted version".into(), |v| {
                format!("{} · {}", v.name, emulsion_io::recent::ago(v.time))
            }),
    }
}

/// Every baseline to offer, newest version first.
pub(crate) fn baselines(editor: &emulsion_core::project::ProjectEditor) -> Vec<(Baseline, String)> {
    editor
        .board_versions()
        .iter()
        .rev()
        .map(|v| Baseline::Version(v.id))
        .chain([Baseline::LastSave, Baseline::LastExport])
        .map(|b| (b, baseline_label(editor, b)))
        .collect()
}

impl EditorView {
    fn change_key(&self, baseline: Baseline) -> Key {
        (
            baseline,
            self.editor.stamp(),
            self.editor.board_tracking_epoch(),
        )
    }

    /// The newest version, else the last save.
    pub(crate) fn default_baseline(&self) -> Baseline {
        self.editor
            .board_versions()
            .last()
            .map_or(Baseline::LastSave, |v| Baseline::Version(v.id))
    }

    /// Track changes against `baseline` and show the marks.
    pub(crate) fn track_changes(&mut self, baseline: Baseline, cx: &mut Context<Self>) {
        let marks = &mut self.review_ui.changes;
        marks.baseline = Some(baseline);
        marks.show = true;
        marks.error = None;
        cx.notify();
    }

    /// View › Show Change Marks.
    pub(crate) fn toggle_change_marks(&mut self, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            return;
        }
        let marks = &mut self.review_ui.changes;
        if marks.baseline.is_none() {
            let baseline = self.default_baseline();
            self.track_changes(baseline, cx);
        } else {
            marks.show = !marks.show;
            cx.notify();
        }
    }

    /// The changes against the chosen baseline, compared off the UI thread;
    /// until they arrive, the previous result for the same baseline.
    pub(crate) fn tracked_changes(&mut self, cx: &mut Context<Self>) -> Option<Arc<Computed>> {
        let baseline = self.review_ui.changes.baseline?;
        let key = self.change_key(baseline);
        let marks = &self.review_ui.changes;
        let cached = marks
            .computed
            .as_ref()
            .filter(|(k, _)| k.0 == baseline)
            .map(|(k, c)| (k == &key, c.clone()));
        if cached.as_ref().is_some_and(|(fresh, _)| *fresh) || marks.pending.as_ref() == Some(&key)
        {
            return cached.map(|(_, c)| c);
        }
        let states = self
            .editor
            .board_state(baseline)
            .and_then(|old| Ok((old, self.editor.current_board_state().ok_or("")?)));
        match states {
            Err(error) => {
                self.review_ui.changes.error = Some(error);
                self.review_ui.changes.computed = None;
                return None;
            }
            Ok((old, now)) => {
                self.review_ui.changes.error = None;
                self.review_ui.changes.pending = Some(key.clone());
                cx.spawn(async move |this, cx| {
                    let computed = cx
                        .background_spawn(async move { Computed::new(old, &now) })
                        .await;
                    this.update(cx, |this, cx| {
                        let marks = &mut this.review_ui.changes;
                        if marks.pending.as_ref() == Some(&key) {
                            marks.pending = None;
                            marks.computed = Some((key, Arc::new(computed)));
                            cx.notify();
                        }
                    })
                    .ok();
                })
                .detach();
            }
        }
        cached.map(|(_, c)| c)
    }

    /// The changes now, compared on the spot when nothing fresh is cached.
    pub(crate) fn changes_now(&mut self) -> Option<Arc<Computed>> {
        let baseline = self.review_ui.changes.baseline?;
        let key = self.change_key(baseline);
        if let Some((k, c)) = &self.review_ui.changes.computed
            && *k == key
        {
            return Some(c.clone());
        }
        let old = self.editor.board_state(baseline).ok()?;
        let now = self.editor.current_board_state()?;
        let computed = Arc::new(Computed::new(old, &now));
        self.review_ui.changes.computed = Some((key, computed.clone()));
        Some(computed)
    }

    /// Next/Previous Change: the next new, changed or moved panel after (or
    /// before) the active one, wrapping around.
    pub(crate) fn step_change(&mut self, next: bool, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            return;
        }
        if self.review_ui.changes.baseline.is_none() || !self.review_ui.changes.show {
            let baseline = self
                .review_ui
                .changes
                .baseline
                .unwrap_or_else(|| self.default_baseline());
            self.track_changes(baseline, cx);
        }
        let Some(computed) = self.changes_now() else {
            let reason = self
                .editor
                .board_state(
                    self.review_ui
                        .changes
                        .baseline
                        .unwrap_or(Baseline::LastSave),
                )
                .err()
                .unwrap_or_else(|| "Nothing to compare with.".into());
            self.set_status(reason, false, cx);
            return;
        };
        let order: Vec<PageId> = self.editor.page_list().iter().map(|m| m.id).collect();
        let changed: Vec<usize> = order
            .iter()
            .enumerate()
            .filter(|(_, id)| computed.of(**id).is_some_and(|c| c.is_change()))
            .map(|(i, _)| i)
            .collect();
        let Some(at) = order.iter().position(|id| *id == self.editor.active_page()) else {
            return;
        };
        let target = if next {
            changed.iter().find(|i| **i > at).or(changed.first())
        } else {
            changed.iter().rev().find(|i| **i < at).or(changed.last())
        };
        let Some(&target) = target else {
            self.set_status(format!("No changes since {}.", computed.label), false, cx);
            return;
        };
        let id = order[target];
        self.select_page(id, cx);
        if self.board_open() {
            self.set_board_selection(vec![id]);
        }
        if let Some(change) = computed.of(id) {
            self.set_status(format!("{} — {}", change.name, change.summary()), false, cx);
        }
        cx.notify();
    }

    /// Read the changes once for a whole Board or Timeline render; each
    /// panel's marks then look themselves up.
    pub(crate) fn refresh_change_marks(&mut self, cx: &mut Context<Self>) {
        self.review_ui.changes.shown = if self.review_ui.changes.show {
            self.tracked_changes(cx)
        } else {
            None
        };
    }

    /// The marks on a panel's picture: a coloured outline and badge for a
    /// change on the Board, a coloured bar on the Timeline (`compact`), and
    /// the review badge.
    pub(crate) fn panel_marks(&self, panel: PageId, compact: bool, p: &Palette) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if self.review_ui.changes.show
            && let Some(computed) = self.review_ui.changes.shown.clone()
            && let Some(change) = computed.of(panel).filter(|c| c.is_change())
        {
            let color = kind_color(change.kind);
            if compact {
                out.push(
                    div()
                        .id(("timeline-change", panel))
                        .test_support()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(3.))
                        .bg(color)
                        .into_any_element(),
                );
            } else {
                out.push(
                    div()
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(color)
                        .rounded(px(3.))
                        .into_any_element(),
                );
                out.push(
                    div()
                        .id(("board-change", panel))
                        .test_support()
                        .aria_label(change.summary())
                        .absolute()
                        .bottom_1()
                        .left_1()
                        .px_1()
                        .rounded(px(3.))
                        .bg(color)
                        .text_color(p.accent_fg)
                        .text_size(px(10.))
                        .child(change.kind.label())
                        .into_any_element(),
                );
            }
        }
        if !compact {
            out.extend(self.review_badge(panel, p));
        }
        out
    }

    /// Board toolbar: Changes… and the review filter.
    pub(crate) fn changes_board_tools(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("board-changes")
                    .label(if self.review_ui.changes.show {
                        "Changes ●"
                    } else {
                        "Changes…"
                    })
                    .tooltip("Panels new, changed or deleted since a version")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| this.open_changes(window, cx))),
            )
            .child(self.review_board_tools(cx))
            .into_any_element()
    }

    /// View › Review: versions, changes, Compare and review layers.
    pub(crate) fn review_menu_items(
        menu: PopupMenu,
        editor: &Entity<Self>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let view = editor.read(cx);
        if view.editor.storyboard().is_none() {
            return menu;
        }
        let shown = view.review_ui.changes.show;
        let owner = editor.downgrade();
        menu.submenu("Review", window, cx, move |menu, _, _| {
            use crate::actions::*;
            let item =
                |label: &'static str,
                 run: fn(&mut EditorView, &mut Window, &mut Context<EditorView>)| {
                    let owner = owner.clone();
                    PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        owner.update(cx, |e, cx| run(e, window, cx)).ok();
                    })
                };
            menu.item(item("Save Board Version…", |e, w, cx| {
                e.save_board_version_dialog(w, cx)
            }))
            .item(item("Changes Since…", |e, w, cx| e.open_changes(w, cx)))
            .menu_with_check("Show Change Marks", shown, Box::new(ToggleChangeMarks))
            .menu("Next Change", Box::new(NextChange))
            .menu("Previous Change", Box::new(PreviousChange))
            .item(item("Compare Versions…", |e, w, cx| {
                e.open_compare(w, cx)
            }))
            .separator()
            .menu("New Review Layer", Box::new(NewReviewLayer))
        })
    }

    /// Save Board Version…: name the whole board as a version.
    pub(crate) fn save_board_version_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.storyboard().is_none() {
            return;
        }
        let count = self.editor.board_versions().len() + 1;
        let name =
            cx.new(|cx| InputState::new(window, cx).default_value(format!("Version {count}")));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let input = name.clone();
            let owner = owner.clone();
            dialog
                .title("Save board version")
                .width(px(380.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Keeps every panel's drawing, timing, captions and order under this name, to see what changed since and to compare. Save the project to keep it.")
                        .child(Input::new(&name)),
                )
                .footer(crate::widgets::form_dialog_footer("Save version"))
                .on_ok(move |_, _, cx| {
                    let name = input.read(cx).value().to_string();
                    owner
                        .update(cx, |this, cx| this.create_board_version(&name, cx))
                        .unwrap_or(false)
                })
        });
    }

    pub(crate) fn create_board_version(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        self.finish_gpu_stroke(cx);
        match self.editor.create_board_version(name) {
            Ok(id) => {
                self.set_status(format!("Saved board version “{}”.", name.trim()), false, cx);
                if self.review_ui.changes.baseline.is_some() {
                    self.review_ui.changes.baseline = Some(Baseline::Version(id));
                }
                cx.notify();
                true
            }
            Err(error) => {
                self.set_status(error, true, cx);
                false
            }
        }
    }

    /// Changes Since…: the list of changes with the baseline picker.
    pub(crate) fn open_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            self.set_status("Change tracking works on storyboards.", false, cx);
            return;
        }
        if self.review_ui.changes.baseline.is_none() {
            let baseline = self.default_baseline();
            self.track_changes(baseline, cx);
        }
        let editor = cx.entity();
        let list = cx.new(|cx| ChangesList::new(editor, cx));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Changes")
                .width(px(720.))
                .child(list.clone())
                .footer(
                    div().flex().justify_end().child(
                        Button::new("changes-close")
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
        });
    }
}

/// The Changes dialog: pick the baseline, show or hide the marks, step
/// through changes, and every change listed (deleted panels too, with their
/// old picture).
pub(crate) struct ChangesList {
    editor: WeakEntity<EditorView>,
    /// Deleted panels' pictures, by baseline state and panel.
    thumbs: HashMap<(usize, PageId), Arc<RenderImage>>,
    loading: HashSet<(usize, PageId)>,
    _subs: Vec<Subscription>,
}

const THUMB: u32 = 96;

impl ChangesList {
    fn new(editor: Entity<EditorView>, cx: &mut Context<Self>) -> Self {
        Self {
            editor: editor.downgrade(),
            thumbs: HashMap::new(),
            loading: HashSet::new(),
            _subs: vec![cx.observe(&editor, |_, _, cx| cx.notify())],
        }
    }

    /// A deleted panel's picture from the baseline state.
    fn old_thumb(
        &mut self,
        state: &Arc<BoardState>,
        panel: PageId,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        let key = (Arc::as_ptr(state) as usize, panel);
        if let Some(image) = self.thumbs.get(&key) {
            return Some(image.clone());
        }
        if self.loading.insert(key) {
            let doc = state.doc(panel)?.clone();
            spawn_thumb(doc, THUMB, cx, move |this: &mut Self, image, cx| {
                this.thumbs.insert(key, image);
                cx.notify();
            });
        }
        None
    }
}

/// Render `doc` as a thumbnail off the UI thread, then hand it to `done`.
/// Review layers follow the thumbnail preference.
pub(crate) fn spawn_thumb<T: 'static>(
    doc: Document,
    max: u32,
    cx: &mut Context<T>,
    done: impl FnOnce(&mut T, Arc<RenderImage>, &mut Context<T>) + 'static,
) {
    let hide = crate::app_state::settings(cx)
        .storyboard
        .hide_review_in_thumbnails;
    cx.spawn(async move |this, cx| {
        let (w, h, bgra) = cx
            .background_spawn(async move {
                let doc = if hide {
                    emulsion_core::storyboard_review::printable(&doc).into_owned()
                } else {
                    doc
                };
                super::history::doc_thumb(&doc, max)
            })
            .await;
        this.update(cx, |this, cx| {
            done(this, Arc::new(viewport::bgra_image(w, h, bgra)), cx)
        })
        .ok();
    })
    .detach();
}

impl Render for ChangesList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let root = div()
            .id("storyboard-changes")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(px(12.));
        let Some(editor) = self.editor.upgrade() else {
            return root.child("The storyboard was closed.");
        };
        let (computed, baseline, shown, error, choices, active) = editor.update(cx, |e, cx| {
            (
                e.tracked_changes(cx),
                e.review_ui.changes.baseline,
                e.review_ui.changes.show,
                e.review_ui.changes.error.clone(),
                baselines(&e.editor),
                e.editor.active_page(),
            )
        });
        let current = baseline.map_or(String::new(), |b| {
            choices
                .iter()
                .find(|(c, _)| *c == b)
                .map_or(String::new(), |(_, l)| l.clone())
        });
        let owner = self.editor.clone();
        let picker = Button::new("changes-since")
            .label(format!("Since: {current} ▾"))
            .small()
            .outline()
            .dropdown_menu({
                let owner = owner.clone();
                move |mut menu, _, _| {
                    for (choice, label) in &choices {
                        let (choice, owner) = (*choice, owner.clone());
                        menu = menu.item(
                            PopupMenuItem::new(label.clone())
                                .checked(Some(choice) == baseline)
                                .on_click(move |_, _, cx| {
                                    owner.update(cx, |e, cx| e.track_changes(choice, cx)).ok();
                                }),
                        );
                    }
                    menu
                }
            });
        let action =
            |id: &'static str, text: &'static str| Button::new(id).label(text).small().ghost();
        let toolbar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(picker)
            .child(
                chip("changes-marks", "Show marks", shown, &p)
                    .test_support()
                    .on_click({
                        let owner = owner.clone();
                        move |_, _, cx| {
                            owner.update(cx, |e, cx| e.toggle_change_marks(cx)).ok();
                        }
                    }),
            )
            .child(action("changes-previous", "◀ Previous").on_click({
                let owner = owner.clone();
                move |_, _, cx| {
                    owner.update(cx, |e, cx| e.step_change(false, cx)).ok();
                }
            }))
            .child(action("changes-next", "Next ▶").on_click({
                let owner = owner.clone();
                move |_, _, cx| {
                    owner.update(cx, |e, cx| e.step_change(true, cx)).ok();
                }
            }))
            .child(div().flex_1())
            .child(action("changes-save-version", "Save version…").on_click({
                let owner = owner.clone();
                move |_, window, cx| {
                    owner
                        .update(cx, |e, cx| e.save_board_version_dialog(window, cx))
                        .ok();
                }
            }))
            .child(action("changes-compare", "Compare…").on_click({
                let owner = owner.clone();
                move |_, window, cx| {
                    owner.update(cx, |e, cx| e.open_compare(window, cx)).ok();
                }
            }));
        let root = root.child(toolbar);
        if let Some(error) = error {
            return root.child(
                div()
                    .id("changes-error")
                    .test_support()
                    .text_color(p.muted)
                    .child(error),
            );
        }
        let Some(computed) = computed else {
            return root.child(mono("Comparing…", 10., p.muted));
        };
        let changes: Vec<&PanelChange> =
            computed.changes.iter().filter(|c| c.is_change()).collect();
        let count = |kind: ChangeKind| changes.iter().filter(|c| c.kind == kind).count();
        let summary = if changes.is_empty() {
            format!("No changes since {}.", computed.label)
        } else {
            format!(
                "{} new · {} changed · {} moved · {} deleted since {}",
                count(ChangeKind::New),
                count(ChangeKind::Changed),
                count(ChangeKind::Moved),
                count(ChangeKind::Deleted),
                computed.label
            )
        };
        let mut list = div()
            .id("changes-list")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_h(px(420.))
            .overflow_y_scroll();
        let thumb_h = THUMB as f32 * 9. / 16.;
        for (index, change) in changes.into_iter().enumerate() {
            let image = match change.new {
                Some(panel) => editor.update(cx, |e, cx| e.page_thumbnail(panel, THUMB, cx)),
                None => self.old_thumb(&computed.old, change.panel(), cx),
            };
            let target = change.new;
            let owner = owner.clone();
            list = list.child(
                div()
                    .id(("changes-row", index))
                    .test_support()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(6.))
                    .py(px(3.))
                    .rounded(px(3.))
                    .when(target == Some(active), |d| d.bg(p.soft_bg))
                    .when(target.is_some(), |d| {
                        d.cursor_pointer().hover(|d| d.bg(p.soft_bg))
                    })
                    .child(
                        div()
                            .flex_none()
                            .w(px(THUMB as f32))
                            .h(px(thumb_h))
                            .bg(gpui_kit::white())
                            .overflow_hidden()
                            .border_2()
                            .border_color(kind_color(change.kind))
                            .children(
                                image.map(|i| img(i).size_full().object_fit(ObjectFit::Contain)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_color(p.ink)
                                    .child(change.name.clone()),
                            )
                            .child(mono(change.summary(), 10., kind_color(change.kind))),
                    )
                    .on_click(move |_, _, cx| {
                        if let Some(panel) = target {
                            owner
                                .update(cx, |e, cx| {
                                    e.select_page(panel, cx);
                                    if e.board_open() {
                                        e.set_board_selection(vec![panel]);
                                    }
                                    cx.notify();
                                })
                                .ok();
                        }
                    }),
            );
        }
        root.child(mono(summary, 10., p.muted)).child(list)
    }
}
