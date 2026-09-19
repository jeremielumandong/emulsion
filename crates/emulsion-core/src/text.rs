//! Text layers: a string with a style, shaped and rasterized with cosmic-text
//! into a document-sized cache whenever it changes.

use emulsion_raster::{IRect, Raster, color};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

pub const MAX_CHARS: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    pub fn parse(s: &str) -> Option<Align> {
        Some(match s.to_ascii_lowercase().as_str() {
            "left" => Align::Left,
            "center" | "centre" => Align::Center,
            "right" => Align::Right,
            "justify" => Align::Justify,
            _ => return None,
        })
    }
    pub fn key(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        }
    }
}

/// Everything that decides how a text layer looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextSpec {
    pub text: String,
    /// Font family name; falls back to the sans-serif default when absent.
    pub font: String,
    /// Font size in document pixels.
    pub size: f32,
    /// Line height as a multiple of the size.
    pub line_height: f32,
    pub color: [u8; 4],
    pub bold: bool,
    pub italic: bool,
    pub align: Align,
    /// Top-left of the text box in document pixels.
    pub x: f32,
    pub y: f32,
    /// Clockwise degrees about the text box's x/y anchor. Text stays editable.
    pub rotation: f32,
    /// Wrap width in document pixels; None means one line per paragraph.
    pub width: Option<f32>,
    /// Extra spacing between glyphs, in pixels.
    pub letter_spacing: f32,
}

impl Default for TextSpec {
    fn default() -> Self {
        Self {
            text: String::new(),
            font: String::new(),
            size: 48.0,
            line_height: 1.2,
            color: [0, 0, 0, 255],
            bold: false,
            italic: false,
            align: Align::Left,
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            width: None,
            letter_spacing: 0.0,
        }
    }
}

impl TextSpec {
    pub fn sanitized(mut self) -> Self {
        if self.text.chars().count() > MAX_CHARS {
            self.text = self.text.chars().take(MAX_CHARS).collect();
        }
        self.size = if self.size.is_finite() {
            self.size.clamp(1.0, 4000.0)
        } else {
            48.0
        };
        self.line_height = if self.line_height.is_finite() {
            self.line_height.clamp(0.5, 4.0)
        } else {
            1.2
        };
        self.letter_spacing = if self.letter_spacing.is_finite() {
            self.letter_spacing.clamp(-50.0, 500.0)
        } else {
            0.0
        };
        if !self.x.is_finite() {
            self.x = 0.0
        }
        if !self.y.is_finite() {
            self.y = 0.0
        }
        self.rotation = if self.rotation.is_finite() {
            self.rotation % 360.0
        } else {
            0.0
        };
        self.width = self
            .width
            .filter(|w| w.is_finite() && *w >= 1.0)
            .map(|w| w.min(30000.0));
        self
    }

    /// A one-line label for panels.
    pub fn label(&self) -> String {
        let first = self.text.lines().next().unwrap_or("").trim();
        if first.is_empty() {
            "Text".into()
        } else {
            first.chars().take(24).collect()
        }
    }
}

struct Fonts {
    system: cosmic_text::FontSystem,
    swash: cosmic_text::SwashCache,
}

fn fonts() -> &'static Mutex<Fonts> {
    static FONTS: OnceLock<Mutex<Fonts>> = OnceLock::new();
    FONTS.get_or_init(|| {
        Mutex::new(Fonts {
            system: cosmic_text::FontSystem::new(),
            swash: cosmic_text::SwashCache::new(),
        })
    })
}

/// Family names available to text layers, sorted, deduplicated.
pub fn font_families() -> Vec<String> {
    let f = fonts().lock().unwrap_or_else(|e| e.into_inner());
    let mut names: Vec<String> = f
        .system
        .db()
        .faces()
        .flat_map(|face| face.families.iter().map(|(n, _)| n.clone()))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Share shaping and glyph sampling between measurement and rasterization.
/// Callbacks receive local glyph coordinates, before placement or clipping.
fn draw_glyphs(spec: &TextSpec, draw: impl FnMut(i32, i32, u32, u32, cosmic_text::Color)) {
    use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight, Wrap};
    if spec.text.trim().is_empty() {
        return;
    }
    let mut f = fonts().lock().unwrap_or_else(|e| e.into_inner());
    let Fonts { system, swash } = &mut *f;
    let metrics = Metrics::new(spec.size, spec.size * spec.line_height);
    let mut buffer = Buffer::new(system, metrics);
    let mut buffer = buffer.borrow_with(system);
    buffer.set_size(spec.width, None);
    buffer.set_wrap(if spec.width.is_some() {
        Wrap::WordOrGlyph
    } else {
        Wrap::None
    });
    let mut attrs = Attrs::new();
    if !spec.font.trim().is_empty() {
        attrs = attrs.family(Family::Name(spec.font.trim()));
    } else {
        attrs = attrs.family(Family::SansSerif);
    }
    if spec.bold {
        attrs = attrs.weight(Weight::BOLD);
    }
    if spec.italic {
        attrs = attrs.style(Style::Italic);
    }
    if spec.letter_spacing != 0.0 {
        attrs = attrs.letter_spacing(spec.letter_spacing);
    }
    buffer.set_text(
        &spec.text,
        &attrs,
        Shaping::Advanced,
        Some(align_of(spec.align)),
    );
    buffer.shape_until_scroll(true);
    let colour =
        cosmic_text::Color::rgba(spec.color[0], spec.color[1], spec.color[2], spec.color[3]);
    buffer.draw(swash, colour, draw);
}

