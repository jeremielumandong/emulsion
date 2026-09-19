//! CPU compositor.
//!
//! [`render_tile`] renders one 256×256 output tile of a [`CompositeTree`] at
//! any mip level. Level `k` renders the document at 1/2^k scale, sampling each
//! layer from its own level-`k` mip (or the nearest level for scaled layers),
//! so zoomed-out views cost what the screen shows, not what the document
//! holds.
//!
//! The same function feeds the viewport, export, and thumbnails, so they can
//! never disagree.

use crate::adjust::Prepared;
use crate::blend::{BlendMode, BlendSpace, blend_px, dissolve_noise};
use crate::color;
use crate::geom::{IRect, TileCoord};
use crate::image::{Mask, Pix, Plane, Raster, Tile};
use crate::tile::{FTile, TILE, TILE_PX, ftile};
use glam::{DAffine2, DVec2, dvec2};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Non-destructive placement of pixel content in the document. Source pixels
/// are never resampled; shrinking and re-enlarging a layer is lossless.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    /// Clockwise degrees about the placed content's centre.
    pub rotation: f64,
    pub flip_x: bool,
    pub flip_y: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation: 0.0,
            flip_x: false,
            flip_y: false,
        }
    }
}

impl Placement {
    pub fn at(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            ..Default::default()
        }
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// Source-pixel space → document space for content of size `w × h`.
    pub fn to_doc(&self, w: u32, h: u32) -> DAffine2 {
        let (w, h) = (w as f64, h as f64);
        let flip = DAffine2::from_cols_array(&[
            if self.flip_x { -1.0 } else { 1.0 },
            0.0,
            0.0,
            if self.flip_y { -1.0 } else { 1.0 },
            if self.flip_x { w } else { 0.0 },
            if self.flip_y { h } else { 0.0 },
        ]);
        let scale = DAffine2::from_scale(dvec2(self.scale_x, self.scale_y));
        let c = dvec2(w * self.scale_x / 2.0, h * self.scale_y / 2.0);
        let rot = DAffine2::from_translation(c)
            * DAffine2::from_angle(self.rotation.to_radians())
            * DAffine2::from_translation(-c);
        DAffine2::from_translation(dvec2(self.x, self.y)) * rot * scale * flip
    }

