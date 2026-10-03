//! Paper worksheets. File → Print Worksheets… lays out sheets of empty
//! frames (chosen panels, a scene or new panels) and prints them through
//! the print dialog or saves them as a PDF, to draw on;
//! File → Import → Paper Worksheets… reads photos or scans of the drawn
//! sheets off the UI thread, shows what was found, and places every
//! drawing on its panel as a new layer, one Undo step.
use super::*;
use crate::file_prompt::FilePrompts;
use emulsion_ai::jobs::Job;
use emulsion_core::project::{PageId, PaperPlaced, Project};
use emulsion_io::printing::Paper;
use emulsion_io::storyboard_export::{
    self as story, Profile, Scope,
    profile::builtins,
    worksheet::{self, Panels, Slot},
    worksheet_scan::{self as scan, Clean, ScanError, ScannedSheet},
};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::sync::{Arc, atomic::AtomicBool};

/// Built-in and saved storyboard PDF profiles: their paper, panels per
/// page and caption placement lay out the sheets.
fn profiles(cx: &App) -> Vec<Profile> {
    let mut all = builtins();
    all.extend(crate::print_dialog::saved_storyboard_profiles(cx));
    all
}

/// A dropdown entry: its label and what it sets on dialog `T`.
type Choice<T> = (String, Box<dyn Fn(&mut T)>);

/// A dialog told when one of its dropdown choices is picked.
trait Chosen: Sized + 'static {
    fn chosen(&mut self, _cx: &mut Context<Self>) {}
}

/// A dropdown of `choices`, each setting something on the dialog.
fn choice_menu<T: Chosen>(
    id: &'static str,
    label: String,
    choices: Vec<Choice<T>>,
    disabled: bool,
    owner: WeakEntity<T>,
) -> AnyElement {
    let choices = Arc::new(choices);
    Button::new(id)
        .label(label)
        .small()
        .outline()
        .dropdown_caret(true)
        .disabled(disabled)
        .dropdown_menu(move |mut menu, _, _| {
            for index in 0..choices.len() {
                let (owner, choices) = (owner.clone(), choices.clone());
                menu = menu.item(PopupMenuItem::new(choices[index].0.clone()).on_click(
                    move |_, _, cx| {
                        owner
                            .update(cx, |this, cx| {
                                (choices[index].1)(this);
                                this.chosen(cx);
                                cx.notify();
                            })
                            .ok();
                    },
                ));
            }
            menu
        })
        .into_any_element()
}

/// What a worksheet print covers.
#[derive(Clone, Debug, PartialEq)]
enum Cover {
    Panels(Scope),
    New,
}

/// File → Print Worksheets…: which panels, the layout and the paper.
pub(crate) struct WorksheetPrint {
    project: Arc<Project>,
    name: String,
    selection: Vec<PageId>,
    cover: Cover,
    count: Entity<InputState>,
    profiles: Vec<Profile>,
    profile: usize,
    paper: Paper,
    landscape: bool,
    busy: bool,
    message: Option<String>,
    _subs: Vec<Subscription>,
}