/// Bounds of the entire shaped text object in document coordinates, including
/// off-canvas glyphs. This measures glyph samples without a temporary canvas.
pub fn bounds(spec: &TextSpec) -> IRect {
    let mut local = IRect::default();
    draw_glyphs(spec, |x, y, width, height, color| {
        if color.a() > 0 {
            local = local.union(&IRect::new(x, y, width as i32, height as i32));
        }
    });
    if local.is_empty() {
        return local;
    }
    let transform =
        glam::DAffine2::from_translation(glam::dvec2(spec.x.round() as f64, spec.y.round() as f64))
            * glam::DAffine2::from_angle((spec.rotation as f64).to_radians());
    let points = [
        glam::dvec2(local.x as f64, local.y as f64),
        glam::dvec2(local.right() as f64, local.y as f64),
        glam::dvec2(local.x as f64, local.bottom() as f64),
        glam::dvec2(local.right() as f64, local.bottom() as f64),
    ]
    .map(|p| transform.transform_point2(p));
    let (mut lo, mut hi) = (points[0], points[0]);
    for p in points {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    let (x, y) = ((lo.x + 1e-9).floor() as i32, (lo.y + 1e-9).floor() as i32);
    IRect::new(
        x,
        y,
        ((hi.x - 1e-9).ceil() as i32 - x).max(0),
        ((hi.y - 1e-9).ceil() as i32 - y).max(0),
    )
}

/// Shape and rasterize `spec` into a `w × h` document-space raster.
pub fn rasterize(spec: &TextSpec, w: u32, h: u32) -> Raster {
    let out = Raster::transparent(w, h);
    if spec.text.trim().is_empty() || w == 0 || h == 0 {
        return out;
    }
    let (ox, oy) = (spec.x.round() as i32, spec.y.round() as i32);
    let (wi, hi) = (w as i32, h as i32);
    let rotated = spec.rotation != 0.0;
    let origin = glam::dvec2(ox as f64, oy as f64);
    let rotation = glam::DAffine2::from_translation(origin)
        * glam::DAffine2::from_angle((spec.rotation as f64).to_radians())
        * glam::DAffine2::from_translation(-origin);
    // Accumulate coverage per pixel so overlapping glyph edges don't double.
    let mut pixels: std::collections::HashMap<(i32, i32), [f32; 4]> =
        std::collections::HashMap::new();
    let base = color::srgba8_to_premul(spec.color);
    draw_glyphs(spec, |x, y, pw, ph, c| {
        let a = c.a() as f32 / 255.0;
        if a <= 0.0 {
            return;
        }
        for dy in 0..ph as i32 {
            for dx in 0..pw as i32 {
                let (px, py) = (ox + x + dx, oy + y + dy);
                if rotated {
                    // Keep glyph samples that can contribute AFTER rotation.
                    // Clipping the unrotated cache first loses letters outside
                    // the canvas that should rotate back into view.
                    let p =
                        rotation.transform_point2(glam::dvec2(px as f64 + 0.5, py as f64 + 0.5));
                    if p.x < -2.0 || p.y < -2.0 || p.x > w as f64 + 2.0 || p.y > h as f64 + 2.0 {
                        continue;
                    }
                } else if px < 0 || py < 0 || px >= wi || py >= hi {
                    continue;
                }
                let e = pixels.entry((px, py)).or_insert([0.0; 4]);
                // Source-over of the same colour: a + b(1-a).
                let keep = 1.0 - a;
                for i in 0..4 {
                    e[i] = base[i] * a + e[i] * keep;
                }
            }
        }
    });
    if pixels.is_empty() {
        return out;
    }
    let (x0, y0) = (
        pixels.keys().map(|k| k.0).min().unwrap_or(0),
        pixels.keys().map(|k| k.1).min().unwrap_or(0),
    );
    let (x1, y1) = (
        pixels.keys().map(|k| k.0).max().unwrap_or(0),
        pixels.keys().map(|k| k.1).max().unwrap_or(0),
    );
    let b = IRect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1);
    if rotated {
        let corners = [
            glam::dvec2(x0 as f64, y0 as f64),
            glam::dvec2((x1 + 1) as f64, y0 as f64),
            glam::dvec2(x0 as f64, (y1 + 1) as f64),
            glam::dvec2((x1 + 1) as f64, (y1 + 1) as f64),
        ]
        .map(|p| rotation.transform_point2(p));
        let (mut lo, mut hi) = (corners[0], corners[0]);
        for p in corners {
            lo = lo.min(p);
            hi = hi.max(p);
        }
        let (left, top) = (
            (lo.x.floor() as i32 - 1).max(0),
            (lo.y.floor() as i32 - 1).max(0),
        );
        let (right, bottom) = (
            (hi.x.ceil() as i32 + 1).min(wi),
            (hi.y.ceil() as i32 + 1).min(h as i32),
        );
        let target = IRect::new(left, top, (right - left).max(0), (bottom - top).max(0));
        if target.is_empty() {
            return out;
        }
        let inverse = rotation.inverse();
        let at = |x: i32, y: i32| pixels.get(&(x, y)).copied().unwrap_or([0.0; 4]);
        let mut px = Vec::with_capacity(target.w as usize * target.h as usize);
        for y in target.y..target.bottom() {
            for x in target.x..target.right() {
                let p = inverse.transform_point2(glam::dvec2(x as f64 + 0.5, y as f64 + 0.5))
                    - glam::dvec2(0.5, 0.5);
                let (ix, iy) = (p.x.floor() as i32, p.y.floor() as i32);
                let (fx, fy) = ((p.x - ix as f64) as f32, (p.y - iy as f64) as f32);
                let (a, b, c, d) = (
                    at(ix, iy),
                    at(ix + 1, iy),
                    at(ix, iy + 1),
                    at(ix + 1, iy + 1),
                );
                px.push(color::f_to_px(std::array::from_fn(|i| {
                    (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy)
                        + (c[i] * (1.0 - fx) + d[i] * fx) * fy
                })));
            }
        }
        return out.write_rect(target, &px);
    }
    let mut px: Vec<[u16; 4]> = vec![[0; 4]; (b.w * b.h) as usize];
    for ((x, y), c) in pixels {
        px[((y - y0) * b.w + (x - x0)) as usize] = color::f_to_px(c);
    }
    out.write_rect(b, &px)
}

