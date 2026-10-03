//! Production hand-offs for storyboards. File → Extract Scenes… writes a
//! run of scenes to a new `.emu` for another artist; File → Merge Extracted
//! Scenes… reads their file back, shows the range and every conflict with
//! a Take theirs / Keep mine choice, and applies it as one Undo step.
//! File → Export Layered Scenes… writes each panel of the chosen scenes as
//! a layered ORA or PSD file with a JSON per scene. Files are read and
//! written off the UI thread with progress.
use super::*;
use crate::file_prompt::FilePrompts;
use emulsion_core::project::{PageId, Project};
use emulsion_core::storyboard::GroupId;
use emulsion_core::storyboard_extract::{MergeOptions, MergeReport, Resolution};
use emulsion_io::storyboard_export::{self as story, layered};
use emulsion_io::storyboard_extract::{read_extract, write_extract};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// One scene of the outline, for pickers.
#[derive(Clone)]
struct SceneRow {
    id: GroupId,
    name: String,
    panels: usize,
    frames: u64,
}

fn scene_rows(project: &Project) -> Vec<SceneRow> {
    let Some(board) = &project.storyboard else {
        return Vec::new();
    };
    let layout: Vec<_> = project.pages.iter().map(|p| p.meta.id).collect();
    board
        .outline(&layout)
        .into_iter()
        .map(|s| SceneRow {
            id: s.scene,
            name: board.scenes[&s.scene].name.clone(),
            panels: s.panels.len(),
            frames: s
                .panels
                .iter()
                .filter(|id| board.panels[id].thumbnails.is_none())
                .map(|id| u64::from(board.panels[id].frames))
                .sum(),
        })
        .collect()
}

/// A dropdown button choosing one of `labels`; `pick` gets the index.
fn picker<T: 'static>(
    id: &'static str,
    label: String,
    labels: Vec<String>,
    disabled: bool,
    cx: &Context<T>,
    pick: fn(&mut T, usize),
) -> AnyElement {
    let owner = cx.weak_entity();
    Button::new(id)
        .label(label)
        .small()
        .outline()
        .dropdown_caret(true)
        .disabled(disabled)
        .dropdown_menu(move |mut menu, _, _| {
            for (i, label) in labels.iter().enumerate() {
                let owner = owner.clone();
                menu = menu.item(PopupMenuItem::new(label.clone()).on_click(move |_, _, cx| {
                    owner
                        .update(cx, |this, cx| {
                            pick(this, i);
                            cx.notify();
                        })
                        .ok();
                }));
            }
            menu
        })
        .into_any_element()
}

/// File → Extract Scenes…: a run of scenes and where to write it.
pub(crate) struct ExtractDialog {
    editor: WeakEntity<EditorView>,
    project: Arc<Project>,
    name: String,
    scenes: Vec<SceneRow>,
    pub(crate) from: usize,
    pub(crate) to: usize,
    busy: bool,
    pub(crate) message: Option<String>,
    /// Claim the extracted scenes for the artist named in `claim_for`.
    pub(crate) claim: bool,
    pub(crate) claim_for: Entity<InputState>,
}