    /// Document-space bounding box of content of size `w × h`, rounded out.
    pub fn doc_bounds(&self, w: u32, h: u32) -> IRect {
        let m = self.to_doc(w, h);
        let pts = [
            dvec2(0.0, 0.0),
            dvec2(w as f64, 0.0),
            dvec2(0.0, h as f64),
            dvec2(w as f64, h as f64),
        ]
        .map(|p| m.transform_point2(p));
        let (mut lo, mut hi) = (pts[0], pts[0]);
        for p in &pts[1..] {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let (x0, y0) = (lo.x.floor() as i32, lo.y.floor() as i32);
        IRect::new(x0, y0, hi.x.ceil() as i32 - x0, hi.y.ceil() as i32 - y0)
    }
}

/// A render-ready description of a document.
pub struct CompositeTree {
    pub width: u32,
    pub height: u32,
    pub space: BlendSpace,
    /// Bottom to top.
    pub nodes: Vec<CompositeNode>,
}

pub struct CompositeNode {
    /// Stable id; seeds Dissolve noise.
    pub id: u64,
    pub visible: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    /// For pixel content the mask lives in the layer's own pixel space and
    /// moves with it; for everything else it is in document space.
    pub mask: Option<Arc<Mask>>,
    /// Clip to the content alpha of a sibling below (index in the same list).
    pub clip_to: Option<usize>,
    pub content: NodeContent,
}

pub enum NodeContent {
    Pixels {
        raster: Arc<Raster>,
        placement: Placement,
    },
    /// Solid premultiplied linear colour over the whole document.
    Fill([f32; 4]),
    Group(Vec<CompositeNode>),
    Adjust(Arc<Prepared>),
}

#[derive(Clone, Copy)]
struct Ctx {
    level: u32,
    /// Document pixels per level pixel.
    scale: f64,
    /// Level-pixel origin of the output tile.
    ox: i64,
    oy: i64,
    space: BlendSpace,
    /// Document size, for effects defined on the whole frame.
    width: u32,
    height: u32,
}

/// Size of the document at `level`, rounding up.
pub fn level_size(width: u32, height: u32, level: u32) -> (u32, u32) {
    let d = 1u32 << level;
    (width.div_ceil(d).max(1), height.div_ceil(d).max(1))
}

/// Tile grid extent of the document at `level`.
pub fn tiles_at(width: u32, height: u32, level: u32) -> (i32, i32) {
    let (w, h) = level_size(width, height, level);
    (w.div_ceil(TILE) as i32, h.div_ceil(TILE) as i32)
}

/// Render one output tile. Pixels outside the document are transparent.
pub fn render_tile(tree: &CompositeTree, level: u32, tile: TileCoord) -> FTile {
    let mut acc = ftile();
    let (lw, lh) = level_size(tree.width, tree.height, level);
    let ox = tile.x as i64 * TILE as i64;
    let oy = tile.y as i64 * TILE as i64;
    if ox >= lw as i64 || oy >= lh as i64 || tile.x < 0 || tile.y < 0 {
        return acc;
    }
    let ctx = Ctx {
        level,
        scale: (1u64 << level) as f64,
        ox,
        oy,
        space: tree.space,
        width: tree.width,
        height: tree.height,
    };
    render_list(&tree.nodes, &mut acc, ctx);
    // Clip to the canvas.
    let vw = (lw as i64 - ox).min(TILE as i64) as usize;
    let vh = (lh as i64 - oy).min(TILE as i64) as usize;
    if vw < TILE as usize || vh < TILE as usize {
        for y in 0..TILE as usize {
            for x in 0..TILE as usize {
                if x >= vw || y >= vh {
                    acc[y * TILE as usize + x] = [0.0; 4];
                }
            }
        }
    }
    acc
}

fn render_list(nodes: &[CompositeNode], acc: &mut FTile, ctx: Ctx) {
    let is_source: Vec<bool> = (0..nodes.len())
        .map(|i| nodes.iter().any(|n| n.visible && n.clip_to == Some(i)))
        .collect();
    let mut alphas: Vec<Option<Vec<f32>>> = (0..nodes.len()).map(|_| None).collect();

    for (i, node) in nodes.iter().enumerate() {
        if !node.visible {
            continue;
        }
        let clip: Option<Vec<f32>> = match node.clip_to {
            Some(j) if j < i => {
                if !nodes[j].visible {
                    continue; // clipped to a hidden base: hidden too
                }
                alphas[j].clone()
            }
            _ => None,
        };
        let clip = clip.as_ref();
        let mask_doc = |m: &Option<Arc<Mask>>| {
            m.as_ref()
                .map(|m| sample_mask(m, &Placement::default(), ctx))
        };

        // Coverage = opacity × mask × clip, per pixel.
        let coverage = |mask: Option<&Vec<f32>>| -> Option<Vec<f32>> {
            if node.opacity >= 1.0 && mask.is_none() && clip.is_none() {
                return None;
            }
            let mut c = vec![node.opacity; TILE_PX];
            if let Some(m) = mask {
                c.iter_mut().zip(m).for_each(|(c, m)| *c *= m);
            }
            if let Some(k) = clip {
                c.iter_mut().zip(k).for_each(|(c, k)| *c *= k);
            }
            Some(c)
        };

        match &node.content {
            NodeContent::Pixels { raster, placement } => {
                let mut src = ftile();
                if !sample_raster(&mut src, raster, placement, ctx) {
                    if is_source[i] {
                        alphas[i] = Some(vec![0.0; TILE_PX]);
                    }
                    continue;
                }
                let mask = node.mask.as_ref().map(|m| sample_mask(m, placement, ctx));
                if let Some(m) = &mask {
                    src.iter_mut()
                        .zip(m)
                        .for_each(|(p, m)| p.iter_mut().for_each(|v| *v *= m));
                }
                if is_source[i] {
                    alphas[i] = Some(src.iter().map(|p| p[3]).collect());
                }
                let cov = coverage(None);
                composite_into(acc, &mut src, cov.as_deref(), node, ctx);
            }
            NodeContent::Fill(c) => {
                let mut src = vec![*c; TILE_PX];
                let mask = mask_doc(&node.mask);
                if let Some(m) = &mask {
                    src.iter_mut()
                        .zip(m)
                        .for_each(|(p, m)| p.iter_mut().for_each(|v| *v *= m));
                }
                if is_source[i] {
                    alphas[i] = Some(src.iter().map(|p| p[3]).collect());
                }
                let cov = coverage(None);
                composite_into(acc, &mut src, cov.as_deref(), node, ctx);
            }
            NodeContent::Group(children) => {
                let mask = mask_doc(&node.mask);
                if node.blend == BlendMode::PassThrough {
                    match coverage(mask.as_ref()) {
                        None => render_list(children, acc, ctx),
                        Some(cov) => {
                            let before = acc.clone();
                            render_list(children, acc, ctx);
                            for ((a, b), k) in acc.iter_mut().zip(&before).zip(&cov) {
                                for c in 0..4 {
                                    a[c] = b[c] + (a[c] - b[c]) * k;
                                }
                            }
                        }
                    }
                } else {
                    let mut sub = ftile();
                    render_list(children, &mut sub, ctx);
                    if let Some(m) = &mask {
                        sub.iter_mut()
                            .zip(m)
                            .for_each(|(p, m)| p.iter_mut().for_each(|v| *v *= m));
                    }
                    if is_source[i] {
                        alphas[i] = Some(sub.iter().map(|p| p[3]).collect());
                    }
                    let cov = coverage(None);
                    composite_into(acc, &mut sub, cov.as_deref(), node, ctx);
                }
            }
            NodeContent::Adjust(op) => {
                let mask = mask_doc(&node.mask);
                let cov = coverage(mask.as_ref());
                apply_adjust(acc, op, cov.as_deref(), node.blend, ctx.space, ctx);
            }
        }
    }
}

/// Scale `src` by coverage and blend it into `acc` with the node's mode.
fn composite_into(
    acc: &mut FTile,
    src: &mut FTile,
    cov: Option<&[f32]>,
    node: &CompositeNode,
    ctx: Ctx,
) {
    let mode = if node.blend == BlendMode::PassThrough {
        BlendMode::Normal
    } else {
        node.blend
    };
    for (idx, (a, s)) in acc.iter_mut().zip(src.iter_mut()).enumerate() {
        if let Some(c) = cov {
            let k = c[idx];
            if k <= 0.0 {
                continue;
            }
            s.iter_mut().for_each(|v| *v *= k);
        }
        if s[3] <= 0.0 {
            continue;
        }
        let noise = if mode == BlendMode::Dissolve {
            let x = ctx.ox + (idx % TILE as usize) as i64;
            let y = ctx.oy + (idx / TILE as usize) as i64;
            dissolve_noise(x as i32, y as i32, node.id ^ ((ctx.level as u64) << 56))
        } else {
            0.0
        };
        *a = blend_px(mode, ctx.space, *a, *s, noise);
    }
}

fn apply_adjust(
    acc: &mut FTile,
    op: &Prepared,
    cov: Option<&[f32]>,
    mode: BlendMode,
    space: BlendSpace,
    ctx: Ctx,
) {
    let positional = op.positional();
    for (idx, p) in acc.iter_mut().enumerate() {
        let a = p[3];
        if a <= 0.0 {
            continue;
        }
        let k = cov.map_or(1.0, |c| c[idx]);
        if k <= 0.0 {
            continue;
        }
        let inv = 1.0 / a;
        let rgb = [p[0] * inv, p[1] * inv, p[2] * inv];
        let adj = if positional {
            // Grain is defined on document pixels at full size; scale up at
            // reduced levels so it stays the same size on screen.
            let x = (ctx.ox + (idx % TILE as usize) as i64) << ctx.level;
            let y = (ctx.oy + (idx / TILE as usize) as i64) << ctx.level;
            op.apply_at(rgb, x as i32, y as i32, ctx.width, ctx.height)
        } else {
            op.apply(rgb)
        };
        let target = match mode {
            BlendMode::Normal | BlendMode::PassThrough | BlendMode::Dissolve => adj,
            m => {
                let src = [
                    adj[0].clamp(0.0, 1.0),
                    adj[1].clamp(0.0, 1.0),
                    adj[2].clamp(0.0, 1.0),
                    1.0,
                ];
                let dst = [
                    rgb[0].clamp(0.0, 1.0),
                    rgb[1].clamp(0.0, 1.0),
                    rgb[2].clamp(0.0, 1.0),
                    1.0,
                ];
                let o = blend_px(m, space, dst, src, 0.0);
                [o[0], o[1], o[2]]
            }
        };
        for c in 0..3 {
            p[c] = (rgb[c] + (target[c] - rgb[c]) * k) * a;
        }
    }
}

/// Pick the source mip level for content drawn with `to_doc` at `scale`
/// document pixels per output pixel.
fn source_level(to_doc: &DAffine2, scale: f64, max_level: u32) -> u32 {
    let det = to_doc.matrix2.determinant().abs().max(1e-12);
    let src_per_out = scale / det.sqrt();
    if src_per_out < 2.0 {
        return 0;
    }
    (src_per_out.log2().floor() as u32).min(max_level)
}

/// The sampling grid for one output tile: source-level coordinates of the
/// first pixel centre, and per-pixel steps along x and y.
struct Grid {
    level: u32,
    p0: DVec2,
    ex: DVec2,
    ey: DVec2,
}

fn grid(to_doc: &DAffine2, max_level: u32, ctx: Ctx) -> Grid {
    let level = source_level(to_doc, ctx.scale, max_level);
    let inv = to_doc.inverse();
    let ls = (1u64 << level) as f64;
    let f = |lx: f64, ly: f64| {
        inv.transform_point2(dvec2((lx + 0.5) * ctx.scale, (ly + 0.5) * ctx.scale)) / ls
    };
    let p0 = f(ctx.ox as f64, ctx.oy as f64);
    Grid {
        level,
        p0,
        ex: f(ctx.ox as f64 + 1.0, ctx.oy as f64) - p0,
        ey: f(ctx.ox as f64, ctx.oy as f64 + 1.0) - p0,
    }
}

/// Source tiles covering one output tile, fetched once.
struct Window<P: Pix> {
    tx0: i32,
    ty0: i32,
    cols: i32,
    rows: i32,
    tiles: Vec<Option<Tile<P>>>,
    fill: P,
    lw: i64,
    lh: i64,
}

impl<P: Pix> Window<P> {
    fn new(plane: &Plane<P>, g: &Grid) -> Option<Self> {
        let n = TILE as f64 - 1.0;
        let corners = [
            g.p0,
            g.p0 + g.ex * n,
            g.p0 + g.ey * n,
            g.p0 + g.ex * n + g.ey * n,
        ];
        let (mut lo, mut hi) = (corners[0], corners[0]);
        for c in &corners[1..] {
            lo = lo.min(*c);
            hi = hi.max(*c);
        }
        let (lw, lh) = plane.level_size(g.level);
        let x0 = (lo.x - 1.0).floor().max(0.0);
        let y0 = (lo.y - 1.0).floor().max(0.0);
        let x1 = (hi.x + 1.0).ceil().min(lw as f64 - 1.0);
        let y1 = (hi.y + 1.0).ceil().min(lh as f64 - 1.0);
        if x1 < x0 || y1 < y0 {
            return None;
        }
        let t = TILE as i32;
        let (tx0, ty0) = (x0 as i32 / t, y0 as i32 / t);
        let (tx1, ty1) = (x1 as i32 / t, y1 as i32 / t);
        let (cols, rows) = (tx1 - tx0 + 1, ty1 - ty0 + 1);
        let mut tiles = Vec::with_capacity((cols * rows) as usize);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                tiles.push(plane.tile(g.level, TileCoord::new(tx, ty)));
            }
        }
        Some(Self {
            tx0,
            ty0,
            cols,
            rows,
            tiles,
            fill: plane.fill(),
            lw: lw as i64,
            lh: lh as i64,
        })
    }

    /// Pixel at level coordinates; `outside` beyond the image.
    #[inline]
    fn get(&self, x: i64, y: i64, outside: P) -> P {
        if x < 0 || y < 0 || x >= self.lw || y >= self.lh {
            return outside;
        }
        let t = TILE as i64;
        let cx = (x / t) as i32 - self.tx0;
        let cy = (y / t) as i32 - self.ty0;
        if cx < 0 || cy < 0 || cx >= self.cols || cy >= self.rows {
            return outside;
        }
        match &self.tiles[(cy * self.cols + cx) as usize] {
            Some(tile) => tile[((y % t) * t + (x % t)) as usize],
            None => self.fill,
        }
    }
}

