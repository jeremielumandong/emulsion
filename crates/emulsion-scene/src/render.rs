//! The CPU renderer (C8, C11, SG7, V6): perspective or orthographic
//! projection, z-buffer, back-face culling, near-plane clipping, toon
//! shading with 2–3 bands, contour lines from depth/normal/object-id
//! discontinuities, drawn faces, ground grid and horizon.
//!
//! Output is straight-alpha RGBA8. Rendering is split into horizontal strips
//! processed with rayon; every pixel's result depends only on the input, so
//! output is bit-for-bit deterministic regardless of thread count.

use glam::{Vec2, Vec3};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::camera::{Camera, View, ViewKind};
use crate::prepare::PreparedScene;
use crate::scene::{ObjectId, Rgb};

/// Visual style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderStyle {
    /// Cel-shaded colour bands with contour lines.
    #[default]
    Toon,
    /// Smoothly shaded flat grey with contour lines.
    Clay,
    /// Contour lines only, on white (for tracing).
    Outline,
    /// Solid black shapes, no lines.
    Silhouette,
}

impl RenderStyle {
    pub const ALL: [RenderStyle; 4] = [
        RenderStyle::Toon,
        RenderStyle::Clay,
        RenderStyle::Outline,
        RenderStyle::Silhouette,
    ];
}

/// Render settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderOptions {
    pub style: RenderStyle,
    /// Leave sky and ground transparent (lines and grid stay).
    pub transparent_background: bool,
    /// Contour thickness in output pixels.
    pub line_width: f32,
    pub line_color: Rgb,
    /// Normal change (degrees) that draws a crease line.
    pub crease_angle: f32,
    /// Toon bands: 2 or 3.
    pub toon_bands: u8,
    /// Supersampling factor 1–4 (antialiasing for snapshots).
    pub supersample: u8,
    /// Draw face presets on mannequin heads.
    pub faces: bool,
    /// Override the scene's ground/grid/horizon switches.
    pub show_ground: Option<bool>,
    pub show_grid: Option<bool>,
    pub show_horizon: Option<bool>,
    /// Draw this object's contours in the highlight colour (selection).
    pub highlight: Option<ObjectId>,
    pub highlight_color: Rgb,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            style: RenderStyle::Toon,
            transparent_background: false,
            line_width: 1.5,
            line_color: Rgb([34, 34, 38]),
            crease_angle: 40.0,
            toon_bands: 3,
            supersample: 1,
            faces: true,
            show_ground: None,
            show_grid: None,
            show_horizon: None,
            highlight: None,
            highlight_color: Rgb([0, 122, 255]),
        }
    }
}

impl RenderOptions {
    pub fn style(style: RenderStyle) -> Self {
        RenderOptions {
            style,
            ..Default::default()
        }
    }
}

