//! The Batch tab: open a folder of pictures, tick the ones to treat, pick
//! a recipe, look at any of them large with the recipe applied, and export
//! them all — the same non-destructive pipeline the editor uses, run one
//! picture at a time off the UI thread.

mod advanced;
mod classic;
mod culling;
mod develop;
mod enhance;
mod hdr;
mod layout;
mod library;
mod local_edits;
mod mcp;
pub(crate) mod preview;
mod printing;
mod profiles;
mod recipe_previews;
mod rotation;

use crate::theme;
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
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};
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

/// GPUI's RenderImage stores BGRA; exports and MCP previews retain RGBA.
fn preview_bgra(raster: &Raster) -> (u32, u32, Vec<u8>) {
    let mut pixels = raster.to_srgba8();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    (raster.width(), raster.height(), pixels)
}

type FileStamp = Option<(u64, Option<std::time::SystemTime>)>;
type ThumbStamp = (FileStamp, FileStamp);

fn thumb_stamp(path: &Path) -> ThumbStamp {
    let stamp = |path: &Path| {
        std::fs::metadata(path)
            .ok()
            .map(|m| (m.len(), m.modified().ok()))
    };
    let sidecar = emulsion_io::photo_develop::supported(path)
        .then(|| emulsion_io::raw_settings::sidecar_path(path).ok())
        .flatten();
    (stamp(path), sidecar.as_deref().and_then(stamp))
}

pub(crate) struct BatchItem {
    pub path: PathBuf,
    pub selected: bool,
    pub thumb: Option<Arc<RenderImage>>,
}