impl ExtractDialog {
    fn new(
        editor: WeakEntity<EditorView>,
        project: Project,
        name: String,
        selection: &[PageId],
        claim_for: Entity<InputState>,
    ) -> Self {
        let scenes = scene_rows(&project);
        // Start from the scenes of the selected panels.
        let board = project.storyboard.as_ref();
        let picked: Vec<usize> = scenes
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                selection.iter().any(|id| {
                    board
                        .and_then(|b| b.panels.get(id))
                        .is_some_and(|p| p.scene == s.id)
                })
            })
            .map(|(i, _)| i)
            .collect();
        let (from, to) = match (picked.first(), picked.last()) {
            (Some(a), Some(b)) => (*a, *b),
            _ => (0, 0),
        };
        Self {
            editor,
            project: Arc::new(project),
            name,
            scenes,
            from,
            to,
            busy: false,
            message: None,
            claim: false,
            claim_for,
        }
    }

    fn chosen(&self) -> Vec<GroupId> {
        let (a, b) = (self.from.min(self.to), self.from.max(self.to));
        self.scenes[a..=b.min(self.scenes.len().saturating_sub(1))]
            .iter()
            .map(|s| s.id)
            .collect()
    }

    fn choose_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let first = self
            .scenes
            .get(self.from.min(self.to))
            .map_or("", |s| s.name.as_str());
        let request = cx.prompt_save_path(
            &std::env::current_dir().unwrap_or_default(),
            Some(&format!("{} - scene {first}.emu", self.name)),
        );
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut path))) = request.await {
                path.set_extension("emu");
                this.update(cx, |this, cx| this.extract_to(path, cx)).ok();
            }
        })
        .detach();
    }

    /// Write the extract to `path` on a worker.
    pub(crate) fn extract_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.busy || self.scenes.is_empty() {
            return;
        }
        let (mut project, name, groups) = (self.project.clone(), self.name.clone(), self.chosen());
        // Claims go into the extract and, once it is written, this board.
        let mut claim = None;
        if self.claim {
            let artist = self.claim_for.read(cx).value().trim().to_string();
            let Some(device) = self.editor.update(cx, |e, _| e.device_id()).ok() else {
                return;
            };
            let now = emulsion_core::storyboard_review::now();
            let claimed = Arc::make_mut(&mut project)
                .storyboard
                .as_mut()
                .map(|b| b.claim_scenes(&groups, &artist, &device, now));
            if let Some(Err(error)) = claimed {
                self.message = Some(error);
                cx.notify();
                return;
            }
            claim = Some((groups.clone(), artist, device, now));
        }
        self.busy = true;
        self.message = Some("Writing the extract…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    write_extract(&project, &groups, &name, &path)
                        .map(|extract| (extract.pages.len(), path))
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                if result.is_ok() {
                    // The extract names this project by its ID; keep it on disk.
                    this.editor
                        .update(cx, |e, cx| {
                            e.editor.mark_storyboard_unsaved();
                            if let Some((scenes, artist, device, now)) = &claim {
                                e.edit_board(|b| b.claim_scenes(scenes, artist, device, *now), cx);
                            }
                            cx.notify();
                        })
                        .ok();
                }
                this.message = Some(match result {
                    Ok((panels, path)) => format!(
                        "Extracted {panels} panels to {}. Send it to the artist; merge it back with File → Merge Extracted Scenes….",
                        path.display()
                    ),
                    Err(e) => format!("Extract failed: {e}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for ExtractDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let labels: Vec<String> = self
            .scenes
            .iter()
            .map(|s| format!("Scene {}", s.name))
            .collect();
        let label = |i: usize| labels.get(i).cloned().unwrap_or_default();
        let chosen: Vec<&SceneRow> = self
            .scenes
            .iter()
            .filter(|s| self.chosen().contains(&s.id))
            .collect();
        let rate = story::board(&self.project)
            .map(|b| b.settings.frame_rate)
            .ok();
        let frames: u64 = chosen.iter().map(|s| s.frames).sum();
        let summary = format!(
            "{} scene{}, {} panels, {}",
            chosen.len(),
            if chosen.len() == 1 { "" } else { "s" },
            chosen.iter().map(|s| s.panels).sum::<usize>(),
            rate.map_or_else(String::new, |r| r.timecode(frames)),
        );
        let busy = self.busy;
        div()
            .id("storyboard-extract")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .text_color(p.ink)
            .child(mono(
                "Hand a run of whole scenes to another artist: their panels, cameras, sound and reference video, and the project library go into a new storyboard file that remembers where it came from.",
                10.5,
                p.muted,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child("From")
                    .child(picker(
                        "storyboard-extract-from",
                        label(self.from),
                        labels.clone(),
                        busy,
                        cx,
                        |this: &mut Self, i| this.from = i,
                    ))
                    .child("to")
                    .child(picker(
                        "storyboard-extract-to",
                        label(self.to),
                        labels.clone(),
                        busy,
                        cx,
                        |this: &mut Self, i| this.to = i,
                    )),
            )
            .child(
                div()
                    .id("storyboard-extract-summary")
                    .test_support()
                    .text_color(p.muted)
                    .child(summary),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Checkbox::new("storyboard-extract-claim")
                            .label("Claim these scenes for")
                            .checked(self.claim)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.claim = *checked;
                                cx.notify();
                            })),
                    )
                    .child(div().w(px(180.)).child(Input::new(&self.claim_for).small())),
            )
            .when_some(self.message.clone(), |d, m| {
                d.child(
                    div()
                        .id("storyboard-extract-message")
                        .test_support()
                        .whitespace_normal()
                        .child(m),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("storyboard-extract-close")
                            .label("Close")
                            .small()
                            .disabled(busy)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("storyboard-extract-button")
                            .label("Extract to…")
                            .small()
                            .primary()
                            .disabled(busy || self.scenes.is_empty())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_path(window, cx)),
                            ),
                    ),
            )
    }
}

