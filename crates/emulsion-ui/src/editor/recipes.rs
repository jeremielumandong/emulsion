//! The Recipes panel: every recipe as a card with a live thumbnail of this
//! picture. Clicking a card previews it on the canvas; only Apply commits
//! (one history step), Cancel puts the picture back, and a new preview
//! replaces the old one instead of stacking. Recipes import from the
//! clipboard, a file (.recipe.toml, Lightroom .xmp, Fujifilm .FP1, text)
//! or a web page — one recipe or a whole index of them.

use super::*;
use emulsion_core::command::Slot;
use emulsion_raster::composite::{flatten, level_size};
use emulsion_recipes::store::{self, Origin};
use emulsion_recipes::{Recipe, import};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use std::collections::HashMap;
use std::path::PathBuf;

/// Thumbnail long side in pixels.
const THUMB: u32 = 150;

#[derive(Default)]
pub(crate) struct RecipeState {
    pub open: bool,
    /// Tag filter, if any.
    pub tag: Option<String>,
    cache: Option<Vec<(Recipe, Origin)>>,
    /// The recipe shown on the canvas but not yet committed.
    pub preview: Option<Preview>,
    /// Small renders of the picture through each recipe, by name, for the
    /// document revision they were made from.
    thumbs: HashMap<String, Arc<RenderImage>>,
    thumbs_rev: Option<u64>,
    thumbs_busy: bool,
    /// The picture at thumbnail size, shared by all renders.
    source: Option<(u64, Arc<Raster>)>,
    /// URL field and its subscription.
    url: Option<(Entity<InputState>, Subscription)>,
    /// Bulk import progress: done, total.
    pub importing: Option<(usize, usize)>,
}

pub(crate) struct Preview {
    pub name: String,
    pub group: NodeId,
}

/// Where saved recipes live.
pub fn recipes_dir() -> PathBuf {
    emulsion_io::recent::data_dir().join("recipes")
}

/// Render `recipe` over `source` at its own size.
fn render_thumb(source: &Arc<Raster>, recipe: &Recipe) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h) = (source.width(), source.height());
    let mut doc = Document::new(w, h);
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Photo",
            source.clone(),
            Placement::default(),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .ok()?;
    let mut ed = emulsion_core::Editor::new(doc, None);
    let compiled = emulsion_recipes::compile_sized(recipe, w, h).ok()?;
    store::add_to(&mut ed, compiled, Slot::TOP).ok()?;
    let flat = flatten(&ed.doc.composite_tree(), 0);
    let mut px = flat.to_srgba8();
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    Some((w, h, px))
}

impl EditorView {
    pub fn toggle_recipes(&mut self, cx: &mut Context<Self>) {
        if self.recipes.open {
            self.cancel_preview(cx);
        }
        self.recipes.open = !self.recipes.open;
        if self.recipes.open {
            self.select_sidebar(SidebarTab::Recipes, cx);
            self.recipes.cache = Some(store::list(&recipes_dir()));
        } else {
            self.select_sidebar(SidebarTab::Properties, cx);
        }
        cx.notify();
    }

    fn recipe_list(&mut self) -> Vec<(Recipe, Origin)> {
        self.recipes
            .cache
            .get_or_insert_with(|| store::list(&recipes_dir()))
            .clone()
    }

    fn reload_recipes(&mut self) {
        self.recipes.cache = Some(store::list(&recipes_dir()));
        self.recipes.thumbs.clear();
        self.recipes.thumbs_rev = None;
    }

    // ── Preview, apply, cancel ──────────────────────────────────────────

