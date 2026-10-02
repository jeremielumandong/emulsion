//! Library RAW development uses Photo's decoder and portable sidecars.
use super::*;
use emulsion_io::{photo_develop::PhotoSource as RawSource, raw::DevelopParams, raw_settings};
use gpui_kit::component::{
    Disableable, Selectable,
    slider::{Slider, SliderEvent, SliderState},
};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
struct ComparisonPreviewKey {
    path: PathBuf,
    params: DevelopParams,
    detail_region: Option<[f32; 2]>,
    full_preview: bool,
}

impl ComparisonPreviewKey {
    fn new(
        path: PathBuf,
        params: DevelopParams,
        detail_region: Option<[f32; 2]>,
        full_preview: bool,
    ) -> Self {
        Self {
            path,
            // Keep spatial corrections so the same subject occupies the same
            // pixels on both sides, while comparing the default tonal treatment.
            params: DevelopParams {
                crop: params.crop,
                rotation: params.rotation,
                straighten: params.straighten,
                perspective: params.perspective,
                distortion: params.distortion,
                lens_profile: params.lens_profile,
                aberration: params.aberration,
                ..Default::default()
            },
            detail_region,
            full_preview,
        }
    }
}

fn develop_preview_pixels(
    source: &RawSource,
    params: &DevelopParams,
    detail_region: Option<[f32; 2]>,
    full_preview: bool,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(u32, u32, Vec<u8>), String> {
    let raster = if let Some(center) = detail_region {
        source.develop_region(params, center, 1024, cancel)
    } else if full_preview {
        source.develop_with_cancel(params, cancel)
    } else {
        source.develop_preview(params, cancel)
    }
    .map_err(|e| e.to_string())?;
    if detail_region.is_some() {
        Ok((raster.width(), raster.height(), raster.to_srgba8()))
    } else {
        display_raster(&raster).ok_or_else(|| t!("library.develop.err_raw_preview").into_owned())
    }
}

/// Computes a photo's new RAW settings from its current ones.
pub(super) type SettingsTransform =
    Arc<dyn Fn(DevelopParams) -> emulsion_io::Result<DevelopParams> + Send + Sync>;

#[derive(Default)]
pub(super) struct Develop {
    pub(super) rotation_controls: super::rotation::RotationControls,
    pub(super) source: Option<Arc<RawSource>>,
    pub(super) drafts: HashMap<PathBuf, DevelopParams>,
    pub(super) saved: HashMap<PathBuf, DevelopParams>,
    pub(super) fingerprints: HashMap<PathBuf, String>,
    pub(super) history: HashMap<PathBuf, Vec<DevelopParams>>,
    pub(super) snapshots: HashMap<PathBuf, std::collections::BTreeMap<String, DevelopParams>>,
    pub(super) snapshot_generation: u64,
    pub(super) sliders: Vec<(Entity<SliderState>, Subscription)>,
    pub(super) slider_key: Option<(PathBuf, DevelopParams)>,
    pub(super) section: usize,
    pub(super) channel: usize,
    pub(super) mask: usize,
    pub(super) picking_sky: bool,
    pub(super) mask_seed: Option<[f32; 2]>,
    pub(super) curve_bounds: crate::widgets::TrackBounds,
    pub(super) navigator_bounds: crate::widgets::TrackBounds,
    pub(super) navigator_preview: Option<(PathBuf, Arc<RenderImage>)>,
    pub(super) curve_drag: Option<usize>,
    pub(super) presets_loaded: bool,
    pub(super) quick_presets_open: bool,
    /// Expanded bundled-preset category, shared by both preset pickers.
    pub(super) film_category: Option<&'static str>,
    pub(super) profiles_open: bool,
    pub(super) color_view_open: bool,
    pub(super) profiles: Option<Vec<emulsion_io::camera_profiles::ProfileSummary>>,
    pub(super) preset_files: Vec<PathBuf>,
    pub(super) preset_report: Option<emulsion_io::lightroom_presets::ImportedPreset>,
    pub(super) preset_import_notes: Vec<String>,
    pub(super) preset_import_expanded: bool,
    pub(super) preset_report_expanded: bool,
    pub(super) ai_job: Option<Arc<emulsion_ai::jobs::Job>>,
    pub busy: bool,
    pub(super) preview_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub(super) gesture_active: bool,
    pub(super) gesture_recorded: bool,
    pub(super) panels_hidden: bool,
    pub(super) filmstrip_hidden: bool,
    pub(super) left_section: usize,
    pub(super) auto_advance: bool,
    pub(super) canvas_tool: usize,
    pub(super) canvas_points: Vec<[f32; 2]>,
    pub(super) perspective_guides: Option<(PathBuf, Vec<[[f32; 2]; 2]>)>,
    pub(super) clone_source: Option<[f32; 2]>,
    pub(super) brush_radius: f32,
    pub(super) active_mask: Option<u32>,
    pub(super) mask_overlay: bool,
    pub(super) dust_visualization: bool,
    pub(super) mask_intersect: bool,
    pub(super) culling_mode: usize,
    pub(super) culling_loading: bool,
    pub(super) culling_key: Vec<(PathBuf, Option<DevelopParams>)>,
    pub(super) culling_images: HashMap<PathBuf, Arc<RenderImage>>,
    pub(super) culling_zoom: f32,
    pub(super) culling_center: Option<[f32; 2]>,
    pub(super) preview_stale: bool,
    pub(super) full_preview: bool,
    pub(super) color_view: emulsion_io::icc::PhotoView,
    pub(super) detail_region: Option<[f32; 2]>,
    pub saving: bool,
    pub before: bool,
    pub histogram: [u32; 32],
    pub rgb_histogram: [[u32; 32]; 3],
    pub clipping: bool,
    pub loupe: bool,
    pub(super) module_develop: bool,
    pub list: bool,
    pub compare: bool,
    pub(super) comparison_position: Option<f32>,
    pub(super) comparison_dragging: bool,
    pub(super) comparison_bounds: crate::widgets::TrackBounds,
    baseline_preview: Option<(ComparisonPreviewKey, Arc<RenderImage>)>,
    pub anchor: Option<usize>,
    pub inspector: usize,
    pub(super) save_task: Option<Task<()>>,
    pub(super) sync_group: raw_settings::RawSettingsGroup,
}

impl Develop {
    pub fn refresh_saved(&mut self) {
        // Returning from Photo rereads persisted settings. Keep any failed or
        // pending local drafts, which must never be discarded by navigation.
        self.drafts
            .retain(|path, params| self.saved.get(path) != Some(params));
        self.saved.retain(|path, _| self.drafts.contains_key(path));
        self.source = None;
        self.slider_key = None;
        self.baseline_preview = None;
    }
    pub fn dirty(&self) -> bool {
        self.drafts
            .iter()
            .any(|(p, v)| self.saved.get(p) != Some(v))
    }
    pub(super) fn current_params(&self, path: &Path) -> Option<DevelopParams> {
        self.drafts
            .get(path)
            .copied()
            .or_else(|| self.saved.get(path).copied())
    }
}

pub(super) fn display_raster(raster: &Raster) -> Option<(u32, u32, Vec<u8>)> {
    let image = image::RgbaImage::from_raw(raster.width(), raster.height(), raster.to_srgba8())?;
    let small = image::DynamicImage::ImageRgba8(image)
        .thumbnail(PREVIEW, PREVIEW)
        .into_rgba8();
    Some((small.width(), small.height(), small.into_raw()))
}

pub(super) fn histogram(bytes: &[u8]) -> [u32; 32] {
    let mut bins = [0; 32];
    for px in bytes.as_chunks::<4>().0 {
        if px[3] == 0 {
            continue;
        }
        let luminance = (54 * px[0] as usize + 183 * px[1] as usize + 19 * px[2] as usize) / 256;
        bins[(luminance / 8).min(31)] += 1;
    }
    bins
}

impl Workspace {
    pub(super) fn invalidate_library_preview(&mut self) {
        if let Some(cancel) = self.batch.develop.preview_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.batch.preview_generation = self.batch.preview_generation.wrapping_add(1);
        if self.batch.preview.as_ref().is_some_and(|(path, _, _)| {
            self.batch
                .current
                .and_then(|i| self.batch.items.get(i))
                .is_none_or(|i| &i.path != path)
        }) {
            self.batch.preview = None;
        }
        self.batch.develop.preview_stale = true;
        self.batch.develop.full_preview = false;
        self.batch.preview_loading = None;
        self.batch.preview_failed = None;
    }

    pub(super) fn library_select(
        &mut self,
        index: usize,
        shift: bool,
        toggle: bool,
        cx: &mut Context<Self>,
    ) {
        if index >= self.batch.items.len() {
            return;
        }
        if shift {
            let anchor = self
                .batch
                .develop
                .anchor
                .unwrap_or(index)
                .min(self.batch.items.len() - 1);
            for (i, item) in self.batch.items.iter_mut().enumerate() {
                if !toggle {
                    item.selected = false;
                }
                if (anchor.min(index)..=anchor.max(index)).contains(&i) {
                    item.selected = true;
                }
            }
        } else if toggle {
            self.batch.items[index].selected = !self.batch.items[index].selected;
            self.batch.develop.anchor = Some(index);
        } else {
            for (i, item) in self.batch.items.iter_mut().enumerate() {
                item.selected = i == index;
            }
            self.batch.develop.anchor = Some(index);
        }
        self.batch.develop.gesture_active = false;
        self.batch.develop.gesture_recorded = false;
        self.batch.develop.canvas_points.clear();
        self.batch.develop.active_mask = None;
        self.batch.current = Some(index);
        if self
            .batch
            .develop
            .source
            .as_ref()
            .is_some_and(|source| source.source != self.batch.items[index].path)
        {
            self.batch.develop.source = None;
        }
        self.batch.develop.before = false;
        self.invalidate_library_preview();
        cx.notify();
    }

    pub(super) fn library_raw_preview(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.batch.develop.busy || self.batch.hdr_cancel.is_some() {
            return;
        }
        let key = (path.clone(), self.batch.recipe.clone());
        if self.batch.preview.as_ref().is_some_and(|(p, r, _)| {
            !self.batch.develop.preview_stale && (p, r) == (&key.0, &key.1)
        }) || self.batch.preview_loading.as_ref() == Some(&key)
            || self.batch.preview_failed.as_ref() == Some(&key)
        {
            return;
        }
        let generation = self.batch.preview_generation;
        let snapshot_generation = self.batch.develop.snapshot_generation;
        let params = self.batch.develop.current_params(&path);
        if self
            .batch
            .develop
            .source
            .as_ref()
            .is_some_and(|s| s.source != path)
        {
            // Release the previous mosaic before reserving memory for its replacement.
            self.batch.develop.source = None;
        }
        let cached = self
            .batch
            .develop
            .source
            .clone()
            .filter(|s| s.source == path);
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.batch.develop.preview_cancel = Some(cancel.clone());
        let detail_region = self.batch.develop.detail_region;
        let full_preview = self.batch.develop.full_preview;
        let tool_active = self.batch.develop.canvas_tool != 0;
        let mask_overlay = self.batch.develop.mask_overlay;
        let dust_visualization = self.batch.develop.dust_visualization;
        let active_mask = self.batch.develop.active_mask;
        let color_view = self.batch.develop.color_view.clone();
        let clipping = self.batch.develop.clipping;
        let before = self.batch.develop.before;
        let cached_baseline = self
            .batch
            .develop
            .baseline_preview
            .as_ref()
            .map(|(key, _)| key.clone());
        let compare = self.batch.develop.compare;
        let recipe = if before { None } else { self.chosen_recipe() };
        self.batch.develop.busy = true;
        self.batch.preview_loading = Some(key.clone());
        cx.spawn(async move |this, cx| {
            let (decoded, result) = cx
                .background_spawn(async move {
                    let source = match cached {
                        Some(s) => s,
                        None => match RawSource::load(&path) {
                            Ok(source) => Arc::new(source),
                            Err(error) => return (None, Err(error.to_string())),
                        },
                    };
                    let result = (|| {
                        let saved =
                            raw_settings::adjacent_settings(&source.source, &source.source_sha256)
                                .map_err(|e| e.to_string())?;
                        let params = params.unwrap_or(saved);
                        let render_params = if before {
                            DevelopParams::default()
                        } else if tool_active {
                            DevelopParams {
                                crop: [0., 0., 1., 1.],
                                straighten: 0.,
                                perspective: [0.; 2],
                                distortion: 0.,
                                lens_profile: None,
                                aberration: [0.; 2],
                                ..params
                            }
                        } else {
                            params
                        };
                        let (w, h, rgba) = develop_preview_pixels(
                            &source,
                            &render_params,
                            detail_region,
                            full_preview,
                            &cancel,
                        )?;
                        let bins = histogram(&rgba);
                        let rgb_bins = rgb_histogram(&rgba);
                        let mut pixels = render_display_pixels(w, h, rgba, recipe.as_ref())
                            .ok_or_else(|| t!("library.develop.err_render_recipe").into_owned())?;
                        if mask_overlay
                            && tool_active
                            && let Some(digest) = params.local_edits
                        {
                            let edits = emulsion_io::develop_edits::load(&digest)
                                .map_err(|e| e.to_string())?;
                            let reference = source
                                .develop_preview(
                                    &DevelopParams {
                                        local_edits: None,
                                        rotation: 0,
                                        ..render_params
                                    },
                                    &cancel,
                                )
                                .map_err(|e| e.to_string())?;
                            let mut spots = edits.clone();
                            spots.masks.clear();
                            let reference =
                                emulsion_io::develop_edits::apply(reference, &spots, &cancel)
                                    .map_err(|e| e.to_string())?;
                            emulsion_io::develop_edits::overlay_oriented(
                                &mut pixels.2,
                                pixels.0,
                                pixels.1,
                                &reference,
                                &edits,
                                active_mask,
                                params.rotation,
                            )
                            .map_err(|e| e.to_string())?;
                        }
                        if dust_visualization {
                            emulsion_io::develop_edits::visualize_dust(
                                &mut pixels.2,
                                pixels.0,
                                pixels.1,
                            );
                        }
                        color_view.apply(&mut pixels.2).map_err(|e| e.to_string())?;
                        if clipping {
                            clipping_overlay(&mut pixels.2);
                        }
                        let comparison_key = ComparisonPreviewKey::new(
                            path.clone(),
                            render_params,
                            detail_region,
                            full_preview,
                        );
                        let baseline =
                            if compare && cached_baseline.as_ref() != Some(&comparison_key) {
                                let (w, h, mut rgba) = develop_preview_pixels(
                                    &source,
                                    &comparison_key.params,
                                    detail_region,
                                    full_preview,
                                    &cancel,
                                )?;
                                for px in rgba.as_chunks_mut::<4>().0 {
                                    px.swap(0, 2);
                                }
                                Some((comparison_key, w, h, rgba))
                            } else {
                                None
                            };
                        let (history, snapshots) =
                            raw_settings::photo_history(&source.source, &source.source_sha256)
                                .map_err(|e| e.to_string())?;
                        Ok::<_, String>((
                            source.clone(),
                            saved,
                            params,
                            pixels,
                            bins,
                            baseline,
                            history,
                            snapshots,
                            rgb_bins,
                        ))
                    })();
                    (Some(source), result)
                })
                .await;
            this.update(cx, |this, cx| {
                this.batch.develop.busy = false;
                // Slider edits can cancel the first preview after decoding has
                // finished. Keep that expensive source for the next generation,
                // but never attach it to a different selected photo.
                this.batch.retain_decoded_preview_source(&key.0, decoded);
                if this.batch.preview_generation != generation
                    || this
                        .batch
                        .current
                        .and_then(|i| this.batch.items.get(i))
                        .map(|i| &i.path)
                        != Some(&key.0)
                {
                    cx.notify();
                    return;
                }
                match result {
                    Ok((
                        source,
                        saved,
                        params,
                        pixels,
                        bins,
                        baseline,
                        history,
                        snapshots,
                        rgb_bins,
                    )) => {
                        this.batch
                            .develop
                            .history
                            .entry(key.0.clone())
                            .or_insert(history);
                        if this.batch.develop.snapshot_generation == snapshot_generation {
                            this.batch
                                .develop
                                .snapshots
                                .insert(key.0.clone(), snapshots);
                        }
                        // Cache just one decoded mosaic. Drafts contain settings, never full images.
                        this.batch
                            .develop
                            .fingerprints
                            .entry(key.0.clone())
                            .or_insert_with(|| source.source_sha256.clone());
                        this.batch.develop.source = Some(source);
                        this.batch
                            .develop
                            .saved
                            .entry(key.0.clone())
                            .or_insert(saved);
                        this.batch
                            .develop
                            .drafts
                            .entry(key.0.clone())
                            .or_insert(params);
                        if this.batch.finish_preview(generation, key, Some(pixels)) {
                            this.batch.develop.preview_stale = false;
                            if detail_region.is_none()
                                && let Some((path, _, image)) = &this.batch.preview
                            {
                                this.batch.develop.navigator_preview =
                                    Some((path.clone(), image.clone()));
                            }
                            if let Some((key, w, h, bytes)) = baseline {
                                this.batch.develop.baseline_preview =
                                    Some((key, Arc::new(bgra_image(w, h, bytes))));
                            }
                            if !full_preview
                                && detail_region.is_none()
                                && !tool_active
                                && this.batch.develop.source.as_ref().is_some_and(|s| {
                                    !s.is_proxy()
                                        && emulsion_io::photo_develop::is_raw_photo(&s.source)
                                        && s.info.width.max(s.info.height) > PREVIEW
                                })
                            {
                                cx.spawn(async move |this, cx| {
                                    cx.background_executor()
                                        .timer(std::time::Duration::from_millis(900))
                                        .await;
                                    this.update(cx, |this, cx| {
                                        if this.batch.preview_generation == generation
                                            && !this.batch.develop.busy
                                            && !this.batch.develop.gesture_active
                                            && this.batch.develop.culling_mode == 0
                                            && this.batch.develop.module_develop
                                        {
                                            this.batch.develop.full_preview = true;
                                            this.batch.develop.preview_stale = true;
                                            cx.notify();
                                        }
                                    })
                                    .ok();
                                })
                                .detach();
                            }
                            this.batch.develop.histogram = bins;
                            this.batch.develop.rgb_histogram = rgb_bins;
                        }
                    }
                    Err(e) => {
                        if full_preview && this.batch.preview.is_some() {
                            this.batch.develop.preview_stale = false;
                            this.batch.preview_loading = None;
                            this.batch.note = Some((
                                t!("library.develop.fit_preview_only", error = e).into(),
                                true,
                            ));
                            cx.notify();
                            return;
                        }
                        if this.batch.finish_preview(generation, key, None) {
                            this.batch.note = Some((e.into(), true));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn library_adjust(&mut self, params: DevelopParams, cx: &mut Context<Self>) {
        let Some(path) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone())
        else {
            return;
        };
        if self
            .batch
            .develop
            .current_params(&path)
            .is_some_and(|p| p.rotation != params.rotation || p.straighten != params.straighten)
        {
            // Geometry changes must show the whole image, including square photos
            // whose dimensions do not change after a quarter turn.
            self.batch.develop.detail_region = None;
            // Editing overlays render the unwarped source. Leave them so the
            // new angle is actually visible, including after drawing a horizon.
            if self.batch.develop.canvas_tool != 7 {
                self.batch.develop.canvas_tool = 0;
            }
            self.batch.develop.canvas_points.clear();
            self.batch.navigation.borrow_mut().fit();
        }
        let dependencies_changed = self.batch.develop.current_params(&path).is_none_or(|p| {
            p.camera_profile != params.camera_profile
                || p.depth_map != params.depth_map
                || p.local_edits != params.local_edits
                || p.wide_gamut != params.wide_gamut
                || p.sensor_noise_reduction != params.sensor_noise_reduction
                || p.sensor_ai_denoise != params.sensor_ai_denoise
                || p.wb_override != params.wb_override
        });
        if dependencies_changed
            && let Some(source) = self
                .batch
                .develop
                .source
                .as_ref()
                .filter(|s| s.source == path)
            && let Err(e) = source.validate_settings(&params)
        {
            self.batch.note = Some((e.to_string().into(), true));
            cx.notify();
            return;
        }
        if let Some(previous) = self.batch.develop.current_params(&path) {
            let history = self.batch.develop.history.entry(path.clone()).or_default();
            if previous != params
                && (!self.batch.develop.gesture_active || !self.batch.develop.gesture_recorded)
            {
                self.batch.develop.gesture_recorded = self.batch.develop.gesture_active;
                history.push(previous);
                if history.len() > 100 {
                    history.remove(0);
                }
            }
        }
        self.batch.develop.drafts.insert(path, params);
        self.batch.develop.before = false;
        self.invalidate_library_preview();
        self.library_schedule_save(cx);
        cx.notify();
    }

    pub(super) fn library_save_develop(&mut self, sync: bool, cx: &mut Context<Self>) {
        if self.batch.develop.saving || self.batch.running.is_some() {
            return;
        }
        let edits: Vec<_> = self
            .batch
            .develop
            .drafts
            .iter()
            .filter(|(p, v)| self.batch.develop.saved.get(*p) != Some(*v))
            .map(|(p, v)| (p.clone(), *v))
            .collect();
        if sync {
            let Some(path) = self
                .batch
                .current
                .and_then(|i| self.batch.items.get(i))
                .map(|i| &i.path)
            else {
                return;
            };
            let Some(params) = self.batch.develop.current_params(path) else {
                return;
            };
            if params.wb_override.is_some()
                && matches!(
                    self.batch.develop.sync_group,
                    raw_settings::RawSettingsGroup::All
                        | raw_settings::RawSettingsGroup::WhiteBalance
                )
            {
                self.batch.note = Some((t!("library.develop.sampled_wb_sync").into(), true));
                cx.notify();
                return;
            }
            let group = self.batch.develop.sync_group;
            self.library_apply_to_selection(
                Arc::new(move |before| Ok(raw_settings::merge_settings(before, params, group))),
                cx,
            );
            return;
        }
        self.library_write_develop(edits, None, cx);
    }

    /// Replace each selected RAW's settings with `transform` of its current
    /// settings, saved and undoable like Sync settings.
    pub(super) fn library_apply_to_selection(
        &mut self,
        transform: SettingsTransform,
        cx: &mut Context<Self>,
    ) {
        if self.batch.develop.saving || self.batch.running.is_some() {
            return;
        }
        let edits = self
            .batch
            .items
            .iter()
            .filter(|i| i.selected && emulsion_io::photo_develop::supported(&i.path))
            .map(|i| (i.path.clone(), DevelopParams::default()))
            .collect();
        self.library_write_develop(edits, Some(transform), cx);
    }

    fn library_write_develop(
        &mut self,
        edits: Vec<(PathBuf, DevelopParams)>,
        transform: Option<SettingsTransform>,
        cx: &mut Context<Self>,
    ) {
        if edits.is_empty() {
            return;
        }
        let sync = transform.is_some();
        let drafts = self.batch.develop.drafts.clone();
        let expected = self.batch.develop.saved.clone();
        let fingerprints = self.batch.develop.fingerprints.clone();
        self.batch.develop.saving = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_spawn(async move {
                    edits
                        .into_iter()
                        .map(|(path, mut params)| {
                            let mut previous = None;
                            let result = (|| {
                                // Settings saves must not reserve a second RAW
                                // mosaic while the active preview is cached.
                                let digest =
                                    emulsion_io::photo_develop::settings_fingerprint(&path)?;
                                // Validate existing settings before replacing them, including fingerprint.
                                let current = raw_settings::adjacent_settings(&path, &digest)?;
                                if expected.get(&path).is_some_and(|p| *p != current)
                                    || fingerprints.get(&path).is_some_and(|d| d != &digest)
                                {
                                    return Err(emulsion_io::IoError::Manifest(
                                        t!("library.develop.changed_outside").into_owned(),
                                    ));
                                }
                                if let Some(transform) = &transform {
                                    let before = drafts.get(&path).copied().unwrap_or(current);
                                    previous = Some(before);
                                    params = transform(before)?;
                                }
                                raw_settings::save_photo_settings(&path, &digest, params)
                            })();
                            (path, params, previous, result.map_err(|e| e.to_string()))
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                this.batch.develop.saving = false;
                let mut failed = Vec::new();
                let mut saved = 0;
                let mut refresh_active = false;
                for (path, params, previous, result) in results {
                    match result {
                        Ok(()) => {
                            this.batch.develop.saved.insert(path.clone(), params);
                            if sync
                                && this
                                    .batch
                                    .develop
                                    .drafts
                                    .get(&path)
                                    .is_none_or(|draft| Some(*draft) == previous)
                            {
                                if let Some(previous) = previous.filter(|p| *p != params) {
                                    let history =
                                        this.batch.develop.history.entry(path.clone()).or_default();
                                    history.push(previous);
                                    if history.len() > 100 {
                                        history.remove(0);
                                    }
                                }
                                refresh_active |= previous != Some(params)
                                    && this
                                        .batch
                                        .current
                                        .and_then(|i| this.batch.items.get(i))
                                        .is_some_and(|item| item.path == path);
                                this.batch.develop.drafts.insert(path.clone(), params);
                            }
                            this.batch.invalidate_thumb(&path);
                            saved += 1;
                        }
                        Err(e) => failed.push(format!("{}: {e}", path.display())),
                    }
                }
                // Autosave persists an already rendered draft; it must not
                // re-develop the active photo or clear unrelated thumbnails.
                if refresh_active {
                    this.invalidate_library_preview();
                }
                this.batch_thumbs(cx);
                if failed.is_empty() && this.batch.develop.dirty() {
                    this.library_schedule_save(cx);
                }
                this.batch.note = Some((
                    if failed.is_empty() {
                        t!("library.develop.saved_raw_settings", count = saved).into_owned()
                    } else {
                        t!(
                            "library.develop.saved_some_failed",
                            saved = saved,
                            failed = failed.len(),
                            first = failed[0]
                        )
                        .into_owned()
                    }
                    .into(),
                    !failed.is_empty(),
                ));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn library_auto(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.batch.develop.source.clone() else {
            return;
        };
        let Some(path) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone())
        else {
            return;
        };
        if source.source != path || self.batch.develop.busy {
            return;
        }
        let params = self.batch.develop.current_params(&path).unwrap_or_default();
        self.batch.develop.busy = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { source.auto_adjust(&params) })
                .await;
            this.update(cx, |this, cx| {
                this.batch.develop.busy = false;
                // Never replace a newer adjustment or another photo's controls.
                if this.batch.develop.current_params(&path) == Some(params) {
                    match result {
                        Ok(next) => {
                            if next != params {
                                let history =
                                    this.batch.develop.history.entry(path.clone()).or_default();
                                history.push(params);
                                if history.len() > 100 {
                                    history.remove(0);
                                }
                            }
                            this.batch.develop.drafts.insert(path, next);
                            this.invalidate_library_preview();
                            this.library_schedule_save(cx);
                        }
                        Err(e) => this.batch.note = Some((e.to_string().into(), true)),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn library_develop_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = classic::palette(cx);
        let path = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone());
        let params = path
            .as_ref()
            .and_then(|p| self.batch.develop.current_params(p));
        let mut panel = div()
            .id("library-develop")
            .flex_none()
            .test_support()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(label(
                        if self.batch.develop.module_develop {
                            t!("library.develop.develop")
                        } else {
                            match self.batch.develop.inspector {
                                1 => t!("library.develop.metadata"),
                                2 => t!("library.develop.keywording"),
                                _ => t!("library.develop.quick_develop"),
                            }
                        },
                        &p,
                    ))
                    .child(mono(
                        if self.batch.develop.saving {
                            t!("library.develop.saving")
                        } else if self.batch.develop.dirty() {
                            t!("library.develop.unsaved_edits")
                        } else {
                            t!("library.develop.saved")
                        },
                        10.,
                        p.muted,
                    )),
            );
        if !self.batch.develop.module_develop && self.batch.recipe_browser {
            return panel.into_any_element();
        }
        let mut tabs = div().flex().gap_1();
        for (index, title) in [
            t!("library.develop.quick_develop"),
            t!("library.develop.metadata"),
            t!("library.develop.keywording"),
        ]
        .into_iter()
        .enumerate()
        {
            tabs = tabs.child(
                Button::new(("library-inspector-tab", index))
                    .label(title)
                    .small()
                    .ghost()
                    .selected(self.batch.develop.inspector == index)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.develop.inspector = index;
                        cx.notify();
                    })),
            );
        }
        if !self.batch.develop.module_develop || self.batch.develop.inspector != 0 {
            panel = panel.child(tabs);
        }
        if self.batch.develop.dirty() && !self.batch.develop.module_develop {
            panel = panel.child(
                Button::new("library-save-all-drafts")
                    .label(t!("library.develop.save_all_edits"))
                    .small()
                    .primary()
                    .disabled(self.batch.develop.saving)
                    .on_click(cx.listener(|this, _, _, cx| this.library_save_develop(false, cx))),
            );
        }
        if self.batch.develop.inspector != 0 {
            return panel
                .child(self.library_info_panel(self.batch.develop.inspector == 2, cx))
                .into_any_element();
        }
        let Some(params) = params.filter(|_| {
            path.as_ref()
                .is_some_and(|p| emulsion_io::photo_develop::supported(p))
        }) else {
            return panel
                .child(mono(
                    if self.batch.develop.busy {
                        t!("library.develop.loading_controls")
                    } else {
                        t!("library.develop.select_photo")
                    },
                    11.,
                    p.muted,
                ))
                .into_any_element();
        };
        let path = path.unwrap();
        if !self.batch.develop.presets_loaded {
            self.batch.develop.presets_loaded = true;
            cx.spawn(async move |this, cx| {
                let files = cx
                    .background_spawn(async { emulsion_io::lightroom_presets::installed() })
                    .await;
                this.update(cx, |this, cx| {
                    this.batch.develop.preset_files = files;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        if self.batch.develop.module_develop && !self.batch.develop.loupe {
            panel = panel.child(self.library_preset_bank(params, cx));
        }
        if params.process_version == 1 {
            panel = panel.child(
                Button::new("library-upgrade-process")
                    .label(t!("library.develop.upgrade_process"))
                    .small()
                    .outline()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.library_adjust(
                            DevelopParams {
                                process_version: 2,
                                ..params
                            },
                            cx,
                        )
                    })),
            );
        }

        panel = panel.child(
            div()
                .flex()
                .gap_1()
                .child(
                    Button::new("library-raw-auto")
                        .label(t!("library.develop.auto"))
                        .small()
                        .outline()
                        .disabled(self.batch.develop.busy || self.batch.develop.saving)
                        .on_click(cx.listener(|this, _, _, cx| this.library_auto(cx))),
                )
                .child(
                    Button::new("library-raw-bw")
                        .label(t!("library.develop.bw"))
                        .small()
                        .ghost()
                        .disabled(self.batch.develop.saving)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.library_adjust(
                                DevelopParams {
                                    saturation: -1.,
                                    ..params
                                },
                                cx,
                            )
                        })),
                )
                .child(
                    Button::new("library-raw-reset")
                        .label(t!("library.develop.reset"))
                        .small()
                        .ghost()
                        .disabled(self.batch.develop.saving)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.library_adjust(DevelopParams::default(), cx)
                        })),
                ),
        );
        if !self.batch.develop.module_develop {
            panel = panel.child(self.library_quick_presets(params, cx));
            panel = panel.child(self.library_preset_notes(
                &self.batch.develop.preset_import_notes,
                true,
                cx,
            ));
            if let Some(report) = &self.batch.develop.preset_report {
                panel = panel.child(self.library_preset_notes(&report.warnings, false, cx));
            }
            return panel.into_any_element();
        }
        panel = panel.child(self.library_profile_panel(params, cx));
        panel = panel.child(
            Button::new("library-raw-undo")
                .label(t!("library.develop.undo_adjustment"))
                .small()
                .ghost()
                .disabled(
                    self.batch.develop.saving
                        || !self
                            .batch
                            .develop
                            .history
                            .get(&path)
                            .is_some_and(|h| !h.is_empty()),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(path) = this
                        .batch
                        .current
                        .and_then(|i| this.batch.items.get(i))
                        .map(|i| i.path.clone())
                        && let Some(params) = this
                            .batch
                            .develop
                            .history
                            .get_mut(&path)
                            .and_then(|h| h.pop())
                    {
                        this.batch.develop.drafts.insert(path, params);
                        this.batch.develop.before = false;
                        this.invalidate_library_preview();
                        this.library_schedule_save(cx);
                        cx.notify();
                    }
                })),
        );
        for &(index, title) in &super::layout::SECTIONS {
            panel = panel.child(self.library_section_header(index, title, cx));
            if index == self.batch.develop.section {
                break;
            }
        }
        if self.batch.develop.section == 8 {
            return panel
                .child(self.library_enhance_panel(cx))
                .into_any_element();
        }
        if self.batch.develop.section == 7 {
            return panel
                .child(self.library_history_panel(path, params, cx))
                .into_any_element();
        }
        if self.batch.develop.section > 0 {
            return panel
                .child(self.library_advanced_panel(path, params, cx))
                .into_any_element();
        }
        let fields: [(SharedString, f32, f32, f32, f32); 17] = [
            (
                t!("library.develop.exposure").into(),
                params.exposure,
                -5.,
                5.,
                0.05,
            ),
            (
                t!("library.develop.contrast").into(),
                params.contrast,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.highlights").into(),
                -params.highlights,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.shadows").into(),
                params.shadows,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.blacks").into(),
                params.blacks,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.whites").into(),
                params.whites,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.temperature").into(),
                params.temperature,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.tint").into(),
                params.tint,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.saturation").into(),
                params.saturation,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.vibrance").into(),
                params.vibrance,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.texture").into(),
                params.texture,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.clarity").into(),
                params.clarity,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.dehaze").into(),
                params.dehaze,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.vignette").into(),
                params.vignette,
                -1.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.sharpening").into(),
                params.sharpening,
                0.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.noise_reduction").into(),
                params.noise_reduction,
                0.,
                1.,
                0.01,
            ),
            (
                t!("library.develop.sensor_denoise").into(),
                params.sensor_noise_reduction,
                0.,
                1.,
                0.01,
            ),
        ];
        if self.batch.develop.slider_key.as_ref() != Some(&(path.clone(), params)) {
            self.batch.develop.sliders.clear();
            for (index, (_, value, min, max, step)) in fields.iter().enumerate() {
                let slider = cx.new(|_| {
                    SliderState::new()
                        .min(*min)
                        .max(*max)
                        .step(*step)
                        .default_value(*value)
                });
                let sub = cx.subscribe(&slider, move |this, _, event, cx| {
                    if let SliderEvent::Release(_) = event {
                        this.batch.develop.gesture_active = false;
                        this.batch.develop.gesture_recorded = false;
                        return;
                    }
                    if let SliderEvent::Change(value) = event {
                        this.batch.develop.gesture_active = true;
                        let Some(path) = this
                            .batch
                            .current
                            .and_then(|i| this.batch.items.get(i))
                            .map(|i| i.path.clone())
                        else {
                            return;
                        };
                        let Some(mut params) = this.batch.develop.current_params(&path) else {
                            return;
                        };
                        let v = value.end();
                        match index {
                            0 => params.exposure = v,
                            1 => params.contrast = v,
                            2 => params.highlights = -v,
                            3 => params.shadows = v,
                            4 => params.blacks = v,
                            5 => params.whites = v,
                            6 => params.temperature = v,
                            7 => params.tint = v,
                            8 => params.saturation = v,
                            9 => params.vibrance = v,
                            10 => params.texture = v,
                            11 => params.clarity = v,
                            12 => params.dehaze = v,
                            13 => params.vignette = v,
                            14 => params.sharpening = v,
                            15 => params.noise_reduction = v,
                            _ => params.sensor_noise_reduction = v,
                        }
                        this.batch.develop.slider_key = Some((path, params));
                        this.library_adjust(params, cx);
                    }
                });
                self.batch.develop.sliders.push((slider, sub));
            }
            self.batch.develop.slider_key = Some((path.clone(), params));
        }
        for index in [6usize, 7, 0, 1, 2, 3, 5, 4, 10, 11, 12, 9, 8, 13] {
            let (name, value, min, max, step) = fields[index].clone();
            if [0, 6, 10, 13].contains(&index) {
                panel = panel.child(label(
                    match index {
                        0 => t!("library.develop.tone"),
                        6 => t!("library.develop.white_balance"),
                        10 => t!("library.develop.presence"),
                        _ => t!("library.develop.effects"),
                    },
                    &p,
                ));
            }
            panel = panel.child(
                div()
                    .id(("library-basic-row", index))
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(27.))
                    .child(
                        div()
                            .w(px(78.))
                            .flex_none()
                            .child(self.library_control_label(
                                index,
                                &name,
                                super::advanced::Field::Basic(index),
                                cx,
                            )),
                    )
                    .child(div().flex_1().min_w_0().child(
                        Slider::new(&self.batch.develop.sliders[index].0).disabled(
                            self.batch.develop.saving
                                || (index == 16
                                    && !emulsion_io::photo_develop::is_raw_photo(&path)),
                        ),
                    ))
                    .child(self.library_numeric_control(
                        index,
                        &name,
                        super::advanced::Field::Basic(index),
                        value,
                        min,
                        max,
                        step,
                        cx,
                    )),
            );
        }
        panel = panel
            .child(
                Button::new("library-white-balance-picker")
                    .label(t!("library.develop.pick_neutral_wb"))
                    .disabled(self.batch.develop.saving || self.batch.develop.busy)
                    .small()
                    .ghost()
                    .selected(self.batch.develop.canvas_tool == 11)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.batch.develop.canvas_tool = if this.batch.develop.canvas_tool == 11 {
                            0
                        } else {
                            11
                        };
                        this.batch.develop.before = false;
                        this.batch.develop.compare = false;
                        this.batch.develop.detail_region = None;
                        this.batch.note = Some((t!("library.develop.click_neutral").into(), false));
                        this.invalidate_library_preview();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("library-raw-as-shot")
                    .label(t!("library.develop.as_shot_wb"))
                    .disabled(self.batch.develop.saving)
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.library_adjust(
                            DevelopParams {
                                temperature: 0.,
                                tint: 0.,
                                wb_override: None,
                                kelvin: None,
                                ..params
                            },
                            cx,
                        )
                    })),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        Button::new("library-raw-before")
                            .label(t!("library.develop.before"))
                            .small()
                            .ghost()
                            .selected(self.batch.develop.before)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.batch.develop.before = !this.batch.develop.before;
                                this.invalidate_library_preview();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("library-raw-save")
                            .label(t!("library.develop.save_edits"))
                            .small()
                            .primary()
                            .disabled(!self.batch.develop.dirty() || self.batch.develop.saving)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.library_save_develop(false, cx)),
                            ),
                    ),
            )
            .child(
                Button::new("library-raw-sync")
                    .label(t!("library.develop.sync_selected"))
                    .small()
                    .outline()
                    .disabled(
                        self.batch.develop.saving
                            || self
                                .batch
                                .items
                                .iter()
                                .filter(|i| {
                                    i.selected && emulsion_io::photo_develop::supported(&i.path)
                                })
                                .count()
                                < 2,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.library_save_develop(true, cx))),
            );
        let mut groups = div().flex().flex_wrap().gap_1().child(mono(
            t!("library.develop.sync_label"),
            10.,
            p.muted,
        ));
        for (index, name, group) in [
            (
                0,
                t!("shell.raw_sync_all"),
                raw_settings::RawSettingsGroup::All,
            ),
            (
                1,
                t!("library.develop.sync_tone_effects"),
                raw_settings::RawSettingsGroup::Tone,
            ),
            (
                2,
                t!("library.develop.white_balance"),
                raw_settings::RawSettingsGroup::WhiteBalance,
            ),
            (
                3,
                t!("shell.raw_sync_curve"),
                raw_settings::RawSettingsGroup::Curve,
            ),
        ] {
            groups = groups.child(
                Button::new(("library-sync-group", index as usize))
                    .label(name)
                    .small()
                    .ghost()
                    .selected(self.batch.develop.sync_group == group)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.develop.sync_group = group;
                        cx.notify();
                    })),
            );
        }
        panel = panel.child(groups).child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    Button::new("library-save-preset")
                        .label(t!("library.develop.save_preset"))
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| this.library_preset_file(true, cx))),
                )
                .child(
                    Button::new("library-load-preset")
                        .label(t!("library.develop.import_lr_preset"))
                        .small()
                        .ghost()
                        .on_click(
                            cx.listener(|this, _, _, cx| this.library_preset_file(false, cx)),
                        ),
                )
                .child(
                    Button::new("library-reload-raw")
                        .label(t!("library.develop.reload_saved"))
                        .small()
                        .ghost()
                        .disabled(self.batch.develop.saving)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(path) = this
                                .batch
                                .current
                                .and_then(|i| this.batch.items.get(i))
                                .map(|i| i.path.clone())
                            {
                                this.batch.develop.drafts.remove(&path);
                                this.batch.develop.saved.remove(&path);
                                this.batch.develop.fingerprints.remove(&path);
                                this.batch.develop.history.remove(&path);
                                this.batch.develop.source = None;
                                this.invalidate_library_preview();
                                cx.notify();
                            }
                        })),
                ),
        );
        if let Some(report) = &self.batch.develop.preset_report {
            panel = panel
                .child(label(t!("library.develop.imported_preset"), &p))
                .child(mono(report.name.clone(), 11., p.ink));
            panel = panel.child(self.library_preset_notes(&report.warnings, false, cx));
        }
        if let Some(source) = self
            .batch
            .develop
            .source
            .as_ref()
            .filter(|s| s.source == path)
        {
            panel = panel.child(label(t!("window.info"), &p)).child(mono(
                format!(
                    "{} {} · {} × {}{}",
                    source.metadata.make,
                    source.metadata.model,
                    source.info.width,
                    source.info.height,
                    if source.is_proxy() {
                        format!(" · {}", t!("library.develop.offline_proxy"))
                    } else {
                        String::new()
                    }
                ),
                10.,
                p.muted,
            ));
        }
        panel.into_any_element()
    }
}