impl WorksheetPrint {
    fn new(
        project: Project,
        name: String,
        selection: Vec<PageId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let count = cx.new(|cx| InputState::new(window, cx).default_value("6"));
        let subs = vec![cx.subscribe(&count, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })];
        let profiles = profiles(cx);
        let first = profiles[0].clone();
        Self {
            project: Arc::new(project),
            name,
            cover: if selection.is_empty() {
                Cover::Panels(Scope::All)
            } else {
                Cover::Panels(Scope::Panels(selection.clone()))
            },
            selection,
            count,
            profiles,
            profile: 0,
            paper: first.paper,
            landscape: first.landscape,
            busy: false,
            message: None,
            _subs: subs,
        }
    }

    /// What to print and how, validated.
    fn draft(&self, cx: &App) -> anyhow::Result<(Panels, Profile)> {
        let profile = Profile {
            paper: self.paper.clone(),
            landscape: self.landscape,
            ..self.profiles[self.profile].clone()
        };
        let panels = match &self.cover {
            Cover::Panels(scope) => Panels::Existing(scope.clone()),
            Cover::New => Panels::New(
                self.count
                    .read(cx)
                    .value()
                    .trim()
                    .parse()
                    .ok()
                    .filter(|n| (1..=worksheet::MAX_NEW_PANELS).contains(n))
                    .ok_or_else(|| {
                        anyhow::anyhow!("Print 1–{} new panels", worksheet::MAX_NEW_PANELS)
                    })?,
            ),
        };
        Ok((panels, profile))
    }

    /// How many pages the print makes, or why it cannot.
    fn summary(&self, cx: &App) -> anyhow::Result<String> {
        let (panels, profile) = self.draft(cx)?;
        let sheets = worksheet::layout(&self.project, &self.name, &panels, &profile, "", "X")?;
        let frames: usize = sheets.codes.iter().map(|c| c.frames.len()).sum();
        Ok(format!(
            "{frames} frame(s) on {} page(s), {} a page.",
            sheets.codes.len(),
            profile.panels_per_page()
        ))
    }

    /// Write the worksheets to `path` on a worker.
    pub(crate) fn save_to(&mut self, mut path: PathBuf, cx: &mut Context<Self>) {
        let (panels, profile) = match self.draft(cx) {
            Ok(draft) => draft,
            Err(e) => {
                self.message = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        path.set_extension("pdf");
        let (project, name) = (self.project.clone(), self.name.clone());
        self.busy = true;
        self.message = Some("Writing worksheets…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    worksheet::write_pdf(
                        &project,
                        &name,
                        &panels,
                        &profile,
                        &path,
                        &AtomicBool::new(false),
                    )
                    .map(|codes| (codes.len(), path))
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                this.message = Some(match result {
                    Ok((pages, path)) => format!(
                        "Wrote {pages} worksheet page(s) to {}. Print them at 100% scale.",
                        path.display()
                    ),
                    Err(e) => format!("Could not write the worksheets: {e}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Print through the print dialog: its printers, paper and preview.
    fn print(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (panels, profile) = match self.draft(cx) {
            Ok(draft) => draft,
            Err(e) => {
                self.message = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        window.close_dialog(cx);
        crate::print_dialog::open_worksheets(
            self.name.clone(),
            self.project.clone(),
            panels,
            profile,
            window,
            cx,
        );
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let request = cx.prompt_save_path(
            &std::env::current_dir().unwrap_or_default(),
            Some(&format!("{} worksheets.pdf", self.name)),
        );
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = request.await {
                this.update(cx, |this, cx| this.save_to(path, cx)).ok();
            }
        })
        .detach();
    }
}

impl Chosen for WorksheetPrint {}

impl Render for WorksheetPrint {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let summary = self.summary(cx);
        let owner = cx.weak_entity();
        let scenes: Vec<_> = story::entries(&self.project)
            .map(|entries| {
                let mut seen = std::collections::HashSet::new();
                entries
                    .into_iter()
                    .filter(|e| seen.insert(e.scene_id))
                    .map(|e| (e.scene_id, e.scene))
                    .collect()
            })
            .unwrap_or_default();
        let cover = match &self.cover {
            Cover::Panels(Scope::All) => "All panels".to_string(),
            Cover::Panels(Scope::Panels(ids)) => format!("Selected panels ({})", ids.len()),
            Cover::Panels(Scope::Scene(id)) => scenes
                .iter()
                .find(|(s, _)| s == id)
                .map_or_else(|| "Scene".into(), |(_, name)| format!("Scene {name}")),
            Cover::New => "New panels".into(),
        };
        let mut covers: Vec<Choice<Self>> = vec![(
            "All panels".into(),
            Box::new(|s: &mut Self| s.cover = Cover::Panels(Scope::All)),
        )];
        if !self.selection.is_empty() {
            let ids = self.selection.clone();
            covers.push((
                format!("Selected panels ({})", ids.len()),
                Box::new(move |s: &mut Self| s.cover = Cover::Panels(Scope::Panels(ids.clone()))),
            ));
        }
        for (id, name) in &scenes {
            let id = *id;
            covers.push((
                format!("Scene {name}"),
                Box::new(move |s: &mut Self| s.cover = Cover::Panels(Scope::Scene(id))),
            ));
        }
        covers.push((
            "New panels".into(),
            Box::new(|s: &mut Self| s.cover = Cover::New),
        ));
        let layouts = self
            .profiles
            .iter()
            .enumerate()
            .map(|(i, profile)| {
                let label: Box<dyn Fn(&mut Self)> = Box::new(move |s: &mut Self| {
                    s.profile = i;
                    s.paper = s.profiles[i].paper.clone();
                    s.landscape = s.profiles[i].landscape;
                });
                (
                    format!("{} ({} a page)", profile.name, profile.panels_per_page()),
                    label,
                )
            })
            .collect();
        let papers = Paper::pdf()
            .into_iter()
            .map(|paper| {
                let name = paper.name.clone();
                let set: Box<dyn Fn(&mut Self)> =
                    Box::new(move |s: &mut Self| s.paper = paper.clone());
                (name, set)
            })
            .collect();
        let orientations: Vec<Choice<Self>> = vec![
            (
                "Landscape".into(),
                Box::new(|s: &mut Self| s.landscape = true),
            ),
            (
                "Portrait".into(),
                Box::new(|s: &mut Self| s.landscape = false),
            ),
        ];
        div()
            .id("storyboard-worksheet-print")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .text_color(p.ink)
            .child(div().text_color(p.muted).child(
                "Sheets of empty frames at the board's shape, with corner marks and a code. Draw on them, then bring photos or scans back with File → Import → Paper Worksheets….",
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(choice_menu(
                        "storyboard-worksheet-cover",
                        cover,
                        covers,
                        self.busy,
                        owner.clone(),
                    ))
                    .child(choice_menu(
                        "storyboard-worksheet-layout",
                        self.profiles[self.profile].name.clone(),
                        layouts,
                        self.busy,
                        owner.clone(),
                    ))
                    .child(choice_menu(
                        "storyboard-worksheet-paper",
                        self.paper.name.clone(),
                        papers,
                        self.busy,
                        owner.clone(),
                    ))
                    .child(choice_menu(
                        "storyboard-worksheet-orientation",
                        if self.landscape { "Landscape" } else { "Portrait" }.into(),
                        orientations,
                        self.busy,
                        owner,
                    )),
            )
            .when(self.cover == Cover::New, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child("New panels")
                        .child(
                            div()
                                .w(px(80.))
                                .child(Input::new(&self.count).small().disabled(self.busy)),
                        ),
                )
            })
            .child(
                div()
                    .id("storyboard-worksheet-summary")
                    .test_support()
                    .text_color(if summary.is_ok() {
                        p.muted
                    } else {
                        rgb(0xc98535).into()
                    })
                    .child(match &summary {
                        Ok(text) => text.clone(),
                        Err(e) => e.to_string(),
                    }),
            )
            .when_some(self.message.clone(), |d, m| {
                d.child(div().id("storyboard-worksheet-message").test_support().child(m))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("storyboard-worksheet-close")
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("storyboard-worksheet-save")
                            .label("Save PDF…")
                            .disabled(self.busy || summary.is_err())
                            .on_click(cx.listener(|this, _, _, cx| this.choose_file(cx))),
                    )
                    .child(
                        Button::new("storyboard-worksheet-print-button")
                            .label("Print…")
                            .primary()
                            .disabled(self.busy || summary.is_err())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.print(window, cx)),
                            ),
                    ),
            )
    }
}

/// One photo read for the import, with its frames as thumbnails.
struct Photo {
    path: PathBuf,
    read: Result<ScannedSheet, ScanError>,
    thumbs: Vec<Arc<RenderImage>>,
}

/// A small picture of a cleaned frame over white, for the preview.
fn thumbnail(image: &image::RgbaImage) -> Arc<RenderImage> {
    let w = 120u32;
    let h = (w * image.height() / image.width().max(1)).max(1);
    let small = image::imageops::resize(image, w, h, image::imageops::FilterType::Triangle);
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    for p in small.pixels() {
        let a = u32::from(p.0[3]);
        let over = |c: u8| ((u32::from(c) * a + 255 * (255 - a)) / 255) as u8;
        bgra.extend_from_slice(&[over(p.0[2]), over(p.0[1]), over(p.0[0]), 255]);
    }
    Arc::new(crate::viewport::bgra_image(w, h, bgra))
}

/// File → Import → Paper Worksheets…: the photos read, each frame with the
/// panel it goes to, and how to clean and place the drawings.
pub(crate) struct WorksheetImport {
    editor: WeakEntity<EditorView>,
    project: Arc<Project>,
    /// "Scene 1 · Panel 2" by panel.
    labels: std::collections::HashMap<PageId, String>,
    paths: Vec<PathBuf>,
    photos: Vec<Photo>,
    clean: Clean,
    replace: bool,
    profiles: Vec<Profile>,
    /// The layout photos without a readable code were printed with.
    layout: Option<usize>,
    job: Option<Arc<Job>>,
    /// The clean mode and layout of the last read.
    read_with: Option<(Clean, Option<usize>)>,
    message: Option<String>,
}

impl Chosen for WorksheetImport {
    /// A new clean mode or layout reads the photos again.
    fn chosen(&mut self, cx: &mut Context<Self>) {
        if self.read_with != Some((self.clean, self.layout)) {
            self.reread(cx);
        }
    }
}

impl WorksheetImport {
    fn new(
        editor: WeakEntity<EditorView>,
        project: Project,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let labels = story::entries(&project)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|e| (e.page, format!("Scene {} · Panel {}", e.scene, e.number)))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            editor,
            project: Arc::new(project),
            labels,
            paths,
            photos: Vec::new(),
            clean: Clean::default(),
            replace: false,
            profiles: profiles(cx),
            layout: None,
            job: None,
            read_with: None,
            message: None,
        }
    }

    /// Read again with the editor showing the progress card.
    fn reread(&mut self, cx: &mut Context<Self>) {
        if let Some(job) = self.read(cx) {
            self.editor
                .update(cx, |e, cx| e.watch_job(job, "Reading worksheets", cx))
                .ok();
        }
    }

    /// Read every photo with the current options on a worker; the job shows
    /// the progress and cancels.
    fn read(&mut self, cx: &mut Context<Self>) -> Option<Arc<Job>> {
        if let Some(job) = self.job.take() {
            job.cancel();
        }
        let board = self.project.storyboard.as_ref()?;
        self.read_with = Some((self.clean, self.layout));
        let layout = match self.layout {
            Some(i) => match worksheet::blank_code(&self.project, &self.profiles[i]) {
                Ok(code) => Some(code),
                Err(e) => {
                    self.message = Some(e.to_string());
                    cx.notify();
                    return None;
                }
            },
            None => None,
        };
        let (project_id, size) = (
            board.project_id.clone(),
            (board.settings.width, board.settings.height),
        );
        let (paths, clean) = (self.paths.clone(), self.clean);
        let job = Job::new();
        self.job = Some(job.clone());
        self.photos.clear();
        self.message = Some(format!("Reading {} photo(s)…", paths.len()));
        cx.notify();
        let watched = job.clone();
        cx.spawn(async move |this, cx| {
            let worker = job.clone();
            let photos = cx
                .background_spawn(async move {
                    let read = scan::scan_files(
                        &paths,
                        &project_id,
                        layout.as_ref(),
                        size,
                        clean,
                        worker.cancel_flag(),
                        |done, total| {
                            worker.set_stage(format!("reading photo {} of {total}", done + 1));
                            worker.progress(done as f32 / total.max(1) as f32);
                        },
                    );
                    let photos: Vec<_> = paths
                        .into_iter()
                        .zip(read)
                        .map(|(path, read)| Photo {
                            thumbs: read.as_ref().map_or_else(
                                |_| Vec::new(),
                                |sheet| sheet.frames.iter().map(|f| thumbnail(&f.image)).collect(),
                            ),
                            path,
                            read,
                        })
                        .collect();
                    worker.finish();
                    photos
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.job.as_ref().is_some_and(|j| Arc::ptr_eq(j, &job)) {
                    return;
                }
                this.job = None;
                this.message = if job.cancelled() {
                    Some("Reading canceled.".into())
                } else {
                    None
                };
                if !job.cancelled() {
                    this.photos = photos;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        Some(watched)
    }

    fn sheets(&self) -> Vec<ScannedSheet> {
        self.photos
            .iter()
            .filter_map(|p| p.read.as_ref().ok().cloned())
            .collect()
    }

    /// Place the drawings on the board, one Undo step; closes on success.
    pub(crate) fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (sheets, replace) = (self.sheets(), self.replace);
        let result = self
            .editor
            .update(cx, |e, cx| e.place_worksheets(&sheets, replace, cx))
            .map_err(|_| "The storyboard was closed.".to_string())
            .and_then(|r| r);
        match result {
            Ok(_) => window.close_dialog(cx),
            Err(e) => {
                self.message = Some(e);
                cx.notify();
            }
        }
    }

    fn target(&self, slot: Slot) -> String {
        match slot {
            Slot::Panel(id) => self
                .labels
                .get(&id)
                .cloned()
                .unwrap_or_else(|| "A removed panel → new panel".into()),
            Slot::New(n) => format!("New panel {n}"),
        }
    }
}

impl Render for WorksheetImport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let owner = cx.weak_entity();
        let reading = self.job.is_some();
        let ids: Vec<_> = self.project.pages.iter().map(|p| p.meta.id).collect();
        let plan = scan::plan(&self.sheets(), &ids);
        let no_code = self
            .photos
            .iter()
            .any(|p| matches!(p.read, Err(ScanError::NoCode)));
        let clean_label = |c: Clean| match c {
            Clean::Transparent => "Paper transparent",
            Clean::White => "Paper white",
            Clean::LineArt => "Line art only",
            Clean::Photo => "Photo as it is",
        };
        let cleans = [
            Clean::Transparent,
            Clean::White,
            Clean::LineArt,
            Clean::Photo,
        ]
        .into_iter()
        .map(|c| {
            let set: Box<dyn Fn(&mut Self)> = Box::new(move |s: &mut Self| s.clean = c);
            (clean_label(c).to_string(), set)
        })
        .collect();
        let modes: Vec<Choice<Self>> = vec![
            (
                "Add a layer".into(),
                Box::new(|s: &mut Self| s.replace = false),
            ),
            (
                "Replace earlier paper drawings".into(),
                Box::new(|s: &mut Self| s.replace = true),
            ),
        ];
        let layouts = self
            .profiles
            .iter()
            .enumerate()
            .map(|(i, profile)| {
                let set: Box<dyn Fn(&mut Self)> = Box::new(move |s: &mut Self| s.layout = Some(i));
                (profile.name.clone(), set)
            })
            .collect();
        let mut list = div()
            .id("storyboard-worksheet-photos")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .max_h(px(420.))
            .overflow_y_scroll();
        for photo in &self.photos {
            let file = photo
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            list = list.child(match &photo.read {
                Err(e) => div()
                    .text_color(rgb(0xc98535))
                    .child(format!("{file}: {e}")),
                Ok(sheet) => {
                    let mut frames = div().flex().flex_wrap().gap_2();
                    for (frame, thumb) in sheet.frames.iter().zip(&photo.thumbs) {
                        frames = frames.child(
                            div()
                                .flex()
                                .flex_col()
                                .w(px(120.))
                                .child(img(ImageSource::Render(thumb.clone())).w(px(120.)))
                                .child(div().text_color(p.muted).child(if frame.drawn() {
                                    self.target(frame.slot)
                                } else {
                                    "Empty, skipped".into()
                                })),
                        );
                    }
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(format!(
                            "{file}: sheet {}{}",
                            sheet.code.sheet,
                            if sheet.code_read {
                                ""
                            } else {
                                " (layout chosen by hand)"
                            }
                        ))
                        .child(frames)
                }
            });
        }
        let summary = format!(
            "{} drawing(s) for existing panels, {} new panel(s), {} empty frame(s).",
            plan.drawings.len(),
            plan.new_panels.len(),
            plan.blank
        );
        div()
            .id("storyboard-worksheet-import")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .text_color(p.ink)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(choice_menu(
                        "storyboard-worksheet-clean",
                        clean_label(self.clean).into(),
                        cleans,
                        reading,
                        owner.clone(),
                    ))
                    .child(choice_menu(
                        "storyboard-worksheet-mode",
                        if self.replace {
                            "Replace earlier paper drawings"
                        } else {
                            "Add a layer"
                        }
                        .into(),
                        modes,
                        reading,
                        owner.clone(),
                    ))
                    .when(no_code || self.layout.is_some(), |d| {
                        d.child(choice_menu(
                            "storyboard-worksheet-manual",
                            self.layout.map_or_else(
                                || "Layout of sheets without a code…".into(),
                                |i| format!("Sheets without a code: {}", self.profiles[i].name),
                            ),
                            layouts,
                            reading,
                            owner,
                        ))
                    })
                    .child(
                        Button::new("storyboard-worksheet-reread")
                            .label("Read again")
                            .small()
                            .disabled(reading)
                            .on_click(cx.listener(|this, _, _, cx| this.reread(cx))),
                    ),
            )
            .child(list)
            .when(!reading, |d| {
                d.child(
                    div()
                        .id("storyboard-worksheet-plan")
                        .test_support()
                        .child(summary),
                )
            })
            .children(
                plan.notes
                    .iter()
                    .map(|n| div().text_color(p.muted).child(n.clone())),
            )
    }
}