fn align_of(a: Align) -> cosmic_text::Align {
    match a {
        Align::Left => cosmic_text::Align::Left,
        Align::Center => cosmic_text::Align::Center,
        Align::Right => cosmic_text::Align::Right,
        Align::Justify => cosmic_text::Align::Justified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact bounds of non-transparent pixels.
    fn ink(r: &Raster) -> IRect {
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, -1, -1);
        for y in 0..r.height() as i32 {
            for x in 0..r.width() as i32 {
                if r.get(x as u32, y as u32)[3] > 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if x1 < 0 {
            IRect::new(0, 0, 0, 0)
        } else {
            IRect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
        }
    }

    #[test]
    fn text_renders_ink_where_expected() {
        let spec = TextSpec {
            text: "Hello".into(),
            size: 40.0,
            color: [255, 0, 0, 255],
            x: 10.0,
            y: 10.0,
            ..Default::default()
        }
        .sanitized();
        let r = rasterize(&spec, 300, 100);
        let b = ink(&r);
        assert!(
            !b.is_empty(),
            "no glyphs drawn (fonts available: {})",
            font_families().len()
        );
        // Ink lands inside the line box and nowhere above it.
        assert!(b.y >= 10 && b.y < 40, "{b:?}");
        assert!(b.x >= 10 && b.right() < 200, "{b:?}");
        let mut red = 0;
        for y in 0..100 {
            for x in 0..300 {
                let p = r.get(x, y);
                if p[3] > 0 {
                    assert!(p[0] >= p[1] && p[0] >= p[2], "red ink");
                    red += 1;
                }
            }
        }
        assert!(red > 100, "{red}");
    }

    #[test]
    fn wrapped_text_stays_in_the_box() {
        let spec = TextSpec {
            text: "one two three four five six seven eight".into(),
            size: 20.0,
            width: Some(120.0),
            ..Default::default()
        };
        let r = rasterize(&spec, 400, 400);
        let b = ink(&r);
        assert!(b.right() <= 130, "wrapped: {b:?}");
        assert!(b.h > 40, "several lines: {b:?}");
        let empty = rasterize(&TextSpec::default(), 50, 50);
        assert!(ink(&empty).is_empty());
    }

    #[test]
    fn rotated_text_keeps_glyphs_that_start_outside_the_canvas() {
        let spec = TextSpec {
            text: "MMMM".into(),
            size: 24.0,
            x: 90.0,
            y: 10.0,
            ..Default::default()
        };
        let unrotated = rasterize(&spec, 300, 150);
        let rotated = rasterize(
            &TextSpec {
                rotation: 90.0,
                ..spec.clone()
            },
            100,
            150,
        );
        let mut recovered = 0;
        for y in 0..150 {
            for x in 0..300 {
                let (rx, ry) = (99 - y as i32, x as i32 - 80);
                if (0..100).contains(&rx) && (0..150).contains(&ry) {
                    let old = unrotated.get(x, y);
                    let new = rotated.get(rx as u32, ry as u32);
                    for i in 0..4 {
                        assert!(old[i].abs_diff(new[i]) <= 1, "glyph mismatch at {x},{y}");
                    }
                    if x >= 100 && old[3] > 0 {
                        recovered += 1;
                    }
                }
            }
        }
        assert!(
            recovered > 100,
            "letters beyond the original canvas must rotate into view"
        );
    }

    #[test]
    fn rotation_serialization_defaults_and_editable_text_survive_rotation() {
        let legacy: TextSpec = serde_json::from_str(r#"{"text":"Hello"}"#).unwrap();
        assert_eq!(legacy.rotation, 0.0);
        let mut d = crate::Document::new(300, 300);
        d.nodes.push(crate::Node::text(
            1,
            "label",
            TextSpec {
                text: "Hello".into(),
                x: 100.0,
                y: 100.0,
                size: 28.0,
                ..Default::default()
            },
            300,
            300,
        ));
        crate::Command::RotateNode {
            id: 1,
            degrees: 35.0,
        }
        .apply(&mut d)
        .unwrap();
        let crate::NodeKind::Text { spec, cache } = &d.nodes[0].kind else {
            panic!()
        };
        assert_eq!(spec.rotation, 35.0);
        let old_cache = cache.clone();
        let roundtrip: TextSpec =
            serde_json::from_str(&serde_json::to_string(spec).unwrap()).unwrap();
        assert_eq!(roundtrip.rotation, 35.0);
        let mut edited = roundtrip;
        edited.text = "Changed".into();
        crate::Command::SetText {
            id: 1,
            spec: Box::new(edited),
        }
        .apply(&mut d)
        .unwrap();
        let crate::NodeKind::Text { spec, cache } = &d.nodes[0].kind else {
            panic!()
        };
        assert_eq!(spec.text, "Changed");
        assert_eq!(spec.rotation, 35.0);
        assert!(!std::sync::Arc::ptr_eq(cache, &old_cache));
        assert!(!ink(cache).is_empty());
    }

    #[test]
    fn text_rotation_pivot_uses_all_glyphs_independent_of_canvas_clipping() {
        let spec = TextSpec {
            text: "MMMM".into(),
            size: 24.0,
            x: 90.0,
            y: 10.0,
            ..Default::default()
        };
        let expected = bounds(&spec);
        assert_eq!(expected, ink(&rasterize(&spec, 300, 300)));
        assert!(expected.right() > 100);
        let mut results = Vec::new();
        for size in [100, 300] {
            let mut doc = crate::Document::new(size, size);
            doc.nodes
                .push(crate::Node::text(1, "label", spec.clone(), size, size));
            assert_eq!(crate::geometry::node_bounds(&doc, 1), Some(expected));
            crate::Command::RotateNode {
                id: 1,
                degrees: 90.0,
            }
            .apply(&mut doc)
            .unwrap();
            let crate::NodeKind::Text { spec, .. } = &doc.nodes[0].kind else {
                panic!()
            };
            results.push((**spec).clone());
        }
        assert_eq!(
            results[0], results[1],
            "canvas clipping must not change the pivot"
        );

        let mut doc = crate::Document::new(100, 100);
        doc.nodes.push(crate::Node::text(
            1,
            "outside",
            TextSpec {
                x: 500.0,
                y: 500.0,
                ..spec
            },
            100,
            100,
        ));
        let crate::NodeKind::Text { cache, .. } = &doc.nodes[0].kind else {
            panic!()
        };
        assert!(ink(cache).is_empty());
        let before = crate::geometry::node_bounds(&doc, 1).unwrap();
        assert!(before.x >= 500 && before.y >= 500);
        crate::Command::RotateNode {
            id: 1,
            degrees: 90.0,
        }
        .apply(&mut doc)
        .unwrap();
        let after = crate::geometry::node_bounds(&doc, 1).unwrap();
        assert_eq!((after.w, after.h), (before.h, before.w));
        assert!(
            ((after.x as f64 + after.w as f64 / 2.0) - (before.x as f64 + before.w as f64 / 2.0))
                .abs()
                <= 0.5
        );
        assert!(
            ((after.y as f64 + after.h as f64 / 2.0) - (before.y as f64 + before.h as f64 / 2.0))
                .abs()
                <= 0.5
        );
    }
}
