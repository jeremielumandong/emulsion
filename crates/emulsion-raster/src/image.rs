//! Sparse tiled images with a lazily built mip chain.
//!
//! [`Plane`] is generic over the pixel type: [`Raster`] holds RGBA16 linear
//! premultiplied colour, [`Mask`] holds 8-bit coverage. Both are immutable
//! once built; edits produce a new plane that shares untouched tiles.
//!
//! Mip level `k` is the exact 2× box reduction of level `k-1`, so level-k pixel
//! `i` covers exactly source pixels `i·2^k .. (i+1)·2^k`. A tile reduced on its
//! own therefore matches the whole image reduced, which lets the viewport
//! re-render single tiles without seams.

use crate::color;
use crate::geom::{IRect, TileCoord};
use crate::tile::{TILE, TILE_PX};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// A pixel type that can live in a [`Plane`].
pub trait Pix: Copy + Send + Sync + PartialEq + 'static {
    /// Average of four pixels (2×2 box filter). Premultiplied colour averages
    /// correctly without unpremultiplying.
    fn avg4(a: Self, b: Self, c: Self, d: Self) -> Self;
}

impl Pix for [u16; 4] {
    #[inline]
    fn avg4(a: Self, b: Self, c: Self, d: Self) -> Self {
        let mut o = [0u16; 4];
        for i in 0..4 {
            o[i] = ((a[i] as u32 + b[i] as u32 + c[i] as u32 + d[i] as u32 + 2) >> 2) as u16;
        }
        o
    }
}

impl Pix for u8 {
    #[inline]
    fn avg4(a: Self, b: Self, c: Self, d: Self) -> Self {
        ((a as u32 + b as u32 + c as u32 + d as u32 + 2) >> 2) as u8
    }
}

pub type Tile<P> = Arc<[P]>;

/// Reduced tiles by (level ≥ 1, coord). `None` = all fill.
type MipCache<P> = HashMap<(u32, TileCoord), Option<Tile<P>>>;

/// Sparse tiled image. Missing tiles read as `fill`.
pub struct Plane<P: Pix> {
    width: u32,
    height: u32,
    fill: P,
    tiles: HashMap<TileCoord, Tile<P>>,
    mips: Mutex<MipCache<P>>,
}

pub type Raster = Plane<[u16; 4]>;
pub type Mask = Plane<u8>;

impl<P: Pix> Clone for Plane<P> {
    fn clone(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            fill: self.fill,
            tiles: self.tiles.clone(),
            mips: Mutex::new(HashMap::new()),
        }
    }
}

impl<P: Pix> std::fmt::Debug for Plane<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plane")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("tiles", &self.tiles.len())
            .finish()
    }
}

