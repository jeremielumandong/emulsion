//! Design's native asset drawer uses the same editable objects and canvas tools.
#[cfg(test)]
#[path = "design_typography_ui_tests.rs"]
mod typography_tests;
use super::*;
use emulsion_core::{
    design::{Element, Template, TextPreset},
    fragment::Fragment,
    project::ProjectKind,
};
use gpui_kit::component::{
    Selectable, Sizable,
    button::{Button, ButtonVariants},
    input::InputEvent,
};
use std::path::PathBuf;

fn template_matches(template: Template, query: &str) -> bool {
    let category = template.category().map(|i| Template::CATEGORIES[i]);
    format!(
        "{} {} {}",
        template.label(),
        category.map_or("", |c| c.label),
        category.map_or("", |c| c.preset)
    )
    .to_lowercase()
    .contains(query)
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Section {
    #[default]
    Templates,
    Elements,
    Text,
    Uploads,
    Tools,
    Frames,
    Brand,
    Photos,
    Magic,
    Motion,
    Position,
}
impl Section {
    const ALL: [Self; 11] = [
        Self::Templates,
        Self::Elements,
        Self::Text,
        Self::Uploads,
        Self::Tools,
        Self::Frames,
        Self::Brand,
        Self::Photos,
        Self::Magic,
        Self::Motion,
        Self::Position,
    ];
    fn label(self) -> String {
        match self {
            Self::Templates => t!("shell.dest_design"),
            Self::Elements => t!("editor.design_ui.section_elements"),
            Self::Text => t!("design.direct.text"),
            Self::Uploads => t!("editor.design_ui.section_uploads"),
            Self::Tools => t!("editor.design_ui.section_tools"),
            Self::Frames => t!("editor.design_ui.section_frames"),
            Self::Brand => t!("editor.design_ui.section_brand"),
            Self::Photos => t!("editor.design_ui.section_photos"),
            Self::Magic => t!("editor.design_ui.section_magic"),
            Self::Motion => t!("editor.design_ui.section_motion"),
            Self::Position => t!("editor.design_ui.section_position"),
        }
        .into_owned()
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Templates => "layout-template",
            Self::Elements => "shapes",
            Self::Text => "type",
            Self::Uploads => "upload",
            Self::Tools => "pen-tool",
            Self::Frames => "square",
            Self::Brand => "palette",
            Self::Photos => "image",
            Self::Magic => "sparkles",
            Self::Motion => "play",
            Self::Position => "move",
        }
    }
}
/// Display name for a core element shape; `Element::label` stays the English
/// layer name written into documents.
pub(super) fn element_label(element: Element) -> String {
    match element {
        Element::Rectangle => t!("editor.design_ui.element_rectangle"),
        Element::Circle => t!("editor.design_ui.element_circle"),
        Element::Triangle => t!("editor.design_ui.element_triangle"),
        Element::Diamond => t!("editor.design_ui.element_diamond"),
        Element::Star => t!("editor.design_ui.element_star"),
        Element::Line => t!("editor.design_ui.element_line"),
        Element::Arrow => t!("editor.design_ui.element_arrow"),
        Element::Heart => t!("editor.design_ui.element_heart"),
    }
    .into_owned()
}

/// Display name for a text preset; `TextPreset::label` stays the English layer name.
fn text_preset_label(preset: TextPreset) -> String {
    match preset {
        TextPreset::Heading => t!("editor.design_ui.text_heading"),
        TextPreset::Subheading => t!("editor.design_ui.text_subheading"),
        TextPreset::Body => t!("editor.design_ui.text_body"),
        TextPreset::Caption => t!("editor.design_ui.text_caption"),
        TextPreset::Quote => t!("editor.design_ui.text_quote"),
    }
    .into_owned()
}

/// Display name for `Template::CATEGORIES[index]`, whose `label` stays English.
fn category_label(index: usize) -> String {
    const KEYS: [&str; 12] = [
        "new_canvas.template_category_instagram_post",
        "new_canvas.template_category_portrait_post",
        "new_canvas.template_category_your_story",
        "new_canvas.template_category_certificate_quote",
        "new_canvas.template_category_presentation",
        "new_canvas.template_category_business_card",
        "new_canvas.template_category_resume_flyer",
        "new_canvas.template_category_poster",
        "new_canvas.template_category_video_thumbnail",
        "new_canvas.template_category_banner",
        "new_canvas.template_category_invitation",
        "new_canvas.template_category_responsive_layouts",
    ];
    match (KEYS.get(index), Template::CATEGORIES.len() == KEYS.len()) {
        (Some(key), true) => t!(*key).into_owned(),
        _ => Template::CATEGORIES[index].label.to_string(),
    }
}