    /// Show `recipe` on the canvas without committing. Any earlier preview
    /// is taken down first, so recipes never stack while being reviewed.
    pub fn preview_recipe(&mut self, recipe: &Recipe, cx: &mut Context<Self>) {
        if self.editor.in_transaction() && self.recipes.preview.is_none() {
            self.set_status(
                "Finish the current edit before previewing a recipe.",
                false,
                cx,
            );
            return;
        }
        if self
            .recipes
            .preview
            .as_ref()
            .is_some_and(|p| p.name == recipe.name)
        {
            self.cancel_preview(cx);
            return;
        }
        if self.recipes.preview.is_some() {
            self.editor.cancel();
            self.recipes.preview = None;
        }
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let compiled = match emulsion_recipes::compile_sized(recipe, w, h) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(format!("Could not preview {}: {e}", recipe.name), true, cx);
                return;
            }
        };
        let slot = self.insertion_slot();
        self.editor.begin(format!("Recipe · {}", recipe.name));
        match store::add_to(&mut self.editor, compiled, slot) {
            Ok(gid) => {
                self.recipes.preview = Some(Preview {
                    name: recipe.name.clone(),
                    group: gid,
                });
                self.set_status(
                    format!(
                        "Previewing {} — Apply to keep it, or pick another.",
                        recipe.name
                    ),
                    false,
                    cx,
                );
            }
            Err(e) => {
                self.editor.cancel();
                self.set_status(e.to_string(), true, cx);
            }
        }
        self.after_change(cx);
    }

    /// Commit the preview as one history step.
    pub fn apply_preview(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.recipes.preview.take() else {
            return;
        };
        self.editor.end();
        self.selected = Some(p.group);
        self.set_status(
            format!("Applied {}. Preview another to stack it on top.", p.name),
            false,
            cx,
        );
        self.after_change(cx);
    }

    /// Put the picture back as it was before the preview.
    pub fn cancel_preview(&mut self, cx: &mut Context<Self>) {
        if self.recipes.preview.take().is_some() {
            self.editor.cancel();
            self.status = None;
            self.after_change(cx);
        }
    }

    /// Apply a recipe above the selected node at once, as one undo step
    /// (the assistant's path, and tests').
    pub fn apply_recipe(&mut self, recipe: &Recipe, cx: &mut Context<Self>) {
        if self.editor.in_transaction() && self.recipes.preview.is_none() {
            self.set_status(
                "Finish the current edit before applying a recipe.",
                false,
                cx,
            );
            return;
        }
        self.cancel_preview(cx);
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let compiled = match emulsion_recipes::compile_sized(recipe, w, h) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(format!("Could not apply {}: {e}", recipe.name), true, cx);
                return;
            }
        };
        let slot = self.insertion_slot();
        match store::add_to(&mut self.editor, compiled, slot) {
            Ok(gid) => {
                self.selected = Some(gid);
                self.set_status(
                    format!(
                        "Applied {}. Open the group to tune each stage.",
                        recipe.name
                    ),
                    false,
                    cx,
                );
                self.after_change(cx);
            }
            Err(e) => self.set_status(e.to_string(), true, cx),
        }
    }

    // ── Import ──────────────────────────────────────────────────────────

    fn save_imported(
        &mut self,
        recipe: Recipe,
        unknown: &[String],
        cx: &mut Context<Self>,
    ) -> bool {
        if let Err(e) = recipe.validate() {
            self.set_status(format!("That does not read as a recipe: {e}"), true, cx);
            return false;
        }
        match store::save(&recipes_dir(), &recipe) {
            Ok(_) => {
                self.reload_recipes();
                let note = if unknown.is_empty() {
                    String::new()
                } else {
                    format!(" (skipped: {})", unknown.join(", "))
                };
                self.set_status(format!("Saved recipe {}{note}", recipe.name), false, cx);
                cx.notify();
                true
            }
            Err(e) => {
                self.set_status(format!("Could not save: {e}"), true, cx);
                false
            }
        }
    }

    /// Read a pasted recipe block or TOML from the clipboard and save it.
    pub fn import_recipe_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            self.set_status("Copy a recipe's settings first, then import.", false, cx);
            return;
        };
        let trimmed = text.trim();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            self.import_recipe_from_url(trimmed.to_string(), cx);
            return;
        }
        let (recipe, unknown) = match Recipe::from_toml(&text) {
            Ok(r) => (r, Vec::new()),
            Err(_) if trimmed.starts_with('<') && trimmed.contains("crs:") => {
                import::from_xmp(&text)
            }
            Err(_) if trimmed.starts_with('<') => import::from_fp1(&text),
            Err(_) => import::parse_text(&text),
        };
        self.save_imported(recipe, &unknown, cx);
    }

    /// Pick .recipe.toml, .xmp, .FP1 or text files and save each.
    /// Write the saved recipes (or, with none saved, every recipe shown)
    /// as one shareable bundle file.
    pub fn export_recipe_bundle(&mut self, cx: &mut Context<Self>) {
        let all = self.recipe_list();
        let mut recipes: Vec<Recipe> = all
            .iter()
            .filter(|(_, o)| matches!(o, Origin::Saved(_)))
            .map(|(r, _)| r.clone())
            .collect();
        if recipes.is_empty() {
            recipes = all.iter().map(|(r, _)| r.clone()).collect();
        }
        if recipes.is_empty() {
            self.set_status("No recipes to export.", false, cx);
            return;
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let rx = cx.prompt_for_new_path(&home, Some("my-recipes.toml"));
        let count = recipes.len();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else {
                return;
            };
            path.set_extension("toml");
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Recipes".into());
            let text = emulsion_recipes::bundle::Bundle::new(name, recipes).to_toml();
            let result = std::fs::write(&path, text);
            this.update(cx, |this, cx| match result {
                Ok(()) => this.set_status(
                    format!("Exported {count} recipes to {}", path.display()),
                    false,
                    cx,
                ),
                Err(e) => this.set_status(format!("Export failed: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn import_recipe_files(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import recipes".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let parsed = cx
                .background_spawn(async move {
                    paths
                        .into_iter()
                        .flat_map(|p| match import::from_file_many(&p) {
                            Ok(v) => v.into_iter().map(Ok).collect::<Vec<_>>(),
                            Err(e) => vec![Err(e)],
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                let mut saved = 0;
                for r in parsed {
                    match r {
                        Ok((recipe, unknown)) => {
                            if this.save_imported(recipe, &unknown, cx) {
                                saved += 1;
                            }
                        }
                        Err(e) => this.set_status(e, true, cx),
                    }
                }
                if saved > 1 {
                    this.set_status(format!("Imported {saved} recipes."), false, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Fetch a recipe page, or an index page of recipes, into the library.
    pub fn import_recipe_from_url(&mut self, url: String, cx: &mut Context<Self>) {
        if self.recipes.importing.is_some() {
            return;
        }
        self.recipes.importing = Some((0, 1));
        self.set_status(format!("Fetching {url}…"), false, cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let page = cx
                .background_spawn({
                    let url = url.clone();
                    async move { import::fetch(&url) }
                })
                .await;
            let html = match page {
                Ok(h) => h,
                Err(e) => {
                    this.update(cx, |this, cx| {
                        this.recipes.importing = None;
                        this.set_status(e, true, cx);
                    })
                    .ok();
                    return;
                }
            };
            let (single, _) = import::from_html(&html, &url);
            let links = import::recipe_links(&html, &url);
            // A page that reads as one recipe is one recipe; otherwise
            // follow every recipe link it lists.
            if single.validate().is_ok()
                && single.film_simulation != Recipe::default().film_simulation
                || links.len() < 2
            {
                let (recipe, unknown) = import::from_html(&html, &url);
                this.update(cx, |this, cx| {
                    this.recipes.importing = None;
                    this.save_imported(recipe, &unknown, cx);
                })
                .ok();
                return;
            }
            let total = links.len();
            this.update(cx, |this, cx| {
                this.recipes.importing = Some((0, total));
                this.set_status(format!("Importing {total} recipes from {url}…"), false, cx);
            })
            .ok();
            let mut saved = 0;
            for (i, link) in links.into_iter().enumerate() {
                let fetched = cx
                    .background_spawn({
                        let link = link.clone();
                        async move { import::fetch(&link).map(|h| import::from_html(&h, &link)) }
                    })
                    .await;
                let keep_going = this
                    .update(cx, |this, cx| {
                        this.recipes.importing = Some((i + 1, total));
                        if let Ok((recipe, _)) = fetched
                            && recipe.validate().is_ok()
                            && store::save(&recipes_dir(), &recipe).is_ok()
                        {
                            saved += 1;
                        }
                        cx.notify();
                        this.recipes.open
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
            this.update(cx, |this, cx| {
                this.recipes.importing = None;
                this.reload_recipes();
                this.set_status(format!("Imported {saved} of {total} recipes."), false, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn delete_recipe(&mut self, name: &str, cx: &mut Context<Self>) {
        if store::delete(&recipes_dir(), name) {
            self.reload_recipes();
        } else {
            self.set_status("Could not delete that recipe.", true, cx);
        }
        cx.notify();
    }

    // ── Thumbnails ──────────────────────────────────────────────────────

    /// Make sure thumbnails exist for the listed recipes at the current
    /// picture; renders happen one at a time off the UI thread.
    fn ensure_thumbs(&mut self, names: Vec<(String, Recipe)>, cx: &mut Context<Self>) {
        if self.recipes.thumbs_busy || self.recipes.preview.is_some() {
            return;
        }
        let rev = self.editor.revision;
        if self.recipes.thumbs_rev != Some(rev) {
            self.recipes.thumbs.clear();
            self.recipes.thumbs_rev = Some(rev);
            self.recipes.source = None;
        }
        let missing: Vec<(String, Recipe)> = names
            .into_iter()
            .filter(|(n, _)| !self.recipes.thumbs.contains_key(n))
            .collect();
        if missing.is_empty() {
            return;
        }
        self.recipes.thumbs_busy = true;
        let source = self.recipes.source.clone();
        let tree = self.tree.clone();
        cx.spawn(async move |this, cx| {
            let source = match source {
                Some((r, s)) if r == rev => s,
                _ => {
                    let s = cx
                        .background_spawn(async move {
                            let mut level = 0;
                            while {
                                let (w, h) = level_size(tree.width, tree.height, level);
                                w.max(h) > THUMB * 2 && level < 16
                            } {
                                level += 1;
                            }
                            let small = flatten(&tree, level);
                            let s = THUMB as f64 / small.width().max(small.height()) as f64;
                            let (w, h) = (
                                ((small.width() as f64 * s.min(1.0)).round() as u32).max(1),
                                ((small.height() as f64 * s.min(1.0)).round() as u32).max(1),
                            );
                            let img = image::RgbaImage::from_raw(
                                small.width(),
                                small.height(),
                                small.to_srgba8(),
                            )
                            .expect("sized");
                            let t = image::imageops::thumbnail(&img, w, h).into_raw();
                            Arc::new(Raster::from_srgba8(w, h, &t))
                        })
                        .await;
                    let ok = this
                        .update(cx, |this, _| {
                            this.recipes.source = Some((rev, s.clone()));
                        })
                        .is_ok();
                    if !ok {
                        return;
                    }
                    s
                }
            };
            for (name, recipe) in missing {
                let src = source.clone();
                let rendered = cx
                    .background_spawn(async move { render_thumb(&src, &recipe) })
                    .await;
                let more = this.update(cx, |this, cx| {
                    if this.recipes.thumbs_rev != Some(rev) {
                        return false;
                    }
                    if let Some((w, h, bgra)) = rendered {
                        this.recipes
                            .thumbs
                            .insert(name, Arc::new(viewport::bgra_image(w, h, bgra)));
                    }
                    cx.notify();
                    true
                });
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
            this.update(cx, |this, cx| {
                this.recipes.thumbs_busy = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn ensure_url_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recipes.url.is_some() {
            return;
        }
        let state = cx.new(|cx| {
            InputState::new(window, cx).placeholder("paste a recipe page or index URL, Enter")
        });
        let sub = cx.subscribe_in(&state, window, |this, st, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                let url = st.read(cx).value().trim().to_string();
                if url.starts_with("http") {
                    st.update(cx, |s, cx| s.set_value("", window, cx));
                    this.import_recipe_from_url(url, cx);
                }
            }
        });
        self.recipes.url = Some((state, sub));
    }

    // ── View ────────────────────────────────────────────────────────────

    pub(crate) fn recipes_view(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.recipes.open {
            return None;
        }
        self.ensure_url_field(window, cx);
        let all = self.recipe_list();
        let mut tags: Vec<String> = all.iter().flat_map(|(r, _)| r.tags.clone()).collect();
        tags.sort();
        tags.dedup();
        let tag = self.recipes.tag.clone();
        let shown: Vec<(Recipe, Origin)> = all
            .iter()
            .filter(|(r, _)| tag.as_ref().is_none_or(|t| r.tags.contains(t)))
            .cloned()
            .collect();
        self.ensure_thumbs(
            shown
                .iter()
                .map(|(r, _)| (r.name.clone(), r.clone()))
                .collect(),
            cx,
        );

        let mut header = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(5.))
            .child(label("Recipes", p))
            .child(div().flex_1())
            .child(
                chip("rc-all", "all", tag.is_none(), p).on_click(cx.listener(|this, _, _, cx| {
                    this.recipes.tag = None;
                    cx.notify();
                })),
            );
        for (i, t) in tags.iter().enumerate() {
            let on = tag.as_deref() == Some(t);
            let t2 = t.clone();
            header = header.child(chip(("rc-tag", i), t.clone(), on, p).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.recipes.tag = Some(t2.clone());
                    cx.notify();
                },
            )));
        }

        let previewing = self.recipes.preview.as_ref().map(|p| p.name.clone());
        let mut grid = div().flex().flex_wrap().gap(px(6.));
        let card_w = px(80.);
        for (i, (r, origin)) in shown.iter().enumerate() {
            let on = previewing.as_deref() == Some(r.name.as_str());
            let recipe = r.clone();
            let thumb: AnyElement = match self.recipes.thumbs.get(&r.name) {
                Some(image) => img(ImageSource::Render(image.clone()))
                    .object_fit(ObjectFit::Cover)
                    .size_full()
                    .into_any_element(),
                None => div()
                    .size_full()
                    .bg(p.stage)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(mono("…", 10., p.muted))
                    .into_any_element(),
            };
            let mut card = div()
                .id(("rc-card", i))
                .w(card_w)
                .flex()
                .flex_col()
                .gap(px(3.))
                .cursor_pointer()
                .child(
                    div()
                        .w(card_w)
                        .h(px(60.))
                        .overflow_hidden()
                        .border_2()
                        .border_color(if on { p.accent } else { p.line })
                        .child(thumb),
                )
                .child(
                    div()
                        .w(card_w)
                        .text_size(px(10.))
                        .text_color(if on { p.accent } else { p.ink })
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(r.name.clone()),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.preview_recipe(&recipe, cx)));
            if let Origin::Saved(_) = origin {
                let name = r.name.clone();
                card = card.child(
                    chip(("rc-del", i), "remove", false, p).on_click(cx.listener(
                        move |this, e: &ClickEvent, _, cx| {
                            let _ = e;
                            this.delete_recipe(&name, cx);
                        },
                    )),
                );
            }
            grid = grid.child(card);
        }

        let mut actions = div().flex().flex_wrap().items_center().gap(px(6.));
        if let Some(pv) = &self.recipes.preview {
            actions = actions
                .child(
                    button("rc-apply", format!("Apply {}", pv.name), true, p)
                        .py(px(4.))
                        .on_click(cx.listener(|this, _, _, cx| this.apply_preview(cx))),
                )
                .child(
                    button("rc-cancel", "Cancel", false, p)
                        .py(px(4.))
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_preview(cx))),
                );
        } else {
            actions = actions.child(mono(
                "click a card to preview it on the canvas",
                9.5,
                p.muted,
            ));
        }

        let mut import_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(
                chip("rc-import", "from clipboard", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.import_recipe_from_clipboard(cx))),
            )
            .child(
                chip("rc-files", "from files…", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.import_recipe_files(cx))),
            )
            .child(
                chip("rc-bundle", "export bundle…", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.export_recipe_bundle(cx))),
            );
        if let Some((done, total)) = self.recipes.importing {
            import_row = import_row.child(mono(format!("importing {done}/{total}"), 9.5, p.accent));
        }
        let url_field = self.recipes.url.as_ref().map(|(st, _)| {
            div()
                .w_full()
                .border_1()
                .border_color(p.line)
                .child(Input::new(st).appearance(false).bordered(false))
        });

        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .px(px(15.))
                .py(px(10.))
                .border_b_1()
                .border_color(p.line)
                .bg(p.panel)
                .child(header)
                .child(actions)
                .child(grid)
                .child(import_row)
                .children(url_field)
                .child(mono(
                    "Fuji X Weekly / Ross's pages, .recipe.toml, Lightroom .xmp, Fujifilm .FP1",
                    9.,
                    p.muted,
                ))
                .into_any_element(),
        )
    }
}
