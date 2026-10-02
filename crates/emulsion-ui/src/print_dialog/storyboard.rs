//! Storyboard boards in the print dialog: File → Export Storyboard PDF… and
//! Print on a storyboard lay panels out with a PDF profile. The dialog keeps
//! its destinations, paper, preview, colour and submission; this section
//! adds the profile picker, the searchable profile options, and the panels
//! to print.
use super::*;
use anyhow::Context as _;
use emulsion_core::project::{PageId, Project};
use emulsion_io::storyboard_export::{
    self as story, Alignment, CaptionPlacement, Fit, Profile, Scope,
    profile::{self as profiles, builtins},
    sheet::{Job, Logo},
};
use std::collections::HashMap;

/// A storyboard option: how the dialog reads and writes it.
enum Kind {
    Number(fn(&Profile) -> f64, fn(&mut Profile, f64)),
    Text(fn(&Profile) -> String, fn(&mut Profile, String)),
    Choice(
        &'static [(&'static str, &'static str)],
        fn(&Profile) -> String,
        fn(&mut Profile, &str),
    ),
}

struct Opt {
    id: &'static str,
    group: &'static str,
    label: &'static str,
    kind: Kind,
}

const ON_OFF: &[(&str, &str)] = &[("true", "On"), ("false", "Off")];
const ALIGN: &[(&str, &str)] = &[("left", "Left"), ("center", "Centre"), ("right", "Right")];
const GROUPS: [&str; 5] = ["Page", "Panels", "Captions", "Header and footer", "Camera"];

fn align_id(a: Alignment) -> String {
    match a {
        Alignment::Left => "left",
        Alignment::Center => "center",
        Alignment::Right => "right",
    }
    .into()
}
fn align(v: &str) -> Alignment {
    match v {
        "center" => Alignment::Center,
        "right" => Alignment::Right,
        _ => Alignment::Left,
    }
}

