use super::*;
use std::collections::HashMap;

const SIZE: u32 = 150;
const CACHE_SIZE: usize = 128;

#[derive(Default)]
pub(super) struct RecipePreviews {
    pub(super) path: Option<PathBuf>,
    pub(super) images: HashMap<usize, Arc<RenderImage>>,
    pub(super) attempted: HashSet<usize>,
    pub(super) visible: Vec<usize>,
    pub(super) busy: bool,
    pub(super) failed_source: bool,
    catalog: Option<Arc<Vec<Recipe>>>,
    folder_generation: u64,
    generation: u64,
    source: Option<Arc<Raster>>,
    cached: VecDeque<usize>,
}

impl RecipePreviews {
    fn sync(&mut self, path: Option<PathBuf>, catalog: Option<Arc<Vec<Recipe>>>, folder: u64) {
        let same_catalog = match (&self.catalog, &catalog) {
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            (None, None) => true,
            _ => false,
        };
        if self.path == path && same_catalog && self.folder_generation == folder {
            return;
        }
        self.path = path;
        self.catalog = catalog;
        self.folder_generation = folder;
        self.generation = self.generation.wrapping_add(1);
        self.images.clear();
        self.attempted.clear();
        self.visible.clear();
        self.cached.clear();
        self.source = None;
        self.failed_source = false;
        // Keep the worker occupied until old work returns, even after switching photos.
    }

    fn next(&mut self) -> Option<(usize, Recipe)> {
        if self.busy || self.failed_source || self.path.is_none() {
            return None;
        }
        let catalog = self.catalog.as_ref()?;
        let index = self
            .visible
            .iter()
            .copied()
            .find(|index| *index < catalog.len() && !self.attempted.contains(index))?;
        self.attempted.insert(index);
        self.busy = true;
        Some((index, catalog[index].clone()))
    }

    fn finish(
        &mut self,
        generation: u64,
        index: usize,
        source: Option<Arc<Raster>>,
        pixels: Option<(u32, u32, Vec<u8>)>,
    ) {
        self.busy = false;
        if generation != self.generation {
            return;
        }
        self.failed_source = source.is_none();
        self.source = source;
        if let Some((width, height, pixels)) = pixels {
            self.images
                .insert(index, Arc::new(bgra_image(width, height, pixels)));
            self.cached.push_back(index);
            while self.cached.len() > CACHE_SIZE {
                let Some(position) = self
                    .cached
                    .iter()
                    .position(|index| !self.visible.contains(index))
                else {
                    break;
                };
                let old = self.cached.remove(position).unwrap();
                self.images.remove(&old);
                self.attempted.remove(&old);
            }
        }
    }
}

impl Workspace {
    pub(super) fn prepare_batch_recipe_previews(&mut self, _: &mut Context<Self>) {
        let path = self
            .batch
            .current
            .and_then(|index| self.batch.items.get(index))
            .map(|item| item.path.clone());
        self.batch.recipe_previews.sync(
            path,
            self.batch.recipes.clone(),
            self.batch.thumbs_generation,
        );
    }