#[inline]
fn is_integral(v: f64) -> bool {
    (v - v.round()).abs() < 1e-9
}

/// Sample a raster through `placement` into `dst`. Returns false when the
/// content does not touch this tile.
fn sample_raster(dst: &mut FTile, raster: &Raster, placement: &Placement, ctx: Ctx) -> bool {
    let to_doc = placement.to_doc(raster.width(), raster.height());
    let g = grid(&to_doc, raster.max_level(), ctx);
    let Some(win) = Window::new(raster, &g) else {
        return false;
    };
    if win.tiles.iter().all(Option::is_none) && win.fill == [0; 4] {
        return false;
    }
    let exact = g.ex == dvec2(1.0, 0.0)
        && g.ey == dvec2(0.0, 1.0)
        && is_integral(g.p0.x - 0.5)
        && is_integral(g.p0.y - 0.5);
    let t = TILE as usize;
    if exact {
        let sx = (g.p0.x - 0.5).round() as i64;
        let sy = (g.p0.y - 0.5).round() as i64;
        for y in 0..t {
            for x in 0..t {
                dst[y * t + x] = color::px_to_f(win.get(sx + x as i64, sy + y as i64, [0; 4]));
            }
        }
        return true;
    }
    for y in 0..t {
        let row = g.p0 + g.ey * y as f64;
        for x in 0..t {
            let p = row + g.ex * x as f64 - dvec2(0.5, 0.5);
            let (fx, fy) = (p.x.floor(), p.y.floor());
            let (ix, iy) = (fx as i64, fy as i64);
            let (ax, ay) = ((p.x - fx) as f32, (p.y - fy) as f32);
            let s00 = color::px_to_f(win.get(ix, iy, [0; 4]));
            let s10 = color::px_to_f(win.get(ix + 1, iy, [0; 4]));
            let s01 = color::px_to_f(win.get(ix, iy + 1, [0; 4]));
            let s11 = color::px_to_f(win.get(ix + 1, iy + 1, [0; 4]));
            let mut o = [0.0; 4];
            for c in 0..4 {
                let top = s00[c] + (s10[c] - s00[c]) * ax;
                let bot = s01[c] + (s11[c] - s01[c]) * ax;
                o[c] = top + (bot - top) * ay;
            }
            dst[y * t + x] = o;
        }
    }
    true
}

