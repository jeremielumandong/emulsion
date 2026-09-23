//! The canvas viewport: view transform, tile cache, and drawing.
//!
//! Two present paths:
//!
//! * **Tiles** (unrotated, below 2 device pixels per document pixel): each
//!   256×256 tile of the composite at the chosen mip level becomes one
//!   `RenderImage`, drawn with `paint_image`. Only changed tiles are
//!   re-rendered and re-uploaded.
//! * **Screen** (rotated, or zoomed to 200 % and beyond): GPUI samples images
//!   with linear filtering, so crisp pixels and rotation are built into one
//!   device-sized image with nearest sampling. CPU is the default; experimental
//!   GPU compute uses CPU correction at numerically ambiguous pixel edges.
//!
//! Tiles render on the background executor. While a new revision renders,
//! the previous tiles stay on screen, so slider drags never flash.

use gpui_kit::*;
use rayon::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

pub const TILE: i32 = 256;
const MAX_CACHED: usize = 480;
const BATCH: usize = 48;

/// View transform: document point `center` sits at the canvas centre,
/// scaled by `zoom` logical pixels per document pixel and rotated
/// `rotation` degrees clockwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub zoom: f64,
    pub center: (f64, f64),
    pub rotation: f64,
}

impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            center: (0.0, 0.0),
            rotation: 0.0,
        }
    }
}

pub const ZOOM_MIN: f64 = 0.01;
pub const ZOOM_MAX: f64 = 64.0;
pub const ZOOM_STEPS: &[f64] = &[
    0.01, 0.02, 0.03, 0.05, 0.0667, 0.0833, 0.125, 0.1667, 0.25, 0.3333, 0.5, 0.6667, 1.0, 2.0,
    3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0, 64.0,
];

impl View {
    fn canvas_center(b: &Bounds<Pixels>) -> (f64, f64) {
        let c = b.center();
        (f32::from(c.x) as f64, f32::from(c.y) as f64)
    }

    pub fn doc_to_screen(&self, doc: (f64, f64), canvas: &Bounds<Pixels>) -> (f64, f64) {
        let (cx, cy) = Self::canvas_center(canvas);
        let (dx, dy) = (
            (doc.0 - self.center.0) * self.zoom,
            (doc.1 - self.center.1) * self.zoom,
        );
        let (s, c) = self.rotation.to_radians().sin_cos();
        (cx + dx * c - dy * s, cy + dx * s + dy * c)
    }

    pub fn screen_to_doc(&self, p: (f64, f64), canvas: &Bounds<Pixels>) -> (f64, f64) {
        let (cx, cy) = Self::canvas_center(canvas);
        let (vx, vy) = (p.0 - cx, p.1 - cy);
        let (s, c) = (-self.rotation).to_radians().sin_cos();
        let (rx, ry) = (vx * c - vy * s, vx * s + vy * c);
        (
            self.center.0 + rx / self.zoom,
            self.center.1 + ry / self.zoom,
        )
    }

    pub fn fit(&mut self, w: u32, h: u32, canvas: &Bounds<Pixels>) {
        let (cw, ch) = (
            f32::from(canvas.size.width) as f64,
            f32::from(canvas.size.height) as f64,
        );
        let margin = 40.0;
        let z = ((cw - 2.0 * margin) / w as f64).min((ch - 2.0 * margin) / h as f64);
        self.zoom = z.clamp(ZOOM_MIN, 1.0f64.max(ZOOM_MIN));
        self.center = (w as f64 / 2.0, h as f64 / 2.0);
        self.rotation = 0.0;
    }

    /// Zoom by `factor`, keeping the document point under `anchor` fixed.
    pub fn zoom_at(&mut self, factor: f64, anchor: (f64, f64), canvas: &Bounds<Pixels>) {
        let before = self.screen_to_doc(anchor, canvas);
        self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        let after = self.screen_to_doc(anchor, canvas);
        self.center.0 += before.0 - after.0;
        self.center.1 += before.1 - after.1;
    }

    /// Next preset zoom step in or out, anchored at `anchor`.
    pub fn step(&mut self, zoom_in: bool, anchor: (f64, f64), canvas: &Bounds<Pixels>) {
        let target = if zoom_in {
            ZOOM_STEPS
                .iter()
                .copied()
                .find(|z| *z > self.zoom * 1.001)
                .unwrap_or(ZOOM_MAX)
        } else {
            ZOOM_STEPS
                .iter()
                .rev()
                .copied()
                .find(|z| *z < self.zoom / 1.001)
                .unwrap_or(ZOOM_MIN)
        };
        self.zoom_at(target / self.zoom, anchor, canvas);
    }