/// Every profile option the dialog offers, in display order.
fn options() -> Vec<Opt> {
    use Kind::*;
    let opt = |id, group, label, kind| Opt {
        id,
        group,
        label,
        kind,
    };
    vec![
        opt(
            "sb-margin",
            "Page",
            "Margin (mm)",
            Number(|p| p.margin_mm, |p, v| p.margin_mm = v),
        ),
        opt(
            "sb-columns",
            "Panels",
            "Panels across (1–6)",
            Number(|p| f64::from(p.columns), |p, v| p.columns = v as u16),
        ),
        opt(
            "sb-rows",
            "Panels",
            "Panels down (1–8)",
            Number(|p| f64::from(p.rows), |p, v| p.rows = v as u16),
        ),
        opt(
            "sb-gutter",
            "Panels",
            "Space between panels (mm)",
            Number(|p| p.gutter_mm, |p, v| p.gutter_mm = v),
        ),
        opt(
            "sb-fit",
            "Panels",
            "Image fitting",
            Choice(
                &[("fit", "Fit · whole panel"), ("fill", "Fill · crop edges")],
                |p| if p.fit == Fit::Fill { "fill" } else { "fit" }.into(),
                |p, v| p.fit = if v == "fill" { Fit::Fill } else { Fit::Fit },
            ),
        ),
        opt(
            "sb-frame",
            "Panels",
            "Panel frame thickness (mm, 0 = none)",
            Number(|p| p.panel_frame_mm, |p, v| p.panel_frame_mm = v),
        ),
        opt(
            "sb-header",
            "Panels",
            "Panel header",
            Text(|p| p.panel_header.clone(), |p, v| p.panel_header = v),
        ),
        opt(
            "sb-header2",
            "Panels",
            "Second panel header",
            Text(
                |p| p.second_panel_header.clone(),
                |p, v| p.second_panel_header = v,
            ),
        ),
        opt(
            "sb-header-align",
            "Panels",
            "Panel header alignment",
            Choice(
                ALIGN,
                |p| align_id(p.panel_header_align),
                |p, v| p.panel_header_align = align(v),
            ),
        ),
        opt(
            "sb-header-pt",
            "Panels",
            "Panel header size (pt)",
            Number(|p| p.panel_header_pt, |p, v| p.panel_header_pt = v),
        ),
        opt(
            "sb-captions",
            "Captions",
            "Caption position",
            Choice(
                &[
                    ("below", "Below the panel"),
                    ("right", "Right of the panel"),
                    ("left", "Left of the panel"),
                    ("none", "No captions"),
                ],
                |p| {
                    match p.captions {
                        CaptionPlacement::None => "none",
                        CaptionPlacement::Below => "below",
                        CaptionPlacement::Right => "right",
                        CaptionPlacement::Left => "left",
                    }
                    .into()
                },
                |p, v| {
                    p.captions = match v {
                        "none" => CaptionPlacement::None,
                        "right" => CaptionPlacement::Right,
                        "left" => CaptionPlacement::Left,
                        _ => CaptionPlacement::Below,
                    }
                },
            ),
        ),
        opt(
            "sb-caption-share",
            "Captions",
            "Caption share of the panel box (%)",
            Number(|p| p.caption_percent, |p, v| p.caption_percent = v),
        ),
        opt(
            "sb-caption-frames",
            "Captions",
            "Caption frames",
            Choice(
                ON_OFF,
                |p| p.caption_frames.to_string(),
                |p, v| p.caption_frames = v == "true",
            ),
        ),
        opt(
            "sb-caption-titles",
            "Captions",
            "Caption field names",
            Choice(
                ON_OFF,
                |p| p.caption_titles.to_string(),
                |p, v| p.caption_titles = v == "true",
            ),
        ),
        opt(
            "sb-caption-fields",
            "Captions",
            "Caption fields (comma separated; empty = fields set to print)",
            Text(
                |p| p.caption_fields.join(", "),
                |p, v| {
                    p.caption_fields = v
                        .split(',')
                        .map(str::trim)
                        .filter(|f| !f.is_empty())
                        .map(String::from)
                        .collect()
                },
            ),
        ),
        opt(
            "sb-caption-pt",
            "Captions",
            "Caption size (pt)",
            Number(|p| p.caption_pt, |p, v| p.caption_pt = v),
        ),
        opt(
            "sb-review-notes",
            "Captions",
            "Review notes (status and open notes after the captions)",
            Choice(
                ON_OFF,
                |p| p.review_notes.to_string(),
                |p, v| p.review_notes = v == "true",
            ),
        ),
        opt(
            "sb-page-header",
            "Header and footer",
            "Page header",
            Text(|p| p.page_header.clone(), |p, v| p.page_header = v),
        ),
        opt(
            "sb-page-header-align",
            "Header and footer",
            "Page header alignment",
            Choice(
                ALIGN,
                |p| align_id(p.page_header_align),
                |p, v| p.page_header_align = align(v),
            ),
        ),
        opt(
            "sb-page-footer",
            "Header and footer",
            "Page footer",
            Text(|p| p.page_footer.clone(), |p, v| p.page_footer = v),
        ),
        opt(
            "sb-page-footer-align",
            "Header and footer",
            "Page footer alignment",
            Choice(
                ALIGN,
                |p| align_id(p.page_footer_align),
                |p, v| p.page_footer_align = align(v),
            ),
        ),
        opt(
            "sb-page-pt",
            "Header and footer",
            "Header and footer size (pt)",
            Number(|p| p.page_text_pt, |p, v| p.page_text_pt = v),
        ),
        opt(
            "sb-logo-align",
            "Header and footer",
            "Logo position",
            Choice(
                ALIGN,
                |p| align_id(p.logo_align),
                |p, v| p.logo_align = align(v),
            ),
        ),
        opt(
            "sb-logo-height",
            "Header and footer",
            "Logo height (mm)",
            Number(|p| p.logo_height_mm, |p, v| p.logo_height_mm = v),
        ),
        opt(
            "sb-camera",
            "Camera",
            "Camera frame",
            Choice(
                ON_OFF,
                |p| p.camera_frame.to_string(),
                |p, v| p.camera_frame = v == "true",
            ),
        ),
        opt(
            "sb-safe",
            "Camera",
            "Safe areas",
            Choice(
                ON_OFF,
                |p| p.safe_areas.to_string(),
                |p, v| p.safe_areas = v == "true",
            ),
        ),
        opt(
            "sb-camera-mm",
            "Camera",
            "Camera frame thickness (mm)",
            Number(|p| p.camera_frame_mm, |p, v| p.camera_frame_mm = v),
        ),
        opt(
            "sb-arrow-mm",
            "Camera",
            "Camera-move arrow thickness (mm)",
            Number(|p| p.camera_arrow_mm, |p, v| p.camera_arrow_mm = v),
        ),
    ]
}

