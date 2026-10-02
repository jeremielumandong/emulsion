//! Local creative catalog controls shared by Design and Diagram.
use super::*;
use emulsion_core::project::ProjectKind;
use emulsion_io::creative_library::{self as library, AssetKind, Brand, Catalog};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
#[derive(Default)]
pub(super) struct CreativeUi {
    pub(super) catalog: Catalog,
    pub(super) folder: Option<u64>,
    loaded: bool,
    loading: bool,
    thumbnails: CreativeThumbnails,
    asset_filter: Option<(AssetKind, String, Option<u64>)>,
    asset_matches: Vec<usize>,
    asset_matches_dirty: bool,
    asset_page: usize,
}

impl CreativeUi {
    /// Cache cheap matching indices, and clone/build only the current bounded page.
    fn page_assets(&mut self, kind: AssetKind, query: &str) -> Vec<library::Asset> {
        let filter = (kind, query.to_lowercase(), self.folder);
        if self.asset_filter.as_ref() != Some(&filter) || self.asset_matches_dirty {
            if self.asset_filter.as_ref() != Some(&filter) {
                self.asset_page = 0;
            }
            self.asset_matches = self
                .catalog
                .assets
                .iter()
                .enumerate()
                .filter(|(_, a)| {
                    a.kind == kind
                        && self.folder.is_none_or(|id| a.folder == Some(id))
                        && (a.name.to_lowercase().contains(&filter.1)
                            || a.tags.iter().any(|t| t.to_lowercase().contains(&filter.1)))
                })
                .map(|(index, _)| index)
                .collect();
            self.asset_filter = Some(filter);
            self.asset_matches_dirty = false;
        }
        let pages = self.asset_matches.len().div_ceil(ASSET_PAGE_SIZE).max(1);
        self.asset_page = self.asset_page.min(pages - 1);
        self.asset_matches
            .iter()
            .skip(self.asset_page * ASSET_PAGE_SIZE)
            .take(ASSET_PAGE_SIZE)
            .map(|index| self.catalog.assets[*index].clone())
            .collect()
    }
}

const ASSET_PAGE_SIZE: usize = 24;
const THUMBNAIL_SIZE: u32 = 224;
const THUMBNAIL_WORKERS: usize = 2;
const THUMBNAIL_CACHE_SIZE: usize = 96;

/// An internal library drag retains the reference and its useful display identity.
/// Only image/logo cards expose this payload; templates retain their import action.
#[derive(Clone)]
pub(super) struct DraggedCreativeAsset {
    pub(super) id: u64,
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) kind: AssetKind,
    preview: Option<Arc<RenderImage>>,
}

impl Render for DraggedCreativeAsset {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .id(("creative-asset-drag-preview", self.id))
            .flex()
            .items_center()
            .gap_2()
            .p_2()
            .max_w(px(220.))
            .bg(p.panel)
            .border_1()
            .border_color(p.accent)
            .rounded(px(6.))
            .text_color(p.ink)
            .text_size(px(11.))
            .when_some(self.preview.clone(), |d, preview| {
                d.child(
                    img(ImageSource::Render(preview))
                        .size(px(48.))
                        .object_fit(ObjectFit::Contain),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().truncate().child(self.name.clone()))
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(p.muted)
                            .child(match self.kind {
                                AssetKind::Logo => "Logo",
                                _ => "Image",
                            }),
                    ),
            )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ThumbnailKey {
    id: u64,
    path: PathBuf,
}

impl From<&library::Asset> for ThumbnailKey {
    fn from(asset: &library::Asset) -> Self {
        Self {
            id: asset.id,
            path: asset.path.clone(),
        }
    }
}

enum CreativeThumbnail {
    Ready(Arc<RenderImage>),
    Failed(String),
}

#[derive(Default)]
struct CreativeThumbnails {
    entries: HashMap<ThumbnailKey, CreativeThumbnail>,
    order: std::collections::VecDeque<ThumbnailKey>,
    loading: HashMap<ThumbnailKey, u64>,
    visible: Vec<ThumbnailKey>,
    generation: u64,
    retired: Vec<Arc<RenderImage>>,
}

impl CreativeThumbnails {
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.retired.extend(
            self.entries
                .drain()
                .filter_map(|(_, thumbnail)| match thumbnail {
                    CreativeThumbnail::Ready(image) => Some(image),
                    CreativeThumbnail::Failed(_) => None,
                }),
        );
        self.order.clear();
        // Retain in-flight jobs until they finish so refresh cannot exceed the
        // worker limit. Their generation prevents stale results being installed.
    }

    fn retain_catalog(&mut self, catalog: &Catalog) {
        let paths = catalog
            .assets
            .iter()
            .map(|a| (a.id, &a.path))
            .collect::<HashMap<_, _>>();
        let present =
            |key: &ThumbnailKey| paths.get(&key.id).is_some_and(|path| **path == key.path);
        let retired = &mut self.retired;
        self.entries.retain(|key, thumbnail| {
            if present(key) {
                return true;
            }
            if let CreativeThumbnail::Ready(image) = thumbnail {
                retired.push(image.clone());
            }
            false
        });
        self.order.retain(present);
        self.visible.retain(present);
    }

    fn set_visible(&mut self, visible: Vec<ThumbnailKey>) {
        self.visible = visible;
        for key in &self.visible {
            if self.entries.contains_key(key) {
                self.order.retain(|cached| cached != key);
                self.order.push_back(key.clone());
            }
        }
    }

