//! Scalable diagram display: glyph outlines and paths are sampled at screen resolution.
//! The scene is independent of zoom. No document-size text bitmap is enlarged.
use crate::{IoError, Result};
use emulsion_core::Document;

#[cfg(test)]
#[path = "viewport_shadow_tests.rs"]
mod shadow_tests;

#[path = "viewport_primitives.rs"]
mod primitives;

use std::{collections::HashMap, sync::Arc};
struct Layer {
    root: emulsion_core::NodeId,
    nodes: Vec<emulsion_core::Node>,
    tree: Arc<resvg::usvg::Tree>,
    primitives: Option<Arc<Vec<primitives::Primitive>>>,
    unbounded_fill: Option<([u8; 4], f32)>,
}
pub struct SvgViewport {
    layers: Vec<Layer>,
    parallel: bool,
    size: (u32, u32),
    design: emulsion_core::design_metadata::Design,
    font_generation: u64,
    infinite_canvas: bool,
    /// Number of subtrees parsed for this revision, for diagnostics/benchmarks.
    pub rebuilt_layers: usize,
}
impl SvgViewport {
    pub fn new(doc: &Document) -> Result<Self> {
        Self::updated(doc, None)
    }
    /// Retain unchanged SVG subtrees, so moving one object does not reparse a page.
    pub fn updated(doc: &Document, previous: Option<&Self>) -> Result<Self> {
        doc.validate()
            .map_err(|e| IoError::Manifest(e.to_string()))?;
        let generation = emulsion_core::text::font_generation();
        let infinite_canvas = emulsion_core::diagram::workspace::infinite_canvas(doc);
        let previous = previous.filter(|p| {
            p.size == (doc.width, doc.height)
                && p.design == doc.design
                && p.font_generation == generation
                && p.infinite_canvas == infinite_canvas
        });
        let old: HashMap<_, _> = previous
            .into_iter()
            .flat_map(|p| &p.layers)
            .map(|l| (l.root, l))
            .collect();
        let parents: HashMap<_, _> = doc.nodes.iter().map(|n| (n.id, n.parent)).collect();
        let mut groups: HashMap<_, Vec<emulsion_core::Node>> = HashMap::new();
        // Cross-root clipping needs its referenced sibling in the same SVG tree.
        let together = doc
            .nodes
            .iter()
            .any(|n| n.parent.is_none() && n.clip_to.is_some());
        // Plain containers and user groups do not form compositing boundaries.
        // Retain their child objects independently, as for top-level diagrams.
        let mut children: HashMap<Option<u64>, Vec<u64>> = HashMap::new();
        let nodes_by_id: HashMap<_, _> = doc.nodes.iter().map(|n| (n.id, n)).collect();
        for n in &doc.nodes {
            children.entry(n.parent).or_default().push(n.id);
        }
        fn partition(
            id: u64,
            doc: &Document,
            nodes: &HashMap<u64, &emulsion_core::Node>,
            children: &HashMap<Option<u64>, Vec<u64>>,
            roots: &mut Vec<u64>,
        ) {
            let n = nodes[&id];
            let atomic = doc.diagram.as_ref().is_none_or(|d| {
                d.edges.contains_key(&id)
                    || d.shapes.get(&id).is_some_and(|s| !s.kind.is_container())
            });
            let kids = children
                .get(&Some(id))
                .map(Vec::as_slice)
                .unwrap_or_default();
            let simple = n.is_group()
                && !atomic
                && n.opacity == 1.
                && !n.has_mask()
                && n.styles.is_empty()
                && n.clip_to.is_none()
                && n.blending == Default::default()
                && matches!(
                    n.blend,
                    emulsion_raster::BlendMode::Normal | emulsion_raster::BlendMode::PassThrough
                )
                && kids.iter().all(|id| nodes[id].clip_to.is_none());
            if simple {
                if n.visible {
                    for child in kids {
                        partition(*child, doc, nodes, children, roots);
                    }
                }
            } else {
                roots.push(id);
            }
        }
        let roots = if together {
            vec![0]
        } else {
            let mut roots = Vec::new();
            for id in doc.children(None) {
                partition(id, doc, &nodes_by_id, &children, &mut roots);
            }
            roots
        };
        let root_set: std::collections::HashSet<_> = roots.iter().copied().collect();
        for node in &doc.nodes {
            let mut root = node.id;
            while !root_set.contains(&root) {
                let Some(Some(parent)) = parents.get(&root) else {
                    break;
                };
                root = *parent;
            }
            if together || root_set.contains(&root) {
                groups
                    .entry(if together { 0 } else { root })
                    .or_default()
                    .push(node.clone());
            }
        }
        let options = crate::svg_vectors::options();
        let mut layers = Vec::with_capacity(roots.len());
        let mut rebuilt_layers = 0;
        for root in roots {
            let nodes = groups.remove(&root).unwrap_or_default();
            let unbounded_fill = infinite_canvas
                .then(|| nodes.first())
                .flatten()
                .and_then(|n| {
                    if nodes.len() == 1
                        && n.visible
                        && n.parent.is_none()
                        && !n.has_mask()
                        && n.styles.is_empty()
                        && n.blending == Default::default()
                        && n.clip_to.is_none()
                        && n.blend == emulsion_raster::BlendMode::Normal
                        && let emulsion_core::NodeKind::Fill { rgba } = n.kind
                    {
                        Some((rgba, n.opacity))
                    } else {
                        None
                    }
                });
            let tree = if let Some(layer) = old.get(&root).filter(|l| l.nodes == nodes) {
                layer.tree.clone()
            } else {
                let svg = if together {
                    crate::project_export::vector_svg_for(
                        doc,
                        crate::project_export::SvgPurpose::Viewport,
                    )?
                } else {
                    let mut subtree = Document::new(doc.width, doc.height);
                    subtree.design = doc.design.clone();
                    subtree.nodes = nodes.clone();
                    subtree.diagram = None;
                    crate::project_export::viewport_subtree(&subtree, root)?
                };
                rebuilt_layers += 1;
                Arc::new(
                    resvg::usvg::Tree::from_data(&svg, &options)
                        .map_err(|e| IoError::Unsupported(e.to_string()))?,
                )
            };
            let primitives = if let Some(layer) = old.get(&root).filter(|l| l.nodes == nodes) {
                layer.primitives.clone()
            } else {
                primitives::retain(&tree).map(Arc::new)
            };
            layers.push(Layer {
                root,
                nodes,
                tree,
                primitives,
                unbounded_fill,
            });
        }
        let parallel = layers.iter().any(|l| l.primitives.is_some())
            && layers.iter().all(|l| primitives::band_safe(l.tree.root()));
        Ok(Self {
            layers,
            parallel,
            size: (doc.width, doc.height),
            design: doc.design.clone(),
            font_generation: generation,
            infinite_canvas,
            rebuilt_layers,
        })
    }
    /// Rerender only the affected screen rectangle when the view is unchanged.
    /// Existing pixels outside the region are retained at their native resolution.
    pub fn render_update(
        &self,
        size: (u32, u32),
        transform: [f64; 6],
        dirty: emulsion_raster::IRect,
        bytes: &mut [u8],
    ) -> Result<()> {
        if size.0 == 0
            || size.1 == 0
            || u64::from(size.0) * u64::from(size.1) > 36_000_000
            || !transform.iter().all(|v| v.is_finite() && v.abs() < 1e12)
            || bytes.len() != size.0 as usize * size.1 as usize * 4
        {
            return Err(IoError::Unsupported("Invalid viewport buffer".into()));
        }
        if dirty.is_empty() {
            return Ok(());
        }
        let [a, b, c, d, e, f] = transform;
        let points = [
            (dirty.x, dirty.y),
            (dirty.right(), dirty.y),
            (dirty.right(), dirty.bottom()),
            (dirty.x, dirty.bottom()),
        ]
        .map(|(x, y)| {
            (
                a * x as f64 + c * y as f64 + e,
                b * x as f64 + d * y as f64 + f,
            )
        });
        let left = (points
            .iter()
            .map(|p| p.0)
            .fold(f64::INFINITY, f64::min)
            .floor()
            - 2.)
            .clamp(0., size.0 as f64) as u32;
        let top = (points
            .iter()
            .map(|p| p.1)
            .fold(f64::INFINITY, f64::min)
            .floor()
            - 2.)
            .clamp(0., size.1 as f64) as u32;
        let right = (points
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            + 2.)
            .clamp(0., size.0 as f64) as u32;
        let bottom = (points
            .iter()
            .map(|p| p.1)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            + 2.)
            .clamp(0., size.1 as f64) as u32;
        if right <= left || bottom <= top {
            return Ok(());
        }
        let patch = self.render(
            (right - left, bottom - top),
            [a, b, c, d, e - left as f64, f - top as f64],
        )?;
        let stride = (right - left) as usize * 4;
        for row in 0..(bottom - top) as usize {
            let start = ((top as usize + row) * size.0 as usize + left as usize) * 4;
            bytes[start..start + stride].copy_from_slice(&patch[row * stride..(row + 1) * stride]);
        }
        Ok(())
    }

