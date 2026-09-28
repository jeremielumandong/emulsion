//! Scalable diagram display: glyph outlines and paths are sampled at screen resolution.
//! The scene is independent of zoom. No document-size text bitmap is enlarged.
use crate::{IoError, Result};
use emulsion_core::Document;

pub struct SvgViewport {
    tree: resvg::usvg::Tree,
}
impl SvgViewport {
    pub fn new(doc: &Document) -> Result<Self> {
        let svg = crate::project_export::vector_svg(doc)?;
        let options = resvg::usvg::Options {
            image_href_resolver: resvg::usvg::ImageHrefResolver {
                resolve_string: Box::new(|_, _| None),
                ..Default::default()
            },
            ..Default::default()
        };
        let tree = resvg::usvg::Tree::from_data(&svg, &options)
            .map_err(|e| IoError::Unsupported(e.to_string()))?;
        Ok(Self { tree })
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
        if bytes.len() != size.0 as usize * size.1 as usize * 4 {
            return Err(IoError::Unsupported("Invalid viewport buffer".into()));
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
        resvg::render(
            &self.tree,
            resvg::tiny_skia::Transform::from_row(a, b, c, d, e, f),
            &mut pixels.as_mut(),
        );
        let mut bytes = pixels.take();
        for p in bytes.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        Ok(bytes)
    }
}

/// Conservative damage tracking for ordinary vector edits. Structural/effect
/// changes fall back to a complete render, preserving compositing correctness.
pub fn changed_bounds(before: &Document, after: &Document) -> Option<emulsion_raster::IRect> {
    use emulsion_core::NodeKind;
    if (before.width, before.height) != (after.width, after.height)
        || before.nodes.len() != after.nodes.len()
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
            dirty = dirty.union(&emulsion_core::geometry::node_bounds(doc, id)?);
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
