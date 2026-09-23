//! The Batch tab: open a folder of pictures, tick the ones to treat, pick
//! a recipe, look at any of them large with the recipe applied, and export
//! them all — the same non-destructive pipeline the editor uses, run one
//! picture at a time off the UI thread.

mod recipe_previews;

use crate::theme::{self, MONO_FONT};
use crate::viewport::bgra_image;
use crate::widgets::{button, chip, label, mono};
use crate::workspace::Workspace;
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Editor, Node};
use emulsion_io::export::ExportOptions;
use emulsion_raster::composite::flatten;
use emulsion_raster::{Placement, Raster};
use emulsion_recipes::Recipe;
use emulsion_recipes::store;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::collections::{HashSet, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const THUMB: u32 = 300;
const THUMB_WORKERS: usize = 2;
const THUMB_CACHE: usize = 128;
const PREVIEW: u32 = 1100;

pub(crate) struct BatchItem {
    pub path: PathBuf,
    pub selected: bool,
    pub thumb: Option<Arc<RenderImage>>,
}

#[derive(Default)]
pub(crate) struct BatchState {
    pub folder: Option<PathBuf>,
    pub items: Vec<BatchItem>,
    /// Retain failed requests too, so redraws do not retry converters forever.
    thumbs_requested: HashSet<PathBuf>,
    thumbs_active: usize,
    thumbs_generation: u64,
    thumbs_visible: Range<usize>,
    thumbs_cached: VecDeque<usize>,
    thumbs_scroll: UniformListScrollHandle,
    /// The picture shown large.
    pub current: Option<usize>,
    /// Chosen recipe name, if any.
    pub recipe: Option<String>,
    /// The recipe library, shared so a frame does not clone it.
    pub(crate) recipes: Option<Arc<Vec<Recipe>>>,
    pub(crate) tag: Option<String>,
    recipe_browser: bool,
    recipe_previews: recipe_previews::RecipePreviews,
    tag_browser: bool,
    pub(crate) search: Option<(Entity<InputState>, Subscription)>,
    /// Large preview for (path, recipe).
    preview: Option<(PathBuf, Option<String>, Arc<RenderImage>)>,
    preview_loading: Option<(PathBuf, Option<String>)>,
    preview_failed: Option<(PathBuf, Option<String>)>,
    /// Invalidates renders for older recipe contents, even when names match.
    preview_generation: u64,
    /// "jpg" or "png".
    pub format: String,
    pub out_dir: Option<PathBuf>,
    /// Export progress: done, total.
    pub running: Option<(usize, usize)>,
    pub(crate) exporting: Option<PathBuf>,
    run_generation: u64,
    pub note: Option<(SharedString, bool)>,
}

impl BatchState {
    fn request_thumbs(&mut self) -> Vec<(usize, PathBuf)> {
        let mut todo = Vec::new();
        for index in self.thumbs_visible.clone() {
            if self.thumbs_active >= THUMB_WORKERS {
                break;
            }
            let Some(item) = self.items.get(index) else {
                break;
            };
            if item.thumb.is_none() && self.thumbs_requested.insert(item.path.clone()) {
                self.thumbs_active += 1;
                todo.push((index, item.path.clone()));
            }
        }
        todo
    }

    fn finish_thumb(
        &mut self,
        generation: u64,
        index: usize,
        rendered: Option<(u32, u32, Vec<u8>)>,
    ) {
        self.thumbs_active = self.thumbs_active.saturating_sub(1);
        if generation != self.thumbs_generation {
            return;
        }
        if let Some((w, h, bgra)) = rendered {
            self.items[index].thumb = Some(Arc::new(bgra_image(w, h, bgra)));
            self.thumbs_cached.push_back(index);
            while self.thumbs_cached.len() > THUMB_CACHE {
                let old = self.thumbs_cached.pop_front().unwrap();
                if self.thumbs_visible.contains(&old) {
                    self.thumbs_cached.push_back(old);
                    // An unusually tall viewport may need more than the normal cache.
                    if self
                        .thumbs_cached
                        .iter()
                        .all(|i| self.thumbs_visible.contains(i))
                    {
                        break;
                    }
                    continue;
                }
                let item = &mut self.items[old];
                item.thumb = None;
                self.thumbs_requested.remove(&item.path);
            }
        }
    }

    fn refresh_recipes(&mut self, dir: &Path) {
        let recipes: Vec<_> = store::list(dir)
            .into_iter()
            .map(|(recipe, _)| recipe)
            .collect();
        if let Some(name) = &self.recipe
            && !recipes.iter().any(|recipe| &recipe.name == name)
        {
            self.note = Some((
                format!("Recipe {name} is no longer available. Choose another recipe.").into(),
                false,
            ));
            self.recipe = None;
        }
        self.recipes = Some(Arc::new(recipes));
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.preview = None;
        self.preview_loading = None;
        self.preview_failed = None;
    }

    fn finish_preview(
        &mut self,
        generation: u64,
        key: (PathBuf, Option<String>),
        rendered: Option<(u32, u32, Vec<u8>)>,
    ) -> bool {
        if generation != self.preview_generation || self.preview_loading.as_ref() != Some(&key) {
            return false;
        }
        self.preview_loading = None;
        if self.recipe != key.1
            || self
                .current
                .and_then(|i| self.items.get(i))
                .map(|item| &item.path)
                != Some(&key.0)
        {
            return false;
        }
        if let Some((w, h, bgra)) = rendered {
            self.preview = Some((key.0, key.1, Arc::new(bgra_image(w, h, bgra))));
            self.preview_failed = None;
        } else {
            self.preview_failed = Some(key);
        }
        true
    }
}

/// Batch accepts pictures, including camera RAW and converter-backed images,
/// but not the page/document formats supported by the editor's general import.
fn is_batch_input(path: &Path) -> bool {
    if path.extension().is_some_and(|ext| {
        ["pdf", "ps", "eps", "ai"]
            .iter()
            .any(|document| ext.eq_ignore_ascii_case(document))
    }) {
        return false;
    }
    emulsion_io::is_openable(path)
}

/// Pictures in a folder, sorted by name.
fn list_folder(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && is_batch_input(p))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// A picture decoded small, as a raster.
fn small_raster(path: &Path, max: u32) -> Option<Raster> {
    let (w, h, rgba) = emulsion_io::thumb::thumbnail(path, max).ok()?;
    Some(Raster::from_srgba8(w, h, &rgba))
}

/// `recipe` applied over `source`, as BGRA bytes.
fn render_with(source: Arc<Raster>, recipe: Option<&Recipe>) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h) = (source.width(), source.height());
    let mut doc = Document::new(w, h);
    Command::AddNode {
        node: Box::new(Node::raster(0, "Photo", source, Placement::default())),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .ok()?;
    let mut ed = Editor::new(doc, None);
    if let Some(r) = recipe {
        let compiled = emulsion_recipes::compile_sized(r, w, h).ok()?;
        store::add_to(&mut ed, compiled, Slot::TOP).ok()?;
    }
    let flat = flatten(&ed.doc.composite_tree(), 0);
    let mut px = flat.to_srgba8();
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    Some((w, h, px))
}

