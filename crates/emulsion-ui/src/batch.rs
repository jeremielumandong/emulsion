//! The Batch tab: open a folder of pictures, tick the ones to treat, pick
//! a recipe, look at any of them large with the recipe applied, and export
//! them all — the same non-destructive pipeline the editor uses, run one
//! picture at a time off the UI thread.

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
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const THUMB: u32 = 300;
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
    thumbs_loading: HashSet<PathBuf>,
    /// The picture shown large.
    pub current: Option<usize>,
    /// Chosen recipe name, if any.
    pub recipe: Option<String>,
    recipes: Option<Vec<Recipe>>,
    tag: Option<String>,
    /// Large preview for (path, recipe).
    preview: Option<(PathBuf, Option<String>, Arc<RenderImage>)>,
    preview_loading: Option<(PathBuf, Option<String>)>,
    /// "jpg" or "png".
    pub format: String,
    pub out_dir: Option<PathBuf>,
    /// Export progress: done, total.
    pub running: Option<(usize, usize)>,
    pub note: Option<(SharedString, bool)>,
}

fn is_picture(p: &Path) -> bool {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| {
            emulsion_io::OPEN_EXTENSIONS
                .iter()
                .any(|x| *x == e && e != "svg")
        })
}

/// Pictures in a folder, sorted by name.
fn list_folder(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && is_picture(p))
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
    let doc = emulsion_io::open(path).map_err(|e| e.to_string())?;
    let mut ed = Editor::new(doc, None);
    let (w, h) = (ed.doc.width, ed.doc.height);
    if let Some(r) = recipe {
        let compiled = emulsion_recipes::compile_sized(r, w, h).map_err(|e| e.to_string())?;
        store::add_to(&mut ed, compiled, Slot::TOP).map_err(|e| e.to_string())?;
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "picture".into());
    let suffix = recipe
        .map(|r| format!("-{}", slug(&r.name)))
        .unwrap_or_default();
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let out = out_dir.join(format!("{stem}{suffix}.{ext}"));
    emulsion_io::export::export(&ed.doc, &out, ExportOptions::for_doc(&ed.doc))
        .map_err(|e| e.to_string())?;
    Ok(out)
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
            .map(|path| BatchItem {
                path,
                selected: true,
                thumb: None,
            })
            .collect();
        b.thumbs_loading.clear();
        b.current = (!b.items.is_empty()).then_some(0);
        b.preview = None;
        b.preview_loading = None;
        b.note = None;
        if b.format.is_empty() {
            b.format = "jpg".into();
        }
        cx.notify();
    }

    fn batch_thumbs(&mut self, cx: &mut Context<Self>) {
        let todo: Vec<PathBuf> = self
            .batch
            .items
            .iter()
            .filter(|i| i.thumb.is_none())
            .map(|i| i.path.clone())
            .filter(|p| !self.batch.thumbs_loading.contains(p))
            .take(6)
            .collect();
        for path in todo {
            self.batch.thumbs_loading.insert(path.clone());
            cx.spawn(async move |this, cx| {
                let p = path.clone();
                let r = cx
                    .background_spawn(async move {
                        emulsion_io::thumb::thumbnail(&p, THUMB).map(|(w, h, mut rgba)| {
                            for px in rgba.as_chunks_mut::<4>().0 {
                                px.swap(0, 2);
                            }
                            (w, h, rgba)
                        })
                    })
                    .await;
                this.update(cx, |this, cx| {
                    this.batch.thumbs_loading.remove(&path);
                    if let Ok((w, h, bgra)) = r
                        && let Some(item) = this.batch.items.iter_mut().find(|i| i.path == path)
                    {
                        item.thumb = Some(Arc::new(bgra_image(w, h, bgra)));
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    fn batch_recipes(&mut self) -> Vec<Recipe> {
        self.batch
            .recipes
            .get_or_insert_with(|| {
                store::list(&crate::editor::recipes_dir())
                    .into_iter()
                    .map(|(r, _)| r)
                    .collect()
            })
            .clone()
    }

    fn chosen_recipe(&mut self) -> Option<Recipe> {
        let name = self.batch.recipe.clone()?;
        self.batch_recipes().into_iter().find(|r| r.name == name)
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
        {
            return;
        }
        self.batch.preview_loading = Some(key.clone());
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
                if this.batch.preview_loading.as_ref() == Some(&key) {
                    this.batch.preview_loading = None;
                }
                if let Some((w, h, bgra)) = r {
                    this.batch.preview = Some((key.0, key.1, Arc::new(bgra_image(w, h, bgra))));
                }
                cx.notify();
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
        let ext = if self.batch.format == "png" {
            "png"
        } else {
            "jpg"
        }
        .to_string();
        let total = paths.len();
        self.batch.running = Some((0, total));
        self.batch.note = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut failed = 0;
            for (i, path) in paths.into_iter().enumerate() {
                let r = cx
                    .background_spawn({
                        let (recipe, out_dir, ext) = (recipe.clone(), out_dir.clone(), ext.clone());
                        async move { process_one(&path, recipe.as_ref(), &out_dir, &ext) }
                    })
                    .await;
                let go_on = this
                    .update(cx, |this, cx| {
                        if let Err(e) = r {
                            failed += 1;
                            this.batch.note = Some((e.into(), true));
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
                this.batch.running = None;
                if failed == 0 {
                    this.batch.note = Some((
                        format!("Exported {total} to {}", out_dir.display()).into(),
                        false,
                    ));
                } else {
                    this.batch.note = Some((
                        format!("Exported {} of {total}; {failed} failed", total - failed).into(),
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
        self.batch.running = None;
        self.batch.note = Some(("Export stopped.".into(), false));
        cx.notify();
    }

    // ── View ────────────────────────────────────────────────────────────

    pub(crate) fn batch_screen(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = theme::palette(cx);
        self.batch_thumbs(cx);
        self.batch_preview(cx);
        let recipes = self.batch_recipes();
        let selected = self.batch.items.iter().filter(|i| i.selected).count();
        let total = self.batch.items.len();

        // Toolbar.
        let folder_label: SharedString = match &self.batch.folder {
            Some(f) => f.display().to_string().into(),
            None => "no folder yet".into(),
        };
        let mut bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
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
                mono(folder_label, 10., p.muted)
                    .max_w(px(360.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis(),
            )
            .child(mono(format!("{selected} of {total} ticked"), 10., p.ink))
            .child(
                chip("batch-all", "all", false, &p).on_click(cx.listener(|this, _, _, cx| {
                    for i in &mut this.batch.items {
                        i.selected = true;
                    }
                    cx.notify();
                })),
            )
            .child(
                chip("batch-none", "none", false, &p).on_click(cx.listener(|this, _, _, cx| {
                    for i in &mut this.batch.items {
                        i.selected = false;
                    }
                    cx.notify();
                })),
            )
            .child(div().flex_1())
            .child(mono("export as", 10., p.muted));
        for f in ["jpg", "png"] {
            let on = self.batch.format == f;
            bar = bar.child(
                chip(
                    ("batch-fmt", f.len() + if f == "jpg" { 0 } else { 10 }),
                    f,
                    on,
                    &p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.batch.format = f.into();
                    cx.notify();
                })),
            );
        }
        let out_label: SharedString = self
            .batch
            .out_dir
            .as_ref()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "…".into())
            .into();
        bar = bar.child(
            chip("batch-out", format!("to {out_label}"), false, &p)
                .on_click(cx.listener(|this, _, _, cx| this.pick_batch_out_dir(cx))),
        );
        bar = match self.batch.running {
            Some((done, n)) => bar
                .child(mono(format!("exporting {done}/{n}"), 10., p.accent))
                .child(
                    chip("batch-stop", "stop", false, &p)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_batch(cx))),
                ),
            None => bar.child(
                button("batch-run", format!("Export {selected}"), selected > 0, &p)
                    .py(px(5.))
                    .on_click(cx.listener(|this, _, _, cx| this.run_batch(cx))),
            ),
        };
        if let Some((msg, err)) = &self.batch.note {
            bar = bar.child(mono(msg.clone(), 10., if *err { p.accent } else { p.ink }));
        }

        // Recipe chips.
        let mut tags: Vec<String> = recipes.iter().flat_map(|r| r.tags.clone()).collect();
        tags.sort();
        tags.dedup();
        let tag = self.batch.tag.clone();
        let mut recipe_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(5.))
            .px(px(16.))
            .py(px(8.))
            .border_b_1()
            .border_color(p.line)
            .child(label("Recipe", &p))
            .child(
                chip("batch-rc-none", "none", self.batch.recipe.is_none(), &p).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.batch.recipe = None;
                        cx.notify();
                    }),
                ),
            );
        for (i, r) in recipes.iter().enumerate() {
            if tag.as_ref().is_some_and(|t| !r.tags.contains(t)) {
                continue;
            }
            let on = self.batch.recipe.as_deref() == Some(r.name.as_str());
            let name = r.name.clone();
            recipe_row = recipe_row.child(chip(("batch-rc", i), r.name.clone(), on, &p).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.batch.recipe = Some(name.clone());
                    cx.notify();
                }),
            ));
        }
        recipe_row = recipe_row
            .child(div().flex_1())
            .child(mono("filter", 9.5, p.muted));
        recipe_row = recipe_row.child(chip("batch-tag-all", "all", tag.is_none(), &p).on_click(
            cx.listener(|this, _, _, cx| {
                this.batch.tag = None;
                cx.notify();
            }),
        ));
        for (i, t) in tags.iter().enumerate() {
            let on = tag.as_deref() == Some(t);
            let t2 = t.clone();
            recipe_row = recipe_row.child(chip(("batch-tag", i), t.clone(), on, &p).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.batch.tag = Some(t2.clone());
                    cx.notify();
                }),
            ));
        }

        // Grid of pictures.
        let current = self.batch.current;
        let mut grid = div().flex().flex_wrap().gap(px(8.)).p(px(12.));
        for (i, item) in self.batch.items.iter().enumerate() {
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
            grid = grid.child(
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
                                    .text_color(gpui_kit::white())
                                    .text_size(px(10.))
                                    .font_family(MONO_FONT)
                                    .child(if item.selected { "✓" } else { "" })
                                    .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                                        let _ = e;
                                        if let Some(it) = this.batch.items.get_mut(i) {
                                            it.selected = !it.selected;
                                        }
                                        cx.notify();
                                    })),
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
                    ),
            );
        }
        if self.batch.items.is_empty() {
            grid = grid.child(
                div()
                    .p(px(24.))
                    .text_color(p.muted)
                    .child("Choose a folder to see its pictures here."),
            );
        }

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
                    } else {
                        ""
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
            .child(bar)
            .child(recipe_row)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("batch-grid")
                            .w(px(460.))
                            .flex_none()
                            .min_h_0()
                            .overflow_y_scroll()
                            .border_r_1()
                            .border_color(p.line)
                            .child(grid),
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
                    ),
            )
    }
}
