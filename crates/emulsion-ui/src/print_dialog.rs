//! Shared print UI. Sources are immutable snapshots; capabilities, rendering and
//! submission run off the UI thread. Generations discard stale worker results.
use crate::file_prompt::FilePrompts;
use crate::theme;
mod creative;
mod production;
mod sources;
mod storyboard;
mod worksheets;
use emulsion_io::printing::{
    self as print, Capabilities, Choice, JobLayout, Layout, Placement, Printer, Settings, Source,
};
use gpui_kit::{
    component::{
        Disableable, Sizable, WindowExt,
        button::{Button, ButtonVariants},
        input::{Input, InputEvent, InputState},
        menu::{DropdownMenu, PopupMenuItem},
    },
    prelude::FluentBuilder,
    *,
};
pub(crate) use sources::open_prepared;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
pub(crate) use storyboard::{open_storyboard, saved_profiles as saved_storyboard_profiles};
pub(crate) use worksheets::open_worksheets;

/// Looks up the system's printers; a field so tests can stand in a slow or
/// failing print service.
type Discover = Arc<dyn Fn() -> anyhow::Result<Vec<Printer>> + Send + Sync>;
/// How long printer discovery may take before the dialog stops waiting for it.
/// CUPS can block indefinitely when its scheduler is unreachable.
const DISCOVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