    /// Render only the physical viewport, including fractional pan, zoom and rotation.
    /// Returns premultiplied BGRA for GPUI, not a cached document raster.
    pub fn render(&self, size: (u32, u32), transform: [f64; 6]) -> Result<Vec<u8>> {
        if size.0 == 0
            || size.1 == 0
            || u64::from(size.0) * u64::from(size.1) > 36_000_000
            || !transform.iter().all(|v| v.is_finite() && v.abs() < 1e12)
        {
            return Err(IoError::Unsupported(
                "Invalid SVG viewport dimensions or transform".into(),
            ));
        }
        let mut pixels = resvg::tiny_skia::Pixmap::new(size.0, size.1)
            .ok_or_else(|| IoError::Unsupported("Cannot allocate SVG viewport".into()))?;
        let [a, b, c, d, e, f] = transform.map(|v| v as f32);
        let transform = resvg::tiny_skia::Transform::from_row(a, b, c, d, e, f);
        if self.parallel && u64::from(size.0) * u64::from(size.1) >= 512 * 1024 {
            use rayon::prelude::*;
            let rows = size.1.div_ceil(4) as usize;
            let stride = size.0 as usize * 4;
            // At most four jobs, borrowing disjoint portions of one output image.
            // Decoded shadows are shared; no full-viewport temporary per worker.
            pixels
                .data_mut()
                .par_chunks_mut(rows * stride)
                .enumerate()
                .for_each(|(band, bytes)| {
                    let height = (bytes.len() / stride) as u32;
                    let mut target =
                        resvg::tiny_skia::PixmapMut::from_bytes(bytes, size.0, height).unwrap();
                    self.render_into(
                        (size.0, height),
                        transform.post_translate(0., -((band * rows) as f32)),
                        &mut target,
                    );
                });
        } else {
            self.render_into(size, transform, &mut pixels.as_mut());
        }
        let mut bytes = pixels.take();
        for p in bytes.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        Ok(bytes)
    }

