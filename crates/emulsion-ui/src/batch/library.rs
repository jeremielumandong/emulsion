//! Local collection and metadata controls retain the existing batch/develop flow.
use super::*;
use emulsion_io::creative_library::{self as catalog, AssetKind, Catalog};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
type FolderRows = (u64, Arc<Vec<(PathBuf, usize)>>);
#[derive(Default)]
pub(super) struct LibraryUi {
    pub(super) catalog: Catalog,
    pub(super) folder_rows: Option<FolderRows>,
    pub(super) photo_index: Option<emulsion_io::photo_index::Index>,
    pub(super) metadata_undo: Vec<Vec<(catalog::Asset, catalog::Asset)>>,
    pub(super) advance_to: Option<PathBuf>,
    pub(super) source_paths: Option<Vec<PathBuf>>,
    pub(super) raw_only: bool,
    pub(super) unedited: bool,
    pub(super) reverse: bool,
    pub(super) capture_sort: bool,
    pub(super) info_busy: bool,
    metadata_all_pending: bool,
    pub(super) metadata:
        std::collections::HashMap<PathBuf, Option<emulsion_core::document::ImageInfo>>,
    pub(super) loaded: bool,
    pub(super) loading: bool,
    removing: bool,
    pub(super) importing: bool,
    pub(super) search: Option<Entity<InputState>>,
    pub(super) search_subscription: Option<Subscription>,
    pub(super) collection: Option<u64>,
    pub(super) rating: u8,
    pub(super) flagged: bool,
    pub(super) collapse_stacks: bool,
    pub(super) deduplicate: bool,
    pub(super) rejected: bool,
    pub(super) color_label: u8,
    pub(super) focus: Option<FocusHandle>,
}
impl Workspace {
    pub(crate) fn library_remove_photos_from(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.batch.library.removing
            || self.batch.library.importing
            || self.batch.running.is_some()
        {
            return;
        }
        let paths = self.library_paths();
        if paths.is_empty() {
            return;
        }
        self.batch.library.removing = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let removed = paths.clone();
            let result = cx.background_spawn(async move {
                catalog::update(&root, |catalog| {
                    emulsion_io::photo_catalog::remove(catalog, &removed);
                    Ok(())
                })
            }).await;
            this.update(cx, |this, cx| {
                this.batch.library.removing = false;
                match result {
                    Ok((catalog, ())) => {
                        if catalog.revision >= this.batch.library.catalog.revision {
                            this.batch.library.catalog = catalog;
                        }
                        if let Some(source) = &mut this.batch.library.source_paths {
                            source.retain(|p| !paths.contains(p));
                        }
                        if this.batch.develop.source.as_ref().is_some_and(|s| paths.contains(&s.source)) {
                            this.batch.develop.source = None;
                        }
                        // Keep pending drafts: autosave still persists their edits
                        // even after their catalog references have been removed.
                        this.library_show(cx);
                        this.batch.note = Some((format!("Removed {} photo(s) from Library. Original files and saved edits kept.", paths.len()).into(), false));
                    }
                    Err(error) => this.batch.note = Some((error.to_string().into(), true)),
                }
                cx.notify();
            }).ok();
        }).detach();
    }

    pub(super) fn pick_library_photos(&mut self, cx: &mut Context<Self>) {
        if self.batch.library.importing
            || self.batch.library.removing
            || self.batch.running.is_some()
        {
            return;
        }
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add photos to Library".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.library_import_photos_from(catalog::root(), paths, None, cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn library_import_photos_from(
        &mut self,
        root: PathBuf,
        paths: Vec<PathBuf>,
        folder: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.batch.library.importing
            || self.batch.library.removing
            || self.batch.running.is_some()
        {
            return;
        }
        let paths: Vec<_> = paths.into_iter().filter(|p| is_batch_input(p)).collect();
        if paths.is_empty() {
            self.batch.note = Some((
                "No supported photos found. Choose RAW, JPEG, PNG, TIFF or WebP images.".into(),
                true,
            ));
            cx.notify();
            return;
        }
        self.batch.library.importing = true;
        let deduplicate = self.batch.library.deduplicate;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move {
                catalog::update(&root, |c| emulsion_io::photo_catalog::import(c, &paths, deduplicate))
            }).await;
            this.update(cx, |this, cx| {
                this.batch.library.importing = false;
                match result {
                    Ok((catalog, imported)) => {
                        if catalog.revision >= this.batch.library.catalog.revision {
                            this.batch.library.catalog = catalog;
                            this.batch.library.photo_index = None;
                        }
                        this.batch.library.loaded = true;
                        this.batch.library.collection = None;
                        if let Some(folder) = folder {
                            this.batch.folder = Some(folder);
                            this.batch.library.source_paths = Some(imported.clone());
                        } else {
                            this.batch.library.source_paths = None;
                        }
                        this.library_show(cx);
                        let visible = this.batch.items.iter().filter(|i| imported.contains(&i.path)).count();
                        let hidden = imported.len().saturating_sub(visible);
                        let suffix = if hidden > 0 {
                            format!(" {hidden} hidden by the current filters; use Clear filters to see them.")
                        } else { String::new() };
                        this.batch.note = Some((format!("Added {} photo(s) to Library.{suffix}", imported.len()).into(), false));
                    }
                    Err(error) => this.batch.note = Some((error.to_string().into(), true)),
                }
                cx.notify();
            }).ok();
        }).detach();
    }

    pub(crate) fn refresh_imported_photo_library(&mut self, cx: &mut Context<Self>) {
        // Routine re-entry keeps cached thumbnails; explicit Refresh also
        // invalidates previews and rereads saved development settings.
        self.batch.library.loaded = false;
        self.library_load(cx);
    }

    pub(crate) fn library_refresh_from(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.batch.running.is_some()
            || self.batch.library.importing
            || self.batch.library.removing
        {
            return;
        }
        self.library_load_from(root, true, cx);
    }

    fn library_load(&mut self, cx: &mut Context<Self>) {
        if !self.batch.library.loaded {
            self.library_load_from(catalog::root(), false, cx);
        }
    }

    fn library_load_from(&mut self, root: PathBuf, refresh: bool, cx: &mut Context<Self>) {
        if self.batch.library.loading {
            return;
        }
        self.batch.library.loading = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let c = catalog::load(&root)?;
                    let index = emulsion_io::photo_index::Index::load(&root, &c);
                    Ok::<_, emulsion_io::IoError>((c, index))
                })
                .await;
            this.update(cx, |this, cx| {
                this.batch.library.loading = false;
                this.batch.library.loaded = true;
                match result {
                    Ok((c, index)) => {
                        if c.revision >= this.batch.library.catalog.revision {
                            this.batch.library.catalog = c;
                            this.batch.library.photo_index = Some(index);
                        }
                        if refresh {
                            this.invalidate_library_preview();
                            this.batch.develop.refresh_saved();
                            this.batch.library.metadata.clear();
                            for item in &mut this.batch.items {
                                item.thumb = None;
                            }
                        }
                        if refresh || this.batch.items.is_empty() {
                            this.library_show(cx);
                        }
                        if refresh && this.batch.note.is_none() {
                            this.batch.note =
                                Some(("Library refreshed. Unsaved edits kept.".into(), false));
                        }
                    }
                    Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn library_edit(
        &mut self,
        edit: impl FnOnce(&mut Catalog) -> emulsion_io::Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    catalog::update(&catalog::root(), |c| {
                        let before = c.assets.clone();
                        edit(c)?;
                        let by_id: std::collections::HashMap<_, _> =
                            c.assets.iter().map(|a| (a.id, a)).collect();
                        Ok(before
                            .into_iter()
                            .filter_map(|a| {
                                by_id
                                    .get(&a.id)
                                    .filter(|b| b.path == a.path && ***b != a)
                                    .map(|b| (a, (*b).clone()))
                            })
                            .collect::<Vec<_>>())
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((c, changes)) => {
                        if !changes.is_empty() {
                            this.batch.library.metadata_undo.push(changes);
                            if this.batch.library.metadata_undo.len() > 100 {
                                this.batch.library.metadata_undo.remove(0);
                            }
                        }
                        let membership_changed = c.assets.iter().map(|a| (&a.id, &a.path)).ne(this
                            .batch
                            .library
                            .catalog
                            .assets
                            .iter()
                            .map(|a| (&a.id, &a.path)))
                            || c.photos.stacks != this.batch.library.catalog.photos.stacks;
                        if c.revision >= this.batch.library.catalog.revision {
                            this.batch.library.catalog = c;
                        }
                        if this.batch.running.is_none()
                            && (membership_changed
                                || this.batch.library.rating > 0
                                || this.batch.library.flagged
                                || this.batch.library.rejected
                                || this.batch.library.color_label > 0
                                || this.batch.library.collapse_stacks
                                || this.batch.library.collection.is_some())
                        {
                            this.library_show(cx);
                        }
                        if let Some(next) = this.batch.library.advance_to.take()
                            && let Some(index) =
                                this.batch.items.iter().position(|i| i.path == next)
                        {
                            this.library_select(index, false, false, cx);
                        }
                        this.batch.note = Some(("Local library updated.".into(), false));
                    }
                    Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn library_paths(&self) -> Vec<PathBuf> {
        let mut paths = self
            .batch
            .items
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.path.clone())
            .collect::<Vec<_>>();
        if paths.is_empty()
            && let Some(item) = self.batch.current.and_then(|i| self.batch.items.get(i))
        {
            paths.push(item.path.clone());
        }
        paths
    }
    pub(super) fn library_show(&mut self, cx: &mut Context<Self>) {
        if self.batch.running.is_some() {
            self.batch.note = Some((
                "Finish the export before changing collections.".into(),
                true,
            ));
            cx.notify();
            return;
        }
        if self
            .batch
            .library
            .photo_index
            .as_ref()
            .is_none_or(|i| i.revision != self.batch.library.catalog.revision)
        {
            self.batch.library.photo_index = Some(emulsion_io::photo_index::Index::build(
                &self.batch.library.catalog,
            ));
        }
        let state = &self.batch.library;
        let query = state
            .search
            .as_ref()
            .map(|s| s.read(cx).value().to_lowercase())
            .unwrap_or_default();
        let members = state
            .collection
            .and_then(|id| state.catalog.collections.iter().find(|c| c.id == id));
        let matches = state.photo_index.as_ref().unwrap().search(
            &query,
            state.rating,
            state.color_label,
            state.flagged,
            state.rejected,
        );
        let photo_index: std::collections::HashMap<_, _> = state
            .catalog
            .assets
            .iter()
            .filter(|a| a.kind == AssetKind::Image)
            .map(|a| (a.path.as_path(), a))
            .collect();
        let mut missing = 0;
        let source = state.source_paths.clone().unwrap_or_else(|| {
            state
                .catalog
                .assets
                .iter()
                .filter(|a| a.kind == AssetKind::Image)
                .map(|a| a.path.clone())
                .collect()
        });
        let mut paths = source
            .into_iter()
            .filter(|path| {
                let asset = photo_index.get(path.as_path()).copied();
                let raw = emulsion_io::photo_develop::is_raw_photo(path);
                let edited =
                    raw && emulsion_io::raw_settings::sidecar_path(path).is_ok_and(|p| p.exists());
                asset.is_none_or(|a| matches.contains(&a.id))
                    && asset.map_or(0, |a| a.rating) >= state.rating
                    && (!state.flagged || asset.is_some_and(|a| a.flagged))
                    && (!state.rejected || asset.is_some_and(|a| a.rejected))
                    && (state.color_label == 0
                        || asset.is_some_and(|a| a.color_label == state.color_label))
                    && (!state.raw_only || raw)
                    && (!state.unedited || !edited)
                    && members.is_none_or(|c| {
                        asset.is_some_and(|a| {
                            state
                                .catalog
                                .photos
                                .smart
                                .get(&c.id)
                                .map_or_else(|| c.assets.contains(&a.id), |r| r.matches(a))
                        })
                    })
                    && (!state.collapse_stacks
                        || asset.is_none_or(|a| {
                            !state
                                .catalog
                                .photos
                                .stacks
                                .iter()
                                .any(|(top, members)| *top != a.id && members.contains(&a.id))
                        }))
                    && (query.is_empty()
                        || path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&query)
                        || asset.is_some_and(|a| {
                            a.tags.iter().any(|t| t.to_lowercase().contains(&query))
                        }))
            })
            .filter(|p| {
                if p.is_file() {
                    true
                } else {
                    missing += 1;
                    true
                }
            })
            .collect::<Vec<_>>();
        paths.sort_by_key(|p| {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let taken = if state.capture_sort {
                state
                    .metadata
                    .get(p)
                    .and_then(|v| v.as_ref())
                    .map(|v| v.taken.clone())
                    .filter(|s| !s.is_empty())
            } else {
                None
            };
            (
                state.capture_sort && taken.is_none(),
                taken.unwrap_or_default(),
                name,
            )
        });
        if state.reverse {
            paths.reverse();
        }
        let source_paths = state.source_paths.clone();
        let out_dir = self.batch.out_dir.clone();
        let loupe = self.batch.develop.loupe;
        let compare = self.batch.develop.compare;
        let selected: HashSet<_> = self
            .batch
            .items
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.path.clone())
            .collect();
        let current = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone());
        let folder = paths
            .first()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .or_else(|| self.batch.folder.clone())
            .unwrap_or_else(catalog::root);
        self.load_batch(folder, paths, cx);
        self.batch.library.source_paths = source_paths;
        self.batch.develop.loupe = loupe;
        self.batch.develop.compare = compare;
        if out_dir.is_some() {
            self.batch.out_dir = out_dir;
        }
        for item in &mut self.batch.items {
            item.selected = selected.contains(&item.path);
        }
        self.batch.current =
            current.and_then(|p| self.batch.items.iter().position(|i| i.path == p));
        if missing > 0 {
            self.batch.note=Some((format!("{missing} offline photo(s). Reconnect the drive or use Relink folder root. Available proxies remain editable.").into(),true));
        }
    }
    fn library_metadata(&mut self, collection: bool, window: &mut Window, cx: &mut Context<Self>) {
        let paths = self.library_paths();
        if paths.is_empty() && !collection {
            self.batch.note = Some(("Select one or more photos first.".into(), true));
            cx.notify();
            return;
        }
        let known = paths
            .first()
            .and_then(|p| p.canonicalize().ok())
            .and_then(|p| {
                self.batch
                    .library
                    .catalog
                    .assets
                    .iter()
                    .find(|a| a.path == p)
            });
        let values = if collection {
            vec![String::new()]
        } else {
            vec![
                known.map(|a| a.tags.join(", ")).unwrap_or_default(),
                known.map_or(0, |a| a.rating).to_string(),
                known.is_some_and(|a| a.flagged).to_string(),
            ]
        };
        let fields = values
            .into_iter()
            .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)))
            .collect::<Vec<_>>();
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let owner = owner.clone();
            let inputs = fields.clone();
            let paths = paths.clone();
            let labels = if collection {
                vec!["Collection name"]
            } else {
                vec![
                    "Keywords · comma separated (replaces selected keywords)",
                    "Rating · 0–5",
                    "Flagged · true / false",
                ]
            };
            dialog
                .title(if collection {
                    "Create collection from selection"
                } else {
                    "Selected photo metadata"
                })
                .width(px(460.))
                .child(
                    div().flex().flex_col().gap_2().children(
                        labels
                            .into_iter()
                            .zip(&fields)
                            .map(|(label, input)| div().child(label).child(Input::new(input))),
                    ),
                )
                .footer(crate::widgets::form_dialog_footer("Save"))
                .on_ok(move |_, _, cx| {
                    let values = inputs
                        .iter()
                        .map(|i| i.read(cx).value().to_string())
                        .collect::<Vec<_>>();
                    if collection && values[0].trim().is_empty() {
                        return false;
                    }
                    let tags = values[0]
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>();
                    let (rating, flagged) = if collection {
                        (0, false)
                    } else {
                        let (Ok(rating), Ok(flagged)) =
                            (values[1].parse::<u8>(), values[2].parse::<bool>())
                        else {
                            return false;
                        };
                        if rating > 5 {
                            return false;
                        }
                        (rating, flagged)
                    };
                    let paths = paths.clone();
                    let name = values[0].trim().to_string();
                    owner
                        .update(cx, |this, cx| {
                            this.library_edit(
                                move |c| {
                                    let mut ids = Vec::new();
                                    for path in paths {
                                        let id = c.add_asset(path, AssetKind::Image)?;
                                        ids.push(id);
                                        if !collection {
                                            let a =
                                                c.assets.iter_mut().find(|a| a.id == id).unwrap();
                                            a.tags = tags.clone();
                                            a.rating = rating;
                                            a.flagged = flagged;
                                            if flagged {
                                                a.rejected = false;
                                            }
                                        }
                                    }
                                    if collection {
                                        c.add_collection(name, ids)?;
                                    }
                                    Ok(())
                                },
                                cx,
                            )
                        })
                        .ok();
                    true
                })
        });
    }
    fn library_add_to_collection(&mut self, id: u64, cx: &mut Context<Self>) {
        let paths = self.library_paths();
        self.library_edit(
            move |c| {
                let ids = paths
                    .into_iter()
                    .map(|p| c.add_asset(p, AssetKind::Image))
                    .collect::<emulsion_io::Result<Vec<_>>>()?;
                let collection =
                    c.collections
                        .iter_mut()
                        .find(|v| v.id == id)
                        .ok_or_else(|| {
                            emulsion_io::IoError::Manifest("Collection no longer exists.".into())
                        })?;
                collection.assets.extend(ids);
                collection.assets.sort_unstable();
                collection.assets.dedup();
                Ok(())
            },
            cx,
        );
    }
    pub(super) fn library_controls(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.library_load(cx);
        self.library_read_metadata(false, cx);
        self.batch
            .library
            .focus
            .get_or_insert_with(|| cx.focus_handle());
        if self.batch.library.search.is_none() {
            let input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Search names / keywords"));
            self.batch.library.search_subscription =
                Some(cx.subscribe(&input, |this, _, event, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.library_show(cx);
                    }
                }));
            self.batch.library.search = Some(input);
        }
        let selected = self.batch.library.collection;
        let owner = cx.weak_entity();
        let collections = self.batch.library.catalog.collections.clone();
        let actions = Button::new("library-manage")
            .label("Organize ▾")
            .small()
            .outline()
            .dropdown_menu(move |mut menu, _, _| {
                let index = owner.clone();
                menu = menu.item(PopupMenuItem::new("Add this folder to library").on_click(
                    move |_, _, cx| {
                        index
                            .update(cx, |this, cx| {
                                let paths = this
                                    .batch
                                    .items
                                    .iter()
                                    .map(|i| i.path.clone())
                                    .collect::<Vec<_>>();
                                this.library_edit(
                                    move |c| {
                                        for path in paths {
                                            c.add_asset(path, AssetKind::Image)?;
                                        }
                                        Ok(())
                                    },
                                    cx,
                                );
                            })
                            .ok();
                    },
                ));
                let create = owner.clone();
                menu = menu.item(
                    PopupMenuItem::new("Create collection from selection…").on_click(
                        move |_, window, cx| {
                            create
                                .update(cx, |this, cx| this.library_metadata(true, window, cx))
                                .ok();
                        },
                    ),
                );
                let metadata = owner.clone();
                menu = menu.item(
                    PopupMenuItem::new("Edit selected keywords / rating / flag…").on_click(
                        move |_, window, cx| {
                            metadata
                                .update(cx, |this, cx| this.library_metadata(false, window, cx))
                                .ok();
                        },
                    ),
                );
                for c in &collections {
                    let owner = owner.clone();
                    let id = c.id;
                    menu = menu.item(
                        PopupMenuItem::new(format!("Add selection to {}", c.name)).on_click(
                            move |_, _, cx| {
                                owner
                                    .update(cx, |this, cx| this.library_add_to_collection(id, cx))
                                    .ok();
                            },
                        ),
                    );
                }
                if let Some(id) = selected {
                    let owner = owner.clone();
                    menu = menu.separator().item(
                        PopupMenuItem::new("Remove this collection (keep files)").on_click(
                            move |_, _, cx| {
                                owner
                                    .update(cx, |this, cx| {
                                        this.batch.library.collection = None;
                                        this.library_edit(
                                            move |c| {
                                                c.collections.retain(|v| v.id != id);
                                                Ok(())
                                            },
                                            cx,
                                        );
                                    })
                                    .ok();
                            },
                        ),
                    );
                }
                let refresh = owner.clone();
                menu.item(
                    PopupMenuItem::new("Reload library").on_click(move |_, _, cx| {
                        refresh
                            .update(cx, |this, cx| {
                                this.library_refresh_from(catalog::root(), cx);
                            })
                            .ok();
                    }),
                )
            });
        let owner = cx.weak_entity();
        let filters = Button::new("library-filters")
            .label(format!(
                "{}★+{} ▾",
                self.batch.library.rating,
                if self.batch.library.flagged {
                    " · flagged"
                } else {
                    ""
                }
            ))
            .small()
            .ghost()
            .dropdown_menu(move |mut menu, _, _| {
                for rating in 0..=5 {
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(if rating == 0 {
                            "Any rating".into()
                        } else {
                            format!("{rating} stars and above")
                        })
                        .on_click(move |_, _, cx| {
                            owner
                                .update(cx, |this, cx| {
                                    this.batch.library.rating = rating;
                                    this.library_show(cx);
                                })
                                .ok();
                        }),
                    );
                }
                let owner = owner.clone();
                menu.item(
                    PopupMenuItem::new("Toggle flagged only").on_click(move |_, _, cx| {
                        owner
                            .update(cx, |this, cx| {
                                this.batch.library.flagged = !this.batch.library.flagged;
                                if this.batch.library.flagged {
                                    this.batch.library.rejected = false;
                                }
                                this.library_show(cx);
                            })
                            .ok();
                    }),
                )
            });
        let p = classic::palette(cx);
        let mut rows = div().flex().flex_col().gap_1().child(
            div()
                .flex()
                .justify_between()
                .child(classic::heading("Catalog", cx))
                .child(
                    Button::new("library-new-collection")
                        .label("+")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.library_metadata(true, window, cx)
                        })),
                ),
        );
        rows = rows.child(
            Button::new("library-all-photos")
                .label(format!(
                    "All photos · {}",
                    self.batch
                        .library
                        .catalog
                        .assets
                        .iter()
                        .filter(|a| a.kind == AssetKind::Image)
                        .count()
                ))
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.batch.library.collection = None;
                    this.batch.library.source_paths = None;
                    this.library_show(cx);
                })),
        );
        rows = rows.child(self.library_folder_panel(cx));
        rows = rows.child(classic::heading("Collections", cx));
        for collection in &self.batch.library.catalog.collections {
            let id = collection.id;
            rows = rows.child(
                Button::new(("library-collection-row", id))
                    .label(format!(
                        "{} · {}",
                        collection.name,
                        self.batch
                            .library
                            .catalog
                            .photos
                            .smart
                            .get(&collection.id)
                            .map_or(collection.assets.len(), |rule| self
                                .batch
                                .library
                                .catalog
                                .assets
                                .iter()
                                .filter(|a| rule.matches(a))
                                .count())
                    ))
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.library.collection = Some(id);
                        this.batch.library.source_paths = None;
                        this.library_show(cx);
                    })),
            );
        }
        rows = rows.child(classic::heading("Attribute filters", cx));
        let mut stars = div().flex().flex_wrap().gap_1();
        for rating in 0..=5u8 {
            stars = stars.child(
                Button::new(("library-rating-filter", rating as usize))
                    .label(if rating == 0 {
                        "All".into()
                    } else {
                        format!("{rating}★")
                    })
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.library.rating = rating;
                        this.library_show(cx);
                    })),
            );
        }
        rows = rows.child(stars);
        for (id, title, on) in [
            (
                "library-filter-flagged",
                "Flagged",
                self.batch.library.flagged,
            ),
            (
                "library-filter-raw",
                "RAW only",
                self.batch.library.raw_only,
            ),
            (
                "library-filter-unedited",
                "Unedited RAW",
                self.batch.library.unedited,
            ),
        ] {
            rows = rows.child(
                Checkbox::new(id)
                    .label(title)
                    .checked(on)
                    .small()
                    .px_2()
                    .py_1()
                    .on_change(cx.listener(move |this, checked, _, cx| {
                        match id {
                            "library-filter-flagged" => {
                                this.batch.library.flagged = *checked;
                                if *checked {
                                    this.batch.library.rejected = false;
                                }
                            }
                            "library-filter-raw" => this.batch.library.raw_only = *checked,
                            _ => this.batch.library.unedited = *checked,
                        }
                        this.library_show(cx);
                    })),
            );
        }
        let mut colors = div().flex().gap_1();
        for (index, name, color) in COLORS {
            colors = colors.child(
                div()
                    .id(("library-color-filter", index as usize))
                    .h(px(16.))
                    .flex_1()
                    .rounded(px(3.))
                    .bg(rgb(color))
                    .border_2()
                    .border_color(if self.batch.library.color_label == index {
                        p.ink
                    } else {
                        rgb(color).into()
                    })
                    .cursor_pointer()
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(name).build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.library.color_label = if this.batch.library.color_label == index
                        {
                            0
                        } else {
                            index
                        };
                        this.library_show(cx);
                    })),
            );
        }
        rows = rows.child(colors).child(
            Checkbox::new("library-filter-rejected")
                .label("Rejected")
                .checked(self.batch.library.rejected)
                .small()
                .px_2()
                .py_1()
                .on_change(cx.listener(|this, checked, _, cx| {
                    this.batch.library.rejected = *checked;
                    if *checked {
                        this.batch.library.flagged = false;
                    }
                    this.library_show(cx);
                })),
        );
        rows = rows.child(
            Button::new("library-reset-filters")
                .label("Clear filters")
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, window, cx| {
                    let state = &mut this.batch.library;
                    state.rating = 0;
                    state.flagged = false;
                    state.rejected = false;
                    state.raw_only = false;
                    state.unedited = false;
                    state.color_label = 0;
                    if let Some(input) = state.search.clone() {
                        input.update(cx, |input, cx| input.set_value("", window, cx));
                    }
                    this.library_show(cx);
                })),
        );
        rows = rows.child(label("RAW presets", &p));
        for (i, name) in [
            "Clean neutral",
            "Warm recovery",
            "Mono contrast",
            "Strong contrast",
        ]
        .into_iter()
        .enumerate()
        {
            rows = rows.child(
                Button::new(("library-raw-preset", i))
                    .label(name)
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| this.library_raw_preset(i, cx))),
            );
        }
        div()
            .id("library-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child(rows)
            .child(
                Button::new("library-import-google-photos")
                    .label("Import from Google Photos…")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.cloud_import_photos(cx))),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(filters)
                    .child(actions)
                    .child(self.library_catalog_controls(cx)),
            )
            .into_any_element()
    }
}