/// Open a picture at full size, apply the recipe and write it out.
fn process_one(
    path: &Path,
    recipe: Option<&Recipe>,
    out_dir: &Path,
    ext: &str,
) -> Result<PathBuf, String> {
    let doc = emulsion_io::open(path).map_err(|e| format!("Could not open input: {e}"))?;
    let mut ed = Editor::new(doc, None);
    let (w, h) = (ed.doc.width, ed.doc.height);
    if let Some(r) = recipe {
        let compiled = emulsion_recipes::compile_sized(r, w, h)
            .map_err(|e| format!("Could not apply recipe {}: {e}", r.name))?;
        store::add_to(&mut ed, compiled, Slot::TOP)
            .map_err(|e| format!("Could not apply recipe {}: {e}", r.name))?;
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "picture".into());
    let suffix = recipe
        .map(|r| format!("-{}", slug(&r.name)))
        .unwrap_or_default();
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("Could not create output folder {}: {e}", out_dir.display()))?;
    let stage = BatchStage::new(out_dir, ext)
        .map_err(|e| format!("Could not write to {}: {e}", out_dir.display()))?;
    emulsion_io::export::export(&ed.doc, &stage.0, ExportOptions::for_doc(&ed.doc))
        .map_err(|e| format!("Could not encode {ext}: {e}"))?;
    publish_batch_file(&stage.0, out_dir, &format!("{stem}{suffix}"), ext)
        .map_err(|e| format!("Could not save output in {}: {e}", out_dir.display()))
}

/// Encode privately, then publish with an exclusive hard link. Unlike an
/// exists-check followed by rename, this never overwrites an existing target.
struct BatchStage(PathBuf);

