//! The Recipes panel: every recipe as a card with a live thumbnail of this
//! picture. Clicking a card previews it on the canvas; only Apply commits
//! (one history step), Cancel puts the picture back, and a new preview
//! replaces the old one instead of stacking. Recipes import from the
//! clipboard, a file (.recipe.toml, Lightroom .xmp, Fujifilm .FP1, text)
//! or a web page — one recipe or a whole index of them. The shipped
//! community library is browsed one collection at a time (Classic Chrome,
//! Velvia, Black and white…) so the grid stays small; the Emulsion
//! collection holds the built-in sets and everything saved.

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
    /// Library collection shown; `None` is the Emulsion collection
    /// (built-in sets plus saved recipes).
    pub collection: Option<String>,
    /// Tag filter within the collection, if any.
    pub tag: Option<String>,
    /// The library, shared so a frame does not clone every recipe.
    cache: Option<Arc<Vec<(Recipe, Origin)>>>,
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
    pub(crate) capture: Option<RecipeCapture>,
    saving: bool,
}

pub(crate) struct RecipeCapture {
    pub(crate) source: NodeId,
    pub(crate) revision: u64,
    pub(crate) name: Entity<InputState>,
    pub(crate) tags: Entity<InputState>,
    pub(crate) notes: Entity<InputState>,
    /// Display order is the layer stack's top-to-bottom order.
    pub(crate) stages: Vec<(NodeId, String, bool)>,
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
    fn recipe_capture_busy(&self) -> bool {
        self.assistant.running
            || self.editor.in_transaction()
            || self.drag.is_some()
            || self.warp.is_some()
    }