impl Workspace {
    pub(super) fn library_badge(&self, path: &Path) -> String {
        let asset = self
            .batch
            .library
            .catalog
            .assets
            .iter()
            .find(|a| a.path == path);
        let stars = asset
            .map(|a| "★".repeat(a.rating as usize))
            .unwrap_or_default();
        let flag = if asset.is_some_and(|a| a.rejected) {
            "Rejected "
        } else if asset.is_some_and(|a| a.flagged) {
            "⚑ "
        } else {
            ""
        };
        let label = asset
            .and_then(|a| COLORS.iter().find(|(id, _, _)| *id == a.color_label))
            .map(|(_, name, _)| *name)
            .unwrap_or("");
        format!(
            "{}{}{} {label}",
            if emulsion_io::photo_develop::is_raw_photo(path) {
                "RAW "
            } else {
                ""
            },
            flag,
            stars
        )
    }
    pub(super) fn library_open_photo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.batch.develop.dirty() || self.batch.develop.saving {
            self.batch.note = Some((
                "Wait for Library edits to finish saving before opening in Photo.".into(),
                true,
            ));
            cx.notify();
            return;
        }
        if let Some(path) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone())
        {
            self.edit_library_photo_path(path, window, cx);
        }
    }
    pub(super) fn library_filmstrip(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = classic::palette(cx);
        let current = self.batch.current.unwrap_or(0);
        let start = current.saturating_sub(12);
        let end = (start + 25).min(self.batch.items.len());
        if self.batch.develop.loupe {
            self.batch.thumbs_visible = start..end;
            let owner = cx.weak_entity();
            cx.defer(move |cx| {
                owner.update(cx, |this, cx| this.batch_thumbs(cx)).ok();
            });
        }
        let mut strip = div()
            .id("library-filmstrip")
            .test_support()
            .h(px(82.))
            .bg(p.panel)
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .border_t_1()
            .border_color(p.line)
            .child(
                Button::new("library-previous")
                    .label("‹")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let i = this.batch.current.unwrap_or(0).saturating_sub(1);
                        this.library_select(i, false, false, cx);
                    })),
            );
        if self.batch.develop.loupe {
            strip =
                strip
                    .child(chip("batch-all", "All", false, &p).test_support().on_click(
                        cx.listener(|this, _, _, cx| {
                            for i in &mut this.batch.items {
                                i.selected = true;
                            }
                            cx.notify();
                        }),
                    ))
                    .child(
                        chip("batch-none", "None", false, &p)
                            .test_support()
                            .on_click(cx.listener(|this, _, _, cx| {
                                for i in &mut this.batch.items {
                                    i.selected = false;
                                }
                                cx.notify();
                            })),
                    );
        }
        let mut photos = div()
            .id("library-filmstrip-scroll")
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .flex()
            .gap_1();
        for i in start..end {
            let item = &self.batch.items[i];
            let mut tile = div()
                .id(("library-filmstrip-photo", i))
                .test_support()
                .flex_none()
                .w(px(80.))
                .h(px(62.))
                .border_2()
                .border_color(if self.batch.current == Some(i) {
                    p.accent
                } else {
                    p.line
                })
                .bg(p.stage)
                .rounded(px(4.))
                .p_1()
                .overflow_hidden()
                .cursor_pointer();
            if let Some(thumb) = &item.thumb {
                tile = tile.child(
                    img(ImageSource::Render(thumb.clone()))
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                );
            } else {
                tile = tile.child(mono(
                    item.path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    9.,
                    p.muted,
                ));
            }
            photos = photos.child(tile.on_click(cx.listener(
                move |this, e: &ClickEvent, _, cx| {
                    this.library_select(
                        i,
                        e.modifiers().shift,
                        e.modifiers().control || e.modifiers().platform,
                        cx,
                    )
                },
            )));
        }
        strip = strip.child(photos).child(
            Button::new("library-next")
                .label("›")
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    let i = (this.batch.current.unwrap_or(0) + 1)
                        .min(this.batch.items.len().saturating_sub(1));
                    this.library_select(i, false, false, cx);
                })),
        );
        strip.into_any_element()
    }
}