impl BatchStage {
    fn new(dir: &Path, ext: &str) -> std::io::Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        loop {
            let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = dir.join(format!(
                ".emulsion-batch-{}-{serial}.{ext}",
                std::process::id()
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for BatchStage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn publish_batch_file(stage: &Path, dir: &Path, stem: &str, ext: &str) -> std::io::Result<PathBuf> {
    for serial in 0u64.. {
        let name = if serial == 0 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{serial}.{ext}")
        };
        let path = dir.join(name);
        match std::fs::hard_link(stage, &path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                // Removable FAT/exFAT volumes may not support hard links.
                // Exclusive creation still protects existing artwork there.
                let mut output = match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                {
                    Ok(file) => file,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e),
                };
                let result = std::fs::File::open(stage)
                    .and_then(|mut input| std::io::copy(&mut input, &mut output))
                    .and_then(|_| output.sync_all());
                if let Err(e) = result {
                    drop(output);
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
                return Ok(path);
            }
        }
    }
    unreachable!()
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

impl Workspace {
    pub(crate) fn refresh_batch_recipes(&mut self, cx: &mut Context<Self>) {
        self.batch.refresh_recipes(&crate::editor::recipes_dir());
        cx.notify();
    }

    pub fn pick_batch_folder(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose a folder of pictures".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let listed = cx
                .background_spawn({
                    let dir = dir.clone();
                    async move { list_folder(&dir) }
                })
                .await;
            this.update(cx, |this, cx| {
                this.load_batch(dir, listed, cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn load_batch(&mut self, dir: PathBuf, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let b = &mut self.batch;
        b.out_dir = Some(dir.join("emulsion-export"));
        b.folder = Some(dir);
        b.items = paths
            .into_iter()
            // `load_batch` is also called by Home, so preserve the supported
            // input invariant even when no folder scan happened first.
            .filter(|path| is_batch_input(path))
            .map(|path| BatchItem {
                path,
                selected: false,
                thumb: None,
            })
            .collect();
        b.thumbs_requested.clear();
        b.thumbs_generation = b.thumbs_generation.wrapping_add(1);
        b.thumbs_visible = 0..0;
        b.thumbs_cached.clear();
        b.thumbs_scroll = UniformListScrollHandle::new();
        b.current = None;
        b.preview = None;
        b.preview_loading = None;
        b.preview_failed = None;
        b.preview_generation = b.preview_generation.wrapping_add(1);
        b.note = None;
        if b.format.is_empty() {
            b.format = "jpg".into();
        }
        cx.notify();
    }

    fn batch_thumbs(&mut self, cx: &mut Context<Self>) {
        let generation = self.batch.thumbs_generation;
        for (index, path) in self.batch.request_thumbs() {
            cx.spawn(async move |this, cx| {
                let p = path.clone();
                let r = cx
                    .background_spawn(async move {
                        emulsion_io::thumb::batch_thumbnail(&p, THUMB).map(|(w, h, mut rgba)| {
                            for px in rgba.as_chunks_mut::<4>().0 {
                                px.swap(0, 2);
                            }
                            (w, h, rgba)
                        })
                    })
                    .await;
                this.update(cx, |this, cx| {
                    this.batch.finish_thumb(generation, index, r.ok());
                    // Refill only the latest viewport; old folders still occupy worker slots
                    // until their decoders return, but can never publish stale images.
                    if this.screen == crate::workspace::Screen::Batch {
                        this.batch_thumbs(cx);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    fn batch_recipes(&mut self) -> Arc<Vec<Recipe>> {
        self.batch
            .recipes
            .get_or_insert_with(|| {
                Arc::new(
                    store::list(&crate::editor::recipes_dir())
                        .into_iter()
                        .map(|(r, _)| r)
                        .collect(),
                )
            })
            .clone()
    }

    fn chosen_recipe(&mut self) -> Option<Recipe> {
        let name = self.batch.recipe.clone()?;
        self.batch_recipes()
            .iter()
            .find(|r| r.name == name)
            .cloned()
    }

    /// Render the current picture large with the chosen recipe, once per
    /// (picture, recipe).
    fn batch_preview(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.batch.current else {
            return;
        };
        let Some(path) = self.batch.items.get(i).map(|it| it.path.clone()) else {
            return;
        };
        let key = (path.clone(), self.batch.recipe.clone());
        if self
            .batch
            .preview
            .as_ref()
            .is_some_and(|(p, r, _)| (p, r) == (&key.0, &key.1))
            || self.batch.preview_loading.as_ref() == Some(&key)
            || self.batch.preview_failed.as_ref() == Some(&key)
        {
            return;
        }
        self.batch.preview_loading = Some(key.clone());
        self.batch.preview_failed = None;
        self.batch.preview = None;
        self.batch.preview_generation = self.batch.preview_generation.wrapping_add(1);
        let generation = self.batch.preview_generation;
        let recipe = self.chosen_recipe();
        cx.spawn(async move |this, cx| {
            let p = path.clone();
            let r = cx
                .background_spawn(async move {
                    let src = Arc::new(small_raster(&p, PREVIEW)?);
                    render_with(src, recipe.as_ref())
                })
                .await;
            this.update(cx, |this, cx| {
                if this.batch.finish_preview(generation, key, r) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn pick_batch_out_dir(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export to".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(dir) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| {
                    this.batch.out_dir = Some(dir);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Export every ticked picture with the chosen recipe.
    pub fn run_batch(&mut self, cx: &mut Context<Self>) {
        if self.batch.running.is_some() {
            return;
        }
        let paths: Vec<PathBuf> = self
            .batch
            .items
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.path.clone())
            .collect();
        let Some(out_dir) = self.batch.out_dir.clone() else {
            return;
        };
        if paths.is_empty() {
            self.batch.note = Some(("Tick at least one picture.".into(), true));
            cx.notify();
            return;
        }
        let recipe = self.chosen_recipe();
        let ext = batch_ext(&self.batch.format).to_string();
        let total = paths.len();
        self.batch.run_generation = self.batch.run_generation.wrapping_add(1);
        let generation = self.batch.run_generation;
        self.batch.running = Some((0, total));
        self.batch.exporting = paths.first().cloned();
        self.batch.note = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut failed = 0;
            let mut first_failure = None;
            for (i, path) in paths.into_iter().enumerate() {
                let go_on = this
                    .update(cx, |this, cx| {
                        if this.batch.run_generation != generation || this.batch.running.is_none() {
                            return false;
                        }
                        this.batch.exporting = Some(path.clone());
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !go_on {
                    return;
                }
                let input = path.display().to_string();
                let r = cx
                    .background_spawn({
                        let (recipe, out_dir, ext) = (recipe.clone(), out_dir.clone(), ext.clone());
                        async move { process_one(&path, recipe.as_ref(), &out_dir, &ext) }
                    })
                    .await;
                let go_on = this
                    .update(cx, |this, cx| {
                        if this.batch.run_generation != generation || this.batch.running.is_none() {
                            return false;
                        }
                        if let Err(e) = r {
                            failed += 1;
                            tracing::warn!(input = %input, error = %e, "Batch export failed");
                            let detail = format!("{input}: {e}");
                            first_failure.get_or_insert_with(|| detail.clone());
                            this.batch.note = Some((detail.into(), true));
                        }
                        this.batch.running = Some((i + 1, total));
                        cx.notify();
                        this.batch.running.is_some()
                    })
                    .unwrap_or(false);
                if !go_on {
                    return;
                }
            }
            this.update(cx, |this, cx| {
                if this.batch.run_generation != generation {
                    return;
                }
                this.batch.running = None;
                this.batch.exporting = None;
                if failed == 0 {
                    this.batch.note = Some((
                        format!("Exported {total} to {}", out_dir.display()).into(),
                        false,
                    ));
                } else {
                    this.batch.note = Some((
                        format!(
                            "Exported {} of {total}; {failed} failed. First error: {}",
                            total - failed,
                            first_failure.as_deref().unwrap_or("Unknown export error")
                        )
                        .into(),
                        true,
                    ));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn cancel_batch(&mut self, cx: &mut Context<Self>) {
        self.batch.run_generation = self.batch.run_generation.wrapping_add(1);
        self.batch.running = None;
        self.batch.exporting = None;
        self.batch.note = Some(("Export stopped.".into(), false));
        cx.notify();
    }

    fn batch_settings_panel(
        &mut self,
        recipes: &[Recipe],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.prepare_batch_recipe_previews(cx);
        let p = theme::palette(cx);
        if self.batch.recipe_browser && self.batch.search.is_none() {
            let input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Search recipes or tags…"));
            let subscription = cx.subscribe(&input, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            });
            self.batch.search = Some((input, subscription));
        }
        let chosen = self
            .batch
            .recipe
            .clone()
            .unwrap_or_else(|| "No recipe".into());
        let mut recipe = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(14.))
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(label("Recipe", &p))
                    .child(div().flex_1())
                    .when(self.batch.recipe.is_some(), |d| {
                        d.child(
                            chip("batch-recipe-clear", "Clear", false, &p)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.batch.recipe = None;
                                    cx.notify();
                                }))
                                .test_support(),
                        )
                    }),
            )
            .child(
                div()
                    .id("batch-recipe-toggle")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .p(px(9.))
                    .border_1()
                    .border_color(p.line)
                    .bg(p.soft_bg)
                    .cursor_pointer()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(12.))
                            .child(chosen),
                    )
                    .child(mono(
                        if self.batch.recipe_browser {
                            "▴"
                        } else {
                            "▾"
                        },
                        11.,
                        p.muted,
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.recipe_browser = !this.batch.recipe_browser;
                        this.batch.tag_browser = false;
                        cx.notify();
                    }))
                    .test_support(),
            );
        if let Some(chosen) = self
            .batch
            .recipe
            .as_ref()
            .and_then(|name| recipes.iter().find(|r| &r.name == name))
        {
            let limitations = chosen.limitations();
            if !limitations.is_empty() {
                recipe = recipe.child(
                    div()
                        .id("batch-recipe-limitations")
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(limitations.join(" · "))
                        .test_support(),
                );
            }
        }
        if self.batch.recipe_browser {
            let input = self
                .batch
                .search
                .as_ref()
                .expect("recipe search initialized")
                .0
                .clone();
            let query = input.read(cx).value().trim().to_lowercase();
            let tag = self.batch.tag.clone();
            let mut browser = div()
                .id("batch-recipe-browser")
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(Input::new(&input))
                .child(
                    chip(
                        "batch-filter-toggle",
                        format!("Category: {} ▾", tag.as_deref().unwrap_or("All")),
                        self.batch.tag_browser,
                        &p,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.tag_browser = !this.batch.tag_browser;
                        cx.notify();
                    }))
                    .test_support(),
                );
            if self.batch.tag_browser {
                let mut tags: Vec<_> = recipes.iter().flat_map(|r| r.tags.clone()).collect();
                tags.sort();
                tags.dedup();
                let mut choices = div().flex().flex_wrap().gap(px(5.)).child(
                    chip("batch-tag-all", "All categories", tag.is_none(), &p)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.batch.tag = None;
                            this.batch.tag_browser = false;
                            cx.notify();
                        }))
                        .test_support(),
                );
                for (i, name) in tags.into_iter().enumerate() {
                    let active = tag.as_ref() == Some(&name);
                    choices = choices.child(
                        chip(("batch-tag", i), name.clone(), active, &p)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.batch.tag = Some(name.clone());
                                this.batch.tag_browser = false;
                                cx.notify();
                            }))
                            .test_support(),
                    );
                }
                browser = browser.child(
                    div()
                        .id("batch-tag-list")
                        .max_h(px(120.))
                        .overflow_y_scroll()
                        .child(choices)
                        .test_support(),
                );
            }
            let filtered: Vec<_> = recipes
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    tag.as_ref().is_none_or(|tag| r.tags.contains(tag))
                        && (query.is_empty()
                            || r.name.to_lowercase().contains(&query)
                            || r.tags.iter().any(|tag| tag.to_lowercase().contains(&query)))
                })
                .collect();
            let indices: Vec<_> = filtered.iter().map(|(index, _)| *index).collect();
            let count = indices.len();
            let library = self.batch_recipes();
            let list_id: SharedString = format!("batch-recipe-rows:{tag:?}:{query}").into();
            let list = uniform_list(
                list_id,
                count.div_ceil(2),
                cx.processor(move |this, rows: Range<usize>, window, cx| {
                    this.batch.recipe_previews.visible =
                        indices[rows.start * 2..(rows.end * 2).min(indices.len())].to_vec();
                    // Measurement and viewport callbacks share the latest range.
                    cx.defer_in(window, |this, _, cx| this.load_batch_recipe_previews(cx));
                    rows.map(|row_index| {
                        let mut row = div().flex().gap_2().pt_2().h(px(114.));
                        for &index in
                            &indices[row_index * 2..((row_index + 1) * 2).min(indices.len())]
                        {
                            let item = &library[index];
                            let name = item.name.clone();
                            let on = this.batch.recipe.as_ref() == Some(&name);
                            let previews = &this.batch.recipe_previews;
                            let image = match previews.images.get(&index) {
                                Some(image) => div()
                                    .id(("batch-rc-image", index))
                                    .size_full()
                                    .child(
                                        img(ImageSource::Render(image.clone()))
                                            .object_fit(ObjectFit::Contain)
                                            .size_full(),
                                    )
                                    .test_support()
                                    .into_any_element(),
                                None => div()
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(mono(
                                        if previews.path.is_none() {
                                            "Select photo"
                                        } else if previews.failed_source
                                            || (!previews.busy
                                                && previews.attempted.contains(&index))
                                        {
                                            "Unavailable"
                                        } else {
                                            "Loading…"
                                        },
                                        9.,
                                        p.muted,
                                    ))
                                    .into_any_element(),
                            };
                            row = row.child(
                                div()
                                    .id(("batch-rc", index))
                                    .w(px(110.))
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .cursor_pointer()
                                    .child(
                                        div()
                                            .id(("batch-rc-preview", index))
                                            .w_full()
                                            .h(px(74.))
                                            .overflow_hidden()
                                            .border_2()
                                            .border_color(if on { p.accent } else { p.line })
                                            .bg(p.stage)
                                            .child(image)
                                            .test_support(),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .text_size(px(10.))
                                            .text_color(if on { p.accent } else { p.ink })
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(name.clone()),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.batch.recipe = Some(name.clone());
                                        this.batch.recipe_browser = false;
                                        cx.notify();
                                    }))
                                    .test_support(),
                            );
                        }
                        row
                    })
                    .collect()
                }),
            )
            .w_full()
            .h(px(260.));
            browser = browser
                .child(mono(format!("{count} recipes"), 9., p.muted))
                .when(self.batch.current.is_none(), |d| {
                    d.child(mono("Select a photo to preview recipes", 10., p.muted))
                })
                .child(
                    chip(
                        "batch-rc-none",
                        "No recipe",
                        self.batch.recipe.is_none(),
                        &p,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.recipe = None;
                        this.batch.recipe_browser = false;
                        cx.notify();
                    }))
                    .test_support(),
                )
                .child(div().id("batch-recipe-list").child(list).test_support())
                .when(count == 0, |d| {
                    d.child(mono("No matching recipes", 10., p.muted))
                });
            recipe = recipe.child(browser.test_support());
        }
        let mut format = div().flex().items_center().gap(px(6.));
        for (value, title, id) in [
            ("jpg", "JPEG", 3usize),
            ("png", "PNG", 13usize),
            ("webp", "WebP", 23usize),
            ("tif", "TIFF", 33usize),
        ] {
            let selected = batch_ext(&self.batch.format) == value;
            format = format.child(
                chip(("batch-fmt", id), title, selected, &p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.format = value.into();
                        cx.notify();
                    }))
                    .test_support(),
            );
        }
        let destination = self
            .batch
            .out_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Choose an output folder".into());
        let destination_tip = destination.clone();
        let export = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(14.))
            .child(label("Export settings", &p))
            .child(mono("Format", 10., p.muted))
            .child(format)
            .child(mono("Destination", 10., p.muted))
            .child(
                div()
                    .id("batch-destination")
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child(destination)
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(destination_tip.clone())
                            .build(window, cx)
                    }),
            )
            .child(
                chip("batch-out", "Choose folder…", false, &p)
                    .on_click(cx.listener(|this, _, _, cx| this.pick_batch_out_dir(cx)))
                    .test_support(),
            );
        div()
            .id("batch-settings")
            .w(px(264.))
            .flex_none()
            .min_h_0()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(p.line)
            .child(recipe)
            .child(export)
            .test_support()
            .into_any_element()
    }

    // ── View ────────────────────────────────────────────────────────────

    pub(crate) fn batch_screen(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = theme::palette(cx);
        self.batch_preview(cx);
        let recipes = self.batch_recipes();
        let selected = self.batch.items.iter().filter(|i| i.selected).count();
        let total = self.batch.items.len();

        // Keep the primary actions in one row; detailed choices live in the dock.
        let folder_label = self
            .batch
            .folder
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Choose a folder to get started".into());
        let mut bar = div()
            .id("batch-toolbar")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .py(px(10.))
            .border_b_1()
            .border_color(p.line)
            .child(
                button(
                    "batch-folder",
                    "Choose folder…",
                    self.batch.folder.is_none(),
                    &p,
                )
                .py(px(5.))
                .on_click(cx.listener(|this, _, _, cx| this.pick_batch_folder(cx))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(mono(folder_label, 10., p.muted)),
            )
            .child(mono(format!("{selected} / {total} selected"), 10., p.ink).whitespace_nowrap());
        bar = match self.batch.running {
            Some(_) => bar.child(
                chip("batch-stop", "Stop", false, &p)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_batch(cx)))
                    .test_support(),
            ),
            None => bar.child(
                button("batch-run", format!("Export {selected}"), selected > 0, &p)
                    .py(px(5.))
                    .test_support()
                    .on_click(cx.listener(|this, _, _, cx| this.run_batch(cx))),
            ),
        };
        let settings = self.batch_settings_panel(&recipes, window, cx);
        let photo_header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .p(px(12.))
            .border_b_1()
            .border_color(p.line)
            .child(label("Photos", &p))
            .child(div().flex_1())
            .child(
                chip("batch-all", "All", false, &p)
                    .on_click(cx.listener(|this, _, _, cx| {
                        for item in &mut this.batch.items {
                            item.selected = true;
                        }
                        cx.notify();
                    }))
                    .test_support(),
            )
            .child(
                chip("batch-none", "None", false, &p)
                    .on_click(cx.listener(|this, _, _, cx| {
                        for item in &mut this.batch.items {
                            item.selected = false;
                        }
                        cx.notify();
                    }))
                    .test_support(),
            );

        // Only build visible rows. Thumbnails are scheduled after layout so measuring
        // a row cannot start expensive work or notify during paint.
        let columns = if f32::from(window.viewport_size().width) < 1050. {
            1
        } else {
            2
        };
        let generation = self.batch.thumbs_generation;
        let grid: AnyElement = if self.batch.items.is_empty() {
            div()
                .p_6()
                .text_color(p.muted)
                .child("Choose a folder to see its pictures here.")
                .into_any_element()
        } else {
            uniform_list(
                "batch-grid-rows",
                self.batch.items.len().div_ceil(columns),
                cx.processor(move |this, rows: Range<usize>, window, cx| {
                    let visible =
                        rows.start * columns..(rows.end * columns).min(this.batch.items.len());
                    // Measurement runs before the real viewport callback. Coalesce by
                    // storing its latest range before any deferred scheduler runs.
                    this.batch.thumbs_visible = visible;
                    cx.defer_in(window, move |this, _, cx| {
                        if this.batch.thumbs_generation == generation {
                            this.batch_thumbs(cx);
                        }
                    });
                    rows.map(|row_index| {
                        let mut row = div().flex().gap_2().px_3().pt_2().h(px(127.));
                        let current = this.batch.current;
                        for i in row_index * columns
                            ..((row_index + 1) * columns).min(this.batch.items.len())
                        {
                            let item = &this.batch.items[i];
                            let is_cur = current == Some(i);
                            let name = item
                                .path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let image: AnyElement = match &item.thumb {
                                Some(t) => img(ImageSource::Render(t.clone()))
                                    .object_fit(ObjectFit::Cover)
                                    .size_full()
                                    .into_any_element(),
                                None => div().size_full().bg(p.stage).into_any_element(),
                            };
                            row = row.child(
                                div()
                                    .id(("batch-item", i))
                                    .w(px(132.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.batch.current = Some(i);
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .w(px(132.))
                                            .h(px(100.))
                                            .relative()
                                            .overflow_hidden()
                                            .border_2()
                                            .border_color(if is_cur { p.accent } else { p.line })
                                            .when(!item.selected, |d| d.opacity(0.45))
                                            .child(image)
                                            .child(
                                                div()
                                                    .id(("batch-tick", i))
                                                    .absolute()
                                                    .top(px(4.))
                                                    .left(px(4.))
                                                    .size(px(16.))
                                                    .border_1()
                                                    .border_color(gpui_kit::white())
                                                    .bg(if item.selected {
                                                        p.accent
                                                    } else {
                                                        gpui_kit::black().opacity(0.4)
                                                    })
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_color(p.accent_fg)
                                                    .text_size(px(10.))
                                                    .font_family(MONO_FONT)
                                                    .child(if item.selected { "✓" } else { "" })
                                                    .on_click(cx.listener(
                                                        move |this, _: &ClickEvent, _, cx| {
                                                            cx.stop_propagation();
                                                            if let Some(it) =
                                                                this.batch.items.get_mut(i)
                                                            {
                                                                it.selected = !it.selected;
                                                            }
                                                            cx.notify();
                                                        },
                                                    ))
                                                    .test_support(),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(px(132.))
                                            .text_size(px(10.))
                                            .text_color(p.muted)
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(name),
                                    )
                                    .test_support(),
                            );
                        }
                        row
                    })
                    .collect()
                }),
            )
            .track_scroll(&self.batch.thumbs_scroll)
            .size_full()
            .into_any_element()
        };

        // Large preview.
        let preview: AnyElement = match &self.batch.preview {
            Some((_, _, image)) => img(ImageSource::Render(image.clone()))
                .object_fit(ObjectFit::Contain)
                .size_full()
                .into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(mono(
                    if self.batch.preview_loading.is_some() {
                        "rendering…"
                    } else if self.batch.preview_failed.is_some() {
                        "Preview unavailable for this photo"
                    } else {
                        "Select a photo to preview its recipe"
                    },
                    10.,
                    p.muted,
                ))
                .into_any_element(),
        };
        let caption = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|it| {
                format!(
                    "{}{}",
                    it.path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    self.batch
                        .recipe
                        .as_ref()
                        .map(|r| format!(" · {r}"))
                        .unwrap_or_default()
                )
            })
            .unwrap_or_default();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(bar.test_support())
            .children(self.batch.running.map(|(done, count)| {
                let percent = if count == 0 {
                    0.
                } else {
                    100. * done as f32 / count as f32
                };
                let filename = self
                    .batch
                    .exporting
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                div()
                    .id("batch-export-progress")
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .id("batch-export-file")
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_sm()
                                    .child(format!("Exporting {filename}"))
                                    .test_support(),
                            )
                            .child(
                                div()
                                    .id("batch-export-count")
                                    .flex_none()
                                    .text_sm()
                                    .child(format!("{done} / {count} processed · {percent:.0}%"))
                                    .test_support(),
                            ),
                    )
                    .child(
                        Progress::new("batch-export-bar")
                            .accessibility_label("Batch export progress")
                            .value(percent)
                            .loading(done == 0),
                    )
                    .test_support()
            }))
            .children(self.batch.note.as_ref().map(|(message, error)| {
                div()
                    .flex_none()
                    .px(px(16.))
                    .py(px(7.))
                    .border_b_1()
                    .border_color(p.line)
                    .child(mono(
                        message.clone(),
                        10.,
                        if *error { p.accent } else { p.ink },
                    ))
            }))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(if f32::from(window.viewport_size().width) < 1050. {
                                172.
                            } else {
                                304.
                            }))
                            .flex_none()
                            .min_h_0()
                            .border_r_1()
                            .border_color(p.line)
                            .child(photo_header)
                            .child(
                                div()
                                    .id("batch-grid")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .child(grid)
                                    .test_support(),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .bg(p.stage)
                            .child(div().flex_1().min_h_0().p(px(12.)).child(preview))
                            .child(
                                div()
                                    .px(px(12.))
                                    .py(px(6.))
                                    .border_t_1()
                                    .border_color(p.line)
                                    .child(mono(caption, 10., p.muted)),
                            ),
                    )
                    .child(settings),
            )
    }
}