/// Sample a mask through `placement` as coverage in [0,1].
fn sample_mask(mask: &Mask, placement: &Placement, ctx: Ctx) -> Vec<f32> {
    let to_doc = placement.to_doc(mask.width(), mask.height());
    let g = grid(&to_doc, mask.max_level(), ctx);
    let fill = mask.fill() as f32 / 255.0;
    let Some(win) = Window::new(mask, &g) else {
        return vec![fill; TILE_PX];
    };
    let t = TILE as usize;
    let mut out = vec![0.0; TILE_PX];
    let f = mask.fill();
    for y in 0..t {
        let row = g.p0 + g.ey * y as f64;
        for x in 0..t {
            let p = row + g.ex * x as f64 - dvec2(0.5, 0.5);
            let (fx, fy) = (p.x.floor(), p.y.floor());
            let (ix, iy) = (fx as i64, fy as i64);
            let (ax, ay) = ((p.x - fx) as f32, (p.y - fy) as f32);
            let s = |dx, dy| win.get(ix + dx, iy + dy, f) as f32;
            let top = s(0, 0) + (s(1, 0) - s(0, 0)) * ax;
            let bot = s(0, 1) + (s(1, 1) - s(0, 1)) * ax;
            out[y * t + x] = (top + (bot - top) * ay) / 255.0;
        }
    }
    out
}