/// Whether an option matches the search text, by its label or group.
fn found(group: &str, label: &str, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || label.to_lowercase().contains(&query)
        || group.to_lowercase().contains(&query)
}

pub(super) struct StoryboardState {
    job: Arc<Job>,
    pub(super) profile: Profile,
    /// The Board's selected panels when the dialog opened.
    selection: Vec<PageId>,
    inputs: HashMap<&'static str, Entity<InputState>>,
    logo_error: Option<String>,
    search: Entity<InputState>,
    name: Entity<InputState>,
    notice: Option<String>,
}

fn saved_profiles(cx: &App) -> Vec<Profile> {
    if cx.has_global::<crate::app_state::AppSettings>() {
        crate::app_state::settings(cx)
            .storyboard_pdf_profiles
            .clone()
    } else {
        Vec::new()
    }
}

fn update_settings(cx: &mut App, f: impl FnOnce(&mut emulsion_io::settings::Settings)) {
    if cx.has_global::<crate::app_state::AppSettings>() {
        crate::app_state::update_settings(cx, f);
    }
}

/// Built-in and saved profiles, by name.
fn all_profiles(cx: &App) -> Vec<Profile> {
    builtins().into_iter().chain(saved_profiles(cx)).collect()
}

/// Open the print dialog on a storyboard: Save PDF when `pdf`, otherwise the
/// printers. `selection` is the Board's selection.
pub(crate) fn open_storyboard(
    name: String,
    project: Project,
    selection: Vec<PageId>,
    pdf: bool,
    window: &mut Window,
    cx: &mut App,
) -> anyhow::Result<()> {
    let job = Arc::new(Job::new(
        &project,
        &name,
        &Scope::All,
        None,
        story::today(),
    )?);
    let last = cx
        .has_global::<crate::app_state::AppSettings>()
        .then(|| {
            crate::app_state::settings(cx)
                .storyboard_pdf_profile
                .clone()
        })
        .flatten();
    let profile = all_profiles(cx)
        .into_iter()
        .find(|p| Some(&p.name) == last.as_ref())
        .unwrap_or_else(|| builtins().remove(0));
    let view = cx.new(|cx| {
        let mut dialog = PrintDialog::new(name, 0, window, cx);
        dialog.attach_storyboard(job.clone(), profile, selection, window, cx);
        dialog
    });
    view.update(cx, |s, cx| {
        let sources_job = job.clone();
        s.load_prepared(
            move |cancel| story::sheet::sources(&project, &sources_job, &cancel),
            true,
            cx,
        );
        s.load_logo(cx);
        if pdf {
            s.choose_destination("pdf".into(), cx);
        } else {
            s.refresh(cx);
        }
    });
    show(
        view,
        if pdf {
            "Export Storyboard PDF"
        } else {
            "Print Storyboard"
        },
        window,
        cx,
    );
    Ok(())
}

