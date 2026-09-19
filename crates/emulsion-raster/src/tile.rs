//! Tile geometry and storage.

/// Tile edge length in pixels.
pub const TILE: u32 = 256;
/// Pixels per tile.
pub const TILE_PX: usize = (TILE * TILE) as usize;

/// A 256×256 block of RGBA16 linear premultiplied pixels, row-major.
#[derive(Clone)]
pub struct RgbaTile(pub Box<[[u16; 4]]>);

impl RgbaTile {
    pub fn transparent() -> Self {
        Self(vec![[0u16; 4]; TILE_PX].into_boxed_slice())
    }
    pub fn is_transparent(&self) -> bool {
        self.0.iter().all(|p| p[3] == 0)
    }
}

/// A 256×256 block of 8-bit coverage, row-major.
#[derive(Clone)]
pub struct MaskTile(pub Box<[u8]>);

impl MaskTile {
    pub fn filled(v: u8) -> Self {
        Self(vec![v; TILE_PX].into_boxed_slice())
    }
}

/// An accumulator tile in `f32` linear premultiplied RGBA.
pub type FTile = Vec<[f32; 4]>;

pub fn ftile() -> FTile {
    vec![[0.0; 4]; TILE_PX]
}
