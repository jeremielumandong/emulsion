//! Local creative catalog controls shared by Design and Diagram.
use super::*;
use emulsion_core::project::ProjectKind;
use emulsion_io::creative_library::{self as library, AssetKind, Brand, Catalog};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
#[derive(Default)]
pub(super) struct CreativeUi {
    pub(super) catalog: Catalog,
    loaded: bool,
    loading: bool,
}
impl EditorView {
    pub(crate) fn refresh_creative_library(&mut self, cx: &mut Context<Self>) {
        self.creative.loaded = false;
        self.load_creative_library(cx);
    }
    pub(super) fn load_creative_library(&mut self, cx: &mut Context<Self>) {
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
    pub(super) fn install_catalog(&mut self, catalog: Catalog) {
        if catalog.revision >= self.creative.catalog.revision {
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
    fn catalog_edit(
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
    fn use_local_template(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ticket = self.edit_ticket();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { emulsion_io::project::read(&path) })
                .await;
            this.update(cx, |this, cx| {
                if this.edit_ticket() != ticket {
                    this.set_status(
                        "The project changed while the template loaded. Place it again.",
                        false,
                        cx,
                    );
                    return;
                }
                match result
                    .map_err(|e| e.to_string())
                    .and_then(|project| this.editor.import_pages(project))
                {
                    Ok(_) => {
                        this.after_change(cx);
                        this.set_status("Added editable template pages.", false, cx);
                    }
                    Err(e) => this.set_status(e, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }
    fn asset_properties(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn creative_asset_list(
        &self,
        kind: AssetKind,
        query: &str,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let assets = self
            .creative
            .catalog
            .assets
            .iter()
            .filter(|a| {
                a.kind == kind
                    && (a.name.to_lowercase().contains(query)
                        || a.tags.iter().any(|t| t.to_lowercase().contains(query)))
            })
            .cloned()
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(assets.into_iter().map(|asset| {
                let id = asset.id;
                let path = asset.path.clone();
                let owner = cx.weak_entity();
                let title = asset.name.clone();
                let tip = format!(
                    "{}\n{}\n{}",
                    asset.path.display(),
                    asset.attribution,
                    asset.license
                );
                div()
                    .flex()
                    .gap_1()
                    .child(
                        Button::new(("creative-asset", id))
                            .label(title)
                            .tooltip(tip)
                            .small()
                            .ghost()
                            .flex_1()
                            .min_w_0()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if kind == AssetKind::Stencil {
                                    this.use_local_stencil(path.clone(), 0, cx);
                                } else if kind == AssetKind::Template {
                                    this.use_local_template(path.clone(), cx);
                                } else {
                                    this.place_design_assets(vec![path.clone()], cx);
                                }
                            })),
                    )
                    .child(
                        Button::new(("creative-asset-menu", id))
                            .label("···")
                            .small()
                            .ghost()
                            .dropdown_menu(move |menu, _, _| {
                                let props = owner.clone();
                                let remove = owner.clone();
                                menu.item(PopupMenuItem::new("Properties / relink…").on_click(
                                    move |_, window, cx| {
                                        props
                                            .update(cx, |this, cx| {
                                                this.asset_properties(id, window, cx)
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
                    )
            }))
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child("Library removal keeps the source file and placed copies."),
            )
            .into_any_element()
    }
    fn edit_brand(&mut self, id: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        let brand = id
            .and_then(|id| self.creative.catalog.brands.iter().find(|b| b.id == id))
            .cloned()
            .unwrap_or(Brand {
                id: 0,
                name: "My brand".into(),
                font: "Geist".into(),
                colors: vec![[28, 30, 36, 255], [230, 103, 69, 255]],
                logos: Vec::new(),
            });
        let fields = [
            brand.name,
            brand.font,
            brand
                .colors
                .iter()
                .map(|c| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]))
                .collect::<Vec<_>>()
                .join(", "),
        ]
        .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let fields = fields.clone();
            let inputs = fields.clone();
            let owner = owner.clone();
            dialog
                .title("Brand kit")
                .width(px(460.))
                .child(
                    div().flex().flex_col().gap_2().children(
                        ["Name", "Font family", "Colors · #RRGGBB, text color first"]
                            .into_iter()
                            .zip(&fields)
                            .map(|(label, input)| div().child(label).child(Input::new(input))),
                    ),
                )
                .footer(crate::widgets::form_dialog_footer("Save brand"))
                .on_ok(move |_, _, cx| {
                    let values = inputs
                        .each_ref()
                        .map(|i| i.read(cx).value().trim().to_string());
                    let colors = values[2]
                        .split(',')
                        .map(|s| {
                            let s = s.trim().trim_start_matches('#');
                            if s.len() != 6 {
                                return None;
                            }
                            let value = u32::from_str_radix(s, 16).ok()?;
                            Some([(value >> 16) as u8, (value >> 8) as u8, value as u8, 255])
                        })
                        .collect::<Option<Vec<_>>>();
                    let Some(colors) = colors else {
                        owner
                            .update(cx, |this, cx| {
                                this.set_status(
                                    "Use comma-separated six-digit hex colors.",
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
        if !self.prepare_page_action(cx) {
            return;
        }
        let mut ids = self.selected_layer_roots();
        if ids.is_empty() {
            ids = self.editor.doc.children(None);
        }
        match emulsion_core::design::brand::apply(
            &mut self.editor,
            &ids,
            &brand.font,
            &brand.colors,
        ) {
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
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                Button::new("creative-library-reload")
                    .label("Reload local library")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.creative.loaded = false;
                        this.load_creative_library(cx);
                    })),
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
                            .child(div().flex().gap_1().children(brand.colors.iter().map(|c| {
                                div()
                                    .size(px(18.))
                                    .rounded(px(3.))
                                    .bg(rgba(u32::from_be_bytes(*c)))
                            })))
                            .child(
                                Button::new(("brand-apply", id))
                                    .label("Apply to selected objects")
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
                                                this.place_design_assets(vec![path.clone()], cx)
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
