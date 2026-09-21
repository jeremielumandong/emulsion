//! The landing image: the Home screen hero, the launch splash, and a
//! ready-made document to try the editor on.

use gpui_kit::RenderImage;

/// Bundled JPEG, 1586×992.
pub const LANDING_JPG: &[u8] = include_bytes!("../../../assets/landing/landing.jpg");
pub const LANDING_NAME: &str = "landing";

/// Decode to a GPUI image (BGRA). Runs on a background thread.
pub fn decode() -> Option<RenderImage> {
    let img = image::load_from_memory_with_format(LANDING_JPG, image::ImageFormat::Jpeg)
        .ok()?
        .into_rgba8();
    let (w, h) = img.dimensions();
    let mut px = img.into_raw();
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    Some(crate::viewport::bgra_image(w, h, px))
}

#[cfg(test)]
mod tests {
    #[test]
    fn landing_decodes_and_imports() {
        let img = super::decode().expect("bundled image decodes");
        assert_eq!(img.size(0).width.0, 1586);
        let doc = emulsion_io::import::import_bytes("landing", super::LANDING_JPG).unwrap();
        assert_eq!((doc.width, doc.height), (1586, 992));
        assert_eq!(doc.nodes.len(), 1);
    }
}