    /// Pan by a screen-space delta.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let (s, c) = (-self.rotation).to_radians().sin_cos();
        let (rx, ry) = (dx * c - dy * s, dx * s + dy * c);
        self.center.0 -= rx / self.zoom;
        self.center.1 -= ry / self.zoom;
    }

    /// Device pixels per document pixel.
    pub fn device_zoom(&self, scale_factor: f32) -> f64 {
        self.zoom * scale_factor as f64
    }

    /// Mip level whose pixels are at least one device pixel.
    pub fn level(&self, scale_factor: f32, max_level: u32) -> u32 {
        let dz = self.device_zoom(scale_factor);
        if dz >= 1.0 {
            return 0;
        }
        ((1.0 / dz).log2().floor().max(0.0) as u32).min(max_level)
    }

    /// Whether this view needs the CPU screen path.
    pub fn needs_screen_path(&self, scale_factor: f32) -> bool {
        self.rotation.rem_euclid(360.0) != 0.0 || self.device_zoom(scale_factor) >= 2.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Which {
    Current,
    Before,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub which: Which,
    pub level: u32,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub key: Key,
    pub rev: u64,
}

struct Entry {
    image: Arc<RenderImage>,
    rev: u64,
    last_used: u64,
}

/// Rendered tiles, shared between the editor entity and its canvas closures.
#[derive(Default)]
pub struct TileCache {
    entries: HashMap<Key, Entry>,
    pending: HashSet<(Key, u64)>,
    pub queue: Vec<Request>,
    pub to_drop: Vec<Arc<RenderImage>>,
    frame: u64,
    pub in_flight: bool,
    /// Bumped whenever tiles arrive, so the screen image knows to rebuild.
    generation: u64,
    screen: Option<(ScreenKey, Arc<RenderImage>)>,
    /// Timing of the last completed batch, for the status line.
    pub last_batch: Option<(usize, std::time::Duration)>,
    /// The view of the last frame and when it last changed: while the
    /// view is moving, the crisp CPU screen image is skipped in favour of
    /// the GPU tiles, and rebuilt once it rests.
    last_view: Option<View>,
    view_changed_at: Option<std::time::Instant>,
    /// The crisp image is due once the view settles; the editor schedules
    /// a redraw for it.
    settle_pending: bool,
    settle_wakeup_running: bool,
}

/// How long the view must rest before the crisp screen image is built.
pub const SETTLE: std::time::Duration = std::time::Duration::from_millis(90);

pub(crate) enum SettleWakeup {
    Wait(std::time::Duration),
    Redraw,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScreenKey {
    view: View,
    size: (u32, u32),
    level: u32,
    generation: u64,
    wipe: Option<i64>,
}

impl TileCache {
    /// Only one worker may wait for the crisp image, regardless of frame rate.
    pub(crate) fn start_settle_wakeup(&mut self) -> bool {
        if !self.settle_pending || self.settle_wakeup_running {
            return false;
        }
        self.settle_wakeup_running = true;
        true
    }

    /// Continued movement extends the wait without producing another frame.
    pub(crate) fn poll_settle_wakeup(&mut self, now: std::time::Instant) -> SettleWakeup {
        if !self.settle_pending {
            self.settle_wakeup_running = false;
            return SettleWakeup::Cancel;
        }
        if let Some(changed) = self.view_changed_at {
            let remaining = SETTLE.saturating_sub(now.saturating_duration_since(changed));
            if !remaining.is_zero() {
                return SettleWakeup::Wait(remaining);
            }
        }
        self.settle_pending = false;
        self.settle_wakeup_running = false;
        SettleWakeup::Redraw
    }

    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// Cached image for `key`, and whether it is at `rev`.
    fn get(&mut self, key: Key, rev: u64) -> Option<(Arc<RenderImage>, bool)> {
        let f = self.frame;
        self.entries.get_mut(&key).map(|e| {
            e.last_used = f;
            (e.image.clone(), e.rev == rev)
        })
    }

    fn request(&mut self, key: Key, rev: u64) {
        if self.pending.insert((key, rev)) {
            self.queue.push(Request { key, rev });
        }
    }

    pub fn insert(&mut self, key: Key, rev: u64, image: Arc<RenderImage>) {
        self.pending.remove(&(key, rev));
        match self.entries.get_mut(&key) {
            Some(e) if e.rev > rev => self.to_drop.push(image),
            Some(e) => {
                let old = std::mem::replace(&mut e.image, image);
                e.rev = rev;
                e.last_used = self.frame;
                self.to_drop.push(old);
            }
            None => {
                self.entries.insert(
                    key,
                    Entry {
                        image,
                        rev,
                        last_used: self.frame,
                    },
                );
            }
        }
        self.generation += 1;
    }

    /// Forget requests for revisions that are no longer wanted.
    pub fn take_batch(&mut self, current: u64, before: Option<u64>) -> Vec<Request> {
        let keep = |r: &Request| match r.key.which {
            Which::Current => r.rev == current,
            Which::Before => Some(r.rev) == before,
        };
        let (want, stale): (Vec<Request>, Vec<Request>) = self.queue.drain(..).partition(keep);
        for r in stale {
            self.pending.remove(&(r.key, r.rev));
        }
        let mut want = want;
        let rest = if want.len() > BATCH {
            want.split_off(BATCH)
        } else {
            Vec::new()
        };
        self.queue = rest;
        want
    }

    /// A new document revision changed only `dirty` (document pixels), or
    /// nothing visible when `dirty` is `None`: carry every tile at `from`
    /// that does not touch it over to `to`, so only dirty tiles re-render.
    pub fn retag(&mut self, from: u64, to: u64, dirty: Option<emulsion_raster::IRect>) {
        for (k, e) in self.entries.iter_mut() {
            if k.which != Which::Current || e.rev != from {
                continue;
            }
            let keep = match dirty {
                None => true,
                Some(d) => {
                    let s = (1i32 << k.level) * TILE;
                    let m = 2 << k.level;
                    let tile = emulsion_raster::IRect::new(k.x * s, k.y * s, s, s);
                    let grown =
                        emulsion_raster::IRect::new(d.x - m, d.y - m, d.w + 2 * m, d.h + 2 * m);
                    tile.intersect(&grown).is_empty()
                }
            };
            if keep {
                e.rev = to;
            }
        }
    }

    /// Drop everything (theme change, new document).
    pub fn clear(&mut self) {
        for (_, e) in self.entries.drain() {
            self.to_drop.push(e.image);
        }
        if let Some((_, img)) = self.screen.take() {
            self.to_drop.push(img);
        }
        self.pending.clear();
        self.queue.clear();
        self.generation += 1;
    }

    /// Hidden canvases cannot paint again to drain deferred GPU disposals.
    pub(crate) fn release(&mut self, window: &mut Window) {
        self.clear();
        for image in self.to_drop.drain(..) {
            let _ = window.drop_image(image);
        }
        self.entries.shrink_to_fit();
        self.pending.shrink_to_fit();
        self.queue.shrink_to_fit();
        self.to_drop.shrink_to_fit();
        self.in_flight = false;
        self.last_view = None;
        self.view_changed_at = None;
        self.settle_pending = false;
        self.settle_wakeup_running = false;
    }

    #[cfg(test)]
    pub(crate) fn resident_image_count(&self) -> usize {
        self.entries.len() + usize::from(self.screen.is_some()) + self.to_drop.len()
    }

    #[cfg(test)]
    pub(crate) fn pending_request_count(&self) -> usize {
        self.pending.len() + self.queue.len()
    }

    /// Drop tiles for `which` (the before/after reference changed).
    pub fn clear_which(&mut self, which: Which) {
        let keys: Vec<Key> = self
            .entries
            .keys()
            .filter(|k| k.which == which)
            .copied()
            .collect();
        for k in keys {
            if let Some(e) = self.entries.remove(&k) {
                self.to_drop.push(e.image);
            }
        }
    }

    fn evict(&mut self) {
        if self.entries.len() <= MAX_CACHED {
            return;
        }
        let mut ages: Vec<(u64, Key)> = self
            .entries
            .iter()
            .map(|(k, e)| (e.last_used, *k))
            .collect();
        ages.sort_by_key(|a| a.0);
        let excess = self.entries.len() - MAX_CACHED;
        for (used, k) in ages.into_iter().take(excess) {
            if used + 2 >= self.frame {
                break; // never evict what is on screen
            }
            if let Some(e) = self.entries.remove(&k) {
                self.to_drop.push(e.image);
            }
        }
    }
}

/// Everything the canvas needs for one frame.
#[derive(Clone)]
pub struct Scene {
    pub view: View,
    pub doc_size: (u32, u32),
    pub max_level: u32,
    pub rev: u64,
    /// Before/after: the reference revision and the wipe position in [0,1]
    /// of the canvas width. `None` when off or when nothing changed.
    pub before: Option<(u64, f32)>,
    /// RAW comparison supplies a draggable, theme-aware divider overlay.
    pub raw_compare: bool,
    pub stage: Hsla,
    pub ink: Hsla,
    pub accent: Hsla,
    pub rulers: bool,
}

pub struct Draw {
    image: Arc<RenderImage>,
    bounds: Bounds<Pixels>,
    image_bounds: Bounds<Pixels>,
}

pub struct Plan {
    draws: Vec<Draw>,
    doc_rect: Option<Bounds<Pixels>>,
    grid: Option<GridSpec>,
    wipe_x: Option<Pixels>,
    rulers: Option<RulerSpec>,
    bounds: Bounds<Pixels>,
}

struct GridSpec {
    x0: f64,
    y0: f64,
    step: f64,
    bounds: Bounds<Pixels>,
}

struct RulerSpec {
    view: View,
}

fn bpx(x: f64, y: f64, w: f64, h: f64) -> Bounds<Pixels> {
    Bounds::new(
        point(px(x as f32), px(y as f32)),
        size(px(w as f32), px(h as f32)),
    )
}

/// Screen rect of a tile at `level`.
fn tile_rect(view: &View, canvas: &Bounds<Pixels>, level: u32, x: i32, y: i32) -> Bounds<Pixels> {
    let s = (1u64 << level) as f64 * TILE as f64;
    let (x0, y0) = view.doc_to_screen((x as f64 * s, y as f64 * s), canvas);
    let (x1, y1) = view.doc_to_screen(((x + 1) as f64 * s, (y + 1) as f64 * s), canvas);
    bpx(x0, y0, x1 - x0, y1 - y0)
}

fn level_tiles(doc: (u32, u32), level: u32) -> (i32, i32) {
    let d = 1u32 << level;
    let (w, h) = (doc.0.div_ceil(d), doc.1.div_ceil(d));
    (
        w.div_ceil(TILE as u32) as i32,
        h.div_ceil(TILE as u32) as i32,
    )
}

/// Visible tile range at `level`, nearest the centre first.
fn visible_tiles(
    view: &View,
    canvas: &Bounds<Pixels>,
    doc: (u32, u32),
    level: u32,
) -> Vec<(i32, i32)> {
    let (b, s) = (canvas, (1u64 << level) as f64 * TILE as f64);
    let corners = [
        (f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64),
        (
            f32::from(b.origin.x + b.size.width) as f64,
            f32::from(b.origin.y) as f64,
        ),
        (
            f32::from(b.origin.x) as f64,
            f32::from(b.origin.y + b.size.height) as f64,
        ),
        (
            f32::from(b.origin.x + b.size.width) as f64,
            f32::from(b.origin.y + b.size.height) as f64,
        ),
    ]
    .map(|p| view.screen_to_doc(p, canvas));
    let (mut lo, mut hi) = (corners[0], corners[0]);
    for c in &corners[1..] {
        lo = (lo.0.min(c.0), lo.1.min(c.1));
        hi = (hi.0.max(c.0), hi.1.max(c.1));
    }
    let (tw, th) = level_tiles(doc, level);
    let x0 = ((lo.0 / s).floor() as i32).max(0);
    let y0 = ((lo.1 / s).floor() as i32).max(0);
    let x1 = ((hi.0 / s).floor() as i32).min(tw - 1);
    let y1 = ((hi.1 / s).floor() as i32).min(th - 1);
    let mut out = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            out.push((x, y));
        }
    }
    let (cx, cy) = (view.center.0 / s, view.center.1 / s);
    out.sort_by(|a, b| {
        let da = (a.0 as f64 + 0.5 - cx).powi(2) + (a.1 as f64 + 0.5 - cy).powi(2);
        let db = (b.0 as f64 + 0.5 - cx).powi(2) + (b.1 as f64 + 0.5 - cy).powi(2);
        da.total_cmp(&db)
    });
    out
}

