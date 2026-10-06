//! Prospective support checks for the future native Smart mapping cutover.
//!
//! This facade is deliberately unused by the model, commands and IO. Metadata
//! preparation allocates no pixels or heap storage. Present projective masks
//! require an O(output-pixels) certification pass, cancellable at tile boundaries.
//! Source/cache bounds never request a world-AABB allocation or certify a bake
//! canvas. Callers must separately admit any document-bounded render destination.
//!
//! Certificates prove geometry, resource dimensions and individual buffer
//! layouts, not pixel provenance, peak memory, filter output or worker identity.
//! Keep asynchronous source/filter identity checks. Recheck metadata after
//! decoding resources and validate the actual raw cache before publication.

use crate::MaskProperties;
use crate::mapping::{Mapping2, SmartPlacement};
use crate::mask_properties::{MaskProcessingError, MaskProcessingLayout};
use emulsion_filters::{Filter, FilterStyle, StackFootprint, stack_footprint};
use emulsion_raster::projective::ProjectiveError;
use emulsion_raster::projective_mask_sample::{
    MaskGridPlan, MaskOutputGrid, MaskSampleError, prepare_mask_grid_with_control,
};
use emulsion_raster::projective_sample::{ProjectivePixelError, ProjectivePixelMapping};
use emulsion_raster::{Mask, Placement};
use thiserror::Error;

const MAX_FILTERS: usize = 32;

/// The single effective grid: predicted active cache, or source at zero origin.
/// A retained inactive raw cache does not alter this grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SmartOutputGrid {
    pub size: (u32, u32),
    pub offset: (i32, i32),
}

impl From<SmartOutputGrid> for MaskOutputGrid {
    fn from(grid: SmartOutputGrid) -> Self {
        Self {
            size: grid.size,
            offset: grid.offset,
        }
    }
}

/// Variant-sensitive derived-cache identity; never serialize as authored state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MappingKey {
    Affine([u64; 6]),
    Projective([u64; 9]),
}