/// The output extension for a batch format setting; anything unknown is JPEG.
pub(crate) fn batch_ext(format: &str) -> &'static str {
    let f = format.trim_start_matches('.').to_ascii_lowercase();
    let f = match f.as_str() {
        "jpeg" => "jpg",
        "tiff" => "tif",
        other => other,
    };
    emulsion_io::export::ExportFormat::exportable_extensions()
        .into_iter()
        .find(|e| *e == f)
        .unwrap_or("jpg")
}

#[cfg(test)]
mod export_safety_tests {
    use super::{BatchStage, list_folder, publish_batch_file};

    fn thumbnail_state(count: usize) -> super::BatchState {
        super::BatchState {
            items: (0..count)
                .map(|i| super::BatchItem {
                    path: format!("photo-{i}.png").into(),
                    selected: false,
                    thumb: None,
                })
                .collect(),
            thumbs_visible: 0..count,
            ..Default::default()
        }
    }

    #[test]
    fn thumbnails_are_bounded_prioritize_viewport_and_retain_failures() {
        let mut batch = thumbnail_state(1000);
        batch.thumbs_visible = 400..410;
        let first = batch.request_thumbs();
        assert_eq!(
            first.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            vec![400, 401]
        );
        for _ in 0..50 {
            assert!(batch.request_thumbs().is_empty());
        }
        batch.finish_thumb(0, 400, None);
        batch.thumbs_visible = 900..910;
        assert_eq!(batch.request_thumbs()[0].0, 900);
        assert_eq!(batch.thumbs_active, super::THUMB_WORKERS);
        batch.finish_thumb(0, 401, None);
        batch.finish_thumb(0, 900, None);
        batch.thumbs_visible = 400..402;
        assert!(
            batch.request_thumbs().is_empty(),
            "failed files are not retried on redraw"
        );
        assert!(batch.items.iter().all(|item| !item.selected));
        assert!(batch.current.is_none() && batch.preview.is_none() && batch.running.is_none());
    }

