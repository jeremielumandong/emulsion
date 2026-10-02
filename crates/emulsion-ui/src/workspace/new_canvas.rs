//! Native, preset-driven document creation. Nothing edits the current tab until
//! the complete specification validates and the user chooses Create.
#[path = "new_canvas_templates.rs"]
mod templates;
use super::*;
use emulsion_core::creation::{Background, CanvasKind, CanvasSpec, Unit, presets};
use gpui_kit::component::Disableable;
use gpui_kit::component::input::{Input, InputEvent, InputState};

/// A built-in catalog name (preset, category, template) in the interface
/// language. Names without a translation, like the user's own, stay as given.
pub(super) fn catalog_label(group: &str, english: &str) -> String {
    catalog_text(group, english, english)
}

/// Catalog text keyed by an English `name`, or `fallback` when untranslated.
pub(super) fn catalog_text(group: &str, name: &str, fallback: &str) -> String {
    let slug = name
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    crate::_rust_i18n_try_translate(&rust_i18n::locale(), format!("new_canvas.{group}_{slug}"))
        .map_or_else(|| fallback.to_string(), |text| text.into_owned())
}

pub(super) fn kind_label(kind: CanvasKind) -> std::borrow::Cow<'static, str> {
    match kind {
        CanvasKind::Photo => t!("new_canvas.kind_photo"),
        CanvasKind::Paint => t!("new_canvas.kind_paint"),
        CanvasKind::Design => t!("new_canvas.kind_design"),
        CanvasKind::Diagram => t!("new_canvas.kind_diagram"),
    }
}

/// The default document name for a kind in the interface language.
pub(super) fn untitled(kind: CanvasKind) -> String {
    match kind {
        CanvasKind::Photo => t!("new_canvas.untitled_photo"),
        CanvasKind::Paint => t!("new_canvas.untitled_paint"),
        CanvasKind::Design => t!("new_canvas.untitled_design"),
        CanvasKind::Diagram => t!("new_canvas.untitled_diagram"),
    }
    .into_owned()
}

fn unit_label(unit: Unit) -> std::borrow::Cow<'static, str> {
    match unit {
        Unit::Pixels => t!("new_canvas.unit_px"),
        Unit::Millimeters => t!("new_canvas.unit_mm"),
        Unit::Inches => t!("new_canvas.unit_in"),
    }
}

fn background_label(background: Background) -> std::borrow::Cow<'static, str> {
    match background {
        Background::White => t!("new_canvas.bg_white"),
        Background::Transparent => t!("new_canvas.bg_transparent"),
        Background::Black => t!("new_canvas.bg_black"),
        Background::Paper => t!("new_canvas.bg_paper"),
    }
}

/// Canvas validation messages from the core in the interface language.
pub(super) fn core_error(error: String) -> String {
    let text = match error.as_str() {
        "Resolution must be between 1 and 9600 ppi." => t!("new_canvas.err_resolution"),
        "Choose 8-bit or 16-bit color." => t!("new_canvas.err_depth"),
        "Canvas dimensions must be between 1 and 30,000 pixels." => {
            t!("new_canvas.err_dimensions")
        }
        "Canvas area must not exceed 400 megapixels." => t!("new_canvas.err_area"),
        "Enter a document name of 1–200 characters." => t!("new_canvas.err_name"),
        "Multiple pages require a Design or Diagram project." => t!("new_canvas.err_multipage"),
        "Project exceeds the total page area limit." => t!("new_canvas.err_page_area"),
        "Bleed must be between 0 and 100 mm." => t!("new_canvas.err_bleed"),
        "Choose Design or Diagram for a page project." => t!("new_canvas.err_project_kind"),
        e if e == format!("Choose 1–{} pages.", emulsion_core::project::MAX_PAGES) => {
            t!("new_canvas.err_pages", max = emulsion_core::project::MAX_PAGES)
        }
        _ => return error,
    };
    text.into_owned()
}

struct NewCanvas {
    workspace: WeakEntity<Workspace>,
    spec: CanvasSpec,
    fields: [Entity<InputState>; 6],
    search: Entity<InputState>,
    category: String,
    folder: Option<u64>,
    notice: Option<String>,
    submitted: bool,
    cancelled: bool,
    templates: templates::Gallery,
    _subscriptions: Vec<Subscription>,
}