impl Workspace {
    pub(super) fn library_selection_controls(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = classic::palette(cx);
        let count = self.batch.items.iter().filter(|i| i.selected).count();
        if count == 0 {
            return div().into_any_element();
        }
        let mut row = div()
            .id("library-selection-actions")
            .test_support()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_3()
            .py_1()
            .bg(p.soft_bg)
            .border_b_1()
            .border_color(p.line)
            .child(mono(format!("{count} selected"), 10., p.ink));
        for rating in 0..=5u8 {
            row =
                row.child(
                    Button::new(("library-rate", rating as usize))
                        .label(if rating == 0 {
                            "Clear ★".into()
                        } else {
                            format!("{rating}★")
                        })
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.library_rate(Some(rating), None, cx)
                        })),
                );
        }
        row.child(
            Button::new("library-flag")
                .label("Flag")
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| this.library_rate(None, Some(true), cx))),
        )
        .child(
            Button::new("library-unflag")
                .label("Unflag")
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| this.library_rate(None, Some(false), cx))),
        )
        .child(
            Button::new("library-keywords")
                .label("Add keywords…")
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, window, cx| this.library_add_keywords(window, cx))),
        )
        .child(
            Button::new("library-reject")
                .label("Reject")
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| this.library_cull(None, Some(true), cx))),
        )
        .child(
            Button::new("library-label-clear")
                .label("Clear label")
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| this.library_cull(Some(0), None, cx))),
        )
        .children(COLORS.into_iter().map(|(index, name, color)| {
            div()
                .id(("library-label", index as usize))
                .size(px(16.))
                .rounded(px(3.))
                .bg(rgb(color))
                .cursor_pointer()
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(name).build(window, cx)
                })
                .on_click(
                    cx.listener(move |this, _, _, cx| this.library_cull(Some(index), None, cx)),
                )
        }))
        .child(
            Button::new("library-selection-sync")
                .label("Sync settings")
                .small()
                .outline()
                .on_click(cx.listener(|this, _, _, cx| this.library_save_develop(true, cx))),
        )
        .child(
            Button::new("library-remove-photos")
                .label("Remove from Library")
                .tooltip("Remove selected photos from Library; keep original files and saved edits · Delete")
                .small()
                .ghost()
                .disabled(self.batch.library.removing || self.batch.library.importing || self.batch.running.is_some())
                .on_click(cx.listener(|this, _, _, cx| this.library_remove_photos_from(catalog::root(), cx))),
        )
        .into_any_element()
    }
    fn library_rate(&mut self, rating: Option<u8>, flagged: Option<bool>, cx: &mut Context<Self>) {
        let paths = self.library_paths();
        self.library_edit(
            move |catalog| {
                for path in paths {
                    let id = catalog.add_asset(path, AssetKind::Image)?;
                    let asset = catalog.assets.iter_mut().find(|a| a.id == id).unwrap();
                    if let Some(rating) = rating {
                        asset.rating = rating;
                    }
                    if let Some(flagged) = flagged {
                        asset.flagged = flagged;
                        asset.rejected = false;
                    }
                }
                Ok(())
            },
            cx,
        );
    }
}