    #[test]
    fn folder_changes_keep_worker_limit_and_reject_stale_thumbnails() {
        let mut batch = thumbnail_state(4);
        assert_eq!(batch.request_thumbs().len(), 2);
        batch.thumbs_generation += 1;
        batch.thumbs_requested.clear();
        assert!(batch.request_thumbs().is_empty());
        batch.finish_thumb(0, 0, Some((1, 1, vec![255; 4])));
        assert!(batch.items[0].thumb.is_none());
        assert_eq!(batch.request_thumbs().len(), 1);
        assert_eq!(batch.thumbs_active, 2);
        batch.finish_thumb(0, 1, None);
        batch.finish_thumb(1, 0, Some((1, 1, vec![255; 4])));
        assert!(batch.items[0].thumb.is_some());
    }

    #[test]
    fn scrolling_evicts_old_successful_thumbnails_but_can_reload_them() {
        let mut batch = thumbnail_state(super::THUMB_CACHE + 2);
        for index in 0..super::THUMB_CACHE + 2 {
            batch.thumbs_visible = index..index + 1;
            assert_eq!(batch.request_thumbs().len(), 1);
            batch.finish_thumb(0, index, Some((1, 1, vec![255; 4])));
        }
        assert_eq!(
            batch
                .items
                .iter()
                .filter(|item| item.thumb.is_some())
                .count(),
            super::THUMB_CACHE
        );
        batch.thumbs_visible = 0..1;
        assert_eq!(batch.request_thumbs()[0].0, 0);
    }

