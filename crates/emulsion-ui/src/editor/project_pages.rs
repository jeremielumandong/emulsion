//! Page navigation keeps the same native tools and owns per-page view state.
use super::*;
use crate::file_prompt::FilePrompts;
use emulsion_core::project::{PageId, ProjectEditor, ProjectStamp};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};

pub(crate) struct PagesUi {
    pub(super) organizer: super::page_organizer::PageOrganizerUi,
    seen_page: PageId,
    strip_scroll: ScrollHandle,
    views: HashMap<PageId, View>,
    /// Thumbnails by page and longest side, so the strip and the Board
    /// keep their own sizes.
    thumbs: HashMap<(PageId, u32), (u64, Arc<RenderImage>)>,
    loading: HashMap<(PageId, u32), u64>,
    thumbnail_epoch: u64,
    thumbnail_order: std::collections::VecDeque<(PageId, u32)>,
    retired_thumbnails: Vec<Arc<RenderImage>>,
    pub(crate) recovery_stamp: Option<ProjectStamp>,
    pub(super) include_bleed: bool,
    pub(super) export_pending: bool,
    pub(crate) board: super::storyboard_board::BoardUi,
}
impl Default for PagesUi {
    fn default() -> Self {
        Self {
            organizer: Default::default(),
            seen_page: 1,
            strip_scroll: ScrollHandle::new(),
            views: HashMap::new(),
            thumbs: HashMap::new(),
            loading: HashMap::new(),
            thumbnail_epoch: 0,
            thumbnail_order: Default::default(),
            retired_thumbnails: Vec::new(),
            recovery_stamp: None,
            include_bleed: false,
            board: Default::default(),
            export_pending: false,
        }
    }
}