impl Workspace {
    pub(super) fn library_grid_tools(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = classic::palette(cx);
        let title = self
            .batch
            .library
            .collection
            .and_then(|id| {
                self.batch
                    .library
                    .catalog
                    .collections
                    .iter()
                    .find(|c| c.id == id)
            })
            .map(|c| c.name.clone())
            .unwrap_or_else(|| {
                if self.batch.library.source_paths.is_some() {
                    "Current folder".into()
                } else {
                    "All photos".into()
                }
            });
        div()
            .id("library-grid-tools")
            .test_support()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .bg(p.soft_bg)
            .border_b_1()
            .border_color(p.line)
            .child(label("Library Filter", &p))
            .child(mono(title, 10., p.muted))
            .child(div().flex_1())
            .children(
                self.batch
                    .library
                    .search
                    .as_ref()
                    .map(|input| div().w(px(180.)).child(Input::new(input).small())),
            )
            .child(
                Button::new("library-search")
                    .label("Search")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.library_show(cx))),
            )
            .child(
                Button::new("library-sort")
                    .label(if self.batch.library.reverse {
                        if self.batch.library.capture_sort {
                            "Capture time ↓"
                        } else {
                            "Filename ↓"
                        }
                    } else {
                        if self.batch.library.capture_sort {
                            "Capture time ↑"
                        } else {
                            "Filename ↑"
                        }
                    })
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.library.reverse = !this.batch.library.reverse;
                        this.library_show(cx);
                    })),
            )
            .child(
                Button::new("library-sort-capture")
                    .label(if self.batch.library.info_busy {
                        "Reading EXIF…"
                    } else if self.batch.library.capture_sort {
                        "Sort by filename"
                    } else {
                        "Sort by capture time"
                    })
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.batch.library.capture_sort {
                            this.batch.library.capture_sort = false;
                            this.library_show(cx);
                        } else {
                            this.batch.library.capture_sort = true;
                            this.library_read_metadata(true, cx);
                            this.library_show(cx);
                        }
                    })),
            )
            .into_any_element()
    }
    pub(super) fn library_info_panel(&self, keywords: bool, cx: &mut Context<Self>) -> AnyElement {
        let p = classic::palette(cx);
        let mut panel = div().flex().flex_col().gap_3();
        let Some(item) = self.batch.current.and_then(|i| self.batch.items.get(i)) else {
            return panel
                .child(mono("Select a photo.", 11., p.muted))
                .into_any_element();
        };
        let asset = self
            .batch
            .library
            .catalog
            .assets
            .iter()
            .find(|a| a.path == item.path);
        if !keywords {
            panel = panel
                .child(label("File", &p))
                .child(mono(
                    item.path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    11.,
                    p.ink,
                ))
                .child(mono(item.path.display().to_string(), 10., p.muted))
                .child(mono(
                    format!("Rating · {} / 5", asset.map_or(0, |a| a.rating)),
                    11.,
                    p.muted,
                ))
                .child(mono(
                    if asset.is_some_and(|a| a.flagged) {
                        "Flag · Pick"
                    } else {
                        "Flag · None"
                    },
                    11.,
                    p.muted,
                ));
        }
        if !keywords && let Some(Some(info)) = self.batch.library.metadata.get(&item.path) {
            panel = panel
                .child(label("Camera", &p))
                .child(mono(info.summary(), 11., p.muted))
                .child(mono(
                    format!(
                        "Captured · {}",
                        if info.taken.is_empty() {
                            "Unknown"
                        } else {
                            &info.taken
                        }
                    ),
                    10.,
                    p.muted,
                ));
        }
        panel
            .child(label("Keywords", &p))
            .child(mono(
                asset
                    .map(|a| a.tags.join(", "))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "No keywords".into()),
                11.,
                p.muted,
            ))
            .child(
                Button::new("library-edit-keywords")
                    .label("Edit selected metadata…")
                    .small()
                    .outline()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.library_metadata(false, window, cx)),
                    ),
            )
            .into_any_element()
    }
}

