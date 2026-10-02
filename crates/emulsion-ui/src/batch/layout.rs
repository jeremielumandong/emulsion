//! Desktop Library/Develop chrome, using the same commands as the inspector.
use super::*;
use gpui_kit::component::Selectable;

/// Develop sections; the titles are catalog keys, translated where they are shown.
pub(super) const SECTIONS: [(usize, &str); 12] = [
    (0, "library.layout.section_basic"),
    (2, "library.layout.section_tone_curve"),
    (3, "library.layout.section_color_mixer"),
    (4, "library.layout.section_color_grading"),
    (9, "library.layout.section_detail"),
    (1, "library.layout.section_lens"),
    (10, "library.layout.section_calibration"),
    (11, "library.layout.section_parametric"),
    (5, "library.layout.section_masking"),
    (6, "library.layout.section_white_balance"),
    (8, "library.layout.section_enhance"),
    (7, "library.layout.section_history"),
];
impl Workspace {
    pub(super) fn library_relink_root_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::component::WindowExt;
        let old =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("library.layout.old_path")));
        let new =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("library.layout.new_path")));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let old = old.clone();
            let new = new.clone();
            let owner = owner.clone();
            dialog
                .title(t!("library.layout.relink_title"))
                .child(Input::new(&old))
                .child(Input::new(&new))
                .footer(crate::widgets::form_dialog_footer(t!(
                    "library.layout.relink"
                )))
                .on_ok(move |_, _, cx| {
                    let old = PathBuf::from(old.read(cx).value().to_string());
                    let new = PathBuf::from(new.read(cx).value().to_string());
                    if !old.is_absolute() || !new.is_dir() {
                        return false;
                    }
                    owner
                        .update(cx, |_, cx| {
                            cx.spawn(async move |this, cx| {
                                let result = cx
                                    .background_spawn(async move {
                                        emulsion_io::creative_library::update(
                                            &emulsion_io::creative_library::root(),
                                            |c| {
                                                emulsion_io::photo_catalog::relink_root(
                                                    c, &old, &new,
                                                )
                                            },
                                        )
                                    })
                                    .await;
                                this.update(cx, |this, cx| {
                                    match result {
                                        Ok((catalog, report)) => {
                                            this.batch.library.catalog = catalog;
                                            this.batch.library.source_paths = None;
                                            this.library_show(cx);
                                            this.batch.note = Some((
                                                t!(
                                                    "library.layout.relinked",
                                                    relinked = report.relinked,
                                                    metadata = report.metadata_only,
                                                    skipped = report.skipped.len()
                                                )
                                                .into(),
                                                !report.skipped.is_empty(),
                                            ));
                                        }
                                        Err(e) => {
                                            this.batch.note = Some((e.to_string().into(), true))
                                        }
                                    }
                                    cx.notify();
                                })
                                .ok();
                            })
                            .detach();
                        })
                        .ok();
                    true
                })
        });
    }
    pub(super) fn library_color_view_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div().flex().flex_col().gap_1().child(
            Button::new("library-proofing-toggle")
                .label(t!("library.layout.soft_proofing"))
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
            (0usize, "library.layout.soft_proof_profile"),
            (1usize, "library.layout.manual_display"),
        ] {
            panel = panel.child(
                Button::new(("library-view-profile", id))
                    .label(t!(label))
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let picker = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some(
                                if id == 0 {
                                    t!("library.layout.choose_proof_icc")
                                } else {
                                    t!("library.layout.choose_display_icc")
                                }
                                .into(),
                            ),
                        });
                        cx.spawn(async move |this, cx| {
                            if let Ok(Ok(Some(paths))) = picker.await
                                && let Some(path) = paths.first()
                            {
                                let path = path.clone();
                                let check_path = path.clone();
                                let validation = cx
                                    .background_spawn(async move {
                                        let mut view = emulsion_io::icc::PhotoView::default();
                                        if id == 0 {
                                            view.proof = Some(check_path);
                                        } else {
                                            view.display = Some(check_path);
                                        }
                                        view.validate().map_err(|error| error.to_string())
                                    })
                                    .await;
                                this.update(cx, |this, cx| {
                                    if let Err(error) = validation {
                                        this.batch.note = Some((error.into(), true));
                                        cx.notify();
                                        return;
                                    }
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
                    .label(SharedString::from(t!("library.layout.proof_gamut")))
                    .checked(self.batch.develop.color_view.gamut_warning)
                    .on_change(cx.listener(|this, value, _, cx| {
                        this.batch.develop.color_view.gamut_warning = *value;
                        this.invalidate_library_preview();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("library-view-profile-clear")
                    .label(t!("library.layout.system_colors"))
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
                    .map(|p| p.name.clone())
            })
            .unwrap_or_else(|| {
                if params.camera_profile.is_some() {
                    t!("library.layout.missing_profile")
                } else {
                    t!("library.layout.camera_color")
                }
                .into_owned()
            });
        let mut panel = div().flex().flex_col().gap_1().child(
            Button::new("develop-profile-toggle")
                .label(t!("library.layout.profile_named", name = name))
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
            .is_some_and(|s| s.supports_wide_gamut())
        {
            panel = panel.child(
                Checkbox::new("develop-wide-working")
                    .label(SharedString::from(t!("library.layout.prophoto")))
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
        panel.into_any_element()
    }
    pub(super) fn library_import_profile_button(&self, cx: &mut Context<Self>) -> AnyElement {
        Button::new("develop-import-profile")
            .label(t!("library.layout.import_profile"))
            .small()
            .outline()
            .on_click(cx.listener(|_, _, _, cx| {
                let picker = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some(t!("library.layout.import_dcp").into()),
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
                                        t!("library.layout.imported_profile", name = profile.name)
                                            .into(),
                                        false,
                                    ));
                                }
                                Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                            }
                            cx.notify();
                        })
                        .ok();
                    }
                })
                .detach();
            }))
            .into_any_element()
    }
    pub(super) fn library_module_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = classic::palette(cx);
        div()
            .id("library-module-picker")
            .test_support()
            .flex()
            .items_center()
            .flex_none()
            .h(px(48.))
            .px_4()
            .gap_3()
            .bg(classic::palette(cx).panel)
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(16.)).text_color(p.ink).child("Emulsion"))
                    .child(mono(t!("library.layout.photo_library"), 9., p.muted)),
            )
            .child(div().flex_1())
            .children(
                [
                    (false, "library.layout.library"),
                    (true, "library.batch.develop"),
                ]
                .into_iter()
                .enumerate()
                .map(|(i, (develop, title))| {
                    Button::new(("library-module", i))
                        .label(t!(title))
                        .text_size(px(15.))
                        .rounded_none()
                        .ghost()
                        .selected(self.batch.develop.module_develop == develop)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.batch.develop.module_develop = develop;
                            this.batch.develop.culling_mode = 0;
                            this.batch.develop.loupe = develop;
                            this.batch.develop.list = false;
                            this.batch.develop.inspector = 0;
                            this.batch.develop.canvas_tool = 0;
                            if develop {
                                this.invalidate_library_preview();
                            }
                            cx.notify();
                        }))
                }),
            )
            .child(
                Button::new("library-hide-panels")
                    .label(t!("library.layout.panels"))
                    .small()
                    .ghost()
                    .selected(!self.batch.develop.panels_hidden)
                    .tooltip(t!("library.layout.panels_tip"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.develop.panels_hidden = !this.batch.develop.panels_hidden;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("library-hide-filmstrip")
                    .label(t!("library.layout.filmstrip"))
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
        if self.batch.develop.module_develop {
            return div()
                .id("library-workflow-toolbar")
                .test_support()
                .flex_none()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .bg(classic::palette(cx).panel)
                .child(
                    Button::new("library-before-after")
                        .label(t!("library.batch.before_after"))
                        .small()
                        .ghost()
                        .selected(self.batch.develop.compare)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.batch.develop.compare = !this.batch.develop.compare;
                            this.batch.develop.canvas_tool = 0;
                            this.batch.develop.detail_region = None;
                            this.batch.develop.before = false;
                            this.invalidate_library_preview();
                            cx.notify();
                        })),
                )
                .child(self.library_edit_photo_button(cx))
                .child(self.library_hdr_button(cx))
                .child(self.library_color_view_panel(cx))
                .child(div().flex_1())
                .when(self.batch.develop.canvas_tool != 0, |d| {
                    d.child(
                        Button::new("library-tool-done")
                            .label(t!("library.layout.done"))
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.batch.develop.canvas_tool = 0;
                                this.invalidate_library_preview();
                                cx.notify();
                            })),
                    )
                })
                .into_any_element();
        }
        div()
            .id("library-workflow-toolbar")
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_2()
            .child(self.library_hdr_button(cx))
            .when(self.batch.develop.module_develop, |d| {
                d.child(
                    Button::new("library-before-after")
                        .label(t!("library.batch.before_after"))
                        .small()
                        .ghost()
                        .selected(self.batch.develop.compare)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.batch.develop.compare = !this.batch.develop.compare;
                            this.batch.develop.canvas_tool = 0;
                            this.batch.develop.detail_region = None;
                            this.batch.develop.before = false;
                            this.invalidate_library_preview();
                            cx.notify();
                        })),
                )
            })
            .child(
                Checkbox::new("library-auto-advance")
                    .label(SharedString::from(t!("library.layout.auto_advance")))
                    .checked(self.batch.develop.auto_advance)
                    .on_change(cx.listener(|this, value, _, cx| {
                        this.batch.develop.auto_advance = *value;
                        cx.notify();
                    })),
            )
            .children(
                [
                    (1usize, "library.layout.compare_photos"),
                    (2usize, "library.layout.survey"),
                ]
                .into_iter()
                .map(|(mode, title)| {
                    Button::new(("library-culling-mode", mode))
                        .label(t!(title))
                        .small()
                        .ghost()
                        .selected(self.batch.develop.culling_mode == mode)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.batch.develop.module_develop = false;
                            this.batch.develop.culling_mode =
                                if this.batch.develop.culling_mode == mode {
                                    0
                                } else {
                                    mode
                                };
                            this.batch.develop.loupe = true;
                            cx.notify();
                        }))
                }),
            )
            .child(
                Button::new("library-create-proxy")
                    .label(t!("library.layout.build_proxies"))
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let paths = this.library_paths();
                        cx.spawn(async move |this, cx| {
                            let result = cx
                                .background_spawn(async move {
                                    for path in paths {
                                        emulsion_io::photo_proxy::create(&path)?;
                                    }
                                    Ok::<_, emulsion_io::IoError>(())
                                })
                                .await;
                            this.update(cx, |this, cx| {
                                this.batch.note = Some(match result {
                                    Ok(()) => (t!("library.layout.proxies_ready").into(), false),
                                    Err(e) => (e.to_string().into(), true),
                                });
                                cx.notify();
                            })
                            .ok();
                        })
                        .detach();
                    })),
            )
            .into_any_element()
    }
    pub(super) fn library_navigator(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = classic::palette(cx);
        let active = self.batch.current.and_then(|i| self.batch.items.get(i));
        let path = active.map(|i| i.path.clone());
        let image = self
            .batch
            .develop
            .navigator_preview
            .as_ref()
            .filter(|(p, _)| Some(p) == path.as_ref())
            .map(|(_, image)| image.clone())
            .or_else(|| active.and_then(|i| i.thumb.clone()));
        let nav_bounds = self.batch.develop.navigator_bounds.clone();
        let panel = div()
            .id("library-navigator")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(label(t!("library.layout.navigator"), &p))
                    .child(
                        Button::new("library-navigator-fit")
                            .label(t!("library.layout.fit"))
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.batch.navigation.borrow_mut().fit();
                                if this.batch.develop.detail_region.take().is_some() {
                                    this.invalidate_library_preview();
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("library-navigator-image")
                    .test_support()
                    .h(px(156.))
                    .bg(p.stage)
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            if this.batch.develop.detail_region.is_none() {
                                return;
                            }
                            let Some(bounds) = this.batch.develop.navigator_bounds.get() else {
                                return;
                            };
                            if !bounds.contains(&event.position) {
                                return;
                            }
                            let center = [
                                f32::from(event.position.x - bounds.origin.x)
                                    / f32::from(bounds.size.width),
                                f32::from(event.position.y - bounds.origin.y)
                                    / f32::from(bounds.size.height),
                            ];
                            this.batch.develop.detail_region = Some(center);
                            this.invalidate_library_preview();
                            cx.notify();
                        }),
                    )
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                if let Some(image) = &image {
                                    let dimensions = image.size(0);
                                    let (w, h) =
                                        (dimensions.width.0 as f32, dimensions.height.0 as f32);
                                    let scale = (f32::from(bounds.size.width) / w)
                                        .min(f32::from(bounds.size.height) / h);
                                    let rect = Bounds::new(
                                        bounds.center()
                                            - point(px(w * scale * 0.5), px(h * scale * 0.5)),
                                        size(px(w * scale), px(h * scale)),
                                    );
                                    nav_bounds.set(Some(rect));
                                    let _ = window.paint_image(
                                        rect,
                                        rect,
                                        Corners::default(),
                                        image.clone(),
                                        0,
                                        false,
                                    );
                                }
                            },
                        )
                        .size_full(),
                    ),
            );
        panel.into_any_element()
    }
    pub(super) fn library_develop_left(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let path = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone());
        let mut panel = div()
            .id("library-develop-left")
            .test_support()
            .flex()
            .flex_col()
            .child(self.library_navigator(cx));
        for (index, title) in [
            "library.layout.presets",
            "library.layout.snapshots",
            "library.layout.collections",
        ]
        .into_iter()
        .enumerate()
        {
            panel = panel.child(
                Button::new(("library-left-section", index))
                    .label(t!(title))
                    .w_full()
                    .h(px(29.))
                    .rounded_none()
                    .justify_start()
                    .bg(classic::palette(cx).panel)
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
        let p = classic::palette(cx);
        div()
            .w_full()
            .bg(classic::palette(cx).panel)
            .border_t_1()
            .border_color(p.line)
            .child(
                Button::new(("library-develop-section", index))
                    .label(format!(
                        "{}  {}",
                        t!(title),
                        if self.batch.develop.section == index {
                            "▾"
                        } else {
                            "▸"
                        }
                    ))
                    .w_full()
                    .h(px(29.))
                    .rounded_none()
                    .justify_end()
                    .small()
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
        if self.batch.develop.module_develop && self.batch.develop.inspector == 0 {
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