    pub(super) fn load_batch_recipe_previews(&mut self, cx: &mut Context<Self>) {
        if !self.batch.recipe_browser || self.screen != crate::workspace::Screen::Batch {
            return;
        }
        self.prepare_batch_recipe_previews(cx);
        let previews = &mut self.batch.recipe_previews;
        let Some((index, recipe)) = previews.next() else {
            return;
        };
        let generation = previews.generation;
        let path = previews.path.clone().expect("preview requires a photo");
        let source = previews.source.clone();
        cx.spawn(async move |this, cx| {
            let (source, pixels) = cx
                .background_spawn(async move {
                    let source = source.or_else(|| small_raster(&path, SIZE).map(Arc::new));
                    let pixels = source
                        .as_ref()
                        .and_then(|source| render_with(source.clone(), Some(&recipe)));
                    (source, pixels)
                })
                .await;
            this.update(cx, |this, cx| {
                // Selection can change before the next frame has prepared the new state.
                this.prepare_batch_recipe_previews(cx);
                this.batch
                    .recipe_previews
                    .finish(generation, index, source, pixels);
                this.load_batch_recipe_previews(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::RecipePreviews;
    use emulsion_raster::Raster;
    use emulsion_recipes::Recipe;
    use std::sync::Arc;

    #[test]
    fn recipe_previews_require_a_photo_and_bound_work_to_visible_cards() {
        let mut previews = RecipePreviews::default();
        let catalog = Arc::new(vec![Recipe::default(); 300]);
        previews.sync(None, Some(catalog.clone()), 0);
        previews.visible = vec![120, 121];
        assert!(previews.next().is_none());
        previews.sync(Some("photo.png".into()), Some(catalog), 0);
        previews.visible = vec![120, 121];
        assert_eq!(previews.next().unwrap().0, 120);
        assert!(previews.next().is_none());
        let source = Arc::new(Raster::from_srgba8(1, 1, &[90, 90, 90, 255]));
        previews.finish(previews.generation, 120, Some(source.clone()), None);
        assert_eq!(previews.next().unwrap().0, 121);
        previews.finish(previews.generation, 121, Some(source), None);
        assert!(
            previews.next().is_none(),
            "failed recipes do not retry on redraw"
        );
    }

    #[test]
    fn photo_and_catalog_changes_reject_old_renders_without_extra_workers() {
        let catalog = Arc::new(vec![Recipe::default()]);
        let mut previews = RecipePreviews::default();
        previews.sync(Some("first.png".into()), Some(catalog.clone()), 0);
        previews.visible = vec![0];
        previews.next().unwrap();
        let generation = previews.generation;
        previews.sync(Some("second.png".into()), Some(catalog), 0);
        previews.visible = vec![0];
        assert!(previews.next().is_none());
        let source = Arc::new(Raster::from_srgba8(1, 1, &[255; 4]));
        previews.finish(
            generation,
            0,
            Some(source.clone()),
            Some((1, 1, vec![255; 4])),
        );
        assert!(previews.images.is_empty() && previews.source.is_none());
        previews.next().unwrap();
        previews.finish(
            previews.generation,
            0,
            Some(source),
            Some((1, 1, vec![255; 4])),
        );
        assert_eq!(previews.images.len(), 1);
        previews.sync(
            Some("second.png".into()),
            Some(Arc::new(vec![Recipe::default()])),
            0,
        );
        assert!(previews.images.is_empty() && previews.source.is_none());
    }

    #[test]
    fn previews_apply_each_recipe_to_the_original_photo() {
        let source = Arc::new(Raster::from_srgba8(2, 2, &[80; 16]));
        let original = source.to_srgba8();
        let normal = Recipe::default();
        let brighter = Recipe {
            exposure_compensation: "+2".into(),
            ..normal.clone()
        };
        let normal_pixels = super::render_with(source.clone(), Some(&normal)).unwrap().2;
        let bright_pixels = super::render_with(source.clone(), Some(&brighter))
            .unwrap()
            .2;
        assert_ne!(normal_pixels, bright_pixels);
        assert_eq!(
            source.to_srgba8(),
            original,
            "browsing never edits the source"
        );
        assert_eq!(
            super::render_with(source, Some(&normal)).unwrap().2,
            normal_pixels
        );
    }

    #[test]
    fn missing_photo_preview_stops_work_until_source_changes() {
        let mut previews = RecipePreviews::default();
        previews.sync(
            Some("missing.png".into()),
            Some(Arc::new(vec![Recipe::default(); 2])),
            0,
        );
        previews.visible = vec![0, 1];
        previews.next().unwrap();
        previews.finish(previews.generation, 0, None, None);
        assert!(previews.failed_source);
        assert!(previews.next().is_none());
    }
}