pub(super) struct DesignUi {
    pub(super) frame_crop: Option<super::design_asset_ui::FrameCrop>,
    pub(super) asset_job: Option<(u64, u64)>,
    pub(super) copied_appearance: Option<emulsion_core::design_appearance::Appearance>,
    section: Section,
    pub(super) inspector: bool,
    open: bool,
    search: Option<Entity<InputState>>,
    subscription: Option<Subscription>,
    template_size: Option<(u32, u32)>,
    template_category: Option<usize>,
    categories_open: bool,
    pub(super) scroll: ScrollHandle,
    preview_size: Option<(u32, u32)>,
    preview_loading: bool,
    preview_attempted: std::collections::HashSet<usize>,
    previews: HashMap<usize, Arc<RenderImage>>,
    pair_previews: HashMap<usize, Arc<RenderImage>>,
    pair_generation: Option<u64>,
    pair_loading: bool,
}
impl Default for DesignUi {
    fn default() -> Self {
        Self {
            frame_crop: None,
            asset_job: None,
            copied_appearance: None,
            section: Section::Templates,
            inspector: false,
            open: true,
            search: None,
            subscription: None,
            template_size: None,
            template_category: None,
            categories_open: false,
            scroll: ScrollHandle::new(),
            preview_size: None,
            preview_loading: false,
            preview_attempted: Default::default(),
            previews: HashMap::new(),
            pair_previews: HashMap::new(),
            pair_generation: None,
            pair_loading: false,
        }
    }
}

impl EditorView {
    pub(super) fn design_library_width(&self, window: &Window) -> Pixels {
        px(68.
            + if self.design_ui.open && window.viewport_size().width >= px(1100.) {
                250.
            } else {
                0.
            })
    }