/// A straight-alpha RGBA8 image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    /// FNV-1a hash of the pixels (for golden tests).
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self
            .width
            .to_le_bytes()
            .iter()
            .chain(self.height.to_le_bytes().iter())
            .chain(self.pixels.iter())
        {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// Largest output side accepted.
pub const MAX_RENDER_SIDE: u32 = 16_384;

const STRIP: usize = 16;

#[derive(Clone, Copy)]
struct STri {
    /// Pixel x, y.
    p: [Vec2; 3],
    /// Depth per vertex (camera z).
    z: [f32; 3],
    /// Perspective weight per vertex (1/z, or 1 for orthographic).
    q: [f32; 3],
    n: [Vec3; 3],
    /// Mesh index + 1.
    mesh: u32,
}

/// G-buffer of one render.
struct GBuffer {
    w: usize,
    h: usize,
    depth: Vec<f32>,
    normal: Vec<Vec3>,
    id: Vec<u32>,
}

/// Renders a prepared scene through `camera`.
pub fn render(
    scene: &PreparedScene,
    camera: &Camera,
    width: u32,
    height: u32,
    opts: &RenderOptions,
) -> RgbaImage {
    let width = width.clamp(1, MAX_RENDER_SIDE);
    let height = height.clamp(1, MAX_RENDER_SIDE);
    let ss =
        (opts.supersample.clamp(1, 4) as u32).min((MAX_RENDER_SIDE / width.max(height)).max(1));
    let (w, h) = (width * ss, height * ss);
    let view = camera.view(w, h);
    let tris = screen_triangles(scene, &view);
    let g = rasterize(&tris, &view, w as usize, h as usize);
    let lw = (opts.line_width.max(0.0) * ss as f32).min(64.0);
    let (lines, line_hl) = contour_lines(scene, &g, &view, opts, lw);
    let lines = draw_faces(scene, &g, &view, opts, lw, lines);
    let img = compose(scene, &g, &view, opts, &lines, &line_hl, lw);
    if ss == 1 {
        RgbaImage {
            width,
            height,
            pixels: img,
        }
    } else {
        downsample(&img, w as usize, h as usize, ss as usize, width, height)
    }
}

fn screen_triangles(scene: &PreparedScene, view: &View) -> Vec<STri> {
    let per_mesh: Vec<Vec<STri>> = scene
        .meshes
        .par_iter()
        .enumerate()
        .map(|(mi, m)| {
            let mut out = Vec::new();
            if !frustum_may_see(view, &m.bounds) {
                return out;
            }
            let cam: Vec<Vec3> = m
                .mesh
                .positions
                .iter()
                .map(|p| view.to_camera(*p))
                .collect();
            let nrm = &m.mesh.normals;
            for t in &m.mesh.indices {
                let vs = t.map(|i| {
                    (
                        cam[i as usize],
                        nrm.get(i as usize).copied().unwrap_or(Vec3::Y),
                    )
                });
                clip_and_emit(view, vs, mi as u32 + 1, m.double_sided, &mut out);
            }
            out
        })
        .collect();
    per_mesh.into_iter().flatten().collect()
}

fn frustum_may_see(view: &View, b: &crate::math::Aabb) -> bool {
    if b.is_empty() {
        return false;
    }
    let corners = b.corners().map(|c| view.to_camera(c));
    if corners.iter().all(|c| c.z < view.near) {
        return false;
    }
    match view.kind {
        ViewKind::Perspective { tan_half } => {
            let out = |f: &dyn Fn(&Vec3) -> bool| corners.iter().all(f);
            !(out(&|c| c.x > c.z * tan_half.x)
                || out(&|c| c.x < -c.z * tan_half.x)
                || out(&|c| c.y > c.z * tan_half.y)
                || out(&|c| c.y < -c.z * tan_half.y))
        }
        ViewKind::Orthographic { half } => {
            let out = |f: &dyn Fn(&Vec3) -> bool| corners.iter().all(f);
            !(out(&|c| c.x > half.x)
                || out(&|c| c.x < -half.x)
                || out(&|c| c.y > half.y)
                || out(&|c| c.y < -half.y))
        }
    }
}

fn clip_and_emit(
    view: &View,
    vs: [(Vec3, Vec3); 3],
    mesh: u32,
    double_sided: bool,
    out: &mut Vec<STri>,
) {
    let near = view.near;
    let inside = vs.map(|(c, _)| c.z >= near);
    let mut poly: [(Vec3, Vec3); 4] = [vs[0]; 4];
    let mut n = 0;
    if inside.iter().all(|i| *i) {
        poly[..3].copy_from_slice(&vs);
        n = 3;
    } else if inside.iter().any(|i| *i) {
        for i in 0..3 {
            let a = vs[i];
            let b = vs[(i + 1) % 3];
            let (ia, ib) = (a.0.z >= near, b.0.z >= near);
            if ia {
                poly[n] = a;
                n += 1;
            }
            if ia != ib {
                let t = (near - a.0.z) / (b.0.z - a.0.z);
                poly[n] = (a.0.lerp(b.0, t), a.1.lerp(b.1, t));
                n += 1;
            }
        }
    }
    if n < 3 {
        return;
    }
    let proj = |c: Vec3| view.ndc_to_pixel(view.camera_to_ndc(c));
    let persp = view.is_perspective();
    for k in 1..n - 1 {
        let tri = [poly[0], poly[k], poly[k + 1]];
        let p = tri.map(|(c, _)| proj(c));
        if !p.iter().all(|v| v.is_finite()) {
            continue;
        }
        let area = (p[1] - p[0]).perp_dot(p[2] - p[0]);
        if area.abs() < 1e-9 {
            continue;
        }
        // Pixel y points down, so front faces (CCW in the image) have negative area.
        let front = area < 0.0;
        let mut ns = tri.map(|(_, n)| n);
        if !front {
            if !double_sided {
                continue;
            }
            ns = ns.map(|n| -n);
        }
        let z = tri.map(|(c, _)| c.z);
        out.push(STri {
            p,
            z,
            q: if persp { z.map(|z| 1.0 / z) } else { [1.0; 3] },
            n: ns,
            mesh,
        });
    }
}

fn rasterize(tris: &[STri], view: &View, w: usize, h: usize) -> GBuffer {
    let strips = h.div_ceil(STRIP);
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); strips];
    for (i, t) in tris.iter().enumerate() {
        let ymin = t.p.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let ymax = t.p.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
        let xmin = t.p.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let xmax = t.p.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
        if ymax < 0.0 || ymin > h as f32 || xmax < 0.0 || xmin > w as f32 {
            continue;
        }
        let s0 = ((ymin.max(0.0) as usize) / STRIP).min(strips - 1);
        let s1 = ((ymax.min(h as f32 - 1.0).max(0.0) as usize) / STRIP).min(strips - 1);
        for bin in &mut bins[s0..=s1] {
            bin.push(i as u32);
        }
    }
    let mut depth = vec![f32::INFINITY; w * h];
    let mut normal = vec![Vec3::ZERO; w * h];
    let mut id = vec![0u32; w * h];
    let far = view.far;
    depth
        .par_chunks_mut(w * STRIP)
        .zip(normal.par_chunks_mut(w * STRIP))
        .zip(id.par_chunks_mut(w * STRIP))
        .enumerate()
        .for_each(|(s, ((dep, nor), ids))| {
            let y0 = s * STRIP;
            let rows = dep.len() / w;
            for &ti in &bins[s] {
                let t = &tris[ti as usize];
                let [a, b, c] = t.p;
                let area = (b - a).perp_dot(c - a);
                let inv_area = 1.0 / area;
                let xmin = a.x.min(b.x).min(c.x).floor().max(0.0) as usize;
                let xmax =
                    (a.x.max(b.x).max(c.x).ceil() as isize).clamp(0, w as isize - 1) as usize;
                let ylo = (a.y.min(b.y).min(c.y).floor().max(y0 as f32) as usize).max(y0);
                let yhi = ((a.y.max(b.y).max(c.y).ceil() as isize).min((y0 + rows) as isize - 1))
                    .max(0) as usize;
                if ylo > yhi || xmin > xmax {
                    continue;
                }
                for y in ylo..=yhi {
                    let py = y as f32 + 0.5;
                    let row = (y - y0) * w;
                    for x in xmin..=xmax {
                        let pt = Vec2::new(x as f32 + 0.5, py);
                        // Barycentric weights; inside when all have the area's sign.
                        let w0 = (c - b).perp_dot(pt - b) * inv_area;
                        let w1 = (a - c).perp_dot(pt - c) * inv_area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                            continue;
                        }
                        let q0 = w0 * t.q[0];
                        let q1 = w1 * t.q[1];
                        let q2 = w2 * t.q[2];
                        let qs = q0 + q1 + q2;
                        let z = (q0 * t.z[0] + q1 * t.z[1] + q2 * t.z[2]) / qs;
                        let i = row + x;
                        if z < dep[i] && z <= far {
                            dep[i] = z;
                            ids[i] = t.mesh;
                            nor[i] = (t.n[0] * q0 + t.n[1] * q1 + t.n[2] * q2).normalize_or_zero();
                        }
                    }
                }
            }
        });
    GBuffer {
        w,
        h,
        depth,
        normal,
        id,
    }
}