const COLORS: [(u8, &str, u32); 5] = [
    (1, "Red", 0xd93a1e),
    (2, "Yellow", 0xe9c46a),
    (3, "Green", 0x2a9d8f),
    (4, "Blue", 0x4a7bd0),
    (5, "Purple", 0x9b5de5),
];
impl Workspace {
    fn library_cull(&mut self, label: Option<u8>, rejected: Option<bool>, cx: &mut Context<Self>) {
        let paths = self.library_paths();
        self.library_edit(
            move |catalog| {
                for path in paths {
                    let id = catalog.add_asset(path, AssetKind::Image)?;
                    let asset = catalog.assets.iter_mut().find(|a| a.id == id).unwrap();
                    if let Some(label) = label {
                        asset.color_label = label;
                    }
                    if let Some(rejected) = rejected {
                        asset.rejected = rejected;
                        if rejected {
                            asset.flagged = false;
                        }
                    }
                }
                Ok(())
            },
            cx,
        );
    }
    pub(super) fn library_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .batch
            .library
            .focus
            .as_ref()
            .is_some_and(|f| f.is_focused(window))
        {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if event.keystroke.key == "tab" {
            self.batch.develop.panels_hidden = !self.batch.develop.panels_hidden;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if (modifiers.control || modifiers.platform)
            && event.keystroke.key == "z"
            && !self.batch.develop.module_develop
        {
            self.library_undo_metadata(cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.control || modifiers.platform || modifiers.alt {
            return;
        }
        let key = event.keystroke.key.as_str();
        if matches!(
            key,
            "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "p" | "u" | "x"
        ) && (self.batch.develop.auto_advance || modifiers.shift)
        {
            self.batch.library.advance_to = self
                .batch
                .current
                .and_then(|i| self.batch.items.get(i + 1))
                .map(|i| i.path.clone());
        }
        match key {
            "f5" => self.library_refresh_from(catalog::root(), cx),
            "delete" | "backspace" => self.library_remove_photos_from(catalog::root(), cx),
            "escape" => {
                self.batch.develop.canvas_tool = 0;
                self.batch.develop.canvas_points.clear();
                self.invalidate_library_preview();
                cx.notify();
            }
            "r" | "q" | "k" | "m" => {
                self.batch.develop.canvas_tool = match key {
                    "r" => 5,
                    "q" => 3,
                    "k" => 1,
                    _ => {
                        if modifiers.shift {
                            8
                        } else {
                            9
                        }
                    }
                };
                self.batch.develop.module_develop = true;
                self.batch.develop.loupe = true;
                self.batch.develop.detail_region = None;
                self.batch.develop.section = if key == "r" { 1 } else { 5 };
                self.invalidate_library_preview();
                cx.notify();
            }
            "6" => self.library_cull(Some(1), None, cx),
            "7" => self.library_cull(Some(2), None, cx),
            "8" => self.library_cull(Some(3), None, cx),
            "9" => self.library_cull(Some(4), None, cx),
            "o" => {
                self.batch.develop.mask_overlay = !self.batch.develop.mask_overlay;
                self.invalidate_library_preview();
                cx.notify();
            }
            "a" => {
                self.batch.develop.auto_advance = !self.batch.develop.auto_advance;
                cx.notify();
            }
            "0" => self.library_rate(Some(0), None, cx),
            "1" => self.library_rate(Some(1), None, cx),
            "2" => self.library_rate(Some(2), None, cx),
            "3" => self.library_rate(Some(3), None, cx),
            "4" => self.library_rate(Some(4), None, cx),
            "5" => self.library_rate(Some(5), None, cx),
            "p" => self.library_rate(None, Some(true), cx),
            "u" => self.library_rate(None, Some(false), cx),
            "x" => self.library_cull(None, Some(true), cx),
            "g" => {
                self.batch.develop.module_develop = false;
                self.batch.develop.loupe = false;
                cx.notify();
            }
            "e" | "d" => {
                self.batch.develop.module_develop = key == "d";
                self.batch.develop.loupe = true;
                cx.notify();
            }
            "left" => self.library_select(
                self.batch.current.unwrap_or(0).saturating_sub(1),
                modifiers.shift,
                false,
                cx,
            ),
            "right" => self.library_select(
                (self.batch.current.unwrap_or(0) + 1).min(self.batch.items.len().saturating_sub(1)),
                modifiers.shift,
                false,
                cx,
            ),
            "enter" => self.library_open_photo(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Workspace {
    pub(super) fn library_read_metadata(&mut self, all: bool, cx: &mut Context<Self>) {
        if self.batch.library.info_busy {
            self.batch.library.metadata_all_pending |= all;
            return;
        }
        let paths: Vec<_> = if all {
            self.batch.library.source_paths.clone().unwrap_or_else(|| {
                self.batch
                    .library
                    .catalog
                    .assets
                    .iter()
                    .filter(|a| a.kind == AssetKind::Image)
                    .map(|a| a.path.clone())
                    .collect()
            })
        } else {
            self.batch
                .current
                .and_then(|i| self.batch.items.get(i))
                .map(|i| vec![i.path.clone()])
                .unwrap_or_default()
        };
        let paths: Vec<_> = paths
            .into_iter()
            .filter(|p| !self.batch.library.metadata.contains_key(p))
            .collect();
        if paths.is_empty() {
            return;
        }
        self.batch.library.info_busy = true;
        cx.spawn(async move |this, cx| {
            let metadata = cx
                .background_spawn(async move {
                    paths
                        .into_iter()
                        .map(|path| {
                            let info = emulsion_io::exif::read(&path);
                            (path, info)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                this.batch.library.info_busy = false;
                this.batch.library.metadata.extend(metadata);
                if std::mem::take(&mut this.batch.library.metadata_all_pending) {
                    this.library_read_metadata(true, cx);
                }
                if this.batch.library.capture_sort {
                    this.library_show(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Workspace {
    fn library_add_keywords(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = self.library_paths();
        if paths.is_empty() {
            return;
        }
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Keywords, separated by commas"));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let input = input.clone();
            let submitted = input.clone();
            let paths = paths.clone();
            let owner = owner.clone();
            dialog
                .title("Add keywords to selected photos")
                .child(Input::new(&input))
                .footer(crate::widgets::form_dialog_footer("Add"))
                .on_ok(move |_, _, cx| {
                    let tags: Vec<_> = submitted
                        .read(cx)
                        .value()
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect();
                    if tags.is_empty() {
                        return false;
                    }
                    let paths = paths.clone();
                    owner
                        .update(cx, |this, cx| {
                            this.library_edit(
                                move |catalog| {
                                    for path in paths {
                                        let id = catalog.add_asset(path, AssetKind::Image)?;
                                        let asset =
                                            catalog.assets.iter_mut().find(|a| a.id == id).unwrap();
                                        for tag in &tags {
                                            if !asset.tags.contains(tag) {
                                                asset.tags.push(tag.clone());
                                            }
                                        }
                                    }
                                    Ok(())
                                },
                                cx,
                            )
                        })
                        .ok();
                    true
                })
        });
    }
}

impl Workspace {
    pub(super) fn library_undo_metadata(&mut self, cx: &mut Context<Self>) {
        let Some(changes) = self.batch.library.metadata_undo.pop() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let rollback = changes.clone();
            let result = cx
                .background_spawn(async move {
                    catalog::update(&catalog::root(), |catalog| {
                        for (_, expected) in &changes {
                            if catalog.assets.iter().find(|a| a.id == expected.id) != Some(expected)
                            {
                                return Err(emulsion_io::IoError::Manifest(
                                    "Photo metadata changed outside this undo history".into(),
                                ));
                            }
                        }
                        for (before, _) in changes {
                            if let Some(asset) =
                                catalog.assets.iter_mut().find(|a| a.id == before.id)
                            {
                                *asset = before;
                            }
                        }
                        Ok(())
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((catalog, _)) => {
                        this.batch.library.catalog = catalog;
                        this.library_show(cx);
                    }
                    Err(e) => {
                        this.batch.library.metadata_undo.push(rollback);
                        this.batch.note = Some((e.to_string().into(), true));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
