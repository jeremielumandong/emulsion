//! Exposure-bracket preview and non-destructive float TIFF merge.
use super::*;
use emulsion_io::photo_hdr::{self, Deghost, Options};
use gpui_kit::component::{Disableable, Selectable, WindowExt};
use std::sync::atomic::{AtomicBool, Ordering};

impl Workspace {
    pub(super) fn library_hdr_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = self.batch.items.iter().filter(|i| i.selected).count();
        div()
            .flex()
            .gap_1()
            .child(
                Button::new("library-panorama-merge")
                    .label(t!("library.hdr.stitch_panorama_button"))
                    .small()
                    .ghost()
                    .disabled(
                        !(2..=9).contains(&count)
                            || self.batch.hdr_cancel.is_some()
                            || self.batch.mcp_busy,
                    )
                    .on_click(
                        cx.listener(|this, _, window, cx| {
                            this.library_hdr_dialog(window, cx, true)
                        }),
                    ),
            )
            .child(
                Button::new("library-hdr-merge")
                    .label(t!("library.hdr.hdr_merge_button"))
                    .small()
                    .ghost()
                    .disabled(
                        !(2..=9).contains(&count)
                            || self.batch.hdr_cancel.is_some()
                            || self.batch.mcp_busy
                            || self.batch.develop.saving
                            || self.batch.develop.busy
                            || self.batch.profiles.busy,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.library_hdr_dialog(window, cx, false)
                    })),
            )
            .into_any_element()
    }
    fn library_hdr_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>, panorama: bool) {
        let paths = self
            .batch
            .items
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.path.clone())
            .collect::<Vec<_>>();
        if !(2..=9).contains(&paths.len()) {
            return;
        }
        let owner = cx.weak_entity();
        let view = cx.new(|cx| {
            let ev = paths
                .iter()
                .map(|_| cx.new(|cx| InputState::new(window, cx).placeholder("EXIF EV")))
                .collect();
            HdrDialog {
                panorama,
                owner,
                paths,
                ev,
                options: Options::default(),
                overlay: false,
                image: None,
                busy: false,
                cancel: Arc::new(AtomicBool::new(false)),
                note: if panorama {
                    t!("library.hdr.note_panorama").into_owned()
                } else {
                    t!("library.hdr.note_hdr").into_owned()
                },
            }
        });
        let cancel = view.read(cx).cancel.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let cancel = cancel.clone();
            dialog
                .title(if panorama {
                    t!("library.hdr.title_panorama")
                } else {
                    t!("library.hdr.title_hdr")
                })
                .width(px(960.))
                .overlay_closable(false)
                .on_close(move |_, _, _| cancel.store(true, Ordering::Relaxed))
                .child(view.clone())
        });
    }
}
struct HdrDialog {
    panorama: bool,
    owner: WeakEntity<Workspace>,
    paths: Vec<PathBuf>,
    ev: Vec<Entity<InputState>>,
    options: Options,
    overlay: bool,
    image: Option<Arc<RenderImage>>,
    busy: bool,
    cancel: Arc<AtomicBool>,
    note: String,
}
impl HdrDialog {
    fn start(&mut self, full: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let values = self
            .ev
            .iter()
            .map(|input| input.read(cx).value().trim().to_owned())
            .collect::<Vec<_>>();
        let mut options = self.options.clone();
        if values.iter().any(|s| !s.is_empty()) {
            match values
                .iter()
                .map(|s| s.parse::<f32>())
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(v) if v.iter().all(|v| v.is_finite()) => options.exposure_ev = Some(v),
                _ => {
                    self.note = t!("library.hdr.ev_invalid").into_owned();
                    cx.notify();
                    return;
                }
            }
        }
        if !self
            .owner
            .update(cx, |ws, cx| {
                if ws.batch.hdr_cancel.is_some()
                    || ws.batch.mcp_busy
                    || ws.batch.develop.saving
                    || ws.batch.develop.busy
                    || ws.batch.profiles.busy
                {
                    return false;
                }
                ws.batch.hdr_cancel = Some(self.cancel.clone());
                ws.batch.develop.source = None;
                cx.notify();
                true
            })
            .unwrap_or(false)
        {
            self.note = t!("library.hdr.wait_library").into_owned();
            cx.notify();
            return;
        }
        self.busy = true;
        self.image = None;
        self.cancel.store(false, Ordering::Relaxed);
        self.note = if full {
            if self.panorama {
                t!("library.hdr.stitching_full")
            } else {
                t!("library.hdr.merging_full")
            }
        } else {
            if self.panorama {
                t!("library.hdr.building_panorama")
            } else {
                t!("library.hdr.building_hdr")
            }
        }
        .into_owned();
        let paths = self.paths.clone();
        let cancel = self.cancel.clone();
        let owner = self.owner.clone();
        let overlay = self.overlay;
        let panorama = self.panorama;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let merged = if panorama {
                        emulsion_io::photo_panorama::merge(&paths, !full, &cancel)?
                    } else {
                        photo_hdr::merge(&paths, &options, !full, &cancel)?
                    };
                    let preview = merged.preview(overlay, &cancel)?;
                    let output = if full {
                        let root = emulsion_io::creative_library::root().join(if panorama {
                            "panoramas"
                        } else {
                            "hdr"
                        });
                        std::fs::create_dir_all(&root)?;
                        let stamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos();
                        let suffix = if panorama { "Panorama" } else { "HDR" };
                        let path = root.join(format!("{stamp}-{suffix}.tif"));
                        merged.save(&path, &cancel)?;
                        Some(path)
                    } else {
                        None
                    };
                    let catalog = if let Some(path) = &output {
                        Some(
                            emulsion_io::creative_library::update(
                                &emulsion_io::creative_library::root(),
                                |c| {
                                    c.add_asset(
                                        path.clone(),
                                        emulsion_io::creative_library::AssetKind::Image,
                                    )?;
                                    Ok(())
                                },
                            )
                            .map_err(|e| {
                                emulsion_io::IoError::Manifest(
                                    t!(
                                        "library.hdr.saved_catalog_failed",
                                        path = path.display(),
                                        error = e
                                    )
                                    .into_owned(),
                                )
                            })?
                            .0,
                        )
                    } else {
                        None
                    };
                    let (w, h, pixels) = preview_bgra(&preview);
                    Ok::<_, emulsion_io::IoError>((w, h, pixels, output, catalog))
                })
                .await;
            owner
                .update(cx, |ws, cx| {
                    ws.batch.hdr_cancel = None;
                    ws.invalidate_library_preview();
                    if let Ok((_, _, _, _, Some(catalog))) = &result {
                        if catalog.revision >= ws.batch.library.catalog.revision {
                            ws.batch.library.catalog = catalog.clone();
                        }
                        ws.batch.library.source_paths = None;
                        ws.library_show(cx);
                    }
                    cx.notify();
                })
                .ok();
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok((w, h, pixels, output, _)) => {
                        this.image = Some(Arc::new(bgra_image(w, h, pixels)));
                        this.note = output.map_or_else(
                            || t!("library.hdr.preview_reduced").into_owned(),
                            |p| t!("library.hdr.saved_added", path = p.display()).into_owned(),
                        );
                    }
                    Err(e) => this.note = e.to_string(),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}