impl Workspace {
    pub(super) fn library_raw_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(path) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone())
        else {
            return;
        };
        let Some(mut params) = self.batch.develop.current_params(&path) else {
            self.batch.note = Some((t!("library.develop.select_photo_preset").into(), false));
            cx.notify();
            return;
        };
        if self.batch.develop.saving {
            return;
        }
        match index {
            0 => params = DevelopParams::default(),
            1 => {
                params.temperature = 0.15;
                params.shadows = 0.12;
                params.highlights = 0.2;
            }
            2 => {
                params.saturation = -1.;
                params.point_curves[0] = Default::default();
                params.tone_curve = DevelopParams::MEDIUM_CONTRAST_CURVE;
                params.smooth_curve = true;
            }
            _ => {
                params.point_curves[0] = Default::default();
                params.tone_curve = DevelopParams::STRONG_CONTRAST_CURVE;
                params.smooth_curve = true;
            }
        }
        self.library_adjust(params, cx);
    }
}

impl Workspace {
    pub(super) fn library_schedule_save(&mut self, cx: &mut Context<Self>) {
        self.batch.develop.save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(450))
                .await;
            this.update(cx, |this, cx| this.library_save_develop(false, cx))
                .ok();
        }));
    }
}

impl Workspace {
    pub(super) fn library_comparison_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = classic::palette(cx);
        let path = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| &i.path);
        let before = self
            .batch
            .develop
            .baseline_preview
            .as_ref()
            .filter(|(key, _)| Some(&key.path) == path)
            .map(|(_, image)| image.clone());
        let loading = before.is_none();
        let position = self
            .batch
            .develop
            .comparison_position
            .unwrap_or(0.5)
            .clamp(0.02, 0.98);
        let measured = self.batch.develop.comparison_bounds.clone();
        let navigation = self.batch.navigation.clone();
        div()
            .id("library-comparison")
            .test_support()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .child(
                canvas(
                    move |bounds, _, _| measured.set(Some(bounds)),
                    move |bounds, _, window, _| {
                        let clip = Bounds::new(
                            bounds.origin,
                            size(bounds.size.width * position, bounds.size.height),
                        );
                        // Both images share the canvas transform. Only the reveal
                        // mask moves when the divider is dragged.
                        let rect = navigation.borrow().image_bounds(&bounds);
                        window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                            window.paint_quad(fill(clip, p.stage));
                            if let Some(image) = before {
                                let _ = window.paint_image(
                                    rect,
                                    rect,
                                    Corners::default(),
                                    image,
                                    0,
                                    false,
                                );
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            )
            .when(loading, |d| {
                d.child(div().absolute().left_0().top_8().child(mono(
                    t!("library.develop.rendering_original"),
                    11.,
                    p.muted,
                )))
            })
            .child(
                div()
                    .absolute()
                    .top(px(8.))
                    .left(px(8.))
                    .bg(p.panel)
                    .px_2()
                    .child(mono(t!("library.develop.before"), 11., p.ink)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .bg(p.panel)
                    .px_2()
                    .child(mono(t!("library.develop.after"), 11., p.ink)),
            )
            .child(
                div()
                    .id("library-comparison-divider")
                    .test_support()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(position))
                    .ml(px(-8.))
                    .w(px(16.))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.batch.develop.comparison_dragging = true;
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                        if event.click_count() == 2 {
                            this.batch.develop.comparison_position = Some(0.5);
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(px(7.))
                            .top_0()
                            .bottom_0()
                            .w(px(2.))
                            .bg(p.accent),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(relative(0.5))
                            .left(px(-7.))
                            .px_2()
                            .py_1()
                            .bg(p.panel)
                            .border_1()
                            .border_color(p.accent)
                            .child("↔"),
                    ),
            )
            .into_any_element()
    }
}

impl Workspace {
    fn library_preset_file(&mut self, save: bool, cx: &mut Context<Self>) {
        let Some(path) = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .map(|i| i.path.clone())
        else {
            return;
        };
        let Some(params) = self.batch.develop.current_params(&path) else {
            return;
        };
        let pick = if save {
            let rx = cx.prompt_for_new_path(
                path.parent().unwrap_or(Path::new(".")),
                Some("settings.emulsion-preset.json"),
            );
            cx.spawn(async move |_, _| rx.await.ok().and_then(|r| r.ok()).flatten())
        } else {
            let rx = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(t!("library.develop.import_preset_prompt").into()),
            });
            cx.spawn(async move |_, _| {
                rx.await
                    .ok()
                    .and_then(|r| r.ok())
                    .flatten()
                    .and_then(|p| p.into_iter().next())
            })
        };
        cx.spawn(async move |this, cx| {
            let Some(file) = pick.await else { return };
            let result = cx
                .background_spawn(async move {
                    if save {
                        raw_settings::save_preset(params, &file).map(|_| None)
                    } else {
                        emulsion_io::lightroom_presets::load(&file, params).map(Some)
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Some(loaded)) => {
                        if this
                            .batch
                            .current
                            .and_then(|i| this.batch.items.get(i))
                            .map(|i| &i.path)
                            == Some(&path)
                            && this.batch.develop.current_params(&path) == Some(params)
                            && !this.batch.develop.saving
                        {
                            this.library_adjust(loaded.params, cx);
                            this.batch.develop.preset_report = Some(loaded);
                        } else {
                            this.batch.note =
                                Some((t!("library.develop.preset_not_applied").into(), false));
                        }
                    }
                    Ok(None) => {
                        this.batch.note =
                            Some((t!("library.develop.raw_preset_saved").into(), false))
                    }
                    Err(error) => this.batch.note = Some((error.to_string().into(), true)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

pub(super) fn rgb_histogram(rgba: &[u8]) -> [[u32; 32]; 3] {
    let mut bins = [[0; 32]; 3];
    for p in rgba.as_chunks::<4>().0 {
        if p[3] > 0 {
            for c in 0..3 {
                bins[c][p[c] as usize / 8] += 1;
            }
        }
    }
    bins
}
/// Input is display BGRA. This overlay is never written into saved edits/export.
pub(super) fn clipping_overlay(bgra: &mut [u8]) {
    for p in bgra.as_chunks_mut::<4>().0 {
        if p[3] == 0 {
            continue;
        }
        if p[..3].contains(&255) {
            p[..3].copy_from_slice(&[0, 0, 255]);
        } else if p[..3].iter().all(|v| *v == 0) {
            p[..3].copy_from_slice(&[255, 0, 0]);
        }
    }
}

impl Workspace {
    pub(crate) fn record_preset_import(
        &mut self,
        imported: usize,
        notes: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let mut seen = std::collections::BTreeSet::new();
        self.batch.develop.preset_import_notes = notes
            .into_iter()
            .filter(|s| seen.insert(s.clone()))
            .collect();
        self.batch.develop.preset_import_expanded = false;
        self.batch.note = Some((
            if imported == 0 {
                t!("library.develop.no_presets_imported").into()
            } else {
                crate::home::recency::plural(
                    imported,
                    "library.develop.imported_presets_one",
                    "library.develop.imported_presets_many",
                )
                .into()
            },
            imported == 0,
        ));
        cx.notify();
    }
    pub(super) fn library_preset_notes(
        &self,
        notes: &[String],
        bank: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = classic::palette(cx);
        let expanded = if bank {
            self.batch.develop.preset_import_expanded
        } else {
            self.batch.develop.preset_report_expanded
        };
        let id = if bank {
            "library-preset-import-details"
        } else {
            "library-preset-compatibility"
        };
        let mut panel = div().flex().flex_col().gap_1();
        if notes.is_empty() {
            return panel.into_any_element();
        }
        panel = panel.child(
            Button::new(id)
                .label(match (expanded, bank) {
                    (false, true) => t!("library.develop.show_import_details", count = notes.len()),
                    (true, true) => t!("library.develop.hide_import_details", count = notes.len()),
                    (false, false) => t!("library.develop.show_compat_notes", count = notes.len()),
                    (true, false) => t!("library.develop.hide_compat_notes", count = notes.len()),
                })
                .small()
                .ghost()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if bank {
                        this.batch.develop.preset_import_expanded =
                            !this.batch.develop.preset_import_expanded;
                    } else {
                        this.batch.develop.preset_report_expanded =
                            !this.batch.develop.preset_report_expanded;
                    }
                    cx.notify();
                })),
        );
        if expanded {
            let mut details = div()
                .id(if bank {
                    "library-preset-import-notes"
                } else {
                    "library-preset-applied-notes"
                })
                .max_h(px(160.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2();
            for note in notes {
                details = details.child(mono(note.clone(), 10., p.muted));
            }
            panel = panel.child(details.test_support());
        }
        panel.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn preset_bank_sorts_by_label_and_strips_only_install_hashes() {
        let files = [
            "/bank/ddfd697f6c1e-zeta.json",
            "/bank/0123456789ab-Alpha.xmp",
            "/bank/My Warm Portrait.json",
            "/bank/not-a-hash-prefix.xmp",
        ]
        .map(PathBuf::from);
        let names: Vec<_> = super::super::advanced::preset_entries(&files)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            names,
            ["Alpha", "My Warm Portrait", "not-a-hash-prefix", "zeta"]
        );
    }

    #[test]
    fn comparison_cache_tracks_geometry_and_resolution_but_not_tonal_edits() {
        let path = PathBuf::from("photo.dng");
        let params = DevelopParams {
            crop: [0.1, 0.2, 0.8, 0.9],
            rotation: 1,
            exposure: 1.,
            ..Default::default()
        };
        let key = ComparisonPreviewKey::new(path.clone(), params, None, false);
        assert_eq!(key.params.exposure, 0.);
        assert_eq!(
            key,
            ComparisonPreviewKey::new(
                path.clone(),
                DevelopParams {
                    exposure: 2.,
                    ..params
                },
                None,
                false
            )
        );
        assert_ne!(
            key,
            ComparisonPreviewKey::new(
                path.clone(),
                DevelopParams {
                    rotation: 2,
                    ..params
                },
                None,
                false
            )
        );
        assert_ne!(
            key,
            ComparisonPreviewKey::new(
                path.clone(),
                DevelopParams {
                    crop: [0., 0., 1., 1.],
                    ..params
                },
                None,
                false
            )
        );
        assert_ne!(
            key,
            ComparisonPreviewKey::new(path.clone(), params, None, true)
        );
        assert_ne!(
            key,
            ComparisonPreviewKey::new(path.clone(), params, Some([0.5, 0.5]), false)
        );
        assert_ne!(
            ComparisonPreviewKey::new(path.clone(), params, Some([0.2, 0.3]), false),
            ComparisonPreviewKey::new(path, params, Some([0.8, 0.7]), false)
        );
    }

    #[test]
    fn raw_comparison_matches_cropped_rotated_preview_and_detail_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("comparison.dng");
        crate::raw_test_fixture::write_dng(&path);
        let source = RawSource::load(&path).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let params = DevelopParams {
            crop: [0.1, 0.2, 0.8, 0.9],
            rotation: 1,
            ..Default::default()
        };
        for (detail, full) in [(None, false), (None, true), (Some([0.3, 0.7]), false)] {
            let key = ComparisonPreviewKey::new(path.clone(), params, detail, full);
            let before =
                develop_preview_pixels(&source, &key.params, detail, full, &cancel).unwrap();
            let after = develop_preview_pixels(&source, &params, detail, full, &cancel).unwrap();
            assert_eq!(
                before, after,
                "geometry-only edits must line up pixel for pixel"
            );
            let unframed =
                develop_preview_pixels(&source, &DevelopParams::default(), detail, full, &cancel)
                    .unwrap();
            assert_ne!((before.0, before.1), (unframed.0, unframed.1));
            let edited = develop_preview_pixels(
                &source,
                &DevelopParams {
                    exposure: 1.,
                    ..params
                },
                detail,
                full,
                &cancel,
            )
            .unwrap();
            assert_eq!((before.0, before.1), (edited.0, edited.1));
            assert_ne!(before.2, edited.2);
        }
    }

    #[test]
    fn luminance_histogram_counts_opaque_pixels_in_correct_bins() {
        let bins = histogram(&[
            0, 0, 0, 255, 255, 255, 255, 255, 128, 128, 128, 255, 255, 0, 0, 0,
        ]);
        assert_eq!(bins.iter().sum::<u32>(), 3);
        assert_eq!((bins[0], bins[16], bins[31]), (1, 1, 1));
    }
    #[test]
    fn returning_from_photo_reloads_saved_settings_but_keeps_unsaved_drafts() {
        let mut state = Develop::default();
        let clean = PathBuf::from("saved.dng");
        let dirty = PathBuf::from("pending.dng");
        let original = DevelopParams::default();
        let edited = DevelopParams {
            exposure: 1.,
            ..original
        };
        state.saved.insert(clean.clone(), original);
        state.drafts.insert(clean.clone(), original);
        state.saved.insert(dirty.clone(), original);
        state.drafts.insert(dirty.clone(), edited);
        state.refresh_saved();
        assert_eq!(state.current_params(&clean), None);
        assert_eq!(state.current_params(&dirty), Some(edited));
        assert_eq!(state.saved.get(&dirty), Some(&original));
        assert!(state.dirty());
    }
    #[test]
    fn drafts_remain_dirty_until_the_exact_settings_are_saved() {
        let path = PathBuf::from("photo.dng");
        let mut state = Develop::default();
        let baseline = DevelopParams::default();
        state.saved.insert(path.clone(), baseline);
        state.drafts.insert(
            path.clone(),
            DevelopParams {
                exposure: 1.,
                ..baseline
            },
        );
        assert!(state.dirty());
        state.saved.insert(path.clone(), state.drafts[&path]);
        assert!(!state.dirty());
        state.drafts.insert(path, baseline);
        assert!(state.dirty(), "undoing a saved edit must also be saved");
    }
}
