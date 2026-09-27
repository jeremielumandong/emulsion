//! Design's native asset drawer uses the same editable objects and canvas tools.
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
    fn label(self) -> &'static str {
        match self {
            Self::Templates => "Design",
            Self::Elements => "Elements",
            Self::Text => "Text",
            Self::Uploads => "Uploads",
            Self::Tools => "Tools",
            Self::Frames => "Frames",
            Self::Brand => "Brand",
            Self::Photos => "Photos",
            Self::Magic => "Magic",
            Self::Motion => "Motion",
            Self::Position => "Position",
        }
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
pub(super) struct DesignUi {
    section: Section,
    open: bool,
    search: Option<Entity<InputState>>,
    subscription: Option<Subscription>,
    template_size: Option<(u32, u32)>,
    preview_size: Option<(u32, u32)>,
    previews: HashMap<usize, Arc<RenderImage>>,
}
impl Default for DesignUi {
    fn default() -> Self {
        Self {
            section: Section::Templates,
            open: true,
            search: None,
            subscription: None,
            template_size: None,
            preview_size: None,
            previews: HashMap::new(),
        }
    }
}

impl EditorView {
    pub(super) fn show_design_section(&mut self, section: Section, cx: &mut Context<Self>) {
        self.design_ui.section = section;
        self.design_ui.open = true;
        cx.notify();
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
            self.set_tool(Tool::Type, cx);
        }
    }
    pub(crate) fn add_design_template(&mut self, template: Template, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let size = self
            .design_ui
            .template_size
            .unwrap_or((self.editor.doc.width, self.editor.doc.height));
        let result = template
            .create(size.0, size.1)
            .and_then(|doc| self.editor.add_page(doc, template.label().into(), 0.));
        match result {
            Ok(_) => {
                self.after_change(cx);
                self.set_tool(Tool::Move, cx);
            }
            Err(error) => self.set_status(error, true, cx),
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

    fn load_design_previews(&mut self, cx: &mut Context<Self>) {
        let size = self
            .design_ui
            .template_size
            .unwrap_or((self.editor.doc.width, self.editor.doc.height));
        if self.design_ui.preview_size == Some(size) {
            return;
        }
        self.design_ui.preview_size = Some(size);
        self.design_ui.previews.clear();
        cx.spawn(async move |this, cx| {
            let previews = cx
                .background_spawn(async move {
                    let scale = 384. / f64::from(size.0.max(size.1));
                    let preview_size = (
                        (f64::from(size.0) * scale).round().max(1.) as u32,
                        (f64::from(size.1) * scale).round().max(1.) as u32,
                    );
                    Template::ALL
                        .into_iter()
                        .enumerate()
                        .filter_map(|(i, template)| {
                            let doc = template.create(preview_size.0, preview_size.1).ok()?;
                            let (w, h, bytes) = super::history::doc_thumb(&doc, 216);
                            Some((i, Arc::new(viewport::bgra_image(w, h, bytes))))
                        })
                        .collect::<HashMap<_, _>>()
                })
                .await;
            this.update(cx, |this, cx| {
                if this.design_ui.preview_size == Some(size) {
                    this.design_ui.previews = previews;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn choose_design_asset(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Place local images or vector files".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            this.update(cx, |this, cx| this.place_design_assets(paths, cx))
                .ok();
        })
        .detach();
    }

    pub(super) fn place_design_assets(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        if paths.len() > 100 {
            self.set_status("Place up to 100 files at a time.", true, cx);
            return;
        }
        let ticket = self.edit_ticket();
        let page = self.editor.active_page();
        let size = (self.editor.doc.width, self.editor.doc.height);
        self.set_status("Loading local assets…", false, cx);
        cx.spawn(async move |this,cx| {
            let loaded=cx.background_spawn(async move {
                paths.into_iter().map(|path| {
                    let result=emulsion_io::open_full(&path).and_then(|opened| {
                        if opened.history_error.is_some() { return Err(emulsion_io::IoError::Manifest("This file's history is damaged; open it directly to review it.".into())); }
                        let mut doc=opened.doc;
                        let scale=(size.0 as f64*0.8/doc.width as f64).min(size.1 as f64*0.8/doc.height as f64).min(1.);
                        if scale<1. { let w=(doc.width as f64*scale).round().max(1.) as u32;let h=(doc.height as f64*scale).round().max(1.) as u32; emulsion_core::geometry::resize(&mut doc,w,h); }
                        let roots=doc.children(None);
                        let fragment=Fragment::capture(&doc,&roots).map_err(emulsion_io::IoError::Manifest)?;
                        let rasterized_svg=emulsion_io::is_svg(&path)&&doc.nodes.iter().any(|n|matches!(n.kind,NodeKind::Raster{..}));
                        Ok((fragment,((size.0 as f64-doc.width as f64)/2.,(size.1 as f64-doc.height as f64)/2.),rasterized_svg))
                    });
                    (path,result)
                }).collect::<Vec<_>>()
            }).await;
            this.update(cx,|this,cx| {
                if this.edit_ticket()!=ticket || this.editor.active_page()!=page {
                    this.set_status("The page changed while assets loaded. Place the files again on the intended page.",false,cx); return;
                }
                let mut notes=Vec::new(); let mut count=0;
                for (path,result) in loaded {
                    match result {
                        Ok((fragment,offset,rasterized_svg))=>match fragment.paste(&mut this.editor,Slot::TOP,offset) {
                            Ok(ids)=>{this.set_layer_selection(ids.clone(),ids.last().copied());count+=1;
                                this.note_creative_asset(path.clone(),emulsion_io::creative_library::AssetKind::Image,cx);
                                if rasterized_svg {notes.push(format!("{} uses SVG features without editable equivalents and was placed as an image.",path.display()));}
                            }, Err(error)=>notes.push(error),
                        }, Err(error)=>notes.push(format!("{}: {error}",path.display())),
                    }
                }
                this.after_change(cx);this.set_tool(Tool::Move,cx);
                this.set_status(format!("Placed {count} local asset(s). {}",notes.join(" ")),!notes.is_empty(),cx);
            }).ok();
        }).detach();
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
    fn choose_frame_image(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self
            .selected
            .filter(|id| emulsion_core::design::frame_parts(&self.editor.doc, *id).is_some())
        else {
            self.set_status("Select a frame or a vector shape first.", false, cx);
            return;
        };
        let ticket = self.edit_ticket();
        let page = self.editor.active_page();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a frame image".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move {
                    emulsion_io::import::decode(&path).map(|decoded| Arc::new(decoded.raster))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.edit_ticket() != ticket || this.editor.active_page() != page {
                    this.set_status(
                        "The page changed while the image loaded. Select the frame and try again.",
                        false,
                        cx,
                    );
                    return;
                }
                match result.map_err(|e| e.to_string()).and_then(|raster| {
                    emulsion_core::design::place_in_frame(&mut this.editor, id, raster)
                }) {
                    Ok(_) => {
                        this.after_change(cx);
                        this.set_status(
                            "Frame image placed. Select its image layer to adjust the crop.",
                            false,
                            cx,
                        );
                    }
                    Err(error) => this.set_status(error, true, cx),
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
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search this library"));
            self.design_ui.subscription = Some(cx.subscribe(&input, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
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
                            .child(rail::tool_icon(s.icon()).size(px(15.)))
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
                            .accessibility_label("Collapse library")
                            .tooltip("Collapse library")
                            .child(rail::tool_icon("chevrons-left").size(px(13.)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.design_ui.open = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div().px(px(10.)).pt(px(8.)).child(
                    Styled::h(Input::new(&search).small(), px(26.))
                        .text_size(px(11.))
                        .prefix(rail::tool_icon("search").size(px(11.))),
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
            .overflow_y_scroll();
        match section {
            Section::Templates => {
                self.load_design_previews(cx);
                content = content.child(
                    div().flex().flex_wrap().gap(px(4.)).children(
                        [
                            ("Instagram", (1080, 1080)),
                            ("Story", (1080, 1920)),
                            ("Poster", (1587, 2245)),
                            ("Presentation", (1920, 1080)),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(i, (label, size))| {
                            Button::new(("design-format", i))
                                .accessibility_label(label)
                                .child(div().text_size(px(10.5)).child(label))
                                .xsmall()
                                .outline()
                                .h(px(22.))
                                .rounded_full()
                                .selected(self.design_ui.template_size == Some(size))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.design_ui.template_size = Some(size);
                                    cx.notify();
                                }))
                        }),
                    ),
                );
                let mut grid = div()
                    .id("design-template-grid")
                    .test_support()
                    .grid()
                    .grid_cols(2)
                    .gap(px(6.));
                for (i, t) in Template::ALL
                    .into_iter()
                    .enumerate()
                    .filter(|(_, t)| t.label().to_lowercase().contains(&query))
                {
                    let preview = self.design_ui.previews.get(&i).cloned();
                    grid = grid.child(
                        Button::new(("design-template", i))
                            .accessibility_label(t.label())
                            .tooltip(t.label())
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
                                            .w_full()
                                            .overflow_hidden()
                                            .bg(p.soft_bg)
                                            .when_some(preview, |tile, preview| {
                                                tile.child(
                                                    img(preview)
                                                        .size_full()
                                                        .object_fit(ObjectFit::Contain),
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
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.add_design_template(t, cx)),
                            ),
                    );
                }
                content = content
                    .child(grid)
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child("Each template adds an editable page."),
                    )
                    .child(self.creative_pack_controls(cx))
                    .child(
                        Button::new("design-save-template")
                            .label("Save page as template…")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_local_template(window, cx)
                            })),
                    )
                    .child(
                        Button::new("design-import-template")
                            .label("Import local template…")
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
                content = content.child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            Button::new("design-open-frames")
                                .label("Frames")
                                .xsmall()
                                .outline()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_design_section(Section::Frames, cx)
                                })),
                        )
                        .child(
                            Button::new("design-open-tools")
                                .label("All tools")
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
                    .filter(|(_, e)| e.label().to_lowercase().contains(&query))
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
                                .accessibility_label(e.label())
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
                                                .child(e.label()),
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
                        .label("+ Add a text box")
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
                    .filter(|(_, t)| t.label().to_lowercase().contains(&query))
                {
                    content = content.child(
                        Button::new(("design-text", i))
                            .accessibility_label(t.label())
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
                                    .child(t.label()),
                            )
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.insert_design_text(t, cx)),
                            ),
                    );
                }
                let combinations = [
                    ("Geist", "Geist Mono"),
                    ("Bold", "Geist"),
                    ("Geist Mono", "Geist"),
                    ("Italic", "Geist"),
                ];
                let cards = combinations
                    .into_iter()
                    .enumerate()
                    .map(|(i, (title, body))| {
                        Button::new(("design-type-pair", i))
                            .outline()
                            .h(px(80.))
                            .w_full()
                            .bg(p.soft_bg)
                            .accessibility_label(format!("Add {title} and {body} font combination"))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(if i == 3 {
                                                FontWeight::NORMAL
                                            } else {
                                                FontWeight::SEMIBOLD
                                            })
                                            .when(i == 3, |heading| heading.italic())
                                            .font_family(if i == 2 {
                                                MONO_FONT
                                            } else {
                                                theme::UI_FONT
                                            })
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.5))
                                            .text_color(p.muted)
                                            .font_family(if i == 0 {
                                                MONO_FONT
                                            } else {
                                                theme::UI_FONT
                                            })
                                            .child(body),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.insert_font_combination(i, cx)
                            }))
                    });
                content = content
                    .child(div().pt_2().text_size(px(10.5)).text_color(p.muted).child("Font combinations"))
                    .child(div().grid().grid_cols(2).gap(px(6.)).children(cards))
                    .child(div().text_size(px(11.)).text_color(p.muted).child("Click text with the Type tool to edit. Character and paragraph controls remain in Properties."));
            }
            Section::Uploads | Section::Photos => {
                content = content.child(
                    Button::new("design-upload")
                        .label("Choose local files…")
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
                content=content.child(div().text_size(px(11.)).text_color(p.muted).child("Placed assets are embedded in the project. Photos, SVG, and native layers stay local."));
            }
            Section::Brand => {
                content = content.child(self.brand_drawer(&query, p, cx));
            }
            Section::Motion => {
                content = content.child(self.design_motion_controls(p, cx));
            }
            Section::Position => {
                content = content.child(self.design_position_controls(p, cx));
            }
            Section::Magic => {
                content=content.child(Button::new("design-open-assistant").label("Ask the assistant…").outline().on_click(cx.listener(|this,_,window,cx|this.open_ask(window,cx)))).child(div().text_size(px(11.)).text_color(p.muted).child("Use your configured provider and the existing preview and approval workflow."));
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
                .filter(|(_, e)| e.label().to_lowercase().contains(&query))
                {
                    content = content.child(
                        Button::new(("design-frame", i))
                            .label(format!("{} frame", e.label()))
                            .w_full()
                            .outline()
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.insert_design_frame(e, cx)),
                            ),
                    );
                }
                content=content.child(Button::new("design-frame-image").label("Place / replace image…").w_full().outline().on_click(cx.listener(|this,_,_,cx|this.choose_frame_image(cx))))
                    .child(Button::new("design-frame-crop").label("Edit image crop").w_full().ghost().on_click(cx.listener(|this,_,_,cx| {
                        if let Some(image)=this.selected.and_then(|id|emulsion_core::design::frame_parts(&this.editor.doc,id)).and_then(|(_,image)|image) {
                            this.set_layer_selection(vec![image],Some(image));this.set_tool(Tool::Move,cx);
                        } else {this.set_status("Select a frame containing an image first.",false,cx);}
                    })))
                    .child(div().text_size(px(11.)).text_color(p.muted).child("Frames keep their original image pixels. Move the group to move the frame; move or transform its image to adjust the crop."));
                content = content.child(self.design_frame_controls(p, cx));
            }
        }
        drawer = drawer.child(content).when(overlay, |d| {
            d.absolute()
                .left(px(68.))
                .top_0()
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
                .when(open, |row| row.child(drawer))
                .into_any_element(),
        )
    }
}
