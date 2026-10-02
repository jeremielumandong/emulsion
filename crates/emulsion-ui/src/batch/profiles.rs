//! Camera-profile cards are rendered from the active source, never recolored thumbnails.
use super::*;
use emulsion_io::{
    photo_profiles::{self, Digest},
    raw::DevelopParams,
};
use gpui_kit::component::Selectable;
use std::collections::{BTreeSet, HashMap};
#[derive(Default)]
pub(super) struct Browser {
    key: Option<(PathBuf, String, DevelopParams)>,
    generation: u64,
    pub(super) busy: bool,
    images: HashMap<Option<Digest>, Arc<RenderImage>>,
    errors: HashMap<Option<Digest>, String>,
    visible: Vec<Option<Digest>>,
    order: VecDeque<Option<Digest>>,
    search: Option<(Entity<InputState>, Subscription)>,
    pub(super) favorites: Option<BTreeSet<Digest>>,
    only_favorites: bool,
}
impl Workspace {
    pub(super) fn library_profile_browser(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::palette(cx);
        let _ = self.library_profile_panel(Default::default(), cx);
        if self.batch.profiles.search.is_none() {
            let input =
                cx.new(|cx| InputState::new(window, cx).placeholder(t!("library.profiles.search")));
            let sub = cx.subscribe(&input, |_, _, _: &InputEvent, cx| cx.notify());
            self.batch.profiles.search = Some((input, sub));
        }
        if self.batch.profiles.favorites.is_none() {
            self.batch.profiles.favorites = Some(photo_profiles::favorites());
        }
        let Some(source) = self.batch.develop.source.clone() else {
            return div()
                .p_3()
                .child(t!("library.profiles.select_photo"))
                .child(
                    Button::new("library-profile-close")
                        .label(t!("library.profiles.close"))
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.batch.develop.profiles_open = false;
                            cx.notify();
                        })),
                )
                .into_any_element();
        };
        let params = self
            .batch
            .develop
            .current_params(&source.source)
            .unwrap_or_default();
        let mut key_params = params;
        key_params.camera_profile = None;
        let key = (
            source.source.clone(),
            source.source_sha256.clone(),
            key_params,
        );
        if self.batch.profiles.key.as_ref() != Some(&key) {
            let b = &mut self.batch.profiles;
            b.key = Some(key);
            b.generation = b.generation.wrapping_add(1);
            b.images.clear();
            b.errors.clear();
            b.order.clear();
            b.visible.clear();
        }
        let search = self.batch.profiles.search.as_ref().unwrap().0.clone();
        let query = search.read(cx).value().to_lowercase();
        let favorites = self.batch.profiles.favorites.as_ref().unwrap().clone();
        let mut entries = vec![(None, t!("library.profiles.camera_color").into_owned())];
        for profile in self.batch.develop.profiles.as_deref().unwrap_or_default() {
            if emulsion_io::photo_develop::is_raw_photo(&source.source)
                && profile.compatible(&source.metadata.make, &source.metadata.model)
            {
                entries.push((Some(profile.digest), profile.name.clone()));
            }
        }
        let entries = Arc::new(
            entries
                .into_iter()
                .enumerate()
                .filter(|(_, (digest, name))| {
                    name.to_lowercase().contains(&query)
                        && (!self.batch.profiles.only_favorites
                            || digest.is_some_and(|d| favorites.contains(&d)))
                })
                .collect::<Vec<_>>(),
        );
        let count = entries.len();
        let filter = self.batch.profiles.only_favorites;
        let ids = entries.clone();
        let list = uniform_list(
            SharedString::from(format!("profile-grid-{query}-{filter}")),
            count.div_ceil(2),
            cx.processor(move |this, rows: Range<usize>, window, cx| {
                this.batch.profiles.visible = ids[rows.start * 2..(rows.end * 2).min(ids.len())]
                    .iter()
                    .map(|(_, (id, _))| *id)
                    .collect();
                cx.defer_in(window, |this, _, cx| this.library_load_profile_previews(cx));
                rows.map(|row| {
                    let mut element = div().flex().gap_2().h(px(136.)).pt_2();
                    for (index, (digest, name)) in &ids[row * 2..((row + 1) * 2).min(ids.len())] {
                        let digest = *digest;
                        let mut card = div()
                            .id(("library-profile-card", *index))
                            .test_support()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1();
                        let preview = div()
                            .id(("library-profile-image", *index))
                            .test_support()
                            .h(px(88.))
                            .w_full()
                            .bg(p.stage)
                            .rounded(px(5.))
                            .border_2()
                            .border_color(if params.camera_profile == digest {
                                p.accent
                            } else {
                                p.line
                            })
                            .overflow_hidden()
                            .when_some(
                                this.batch.profiles.images.get(&digest).cloned(),
                                |d, image| {
                                    d.child(
                                        img(ImageSource::Render(image))
                                            .size_full()
                                            .object_fit(ObjectFit::Contain),
                                    )
                                },
                            )
                            .when(!this.batch.profiles.images.contains_key(&digest), |d| {
                                d.child(mono(
                                    if this.batch.profiles.errors.contains_key(&digest) {
                                        t!("new_canvas.preview_unavailable")
                                    } else {
                                        t!("library.profiles.rendering")
                                    },
                                    10.,
                                    p.muted,
                                ))
                            })
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.library_adjust(
                                    DevelopParams {
                                        camera_profile: digest,
                                        ..params
                                    },
                                    cx,
                                )
                            }));
                        card = card.child(preview).child(
                            Button::new(("develop-profile", *index))
                                .label(name.clone())
                                .small()
                                .ghost()
                                .w_full()
                                .selected(params.camera_profile == digest)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.library_adjust(
                                        DevelopParams {
                                            camera_profile: digest,
                                            ..params
                                        },
                                        cx,
                                    )
                                })),
                        );
                        if let Some(digest) = digest {
                            let on = favorites.contains(&digest);
                            card = card.child(
                                Button::new(("profile-favorite", *index))
                                    .label(if on {
                                        t!("library.profiles.favorite_on")
                                    } else {
                                        t!("library.profiles.favorite_off")
                                    })
                                    .xsmall()
                                    .ghost()
                                    .selected(on)
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.spawn(async move |this, cx| {
                                            let result = cx
                                                .background_spawn(async move {
                                                    photo_profiles::set_favorite(digest, !on)
                                                })
                                                .await;
                                            this.update(cx, |this, cx| {
                                                match result {
                                                    Ok(()) => this.batch.profiles.favorites = None,
                                                    Err(e) => {
                                                        this.batch.note =
                                                            Some((e.to_string().into(), true))
                                                    }
                                                }
                                                cx.notify();
                                            })
                                            .ok();
                                        })
                                        .detach();
                                    })),
                            );
                        }
                        element = element.child(card);
                    }
                    element
                })
                .collect()
            }),
        )
        .w_full()
        .h(px(360.));
        div()
            .id("library-profile-browser")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(t!("library.profiles.title"), &p))
                    .child(
                        Button::new("library-profile-close")
                            .label(t!("library.profiles.close"))
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.batch.develop.profiles_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(Input::new(&search).small())
            .child(
                Button::new("library-profile-favorites")
                    .label(t!("library.profiles.favorites_only"))
                    .small()
                    .ghost()
                    .selected(filter)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.profiles.only_favorites = !this.batch.profiles.only_favorites;
                        cx.notify();
                    })),
            )
            .child(mono(
                t!("library.profiles.compatible", count = count),
                10.,
                p.muted,
            ))
            .child(list)
            .child(self.library_import_profile_button(cx))
            .when(count == 0, |d| {
                d.child(mono(t!("library.profiles.no_matches"), 11., p.muted))
            })
            .into_any_element()
    }
    fn library_load_profile_previews(&mut self, cx: &mut Context<Self>) {
        if !self.batch.develop.profiles_open
            || self.batch.profiles.busy
            || self.batch.hdr_cancel.is_some()
        {
            return;
        }
        let Some(source) = self.batch.develop.source.clone() else {
            return;
        };
        let Some((path, hash, params)) = self.batch.profiles.key.clone() else {
            return;
        };
        if path != source.source || hash != source.source_sha256 {
            return;
        }
        let b = &mut self.batch.profiles;
        let Some(digest) = b
            .visible
            .iter()
            .copied()
            .find(|id| !b.images.contains_key(id) && !b.errors.contains_key(id))
        else {
            return;
        };
        b.busy = true;
        let generation = b.generation;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let r = photo_profiles::preview(
                        &source,
                        &params,
                        digest,
                        &std::sync::atomic::AtomicBool::new(false),
                    )?;
                    Ok::<_, emulsion_io::IoError>(preview_bgra(&r))
                })
                .await;
            this.update(cx, |this, cx| {
                let b = &mut this.batch.profiles;
                b.busy = false;
                if b.generation == generation {
                    match result {
                        Ok((w, h, pixels)) => {
                            b.images.insert(digest, Arc::new(bgra_image(w, h, pixels)));
                            b.order.push_back(digest);
                            while b.order.len() > 32 {
                                if let Some(old) = b.order.pop_front() {
                                    b.images.remove(&old);
                                }
                            }
                        }
                        Err(e) => {
                            b.errors.insert(digest, e.to_string());
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
