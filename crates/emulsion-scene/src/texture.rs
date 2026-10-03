//! Base-colour textures and per-vertex albedo of imported models (C7).
//!
//! glTF `baseColorTexture` images (PNG or JPEG) are decoded once at import
//! into sRGB RGBA8 texels, within [`MAX_TEXTURE_SIDE`] and a per-model budget
//! of [`MAX_TEXTURE_BYTES`]; larger images are refused with an error rather
//! than decoded. The renderer samples them bilinearly in linear light with
//! the sampler's wrap modes.

use std::io::Cursor;
use std::sync::Arc;

use glam::{Vec2, Vec3};

use crate::error::SceneError;

/// Largest texture width or height accepted.
pub const MAX_TEXTURE_SIDE: u32 = 4096;
/// Decoded texture memory accepted per imported model (bytes, RGBA8).
pub const MAX_TEXTURE_BYTES: usize = 64 << 20;

/// How texture coordinates outside 0..1 are resolved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Wrap {
    #[default]
    Repeat,
    MirroredRepeat,
    ClampToEdge,
}

impl Wrap {
    /// From a glTF sampler's `wrapS`/`wrapT` (OpenGL enum; unknown → repeat).
    pub fn from_gl(v: u64) -> Wrap {
        match v {
            33071 => Wrap::ClampToEdge,
            33648 => Wrap::MirroredRepeat,
            _ => Wrap::Repeat,
        }
    }

    fn index(self, i: i64, n: i64) -> usize {
        (match self {
            Wrap::Repeat => i.rem_euclid(n),
            Wrap::ClampToEdge => i.clamp(0, n - 1),
            Wrap::MirroredRepeat => {
                let p = i.rem_euclid(2 * n);
                if p < n { p } else { 2 * n - 1 - p }
            }
        }) as usize
    }
}

/// A decoded colour texture (sRGB RGBA8, rows top to bottom).
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub wrap_s: Wrap,
    pub wrap_t: Wrap,
    pixels: Vec<u8>,
}

fn srgb_to_linear_lut() -> &'static [f32; 256] {
    static LUT: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    LUT.get_or_init(|| {
        std::array::from_fn(|i| {
            let v = i as f32 / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        })
    })
}

fn texture_err(msg: impl Into<String>) -> SceneError {
    SceneError::Import(msg.into())
}

impl Texture {
    /// A texture from sRGB RGBA8 texels.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Result<Texture, SceneError> {
        if width == 0 || height == 0 || pixels.len() != width as usize * height as usize * 4 {
            return Err(texture_err("texture pixel data does not match its size"));
        }
        Ok(Texture {
            width,
            height,
            wrap_s: Wrap::Repeat,
            wrap_t: Wrap::Repeat,
            pixels,
        })
    }

    /// Decodes a PNG or JPEG image, charging its decoded size to `budget`.
    /// Oversized, unsupported or damaged images are refused.
    pub fn decode(bytes: &[u8], budget: &mut usize) -> Result<Texture, SceneError> {
        let format = match image::guess_format(bytes) {
            Ok(f @ (image::ImageFormat::Png | image::ImageFormat::Jpeg)) => f,
            _ => return Err(texture_err("texture image is not a PNG or JPEG")),
        };
        let reader = |limits: image::Limits| {
            let mut r = image::ImageReader::with_format(Cursor::new(bytes), format);
            r.limits(limits);
            r
        };
        let (w, h) = reader(image::Limits::default())
            .into_dimensions()
            .map_err(|e| texture_err(format!("damaged texture image: {e}")))?;
        if w == 0 || h == 0 || w > MAX_TEXTURE_SIDE || h > MAX_TEXTURE_SIDE {
            return Err(texture_err(format!(
                "texture is {w}×{h} pixels (max {MAX_TEXTURE_SIDE} per side)"
            )));
        }
        let size = w as usize * h as usize * 4;
        if size > *budget {
            return Err(texture_err(format!(
                "textures need more than {} MB once decoded",
                MAX_TEXTURE_BYTES >> 20
            )));
        }
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_TEXTURE_SIDE);
        limits.max_image_height = Some(MAX_TEXTURE_SIDE);
        limits.max_alloc = Some(size as u64 * 4 + (1 << 20));
        let img = reader(limits)
            .decode()
            .map_err(|e| texture_err(format!("damaged texture image: {e}")))?
            .into_rgba8();
        if img.dimensions() != (w, h) {
            return Err(texture_err("damaged texture image: size mismatch"));
        }
        *budget -= size;
        Texture::from_rgba8(w, h, img.into_raw())
    }

    /// Bilinearly filtered linear-light RGB at texture coordinate `uv`
    /// (glTF convention: (0, 0) is the top-left corner of the image).
    pub fn sample(&self, uv: Vec2) -> Vec3 {
        let (w, h) = (self.width as i64, self.height as i64);
        let uv = if uv.is_finite() {
            uv.clamp(Vec2::splat(-1e6), Vec2::splat(1e6))
        } else {
            Vec2::ZERO
        };
        let x = uv.x * w as f32 - 0.5;
        let y = uv.y * h as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let xs = [self.wrap_s.index(x0, w), self.wrap_s.index(x0 + 1, w)];
        let ys = [self.wrap_t.index(y0, h), self.wrap_t.index(y0 + 1, h)];
        let lut = srgb_to_linear_lut();
        let texel = |x: usize, y: usize| {
            let i = (y * self.width as usize + x) * 4;
            let p = &self.pixels[i..i + 3];
            Vec3::new(lut[p[0] as usize], lut[p[1] as usize], lut[p[2] as usize])
        };
        let top = texel(xs[0], ys[0]).lerp(texel(xs[1], ys[0]), fx);
        let bottom = texel(xs[0], ys[1]).lerp(texel(xs[1], ys[1]), fx);
        top.lerp(bottom, fy)
    }

    /// Decoded size in bytes.
    pub fn byte_size(&self) -> usize {
        self.pixels.len()
    }
}

