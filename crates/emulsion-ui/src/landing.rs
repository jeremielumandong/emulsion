//! Launch artwork and an optional example document, decoded only when needed.

use gpui_kit::RenderImage;
use std::sync::Arc;

/// Bundled wide PNG, composed for the compact Home hero.
pub const LANDING_PNG: &[u8] = include_bytes!("../../../assets/landing/landing.png");
const SPLASH_PNG: &[u8] = include_bytes!("../../../assets/landing/splash.png");
pub const LANDING_NAME: &str = "landing";

pub(crate) struct LandingImages {
    pub(crate) splash: Arc<RenderImage>,
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

/// Decode the splash to GPUI BGRA; Home no longer needs three hero bitmaps.
pub(crate) fn decode() -> Option<LandingImages> {
    Some(LandingImages {
        splash: Arc::new(decode_image(SPLASH_PNG)?),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn landing_decodes_and_imports() {
        let images = super::decode().expect("bundled images decode");
        assert_eq!(images.splash.size(0).width.0, 1672);
        assert_eq!(images.splash.size(0).height.0, 941);
        let doc = emulsion_io::import::import_bytes("landing", super::LANDING_PNG).unwrap();
        assert_eq!((doc.width, doc.height), (2508, 627));
        assert_eq!(doc.nodes.len(), 1);
    }
}