    fn render_into(
        &self,
        size: (u32, u32),
        transform: resvg::tiny_skia::Transform,
        pixels: &mut resvg::tiny_skia::PixmapMut<'_>,
    ) {
        for layer in &self.layers {
            if let Some((rgba, opacity)) = layer.unbounded_fill {
                let mut paint = resvg::tiny_skia::Paint::default();
                paint.set_color_rgba8(
                    rgba[0],
                    rgba[1],
                    rgba[2],
                    (rgba[3] as f32 * opacity).round() as u8,
                );
                pixels.fill_rect(
                    resvg::tiny_skia::Rect::from_xywh(0., 0., size.0 as f32, size.1 as f32)
                        .unwrap(),
                    &paint,
                    resvg::tiny_skia::Transform::identity(),
                    None,
                );
                continue;
            }
            // resvg also culls individual paths; skip entire offscreen groups here.
            let bounds = layer
                .tree
                .root()
                .abs_layer_bounding_box()
                .transform(transform);
            if bounds.is_some_and(|r| {
                r.right() < 0.
                    || r.bottom() < 0.
                    || r.left() > size.0 as f32
                    || r.top() > size.1 as f32
            }) {
                continue;
            }
            if let Some(primitives) = &layer.primitives {
                primitives::render(primitives, transform, pixels);
            } else {
                resvg::render(&layer.tree, transform, pixels);
            }
        }
    }
}