impl<P: Pix> Plane<P> {
    pub fn empty(width: u32, height: u32, fill: P) -> Self {
        Self {
            width,
            height,
            fill,
            tiles: HashMap::new(),
            mips: Mutex::new(HashMap::new()),
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn fill(&self) -> P {
        self.fill
    }
    pub fn bounds(&self) -> IRect {
        IRect::new(0, 0, self.width as i32, self.height as i32)
    }
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Size of mip level `level`, rounding up.
    pub fn level_size(&self, level: u32) -> (u32, u32) {
        let d = 1u32 << level;
        (
            self.width.div_ceil(d).max(1),
            self.height.div_ceil(d).max(1),
        )
    }

    /// Highest level that is still larger than one pixel in some dimension.
    pub fn max_level(&self) -> u32 {
        let m = self.width.max(self.height).max(1);
        31 - m.leading_zeros()
    }

    /// Tile grid extent at `level`.
    pub fn tiles_at(&self, level: u32) -> (i32, i32) {
        let (w, h) = self.level_size(level);
        (w.div_ceil(TILE) as i32, h.div_ceil(TILE) as i32)
    }

    pub fn base_tile(&self, c: TileCoord) -> Option<&Tile<P>> {
        self.tiles.get(&c)
    }

    pub fn base_tiles(&self) -> impl Iterator<Item = (&TileCoord, &Tile<P>)> {
        self.tiles.iter()
    }

    /// Insert or replace a level-0 tile. Tiles that are entirely `fill` are
    /// dropped to keep the plane sparse. Clears the mip cache.
    pub fn set_tile(&mut self, c: TileCoord, px: Vec<P>) {
        debug_assert_eq!(px.len(), TILE_PX);
        if px.iter().all(|p| *p == self.fill) {
            self.tiles.remove(&c);
        } else {
            self.tiles.insert(c, px.into());
        }
        self.mips.get_mut().clear();
    }

    /// Tile at any mip level. `None` means every pixel is `fill`.
    pub fn tile(&self, level: u32, c: TileCoord) -> Option<Tile<P>> {
        if level == 0 {
            return self.tiles.get(&c).cloned();
        }
        let (tx, ty) = self.tiles_at(level);
        if c.x < 0 || c.y < 0 || c.x >= tx || c.y >= ty {
            return None;
        }
        if let Some(t) = self.mips.lock().get(&(level, c)) {
            return t.clone();
        }
        let children = [
            self.tile(level - 1, TileCoord::new(c.x * 2, c.y * 2)),
            self.tile(level - 1, TileCoord::new(c.x * 2 + 1, c.y * 2)),
            self.tile(level - 1, TileCoord::new(c.x * 2, c.y * 2 + 1)),
            self.tile(level - 1, TileCoord::new(c.x * 2 + 1, c.y * 2 + 1)),
        ];
        let out = if children.iter().all(Option::is_none) {
            None
        } else {
            Some(reduce(&children, self.fill))
        };
        self.mips.lock().insert((level, c), out.clone());
        out
    }

    /// Pixel at level 0 (for tests and pickers).
    pub fn get(&self, x: u32, y: u32) -> P {
        let c = TileCoord::new((x / TILE) as i32, (y / TILE) as i32);
        match self.tiles.get(&c) {
            Some(t) => t[((y % TILE) * TILE + (x % TILE)) as usize],
            None => self.fill,
        }
    }

    /// Build from a row-major pixel buffer of exactly `width × height`.
    pub fn from_pixels(width: u32, height: u32, fill: P, px: &[P]) -> Self {
        assert_eq!(px.len(), width as usize * height as usize);
        Self::from_fn(width, height, fill, |x, y| {
            px[y as usize * width as usize + x as usize]
        })
    }

    /// Build by evaluating `f` for every pixel inside `width × height`.
    pub fn from_fn(width: u32, height: u32, fill: P, f: impl Fn(u32, u32) -> P + Sync) -> Self {
        use rayon::prelude::*;
        let mut plane = Self::empty(width, height, fill);
        let (tx, ty) = plane.tiles_at(0);
        let coords: Vec<TileCoord> = (0..ty)
            .flat_map(|y| (0..tx).map(move |x| TileCoord::new(x, y)))
            .collect();
        let built: Vec<(TileCoord, Vec<P>)> = coords
            .into_par_iter()
            .map(|c| {
                let mut t = vec![fill; TILE_PX];
                let x0 = c.x as u32 * TILE;
                let y0 = c.y as u32 * TILE;
                for ly in 0..TILE.min(height - y0) {
                    for lx in 0..TILE.min(width - x0) {
                        t[(ly * TILE + lx) as usize] = f(x0 + lx, y0 + ly);
                    }
                }
                (c, t)
            })
            .collect();
        for (c, t) in built {
            if t.iter().any(|p| *p != fill) {
                plane.tiles.insert(c, t.into());
            }
        }
        plane
    }

    /// Copy out the full level-0 image row-major.
    pub fn to_pixels(&self) -> Vec<P> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut out = vec![self.fill; w * h];
        for (c, t) in &self.tiles {
            let x0 = c.x as usize * TILE as usize;
            let y0 = c.y as usize * TILE as usize;
            if x0 >= w || y0 >= h {
                continue;
            }
            let cw = (TILE as usize).min(w - x0);
            for ly in 0..(TILE as usize).min(h - y0) {
                let src = &t[ly * TILE as usize..ly * TILE as usize + cw];
                out[(y0 + ly) * w + x0..(y0 + ly) * w + x0 + cw].copy_from_slice(src);
            }
        }
        out
    }
}

fn reduce<P: Pix>(children: &[Option<Tile<P>>; 4], fill: P) -> Tile<P> {
    let mut out = vec![fill; TILE_PX];
    let half = (TILE / 2) as usize;
    let t = TILE as usize;
    for (q, child) in children.iter().enumerate() {
        let Some(child) = child else { continue };
        let ox = (q % 2) * half;
        let oy = (q / 2) * half;
        for y in 0..half {
            let r0 = &child[(2 * y) * t..(2 * y + 1) * t];
            let r1 = &child[(2 * y + 1) * t..(2 * y + 2) * t];
            let dst = &mut out[(oy + y) * t + ox..(oy + y) * t + ox + half];
            for (x, d) in dst.iter_mut().enumerate() {
                *d = P::avg4(r0[2 * x], r0[2 * x + 1], r1[2 * x], r1[2 * x + 1]);
            }
        }
    }
    out.into()
}

impl Raster {
    pub fn transparent(width: u32, height: u32) -> Self {
        Self::empty(width, height, [0; 4])
    }