    pub(crate) fn begin_recipe_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recipe_capture_busy() || self.recipes.saving {
            self.set_status(
                "Finish the current edit or recipe preview before saving a recipe.",
                false,
                cx,
            );
            return;
        }
        let Some(source) = self.selected else {
            self.set_status(
                "Select an adjustment layer or adjustment group to save.",
                false,
                cx,
            );
            return;
        };
        let Some(node) = self.editor.doc.node(source) else {
            return;
        };
        let name = node
            .name
            .strip_prefix("Recipe · ")
            .unwrap_or(&node.name)
            .to_string();
        if let Err(error) =
            emulsion_recipes::capture_adjustments(&self.editor.doc, source, &name, &[])
        {
            self.set_status(error.to_string(), true, cx);
            return;
        }
        let ids = if node.is_group() {
            self.editor
                .doc
                .children(Some(source))
                .into_iter()
                .rev()
                .collect()
        } else {
            vec![source]
        };
        let stages = ids
            .into_iter()
            .map(|id| {
                let node = self.editor.doc.node(id).expect("captured node");
                (id, node.name.clone(), true)
            })
            .collect();
        let name = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Recipe name");
            state.set_value(name, window, cx);
            state
        });
        let tags =
            cx.new(|cx| InputState::new(window, cx).placeholder("Tags, separated by commas"));
        let notes = cx.new(|cx| InputState::new(window, cx).placeholder("Notes (optional)"));
        self.recipes.capture = Some(RecipeCapture {
            source,
            revision: self.editor.revision,
            name,
            tags,
            notes,
            stages,
        });
        self.status = None;
        cx.notify();
    }

    pub(crate) fn captured_recipe(
        &self,
        cx: &Context<Self>,
    ) -> Result<Recipe, emulsion_recipes::RecipeError> {
        let invalid = |message: &str| emulsion_recipes::RecipeError::Invalid(message.into());
        let draft = self
            .recipes
            .capture
            .as_ref()
            .ok_or_else(|| invalid("Open Save edits as recipe first."))?;
        if self.recipe_capture_busy() {
            return Err(invalid("Finish the current edit before saving a recipe."));
        }
        if draft.revision != self.editor.revision || self.selected != Some(draft.source) {
            return Err(invalid(
                "The artwork or selected layer changed. Reopen Save edits as recipe to capture the current edits.",
            ));
        }
        let excluded: Vec<_> = draft
            .stages
            .iter()
            .filter(|(_, _, included)| !included)
            .map(|(id, _, _)| *id)
            .collect();
        let mut recipe = emulsion_recipes::capture_adjustments(
            &self.editor.doc,
            draft.source,
            draft.name.read(cx).value().trim(),
            &excluded,
        )?;
        recipe.tags = draft
            .tags
            .read(cx)
            .value()
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(str::to_string)
            .collect();
        recipe.tags.sort();
        recipe.tags.dedup();
        recipe.notes = draft.notes.read(cx).value().trim().to_string();
        recipe.validate()?;
        Ok(recipe)
    }

    fn save_captured_recipe(&mut self, overwrite: bool, cx: &mut Context<Self>) {
        if self.recipes.saving {
            return;
        }
        let recipe = match self.captured_recipe(cx) {
            Ok(recipe) => recipe,
            Err(error) => {
                self.set_status(error.to_string(), true, cx);
                return;
            }
        };
        let name = recipe.name.clone();
        let dir = recipes_dir();
        self.recipes.saving = true;
        self.set_status("Saving recipe…", false, cx);
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move {
                if overwrite { store::update(&dir, &recipe.name, &recipe) }
                else { store::save_new(&dir, &recipe) }
            }).await;
            let _ = this.update(cx, |this, cx| {
                this.recipes.saving = false;
                match result {
                    Ok(_) => {
                        this.recipes.capture = None;
                        this.reload_recipes();
                        this.recipes.tag = None;
                        this.set_status(format!("Saved {name}. Available in Recipes and Batch; your document is unchanged."), false, cx);
                    }
                    Err(error) => this.set_status(error.to_string(), true, cx),
                }
            });
        }).detach();
    }

    fn recipe_capture_view(&self, p: &Palette, cx: &Context<Self>) -> Option<AnyElement> {
        let draft = self.recipes.capture.as_ref()?;
        if self.recipes.saving {
            return Some(mono("Saving recipe…", 11., p.muted).into_any_element());
        }
        let mut stages = div()
            .id("rc-capture-stages")
            .max_h(px(220.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(3.));
        for (index, (_, name, included)) in draft.stages.iter().enumerate() {
            stages = stages.child(
                chip(
                    ("rc-capture-stage", index),
                    format!("{} {}", if *included { "✓" } else { "□" }, name),
                    *included,
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(draft) = &mut this.recipes.capture
                        && let Some(stage) = draft.stages.get_mut(index)
                    {
                        stage.2 = !stage.2;
                    }
                    cx.notify();
                })),
            );
        }
        Some(div().id("rc-capture-form").test_support().flex().flex_col().gap(px(6.)).p(px(8.)).border_1().border_color(p.line)
            .child(label("Save current edits", p))
            .child(mono("Choose the adjustments to reuse. Exposure and white balance can be left out for other lighting.", 10., p.muted))
            .child(Input::new(&draft.name))
            .child(Input::new(&draft.tags))
            .child(Input::new(&draft.notes))
            .child(stages)
            .child(div().flex().flex_wrap().gap(px(5.))
                .child(button("rc-save-new", "Save new", true, p).on_click(cx.listener(|this, _, _, cx| this.save_captured_recipe(false, cx))))
                .child(button("rc-update", "Update existing", false, p).on_click(cx.listener(|this, _, _, cx| this.save_captured_recipe(true, cx))))
                .child(chip("rc-capture-cancel", "Cancel", false, p).on_click(cx.listener(|this, _, _, cx| { this.recipes.capture = None; cx.notify(); }))))
            .child(mono("Update existing replaces your saved recipe with the same name. Image pixels, masks and RAW settings are not captured.", 9.5, p.muted))
            .into_any_element())
    }

    /// Show one library collection, or the Emulsion collection for `None`.
    /// The tag filter belongs to a collection, so it clears on a change.
    pub(crate) fn select_collection(&mut self, collection: Option<String>) {
        if self.recipes.collection != collection {
            self.recipes.tag = None;
        }
        self.recipes.collection = collection;
    }

    pub fn toggle_recipes(&mut self, cx: &mut Context<Self>) {
        if self.recipes.open {
            self.cancel_preview(cx);
        }
        self.recipes.open = !self.recipes.open;
        if self.recipes.open {
            self.select_sidebar(SidebarTab::Recipes, cx);
        } else {
            self.select_sidebar(SidebarTab::Properties, cx);
        }
        cx.notify();
    }

    fn recipe_list(&mut self) -> Arc<Vec<(Recipe, Origin)>> {
        self.recipes
            .cache
            .get_or_insert_with(|| Arc::new(store::list(&recipes_dir())))
            .clone()
    }

    pub(super) fn reload_recipes(&mut self) {
        self.recipes.cache = Some(Arc::new(store::list(&recipes_dir())));
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
            self.cancel_preview(cx);
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
        self.set_layer_selection(vec![p.group], Some(p.group));
        let limitations = self
            .recipe_list()
            .iter()
            .find(|(recipe, _)| recipe.name == p.name)
            .map(|(recipe, _)| recipe.limitations().join(" "))
            .unwrap_or_default();
        self.set_status(
            format!(
                "Applied {}. Preview another to stack it on top. {limitations}",
                p.name
            ),
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
                self.set_layer_selection(vec![gid], Some(gid));
                self.set_status(
                    format!(
                        "Applied {}. Open the group to tune each stage. {}",
                        recipe.name,
                        recipe.limitations().join(" ")
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
        let parsed = if trimmed.starts_with('<') && trimmed.contains("crs:") {
            Ok(import::from_xmp(&text))
        } else if trimmed.starts_with('<') {
            Ok(import::from_fp1(&text))
        } else {
            import::from_text(&text)
        };
        let (recipe, unknown) = match parsed {
            Ok(value) => value,
            Err(error) => {
                self.set_status(format!("Could not import recipe: {error}"), true, cx);
                return;
            }
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
            let output = path.clone();
            let result = cx
                .background_spawn(async move {
                    let bundle = emulsion_recipes::bundle::Bundle::new(name, recipes)
                        .portable()
                        .map_err(|e| e.to_string())?;
                    let text = bundle.to_toml();
                    if text.is_empty() {
                        return Err("Could not serialize recipe bundle".to_string());
                    }
                    std::fs::write(&output, text).map_err(|e| e.to_string())
                })
                .await;
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
        let collection = self.recipes.collection.clone();
        let in_collection: Vec<&(Recipe, Origin)> = all
            .iter()
            .filter(|(_, o)| match &collection {
                None => !matches!(o, Origin::Library(_)),
                Some(c) => o.collection() == c,
            })
            .collect();
        let mut tags: Vec<String> = in_collection
            .iter()
            .flat_map(|(r, _)| r.tags.clone())
            .collect();
        tags.sort();
        tags.dedup();
        let tag = self.recipes.tag.clone();
        let shown: Vec<(Recipe, Origin)> = in_collection
            .iter()
            .filter(|(r, _)| tag.as_ref().is_none_or(|t| r.tags.contains(t)))
            .map(|entry| (*entry).clone())
            .collect();
        let collection_notes = collection.as_ref().and_then(|c| {
            emulsion_recipes::library::collections()
                .iter()
                .find(|l| &l.name == c)
                .map(|l| format!("{} · {} recipes", l.notes, l.recipes.len()))
        });
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
            .child(
                chip("rc-capture", "Save edits as recipe…", false, p).on_click(
                    cx.listener(|this, _, window, cx| this.begin_recipe_capture(window, cx)),
                ),
            )
            .child(div().flex_1())
            .child(
                chip(
                    "rc-collection-emulsion",
                    store::EMULSION_COLLECTION,
                    collection.is_none(),
                    p,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.select_collection(None);
                    cx.notify();
                }))
                .test_support(),
            );
        for (i, c) in emulsion_recipes::library::collections().iter().enumerate() {
            let on = collection.as_deref() == Some(c.name.as_str());
            let name = c.name.clone();
            header = header.child(
                chip(("rc-collection", i), c.name.clone(), on, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_collection(Some(name.clone()));
                        cx.notify();
                    }))
                    .test_support(),
            );
        }
        let mut tag_row = div().flex().flex_wrap().items_center().gap(px(5.)).child(
            chip("rc-all", "all", tag.is_none(), p).on_click(cx.listener(|this, _, _, cx| {
                this.recipes.tag = None;
                cx.notify();
            })),
        );
        for (i, t) in tags.iter().enumerate() {
            let on = tag.as_deref() == Some(t);
            let t2 = t.clone();
            tag_row = tag_row.child(chip(("rc-tag", i), t.clone(), on, p).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.recipes.tag = Some(t2.clone());
                    cx.notify();
                },
            )));
        }

        let previewing = self.recipes.preview.as_ref().map(|p| p.name.clone());
        let limitations = previewing
            .as_ref()
            .and_then(|name| all.iter().find(|(r, _)| &r.name == name))
            .map(|(recipe, _)| recipe.limitations())
            .unwrap_or_default();
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
                .on_click(cx.listener(move |this, _, _, cx| this.preview_recipe(&recipe, cx)))
                .test_support();
            if let Origin::Saved(_) = origin {
                let name = r.name.clone();
                card = card.child(
                    chip(("rc-del", i), "remove", false, p)
                        .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                            let _ = e;
                            this.delete_recipe(&name, cx);
                        }))
                        .test_support(),
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
                .children(self.recipe_capture_view(p, cx))
                .child(header)
                .children(collection_notes.map(|notes| mono(notes, 9.5, p.muted)))
                .child(tag_row)
                .child(actions)
                .when(!limitations.is_empty(), |view| {
                    view.child(mono(
                        format!("Saved but not rendered: {}", limitations.join(", ")),
                        10.,
                        p.accent,
                    ))
                })
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
