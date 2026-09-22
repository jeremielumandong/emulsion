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
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

/// Pixel count of `rect` (0 when empty), computed without `i32` overflow.
fn rect_area(rect: IRect) -> usize {
    rect.w.max(0) as usize * rect.h.max(0) as usize
}

fn next_content_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("plane content identity exhausted")
}

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
    coverage_bounds_cache: OnceLock<IRect>,
    content_id: u64,
    masked_bounds_cache: Mutex<Vec<(u64, IRect)>>,
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
            coverage_bounds_cache: self.coverage_bounds_cache.clone(),
            content_id: self.content_id,
            masked_bounds_cache: Mutex::new(self.masked_bounds_cache.lock().clone()),
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
            coverage_bounds_cache: OnceLock::new(),
            content_id: next_content_id(),
            masked_bounds_cache: Mutex::new(Vec::new()),
        }
    }

    /// Identity of immutable pixel content, preserved by clones and renewed by edits.
    pub fn content_id(&self) -> u64 {
        self.content_id
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

    /// Backing allocation identities and sizes, including lazily cached mips.
    /// Callers deduplicate identities across planes and history snapshots.
    pub fn buffer_allocations(&self) -> Vec<(usize, usize)> {
        self.tiles
            .values()
            .chain(self.mips.lock().values().flatten())
            .map(|tile| {
                (
                    tile.as_ptr() as usize,
                    tile.len() * std::mem::size_of::<P>(),
                )
            })
            .collect()
    }

    /// A plane built from existing level-0 tiles, sharing them. None when a
    /// tile has the wrong length or lies outside the plane's grid.
    pub fn from_tiles(
        width: u32,
        height: u32,
        fill: P,
        tiles: impl IntoIterator<Item = (TileCoord, Tile<P>)>,
    ) -> Option<Self> {
        let mut p = Self::empty(width, height, fill);
        let (tw, th) = p.tiles_at(0);
        for (c, t) in tiles {
            if t.len() != TILE_PX || c.x < 0 || c.y < 0 || c.x >= tw || c.y >= th {
                return None;
            }
            p.tiles.insert(c, t);
        }
        Some(p)
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
        self.coverage_bounds_cache.take();
        self.content_id = next_content_id();
        self.masked_bounds_cache.get_mut().clear();
    }

    /// A copy with some level-0 tiles replaced (`None` = all fill). Cached
    /// mip tiles that do not cover a changed tile are kept, so an edit costs
    /// only the reductions above the tiles it touched.
    pub fn with_changes(&self, changes: Vec<(TileCoord, Option<Vec<P>>)>) -> Self {
        let mut tiles = self.tiles.clone();
        let mut stale = std::collections::HashSet::new();
        let top = self.max_level() + 1;
        for (c, px) in changes {
            for k in 1..=top {
                stale.insert((k, TileCoord::new(c.x >> k, c.y >> k)));
            }
            match px {
                Some(px) if px.iter().any(|p| *p != self.fill) => {
                    tiles.insert(c, px.into());
                }
                _ => {
                    tiles.remove(&c);
                }
            }
        }
        let mips = self
            .mips
            .lock()
            .iter()
            .filter(|(k, _)| !stale.contains(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        Self {
            width: self.width,
            height: self.height,
            fill: self.fill,
            tiles,
            mips: Mutex::new(mips),
            coverage_bounds_cache: OnceLock::new(),
            content_id: next_content_id(),
            masked_bounds_cache: Mutex::new(Vec::new()),
        }
    }

    /// A dense copy of `rect` (clipped to the plane), row-major, with the
    /// rect's size; pixels outside the plane read as `fill`.
    ///
    /// # Panics
    /// When the rect's area cannot be allocated.
    pub fn read_rect(&self, rect: IRect) -> Vec<P> {
        let mut out = vec![self.fill; rect_area(rect)];
        let clip = rect.intersect(&self.bounds());
        if clip.is_empty() {
            return out;
        }
        let t = TILE as i32;
        for ty in clip.y.div_euclid(t)..=(clip.bottom() - 1).div_euclid(t) {
            for tx in clip.x.div_euclid(t)..=(clip.right() - 1).div_euclid(t) {
                let Some(tile) = self.tiles.get(&TileCoord::new(tx, ty)) else {
                    continue;
                };
                let tr = IRect::new(tx * t, ty * t, t, t).intersect(&clip);
                for y in tr.y..tr.bottom() {
                    let src = ((y - ty * t) * t + (tr.x - tx * t)) as usize;
                    let dst = (y - rect.y) as usize * rect.w as usize + (tr.x - rect.x) as usize;
                    out[dst..dst + tr.w as usize].copy_from_slice(&tile[src..src + tr.w as usize]);
                }
            }
        }
        out
    }

    /// A copy with a dense `rect` written in (clipped to the plane).
    ///
    /// # Panics
    /// When `px.len()` is not the rect's area.
    pub fn write_rect(&self, rect: IRect, px: &[P]) -> Self {
        assert_eq!(px.len(), rect_area(rect));
        let clip = rect.intersect(&self.bounds());
        if clip.is_empty() {
            return self.clone();
        }
        let t = TILE as i32;
        let mut changes = Vec::new();
        for ty in clip.y.div_euclid(t)..=(clip.bottom() - 1).div_euclid(t) {
            for tx in clip.x.div_euclid(t)..=(clip.right() - 1).div_euclid(t) {
                let c = TileCoord::new(tx, ty);
                let mut tile: Vec<P> = match self.tiles.get(&c) {
                    Some(t) => t.to_vec(),
                    None => vec![self.fill; TILE_PX],
                };
                let tr = IRect::new(tx * t, ty * t, t, t).intersect(&clip);
                for y in tr.y..tr.bottom() {
                    let dst = ((y - ty * t) * t + (tr.x - tx * t)) as usize;
                    let src = (y - rect.y) as usize * rect.w as usize + (tr.x - rect.x) as usize;
                    tile[dst..dst + tr.w as usize].copy_from_slice(&px[src..src + tr.w as usize]);
                }
                changes.push((c, Some(tile)));
            }
        }
        self.with_changes(changes)
    }

    /// Bounding box of stored (non-fill) tiles, in pixels.
    pub fn tile_bounds(&self) -> IRect {
        let t = TILE as i32;
        self.tiles
            .keys()
            .fold(IRect::default(), |acc, c| {
                acc.union(&IRect::new(c.x * t, c.y * t, t, t))
            })
            .intersect(&self.bounds())
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
        } else if let Some(shared) = uniform_quad(&children) {
            // Four copies of one uniform buffer reduce to themselves.
            Some(shared)
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

    /// Run `f` over every row of the level-0 image in parallel, writing
    /// `per` output values per pixel into a row-major buffer.
    pub fn rows_par<T: Copy + Send + Sync>(
        &self,
        per: usize,
        init: T,
        f: impl Fn(&[P], &mut [T]) + Sync,
    ) -> Vec<T> {
        use rayon::prelude::*;
        let (w, h) = (self.width as usize, self.height as usize);
        let t = TILE as usize;
        let mut out = vec![init; w * h * per];
        out.par_chunks_mut(w * per)
            .enumerate()
            .for_each(|(y, dst)| {
                let mut row: Vec<P> = vec![self.fill; w];
                let ty = (y / t) as i32;
                let ly = y % t;
                for tx in 0..w.div_ceil(t) {
                    if let Some(tile) = self.tiles.get(&TileCoord::new(tx as i32, ty)) {
                        let x0 = tx * t;
                        let cw = t.min(w - x0);
                        row[x0..x0 + cw].copy_from_slice(&tile[ly * t..ly * t + cw]);
                    }
                }
                f(&row, dst);
            });
        out
    }

    /// Copy out the full level-0 image row-major.
    pub fn to_pixels(&self) -> Vec<P> {
        self.rows_par(1, self.fill, |row, dst| dst.copy_from_slice(row))
    }
}

/// The shared buffer when all four children are one uniform tile whose box
/// average is itself, as with the interior of [`Raster::solid`].
fn uniform_quad<P: Pix>(children: &[Option<Tile<P>>; 4]) -> Option<Tile<P>> {
    let first = children[0].as_ref()?;
    let p = *first.first()?;
    let shared = children[1..]
        .iter()
        .all(|c| c.as_ref().is_some_and(|c| Arc::ptr_eq(c, first)));
    (shared && P::avg4(p, p, p, p) == p && first.iter().all(|q| *q == p)).then(|| first.clone())
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
    /// Cached source-space coverage bounds for immutable pixels. A nonzero
    /// implicit alpha fill conservatively covers the full image, even where
    /// explicit tiles override that fill. This matches layer-handle semantics.
    pub fn coverage_bounds(&self) -> IRect {
        *self.coverage_bounds_cache.get_or_init(|| {
            if self.fill[3] != 0 {
                return self.bounds();
            }
            let extent = self.bounds();
            let mut bounds = IRect::default();
            for (coord, pixels) in &self.tiles {
                let origin = (coord.x * TILE as i32, coord.y * TILE as i32);
                let rect =
                    IRect::new(origin.0, origin.1, TILE as i32, TILE as i32).intersect(&extent);
                if rect.is_empty() {
                    continue;
                }
                if !bounds.is_empty() && rect.intersect(&bounds) == rect {
                    continue;
                }
                for y in rect.y..rect.bottom() {
                    let offset = ((y - origin.1) as usize) * TILE as usize;
                    let start = (rect.x - origin.0) as usize;
                    let row = &pixels[offset + start..offset + start + rect.w as usize];
                    let Some(first) = row.iter().position(|p| p[3] != 0) else {
                        continue;
                    };
                    let last = row.iter().rposition(|p| p[3] != 0).expect("covered row");
                    bounds = bounds.union(&IRect::new(
                        rect.x + first as i32,
                        y,
                        (last - first + 1) as i32,
                        1,
                    ));
                }
                if bounds == extent {
                    break;
                }
            }
            bounds
        })
    }

    /// Exact masked coverage, with a small metadata-only cache. No mask or
    /// source pixel buffers are retained by cache entries.
    pub fn masked_coverage_bounds(&self, mask: &Mask) -> IRect {
        if self.fill[3] != 0 {
            return self.bounds().intersect(&mask.coverage_bounds());
        }
        if mask.tiles.is_empty() {
            return if mask.fill != 0 {
                self.coverage_bounds()
            } else {
                IRect::default()
            };
        }
        if let Some((_, bounds)) = self
            .masked_bounds_cache
            .lock()
            .iter()
            .find(|(id, _)| *id == mask.content_id)
        {
            return *bounds;
        }
        let mut bounds = IRect::default();
        let extent = self.bounds();
        let mask_contains_raster = mask.width >= self.width && mask.height >= self.height;
        for (coord, pixels) in &self.tiles {
            let origin = (coord.x * TILE as i32, coord.y * TILE as i32);
            let rect = IRect::new(origin.0, origin.1, TILE as i32, TILE as i32).intersect(&extent);
            if rect.is_empty() || (!bounds.is_empty() && rect.intersect(&bounds) == rect) {
                continue;
            }
            let mask_tile = mask.tiles.get(coord);
            if mask_tile.is_none() && mask.fill == 0 {
                continue;
            }
            for y in rect.y..rect.bottom() {
                let offset = (y - origin.1) as usize * TILE as usize + (rect.x - origin.0) as usize;
                let row = &pixels[offset..offset + rect.w as usize];
                let covered = |i: usize| {
                    let coverage = if mask_contains_raster
                        || ((rect.x as usize + i) < mask.width as usize && y < mask.height as i32)
                    {
                        mask_tile.map_or(mask.fill, |tile| tile[offset + i])
                    } else {
                        mask.fill
                    };
                    row[i][3] != 0 && coverage != 0
                };
                let Some(first) = (0..row.len()).find(|i| covered(*i)) else {
                    continue;
                };
                let last = (first..row.len())
                    .rfind(|i| covered(*i))
                    .expect("covered row");
                bounds = bounds.union(&IRect::new(
                    rect.x + first as i32,
                    y,
                    (last - first + 1) as i32,
                    1,
                ));
            }
            if bounds == extent {
                break;
            }
        }
        let mut cache = self.masked_bounds_cache.lock();
        if cache.len() >= 4 {
            cache.remove(0);
        }
        cache.push((mask.content_id, bounds));
        bounds
    }

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

    /// Uniform colour, premultiplied linear. Tiles of the same extent share
    /// one buffer, so the cost is independent of the image area.
    pub fn solid(width: u32, height: u32, premul: [f32; 4]) -> Self {
        let p = color::f_to_px(premul);
        let mut plane = Self::empty(width, height, [0; 4]);
        if p == [0; 4] || width == 0 || height == 0 {
            return plane;
        }
        let mut shared: HashMap<(u32, u32), Tile<[u16; 4]>> = HashMap::new();
        let (tx, ty) = plane.tiles_at(0);
        for y in 0..ty {
            for x in 0..tx {
                let cw = TILE.min(width - x as u32 * TILE);
                let ch = TILE.min(height - y as u32 * TILE);
                let tile = shared.entry((cw, ch)).or_insert_with(|| {
                    let mut t = vec![[0; 4]; TILE_PX];
                    for row in t
                        .as_chunks_mut::<{ TILE as usize }>()
                        .0
                        .iter_mut()
                        .take(ch as usize)
                    {
                        row[..cw as usize].fill(p);
                    }
                    t.into()
                });
                plane.tiles.insert(TileCoord::new(x, y), tile.clone());
            }
        }
        plane
    }

    pub fn to_srgba8(&self) -> Vec<u8> {
        self.rows_par(4, 0u8, |row, dst| {
            for (p, o) in row.iter().zip(dst.as_chunks_mut::<4>().0.iter_mut()) {
                o.copy_from_slice(&color::premul_to_srgba8(color::px_to_f(*p)));
            }
        })
    }

    pub fn to_srgba16(&self) -> Vec<u16> {
        self.rows_par(4, 0u16, |row, dst| {
            for (p, o) in row.iter().zip(dst.as_chunks_mut::<4>().0.iter_mut()) {
                o.copy_from_slice(&color::premul_to_srgba16(color::px_to_f(*p)));
            }
        })
    }
}

impl Mask {
    /// Exact nonzero coverage without allocating a dense copy of the mask.
    pub fn coverage_bounds(&self) -> IRect {
        *self.coverage_bounds_cache.get_or_init(|| {
            if self.tiles.is_empty() {
                return if self.fill != 0 {
                    self.bounds()
                } else {
                    IRect::default()
                };
            }
            let extent = self.bounds();
            let mut bounds = IRect::default();
            let mut scan = |coord: TileCoord, pixels: Option<&Tile<u8>>| {
                let origin = (coord.x * TILE as i32, coord.y * TILE as i32);
                let rect =
                    IRect::new(origin.0, origin.1, TILE as i32, TILE as i32).intersect(&extent);
                if rect.is_empty() || (!bounds.is_empty() && rect.intersect(&bounds) == rect) {
                    return;
                }
                let Some(pixels) = pixels else {
                    bounds = bounds.union(&rect);
                    return;
                };
                for y in rect.y..rect.bottom() {
                    let offset =
                        (y - origin.1) as usize * TILE as usize + (rect.x - origin.0) as usize;
                    let row = &pixels[offset..offset + rect.w as usize];
                    let Some(first) = row.iter().position(|p| *p != 0) else {
                        continue;
                    };
                    let last = row.iter().rposition(|p| *p != 0).expect("covered row");
                    bounds = bounds.union(&IRect::new(
                        rect.x + first as i32,
                        y,
                        (last - first + 1) as i32,
                        1,
                    ));
                }
            };
            if self.fill == 0 {
                for (coord, pixels) in &self.tiles {
                    scan(*coord, Some(pixels));
                }
            } else {
                let (width, height) = self.tiles_at(0);
                for y in 0..height {
                    for x in 0..width {
                        let coord = TileCoord::new(x, y);
                        scan(coord, self.tiles.get(&coord));
                    }
                }
            }
            bounds
        })
    }

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
    fn rect_area_does_not_overflow_i32() {
        assert_eq!(rect_area(IRect::new(0, 0, 50_000, 50_000)), 2_500_000_000);
        assert_eq!(rect_area(IRect::new(0, 0, -4, 7)), 0);
        let m = Mask::from_fn(4, 4, 0, |x, y| (x + y) as u8);
        let r = IRect::new(1, 1, 2, 3);
        let px = m.read_rect(r);
        assert_eq!(px.len(), 6);
        assert_eq!(
            m.write_rect(r, &px).read_rect(m.bounds()),
            m.read_rect(m.bounds())
        );
    }

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
    fn edits_keep_unaffected_mips() {
        // Level-2 tile (0,0) covers pixels 0..1024; the edit lands at 1900.
        let r = Raster::from_fn(2048, 1024, [0; 4], |x, _| {
            [(x % 1000) as u16 * 60, 0, 0, 65535]
        });
        let far = r.tile(2, TileCoord::new(0, 0)).unwrap();
        let near = r.tile(2, TileCoord::new(0, 0)).unwrap();
        assert!(std::sync::Arc::ptr_eq(&far, &near));
        let edited = r.write_rect(
            IRect::new(1900, 900, 10, 10),
            &vec![[65535, 65535, 65535, 65535]; 100],
        );
        let kept = edited.tile(2, TileCoord::new(0, 0)).unwrap();
        assert!(
            std::sync::Arc::ptr_eq(&far, &kept),
            "untouched region reuses its mip"
        );
        assert!(!std::sync::Arc::ptr_eq(
            &r.tile(2, TileCoord::new(1, 0)).unwrap(),
            &edited.tile(2, TileCoord::new(1, 0)).unwrap()
        ));
        assert_eq!(edited.get(1905, 905), [65535; 4]);
        assert_eq!(edited.read_rect(IRect::new(1899, 899, 2, 2))[3], [65535; 4]);
        assert_eq!(r.get(1905, 905)[1], 0, "the original is unchanged");
    }

    #[test]
    fn shared_solid_matches_dense_at_every_level() {
        for (w, h) in [
            (1031, 777),
            (768, 256),
            (300, 1),
            (4096, 256),
            (1, 1),
            (0, 5),
        ] {
            for premul in [[0.0, 0.0, 0.0, 1.0], [0.2, 0.4, 0.1, 0.5], [0.0; 4]] {
                let p = color::f_to_px(premul);
                let dense = Raster::from_fn(w, h, [0; 4], |_, _| p);
                let solid = Raster::solid(w, h, premul);
                assert_eq!(solid.tile_count(), dense.tile_count());
                for level in 0..=dense.max_level() + 1 {
                    let (tx, ty) = dense.tiles_at(level);
                    for y in -1..=ty {
                        for x in -1..=tx {
                            let c = TileCoord::new(x, y);
                            assert_eq!(
                                solid.tile(level, c).as_deref(),
                                dense.tile(level, c).as_deref(),
                                "{w}×{h} level {level} tile {x},{y}"
                            );
                        }
                    }
                }
            }
        }
        let shared = Raster::solid(1031, 777, [1.0; 4]);
        assert!(shared.buffer_allocations().len() > 4);
        let mut buffers: Vec<_> = shared.buffer_allocations().iter().map(|b| b.0).collect();
        buffers.sort_unstable();
        buffers.dedup();
        assert_eq!(buffers.len(), 4, "interior, right, bottom and corner tiles");
    }

    #[test]
    fn max_level_and_sizes() {
        let r = Raster::transparent(6000, 4000);
        assert_eq!(r.max_level(), 12);
        assert_eq!(r.level_size(3), (750, 500));
        assert_eq!(r.tiles_at(0), (24, 16));
    }
}

#[cfg(test)]
mod coverage_bounds_tests {
    use super::*;

    #[test]
    fn cached_coverage_tracks_clone_tile_replacement_and_copy_on_write() {
        let mut raster = Raster::transparent(300, 270);
        assert_eq!(raster.coverage_bounds(), IRect::default());
        let mut tile = vec![[0; 4]; TILE_PX];
        tile[5 * TILE as usize + 7] = [1, 2, 3, 4];
        raster.set_tile(TileCoord::new(0, 0), tile);
        assert!(raster.coverage_bounds_cache.get().is_none());
        let expected = IRect::new(7, 5, 1, 1);
        assert_eq!(raster.coverage_bounds(), expected);
        let mut copy = raster.clone();
        assert_eq!(copy.coverage_bounds_cache.get(), Some(&expected));
        copy.set_tile(TileCoord::new(0, 0), vec![[0; 4]; TILE_PX]);
        assert!(copy.coverage_bounds_cache.get().is_none());
        assert!(copy.coverage_bounds().is_empty());
        assert_eq!(
            raster.coverage_bounds(),
            expected,
            "clone edits leave original cached coverage intact"
        );
        let changed = raster.write_rect(IRect::new(299, 269, 1, 1), &[[1, 2, 3, 65535]]);
        assert!(changed.coverage_bounds_cache.get().is_none());
        assert_eq!(changed.coverage_bounds(), IRect::new(7, 5, 293, 265));
        let removed = changed.with_changes(vec![(TileCoord::new(0, 0), None)]);
        assert!(removed.coverage_bounds_cache.get().is_none());
        assert_eq!(removed.coverage_bounds(), IRect::new(299, 269, 1, 1));
    }

    #[test]
    fn coverage_ignores_tile_padding_and_preserves_nonzero_fill_semantics() {
        let tile = vec![[0, 0, 0, 65535]; TILE_PX];
        let raster =
            Raster::from_tiles(3, 2, [0; 4], [(TileCoord::new(0, 0), Arc::from(tile))]).unwrap();
        assert_eq!(raster.coverage_bounds(), IRect::new(0, 0, 3, 2));
        let mut implicit = Raster::empty(3, 2, [0, 0, 0, 1]);
        implicit.set_tile(TileCoord::new(0, 0), vec![[0; 4]; TILE_PX]);
        assert_eq!(implicit.coverage_bounds(), implicit.bounds());
        let invisible_rgb = Raster::from_fn(8, 8, [0; 4], |_, _| [200, 300, 400, 0]);
        assert!(invisible_rgb.coverage_bounds().is_empty());
    }

    #[test]
    fn large_import_repeated_coverage_queries_do_not_rescan_pixels() {
        let raster = Raster::from_fn(2048, 1024, [0; 4], |x, y| {
            [x as u16, y as u16, 12345, 65535]
        });
        let expected = raster.bounds();
        assert_eq!(raster.coverage_bounds(), expected);
        assert_eq!(raster.coverage_bounds_cache.get(), Some(&expected));
        let start = std::time::Instant::now();
        for _ in 0..20_000 {
            assert_eq!(std::hint::black_box(&raster).coverage_bounds(), expected);
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "cached queries must not do full-image work per frame"
        );
    }
}

#[cfg(test)]
mod masked_coverage_tests {
    use super::*;

    #[test]
    fn smaller_masks_use_implicit_fill_outside_dimensions_not_tile_padding() {
        let raster = Raster::from_fn(8, 8, [0; 4], |x, y| {
            if (x, y) == (1, 1) || (x, y) == (7, 7) {
                [1, 2, 3, 65535]
            } else {
                [0; 4]
            }
        });
        for fill in [0, 255] {
            let mut tile = vec![255 - fill; TILE_PX];
            for y in 0..2 {
                for x in 0..3 {
                    tile[y * TILE as usize + x] = 0;
                }
            }
            if fill == 0 {
                tile[TILE as usize + 1] = 255;
            }
            let mask =
                Mask::from_tiles(3, 2, fill, [(TileCoord::new(0, 0), Arc::from(tile))]).unwrap();
            let expected = if fill == 0 {
                IRect::new(1, 1, 1, 1)
            } else {
                IRect::new(7, 7, 1, 1)
            };
            assert_eq!(raster.masked_coverage_bounds(&mask), expected);
            assert_eq!(
                raster.masked_coverage_bounds(&mask),
                expected,
                "cached result respects finite mask extent"
            );
        }
    }

    #[test]
    fn mask_bounds_are_exact_with_implicit_fill_and_invalidate_on_edits() {
        let mut mask = Mask::white(300, 270);
        assert_eq!(mask.coverage_bounds(), mask.bounds());
        let old_id = mask.content_id();
        let original = mask.clone();
        assert_eq!(original.content_id(), old_id);
        assert_eq!(
            original.coverage_bounds_cache.get(),
            Some(&original.bounds())
        );
        for y in 0..2 {
            for x in 0..2 {
                mask.set_tile(TileCoord::new(x, y), vec![0; TILE_PX]);
            }
        }
        assert_ne!(mask.content_id(), old_id);
        assert!(mask.coverage_bounds_cache.get().is_none());
        assert!(
            mask.coverage_bounds().is_empty(),
            "explicit zero tiles override white fill"
        );
        let changed = mask.write_rect(IRect::new(299, 269, 1, 1), &[1]);
        assert_ne!(changed.content_id(), mask.content_id());
        assert_eq!(changed.coverage_bounds(), IRect::new(299, 269, 1, 1));
        assert!(mask.coverage_bounds().is_empty());
        assert_eq!(original.coverage_bounds(), original.bounds());
    }

    #[test]
    fn masked_bounds_cache_uses_content_identity_and_exact_pixel_intersection() {
        let raster = Raster::from_fn(32, 32, [0; 4], |x, y| {
            if (x, y) == (1, 1) || (x, y) == (20, 20) {
                [1, 2, 3, 65535]
            } else {
                [0; 4]
            }
        });
        let mut mask = Mask::from_fn(32, 32, 0, |x, y| {
            if (x, y) == (1, 20) || (x, y) == (20, 1) {
                255
            } else {
                0
            }
        });
        assert!(
            !raster
                .coverage_bounds()
                .intersect(&mask.coverage_bounds())
                .is_empty()
        );
        assert!(
            raster.masked_coverage_bounds(&mask).is_empty(),
            "intersecting bounding boxes do not imply intersecting ink"
        );
        let old_id = mask.content_id();
        let clone = mask.clone();
        assert_eq!(clone.content_id(), old_id);
        assert!(raster.masked_coverage_bounds(&clone).is_empty());
        assert_eq!(
            raster.masked_bounds_cache.lock().len(),
            1,
            "unchanged clone uses the same entry"
        );
        let mut tile = vec![0; TILE_PX];
        tile[TILE as usize + 1] = 255;
        mask.set_tile(TileCoord::new(0, 0), tile);
        assert_ne!(mask.content_id(), old_id);
        assert_eq!(raster.masked_coverage_bounds(&mask), IRect::new(1, 1, 1, 1));
        let changed = raster.write_rect(IRect::new(1, 1, 1, 1), &[[0; 4]]);
        assert_ne!(changed.content_id(), raster.content_id());
        assert!(changed.masked_coverage_bounds(&mask).is_empty());
        for i in 0..20 {
            let replacement = Arc::new(Mask::from_fn(32, 32, 0, |x, y| {
                if (x, y) == (i, i) { 255 } else { 0 }
            }));
            let weak = Arc::downgrade(&replacement);
            raster.masked_coverage_bounds(&replacement);
            drop(replacement);
            assert!(
                weak.upgrade().is_none(),
                "bounds cache must not retain mask buffers"
            );
        }
        assert!(raster.masked_bounds_cache.lock().len() <= 4);
        assert!(raster.masked_coverage_bounds(&clone).is_empty());
    }

    #[test]
    fn repeated_large_mask_queries_use_constant_time_metadata() {
        let raster = Raster::from_fn(2048, 1024, [0; 4], |x, y| {
            [x as u16, y as u16, 12345, 65535]
        });
        let white = Mask::white(2048, 1024);
        assert_eq!(raster.masked_coverage_bounds(&white), raster.bounds());
        assert!(
            raster.masked_bounds_cache.lock().is_empty(),
            "uniform reveal requires no pair cache"
        );
        let mask = Mask::from_fn(2048, 1024, 0, |x, y| {
            if (500..1800).contains(&x) && (200..900).contains(&y) {
                128
            } else {
                0
            }
        });
        let expected = IRect::new(500, 200, 1300, 700);
        assert_eq!(mask.coverage_bounds(), expected);
        assert_eq!(raster.masked_coverage_bounds(&mask), expected);
        let start = std::time::Instant::now();
        for _ in 0..20_000 {
            assert_eq!(crate::select::bounds(std::hint::black_box(&mask)), expected);
            assert_eq!(
                raster.masked_coverage_bounds(std::hint::black_box(&mask)),
                expected
            );
            assert_eq!(
                raster.masked_coverage_bounds(std::hint::black_box(&white)),
                raster.bounds()
            );
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "warm mask bounds queries must not scan or allocate image-sized buffers"
        );
    }
}