/// Per-vertex albedo of a mesh: texture coordinates and colours (either may
/// be empty) and the base-colour texture. Multiplies the mesh's base colour.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshAlbedo {
    pub uvs: Vec<Vec2>,
    /// Linear-light vertex colours.
    pub colors: Vec<Vec3>,
    pub texture: Option<Arc<Texture>>,
}

impl MeshAlbedo {
    /// The vertex's texture coordinate and colour.
    pub(crate) fn vertex(&self, i: usize) -> (Vec2, Vec3) {
        (
            self.uvs.get(i).copied().unwrap_or(Vec2::ZERO),
            self.colors.get(i).copied().unwrap_or(Vec3::ONE),
        )
    }

    /// Albedo multiplier for an interpolated texture coordinate and colour.
    pub(crate) fn shade(&self, uv: Vec2, color: Vec3) -> Vec3 {
        match &self.texture {
            Some(t) => t.sample(uv) * color,
            None => color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_modes() {
        assert_eq!(Wrap::Repeat.index(-1, 4), 3);
        assert_eq!(Wrap::Repeat.index(5, 4), 1);
        assert_eq!(Wrap::ClampToEdge.index(-3, 4), 0);
        assert_eq!(Wrap::ClampToEdge.index(9, 4), 3);
        assert_eq!(Wrap::MirroredRepeat.index(4, 4), 3);
        assert_eq!(Wrap::MirroredRepeat.index(-1, 4), 0);
        assert_eq!(Wrap::from_gl(33071), Wrap::ClampToEdge);
        assert_eq!(Wrap::from_gl(33648), Wrap::MirroredRepeat);
        assert_eq!(Wrap::from_gl(10497), Wrap::Repeat);
    }

    #[test]
    fn bilinear_sampling() {
        // 2×1: black, white.
        let t = Texture::from_rgba8(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]).unwrap();
        assert!(t.sample(Vec2::new(0.25, 0.5)).x < 1e-6);
        assert!((t.sample(Vec2::new(0.75, 0.5)).x - 1.0).abs() < 1e-6);
        let mid = t.sample(Vec2::new(0.5, 0.5)).x;
        assert!((mid - 0.5).abs() < 1e-6, "{mid}");
        // Repeat wraps the left edge to the white texel.
        assert!((t.sample(Vec2::new(0.0, 0.5)).x - 0.5).abs() < 1e-6);
        assert!(t.sample(Vec2::new(f32::NAN, 0.0)).is_finite());
        let mut budget = MAX_TEXTURE_BYTES;
        assert!(Texture::decode(b"not an image", &mut budget).is_err());
        assert_eq!(budget, MAX_TEXTURE_BYTES);
    }
}