pub fn open(
    name: String,
    docs: Vec<(String, emulsion_core::Document)>,
    active: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| PrintDialog::new(name, active, window, cx));
    view.update(cx, |this, cx| {
        this.load_presets(cx);
        this.load_sources(docs, cx);
        this.refresh(cx)
    });
    show(view, t!("print.print_dialog.title").into(), window, cx);
}
fn show(view: Entity<PrintDialog>, title: SharedString, window: &mut Window, cx: &mut App) {
    let cancel = view.read(cx).cancel.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let cancel = cancel.clone();
        crate::dialog_actions::with_actions(dialog, &view, window, cx)
            .title(title.clone())
            .width(px(1020.))
            .overlay_closable(false)
            .on_close(move |_, _, _| {
                cancel.store(true, Ordering::Relaxed);
            })
    });
}
struct PrintDialog {
    name: String,
    active: usize,
    sources: Option<Arc<Vec<Source>>>,
    source_error: Option<String>,
    printers: Vec<Printer>,
    destination: String,
    caps: Option<Capabilities>,
    settings: Settings,
    paper_chosen: bool,
    scope: String,
    fields: [Entity<InputState>; 18],
    original_sources: Option<Arc<Vec<Source>>>,
    original_active: usize,
    source_pending: bool,
    presets: Vec<print::presets::Preset>,
    preset_busy: bool,
    preset_notice: Option<String>,
    _subscriptions: Vec<Subscription>,
    generation: u64,
    discovery: u64,
    preview_generation: u64,
    preview: Option<Arc<RenderImage>>,
    preview_pending: bool,
    sheet: usize,
    notice: Option<String>,
    device_notice: Option<String>,
    busy: bool,
    /// Printer discovery is running. Only printer destinations wait for it.
    loading: bool,
    discover: Discover,
    cancel: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    portal: Option<Arc<print::portal::Prepared>>,
    /// Set when printing a storyboard with a PDF layout profile.
    storyboard: Option<storyboard::StoryboardState>,
    /// Set when printing paper worksheets.
    worksheet: Option<worksheets::WorksheetState>,
}
impl Drop for PrintDialog {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed)
    }
}
impl PrintDialog {
    fn new(name: String, active: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let fields = [
            "1",
            "100",
            "5",
            "5",
            "",
            "101.6",
            "152.4",
            "3",
            "2",
            "5",
            "50",
            "50",
            "0",
            "",
            "",
            "300",
            "Custom print condition",
            "0, 1000, 2000",
        ]
        .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let subscriptions = fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                cx.subscribe(field, move |this, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        if index == 13 || index == 17 {
                            cx.notify();
                        } else {
                            this.changed(cx);
                        }
                    }
                })
            })
            .collect();
        Self {
            name,
            active,
            sources: None,
            source_error: None,
            printers: vec![],
            destination: "initial".into(),
            caps: None,
            settings: Settings::default(),
            paper_chosen: false,
            scope: "current".into(),
            fields,
            original_sources: None,
            original_active: active,
            source_pending: false,
            presets: vec![],
            preset_busy: false,
            preset_notice: None,
            _subscriptions: subscriptions,
            generation: 0,
            discovery: 0,
            preview_generation: 0,
            preview: None,
            preview_pending: false,
            sheet: 0,
            notice: None,
            device_notice: None,
            busy: false,
            loading: false,
            discover: Arc::new(print::discover),
            cancel: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "linux")]
            portal: None,
            storyboard: None,
            worksheet: None,
        }
    }
    fn load_sources(
        &mut self,
        docs: Vec<(String, emulsion_core::Document)>,
        cx: &mut Context<Self>,
    ) {
        self.load_prepared(
            move |cancel| print::prepare_sources(docs, &cancel),
            true,
            cx,
        );
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "linux")]
        if print::portal::required() {
            self.destination = "portal".into();
            self.caps = Some(Capabilities::pdf());
            self.loading = false;
            self.changed(cx);
            return;
        }
        self.discovery += 1;
        let generation = self.discovery;
        self.loading = true;
        self.device_notice = None;
        cx.notify();
        // A dedicated thread, so a lookup that never returns holds neither the
        // dialog nor a shared executor worker. The dialog polls for the answer
        // on its own timer, and stops waiting after the timeout.
        let (sender, receiver) = std::sync::mpsc::channel();
        let discover = self.discover.clone();
        std::thread::Builder::new()
            .name("printer-discovery".into())
            .spawn(move || {
                sender.send(discover()).ok();
            })
            .ok();
        cx.spawn(async move |this, cx| {
            let started = cx.background_executor().now();
            loop {
                let waited = cx.background_executor().now() - started;
                let answer = match receiver.try_recv() {
                    Ok(result) => Some(Some(result)),
                    // The thread could not start or died without an answer.
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                };
                let answered = answer.is_some();
                let alive = this.update(cx, |this, cx| {
                    match answer {
                        Some(Some(result)) => this.discovered(generation, result, cx),
                        Some(None) => this.discovery_failed(generation, cx),
                        None if waited >= DISCOVERY_TIMEOUT => {
                            this.discovery_failed(generation, cx)
                        }
                        None => {}
                    }
                    generation == this.discovery
                });
                // Stop once answered, superseded by another search, or closed.
                if answered || !matches!(alive, Ok(true)) {
                    return;
                }
                // Poll quickly at first, then keep listening for a late answer.
                let step = if waited < DISCOVERY_TIMEOUT {
                    std::time::Duration::from_millis(50)
                } else {
                    std::time::Duration::from_secs(1)
                };
                cx.background_executor().timer(step).await;
            }
        })
        .detach();
    }
    /// Discovery timed out or failed without an answer: stop searching, say so,
    /// and keep Save PDF usable.
    fn discovery_failed(&mut self, generation: u64, cx: &mut Context<Self>) {
        if generation != self.discovery || !self.loading {
            return;
        }
        self.loading = false;
        self.device_notice = Some(t!("print.print_dialog.unreachable").into());
        if self.destination == "initial" {
            self.choose_destination("pdf".into(), cx)
        } else {
            cx.notify();
        }
    }
    /// Applies a discovery result. One that arrives after the timeout still
    /// fills in the printer list.
    fn discovered(
        &mut self,
        generation: u64,
        result: anyhow::Result<Vec<Printer>>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.discovery {
            return;
        }
        self.loading = false;
        match result {
            Ok(printers) => {
                self.printers = printers;
                self.device_notice = None;
                if self.destination == "initial" {
                    let dest = self
                        .printers
                        .iter()
                        .find(|p| p.default)
                        .or(self.printers.first())
                        .map(|p| p.id.clone())
                        .unwrap_or_else(|| "pdf".into());
                    self.choose_destination(dest, cx)
                } else if self.destination != "pdf"
                    && self.destination != "portal"
                    && !self.printers.iter().any(|p| p.id == self.destination)
                {
                    self.caps = None;
                    self.device_notice = Some(t!("print.print_dialog.printer_gone").into());
                    self.changed(cx)
                } else if self.destination != "pdf" && self.destination != "portal" {
                    self.choose_destination(self.destination.clone(), cx)
                } else {
                    cx.notify();
                }
            }
            Err(e) => {
                self.device_notice = Some(e.to_string());
                if self.destination == "initial" {
                    self.choose_destination("pdf".into(), cx)
                } else {
                    cx.notify();
                }
            }
        }
    }
    /// Printer destinations can't submit while discovery runs; Save PDF and
    /// the system dialog never need a printer list.
    fn awaiting_printers(&self) -> bool {
        self.loading && self.destination != "pdf" && self.destination != "portal"
    }
    fn choose_destination(&mut self, id: String, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.destination = id.clone();
        self.settings.production.driver_color_disabled = false;
        if id != "pdf" {
            self.settings.production.managed = self.settings.production.enabled();
            self.settings.production.standard = print::production::PdfStandard::Pdf;
        }
        self.paper_chosen = false;
        if id == "pdf" && self.settings.layout == Layout::Single {
            self.settings.layout = Layout::Document;
        }
        if id != "pdf" && self.settings.layout == Layout::Document {
            self.settings.layout = Layout::Single;
        }
        self.caps = None;
        self.preview = None;
        self.settings.media = None;
        self.settings.tray = None;
        self.settings.quality = None;
        self.settings.sides = None;
        #[cfg(target_os = "linux")]
        {
            self.portal = None;
        }
        if id == "pdf" || id == "portal" {
            self.caps = Some(Capabilities::pdf());
            self.settings.paper = print::Paper::pdf().remove(0);
            self.suggest_paper();
            self.changed(cx);
            return;
        }
        self.device_notice = Some(t!("print.print_dialog.reading_caps").into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { print::capabilities(&id) })
                .await;
            this.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                match result {
                    Ok(caps) => {
                        if let Some(paper) = caps
                            .papers
                            .iter()
                            .find(|p| p.id == caps.default_paper)
                            .or(caps.papers.first())
                        {
                            this.settings.paper = paper.clone()
                        }
                        this.settings.sides = caps
                            .sides
                            .iter()
                            .find(|c| c.id == "one-sided")
                            .map(|c| c.id.clone());
                        if !caps.color {
                            this.settings.grayscale = true;
                        }
                        this.caps = Some(caps);
                        this.suggest_paper();
                        this.device_notice = None;
                    }
                    Err(e) => this.device_notice = Some(e.to_string()),
                }
                this.changed(cx);
            })
            .ok();
        })
        .detach();
    }
    fn suggest_paper(&mut self) {
        if self.paper_chosen {
            return;
        }
        if self.storyboard.is_some() || self.worksheet.is_some() {
            self.storyboard_paper();
            return;
        }
        let Some(source) = self.sources.as_ref().and_then(|s| s.get(self.active)) else {
            return;
        };
        let Some(caps) = &self.caps else {
            return;
        };
        if let Some((paper, landscape)) = print::matching_paper(source, &caps.papers) {
            self.settings.paper = paper;
            self.settings.landscape = landscape;
        } else {
            self.settings.landscape = source.width > source.height;
        }
    }
    fn draft(&self, cx: &App) -> anyhow::Result<(Settings, JobLayout)> {
        use anyhow::{Context, bail};
        if let Some(error) = &self.source_error {
            bail!("{error}")
        }
        if self.source_pending {
            bail!("{}", t!("print.print_dialog.preparing_sources"))
        }
        if self.caps.is_none() {
            bail!("{}", t!("print.print_dialog.select_printer"))
        }
        let sources = self
            .sources
            .as_ref()
            .context(t!("print.print_dialog.preparing_artwork"))?;
        let mut settings = self.settings.clone();
        let document = settings.layout == Layout::Document;
        if document && self.destination != "pdf" {
            bail!("{}", t!("print.print_dialog.document_pdf_only"))
        }
        settings.copies = if self.destination == "pdf" || self.destination == "portal" {
            1
        } else {
            self.fields[0]
                .read(cx)
                .value()
                .parse()
                .context(t!("print.print_dialog.copies_invalid"))?
        };
        settings.scale = if !document
            && (settings.placement == Placement::Actual || settings.layout == Layout::Poster)
            && !(settings.layout == Layout::Poster && settings.creative.artwork_mm.is_some())
        {
            self.fields[1]
                .read(cx)
                .value()
                .parse()
                .context(t!("print.print_dialog.scale_invalid"))?
        } else {
            100.
        };
        settings.extra_margin = if document {
            0.
        } else {
            self.fields[2]
                .read(cx)
                .value()
                .parse()
                .context(t!("print.print_dialog.margin_invalid"))?
        };
        settings.overlap = if settings.layout == Layout::Poster {
            self.fields[3]
                .read(cx)
                .value()
                .parse()
                .context(t!("print.print_dialog.overlap_invalid"))?
        } else {
            5.
        };
        #[cfg(target_os = "linux")]
        if self.destination == "portal"
            && let Some(p) = &self.portal
        {
            settings.paper = p.paper.clone();
            settings.landscape = p.landscape;
            settings.copies = p.copies;
            settings.grayscale |= p.grayscale;
        }
        if self.destination == "pdf" {
            settings.copies = 1;
        }
        if self.worksheet.is_some() {
            self.production_draft(&mut settings, cx)?;
            let layout = self.worksheet_sheets(&settings)?.layout;
            return Ok((settings, layout));
        }
        if self.storyboard.is_some() {
            self.production_draft(&mut settings, cx)?;
            let layout = self.storyboard_layout(sources, &settings, cx)?;
            return Ok((settings, layout));
        }
        if self.scope == "range" && self.fields[4].read(cx).value().trim().is_empty() {
            bail!("{}", t!("print.print_dialog.range_missing"))
        }
        let selected = match self.scope.as_str() {
            "all" => (0..sources.len()).collect(),
            "range" => print::page_range(&self.fields[4].read(cx).value(), sources.len())?,
            _ => vec![self.active],
        };
        self.creative_draft(&mut settings, cx)?;
        self.production_draft(&mut settings, cx)?;
        let layout = print::layout(sources, &selected, &settings)?;
        Ok((settings, layout))
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        self.preview_generation += 1;
        let generation = self.preview_generation;
        self.preview = None;
        self.preview_pending = false;
        let Ok((settings, layout)) = self.draft(cx) else {
            cx.notify();
            return;
        };
        self.sheet = self.sheet.min(layout.sheets.len().saturating_sub(1));
        let sheet = layout.sheets[self.sheet].clone();
        let sources = self.sources.as_ref().unwrap().clone();
        self.preview_pending = true;
        let device = self.destination != "pdf";
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    if device {
                        print::production::validate_device_profile(&settings)?;
                    }
                    print::production::preview(&sources, &sheet, &settings, 900)
                })
                .await;
            this.update(cx, |this, cx| {
                if generation != this.preview_generation {
                    return;
                }
                this.preview_pending = false;
                match result {
                    Ok(image) => {
                        let (w, h) = image.dimensions();
                        let mut bytes = image.into_raw();
                        for p in bytes.as_chunks_mut::<4>().0 {
                            p.swap(0, 2)
                        }
                        this.preview = Some(Arc::new(crate::viewport::bgra_image(w, h, bytes)))
                    }
                    Err(e) => {
                        this.notice =
                            Some(t!("print.print_dialog.preview_failed", error = e).into_owned())
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    fn select(
        &self,
        id: &'static str,
        label: &str,
        current: String,
        choices: Vec<(String, String)>,
        apply: fn(&mut Self, String, &mut Context<Self>),
        cx: &Context<Self>,
    ) -> AnyElement {
        let owner = cx.weak_entity();
        let selected = current.clone();
        let display = choices
            .iter()
            .find(|(key, _)| *key == current)
            .map(|(_, name)| name.clone())
            .unwrap_or(current);
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_color(theme::palette(cx).muted)
                    .child(label.to_owned()),
            )
            .child(
                Button::new(id)
                    .label(display)
                    .small()
                    .outline()
                    .dropdown_caret(true)
                    .disabled(self.busy || choices.is_empty())
                    .w_full()
                    .dropdown_menu(move |mut menu, _, _| {
                        for (key, name) in &choices {
                            let owner = owner.clone();
                            let key = key.clone();
                            let chosen = key == selected;
                            menu = menu.item(
                                PopupMenuItem::new(name.clone()).checked(chosen).on_click(
                                    move |_, _, cx| {
                                        owner
                                            .update(cx, |this, cx| {
                                                if !this.busy {
                                                    apply(this, key.clone(), cx)
                                                }
                                            })
                                            .ok();
                                    },
                                ),
                            );
                        }
                        menu
                    }),
            )
            .into_any_element()
    }
    fn option(
        &self,
        id: &'static str,
        label: &str,
        value: &Option<String>,
        choices: &[Choice],
        apply: fn(&mut Self, String, &mut Context<Self>),
        cx: &Context<Self>,
    ) -> AnyElement {
        let options = std::iter::once((
            String::new(),
            t!("print.print_dialog.printer_default").into(),
        ))
        .chain(choices.iter().map(|v| (v.id.clone(), v.name.clone())))
        .collect();
        self.select(
            id,
            label,
            value.clone().unwrap_or_default(),
            options,
            apply,
            cx,
        )
    }
    fn field(&self, index: usize, label: &str) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(label.to_owned())
            .child(Input::new(&self.fields[index]).small().disabled(self.busy))
            .into_any_element()
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.preview_pending || self.preview.is_none() {
            return;
        }
        let Ok((settings, layout)) = self.draft(cx) else {
            return;
        };
        #[cfg(target_os = "linux")]
        if self.destination == "portal" && self.portal.is_none() {
            self.prepare_portal(settings, cx);
            return;
        }
        let sources = self.sources.as_ref().unwrap().clone();
        let title = self.name.clone();
        let destination = self.destination.clone();
        let cancel = self.cancel.clone();
        #[cfg(target_os = "linux")]
        let portal = self.portal.clone();
        self.busy = true;
        self.notice = Some(
            if destination == "pdf" {
                t!("print.print_dialog.choose_pdf_path")
            } else {
                t!("print.print_dialog.preparing_job")
            }
            .into(),
        );
        cx.notify();
        let suggested = if self.worksheet.is_some() {
            format!("{} worksheets.pdf", self.name)
        } else if self.storyboard.is_some() {
            format!("{}.pdf", self.name)
        } else {
            "Print.pdf".into()
        };
        let path_request = (destination == "pdf").then(|| {
            cx.prompt_save_path(
                &std::env::current_dir().unwrap_or_default(),
                Some(&suggested),
            )
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = if let Some(request) = path_request {
                match request.await {
                    Ok(Ok(Some(mut path))) => {
                        path.set_extension("pdf");
                        Some(path)
                    }
                    _ => {
                        this.update(cx, |this, cx| {
                            this.busy = false;
                            this.notice = None;
                            cx.notify();
                        })
                        .ok();
                        return;
                    }
                }
            } else {
                None
            };
            let result = cx
                .background_spawn(async move {
                    print::canceled(&cancel)?;
                    if let Some(path) = path {
                        print::production::write_pdf(&sources, &layout, &settings, &path, &cancel)?;
                        return Ok(
                            t!("print.print_dialog.saved", path = path.display()).into_owned()
                        );
                    }
                    #[cfg(target_os = "linux")]
                    if let Some(portal) = portal {
                        let dir = tempfile::tempdir()?;
                        let path = dir.path().join("print.pdf");
                        print::production::write_pdf(&sources, &layout, &settings, &path, &cancel)?;
                        print::canceled(&cancel)?;
                        return print::portal::submit(&portal, &title, &path);
                    }
                    print::submit(&destination, &title, &sources, &layout, &settings, &cancel)
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                this.notice = Some(match result {
                    Ok(message) => message,
                    Err(e) => t!("print.print_dialog.print_failed", error = e).into_owned(),
                });
                #[cfg(target_os = "linux")]
                {
                    this.portal = None;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    #[cfg(target_os = "linux")]
    fn prepare_portal(&mut self, settings: Settings, cx: &mut Context<Self>) {
        self.busy = true;
        self.notice = Some(t!("print.print_dialog.portal_choose").into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { print::portal::prepare(&settings) })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(prepared) => {
                        let summary = prepared.summary.clone();
                        this.portal = Some(Arc::new(prepared));
                        this.changed(cx);
                        this.notice = Some(summary);
                    }
                    Err(e) => {
                        this.notice = Some(e.to_string());
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }
}
impl Render for PrintDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let narrow = window.viewport_size().width < px(820.);
        let draft = self.draft(cx);
        let portal = self.destination == "portal";
        let mut destinations = vec![("pdf".into(), t!("print.print_dialog.save_pdf").into())];
        #[cfg(target_os = "linux")]
        destinations.push((
            "portal".into(),
            t!("print.print_dialog.system_dialog").into(),
        ));
        destinations.extend(self.printers.iter().map(|v| {
            (
                v.id.clone(),
                if v.default {
                    t!(
                        "print.print_dialog.printer_default_named",
                        name = v.name,
                        status = v.status
                    )
                    .into_owned()
                } else {
                    format!("{} · {}", v.name, v.status)
                },
            )
        }));
        let caps = self.caps.clone().unwrap_or_else(Capabilities::pdf);
        let mut controls = div()
            .id("print-controls")
            .w(px(310.))
            .when(narrow, |d| d.w_full())
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .pr_2()
            .child(self.select(
                "print-destination",
                &t!("print.print_dialog.destination"),
                self.destination.clone(),
                destinations,
                |s, v, cx| s.choose_destination(v, cx),
                cx,
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("print-refresh")
                            .label(if self.loading {
                                t!("print.print_dialog.searching")
                            } else {
                                t!("print.print_dialog.refresh")
                            })
                            .small()
                            .ghost()
                            .disabled(self.busy || self.loading)
                            .on_click(cx.listener(|s, _, _, cx| s.refresh(cx))),
                    )
                    .child(
                        Button::new("print-system-setup")
                            .label(t!("print.print_dialog.printer_setup"))
                            .small()
                            .ghost()
                            .on_click(|_, _, cx| {
                                #[cfg(target_os = "linux")]
                                cx.open_url("http://localhost:631/printers/");
                                #[cfg(target_os = "windows")]
                                cx.open_url("ms-settings:printers");
                                #[cfg(target_os = "macos")]
                                cx.open_url(
                                    "x-apple.systempreferences:com.apple.preference.printfax",
                                );
                            }),
                    ),
            )
            .when_some(self.device_notice.clone(), |d, n| {
                d.child(div().text_color(p.muted).child(n))
            })
            .when(
                self.discovery > 0 && self.printers.is_empty() && !self.loading,
                |d| {
                    d.child(
                        div()
                            .text_color(p.muted)
                            .child(t!("print.print_dialog.no_printers")),
                    )
                },
            )
            .child(if self.storyboard.is_some() {
                self.storyboard_scope(cx)
            } else if self.worksheet.is_some() {
                self.worksheet_note(cx)
            } else {
                self.select(
                    "print-content",
                    &t!("print.print_dialog.content"),
                    self.scope.clone(),
                    vec![
                        (
                            "current".into(),
                            t!("print.print_dialog.scope_current").into(),
                        ),
                        ("all".into(), t!("print.print_dialog.scope_all").into()),
                        ("range".into(), t!("print.print_dialog.scope_range").into()),
                    ],
                    |s, v, cx| {
                        s.scope = v;
                        s.sheet = 0;
                        s.changed(cx)
                    },
                    cx,
                )
            })
            .when(self.scope == "range", |d| {
                d.child(self.field(4, &t!("print.print_dialog.pages")))
            });
        if !portal && self.settings.layout != Layout::Document {
            controls = controls
                .child(
                    self.select(
                        "print-paper",
                        &t!("print.print_dialog.paper"),
                        self.settings.paper.id.clone(),
                        caps.papers
                            .iter()
                            .map(|p| (p.id.clone(), p.name.clone()))
                            .collect(),
                        |s, v, cx| {
                            if let Some(p) = s
                                .caps
                                .as_ref()
                                .and_then(|c| c.papers.iter().find(|p| p.id == v))
                            {
                                s.settings.paper = p.clone();
                                s.paper_chosen = true;
                            }
                            s.changed(cx)
                        },
                        cx,
                    ),
                )
                .child(self.select(
                    "print-orientation",
                    &t!("print.print_dialog.orientation"),
                    self.settings.landscape.to_string(),
                    vec![
                        ("false".into(), t!("print.print_dialog.portrait").into()),
                        ("true".into(), t!("print.print_dialog.landscape").into()),
                    ],
                    |s, v, cx| {
                        s.settings.landscape = v == "true";
                        s.paper_chosen = true;
                        s.changed(cx)
                    },
                    cx,
                ));
        }
        if self.storyboard.is_some() {
            controls = controls
                .child(self.storyboard_controls(cx))
                .child(self.production_controls(cx));
        } else if self.worksheet.is_some() {
            controls = controls.child(self.production_controls(cx));
        } else {
            let mut layouts = vec![
                (
                    "Single".into(),
                    t!("print.print_dialog.layout_single").into(),
                ),
                (
                    "Contact".into(),
                    t!("print.print_dialog.layout_contact").into(),
                ),
                (
                    "Repeat".into(),
                    t!("print.print_dialog.layout_repeat").into(),
                ),
                (
                    "Poster".into(),
                    t!("print.print_dialog.layout_poster").into(),
                ),
            ];
            if self.destination == "pdf" {
                layouts.insert(
                    0,
                    (
                        "Document".into(),
                        t!("print.print_dialog.layout_document").into(),
                    ),
                );
            }
            controls = controls.child(self.select(
                "print-layout",
                &t!("print.print_dialog.layout"),
                format!("{:?}", self.settings.layout),
                layouts,
                |s, v, cx| {
                    s.settings.layout = match v.as_str() {
                        "Document" => Layout::Document,
                        "Contact" => Layout::Contact,
                        "Repeat" => Layout::Repeat,
                        "Poster" => Layout::Poster,
                        _ => Layout::Single,
                    };
                    s.sheet = 0;
                    s.changed(cx)
                },
                cx,
            ));
            if !matches!(self.settings.layout, Layout::Poster | Layout::Document) {
                controls = controls.child(self.select(
                    "print-placement",
                    &t!("print.print_dialog.placement"),
                    format!("{:?}", self.settings.placement),
                    vec![
                        ("Fit".into(), t!("print.print_dialog.placement_fit").into()),
                        (
                            "Fill".into(),
                            t!("print.print_dialog.placement_fill").into(),
                        ),
                        (
                            "Actual".into(),
                            t!("print.print_dialog.placement_actual").into(),
                        ),
                    ],
                    |s, v, cx| {
                        s.settings.placement = match v.as_str() {
                            "Fill" => Placement::Fill,
                            "Actual" => Placement::Actual,
                            _ => Placement::Fit,
                        };
                        s.changed(cx)
                    },
                    cx,
                ));
            }
            if self.settings.layout != Layout::Document
                && !(self.settings.layout == Layout::Poster
                    && self.settings.creative.artwork_mm.is_some())
                && (self.settings.placement == Placement::Actual
                    || self.settings.layout == Layout::Poster)
            {
                controls = controls.child(self.field(1, &t!("print.print_dialog.scale")));
            }
            if self.settings.layout != Layout::Document {
                controls = controls.child(self.field(2, &t!("print.print_dialog.extra_margin")));
            }
            if self.settings.layout == Layout::Poster {
                controls = controls.child(self.field(3, &t!("print.print_dialog.tile_overlap")));
            }
            controls = controls
                .child(self.source_controls(cx))
                .child(self.production_controls(cx))
                .child(self.creative_controls(cx))
                .child(self.preset_controls(cx));
        }
        if !portal && self.destination != "pdf" {
            controls = controls.child(self.field(0, &t!("print.print_dialog.copies")));
        }
        controls = controls.child(self.select(
            "print-color",
            &t!("print.print_dialog.output"),
            self.settings.grayscale.to_string(),
            if caps.color {
                vec![
                    ("false".into(), t!("print.print_dialog.color").into()),
                    ("true".into(), t!("print.print_dialog.grayscale").into()),
                ]
            } else {
                vec![("true".into(), t!("print.print_dialog.grayscale").into())]
            },
            |s, v, cx| {
                s.settings.grayscale = v == "true";
                s.changed(cx)
            },
            cx,
        ));
        if !portal && self.destination != "pdf" {
            if !caps.media.is_empty() {
                controls = controls.child(self.option(
                    "print-media",
                    &t!("print.print_dialog.paper_type"),
                    &self.settings.media,
                    &caps.media,
                    |s, v, cx| {
                        s.settings.media = (!v.is_empty()).then_some(v);
                        s.changed(cx)
                    },
                    cx,
                ));
            }
            if !caps.trays.is_empty() {
                controls = controls.child(self.option(
                    "print-tray",
                    &t!("print.print_dialog.paper_source"),
                    &self.settings.tray,
                    &caps.trays,
                    |s, v, cx| {
                        s.settings.tray = (!v.is_empty()).then_some(v);
                        s.changed(cx)
                    },
                    cx,
                ));
            }
            if !caps.quality.is_empty() {
                controls = controls.child(self.option(
                    "print-quality",
                    &t!("print.print_dialog.print_quality"),
                    &self.settings.quality,
                    &caps.quality,
                    |s, v, cx| {
                        s.settings.quality = (!v.is_empty()).then_some(v);
                        s.changed(cx)
                    },
                    cx,
                ));
            }
            if !caps.sides.is_empty() {
                controls = controls.child(self.option(
                    "print-sides",
                    &t!("print.print_dialog.sides"),
                    &self.settings.sides,
                    &caps.sides,
                    |s, v, cx| {
                        s.settings.sides = (!v.is_empty()).then_some(v);
                        s.changed(cx)
                    },
                    cx,
                ));
            }
        }
        let size = draft.as_ref().ok().map(|(_, l)| {
            let s = &l.sheets[self.sheet.min(l.sheets.len() - 1)];
            (s.width, s.height, l.sheets.len())
        });
        let (pw, ph) = size
            .map(|(w, h, _)| {
                let max_width = if narrow {
                    (f32::from(window.viewport_size().width) - 100.).max(120.) as f64
                } else {
                    420.
                };
                let k = (max_width / w).min(440. / h);
                (w * k, h * k)
            })
            .unwrap_or((300., 420.));
        let image = self.preview.clone();
        let artwork = draft.as_ref().ok().and_then(|(_, layout)| {
            let sheet = layout.sheets.get(self.sheet)?;
            let item = sheet.items.first()?;
            let source = self.sources.as_ref()?.get(item.source)?;
            let (w, h) = source.physical_size().ok()?;
            Some(
                t!(
                    "print.print_dialog.artwork_size",
                    name = source.name,
                    width = format!("{w:.1}"),
                    height = format!("{h:.1}"),
                    ppi = format!("{:.0}", source.ppi),
                    paper_width = format!("{:.1}", item.bounds.w),
                    paper_height = format!("{:.1}", item.bounds.h),
                    percent = format!("{:.1}", item.bounds.w / w * 100.)
                )
                .into_owned(),
            )
        });
        let warnings = draft
            .as_ref()
            .ok()
            .map(|(_, l)| l.warnings.join("\n"))
            .unwrap_or_default();
        div()
            .id("print-dialog")
            .test_support()
            .flex()
            .flex_col()
            .gap_3()
            .text_size(px(12.))
            .text_color(p.ink)
            .child(
                div()
                    .text_color(p.muted)
                    .child(t!("print.print_dialog.subtitle", name = self.name)),
            )
            .child(
                div().id("print-scroll").child(
                    div()
                        .flex()
                        .gap_5()
                        .when(narrow, |d| d.flex_col())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .w_full()
                                        .min_h(px(460.))
                                        .bg(p.stage)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .w(px(pw as f32))
                                                .h(px(ph as f32))
                                                .bg(rgb(0xffffff))
                                                .when_some(image, |d, image| {
                                                    d.child(
                                                        img(ImageSource::Render(image))
                                                            .size_full()
                                                            .object_fit(ObjectFit::Contain),
                                                    )
                                                }),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap_3()
                                        .items_center()
                                        .child(
                                            Button::new("print-previous-sheet")
                                                .label(t!("print.print_dialog.previous"))
                                                .small()
                                                .disabled(self.busy || self.sheet == 0)
                                                .on_click(cx.listener(|s, _, _, cx| {
                                                    s.sheet = s.sheet.saturating_sub(1);
                                                    s.changed(cx)
                                                })),
                                        )
                                        .child(t!(
                                            "print.print_dialog.sheet_of",
                                            current = self.sheet + 1,
                                            total = size.map(|s| s.2).unwrap_or(1)
                                        ))
                                        .child(
                                            Button::new("print-next-sheet")
                                                .label(t!("print.print_dialog.next"))
                                                .small()
                                                .disabled(
                                                    self.busy
                                                        || self.sheet + 1
                                                            >= size.map(|s| s.2).unwrap_or(1),
                                                )
                                                .on_click(cx.listener(|s, _, _, cx| {
                                                    s.sheet += 1;
                                                    s.changed(cx)
                                                })),
                                        ),
                                )
                                .when(self.preview_pending, |d| {
                                    d.child(t!("print.print_dialog.updating_preview"))
                                })
                                .when_some(artwork, |d, text| {
                                    d.child(
                                        div()
                                            .id("print-artwork-size")
                                            .test_support()
                                            .text_color(p.muted)
                                            .child(text),
                                    )
                                })
                                .child(
                                    div()
                                        .text_color(p.muted)
                                        .child(t!("print.print_dialog.paper_note")),
                                )
                                .when(!warnings.is_empty(), |d| {
                                    d.child(div().text_color(rgb(0xc98535)).child(warnings))
                                })
                                .child(
                                    div()
                                        .text_color(p.muted)
                                        .child(t!("print.print_dialog.video_note")),
                                ),
                        )
                        .child(controls),
                ),
            )
    }
}

impl crate::dialog_actions::DialogActions for PrintDialog {
    /// Notices, the job summary and Close / Save PDF / Print, pinned in the
    /// dialog footer so they stay reachable while the preview and controls
    /// scroll.
    fn render_actions(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let draft = self.draft(cx);
        let summary = draft
            .as_ref()
            .map(|(s, l)| {
                t!(
                    "print.print_dialog.summary",
                    sides = l.sheets.len(),
                    copies = s.copies,
                    width = format!("{:.1}", l.sheets[self.sheet.min(l.sheets.len() - 1)].width),
                    height = format!("{:.1}", l.sheets[self.sheet.min(l.sheets.len() - 1)].height)
                )
                .into_owned()
            })
            .unwrap_or_else(|e| e.to_string());
        let notice = self.source_error.clone().or(self.notice.clone());
        let button_label = if self.destination == "pdf" {
            t!("print.print_dialog.save_pdf")
        } else {
            t!("print.print_dialog.print")
        };
        #[cfg(target_os = "linux")]
        let button_label = if self.destination == "portal" && self.portal.is_none() {
            t!("print.print_dialog.choose_printer")
        } else {
            button_label
        };
        let disabled = self.busy
            || self.awaiting_printers()
            || self.preview_pending
            || self.preview.is_none()
            || draft.is_err()
            || self.source_error.is_some();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .text_size(px(12.))
            .text_color(p.ink)
            .when_some(notice, |d, n| {
                d.child(div().id("print-notice").test_support().child(n))
            })
            .child(
                div()
                    .border_t_1()
                    .border_color(p.line)
                    .pt_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().flex_1().child(summary))
                    .child(
                        Button::new("print-cancel")
                            .label(if self.busy {
                                t!("print.print_dialog.cancel_close")
                            } else {
                                t!("print.print_dialog.close")
                            })
                            .on_click(cx.listener(|s, _, window, cx| {
                                s.cancel.store(true, Ordering::Relaxed);
                                window.close_dialog(cx)
                            })),
                    )
                    .child(
                        Button::new("print-submit")
                            .label(button_label)
                            .primary()
                            .disabled(disabled)
                            .on_click(cx.listener(|s, _, window, cx| s.submit(window, cx))),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
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
    fn red_square() -> Source {
        Source {
            name: "Page 1".into(),
            width: 100,
            height: 100,
            ppi: 100.,
            rasterized: false,
            document: None,
            original_paths: vec![],
            svg: r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="red"/></svg>"#.into(),
        }
    }
    /// Opens the real print dialog at `viewport` with a discovery that blocks
    /// until the test sends its answer.
    fn open_with_slow_discovery(
        cx: &mut TestAppContext,
        viewport: Size<Pixels>,
    ) -> (
        Entity<PrintDialog>,
        std::sync::mpsc::Sender<anyhow::Result<Vec<Printer>>>,
        &mut VisualTestContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            Root::new(host, window, cx)
        });
        cx.simulate_resize(viewport);
        let (answer, answers) = std::sync::mpsc::channel();
        let answers = Arc::new(parking_lot::Mutex::new(answers));
        let view = cx.update(|window, cx| {
            let view = cx.new(|cx| PrintDialog::new("Proof".into(), 0, window, cx));
            view.update(cx, |v, cx| {
                v.discover = Arc::new(move || answers.lock().recv().unwrap_or_else(|_| Ok(vec![])));
                v.sources = Some(Arc::new(vec![red_square()]));
                v.refresh(cx);
                v.choose_destination("pdf".into(), cx);
            });
            show(view.clone(), "Print".into(), window, cx);
            view
        });
        cx.run_until_parked();
        (view, answer, cx)
    }
    /// Save PDF needs no printer: it must not wait for a print service that
    /// never answers, and the footer stays inside a 1280×720 window.
    #[gpui_kit::test]
    fn pdf_export_works_while_printer_discovery_hangs(cx: &mut TestAppContext) {
        let viewport = size(px(1280.), px(720.));
        let (view, _answer, cx) = open_with_slow_discovery(cx, viewport);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("proof.pdf");
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(view.read(cx).loading, "discovery is still running");
            assert!(!view.read(cx).awaiting_printers());
            let submit = window.find("print-submit");
            assert!(submit.visible());
            assert!(
                submit.bounds().bottom() <= viewport.height,
                "Save PDF is below the window: {:?}",
                submit.bounds()
            );
            window.click("print-submit", cx);
        });
        cx.run_until_parked();
        assert!(cx.did_prompt_for_new_path());
        let chosen = path.clone();
        cx.simulate_new_path_selection(move |_| Some(chosen));
        cx.run_until_parked();
        assert!(path.exists(), "the PDF was not written");
        cx.update(|window, cx| {
            assert!(!view.read(cx).busy);
            window.close_dialog(cx);
        });
    }
    /// Printer destinations wait for discovery, which gives up after the
    /// timeout with a notice; a late answer still fills in the printers.
    #[gpui_kit::test]
    fn printer_destinations_wait_for_discovery_until_it_times_out(cx: &mut TestAppContext) {
        let (view, answer, cx) = open_with_slow_discovery(cx, size(px(1366.), px(768.)));
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                // A printer whose capabilities are already known.
                v.destination = "test-printer".into();
                v.caps = Some(Capabilities::pdf());
                v.changed(cx);
            });
            window.render_frame(cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(view.read(cx).awaiting_printers());
            // The disabled Print button does nothing.
            window.click("print-submit", cx);
            assert!(!view.read(cx).busy);
        });
        cx.executor()
            .advance_clock(DISCOVERY_TIMEOUT + std::time::Duration::from_secs(1));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            let v = view.read(cx);
            assert!(!v.loading, "still searching after the timeout");
            assert_eq!(
                v.device_notice.as_deref(),
                Some(&*t!("print.print_dialog.unreachable"))
            );
            assert!(!v.awaiting_printers());
            assert!(window.find("print-refresh").visible());
        });
        cx.update(|_, cx| view.update(cx, |v, cx| v.choose_destination("pdf".into(), cx)));
        answer
            .send(Ok(vec![Printer {
                id: "late".into(),
                name: "Late".into(),
                default: false,
                status: "idle".into(),
            }]))
            .unwrap();
        for _ in 0..500 {
            // The discovery thread answers in real time; the dialog polls on
            // the (simulated) clock.
            std::thread::sleep(std::time::Duration::from_millis(10));
            cx.executor()
                .advance_clock(std::time::Duration::from_secs(1));
            cx.run_until_parked();
            if !view.read_with(cx, |v, _| v.printers.is_empty()) {
                break;
            }
        }
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).printers[0].id, "late");
            assert_eq!(view.read(cx).device_notice, None);
            window.close_dialog(cx);
        });
    }
    #[gpui_kit::test]
    fn print_dialog_validates_ranges_and_copies_and_renders_preview(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            Root::new(host, window, cx)
        });
        cx.simulate_resize(size(px(1200.), px(1000.)));
        let view=cx.update(|window,cx|{
            let view=cx.new(|cx|PrintDialog::new("Proof".into(),0,window,cx));
            view.update(cx,|v,cx|{
                v.destination="test-printer".into();v.caps=Some(Capabilities::pdf());v.loading=false;
                v.sources=Some(Arc::new(vec![Source{name:"Page 1".into(),width:100,height:100,ppi:100.,rasterized:false,document:None,original_paths:vec![],svg:"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\"><rect width=\"100\" height=\"100\" fill=\"red\"/></svg>".into()}]));
                v.changed(cx);
            });
            let body=view.clone();window.open_dialog(cx,move|dialog,_,_|dialog.title(t!("print.print_dialog.title")).width(px(1020.)).child(body.clone()));view
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("print-dialog").bounds().size.width > px(0.));
            assert!(view.read(cx).preview.is_some());
            assert!(view.read(cx).draft(cx).is_ok());
            let field = view.read(cx).fields[0].clone();
            field.update(cx, |f, cx| f.set_value("0", window, cx));
            assert!(view.read(cx).draft(cx).is_err());
            field.update(cx, |f, cx| f.set_value("2", window, cx));
            view.update(cx, |v, _| v.scope = "range".into());
            let range = view.read(cx).fields[4].clone();
            range.update(cx, |f, cx| f.set_value("9", window, cx));
            assert!(view.read(cx).draft(cx).is_err());
            range.update(cx, |f, cx| f.set_value("1", window, cx));
            assert_eq!(view.read(cx).draft(cx).unwrap().0.copies, 2);
            window.close_dialog(cx);
        });
    }
    #[gpui_kit::test]
    fn pdf_document_layout_hides_sheet_controls_and_printer_switch_restores_them(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            Root::new(host, window, cx)
        });
        cx.simulate_resize(size(px(1200.), px(1000.)));
        let view = cx.update(|window, cx| {
            let view = cx.new(|cx| PrintDialog::new("Card".into(), 0, window, cx));
            view.update(cx, |v, cx| {
                v.loading = false;
                v.sources = Some(Arc::new(vec![Source { name:"Business card".into(), width:1050, height:600,
                    ppi:300., rasterized:false, document:None,original_paths:vec![],
                    svg:r#"<svg xmlns="http://www.w3.org/2000/svg" width="1050" height="600"><rect width="1050" height="600" fill="red"/></svg>"#.into() }]));
                v.choose_destination("pdf".into(), cx);
            });
            let body = view.clone(); window.open_dialog(cx, move |dialog, _, _| dialog.title(t!("print.print_dialog.title")).width(px(1020.)).child(body.clone()));
            view
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("print-paper").is_none());
            assert!(window.try_find("print-orientation").is_none());
            assert!(window.try_find("print-placement").is_none());
            assert!(window.find("print-artwork-size").visible());
            let (settings, job) = view.read(cx).draft(cx).unwrap();
            assert_eq!(settings.layout, Layout::Document);
            assert!((job.sheets[0].width - 88.9).abs() < 0.001);
            assert!((job.sheets[0].height - 50.8).abs() < 0.001);
            view.update(cx, |v, cx| v.choose_destination("portal".into(), cx));
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert_eq!(view.read(cx).settings.layout, Layout::Single);
            assert!(view.read(cx).settings.landscape);
            assert!(window.find("print-placement").visible());
            window.close_dialog(cx);
        });
    }
    #[gpui_kit::test]
    fn creative_controls_drive_shared_layout_and_preset_fields(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            Root::new(host, window, cx)
        });
        cx.update(|window, cx| {
            let view = cx.new(|cx| PrintDialog::new("Print controls".into(), 0, window, cx));
            view.update(cx, |v, cx| {
                v.destination = "pdf".into();
                v.caps = Some(Capabilities::pdf());
                v.loading = false;
                v.sources = Some(Arc::new(vec![
                    Source {
                        name: "Page".into(),
                        width: 2000,
                        height: 1000,
                        ppi: 254.,
                        rasterized: false,
                        document: None,
                        original_paths: vec![],
                        svg: String::new()
                    };
                    5
                ]));
                v.settings.layout = Layout::Contact;
                v.settings.placement = Placement::Fill;
                v.scope = "all".into();
                for (i, value) in [
                    (7, "2"),
                    (8, "2"),
                    (9, "10"),
                    (10, "0"),
                    (11, "100"),
                    (12, "3"),
                ] {
                    v.fields[i].update(cx, |f, cx| f.set_value(value, window, cx));
                }
                v.settings.creative.crop_marks = true;
                let (settings, job) = v.draft(cx).unwrap();
                assert_eq!(job.sheets.len(), 2);
                assert_eq!(job.sheets[0].items.len(), 4);
                assert_eq!(settings.creative.crop, [0., 1.]);
                assert!(job.sheets[0].items[0].crop_marks);
                v.fields[7].update(cx, |f, cx| f.set_value("0", window, cx));
                assert!(v.draft(cx).is_err());
                let preset = print::presets::Preset {
                    name: "Grid proof".into(),
                    settings,
                };
                v.apply_preset(&preset, window, cx);
                assert_eq!(v.fields[7].read(cx).value().as_str(), "2");
                assert_eq!(v.fields[13].read(cx).value().as_str(), "Grid proof");
                assert_eq!(v.draft(cx).unwrap().1.sheets.len(), 2);
                // Invalid hidden crop controls cannot break document-size output.
                v.settings.layout = Layout::Document;
                v.fields[10].update(cx, |f, cx| f.set_value("bad", window, cx));
                assert!(v.draft(cx).is_ok());
            });
        });
    }
    #[gpui_kit::test]
    fn production_controls_preflight_profiles_and_source_changes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            Root::new(host, window, cx)
        });
        cx.update(|window, cx| {
            let view = cx.new(|cx| PrintDialog::new("Proof".into(), 0, window, cx));
            view.update(cx, |s, cx| {
                s.caps = Some(Capabilities::pdf());
                s.destination = "pdf".into();
                s.loading = false;
                s.sources = Some(Arc::new(vec![Source {
                    name: "Photo".into(),
                    width: 100,
                    height: 100,
                    ppi: 100.,
                    svg: String::new(),
                    rasterized: false,
                    document: None,
                    original_paths: vec![],
                }]));
                s.settings.layout = Layout::Contact;
                s.settings.creative.labels = print::LabelMode::Name;
                assert!(s.draft(cx).unwrap().1.sheets[0].items[0].label.is_some());
                s.settings.production.managed = true;
                assert!(s.draft(cx).is_err());
                s.fields[14].update(cx, |f, cx| f.set_value("/path/to/output.icc", window, cx));
                let settings = s.draft(cx).unwrap().0;
                assert!(settings.production.enabled());
                s.destination = "queue".into();
                assert!(s.draft(cx).is_err());
                s.settings.production.driver_color_disabled = true;
                assert!(s.draft(cx).is_ok());
                s.destination = "portal".into();
                assert!(s.draft(cx).is_err());
                s.destination = "pdf".into();
                s.source_pending = true;
                assert!(s.draft(cx).is_err());
                s.source_pending = false;
                s.source_error = Some("Cannot decode chosen frame".into());
                assert!(s.draft(cx).is_err());
                s.settings.production.managed = false;
                s.settings.production.standard = print::production::PdfStandard::PdfX1a2001;
                s.choose_destination("portal".into(), cx);
                assert!(s.settings.production.managed);
                assert!(!s.settings.production.driver_color_disabled);
                assert_eq!(
                    s.settings.production.standard,
                    print::production::PdfStandard::Pdf
                );
            });
        });
    }
}
