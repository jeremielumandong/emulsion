//! Desktop Library/Develop chrome, using the same commands as the inspector.
use super::*;
use gpui_kit::component::Selectable;

pub(super) const SECTIONS: [(usize, &str); 12] = [
    (0, "Basic"),
    (2, "Tone Curve"),
    (3, "Color Mixer"),
    (4, "Color Grading"),
    (9, "Detail"),
    (1, "Lens Corrections / Transform"),
    (10, "Calibration"),
    (11, "Parametric Curve"),
    (5, "Masking"),
    (6, "White Balance"),
    (8, "Enhance"),
    (7, "History"),
];
impl Workspace {
    pub(super) fn library_relink_root_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::component::WindowExt;
        let old = cx.new(|cx| InputState::new(window, cx).placeholder("Old absolute folder path"));
        let new = cx.new(|cx| InputState::new(window, cx).placeholder("Replacement folder path"));
        let owner = cx.weak_entity();
        window.open_dialog(cx,move|dialog,_,_|{let old=old.clone();let new=new.clone();let owner=owner.clone();dialog.title("Relink folder root").child(Input::new(&old)).child(Input::new(&new)).footer(crate::widgets::form_dialog_footer("Relink"))
            .on_ok(move|_,_,cx|{let old=PathBuf::from(old.read(cx).value().to_string());let new=PathBuf::from(new.read(cx).value().to_string());if !old.is_absolute()||!new.is_dir(){return false;}owner.update(cx,|_,cx|{cx.spawn(async move|this,cx|{let result=cx.background_spawn(async move{emulsion_io::creative_library::update(&emulsion_io::creative_library::root(),|c|emulsion_io::photo_catalog::relink_root(c,&old,&new))}).await;this.update(cx,|this,cx|{match result{Ok((catalog,report))=>{this.batch.library.catalog=catalog;this.batch.library.source_paths=None;this.library_show(cx);this.batch.note=Some((format!("Relinked {} photos ({} metadata-only references); {} skipped.",report.relinked,report.metadata_only,report.skipped.len()).into(),!report.skipped.is_empty()));},Err(e)=>this.batch.note=Some((e.to_string().into(),true))}cx.notify();}).ok();}).detach();}).ok();true})});
    }
    pub(super) fn library_color_view_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div().flex().flex_col().gap_1().child(
            Button::new("library-proofing-toggle")
                .label("Soft Proofing / Display")
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.batch.develop.color_view_open = !this.batch.develop.color_view_open;
                    cx.notify();
                })),
        );
        if !self.batch.develop.color_view_open {
            return panel.into_any_element();
        }
        for (id, label) in [
            (0usize, "Soft-proof profile…"),
            (1usize, "Manual display profile…"),
        ] {
            panel = panel.child(
                Button::new(("library-view-profile", id))
                    .label(label)
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let picker = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some("Choose an RGB ICC profile".into()),
                        });
                        cx.spawn(async move |this, cx| {
                            if let Ok(Ok(Some(paths))) = picker.await
                                && let Some(path) = paths.first()
                            {
                                let path = path.clone();
                                this.update(cx, |this, cx| {
                                    if id == 0 {
                                        this.batch.develop.color_view.proof = Some(path);
                                    } else {
                                        this.batch.develop.color_view.display = Some(path);
                                    }
                                    this.invalidate_library_preview();
                                    cx.notify();
                                })
                                .ok();
                            }
                        })
                        .detach();
                    })),
            );
        }
        panel
            .child(
                Checkbox::new("library-proof-gamut")
                    .label("Proof gamut warning")
                    .checked(self.batch.develop.color_view.gamut_warning)
                    .on_change(cx.listener(|this, value, _, cx| {
                        this.batch.develop.color_view.gamut_warning = *value;
                        this.invalidate_library_preview();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("library-view-profile-clear")
                    .label("Use system display colors")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.develop.color_view = Default::default();
                        this.invalidate_library_preview();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
    pub(super) fn library_profile_panel(
        &mut self,
        params: emulsion_core::raw::DevelopParams,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.batch.develop.profiles.is_none() {
            self.batch.develop.profiles = Some(vec![]);
            cx.spawn(async move |this, cx| {
                let profiles = cx
                    .background_spawn(async { emulsion_io::camera_profiles::installed() })
                    .await;
                this.update(cx, |this, cx| {
                    this.batch.develop.profiles = Some(profiles);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        let name = params
            .camera_profile
            .and_then(|digest| {
                self.batch
                    .develop
                    .profiles
                    .as_ref()
                    .unwrap()
                    .iter()
                    .find(|p| p.digest == digest)
                    .map(|p| p.name.as_str())
            })
            .unwrap_or(if params.camera_profile.is_some() {
                "Missing camera profile"
            } else {
                "Camera color"
            });
        let mut panel = div().flex().flex_col().gap_1().child(
            Button::new("develop-profile-toggle")
                .label(format!("Profile · {name}"))
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.batch.develop.profiles_open = !this.batch.develop.profiles_open;
                    cx.notify();
                })),
        );
        if self
            .batch
            .develop
            .source
            .as_ref()
            .is_some_and(|s| emulsion_io::photo_develop::is_raw_photo(&s.source))
        {
            panel = panel.child(
                Checkbox::new("develop-wide-working")
                    .label("Wide-gamut RAW working space")
                    .checked(params.wide_gamut)
                    .on_change(cx.listener(move |this, value, _, cx| {
                        this.library_adjust(
                            emulsion_core::raw::DevelopParams {
                                wide_gamut: *value,
                                process_version: 2,
                                ..params
                            },
                            cx,
                        )
                    })),
            );
        }
        if !self.batch.develop.profiles_open {
            return panel.into_any_element();
        }
        panel = panel.child(
            Button::new("develop-profile-camera")
                .label("Camera color")
                .small()
                .ghost()
                .selected(params.camera_profile.is_none())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.library_adjust(
                        emulsion_core::raw::DevelopParams {
                            camera_profile: None,
                            ..params
                        },
                        cx,
                    )
                })),
        );
        for (index, profile) in self
            .batch
            .develop
            .profiles
            .as_ref()
            .unwrap()
            .iter()
            .enumerate()
        {
            let compatible = self.batch.develop.source.as_ref().is_some_and(|s| {
                emulsion_io::photo_develop::is_raw_photo(&s.source)
                    && profile.compatible(&s.metadata.make, &s.metadata.model)
            });
            if compatible {
                let digest = profile.digest;
                panel = panel.child(
                    Button::new(("develop-profile", index))
                        .label(profile.name.clone())
                        .small()
                        .ghost()
                        .selected(params.camera_profile == Some(digest))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.library_adjust(
                                emulsion_core::raw::DevelopParams {
                                    camera_profile: Some(digest),
                                    ..params
                                },
                                cx,
                            )
                        })),
                );
            }
        }
        panel
            .child(
                Button::new("develop-import-profile")
                    .label("Import camera profile…")
                    .small()
                    .outline()
                    .on_click(cx.listener(|_, _, _, cx| {
                        let picker = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some("Import a DCP camera profile".into()),
                        });
                        cx.spawn(async move |this, cx| {
                            if let Ok(Ok(Some(files))) = picker.await
                                && let Some(file) = files.first()
                            {
                                let file = file.clone();
                                let result = cx
                                    .background_spawn(async move {
                                        emulsion_io::camera_profiles::install(&file)
                                    })
                                    .await;
                                this.update(cx, |this, cx| {
                                    match result {
                                        Ok(profile) => {
                                            this.batch.develop.profiles = None;
                                            this.batch.note = Some((
                                                format!("Imported profile {}", profile.name).into(),
                                                false,
                                            ));
                                        }
                                        Err(e) => {
                                            this.batch.note = Some((e.to_string().into(), true))
                                        }
                                    }
                                    cx.notify();
                                })
                                .ok();
                            }
                        })
                        .detach();
                    })),
            )
            .into_any_element()
    }
    pub(super) fn library_module_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        div()
            .id("library-module-picker")
            .test_support()
            .flex()
            .items_center()
            .flex_none()
            .h(px(46.))
            .px_4()
            .gap_3()
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .child(mono("EMULSION", 14., p.ink))
            .child(div().flex_1())
            .children(
                [(false, "Library"), (true, "Develop")]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (develop, title))| {
                        Button::new(("library-module", i))
                            .label(title)
                            .ghost()
                            .selected(self.batch.develop.module_develop == develop)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.batch.develop.module_develop = develop;
                                this.batch.develop.culling_mode = 0;
                                this.batch.develop.loupe = develop;
                                this.batch.develop.list = false;
                                this.batch.develop.inspector = 0;
                                cx.notify();
                            }))
                    }),
            )
            .child(
                Button::new("library-hide-panels")
                    .label("Panels")
                    .small()
                    .ghost()
                    .selected(!self.batch.develop.panels_hidden)
                    .tooltip("Show or hide side panels · Tab")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.develop.panels_hidden = !this.batch.develop.panels_hidden;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("library-hide-filmstrip")
                    .label("Filmstrip")
                    .small()
                    .ghost()
                    .selected(!self.batch.develop.filmstrip_hidden)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.develop.filmstrip_hidden = !this.batch.develop.filmstrip_hidden;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
    pub(super) fn library_workflow_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        div().id("library-workflow-toolbar").flex().flex_wrap().items_center().gap_1().px_2()
            .child(Checkbox::new("library-auto-advance").label("Auto advance").checked(self.batch.develop.auto_advance)
                .on_change(cx.listener(|this,value,_,cx|{this.batch.develop.auto_advance=*value;cx.notify();})))
            .children([(1usize,"Compare photos"),(2usize,"Survey")].into_iter().map(|(mode,title)|Button::new(("library-culling-mode",mode)).label(title).small().ghost().selected(self.batch.develop.culling_mode==mode).on_click(cx.listener(move|this,_,_,cx|{this.batch.develop.module_develop=false;this.batch.develop.culling_mode=if this.batch.develop.culling_mode==mode{0}else{mode};this.batch.develop.loupe=true;cx.notify();}))))
            .child(Button::new("library-create-proxy").label("Build proxies").small().ghost().on_click(cx.listener(|this,_,_,cx|{let paths=this.library_paths();cx.spawn(async move|this,cx|{let result=cx.background_spawn(async move{for path in paths{emulsion_io::photo_proxy::create(&path)?;}Ok::<_,emulsion_io::IoError>(())}).await;this.update(cx,|this,cx|{this.batch.note=Some(match result{Ok(())=>("Offline edit proxies ready. Originals are required for export.".into(),false),Err(e)=>(e.to_string().into(),true)});cx.notify();}).ok();}).detach();})))
            .into_any_element()
    }
    pub(super) fn library_develop_left(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let active = self.batch.current.and_then(|i| self.batch.items.get(i));
        let path = active.map(|i| i.path.clone());
        let image=self.batch.develop.navigator_preview.as_ref().filter(|(p,_)|Some(p)==path.as_ref()).map(|(_,image)|image.clone()).or_else(||active.and_then(|i|i.thumb.clone()));
        let nav_bounds=self.batch.develop.navigator_bounds.clone();
        let mut panel = div()
            .id("library-develop-left")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(label("Navigator", &p))
                    .child(
                        Button::new("library-navigator-fit")
                            .label("Fit")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.batch.navigation.borrow_mut().fit();
                                if this.batch.develop.detail_region.take().is_some(){this.invalidate_library_preview();}
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div().id("library-navigator-image").test_support().h(px(148.)).bg(p.stage).overflow_hidden()
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,event:&MouseDownEvent,_,cx|{
                        if this.batch.develop.detail_region.is_none(){return;}
                        let Some(bounds)=this.batch.develop.navigator_bounds.get()else{return;};
                        if !bounds.contains(&event.position){return;}
                        let center=[f32::from(event.position.x-bounds.origin.x)/f32::from(bounds.size.width),f32::from(event.position.y-bounds.origin.y)/f32::from(bounds.size.height)];
                        this.batch.develop.detail_region=Some(center);this.invalidate_library_preview();cx.notify();
                    }))
                    .child(canvas(|_,_,_|{},move|bounds,_,window,_|{
                        if let Some(image)=&image{let dimensions=image.size(0);let(w,h)=(dimensions.width.0 as f32,dimensions.height.0 as f32);let scale=(f32::from(bounds.size.width)/w).min(f32::from(bounds.size.height)/h);let rect=Bounds::new(bounds.center()-point(px(w*scale*0.5),px(h*scale*0.5)),size(px(w*scale),px(h*scale)));nav_bounds.set(Some(rect));let _=window.paint_image(rect,rect,Corners::default(),image.clone(),0,false);}
                    }).size_full()),
            );
        for (index, title) in ["Presets", "Snapshots / History", "Collections"]
            .into_iter()
            .enumerate()
        {
            panel = panel.child(
                Button::new(("library-left-section", index))
                    .label(title)
                    .ghost()
                    .selected(self.batch.develop.left_section == index)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.develop.left_section = index;
                        cx.notify();
                    })),
            );
            if self.batch.develop.left_section == index {
                if let Some(path) = path.as_ref()
                    && let Some(params) = self.batch.develop.current_params(path)
                {
                    panel = match index {
                        0 => panel.child(self.library_preset_bank(params, cx)),
                        1 => panel.child(self.library_history_panel(path.clone(), params, cx)),
                        _ => panel,
                    };
                }
                if index == 2 {
                    for collection in self.batch.library.catalog.collections.clone() {
                        panel = panel.child(
                            Button::new(("develop-collection", collection.id as usize))
                                .label(collection.name)
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.batch.library.collection = Some(collection.id);
                                    this.library_show(cx);
                                })),
                        );
                    }
                }
            }
        }
        panel.into_any_element()
    }
    pub(super) fn library_section_header(
        &self,
        index: usize,
        title: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::palette(cx);
        div()
            .w_full()
            .border_t_1()
            .border_color(p.line)
            .py_1()
            .child(
                Button::new(("library-develop-section", index))
                    .label(format!(
                        "{}  {title}",
                        if self.batch.develop.section == index {
                            "▾"
                        } else {
                            "▸"
                        }
                    ))
                    .ghost()
                    .selected(self.batch.develop.section == index)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.develop.section = index;
                        this.batch.develop.slider_key = None;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
    pub(super) fn library_develop_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let content = self.library_develop_content(cx);
        let mut panel = div().flex().flex_col().child(content);
        if self.batch.develop.inspector == 0 {
            let current = SECTIONS
                .iter()
                .position(|(id, _)| *id == self.batch.develop.section)
                .unwrap_or(0);
            for &(index, title) in &SECTIONS[current + 1..] {
                panel = panel.child(self.library_section_header(index, title, cx));
            }
        }
        panel.into_any_element()
    }
}