    fn requests(&mut self) -> Vec<(ThumbnailKey, u64)> {
        let mut requests = Vec::new();
        for key in &self.visible {
            if self.loading.len() >= THUMBNAIL_WORKERS {
                break;
            }
            if self.entries.contains_key(key) || self.loading.contains_key(key) {
                continue;
            }
            self.loading.insert(key.clone(), self.generation);
            requests.push((key.clone(), self.generation));
        }
        requests
    }

    fn finish(&mut self, key: ThumbnailKey, generation: u64, result: CreativeThumbnail) {
        if self.loading.get(&key) != Some(&generation) {
            return;
        }
        self.loading.remove(&key);
        if generation != self.generation || !self.visible.contains(&key) {
            return;
        }
        self.order.retain(|cached| cached != &key);
        self.order.push_back(key.clone());
        if let Some(CreativeThumbnail::Ready(image)) = self.entries.insert(key, result) {
            self.retired.push(image);
        }
        while self.order.len() > THUMBNAIL_CACHE_SIZE {
            if let Some(old) = self.order.pop_front()
                && let Some(CreativeThumbnail::Ready(image)) = self.entries.remove(&old)
            {
                self.retired.push(image);
            }
        }
    }

    fn retry(&mut self, key: &ThumbnailKey) {
        if let Some(CreativeThumbnail::Ready(image)) = self.entries.remove(key) {
            self.retired.push(image);
        }
        self.order.retain(|cached| cached != key);
    }
}
impl EditorView {
    fn drop_retired_creative_thumbnails(&mut self, cx: &mut Context<Self>) {
        let retired = std::mem::take(&mut self.creative.thumbnails.retired);
        if !retired.is_empty() {
            // During rendering/events the current Window is removed from App.
            // Deferring lets App release the atlas entry in every window too.
            cx.defer(move |cx| {
                for image in retired {
                    cx.drop_image(image, None);
                }
            });
        }
    }

    pub(super) fn release_creative_thumbnails(&mut self, window: &mut Window) {
        self.creative.thumbnails.invalidate();
        self.creative.thumbnails.visible.clear();
        for image in self.creative.thumbnails.retired.drain(..) {
            let _ = window.drop_image(image);
        }
    }

