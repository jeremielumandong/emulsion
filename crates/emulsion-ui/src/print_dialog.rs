//! Shared print UI. Sources are immutable snapshots; capabilities, rendering and
//! submission run off the UI thread. Generations discard stale worker results.
use crate::theme;
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
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub fn open(
    name: String,
    docs: Vec<(String, emulsion_core::Document)>,
    active: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| PrintDialog::new(name, active, window, cx));
    view.update(cx, |this, cx| {
        this.load_sources(docs, cx);
        this.refresh(cx)
    });
    let cancel = view.read(cx).cancel.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let cancel = cancel.clone();
        dialog
            .title("Print")
            .width(px(1020.))
            .overlay_closable(false)
            .on_close(move |_, _, _| {
                cancel.store(true, Ordering::Relaxed);
            })
            .child(view.clone())
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
    scope: String,
    fields: [Entity<InputState>; 5],
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
    loading: bool,
    cancel: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    portal: Option<Arc<print::portal::Prepared>>,
}
impl Drop for PrintDialog {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed)
    }
}
impl PrintDialog {
    fn new(name: String, active: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let fields = ["1", "100", "5", "5", ""]
            .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let subscriptions = fields
            .iter()
            .map(|field| {
                cx.subscribe(field, |this, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.changed(cx)
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
            scope: "current".into(),
            fields,
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
            loading: true,
            cancel: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "linux")]
            portal: None,
        }
    }
    fn load_sources(
        &mut self,
        docs: Vec<(String, emulsion_core::Document)>,
        cx: &mut Context<Self>,
    ) {
        let cancel = self.cancel.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { print::prepare_sources(docs, &cancel) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(s) => this.sources = Some(Arc::new(s)),
                    Err(e) => this.source_error = Some(e.to_string()),
                }
                this.changed(cx)
            })
            .ok();
        })
        .detach();
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
        cx.spawn(async move|this,cx|{
            let result=cx.background_spawn(async{print::discover()}).await;
            this.update(cx,|this,cx|{
                if generation!=this.discovery{return}
                this.loading=false;
                match result{
                    Ok(printers)=>{this.printers=printers;
                        if this.destination=="initial"{let dest=this.printers.iter().find(|p|p.default).or(this.printers.first()).map(|p|p.id.clone()).unwrap_or_else(||"pdf".into());this.choose_destination(dest,cx)}
                        else if this.destination!="pdf"&&this.destination!="portal"&&!this.printers.iter().any(|p|p.id==this.destination){this.caps=None;this.device_notice=Some("The selected printer disappeared. Choose another destination or Save PDF.".into());this.changed(cx)}
                        else if this.destination!="pdf"&&this.destination!="portal"{this.choose_destination(this.destination.clone(),cx)}
                        else {cx.notify();}
                    },
                    Err(e)=>{this.device_notice=Some(e.to_string());if this.destination=="initial"{this.choose_destination("pdf".into(),cx)}else{cx.notify();}}
                }
            }).ok();
        }).detach();
    }
    fn choose_destination(&mut self, id: String, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.destination = id.clone();
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
            self.changed(cx);
            return;
        }
        self.device_notice = Some("Reading printer capabilities…".into());
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
    fn draft(&self, cx: &App) -> anyhow::Result<(Settings, JobLayout)> {
        use anyhow::{Context, bail};
        if self.caps.is_none() {
            bail!("Select an available printer or Save PDF")
        }
        let sources = self
            .sources
            .as_ref()
            .context("Preparing document artwork…")?;
        let mut settings = self.settings.clone();
        settings.copies = if self.destination == "pdf" || self.destination == "portal" {
            1
        } else {
            self.fields[0]
                .read(cx)
                .value()
                .parse()
                .context("Copies must be a whole number from 1 to 999")?
        };
        settings.scale =
            if settings.placement == Placement::Actual || settings.layout == Layout::Poster {
                self.fields[1]
                    .read(cx)
                    .value()
                    .parse()
                    .context("Enter a scale from 1 to 1000 percent")?
            } else {
                100.
            };
        settings.extra_margin = self.fields[2]
            .read(cx)
            .value()
            .parse()
            .context("Enter an additional margin in millimeters")?;
        settings.overlap = if settings.layout == Layout::Poster {
            self.fields[3]
                .read(cx)
                .value()
                .parse()
                .context("Enter poster overlap in millimeters")?
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
        if self.scope == "range" && self.fields[4].read(cx).value().trim().is_empty() {
            bail!("Enter a page range, for example 1-3, 5")
        }
        let selected = match self.scope.as_str() {
            "all" => (0..sources.len()).collect(),
            "range" => print::page_range(&self.fields[4].read(cx).value(), sources.len())?,
            _ => vec![self.active],
        };
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
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    print::preview(&sources, &sheet, settings.grayscale, 900)
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
                    Err(e) => this.notice = Some(format!("Preview failed: {e}")),
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
        let options = std::iter::once((String::new(), "Printer default".into()))
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
                "Choose where to save the PDF…"
            } else {
                "Preparing print job…"
            }
            .into(),
        );
        cx.notify();
        let path_request = (destination == "pdf").then(|| {
            cx.prompt_for_new_path(
                &std::env::current_dir().unwrap_or_default(),
                Some("Print.pdf"),
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
                        print::write_pdf(&sources, &layout, settings.grayscale, &path, &cancel)?;
                        return Ok(format!("Saved {}", path.display()));
                    }
                    #[cfg(target_os = "linux")]
                    if let Some(portal) = portal {
                        let dir = tempfile::tempdir()?;
                        let path = dir.path().join("print.pdf");
                        print::write_pdf(&sources, &layout, settings.grayscale, &path, &cancel)?;
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
                    Err(e) => format!("Print failed: {e}"),
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
        self.notice = Some("Choose printer and paper in the system dialog…".into());
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
        let mut destinations = vec![("pdf".into(), "Save PDF…".into())];
        #[cfg(target_os = "linux")]
        destinations.push(("portal".into(), "System print dialog…".into()));
        destinations.extend(self.printers.iter().map(|v| {
            (
                v.id.clone(),
                format!(
                    "{}{} · {}",
                    v.name,
                    if v.default { " (default)" } else { "" },
                    v.status
                ),
            )
        }));
        let caps = self.caps.clone().unwrap_or_else(Capabilities::pdf);
        let mut controls=div().id("print-controls").w(px(310.)).when(narrow, |d| d.w_full()).flex_none().flex().flex_col().gap_3().pr_2()
        .child(self.select("print-destination","Destination",self.destination.clone(),destinations,|s,v,cx|s.choose_destination(v,cx),cx))
        .child(div().flex().gap_2().child(Button::new("print-refresh").label(if self.loading{"Searching…"}else{"Refresh"}).small().ghost().disabled(self.busy||self.loading).on_click(cx.listener(|s,_,_,cx|s.refresh(cx))))
            .child(Button::new("print-system-setup").label("Printer setup…").small().ghost().on_click(|_,_,cx|{
                #[cfg(target_os="linux")]cx.open_url("http://localhost:631/printers/");
                #[cfg(target_os="windows")]cx.open_url("ms-settings:printers");
                #[cfg(target_os="macos")]cx.open_url("x-apple.systempreferences:com.apple.preference.printfax");
            })))
        .when_some(self.device_notice.clone(),|d,n|d.child(div().text_color(p.muted).child(n)))
        .when(self.printers.is_empty()&&!self.loading,|d|d.child(div().text_color(p.muted).child("No printer queues found. Add a printer in system settings, then refresh. Save PDF is available.")))
        .child(self.select("print-content","Content",self.scope.clone(),vec![("current".into(),"Current page / canvas".into()),("all".into(),"All document pages".into()),("range".into(),"Page range…".into())],|s,v,cx|{s.scope=v;s.sheet=0;s.changed(cx)},cx))
        .when(self.scope=="range",|d|d.child(self.field(4,"Pages (for example 1-3, 5)")));
        if !portal {
            controls = controls
                .child(
                    self.select(
                        "print-paper",
                        "Paper / borderless sizes",
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
                            }
                            s.changed(cx)
                        },
                        cx,
                    ),
                )
                .child(self.select(
                    "print-orientation",
                    "Orientation",
                    self.settings.landscape.to_string(),
                    vec![
                        ("false".into(), "Portrait".into()),
                        ("true".into(), "Landscape".into()),
                    ],
                    |s, v, cx| {
                        s.settings.landscape = v == "true";
                        s.changed(cx)
                    },
                    cx,
                ));
        }
        controls = controls.child(self.select(
            "print-layout",
            "Layout",
            format!("{:?}", self.settings.layout),
            vec![
                ("Single".into(), "One image / page per sheet".into()),
                ("Contact".into(), "Contact sheet · 2 × 3".into()),
                (
                    "Repeat".into(),
                    "Repeat first selected image · 2 × 3".into(),
                ),
                ("Poster".into(), "Tiled poster".into()),
            ],
            |s, v, cx| {
                s.settings.layout = match v.as_str() {
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
        if self.settings.layout != Layout::Poster {
            controls = controls.child(self.select(
                "print-placement",
                "Placement",
                format!("{:?}", self.settings.placement),
                vec![
                    ("Fit".into(), "Fit · entire artwork".into()),
                    ("Fill".into(), "Fill · crop edges".into()),
                    ("Actual".into(), "Actual size / custom scale".into()),
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
        if self.settings.placement == Placement::Actual || self.settings.layout == Layout::Poster {
            controls = controls.child(self.field(1, "Scale (%) · 100 = document physical size"));
        }
        controls = controls.child(self.field(2, "Extra margin (mm)"));
        if self.settings.layout == Layout::Poster {
            controls = controls.child(self.field(3, "Tile overlap (mm)"));
        }
        if !portal && self.destination != "pdf" {
            controls = controls.child(self.field(0, "Copies"));
        }
        controls = controls.child(self.select(
            "print-color",
            "Output",
            self.settings.grayscale.to_string(),
            if caps.color {
                vec![
                    ("false".into(), "Color".into()),
                    ("true".into(), "Grayscale".into()),
                ]
            } else {
                vec![("true".into(), "Grayscale".into())]
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
                    "Paper type",
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
                    "Paper source",
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
                    "Print quality",
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
                    "Sides",
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
        let summary = draft
            .as_ref()
            .map(|(s, l)| {
                format!(
                    "{} sheet sides × {} copies · {:.1} × {:.1} mm",
                    l.sheets.len(),
                    s.copies,
                    l.sheets[0].width,
                    l.sheets[0].height
                )
            })
            .unwrap_or_else(|e| e.to_string());
        let warnings = draft
            .as_ref()
            .ok()
            .map(|(_, l)| l.warnings.join("\n"))
            .unwrap_or_default();
        let notice = self.source_error.clone().or(self.notice.clone());
        let button_label = if self.destination == "pdf" {
            "Save PDF…"
        } else {
            "Print"
        };
        #[cfg(target_os = "linux")]
        let button_label = if portal && self.portal.is_none() {
            "Choose printer…"
        } else {
            button_label
        };
        let disabled = self.busy
            || self.loading
            || self.preview_pending
            || self.preview.is_none()
            || draft.is_err()
            || self.source_error.is_some();
        div().id("print-dialog").test_support().flex().flex_col().gap_3().text_size(px(12.)).text_color(p.ink)
        .child(div().text_color(p.muted).child(format!("{} · current edited appearance",self.name)))
        .child(div().id("print-scroll").max_h((window.viewport_size().height-px(250.)).max(px(180.))).overflow_y_scroll()
            .child(div().flex().gap_5().when(narrow, |d| d.flex_col()).child(div().flex_1().min_w_0().flex().flex_col().items_center().gap_3()
                .child(div().w_full().min_h(px(460.)).bg(p.stage).flex().items_center().justify_center()
                    .child(div().w(px(pw as f32)).h(px(ph as f32)).bg(rgb(0xffffff)).when_some(image,|d,image|d.child(img(ImageSource::Render(image)).size_full().object_fit(ObjectFit::Contain)))))
                .child(div().flex().gap_3().items_center().child(Button::new("print-previous-sheet").label("Previous").small().disabled(self.busy||self.sheet==0).on_click(cx.listener(|s,_,_,cx|{s.sheet=s.sheet.saturating_sub(1);s.changed(cx)})))
                    .child(format!("Sheet {} of {}",self.sheet+1,size.map(|s|s.2).unwrap_or(1)))
                    .child(Button::new("print-next-sheet").label("Next").small().disabled(self.busy||self.sheet+1>=size.map(|s|s.2).unwrap_or(1)).on_click(cx.listener(|s,_,_,cx|{s.sheet+=1;s.changed(cx)}))))
                .when(self.preview_pending,|d|d.child("Updating preview…"))
                .child(div().text_color(p.muted).child("Paper is white. Transparent artwork prints against the paper. Color is managed by the printer."))
                .when(!warnings.is_empty(),|d|d.child(div().text_color(rgb(0xc98535)).child(warnings)))
                .child(div().text_color(p.muted).child("Video objects print their poster artwork. Contact sheets use the selected document pages. Poster tiles run left to right, then top to bottom.")))
                .child(controls)))
        .when_some(notice,|d,n|d.child(div().id("print-notice").test_support().child(n)))
        .child(div().border_t_1().border_color(p.line).pt_3().flex().items_center().gap_3()
            .child(div().flex_1().child(summary))
            .child(Button::new("print-cancel").label(if self.busy{"Cancel / close"}else{"Close"}).on_click(cx.listener(|s,_,window,cx|{s.cancel.store(true,Ordering::Relaxed);window.close_dialog(cx)})))
            .child(Button::new("print-submit").label(button_label).primary().disabled(disabled).on_click(cx.listener(|s,_,window,cx|s.submit(window,cx)))))
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
                v.sources=Some(Arc::new(vec![Source{name:"Page 1".into(),width:100,height:100,ppi:100.,rasterized:false,original_paths:vec![],svg:"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\"><rect width=\"100\" height=\"100\" fill=\"red\"/></svg>".into()}]));
                v.changed(cx);
            });
            let body=view.clone();window.open_dialog(cx,move|dialog,_,_|dialog.title("Print").width(px(1020.)).child(body.clone()));view
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
}