impl PrintDialog {
    pub(super) fn attach_storyboard(
        &mut self,
        job: Arc<Job>,
        profile: Profile,
        selection: Vec<PageId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut inputs = HashMap::new();
        for opt in options() {
            let value = match opt.kind {
                Kind::Number(get, _) => get(&profile).to_string(),
                Kind::Text(get, _) => get(&profile),
                Kind::Choice(..) => continue,
            };
            let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
            self._subscriptions
                .push(cx.subscribe(&input, |this, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.changed(cx);
                    }
                }));
            inputs.insert(opt.id, input);
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search options"));
        self._subscriptions
            .push(cx.subscribe(&search, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }));
        let name = cx.new(|cx| InputState::new(window, cx).default_value(profile.name.clone()));
        self.settings.layout = Layout::Contact;
        self.scope = "all".into();
        self.storyboard = Some(StoryboardState {
            job,
            profile,
            selection,
            inputs,
            logo_error: None,
            search,
            name,
            notice: None,
        });
    }

    /// Paper and orientation from the profile, when the destination has them.
    pub(super) fn storyboard_paper(&mut self) {
        let (Some(story), Some(caps)) = (&self.storyboard, &self.caps) else {
            return;
        };
        let want = &story.profile.paper;
        let same = |p: &print::Paper| {
            (p.width - want.width).abs() < 0.5 && (p.height - want.height).abs() < 0.5
        };
        if let Some(paper) = caps
            .papers
            .iter()
            .find(|p| p.id == want.id && same(p))
            .or_else(|| caps.papers.iter().find(|p| same(p)))
        {
            self.settings.paper = paper.clone();
        }
        self.settings.landscape = story.profile.landscape;
    }

    /// Use `profile`: fill every option and the paper from it.
    pub(super) fn apply_profile(
        &mut self,
        profile: Profile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(story) = self.storyboard.as_mut() else {
            return;
        };
        for opt in options() {
            let value = match opt.kind {
                Kind::Number(get, _) => get(&profile).to_string(),
                Kind::Text(get, _) => get(&profile),
                Kind::Choice(..) => continue,
            };
            if let Some(input) = story.inputs.get(opt.id) {
                input.update(cx, |f, cx| f.set_value(value, window, cx));
            }
        }
        story
            .name
            .update(cx, |f, cx| f.set_value(profile.name.clone(), window, cx));
        story.notice = None;
        let name = profile.name.clone();
        story.profile = profile;
        self.paper_chosen = false;
        self.storyboard_paper();
        self.sheet = 0;
        update_settings(cx, |s| s.storyboard_pdf_profile = Some(name));
        self.load_logo(cx);
        self.changed(cx);
    }

    /// Load the profile's logo into the job, off the UI thread.
    fn load_logo(&mut self, cx: &mut Context<Self>) {
        let Some(story) = self.storyboard.as_mut() else {
            return;
        };
        let Some(path) = story.profile.logo.clone() else {
            if story.job.logo.is_some() {
                Arc::make_mut(&mut story.job).logo = None;
            }
            return;
        };
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_spawn({
                    let path = path.clone();
                    async move { Logo::load(&path) }
                })
                .await;
            this.update(cx, |s, cx| {
                let Some(story) = s.storyboard.as_mut() else {
                    return;
                };
                if story.profile.logo.as_ref() != Some(&path) {
                    return;
                }
                match loaded {
                    Ok(logo) => {
                        Arc::make_mut(&mut story.job).logo = Some(logo);
                        story.logo_error = None;
                    }
                    Err(e) => {
                        story.logo_error = Some(e.to_string());
                    }
                }
                s.changed(cx);
            })
            .ok();
        })
        .detach();
    }

    /// The profile as the dialog shows it: typed options read from their
    /// fields, with the paper and orientation of `settings`.
    pub(super) fn storyboard_profile(
        &self,
        settings: &Settings,
        cx: &App,
    ) -> anyhow::Result<Profile> {
        let story = self.storyboard.as_ref().context("Not a storyboard print")?;
        if let Some(error) = &story.logo_error {
            anyhow::bail!("{error}")
        }
        let mut profile = story.profile.clone();
        for opt in options() {
            let Some(input) = story.inputs.get(opt.id) else {
                continue;
            };
            let value = input.read(cx).value();
            match opt.kind {
                Kind::Number(_, set) => set(
                    &mut profile,
                    value
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .with_context(|| format!("Enter a number for {}", opt.label))?,
                ),
                Kind::Text(_, set) => set(&mut profile, value.to_string()),
                Kind::Choice(..) => {}
            }
        }
        profile.paper = settings.paper.clone();
        profile.landscape = settings.landscape;
        profile.validate()?;
        Ok(profile)
    }

    /// Indexes of the panels to print, in board order.
    fn storyboard_selected(&self) -> anyhow::Result<Vec<usize>> {
        let story = self.storyboard.as_ref().context("Not a storyboard print")?;
        let entries = &story.job.entries;
        let chosen: Vec<_> = match self.scope.as_str() {
            "selected" => entries
                .iter()
                .enumerate()
                .filter(|(_, e)| story.selection.contains(&e.page))
                .map(|(i, _)| i)
                .collect(),
            scope => match scope
                .strip_prefix("scene:")
                .and_then(|s| s.parse::<u64>().ok())
            {
                Some(scene) => entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.scene_id == scene)
                    .map(|(i, _)| i)
                    .collect(),
                None => (0..entries.len()).collect(),
            },
        };
        if chosen.is_empty() {
            anyhow::bail!("Choose at least one panel to print")
        }
        Ok(chosen)
    }

    /// The storyboard pages for the draft.
    pub(super) fn storyboard_layout(
        &self,
        sources: &[Source],
        settings: &Settings,
        cx: &App,
    ) -> anyhow::Result<JobLayout> {
        let story = self.storyboard.as_ref().context("Not a storyboard print")?;
        let profile = self.storyboard_profile(settings, cx)?;
        story::sheet::layout(&story.job, sources, &self.storyboard_selected()?, &profile)
    }

    fn save_profile(&mut self, remove: bool, cx: &mut Context<Self>) {
        let Some(story) = self.storyboard.as_ref() else {
            return;
        };
        let name = story.name.read(cx).value().trim().to_string();
        let result = if remove {
            let mut saved = saved_profiles(cx);
            let before = saved.len();
            saved.retain(|p| p.name != name);
            if saved.len() == before {
                Err(anyhow::anyhow!("No saved profile is named “{name}”"))
            } else {
                update_settings(cx, |s| s.storyboard_pdf_profiles = saved);
                Ok("Profile deleted.")
            }
        } else {
            self.storyboard_profile(&self.settings, cx)
                .and_then(|mut profile| {
                    profile.name = name.clone();
                    let mut saved = saved_profiles(cx);
                    profiles::save(&mut saved, profile.clone())?;
                    update_settings(cx, |s| {
                        s.storyboard_pdf_profiles = saved;
                        s.storyboard_pdf_profile = Some(name);
                    });
                    if let Some(story) = self.storyboard.as_mut() {
                        story.profile = profile;
                    }
                    Ok("Profile saved.")
                })
        };
        if let Some(story) = self.storyboard.as_mut() {
            story.notice = Some(match result {
                Ok(message) => message.into(),
                Err(e) => e.to_string(),
            });
        }
        cx.notify();
    }

    fn export_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let profile = match self.storyboard_profile(&self.settings, cx) {
            Ok(profile) => profile,
            Err(e) => {
                self.storyboard_notice(e.to_string(), cx);
                return;
            }
        };
        let request = cx.prompt_for_new_path(
            &std::env::current_dir().unwrap_or_default(),
            Some(&format!("{}.json", profile.name)),
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut path))) = request.await else {
                return;
            };
            path.set_extension("json");
            let result = cx
                .background_spawn(async move { profiles::export(&profile, &path).map(|_| path) })
                .await;
            this.update(cx, |s, cx| {
                s.storyboard_notice(
                    match result {
                        Ok(path) => format!("Profile saved to {}", path.display()),
                        Err(e) => e.to_string(),
                    },
                    cx,
                )
            })
            .ok();
        })
        .detach();
    }

    fn import_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = request.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move { profiles::import(&path) })
                .await;
            this.update_in(cx, |s, window, cx| match result {
                Ok(profile) => {
                    let mut saved = saved_profiles(cx);
                    match profiles::save(&mut saved, profile.clone()) {
                        Ok(()) => {
                            update_settings(cx, |s| s.storyboard_pdf_profiles = saved);
                            s.apply_profile(profile, window, cx);
                            s.storyboard_notice("Profile imported and saved.".into(), cx);
                        }
                        Err(e) => s.storyboard_notice(e.to_string(), cx),
                    }
                }
                Err(e) => s.storyboard_notice(format!("Cannot import the profile: {e}"), cx),
            })
            .ok();
        })
        .detach();
    }

    fn choose_logo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = request.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |s, cx| s.set_logo(Some(path), cx)).ok();
            }
        })
        .detach();
    }

    pub(super) fn set_logo(&mut self, path: Option<std::path::PathBuf>, cx: &mut Context<Self>) {
        if let Some(story) = self.storyboard.as_mut() {
            story.profile.logo = path;
            story.logo_error = None;
        }
        self.load_logo(cx);
        self.changed(cx);
    }

    fn storyboard_notice(&mut self, notice: String, cx: &mut Context<Self>) {
        if let Some(story) = self.storyboard.as_mut() {
            story.notice = Some(notice);
        }
        cx.notify();
    }

    /// What to print: all panels, the Board's selection or one scene.
    pub(super) fn storyboard_scope(&self, cx: &Context<Self>) -> AnyElement {
        let Some(story) = &self.storyboard else {
            return div().into_any_element();
        };
        let mut choices = vec![("all".to_string(), "All panels".to_string())];
        if !story.selection.is_empty() {
            choices.push((
                "selected".into(),
                format!("Selected panels ({})", story.selection.len()),
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for entry in &story.job.entries {
            if seen.insert(entry.scene_id) {
                choices.push((
                    format!("scene:{}", entry.scene_id),
                    format!("Scene {}", entry.scene),
                ));
            }
        }
        self.select(
            "storyboard-scope",
            "Panels",
            self.scope.clone(),
            choices,
            |s, v, cx| {
                s.scope = v;
                s.sheet = 0;
                s.changed(cx)
            },
            cx,
        )
    }

    /// A menu option. Menu keys are `id=value`, so one handler serves every
    /// option.
    fn storyboard_choice(&self, opt: &Opt, cx: &Context<Self>) -> AnyElement {
        let Kind::Choice(choices, get, _) = opt.kind else {
            return div().into_any_element();
        };
        let current = self
            .storyboard
            .as_ref()
            .map(|s| format!("{}={}", opt.id, get(&s.profile)))
            .unwrap_or_default();
        self.select(
            opt.id,
            opt.label,
            current,
            choices
                .iter()
                .map(|(k, v)| (format!("{}={k}", opt.id), v.to_string()))
                .collect(),
            |s, v, cx| s.storyboard_choose(&v, cx),
            cx,
        )
    }

    pub(super) fn storyboard_choose(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some((id, value)) = key.split_once('=') else {
            return;
        };
        let Some(story) = self.storyboard.as_mut() else {
            return;
        };
        if let Some(Kind::Choice(_, _, set)) =
            options().into_iter().find(|o| o.id == id).map(|o| o.kind)
        {
            set(&mut story.profile, value);
            self.changed(cx);
        }
    }

    /// The profile picker and every option, grouped and searchable.
    pub(super) fn storyboard_controls(&self, cx: &Context<Self>) -> AnyElement {
        let Some(story) = &self.storyboard else {
            return div().into_any_element();
        };
        let p = theme::palette(cx);
        let owner = cx.weak_entity();
        let profiles = all_profiles(cx);
        let builtin_count = builtins().len();
        let query = story.search.read(cx).value().to_string();
        let name = story.name.read(cx).value().trim().to_string();
        let saved = saved_profiles(cx).iter().any(|p| p.name == name);
        let mut section = div()
            .id("storyboard-pdf-options")
            .test_support()
            .flex()
            .flex_col()
            .gap_3()
            .border_t_1()
            .border_color(p.line)
            .pt_2()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Storyboard layout"),
            )
            .child(
                Button::new("storyboard-profile")
                    .label(format!("Profile · {}", story.profile.name))
                    .small()
                    .outline()
                    .dropdown_caret(true)
                    .disabled(self.busy)
                    .w_full()
                    .dropdown_menu(move |mut menu, _, _| {
                        for (i, profile) in profiles.iter().enumerate() {
                            if i == builtin_count {
                                menu = menu.separator();
                            }
                            let owner = owner.clone();
                            let profile = profile.clone();
                            menu = menu.item(PopupMenuItem::new(profile.name.clone()).on_click(
                                move |_, window, cx| {
                                    owner
                                        .update(cx, |s, cx| {
                                            if !s.busy {
                                                s.apply_profile(profile.clone(), window, cx)
                                            }
                                        })
                                        .ok();
                                },
                            ));
                        }
                        menu
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child("Profile name")
                    .child(Input::new(&story.name).small().disabled(self.busy)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("storyboard-save-profile")
                            .label("Save profile")
                            .small()
                            .outline()
                            .disabled(self.busy || name.is_empty())
                            .on_click(cx.listener(|s, _, _, cx| s.save_profile(false, cx))),
                    )
                    .child(
                        Button::new("storyboard-delete-profile")
                            .label("Delete")
                            .small()
                            .ghost()
                            .disabled(self.busy || !saved)
                            .on_click(cx.listener(|s, _, _, cx| s.save_profile(true, cx))),
                    )
                    .child(
                        Button::new("storyboard-export-profile")
                            .label("Share…")
                            .small()
                            .ghost()
                            .disabled(self.busy)
                            .on_click(cx.listener(|s, _, window, cx| s.export_profile(window, cx))),
                    )
                    .child(
                        Button::new("storyboard-import-profile")
                            .label("Import…")
                            .small()
                            .ghost()
                            .disabled(self.busy)
                            .on_click(cx.listener(|s, _, window, cx| s.import_profile(window, cx))),
                    ),
            )
            .when_some(story.notice.clone(), |d, n| {
                d.child(
                    div()
                        .id("storyboard-profile-notice")
                        .test_support()
                        .text_color(p.muted)
                        .child(n),
                )
            })
            .child(Input::new(&story.search).small());
        let all = options();
        let mut shown = 0;
        for group in GROUPS {
            let visible: Vec<_> = all
                .iter()
                .filter(|o| o.group == group && found(o.group, o.label, &query))
                .collect();
            let logo = group == "Header and footer" && found(group, "Logo image", &query);
            if visible.is_empty() && !logo {
                continue;
            }
            section = section.child(div().text_color(p.muted).child(group));
            for opt in visible {
                shown += 1;
                section = section.child(match opt.kind {
                    Kind::Choice(..) => self.storyboard_choice(opt, cx),
                    _ => div()
                        .id(opt.id)
                        .test_support()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(opt.label)
                        .child(
                            Input::new(&story.inputs[opt.id])
                                .small()
                                .disabled(self.busy),
                        )
                        .into_any_element(),
                });
            }
            if logo {
                shown += 1;
                let current = story
                    .profile
                    .logo
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map_or("No logo".into(), |n| n.to_string_lossy().into_owned());
                section = section.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().child(format!("Logo image · {current}")))
                        .child(
                            Button::new("storyboard-logo")
                                .label("Choose…")
                                .small()
                                .outline()
                                .disabled(self.busy)
                                .on_click(
                                    cx.listener(|s, _, window, cx| s.choose_logo(window, cx)),
                                ),
                        )
                        .child(
                            Button::new("storyboard-logo-clear")
                                .label("Remove")
                                .small()
                                .ghost()
                                .disabled(self.busy || story.profile.logo.is_none())
                                .on_click(cx.listener(|s, _, _, cx| s.set_logo(None, cx))),
                        ),
                );
                if let Some(error) = &story.logo_error {
                    section = section.child(div().text_color(rgb(0xc98535)).child(error.clone()));
                }
            }
        }
        if shown == 0 {
            section = section.child(
                div()
                    .text_color(p.muted)
                    .child("No options match your search."),
            );
        }
        section
            .child(div().text_color(p.muted).child(
                "Headers use tokens such as {scene}, {name}, {duration} and {shot}; the page header and footer use {project}, {page}, {pages} and {date}. Camera moves arrive with the timeline; their arrows will use the arrow thickness.",
            ))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt;

    struct Host;
    impl Render for Host {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .children(Root::render_dialog_layer(window, cx))
        }
    }

    /// Five panels; the last two start scene "2".
    fn project() -> Project {
        let mut editor = ProjectEditor::new_project(
            ProjectKind::Storyboard,
            emulsion_core::Document::new(64, 36),
        )
        .unwrap();
        let blank = editor.storyboard().unwrap().blank_panel().unwrap();
        let items = (2..=5)
            .map(|n| {
                (
                    format!("Panel {n}"),
                    emulsion_core::storyboard::Panel::new(0, 24),
                )
            })
            .collect();
        let ids = editor.insert_panels(Some(1), &blank, items, None).unwrap();
        editor
            .edit_storyboard(|b| {
                let layout: Vec<_> = std::iter::once(1).chain(ids.iter().copied()).collect();
                b.split(
                    &layout,
                    ids[2],
                    emulsion_core::storyboard::Level::Scene,
                    Some("2"),
                )?;
                let action = b.caption("Action").unwrap();
                b.panels
                    .get_mut(&1)
                    .unwrap()
                    .captions
                    .insert(action, "Mia runs".into());
                Ok(())
            })
            .unwrap();
        editor.snapshot().unwrap()
    }

    #[gpui_kit::test]
    fn storyboard_profiles_drive_the_shared_preview_options_and_search(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            cx.set_reduce_motion(true);
            cx.set_global(crate::app_state::AppSettings(Default::default()));
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            Root::new(host, window, cx)
        });
        cx.simulate_resize(size(px(1200.), px(1000.)));
        let project = project();
        let view = cx.update(|window, cx| {
            let job = Arc::new(
                Job::new(&project, "Film", &Scope::All, None, "2026-10-01".into()).unwrap(),
            );
            let sources = story::sheet::sources(&project, &job, &AtomicBool::new(false)).unwrap();
            let view = cx.new(|cx| {
                let mut dialog = PrintDialog::new("Film".into(), 0, window, cx);
                dialog.attach_storyboard(job, builtins().remove(1), vec![2, 3], window, cx);
                dialog
            });
            view.update(cx, |v, cx| {
                v.sources = Some(Arc::new(sources));
                v.loading = false;
                v.choose_destination("pdf".into(), cx);
            });
            let body = view.clone();
            window.open_dialog(cx, move |dialog, _, _| {
                dialog
                    .title("Export Storyboard PDF")
                    .width(px(1020.))
                    .child(body.clone())
            });
            view
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-pdf-options").visible());
            assert!(
                window.try_find("print-layout").is_none(),
                "storyboard replaces sheet layouts"
            );
            assert!(view.read(cx).preview.is_some());
            let (settings, job) = view.read(cx).draft(cx).unwrap();
            assert!(settings.landscape, "the profile's orientation");
            assert_eq!(job.sheets.len(), 1);
            assert_eq!(job.sheets[0].items.len(), 5);
            // Typed options change the layout; a bad number blocks it.
            let rows = view.read(cx).storyboard.as_ref().unwrap().inputs["sb-rows"].clone();
            rows.update(cx, |f, cx| f.set_value("x", window, cx));
            assert!(view.read(cx).draft(cx).is_err());
            rows.update(cx, |f, cx| f.set_value("1", window, cx));
            assert_eq!(view.read(cx).draft(cx).unwrap().1.sheets.len(), 2);
            view.update(cx, |v, cx| v.storyboard_choose("sb-captions=right", cx));
            assert_eq!(
                view.read(cx)
                    .storyboard_profile(&view.read(cx).settings, cx)
                    .unwrap()
                    .captions,
                CaptionPlacement::Right
            );
            // Scopes: the Board's selection and one scene.
            view.update(cx, |v, cx| {
                v.scope = "selected".into();
                v.changed(cx)
            });
            assert_eq!(view.read(cx).draft(cx).unwrap().1.sheets[0].items.len(), 2);
            let scene = view.read(cx).storyboard.as_ref().unwrap().job.entries[4].scene_id;
            view.update(cx, |v, cx| {
                v.scope = format!("scene:{scene}");
                v.changed(cx)
            });
            let pages = view.read(cx).draft(cx).unwrap().1;
            assert_eq!(pages.sheets.iter().map(|s| s.items.len()).sum::<usize>(), 2);
            // Option search hides what does not match.
            let search = view.read(cx).storyboard.as_ref().unwrap().search.clone();
            search.update(cx, |f, cx| f.set_value("camera", window, cx));
            window.render_frame(cx);
            assert!(window.find("sb-camera-mm").visible());
            assert!(window.try_find("sb-columns").is_none());
            search.update(cx, |f, cx| f.set_value("zzz", window, cx));
            window.render_frame(cx);
            assert!(window.try_find("sb-camera-mm").is_none());
            // Applying a profile fills every option and the paper.
            view.update(cx, |v, cx| {
                v.apply_profile(builtins().remove(0), window, cx)
            });
            assert_eq!(rows.read(cx).value().as_str(), "3");
            assert!(!view.read(cx).settings.landscape);
            // Saved profiles live in settings; built-in names are reserved.
            let name = view.read(cx).storyboard.as_ref().unwrap().name.clone();
            name.update(cx, |f, cx| f.set_value("Studio", window, cx));
            view.update(cx, |v, cx| v.save_profile(false, cx));
            let saved = &crate::app_state::settings(cx).storyboard_pdf_profiles;
            assert_eq!(saved.len(), 1);
            assert_eq!(saved[0].rows, 3);
            name.update(cx, |f, cx| f.set_value("1 per page · large", window, cx));
            view.update(cx, |v, cx| v.save_profile(false, cx));
            assert_eq!(
                crate::app_state::settings(cx).storyboard_pdf_profiles.len(),
                1
            );
            name.update(cx, |f, cx| f.set_value("Studio", window, cx));
            view.update(cx, |v, cx| v.save_profile(true, cx));
            assert!(
                crate::app_state::settings(cx)
                    .storyboard_pdf_profiles
                    .is_empty()
            );
            window.close_dialog(cx);
        });
    }
}