/// Render one document-space region at full resolution, row-major. Pixels
/// outside the canvas are transparent.
pub fn region(tree: &CompositeTree, rect: IRect) -> Vec<[f32; 4]> {
    let t = TILE as i32;
    let mut out = vec![[0.0f32; 4]; (rect.w.max(0) * rect.h.max(0)) as usize];
    let clip = rect.intersect(&IRect::new(0, 0, tree.width as i32, tree.height as i32));
    if clip.is_empty() {
        return out;
    }
    let coords: Vec<TileCoord> = (clip.y.div_euclid(t)..=(clip.bottom() - 1).div_euclid(t))
        .flat_map(|ty| {
            (clip.x.div_euclid(t)..=(clip.right() - 1).div_euclid(t))
                .map(move |tx| TileCoord::new(tx, ty))
        })
        .collect();
    let tiles: Vec<(TileCoord, FTile)> = coords
        .into_par_iter()
        .map(|c| (c, render_tile(tree, 0, c)))
        .collect();
    for (c, tile) in tiles {
        let tr = IRect::new(c.x * t, c.y * t, t, t).intersect(&clip);
        for y in tr.y..tr.bottom() {
            let src = ((y - c.y * t) * t + (tr.x - c.x * t)) as usize;
            let dst = ((y - rect.y) * rect.w + (tr.x - rect.x)) as usize;
            out[dst..dst + tr.w as usize].copy_from_slice(&tile[src..src + tr.w as usize]);
        }
    }
    out
}