#[derive(Default)]
pub(crate) struct BatchState {
    library: library::LibraryUi,
    profiles: profiles::Browser,
    hdr_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    mcp_busy: bool,
    pub(crate) assistant_host: Option<Entity<crate::editor::EditorView>>,
    assistant_observer: Option<Subscription>,
    develop: develop::Develop,
    settings_open: bool,
    output_settings: emulsion_io::photo_export::OutputSettings,
    pub folder: Option<PathBuf>,
    pub items: Vec<BatchItem>,
    /// Retain failed requests too, so redraws do not retry converters forever.
    thumbs_requested: HashSet<PathBuf>,
    thumbs_failed: std::collections::HashMap<PathBuf, String>,
    thumbs_revisions: std::collections::HashMap<PathBuf, u64>,
    thumbs_stamps: std::collections::HashMap<PathBuf, ThumbStamp>,
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
    navigation: preview::Navigation,
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
    fn invalidate_thumb(&mut self, path: &Path) {
        self.recipe_previews.invalidate_source(path);
        let revision = self.thumbs_revisions.entry(path.to_path_buf()).or_default();
        *revision = revision.wrapping_add(1);
        self.thumbs_requested.remove(path);
        self.thumbs_failed.remove(path);
        self.thumbs_stamps.remove(path);
        for (i, item) in self.items.iter_mut().enumerate() {
            if item.path == path {
                item.thumb = None;
                self.thumbs_cached.retain(|cached| *cached != i);
            }
        }
    }

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
        revision: u64,
        rendered: Result<(u32, u32, Vec<u8>), String>,
    ) {
        self.thumbs_active = self.thumbs_active.saturating_sub(1);
        if generation != self.thumbs_generation
            || self.items.get(index).is_none_or(|item| {
                self.thumbs_revisions.get(&item.path).copied().unwrap_or(0) != revision
            })
        {
            return;
        }
        if let Ok((w, h, bgra)) = rendered {
            self.thumbs_failed.remove(&self.items[index].path);
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
                self.thumbs_stamps.remove(&item.path);
            }
        } else if let Err(error) = rendered {
            self.thumbs_failed
                .insert(self.items[index].path.clone(), error);
        }
    }

    fn retry_thumb(&mut self, index: usize) {
        let Some(item) = self.items.get(index) else {
            return;
        };
        if self.thumbs_failed.remove(&item.path).is_some() {
            self.thumbs_requested.remove(&item.path);
            if self
                .preview_failed
                .as_ref()
                .is_some_and(|(path, _)| path == &item.path)
            {
                self.preview_failed = None;
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
    if emulsion_io::diagram_import::is_diagram(path) || emulsion_io::template_pack::is_pack(path) {
        return false;
    }
    if path.extension().is_some_and(|ext| {
        ["pdf", "ps", "eps", "ai", "emu", "drawio"]
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
#[cfg(test)]
fn process_one(
    path: &Path,
    recipe: Option<&Recipe>,
    out_dir: &Path,
    ext: &str,
) -> Result<PathBuf, String> {
    process_one_with(path, recipe, out_dir, ext, &Default::default())
}

fn process_one_with(
    path: &Path,
    recipe: Option<&Recipe>,
    out_dir: &Path,
    ext: &str,
    settings: &emulsion_io::photo_export::OutputSettings,
) -> Result<PathBuf, String> {
    let (doc, working) = emulsion_io::photo_develop::open_saved_working(path)
        .map_err(|e| format!("Could not open input: {e}"))?;
    if working != emulsion_io::photo_color::Space::Srgb && recipe.is_some() {
        return Err("Wide-gamut RAW export requires Develop presets; remove the additional Photo recipe or select sRGB working space.".into());
    }
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
    let mut output = settings
        .prepare_in_space(&ed.doc, working)
        .map_err(|e| e.to_string())?;
    // open_saved already developed these verified pixels. Avoid a second full RAW decode.
    output.raw = None;
    let metadata =
        emulsion_io::photo_metadata::build(path, settings.metadata).map_err(|e| e.to_string())?;
    if working != emulsion_io::photo_color::Space::Srgb
        || settings.color_space != emulsion_io::photo_color::Space::Srgb
    {
        emulsion_io::photo_color::export(
            &flatten(&output.composite_tree(), 0),
            working,
            settings.color_space,
            &stage.0,
            output.source_depth,
            settings.jpeg_quality,
            metadata.as_deref(),
        )
        .map_err(|e| format!("Could not encode {ext}: {e}"))?;
    } else {
        emulsion_io::export::export_with_exif(
            &output,
            &stage.0,
            ExportOptions {
                depth: output.source_depth,
                jpeg_quality: settings.jpeg_quality,
            },
            metadata.as_deref(),
        )
        .map_err(|e| format!("Could not encode {ext}: {e}"))?;
    }
    let output = publish_batch_file(&stage.0, out_dir, &format!("{stem}{suffix}"), ext)
        .map_err(|e| format!("Could not save output in {}: {e}", out_dir.display()))?;
    if let Some(destination) = &settings.publish {
        emulsion_io::photo_publish::publish(&output, &format!("{stem}{suffix}"), destination)
            .map_err(|e| format!("Saved {} locally. {e}", output.display()))?;
    }
    Ok(output)
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
        self.batch.develop.refresh_saved();
        // Re-entering Library keeps unchanged previews. Check source/sidecar
        // metadata off the UI thread, and invalidate only files that changed.
        let stamps = self.batch.thumbs_stamps.clone();
        let generation = self.batch.thumbs_generation;
        cx.spawn(async move |this, cx| {
            let changed = cx
                .background_spawn(async move {
                    stamps
                        .into_iter()
                        .filter_map(|(path, old)| (thumb_stamp(&path) != old).then_some(path))
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                if this.batch.thumbs_generation == generation {
                    for path in changed {
                        this.batch.invalidate_thumb(&path);
                    }
                    this.batch_thumbs(cx);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        self.refresh_imported_photo_library(cx);
        cx.notify();
    }

    pub fn pick_batch_folder(&mut self, cx: &mut Context<Self>) {
        let deduplicate = self.batch.library.deduplicate;
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
            let folder = dir.clone();
            let listed = cx
                .background_spawn(async move { list_folder(&folder) })
                .await;
            if !deduplicate {
                let folder = dir.clone();
                let paths = listed.clone();
                this.update(cx, |this, cx| this.load_batch(folder, paths, cx))
                    .ok();
            }
            let result = cx
                .background_spawn(async move {
                    emulsion_io::creative_library::update(
                        &emulsion_io::creative_library::root(),
                        |c| emulsion_io::photo_catalog::import(c, &listed, deduplicate),
                    )
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((catalog, listed)) => {
                    if catalog.revision >= this.batch.library.catalog.revision {
                        this.batch.library.catalog = catalog;
                    }
                    this.batch.library.loaded = true;
                    if deduplicate {
                        this.load_batch(dir, listed, cx);
                    } else {
                        cx.notify();
                    }
                }
                Err(e) => {
                    this.batch.note = Some((e.to_string().into(), true));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn load_batch(&mut self, dir: PathBuf, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let b = &mut self.batch;
        let mut cached: std::collections::HashMap<_, _> = b
            .items
            .iter()
            .filter_map(|item| {
                item.thumb
                    .as_ref()
                    .map(|thumb| (item.path.clone(), thumb.clone()))
            })
            .collect();
        b.out_dir = Some(dir.join("emulsion-export"));
        b.folder = Some(dir);
        b.library.source_paths = Some(paths.clone());
        b.items = paths
            .into_iter()
            // `load_batch` is also called by Home, so preserve the supported
            // input invariant even when no folder scan happened first.
            .filter(|path| is_batch_input(path))
            .map(|path| {
                let path = path.canonicalize().unwrap_or(path);
                BatchItem {
                    thumb: cached.remove(&path),
                    path,
                    selected: false,
                }
            })
            .collect();
        b.thumbs_requested = b
            .items
            .iter()
            .filter(|i| i.thumb.is_some())
            .map(|i| i.path.clone())
            .collect();
        b.thumbs_failed.clear();
        b.thumbs_revisions.clear();
        b.thumbs_stamps
            .retain(|path, _| b.thumbs_requested.contains(path));
        b.thumbs_generation = b.thumbs_generation.wrapping_add(1);
        // Prime a bounded first pair as soon as import completes. The virtual
        // list replaces this range after layout; thumbnail loading must not
        // depend on a measurement callback or on selecting the first photo.
        b.thumbs_visible = 0..b.items.len().min(THUMB_WORKERS);
        b.thumbs_cached = b
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.thumb.is_some())
            .map(|(i, _)| i)
            .collect();
        b.thumbs_scroll = UniformListScrollHandle::new();
        b.current = None;
        b.develop.loupe = false;
        b.develop.compare = false;
        b.develop.anchor = None;
        b.preview = None;
        b.preview_loading = None;
        b.preview_failed = None;
        b.preview_generation = b.preview_generation.wrapping_add(1);
        b.note = None;
        if b.format.is_empty() {
            b.format = "jpg".into();
        }
        self.batch_thumbs(cx);
        cx.notify();
    }

    fn batch_thumbs(&mut self, cx: &mut Context<Self>) {
        let generation = self.batch.thumbs_generation;
        for (index, path) in self.batch.request_thumbs() {
            let revision = self.batch.thumbs_revisions.get(&path).copied().unwrap_or(0);
            cx.spawn(async move |this, cx| {
                let p = path.clone();
                let r = cx
                    .background_spawn(async move {
                        let stamp = thumb_stamp(&p);
                        let result = if !emulsion_io::raw::is_raw(&p)
                            && emulsion_io::photo_develop::supported(&p)
                            && emulsion_io::raw_settings::sidecar_path(&p).is_ok_and(|s| s.exists())
                        {
                            emulsion_io::photo_develop::PhotoSource::load(&p).and_then(|s| {
                                let params = emulsion_io::raw_settings::adjacent_settings(
                                    &p,
                                    &s.source_sha256,
                                )?;
                                let raster = s.develop_with(&params)?;
                                let image = image::RgbaImage::from_raw(
                                    raster.width(),
                                    raster.height(),
                                    raster.to_srgba8(),
                                )
                                .ok_or_else(|| {
                                    emulsion_io::IoError::Manifest("Invalid photo preview".into())
                                })?;
                                let small = image::DynamicImage::ImageRgba8(image)
                                    .thumbnail(THUMB, THUMB)
                                    .into_rgba8();
                                Ok((small.width(), small.height(), small.into_raw()))
                            })
                        } else if emulsion_io::raw::is_raw(&p) {
                            emulsion_io::thumb::thumbnail(&p, THUMB)
                        } else {
                            emulsion_io::thumb::batch_thumbnail(&p, THUMB)
                        }
                        .map(|(w, h, mut rgba)| {
                            for px in rgba.as_chunks_mut::<4>().0 {
                                px.swap(0, 2);
                            }
                            (w, h, rgba)
                        });
                        (stamp, result)
                    })
                    .await;
                this.update(cx, |this, cx| {
                    let (stamp, result) = r;
                    if generation == this.batch.thumbs_generation
                        && revision == this.batch.thumbs_revisions.get(&path).copied().unwrap_or(0)
                        && result.is_ok()
                    {
                        this.batch.thumbs_stamps.insert(path.clone(), stamp);
                    }
                    this.batch.finish_thumb(
                        generation,
                        index,
                        revision,
                        result.map_err(|error| error.to_string()),
                    );
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
        if emulsion_io::photo_develop::supported(&path) {
            self.library_raw_preview(path, cx);
            return;
        }
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
        if self.batch.develop.dirty() || self.batch.develop.saving {
            self.batch.note = Some(("Save RAW edits in Develop before exporting.".into(), true));
            cx.notify();
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
        let output_settings = self.batch.output_settings.clone();
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
                        {
                            let settings = output_settings.clone();
                            async move {
                                process_one_with(&path, recipe.as_ref(), &out_dir, &ext, &settings)
                            }
                        }
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
        let develop = if self.batch.develop.profiles_open && self.batch.develop.module_develop {
            self.library_profile_browser(window, cx)
        } else {
            self.library_develop_panel(cx)
        };
        if self.batch.develop.module_develop {
            return div()
                .id("library-settings-panel")
                .test_support()
                .w_full()
                .h_full()
                .flex()
                .flex_col()
                .min_h_0()
                .bg(classic::palette(cx).panel)
                .child(self.library_histogram_panel(cx))
                .child(self.library_editing_toolstrip(cx))
                .child(
                    div()
                        .id("library-adjustment-scroll")
                        .test_support()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(develop),
                )
                .child(self.library_develop_footer(cx))
                .into_any_element();
        }
        self.prepare_batch_recipe_previews(cx);
        let p = classic::palette(cx);
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
            )
            .child(self.library_output_controls(cx));
        div()
            .id("batch-settings")
            .w_full()
            .h_full()
            .flex_none()
            .min_h_0()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(p.line)
            .when(!self.batch.recipe_browser, |d| {
                d.child(self.library_histogram_panel(cx))
            })
            .child(develop)
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
        let p = classic::palette(cx);
        let narrow = window.viewport_size().width < window.rem_size() * 64.;
        if self.batch.develop.culling_mode == 0 {
            self.batch_preview(cx);
        }
        let overview = !self.batch.develop.loupe;
        let library_controls = self.library_controls(window, cx);
        let library_controls = if self.batch.develop.module_develop {
            self.library_develop_left(cx)
        } else {
            library_controls
        };
        let library_focus = self.batch.library.focus.clone().unwrap();
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
            .flex_wrap()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(8.))
            .py(px(4.))
            .border_b_1()
            .border_color(p.line)
            .child(
                button(
                    "batch-folder",
                    "Import folder…",
                    self.batch.folder.is_none(),
                    &p,
                )
                .py(px(5.))
                .on_click(cx.listener(|this, _, _, cx| this.pick_batch_folder(cx))),
            )
            .child(
                Button::new("library-assistant")
                    .label("Ask Library · F1")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| this.open_assistant(window, cx))),
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
            .child(
                Button::new("library-print-selected")
                    .label("Print selected…")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| this.library_print(window, cx))),
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
        if narrow {
            bar = bar.child(
                Button::new("library-settings-toggle")
                    .label("Develop / Export")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.settings_open = !this.batch.settings_open;
                        cx.notify();
                    })),
            );
        }
        let settings = if (!narrow || self.batch.settings_open) && !self.batch.develop.panels_hidden
        {
            Some(self.batch_settings_panel(&recipes, window, cx))
        } else {
            None
        };
        let settings = settings.map(|settings| {
            if narrow {
                deferred(
                    anchored()
                        .position(point(
                            (window.viewport_size().width - window.rem_size() * 18.75 - px(8.))
                                .max(px(8.)),
                            px(100.),
                        ))
                        .snap_to_window()
                        .child(
                            div()
                                .id("library-settings-overlay")
                                .test_support()
                                .occlude()
                                .shadow_lg()
                                .bg(p.panel)
                                .h((window.viewport_size().height - px(120.)).max(px(180.)))
                                .flex()
                                .flex_col()
                                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                    this.batch.settings_open = false;
                                    cx.notify();
                                }))
                                .child(
                                    Button::new("library-settings-close")
                                        .label("Close settings")
                                        .small()
                                        .ghost()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.batch.settings_open = false;
                                            cx.notify();
                                        })),
                                )
                                .child(div().flex().flex_1().min_h_0().child(settings)),
                        ),
                )
                .into_any_element()
            } else {
                settings
            }
        });
        if !overview || self.batch.develop.list {
            bar = bar.child(
                Button::new("library-grid-view")
                    .label("Grid view")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.develop.module_develop = false;
                        this.batch.develop.loupe = false;
                        this.batch.develop.list = false;
                        this.batch.develop.compare = false;
                        cx.notify();
                    })),
            );
        }
        let navigation = self.library_module_picker(cx);
        let selection_controls = self.library_selection_controls(window, cx);
        let grid_tools = self.library_grid_tools(cx);
        let photo_header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(p.line)
            .child(label(format!("Photos · {}", self.batch.items.len()), &p))
            .child(
                Button::new("library-loupe-view")
                    .label("Develop")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.batch.current.is_none() && !this.batch.items.is_empty() {
                            this.library_select(0, false, false, cx);
                        }
                        this.batch.develop.module_develop = true;
                        this.batch.develop.loupe = true;
                        this.invalidate_library_preview();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("library-open-photo")
                    .label("Open in Photo")
                    .small()
                    .ghost()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.library_open_photo(window, cx)),
                    ),
            )
            .child(
                Button::new("library-list-view")
                    .label("List")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.develop.module_develop = false;
                        this.batch.develop.list = !this.batch.develop.list;
                        this.batch.develop.loupe = false;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("library-compare-view")
                    .label("Compare")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this
                            .batch
                            .current
                            .and_then(|i| this.batch.items.get(i))
                            .is_some_and(|i| emulsion_io::photo_develop::supported(&i.path))
                        {
                            this.batch.develop.compare = !this.batch.develop.compare;
                            this.batch.develop.module_develop = true;
                            this.batch.develop.loupe = true;
                            this.batch.develop.before = false;
                            this.invalidate_library_preview();
                            cx.notify();
                        }
                    })),
            )
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
        let list_mode = self.batch.develop.list;
        let columns = if list_mode {
            1
        } else if overview {
            let available = f32::from(window.viewport_size().width)
                - f32::from(window.rem_size()) * 13.75
                - if narrow {
                    0.
                } else {
                    f32::from(window.rem_size()) * 18.75
                };
            ((available - 24.) / 164.).floor().clamp(1., 12.) as usize
        } else if f32::from(window.viewport_size().width) < 1050. {
            1
        } else {
            2
        };
        let generation = self.batch.thumbs_generation;
        let grid: AnyElement = if self.batch.items.is_empty() {
            div()
                .p_6()
                .text_color(p.muted)
                .child(if self.batch.folder.is_some() {
                    "No photos match this collection or its filters."
                } else {
                    "Import a folder to see your photos here."
                })
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
                        let mut row = div().flex().gap_1().px_1().pt_1().h(px(if list_mode {
                            70.
                        } else {
                            151.
                        }));
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
                            let developed = this
                                .batch
                                .preview
                                .as_ref()
                                .filter(|(path, _, _)| path == &item.path)
                                .map(|(_, _, image)| image);
                            let image: AnyElement = match developed.or(item.thumb.as_ref()) {
                                Some(t) => img(ImageSource::Render(t.clone()))
                                    .id(("batch-thumbnail", i))
                                    .object_fit(ObjectFit::Contain)
                                    .size_full()
                                    .test_support()
                                    .into_any_element(),
                                None => div()
                                    .size_full()
                                    .bg(p.stage)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        if let Some(error) =
                                            this.batch.thumbs_failed.get(&item.path)
                                        {
                                            Button::new(("batch-thumb-retry", i))
                                                .label(if list_mode {
                                                    "Retry"
                                                } else {
                                                    "Retry preview"
                                                })
                                                .accessibility_label(format!(
                                                    "Retry preview for {name}"
                                                ))
                                                .small()
                                                .ghost()
                                                .tooltip(error.clone())
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    this.batch.retry_thumb(i);
                                                    this.batch_thumbs(cx);
                                                    cx.notify();
                                                }))
                                                .into_any_element()
                                        } else {
                                            mono("Loading…", 10., p.muted).into_any_element()
                                        },
                                    )
                                    .into_any_element(),
                            };
                            row = row.child(
                                div()
                                    .id(("batch-item", i))
                                    .w(px(if list_mode { 600. } else { 156. }))
                                    .flex()
                                    .when(!list_mode, |d| d.flex_col())
                                    .when(list_mode, |d| d.items_center())
                                    .gap(px(3.))
                                    .bg(if is_cur || item.selected {
                                        p.soft_bg
                                    } else {
                                        p.panel
                                    })
                                    .rounded(px(5.))
                                    .border_1()
                                    .border_color(p.line)
                                    .cursor_pointer()
                                    .on_click(cx.listener(
                                        move |this, e: &ClickEvent, _window, cx| {
                                            if e.click_count() == 2 {
                                                this.library_select(i, false, false, cx);
                                                this.batch.develop.module_develop = true;
                                                this.batch.develop.loupe = true;
                                                cx.notify();
                                            } else {
                                                this.library_select(
                                                    i,
                                                    e.modifiers().shift,
                                                    e.modifiers().control || e.modifiers().platform,
                                                    cx,
                                                );
                                            }
                                        },
                                    ))
                                    .child(
                                        div()
                                            .w(px(if list_mode { 78. } else { 156. }))
                                            .h(px(if list_mode { 58. } else { 118. }))
                                            .relative()
                                            .overflow_hidden()
                                            .border_2()
                                            .border_color(if is_cur || item.selected {
                                                p.accent
                                            } else {
                                                p.line
                                            })
                                            .p_2()
                                            .child(image)
                                            .child(
                                                Checkbox::new(("batch-tick", i))
                                                    .absolute()
                                                    .top(px(4.))
                                                    .left(px(4.))
                                                    .p(px(4.))
                                                    .bg(p.panel)
                                                    .rounded_none()
                                                    .checked(item.selected)
                                                    .accessibility_label(format!("Select {name}"))
                                                    .tooltip("Select photo")
                                                    // The grid's mouse handler focuses its culling
                                                    // shortcuts. Preserve the kit's existing focus.
                                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                        cx.stop_propagation()
                                                    })
                                                    .on_change(cx.listener(
                                                        move |this, checked, _, cx| {
                                                            cx.stop_propagation();
                                                            if let Some(it) =
                                                                this.batch.items.get_mut(i)
                                                            {
                                                                it.selected = *checked;
                                                            }
                                                            cx.notify();
                                                        },
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(px(if list_mode { 450. } else { 156. }))
                                            .text_size(px(10.))
                                            .text_color(p.ink)
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .flex()
                                            .flex_col()
                                            .child(name)
                                            .child(mono(
                                                this.library_badge(&item.path),
                                                9.,
                                                p.muted,
                                            )),
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
        let preview: AnyElement = match self.batch.preview.clone() {
            Some((path, _, image)) => self.batch_image_preview(path, image, cx),
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
        let preview = if self.batch.develop.culling_mode > 0 {
            self.library_culling_view(cx)
        } else if self.batch.develop.compare {
            self.library_comparison_view(preview, cx)
        } else {
            preview
        };
        let filmstrip = self.library_filmstrip(cx);
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

        use gpui_kit::component::resizable::{h_resizable, resizable_panel};
        let left = div()
            .id("library-navigation")
            .test_support()
            .flex_none()
            .w_full()
            .h_full()
            .overflow_hidden()
            .border_r_1()
            .border_color(p.line)
            .flex()
            .flex_col()
            .min_h_0()
            .child(
                div()
                    .id("library-sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(!self.batch.develop.module_develop, |d| {
                        d.child(self.library_navigator(cx))
                    })
                    .child(library_controls),
            );
        let center = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_w_0()
            .min_h_0()
            .border_r_1()
            .border_color(p.line)
            .when(overview, |d| d.child(grid_tools))
            .child(
                div()
                    .id("batch-grid")
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        library_focus.focus(window, cx)
                    })
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .bg(p.stage)
                    .when(overview, |d| d.child(grid))
                    .when(!overview, |d| {
                        d.child(
                            div()
                                .id("library-preview")
                                .test_support()
                                .size_full()
                                .p_3()
                                .bg(p.stage)
                                .child(preview),
                        )
                    })
                    .test_support(),
            )
            .when(overview, |d| d.child(photo_header))
            .child(self.library_workflow_toolbar(cx))
            .when(overview, |d| d.child(selection_controls))
            .children(self.library_assistant_surface(cx))
            .child(
                div()
                    .px_2()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child(caption),
            );
        let body = if self.batch.develop.panels_hidden {
            center.into_any_element()
        } else {
            let mut split = h_resizable("library-panel-split")
                .child(
                    resizable_panel()
                        .size(px(220.))
                        .size_range(px(180.)..px(420.))
                        .child(left),
                )
                .child(
                    resizable_panel()
                        .size_range(px(240.)..Pixels::MAX)
                        .child(center),
                );
            if let Some(settings) = settings {
                split = split.child(
                    resizable_panel()
                        .size(px(310.))
                        .size_range(px(260.)..px(480.))
                        .child(settings),
                );
            }
            div().flex_1().min_h_0().child(split).into_any_element()
        };

        div()
            .id("library-workspace")
            .track_focus(self.batch.library.focus.as_ref().unwrap())
            .on_key_down(cx.listener(|this, event, window, cx| this.library_key(event, window, cx)))
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(p.paper)
            .text_color(p.ink)
            .child(navigation)
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
            .child(body)
            .child(bar.test_support())
            .when(!self.batch.develop.filmstrip_hidden, |d| d.child(filmstrip))
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

    #[test]
    fn profile_and_hdr_preview_pixels_use_gpui_channel_order() {
        let raster = emulsion_raster::Raster::from_pixels(
            2,
            1,
            [0; 4],
            &[[65535, 0, 0, 65535], [0, 0, 65535, 65535]],
        );
        let (w, h, bytes) = super::preview_bgra(&raster);
        let rendered = super::bgra_image(w, h, bytes);
        assert_eq!(
            rendered.as_bytes(0).unwrap(),
            &[0, 0, 255, 255, 255, 0, 0, 255]
        );
    }

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
        batch.finish_thumb(0, 400, 0, Err("decode failed".into()));
        batch.thumbs_visible = 900..910;
        assert_eq!(batch.request_thumbs()[0].0, 900);
        assert_eq!(batch.thumbs_active, super::THUMB_WORKERS);
        batch.finish_thumb(0, 401, 0, Err("decode failed".into()));
        batch.finish_thumb(0, 900, 0, Err("decode failed".into()));
        batch.thumbs_visible = 400..402;
        assert!(
            batch.request_thumbs().is_empty(),
            "failed files are not retried on redraw"
        );
        assert!(batch.items.iter().all(|item| !item.selected));
        assert!(batch.current.is_none() && batch.preview.is_none() && batch.running.is_none());
    }

    #[test]
    fn editing_one_photo_rejects_its_old_job_and_keeps_other_thumbnails() {
        let mut batch = thumbnail_state(2);
        assert_eq!(batch.request_thumbs().len(), 2);
        let edited = batch.items[0].path.clone();
        batch.invalidate_thumb(&edited);
        batch.finish_thumb(0, 0, 0, Ok((1, 1, vec![255; 4])));
        batch.finish_thumb(0, 1, 0, Ok((1, 1, vec![255; 4])));
        assert!(batch.items[0].thumb.is_none());
        let untouched = batch.items[1].thumb.clone().unwrap();
        assert_eq!(batch.request_thumbs(), vec![(0, edited)]);
        batch.finish_thumb(0, 0, 1, Ok((1, 1, vec![255; 4])));
        assert!(batch.items[0].thumb.is_some());
        assert!(std::sync::Arc::ptr_eq(
            batch.items[1].thumb.as_ref().unwrap(),
            &untouched
        ));
    }

    #[test]
    fn folder_changes_keep_worker_limit_and_reject_stale_thumbnails() {
        let mut batch = thumbnail_state(4);
        assert_eq!(batch.request_thumbs().len(), 2);
        batch.thumbs_generation += 1;
        batch.thumbs_requested.clear();
        assert!(batch.request_thumbs().is_empty());
        batch.finish_thumb(0, 0, 0, Ok((1, 1, vec![255; 4])));
        assert!(batch.items[0].thumb.is_none());
        assert_eq!(batch.request_thumbs().len(), 1);
        assert_eq!(batch.thumbs_active, 2);
        batch.finish_thumb(0, 1, 0, Err("decode failed".into()));
        batch.finish_thumb(1, 0, 0, Ok((1, 1, vec![255; 4])));
        assert!(batch.items[0].thumb.is_some());
    }

    #[test]
    fn scrolling_evicts_old_successful_thumbnails_but_can_reload_them() {
        let mut batch = thumbnail_state(super::THUMB_CACHE + 2);
        for index in 0..super::THUMB_CACHE + 2 {
            batch.thumbs_visible = index..index + 1;
            assert_eq!(batch.request_thumbs().len(), 1);
            batch.finish_thumb(0, index, 0, Ok((1, 1, vec![255; 4])));
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