impl crate::dialog_actions::DialogActions for WorksheetImport {
    /// The import message and Cancel / Import, pinned in the dialog footer.
    fn render_actions(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let reading = self.job.is_some();
        let ids: Vec<_> = self.project.pages.iter().map(|p| p.meta.id).collect();
        let plan = scan::plan(&self.sheets(), &ids);
        let ready = !reading && !(plan.drawings.is_empty() && plan.new_panels.is_empty());
        div()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .when_some(self.message.clone(), |d, m| {
                d.child(
                    div()
                        .id("storyboard-worksheet-import-message")
                        .test_support()
                        .child(m),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("storyboard-worksheet-cancel")
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(job) = this.job.take() {
                                    job.cancel();
                                }
                                window.close_dialog(cx);
                            })),
                    )
                    .child(
                        Button::new("storyboard-worksheet-place")
                            .label("Import")
                            .primary()
                            .disabled(!ready)
                            .on_click(cx.listener(|this, _, window, cx| this.import(window, cx))),
                    ),
            )
            .into_any_element()
    }
}

impl EditorView {
    /// File → Print Worksheets…
    pub(crate) fn print_worksheets_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.board_snapshot(cx) else {
            return;
        };
        let (name, selection) = (self.name.clone(), self.board_selection());
        let view = cx.new(|cx| WorksheetPrint::new(project, name, selection, window, cx));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Print worksheets")
                .width(px(560.))
                .child(view.clone())
        });
    }

    /// File → Import → Paper Worksheets…
    pub(crate) fn import_worksheets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_open_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import photos or scans of drawn worksheets".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.open_worksheet_import(paths, window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// The import preview for `paths`, reading them at once.
    pub(crate) fn open_worksheet_import(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        if paths.len() > scan::MAX_PHOTOS {
            self.set_status(
                format!("Import up to {} photos at a time.", scan::MAX_PHOTOS),
                true,
                cx,
            );
            return;
        }
        let Some(project) = self.board_snapshot(cx) else {
            return;
        };
        let editor = cx.weak_entity();
        let view = cx.new(|cx| WorksheetImport::new(editor, project, paths, cx));
        if let Some(job) = view.update(cx, |view, cx| view.read(cx)) {
            self.watch_job(job, "Reading worksheets", cx);
        }
        window.open_dialog(cx, move |dialog, window, cx| {
            crate::dialog_actions::with_actions(dialog, &view, window, cx)
                .title("Import paper worksheets")
                .width(px(720.))
        });
    }

    /// Put the drawn frames of `sheets` on their panels and add their new
    /// panels after the active panel, one Undo step.
    fn place_worksheets(
        &mut self,
        sheets: &[ScannedSheet],
        replace: bool,
        cx: &mut Context<Self>,
    ) -> Result<PaperPlaced, String> {
        if !self.prepare_page_action(cx) {
            return Err("Finish the current edit first.".into());
        }
        self.finish_gpu_stroke(cx);
        if self.editor.in_transaction() {
            return Err("Finish the current edit first.".into());
        }
        let ids: Vec<_> = self.editor.page_list().iter().map(|m| m.id).collect();
        let plan = scan::plan(sheets, &ids);
        let notes = plan.notes.join(" ");
        let after = self.editor.active_page();
        let placed = scan::place(&mut self.editor, plan, Some(after), replace)?;
        self.after_change(cx);
        if !placed.added.is_empty() {
            self.set_board_selection(placed.added.clone());
        }
        self.set_status(
            format!(
                "Placed {} paper drawing(s) and added {} panel(s). {notes}",
                placed.changed.len(),
                placed.added.len()
            ),
            false,
            cx,
        );
        Ok(placed)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_io::printing::Mark;
    use gpui_kit::test::TestWindowExt;

    /// A two-panel storyboard open in the editor.
    pub(in crate::editor) fn storyboard(
        cx: &mut TestAppContext,
    ) -> (Entity<EditorView>, &mut VisualTestContext) {
        let (ws, cx) = crate::tests::open(cx, Document::new(64, 36));
        cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
        let mut project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(160, 90)).unwrap();
        let blank = project.storyboard().unwrap().blank_panel().unwrap();
        project
            .insert_panels(
                Some(1),
                &blank,
                vec![(
                    "Panel 2".into(),
                    emulsion_core::storyboard::Panel::new(0, 24),
                )],
                None,
            )
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

    /// A flatbed scan of a two-up worksheet for the board's panels, with a
    /// cross drawn in the first frame and the second left empty.
    fn scan_of(project: &Project, dir: &std::path::Path) -> PathBuf {
        let profile = Profile {
            columns: 1,
            rows: 2,
            ..builtins().remove(0)
        };
        let mut sheets = worksheet::layout(
            project,
            "Board",
            &Panels::Existing(Scope::All),
            &profile,
            "2026-10-03",
            "UI000001",
        )
        .unwrap();
        let code = sheets.codes.remove(0);
        let sheet = &mut sheets.layout.sheets[0];
        let origin = worksheet::marks_origin(sheet);
        let rect = code.frame_rect(code.frames[0].1);
        for line in [[(0.2, 0.2), (0.8, 0.8)], [(0.2, 0.8), (0.8, 0.2)]] {
            sheet.marks.push(Mark::Path {
                points: line
                    .iter()
                    .map(|(u, v)| {
                        (
                            origin.0 + rect.x + u * rect.w,
                            origin.1 + rect.y + v * rect.h,
                        )
                    })
                    .collect(),
                closed: false,
                filled: false,
                stroke_mm: 1.5,
                color: [20, 20, 20],
            });
        }
        let image = worksheet::render(sheet, 2400).unwrap();
        let path = dir.join("scan.png");
        image.save(&path).unwrap();
        path
    }

    #[gpui_kit::test]
    fn worksheet_dialogs_print_and_import_drawings_as_one_undo_step(cx: &mut TestAppContext) {
        let (e, cx) = storyboard(cx);
        let dir = tempfile::tempdir().unwrap();
        cx.update(|window, cx| e.update(cx, |e, cx| e.print_worksheets_dialog(window, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-worksheet-print").visible());
            assert!(window.find("storyboard-worksheet-summary").visible());
            // Print… hands the sheets to the print dialog.
            window.click("storyboard-worksheet-print-button", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("print-worksheet-note").visible());
            assert!(window.try_find("storyboard-worksheet-print").is_none());
            window.close_dialog(cx);
        });
        let project = cx.update(|_, cx| e.read(cx).editor.snapshot().unwrap());
        let ids: Vec<_> = project.pages.iter().map(|p| p.meta.id).collect();
        let path = scan_of(&project, dir.path());
        cx.update(|window, cx| {
            e.update(cx, |e, cx| e.open_worksheet_import(vec![path], window, cx))
        });
        cx.run_until_parked();
        let layers = |cx: &mut VisualTestContext, id: PageId| {
            cx.update(|_, cx| {
                e.read(cx)
                    .editor
                    .page(id)
                    .unwrap()
                    .doc
                    .nodes
                    .iter()
                    .filter(|n| n.name.starts_with("Paper drawing"))
                    .count()
            })
        };
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-worksheet-import").visible());
            assert!(window.find("storyboard-worksheet-plan").visible());
        });
        // Press Import in the preview.
        cx.update(|window, cx| window.click("storyboard-worksheet-place", cx));
        cx.run_until_parked();
        assert_eq!(layers(cx, ids[0]), 1, "the drawn frame lands on panel 1");
        assert_eq!(layers(cx, ids[1]), 0, "the empty frame is skipped");
        cx.update(|_, cx| {
            let status = e.read(cx).status.clone().unwrap().0;
            assert!(status.contains("Placed 1 paper drawing"), "{status}");
        });
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert_eq!(layers(cx, ids[0]), 0, "one Undo takes it back off");
    }
}
