//! Local collection and metadata controls retain the existing batch/develop flow.
use super::*;
use emulsion_io::creative_library::{self as catalog, AssetKind, Catalog};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
#[derive(Default)]
pub(super) struct LibraryUi {
    catalog: Catalog,
    loaded: bool,
    loading: bool,
    search: Option<Entity<InputState>>,
    collection: Option<u64>,
    rating: u8,
    flagged: bool,
}
impl Workspace {
    pub(crate) fn refresh_imported_photo_library(&mut self, cx: &mut Context<Self>) {
        self.batch.library.loaded = false;
        self.library_load(cx);
    }
    fn library_load(&mut self, cx: &mut Context<Self>) {
        if self.batch.library.loaded || self.batch.library.loading {
            return;
        }
        self.batch.library.loading = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async { catalog::load(&catalog::root()) })
                .await;
            this.update(cx, |this, cx| {
                this.batch.library.loading = false;
                this.batch.library.loaded = true;
                match result {
                    Ok(c) => {
                        if c.revision >= this.batch.library.catalog.revision {
                            this.batch.library.catalog = c;
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
    fn library_edit(
        &mut self,
        edit: impl FnOnce(&mut Catalog) -> emulsion_io::Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { catalog::update(&catalog::root(), edit) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((c, _)) => {
                        if c.revision >= this.batch.library.catalog.revision {
                            this.batch.library.catalog = c;
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
    fn library_paths(&self) -> Vec<PathBuf> {
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
    fn library_show(&mut self, cx: &mut Context<Self>) {
        if self.batch.running.is_some() {
            self.batch.note = Some((
                "Finish the export before changing collections.".into(),
                true,
            ));
            cx.notify();
            return;
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
        let mut missing = 0;
        let paths = state
            .catalog
            .assets
            .iter()
            .filter(|a| {
                a.kind == AssetKind::Image
                    && a.rating >= state.rating
                    && (!state.flagged || a.flagged)
                    && members.is_none_or(|c| c.assets.contains(&a.id))
                    && (query.is_empty()
                        || a.name.to_lowercase().contains(&query)
                        || a.tags.iter().any(|t| t.to_lowercase().contains(&query)))
            })
            .filter_map(|a| {
                if a.path.is_file() {
                    Some(a.path.clone())
                } else {
                    missing += 1;
                    None
                }
            })
            .collect::<Vec<_>>();
        let folder = paths
            .first()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .or_else(|| self.batch.folder.clone())
            .unwrap_or_else(catalog::root);
        self.load_batch(folder, paths, cx);
        if missing > 0 {
            self.batch.note=Some((format!("{missing} missing file(s) omitted. Relink their entries from Design asset properties.").into(),true));
        }
    }
    fn library_metadata(&mut self, collection: bool, window: &mut Window, cx: &mut Context<Self>) {
        let paths = self.library_paths();
        if paths.is_empty() {
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
        if self.batch.library.search.is_none() {
            self.batch.library.search = Some(
                cx.new(|cx| InputState::new(window, cx).placeholder("Search names / keywords")),
            );
        }
        let owner = cx.weak_entity();
        let collections = self.batch.library.catalog.collections.clone();
        let selected = self.batch.library.collection;
        let title = collections
            .iter()
            .find(|c| Some(c.id) == selected)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "All library photos".into());
        let menu = Button::new("library-collection")
            .label(format!("{title} ▾"))
            .small()
            .outline()
            .dropdown_menu(move |mut menu, _, _| {
                let all = owner.clone();
                menu = menu.item(PopupMenuItem::new("All library photos").on_click(
                    move |_, _, cx| {
                        all.update(cx, |this, cx| {
                            this.batch.library.collection = None;
                            this.library_show(cx);
                        })
                        .ok();
                    },
                ));
                for c in &collections {
                    let owner = owner.clone();
                    let id = c.id;
                    menu = menu.item(PopupMenuItem::new(c.name.clone()).on_click(
                        move |_, _, cx| {
                            owner
                                .update(cx, |this, cx| {
                                    this.batch.library.collection = Some(id);
                                    this.library_show(cx);
                                })
                                .ok();
                        },
                    ));
                }
                menu
            });
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
                                this.batch.library.loaded = false;
                                this.library_load(cx);
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
                                this.library_show(cx);
                            })
                            .ok();
                    }),
                )
            });
        div()
            .id("library-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child(menu)
            .child(
                Button::new("library-import-google-photos")
                    .label("Import from Google Photos…")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.cloud_import_photos(cx))),
            )
            .child(Input::new(self.batch.library.search.as_ref().unwrap()).small())
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        Button::new("library-search")
                            .label("Search")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| this.library_show(cx))),
                    )
                    .child(filters)
                    .child(actions),
            )
            .into_any_element()
    }
}
