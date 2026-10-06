//! The storyboard Library panel: reusable drawings (characters, props,
//! backgrounds) in the project library, saved in the storyboard, and the
//! personal library, shared by every storyboard on this computer, with
//! thumbnails and search. Click an item, choose Place, or drag it onto the
//! Stage or a Board panel: layers go on top of the panel at their original
//! position; a panel item becomes a new panel after it; a scene item becomes
//! a new scene after the panel's scene. Panel and scene items keep their
//! animation: durations, layer keyframes, comps and camera moves.
//!
//! Placing is one Undo step, and so is every project library change, since
//! the project library is part of the storyboard. The personal library is a
//! set of files saved at once, outside the project and its Undo.
//!
//! Saving the storyboard as a template lives here too.
use super::*;
use emulsion_core::project::PageId;
use emulsion_core::storyboard_library::{ItemKind, LibraryItem, Placed, matches};
use emulsion_io::creative_library::{self as library, Catalog};
use emulsion_io::storyboard_library as personal;
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenuItem},
};
use std::cell::Cell;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Scope {
    /// Saved in this storyboard.
    Project,
    /// Shared by every storyboard.
    Personal,
}

impl Scope {
    fn key(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Personal => "personal",
        }
    }
}

/// A library item dragged to the Stage or a Board panel.
#[derive(Clone)]
pub(crate) struct LibraryDrag {
    pub(crate) scope: Scope,
    pub(crate) id: u64,
    name: String,
    thumb: Option<Arc<RenderImage>>,
}

impl Render for LibraryDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .flex()
            .items_center()
            .gap_2()
            .p_1()
            .bg(p.panel)
            .text_color(p.ink)
            .border_1()
            .border_color(p.accent)
            .rounded(px(4.))
            .text_size(px(12.))
            .children(
                self.thumb
                    .clone()
                    .map(|t| img(t).size(px(32.)).object_fit(ObjectFit::Contain)),
            )
            .child(self.name.clone())
    }
}

/// One item as the panel shows it.
#[derive(Clone)]
struct Entry {
    scope: Scope,
    id: u64,
    name: String,
    tags: Vec<String>,
    kind: ItemKind,
    /// The item, once available.
    item: Option<Arc<LibraryItem>>,
}

impl Entry {
    fn thumb_key(&self) -> Option<(Scope, u64, usize)> {
        let doc = &self.item.as_ref()?.doc;
        Some((self.scope, self.id, Arc::as_ptr(doc) as usize))
    }

    /// The kind, and whether the item brings animation, for its card.
    fn detail(&self) -> String {
        let animated = self.item.as_ref().is_some_and(|i| i.is_animated());
        let mut detail = match (self.kind, &self.item) {
            (ItemKind::Scene, Some(item)) => {
                format!("Scene · {} panels", item.drawings().count())
            }
            (kind, _) => kind.label().to_string(),
        };
        if animated {
            detail.push_str(" · animated");
        }
        if !self.tags.is_empty() {
            detail = format!("{detail} · {}", self.tags.join(", "));
        }
        detail
    }
}

#[derive(Default)]
pub(crate) struct LibraryUi {
    search: Option<Entity<InputState>>,
    thumbs: HashMap<(Scope, u64, usize), Arc<RenderImage>>,
    rendering: HashSet<(Scope, u64, usize)>,
    /// Personal items read from disk, by asset ID.
    drawings: HashMap<u64, Arc<LibraryItem>>,
    loading: HashSet<u64>,
    /// The personal catalog was reread from disk since the panel opened.
    refreshed: bool,
}

impl EditorView {
    fn library_entries(&self, query: &str) -> Vec<Entry> {
        let Some(board) = self.editor.storyboard() else {
            return Vec::new();
        };
        let project = board.library.items.iter().map(|i| Entry {
            scope: Scope::Project,
            id: i.id,
            name: i.name.clone(),
            tags: i.tags.clone(),
            kind: i.kind,
            item: Some(Arc::new(i.clone())),
        });
        let mine = personal::items(&self.creative.catalog).map(|(a, kind)| Entry {
            scope: Scope::Personal,
            id: a.id,
            name: a.name.clone(),
            tags: a.tags.clone(),
            kind,
            item: self.storyboard_library.drawings.get(&a.id).cloned(),
        });
        project
            .chain(mine)
            .filter(|e| matches(&e.name, &e.tags, query))
            .collect()
    }