impl NewCanvas {
    fn new(
        workspace: WeakEntity<Workspace>,
        folder: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let spec = CanvasSpec {
            name: untitled(CanvasKind::Photo),
            ..CanvasSpec::default()
        };
        let fields = [
            spec.name.clone(),
            spec.width.to_string(),
            spec.height.to_string(),
            spec.resolution.to_string(),
            spec.pages.to_string(),
            spec.bleed_mm.to_string(),
        ]
        .map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value)));
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("new_canvas.search")));
        let mut subscriptions = vec![cx.subscribe(&search, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.templates.page = 0;
                cx.notify();
            }
        })];
        for field in &fields {
            subscriptions.push(
                cx.subscribe_in(field, window, |this, _, event, window, cx| match event {
                    InputEvent::Change => {
                        this.notice = None;
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } if this.submit(window, cx) => {
                        window.close_dialog(cx);
                    }
                    _ => {}
                }),
            );
        }
        if let Some(owner) = workspace.upgrade() {
            subscriptions.push(cx.observe(&owner, |_, _, cx| cx.notify()));
        }
        Self {
            folder,
            workspace,
            spec,
            fields,
            search,
            category: "Screen".into(),
            notice: None,
            submitted: false,
            cancelled: false,
            templates: templates::Gallery::default(),
            _subscriptions: subscriptions,
        }
    }

    fn project_destination(&self, cx: &mut Context<Self>) -> AnyElement {
        let folders = self
            .workspace
            .upgrade()
            .map(|w| w.read(cx).home_state.projects.catalog.folders.clone())
            .unwrap_or_default();
        let selected = self.folder;
        let label = folders
            .iter()
            .find(|f| Some(f.id) == selected)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| t!("home.unfiled").into_owned());
        let owner = cx.weak_entity();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(t!("new_canvas.save_to_project"))
            .child(
                Button::new("new-canvas-project")
                    .label(label)
                    .dropdown_caret(true)
                    .small()
                    .outline()
                    .disabled(self.submitted)
                    .dropdown_menu(move |mut menu, _, _| {
                        let owner_none = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(t!("home.unfiled"))
                                .checked(selected.is_none())
                                .on_click(move |_, _, cx| {
                                    owner_none
                                        .update(cx, |this, cx| {
                                            this.folder = None;
                                            cx.notify();
                                        })
                                        .ok();
                                }),
                        );
                        for folder in &folders {
                            let owner = owner.clone();
                            let id = folder.id;
                            menu = menu.item(
                                PopupMenuItem::new(folder.name.clone())
                                    .checked(selected == Some(id))
                                    .on_click(move |_, _, cx| {
                                        owner
                                            .update(cx, |this, cx| {
                                                this.folder = Some(id);
                                                cx.notify();
                                            })
                                            .ok();
                                    }),
                            );
                        }
                        menu
                    }),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .child(t!("new_canvas.filed_hint")),
            )
            .into_any_element()
    }

    fn draft(&self, cx: &App) -> Result<CanvasSpec, String> {
        let mut spec = self.spec.clone();
        spec.name = self.fields[0].read(cx).value().trim().to_string();
        let number = |index: usize, message: std::borrow::Cow<'static, str>| {
            self.fields[index]
                .read(cx)
                .value()
                .trim()
                .parse::<f64>()
                .map_err(|_| message.into_owned())
        };
        spec.width = number(1, t!("new_canvas.enter_width"))?;
        spec.height = number(2, t!("new_canvas.enter_height"))?;
        spec.resolution = number(3, t!("new_canvas.enter_resolution"))?;
        if matches!(spec.kind, CanvasKind::Design | CanvasKind::Diagram) {
            spec.pages = self.fields[4]
                .read(cx)
                .value()
                .trim()
                .parse::<usize>()
                .map_err(|_| t!("new_canvas.enter_pages").into_owned())?;
            spec.bleed_mm = number(5, t!("new_canvas.enter_bleed"))?;
        } else {
            spec.pages = 1;
            spec.bleed_mm = 0.;
        }
        spec.validate().map_err(core_error)?;
        Ok(spec)
    }

    fn show_spec(&mut self, spec: CanvasSpec, window: &mut Window, cx: &mut Context<Self>) {
        let values = [
            spec.name.clone(),
            spec.width.to_string(),
            spec.height.to_string(),
            spec.resolution.to_string(),
            spec.pages.to_string(),
            spec.bleed_mm.to_string(),
        ];
        for (field, value) in self.fields.iter().zip(values) {
            field.update(cx, |field, cx| field.set_value(value, window, cx));
        }
        self.spec = spec;
        self.notice = None;
        cx.notify();
    }

    fn pick_kind(&mut self, kind: CanvasKind, window: &mut Window, cx: &mut Context<Self>) {
        self.templates.enabled = matches!(kind, CanvasKind::Design | CanvasKind::Diagram);
        self.templates.selected = None;
        self.templates.category = None;
        self.templates.page = 0;
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        let mut spec = self.draft(cx).unwrap_or_else(|_| self.spec.clone());
        if CanvasKind::ALL.into_iter().any(|old| {
            spec.name == untitled(old) || spec.name == format!("Untitled {}", old.label().to_lowercase())
        }) {
            spec.name = untitled(kind);
        }
        spec.kind = kind;
        if !matches!(kind, CanvasKind::Design | CanvasKind::Diagram) {
            spec.pages = 1;
            spec.bleed_mm = 0.;
        }
        spec.background = if kind == CanvasKind::Paint {
            Background::Paper
        } else {
            Background::White
        };
        let preset = &presets(kind)[0];
        preset.apply(&mut spec);
        self.category = preset.category.into();
        self.show_spec(spec, window, cx);
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.templates.enabled {
            self.submit_template(window, cx);
            return false;
        }
        if self.submitted {
            return true;
        }
        let result = self.draft(cx).and_then(|spec| {
            if matches!(spec.kind, CanvasKind::Design | CanvasKind::Diagram) {
                spec.create_project()
                    .map(|project| (spec, project.doc.clone(), Some(project)))
            } else {
                spec.create().map(|doc| (spec, doc, None))
            }
        });
        let (spec, doc, project) = match result {
            Ok(value) => value,
            Err(error) => {
                self.notice = Some(core_error(error));
                cx.notify();
                return false;
            }
        };
        self.install_created(spec, doc, project, window, cx)
    }

    fn install_created(
        &mut self,
        spec: CanvasSpec,
        doc: Document,
        project: Option<emulsion_core::project::ProjectEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(workspace) = self.workspace.upgrade() else {
            return false;
        };
        crate::app_state::update_settings(cx, |settings| {
            settings.recent_canvases.retain(|old| old != &spec);
            settings.recent_canvases.insert(0, spec.clone());
            settings.recent_canvases.truncate(8);
        });
        self.submitted = true;
        let folder = self.folder;
        workspace.update(cx, |workspace, cx| {
            workspace.add_tab_then(window, cx, move |workspace, window, cx| {
                if let Some(project) = project {
                    workspace.install_project(project, spec.name, window, cx);
                } else {
                    workspace.install(doc, None, None, None, spec.name, window, cx);
                }
                if let Some(editor) = &workspace.editor {
                    editor.update(cx, |editor, cx| {
                        editor.home_folder_on_save = Some(folder);
                        editor.home_canvas_kind = Some(spec.kind);
                        if editor.draw_mode != (spec.kind == CanvasKind::Paint) {
                            editor.toggle_draw_mode(cx);
                        }
                    });
                }
            });
        });
        true
    }

    fn save_preset(&mut self, cx: &mut Context<Self>) {
        match self.draft(cx) {
            Ok(spec) => {
                let saved = &crate::app_state::settings(cx).canvas_presets;
                if saved.len() >= 100
                    && !saved
                        .iter()
                        .any(|old| old.name == spec.name && old.kind == spec.kind)
                {
                    self.notice = Some(t!("new_canvas.presets_full").into_owned());
                    cx.notify();
                    return;
                }
                crate::app_state::update_settings(cx, |settings| {
                    settings
                        .canvas_presets
                        .retain(|old| old.name != spec.name || old.kind != spec.kind);
                    settings.canvas_presets.insert(0, spec);
                });
                self.notice = Some(t!("new_canvas.preset_saved").into_owned());
            }
            Err(error) => self.notice = Some(error),
        }
        cx.notify();
    }
}