/// Contour-line coverage (0..1) per pixel, plus whether the line belongs to
/// the highlighted object.
fn contour_lines(
    scene: &PreparedScene,
    g: &GBuffer,
    view: &View,
    opts: &RenderOptions,
    lw: f32,
) -> (Vec<f32>, Vec<bool>) {
    let (w, h) = (g.w, g.h);
    let n = w * h;
    if opts.style == RenderStyle::Silhouette || lw <= 0.0 {
        return (vec![0.0; n], vec![false; n]);
    }
    let cos_crease = opts.crease_angle.clamp(1.0, 179.0).to_radians().cos();
    let persp = view.is_perspective();
    let lin = |z: f32| if persp { 1.0 / z } else { z };
    let obj = |i: usize| -> u64 {
        let m = g.id[i];
        if m == 0 {
            0
        } else {
            scene.meshes[m as usize - 1].id.0 + 1
        }
    };
    // 1. Edge pixels (marked on the nearer side of each discontinuity).
    let mut edge = vec![0u8; n];
    edge.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, out) in row.iter_mut().enumerate() {
            let i = y * w + x;
            if g.id[i] == 0 {
                continue;
            }
            let zp = g.depth[i];
            let mut mark = false;
            let nb = [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ];
            for q in nb.iter().flatten() {
                let q = *q;
                let zq = g.depth[q];
                if zp > zq {
                    continue; // the neighbour is nearer: it draws the line
                }
                if obj(q) != obj(i) || g.id[q] == 0 {
                    mark = true;
                    break;
                }
                if g.normal[i].dot(g.normal[q]) < cos_crease {
                    mark = true;
                    break;
                }
            }
            if !mark {
                // Depth discontinuity: second difference of linear depth.
                for (a, b) in [(nb[0], nb[1]), (nb[2], nb[3])] {
                    if let (Some(a), Some(b)) = (a, b)
                        && g.id[a] != 0
                        && g.id[b] != 0
                    {
                        let (la, lp, lb) = (lin(g.depth[a]), lin(zp), lin(g.depth[b]));
                        let d2 = (la + lb - 2.0 * lp).abs();
                        if d2 > 0.04 * lp.abs() {
                            let far_side = if (la - lp).abs() > (lb - lp).abs() {
                                a
                            } else {
                                b
                            };
                            if zp <= g.depth[far_side] {
                                mark = true;
                                break;
                            }
                        }
                    }
                }
            }
            if mark {
                *out = if opts.highlight.is_some_and(|hl| obj(i) == hl.0 + 1) {
                    2
                } else {
                    1
                };
            }
        }
    });
    // 2. Thicken into antialiased lines: splat a disc around each edge
    // pixel (edges are sparse). Strips write only their own rows.
    let r = lw * 0.5;
    let reach = (r + 0.5).ceil() as isize;
    let mut cov = vec![0.0f32; n];
    let mut hl = vec![false; n];
    cov.par_chunks_mut(w * STRIP)
        .zip(hl.par_chunks_mut(w * STRIP))
        .enumerate()
        .for_each(|(s, (crows, hrows))| {
            let y0 = (s * STRIP) as isize;
            let y1 = y0 + (crows.len() / w) as isize;
            for ey in (y0 - reach).max(0)..(y1 + reach).min(h as isize) {
                for ex in 0..w as isize {
                    let e = edge[ey as usize * w + ex as usize];
                    if e == 0 {
                        continue;
                    }
                    for dy in -reach..=reach {
                        let yy = ey + dy;
                        if yy < y0 || yy >= y1 {
                            continue;
                        }
                        for dx in -reach..=reach {
                            let xx = ex + dx;
                            if xx < 0 || xx >= w as isize {
                                continue;
                            }
                            let d = ((dx * dx + dy * dy) as f32).sqrt();
                            let c = (r + 0.5 - d).clamp(0.0, 1.0);
                            let i = (yy - y0) as usize * w + xx as usize;
                            if c > crows[i] || (c > 0.0 && c == crows[i] && e == 2) {
                                crows[i] = c;
                                hrows[i] = e == 2;
                            }
                        }
                    }
                }
            }
        });
    (cov, hl)
}

