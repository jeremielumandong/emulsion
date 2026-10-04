//! A display-only grayscale view of the active layer mask.
use super::*;
use emulsion_raster::Mask;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};

#[derive(Default)]
pub(crate) struct MaskViewState {
    pub target: Option<(NodeId, MaskEditTarget)>,
    pub cache: Rc<MaskViewCache>,
}

#[derive(Default)]
pub(crate) struct MaskViewCache(RefCell<Option<(u64, Arc<RenderImage>)>>);

impl MaskViewCache {
    pub(super) fn release(&self, window: &mut Window) {
        if let Some((_, image)) = self.0.borrow_mut().take() {
            let _ = window.drop_image(image);
        }
    }
}

pub(super) struct MaskView {
    mask: Arc<Mask>,
    from_doc: glam::DAffine2,
    doc_size: (u32, u32),
}

/// Effective coverage already contains the mask affine. Smart coverage lives
/// in the filtered cache grid rather than the raw source grid.
pub(super) fn mask_inspection_to_document(node: &Node) -> glam::DAffine2 {
    match &node.kind {
        NodeKind::Raster { raster, placement } => placement.to_doc(raster.width(), raster.height()),
        NodeKind::Smart {
            source,
            placement,
            cache,
            offset,
            ..
        } => emulsion_core::smart::cache_placement(
            placement,
            (source.width(), source.height()),
            (cache.width(), cache.height()),
            *offset,
        )
        .to_doc(cache.width(), cache.height()),
        _ => glam::DAffine2::IDENTITY,
    }
}

impl EditorView {
    pub(super) fn mask_view_snapshot(&self) -> Option<MaskView> {
        let (id, target) = self.mask_view.target.filter(|(id, target)| {
            Some(*id) == self.selected && *target == self.tools.mask_edit_target
        })?;
        let node = self.editor.doc.node(id)?;
        let mask = target.inspection(&self.editor.doc, node)?;
        let to_doc = mask_inspection_to_document(node);
        if to_doc.matrix2.determinant().abs() < f64::EPSILON {
            return None;
        }
        Some(MaskView {
            mask,
            from_doc: to_doc.inverse(),
            doc_size: (self.editor.doc.width, self.editor.doc.height),
        })
    }
}

impl MaskView {
    fn pixel(&self, x: f64, y: f64) -> [u8; 4] {
        if x < 0. || y < 0. || x >= self.doc_size.0 as f64 || y >= self.doc_size.1 as f64 {
            return [0; 4];
        }
        let p = self.from_doc.transform_point2(glam::dvec2(x, y));
        let value = if p.x < 0.
            || p.y < 0.
            || p.x >= self.mask.width() as f64
            || p.y >= self.mask.height() as f64
        {
            self.mask.fill()
        } else {
            self.mask.get(p.x as u32, p.y as u32)
        };
        [value, value, value, 255]
    }
}

