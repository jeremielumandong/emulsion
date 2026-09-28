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
                        "Export stencil pack…"
                    } else {
                        "Export Design template…"
                    })
                    .small()
                    .outline()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.export_creative_pack(window, cx)),
                    ),
            )
            .child(
                Button::new("creative-install-pack")
                    .label("Install template / stencil file…")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.install_creative_pack_file(cx))),
            )
            .child(
                Button::new("creative-install-github")
                    .label("Install from GitHub URL…")
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
            prompt: Some(
                "Choose a stencil pack, draw.io XML, Visio stencil, SVG file or SVG folder".into(),
            ),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.set_status("Installing stencil library…", false, cx)
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
                        format!(
                            "Installed {count} reusable stencil entries. {} import notes.",
                            warnings.len()
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
            prompt: Some("Choose an .emutemplate or .emustencil package".into()),
        });
        cx.spawn(async move|this,cx|{
            let Ok(Ok(Some(paths)))=rx.await else{return;};let Some(path)=paths.into_iter().next()else{return;};
            this.update(cx,|this,cx|this.set_status("Installing creative package…",false,cx)).ok();
            let result=cx.background_spawn(async move{template_pack::install(&library::root(),template_pack::read(&path)?)}).await;
            this.update(cx,|this,cx|match result{Ok((catalog,_))=>{this.install_catalog(catalog);this.set_status("Installed. Find Design templates in Templates and stencil packs in Diagram.",false,cx);},Err(e)=>this.set_status(e.to_string(),true,cx)}).ok();
        }).detach();
    }
    fn install_creative_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("https://github.com/owner/template-pack")
        });
        let owner = cx.weak_entity();
        window.open_dialog(cx,move|dialog,_,_|{
            let value=input.clone();let owner=owner.clone();
            dialog.title("Install from GitHub").width(px(520.)).child(div().flex().flex_col().gap_2().child("Public repository URL or /tree/<ref>/<directory>. The directory must contain emulsion-template.json and its project file.").child(Input::new(&input)).child("The pack is copied into your local library. Installed artwork stays available offline."))
            .footer(crate::widgets::form_dialog_footer("Install"))
            .on_ok(move|_,_,cx|{
                let url=value.read(cx).value().trim().to_string();
                if let Err(e)=template_pack::GithubSource::parse(&url){owner.update(cx,|this,cx|this.set_status(e.to_string(),true,cx)).ok();return false;}
                owner.update(cx,|this,cx|{
                    this.set_status("Downloading template package from GitHub…",false,cx);
                    cx.spawn(async move|this,cx|{
                        let result=cx.background_spawn(async move{template_pack::install(&library::root(),template_pack::download_github(&url)?)}).await;
                        this.update(cx,|this,cx|match result{Ok((catalog,_))=>{this.install_catalog(catalog);this.set_status("Installed GitHub pack in the local library.",false,cx);},Err(e)=>this.set_status(e.to_string(),true,cx)}).ok();
                    }).detach();
                }).is_ok()
            })
        });
    }
    fn export_creative_pack(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(mut project) = self.editor.snapshot() else {
            return;
        };
        let kind = if self.is_diagram() {
            Kind::Stencil
        } else {
            Kind::Design
        };
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
        window.open_dialog(cx,move|dialog,_,_|{
            let values=fields.clone();let owner=owner.clone();let project=project.clone();
            dialog.title(if kind==Kind::Stencil{"Export editable stencil pack"}else{"Export editable Design template"}).width(px(480.))
                .child(div().flex().flex_col().gap_2().child(if kind==Kind::Stencil{"Each page becomes a stencil users can place on their canvas. Page backgrounds are excluded."}else{"All pages are included with editable artwork. Exported templates omit local version history."})
                    .children(["Pack name","Author / attribution","License","Tags · comma separated"].into_iter().zip(&fields).map(|(label,input)|div().child(label).child(Input::new(input)))))
                .footer(crate::widgets::form_dialog_footer("Export file…"))
                .on_ok(move|_,_,cx|{
                    let texts=values.each_ref().map(|i|i.read(cx).value().trim().to_string());
                    if texts[0].is_empty()||texts[0].chars().count()>200{return false;}
                    let mut manifest=Manifest::new(kind,texts[0].clone());manifest.author=texts[1].clone();manifest.license=texts[2].clone();manifest.tags=texts[3].split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_string).collect();
                    let project=project.clone();
                    owner.update(cx,|this,cx|{
                        let dir=this.editor.path.as_ref().and_then(|p|p.parent()).map(PathBuf::from).unwrap_or_else(||std::env::home_dir().unwrap_or_else(||".".into()));
                        let file=format!("{}.{}", manifest.name.chars().map(|c| if c.is_alphanumeric() || c==' ' || c=='-' || c=='_' {c} else {'_'}).collect::<String>(),kind.extension());let rx=cx.prompt_for_new_path(&dir,Some(&file));
                        cx.spawn(async move|this,cx|{
                            let path = match rx.await {
                                Ok(Ok(Some(path))) => path,
                                Ok(Ok(None)) => return,
                                _ => { this.update(cx, |this,cx|this.set_status("Could not open the export file picker.",true,cx)).ok(); return; }
                            };
                            let mut path=path;path.set_extension(kind.extension());let output=path.clone();
                            this.update(cx, |this,cx|this.set_status(format!("Exporting {}…",path.display()),false,cx)).ok();
                            let result=cx.background_spawn(async move{template_pack::write(&project,&manifest,&output)}).await;
                            this.update(cx,|this,cx|match result{Ok(())=>this.set_status(format!("Exported {}. Share this file, or unzip its contents into a GitHub repository.",path.display()),false,cx),Err(e)=>this.set_status(e.to_string(),true,cx)}).ok();
                        }).detach();
                    }).is_ok()
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
                        emulsion_io::IoError::Manifest("Stencil page no longer exists.".into())
                    })?;
                    let roots = page
                        .doc
                        .nodes
                        .iter()
                        .filter(|n| n.parent.is_none() && !matches!(n.kind, NodeKind::Fill { .. }))
                        .map(|n| n.id)
                        .collect::<Vec<_>>();
                    let fragment = emulsion_core::fragment::Fragment::capture(&page.doc, &roots)
                        .map_err(emulsion_io::IoError::Manifest)?;
                    let bounds = roots
                        .iter()
                        .filter_map(|id| emulsion_core::geometry::node_bounds(&page.doc, *id))
                        .fold(emulsion_raster::IRect::default(), |a, b| a.union(&b));
                    Ok::<_, emulsion_io::IoError>((fragment, bounds))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.edit_ticket() != ticket {
                    this.set_status(
                        "The page changed while the stencil loaded. Place it again.",
                        false,
                        cx,
                    );
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
                        fragment.paste(&mut this.editor, Slot::TOP, offset)
                    }) {
                    Ok(ids) => {
                        this.after_change(cx);
                        this.set_layer_selection(ids.clone(), ids.first().copied());
                        this.set_tool(Tool::Move, cx);
                        this.set_status(
                            "Placed editable stencil. One undo removes the whole insertion.",
                            false,
                            cx,
                        );
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
        let enabled = crate::app_state::settings(cx).diagram_stencil_packs.clone();
        let mut list = div().flex().flex_col().gap_1();
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
            list=list.child(div().text_size(px(11.)).child(format!("Previously collected imports · {} packs",collected.len())))
                .child(Button::new("stencil-clear-collected").label("Clear collected imports").small().ghost()
                    .tooltip("Remove previously collected packs from the library. Source files and canvas objects are kept.")
                    .on_click(cx.listener(move |this,_,_,cx| {
                        let ids=collected.clone();
                        this.catalog_edit(move |catalog| {for id in ids {catalog.remove_asset(id);} Ok(())},cx);
                    })));
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
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new(("stencil-pack-toggle", id))
                            .label(format!(
                                "{} {} ({})",
                                if expanded { "▾" } else { "▸" },
                                asset.name,
                                asset.variants.len()
                            ))
                            .small()
                            .ghost()
                            .flex_1()
                            .min_w_0()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.diagram_ui.expanded_stencil_packs.remove(&id) {
                                    this.diagram_ui.expanded_stencil_packs.insert(id);
                                }
                                this.diagram_ui.stencil_page = 0;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("stencil-pack-actions", id))
                            .label("···")
                            .tooltip("Pack properties and folders")
                            .xsmall()
                            .ghost()
                            .dropdown_menu(move |menu, _, _| {
                                let props = owner.clone();
                                let folders = owner.clone();
                                menu.item(PopupMenuItem::new("Properties / relink…").on_click(
                                    move |_, window, cx| {
                                        props
                                            .update(cx, |v, cx| v.asset_properties(id, window, cx))
                                            .ok();
                                    },
                                ))
                                .item(
                                    PopupMenuItem::new("Move to asset folder…").on_click(
                                        move |_, window, cx| {
                                            folders
                                                .update(cx, |v, cx| {
                                                    v.move_creative_asset_dialog(id, window, cx)
                                                })
                                                .ok();
                                        },
                                    ),
                                )
                            }),
                    )
                    .child(
                        Button::new(("stencil-pack-remove", id))
                            .label("Remove")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.catalog_edit(
                                    move |catalog| {
                                        catalog.remove_asset(id);
                                        Ok(())
                                    },
                                    cx,
                                )
                            })),
                    ),
            );
            if !expanded {
                continue;
            }
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
                list = list.child(
                    div()
                        .id((
                            ElementId::from("stencil-pack-item"),
                            format!("{}-{index}", asset.id),
                        ))
                        .test_support()
                        .flex()
                        .items_center()
                        .gap_2()
                        .p_1()
                        .border_1()
                        .border_color(p.line)
                        .rounded(px(5.))
                        .cursor_pointer()
                        .hover(|d| d.border_color(p.accent).bg(p.accent.opacity(0.06)))
                        .child(img(preview).size(px(42.)).object_fit(ObjectFit::Contain))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(11.))
                                .child(format!("{} · {name}", asset.name)),
                        )
                        .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.use_local_stencil(path.clone(), index, cx)
                        })),
                );
            }
        }
        if total > PAGE {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("stencil-previous")
                            .label("Previous")
                            .small()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.diagram_ui.stencil_page = page.saturating_sub(1);
                                cx.notify();
                            })),
                    )
                    .child(format!("{} / {}", page + 1, total.div_ceil(PAGE)))
                    .child(Button::new("stencil-next").label("Next").small().on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.diagram_ui.stencil_page =
                                (page + 1).min(total.saturating_sub(1) / PAGE);
                            cx.notify();
                        }),
                    )),
            );
        }
        list.child(div().text_size(px(10.)).text_color(p.muted).child("Saved packs stay in your library. Removing a pack keeps its source and placed objects."))
            .into_any_element()
    }
}