/// Draws face-preset strokes into the line coverage, depth-tested.
fn draw_faces(
    scene: &PreparedScene,
    g: &GBuffer,
    view: &View,
    opts: &RenderOptions,
    lw: f32,
    mut cov: Vec<f32>,
) -> Vec<f32> {
    if !opts.faces || matches!(opts.style, RenderStyle::Silhouette) {
        return cov;
    }
    for (_, strokes, head_h) in &scene.face_strokes {
        // Size strokes to the head on screen; skip tiny heads.
        let Some(c) = strokes
            .first()
            .and_then(|s| s.first())
            .and_then(|p| view.project(*p))
        else {
            continue;
        };
        let head_px = head_h / view.pixel_size_at(c.depth.max(view.near));
        if head_px < 10.0 {
            continue;
        }
        let width = (lw * 0.8).max(1.0).min(head_px / 25.0).max(0.75);
        let bias = head_h * 0.2;
        for s in strokes {
            for seg in s.windows(2) {
                let (Some(a), Some(b)) = (view.project(seg[0]), view.project(seg[1])) else {
                    continue;
                };
                stroke_segment(&mut cov, g, a, b, width, bias);
            }
        }
    }
    cov
}

fn stroke_segment(
    cov: &mut [f32],
    g: &GBuffer,
    a: crate::camera::ScreenPoint,
    b: crate::camera::ScreenPoint,
    width: f32,
    bias: f32,
) {
    let r = width * 0.5;
    let pa = Vec2::new(a.x, a.y);
    let pb = Vec2::new(b.x, b.y);
    let x0 = (pa.x.min(pb.x) - r - 1.0).floor().max(0.0) as usize;
    let y0 = (pa.y.min(pb.y) - r - 1.0).floor().max(0.0) as usize;
    let x1 = ((pa.x.max(pb.x) + r + 1.0).ceil() as isize).clamp(0, g.w as isize - 1) as usize;
    let y1 = ((pa.y.max(pb.y) + r + 1.0).ceil() as isize).clamp(0, g.h as isize - 1) as usize;
    if x0 > x1 || y0 > y1 {
        return;
    }
    let ab = pb - pa;
    let len2 = ab.length_squared().max(1e-6);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let t = ((p - pa).dot(ab) / len2).clamp(0.0, 1.0);
            let d = (pa + ab * t - p).length();
            let c = (r + 0.5 - d).clamp(0.0, 1.0);
            if c <= 0.0 {
                continue;
            }
            let z = a.depth + (b.depth - a.depth) * t;
            let i = y * g.w + x;
            if z <= g.depth[i] + bias && g.id[i] != 0 {
                cov[i] = cov[i].max(c);
            }
        }
    }
}

