//! Page navigation keeps the same native tools and owns per-page view state.
use super::*;
use emulsion_core::project::{PageId, ProjectEditor, ProjectStamp};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};

pub(crate) struct PagesUi {
    seen_page: PageId,
    views: HashMap<PageId, View>,
    thumbs: HashMap<PageId, (u64, Arc<RenderImage>)>,
    loading: HashMap<PageId, u64>,
    pub(crate) recovery_stamp: Option<ProjectStamp>,
    include_bleed: bool,
}
impl Default for PagesUi {
    fn default() -> Self {
        Self {
            seen_page: 1,
            views: HashMap::new(),
            thumbs: HashMap::new(),
            loading: HashMap::new(),
            recovery_stamp: None,
            include_bleed: false,
        }
    }
}

impl EditorView {
    fn export_project_pages(
        &mut self,
        format: emulsion_io::project_export::Format,
        all: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(project) = self.editor.snapshot() else {
            return;
        };
        let selected = if all {
            project.pages.iter().map(|page| page.meta.id).collect()
        } else {
            vec![project.active]
        };
        let bleed = self.pages_ui.include_bleed;
        let dir = self
            .editor
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| ".".into())
            });
        let extension = if format == emulsion_io::project_export::Format::Pdf {
            "pdf"
        } else {
            "zip"
        };
        let name = format!("{}-pages.{extension}", self.name);
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this,cx| {
            let Ok(Ok(Some(mut path)))=rx.await else{return;};path.set_extension(extension);
            this.update(cx,|this,cx|this.set_status("Exporting project pages…",false,cx)).ok();
            let output=path.clone();
            let result=cx.background_spawn(async move{emulsion_io::project_export::write(&project,&selected,format,bleed,&output)}).await;
            this.update(cx,|this,cx|match result {
                Ok(report)=>this.set_status(format!("Exported {} page(s) to {}.{}",report.pages,path.display(),if report.rasterized_pages.is_empty(){String::new()}else{format!(" {} page(s) use rendered images for effects unsupported by vector export; the project remains editable.",report.rasterized_pages.len())}),false,cx),
                Err(error)=>this.set_status(format!("Export failed: {error}"),true,cx),
            }).ok();
        }).detach();
    }

    fn project_export_button(&self, cx: &Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        let include_bleed = self.pages_ui.include_bleed;
        Button::new("project-export-pages")
            .label("Export ▾")
            .small()
            .outline()
            .dropdown_menu(move |mut menu, _, _| {
                let bleed_owner = owner.clone();
                menu = menu
                    .item(
                        PopupMenuItem::new("Include page bleed")
                            .checked(include_bleed)
                            .on_click(move |_, _, cx| {
                                bleed_owner
                                    .update(cx, |this, cx| {
                                        this.pages_ui.include_bleed = !include_bleed;
                                        cx.notify();
                                    })
                                    .ok();
                            }),
                    )
                    .separator();
                for format in emulsion_io::project_export::Format::ALL {
                    for all in [true, false] {
                        let owner = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(format!(
                                "{} · {}",
                                format.label(),
                                if all { "all pages" } else { "current page" }
                            ))
                            .on_click(move |_, _, cx| {
                                owner
                                    .update(cx, |this, cx| {
                                        this.export_project_pages(format, all, cx)
                                    })
                                    .ok();
                            }),
                        );
                    }
                }
                menu
            })
            .into_any_element()
    }
    pub(crate) fn install_project_session(
        &mut self,
        session: ProjectEditor,
        cx: &mut Context<Self>,
    ) {
        self.editor = session;
        self.pages_ui.seen_page = 0;
        self.after_change(cx);
        if self.is_design() || self.is_diagram() {
            self.set_tool(Tool::Move, cx);
        }
    }

    /// Revisions and node IDs are page-local. Never allow caches, selections or
    /// delayed work from a previous page to match the same IDs on another page.
    pub(super) fn sync_page_view(&mut self, cx: &mut Context<Self>) {
        let id = self.editor.active_page();
        if id == self.pages_ui.seen_page {
            return;
        }
        self.pages_ui
            .views
            .insert(self.pages_ui.seen_page, self.view);
        self.pages_ui.seen_page = id;
        self.stop_motion(cx);
        self.diagram_cancel_connection();
        self.invalidate_pending_edits();
        self.render_epoch = self.render_epoch.wrapping_add(1);
        if let Some(cancelled) = self.tile_cancel.take() {
            cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.tile_task = None;
        self.tree_request = self.tree_request.wrapping_add(1);
        self.tree_building = None;
        self.tree_dirty = emulsion_core::Dirty::All;
        self.seen_rev = u64::MAX;
        self.seen_commit = u64::MAX;
        self.before_tree = None;
        self.tree = Arc::new(emulsion_raster::composite::CompositeTree {
            width: self.editor.doc.width,
            height: self.editor.doc.height,
            space: self.editor.doc.blend_space,
            nodes: Vec::new(),
        });
        self.cache.borrow_mut().clear();
        *self.gpu_canvas.borrow_mut() = Default::default();
        self.thumbs.clear();
        self.reset_page_history_view();
        self.set_layer_selection(Vec::new(), None);
        self.drag = None;
        self.warp = None;
        self.tools.transform_lift = None;
        self.tools.crop = None;
        self.tools.polygon.clear();
        self.tools.magnetic_live.clear();
        self.pen_cancel();
        self.compare = 0.;
        self.anim = Default::default();
        self.raw = Default::default();
        self.channels = Default::default();
        self.suggest_rev = u64::MAX;
        if let Some(view) = self.pages_ui.views.get(&id) {
            self.view = *view;
            self.fit_pending = false;
        } else {
            self.view = View::default();
            self.fit_pending = true;
        }
        self.notify_canvas(cx);
    }

    pub(super) fn prepare_page_action(&mut self, cx: &mut Context<Self>) -> bool {
        if self.styles_ui.dialog_for.is_some() || self.raw.is_pending() || self.assistant.running {
            self.set_status(
                "Finish the current dialog, RAW development, or assistant operation first.",
                false,
                cx,
            );
            return false;
        }
        self.stop_motion(cx);
        self.finish_pointer_gesture(cx);
        self.close_text_field(cx);
        self.finish_shape_color_edit(cx);
        true
    }

    pub(crate) fn select_page(&mut self, id: PageId, cx: &mut Context<Self>) {
        if id == self.editor.active_page() || !self.prepare_page_action(cx) {
            return;
        }
        match self.editor.set_active_page(id) {
            Ok(()) => self.after_change(cx),
            Err(error) => self.set_status(error, true, cx),
        }
    }

    pub(crate) fn add_project_page(&mut self, duplicate: bool, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let id = self.editor.active_page();
        let result = if duplicate {
            self.editor.duplicate_page(id)
        } else {
            let current = &self.editor.doc;
            let spec = emulsion_core::creation::CanvasSpec {
                width: current.width as f64,
                height: current.height as f64,
                resolution: current.resolution as f64,
                depth: current.source_depth,
                ..Default::default()
            };
            let bleed = self
                .editor
                .page_list()
                .iter()
                .find(|p| p.id == id)
                .map_or(0., |p| p.bleed_mm);
            spec.create().and_then(|doc| {
                self.editor.add_page(
                    doc,
                    format!("Page {}", self.editor.page_list().len() + 1),
                    bleed,
                )
            })
        };
        match result {
            Ok(_) => self.after_change(cx),
            Err(error) => self.set_status(error, true, cx),
        }
    }

    pub(crate) fn delete_project_page(&mut self, id: PageId, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        match self.editor.remove_page(id) {
            Ok(()) => self.after_change(cx),
            Err(error) => self.set_status(error, true, cx),
        }
    }

    fn move_project_page(&mut self, id: PageId, to: usize, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        match self.editor.move_page(id, to) {
            Ok(()) => self.after_change(cx),
            Err(error) => self.set_status(error, true, cx),
        }
    }

    fn page_properties(&mut self, id: PageId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(meta) = self.editor.page_list().iter().find(|p| p.id == id).cloned() else {
            return;
        };
        let name = cx.new(|cx| InputState::new(window, cx).default_value(meta.name));
        let bleed =
            cx.new(|cx| InputState::new(window, cx).default_value(meta.bleed_mm.to_string()));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let name_input = name.clone();
            let bleed_input = bleed.clone();
            let owner = owner.clone();
            dialog
                .title("Page properties")
                .width(px(360.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Page name")
                        .child(Input::new(&name))
                        .child("Bleed · mm")
                        .child(Input::new(&bleed)),
                )
                .footer(crate::widgets::form_dialog_footer("Save changes"))
                .on_ok(move |_, _, cx| {
                    let name = name_input.read(cx).value().to_string();
                    let bleed = bleed_input
                        .read(cx)
                        .value()
                        .parse::<f64>()
                        .unwrap_or(f64::NAN);
                    owner
                        .update(cx, |this, cx| {
                            match this.editor.rename_page(id, name, bleed) {
                                Ok(()) => {
                                    this.after_change(cx);
                                    true
                                }
                                Err(error) => {
                                    this.set_status(error, true, cx);
                                    false
                                }
                            }
                        })
                        .unwrap_or(false)
                })
        });
    }

    fn page_thumbnail(&mut self, id: PageId, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        let editor = self.editor.page(id)?;
        let revision = editor.revision;
        if let Some((rev, image)) = self.pages_ui.thumbs.get(&id)
            && *rev == revision
        {
            return Some(image.clone());
        }
        if !self.pages_ui.loading.contains_key(&id) && self.pages_ui.loading.len() < 4 {
            let doc = editor.doc.clone();
            self.pages_ui.loading.insert(id, revision);
            cx.spawn(async move |this, cx| {
                let (w, h, bytes) = cx
                    .background_spawn(async move { super::history::doc_thumb(&doc, 96) })
                    .await;
                this.update(cx, |this, cx| {
                    this.pages_ui.loading.remove(&id);
                    if this.editor.page(id).is_some_and(|p| p.revision == revision) {
                        this.pages_ui
                            .thumbs
                            .insert(id, (revision, Arc::new(viewport::bgra_image(w, h, bytes))));
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        self.pages_ui
            .thumbs
            .get(&id)
            .map(|(_, image)| image.clone())
    }

    pub(super) fn project_page_strip(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let kind = self.editor.kind()?;
        let pages = self.editor.page_list().to_vec();
        let active = self.editor.active_page();
        let count = pages.len();
        let mut row = div()
            .id("project-pages-scroll")
            .flex()
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .gap_2()
            .p_2();
        for (index, meta) in pages.into_iter().enumerate() {
            let id = meta.id;
            let image = self.page_thumbnail(id, cx);
            let owner = cx.weak_entity();
            row = row.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .w(px(78.))
                    .flex_none()
                    .child(
                        div()
                            .id(("project-page", id))
                            .test_support()
                            .h(px(52.))
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(p.stage)
                            .rounded(px(4.))
                            .border_1()
                            .border_color(if id == active { p.accent } else { p.line })
                            .cursor_pointer()
                            .children(
                                image.map(|image| {
                                    img(image).size_full().object_fit(ObjectFit::Contain)
                                }),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.select_page(id, cx))),
                    )
                    .child(
                        Button::new(("project-page-menu", id))
                            .label(format!("{}. {}", index + 1, meta.name))
                            .xsmall()
                            .ghost()
                            .dropdown_menu(move |menu, _, _| {
                                let select = owner.clone();
                                let duplicate = owner.clone();
                                let remove = owner.clone();
                                let left = owner.clone();
                                let right = owner.clone();
                                let properties = owner.clone();
                                menu.item(PopupMenuItem::new("Open page").on_click(
                                    move |_, _, cx| {
                                        select.update(cx, |e, cx| e.select_page(id, cx)).ok();
                                    },
                                ))
                                .item(PopupMenuItem::new("Duplicate page").on_click(
                                    move |_, _, cx| {
                                        duplicate
                                            .update(cx, |e, cx| {
                                                e.select_page(id, cx);
                                                e.add_project_page(true, cx);
                                            })
                                            .ok();
                                    },
                                ))
                                .item(PopupMenuItem::new("Page name and bleed…").on_click(
                                    move |_, window, cx| {
                                        properties
                                            .update(cx, |e, cx| e.page_properties(id, window, cx))
                                            .ok();
                                    },
                                ))
                                .item(
                                    PopupMenuItem::new("Move left")
                                        .disabled(index == 0)
                                        .on_click(move |_, _, cx| {
                                            left.update(cx, |e, cx| {
                                                e.move_project_page(id, index.saturating_sub(1), cx)
                                            })
                                            .ok();
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new("Move right")
                                        .disabled(index + 1 == count)
                                        .on_click(move |_, _, cx| {
                                            right
                                                .update(cx, |e, cx| {
                                                    e.move_project_page(id, index + 1, cx)
                                                })
                                                .ok();
                                        }),
                                )
                                .separator()
                                .item(
                                    PopupMenuItem::new("Delete page")
                                        .disabled(count == 1)
                                        .on_click(move |_, _, cx| {
                                            remove
                                                .update(cx, |e, cx| e.delete_project_page(id, cx))
                                                .ok();
                                        }),
                                )
                            }),
                    ),
            );
        }
        Some(
            div()
                .id("project-page-strip")
                .test_support()
                .flex()
                .items_center()
                .h(px(88.))
                .flex_none()
                .gap_2()
                .bg(p.panel)
                .border_t_1()
                .border_color(p.line)
                .child(row)
                .child(self.project_export_button(cx))
                .child(
                    Button::new("project-page-add")
                        .label("+")
                        .tooltip("Add page")
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| this.add_project_page(false, cx))),
                )
                .child(
                    Button::new("project-undo")
                        .label("↶")
                        .tooltip("Undo across pages")
                        .small()
                        .ghost()
                        .disabled(!self.editor.can_undo())
                        .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
                )
                .child(
                    Button::new("project-redo")
                        .label("↷")
                        .tooltip("Redo across pages")
                        .small()
                        .ghost()
                        .disabled(!self.editor.can_redo())
                        .on_click(cx.listener(|this, _, _, cx| this.redo(cx))),
                )
                .child(
                    div()
                        .pr_3()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(format!("{} · {} pages", kind.label(), count)),
                )
                .into_any_element(),
        )
    }
}