impl From<Mapping2> for MappingKey {
    fn from(map: Mapping2) -> Self {
        match map {
            Mapping2::Affine(affine) => Self::Affine(affine.to_cols_array().map(f64::to_bits)),
            Mapping2::Projective(projective) => {
                Self::Projective(projective.to_row_major().map(f64::to_bits))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SmartPlacementKey {
    Legacy { numbers: [u64; 5], flips: [bool; 2] },
    Projective([u64; 9]),
}

impl From<SmartPlacement> for SmartPlacementKey {
    fn from(placement: SmartPlacement) -> Self {
        match placement {
            SmartPlacement::Legacy(p) => Self::Legacy {
                numbers: [p.x, p.y, p.scale_x, p.scale_y, p.rotation].map(f64::to_bits),
                flips: [p.flip_x, p.flip_y],
            },
            SmartPlacement::Projective(p) => Self::Projective(p.to_row_major().map(f64::to_bits)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProjectiveFeatures {
    pub placement: bool,
    pub raster_mask: bool,
    pub filter_mask: bool,
}

impl ProjectiveFeatures {
    pub fn any(self) -> bool {
        self.placement || self.raster_mask || self.filter_mask
    }
}

/// `has_detail` is the actual tile-presence fact used by intrinsic processing,
/// not a visibility, enabled, fill-color or nonzero-density approximation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaskPlaneMetadata {
    pub size: (u32, u32),
    pub has_detail: bool,
}

impl MaskPlaneMetadata {
    pub fn from_mask(mask: &Mask) -> Self {
        Self {
            size: (mask.width(), mask.height()),
            has_detail: mask.tile_count() > 0,
        }
    }
}

/// Raster metadata exists even with no plane. A filter descriptor must have a
/// plane; absence of the descriptor is represented by `filter_mask: None`.
#[derive(Clone, Copy, Debug)]
pub struct ComponentMaskMetadata {
    pub plane: Option<MaskPlaneMetadata>,
    pub transform: Mapping2,
    pub properties: MaskProperties,
    pub enabled: bool,
    pub linked: bool,
}

impl Default for ComponentMaskMetadata {
    fn default() -> Self {
        Self {
            plane: None,
            transform: Mapping2::IDENTITY,
            properties: MaskProperties::default(),
            enabled: true,
            linked: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SmartSupportMetadata<'a> {
    pub source_size: (u32, u32),
    pub placement: SmartPlacement,
    pub filters: &'a [Filter],
    pub styles: &'a [FilterStyle],
    pub filters_enabled: bool,
    /// Existing resource, if one is already available. A prospective filter
    /// change may retain an old, differently sized cache until publication.
    pub retained_cache: Option<SmartOutputGrid>,
    pub raster_mask: ComponentMaskMetadata,
    pub filter_mask: Option<ComponentMaskMetadata>,
    /// Includes disabled and empty descriptors.
    pub has_vector_mask: bool,
}

impl SmartSupportMetadata<'_> {
    pub fn features(self) -> ProjectiveFeatures {
        ProjectiveFeatures {
            placement: matches!(self.placement, SmartPlacement::Projective(_)),
            raster_mask: matches!(self.raster_mask.transform, Mapping2::Projective(_)),
            filter_mask: self
                .filter_mask
                .is_some_and(|mask| matches!(mask.transform, Mapping2::Projective(_))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaskComponent {
    Raster,
    Filter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SupportResource {
    Source,
    RetainedCache,
    EffectiveGrid,
    ActualCache,
    RawMask(MaskComponent),
    ProcessedMask(MaskComponent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelDomain {
    Source,
    EffectiveGrid,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum SmartSupportError {
    #[error("{resource:?} dimensions {size:?} exceed positive 30,000-side / 400-MP limits")]
    Resource {
        resource: SupportResource,
        size: (u32, u32),
    },
    #[error("{0:?} buffer layout overflows")]
    Layout(SupportResource),
    #[error("projective Smart support allows at most 32 filters")]
    FilterCount,
    #[error("Smart filter styles cannot outnumber their stages")]
    OrphanFilterStyles,
    #[error("a vector-mask descriptor cannot coexist with retained projective Smart metadata")]
    VectorMaskConflict,
    #[error("a Smart Filter mask descriptor requires its raw plane")]
    MissingFilterMaskPlane,
    #[error("invalid legacy Smart placement")]
    LegacyPlacement,
    #[error("invalid legacy {0:?} mask affine")]
    AffineMask(MaskComponent),
    #[error("invalid {component:?} mask processing metadata: {source}")]
    MaskProcessing {
        component: MaskComponent,
        source: MaskProcessingError,
    },
    #[error("invalid {component:?} projective mask representation: {source}")]
    MaskMapping {
        component: MaskComponent,
        source: ProjectiveError,
    },
    #[error("unsafe {domain:?} projective pixel support: {source}")]
    Pixels {
        domain: PixelDomain,
        source: ProjectivePixelError,
    },
    #[error("unsafe {component:?} projective mask sampling: {source}")]
    MaskSampling {
        component: MaskComponent,
        source: MaskSampleError,
    },
    #[error("Smart support preparation was cancelled")]
    Cancelled,
    #[error("actual Smart cache {actual:?} does not match predicted active grid {expected:?}")]
    ActualCacheMismatch {
        expected: SmartOutputGrid,
        actual: SmartOutputGrid,
    },
}

/// This is a route selection, not a successful certificate for legacy nodes.
/// The caller must retain its existing Legacy/Affine admission unchanged.
// Metadata admission is allocation-free: the bounded proof travels by value,
// including when preparing a candidate before decoding or allocating pixels.
#[expect(
    clippy::large_enum_variant,
    reason = "metadata preflight must not allocate a boxed support certificate"
)]
#[derive(Clone, Debug)]
pub enum SmartSupportPreflight {
    LegacyCompatibility,
    Projective(SmartSupport),
}

/// Only footprint-relevant stack inputs. Different pixels/opacity/blend and
/// filters with the same sanitized spread may share this support key; they may
/// never share a rendered cache on this evidence alone. Stage positions,
/// disabled spread, missing styles and the root switch remain explicit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StackSupportKey {
    enabled: bool,
    style_count: usize,
    stages: [Option<(bool, i32)>; MAX_FILTERS],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComponentSupportKey {
    pub plane: Option<MaskPlaneMetadata>,
    pub mapping: MappingKey,
    pub properties: [u32; 2],
    pub enabled: bool,
    pub linked: bool,
}

impl From<ComponentMaskMetadata> for ComponentSupportKey {
    fn from(mask: ComponentMaskMetadata) -> Self {
        Self {
            plane: mask.plane,
            mapping: mask.transform.into(),
            properties: [
                mask.properties.density.to_bits(),
                mask.properties.feather.to_bits(),
            ],
            enabled: mask.enabled,
            linked: mask.linked,
        }
    }
}

/// Exact stored mapping/placement/property bits plus all resource and stack
/// inputs used by these support checks. No pixel or resource-content identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SmartSupportKey {
    pub source_size: (u32, u32),
    pub placement: SmartPlacementKey,
    pub stack: StackSupportKey,
    pub retained_cache: Option<SmartOutputGrid>,
    pub raster_mask: ComponentSupportKey,
    pub filter_mask: Option<ComponentSupportKey>,
    pub has_vector_mask: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterBufferLayouts {
    /// One dense linear RGBA source buffer.
    pub source_bytes: usize,
    /// One dense linear RGBA output/intermediate buffer, not peak memory.
    pub output_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct ComponentSupport {
    key: ComponentSupportKey,
    processing: Option<MaskProcessingLayout>,
    projective_plan: Option<MaskGridPlan>,
}

impl ComponentSupport {
    pub fn key(&self) -> ComponentSupportKey {
        self.key
    }
    pub fn processing(&self) -> Option<MaskProcessingLayout> {
        self.processing
    }
    /// None means an absent plane or the existing affine sampling route.
    pub fn projective_plan(&self) -> Option<&MaskGridPlan> {
        self.projective_plan.as_ref()
    }
}

/// Legacy mixed states retain the original cache-placement arithmetic.
// Consumers copy this checked mapping out of a proof. Boxing would remove Copy
// and add allocation to the metadata-only support path.
#[expect(
    clippy::large_enum_variant,
    reason = "preserve the allocation-free Copy pixel-support certificate"
)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectivePixelSupport {
    Legacy(Placement),
    Projective(ProjectivePixelMapping),
}

/// Prepared geometry only. Actual resource publication requires the separate
/// consuming `validate_actual_cache` step, even when the predicted grid is zero
/// spread or bypassed. Private fields prevent forging either certificate.
#[derive(Clone, Debug)]
pub struct SmartSupport {
    key: SmartSupportKey,
    features: ProjectiveFeatures,
    footprint: StackFootprint,
    output: SmartOutputGrid,
    filter_buffers: Option<FilterBufferLayouts>,
    source_projective: Option<ProjectivePixelMapping>,
    effective_pixels: EffectivePixelSupport,
    raster_mask: ComponentSupport,
    filter_mask: Option<ComponentSupport>,
}

impl SmartSupport {
    pub fn key(&self) -> SmartSupportKey {
        self.key
    }
    pub fn features(&self) -> ProjectiveFeatures {
        self.features
    }
    pub fn output_grid(&self) -> SmartOutputGrid {
        self.output
    }
    pub fn footprint(&self) -> StackFootprint {
        self.footprint
    }
    pub fn filter_buffers(&self) -> Option<FilterBufferLayouts> {
        self.filter_buffers
    }
    pub fn source_projective(&self) -> Option<ProjectivePixelMapping> {
        self.source_projective
    }
    pub fn effective_pixels(&self) -> EffectivePixelSupport {
        self.effective_pixels
    }
    pub fn raster_mask(&self) -> &ComponentSupport {
        &self.raster_mask
    }
    pub fn filter_mask(&self) -> Option<&ComponentSupport> {
        self.filter_mask.as_ref()
    }

    /// Cheap, allocation-free guard against reusing a plan after its support
    /// metadata changed. Rebuild it on false; retain independent content checks.
    pub fn matches_metadata(&self, metadata: SmartSupportMetadata<'_>) -> bool {
        support_key(metadata).is_ok_and(|key| key == self.key)
    }
}

#[derive(Clone, Debug)]
pub struct ValidatedSmartSupport {
    support: SmartSupport,
    actual_cache: SmartOutputGrid,
}

impl ValidatedSmartSupport {
    pub fn support(&self) -> &SmartSupport {
        &self.support
    }
    pub fn actual_cache(&self) -> SmartOutputGrid {
        self.actual_cache
    }
}

/// Borrow exact authored fields; this never materializes retained resources.
pub fn metadata_for_node(
    node: &crate::Node,
) -> Result<SmartSupportMetadata<'_>, crate::GeometryError> {
    let crate::NodeKind::Smart {
        source,
        placement,
        filters,
        filter_styles,
        filters_enabled,
        cache,
        offset,
        filter_mask,
        ..
    } = &node.kind
    else {
        return Err(crate::GeometryError::NotSmart);
    };
    Ok(SmartSupportMetadata {
        source_size: (source.width(), source.height()),
        placement: *placement,
        filters,
        styles: filter_styles,
        filters_enabled: *filters_enabled,
        retained_cache: Some(SmartOutputGrid {
            size: (cache.width(), cache.height()),
            offset: *offset,
        }),
        raster_mask: ComponentMaskMetadata {
            plane: node
                .mask
                .as_ref()
                .map(|mask| MaskPlaneMetadata::from_mask(mask)),
            transform: node.mask_transform,
            properties: node.mask_properties,
            enabled: node.mask_enabled,
            linked: node.mask_linked,
        },
        filter_mask: filter_mask.as_ref().map(|mask| ComponentMaskMetadata {
            plane: Some(MaskPlaneMetadata::from_mask(&mask.pixels)),
            transform: mask.transform,
            properties: mask.properties,
            enabled: mask.enabled,
            linked: mask.linked,
        }),
        has_vector_mask: node.vector_mask.is_some(),
    })
}

/// Complete geometry/resource admission, including the actual raw cache.
/// Pure Legacy/Affine nodes deliberately retain their old validation route.
pub fn validate_node(node: &crate::Node) -> Result<SmartSupportPreflight, crate::GeometryError> {
    if !matches!(node.kind, crate::NodeKind::Smart { .. }) {
        if node.has_projective_metadata() {
            return Err(crate::GeometryError::Unsupported {
                operation: "component mapping",
                reason: "projective component mappings require a Smart owner",
            });
        }
        return Ok(SmartSupportPreflight::LegacyCompatibility);
    }
    let metadata = metadata_for_node(node)?;
    if !metadata.features().any() {
        return Ok(SmartSupportPreflight::LegacyCompatibility);
    }
    let key = support_key(metadata)?;
    static SUPPORTS: std::sync::OnceLock<parking_lot::Mutex<Vec<(SmartSupportKey, SmartSupport)>>> =
        std::sync::OnceLock::new();
    let supports = SUPPORTS.get_or_init(|| parking_lot::Mutex::new(Vec::new()));
    if let Some((_, support)) = supports.lock().iter().find(|(stored, _)| *stored == key) {
        return Ok(SmartSupportPreflight::Projective(support.clone()));
    }
    // Certification and processing never run while holding the shared lock.
    let prepared = preflight_stack_support(metadata)?;
    if let SmartSupportPreflight::Projective(support) = &prepared {
        validate_actual_cache(
            support.clone(),
            metadata
                .retained_cache
                .expect("Smart metadata includes its raw cache"),
        )?;
        let mut entries = supports.lock();
        if entries.len() >= 32 {
            entries.remove(0);
        }
        entries.push((key, support.clone()));
    }
    Ok(prepared)
}

/// One output grid for pixels, coverage, inspection and filter mixing.
/// This is the actual active cache; support validation separately checks its
/// agreement with the prospective footprint.
pub fn output_grid(node: &crate::Node) -> Result<SmartOutputGrid, crate::GeometryError> {
    let metadata = metadata_for_node(node)?;
    if crate::smart::has_active_filters(metadata.filters, metadata.styles, metadata.filters_enabled)
    {
        Ok(metadata
            .retained_cache
            .expect("Smart metadata includes its raw cache"))
    } else {
        Ok(SmartOutputGrid {
            size: metadata.source_size,
            offset: (0, 0),
        })
    }
}

pub fn preflight_stack_support(
    metadata: SmartSupportMetadata<'_>,
) -> Result<SmartSupportPreflight, SmartSupportError> {
    preflight_stack_support_with_control(metadata, || true)
}

pub fn preflight_stack_support_with_control(
    metadata: SmartSupportMetadata<'_>,
    keep_going: impl FnMut() -> bool,
) -> Result<SmartSupportPreflight, SmartSupportError> {
    prepare_support(metadata, keep_going, true)
}

/// Before mask decoding, validate available source/cache/map/resource metadata.
/// Tile presence and the processed footprint are deliberately not guessed.
/// This returns no publishable certificate: exact planes must pass the full
/// preflight before filtering or document publication.
pub fn preflight_pending_mask_resources(
    mut metadata: SmartSupportMetadata<'_>,
) -> Result<(), SmartSupportError> {
    if let Some(plane) = &mut metadata.raster_mask.plane {
        plane.has_detail = false;
    }
    if let Some(mask) = &mut metadata.filter_mask
        && let Some(plane) = &mut mask.plane
    {
        plane.has_detail = false;
    }
    prepare_support(metadata, || true, false)?;
    Ok(())
}

fn prepare_support(
    metadata: SmartSupportMetadata<'_>,
    mut keep_going: impl FnMut() -> bool,
    certify_masks: bool,
) -> Result<SmartSupportPreflight, SmartSupportError> {
    let features = metadata.features();
    if !features.any() {
        return Ok(SmartSupportPreflight::LegacyCompatibility);
    }
    if !keep_going() {
        return Err(SmartSupportError::Cancelled);
    }
    if metadata.has_vector_mask {
        return Err(SmartSupportError::VectorMaskConflict);
    }
    let key = support_key(metadata)?;
    resource::<[u16; 4]>(metadata.source_size, SupportResource::Source)?;
    if let Some(cache) = metadata.retained_cache {
        resource::<[u16; 4]>(cache.size, SupportResource::RetainedCache)?;
    }
    let footprint = stack_footprint(
        metadata.source_size,
        metadata.filters,
        metadata.styles,
        metadata.filters_enabled,
    );
    let output = SmartOutputGrid {
        size: footprint
            .checked_raster_size()
            .ok_or(SmartSupportError::Layout(SupportResource::EffectiveGrid))?,
        offset: footprint.offset,
    };
    resource::<[u16; 4]>(output.size, SupportResource::EffectiveGrid)?;
    let filter_buffers = if footprint.active {
        Some(FilterBufferLayouts {
            source_bytes: dense_bytes::<[f32; 4]>(metadata.source_size, SupportResource::Source)?,
            output_bytes: footprint
                .checked_dense_bytes()
                .ok_or(SmartSupportError::Layout(SupportResource::EffectiveGrid))?,
        })
    } else {
        None
    };
    // Finish all cheap resource/descriptor checks before either component's
    // potentially long output-center certification pass.
    let mut raster_mask = prepare_component_metadata(metadata.raster_mask, MaskComponent::Raster)?;
    let mut filter_mask = metadata
        .filter_mask
        .map(|mask| {
            if mask.plane.is_none() {
                return Err(SmartSupportError::MissingFilterMaskPlane);
            }
            prepare_component_metadata(mask, MaskComponent::Filter)
        })
        .transpose()?;
    let (source_projective, effective_pixels) = match metadata.placement {
        SmartPlacement::Legacy(placement) => {
            validate_legacy_placement(placement)?;
            let cache_placement = crate::smart::cache_placement(
                &placement,
                metadata.source_size,
                output.size,
                output.offset,
            );
            validate_legacy_placement(cache_placement)?;
            (None, EffectivePixelSupport::Legacy(cache_placement))
        }
        SmartPlacement::Projective(projective) => {
            let source = ProjectivePixelMapping::new(
                metadata.source_size.0,
                metadata.source_size.1,
                projective,
            )
            .map_err(|source| SmartSupportError::Pixels {
                domain: PixelDomain::Source,
                source,
            })?;
            let effective = if output.size == metadata.source_size && output.offset == (0, 0) {
                source
            } else {
                let cache_map = Mapping2::Projective(projective)
                    .with_source_offset(output.offset)
                    .and_then(Mapping2::to_projective)
                    .map_err(|source| SmartSupportError::Pixels {
                        domain: PixelDomain::EffectiveGrid,
                        source: source.into(),
                    })?;
                ProjectivePixelMapping::new(output.size.0, output.size.1, cache_map).map_err(
                    |source| SmartSupportError::Pixels {
                        domain: PixelDomain::EffectiveGrid,
                        source,
                    },
                )?
            };
            (Some(source), EffectivePixelSupport::Projective(effective))
        }
    };
    if certify_masks {
        certify_component_grid(
            &mut raster_mask,
            metadata.raster_mask.transform,
            MaskComponent::Raster,
            output,
            &mut keep_going,
        )?;
        if let (Some(support), Some(mask)) = (&mut filter_mask, metadata.filter_mask) {
            certify_component_grid(
                support,
                mask.transform,
                MaskComponent::Filter,
                output,
                &mut keep_going,
            )?;
        }
    }
    if !keep_going() {
        return Err(SmartSupportError::Cancelled);
    }
    Ok(SmartSupportPreflight::Projective(SmartSupport {
        key,
        features,
        footprint,
        output,
        filter_buffers,
        source_projective,
        effective_pixels,
        raster_mask,
        filter_mask,
    }))
}

/// Validate the actual raw cache immediately before publishing. During bypass
/// it may remain expanded and displaced, but is still independently resource
/// checked. It does not replace the certified source/zero effective grid.
pub fn validate_actual_cache(
    mut support: SmartSupport,
    actual: SmartOutputGrid,
) -> Result<ValidatedSmartSupport, SmartSupportError> {
    resource::<[u16; 4]>(actual.size, SupportResource::ActualCache)?;
    if support.footprint.active && actual != support.output {
        return Err(SmartSupportError::ActualCacheMismatch {
            expected: support.output,
            actual,
        });
    }
    // The new publication certificate now proves this retained resource, not
    // the possibly obsolete cache admitted during prospective preparation.
    support.key.retained_cache = Some(actual);
    Ok(ValidatedSmartSupport {
        support,
        actual_cache: actual,
    })
}

fn support_key(metadata: SmartSupportMetadata<'_>) -> Result<SmartSupportKey, SmartSupportError> {
    if metadata.filters.len() > MAX_FILTERS {
        return Err(SmartSupportError::FilterCount);
    }
    if metadata.styles.len() > metadata.filters.len() {
        return Err(SmartSupportError::OrphanFilterStyles);
    }
    let mut stages = [None; MAX_FILTERS];
    for (i, filter) in metadata.filters.iter().enumerate() {
        stages[i] = Some((
            metadata.styles.get(i).is_none_or(|style| style.enabled),
            filter.sanitized().spread(),
        ));
    }
    Ok(SmartSupportKey {
        source_size: metadata.source_size,
        placement: metadata.placement.into(),
        stack: StackSupportKey {
            enabled: metadata.filters_enabled,
            style_count: metadata.styles.len(),
            stages,
        },
        retained_cache: metadata.retained_cache,
        raster_mask: metadata.raster_mask.into(),
        filter_mask: metadata.filter_mask.map(Into::into),
        has_vector_mask: metadata.has_vector_mask,
    })
}

fn prepare_component_metadata(
    metadata: ComponentMaskMetadata,
    component: MaskComponent,
) -> Result<ComponentSupport, SmartSupportError> {
    if !metadata.properties.valid() {
        return Err(SmartSupportError::MaskProcessing {
            component,
            source: MaskProcessingError::Properties,
        });
    }
    match metadata.transform {
        Mapping2::Affine(affine) => {
            let valid = match component {
                MaskComponent::Raster => {
                    // Match legacy Document validation exactly. Finite input
                    // coefficients can overflow the determinant to infinity or
                    // unordered NaN; neither was classified as below threshold.
                    affine.is_finite()
                        && affine.matrix2.determinant().abs().partial_cmp(&1e-12)
                            != Some(std::cmp::Ordering::Less)
                }
                MaskComponent::Filter => {
                    crate::vector_mask::valid_transform(affine.to_cols_array())
                }
            };
            if !valid {
                return Err(SmartSupportError::AffineMask(component));
            }
        }
        Mapping2::Projective(projective) => {
            projective
                .inverse()
                .map_err(|source| SmartSupportError::MaskMapping { component, source })?;
        }
    }
    let Some(plane) = metadata.plane else {
        // Latent C has representation/inverse admission, no invented plane,
        // intrinsic forward frame, processed support or mask sampling plan.
        return Ok(ComponentSupport {
            key: metadata.into(),
            processing: None,
            projective_plan: None,
        });
    };
    resource::<u8>(plane.size, SupportResource::RawMask(component))?;
    let processing = metadata
        .properties
        .checked_processing_layout(plane.size, plane.has_detail)
        .map_err(|source| SmartSupportError::MaskProcessing { component, source })?;
    resource::<u8>(processing.size, SupportResource::ProcessedMask(component))?;
    Ok(ComponentSupport {
        key: metadata.into(),
        processing: Some(processing),
        projective_plan: None,
    })
}

fn certify_component_grid(
    support: &mut ComponentSupport,
    transform: Mapping2,
    component: MaskComponent,
    output: SmartOutputGrid,
    keep_going: &mut impl FnMut() -> bool,
) -> Result<(), SmartSupportError> {
    if !keep_going() {
        return Err(SmartSupportError::Cancelled);
    }
    if let (Mapping2::Projective(projective), Some(plane), Some(processing)) =
        (transform, support.key.plane, support.processing)
    {
        support.projective_plan = Some(
            prepare_mask_grid_with_control(
                projective,
                plane.size,
                processing.halo,
                output.into(),
                keep_going,
            )
            .map_err(|source| match source {
                MaskSampleError::Cancelled => SmartSupportError::Cancelled,
                _ => SmartSupportError::MaskSampling { component, source },
            })?,
        );
    }
    Ok(())
}

fn validate_legacy_placement(placement: Placement) -> Result<(), SmartSupportError> {
    // Document's existing admission, not Mapping2::validate_for_operation or
    // ProjectivePixelMapping promotion. Retain legacy arithmetic exactly.
    if ![
        placement.x,
        placement.y,
        placement.scale_x,
        placement.scale_y,
        placement.rotation,
    ]
    .into_iter()
    .all(f64::is_finite)
        || placement.scale_x.abs() < 1e-6
        || placement.scale_y.abs() < 1e-6
    {
        Err(SmartSupportError::LegacyPlacement)
    } else {
        Ok(())
    }
}

fn resource<T>(size: (u32, u32), resource: SupportResource) -> Result<usize, SmartSupportError> {
    if size.0 == 0
        || size.1 == 0
        || size.0 > crate::document::MAX_SIDE
        || size.1 > crate::document::MAX_SIDE
        || u64::from(size.0) * u64::from(size.1) > crate::document::MAX_PIXELS
    {
        return Err(SmartSupportError::Resource { resource, size });
    }
    dense_bytes::<T>(size, resource)
}

fn dense_bytes<T>(size: (u32, u32), resource: SupportResource) -> Result<usize, SmartSupportError> {
    let count = usize::try_from(u64::from(size.0) * u64::from(size.1))
        .map_err(|_| SmartSupportError::Layout(resource))?;
    std::alloc::Layout::array::<T>(count)
        .map(|layout| layout.size())
        .map_err(|_| SmartSupportError::Layout(resource))
}

#[cfg(test)]
#[path = "smart_support_tests.rs"]
mod tests;