    fn library_entry(&self, scope: Scope, id: u64) -> Option<Entry> {
        self.library_entries("")
            .into_iter()
            .find(|e| e.scope == scope && e.id == id)
    }

    /// Install a catalog changed by a personal library operation here and in
    /// every open tab.
    fn library_publish(&mut self, catalog: Catalog, cx: &mut Context<Self>) {
        self.install_catalog(catalog.clone());
        if let Some(workspace) = self.library_workspace.as_ref().and_then(|w| w.upgrade()) {
            cx.defer(move |cx| {
                workspace.update(cx, |ws, cx| ws.publish_creative_catalog(catalog, cx));
            });
        }
        cx.notify();
    }

    /// Read personal drawings and render thumbnails off the UI thread.
    fn library_load(&mut self, entries: &[Entry], cx: &mut Context<Self>) {
        if !self.storyboard_library.refreshed {
            self.storyboard_library.refreshed = true;
            cx.spawn(async move |this, cx| {
                let catalog = cx
                    .background_spawn(async { library::load(&library::root()) })
                    .await;
                if let Ok(catalog) = catalog {
                    this.update(cx, |this, cx| {
                        this.install_catalog(catalog);
                        cx.notify();
                    })
                    .ok();
                }
            })
            .detach();
        }
        for entry in entries {
            if entry.scope == Scope::Personal
                && entry.item.is_none()
                && self.storyboard_library.loading.insert(entry.id)
            {
                let Some(asset) = self
                    .creative
                    .catalog
                    .assets
                    .iter()
                    .find(|a| a.id == entry.id)
                    .cloned()
                else {
                    continue;
                };
                let id = entry.id;
                cx.spawn(async move |this, cx| {
                    let doc = cx
                        .background_spawn(async move { personal::load_item(&asset) })
                        .await;
                    this.update(cx, |this, cx| {
                        match doc {
                            Ok(item) => {
                                this.storyboard_library.drawings.insert(id, Arc::new(item));
                                this.storyboard_library.loading.remove(&id);
                            }
                            // Stays in `loading`, so a broken file is not reread every frame.
                            Err(e) => this.set_status(format!("Library item: {e}"), true, cx),
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
            let (Some(key), Some(doc)) = (
                entry.thumb_key(),
                entry.item.as_ref().map(|i| i.doc.clone()),
            ) else {
                continue;
            };
            if self.storyboard_library.thumbs.contains_key(&key)
                || !self.storyboard_library.rendering.insert(key)
            {
                continue;
            }
            cx.spawn(async move |this, cx| {
                let thumbnail = cx
                    .background_spawn(async move { crate::editor::doc_thumb(&doc, 128) })
                    .await;
                this.update(cx, |this, cx| {
                    this.storyboard_library.rendering.remove(&key);
                    let (w, h, bytes) = match thumbnail {
                        Ok(thumbnail) => thumbnail,
                        Err(error) => {
                            this.set_status(error, true, cx);
                            return;
                        }
                    };
                    let ui = &mut this.storyboard_library;
                    ui.rendering.remove(&key);
                    ui.thumbs
                        .insert(key, Arc::new(crate::viewport::bgra_image(w, h, bytes)));
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// Report a library change: refresh on success, status bar on error.
    fn library_result<T>(
        &mut self,
        result: Result<T, String>,
        message: impl FnOnce(&T) -> String,
        cx: &mut Context<Self>,
    ) -> Option<T> {
        match result {
            Ok(value) => {
                self.after_change(cx);
                let message = message(&value);
                self.set_status(message, false, cx);
                Some(value)
            }
            Err(error) => {
                self.set_status(error, true, cx);
                None
            }
        }
    }

    /// Add the selected layers (`kind` Layers), the active panel or its
    /// scene to a library under `name`. Panels and scenes keep their
    /// animation. Returns false when nothing could be added.
    pub(crate) fn library_add(
        &mut self,
        scope: Scope,
        kind: ItemKind,
        name: &str,
        tags: &[String],
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.prepare_page_action(cx) {
            return false;
        }
        let panel = self.editor.active_page();
        let layers = self.selected_layer_ids();
        if kind == ItemKind::Layers && layers.is_empty() {
            self.set_status("Select the layers to add first.", true, cx);
            return false;
        }
        let item = match kind {
            ItemKind::Layers => {
                emulsion_core::storyboard_library::capture_layers(&self.editor.doc, &layers)
                    .map(|doc| LibraryItem::drawing(ItemKind::Layers, doc))
            }
            ItemKind::Panel => self.editor.capture_panel_item(panel),
            ItemKind::Scene => match self.editor.storyboard().map(|b| b.panels.get(&panel)) {
                Some(Some(p)) => self.editor.capture_scene_item(p.scene),
                _ => Err("Select a panel first.".into()),
            },
        };
        let item = match item {
            Ok(item) => item,
            Err(error) => {
                self.set_status(error, true, cx);
                return false;
            }
        };
        match scope {
            Scope::Project => {
                let result = self.editor.add_library_entry(name, tags, item);
                self.library_result(
                    result,
                    |_| format!("Added {} to the project library.", name.trim()),
                    cx,
                )
                .is_some()
            }
            Scope::Personal => {
                self.library_add_personal(name.to_string(), tags.to_vec(), item, cx);
                true
            }
        }
    }

    fn library_add_personal(
        &mut self,
        name: String,
        tags: Vec<String>,
        item: LibraryItem,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let label = name.trim().to_string();
            let result = cx
                .background_spawn(async move {
                    personal::add_item(&library::root(), &name, &tags, &item)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((catalog, _)) => {
                    this.library_publish(catalog, cx);
                    this.set_status(
                        format!("Added {label} to your personal library."),
                        false,
                        cx,
                    );
                }
                Err(e) => this.set_status(e.to_string(), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Place an item on `target` (default: the active panel). One Undo step.
    pub(crate) fn library_place(
        &mut self,
        scope: Scope,
        id: u64,
        target: Option<PageId>,
        cx: &mut Context<Self>,
    ) {
        if let Some(target) = target {
            self.select_page(target, cx);
            if self.editor.active_page() != target {
                return;
            }
        }
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(entry) = self.library_entry(scope, id) else {
            self.set_status("That library item no longer exists.", true, cx);
            return;
        };
        let result = match (scope, &entry.item) {
            (Scope::Project, _) => self.editor.place_library_item(id),
            (Scope::Personal, Some(item)) => self.editor.place_item(item),
            (Scope::Personal, None) => {
                self.set_status(
                    "That drawing is still loading. Try again in a moment.",
                    true,
                    cx,
                );
                return;
            }
        };
        let name = entry.name.clone();
        if let Some(placed) = self.library_result(result, |_| format!("Placed {name}."), cx)
            && let Placed::Layers(layers) = placed
        {
            let active = layers.last().copied();
            self.set_layer_selection(layers, active);
            cx.notify();
        }
    }

    pub(crate) fn library_rename(
        &mut self,
        scope: Scope,
        id: u64,
        name: String,
        tags: Vec<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        match scope {
            Scope::Project => {
                if !self.prepare_page_action(cx) {
                    return false;
                }
                let result = self
                    .editor
                    .edit_storyboard(|b| b.library.rename(id, &name, Some(&tags)));
                self.library_result(result, |_| "Renamed library item.".into(), cx)
                    .is_some()
            }
            Scope::Personal => {
                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_spawn(async move {
                            personal::rename(&library::root(), id, &name, Some(&tags))
                        })
                        .await;
                    this.update(cx, |this, cx| match result {
                        Ok(catalog) => this.library_publish(catalog, cx),
                        Err(e) => this.set_status(e.to_string(), true, cx),
                    })
                    .ok();
                })
                .detach();
                true
            }
        }
    }

    pub(crate) fn library_delete(&mut self, scope: Scope, id: u64, cx: &mut Context<Self>) {
        match scope {
            Scope::Project => {
                if !self.prepare_page_action(cx) {
                    return;
                }
                let result = self
                    .editor
                    .edit_storyboard(|b| b.library.remove(id).map(|_| ()));
                self.library_result(
                    result,
                    |_| "Deleted from the project library. Undo brings it back.".into(),
                    cx,
                );
            }
            Scope::Personal => {
                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_spawn(async move { personal::remove(&library::root(), id) })
                        .await;
                    this.update(cx, |this, cx| match result {
                        Ok(catalog) => {
                            this.storyboard_library.drawings.remove(&id);
                            this.library_publish(catalog, cx);
                            this.set_status("Deleted from your personal library.", false, cx);
                        }
                        Err(e) => this.set_status(e.to_string(), true, cx),
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// Copy an item into the other library.
    fn library_copy(&mut self, from: Scope, id: u64, cx: &mut Context<Self>) {
        let Some(Entry {
            name,
            tags,
            item: Some(item),
            ..
        }) = self.library_entry(from, id)
        else {
            self.set_status(
                "That drawing is still loading. Try again in a moment.",
                true,
                cx,
            );
            return;
        };
        match from {
            Scope::Project => self.library_add_personal(name, tags, (*item).clone(), cx),
            Scope::Personal => {
                if !self.prepare_page_action(cx) {
                    return;
                }
                let result = self.editor.add_library_entry(&name, &tags, (*item).clone());
                self.library_result(
                    result,
                    |_| format!("Added {name} to the project library."),
                    cx,
                );
            }
        }
    }

    /// Name, tags and library for a new item.
    fn library_add_dialog(&mut self, kind: ItemKind, window: &mut Window, cx: &mut Context<Self>) {
        let layers = self.selected_layer_ids();
        if kind == ItemKind::Layers && layers.is_empty() {
            self.set_status("Select the layers to add first.", true, cx);
            return;
        }
        let suggested = match kind {
            ItemKind::Layers => self
                .editor
                .doc
                .node(layers[0])
                .map(|n| n.name.clone())
                .unwrap_or_default(),
            ItemKind::Panel => self
                .editor
                .page_list()
                .iter()
                .find(|m| m.id == self.editor.active_page())
                .map(|m| m.name.clone())
                .unwrap_or_default(),
            ItemKind::Scene => self
                .editor
                .storyboard()
                .and_then(|b| {
                    let scene = b.panels.get(&self.editor.active_page())?.scene;
                    b.scenes.get(&scene).map(|s| s.name.clone())
                })
                .unwrap_or_default(),
        };
        let name = cx.new(|cx| InputState::new(window, cx).default_value(suggested));
        let tags = cx.new(|cx| InputState::new(window, cx).placeholder("character, prop"));
        let mine = Rc::new(Cell::new(false));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let (name, tags, owner) = (name.clone(), tags.clone(), owner.clone());
            let (m, mine) = (mine.clone(), mine.clone());
            dialog
                .title(match kind {
                    ItemKind::Layers => "Add layers to the library",
                    ItemKind::Panel => "Add panel to the library",
                    ItemKind::Scene => "Add scene to the library",
                })
                .width(px(380.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Name")
                        .child(Input::new(&name))
                        .child("Tags · comma separated")
                        .child(Input::new(&tags))
                        .child(
                            Checkbox::new("storyboard-library-personal")
                                .label("Personal library (shared by every storyboard)")
                                .checked(m.get())
                                .on_click(move |value, _, _| m.set(*value)),
                        ),
                )
                .footer(crate::widgets::form_dialog_footer("Add"))
                .on_ok(move |_, _, cx| {
                    let text = name.read(cx).value().to_string();
                    let tags = split_tags(&tags.read(cx).value());
                    let scope = if mine.get() {
                        Scope::Personal
                    } else {
                        Scope::Project
                    };
                    owner
                        .update(cx, |this, cx| {
                            this.library_add(scope, kind, &text, &tags, cx)
                        })
                        .unwrap_or(false)
                })
        });
    }

    fn library_rename_dialog(&mut self, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| InputState::new(window, cx).default_value(entry.name.clone()));
        let tags = cx.new(|cx| InputState::new(window, cx).default_value(entry.tags.join(", ")));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let (name, tags, owner) = (name.clone(), tags.clone(), owner.clone());
            dialog
                .title("Rename library item")
                .width(px(360.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Name")
                        .child(Input::new(&name))
                        .child("Tags · comma separated")
                        .child(Input::new(&tags)),
                )
                .footer(crate::widgets::form_dialog_footer("Rename"))
                .on_ok(move |_, _, cx| {
                    let text = name.read(cx).value().to_string();
                    let tags = split_tags(&tags.read(cx).value());
                    owner
                        .update(cx, |this, cx| {
                            this.library_rename(entry.scope, entry.id, text, tags, cx)
                        })
                        .unwrap_or(false)
                })
        });
    }

    fn library_delete_dialog(&mut self, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        if entry.scope == Scope::Project {
            // Undo brings it back.
            self.library_delete(entry.scope, entry.id, cx);
            return;
        }
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let owner = owner.clone();
            dialog
                .title("Delete from your personal library?")
                .width(px(360.))
                .child(format!(
                    "{} is removed for every storyboard. Panels it was placed on keep their copies.",
                    entry.name
                ))
                .footer(crate::widgets::form_dialog_footer("Delete"))
                .on_ok(move |_, _, cx| {
                    owner
                        .update(cx, |this, cx| this.library_delete(entry.scope, entry.id, cx))
                        .is_ok()
                })
        });
    }

    /// Save the storyboard as a template in the personal library, ready in
    /// New canvas.
    pub(crate) fn save_storyboard_template(
        &mut self,
        name: String,
        tags: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(project) = self.editor.snapshot().filter(|p| p.storyboard.is_some()) else {
            self.set_status("Open a storyboard to save it as a template.", true, cx);
            return;
        };
        let mut manifest = emulsion_io::template_pack::Manifest::new(
            emulsion_io::template_pack::Kind::Storyboard,
            name.trim().to_string(),
        );
        manifest.tags = tags;
        cx.spawn(async move |this, cx| {
            let label = manifest.name.clone();
            let result = cx
                .background_spawn(async move {
                    let pack = emulsion_io::template_pack::pack(&project, &manifest, None)?;
                    emulsion_io::template_pack::install(&library::root(), pack)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((catalog, _)) => {
                    this.library_publish(catalog, cx);
                    this.set_status(
                        format!("Saved template {label}. Choose it in New canvas › Storyboard › Templates."),
                        false,
                        cx,
                    );
                }
                Err(e) => this.set_status(e.to_string(), true, cx),
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn save_storyboard_template_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| InputState::new(window, cx).default_value(self.name.clone()));
        let tags = cx.new(|cx| InputState::new(window, cx).placeholder("series, pitch"));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let (name, tags, owner) = (name.clone(), tags.clone(), owner.clone());
            dialog
                .title("Save as storyboard template")
                .width(px(420.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("New storyboards from this template start with its resolution, frame rate, caption fields, naming, Smart add layers, stage guides, palette, library and panels. Version history is left out.")
                        .child("Name")
                        .child(Input::new(&name))
                        .child("Tags · comma separated")
                        .child(Input::new(&tags)),
                )
                .footer(crate::widgets::form_dialog_footer("Save template"))
                .on_ok(move |_, _, cx| {
                    let text = name.read(cx).value().trim().to_string();
                    if text.is_empty() || text.chars().count() > 200 {
                        return false;
                    }
                    let tags = split_tags(&tags.read(cx).value());
                    owner
                        .update(cx, |this, cx| this.save_storyboard_template(text, tags, cx))
                        .is_ok()
                })
        });
    }

    /// The Library sidebar panel.
    pub(super) fn storyboard_library_panel(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.editor.storyboard().is_none() {
            return div()
                .p_3()
                .child("The library is part of storyboards.")
                .into_any_element();
        }
        let search = self
            .storyboard_library
            .search
            .get_or_insert_with(|| {
                cx.new(|cx| InputState::new(window, cx).placeholder("Search names and tags"))
            })
            .clone();
        let query = search.read(cx).value().to_string();
        let entries = self.library_entries(&query);
        self.library_load(&entries, cx);
        let has_layers = !self.selected_layer_ids().is_empty();
        let mut panel = div()
            .id("storyboard-library")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .text_size(px(12.))
            .text_color(p.ink)
            .child(Input::new(&search).small())
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        Button::new("storyboard-library-add-layers")
                            .label("Add layers…")
                            .tooltip("Add the selected layers to a library")
                            .xsmall()
                            .outline()
                            .disabled(!has_layers)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.library_add_dialog(ItemKind::Layers, window, cx)
                            })),
                    )
                    .child(
                        Button::new("storyboard-library-add-panel")
                            .label("Add panel…")
                            .tooltip("Add the active panel to a library")
                            .xsmall()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.library_add_dialog(ItemKind::Panel, window, cx)
                            })),
                    )
                    .child(
                        Button::new("storyboard-library-add-scene")
                            .label("Add scene…")
                            .tooltip("Add the active panel's scene, with its timing, keyframes and camera, to a library")
                            .xsmall()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.library_add_dialog(ItemKind::Scene, window, cx)
                            })),
                    )
                    .child(
                        Button::new("storyboard-library-refresh")
                            .label("↻")
                            .accessibility_label("Reload the personal library")
                            .tooltip("Reload the personal library")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let ui = &mut this.storyboard_library;
                                ui.refreshed = false;
                                ui.loading.clear();
                                cx.notify();
                            })),
                    ),
            );
        for (scope, title, empty) in [
            (
                Scope::Project,
                "In this storyboard",
                "Select layers or a panel and add them, to reuse them on other panels.",
            ),
            (
                Scope::Personal,
                "Personal library",
                "Items here are shared by every storyboard.",
            ),
        ] {
            let items: Vec<_> = entries.iter().filter(|e| e.scope == scope).collect();
            panel = panel
                .child(
                    div()
                        .pt_1()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{title} · {}", items.len())),
                )
                .when(items.is_empty(), |d| {
                    d.child(div().text_size(px(11.)).text_color(p.muted).child(
                        if query.trim().is_empty() {
                            empty
                        } else {
                            "No matches."
                        },
                    ))
                })
                .child(
                    div().grid().grid_cols(2).gap_2().children(
                        items
                            .into_iter()
                            .map(|e| self.library_card(e.clone(), p, cx)),
                    ),
                );
        }
        panel
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child("Click or drag onto the Stage or a Board panel. Layers land where they were drawn; a panel item becomes the next panel; a scene item becomes the next scene. Panels and scenes keep their keyframes and camera moves."),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .pt_2()
                    .child(
                        Button::new("storyboard-save-template")
                            .label("Save as storyboard template…")
                            .xsmall()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_storyboard_template_dialog(window, cx)
                            })),
                    )
                    .child(
                        Button::new("storyboard-export-template")
                            .label("Export template file…")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.export_creative_pack(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    fn library_card(&self, entry: Entry, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let thumb = entry
            .thumb_key()
            .and_then(|key| self.storyboard_library.thumbs.get(&key).cloned());
        let (scope, id) = (entry.scope, entry.id);
        let drag = LibraryDrag {
            scope,
            id,
            name: entry.name.clone(),
            thumb: thumb.clone(),
        };
        let owner = cx.weak_entity();
        let menu_entry = entry.clone();
        let detail = entry.detail();
        let accent = p.accent;
        div()
            .id(SharedString::from(format!(
                "storyboard-library-{}-{id}",
                scope.key()
            )))
            .test_support()
            .flex()
            .flex_col()
            .gap_1()
            .p_1()
            .min_w_0()
            .rounded(px(4.))
            .border_1()
            .border_color(p.line)
            .bg(p.panel)
            .cursor_pointer()
            .hover(move |s| s.border_color(accent))
            .on_click(cx.listener(move |this, _, _, cx| this.library_place(scope, id, None, cx)))
            .on_drag(drag, |d, _, _, cx| cx.new(|_| d.clone()))
            .child(
                div()
                    .h(px(64.))
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(p.soft_bg)
                    .child(match thumb {
                        Some(t) => img(t)
                            .size_full()
                            .object_fit(ObjectFit::Contain)
                            .into_any_element(),
                        None => div()
                            .text_size(px(10.))
                            .text_color(p.muted)
                            .child("…")
                            .into_any_element(),
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(entry.name.clone()),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "storyboard-library-menu-{}-{id}",
                            scope.key()
                        )))
                        .label("···")
                        .accessibility_label(format!("{} options", entry.name))
                        .xsmall()
                        .ghost()
                        .dropdown_menu(move |menu, _, _| {
                            let item = |label: &'static str,
                                        run: fn(
                                &mut EditorView,
                                Entry,
                                &mut Window,
                                &mut Context<EditorView>,
                            )| {
                                let owner = owner.clone();
                                let entry = menu_entry.clone();
                                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                    owner
                                        .update(cx, |this, cx| run(this, entry.clone(), window, cx))
                                        .ok();
                                })
                            };
                            menu.item(item("Place", |this, e, _, cx| {
                                this.library_place(e.scope, e.id, None, cx)
                            }))
                            .item(item(
                                if menu_entry.scope == Scope::Project {
                                    "Copy to personal library"
                                } else {
                                    "Copy to this storyboard"
                                },
                                |this, e, _, cx| this.library_copy(e.scope, e.id, cx),
                            ))
                            .item(item("Rename…", |this, e, window, cx| {
                                this.library_rename_dialog(e, window, cx)
                            }))
                            .separator()
                            .item(item("Delete", |this, e, window, cx| {
                                this.library_delete_dialog(e, window, cx)
                            }))
                        }),
                    ),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .text_ellipsis()
                    .child(detail),
            )
            .into_any_element()
    }
}

fn split_tags(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests;
