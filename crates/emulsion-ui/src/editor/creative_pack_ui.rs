//! Author and install portable Design templates and Diagram stencil packs.
use super::*;
use emulsion_io::{
    creative_library::{self as library, AssetKind},
    template_pack::{self, Kind, Manifest},
};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
#[derive(Clone)]
pub(super) struct DraggedPackStencil {
    pub path: PathBuf,
    pub index: usize,
    pub name: String,
}
impl Render for DraggedPackStencil {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .px_3()
            .py_2()
            .rounded(px(6.))
            .bg(p.panel)
            .border_1()
            .border_color(p.accent)
            .text_color(p.ink)
            .child(self.name.clone())
    }
}
impl EditorView {
    pub(super) fn creative_pack_controls(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                Button::new("creative-export-pack")
                    .label(if self.is_diagram() {
                        t!("editor.creative_pack_ui.export_stencil_pack")
                    } else {
                        t!("editor.creative_pack_ui.export_design_template")
                    })
                    .small()
                    .outline()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.export_creative_pack(window, cx)),
                    ),
            )
            .child(
                Button::new("creative-install-pack")
                    .label(t!("editor.creative_pack_ui.install_file"))
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.install_creative_pack_file(cx))),
            )
            .child(
                Button::new("creative-install-github")
                    .label(t!("editor.creative_pack_ui.install_github_url"))
                    .small()
                    .ghost()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.install_creative_github(window, cx)),
                    ),
            )
            .into_any_element()
    }
    pub(super) fn install_diagram_stencils(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some(t!("editor.creative_pack_ui.choose_stencil_source").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.set_status(t!("editor.creative_pack_ui.installing_stencils"), false, cx)
            })
            .ok();
            let result = cx
                .background_spawn(async move {
                    let (pack, warnings) = template_pack::read_stencil_source(&path)?;
                    let count = pack.project.pages.len();
                    template_pack::install(&library::root(), pack)
                        .map(|(catalog, id)| (catalog, id, count, warnings))
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((catalog, id, count, warnings)) => {
                    crate::app_state::update_settings(cx, |s| {
                        if !s.diagram_stencil_packs.contains(&id) {
                            s.diagram_stencil_packs.push(id);
                        }
                    });
                    this.diagram_ui.expanded_stencil_packs.insert(id);
                    this.install_catalog(catalog);
                    this.diagram_import_notes(warnings.clone());
                    this.set_status(
                        t!(
                            "editor.creative_pack_ui.installed_stencils",
                            count = count,
                            notes = warnings.len()
                        ),
                        false,
                        cx,
                    );
                }
                Err(error) => this.set_status(error.to_string(), true, cx),
            })
            .ok();
        })
        .detach();
    }

    fn install_creative_pack_file(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("editor.creative_pack_ui.choose_package").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.set_status(t!("editor.creative_pack_ui.installing_package"), false, cx)
            })
            .ok();
            let result = cx
                .background_spawn(async move {
                    template_pack::install(&library::root(), template_pack::read(&path)?)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((catalog, _)) => {
                    this.install_catalog(catalog);
                    this.set_status(t!("editor.creative_pack_ui.installed_package"), false, cx);
                }
                Err(e) => this.set_status(e.to_string(), true, cx),
            })
            .ok();
        })
        .detach();
    }
    fn install_creative_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("https://github.com/owner/template-pack")
        });
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let value = input.clone();
            let owner = owner.clone();
            dialog
                .title(t!("editor.creative_pack_ui.github_title").to_string())
                .width(px(520.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(t!("editor.creative_pack_ui.github_url_hint").to_string())
                        .child(Input::new(&input))
                        .child(t!("editor.creative_pack_ui.github_offline_hint").to_string()),
                )
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.creative_pack_ui.install"
                )))
                .on_ok(move |_, _, cx| {
                    let url = value.read(cx).value().trim().to_string();
                    if let Err(e) = template_pack::GithubSource::parse(&url) {
                        owner
                            .update(cx, |this, cx| this.set_status(e.to_string(), true, cx))
                            .ok();
                        return false;
                    }
                    owner
                        .update(cx, |this, cx| {
                            this.set_status(
                                t!("editor.creative_pack_ui.downloading_github"),
                                false,
                                cx,
                            );
                            cx.spawn(async move |this, cx| {
                                let result = cx
                                    .background_spawn(async move {
                                        template_pack::install(
                                            &library::root(),
                                            template_pack::download_github(&url)?,
                                        )
                                    })
                                    .await;
                                this.update(cx, |this, cx| match result {
                                    Ok((catalog, _)) => {
                                        this.install_catalog(catalog);
                                        this.set_status(
                                            t!("editor.creative_pack_ui.installed_github"),
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
    pub(super) fn export_creative_pack(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(mut project) = self.editor.snapshot() else {
            return;
        };
        let kind = Kind::of(project.kind);
        if kind == Kind::Stencil {
            match template_pack::stencil_project(&project) {
                Ok(stencils) => project = stencils,
                Err(error) => {
                    self.set_status(error.to_string(), true, cx);
                    return;
                }
            }
        }
        let fields = [
            self.name.clone(),
            String::new(),
            String::new(),
            String::new(),
        ]
        .map(|s| cx.new(|cx| InputState::new(window, cx).default_value(s)));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let values = fields.clone();
            let owner = owner.clone();
            let project = project.clone();
            dialog
                .title(
                    match kind {
                        Kind::Stencil => t!("editor.creative_pack_ui.export_stencil_title"),
                        Kind::Storyboard => "Export storyboard template".into(),
                        Kind::Design => t!("editor.creative_pack_ui.export_design_title"),
                    }
                    .to_string(),
                )
                .width(px(480.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            if kind == Kind::Stencil {
                                t!("editor.creative_pack_ui.export_stencil_hint")
                            } else {
                                t!("editor.creative_pack_ui.export_design_hint")
                            }
                            .to_string(),
                        )
                        .children(
                            [
                                t!("editor.creative_pack_ui.field_name"),
                                t!("editor.creative_pack_ui.field_author"),
                                t!("editor.creative_pack_ui.field_license"),
                                t!("editor.creative_pack_ui.field_tags"),
                            ]
                            .into_iter()
                            .zip(&fields)
                            .map(|(label, input)| {
                                div().child(label.to_string()).child(Input::new(input))
                            }),
                        ),
                )
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.creative_pack_ui.export_file"
                )))
                .on_ok(move |_, _, cx| {
                    let texts = values
                        .each_ref()
                        .map(|i| i.read(cx).value().trim().to_string());
                    if texts[0].is_empty() || texts[0].chars().count() > 200 {
                        return false;
                    }
                    let mut manifest = Manifest::new(kind, texts[0].clone());
                    manifest.author = texts[1].clone();
                    manifest.license = texts[2].clone();
                    manifest.tags = texts[3]
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect();
                    let project = project.clone();
                    owner
                        .update(cx, |this, cx| {
                            let dir = this
                                .editor
                                .path
                                .as_ref()
                                .and_then(|p| p.parent())
                                .map(PathBuf::from)
                                .unwrap_or_else(|| {
                                    std::env::home_dir().unwrap_or_else(|| ".".into())
                                });
                            let file = format!(
                                "{}.{}",
                                manifest
                                    .name
                                    .chars()
                                    .map(|c| {
                                        if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                                            c
                                        } else {
                                            '_'
                                        }
                                    })
                                    .collect::<String>(),
                                kind.extension()
                            );
                            let rx = cx.prompt_for_new_path(&dir, Some(&file));
                            cx.spawn(async move |this, cx| {
                                let path = match rx.await {
                                    Ok(Ok(Some(path))) => path,
                                    Ok(Ok(None)) => return,
                                    _ => {
                                        this.update(cx, |this, cx| {
                                            this.set_status(
                                                t!("editor.creative_pack_ui.picker_failed"),
                                                true,
                                                cx,
                                            )
                                        })
                                        .ok();
                                        return;
                                    }
                                };
                                let mut path = path;
                                path.set_extension(kind.extension());
                                let output = path.clone();
                                this.update(cx, |this, cx| {
                                    this.set_status(
                                        t!("shell.exporting", path = path.display()),
                                        false,
                                        cx,
                                    )
                                })
                                .ok();
                                let result = cx
                                    .background_spawn(async move {
                                        template_pack::write(&project, &manifest, &output)
                                    })
                                    .await;
                                this.update(cx, |this, cx| match result {
                                    Ok(()) => this.set_status(
                                        t!(
                                            "editor.creative_pack_ui.exported",
                                            path = path.display()
                                        ),
                                        false,
                                        cx,
                                    ),
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
    pub(super) fn use_local_stencil(
        &mut self,
        path: PathBuf,
        page_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.use_local_stencil_at(path, page_index, None, cx);
    }
    pub(super) fn use_local_stencil_at(
        &mut self,
        path: PathBuf,
        page_index: usize,
        center: Option<(f64, f64)>,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ticket = self.edit_ticket();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let project = emulsion_io::project::read(&path)?;
                    let page = project.pages.get(page_index).ok_or_else(|| {
                        emulsion_io::IoError::Manifest(
                            t!("editor.creative_pack_ui.stencil_page_gone").into_owned(),
                        )
                    })?;
                    let mut doc = page.doc.clone();
                    emulsion_core::diagram::caption_icon_labels(&mut doc, &page.meta.name);
                    let roots = doc
                        .nodes
                        .iter()
                        .filter(|n| n.parent.is_none() && !matches!(n.kind, NodeKind::Fill { .. }))
                        .map(|n| n.id)
                        .collect::<Vec<_>>();
                    let fragment = emulsion_core::fragment::Fragment::capture(&doc, &roots)
                        .map_err(emulsion_io::IoError::Manifest)?;
                    let bounds = roots
                        .iter()
                        .filter_map(|id| emulsion_core::geometry::node_bounds(&doc, *id))
                        .fold(emulsion_raster::IRect::default(), |a, b| a.union(&b));
                    Ok::<_, emulsion_io::IoError>((fragment, bounds))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.edit_ticket() != ticket {
                    this.set_status(t!("editor.creative_pack_ui.page_changed"), false, cx);
                    return;
                }
                match result
                    .map_err(|e| e.to_string())
                    .and_then(|(fragment, bounds)| {
                        let center = center.unwrap_or((
                            this.editor.doc.width as f64 / 2.,
                            this.editor.doc.height as f64 / 2.,
                        ));
                        let offset = (
                            center.0 - bounds.w as f64 / 2. - bounds.x as f64,
                            center.1 - bounds.h as f64 / 2. - bounds.y as f64,
                        );
                        fragment.paste_into_project(&mut this.editor, Slot::TOP, offset)
                    }) {
                    Ok(ids) => {
                        this.after_change(cx);
                        this.set_layer_selection(ids.clone(), ids.first().copied());
                        this.set_tool(Tool::Move, cx);
                        this.set_status(t!("editor.creative_pack_ui.placed_stencil"), false, cx);
                    }
                    Err(e) => this.set_status(e, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn stencil_pack_list(
        &self,
        query: &str,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use gpui_kit::assets::IconName;
        let enabled = crate::app_state::settings(cx).diagram_stencil_packs.clone();
        let mut list = div().flex().flex_col().gap(px(12.));
        let collected = self
            .creative
            .catalog
            .assets
            .iter()
            .filter(|a| {
                a.kind == AssetKind::Stencil
                    && enabled.contains(&a.id)
                    && a.tags.iter().any(|t| t == "Imported shapes")
            })
            .map(|a| a.id)
            .collect::<Vec<_>>();
        if !collected.is_empty() {
            list = list
                .child(
                    div().text_size(px(11.)).child(
                        t!(
                            "editor.creative_pack_ui.collected_imports",
                            count = collected.len()
                        )
                        .to_string(),
                    ),
                )
                .child(
                    Button::new("stencil-clear-collected")
                        .label(t!("editor.creative_pack_ui.clear_collected"))
                        .small()
                        .ghost()
                        .tooltip(t!("editor.creative_pack_ui.clear_collected_tip"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let ids = collected.clone();
                            this.catalog_edit(
                                move |catalog| {
                                    for id in ids {
                                        catalog.remove_asset(id);
                                    }
                                    Ok(())
                                },
                                cx,
                            );
                        })),
                );
        }
        const PAGE: usize = 96;
        let total = self
            .creative
            .catalog
            .assets
            .iter()
            .filter(|a| {
                a.kind == AssetKind::Stencil
                    && enabled.contains(&a.id)
                    && (self.diagram_ui.expanded_stencil_packs.contains(&a.id) || !query.is_empty())
            })
            .map(|a| {
                a.variants
                    .iter()
                    .filter(|name| {
                        format!("{} {name} {}", a.name, a.tags.join(" "))
                            .to_lowercase()
                            .contains(query)
                    })
                    .count()
            })
            .sum::<usize>();
        let page = self
            .diagram_ui
            .stencil_page
            .min(total.saturating_sub(1) / PAGE);
        let mut matched = 0;

        for asset in self
            .creative
            .catalog
            .assets
            .iter()
            .filter(|a| a.kind == AssetKind::Stencil && enabled.contains(&a.id))
        {
            if !format!(
                "{} {} {}",
                asset.name,
                asset.tags.join(" "),
                asset.variants.join(" ")
            )
            .to_lowercase()
            .contains(query)
            {
                continue;
            }
            let id = asset.id;
            let owner = cx.weak_entity();
            let expanded =
                self.diagram_ui.expanded_stencil_packs.contains(&id) || !query.is_empty();
            let header = div()
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(24.))
                .child(
                    super::diagram_ui::drawer::section_header(
                        ("stencil-pack-toggle", id),
                        &asset.name,
                        Some(asset.variants.len()),
                        expanded,
                        p,
                    )
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.diagram_ui.expanded_stencil_packs.remove(&id) {
                            this.diagram_ui.expanded_stencil_packs.insert(id);
                        }
                        this.diagram_ui.stencil_page = 0;
                        cx.notify();
                    })),
                )
                .child(
                    super::diagram_ui::drawer::icon_button(
                        ("stencil-pack-actions", id),
                        IconName::Ellipsis,
                        t!("editor.creative_pack_ui.pack_actions"),
                    )
                    .with_size(px(22.))
                    .dropdown_menu(move |menu, _, _| {
                        let props = owner.clone();
                        let folders = owner.clone();
                        menu.item(
                            PopupMenuItem::new(t!("editor.creative_pack_ui.properties_relink"))
                                .on_click(move |_, window, cx| {
                                    props
                                        .update(cx, |v, cx| v.asset_properties(id, window, cx))
                                        .ok();
                                }),
                        )
                        .item(
                            PopupMenuItem::new(t!("editor.creative_pack_ui.move_to_folder"))
                                .on_click(move |_, window, cx| {
                                    folders
                                        .update(cx, |v, cx| {
                                            v.move_creative_asset_dialog(id, window, cx)
                                        })
                                        .ok();
                                }),
                        )
                    }),
                )
                .child(
                    super::diagram_ui::drawer::icon_button(
                        ("stencil-pack-remove", id),
                        IconName::Trash,
                        t!("editor.creative_pack_ui.remove_pack"),
                    )
                    .with_size(px(22.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.catalog_edit(
                            move |catalog| {
                                catalog.remove_asset(id);
                                Ok(())
                            },
                            cx,
                        )
                    })),
                );
            let section = div().flex().flex_col().gap(px(8.)).child(header);
            if !expanded {
                list = list.child(section);
                continue;
            }
            let mut grid = super::diagram_ui::drawer::tile_grid(("stencil-pack-grid", id), 5);
            for (index, name) in asset.variants.iter().enumerate() {
                if !format!("{} {name} {}", asset.name, asset.tags.join(" "))
                    .to_lowercase()
                    .contains(query)
                {
                    continue;
                }
                matched += 1;
                if matched <= page * PAGE || matched > (page + 1) * PAGE {
                    continue;
                }
                let path = asset.path.clone();
                let preview = path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(format!("entry-{index}.png"));
                let drag = DraggedPackStencil {
                    path: path.clone(),
                    index,
                    name: name.clone(),
                };
                let tip: SharedString =
                    t!("editor.creative_pack_ui.drag_to_canvas", name = name).into();
                grid = grid.child(
                    super::diagram_ui::drawer::tile(
                        (
                            ElementId::from("stencil-pack-item"),
                            format!("{}-{index}", asset.id),
                        ),
                        p.soft_bg,
                        p,
                    )
                    .test_support()
                    .p(px(4.))
                    .cursor_grab()
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                    })
                    .child(img(preview).size_full().object_fit(ObjectFit::Contain))
                    .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.use_local_stencil(path.clone(), index, cx)
                    })),
                );
            }
            list = list.child(section.child(grid));
        }
        if total > PAGE {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("stencil-previous")
                            .label(t!("home.previous"))
                            .small()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.diagram_ui.stencil_page = page.saturating_sub(1);
                                cx.notify();
                            })),
                    )
                    .child(format!("{} / {}", page + 1, total.div_ceil(PAGE)))
                    .child(
                        Button::new("stencil-next")
                            .label(t!("home.next"))
                            .small()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.diagram_ui.stencil_page =
                                    (page + 1).min(total.saturating_sub(1) / PAGE);
                                cx.notify();
                            })),
                    ),
            );
        }
        list.into_any_element()
    }
}