impl Render for HdrDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let mut options = div()
            .w(px(245.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                Checkbox::new("hdr-align")
                    .label(SharedString::from(t!("library.hdr.auto_align")))
                    .checked(self.options.align)
                    .disabled(self.busy)
                    .on_change(cx.listener(|this, v, _, cx| {
                        this.options.align = *v;
                        this.image = None;
                        cx.notify();
                    })),
            )
            .child(
                Checkbox::new("hdr-auto-tone")
                    .label(SharedString::from(t!("library.hdr.auto_tone")))
                    .checked(self.options.auto_tone)
                    .disabled(self.busy)
                    .on_change(cx.listener(|this, v, _, cx| {
                        this.options.auto_tone = *v;
                        this.image = None;
                        cx.notify();
                    })),
            )
            .child(label(t!("library.hdr.deghost"), &p));
        for (i, (value, name)) in [
            (Deghost::None, "library.hdr.deghost_none"),
            (Deghost::Low, "library.hdr.deghost_low"),
            (Deghost::Medium, "library.hdr.deghost_medium"),
            (Deghost::High, "library.hdr.deghost_high"),
        ]
        .into_iter()
        .enumerate()
        {
            options = options.child(
                Button::new(("hdr-deghost", i))
                    .label(t!(name))
                    .small()
                    .ghost()
                    .selected(self.options.deghost == value)
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.options.deghost = value;
                        this.image = None;
                        cx.notify();
                    })),
            );
        }
        options = options
            .child(
                Checkbox::new("hdr-overlay")
                    .label(SharedString::from(t!("library.hdr.overlay")))
                    .checked(self.overlay)
                    .disabled(self.busy)
                    .on_change(cx.listener(|this, v, _, cx| {
                        this.overlay = *v;
                        this.image = None;
                        cx.notify();
                    })),
            )
            .child(mono(t!("library.hdr.ev_overrides"), 11., p.muted));
        for (path, input) in self.paths.iter().zip(&self.ev) {
            options = options.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div().flex_1().overflow_hidden().child(mono(
                            path.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                            10.,
                            p.muted,
                        )),
                    )
                    .child(
                        div()
                            .w(px(75.))
                            .child(Input::new(input).small().disabled(self.busy)),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(420.))
                            .bg(p.stage)
                            .rounded(px(6.))
                            .overflow_hidden()
                            .when_some(self.image.clone(), |d, img_src| {
                                d.child(
                                    img(ImageSource::Render(img_src))
                                        .size_full()
                                        .object_fit(ObjectFit::Contain),
                                )
                            })
                            .when(self.image.is_none(), |d| {
                                d.child(div().p_4().child(label(
                                    if self.busy {
                                        t!("library.hdr.processing")
                                    } else {
                                        t!("library.hdr.preview_selected")
                                    },
                                    &p,
                                )))
                            }),
                    )
                    .child(if self.panorama {
                        div()
                            .w(px(245.))
                            .child(label(t!("library.hdr.panorama_hint"), &p))
                            .into_any_element()
                    } else {
                        options.into_any_element()
                    }),
            )
            .child(mono(self.note.clone(), 11., p.muted))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("hdr-preview")
                            .label(t!("library.hdr.update_preview"))
                            .small()
                            .outline()
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.start(false, cx))),
                    )
                    .child(
                        Button::new("hdr-merge")
                            .label(if self.panorama {
                                t!("library.hdr.save_panorama")
                            } else {
                                t!("library.hdr.merge_hdr")
                            })
                            .small()
                            .primary()
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.start(true, cx))),
                    )
                    .child(
                        Button::new("hdr-cancel")
                            .label(if self.busy {
                                t!("library.hdr.cancel_merge")
                            } else {
                                t!("library.hdr.close")
                            })
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel.store(true, Ordering::Relaxed);
                                if !this.busy {
                                    window.close_dialog(cx);
                                } else {
                                    this.note = t!("library.hdr.cancelling").into_owned();
                                    cx.notify();
                                }
                            })),
                    ),
            )
    }
}