/// File → Merge Extracted Scenes…: the extract's range and conflicts, a
/// choice per conflict, then Apply.
pub(crate) struct MergeDialog {
    editor: WeakEntity<EditorView>,
    path: Option<PathBuf>,
    /// The read extract, or why it could not be read; `None` while reading.
    extract: Option<Result<Arc<Project>, String>>,
    pub(crate) report: Option<Result<MergeReport, String>>,
    pub(crate) choices: BTreeMap<PageId, Resolution>,
    pub(crate) anyway: bool,
    /// What the last Apply did.
    pub(crate) applied: Option<String>,
    /// Why the last Apply did not go ahead.
    notice: Option<String>,
}

impl MergeDialog {
    fn choose(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_open_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose the extracted storyboard (.emu)".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                this.update(cx, |this, cx| this.load(path, cx)).ok();
            }
        })
        .detach();
    }

    /// Read `path` off the UI thread, then check it against the board.
    pub(crate) fn load(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.path = Some(path.clone());
        self.extract = None;
        self.report = None;
        self.applied = None;
        self.choices.clear();
        self.anyway = false;
        cx.notify();
        let job = emulsion_ai::jobs::Job::new();
        job.set_stage("reading the extract");
        if let Some(editor) = self.editor.upgrade() {
            editor.update(cx, |e, cx| {
                e.watch_job(job.clone(), "Reading the extract", cx)
            });
        }
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn({
                    let (path, job) = (path.clone(), job.clone());
                    async move {
                        let read = read_extract(&path).map(Arc::new).map_err(|e| e.to_string());
                        job.finish();
                        read
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                if this.path.as_ref() == Some(&path) {
                    this.extract = Some(read);
                    this.refresh(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Check the extract against the board as it is now.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.report = match &self.extract {
            Some(Ok(extract)) => {
                let extract = extract.clone();
                self.editor
                    .update(cx, |e, _| e.editor.plan_merge(&extract))
                    .ok()
            }
            Some(Err(e)) => Some(Err(e.clone())),
            None => None,
        };
        if let Some(Ok(report)) = &self.report {
            self.choices
                .retain(|id, _| report.conflicts.iter().any(|c| c.panel == *id));
        }
        cx.notify();
    }

    /// The choice shown for a conflict.
    fn choice(&self, panel: PageId, default: Resolution) -> Resolution {
        self.choices.get(&panel).copied().unwrap_or(default)
    }

    /// Merge with the choices made; true when it landed.
    pub(crate) fn apply(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(Ok(extract)) = self.extract.clone() else {
            return false;
        };
        let shown = self.report.clone();
        self.refresh(cx);
        let same = |a: &Option<Result<MergeReport, String>>| {
            a.as_ref().and_then(|r| r.as_ref().ok()).map(|r| {
                r.conflicts
                    .iter()
                    .map(|c| (c.panel, c.kind))
                    .collect::<Vec<_>>()
            })
        };
        if same(&shown) != same(&self.report) {
            self.notice = Some(
                "The storyboard changed while this was open. Check the conflicts again.".into(),
            );
            cx.notify();
            return false;
        }
        self.notice = None;
        let options = MergeOptions {
            resolutions: self.choices.clone(),
            allow_other_project: self.anyway,
        };
        match self
            .editor
            .update(cx, |e, cx| e.merge_extract_file(&extract, &options, cx))
        {
            Ok(Ok(text)) => {
                self.applied = Some(text);
                self.report = None;
                self.extract = None;
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
}

impl Render for MergeDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let mut body = div().flex().flex_col().gap(px(8.));
        match (&self.path, &self.extract, &self.report) {
            (_, _, _) if self.applied.is_some() => {
                body = body.child(
                    div()
                        .id("storyboard-merge-applied")
                        .test_support()
                        .whitespace_normal()
                        .child(self.applied.clone().unwrap()),
                );
            }
            (None, _, _) => {
                body = body.child(mono(
                    "Choose a storyboard made with File → Extract Scenes… and edited since. Its scenes replace the ones they came from, as one Undo step.",
                    10.5,
                    p.muted,
                ));
            }
            (Some(_), None, _) => body = body.child(mono("Reading the extract…", 10.5, p.muted)),
            (Some(_), _, Some(Err(error))) => {
                body = body.child(
                    div()
                        .id("storyboard-merge-error")
                        .test_support()
                        .whitespace_normal()
                        .text_color(p.accent)
                        .child(error.clone()),
                );
            }
            (Some(_), _, Some(Ok(report))) => {
                let rate = self.editor.upgrade().and_then(|e| {
                    e.read(cx)
                        .editor
                        .storyboard()
                        .map(|b| b.settings.frame_rate)
                });
                let time =
                    |frames: u64| rate.map_or_else(|| frames.to_string(), |r| r.timecode(frames));
                body = body.child(div().whitespace_normal().child(format!(
                    "Here: {} panels, {}. In the extract: {} panels, {}. Everything after the range moves by the difference.",
                    report.panels_here,
                    time(report.frames_here),
                    report.panels_there,
                    time(report.frames_there),
                )));
                if !report.same_project {
                    body = body.child(
                        div()
                            .id("storyboard-merge-other-project")
                            .test_support()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(div().text_color(p.accent).whitespace_normal().child(format!(
                                "This extract was made from another project{}. Merging it can replace the wrong panels.",
                                if report.source_name.is_empty() {
                                    String::new()
                                } else {
                                    format!(" (“{}”)", report.source_name)
                                }
                            )))
                            .child(
                                Checkbox::new("storyboard-merge-anyway")
                                    .label("Merge anyway: this project is a copy of that one")
                                    .checked(self.anyway)
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        this.anyway = *checked;
                                        cx.notify();
                                    })),
                            ),
                    );
                }
                if report.conflicts.is_empty() {
                    body = body.child(mono("No conflicts.", 10.5, p.muted));
                } else {
                    let rows = report.conflicts.iter().map(|c| {
                        let choice = self.choice(c.panel, c.default);
                        let panel = c.panel;
                        let detail = if c.changed_there
                            && c.kind
                                == emulsion_core::storyboard_extract::ConflictKind::ChangedHere
                        {
                            format!("{} · changed in the extract too", c.kind.label())
                        } else {
                            c.kind.label().to_string()
                        };
                        div()
                            .id(SharedString::from(format!(
                                "storyboard-merge-conflict-{panel}"
                            )))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(if c.name.is_empty() {
                                        format!("Panel {panel}")
                                    } else {
                                        c.name.clone()
                                    })
                                    .child(mono(detail, 10., p.muted)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(6.))
                                    .child(
                                        chip(
                                            SharedString::from(format!(
                                                "storyboard-merge-theirs-{panel}"
                                            )),
                                            "Take theirs",
                                            choice == Resolution::Theirs,
                                            &p,
                                        )
                                        .test_support()
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.choices.insert(panel, Resolution::Theirs);
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                    .child(
                                        chip(
                                            SharedString::from(format!(
                                                "storyboard-merge-mine-{panel}"
                                            )),
                                            "Keep mine",
                                            choice == Resolution::Mine,
                                            &p,
                                        )
                                        .test_support()
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.choices.insert(panel, Resolution::Mine);
                                                cx.notify();
                                            }),
                                        ),
                                    ),
                            )
                    });
                    body = body
                        .child(mono(
                            format!("{} conflicts", report.conflicts.len()),
                            10.,
                            p.muted,
                        ))
                        .child(
                            div()
                                .id("storyboard-merge-conflicts")
                                .test_support()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .max_h(px(320.))
                                .overflow_y_scroll()
                                .children(rows),
                        );
                }
            }
            (Some(_), _, None) => body = body.child(mono("Checking the extract…", 10.5, p.muted)),
        }
        div()
            .id("storyboard-merge")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .text_color(p.ink)
            .child(body)
    }
}