fn field(id: &'static str, input: &Entity<InputState>) -> impl IntoElement {
    editable_field(id, input, false)
}
/// A labelled input; `id` is the stable English name used in its element id.
fn editable_field(id: &'static str, input: &Entity<InputState>, disabled: bool) -> impl IntoElement {
    let label = match id {
        "Name" => t!("new_canvas.field_name"),
        "Width" => t!("new_canvas.field_width"),
        "Height" => t!("new_canvas.field_height"),
        "Resolution · ppi" => t!("new_canvas.field_resolution"),
        "Pages" => t!("new_canvas.field_pages"),
        "Bleed · mm" => t!("new_canvas.field_bleed"),
        other => other.into(),
    };
    div()
        .flex()
        .flex_col()
        .gap_1()
        .flex_1()
        .min_w_0()
        .child(label)
        .child(
            div()
                .id(SharedString::from(format!("new-canvas-field-{id}")))
                .test_support()
                .child(Input::new(input).small().disabled(disabled)),
        )
}

impl Render for NewCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.templates.enabled {
            return self.template_gallery(window, cx);
        }
        let p = theme::palette(cx);
        let draft = self.draft(cx);
        let valid = draft.is_ok();
        let size = draft.as_ref().ok().and_then(|s| s.pixel_size().ok());
        let (preview_w, preview_h) = size
            .map(|(w, h)| {
                let scale = 72. / (w.max(h) as f32);
                (w as f32 * scale, h as f32 * scale)
            })
            .unwrap_or((72., 48.));
        let query = self.search.read(cx).value().to_lowercase();
        let catalog = presets(self.spec.kind);
        let mut categories = Vec::new();
        for preset in catalog {
            if !categories.contains(&preset.category) {
                categories.push(preset.category);
            }
        }
        let settings = crate::app_state::settings(cx);
        let radius = settings.corners.radius();
        let saved = settings
            .canvas_presets
            .iter()
            .filter(|s| s.kind == self.spec.kind)
            .cloned()
            .collect::<Vec<_>>();
        let recent = settings
            .recent_canvases
            .iter()
            .filter(|s| s.kind == self.spec.kind)
            .take(4)
            .cloned()
            .collect::<Vec<_>>();
        if !saved.is_empty() {
            categories.push("Saved");
        }
        let message = self.notice.clone().or_else(|| draft.as_ref().err().cloned()).unwrap_or_else(|| {
            let bytes = draft.as_ref().unwrap().layer_bytes().unwrap_or(0);
            t!("new_canvas.memory", size = format!("{:.1}", bytes as f64 / 1_048_576.)).into_owned()
        });
        let preview_color = self.spec.background.rgba().unwrap_or([210, 210, 210, 255]);
        let [r, g, b, _] = preview_color;
        let preview_color = rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
        div()
            .id("new-canvas-form")
            .test_support()
            .flex()
            .flex_col()
            .gap_4()
            .text_size(px(12.))
            .text_color(p.ink)
            .child(self.creation_mode(cx))
            .child(
                div()
                    .id("new-canvas-scroll")
                    .max_h((window.viewport_size().height - px(230.)).max(px(150.)))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_4()
                            .child(
                                div()
                                    .id("new-canvas-types").test_support()
                                    .w(px(200.))
                                    .flex_none()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(div().font_family(theme::MONO_FONT).text_size(px(9.5)).text_color(p.muted).child(t!("new_canvas.document_type").to_uppercase()))
                                    .children(CanvasKind::ALL.map(|kind| {
                                        Button::new(("new-canvas-kind", kind as usize))
                                            .label(kind_label(kind))
                                            .small()
                                            .ghost()
                                            .h(px(34.))
                                            .selected(self.spec.kind == kind)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.pick_kind(kind, window, cx)
                                            }))
                                    }))
                                    .child(div().mt_3().text_color(p.muted).child(t!("new_canvas.recent_sizes")))
                                    .children(recent.into_iter().enumerate().map(
                                        |(index, spec)| {
                                            Button::new(("new-canvas-recent", index))
                                                .label(spec.name.clone())
                                                .small()
                                                .ghost()
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.show_spec(spec.clone(), window, cx)
                                                    },
                                                ))
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(220.))
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(div().id("new-canvas-search").test_support().child(Input::new(&self.search).small()))
                                    .child(div().flex().flex_wrap().gap_1().children(
                                        categories.into_iter().enumerate().map(
                                            |(index, category)| {
                                                Button::new(("new-canvas-category", index))
                                                    .label(catalog_label("category", category))
                                                    .xsmall()
                                                    .ghost()
                                                    .selected(self.category == category)
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.category = category.into();
                                                        cx.notify();
                                                    }))
                                            },
                                        ),
                                    ))
                                    .child(
                                        div().flex().flex_wrap().gap_2().children(
                                            catalog
                                                .iter()
                                                .enumerate()
                                                .filter(|(_, preset)| {
                                                    if query.is_empty() { preset.category == self.category } else { [preset.name.to_string(), preset.category.to_string(), catalog_label("preset", preset.name), catalog_label("category", preset.category)].iter().any(|text| text.to_lowercase().contains(&query)) }
                                                })
                                                .map(|(index, preset)| {
                                                    let selected = draft.as_ref().is_ok_and(|s| {
                                                        s.width == preset.width
                                                            && s.height == preset.height
                                                            && s.unit == preset.unit
                                                            && s.resolution == preset.resolution
                                                    });
                                                    let scale = (86. / preset.width.max(1.)).min(60. / preset.height.max(1.));
                                                    let name = catalog_label("preset", preset.name);
                                                    let unit = unit_label(preset.unit);
                                                    Button::new(("new-canvas-preset", index))
                                                        .accessibility_label(format!("{} · {} × {} {}", name, preset.width, preset.height, unit))
                                                        .child(div().flex().flex_col().w_full().gap(px(2.))
                                                            .child(div().h(px(72.)).flex().items_center().justify_center()
                                                                .child(div().w(px((preset.width * scale) as f32))
                                                                    .h(px((preset.height * scale) as f32))
                                                                    .bg(p.paper).border_1().border_color(p.muted)))
                                                            .child(div().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).text_ellipsis().child(name))
                                                            .child(div().font_family(theme::MONO_FONT).text_size(px(10.)).text_color(p.muted)
                                                                .child(format!("{} × {} {}", preset.width, preset.height, unit))))
                                                        .h(px(124.))
                                                        .w(px(132.))
                                                        .small()
                                                        .outline()
                                                        .selected(selected)
                                                        .on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                let mut spec =
                                                                    this.draft(cx).unwrap_or_else(
                                                                        |_| this.spec.clone(),
                                                                    );
                                                                // Keep a typed name even if a size field is temporarily invalid.
                                                                spec.name = this.fields[0]
                                                                    .read(cx)
                                                                    .value()
                                                                    .to_string();
                                                                preset.apply(&mut spec);
                                                                this.show_spec(spec, window, cx);
                                                            },
                                                        ))
                                                }),
                                        ),
                                    )
                                    .when(self.category == "Saved" || !query.is_empty(), |panel| {
                                        panel.children(saved.into_iter().filter(|spec| query.is_empty() || spec.name.to_lowercase().contains(&query)).enumerate().map(
                                            |(index, spec)| {
                                                let remove_name = spec.name.clone();
                                                let remove_kind = spec.kind;
                                                div().flex().gap_1().child(Button::new(("new-canvas-saved", index))
                                                    .label(spec.name.clone())
                                                    .small()
                                                    .outline()
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.show_spec(spec.clone(), window, cx)
                                                        },
                                                    )))
                                                    .child(Button::new(("new-canvas-remove-preset", index))
                                                        .label("×").tooltip(t!("new_canvas.remove_preset")).small().ghost()
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            crate::app_state::update_settings(cx, |settings| {
                                                                settings.canvas_presets.retain(|old| old.name != remove_name || old.kind != remove_kind);
                                                            });
                                                            if !crate::app_state::settings(cx).canvas_presets.iter().any(|s| s.kind == this.spec.kind) {
                                                                this.category = presets(this.spec.kind)[0].category.into();
                                                            }
                                                            cx.notify();
                                                        })))
                                            },
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .w(px(260.))
                                    .flex_none()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .h(px(88.))
                                            .rounded(px(radius))
                                            .bg(p.soft_bg)
                                            .child(
                                                div()
                                                    .w(px(preview_w))
                                                    .h(px(preview_h))
                                                    .bg(preview_color)
                                                    .border_1()
                                                    .border_color(p.line),
                                            ),
                                    )
                                    .child(field("Name", &self.fields[0]))
                                    .child(self.project_destination(cx))
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .child(field("Width", &self.fields[1]))
                                            .child(field("Height", &self.fields[2])),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap_1()
                                            .children(Unit::ALL.map(|unit| {
                                                Button::new(("new-canvas-unit", unit as usize))
                                                    .label(unit_label(unit))
                                                    .xsmall()
                                                    .ghost()
                                                    .selected(self.spec.unit == unit)
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            match this.draft(cx).and_then(
                                                                |mut s| {
                                                                    s.convert_unit(unit).map(|_| s)
                                                                },
                                                            ) {
                                                                Ok(spec) => {
                                                                    this.show_spec(spec, window, cx)
                                                                }
                                                                Err(error) => {
                                                                    this.notice = Some(core_error(error));
                                                                    cx.notify();
                                                                }
                                                            }
                                                        },
                                                    ))
                                            }))
                                            .child(
                                                Button::new("new-canvas-orientation")
                                                    .label(t!("new_canvas.swap"))
                                                    .xsmall()
                                                    .ghost()
                                                    .tooltip(t!("new_canvas.swap_tooltip"))
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            let w = this.fields[1]
                                                                .read(cx)
                                                                .value()
                                                                .to_string();
                                                            let h = this.fields[2]
                                                                .read(cx)
                                                                .value()
                                                                .to_string();
                                                            this.fields[1].update(
                                                                cx,
                                                                |input, cx| {
                                                                    input.set_value(h, window, cx)
                                                                },
                                                            );
                                                            this.fields[2].update(
                                                                cx,
                                                                |input, cx| {
                                                                    input.set_value(w, window, cx)
                                                                },
                                                            );
                                                            cx.notify();
                                                        },
                                                    )),
                                            ),
                                    )
                                    .child(field("Resolution · ppi", &self.fields[3]))
                                    .when(matches!(self.spec.kind, CanvasKind::Design | CanvasKind::Diagram), |panel| panel.child(div().flex().gap_2()
                                        .child(field("Pages", &self.fields[4])).child(field("Bleed · mm", &self.fields[5]))))
                                    .child(div().flex().gap_1().children([8, 16].map(|depth| {
                                        Button::new(("new-canvas-depth", depth as usize))
                                            .label(t!("new_canvas.depth", depth = depth))
                                            .xsmall()
                                            .ghost()
                                            .selected(self.spec.depth == depth)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.spec.depth = depth;
                                                cx.notify();
                                            }))
                                    })))
                                    .child(div().text_color(p.muted).child(t!("new_canvas.background")))
                                    .child(div().flex().flex_wrap().gap_1().children(
                                        Background::ALL.map(|background| {
                                            Button::new((
                                                "new-canvas-background",
                                                background as usize,
                                            ))
                                            .label(background_label(background))
                                            .xsmall()
                                            .ghost()
                                            .selected(self.spec.background == background)
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    this.spec.background = background;
                                                    cx.notify();
                                                }),
                                            )
                                        }),
                                    )),
                            ),
                    ),
            )
            .child(
                div()
                    .id("new-canvas-message")
                    .test_support()
                    .text_color(p.muted)
                    .child(message),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("new-canvas-save-preset")
                            .label(t!("new_canvas.save_preset"))
                            .small()
                            .ghost()
                            .disabled(!valid)
                            .on_click(cx.listener(|this, _, _, cx| this.save_preset(cx))),
                    )
                    .child(
                        div().flex_1().text_color(p.muted).child(
                            size.map(|(w, h)| format!("{w} × {h} px"))
                                .unwrap_or_default(),
                        ),
                    )
                    .child(
                        Button::new("new-canvas-cancel")
                            .label(t!("new_canvas.cancel"))
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("new-canvas-create")
                            .label(t!("new_canvas.create"))
                            .small()
                            .primary()
                            .disabled(!valid)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.submit(window, cx) {
                                    window.close_dialog(cx);
                                }
                            })),
                    ),
            ).into_any_element()
    }
}