/// Conservative damage tracking for ordinary vector edits. Structural/effect
/// changes fall back to a complete render, preserving compositing correctness.
pub fn changed_bounds(before: &Document, after: &Document) -> Option<emulsion_raster::IRect> {
    use emulsion_core::NodeKind;
    if (before.width, before.height) != (after.width, after.height)
        || before.nodes.len() != after.nodes.len()
        || before.design != after.design
        || emulsion_core::diagram::workspace::infinite_canvas(before)
            != emulsion_core::diagram::workspace::infinite_canvas(after)
    {
        return None;
    }
    let mut dirty = emulsion_raster::IRect::default();
    for (old, new) in before.nodes.iter().zip(&after.nodes) {
        if old.id != new.id {
            return None;
        }
        if old == new {
            continue;
        }
        if old.parent != new.parent
            || old.clip_to != new.clip_to
            || !old.styles.is_empty()
            || !new.styles.is_empty()
            || !matches!(
                (&old.kind, &new.kind),
                (NodeKind::Path { .. }, NodeKind::Path { .. })
                    | (NodeKind::Text { .. }, NodeKind::Text { .. })
            )
        {
            return None;
        }
        for (doc, id) in [(before, old.id), (after, new.id)] {
            // Empty connector labels and absent arrowheads have no ink bounds.
            // Their siblings still contribute all changed visible geometry.
            if let Some(bounds) = emulsion_core::geometry::node_bounds(doc, id) {
                dirty = dirty.union(&bounds);
            }
        }
    }
    Some(dirty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{
        NodeKind,
        diagram::{Builder, ShapeKind},
    };
    #[test]
    fn diagram_text_is_svg_geometry_and_rerenders_at_device_resolution() {
        let mut b = Builder::new(320, 180).unwrap();
        let id = b
            .add_shape(
                ShapeKind::Process,
                [20., 20., 200., 90.],
                "Sharp diagram label",
            )
            .unwrap();
        let doc = b.finish().unwrap();
        let source = crate::project_export::vector_svg(&doc).unwrap();
        let source = String::from_utf8(source).unwrap();
        assert!(source.contains("<path"));
        assert!(!source.contains("<image"));
        let label = doc.diagram.as_ref().unwrap().shapes[&id].label;
        assert!(matches!(
            &doc.node(label).unwrap().kind,
            NodeKind::Text { .. }
        ));
        let scene = SvgViewport::new(&doc).unwrap();
        let one = scene.render((320, 180), [1., 0., 0., 1., 0., 0.]).unwrap();
        let four = scene.render((1280, 720), [4., 0., 0., 4., 0., 0.]).unwrap();
        let mut different = 0;
        for y in 0..720usize {
            for x in 0..1280usize {
                let a = &four[(y * 1280 + x) * 4..][..4];
                let b = &one[((y / 4) * 320 + x / 4) * 4..][..4];
                different += usize::from(a != b);
            }
        }
        assert!(
            different > 4000,
            "zoom must sample glyph outlines rather than enlarge source pixels"
        );
        assert!(
            scene
                .render((320, 180), [f64::NAN, 0., 0., 1., 0., 0.])
                .is_err()
        );
        assert!(
            scene
                .render((100000, 100000), [1., 0., 0., 1., 0., 0.])
                .is_err()
        );
    }
    #[test]
    fn letter_curve_remains_vector_at_sixty_four_times_zoom() {
        let mut builder = Builder::new(320, 180).unwrap();
        let id = builder
            .add_shape(ShapeKind::Process, [20., 20., 200., 90.], "n")
            .unwrap();
        let doc = builder.finish().unwrap();
        let label = doc.diagram.as_ref().unwrap().shapes[&id].label;
        let bounds = emulsion_core::geometry::node_bounds(&doc, label).unwrap();
        let scene = SvgViewport::new(&doc).unwrap();
        let x = bounds.x as f64;
        let y = bounds.y as f64;
        let low = scene.render((16, 24), [1., 0., 0., 1., -x, -y]).unwrap();
        let high = scene
            .render((1024, 1536), [64., 0., 0., 64., -x * 64., -y * 64.])
            .unwrap();
        let mut differences = 0;
        for y in 0..1536usize {
            for x in 0..1024usize {
                let a = &high[(y * 1024 + x) * 4..][..4];
                let b = &low[((y / 64) * 16 + x / 64) * 4..][..4];
                differences += usize::from(a != b);
            }
        }
        assert!(
            differences > 10_000,
            "Extreme zoom must resample the glyph curve"
        );
    }

    #[test]
    fn moved_vector_patch_matches_full_render_at_zoom_and_rotation() {
        use emulsion_core::{Command, Editor};
        let doc = emulsion_core::diagram_library::TEMPLATES[0]
            .build()
            .unwrap();
        let id = *doc.diagram.as_ref().unwrap().shapes.keys().next().unwrap();
        let mut editor = Editor::new(doc.clone(), None);
        editor
            .execute(Command::TranslateNode {
                id,
                dx: 25.,
                dy: 17.,
            })
            .unwrap();
        let dirty = changed_bounds(&doc, &editor.doc).unwrap();
        for matrix in [[1., 0., 0., 1., 0., 0.], [1.5, 0.2, -0.2, 1.5, 0., 0.]] {
            let mut pixels = SvgViewport::new(&doc)
                .unwrap()
                .render((1440, 1080), matrix)
                .unwrap();
            let scene = SvgViewport::new(&editor.doc).unwrap();
            scene
                .render_update((1440, 1080), matrix, dirty, &mut pixels)
                .unwrap();
            let expected = scene.render((1440, 1080), matrix).unwrap();
            let differences = pixels
                .iter()
                .zip(&expected)
                .filter(|(a, b)| a.abs_diff(**b) > 2)
                .count();
            assert!(
                differences < 100,
                "Patch must preserve background, old location, label and rerouted edges: {differences}"
            );
        }
    }
}

#[cfg(test)]
mod retained_tests {
    use super::*;
    #[test]
    fn movement_rebuilds_only_changed_objects_and_preserves_rendering() {
        use emulsion_core::{
            Command, Editor,
            diagram::{Builder, ShapeKind},
        };
        let mut b = Builder::new(1200, 900).unwrap();
        let mut ids = Vec::new();
        for i in 0..100 {
            ids.push(
                b.add_shape(
                    ShapeKind::Process,
                    [
                        10. + (i % 10) as f64 * 110.,
                        10. + (i / 10) as f64 * 80.,
                        100.,
                        65.,
                    ],
                    "Service",
                )
                .unwrap(),
            );
        }
        let mut e = Editor::new(b.finish().unwrap(), None);
        let old = SvgViewport::new(&e.doc).unwrap();
        e.execute(Command::TranslateNode {
            id: ids[45],
            dx: 7.,
            dy: 3.,
        })
        .unwrap();
        let next = SvgViewport::updated(&e.doc, Some(&old)).unwrap();
        assert_eq!(next.rebuilt_layers, 1);
        let matrix = [1., 0., 0., 1., 0., 0.];
        assert_eq!(
            next.render((1200, 900), matrix).unwrap(),
            SvgViewport::new(&e.doc)
                .unwrap()
                .render((1200, 900), matrix)
                .unwrap()
        );
        let same = SvgViewport::updated(&e.doc, Some(&next)).unwrap();
        assert_eq!(same.rebuilt_layers, 0);
    }
}

#[cfg(test)]
mod compositing_tests {
    use super::*;
    #[test]
    fn retained_roots_match_monolithic_svg_with_overlap_and_opacity() {
        use emulsion_core::diagram::{Builder, ShapeKind};
        let mut b = Builder::new(320, 240).unwrap();
        let a = b
            .add_shape(ShapeKind::Process, [20., 20., 180., 100.], "Under")
            .unwrap();
        let c = b
            .add_shape(ShapeKind::Decision, [85., 50., 180., 100.], "Over")
            .unwrap();
        let mut doc = b.finish().unwrap();
        doc.node_mut(a).unwrap().opacity = 0.7;
        doc.node_mut(c).unwrap().opacity = 0.4;
        let svg = crate::project_export::vector_svg(&doc).unwrap();
        let tree = resvg::usvg::Tree::from_data(&svg, &Default::default()).unwrap();
        let mut expected = resvg::tiny_skia::Pixmap::new(640, 480).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(2., 2.),
            &mut expected.as_mut(),
        );
        let mut bytes = expected.take();
        for p in bytes.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        assert_eq!(
            SvgViewport::new(&doc)
                .unwrap()
                .render((640, 480), [2., 0., 0., 2., 0., 0.])
                .unwrap(),
            bytes
        );
    }
}