fn compose(
    scene: &PreparedScene,
    g: &GBuffer,
    view: &View,
    opts: &RenderOptions,
    lines: &[f32],
    line_hl: &[bool],
    lw: f32,
) -> Vec<u8> {
    let env = &scene.environment;
    let show_ground = opts.show_ground.unwrap_or(env.show_ground);
    let show_grid = opts.show_grid.unwrap_or(env.show_grid);
    let show_horizon = opts.show_horizon.unwrap_or(env.show_horizon) && view.is_perspective();
    let ambient = env.ambient.clamp(0.0, 1.0);
    let sky = env.sky_color.to_linear();
    let horizon = env.horizon_color.to_linear();
    let ground = env.ground_color.to_linear();
    let line = opts.line_color.to_linear();
    let hl_color = opts.highlight_color.to_linear();
    let paper = Vec3::ONE;
    let spacing = env.grid_spacing.max(1e-3);
    let bands = opts.toon_bands.clamp(2, 3);
    let (w, h) = (g.w, g.h);
    let mut out = vec![0u8; w * h * 4];
    let lights = &scene.lights;
    let horizon_grad = {
        let (_, a) = raw_ray(view, 0.5, 0.5);
        let (_, b) = raw_ray(view, 1.5, 0.5);
        let (_, c) = raw_ray(view, 0.5, 1.5);
        Vec2::new(b.y - a.y, c.y - a.y).length().max(1e-9)
    };
    // Unnormalized ray directions are linear in pixel coordinates, which
    // keeps the per-pixel background maths cheap and the horizon exact.
    let ground_hit = |x: f32, y: f32| -> Option<(Vec3, f32)> {
        let (o, d) = raw_ray(view, x, y);
        if d.y.abs() < 1e-9 {
            return None;
        }
        let t = -o.y / d.y;
        if t <= 0.0 {
            return None;
        }
        let p = o + d * t;
        let depth = (p - view.origin).dot(view.forward);
        (depth > 0.0 && depth <= view.far).then_some((p, depth))
    };
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let i = y * w + x;
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut obj_px = g.id[i] != 0;
            let mut ground_info = None;
            if show_ground || show_grid {
                ground_info = ground_hit(fx, fy);
                if let Some((_, gd)) = ground_info
                    && obj_px
                    && gd < g.depth[i]
                {
                    obj_px = false; // geometry below the ground
                }
            }
            let (mut color, mut alpha);
            if obj_px {
                let base = scene.meshes[g.id[i] as usize - 1].color;
                let n = g.normal[i];
                let mut lit = ambient;
                for l in lights {
                    lit += l.intensity * n.dot(l.to_light).max(0.0);
                }
                color = match opts.style {
                    RenderStyle::Toon => {
                        let level = if bands == 2 {
                            if lit < 0.6 { 0.62 } else { 1.0 }
                        } else if lit < 0.45 {
                            0.55
                        } else if lit < 0.85 {
                            0.8
                        } else {
                            1.0
                        };
                        base * level
                    }
                    RenderStyle::Clay => Vec3::splat(0.5) * (0.35 + 0.65 * lit.min(1.25)),
                    RenderStyle::Outline => paper,
                    RenderStyle::Silhouette => Vec3::splat(0.004),
                };
                alpha = 1.0;
            } else {
                let on_ground = show_ground && ground_info.is_some();
                let paper_bg = matches!(opts.style, RenderStyle::Outline | RenderStyle::Silhouette);
                color = if paper_bg {
                    paper
                } else if on_ground {
                    ground
                } else {
                    let (_, d) = raw_ray(view, fx, fy);
                    let t = (d.y / d.length() * 4.0).clamp(0.0, 1.0);
                    horizon.lerp(sky, t * t * (3.0 - 2.0 * t))
                };
                alpha = if opts.transparent_background {
                    0.0
                } else {
                    1.0
                };
                // Grid lines.
                if show_grid && let Some((p, depth)) = ground_info {
                    let px = ground_hit(fx + 1.0, fy).map(|(q, _)| q - p);
                    let py = ground_hit(fx, fy + 1.0).map(|(q, _)| q - p);
                    if let (Some(dx), Some(dy)) = (px, py) {
                        let fp_x = dx.x.abs().max(dy.x.abs()).max(1e-6);
                        let fp_z = dx.z.abs().max(dy.z.abs()).max(1e-6);
                        let dist = |v: f32| {
                            let m = (v / spacing).rem_euclid(1.0);
                            m.min(1.0 - m) * spacing
                        };
                        let gw = (lw * 0.6).max(0.8);
                        let cx = (gw * 0.5 + 0.5 - dist(p.x) / fp_x).clamp(0.0, 1.0);
                        let cz = (gw * 0.5 + 0.5 - dist(p.z) / fp_z).clamp(0.0, 1.0);
                        let fade_fp =
                            (1.0 - ((fp_x.max(fp_z) / spacing) - 0.12) / 0.2).clamp(0.0, 1.0);
                        let fade_far = (1.0 - depth / (view.far * 0.5).min(150.0)).clamp(0.0, 1.0);
                        let c = cx.max(cz) * fade_fp * fade_far * 0.7;
                        if c > 0.0 {
                            let gc = if paper_bg {
                                Vec3::splat(0.6)
                            } else {
                                ground * 0.72
                            };
                            color = (color * alpha * (1.0 - c) + gc * c)
                                / (alpha * (1.0 - c) + c).max(1e-6);
                            alpha = alpha + c * (1.0 - alpha);
                        }
                    }
                }
                // Horizon line (at infinity, so only on background).
                if show_horizon {
                    let (_, d0) = raw_ray(view, fx, fy);
                    let dist = d0.y.abs() / horizon_grad;
                    let c = (lw.max(1.0) * 0.5 + 0.5 - dist).clamp(0.0, 1.0) * 0.8;
                    if c > 0.0 {
                        let hc = Vec3::splat(0.45);
                        color = (color * alpha * (1.0 - c) + hc * c)
                            / (alpha * (1.0 - c) + c).max(1e-6);
                        alpha = alpha + c * (1.0 - alpha);
                    }
                }
            }
            let c = lines[i];
            if c > 0.0 {
                let lc = if line_hl[i] { hl_color } else { line };
                color = (color * alpha * (1.0 - c) + lc * c) / (alpha * (1.0 - c) + c).max(1e-6);
                alpha = alpha + c * (1.0 - alpha);
            }
            let o = &mut row[x * 4..x * 4 + 4];
            o[0] = to_srgb8(color.x);
            o[1] = to_srgb8(color.y);
            o[2] = to_srgb8(color.z);
            o[3] = (alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    });
    out
}

