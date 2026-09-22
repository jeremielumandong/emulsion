//! The landing image: the Home screen hero, the launch splash, and a
//! ready-made document to try the editor on.

use gpui_kit::RenderImage;
use std::sync::Arc;

/// Bundled wide PNG, composed for the compact Home hero.
pub const LANDING_PNG: &[u8] = include_bytes!("../../../assets/landing/landing.png");
const LANDING_MEDIUM_PNG: &[u8] = include_bytes!("../../../assets/landing/landing-medium.png");
const LANDING_TALL_PNG: &[u8] = include_bytes!("../../../assets/landing/landing-tall.png");
pub const LANDING_NAME: &str = "landing";

pub(crate) struct LandingImages {
    wide: Arc<RenderImage>,
    medium: Arc<RenderImage>,
    tall: Arc<RenderImage>,
}

impl LandingImages {
    /// Pick the closest composition and its eye-centered focal position.
    pub(crate) fn for_aspect(&self, aspect: f32) -> (Arc<RenderImage>, (f32, f32)) {
        if aspect < 2.05 {
            (self.tall.clone(), (0.78, 0.22))
        } else if aspect < 3.2 {
            (self.medium.clone(), (0.73, 0.23))
        } else {
            (self.wide.clone(), (0.80, 0.21))
        }
    }
}

fn decode_image(bytes: &[u8]) -> Option<RenderImage> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (w, h) = img.dimensions();
    let mut px = img.into_raw();
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    Some(crate::viewport::bgra_image(w, h, px))
}

/// Decode the responsive Home hero compositions to GPUI images (BGRA).
pub(crate) fn decode() -> Option<LandingImages> {
    Some(LandingImages {
        wide: Arc::new(decode_image(LANDING_PNG)?),
        medium: Arc::new(decode_image(LANDING_MEDIUM_PNG)?),
        tall: Arc::new(decode_image(LANDING_TALL_PNG)?),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn landing_decodes_and_imports() {
        let images = super::decode().expect("bundled images decode");
        assert_eq!(images.for_aspect(14.0).0.size(0).width.0, 2508);
        assert_eq!(images.for_aspect(2.5).0.size(0).width.0, 1916);
        assert_eq!(images.for_aspect(1.5).0.size(0).width.0, 1672);
        let doc = emulsion_io::import::import_bytes("landing", super::LANDING_PNG).unwrap();
        assert_eq!((doc.width, doc.height), (2508, 627));
        assert_eq!(doc.nodes.len(), 1);
    }
}