#[cfg(test)]
mod container_tests {
    use super::*;
    #[test]
    fn moving_a_shape_inside_a_large_container_reuses_its_siblings() {
        use emulsion_core::{
            Command, Editor,
            diagram::{Builder, ShapeKind},
        };
        let mut b = Builder::new(1200, 900).unwrap();
        let container = b
            .add_shape(ShapeKind::Container, [0., 0., 1200., 900.], "Services")
            .unwrap();
        let mut ids = Vec::new();
        for i in 0..100 {
            ids.push(
                b.add_shape(
                    ShapeKind::Process,
                    [
                        10. + (i % 10) as f64 * 110.,
                        40. + (i / 10) as f64 * 80.,
                        100.,
                        65.,
                    ],
                    "Service",
                )
                .unwrap(),
            );
        }
        let mut doc = b.finish().unwrap();
        for id in &ids {
            doc.node_mut(*id).unwrap().parent = Some(container);
            Arc::make_mut(doc.diagram.as_mut().unwrap())
                .shapes
                .get_mut(id)
                .unwrap()
                .container = Some(container);
        }
        doc.normalize();
        doc.validate().unwrap();
        let before = SvgViewport::new(&doc).unwrap();
        let mut editor = Editor::new(doc, None);
        editor
            .execute(Command::TranslateNode {
                id: ids[45],
                dx: 7.,
                dy: 3.,
            })
            .unwrap();
        let after = SvgViewport::updated(&editor.doc, Some(&before)).unwrap();
        assert_eq!(after.rebuilt_layers, 1);
        let svg = crate::project_export::vector_svg(&editor.doc).unwrap();
        let tree = resvg::usvg::Tree::from_data(&svg, &Default::default()).unwrap();
        let mut expected = resvg::tiny_skia::Pixmap::new(1200, 900).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut expected.as_mut(),
        );
        let mut bytes = expected.take();
        for p in bytes.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        assert_eq!(
            after.render((1200, 900), [1., 0., 0., 1., 0., 0.]).unwrap(),
            bytes
        );
    }
}