/// Plan one frame: which images to draw where, and which tiles to request.
pub fn prepaint(
    scene: &Scene,
    cache: &mut TileCache,
    canvas: Bounds<Pixels>,
    scale_factor: f32,
) -> Plan {
    cache.begin_frame();
    let view = scene.view;
    let level = view.level(scale_factor, scene.max_level);
    let tiles = visible_tiles(&view, &canvas, scene.doc_size, level);
    let wipe_x = scene
        .before
        .map(|(_, w)| canvas.origin.x + canvas.size.width * w.clamp(0.0, 1.0));

    // Ask for everything visible at this level, for both sides of the wipe.
    let mut want = vec![(Which::Current, scene.rev)];
    if let Some((rev, _)) = scene.before {
        want.push((Which::Before, rev));
    }
    for &(which, rev) in &want {
        for &(x, y) in &tiles {
            let key = Key { which, level, x, y };
            match cache.get(key, rev) {
                Some((_, true)) => {}
                _ => cache.request(key, rev),
            }
        }
    }

    let (x0, y0) = view.doc_to_screen((0.0, 0.0), &canvas);
    let (x1, y1) = view.doc_to_screen((scene.doc_size.0 as f64, scene.doc_size.1 as f64), &canvas);
    let doc_rect = if view.rotation.rem_euclid(360.0) == 0.0 {
        Some(bpx(x0, y0, x1 - x0, y1 - y0))
    } else {
        None
    };

    let mut draws = Vec::new();
    if cache.last_view != Some(view) {
        cache.last_view = Some(view);
        cache.view_changed_at = Some(std::time::Instant::now());
    }
    let moving = cache.view_changed_at.is_some_and(|t| t.elapsed() < SETTLE);
    let crisp = view.needs_screen_path(scale_factor);
    // Rotation has no GPU fallback; magnification does (slightly soft).
    let use_screen = crisp && (!moving || view.rotation.rem_euclid(360.0) != 0.0);
    cache.settle_pending = crisp && !use_screen;
    if use_screen {
        if let Some(img) = screen_image(scene, cache, &canvas, scale_factor, level, &tiles) {
            draws.push(Draw {
                image: img,
                bounds: canvas,
                image_bounds: canvas,
            });
        }
    } else {
        for &(which, rev) in &want {
            let region = match (which, wipe_x) {
                (Which::Before, Some(wx)) => bpx(
                    f32::from(canvas.origin.x) as f64,
                    f32::from(canvas.origin.y) as f64,
                    f32::from(wx - canvas.origin.x) as f64,
                    f32::from(canvas.size.height) as f64,
                ),
                (Which::Current, Some(wx)) => bpx(
                    f32::from(wx) as f64,
                    f32::from(canvas.origin.y) as f64,
                    f32::from(canvas.origin.x + canvas.size.width - wx) as f64,
                    f32::from(canvas.size.height) as f64,
                ),
                _ => canvas,
            };
            for &(x, y) in &tiles {
                let rect = tile_rect(&view, &canvas, level, x, y);
                let clip = rect.intersect(&region);
                if clip.size.width <= px(0.) || clip.size.height <= px(0.) {
                    continue;
                }
                match cache.get(Key { which, level, x, y }, rev) {
                    Some((image, _)) => draws.push(Draw {
                        image,
                        bounds: clip,
                        image_bounds: rect,
                    }),
                    None => {
                        // Fall back to a coarser cached tile while this one renders.
                        for up in 1..=4u32 {
                            let pl = level + up;
                            if pl > scene.max_level {
                                break;
                            }
                            let (px_, py_) = (x >> up, y >> up);
                            if let Some((image, _)) = cache.get(
                                Key {
                                    which,
                                    level: pl,
                                    x: px_,
                                    y: py_,
                                },
                                rev,
                            ) {
                                let parent = tile_rect(&view, &canvas, pl, px_, py_);
                                draws.push(Draw {
                                    image,
                                    bounds: clip,
                                    image_bounds: parent,
                                });
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    cache.evict();

    let dz = view.device_zoom(scale_factor);
    let grid = (dz >= 8.0 && view.rotation.rem_euclid(360.0) == 0.0).then(|| GridSpec {
        x0,
        y0,
        step: view.zoom,
        bounds: doc_rect.unwrap_or(canvas).intersect(&canvas),
    });
    Plan {
        draws,
        doc_rect,
        grid,
        wipe_x,
        rulers: (scene.rulers && view.rotation.rem_euclid(360.0) == 0.0)
            .then_some(RulerSpec { view }),
        bounds: canvas,
    }
}

/// Build (or reuse) the device-sized image for the screen path.
fn screen_image(
    scene: &Scene,
    cache: &mut TileCache,
    canvas: &Bounds<Pixels>,
    sf: f32,
    level: u32,
    tiles: &[(i32, i32)],
) -> Option<Arc<RenderImage>> {
    let dw = (f32::from(canvas.size.width) * sf).round().max(1.0) as u32;
    let dh = (f32::from(canvas.size.height) * sf).round().max(1.0) as u32;
    let wipe = scene
        .before
        .map(|(_, w)| (w.clamp(0.0, 1.0) * dw as f32) as i64);
    let key = ScreenKey {
        view: scene.view,
        size: (dw, dh),
        level,
        generation: cache.generation,
        wipe,
    };
    if let Some((k, img)) = &cache.screen
        && *k == key
    {
        return Some(img.clone());
    }
    // Gather tile bytes (fresh or stale) for both sides.
    let mut cur: HashMap<(i32, i32), Arc<RenderImage>> = HashMap::new();
    let mut before: HashMap<(i32, i32), Arc<RenderImage>> = HashMap::new();
    for &(x, y) in tiles {
        if let Some((img, _)) = cache.get(
            Key {
                which: Which::Current,
                level,
                x,
                y,
            },
            scene.rev,
        ) {
            cur.insert((x, y), img);
        }
        if let Some((rev, _)) = scene.before
            && let Some((img, _)) = cache.get(
                Key {
                    which: Which::Before,
                    level,
                    x,
                    y,
                },
                rev,
            )
        {
            before.insert((x, y), img);
        }
    }
    let view = scene.view;
    let ls = (1u64 << level) as f64;
    let (lw, lh) = {
        let d = 1u32 << level;
        (
            scene.doc_size.0.div_ceil(d) as i64,
            scene.doc_size.1.div_ceil(d) as i64,
        )
    };
    let origin = (
        f32::from(canvas.origin.x) as f64,
        f32::from(canvas.origin.y) as f64,
    );
    let gpu_pixels = emulsion_gpu::screen_context().and_then(|gpu| {
        // Keep precise CPU correction at nearest-pixel boundaries. All other
        // viewport sampling (including rotated views and the compare wipe) runs
        // on the GPU; missing/failed devices retain the reference loop below.
        let start = view.screen_to_doc(
            (origin.0 + 0.5 / sf as f64, origin.1 + 0.5 / sf as f64),
            canvas,
        );
        let step_x = view.screen_to_doc(
            (origin.0 + 1.5 / sf as f64, origin.1 + 0.5 / sf as f64),
            canvas,
        );
        let step_y = view.screen_to_doc(
            (origin.0 + 0.5 / sf as f64, origin.1 + 1.5 / sf as f64),
            canvas,
        );
        let request = emulsion_gpu::screen::View {
            size: [dw, dh],
            document: [lw as u32, lh as u32],
            origin: [start.0 / ls, start.1 / ls],
            dx: [(step_x.0 - start.0) / ls, (step_x.1 - start.1) / ls],
            dy: [(step_y.0 - start.0) / ls, (step_y.1 - start.1) / ls],
            wipe: wipe.map(|w| w as u32),
        };
        let sources: Vec<_> = cur
            .iter()
            .map(|(p, img)| (p, img, false))
            .chain(before.iter().map(|(p, img)| (p, img, true)))
            .filter_map(|(&(x, y), img, before)| {
                Some(emulsion_gpu::screen::Tile {
                    x,
                    y,
                    before,
                    bgra: img.as_bytes(0)?,
                })
            })
            .collect();
        gpu.sample_screen(&request, &sources)
    });
    let mut buf = vec![0u8; (dw * dh * 4) as usize];
    let t = TILE as i64;
    buf.par_chunks_mut((dw * 4) as usize)
        .enumerate()
        .for_each(|(row, line)| {
            for col in 0..dw as usize {
                if let Some(pixels) = &gpu_pixels {
                    let [pixel, needs_correction] = pixels[row * dw as usize + col];
                    if needs_correction == 0 {
                        line[col * 4..col * 4 + 4].copy_from_slice(&pixel.to_le_bytes());
                        continue;
                    }
                }
                let sx = origin.0 + (col as f64 + 0.5) / sf as f64;
                let sy = origin.1 + (row as f64 + 0.5) / sf as f64;
                let d = view.screen_to_doc((sx, sy), canvas);
                let (lx, ly) = ((d.0 / ls).floor() as i64, (d.1 / ls).floor() as i64);
                if lx < 0 || ly < 0 || lx >= lw || ly >= lh {
                    continue;
                }
                let src = match wipe {
                    Some(w) if (col as i64) < w => &before,
                    _ => &cur,
                };
                let Some(img) = src.get(&((lx / t) as i32, (ly / t) as i32)) else {
                    continue;
                };
                let Some(bytes) = img.as_bytes(0) else {
                    continue;
                };
                let i = (((ly % t) * t + (lx % t)) * 4) as usize;
                line[col * 4..col * 4 + 4].copy_from_slice(&bytes[i..i + 4]);
            }
        });
    let img = Arc::new(bgra_image(dw, dh, buf));
    if let Some((_, old)) = cache.screen.replace((key, img.clone())) {
        cache.to_drop.push(old);
    }
    Some(img)
}

/// Wrap BGRA bytes as a GPUI image.
pub fn bgra_image(w: u32, h: u32, bgra: Vec<u8>) -> RenderImage {
    let buf = image::RgbaImage::from_raw(w, h, bgra).expect("buffer matches size");
    RenderImage::new(smallvec::SmallVec::from_elem(image::Frame::new(buf), 1))
}

/// Paint a planned frame.
pub fn paint(
    plan: Plan,
    scene: &Scene,
    cache: &Rc<RefCell<TileCache>>,
    window: &mut Window,
    cx: &mut App,
) {
    window.paint_quad(fill(plan.bounds, scene.stage));
    if let Some(r) = plan.doc_rect {
        // A hairline around the plate.
        window.paint_quad(outline(
            Bounds::new(
                r.origin - point(px(1.), px(1.)),
                r.size + size(px(2.), px(2.)),
            ),
            scene.ink.opacity(0.18),
            BorderStyle::Solid,
        ));
    }
    for d in &plan.draws {
        let _ = window.paint_image(
            d.bounds,
            d.image_bounds,
            Corners::default(),
            d.image.clone(),
            0,
            false,
        );
    }
    if let Some(g) = &plan.grid {
        paint_grid(g, scene, window);
    }
    if let Some(x) = plan.wipe_x.filter(|_| !scene.raw_compare) {
        window.paint_quad(fill(
            Bounds::new(
                point(x - px(1.), plan.bounds.origin.y),
                size(px(2.), plan.bounds.size.height),
            ),
            scene.accent,
        ));
        paint_label(
            "ORIGINAL",
            point(x - px(70.), plan.bounds.origin.y + px(10.)),
            scene,
            window,
            cx,
        );
    }
    if let Some(r) = &plan.rulers {
        paint_rulers(r, plan.bounds, scene, window, cx);
    }
    for img in cache.borrow_mut().to_drop.drain(..) {
        let _ = window.drop_image(img);
    }
}

fn paint_grid(g: &GridSpec, scene: &Scene, window: &mut Window) {
    let color = scene.ink.opacity(0.12);
    let b = g.bounds;
    let (bx0, by0) = (f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64);
    let (bx1, by1) = (
        bx0 + f32::from(b.size.width) as f64,
        by0 + f32::from(b.size.height) as f64,
    );
    let first = |o: f64, lo: f64| o + ((lo - o) / g.step).ceil() * g.step;
    let mut x = first(g.x0, bx0);
    while x < bx1 {
        window.paint_quad(fill(bpx(x.floor(), by0, 1.0, by1 - by0), color));
        x += g.step;
    }
    let mut y = first(g.y0, by0);
    while y < by1 {
        window.paint_quad(fill(bpx(bx0, y.floor(), bx1 - bx0, 1.0), color));
        y += g.step;
    }
}

fn shaped(text: &str, size_px: f32, color: Hsla, window: &Window) -> ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: font(crate::theme::MONO_FONT),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().shape_line(
        SharedString::from(text.to_string()),
        px(size_px),
        &[run],
        None,
    )
}

fn paint_label(text: &str, at: Point<Pixels>, scene: &Scene, window: &mut Window, cx: &mut App) {
    let line = shaped(text, 9.5, gpui_kit::white(), window);
    let w = line.width + px(12.);
    window.paint_quad(fill(
        Bounds::new(at, size(w, px(16.))),
        scene.ink.opacity(0.8),
    ));
    let _ = line.paint(
        at + point(px(6.), px(2.)),
        px(12.),
        TextAlign::Left,
        None,
        window,
        cx,
    );
}

const RULER: f32 = 16.0;

fn paint_rulers(
    r: &RulerSpec,
    b: Bounds<Pixels>,
    scene: &Scene,
    window: &mut Window,
    cx: &mut App,
) {
    let bg = scene.stage.blend(scene.ink.opacity(0.06));
    let tick = scene.ink.opacity(0.45);
    let text = scene.ink.opacity(0.6);
    window.paint_quad(fill(
        Bounds::new(b.origin, size(b.size.width, px(RULER))),
        bg,
    ));
    window.paint_quad(fill(
        Bounds::new(b.origin, size(px(RULER), b.size.height)),
        bg,
    ));
    // Pick a labelled step at least ~60 px apart.
    let steps = [
        1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0,
    ];
    let step = steps
        .iter()
        .copied()
        .find(|s| s * r.view.zoom >= 60.0)
        .unwrap_or(20000.0);
    let minor = step / 10.0;
    let (bx0, by0) = (f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64);
    let (bx1, by1) = (
        bx0 + f32::from(b.size.width) as f64,
        by0 + f32::from(b.size.height) as f64,
    );
    let d0 = r.view.screen_to_doc((bx0, by0), &b);
    let d1 = r.view.screen_to_doc((bx1, by1), &b);
    let mut v = (d0.0 / minor).floor() * minor;
    while v <= d1.0 {
        let (sx, _) = r.view.doc_to_screen((v, 0.0), &b);
        if sx >= bx0 + RULER as f64 {
            let major = (v / step).round() * step == v || ((v / step).fract()).abs() < 1e-9;
            let h = if major {
                RULER as f64
            } else if minor * r.view.zoom >= 6.0 {
                5.0
            } else {
                0.0
            };
            if h > 0.0 {
                window.paint_quad(fill(bpx(sx.floor(), by0 + RULER as f64 - h, 1.0, h), tick));
            }
            if major {
                let label = shaped(&format!("{}", v as i64), 8.5, text, window);
                let _ = label.paint(
                    point(px(sx as f32 + 3.), px(by0 as f32 + 1.)),
                    px(10.),
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
        }
        v += minor;
    }
    let mut v = (d0.1 / minor).floor() * minor;
    while v <= d1.1 {
        let (_, sy) = r.view.doc_to_screen((0.0, v), &b);
        if sy >= by0 + RULER as f64 {
            let major = ((v / step).fract()).abs() < 1e-9;
            let w = if major {
                RULER as f64
            } else if minor * r.view.zoom >= 6.0 {
                5.0
            } else {
                0.0
            };
            if w > 0.0 {
                window.paint_quad(fill(bpx(bx0 + RULER as f64 - w, sy.floor(), w, 1.0), tick));
            }
            if major {
                let s = format!("{}", v as i64);
                // Stack digits vertically in the narrow ruler.
                for (i, ch) in s.chars().enumerate() {
                    let label = shaped(&ch.to_string(), 8.5, text, window);
                    let _ = label.paint(
                        point(px(bx0 as f32 + 4.), px(sy as f32 + 2. + i as f32 * 9.)),
                        px(9.),
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
            }
        }
        v += minor;
    }
    window.paint_quad(fill(Bounds::new(b.origin, size(px(RULER), px(RULER))), bg));
}

/// Shared handle to the canvas's last laid-out bounds, for event handlers.
pub type CanvasBounds = Rc<Cell<Option<Bounds<Pixels>>>>;

#[cfg(test)]
mod tests {
    // Import explicitly: GPUI's glob exports its own `test` macro.
    use super::{
        SETTLE, Scene, SettleWakeup, TileCache, View, level_tiles, prepaint, visible_tiles,
    };
    use gpui_kit::{Bounds, Pixels, point, px, size};

    fn canvas() -> Bounds<Pixels> {
        Bounds::new(point(px(100.), px(50.)), size(px(800.), px(600.)))
    }

    #[test]
    fn settle_wakeup_coalesces_frames_and_waits_for_last_movement() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let frame = Duration::from_millis(8);
        let mut cache = TileCache {
            settle_pending: true,
            view_changed_at: Some(start),
            ..Default::default()
        };
        assert!(cache.start_settle_wakeup());
        // A second of movement must neither start more workers nor ask for
        // frames when an earlier deadline expires during the gesture.
        for index in 1..=120 {
            let now = start + frame * index;
            cache.view_changed_at = Some(now);
            assert!(!cache.start_settle_wakeup());
            assert!(matches!(cache.poll_settle_wakeup(now), SettleWakeup::Wait(d) if d == SETTLE));
        }
        let deadline = start + frame * 120 + SETTLE;
        assert!(
            matches!(cache.poll_settle_wakeup(deadline - frame), SettleWakeup::Wait(d) if d == frame)
        );
        assert!(matches!(
            cache.poll_settle_wakeup(deadline),
            SettleWakeup::Redraw
        ));
        assert!(!cache.start_settle_wakeup());
        assert!(matches!(
            cache.poll_settle_wakeup(deadline),
            SettleWakeup::Cancel
        ));

        // A later gesture still gets its own final crisp frame.
        cache.settle_pending = true;
        cache.view_changed_at = Some(deadline);
        assert!(cache.start_settle_wakeup());
        assert!(matches!(
            cache.poll_settle_wakeup(deadline + SETTLE),
            SettleWakeup::Redraw
        ));
    }

    #[test]
    fn settle_wakeup_cancels_when_screen_image_is_no_longer_needed() {
        let now = std::time::Instant::now();
        let mut cache = TileCache {
            settle_pending: true,
            view_changed_at: Some(now),
            ..Default::default()
        };
        assert!(cache.start_settle_wakeup());
        // Zooming out or another frame completing the crisp image removes
        // the need for the delayed redraw.
        cache.settle_pending = false;
        assert!(matches!(
            cache.poll_settle_wakeup(now + SETTLE),
            SettleWakeup::Cancel
        ));
        assert!(!cache.start_settle_wakeup());
        cache.settle_pending = true;
        assert!(cache.start_settle_wakeup());
    }

    #[test]
    fn prepaint_cancels_redundant_settle_wakeups() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(16.), px(16.)));
        let mut scene = Scene {
            view: View {
                zoom: 2.0,
                ..Default::default()
            },
            doc_size: (16, 16),
            max_level: 4,
            rev: 0,
            before: None,
            raw_compare: false,
            stage: Default::default(),
            ink: Default::default(),
            accent: Default::default(),
            rulers: false,
        };
        let mut cache = TileCache::default();
        prepaint(&scene, &mut cache, bounds, 1.0);
        assert!(cache.start_settle_wakeup());

        // Zooming below the crisp-image threshold needs no delayed frame.
        scene.view.zoom = 1.0;
        prepaint(&scene, &mut cache, bounds, 1.0);
        assert!(matches!(
            cache.poll_settle_wakeup(std::time::Instant::now()),
            SettleWakeup::Cancel
        ));

        scene.view.zoom = 2.0;
        prepaint(&scene, &mut cache, bounds, 1.0);
        assert!(cache.start_settle_wakeup());
        // Rotation paints the screen image immediately, even while moving.
        scene.view.rotation = 15.0;
        prepaint(&scene, &mut cache, bounds, 1.0);
        assert!(matches!(
            cache.poll_settle_wakeup(std::time::Instant::now()),
            SettleWakeup::Cancel
        ));

        scene.view.rotation = 0.0;
        prepaint(&scene, &mut cache, bounds, 1.0);
        assert!(cache.start_settle_wakeup());
        // An unrelated frame after the deadline can already finish the crisp
        // image before the worker runs. It must not cause another redraw.
        cache.view_changed_at = Some(std::time::Instant::now() - SETTLE);
        prepaint(&scene, &mut cache, bounds, 1.0);
        assert!(matches!(
            cache.poll_settle_wakeup(std::time::Instant::now()),
            SettleWakeup::Cancel
        ));
    }

    #[test]
    fn screen_doc_roundtrip_with_rotation() {
        let v = View {
            zoom: 0.37,
            center: (1234.0, 567.0),
            rotation: 33.0,
        };
        for p in [(0.0, 0.0), (500.0, 200.0), (6000.0, 4000.0)] {
            let s = v.doc_to_screen(p, &canvas());
            let back = v.screen_to_doc(s, &canvas());
            assert!((back.0 - p.0).abs() < 1e-6 && (back.1 - p.1).abs() < 1e-6);
        }
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut v = View {
            zoom: 0.5,
            center: (300.0, 200.0),
            rotation: 0.0,
        };
        let anchor = (420.0, 310.0);
        let before = v.screen_to_doc(anchor, &canvas());
        v.zoom_at(3.0, anchor, &canvas());
        let after = v.screen_to_doc(anchor, &canvas());
        assert!((before.0 - after.0).abs() < 1e-9 && (before.1 - after.1).abs() < 1e-9);
    }

    #[test]
    fn level_follows_device_zoom() {
        let v = |z| View {
            zoom: z,
            ..Default::default()
        };
        assert_eq!(v(1.0).level(1.0, 12), 0);
        assert_eq!(v(0.5).level(1.0, 12), 1);
        assert_eq!(v(0.5).level(2.0, 12), 0, "HiDPI keeps full resolution");
        assert_eq!(v(0.1).level(1.0, 12), 3);
        assert_eq!(v(0.1).level(1.0, 2), 2, "clamped");
    }

    #[test]
    fn fit_centres_document() {
        let mut v = View::default();
        v.fit(6000, 4000, &canvas());
        assert_eq!(v.center, (3000.0, 2000.0));
        assert!(
            (v.zoom - 0.12).abs() < 1e-12,
            "width-limited: (800 - 2·40) / 6000"
        );
    }

    #[test]
    fn visible_tiles_cover_the_view() {
        let mut v = View::default();
        v.fit(6000, 4000, &canvas());
        let level = v.level(1.0, 12);
        let t = visible_tiles(&v, &canvas(), (6000, 4000), level);
        let (tw, th) = level_tiles((6000, 4000), level);
        assert_eq!(t.len() as i32, tw * th, "fit shows the whole document");
    }
}