    /// From straight-alpha 8-bit sRGBA bytes.
    pub fn from_srgba8(width: u32, height: u32, data: &[u8]) -> Self {
        assert_eq!(data.len(), width as usize * height as usize * 4);
        Self::from_fn(width, height, [0; 4], |x, y| {
            let i = (y as usize * width as usize + x as usize) * 4;
            color::f_to_px(color::srgba8_to_premul([
                data[i],
                data[i + 1],
                data[i + 2],
                data[i + 3],
            ]))
        })
    }

    /// From straight-alpha 16-bit sRGBA samples.
    pub fn from_srgba16(width: u32, height: u32, data: &[u16]) -> Self {
        assert_eq!(data.len(), width as usize * height as usize * 4);
        Self::from_fn(width, height, [0; 4], |x, y| {
            let i = (y as usize * width as usize + x as usize) * 4;
            color::f_to_px(color::srgba16_to_premul([
                data[i],
                data[i + 1],
                data[i + 2],
                data[i + 3],
            ]))
        })
    }

    /// Uniform colour, premultiplied linear.
    pub fn solid(width: u32, height: u32, premul: [f32; 4]) -> Self {
        let p = color::f_to_px(premul);
        Self::from_fn(width, height, [0; 4], |_, _| p)
    }

    pub fn to_srgba8(&self) -> Vec<u8> {
        self.to_pixels()
            .into_iter()
            .flat_map(|p| color::premul_to_srgba8(color::px_to_f(p)))
            .collect()
    }

    pub fn to_srgba16(&self) -> Vec<u16> {
        self.to_pixels()
            .into_iter()
            .flat_map(|p| color::premul_to_srgba16(color::px_to_f(p)))
            .collect()
    }
}

impl Mask {
    /// A mask that reveals everything.
    pub fn white(width: u32, height: u32) -> Self {
        Self::empty(width, height, 255)
    }

    pub fn from_gray8(width: u32, height: u32, data: &[u8]) -> Self {
        Self::from_pixels(width, height, 255, data)
    }

    pub fn to_gray8(&self) -> Vec<u8> {
        self.to_pixels()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_sparsity() {
        let (w, h) = (600, 300);
        let mut data = vec![0u8; w * h * 4];
        // One opaque red block in the first tile only.
        for y in 0..10 {
            for x in 0..10 {
                let i = (y * w + x) * 4;
                data[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
        let r = Raster::from_srgba8(w as u32, h as u32, &data);
        assert_eq!(r.tile_count(), 1, "transparent tiles must not be stored");
        assert_eq!(r.to_srgba8(), data);
    }

    #[test]
    fn mip_alignment_invariant() {
        // A tile reduced on its own equals the same region of the whole image
        // reduced: every level-k pixel is the mean of its 2^k × 2^k block.
        let (w, h) = (1024u32, 768u32);
        let r = Raster::from_fn(w, h, [0; 4], |x, y| {
            let v = ((x * 7 + y * 13) % 251) as u16 * 200;
            [v, v / 2, v / 3, 65535]
        });
        for level in 1..=3u32 {
            let s = 1u32 << level;
            let t = r.tile(level, TileCoord::new(0, 0)).unwrap();
            for (py, px) in [(0u32, 0u32), (5, 17), (40, 90)] {
                let mut acc = [0u64; 4];
                for yy in 0..s {
                    for xx in 0..s {
                        let p = r.get(px * s + xx, py * s + yy);
                        for i in 0..4 {
                            acc[i] += p[i] as u64;
                        }
                    }
                }
                let got = t[(py * TILE + px) as usize];
                for i in 0..4 {
                    let want = acc[i] as f64 / (s * s) as f64;
                    assert!(
                        (got[i] as f64 - want).abs() <= level as f64,
                        "level {level} ch {i}"
                    );
                }
            }
        }
    }

    #[test]
    fn max_level_and_sizes() {
        let r = Raster::transparent(6000, 4000);
        assert_eq!(r.max_level(), 12);
        assert_eq!(r.level_size(3), (750, 500));
        assert_eq!(r.tiles_at(0), (24, 16));
    }
}