#[cfg(test)]
mod retained_source_tests {
    use super::*;
    #[test]
    fn infinite_canvas_renders_background_and_shapes_beyond_both_page_edges() {
        use emulsion_core::{
            Editor,
            diagram::{Builder, ShapeKind, workspace},
        };
        let mut builder = Builder::new(100, 100).unwrap();
        builder
            .add_shape(ShapeKind::Note, [-100., -100., 40., 40.], "")
            .unwrap();
        builder
            .add_shape(ShapeKind::Note, [400., 400., 40., 40.], "")
            .unwrap();
        let mut editor = Editor::new(builder.finish().unwrap(), None);
        let finite = SvgViewport::new(&editor.doc).unwrap();
        workspace::set_infinite_canvas(&mut editor, true).unwrap();
        let infinite = SvgViewport::updated(&editor.doc, Some(&finite)).unwrap();
        for offset in [130., -370.] {
            let matrix = [1., 0., 0., 1., offset, offset];
            let before = finite.render((100, 100), matrix).unwrap();
            let after = infinite.render((100, 100), matrix).unwrap();
            assert_eq!(
                before[3], 0,
                "finite page must not fill the off-page viewport"
            );
            assert_eq!(&after[..4], &[255; 4]);
            let center = (50 * 100 + 50) * 4;
            assert_ne!(
                &after[center..center + 4],
                &[255; 4],
                "off-page shape disappeared"
            );
        }
        editor.undo();
        let restored = SvgViewport::updated(&editor.doc, Some(&infinite)).unwrap();
        assert_eq!(
            restored
                .render((100, 100), [1., 0., 0., 1., 130., 130.])
                .unwrap()[3],
            0
        );
    }

