//! Author and install portable Design templates and Diagram stencil packs.
use super::*;
use emulsion_io::{
    creative_library::{self as library, AssetKind},
    template_pack::{self, Kind, Manifest},
};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
};
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
                        .map(|(catalog, _)| (catalog, count, warnings))
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((catalog, count, warnings)) => {
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
            for page in &mut project.pages {
                page.doc
                    .nodes
                    .retain(|n| !(n.parent.is_none() && matches!(n.kind, NodeKind::Fill { .. })));
                page.graph = emulsion_core::graph::Graph::new(page.doc.clone(), "Stencil");
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
                        let file=format!("template.{}",kind.extension());let rx=cx.prompt_for_new_path(&dir,Some(&file));
                        cx.spawn(async move|this,cx|{
                            let Ok(Ok(Some(mut path)))=rx.await else{return;};path.set_extension(kind.extension());let output=path.clone();
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
                        let offset = (
                            (this.editor.doc.width as f64 - bounds.w as f64) / 2. - bounds.x as f64,
                            (this.editor.doc.height as f64 - bounds.h as f64) / 2.
                                - bounds.y as f64,
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
        let mut list = div().flex().flex_col().gap_1();
        for asset in self
            .creative
            .catalog
            .assets
            .iter()
            .filter(|a| a.kind == AssetKind::Stencil)
        {
            for (index, name) in asset.variants.iter().enumerate() {
                if !format!("{} {name} {}", asset.name, asset.tags.join(" "))
                    .to_lowercase()
                    .contains(query)
                {
                    continue;
                }
                let path = asset.path.clone();
                list = list.child(
                    Button::new((
                        ElementId::from("stencil-pack-item"),
                        format!("{}-{index}", asset.id),
                    ))
                    .label(format!("{} · {name}", asset.name))
                    .tooltip(format!("{}\n{}", asset.attribution, asset.license))
                    .small()
                    .outline()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.use_local_stencil(path.clone(), index, cx)
                    })),
                );
            }
        }
        list.child(self.creative_asset_list(AssetKind::Stencil, query, p, cx))
            .into_any_element()
    }
}