impl Workspace {
    pub(super) fn open_new_canvas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self
            .destination(cx)
            .and_then(super::destinations::Destination::canvas)
            .unwrap_or(CanvasKind::Photo);
        self.open_new_canvas_kind(kind, window, cx);
    }
    pub(crate) fn open_new_canvas_kind(
        &mut self,
        kind: CanvasKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_style_dialog(window, cx);
        self.ensure_home_projects(cx);
        let workspace = cx.weak_entity();
        let folder = self.home_state.projects.folder;
        let view = cx.new(|cx| {
            let mut view = NewCanvas::new(workspace, folder, window, cx);
            if kind != CanvasKind::Photo {
                view.pick_kind(kind, window, cx);
            }
            view
        });
        window.open_dialog(cx, move |dialog, window, _| {
            let submit = view.clone();
            let cancel = view.clone();
            let close = view.clone();
            dialog
                .title(t!("new_canvas.title"))
                .width(px(880.).min(window.viewport_size().width - px(32.)))
                .overlay_closable(false)
                .footer(div())
                .child(view.clone())
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |view, _| view.cancelled = true);
                    true
                })
                .on_close(move |_, _, cx| close.update(cx, |view, _| view.cancelled = true))
                .on_ok(move |_, window, cx| submit.update(cx, |view, cx| view.submit(window, cx)))
        });
    }
}