    pub(super) fn show_design_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.design_ui.section = section;
        self.design_ui.open = true;
        cx.notify();
    }
    pub(super) fn design_full_tools(&self) -> bool {
        self.design_ui.section == Section::Tools && self.design_ui.open
    }
    pub(super) fn is_design(&self) -> bool {
        self.editor.kind() == Some(ProjectKind::Design)
    }

    pub(crate) fn insert_design_element(&mut self, element: Element, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let node = element.node(
            (self.editor.doc.width, self.editor.doc.height),
            [230, 103, 69, 255],
        );
        if let Some(id) = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            },
            cx,
        ) {
            self.set_layer_selection(vec![id], Some(id));
            self.set_tool(Tool::Move, cx);
        }
    }
    pub(crate) fn insert_design_text(&mut self, preset: TextPreset, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let node = preset.node(
            (self.editor.doc.width, self.editor.doc.height),
            "Geist",
            [28, 30, 36, 255],
        );
        if let Some(id) = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            },
            cx,
        ) {
            self.set_layer_selection(vec![id], Some(id));
            self.set_tool(Tool::Move, cx);
        }
    }

    fn insert_font_combination(&mut self, variant: usize, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(fragment) = emulsion_core::design::typography_pair(&self.editor.doc, variant)
        else {
            return;
        };
        match fragment.paste(&mut self.editor, Slot::TOP, (0., 0.)) {
            Ok(ids) => {
                let primary = ids.first().copied();
                self.set_layer_selection(ids, primary);
                self.after_change(cx);
                self.set_tool(Tool::Move, cx);
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }

    pub(super) fn release_pair_previews(&mut self, window: &mut Window) {
        for (_, image) in self.design_ui.pair_previews.drain() {
            let _ = window.drop_image(image);
        }
        self.design_ui.pair_generation = None;
    }

    fn load_pair_previews(&mut self, cx: &mut Context<Self>) {
        let generation = emulsion_core::text::font_generation();
        if self.design_ui.pair_loading || self.design_ui.pair_generation == Some(generation) {
            return;
        }
        self.design_ui.pair_loading = true;
        cx.spawn(async move |this, cx| {
            let previews = cx
                .background_spawn(async move {
                    emulsion_core::design::typography_pairs()
                        .into_iter()
                        .filter_map(|pair| {
                            let mut editor =
                                emulsion_core::Editor::new(Document::new(640, 280), None);
                            let fragment =
                                emulsion_core::design::typography_pair(&editor.doc, pair.index)?;
                            fragment.paste(&mut editor, Slot::TOP, (0., 0.)).ok()?;
                            let (w, h, bytes) = super::history::doc_thumb(&editor.doc, 480);
                            Some((pair.index, Arc::new(viewport::bgra_image(w, h, bytes))))
                        })
                        .collect()
                })
                .await;
            this.update(cx, |this, cx| {
                this.design_ui.pair_loading = false;
                if this.visible && emulsion_core::text::font_generation() == generation {
                    this.design_ui.pair_generation = Some(generation);
                    let old = std::mem::replace(&mut this.design_ui.pair_previews, previews);
                    cx.defer(move |cx| {
                        for (_, image) in old {
                            cx.drop_image(image, None);
                        }
                    });
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_design_previews(&mut self, query: &str, cx: &mut Context<Self>) {
        let size = self.design_ui.template_size.unwrap_or((0, 0));
        if self.design_ui.preview_size != Some(size) {
            self.design_ui.preview_size = Some(size);
            self.design_ui.previews.clear();
            self.design_ui.preview_attempted.clear();
        }
        if self.design_ui.preview_loading {
            return;
        }
        let category = self.design_ui.template_category;
        let categories_open = self.design_ui.categories_open;
        let first_row =
            ((-f32::from(self.design_ui.scroll.offset().y) - 100.).max(0.) / 114.) as usize;
        let skip = if categories_open {
            0
        } else {
            (first_row * 2).saturating_sub(4)
        };
        let batch: Vec<_> = Template::catalog()
            .chain(Template::ADDITIONAL)
            .enumerate()
            .filter(|(i, t)| {
                if categories_open {
                    t.category().is_some_and(|category| {
                        Template::catalog().position(|t| t.category() == Some(category)) == Some(*i)
                    })
                } else {
                    (category.is_none() || t.category() == category) && template_matches(*t, query)
                }
            })
            .skip(skip)
            .take(24)
            .filter(|(i, _)| !self.design_ui.preview_attempted.contains(i))
            .take(12)
            .collect();
        if batch.is_empty() {
            return;
        }
        self.design_ui
            .preview_attempted
            .extend(batch.iter().map(|(i, _)| *i));
        self.design_ui.preview_loading = true;
        cx.spawn(async move |this, cx| {
            let previews = cx
                .background_spawn(async move {
                    batch
                        .into_iter()
                        .filter_map(|(i, template)| {
                            let size = if size == (0, 0) {
                                template.native_size()
                            } else {
                                size
                            };
                            let scale = 768. / f64::from(size.0.max(size.1));
                            let preview_size = (
                                (f64::from(size.0) * scale).round().max(1.) as u32,
                                (f64::from(size.1) * scale).round().max(1.) as u32,
                            );
                            let build_size = if matches!(template, Template::Responsive(_)) {
                                size
                            } else {
                                preview_size
                            };
                            let doc = template.create(build_size.0, build_size.1).ok()?;
                            let (w, h, bytes) = super::history::doc_thumb(&doc, 216);
                            Some((i, Arc::new(viewport::bgra_image(w, h, bytes))))
                        })
                        .collect::<HashMap<_, _>>()
                })
                .await;
            this.update(cx, |this, cx| {
                this.design_ui.preview_loading = false;
                if this.design_ui.preview_size == Some(size) {
                    this.design_ui.previews.extend(previews);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn choose_design_asset(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let target = (self.selected_layer_ids().len() == 1)
            .then_some(self.selected)
            .flatten()
            .filter(|id| emulsion_core::design::frame_parts(&self.editor.doc, *id).is_some());
        let ticket = self.begin_design_asset_request();
        let page = self.editor.active_page();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(t!("editor.design_ui.place_prompt").into()),
        });
        cx.spawn(async move |this, cx| {
            let paths = rx.await;
            this.update(cx, |this, cx| {
                if !this.accept_design_asset_result(ticket, page, cx) {
                    return;
                }
                if let Ok(Ok(Some(paths))) = paths {
                    if paths.len() == 1 {
                        if let Some(id) = target {
                            this.place_design_asset_in_frame(paths[0].clone(), id, cx);
                        } else {
                            this.place_design_assets(paths, cx);
                        }
                    } else {
                        this.place_design_assets(paths, cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn place_design_assets(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.place_design_assets_at(paths, None, cx);
    }

    pub(super) fn place_design_assets_at(
        &mut self,
        paths: Vec<PathBuf>,
        center: Option<(f64, f64)>,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        if paths.len() > 100 {
            self.set_status(t!("editor.design_ui.place_limit"), true, cx);
            return;
        }
        let ticket = self.begin_design_asset_request();
        let page = self.editor.active_page();
        let size = (self.editor.doc.width, self.editor.doc.height);
        self.set_status(t!("editor.design_ui.loading_assets"), false, cx);
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_spawn(async move {
                    paths
                        .into_iter()
                        .map(|path| {
                            let result = emulsion_io::open_full(&path).and_then(|opened| {
                                if opened.history_error.is_some() {
                                    return Err(emulsion_io::IoError::Manifest(
                                        t!("editor.design_ui.history_damaged").into_owned(),
                                    ));
                                }
                                let mut doc = opened.doc;
                                let scale = (size.0 as f64 * 0.8 / doc.width as f64)
                                    .min(size.1 as f64 * 0.8 / doc.height as f64)
                                    .min(1.);
                                if scale < 1. {
                                    let w = (doc.width as f64 * scale).round().max(1.) as u32;
                                    let h = (doc.height as f64 * scale).round().max(1.) as u32;
                                    emulsion_core::geometry::resize(&mut doc, w, h);
                                }
                                let roots = doc.children(None);
                                let fragment = Fragment::capture(&doc, &roots)
                                    .map_err(emulsion_io::IoError::Manifest)?;
                                let rasterized_svg = emulsion_io::is_svg(&path)
                                    && doc
                                        .nodes
                                        .iter()
                                        .any(|n| matches!(n.kind, NodeKind::Raster { .. }));
                                let center =
                                    center.unwrap_or((size.0 as f64 / 2., size.1 as f64 / 2.));
                                Ok((
                                    fragment,
                                    (
                                        center.0 - doc.width as f64 / 2.,
                                        center.1 - doc.height as f64 / 2.,
                                    ),
                                    rasterized_svg,
                                ))
                            });
                            (path, result)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_design_asset_result(ticket, page, cx) {
                    return;
                }
                let mut notes = Vec::new();
                let mut count = 0;
                for (path, result) in loaded {
                    match result {
                        Ok((fragment, offset, rasterized_svg)) => {
                            match fragment.paste(&mut this.editor, Slot::TOP, offset) {
                                Ok(ids) => {
                                    this.set_layer_selection(ids.clone(), ids.last().copied());
                                    count += 1;
                                    this.note_creative_asset(
                                        path.clone(),
                                        emulsion_io::creative_library::AssetKind::Image,
                                        cx,
                                    );
                                    if rasterized_svg {
                                        notes.push(
                                            t!(
                                                "editor.design_ui.svg_rasterized",
                                                path = path.display()
                                            )
                                            .into_owned(),
                                        );
                                    }
                                }
                                Err(error) => notes.push(error),
                            }
                        }
                        Err(error) => notes.push(format!("{}: {error}", path.display())),
                    }
                }
                this.after_change(cx);
                this.set_tool(Tool::Move, cx);
                this.set_status(
                    t!(
                        "editor.design_ui.placed_assets",
                        count = count,
                        notes = notes.join(" ")
                    ),
                    !notes.is_empty(),
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    fn insert_design_frame(&mut self, element: Element, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let frame = emulsion_core::design::frame(&self.editor.doc, element);
        match frame.paste(&mut self.editor, Slot::TOP, (0., 0.)) {
            Ok(ids) => {
                self.set_layer_selection(ids.clone(), ids.last().copied());
                self.after_change(cx);
                self.set_tool(Tool::Move, cx);
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }
    pub(super) fn choose_frame_image(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self
            .selected
            .filter(|id| emulsion_core::design::frame_parts(&self.editor.doc, *id).is_some())
        else {
            self.set_status(t!("editor.design_ui.select_frame_first"), false, cx);
            return;
        };
        let ticket = self.begin_design_asset_request();
        let page = self.editor.active_page();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("editor.design_ui.choose_frame_image").into()),
        });
        cx.spawn(async move |this, cx| {
            let paths = rx.await;
            this.update(cx, |this, cx| {
                if !this.accept_design_asset_result(ticket, page, cx) {
                    return;
                }
                if let Ok(Ok(Some(paths))) = paths
                    && let Some(path) = paths.into_iter().next()
                {
                    this.place_design_asset_in_frame(path, id, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn design_drawer(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.is_design() {
            return None;
        }
        self.load_creative_library(cx);
        if self.design_ui.search.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder(t!("editor.design_ui.search_library"))
            });
            self.design_ui.subscription = Some(cx.subscribe(&input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.design_ui.scroll.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                }
            }));
            self.design_ui.search = Some(input);
        }
        let search = self.design_ui.search.as_ref().unwrap();
        let query = search.read(cx).value().to_lowercase();
        let search = search.clone();
        let section = self.design_ui.section;
        let open = self.design_ui.open;
        let overlay = window.viewport_size().width < px(1100.);
        let rail = div()
            .id("design-rail")
            .test_support()
            .w(px(68.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .py_2()
            .min_h_0()
            .overflow_y_scroll()
            .bg(p.panel)
            .border_r_1()
            .border_color(p.line)
            .children([0, 1, 2, 3, 7, 6, 8].into_iter().map(|i| {
                let s = Section::ALL[i];
                Button::new(("design-section", i))
                    .accessibility_label(s.label())
                    .w(px(56.))
                    .h(px(48.))
                    .xsmall()
                    .ghost()
                    .selected(section == s && open)
                    .text_color(if section == s && open { p.ink } else { p.muted })
                    .when(section == s && open, |button| button.bg(p.soft_bg))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(4.))
                            .child(rail::tool_icon(s.icon()).text_color(p.ink).size(px(15.)))
                            .child(div().text_size(px(9.5)).child(s.label())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.design_ui.open = !(this.design_ui.section == s && this.design_ui.open);
                        this.design_ui.section = s;
                        cx.notify();
                    }))
            }));
        let mut drawer = div()
            .id("design-drawer")
            .test_support()
            .w(px(250.))
            .flex_none()
            .flex()
            .flex_col()
            .bg(p.panel)
            .border_r_1()
            .border_color(p.line)
            .child(
                div()
                    .id("design-drawer-heading")
                    .test_support()
                    .flex()
                    .items_center()
                    .h(px(38.))
                    .flex_none()
                    .px(px(12.))
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(section.label()),
                    )
                    .child(
                        Button::new("design-drawer-close")
                            .ghost()
                            .xsmall()
                            .accessibility_label(t!("editor.design_ui.collapse_library"))
                            .tooltip(t!("editor.design_ui.collapse_library"))
                            .child(
                                rail::tool_icon("chevrons-left")
                                    .text_color(p.ink)
                                    .size(px(13.)),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.design_ui.open = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("design-library-search")
                    .test_support()
                    .px(px(10.))
                    .pt(px(8.))
                    .child(
                        Styled::h(Input::new(&search).small(), px(26.))
                            .text_size(px(11.))
                            .prefix(rail::tool_icon("search").text_color(p.ink).size(px(11.))),
                    ),
            );
        let mut content = div()
            .id("design-drawer-content")
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(10.))
            .flex_1()
            .min_h_0()
            .track_scroll(&self.design_ui.scroll)
            .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
            .overflow_y_scroll();
        match section {
            Section::Templates => {
                self.load_design_previews(&query, cx);
                content = content
                    .child(
                        Button::new("design-bulk-create")
                            .label(t!("editor.design_ui.bulk_create"))
                            .tooltip(t!("editor.design_ui.bulk_create_tip"))
                            .small()
                            .outline()
                            .w_full()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.design_bulk_dialog(None, window, cx)
                            })),
                    )
                    .child(
                        Button::new("design-data-bind")
                            .label(t!("editor.design_ui.bind_selected"))
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.design_data_binding_dialog(window, cx)
                            })),
                    );
                content = content.child(
                    Button::new("design-explore-templates")
                        .label(t!("editor.design_ui.explore_templates"))
                        .small()
                        .outline()
                        .w_full()
                        .selected(self.design_ui.categories_open)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.design_ui.categories_open = !this.design_ui.categories_open;
                            this.design_ui.scroll.set_offset(point(px(0.), px(0.)));
                            cx.notify();
                        })),
                );
                if self.design_ui.categories_open {
                    let mut categories = div()
                        .id("design-template-categories")
                        .test_support()
                        .grid()
                        .grid_cols(2)
                        .gap(px(8.));
                    for (index, category) in Template::CATEGORIES.iter().enumerate() {
                        if !format!(
                            "{} {} {}",
                            category.label,
                            category_label(index),
                            category.preset
                        )
                        .to_lowercase()
                        .contains(&query)
                        {
                            continue;
                        }
                        let preview = Template::catalog()
                            .position(|t| t.category() == Some(index))
                            .and_then(|i| self.design_ui.previews.get(&i))
                            .cloned();
                        categories = categories.child(
                            Button::new(("design-template-category", index))
                                .accessibility_label(t!(
                                    "editor.design_ui.category_count",
                                    name = category_label(index)
                                ))
                                .tooltip(t!(
                                    "editor.design_ui.category_count",
                                    name = category_label(index)
                                ))
                                .outline()
                                .p_0()
                                .w_full()
                                .h(px(96.))
                                .rounded(px(14.))
                                .child(
                                    div()
                                        .relative()
                                        .w_full()
                                        .h(px(94.))
                                        .rounded(px(13.))
                                        .overflow_hidden()
                                        .bg(rgb(category.tint))
                                        .child(
                                            div()
                                                .absolute()
                                                .right(px(10.))
                                                .bottom(px(-6.))
                                                .size(px(68.))
                                                .rounded(px(4.))
                                                .bg(rgb(category.back)),
                                        )
                                        .when_some(preview, |tile, preview| {
                                            tile.child(
                                                img(preview)
                                                    .absolute()
                                                    .right(px(-6.))
                                                    .bottom(px(-10.))
                                                    .size(px(76.))
                                                    .aspect_square()
                                                    .object_fit(ObjectFit::Contain),
                                            )
                                        })
                                        .child(
                                            div()
                                                .relative()
                                                .p(px(12.))
                                                .max_w(relative(0.85))
                                                .text_size(px(12.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(rgb(category.ink))
                                                .child(category_label(index)),
                                        ),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.design_ui.template_category =
                                        if this.design_ui.template_category == Some(index) {
                                            None
                                        } else {
                                            Some(index)
                                        };
                                    this.design_ui.categories_open = false;
                                    this.design_ui.scroll.set_offset(point(px(0.), px(0.)));
                                    if let Some(search) = &this.design_ui.search {
                                        search.update(cx, |s, cx| s.set_value("", window, cx));
                                    }
                                    cx.notify();
                                })),
                        );
                    }
                    content = content.child(categories);
                }
                content = content.child(
                    div().flex().flex_wrap().gap(px(4.)).children(
                        [
                            (SharedString::from("Instagram"), (1080, 1080)),
                            (t!("new_canvas.preset_story").into(), (1080, 1920)),
                            (t!("new_canvas.preset_poster").into(), (1587, 2245)),
                            (t!("new_canvas.category_presentation").into(), (1920, 1080)),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(i, (label, size))| {
                            Button::new(("design-format", i))
                                .accessibility_label(label.clone())
                                .child(div().text_size(px(10.5)).child(label))
                                .xsmall()
                                .outline()
                                .h(px(22.))
                                .rounded_full()
                                .selected(self.design_ui.template_size == Some(size))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.design_ui.template_size =
                                        if this.design_ui.template_size == Some(size) {
                                            None
                                        } else {
                                            Some(size)
                                        };
                                    cx.notify();
                                }))
                        }),
                    ),
                );
                let mut grid = div()
                    .id("design-template-grid")
                    .test_support()
                    .grid()
                    .flex_none()
                    .grid_cols(2)
                    .gap(px(6.));
                let category = self.design_ui.template_category;
                let templates: Vec<_> = Template::catalog()
                    .chain(Template::ADDITIONAL)
                    .enumerate()
                    .filter(|(_, t)| category.is_none() || t.category() == category)
                    .filter(|(_, t)| template_matches(*t, &query))
                    .collect();
                content = content.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .id("design-template-count")
                                .test_support()
                                .flex_1()
                                .text_size(px(11.))
                                .child(format!(
                                    "{} · {}",
                                    category.map_or_else(
                                        || t!("editor.design_ui.all_templates"),
                                        |i| category_label(i).into()
                                    ),
                                    templates.len()
                                )),
                        )
                        .when(category.is_some(), |row| {
                            row.child(
                                Button::new("design-template-all")
                                    .label(t!("editor.design_ui.all"))
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.design_ui.template_category = None;
                                        cx.notify();
                                    })),
                            )
                        }),
                );
                for (i, t) in templates {
                    let preview = self.design_ui.previews.get(&i).cloned();
                    grid = grid.child(
                        Button::new(("design-template", i))
                            .accessibility_label(t.label())
                            .tooltip(format!(
                                "{} · {} × {}",
                                t.label(),
                                t.native_size().0,
                                t.native_size().1
                            ))
                            .outline()
                            .w_full()
                            .h(px(108.))
                            .p_0()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .w_full()
                                    .min_w_0()
                                    .gap(px(5.))
                                    .child(
                                        div()
                                            .h(px(78.))
                                            .flex_none()
                                            .w_full()
                                            .overflow_hidden()
                                            .bg(p.soft_bg)
                                            .when_some(preview, |tile, preview| {
                                                tile.child(
                                                    img(preview)
                                                        .w_full()
                                                        .h(px(78.))
                                                        .aspect_ratio(112. / 78.)
                                                        .object_fit(ObjectFit::Contain)
                                                        .id(("design-template-preview", i))
                                                        .test_support(),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .text_size(px(9.5))
                                            .font_family(MONO_FONT)
                                            .overflow_hidden()
                                            .child(t.label()),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let size = this
                                    .design_ui
                                    .template_size
                                    .unwrap_or_else(|| t.native_size());
                                this.preview_design_template(t, size, window, cx)
                            })),
                    );
                }
                content = content
                    .when(!self.design_ui.categories_open, |content| {
                        content.child(grid)
                    })
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child(t!("editor.design_ui.preview_hint")),
                    )
                    .child(self.creative_pack_controls(cx))
                    .child(
                        Button::new("design-save-template")
                            .label(t!("editor.design_ui.save_template"))
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_local_template(window, cx)
                            })),
                    )
                    .child(
                        Button::new("design-import-template")
                            .label(t!("editor.design_ui.import_template"))
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.import_local_template(cx))),
                    )
                    .child(self.creative_asset_list(
                        emulsion_io::creative_library::AssetKind::Template,
                        &query,
                        p,
                        cx,
                    ));
            }
            Section::Elements => {
                content = content.child(self.design_video_controls(cx));
                content = content
                    .child(self.design_chart_controls(cx))
                    .child(self.design_component_controls(p, cx));
                content = content.child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            Button::new("design-open-frames")
                                .label(t!("editor.design_ui.section_frames"))
                                .xsmall()
                                .outline()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_design_section(Section::Frames, cx)
                                })),
                        )
                        .child(
                            Button::new("design-open-tools")
                                .label(t!("editor.design_ui.all_tools"))
                                .xsmall()
                                .outline()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_design_section(Section::Tools, cx)
                                })),
                        ),
                );
                let mut grid = div()
                    .id("design-element-grid")
                    .test_support()
                    .grid()
                    .grid_cols(2)
                    .gap(px(6.));
                for (i, e) in Element::ALL
                    .into_iter()
                    .enumerate()
                    .filter(|(_, e)| element_label(*e).to_lowercase().contains(&query))
                {
                    let drawing = format!(
                        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><path d='{}' fill='{}' stroke='currentColor' stroke-width='2'/></svg>",
                        e.path(8., 8., 48., 48.).to_svg(),
                        if e == Element::Line {
                            "none"
                        } else {
                            "currentColor"
                        }
                    );
                    grid =
                        grid.child(
                            Button::new(("design-element", i))
                                .accessibility_label(element_label(e))
                                .w_full()
                                .h(px(88.))
                                .outline()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            svg()
                                                .data(drawing.as_bytes())
                                                .size(px(48.))
                                                .text_color(p.ink),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(9.5))
                                                .font_family(MONO_FONT)
                                                .child(element_label(e)),
                                        ),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.insert_design_element(e, cx)
                                })),
                        );
                }
                content = content.child(grid);
                content = content.child(self.alignment_controls(p, cx));
            }
            Section::Text => {
                content = content.child(
                    Button::new("design-add-text")
                        .label(t!("editor.design_ui.add_text_box"))
                        .h(px(32.))
                        .w_full()
                        .primary()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.insert_design_text(TextPreset::Body, cx)
                        })),
                );
                for (i, t) in TextPreset::ALL
                    .into_iter()
                    .enumerate()
                    .filter(|(_, t)| text_preset_label(*t).to_lowercase().contains(&query))
                {
                    content = content.child(
                        Button::new(("design-text", i))
                            .accessibility_label(text_preset_label(t))
                            .px(px(12.))
                            .bg(p.soft_bg)
                            .h(px(match i {
                                0 => 44.,
                                1 => 38.,
                                _ => 32.,
                            }))
                            .w_full()
                            .outline()
                            .child(
                                div()
                                    .w_full()
                                    .font_weight(match i {
                                        0 => FontWeight::SEMIBOLD,
                                        1 => FontWeight::MEDIUM,
                                        _ => FontWeight::NORMAL,
                                    })
                                    .text_size(px(match i {
                                        0 => 22.,
                                        1 => 15.,
                                        _ => 12.,
                                    }))
                                    .child(text_preset_label(t)),
                            )
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.insert_design_text(t, cx)),
                            ),
                    );
                }
                self.load_pair_previews(cx);
                let pairs = emulsion_core::design::typography_pairs();
                let matches: Vec<_> = pairs
                    .into_iter()
                    .filter(|pair| pair.matches_query(&query))
                    .collect();
                let count = matches.len();
                let cards = matches.into_iter().map(|pair| {
                    let index = pair.index;
                    let preview = self.design_ui.pair_previews.get(&index).cloned();
                    Button::new(("design-type-pair", index))
                        .outline()
                        .h(px(150.))
                        .p_0()
                        .w_full()
                        .tooltip(pair.description)
                        .accessibility_label(t!(
                            "editor.design_ui.add_pair",
                            name = pair.name,
                            heading = pair.heading.font,
                            body = pair.body.font
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .w_full()
                                .min_w_0()
                                .items_start()
                                .child(
                                    div()
                                        .h(px(96.))
                                        .w_full()
                                        .bg(rgb(0xf7f5f0))
                                        .overflow_hidden()
                                        .when_some(preview, |d, image| {
                                            d.child(
                                                img(image)
                                                    .w_full()
                                                    .h(px(96.))
                                                    .object_fit(ObjectFit::Contain),
                                            )
                                        })
                                        .when(
                                            !self.design_ui.pair_previews.contains_key(&index),
                                            |d| {
                                                d.child(
                                                    div()
                                                        .p_2()
                                                        .text_size(px(12.))
                                                        .text_color(rgb(0x1c1e24))
                                                        .child(pair.heading_sample),
                                                )
                                            },
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_start()
                                        .px_2()
                                        .py_1()
                                        .gap(px(2.))
                                        .w_full()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(pair.name),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(9.5))
                                                .text_color(p.muted)
                                                .w_full()
                                                .truncate()
                                                .child(format!(
                                                    "{} / {}",
                                                    pair.heading.font, pair.body.font
                                                )),
                                        ),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.insert_font_combination(index, cx)
                        }))
                });
                content = content
                    .child(
                        div()
                            .pt_2()
                            .text_size(px(10.5))
                            .text_color(p.muted)
                            .child(t!("editor.design_ui.pair_count", count = count)),
                    )
                    .when(count == 0, |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(p.muted)
                                .child(t!("editor.design_ui.no_pairs")),
                        )
                    })
                    .child(div().flex().flex_col().gap(px(8.)).children(cards))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child(t!("editor.design_ui.pairs_note")),
                    );
            }
            Section::Uploads | Section::Photos => {
                content = content.child(
                    Button::new("design-upload")
                        .label(t!("editor.design_ui.choose_files"))
                        .w_full()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| this.choose_design_asset(cx))),
                );
                content = content.child(self.creative_asset_list(
                    emulsion_io::creative_library::AssetKind::Image,
                    &query,
                    p,
                    cx,
                ));
                content = content.child(
                    div()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(t!("editor.design_ui.uploads_note")),
                );
            }
            Section::Brand => {
                content = content
                    .child(self.design_saved_style_controls(&query, p, cx))
                    .child(self.brand_drawer(&query, p, cx));
            }
            Section::Motion => {
                content = content.child(self.design_motion_controls(p, cx));
            }
            Section::Position => {
                content = content.child(self.design_position_controls(p, cx));
            }
            Section::Magic => {
                content = content
                    .child(
                        Button::new("design-open-assistant")
                            .label(t!("editor.design_ui.ask_assistant"))
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| this.open_ask(window, cx))),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child(t!("editor.design_ui.assistant_note")),
                    );
            }
            Section::Tools => {
                content = content
                    .child(self.tool_rail(p, window, cx))
                    .child(self.alignment_controls(p, cx));
            }
            Section::Frames => {
                for (i, e) in [
                    Element::Rectangle,
                    Element::Circle,
                    Element::Diamond,
                    Element::Heart,
                ]
                .into_iter()
                .enumerate()
                .filter(|(_, e)| element_label(*e).to_lowercase().contains(&query))
                {
                    content = content.child(
                        Button::new(("design-frame", i))
                            .label(t!("editor.design_ui.shape_frame", shape = element_label(e)))
                            .w_full()
                            .outline()
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.insert_design_frame(e, cx)),
                            ),
                    );
                }
                content = content
                    .child(
                        Button::new("design-frame-image")
                            .label(t!("editor.design_ui.place_image"))
                            .w_full()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| this.choose_frame_image(cx))),
                    )
                    .child(
                        Button::new("design-frame-crop")
                            .label(t!("editor.design_ui.edit_crop"))
                            .w_full()
                            .ghost()
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.start_frame_crop(window, cx)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child(t!("editor.design_ui.frames_note")),
                    );
                content = content.child(self.design_frame_controls(p, cx));
            }
        }
        // A narrow library floats over the canvas, never over the persistent
        // page/object controls. Keep those controls clickable with the library
        // open instead of making beginners discover a collapse-first sequence.
        let header_height = px(38.)
            + if self.document_tabs.is_some() {
                px(38.)
            } else {
                px(0.)
            }
            + window.rem_size() * 2.25; // The direct-controls row uses h_9.
        drawer = drawer.child(content).when(overlay, |d| {
            d.absolute()
                .left(px(68.))
                .top(header_height)
                .bottom_0()
                .occlude()
                .shadow_lg()
        });
        Some(
            div()
                .relative()
                .flex_none()
                .flex()
                .min_h_0()
                .child(rail)
                .when(open, |row| {
                    row.child(if overlay {
                        deferred(drawer).with_priority(1).into_any_element()
                    } else {
                        drawer.into_any_element()
                    })
                })
                .into_any_element(),
        )
    }
}