impl crate::dialog_actions::DialogActions for MergeDialog {
    /// The notice and Choose / Close / Apply, pinned in the dialog footer.
    fn render_actions(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let ready = self.applied.is_none()
            && self.path.is_some()
            && self.extract.is_some()
            && matches!(&self.report, Some(Ok(report)) if report.same_project || self.anyway);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .when_some(self.notice.clone(), |d, notice| {
                d.child(div().text_color(p.accent).whitespace_normal().child(notice))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("storyboard-merge-choose")
                            .label("Choose extract…")
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| this.choose(cx))),
                    )
                    .child(
                        Button::new("storyboard-merge-close")
                            .label("Close")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("storyboard-merge-apply")
                            .label("Apply")
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

/// File → Export Layered Scenes…: format, scenes, names and folder.
pub(crate) struct LayeredExport {
    project: Arc<Project>,
    name: String,
    scenes: Vec<SceneRow>,
    /// Scenes of the Board selection.
    selected: Vec<GroupId>,
    pub(crate) options: layered::Options,
    pattern: Entity<InputState>,
    scene_pattern: Entity<InputState>,
    busy: bool,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    pub(crate) message: Option<String>,
    _subs: Vec<Subscription>,
}

impl LayeredExport {
    fn new(
        project: Project,
        name: String,
        selection: &[PageId],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let options = layered::Options::default();
        let input = |text: &str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).default_value(text.to_string()))
        };
        let pattern = input(&options.pattern, window, cx);
        let scene_pattern = input(&options.scene_pattern, window, cx);
        let subs = [&pattern, &scene_pattern]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |_, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                })
            })
            .collect();
        let scenes = scene_rows(&project);
        let selected = scenes
            .iter()
            .filter(|s| {
                selection.iter().any(|id| {
                    project
                        .storyboard
                        .as_ref()
                        .and_then(|b| b.panels.get(id))
                        .is_some_and(|p| p.scene == s.id)
                })
            })
            .map(|s| s.id)
            .collect();
        Self {
            project: Arc::new(project),
            name,
            scenes,
            selected,
            options,
            pattern,
            scene_pattern,
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(0)),
            message: None,
            _subs: subs,
        }
    }

    /// The options as entered, validated.
    fn draft(&self, cx: &App) -> anyhow::Result<layered::Options> {
        let mut options = self.options.clone();
        options.pattern = self.pattern.read(cx).value().trim().to_string();
        options.scene_pattern = self.scene_pattern.read(cx).value().trim().to_string();
        options.validate()?;
        Ok(options)
    }

    fn choose_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = cx.prompt_open_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export layered scenes into this folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = request.await
                && let Some(dir) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| this.export_to(dir, cx)).ok();
            }
        })
        .detach();
    }

    /// Write the files into `dir` on a worker, showing progress.
    pub(crate) fn export_to(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let options = match self.draft(cx) {
            Ok(options) => options,
            Err(e) => {
                self.message = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let (project, name) = (self.project.clone(), self.name.clone());
        let (cancel, done, total) = (self.cancel.clone(), self.done.clone(), self.total.clone());
        cancel.store(false, Ordering::Relaxed);
        done.store(0, Ordering::Relaxed);
        total.store(0, Ordering::Relaxed);
        self.busy = true;
        self.message = None;
        cx.notify();
        let task = cx.background_spawn(async move {
            let mut progress = |d: usize, t: usize| {
                done.store(d as u64, Ordering::Relaxed);
                total.store(t as u64, Ordering::Relaxed);
            };
            layered::write(&project, &name, &options, &dir, &cancel, &mut progress)
                .map(|written| (written, dir))
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let busy = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.busy
                    })
                    .unwrap_or(false);
                if !busy {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                this.message = Some(match result {
                    Ok((written, dir)) => format!(
                        "Wrote {} layered panels and {} scene files to {}",
                        written.panels.len(),
                        written.scenes.len(),
                        dir.display()
                    ),
                    Err(e) => format!("Export failed: {e:#}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for LayeredExport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let draft = self.draft(cx);
        let busy = self.busy;
        let progress = busy.then(|| {
            let (d, t) = (
                self.done.load(Ordering::Relaxed),
                self.total.load(Ordering::Relaxed),
            );
            if t == 0 {
                "Preparing…".to_string()
            } else {
                format!("Panel {d} of {t}")
            }
        });
        let formats = [
            (
                layered::Format::Ora,
                "storyboard-layered-ora",
                "OpenRaster (.ora)",
            ),
            (
                layered::Format::Psd,
                "storyboard-layered-psd",
                "Photoshop (.psd)",
            ),
        ]
        .into_iter()
        .map(|(format, id, label)| {
            chip(id, label, self.options.format == format, &p)
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.busy {
                        this.options.format = format;
                        cx.notify();
                    }
                }))
        });
        let scope = if self.options.scenes.is_empty() {
            "All scenes".to_string()
        } else if self.options.scenes == self.selected {
            format!("Scenes of the selected panels ({})", self.selected.len())
        } else {
            self.scenes
                .iter()
                .find(|s| self.options.scenes == [s.id])
                .map_or_else(|| "Chosen scenes".into(), |s| format!("Scene {}", s.name))
        };
        let mut labels = vec![
            "All scenes".to_string(),
            format!("Scenes of the selected panels ({})", self.selected.len()),
        ];
        labels.extend(self.scenes.iter().map(|s| format!("Scene {}", s.name)));
        let tokens = story::PANEL_TOKENS
            .iter()
            .map(|t| format!("{{{t}}}"))
            .collect::<Vec<_>>()
            .join(" ");
        div()
            .id("storyboard-layered-export")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .text_color(p.ink)
            .child(mono(
                "Each panel becomes a layered file (groups, blend modes, opacity and visibility kept), with one JSON per scene giving timing, timecode, camera keys, layer keyframes and comps for animation.",
                10.5,
                p.muted,
            ))
            .child(div().flex().flex_wrap().gap(px(6.)).children(formats))
            .child(picker(
                "storyboard-layered-scenes",
                scope,
                labels,
                busy,
                cx,
                |this: &mut Self, i| {
                    this.options.scenes = match i {
                        0 => Vec::new(),
                        1 => this.selected.clone(),
                        i => vec![this.scenes[i - 2].id],
                    }
                },
            ))
            .child("Panel file names")
            .child(Input::new(&self.pattern).small().disabled(busy))
            .child("Scene file names")
            .child(Input::new(&self.scene_pattern).small().disabled(busy))
            .child(mono(
                format!("Panel tokens: {tokens}. Scene tokens: {{project}} {{act}} {{seq}} {{scene}}. {{index:3}} pads numbers."),
                10.,
                p.muted,
            ))
            .when_some(draft.as_ref().err(), |d, e| {
                d.child(div().text_color(p.accent).child(e.to_string()))
            })
            .when_some(progress.or(self.message.clone()), |d, m| {
                d.child(
                    div()
                        .id("storyboard-layered-message")
                        .test_support()
                        .whitespace_normal()
                        .child(m),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .when(busy, |d| {
                        d.child(
                            Button::new("storyboard-layered-cancel")
                                .label("Cancel export")
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel.store(true, Ordering::Relaxed);
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        Button::new("storyboard-layered-close")
                            .label("Close")
                            .small()
                            .disabled(busy)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("storyboard-layered-button")
                            .label("Export to folder…")
                            .small()
                            .primary()
                            .disabled(busy || draft.is_err())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_folder(window, cx)),
                            ),
                    ),
            )
    }
}

impl EditorView {
    /// File menu: Shared Project…, Extract Scenes… and Merge Extracted Scenes….
    pub(super) fn extract_menu_items(menu: PopupMenu, owner: &WeakEntity<Self>) -> PopupMenu {
        let (extract, merge, shared) = (owner.clone(), owner.clone(), owner.clone());
        menu.item(
            PopupMenuItem::new(t!("file.shared_project")).on_click(move |_, window, cx| {
                shared
                    .update(cx, |e, cx| {
                        e.shared_project_dialog(window, cx);
                    })
                    .ok();
            }),
        )
        .item(
            PopupMenuItem::new("Extract Scenes…").on_click(move |_, window, cx| {
                extract
                    .update(cx, |e, cx| {
                        e.extract_scenes_dialog(window, cx);
                    })
                    .ok();
            }),
        )
        .item(
            PopupMenuItem::new("Merge Extracted Scenes…").on_click(move |_, window, cx| {
                merge
                    .update(cx, |e, cx| {
                        e.merge_extract_dialog(window, cx);
                    })
                    .ok();
            }),
        )
    }

    /// File menu: Export Layered Scenes….
    pub(super) fn layered_export_menu_item(menu: PopupMenu, owner: &WeakEntity<Self>) -> PopupMenu {
        let owner = owner.clone();
        menu.item(
            PopupMenuItem::new("Export Layered Scenes (ORA, PSD)…").on_click(
                move |_, window, cx| {
                    owner
                        .update(cx, |e, cx| {
                            e.layered_export_dialog(window, cx);
                        })
                        .ok();
                },
            ),
        )
    }

    /// File → Extract Scenes…
    pub(crate) fn extract_scenes_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ExtractDialog>> {
        let project = self.board_snapshot(cx)?;
        let selection = self.board_selection();
        let name = self.name.clone();
        let editor = cx.entity().downgrade();
        let artist = crate::app_state::settings(cx)
            .storyboard
            .review_author
            .clone();
        let view = cx.new(|cx| {
            let claim_for = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Artist's name")
                    .default_value(artist)
            });
            ExtractDialog::new(editor, project, name, &selection, claim_for)
        });
        let shown = view.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Extract scenes")
                .width(px(520.))
                .child(shown.clone())
        });
        Some(view)
    }

    /// File → Merge Extracted Scenes…
    pub(crate) fn merge_extract_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MergeDialog>> {
        if self.editor.storyboard().is_none() {
            self.set_status("Merge extracted scenes into a storyboard.", false, cx);
            return None;
        }
        let editor = cx.entity().downgrade();
        let view = cx.new(|_| MergeDialog {
            editor,
            path: None,
            extract: None,
            report: None,
            choices: BTreeMap::new(),
            anyway: false,
            applied: None,
            notice: None,
        });
        let shown = view.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            crate::dialog_actions::with_actions(dialog, &shown, window, cx)
                .title("Merge extracted scenes")
                .width(px(600.))
        });
        Some(view)
    }

    /// File → Export Layered Scenes…
    pub(crate) fn layered_export_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<LayeredExport>> {
        let project = self.storyboard_snapshot(cx)?;
        let (name, selection) = (self.name.clone(), self.board_selection());
        let view = cx.new(|cx| LayeredExport::new(project, name, &selection, window, cx));
        let shown = view.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Export layered scenes")
                .width(px(560.))
                .child(shown.clone())
        });
        Some(view)
    }

    /// Merge `extract` into the board as one Undo step; the merged panels
    /// become the Board selection. Returns what happened, for the dialog.
    pub(crate) fn merge_extract_file(
        &mut self,
        extract: &Project,
        options: &MergeOptions,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        if !self.prepare_page_action(cx) {
            return Err("Finish the current edit first.".into());
        }
        let summary = self
            .editor
            .merge_extract(extract, options)
            .inspect_err(|error| self.set_status(error.clone(), true, cx))?;
        self.after_change(cx);
        self.set_board_selection(summary.panels.clone());
        let delta = match summary.frames_delta {
            0 => "the running time is unchanged".to_string(),
            d if d > 0 => format!("the board is {d} frames longer"),
            d => format!("the board is {} frames shorter", -d),
        };
        let text = format!(
            "Merged {} panels ({} from the extract, {} kept from this board); {delta}.",
            summary.panels.len(),
            summary.took_theirs,
            summary.kept_mine
        );
        self.set_status(text.clone(), false, cx);
        Ok(text)
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
        let mut project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        let blank = project.storyboard().unwrap().blank_panel().unwrap();
        let first = project.active_page();
        project
            .insert_panels(
                Some(first),
                &blank,
                vec![
                    ("Panel 2".into(), Panel::new(0, 24)),
                    ("Panel 3".into(), Panel::new(0, 12)),
                ],
                None,
            )
            .unwrap();
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

    #[gpui_kit::test]
    fn scenes_extract_and_merge_back_through_the_dialogs(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chase.emu");
        let (e, cx) = storyboard_editor(cx);
        let ids = layout(&e, cx);
        let extract = cx
            .update(|window, cx| e.update(cx, |e, cx| e.extract_scenes_dialog(window, cx)))
            .unwrap();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-extract-summary").visible());
            extract.update(cx, |d, cx| {
                d.from = 1;
                d.to = 1;
                // Claim the scene for the artist it goes to.
                d.claim = true;
                d.claim_for
                    .update(cx, |input, cx| input.set_value("Ravi", window, cx));
                d.extract_to(file.clone(), cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let message = extract.read(cx).message.clone().unwrap();
            assert!(message.starts_with("Extracted 2 panels"), "{message}");
            assert!(
                e.read(cx).has_unsaved_changes(),
                "save to keep the project ID"
            );
            window.close_dialog(cx);
        });

        let chase =
            cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().panels[&ids[1]].scene);
        let claimed =
            |b: &emulsion_core::storyboard::Storyboard| b.claim(chase).map(|c| c.claimant.clone());
        assert_eq!(
            cx.update(|_, cx| claimed(e.read(cx).editor.storyboard().unwrap())),
            Some("Ravi".into())
        );
        // Another artist lengthens panel 2; here panel 3 changes too.
        let mut artist = ProjectEditor::open(read_extract(&file).unwrap(), None).unwrap();
        assert_eq!(claimed(artist.storyboard().unwrap()), Some("Ravi".into()));
        artist
            .edit_storyboard(|b| {
                b.panels.get_mut(&ids[1]).unwrap().frames = 48;
                b.panels.get_mut(&ids[2]).unwrap().frames = 6;
                Ok(())
            })
            .unwrap();
        emulsion_io::project::write(&artist.snapshot().unwrap(), &file).unwrap();
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

        let merge = cx
            .update(|window, cx| e.update(cx, |e, cx| e.merge_extract_dialog(window, cx)))
            .unwrap();
        cx.update(|_, cx| merge.update(cx, |d, cx| d.load(file.clone(), cx)));
        cx.run_until_parked();
        let frames = |cx: &mut VisualTestContext, id: PageId| {
            cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().panels[&id].frames)
        };
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-merge-conflicts").visible());
            let report = merge.read(cx).report.clone().unwrap().unwrap();
            assert_eq!(report.conflicts.len(), 1);
            assert_eq!(report.conflicts[0].panel, ids[2]);
            // Keep mine for panel 3.
            window.click(
                SharedString::from(format!("storyboard-merge-mine-{}", ids[2])),
                cx,
            );
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(merge.read(cx).choices[&ids[2]], Resolution::Mine);
            assert!(merge.update(cx, |d, cx| d.apply(cx)));
            window.render_frame(cx);
            assert!(window.find("storyboard-merge-applied").visible());
        });
        let merged = layout(&e, cx);
        assert_eq!(merged.len(), 3);
        assert_eq!(frames(cx, merged[1]), 48);
        assert_eq!(frames(cx, merged[2]), 30, "mine was kept");
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert_eq!(layout(&e, cx), ids);
    }

    #[gpui_kit::test]
    fn layered_scenes_export_from_the_dialog(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (e, cx) = storyboard_editor(cx);
        let view = cx
            .update(|window, cx| e.update(cx, |e, cx| e.layered_export_dialog(window, cx)))
            .unwrap();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-layered-export").visible());
            window.click("storyboard-layered-psd", cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                assert_eq!(v.options.format, layered::Format::Psd);
                v.export_to(dir.path().join("out"), cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let message = view.read(cx).message.clone().unwrap();
            assert!(
                message.starts_with("Wrote 3 layered panels and 2 scene files"),
                "{message}"
            );
        });
        assert!(dir.path().join("out/Sequence 1_Chase_1.psd").is_file());
        assert!(dir.path().join("out/Sequence 1_Chase.json").is_file());
    }
}