    #[test]
    fn failed_preview_is_remembered_after_loading_finishes() {
        let mut batch = thumbnail_state(1);
        batch.current = Some(0);
        let key = (batch.items[0].path.clone(), None);
        batch.preview_loading = Some(key.clone());
        assert!(batch.finish_preview(0, key.clone(), None));
        assert_eq!(batch.preview_failed, Some(key));
        assert!(batch.preview_loading.is_none());
    }

    #[test]
    fn folder_scan_keeps_supported_images_and_camera_raw_files_only() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "emulsion-batch-inputs-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        for name in [
            "portrait.PNG",
            "camera.CR3",
            "drawing.svg",
            "tax-return.pdf",
            "statement.PDF",
            "document.ps",
            "document.EPS",
            "document.ai",
            "notes.txt",
            "raw-sidecar.json",
            "no-extension",
        ] {
            std::fs::write(dir.join(name), b"fixture").unwrap();
        }
        std::fs::create_dir(dir.join("looks-like-a-photo.jpg")).unwrap();

        let listed = list_folder(&dir);
        let mut expected = vec![
            dir.join("portrait.PNG"),
            dir.join("camera.CR3"),
            dir.join("drawing.svg"),
        ];
        expected.sort();
        assert_eq!(listed, expected);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refreshing_recipe_catalog_loads_new_and_updated_workflows_and_rejects_old_preview() {
        use super::{BatchItem, BatchState};
        use emulsion_core::command::Slot;
        use emulsion_core::{Command, Document, Node};
        use emulsion_raster::Adjustment;
        use emulsion_recipes::{capture_adjustments, store};
        use std::sync::Arc;

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "emulsion-batch-refresh-{}-{unique}",
            std::process::id()
        ));
        let mut doc = Document::new(16, 16);
        let id = Command::AddNode {
            node: Box::new(Node::adjust(
                0,
                Adjustment::Exposure {
                    exposure: 0.25,
                    offset: 0.0,
                    gamma: 1.0,
                },
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        let mut original = capture_adjustments(&doc, id, "Existing workflow", &[]).unwrap();
        store::save_new(&dir, &original).unwrap();
        let path = dir.join("photo.png");
        let mut batch = BatchState {
            recipe: Some(original.name.clone()),
            items: vec![BatchItem {
                path: path.clone(),
                selected: true,
                thumb: None,
            }],
            current: Some(0),
            format: "png".into(),
            out_dir: Some(dir.join("exports")),
            ..Default::default()
        };
        batch.refresh_recipes(&dir);
        assert!(batch.recipes.as_ref().unwrap().contains(&original));
        let key = (path.clone(), Some(original.name.clone()));
        let old_generation = batch.preview_generation;
        batch.preview_loading = Some(key.clone());
        batch.preview = Some((
            path,
            key.1.clone(),
            Arc::new(super::bgra_image(1, 1, vec![0, 0, 0, 255])),
        ));
        original.workflow.as_mut().unwrap().stages[0].adjustment = Adjustment::Exposure {
            exposure: 1.25,
            offset: 0.0,
            gamma: 1.0,
        };
        store::update(&dir, &original.name, &original).unwrap();
        let mut added = original.clone();
        added.name = "New workflow".into();
        store::save_new(&dir, &added).unwrap();

        batch.refresh_recipes(&dir);
        let recipes = batch.recipes.as_ref().unwrap();
        assert!(
            recipes.contains(&original),
            "same-name workflow reloads changed stages"
        );
        assert!(recipes.contains(&added), "newly saved workflow appears");
        assert_eq!(batch.recipe.as_deref(), Some("Existing workflow"));
        assert!(batch.items[0].selected);
        assert_eq!(batch.current, Some(0));
        assert_eq!(batch.format, "png");
        assert_eq!(batch.out_dir, Some(dir.join("exports")));
        assert!(batch.preview.is_none() && batch.preview_loading.is_none());

        // Even an identical photo/name pair must reject the old recipe render.
        batch.preview_loading = Some(key.clone());
        let new_generation = batch.preview_generation;
        assert!(!batch.finish_preview(
            old_generation,
            key.clone(),
            Some((1, 1, vec![0, 0, 0, 255]))
        ));
        assert!(batch.preview.is_none());
        assert_eq!(batch.preview_loading.as_ref(), Some(&key));
        assert!(batch.finish_preview(new_generation, key.clone(), Some((1, 1, vec![255; 4]))));
        let current = batch.preview.as_ref().unwrap().2.clone();
        assert!(!batch.finish_preview(old_generation, key, Some((1, 1, vec![0, 0, 0, 255]))));
        assert!(Arc::ptr_eq(&current, &batch.preview.as_ref().unwrap().2));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saved_workflow_batch_export_matches_native_stages_and_preserves_existing_files() {
        use emulsion_core::command::Slot;
        use emulsion_core::{Command, Node};
        use emulsion_raster::{Adjustment, composite::flatten};
        use emulsion_recipes::{Recipe, capture_adjustments, store};

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "emulsion-workflow-batch-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let input = dir.join("photo.png");
        let pixels: Vec<u8> = (0..120)
            .flat_map(|i| [30 + i as u8, 90, 130, 255])
            .collect();
        let source = emulsion_io::export::png8(12, 10, &pixels).unwrap();
        std::fs::write(&input, &source).unwrap();
        let mut expected = emulsion_io::open(&input).unwrap();
        let group = Command::AddNode {
            node: Box::new(Node::group(0, "Original grade")),
            slot: Slot::TOP,
        }
        .apply(&mut expected)
        .unwrap()
        .unwrap();
        for adjustment in [
            Adjustment::Exposure {
                exposure: 0.6,
                offset: 0.02,
                gamma: 1.1,
            },
            Adjustment::HueSaturation {
                hue: 21.,
                saturation: -30.,
                lightness: 4.,
            },
        ] {
            let mut node = Node::adjust(0, adjustment);
            node.opacity = 0.7;
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::top_of(Some(group)),
            }
            .apply(&mut expected)
            .unwrap();
        }
        let recipe = capture_adjustments(&expected, group, "Exact grade", &[]).unwrap();
        let saved = store::save_new(&dir.join("recipes"), &recipe).unwrap();
        let recipe = Recipe::from_toml(&std::fs::read_to_string(saved).unwrap()).unwrap();
        let expected_pixels = flatten(&expected.composite_tree(), 0).to_srgba8();
        assert_ne!(expected_pixels, pixels);
        let existing = dir.join("photo-exact-grade.png");
        std::fs::write(&existing, b"existing artwork").unwrap();
        let first = super::process_one(&input, Some(&recipe), &dir, "png").unwrap();
        let first_bytes = std::fs::read(&first).unwrap();
        let second = super::process_one(&input, Some(&recipe), &dir, "png").unwrap();
        assert_ne!(first, second);
        assert_ne!(first, existing);
        assert_ne!(second, existing);
        assert_eq!(std::fs::read(&input).unwrap(), source);
        assert_eq!(std::fs::read(&existing).unwrap(), b"existing artwork");
        assert_eq!(std::fs::read(&first).unwrap(), first_bytes);
        for output in [first, second] {
            let actual = image::open(output).unwrap().to_rgba8();
            assert_eq!(actual.dimensions(), (12, 10));
            assert_eq!(actual.as_raw(), &expected_pixels);
        }
        assert!(std::fs::read_dir(&dir).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".emulsion-batch-")
        }));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn jpeg_png_and_raw_batch_exports_support_every_offered_format_with_a_recipe() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "emulsion-batch-formats-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let png = dir.join("photo.png");
        std::fs::write(
            &png,
            emulsion_io::export::png8(36, 24, &[80, 110, 150, 255].repeat(36 * 24)).unwrap(),
        )
        .unwrap();
        let jpeg = dir.join("photo.jpg");
        image::RgbImage::from_pixel(36, 24, image::Rgb([80, 110, 150]))
            .save(&jpeg)
            .unwrap();
        let raw = dir.join("camera.dng");
        crate::raw_test_fixture::write_dng(&raw);
        let recipe = emulsion_recipes::Recipe {
            name: "Warm grade".into(),
            exposure_compensation: "+1/3".into(),
            color: 2.0,
            shadow: 1.0,
            ..Default::default()
        };
        for input in [&jpeg, &png, &raw] {
            let original = std::fs::read(input).unwrap();
            let source = emulsion_io::open(input).unwrap();
            for ext in ["jpg", "png", "webp", "tif"] {
                let output = super::process_one(input, Some(&recipe), &dir.join("exports"), ext)
                    .unwrap_or_else(|error| panic!("{} -> {ext}: {error}", input.display()));
                let decoded = image::open(&output).unwrap();
                assert_eq!(
                    (decoded.width(), decoded.height()),
                    (source.width, source.height)
                );
                assert!(decoded.to_rgba8().pixels().all(|pixel| pixel[3] == 255));
                if input == &raw && ["png", "tif"].contains(&ext) {
                    assert_eq!(decoded.color(), image::ColorType::Rgba16);
                }
            }
            assert_eq!(std::fs::read(input).unwrap(), original);
        }
        assert!(
            std::fs::read_dir(dir.join("exports"))
                .unwrap()
                .all(|entry| {
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with('.')
                })
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicate_stems_and_existing_outputs_are_never_overwritten() {
        let dir =
            std::env::temp_dir().join(format!("emulsion-batch-collisions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("photo.png");
        std::fs::write(&original, b"original artwork").unwrap();
        let stage = BatchStage::new(&dir, "png").unwrap();
        std::fs::write(&stage.0, b"new output").unwrap();
        let a = publish_batch_file(&stage.0, &dir, "photo", "png").unwrap();
        let b = publish_batch_file(&stage.0, &dir, "photo", "png").unwrap();
        assert_ne!(a, b);
        assert_ne!(a, original);
        assert_eq!(std::fs::read(&original).unwrap(), b"original artwork");
        assert_eq!(std::fs::read(a).unwrap(), b"new output");
        assert_eq!(std::fs::read(b).unwrap(), b"new output");
        drop(stage);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