impl EditorView {
    pub(super) fn export_project_pages(
        &mut self,
        format: emulsion_io::project_export::Format,
        all: bool,
        cx: &mut Context<Self>,
    ) {
        let selected = if all {
            self.editor.page_list().iter().map(|page| page.id).collect()
        } else {
            vec![self.editor.active_page()]
        };
        self.export_project_selection(format, selected, cx);
    }
    pub(super) fn export_project_selection(
        &mut self,
        format: emulsion_io::project_export::Format,
        selected: Vec<PageId>,
        cx: &mut Context<Self>,
    ) {
        if self.pages_ui.export_pending {
            self.set_status(t!("editor.project_pages.export_busy"), false, cx);
            return;
        }
        if !self.prepare_page_action(cx) {
            return;
        }
        let selected = match self.editor.ordered_page_selection(&selected) {
            Ok(ids) => ids,
            Err(error) => {
                self.set_status(error, true, cx);
                return;
            }
        };
        let Some(project) = self.editor.snapshot() else {
            return;
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
        let window = cx.active_window().or_else(|| cx.windows().first().copied());
        self.pages_ui.export_pending = true;
        let rx = cx.prompt_save_path(&dir, Some(&name));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let Some(mut path) = this
                .update(cx, |this, cx| this.export_destination(result, cx))
                .ok()
                .flatten()
            else {
                return;
            };
            let chosen = path.clone();
            path.set_extension(extension);
            let result =
                super::export_ui::confirm_normalized_write_path(chosen, path, window, cx).await;
            let Some(path) = this
                .update(cx, |this, cx| {
                    this.export_destination(Ok::<_, std::convert::Infallible>(result), cx)
                })
                .ok()
                .flatten()
            else {
                return;
            };
            this.update(cx, |this, cx| {
                this.set_status(t!("editor.project_pages.exporting"), false, cx)
            })
            .ok();
            let output = path.clone();
            let result = cx
                .background_spawn(async move {
                    emulsion_io::project_export::write(&project, &selected, format, bleed, &output)
                })
                .await;
            this.update(cx, |this, cx| {
                this.pages_ui.export_pending = false;
                match result {
                    Ok(report) => {
                        let mut status = t!(
                            "editor.project_pages.exported",
                            count = report.pages,
                            path = path.display()
                        )
                        .into_owned();
                        if !report.rasterized_pages.is_empty() {
                            status.push_str(&t!(
                                "editor.project_pages.exported_rasterized",
                                count = report.rasterized_pages.len(),
                                ppi = report
                                    .rasterized_page_ppi
                                    .iter()
                                    .map(|ppi| format!("{ppi:.0}"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
                        if !report.rasterized_effect_pages.is_empty() {
                            status.push_str(&t!(
                                "editor.project_pages.exported_effects",
                                count = report.rasterized_effect_pages.len()
                            ));
                        }
                        if !report.insufficient_bleed_pages.is_empty() {
                            status.push_str(&t!(
                                "editor.project_pages.exported_bleed_warning",
                                pages = report.insufficient_bleed_pages.join(", ")
                            ));
                        }
                        this.set_status(status, !report.insufficient_bleed_pages.is_empty(), cx);
                    }
                    Err(error) => {
                        this.set_status(t!("shell.export_failed", error = error), true, cx)
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn project_export_button(&self, cx: &Context<Self>) -> AnyElement {
        if self.pages_ui.organizer.open {
            let owner = cx.weak_entity();
            let count = self.selected_project_pages().len();
            return Button::new("project-export-pages")
                .label(t!("editor.project_pages.export_selected", count = count))
                .small()
                .outline()
                .disabled(count == 0 || self.pages_ui.export_pending)
                .dropdown_menu(move |mut menu, _, _| {
                    for format in emulsion_io::project_export::Format::ALL {
                        let owner = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(t!(
                                "editor.project_pages.format_selected_count",
                                format = format_label(format),
                                count = count
                            ))
                            .on_click(move |_, _, cx| {
                                owner
                                    .update(cx, |this, cx| {
                                        let ids = this.selected_project_pages();
                                        this.export_project_selection(format, ids, cx);
                                    })
                                    .ok();
                            }),
                        );
                    }
                    menu
                })
                .into_any_element();
        }
        Button::new("project-export-pages")
            .label(t!("editor.project_pages.export"))
            .small()
            .outline()
            .on_click(cx.listener(|this, _, window, cx| this.open_export_dialog(window, cx)))
            .into_any_element()
    }

    pub(super) fn project_export_options(&self, cx: &Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        let include_bleed = self.pages_ui.include_bleed;
        Button::new("project-export-options")
            .label(t!("editor.project_pages.export_options"))
            .small()
            .outline()
            .dropdown_menu(move |mut menu, _, _| {
                let bleed_owner = owner.clone();
                menu = menu
                    .item(
                        PopupMenuItem::new(t!("editor.project_pages.include_bleed"))
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
                let selection_owner = owner.clone();
                menu = menu.item(
                    PopupMenuItem::new(t!("editor.project_pages.export_selected_objects"))
                        .on_click(move |_, window, cx| {
                            selection_owner
                                .update(cx, |this, cx| {
                                    this.dismiss_export_dialog(window, cx);
                                    this.show_selection_export(window, cx)
                                })
                                .ok();
                        }),
                );
                for all in [true, false] {
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(if all {
                            t!("editor.project_pages.html_all")
                        } else {
                            t!("editor.project_pages.html_current")
                        })
                        .on_click(move |_, window, cx| {
                            owner
                                .update(cx, |this, cx| {
                                    this.dismiss_export_dialog(window, cx);
                                    this.export_design_html(all, window, cx)
                                })
                                .ok();
                        }),
                    );
                }
                let notes_owner = owner.clone();
                menu = menu.item(
                    PopupMenuItem::new(t!("editor.project_pages.import_export_notes")).on_click(
                        move |_, window, cx| {
                            notes_owner
                                .update(cx, |this, cx| {
                                    this.dismiss_export_dialog(window, cx);
                                    this.show_diagram_import_notes(window, cx)
                                })
                                .ok();
                        },
                    ),
                );
                for all in [true, false] {
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(if all {
                            t!("editor.project_pages.pptx_all")
                        } else {
                            t!("editor.project_pages.pptx_current")
                        })
                        .on_click(move |_, window, cx| {
                            owner
                                .update(cx, |this, cx| {
                                    this.dismiss_export_dialog(window, cx);
                                    this.export_design_pptx(all, cx)
                                })
                                .ok();
                        }),
                    );
                }
                for format in emulsion_io::project_export::Format::ALL {
                    let selection_owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(t!(
                            "editor.project_pages.format_selected",
                            format = format_label(format)
                        ))
                        .on_click(move |_, window, cx| {
                            selection_owner
                                .update(cx, |this, cx| {
                                    this.dismiss_export_dialog(window, cx);
                                    let ids = this.selected_project_pages();
                                    this.export_project_selection(format, ids, cx);
                                })
                                .ok();
                        }),
                    );
                    for all in [true, false] {
                        let owner = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(if all {
                                t!(
                                    "editor.project_pages.format_all",
                                    format = format_label(format)
                                )
                            } else {
                                t!(
                                    "editor.project_pages.format_current",
                                    format = format_label(format)
                                )
                            })
                            .on_click(move |_, window, cx| {
                                owner
                                    .update(cx, |this, cx| {
                                        this.dismiss_export_dialog(window, cx);
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
        if !self.photo_transform_ready(cx) {
            return;
        }
        self.photo_transform = Default::default();
        self.editor = session;
        self.pages_ui.organizer = Default::default();
        self.pages_ui.thumbnail_epoch = self.pages_ui.thumbnail_epoch.wrapping_add(1);
        self.pages_ui
            .retired_thumbnails
            .extend(self.pages_ui.thumbs.drain().map(|(_, (_, image))| image));
        self.pages_ui.loading.clear();
        self.pages_ui.thumbnail_order.clear();
        self.pages_ui.views.clear();
        self.pages_ui.seen_page = 0;
        self.after_change(cx);
        if self.is_design() || self.is_diagram() {
            self.rulers = false;
            self.sidebar_tab = SidebarTab::Properties;
            self.set_tool(Tool::Move, cx);
        }
        // Storyboard panels are drawn with Paint's tools.
        if self.editor.storyboard().is_some() && !self.draw_mode {
            self.toggle_draw_mode(cx);
        }
        // The panel inspector sits above the Layers dock.
        if self.editor.storyboard().is_some() {
            self.sidebar_tab = SidebarTab::Storyboard;
        }
        self.restore_storyboard_layout(cx);
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
        self.photo_transform = Default::default();
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
            knockout_background: None,
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
        // A flipped view is a way of looking, so it follows to the next page.
        let flips = (self.view.flip_x, self.view.flip_y);
        if let Some(view) = self.pages_ui.views.get(&id) {
            self.view = *view;
            self.fit_pending = false;
        } else {
            self.view = View::default();
            self.fit_pending = true;
        }
        (self.view.flip_x, self.view.flip_y) = flips;
        self.notify_canvas(cx);
    }

    pub(super) fn prepare_page_action(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.photo_transform_ready(cx) {
            return false;
        }
        self.cancel_frame_crop(cx);
        if self.styles_ui.dialog_for.is_some()
            || self.raw.is_pending()
            || self.smart.has_pending()
            || self.assistant.running
        {
            self.set_status(t!("editor.project_pages.finish_first"), false, cx);
            return false;
        }
        self.exit_responsive_preview(cx);
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
        } else if let Some(board) = self.editor.storyboard() {
            // A blank panel in the active panel's scene, named by the rules.
            let panel = emulsion_core::storyboard::Panel::new(0, board.settings.panel_frames);
            let name = self.editor.next_panel_name(id);
            board.blank_panel().and_then(|blank| {
                self.editor
                    .insert_panels(Some(id), &blank, vec![(name, panel)], None)
                    .map(|ids| ids[0])
            })
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
                .title(t!("editor.project_pages.properties"))
                .width(px(360.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(t!("editor.project_pages.page_name"))
                        .child(Input::new(&name))
                        .child(t!("editor.project_pages.bleed_mm"))
                        .child(Input::new(&bleed)),
                )
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.project_pages.save_changes"
                )))
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

    pub(super) fn retire_page_thumbnails(&mut self, window: &mut Window) {
        for image in self.pages_ui.retired_thumbnails.drain(..) {
            let _ = window.drop_image(image);
        }
    }
    pub(super) fn release_page_thumbnails(&mut self, window: &mut Window) {
        self.pages_ui.thumbnail_epoch = self.pages_ui.thumbnail_epoch.wrapping_add(1);
        self.pages_ui.loading.clear();
        self.pages_ui.thumbnail_order.clear();
        for (_, (_, image)) in self.pages_ui.thumbs.drain() {
            let _ = window.drop_image(image);
        }
        self.retire_page_thumbnails(window);
    }

    /// A page picture at most `max` pixels on its longest side, rendered in
    /// the background; the previous picture shows until it is ready.
    pub(super) fn page_thumbnail(
        &mut self,
        id: PageId,
        max: u32,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        if !self.visible {
            return None;
        }
        let editor = self.editor.page(id)?;
        let revision = editor.revision;
        // Review layers can be left out of thumbnails (Settings › Storyboard).
        let hide = crate::app_state::settings(cx)
            .storyboard
            .hide_review_in_thumbnails;
        let shown = revision * 2 + u64::from(hide);
        let key = (id, max);
        self.pages_ui
            .thumbnail_order
            .retain(|cached| *cached != key);
        self.pages_ui.thumbnail_order.push_back(key);
        if let Some((rev, image)) = self.pages_ui.thumbs.get(&key)
            && *rev == shown
        {
            return Some(image.clone());
        }
        if !self.pages_ui.loading.contains_key(&key) && self.pages_ui.loading.len() < 4 {
            let doc = editor.doc.clone();
            let epoch = self.pages_ui.thumbnail_epoch;
            self.pages_ui.loading.insert(key, revision);
            cx.spawn(async move |this, cx| {
                let thumbnail = cx
                    .background_spawn(async move {
                        let doc = match hide {
                            true => emulsion_core::storyboard_review::printable(&doc),
                            false => std::borrow::Cow::Borrowed(&doc),
                        };
                        super::history::doc_thumb(&doc, max)
                    })
                    .await;
                this.update(cx, |this, cx| {
                    if this.pages_ui.thumbnail_epoch != epoch || !this.visible {
                        return;
                    }
                    this.pages_ui.loading.remove(&key);
                    if !this.editor.page(id).is_some_and(|p| p.revision == revision) {
                        return;
                    }
                    let (w, h, bytes) = match thumbnail {
                        Ok(thumbnail) => thumbnail,
                        Err(error) => {
                            this.set_status(error, true, cx);
                            return;
                        }
                    };
                    if this.pages_ui.thumbnail_epoch != epoch || !this.visible {
                        return;
                    }
                    this.pages_ui.loading.remove(&key);
                    if this.editor.page(id).is_some_and(|p| p.revision == revision) {
                        if let Some((_, old)) = this
                            .pages_ui
                            .thumbs
                            .insert(key, (shown, Arc::new(viewport::bgra_image(w, h, bytes))))
                        {
                            this.pages_ui.retired_thumbnails.push(old);
                        }
                        // Room for each page at a few sizes (strip, Board,
                        // Stage, player), so pictures on screen are never
                        // evicted and re-rendered in a loop.
                        let cap = 128.max(this.editor.page_list().len() * 4);
                        while this.pages_ui.thumbs.len() > cap {
                            let Some(oldest) = this.pages_ui.thumbnail_order.pop_front() else {
                                break;
                            };
                            if let Some((_, old)) = this.pages_ui.thumbs.remove(&oldest) {
                                this.pages_ui.retired_thumbnails.push(old);
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        self.pages_ui
            .thumbs
            .get(&key)
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
        let design = self.is_design();
        // Storyboards are read by their pictures, like Design pages.
        let thumbs = design || kind == emulsion_core::project::ProjectKind::Storyboard;
        // Running time in seconds, for the strip's summary.
        let storyboard = self
            .editor
            .storyboard()
            .map(|board| board.total_frames() as f64 / board.settings.frame_rate.fps());
        let page_number = pages.iter().position(|p| p.id == active).unwrap_or(0) + 1;
        // The strip is horizontally scrollable: rasterize only visible/nearby
        // cards so a large project cannot continually churn the bounded cache.
        let scroll = self.pages_ui.strip_scroll.clone();
        let start = ((-f32::from(scroll.offset().x)).max(0.) / 72.) as usize;
        let visible = (f32::from(scroll.bounds().size.width).max(1200.) / 72.).ceil() as usize + 2;
        let mut row = div()
            .id("project-pages-scroll")
            .flex()
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&scroll)
            .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
            .gap_2()
            .px_2()
            .py(px(if thumbs { 8. } else { 2. }));
        for (index, meta) in pages.into_iter().enumerate() {
            let id = meta.id;
            let image = if thumbs && index >= start.saturating_sub(1) && index <= start + visible {
                self.page_thumbnail(id, 96, cx)
            } else {
                None
            };
            let owner = cx.weak_entity();
            row = row.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .w(px(if thumbs { 64. } else { 78. }))
                    .when(thumbs, |tile| tile.relative().h(px(64.)))
                    .when(!thumbs, |tile| tile.flex_row().items_center().w(px(138.)))
                    .flex_none()
                    .when(!thumbs, |tile| {
                        tile.child(
                            Button::new(("project-page", id))
                                .label(meta.name.clone())
                                .accessibility_label(t!(
                                    "editor.project_pages.page_label",
                                    number = index + 1,
                                    name = meta.name
                                ))
                                .tooltip(meta.name.clone())
                                .xsmall()
                                .ghost()
                                .h(px(26.))
                                .w(px(106.))
                                .bg(if id == active { p.soft_bg } else { p.panel })
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.select_page(id, cx)),
                                ),
                        )
                    })
                    .when(thumbs, |tile| {
                        tile.child(
                            div()
                                .id(("project-page", id))
                                .test_support()
                                .h(px(if thumbs { 64. } else { 52. }))
                                .w_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(p.stage)
                                .rounded(px(4.))
                                .border_1()
                                .border_color(if id == active { p.accent } else { p.line })
                                .cursor_pointer()
                                .children(image.map(|image| {
                                    img(image)
                                        .size_full()
                                        .aspect_square()
                                        .object_fit(ObjectFit::Contain)
                                }))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.select_page(id, cx)),
                                ),
                        )
                    })
                    .when(thumbs, |tile| tile.children(self.external_strip_badge(id, p)))
                    .when(thumbs, |tile| {
                        tile.child(
                            Button::new(("project-page-remove", id))
                                .accessibility_label(t!(
                                    "editor.project_pages.remove_page_label",
                                    number = index + 1,
                                    name = meta.name
                                ))
                                .tooltip(if count == 1 {
                                    if kind == emulsion_core::project::ProjectKind::Storyboard {
                                        "Keep one panel. To close the storyboard, use its document tab."
                                            .into()
                                    } else {
                                        t!("editor.project_pages.keep_one")
                                    }
                                } else {
                                    t!("editor.project_pages.remove_page_tip")
                                })
                                .label("×")
                                .xsmall()
                                .ghost()
                                .absolute()
                                .top_0()
                                .right_0()
                                .size(px(20.))
                                .bg(p.panel.opacity(0.95))
                                .disabled(count == 1)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.delete_project_page(id, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new(("project-page-menu", id))
                            .label(if thumbs {
                                format!("{} ···", index + 1)
                            } else {
                                "···".into()
                            })
                            .tooltip(meta.name.clone())
                            .when(thumbs, |button| {
                                button
                                    .absolute()
                                    .bottom_0()
                                    .left_0()
                                    .h(px(18.))
                                    .bg(p.panel.opacity(0.9))
                            })
                            .xsmall()
                            .ghost()
                            .when(!thumbs, |button| {
                                button
                                    .h(px(26.))
                                    .bg(if id == active { p.soft_bg } else { p.panel })
                            })
                            .dropdown_menu(move |menu, _, _| {
                                let select = owner.clone();
                                let duplicate = owner.clone();
                                let remove = owner.clone();
                                let left = owner.clone();
                                let right = owner.clone();
                                let properties = owner.clone();
                                menu.item(
                                    PopupMenuItem::new(t!("editor.project_pages.open_page"))
                                        .on_click(move |_, _, cx| {
                                            select.update(cx, |e, cx| e.select_page(id, cx)).ok();
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new(t!("editor.project_pages.duplicate_page"))
                                        .on_click(move |_, _, cx| {
                                            duplicate
                                                .update(cx, |e, cx| {
                                                    e.select_page(id, cx);
                                                    e.add_project_page(true, cx);
                                                })
                                                .ok();
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new(t!("editor.project_pages.name_and_bleed"))
                                        .on_click(move |_, window, cx| {
                                            properties
                                                .update(cx, |e, cx| {
                                                    e.page_properties(id, window, cx)
                                                })
                                                .ok();
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new(t!("editor.project_pages.move_left"))
                                        .disabled(index == 0)
                                        .on_click(move |_, _, cx| {
                                            left.update(cx, |e, cx| {
                                                e.move_project_page(id, index.saturating_sub(1), cx)
                                            })
                                            .ok();
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new(t!("editor.project_pages.move_right"))
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
                                    PopupMenuItem::new(t!("editor.project_pages.delete_page"))
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
        if design {
            row = row.items_center().child(
                Button::new("project-page-add")
                    .accessibility_label(t!("editor.project_pages.add_page"))
                    .label("+")
                    .outline()
                    .size(px(64.))
                    .flex_none()
                    .on_click(cx.listener(|this, _, _, cx| this.add_project_page(false, cx))),
            );
            return Some(
                div()
                    .id("project-page-strip")
                    .test_support()
                    .flex()
                    .items_center()
                    .h(px(88.))
                    .flex_none()
                    .gap(px(8.))
                    .px(px(12.))
                    .bg(p.panel)
                    .border_t_1()
                    .border_color(p.line)
                    .child(row)
                    .child(
                        Button::new("project-page-organizer")
                            .label(t!("editor.project_pages.pages"))
                            .accessibility_label(t!("editor.project_pages.organize"))
                            .tooltip(t!("editor.project_pages.organize_tip"))
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_page_organizer(window, cx)
                            })),
                    )
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(px(10.5))
                            .text_color(p.muted)
                            .child(t!(
                                "editor.project_pages.page_of",
                                number = page_number,
                                count = count
                            )),
                    )
                    .child(
                        div()
                            .id("design-page-zoom")
                            .test_support()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .p(px(2.))
                            .rounded(px(5.))
                            .bg(p.soft_bg)
                            .border_1()
                            .border_color(p.line)
                            .child(
                                Button::new("design-zoom-out")
                                    .accessibility_label(t!("editor.project_pages.zoom_out"))
                                    .label("−")
                                    .xsmall()
                                    .ghost()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.zoom_step(false, cx)),
                                    ),
                            )
                            .child(
                                Button::new("design-zoom-fit")
                                    .accessibility_label(t!("editor.project_pages.fit_page"))
                                    .tooltip(t!("editor.project_pages.fit_page"))
                                    .label(format!("{:.0}%", self.view.zoom * 100.))
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| this.zoom_fit(cx))),
                            )
                            .child(
                                Button::new("design-zoom-in")
                                    .accessibility_label(t!("editor.project_pages.zoom_in"))
                                    .label("+")
                                    .xsmall()
                                    .ghost()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.zoom_step(true, cx)),
                                    ),
                            ),
                    )
                    .into_any_element(),
            );
        }
        Some(
            div()
                .id("project-page-strip")
                .test_support()
                .flex()
                .items_center()
                .h(px(if thumbs { 84. } else { 32. }))
                .flex_none()
                .gap_2()
                .bg(p.panel)
                .border_t_1()
                .border_color(p.line)
                .child(row)
                .child(
                    Button::new("project-page-add")
                        .label("+")
                        .tooltip(if storyboard.is_some() {
                            "Add panel".into()
                        } else {
                            t!("editor.project_pages.add_page")
                        })
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| this.add_project_page(false, cx))),
                )
                .child(
                    Button::new("project-undo")
                        .label("↶")
                        .tooltip(t!("editor.project_pages.undo_tip"))
                        .small()
                        .ghost()
                        .disabled(!self.editor.can_undo())
                        .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
                )
                .child(
                    Button::new("project-redo")
                        .label("↷")
                        .tooltip(t!("editor.project_pages.redo_tip"))
                        .small()
                        .ghost()
                        .disabled(!self.editor.can_redo())
                        .on_click(cx.listener(|this, _, _, cx| this.redo(cx))),
                )
                .children(storyboard.map(|_| self.storyboard_view_toggle(cx)))
                .children(storyboard.map(|_| self.storyboard_timeline_toggle(cx)))
                .child(
                    div()
                        .pr_3()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(match storyboard {
                            Some(seconds) => format!(
                                "Storyboard · {count} panels · {}",
                                super::storyboard_board::running_time(seconds)
                            ),
                            None => t!(
                                "editor.project_pages.kind_pages",
                                kind = match kind {
                                    emulsion_core::project::ProjectKind::Design => {
                                        t!("shell.dest_design")
                                    }
                                    emulsion_core::project::ProjectKind::Diagram => {
                                        t!("shell.dest_diagram")
                                    }
                                    emulsion_core::project::ProjectKind::Storyboard => {
                                        "Storyboard".into()
                                    }
                                },
                                count = count
                            )
                            .into_owned(),
                        }),
                )
                .into_any_element(),
        )
    }
}

/// Localized display name for a page export format; `Format::label` stays English.
fn format_label(format: emulsion_io::project_export::Format) -> std::borrow::Cow<'static, str> {
    use emulsion_io::project_export::Format;
    match format {
        Format::Png => t!("editor.project_pages.format_png"),
        Format::Jpeg => t!("editor.project_pages.format_jpeg"),
        Format::Svg => t!("editor.project_pages.format_svg"),
        Format::Pdf => t!("editor.project_pages.format_pdf"),
    }
}