    pub(crate) fn refresh_creative_library(&mut self, cx: &mut Context<Self>) {
        self.creative.loaded = false;
        self.creative.thumbnails.invalidate();
        self.load_creative_library(cx);
    }
    pub(super) fn load_creative_library(&mut self, cx: &mut Context<Self>) {
        self.drop_retired_creative_thumbnails(cx);
        if self.creative.loaded || self.creative.loading {
            return;
        }
        self.creative.loading = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async { library::load(&library::root()) })
                .await;
            this.update(cx, |this, cx| {
                this.creative.loading = false;
                this.creative.loaded = true;
                match result {
                    Ok(c) => this.install_catalog(c),
                    Err(e) => this.set_status(e.to_string(), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(crate) fn install_catalog(&mut self, catalog: Catalog) {
        if catalog.revision >= self.creative.catalog.revision {
            self.creative.thumbnails.retain_catalog(&catalog);
            self.creative.asset_matches_dirty = true;
            self.creative.catalog = catalog;
        }
    }
    pub(super) fn note_creative_asset(
        &mut self,
        path: PathBuf,
        kind: AssetKind,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    library::update(&library::root(), |c| c.add_asset(path, kind))
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((c, _)) => this.install_catalog(c),
                    Err(e) => this.set_status(
                        format!(
                            "The asset was placed, but its library entry could not be saved: {e}"
                        ),
                        true,
                        cx,
                    ),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn catalog_edit(
        &mut self,
        edit: impl FnOnce(&mut Catalog) -> emulsion_io::Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { library::update(&library::root(), edit) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((catalog, _)) => this.install_catalog(catalog),
                    Err(e) => this.set_status(e.to_string(), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn save_local_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let name = cx.new(|cx| InputState::new(window, cx).default_value(self.name.clone()));
        let doc = self.editor.doc.clone();
        let kind = self.editor.kind().unwrap_or(ProjectKind::Design);
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let name = name.clone();
            let doc = doc.clone();
            let owner = owner.clone();
            dialog
                .title("Save this page as a local template")
                .width(px(400.))
                .child(Input::new(&name))
                .footer(crate::widgets::form_dialog_footer("Save template"))
                .on_ok(move |_, _, cx| {
                    let name = name.read(cx).value().trim().to_string();
                    if name.is_empty() || name.chars().count() > 200 {
                        return false;
                    }
                    let doc = doc.clone();
                    owner
                        .update(cx, |_this, cx| {
                            cx.spawn(async move |this, cx| {
                                let result = cx
                                    .background_spawn(async move {
                                        let root = library::root();
                                        std::fs::create_dir_all(root.join("templates"))?;
                                        static NEXT: std::sync::atomic::AtomicU64 =
                                            std::sync::atomic::AtomicU64::new(0);
                                        let time = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_nanos();
                                        let seq =
                                            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                        let path = root.join("templates").join(format!(
                                            "{time}-{}-{seq}.emu",
                                            std::process::id()
                                        ));
                                        let session =
                                            emulsion_core::project::ProjectEditor::new_project(
                                                kind, doc,
                                            )
                                            .map_err(emulsion_io::IoError::Manifest)?;
                                        emulsion_io::project::write(
                                            &session.snapshot().unwrap(),
                                            &path,
                                        )?;
                                        match library::update(&root, |c| {
                                            let id =
                                                c.add_asset(path.clone(), AssetKind::Template)?;
                                            c.assets
                                                .iter_mut()
                                                .find(|a| a.id == id)
                                                .unwrap()
                                                .name = name;
                                            Ok(())
                                        }) {
                                            Ok((catalog, _)) => Ok(catalog),
                                            Err(e) => {
                                                let _ = std::fs::remove_file(path);
                                                Err(e)
                                            }
                                        }
                                    })
                                    .await;
                                this.update(cx, |this, cx| match result {
                                    Ok(c) => {
                                        this.install_catalog(c);
                                        this.set_status(
                                            "Saved editable local template.",
                                            false,
                                            cx,
                                        );
                                    }
                                    Err(e) => this.set_status(e.to_string(), true, cx),
                                })
                                .ok();
                            })
                            .detach();
                        })
                        .is_ok()
                })
        });
    }
    pub(super) fn import_local_template(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose an Emulsion project to add to templates".into()),
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
                    emulsion_io::project::read(&path)?;
                    library::update(&library::root(), |c| c.add_asset(path, AssetKind::Template))
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((c, _)) => this.install_catalog(c),
                    Err(e) => this.set_status(e.to_string(), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn asset_properties(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(asset) = self
            .creative
            .catalog
            .assets
            .iter()
            .find(|a| a.id == id)
            .cloned()
        else {
            return;
        };
        let fields = [
            asset.name,
            asset.path.to_string_lossy().into_owned(),
            asset.tags.join(", "),
            asset.attribution,
            asset.license,
        ]
        .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let inputs = fields.clone();
            let owner = owner.clone();
            dialog
                .title("Local asset properties")
                .width(px(460.))
                .child(
                    div().flex().flex_col().gap_2().children(
                        [
                            "Name",
                            "Local path · change to relink a missing file",
                            "Tags · comma separated",
                            "Attribution",
                            "License",
                        ]
                        .into_iter()
                        .zip(&fields)
                        .map(|(label, input)| {
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(label)
                                .child(Input::new(input))
                        }),
                    ),
                )
                .footer(crate::widgets::form_dialog_footer("Save changes"))
                .on_ok(move |_, _, cx| {
                    let values = inputs.each_ref().map(|i| i.read(cx).value().to_string());
                    owner
                        .update(cx, |this, cx| {
                            this.catalog_edit(
                                move |c| {
                                    let asset =
                                        c.assets.iter_mut().find(|a| a.id == id).ok_or_else(
                                            || {
                                                emulsion_io::IoError::Manifest(
                                                    "Asset no longer exists".into(),
                                                )
                                            },
                                        )?;
                                    let path = PathBuf::from(values[1].trim()).canonicalize()?;
                                    if !path.is_file() {
                                        return Err(emulsion_io::IoError::Manifest(
                                            "Choose a local file.".into(),
                                        ));
                                    }
                                    asset.name = values[0].trim().into();
                                    asset.path = path;
                                    asset.tags = values[2]
                                        .split(',')
                                        .map(str::trim)
                                        .filter(|s| !s.is_empty())
                                        .map(String::from)
                                        .collect();
                                    asset.attribution = values[3].clone();
                                    asset.license = values[4].clone();
                                    Ok(())
                                },
                                cx,
                            )
                        })
                        .is_ok()
                })
        });
    }
    fn load_creative_thumbnails(&mut self, cx: &mut Context<Self>) {
        self.drop_retired_creative_thumbnails(cx);
        if !self.visible {
            return;
        }
        for (key, generation) in self.creative.thumbnails.requests() {
            cx.spawn(async move |this, cx| {
                let path = key.path.clone();
                let result = cx
                    .background_spawn(async move {
                        emulsion_io::thumb::thumbnail(&path, THUMBNAIL_SIZE)
                            .map(|(w, h, mut rgba)| {
                                for pixel in rgba.as_chunks_mut::<4>().0 {
                                    pixel.swap(0, 2);
                                }
                                Arc::new(viewport::bgra_image(w, h, rgba))
                            })
                            .map_err(|error| error.to_string())
                    })
                    .await;
                this.update(cx, |this, cx| {
                    let result = match result {
                        Ok(image) => CreativeThumbnail::Ready(image),
                        Err(error) => CreativeThumbnail::Failed(error),
                    };
                    this.creative.thumbnails.finish(key, generation, result);
                    this.load_creative_thumbnails(cx);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    fn creative_asset_card(
        &self,
        asset: library::Asset,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = asset.id;
        let kind = asset.kind;
        let key = ThumbnailKey::from(&asset);
        let path = asset.path.clone();
        let owner = cx.weak_entity();
        let cached = self.creative.thumbnails.entries.get(&key);
        let preview = match cached {
            Some(CreativeThumbnail::Ready(image)) => Some(image.clone()),
            _ => None,
        };
        let error = match cached {
            Some(CreativeThumbnail::Failed(error)) => Some(error.clone()),
            _ => None,
        };
        let tip = format!(
            "{}\n{}\n{}{}",
            asset.path.display(),
            asset.attribution,
            asset.license,
            error
                .as_ref()
                .map(|e| format!(
                    "\nPreview unavailable: {e}\nUse Properties / relink or Retry preview."
                ))
                .unwrap_or_default()
        );
        let payload = DraggedCreativeAsset {
            id,
            path: path.clone(),
            name: asset.name.clone(),
            kind,
            preview: preview.clone(),
        };
        let image = div()
            .id(("creative-asset-thumbnail", id))
            .test_support()
            .w_full()
            .h(px(88.))
            .flex()
            .items_center()
            .justify_center()
            .bg(p.stage)
            .rounded(px(4.))
            .overflow_hidden()
            .when_some(preview, |d, image| {
                d.child(
                    img(ImageSource::Render(image))
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                )
            })
            .when(!matches!(cached, Some(CreativeThumbnail::Ready(_))), |d| {
                d.child(div().px_1().text_size(px(10.)).text_color(p.muted).child(
                    if error.is_some() {
                        "Preview unavailable"
                    } else {
                        "Loading preview…"
                    },
                ))
            });
        let title = asset.name.clone();
        let card = div().id(("creative-asset-drag", id)).child(
            Button::new(("creative-asset", id))
                .when(
                    matches!(kind, AssetKind::Image | AssetKind::Logo),
                    |mut button| {
                        button
                            .interactivity()
                            .on_drag(payload, |drag, _, _, cx| cx.new(|_| drag.clone()));
                        button
                    },
                )
                .accessibility_label(title.clone())
                .tooltip(tip)
                .ghost()
                .cursor_pointer()
                .w_full()
                .h(px(120.))
                .p_1()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .w_full()
                        .min_w_0()
                        .child(image)
                        .child(div().w_full().truncate().text_size(px(11.)).child(title)),
                )
                .on_click(cx.listener(move |this, _, window, cx| match kind {
                    AssetKind::Stencil => this.use_local_stencil(path.clone(), 0, cx),
                    AssetKind::Template => this.preview_local_template(path.clone(), window, cx),
                    AssetKind::Image | AssetKind::Logo => this.place_design_asset(path.clone(), cx),
                })),
        );
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .border_1()
            .border_color(p.line)
            .rounded(px(6.))
            .child(card)
            .child(
                div()
                    .flex()
                    .items_center()
                    .min_w_0()
                    .px_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(9.))
                            .text_color(p.muted)
                            .child(asset.tags.join(", ")),
                    )
                    .child(
                        Button::new(("creative-asset-menu", id))
                            .label("···")
                            .accessibility_label(format!("Options for {}", asset.name))
                            .small()
                            .ghost()
                            .dropdown_menu(move |menu, _, _| {
                                let props = owner.clone();
                                let folders = owner.clone();
                                let remove = owner.clone();
                                let retry = owner.clone();
                                let key = key.clone();
                                menu.item(PopupMenuItem::new("Move to asset folder…").on_click(
                                    move |_, window, cx| {
                                        folders
                                            .update(cx, |this, cx| {
                                                this.move_creative_asset_dialog(id, window, cx)
                                            })
                                            .ok();
                                    },
                                ))
                                .item(PopupMenuItem::new("Properties / relink…").on_click(
                                    move |_, window, cx| {
                                        props
                                            .update(cx, |this, cx| {
                                                this.asset_properties(id, window, cx)
                                            })
                                            .ok();
                                    },
                                ))
                                .item(PopupMenuItem::new("Retry preview").on_click(
                                    move |_, _, cx| {
                                        retry
                                            .update(cx, |this, cx| {
                                                this.creative.thumbnails.retry(&key);
                                                this.load_creative_thumbnails(cx);
                                                cx.notify();
                                            })
                                            .ok();
                                    },
                                ))
                                .item(
                                    PopupMenuItem::new("Remove from library").on_click(
                                        move |_, _, cx| {
                                            remove
                                                .update(cx, |this, cx| {
                                                    this.catalog_edit(
                                                        move |c| {
                                                            c.remove_asset(id);
                                                            Ok(())
                                                        },
                                                        cx,
                                                    )
                                                })
                                                .ok();
                                        },
                                    ),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn creative_asset_list(
        &mut self,
        kind: AssetKind,
        query: &str,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let assets = self.creative.page_assets(kind, query);
        let total = self.creative.asset_matches.len();
        let pages = total.div_ceil(ASSET_PAGE_SIZE).max(1);
        let page = self.creative.asset_page;
        self.creative
            .thumbnails
            .set_visible(assets.iter().map(ThumbnailKey::from).collect());
        self.load_creative_thumbnails(cx);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.creative_folder_controls(cx))
            .child(
                Button::new("creative-assets-reload")
                    .label("Refresh previews and library")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.refresh_creative_library(cx))),
            )
            .when(total == 0, |d| {
                d.child(div().text_size(px(11.)).text_color(p.muted).child(
                    if self.creative.loading {
                        "Loading local library…"
                    } else {
                        "No matching assets. Choose local files or try another search or folder."
                    },
                ))
            })
            .when(pages > 1, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_1()
                        .child(
                            Button::new("creative-assets-previous")
                                .label("Previous")
                                .small()
                                .ghost()
                                .disabled(page == 0)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.creative.asset_page =
                                        this.creative.asset_page.saturating_sub(1);
                                    this.design_ui.scroll.set_offset(point(px(0.), px(0.)));
                                    cx.notify();
                                })),
                        )
                        .child(div().text_size(px(10.)).text_color(p.muted).child(format!(
                            "{} / {}",
                            page + 1,
                            pages
                        )))
                        .child(
                            Button::new("creative-assets-next")
                                .label("Next")
                                .small()
                                .ghost()
                                .disabled(page + 1 >= pages)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.creative.asset_page =
                                        this.creative.asset_page.saturating_add(1);
                                    this.design_ui.scroll.set_offset(point(px(0.), px(0.)));
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(
                div().grid().grid_cols(2).gap(px(6.)).children(
                    assets
                        .into_iter()
                        .map(|asset| self.creative_asset_card(asset, p, cx)),
                ),
            )
            .child(div().text_size(px(10.)).text_color(p.muted).child(format!(
                "{total} asset(s). Library removal keeps the source file and placed copies."
            )))
            .into_any_element()
    }
    fn edit_brand(&mut self, id: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        let brand = id
            .and_then(|id| self.creative.catalog.brands.iter().find(|b| b.id == id))
            .cloned()
            .unwrap_or(Brand {
                typography: Default::default(),
                palettes: Default::default(),
                fonts: Default::default(),
                id: 0,
                name: "My brand".into(),
                font: "Geist".into(),
                colors: vec![[28, 30, 36, 255], [230, 103, 69, 255]],
                logos: Vec::new(),
            });
        let embedded_fonts = brand.fonts.clone();
        let font_label = brand
            .fonts
            .get(&brand.font)
            .map(|f| f.family().to_owned())
            .unwrap_or(brand.font.clone());
        let fields = [
            brand.name,
            font_label,
            brand
                .colors
                .iter()
                .map(|c| format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]))
                .collect::<Vec<_>>()
                .join(", "),
        ]
        .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let fields = fields.clone();
            let inputs = fields.clone();
            let owner = owner.clone();
            let embedded_fonts = embedded_fonts.clone();
            dialog
                .title("Brand kit")
                .width(px(460.))
                .child(
                    div().flex().flex_col().gap_2().children(
                        [
                            "Name",
                            "Font family",
                            "Colors · #RRGGBB or #RRGGBBAA, text first",
                        ]
                        .into_iter()
                        .zip(&fields)
                        .map(|(label, input)| div().child(label).child(Input::new(input))),
                    ),
                )
                .footer(crate::widgets::form_dialog_footer("Save brand"))
                .on_ok(move |_, _, cx| {
                    let mut values = inputs
                        .each_ref()
                        .map(|i| i.read(cx).value().trim().to_string());
                    if let Some(font) = embedded_fonts.values().find(|f| f.family() == values[1]) {
                        values[1] = font.alias().into();
                    }
                    let colors = values[2]
                        .split(',')
                        .map(super::design_brand_ui::parse_rgba)
                        .collect::<Option<Vec<_>>>();
                    let Some(colors) = colors else {
                        owner
                            .update(cx, |this, cx| {
                                this.set_status(
                                    "Use comma-separated six/eight-digit hex colors.",
                                    true,
                                    cx,
                                )
                            })
                            .ok();
                        return false;
                    };
                    if values[0].is_empty() || values[1].is_empty() || colors.len() > 32 {
                        return false;
                    }
                    owner
                        .update(cx, |this, cx| {
                            this.catalog_edit(
                                move |c| {
                                    if let Some(id) = id {
                                        let brand =
                                            c.brands.iter_mut().find(|b| b.id == id).ok_or_else(
                                                || {
                                                    emulsion_io::IoError::Manifest(
                                                        "Brand no longer exists".into(),
                                                    )
                                                },
                                            )?;
                                        brand.name = values[0].clone();
                                        brand.font = values[1].clone();
                                        brand.colors = colors;
                                    } else {
                                        c.add_brand(values[0].clone(), values[1].clone(), colors)?;
                                    }
                                    Ok(())
                                },
                                cx,
                            )
                        })
                        .is_ok()
                })
        });
    }
    fn apply_brand(&mut self, brand: Brand, cx: &mut Context<Self>) {
        let ids = self.selected_layer_roots();
        if ids.is_empty() {
            self.set_status(
                "Select text or shapes before applying a brand kit.",
                false,
                cx,
            );
            return;
        }
        if !self.prepare_page_action(cx) {
            return;
        }
        let result = (|| {
            let mut trial = emulsion_core::Editor::new(self.editor.doc.clone(), None);
            let mut design = trial.doc.design.clone();
            if let Some(font) = brand.fonts.get(&brand.font) {
                design.fonts.insert(brand.font.clone(), font.clone());
            }
            trial
                .execute(Command::SetDesign {
                    design: Box::new(design),
                })
                .map_err(|e| e.to_string())?;
            emulsion_core::design::brand::apply(&mut trial, &ids, &brand.font, &brand.colors)?;
            self.editor
                .commit_design_document(trial.doc, "Apply portable brand kit")
        })();
        match result {
            Ok(()) => {
                self.after_change(cx);
                self.set_status(
                    format!("Applied {} to selected text and shapes.", brand.name),
                    false,
                    cx,
                );
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
    fn import_brand(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import brand fonts and colors".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move { library::import_brand(&library::root(), &path) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(c) => this.install_catalog(c),
                    Err(e) => this.set_status(e.to_string(), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    fn export_brand(&mut self, brand: Brand, cx: &mut Context<Self>) {
        let dir = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| ".".into());
        let rx = cx.prompt_for_new_path(&dir, Some("brand.json"));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else {
                return;
            };
            path.set_extension("json");
            let result = cx
                .background_spawn(async move { library::export_brand(&brand, &path) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => this.set_status("Exported brand fonts and colors.", false, cx),
                Err(e) => this.set_status(e.to_string(), true, cx),
            })
            .ok();
        })
        .detach();
    }
    fn attach_brand_logos(&mut self, brand: u64, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add local brand logos".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.catalog_edit(
                    move |c| {
                        let mut ids = Vec::new();
                        for path in paths {
                            if !emulsion_io::is_openable(&path) {
                                return Err(emulsion_io::IoError::Manifest(
                                    "Choose an image or SVG logo.".into(),
                                ));
                            }
                            ids.push(c.add_asset(path, AssetKind::Logo)?);
                        }
                        let brand =
                            c.brands.iter_mut().find(|b| b.id == brand).ok_or_else(|| {
                                emulsion_io::IoError::Manifest("Brand no longer exists.".into())
                            })?;
                        brand.logos.extend(ids);
                        brand.logos.sort_unstable();
                        brand.logos.dedup();
                        Ok(())
                    },
                    cx,
                )
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn brand_drawer(
        &self,
        query: &str,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let has_selection = !self.selected_layer_roots().is_empty();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when(!has_selection, |d| {
                d.child(
                    div()
                        .id("brand-selection-hint")
                        .test_support()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child("Select text or shapes to apply a brand kit."),
                )
            })
            .child(
                Button::new("creative-library-reload")
                    .label("Reload local library")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.refresh_creative_library(cx);
                    })),
            )
            .child(
                Button::new("brand-embed-selection")
                    .label("Embed font in selected text…")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.import_brand_font(None, cx))),
            )
            .child(
                Button::new("brand-new")
                    .label("New brand kit…")
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| this.edit_brand(None, window, cx))),
            )
            .child(
                Button::new("brand-import")
                    .label("Import fonts and colors…")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.import_brand(cx))),
            )
            .children(
                self.creative
                    .catalog
                    .brands
                    .iter()
                    .filter(|b| b.name.to_lowercase().contains(query))
                    .cloned()
                    .map(|brand| {
                        let id = brand.id;
                        let title = brand.name.clone();
                        let apply = brand.clone();
                        let owner = cx.weak_entity();
                        let logos = brand
                            .logos
                            .iter()
                            .filter_map(|id| {
                                self.creative.catalog.assets.iter().find(|a| a.id == *id)
                            })
                            .cloned()
                            .collect::<Vec<_>>();
                        div()
                            .id(("brand-card", id))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .p_2()
                            .border_1()
                            .border_color(p.line)
                            .rounded(px(6.))
                            .child(title)
                            .child(self.brand_extended_controls(&brand, cx))
                            .child(div().flex().gap_1().children(brand.colors.iter().map(|c| {
                                div()
                                    .size(px(18.))
                                    .rounded(px(3.))
                                    .bg(rgba(u32::from_be_bytes(*c)))
                            })))
                            .child(
                                Button::new(("brand-apply", id))
                                    .label("Apply to selected objects")
                                    .disabled(!has_selection)
                                    .tooltip(if has_selection {
                                        "Apply this kit to the selected objects only"
                                    } else {
                                        "Select text or shapes first"
                                    })
                                    .small()
                                    .outline()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.apply_brand(apply.clone(), cx)
                                    })),
                            )
                            .child(
                                Button::new(("brand-add-logo", id))
                                    .label("Add logos…")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.attach_brand_logos(id, cx)
                                    })),
                            )
                            .children(logos.into_iter().map(|logo| {
                                let path = logo.path.clone();
                                let logo_id = logo.id;
                                div()
                                    .flex()
                                    .gap_1()
                                    .child(
                                        Button::new(("brand-place-logo", logo_id))
                                            .label(logo.name)
                                            .small()
                                            .outline()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.place_design_asset(path.clone(), cx)
                                            })),
                                    )
                                    .child(
                                        Button::new(("brand-logo-properties", logo_id))
                                            .label("…")
                                            .small()
                                            .ghost()
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.asset_properties(logo_id, window, cx)
                                            })),
                                    )
                            }))
                            .child(
                                Button::new(("brand-menu", id))
                                    .label("Edit / export ▾")
                                    .small()
                                    .ghost()
                                    .dropdown_menu(move |menu, _, _| {
                                        let edit = owner.clone();
                                        let export = owner.clone();
                                        let remove = owner.clone();
                                        let brand = brand.clone();
                                        menu.item(PopupMenuItem::new("Edit brand…").on_click(
                                            move |_, window, cx| {
                                                edit.update(cx, |this, cx| {
                                                    this.edit_brand(Some(id), window, cx)
                                                })
                                                .ok();
                                            },
                                        ))
                                        .item(
                                            PopupMenuItem::new("Export fonts and colors…")
                                                .on_click(move |_, _, cx| {
                                                    export
                                                        .update(cx, |this, cx| {
                                                            this.export_brand(brand.clone(), cx)
                                                        })
                                                        .ok();
                                                }),
                                        )
                                        .item(
                                            PopupMenuItem::new("Remove brand kit").on_click(
                                                move |_, _, cx| {
                                                    remove
                                                        .update(cx, |this, cx| {
                                                            this.catalog_edit(
                                                                move |c| {
                                                                    c.brands.retain(|b| b.id != id);
                                                                    Ok(())
                                                                },
                                                                cx,
                                                            )
                                                        })
                                                        .ok();
                                                },
                                            ),
                                        )
                                    }),
                            )
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt;

    fn asset(id: u64) -> library::Asset {
        library::Asset {
            id,
            path: PathBuf::from(format!("/creative-assets/{id}.png")),
            name: format!("Asset {id}"),
            kind: AssetKind::Image,
            tags: Vec::new(),
            attribution: String::new(),
            license: String::new(),
            rating: 0,
            flagged: false,
            rejected: false,
            color_label: 0,
            variants: Vec::new(),
            folder: None,
        }
    }

    fn key(id: u64) -> ThumbnailKey {
        ThumbnailKey::from(&asset(id))
    }

    #[test]
    fn creative_asset_pages_are_bounded_and_search_names_tags_and_folders() {
        let mut ui = CreativeUi::default();
        ui.catalog.assets = (1..=10_000).map(asset).collect();
        ui.catalog.assets[41].tags.push("OCEAN".into());
        ui.catalog.assets[41].folder = Some(7);
        assert_eq!(ui.page_assets(AssetKind::Image, "").len(), ASSET_PAGE_SIZE);
        ui.asset_page = 2;
        assert_eq!(ui.page_assets(AssetKind::Image, "")[0].id, 49);
        let matches = ui.page_assets(AssetKind::Image, "ocean");
        assert_eq!(matches.iter().map(|a| a.id).collect::<Vec<_>>(), vec![42]);
        assert_eq!(ui.asset_page, 0);
        assert_eq!(
            ui.page_assets(AssetKind::Image, "ASSET 10000")[0].id,
            10_000
        );
        ui.folder = Some(7);
        assert_eq!(ui.page_assets(AssetKind::Image, "")[0].id, 42);
        ui.folder = Some(8);
        assert!(ui.page_assets(AssetKind::Image, "").is_empty());
        ui.folder = None;
        assert!(ui.page_assets(AssetKind::Template, "").is_empty());
        ui.page_assets(AssetKind::Image, "");
        ui.asset_page = 400;
        ui.catalog.assets.truncate(1);
        ui.asset_matches_dirty = true;
        assert_eq!(ui.page_assets(AssetKind::Image, "")[0].id, 1);
        assert_eq!(ui.asset_page, 0);
    }

    #[test]
    fn creative_thumbnail_workers_remain_bounded_across_refresh() {
        let mut cache = CreativeThumbnails::default();
        cache.set_visible((1..=ASSET_PAGE_SIZE as u64).map(key).collect());
        let requests = cache.requests();
        assert_eq!(requests.len(), THUMBNAIL_WORKERS);
        assert!(cache.requests().is_empty());
        cache.invalidate();
        assert!(cache.requests().is_empty());
        for (key, generation) in requests {
            cache.finish(key, generation, CreativeThumbnail::Failed("stale".into()));
        }
        assert!(cache.entries.is_empty());
        assert_eq!(cache.requests().len(), THUMBNAIL_WORKERS);
    }

    #[test]
    fn creative_thumbnail_failures_do_not_retry_on_every_render() {
        let mut cache = CreativeThumbnails::default();
        cache.set_visible(vec![key(1)]);
        let (key, generation) = cache.requests().pop().unwrap();
        cache.finish(
            key.clone(),
            generation,
            CreativeThumbnail::Failed("missing".into()),
        );
        assert!(cache.requests().is_empty());
        cache.retry(&key);
        assert_eq!(cache.requests().len(), 1);
    }

    #[test]
    fn creative_thumbnail_cache_is_bounded_and_relink_discards_stale_results() {
        let mut cache = CreativeThumbnails::default();
        for id in 1..=120 {
            cache.set_visible(vec![key(id)]);
            let (key, generation) = cache.requests().pop().unwrap();
            cache.finish(
                key,
                generation,
                CreativeThumbnail::Failed("placeholder".into()),
            );
            assert!(cache.entries.len() <= THUMBNAIL_CACHE_SIZE);
            assert!(cache.order.len() <= THUMBNAIL_CACHE_SIZE);
        }
        assert!(!cache.entries.contains_key(&key(1)));
        assert!(cache.entries.contains_key(&key(120)));
        cache.set_visible(vec![key(121)]);
        let (old_key, generation) = cache.requests().pop().unwrap();
        let mut relinked = asset(121);
        relinked.path = "/creative-assets/relinked.png".into();
        let mut catalog = Catalog::default();
        catalog.assets.push(relinked.clone());
        cache.retain_catalog(&catalog);
        cache.set_visible(vec![ThumbnailKey::from(&relinked)]);
        cache.finish(
            old_key.clone(),
            generation,
            CreativeThumbnail::Failed("old path".into()),
        );
        assert!(!cache.entries.contains_key(&old_key));
        assert_eq!(cache.requests()[0].0.path, relinked.path);
    }

    #[test]
    fn creative_thumbnail_removal_retires_every_render_image() {
        let mut cache = CreativeThumbnails::default();
        for id in 1..=120 {
            cache.set_visible(vec![key(id)]);
            let (key, generation) = cache.requests().pop().unwrap();
            cache.finish(
                key,
                generation,
                CreativeThumbnail::Ready(Arc::new(viewport::bgra_image(1, 1, vec![0, 0, 0, 255]))),
            );
        }
        assert_eq!(cache.entries.len(), THUMBNAIL_CACHE_SIZE);
        assert_eq!(cache.retired.len(), 120 - THUMBNAIL_CACHE_SIZE);
        cache.retry(&key(120));
        assert_eq!(cache.retired.len(), 121 - THUMBNAIL_CACHE_SIZE);
        cache.retain_catalog(&Catalog::default());
        assert!(cache.entries.is_empty());
        assert_eq!(cache.retired.len(), 120);
        cache.set_visible(vec![key(121)]);
        let (key, generation) = cache.requests().pop().unwrap();
        cache.finish(
            key,
            generation,
            CreativeThumbnail::Ready(Arc::new(viewport::bgra_image(1, 1, vec![0, 0, 0, 255]))),
        );
        cache.invalidate();
        assert!(cache.entries.is_empty());
        assert_eq!(cache.retired.len(), 121);
    }

    #[gpui_kit::test]
    fn applying_brand_without_selection_is_disabled_and_never_edits_page(cx: &mut TestAppContext) {
        let doc = emulsion_core::design::Template::Announcement
            .create(600, 400)
            .unwrap();
        let (ws, cx) = crate::tests::open(cx, doc.clone());
        cx.simulate_resize(size(px(1440.), px(1000.)));
        let view = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(
                    emulsion_core::project::ProjectEditor::new_project(ProjectKind::Design, doc)
                        .unwrap(),
                    "Brand safety".into(),
                    window,
                    cx,
                );
            });
            let view = ws.read(cx).editor.clone().unwrap();
            view.update(cx, |e, cx| {
                e.creative.loaded = true;
                e.creative
                    .catalog
                    .add_brand(
                        "Test brand".into(),
                        "Geist Mono".into(),
                        vec![[200, 30, 60, 255]],
                    )
                    .unwrap();
                e.set_layer_selection(Vec::new(), None);
                e.show_design_section(super::super::design_ui::Section::Brand, cx);
                let revision = e.editor.revision;
                let history = e.editor.history.len();
                e.apply_brand(e.creative.catalog.brands[0].clone(), cx);
                assert_eq!(e.editor.revision, revision);
                assert_eq!(e.editor.history.len(), history);
                e.set_status("Brand click has not run.", false, cx);
            });
            view
        });
        cx.run_until_parked();
        let (selected, before, history) = cx.update(|window, cx| {
            let id = view.read(cx).creative.catalog.brands[0].id;
            assert!(window.find("brand-selection-hint").visible());
            assert!(window.find(("brand-apply", id)).visible());
            // GPUI does not expose aria-disabled in its native snapshots. A real
            // pointer click must not even enter the handler: its defensive
            // no-selection status would overwrite this sentinel if invoked.
            window.click(("brand-apply", id), cx);
            view.update(cx, |e, cx| {
                assert_eq!(
                    e.status.as_ref().map(|(text, _)| text.as_ref()),
                    Some("Brand click has not run.")
                );
                let before = e
                    .editor
                    .doc
                    .nodes
                    .iter()
                    .filter_map(|node| match &node.kind {
                        NodeKind::Text { spec, .. } => {
                            Some((node.id, spec.font.clone(), spec.color))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert!(before.len() > 1, "fixture must include unselected text");
                let selected = before[0].0;
                let history = e.editor.history.len();
                e.set_layer_selection(vec![selected], Some(selected));
                cx.notify();
                (selected, before, history)
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let id = view.read(cx).creative.catalog.brands[0].id;
            assert!(window.try_find("brand-selection-hint").is_none());
            window.click(("brand-apply", id), cx);
            let e = view.read(cx);
            assert_eq!(e.editor.history.len(), history + 1);
            for (id, font, color) in &before {
                let NodeKind::Text { spec, .. } = &e.editor.doc.node(*id).unwrap().kind else {
                    panic!("brand application must retain editable text")
                };
                if *id == selected {
                    assert_eq!(spec.font, "Geist Mono");
                    assert_eq!(spec.color, [200, 30, 60, 255]);
                } else {
                    assert_eq!(&spec.font, font);
                    assert_eq!(&spec.color, color);
                }
            }
        });
    }

    #[gpui_kit::test]
    fn creative_library_loads_real_previews_and_keeps_card_actions(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("photo.png");
        image::RgbaImage::from_pixel(24, 16, image::Rgba([40, 80, 120, 255]))
            .save(&path)
            .unwrap();
        let doc = emulsion_core::design::Template::Announcement
            .create(600, 400)
            .unwrap();
        let (ws, cx) = crate::tests::open(cx, doc.clone());
        cx.simulate_resize(size(px(1440.), px(1000.)));
        let view = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(
                    emulsion_core::project::ProjectEditor::new_project(ProjectKind::Design, doc)
                        .unwrap(),
                    "Asset previews".into(),
                    window,
                    cx,
                );
            });
            let view = ws.read(cx).editor.clone().unwrap();
            view.update(cx, |e, cx| {
                e.creative.loaded = true;
                let mut catalog = Catalog::default();
                catalog.add_asset(path.clone(), AssetKind::Image).unwrap();
                e.install_catalog(catalog);
                e.show_design_section(super::super::design_ui::Section::Uploads, cx);
            });
            view
        });
        cx.run_until_parked();
        let (key, old_image) = cx.update(|window, cx| {
            let e = view.read(cx);
            let asset = &e.creative.catalog.assets[0];
            assert!(matches!(
                e.creative
                    .thumbnails
                    .entries
                    .get(&ThumbnailKey::from(asset)),
                Some(CreativeThumbnail::Ready(_))
            ));
            assert!(window.find(("creative-asset", asset.id)).visible());
            assert!(window.find(("creative-asset-menu", asset.id)).visible());
            let bounds = window.find(("creative-asset-thumbnail", asset.id)).bounds();
            assert!(bounds.size.width > px(0.) && bounds.size.height >= px(80.));
            let key = ThumbnailKey::from(asset);
            let Some(CreativeThumbnail::Ready(image)) = e.creative.thumbnails.entries.get(&key)
            else {
                panic!("preview should be ready")
            };
            assert!(window.has_image_atlas_entry(image));
            (key, image.clone())
        });
        cx.update(|_, cx| {
            view.update(cx, |e, cx| {
                e.creative.thumbnails.retry(&key);
                e.load_creative_thumbnails(cx);
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(!window.has_image_atlas_entry(&old_image));
            view.update(cx, |e, _| {
                let Some(CreativeThumbnail::Ready(image)) = e.creative.thumbnails.entries.get(&key)
                else {
                    panic!("retry should replace the preview")
                };
                let image = image.clone();
                assert!(window.has_image_atlas_entry(&image));
                e.release_creative_thumbnails(window);
                assert!(!window.has_image_atlas_entry(&image));
                assert!(e.creative.thumbnails.entries.is_empty());
                assert!(e.creative.thumbnails.visible.is_empty());
            });
        });
    }
}