/// The ray through a pixel with an unnormalized direction.
fn raw_ray(view: &View, x: f32, y: f32) -> (Vec3, Vec3) {
    let nx = x / view.width * 2.0 - 1.0;
    let ny = 1.0 - y / view.height * 2.0;
    match view.kind {
        ViewKind::Perspective { tan_half } => (
            view.origin,
            view.forward + view.right * (nx * tan_half.x) + view.up * (ny * tan_half.y),
        ),
        ViewKind::Orthographic { half } => (
            view.origin + view.right * (nx * half.x) + view.up * (ny * half.y),
            view.forward,
        ),
    }
}

const SRGB_LUT_SIZE: usize = 4096;

fn srgb_lut() -> &'static [u8; SRGB_LUT_SIZE + 1] {
    static LUT: std::sync::OnceLock<[u8; SRGB_LUT_SIZE + 1]> = std::sync::OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = [0u8; SRGB_LUT_SIZE + 1];
        for (i, o) in t.iter_mut().enumerate() {
            let v = i as f32 / SRGB_LUT_SIZE as f32;
            let s = if v <= 0.003_130_8 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            };
            *o = (s * 255.0 + 0.5) as u8;
        }
        t
    })
}

fn to_srgb8(v: f32) -> u8 {
    let v = if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    };
    srgb_lut()[(v * SRGB_LUT_SIZE as f32 + 0.5) as usize]
}

fn downsample(img: &[u8], w: usize, _h: usize, ss: usize, ow: u32, oh: u32) -> RgbaImage {
    let (ow_, oh_) = (ow as usize, oh as usize);
    let mut out = vec![0u8; ow_ * oh_ * 4];
    let n = (ss * ss) as f32;
    out.par_chunks_mut(ow_ * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..ow_ {
                let mut acc = [0.0f32; 4];
                for sy in 0..ss {
                    for sx in 0..ss {
                        let i = ((y * ss + sy) * w + x * ss + sx) * 4;
                        let a = img[i + 3] as f32 / 255.0;
                        for c in 0..3 {
                            acc[c] += img[i + c] as f32 * a;
                        }
                        acc[3] += a;
                    }
                }
                let a = acc[3] / n;
                let o = &mut row[x * 4..x * 4 + 4];
                for c in 0..3 {
                    o[c] = if acc[3] > 0.0 {
                        (acc[c] / acc[3] + 0.5).min(255.0) as u8
                    } else {
                        0
                    };
                }
                o[3] = (a * 255.0 + 0.5) as u8;
            }
        });
    RgbaImage {
        width: ow,
        height: oh,
        pixels: out,
    }
}