    #[test]
    fn infinite_canvas_roundtrips_per_page_in_native_projects() {
        use emulsion_core::{
            diagram::{Builder, workspace},
            project::{ProjectEditor, ProjectKind},
        };
        let doc = Builder::new(800, 600).unwrap().finish().unwrap();
        let mut project = ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap();
        workspace::set_infinite_canvas(&mut project, true).unwrap();
        project
            .add_page(
                Builder::new(400, 300).unwrap().finish().unwrap(),
                "Fixed page".into(),
                0.,
            )
            .unwrap();
        let mut file = std::io::Cursor::new(Vec::new());
        crate::project::write_to(&project.snapshot().unwrap(), &mut file).unwrap();
        file.set_position(0);
        let reopened = crate::project::read_from(file).unwrap();
        assert!(workspace::infinite_canvas(&reopened.pages[0].doc));
        assert!(!workspace::infinite_canvas(&reopened.pages[1].doc));
        assert_eq!(
            (reopened.pages[0].doc.width, reopened.pages[0].doc.height),
            (800, 600)
        );
    }

    #[test]
    fn retained_complex_svg_keeps_its_text_when_rendered() {
        let xml = r##"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="100"><defs><linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient></defs><rect width="240" height="100" fill="url(#g)"/><text x="20" y="70" font-family="DejaVu Sans" font-size="48" fill="#000">SVG</text></svg>"##;
        let doc = crate::svg_vectors::document(xml).unwrap();
        let pixels = SvgViewport::new(&doc)
            .unwrap()
            .render((240, 100), [1., 0., 0., 1., 0., 0.])
            .unwrap();
        let black = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] < 40 && p[1] < 40 && p[2] < 40 && p[3] > 240)
            .count();
        assert!(
            black > 300,
            "SVG source text must be shaped inside retained artwork: {black}"
        );
    }
}

#[cfg(test)]
mod vector_mask_guard_tests {
    use super::*;
    use emulsion_core::{
        Editor, EmptyVectorCoverage, VectorMask,
        diagram::{Builder, ShapeKind, workspace},
    };

    #[test]
    fn masked_container_is_not_partitioned_into_unmasked_children() {
        let mut builder = Builder::new(80, 60).unwrap();
        let container = builder
            .add_shape(ShapeKind::Container, [5., 5., 70., 50.], "")
            .unwrap();
        let mut doc = builder.finish().unwrap();
        doc.node_mut(container).unwrap().vector_mask =
            Some(VectorMask::empty(EmptyVectorCoverage::HideAll));
        // Unsupported enabled masks must hand rendering back to the compositor.
        // Partitioning a container into children would silently drop its mask.
        assert!(SvgViewport::new(&doc).is_err());
        doc.node_mut(container)
            .unwrap()
            .vector_mask
            .as_mut()
            .unwrap()
            .enabled = false;
        let scene = SvgViewport::new(&doc).unwrap();
        assert!(scene.layers.iter().any(|layer| layer.root == container));
    }

    #[test]
    fn disabled_vector_masked_background_does_not_use_infinite_fill_shortcut() {
        let mut editor = Editor::new(Builder::new(80, 60).unwrap().finish().unwrap(), None);
        workspace::set_infinite_canvas(&mut editor, true).unwrap();
        let fill = editor
            .doc
            .nodes
            .iter_mut()
            .find(|node| matches!(node.kind, emulsion_core::NodeKind::Fill { .. }))
            .unwrap();
        fill.vector_mask = Some(VectorMask {
            enabled: false,
            ..Default::default()
        });
        let id = fill.id;
        let scene = SvgViewport::new(&editor.doc).unwrap();
        let layer = scene.layers.iter().find(|layer| layer.root == id).unwrap();
        assert!(layer.unbounded_fill.is_none());
    }
}
