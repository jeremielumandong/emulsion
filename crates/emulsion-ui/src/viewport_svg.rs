//! Diagram SVG scenes, cached independently from the document's pixel tiles.
use crate::viewport::{self, View};
use emulsion_io::svg_viewport::SvgViewport;
use gpui_kit::*;
use std::sync::Arc;

pub(crate) type Revision = (u64, u64);
#[derive(Default)]
pub(crate) struct Cache {
    pub requested: Option<Revision>,
    pub building: bool,
    pub document: Option<(Revision, Arc<emulsion_core::Document>)>,
    pub damage: Option<(Revision, emulsion_raster::IRect)>,
    pixels: Vec<u8>,
    #[cfg(test)]
    pub rendered_frames: usize,
    pub scene: Option<(Revision, Arc<SvgViewport>)>,
    frame: Option<(FrameKey, Arc<RenderImage>)>,
}
#[derive(PartialEq)]
struct FrameKey {
    revision: Revision,
    size: (u32, u32),
    matrix: [f64; 6],
}
impl Cache {
    pub fn release(&mut self, window: &mut Window) {
        self.pixels = Vec::new();
        if let Some((_, image)) = self.frame.take() {
            let _ = window.drop_image(image);
        }
    }

    #[cfg(test)]
    pub fn ready(&self, revision: Revision) -> bool {
        self.scene.as_ref().is_some_and(|(key, _)| *key == revision)
    }
    /// Keep the previous vector scene on the same page until its replacement is
    /// ready. Never flash CPU tiles between interactive document revisions.
    pub fn displayable(&self, revision: Revision) -> bool {
        self.scene
            .as_ref()
            .is_some_and(|(key, _)| key.0 == revision.0)
    }
    pub fn paint(
        &mut self,
        revision: Revision,
        view: &View,
        bounds: Bounds<Pixels>,
        window: &mut Window,
    ) -> bool {
        let Some((key, scene)) = self.scene.as_ref().filter(|(key, _)| key.0 == revision.0) else {
            return false;
        };
        let scale = f64::from(window.scale_factor());
        let size = (
            (f64::from(f32::from(bounds.size.width)) * scale)
                .round()
                .max(1.) as u32,
            (f64::from(f32::from(bounds.size.height)) * scale)
                .round()
                .max(1.) as u32,
        );
        let (s, c) = view.rotation.to_radians().sin_cos();
        let z = view.zoom * scale;
        let matrix = [
            z * c,
            z * s,
            -z * s,
            z * c,
            size.0 as f64 / 2. - z * c * view.center.0 + z * s * view.center.1,
            size.1 as f64 / 2. - z * s * view.center.0 - z * c * view.center.1,
        ];
        let frame_key = FrameKey {
            revision: *key,
            size,
            matrix,
        };
        if self.frame.as_ref().is_none_or(|(key, _)| *key != frame_key) {
            let incremental = self.damage.filter(|(revision, _)| {
                self.frame.as_ref().is_some_and(|(old, _)| {
                    old.revision == *revision && old.size == size && old.matrix == matrix
                })
            });
            if let Some((_, dirty)) = incremental {
                if scene
                    .render_update(size, matrix, dirty, &mut self.pixels)
                    .is_err()
                {
                    return false;
                }
            } else {
                let Ok(bytes) = scene.render(size, matrix) else {
                    return false;
                };
                self.pixels = bytes;
            }
            #[cfg(test)]
            {
                self.rendered_frames += 1;
            }
            let image = Arc::new(viewport::bgra_image(size.0, size.1, self.pixels.clone()));
            if let Some((_, old)) = self.frame.replace((frame_key, image)) {
                let _ = window.drop_image(old);
            }
        }
        let image = self.frame.as_ref().unwrap().1.clone();
        window
            .paint_image(bounds, bounds, Corners::default(), image, 0, false)
            .is_ok()
    }
}