pub(super) fn paint(
    mask: &MaskView,
    view: &View,
    bounds: Bounds<Pixels>,
    cache: &MaskViewCache,
    window: &mut Window,
) {
    let scale = window.scale_factor() as f64;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    mask.mask.content_id().hash(&mut hash);
    mask.doc_size.hash(&mut hash);
    for v in mask.from_doc.to_cols_array().into_iter().chain([
        view.zoom,
        view.center.0,
        view.center.1,
        view.rotation,
        f64::from(u8::from(view.flip_x)),
        f64::from(u8::from(view.flip_y)),
        scale,
        f32::from(bounds.origin.x) as f64,
        f32::from(bounds.origin.y) as f64,
        f32::from(bounds.size.width) as f64,
        f32::from(bounds.size.height) as f64,
    ]) {
        v.to_bits().hash(&mut hash);
    }
    let key = hash.finish();
    let cached = cache
        .0
        .borrow()
        .as_ref()
        .filter(|(k, _)| *k == key)
        .map(|(_, image)| image.clone());
    let image = cached.unwrap_or_else(|| {
        let width = (f32::from(bounds.size.width) as f64 * scale).ceil().max(1.) as u32;
        let height = (f32::from(bounds.size.height) as f64 * scale)
            .ceil()
            .max(1.) as u32;
        let mut pixels = vec![0; width as usize * height as usize * 4];
        use rayon::prelude::*;
        pixels
            .par_chunks_mut(width as usize * 4)
            .enumerate()
            .for_each(|(y, row)| {
                for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    let screen = (
                        f32::from(bounds.origin.x) as f64 + (x as f64 + 0.5) / scale,
                        f32::from(bounds.origin.y) as f64 + (y as f64 + 0.5) / scale,
                    );
                    let (x, y) = view.screen_to_doc(screen, &bounds);
                    pixel.copy_from_slice(&mask.pixel(x, y));
                }
            });
        let image = Arc::new(viewport::bgra_image(width, height, pixels));
        if let Some((_, old)) = cache.0.borrow_mut().replace((key, image.clone())) {
            let _ = window.drop_image(old);
        }
        image
    });
    let _ = window.paint_image(bounds, bounds, Corners::default(), image, 0, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn mask_inspection_uses_density_and_affine_once_even_when_disabled() {
        let mut node = Node::raster(
            1,
            "Masked",
            Arc::new(Raster::solid(4, 2, [1.; 4])),
            Placement {
                x: 2.,
                y: 1.,
                ..Default::default()
            },
        );
        node.mask = Some(Arc::new(Mask::from_fn(4, 2, 255, |x, _| {
            if x == 0 { 0 } else { 255 }
        })));
        node.mask_transform[4] = 1.;
        node.mask_properties.density = 0.5;
        node.mask_enabled = false;
        let document = Document::new(12, 8);
        let mask = document.mask_for_inspection(&node).unwrap();
        assert!(document.composite_mask(&node).is_none());
        let view = MaskView {
            mask,
            from_doc: mask_inspection_to_document(&node).inverse(),
            doc_size: (12, 8),
        };
        assert_eq!(view.pixel(2.5, 1.5), [255; 4]);
        assert_eq!(view.pixel(3.5, 1.5), [128, 128, 128, 255]);
        assert_eq!(view.pixel(4.5, 1.5), [255; 4]);
        assert_eq!(view.pixel(8.5, 1.5), [255; 4]);
    }

    #[test]
    fn smart_mask_inspection_accounts_for_expanded_cache_origin() {
        let mut node = Node::smart(
            1,
            "Smart",
            Arc::new(Raster::solid(4, 2, [1.; 4])),
            Vec::new(),
            Placement {
                x: 3.,
                y: 2.,
                ..Default::default()
            },
        );
        node.mask = Some(Arc::new(Mask::from_fn(4, 2, 255, |x, _| {
            if x == 0 { 0 } else { 255 }
        })));
        node.mask_properties.density = 0.5;
        node.mask_enabled = false;
        let NodeKind::Smart { cache, offset, .. } = &mut node.kind else {
            unreachable!()
        };
        *cache = Arc::new(Raster::solid(8, 6, [1.; 4]));
        *offset = (-2, -2);
        let document = Document::new(16, 12);
        let view = MaskView {
            mask: document.mask_for_inspection(&node).unwrap(),
            from_doc: mask_inspection_to_document(&node).inverse(),
            doc_size: (16, 12),
        };
        assert_eq!(view.pixel(3.5, 2.5), [128, 128, 128, 255]);
        assert_eq!(view.pixel(1.5, 0.5), [255; 4]);
        assert_eq!(view.pixel(4.5, 2.5), [255; 4]);
    }

    #[test]
    fn document_mask_inspection_uses_document_grid_after_raw_size_diverges() {
        let document = Document::new(12, 8);
        let mut node = Node::new(1, "Fill", NodeKind::Fill { rgba: [255; 4] });
        node.mask = Some(Arc::new(Mask::from_fn(4, 2, 255, |x, _| {
            if x == 0 { 0 } else { 255 }
        })));
        node.mask_transform[4] = 5.;
        node.mask_transform[5] = 3.;
        node.mask_properties.density = 0.5;
        let mask = document.mask_for_inspection(&node).unwrap();
        assert_eq!((mask.width(), mask.height()), (12, 8));
        let view = MaskView {
            mask,
            from_doc: mask_inspection_to_document(&node).inverse(),
            doc_size: (12, 8),
        };
        assert_eq!(view.pixel(5.5, 3.5), [128, 128, 128, 255]);
        assert_eq!(view.pixel(0.5, 0.5), [255; 4]);
    }

    #[test]
    fn mask_view_shows_grayscale_in_document_coordinates() {
        let view = MaskView {
            mask: Arc::new(Mask::from_fn(
                2,
                1,
                0,
                |x, _| if x == 0 { 128 } else { 255 },
            )),
            from_doc: glam::DAffine2::from_translation(glam::dvec2(-2., -1.)),
            doc_size: (6, 4),
        };
        assert_eq!(view.pixel(2.5, 1.5), [128, 128, 128, 255]);
        assert_eq!(view.pixel(3.5, 1.5), [255; 4]);
        assert_eq!(view.pixel(0.5, 0.5), [0, 0, 0, 255]);
        assert_eq!(view.pixel(-0.5, 1.5), [0; 4]);
    }
}