/// Render the whole document at `level` into a premultiplied RGBA16 raster.
pub fn flatten(tree: &CompositeTree, level: u32) -> Raster {
    let (lw, lh) = level_size(tree.width, tree.height, level);
    let (tx, ty) = tiles_at(tree.width, tree.height, level);
    let coords: Vec<TileCoord> = (0..ty)
        .flat_map(|y| (0..tx).map(move |x| TileCoord::new(x, y)))
        .collect();
    let tiles: Vec<(TileCoord, Vec<[u16; 4]>)> = coords
        .into_par_iter()
        .map(|c| {
            (
                c,
                render_tile(tree, level, c)
                    .into_iter()
                    .map(color::f_to_px)
                    .collect(),
            )
        })
        .collect();
    let mut out = Raster::transparent(lw, lh);
    for (c, t) in tiles {
        out.set_tile(c, t);
    }
    out
}

/// Encode a rendered tile as BGRA8 for display, composited over a
/// checkerboard. `origin` is the tile's level-pixel origin; pixels outside
/// `valid` (level size) become fully transparent.
pub fn tile_to_bgra8(
    tile: &FTile,
    origin: (i64, i64),
    valid: (u32, u32),
    cell: u32,
    light: u8,
    dark: u8,
) -> Vec<u8> {
    let t = TILE as usize;
    let ll = color::SRGB8_TO_LINEAR[light as usize];
    let ld = color::SRGB8_TO_LINEAR[dark as usize];
    let mut out = vec![0u8; TILE_PX * 4];
    for y in 0..t {
        let gy = origin.1 + y as i64;
        for x in 0..t {
            let gx = origin.0 + x as i64;
            if gx >= valid.0 as i64 || gy >= valid.1 as i64 {
                continue;
            }
            let p = tile[y * t + x];
            let a = p[3].clamp(0.0, 1.0);
            let bg = if ((gx / cell as i64) + (gy / cell as i64)) % 2 == 0 {
                ll
            } else {
                ld
            };
            let k = 1.0 - a;
            let i = (y * t + x) * 4;
            out[i] = color::linear_to_srgb8(p[2] + bg * k);
            out[i + 1] = color::linear_to_srgb8(p[1] + bg * k);
            out[i + 2] = color::linear_to_srgb8(p[0] + bg * k);
            out[i + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adjust::Adjustment;

    fn px(n: &CompositeNode) -> CompositeNode {
        CompositeNode {
            id: n.id,
            visible: n.visible,
            opacity: n.opacity,
            blend: n.blend,
            mask: n.mask.clone(),
            clip_to: n.clip_to,
            content: match &n.content {
                NodeContent::Pixels { raster, placement } => NodeContent::Pixels {
                    raster: raster.clone(),
                    placement: *placement,
                },
                _ => unreachable!(),
            },
        }
    }

    fn layer(id: u64, raster: Raster) -> CompositeNode {
        CompositeNode {
            id,
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            mask: None,
            clip_to: None,
            content: NodeContent::Pixels {
                raster: Arc::new(raster),
                placement: Placement::default(),
            },
        }
    }

    fn tree(nodes: Vec<CompositeNode>) -> CompositeTree {
        CompositeTree {
            width: 300,
            height: 300,
            space: BlendSpace::Linear,
            nodes,
        }
    }

    fn at(t: &FTile, x: usize, y: usize) -> [f32; 4] {
        t[y * TILE as usize + x]
    }

    #[test]
    fn opacity_and_hidden() {
        let red = layer(1, Raster::solid(300, 300, [1.0, 0.0, 0.0, 1.0]));
        let mut half = px(&red);
        half.opacity = 0.5;
        let out = render_tile(&tree(vec![half]), 0, TileCoord::new(0, 0));
        assert!((at(&out, 10, 10)[3] - 0.5).abs() < 1e-3);
        let mut hidden = px(&red);
        hidden.visible = false;
        let out = render_tile(&tree(vec![hidden]), 0, TileCoord::new(0, 0));
        assert_eq!(at(&out, 10, 10), [0.0; 4]);
    }

    #[test]
    fn canvas_clips_edge_tiles() {
        let red = layer(1, Raster::solid(300, 300, [1.0, 0.0, 0.0, 1.0]));
        let out = render_tile(&tree(vec![red]), 0, TileCoord::new(1, 1));
        assert!(at(&out, 10, 10)[3] > 0.99, "inside 300×300");
        assert_eq!(at(&out, 60, 60), [0.0; 4], "outside the canvas");
    }

    #[test]
    fn integer_placement_is_exact() {
        let mut data = vec![0u8; 4 * 4 * 4];
        data[0..4].copy_from_slice(&[255, 255, 255, 255]);
        let mut n = layer(1, Raster::from_srgba8(4, 4, &data));
        if let NodeContent::Pixels { placement, .. } = &mut n.content {
            *placement = Placement::at(7.0, 3.0);
        }
        let out = render_tile(&tree(vec![n]), 0, TileCoord::new(0, 0));
        assert!(at(&out, 7, 3)[3] > 0.999);
        assert_eq!(at(&out, 8, 3)[3], 0.0);
        assert_eq!(at(&out, 6, 3)[3], 0.0);
    }

    #[test]
    fn mip_render_matches_downsampled_full_render() {
        let r = Raster::from_fn(512, 512, [0; 4], |x, y| {
            let v = if (x / 8 + y / 8) % 2 == 0 {
                60000
            } else {
                5000
            };
            [v, v, v, 65535]
        });
        let t = CompositeTree {
            width: 512,
            height: 512,
            space: BlendSpace::Linear,
            nodes: vec![layer(1, r)],
        };
        let full = flatten(&t, 0);
        let half = render_tile(&t, 1, TileCoord::new(0, 0));
        for (x, y) in [(0u32, 0u32), (13, 77), (200, 100)] {
            let mut acc = 0.0;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                acc += color::px_to_f(full.get(2 * x + dx, 2 * y + dy))[0];
            }
            assert!((acc / 4.0 - at(&half, x as usize, y as usize)[0]).abs() < 2e-3);
        }
    }

    #[test]
    fn adjustment_applies_to_backdrop_only_below() {
        let grey = layer(1, Raster::solid(300, 300, [0.2, 0.2, 0.2, 1.0]));
        let adj = CompositeNode {
            id: 2,
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            mask: None,
            clip_to: None,
            content: NodeContent::Adjust(Arc::new(
                Adjustment::Exposure {
                    exposure: 1.0,
                    offset: 0.0,
                    gamma: 1.0,
                }
                .prepare(),
            )),
        };
        let out = render_tile(&tree(vec![grey, adj]), 0, TileCoord::new(0, 0));
        assert!((at(&out, 5, 5)[0] - 0.4).abs() < 2e-3);
    }

    #[test]
    fn clipping_limits_to_base_alpha() {
        // Base covers only the left half; clipped red covers everything.
        let base = layer(
            1,
            Raster::from_fn(300, 300, [0; 4], |x, _| {
                if x < 150 {
                    [0, 0, 65535, 65535]
                } else {
                    [0; 4]
                }
            }),
        );
        let mut red = layer(2, Raster::solid(300, 300, [1.0, 0.0, 0.0, 1.0]));
        red.clip_to = Some(0);
        let out = render_tile(&tree(vec![base, red]), 0, TileCoord::new(0, 0));
        assert!(at(&out, 10, 10)[0] > 0.99, "red over base");
        assert_eq!(at(&out, 200, 10)[3], 0.0, "nothing outside the base");
    }

    #[test]
    fn isolated_group_vs_pass_through() {
        // Multiply inside a group: isolated sees a transparent backdrop.
        let bg = layer(1, Raster::solid(300, 300, [0.5, 0.5, 0.5, 1.0]));
        let mut mul = layer(2, Raster::solid(300, 300, [0.5, 0.5, 0.5, 1.0]));
        mul.blend = BlendMode::Multiply;
        let group = |blend| CompositeNode {
            id: 3,
            visible: true,
            opacity: 1.0,
            blend,
            mask: None,
            clip_to: None,
            content: NodeContent::Group(vec![px(&mul)]),
        };
        let pass = render_tile(
            &tree(vec![px(&bg), group(BlendMode::PassThrough)]),
            0,
            TileCoord::new(0, 0),
        );
        let iso = render_tile(
            &tree(vec![px(&bg), group(BlendMode::Normal)]),
            0,
            TileCoord::new(0, 0),
        );
        assert!(
            (at(&pass, 1, 1)[0] - 0.25).abs() < 2e-3,
            "multiplied with backdrop"
        );
        assert!(
            (at(&iso, 1, 1)[0] - 0.5).abs() < 2e-3,
            "isolated group is plain grey"
        );
    }

    #[test]
    fn scaled_placement_samples_smaller_mip() {
        let r = Raster::solid(1000, 1000, [0.0, 1.0, 0.0, 1.0]);
        let mut n = layer(1, r);
        if let NodeContent::Pixels { placement, .. } = &mut n.content {
            placement.scale_x = 0.25;
            placement.scale_y = 0.25;
        }
        let out = render_tile(&tree(vec![n]), 0, TileCoord::new(0, 0));
        assert!(at(&out, 100, 100)[1] > 0.99);
        assert_eq!(at(&out, 251, 100)[3], 0.0);
    }
}
