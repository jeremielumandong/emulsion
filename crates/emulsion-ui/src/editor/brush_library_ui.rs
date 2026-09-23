//! A desktop workspace for organizing brushes without changing the active tool.
use super::presets::LibraryState;
use super::*;
use emulsion_io::brush_library::{self as store, Catalog};
use gpui_kit::component::{
    ActiveTheme, Disableable, Selectable, Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::collections::HashSet;
use std::ops::Range;

#[derive(Clone)]
enum NameTarget {
    NewLibrary,
    NewSet(String),
    Library(String),
    Set(String),
    Brush(String),
}

pub(super) struct BrushWorkspace {
    pub owner: WeakEntity<EditorView>,
    pub library: Entity<LibraryState>,
    pub catalog: Catalog,
    pub studio: Option<Entity<super::brush_studio::BrushStudio>>,
    focus: FocusHandle,
    search: Entity<InputState>,
    query: String,
    library_id: String,
    set_id: Option<String>,
    filter: &'static str,
    selected: HashSet<String>,
    visible: Vec<String>,
    thumbnails: HashMap<String, Arc<RenderImage>>,
    pending: HashSet<String>,
    preview_revision: u64,
    naming: Option<(NameTarget, Entity<InputState>)>,
    error: Option<String>,
    undo: Vec<(u64, Catalog)>,
    _subscriptions: Vec<Subscription>,
}

impl BrushWorkspace {
    pub fn new(
        owner: WeakEntity<EditorView>,
        library: Entity<LibraryState>,
        selected: Option<String>,
        selected_library: Option<String>,
        selected_set: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let catalog = library.read(cx).catalog.clone();
        let set_id = selected_set.or_else(|| {
            selected
                .as_ref()
                .and_then(|id| catalog.brush(id).map(|b| b.set_id.clone()))
                .or_else(|| {
                    catalog
                        .sets
                        .iter()
                        .find(|s| s.builtin)
                        .map(|s| s.id.clone())
                })
        });
        let library_id = selected_library.unwrap_or_else(|| {
            catalog
                .sets
                .iter()
                .find(|s| Some(&s.id) == set_id.as_ref())
                .map(|s| s.library_id.clone())
                .unwrap_or_default()
        });
        let set_id = set_id.filter(|id| {
            catalog
                .sets
                .iter()
                .any(|set| &set.id == id && set.library_id == library_id)
        });
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search brushes and sets"));
        let subscriptions = vec![
            cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.query = input.read(cx).value().to_string();
                    this.refresh(cx);
                }
            }),
            cx.observe(&library, |this, library, cx| {
                let catalog = library.read(cx).catalog.clone();
                this.replace_catalog(catalog, cx);
            }),
        ];
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let error = library.read(cx).error.clone();
        let mut view = Self {
            owner,
            library,
            catalog,
            studio: None,
            focus,
            search,
            query: String::new(),
            library_id,
            set_id,
            filter: "set",
            selected: selected.into_iter().collect(),
            visible: vec![],
            thumbnails: HashMap::new(),
            pending: HashSet::new(),
            preview_revision: 0,
            naming: None,
            error,
            undo: vec![],
            _subscriptions: subscriptions,
        };
        view.refresh(cx);
        view
    }
    fn replace_catalog(&mut self, catalog: Catalog, cx: &mut Context<Self>) {
        if self.catalog.brushes != catalog.brushes {
            self.thumbnails.clear();
            self.pending.clear();
            self.preview_revision = self.preview_revision.wrapping_add(1);
        }
        self.catalog = catalog;
        self.refresh(cx);
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.selected.retain(|id| self.catalog.brush(id).is_some());
        if !self
            .catalog
            .libraries
            .iter()
            .any(|l| l.id == self.library_id)
        {
            self.library_id = self
                .catalog
                .libraries
                .first()
                .map(|l| l.id.clone())
                .unwrap_or_default();
        }
        if !self
            .catalog
            .sets
            .iter()
            .any(|s| Some(&s.id) == self.set_id.as_ref())
        {
            self.set_id = self
                .catalog
                .sets
                .iter()
                .find(|s| s.library_id == self.library_id)
                .map(|s| s.id.clone());
        }
        if let Some(set) = self
            .catalog
            .sets
            .iter()
            .find(|s| Some(&s.id) == self.set_id.as_ref())
        {
            self.library_id = set.library_id.clone();
        }
        let query = self.query.trim().to_lowercase();
        self.visible = self
            .catalog
            .brushes
            .iter()
            .filter(|b| {
                let set = self.catalog.sets.iter().find(|s| s.id == b.set_id);
                if !query.is_empty() {
                    return b.name.to_lowercase().contains(&query)
                        || b.note.to_lowercase().contains(&query)
                        || set.is_some_and(|s| s.name.to_lowercase().contains(&query));
                }
                match self.filter {
                    "recent" => self.catalog.recent.contains(&b.id),
                    "pinned" => self.catalog.pinned.contains(&b.id),
                    _ => Some(&b.set_id) == self.set_id.as_ref(),
                }
            })
            .map(|b| b.id.clone())
            .collect();
        if self.filter == "recent" && query.is_empty() {
            self.visible.sort_by_key(|id| {
                self.catalog
                    .recent
                    .iter()
                    .position(|r| r == id)
                    .unwrap_or(usize::MAX)
            });
        }
        cx.notify();
    }
    fn commit(&mut self, draft: Catalog, cx: &mut Context<Self>) -> bool {
        let old = self.library.read(cx).catalog.clone();
        match self.library.update(cx, |state, cx| state.commit(draft, cx)) {
            Ok(()) => {
                let catalog = self.library.read(cx).catalog.clone();
                self.replace_catalog(catalog, cx);
                self.undo.push((self.catalog.revision, old));
                self.error = None;
                self.refresh(cx);
                true
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                false
            }
        }
    }
    fn undo_change(&mut self, cx: &mut Context<Self>) {
        let Some((revision, mut draft)) = self.undo.last().cloned() else {
            return;
        };
        if revision != self.library.read(cx).catalog.revision {
            self.error=Some("The library changed since this operation. Undo is unavailable to protect newer edits.".into());
            cx.notify();
            return;
        }
        draft.revision = revision;
        match self.library.update(cx, |state, cx| state.commit(draft, cx)) {
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
            Ok(()) => {
                self.undo.pop();
                let catalog = self.library.read(cx).catalog.clone();
                self.replace_catalog(catalog, cx);
                if let Some((revision, _)) = self.undo.last_mut() {
                    *revision = self.catalog.revision;
                }
                self.error = None;
                cx.notify();
            }
        }
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.owner
            .update(cx, |owner, cx| {
                owner.select_brush_library(&self.library_id, cx);
                if let Some(set) = &self.set_id {
                    owner.select_brush_set(set, cx);
                }
                owner.brush_workspace = None;
                window.focus(&owner.canvas_focus, cx);
                cx.notify();
            })
            .ok();
    }
    fn select(&mut self, id: &str, multiple: bool, cx: &mut Context<Self>) {
        if multiple {
            if !self.selected.remove(id) {
                self.selected.insert(id.into());
            }
        } else {
            self.selected.clear();
            self.selected.insert(id.into());
            self.owner
                .update(cx, |owner, cx| owner.apply_brush_id(id, cx))
                .ok();
        }
        cx.notify();
    }
    fn name(
        &mut self,
        target: NameTarget,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        input.update(cx, |s, cx| s.focus(window, cx));
        self.naming = Some((target, input));
        cx.notify();
    }
    fn finish_name(&mut self, cx: &mut Context<Self>) {
        let Some((target, input)) = &self.naming else {
            return;
        };
        let value = input.read(cx).value().to_string();
        let mut draft = self.catalog.clone();
        let result = match target {
            NameTarget::NewLibrary => draft.create_library(&value).map(|_| ()),
            NameTarget::NewSet(parent) => draft.create_set(parent, &value).map(|_| ()),
            NameTarget::Library(id) => draft.rename_library(id, &value),
            NameTarget::Set(id) => draft.rename_set(id, &value),
            NameTarget::Brush(id) => draft.rename_brush(id, &value),
        };
        let created_library = matches!(target, NameTarget::NewLibrary)
            .then(|| draft.libraries.last().map(|l| l.id.clone()))
            .flatten();
        let created_set = matches!(target, NameTarget::NewSet(_))
            .then(|| draft.sets.last().map(|s| s.id.clone()))
            .flatten();
        match result {
            Ok(()) => {
                if self.commit(draft, cx) {
                    if let Some(id) = created_library {
                        self.library_id = id;
                        self.set_id = None;
                    }
                    if let Some(id) = created_set {
                        self.set_id = Some(id);
                    }
                    self.filter = "set";
                    self.refresh(cx);
                    self.naming = None;
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }
    fn target_set(&self, draft: &mut Catalog) -> Result<String, String> {
        if let Some(set) = draft
            .sets
            .iter()
            .find(|s| Some(&s.id) == self.set_id.as_ref() && s.library_id == self.library_id)
        {
            return Ok(set.id.clone());
        }
        draft
            .create_set(&self.library_id, "My brushes")
            .map_err(|e| e.to_string())
    }
    fn edit(&mut self, new: bool, window: &mut Window, cx: &mut Context<Self>) {
        // Input events can precede delivery of the catalog observer notification.
        let mut draft = self.library.read(cx).catalog.clone();
        let id = if new {
            let target = match self.target_set(&mut draft) {
                Ok(target) => target,
                Err(error) => {
                    self.error = Some(error);
                    cx.notify();
                    return;
                }
            };
            match draft.add_brush(
                &target,
                "New brush",
                emulsion_raster::paint::Brush::default(),
            ) {
                Ok(id) => id,
                Err(e) => {
                    self.error = Some(e.to_string());
                    cx.notify();
                    return;
                }
            }
        } else {
            let Some(id) = self.selected.iter().next().cloned() else {
                return;
            };
            id
        };
        if draft.brush(&id).is_none() {
            self.error = Some(
                "This brush was removed in another editor. Reload the library to continue.".into(),
            );
            cx.notify();
            return;
        }
        let parent = cx.entity().downgrade();
        self.studio =
            Some(cx.new(|cx| {
                super::brush_studio::BrushStudio::new(parent, draft, id, new, window, cx)
            }));
        cx.notify();
    }
    pub(super) fn finish_studio(
        &mut self,
        draft: Option<Catalog>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(draft) = draft
            && !self.commit(draft, cx)
        {
            return false;
        }
        self.studio = None;
        window.focus(&self.focus, cx);
        cx.notify();
        true
    }
    pub(super) fn complete_studio_save(
        &mut self,
        saved: Catalog,
        previous: Catalog,
        saved_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let saved_revision = saved.revision;
        self.library.update(cx, |state, cx| {
            if saved.revision >= state.catalog.revision {
                state.catalog = saved;
                cx.notify();
            }
        });
        let catalog = self.library.read(cx).catalog.clone();
        if catalog.revision == saved_revision {
            self.undo.push((catalog.revision, previous));
        }
        self.replace_catalog(catalog, cx);
        if let Some(brush) = self.catalog.brush(saved_id) {
            self.set_id = Some(brush.set_id.clone());
            self.filter = "set";
            self.query.clear();
            self.search
                .update(cx, |search, cx| search.set_value("", window, cx));
            self.selected.clear();
            self.selected.insert(saved_id.to_owned());
            self.refresh(cx);
            self.owner
                .update(cx, |owner, cx| owner.apply_committed_brush_id(saved_id, cx))
                .ok();
        }
        self.error = None;
        self.studio = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }
    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let mut draft = self.catalog.clone();
        let target = match self.target_set(&mut draft) {
            Ok(target) => target,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        for id in &self.selected {
            if let Err(e) = draft.duplicate_brush(id, &target) {
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        }
        self.commit(draft, cx);
    }
    fn delete(&mut self, cx: &mut Context<Self>) {
        let mut draft = self.catalog.clone();
        for id in &self.selected {
            if let Err(e) = draft.delete_brush(id) {
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        }
        self.commit(draft, cx);
    }
    fn move_selected(&mut self, set: &str, index: usize, cx: &mut Context<Self>) {
        let mut draft = self.catalog.clone();
        for (offset, id) in self
            .catalog
            .brushes
            .iter()
            .filter(|b| self.selected.contains(&b.id))
            .map(|b| &b.id)
            .enumerate()
        {
            if let Err(e) = draft.move_brush(id, set, index.saturating_add(offset)) {
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        }
        self.commit(draft, cx);
    }
    fn previews(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        let brushes: Vec<_> = ids
            .into_iter()
            .filter(|id| !self.thumbnails.contains_key(id) && !self.pending.contains(id))
            .take(24usize.saturating_sub(self.pending.len()))
            .filter_map(|id| {
                self.catalog
                    .brush(&id)
                    .map(|b| (id, b.brush, b.secondary, b.combine_mode))
            })
            .collect();
        if brushes.is_empty() {
            return;
        }
        for (id, _, _, _) in &brushes {
            self.pending.insert(id.clone());
        }
        let revision = self.preview_revision;
        cx.spawn(async move |this, cx| {
            let images = cx
                .background_spawn(async move {
                    brushes
                        .into_iter()
                        .map(|(id, brush, secondary, mode)| {
                            (id, stroke_preview(brush, secondary, mode))
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                if revision != this.preview_revision {
                    return;
                }
                if this.thumbnails.len() > 128 {
                    this.thumbnails.clear();
                }
                for (id, image) in images {
                    this.pending.remove(&id);
                    this.thumbnails.insert(id, image);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    fn export(&mut self, scope: store::ExportScope, cx: &mut Context<Self>) {
        let ids: Vec<_> = if self.selected.is_empty() {
            self.visible.clone()
        } else {
            self.catalog
                .brushes
                .iter()
                .filter(|brush| self.selected.contains(&brush.id))
                .map(|brush| brush.id.clone())
                .collect()
        };
        let catalog = self.catalog.clone();
        let rx = cx.prompt_for_new_path(&PathBuf::from("."), Some("brushes.embrushes"));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let result = cx
                .background_spawn(async move {
                    store::export_package_scoped(&path, &catalog, &ids, scope)
                        .map_err(|e| e.to_string())
                })
                .await;
            this.update(cx, |this, cx| {
                this.error = result.err();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for BrushWorkspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(studio) = &self.studio {
            return div().size_full().child(studio.clone()).into_any_element();
        }
        let active_mode = self
            .owner
            .upgrade()
            .map(|owner| {
                let owner = owner.read(cx);
                match owner.tool {
                    Tool::Heal => "Heal",
                    Tool::Clone => "Clone",
                    Tool::Mask => "Mask",
                    _ => match owner.paint_kind() {
                        PaintKind::Eraser => "Erase",
                        PaintKind::Smudge => "Smudge",
                        _ => "Paint",
                    },
                }
            })
            .unwrap_or("Paint");
        let theme = cx.theme();
        let (background, foreground, border, muted) = (
            theme.background,
            theme.foreground,
            theme.border,
            theme.muted_foreground,
        );
        let mut navigation = div()
            .id("brush-library-navigation")
            .overflow_y_scroll()
            .w_56()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .border_r_1()
            .border_color(border);
        for (key, label) in [("recent", "Recent"), ("pinned", "Pinned")] {
            navigation = navigation.child(
                Button::new(key)
                    .ghost()
                    .selected(self.filter == key)
                    .label(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.filter = key;
                        this.refresh(cx);
                    })),
            );
        }
        for library in &self.catalog.libraries {
            let id = library.id.clone();
            navigation = navigation.child(
                Button::new(SharedString::from(format!("library-{id}")))
                    .ghost()
                    .selected(self.library_id == id)
                    .label(library.name.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.library_id = id.clone();
                        this.set_id = this
                            .catalog
                            .sets
                            .iter()
                            .find(|s| s.library_id == id)
                            .map(|s| s.id.clone());
                        this.filter = "set";
                        this.refresh(cx);
                    })),
            );
            if library.id == self.library_id {
                for set in self
                    .catalog
                    .sets
                    .iter()
                    .filter(|s| s.library_id == library.id)
                {
                    let id = set.id.clone();
                    navigation = navigation.child(
                        Button::new(SharedString::from(format!("set-{id}")))
                            .ghost()
                            .small()
                            .selected(self.filter == "set" && self.set_id.as_ref() == Some(&id))
                            .label(set.name.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_id = Some(id.clone());
                                this.filter = "set";
                                this.refresh(cx);
                            })),
                    );
                }
            }
        }
        navigation = navigation
            .child(
                Button::new("new-library")
                    .label("New library…")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.name(NameTarget::NewLibrary, "New library".into(), window, cx)
                    })),
            )
            .child(
                Button::new("new-brush-set")
                    .label("New set…")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.name(
                            NameTarget::NewSet(this.library_id.clone()),
                            "New set".into(),
                            window,
                            cx,
                        )
                    })),
            );
        let library_owner = cx.entity().downgrade();
        if let Some(library) = self
            .catalog
            .libraries
            .iter()
            .find(|l| l.id == self.library_id)
            .cloned()
        {
            navigation = navigation.child(
                Button::new("library-actions")
                    .label("Library actions...")
                    .dropdown_menu(move |mut menu, _, _| {
                        for (op, label) in [
                            (0, "Rename library..."),
                            (1, "Duplicate library"),
                            (2, "Delete library"),
                            (3, "Move library up"),
                            (4, "Move library down"),
                        ] {
                            let owner = library_owner.clone();
                            let library = library.clone();
                            menu = menu.item(PopupMenuItem::new(label).on_click(
                                move |_, window, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            if op == 0 {
                                                this.name(
                                                    NameTarget::Library(library.id.clone()),
                                                    library.name.clone(),
                                                    window,
                                                    cx,
                                                );
                                                return;
                                            }
                                            let mut draft = this.catalog.clone();
                                            let result = match op {
                                                1 => {
                                                    draft.duplicate_library(&library.id).map(|_| ())
                                                }
                                                2 => draft.delete_library(&library.id),
                                                _ => {
                                                    let index = draft
                                                        .libraries
                                                        .iter()
                                                        .position(|l| l.id == library.id)
                                                        .unwrap_or(0);
                                                    draft.reorder_library(
                                                        &library.id,
                                                        if op == 3 {
                                                            index.saturating_sub(1)
                                                        } else {
                                                            index + 1
                                                        },
                                                    )
                                                }
                                            };
                                            match result {
                                                Ok(()) => {
                                                    this.commit(draft, cx);
                                                }
                                                Err(e) => {
                                                    this.error = Some(e.to_string());
                                                    cx.notify();
                                                }
                                            }
                                        })
                                        .ok();
                                },
                            ));
                        }
                        menu
                    }),
            );
        }
        if let Some(set) = self
            .catalog
            .sets
            .iter()
            .find(|s| Some(&s.id) == self.set_id.as_ref())
            .cloned()
        {
            let set_owner = cx.entity().downgrade();
            let set_menu = set.clone();
            let destinations: Vec<_> = self
                .catalog
                .libraries
                .iter()
                .filter(|l| !l.builtin)
                .map(|l| (l.id.clone(), l.name.clone()))
                .collect();
            navigation = navigation.child(
                Button::new("set-position-actions")
                    .label("Move set...")
                    .dropdown_menu(move |mut menu, _, _| {
                        for (up, label) in [(true, "Move set up"), (false, "Move set down")] {
                            let owner = set_owner.clone();
                            let set = set_menu.clone();
                            menu =
                                menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            let mut draft = this.catalog.clone();
                                            let index = draft
                                                .sets
                                                .iter()
                                                .filter(|s| s.library_id == set.library_id)
                                                .position(|s| s.id == set.id)
                                                .unwrap_or(0);
                                            let result = draft.move_set(
                                                &set.id,
                                                &set.library_id,
                                                if up {
                                                    index.saturating_sub(1)
                                                } else {
                                                    index + 1
                                                },
                                            );
                                            match result {
                                                Ok(()) => {
                                                    this.commit(draft, cx);
                                                }
                                                Err(e) => {
                                                    this.error = Some(e.to_string());
                                                    cx.notify();
                                                }
                                            }
                                        })
                                        .ok();
                                }));
                        }
                        for (id, name) in &destinations {
                            let owner = set_owner.clone();
                            let set = set_menu.clone();
                            let id = id.clone();
                            menu =
                                menu.item(PopupMenuItem::new(format!("Move to {name}")).on_click(
                                    move |_, _, cx| {
                                        owner
                                            .update(cx, |this, cx| {
                                                let mut draft = this.catalog.clone();
                                                match draft.move_set(&set.id, &id, usize::MAX) {
                                                    Ok(()) => {
                                                        this.commit(draft, cx);
                                                    }
                                                    Err(e) => {
                                                        this.error = Some(e.to_string());
                                                        cx.notify();
                                                    }
                                                }
                                            })
                                            .ok();
                                    },
                                ));
                        }
                        menu
                    }),
            );
            let rename = set.clone();
            let duplicate = set.clone();
            let delete = set.clone();
            navigation = navigation
                .child(
                    Button::new("rename-set")
                        .ghost()
                        .label("Rename set…")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.name(
                                NameTarget::Set(rename.id.clone()),
                                rename.name.clone(),
                                window,
                                cx,
                            )
                        })),
                )
                .child(
                    Button::new("duplicate-set")
                        .ghost()
                        .label("Duplicate set")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let mut draft = this.catalog.clone();
                            match draft.duplicate_set(&duplicate.id, "library:user") {
                                Ok(_) => {
                                    this.commit(draft, cx);
                                }
                                Err(e) => {
                                    this.error = Some(e.to_string());
                                    cx.notify();
                                }
                            }
                        })),
                )
                .child(
                    Button::new("delete-set")
                        .ghost()
                        .disabled(set.builtin)
                        .label("Delete set")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let mut draft = this.catalog.clone();
                            match draft.delete_set(&delete.id) {
                                Ok(()) => {
                                    this.commit(draft, cx);
                                }
                                Err(e) => {
                                    this.error = Some(e.to_string());
                                    cx.notify();
                                }
                            }
                        })),
                );
        }
        let visible = self.visible.clone();
        let list = uniform_list(
            "brush-library-rows",
            visible.len(),
            cx.processor(move |this, range: Range<usize>, window, cx| {
                let ids = visible[range.clone()].to_vec();
                cx.defer_in(window, move |this, _, cx| this.previews(ids, cx));
                range
                    .filter_map(|i| {
                        let id = visible[i].clone();
                        let brush = this.catalog.brush(&id)?.clone();
                        let selected = this.selected.contains(&id);
                        let location = this
                            .catalog
                            .sets
                            .iter()
                            .find(|s| s.id == brush.set_id)
                            .map(|s| s.name.clone())
                            .unwrap_or_default();
                        let mut row = div()
                            .id(SharedString::from(format!("brush-row-{id}")))
                            .test_support()
                            .h_24()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .border_b_1()
                            .border_color(cx.theme().border);
                        if let Some(image) = this.thumbnails.get(&id) {
                            row = row.child(
                                img(ImageSource::Render(image.clone()))
                                    .w_48()
                                    .h_16()
                                    .object_fit(ObjectFit::Contain),
                            );
                        } else {
                            row = row.child(
                                div()
                                    .w_48()
                                    .h_16()
                                    .flex_none()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Preview…"),
                            );
                        }
                        Some(
                            row.child(
                                Button::new(SharedString::from(format!("choose-{id}")))
                                    .ghost()
                                    .selected(selected)
                                    .label(brush.name)
                                    .on_click(cx.listener(
                                        move |this, event: &ClickEvent, window, cx| {
                                            this.select(
                                                &id,
                                                event.modifiers().control
                                                    || event.modifiers().shift,
                                                cx,
                                            );
                                            if event.click_count() == 2 {
                                                this.edit(false, window, cx);
                                            }
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(location),
                            ),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .flex_1()
        .min_h_0();
        let one = self.selected.len() == 1;
        let selected_id = self
            .catalog
            .brushes
            .iter()
            .find(|b| self.selected.contains(&b.id))
            .map(|b| b.id.clone());
        let mut actions = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                Button::new("new-brush")
                    .label("New brush…")
                    .on_click(cx.listener(|this, _, window, cx| this.edit(true, window, cx))),
            )
            .child(
                Button::new("edit-brush")
                    .label("Brush Studio…")
                    .disabled(!one)
                    .on_click(cx.listener(|this, _, window, cx| this.edit(false, window, cx))),
            )
            .child(
                Button::new("duplicate-brush")
                    .label("Duplicate")
                    .disabled(self.selected.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| this.duplicate(cx))),
            )
            .child(
                Button::new("delete-brush")
                    .label("Delete")
                    .disabled(self.selected.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| this.delete(cx))),
            )
            .child(
                Button::new("import-brushes")
                    .label("Import…")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.owner
                            .update(cx, |owner, cx| owner.import_brushes(cx))
                            .ok();
                    })),
            )
            .child(
                Button::new("export-brushes")
                    .label("Export…")
                    .disabled(self.visible.is_empty())
                    .on_click(
                        cx.listener(|this, _, _, cx| this.export(store::ExportScope::Brushes, cx)),
                    ),
            );
        let export_owner = cx.entity().downgrade();
        actions = actions.child(
            Button::new("export-collection")
                .label("Export set or library...")
                .dropdown_menu(move |mut menu, _, _| {
                    for (whole, label) in [
                        (false, "Export current set"),
                        (true, "Export current library"),
                    ] {
                        let owner = export_owner.clone();
                        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                            owner
                                .update(cx, |this, cx| {
                                    let scope = if whole {
                                        store::ExportScope::Library(this.library_id.clone())
                                    } else {
                                        let Some(id) = this.set_id.clone() else {
                                            return;
                                        };
                                        store::ExportScope::Set(id)
                                    };
                                    this.export(scope, cx);
                                })
                                .ok();
                        }));
                    }
                    menu
                }),
        );
        if let Some(id) = selected_id {
            let pinned = self
                .selected
                .iter()
                .all(|id| self.catalog.pinned.contains(id));
            let name = self
                .catalog
                .brush(&id)
                .map(|b| b.name.clone())
                .unwrap_or_default();
            actions = actions
                .child(
                    Button::new("rename-brush")
                        .label("Rename…")
                        .disabled(!one)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.name(NameTarget::Brush(id.clone()), name.clone(), window, cx)
                        })),
                )
                .child(
                    Button::new("pin-brush")
                        .label(if pinned { "Unpin" } else { "Pin" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let mut draft = this.catalog.clone();
                            for id in &this.selected {
                                let _ = draft.pin(id, !pinned);
                            }
                            this.commit(draft, cx);
                        })),
                );
        }
        actions = actions
            .child(
                Button::new("combine-brushes")
                    .label("Combine")
                    .disabled(self.selected.len() != 2)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let ids: Vec<_> = this
                            .catalog
                            .brushes
                            .iter()
                            .filter(|b| this.selected.contains(&b.id))
                            .map(|b| b.id.clone())
                            .collect();
                        if ids.len() != 2 {
                            return;
                        }
                        let mut draft = this.catalog.clone();
                        match draft.combine_brushes(&ids[0], &ids[1]) {
                            Ok(id) => {
                                if this.commit(draft, cx) {
                                    this.selected = HashSet::from([id]);
                                }
                            }
                            Err(e) => {
                                this.error = Some(e.to_string());
                                cx.notify();
                            }
                        }
                    })),
            )
            .child(
                Button::new("uncombine-brush")
                    .label("Uncombine")
                    .disabled(
                        !one || !self.selected.iter().any(|id| {
                            self.catalog
                                .brush(id)
                                .is_some_and(|b| b.secondary.is_some())
                        }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        let Some(id) = this.selected.iter().next().cloned() else {
                            return;
                        };
                        let mut draft = this.catalog.clone();
                        match draft.uncombine_brush(&id) {
                            Ok((a, b)) => {
                                if this.commit(draft, cx) {
                                    this.selected = HashSet::from([a, b]);
                                }
                            }
                            Err(e) => {
                                this.error = Some(e.to_string());
                                cx.notify();
                            }
                        }
                    })),
            );
        for (key, label, up) in [
            ("brush-up", "Move up", true),
            ("brush-down", "Move down", false),
        ] {
            actions = actions.child(
                Button::new(key)
                    .label(label)
                    .disabled(!one || self.filter != "set" || !self.query.is_empty())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let Some(id) = this.selected.iter().next() else {
                            return;
                        };
                        let Some(index) = this.visible.iter().position(|v| v == id) else {
                            return;
                        };
                        let Some(set) = this.set_id.clone() else {
                            return;
                        };
                        this.move_selected(
                            &set,
                            if up {
                                index.saturating_sub(1)
                            } else {
                                (index + 1).min(this.visible.len().saturating_sub(1))
                            },
                            cx,
                        );
                    })),
            );
        }
        actions = actions.child(
            Button::new("reload-brush-library")
                .label("Reload library")
                .on_click(cx.listener(|this, _, _, cx| {
                    match store::load_with_report() {
                        Ok(report) => {
                            this.library.update(cx, |state, cx| {
                                state.catalog = report.catalog;
                                state.warnings = report.warnings;
                                state.error = None;
                                cx.notify();
                            });
                            this.error = None;
                            this.undo.clear();
                        }
                        Err(error) => {
                            this.error = Some(error.to_string());
                        }
                    }
                    cx.notify();
                })),
        );
        actions = actions.child(
            Button::new("undo-library")
                .label("Undo library change")
                .disabled(self.undo.is_empty())
                .on_click(cx.listener(|this, _, _, cx| this.undo_change(cx))),
        );
        let move_owner = cx.entity().downgrade();
        let targets: Vec<_> = self
            .catalog
            .sets
            .iter()
            .filter(|s| !s.builtin)
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();
        let move_row = Button::new("move-brushes")
            .label("Move selected to...")
            .disabled(self.selected.is_empty())
            .dropdown_menu(move |mut menu, _, _| {
                for (id, name) in &targets {
                    let owner = move_owner.clone();
                    let id = id.clone();
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(move |_, _, cx| {
                        owner
                            .update(cx, |this, cx| this.move_selected(&id, usize::MAX, cx))
                            .ok();
                    }));
                }
                menu
            });
        let mut content = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap_3()
            .p_3()
            .child(super::presets::import_review(&self.library, cx))
            .child(Input::new(&self.search))
            .child(actions)
            .child(move_row);
        if let Some((_, input)) = &self.naming {
            content =
                content.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(Input::new(input))
                        .child(
                            Button::new("save-brush-name")
                                .primary()
                                .label("Save name")
                                .on_click(cx.listener(|this, _, _, cx| this.finish_name(cx))),
                        )
                        .child(Button::new("cancel-brush-name").label("Cancel").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.naming = None;
                                cx.notify();
                            }),
                        )),
                );
        }
        if !self.library.read(cx).warnings.is_empty() {
            content =
                content.child(
                    div()
                        .id("library-load-warnings")
                        .max_h_32()
                        .overflow_y_scroll()
                        .children(self.library.read(cx).warnings.iter().map(|warning| {
                            div().text_sm().text_color(muted).child(warning.clone())
                        })),
                );
        }
        if let Some(error) = &self.error {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        if self.visible.is_empty() {
            content =
                content.child(div().p_4().text_color(muted).child(
                    "No brushes here. Create a brush, import a set, or change your search.",
                ));
        }
        content = content.child(list);
        div()
            .id("brush-library-workspace")
            .test_support()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &crate::actions::Undo, _, cx| {
                this.undo_change(cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|_, _: &crate::actions::Redo, _, cx| cx.stop_propagation()))
            .flex()
            .flex_col()
            .size_full()
            .bg(background)
            .text_color(foreground)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    if this.naming.take().is_some() {
                        window.focus(&this.focus, cx);
                        cx.notify();
                    } else {
                        this.close(window, cx);
                    }
                    cx.stop_propagation();
                } else if this.focus.is_focused(window) {
                    match event.keystroke.key.as_str() {
                        "up" | "down" if !this.visible.is_empty() => {
                            let index = this
                                .visible
                                .iter()
                                .position(|id| this.selected.contains(id))
                                .unwrap_or(0);
                            let index = if event.keystroke.key == "up" {
                                index.saturating_sub(1)
                            } else {
                                (index + 1).min(this.visible.len() - 1)
                            };
                            let id = this.visible[index].clone();
                            this.select(&id, false, cx);
                            cx.stop_propagation();
                        }
                        "enter" if this.selected.len() == 1 => {
                            this.edit(false, window, cx);
                            cx.stop_propagation();
                        }
                        _ => {}
                    }
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_3()
                    .border_b_1()
                    .border_color(border)
                    .child(format!("Brush library - {active_mode}"))
                    .child(
                        Button::new("close-brush-library")
                            .label("Return to canvas")
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(navigation)
                    .child(content),
            )
            .into_any_element()
    }
}

/// A sample stroke drawn with this brush, for library rows and the gallery.
pub(super) fn stroke_preview(
    mut brush: emulsion_raster::paint::Brush,
    secondary: Option<emulsion_raster::paint::Brush>,
    mode: emulsion_raster::paint::DualBlend,
) -> Arc<RenderImage> {
    let largest = secondary.map_or(brush.size, |other| brush.size.max(other.size));
    let scale = (36. / largest.max(1.)).min(1.);
    brush.size *= scale;
    let base = Raster::from_srgba8(240, 64, &[245u8, 245, 245, 255].repeat(240 * 64));
    let samples = emulsion_raster::preview::sample_stroke(240, 64);
    let ink =
        emulsion_raster::preview::PreviewMode::Paint(color::srgba8_to_premul([35, 55, 75, 255]));
    let raster = if let Some(mut secondary) = secondary {
        secondary.size *= scale;
        emulsion_raster::preview::render_dual_stroke(
            Arc::new(base),
            brush,
            secondary,
            mode,
            ink,
            &samples,
            1,
        )
    } else {
        emulsion_raster::preview::render_stroke(Arc::new(base), brush, ink, &samples, 1)
    };
    super::brush_studio::raster_image(&raster)
}
