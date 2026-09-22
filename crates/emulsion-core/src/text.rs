//! Text layers: a string with a style, shaped and rasterized with cosmic-text
//! into a document-sized cache whenever it changes.

use emulsion_raster::{IRect, Raster, color};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use unicode_segmentation::UnicodeSegmentation;

pub const MAX_CHARS: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AntiAliasMode {
    #[default]
    Smooth,
    Crisp,
    Strong,
    None,
}

/// Character formatting stored on a UTF-8 byte range.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    pub font: String,
    pub size: f32,
    pub color: [u8; 4],
    pub bold: bool,
    pub italic: bool,
    pub letter_spacing: f32,
    /// Positive values raise characters from the baseline, in pixels.
    pub baseline: f32,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font: String::new(),
            size: 48.0,
            color: [0, 0, 0, 255],
            bold: false,
            italic: false,
            letter_spacing: 0.0,
            baseline: 0.0,
        }
    }
}

impl TextStyle {
    fn sanitized(mut self) -> Self {
        self.size = finite_clamp(self.size, 1.0, 4000.0, 48.0);
        self.letter_spacing = finite_clamp(self.letter_spacing, -50.0, 500.0, 0.0);
        self.baseline = finite_clamp(self.baseline, -4000.0, 4000.0, 0.0);
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextRun {
    pub start: usize,
    pub end: usize,
    pub style: TextStyle,
}

fn finite_clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

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
    /// Upright glyphs flow downward; new lines start a new column.
    pub vertical: bool,
    /// Editable local-axis scaling; negative values mirror the text.
    pub scale_x: f32,
    pub scale_y: f32,
    pub align: Align,
    /// Top-left of the text box in document pixels.
    pub x: f32,
    pub y: f32,
    /// Clockwise degrees about the text box's x/y anchor. Text stays editable.
    pub rotation: f32,
    /// Paragraph frame width in local pixels. None means unbounded width.
    pub width: Option<f32>,
    /// Extra spacing between glyphs, in pixels.
    pub letter_spacing: f32,
    /// Independently formatted character ranges, indexed by UTF-8 byte offset.
    pub runs: Vec<TextRun>,
    /// Optional paragraph frame height. Ink outside it is clipped.
    pub height: Option<f32>,
    pub anti_alias: AntiAliasMode,
    pub warp: crate::text_effects::TextWarp,
    pub text_path: Option<crate::text_effects::TextPath>,
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
            vertical: false,
            scale_x: 1.0,
            scale_y: 1.0,
            align: Align::Left,
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            width: None,
            letter_spacing: 0.0,
            runs: Vec::new(),
            height: None,
            anti_alias: AntiAliasMode::Smooth,
            warp: crate::text_effects::TextWarp::default(),
            text_path: None,
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
        for scale in [&mut self.scale_x, &mut self.scale_y] {
            *scale = if scale.is_finite() && *scale != 0.0 {
                scale.signum() * scale.abs().clamp(0.001, 1000.0)
            } else {
                1.0
            };
        }
        self.width = self
            .width
            .filter(|w| w.is_finite() && *w >= 1.0)
            .map(|w| w.min(30000.0));
        self.height = self
            .height
            .filter(|h| h.is_finite() && *h >= 1.0)
            .map(|h| h.min(30000.0));
        self.warp = self.warp.sanitized();
        self.text_path = self.text_path.map(crate::text_effects::TextPath::sanitized);
        self.normalize_runs();
        self
    }

    pub fn base_style(&self) -> TextStyle {
        TextStyle {
            font: self.font.clone(),
            size: self.size,
            color: self.color,
            bold: self.bold,
            italic: self.italic,
            letter_spacing: self.letter_spacing,
            baseline: 0.0,
        }
        .sanitized()
    }

    pub fn style_at(&self, byte: usize) -> TextStyle {
        self.runs
            .iter()
            .find(|run| run.start <= byte && byte < run.end)
            .map(|run| run.style.clone())
            .unwrap_or_else(|| self.base_style())
    }

    /// Apply character formatting to a selection. Partial UTF-8/grapheme
    /// offsets expand to include the whole grapheme, so stored ranges are safe.
    pub fn apply_style(
        &mut self,
        range: std::ops::Range<usize>,
        mut edit: impl FnMut(&mut TextStyle),
    ) {
        let Some(range) = grapheme_range(&self.text, range) else {
            return;
        };
        let base = self.base_style();
        let mut cuts = vec![range.start, range.end];
        for run in &self.runs {
            if run.end > range.start && run.start < range.end {
                cuts.push(run.start.max(range.start));
                cuts.push(run.end.min(range.end));
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        let mut additions = Vec::new();
        for pair in cuts.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            if start == end {
                continue;
            }
            let mut style = self.style_at(start);
            edit(&mut style);
            let style = style.sanitized();
            if style != base {
                additions.push(TextRun { start, end, style });
            }
        }
        let mut kept = Vec::new();
        for run in &self.runs {
            if run.start < range.start {
                kept.push(TextRun {
                    start: run.start,
                    end: run.end.min(range.start),
                    style: run.style.clone(),
                });
            }
            if run.end > range.end {
                kept.push(TextRun {
                    start: run.start.max(range.end),
                    end: run.end,
                    style: run.style.clone(),
                });
            }
        }
        kept.extend(additions);
        self.runs = kept;
        self.normalize_runs();
    }

    /// Replace selected text while retaining character formatting around it.
    /// Inserted text inherits the style at the insertion point.
    pub fn replace_range(&mut self, range: std::ops::Range<usize>, replacement: &str) {
        let range = grapheme_range_allow_empty(&self.text, range);
        let inherited = self.style_at(range.start.saturating_sub(1));
        let removed = range.end - range.start;
        self.text.replace_range(range.clone(), replacement);
        let delta = replacement.len() as isize - removed as isize;
        let shift = |value: usize| value.saturating_add_signed(delta);
        let mut next = Vec::new();
        for run in &self.runs {
            if run.start < range.start {
                next.push(TextRun {
                    start: run.start,
                    end: run.end.min(range.start),
                    style: run.style.clone(),
                });
            }
            if run.end > range.end {
                next.push(TextRun {
                    start: shift(run.start.max(range.end)),
                    end: shift(run.end),
                    style: run.style.clone(),
                });
            }
        }
        if !replacement.is_empty() && inherited != self.base_style() {
            next.push(TextRun {
                start: range.start,
                end: range.start + replacement.len(),
                style: inherited,
            });
        }
        self.runs = next;
        self.normalize_runs();
    }

    pub fn normalize_runs(&mut self) {
        let base = self.base_style();
        let text = self.text.clone();
        let mut runs = std::mem::take(&mut self.runs);
        runs.sort_by_key(|run| (run.start, run.end));
        let mut normalized: Vec<TextRun> = Vec::new();
        for run in runs {
            let Some(range) = grapheme_range(&text, run.start..run.end) else {
                continue;
            };
            let start = normalized
                .last()
                .map_or(range.start, |last| range.start.max(last.end));
            let style = run.style.sanitized();
            if start >= range.end || style == base {
                continue;
            }
            if let Some(last) = normalized.last_mut()
                && last.end == start
                && last.style == style
            {
                last.end = range.end;
            } else {
                normalized.push(TextRun {
                    start,
                    end: range.end,
                    style,
                });
            }
        }
        self.runs = normalized;
    }

    /// Convert local layout coordinates into document coordinates.
    pub fn transform(&self) -> glam::DAffine2 {
        glam::DAffine2::from_translation(glam::dvec2(self.x.round() as f64, self.y.round() as f64))
            * glam::DAffine2::from_angle((self.rotation as f64).to_radians())
            * glam::DAffine2::from_scale(glam::dvec2(self.scale_x as f64, self.scale_y as f64))
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

fn grapheme_range(text: &str, range: std::ops::Range<usize>) -> Option<std::ops::Range<usize>> {
    let range = grapheme_range_allow_empty(text, range);
    (range.start < range.end).then_some(range)
}

fn grapheme_range_allow_empty(text: &str, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let start_target = range.start.min(text.len());
    let end_target = range.end.min(text.len()).max(start_target);
    let mut starts: Vec<usize> = text.grapheme_indices(true).map(|(i, _)| i).collect();
    starts.push(text.len());
    let start = starts
        .iter()
        .copied()
        .take_while(|i| *i <= start_target)
        .last()
        .unwrap_or(0);
    if start_target == end_target {
        return start..start;
    }
    let end = starts
        .iter()
        .copied()
        .find(|i| *i >= end_target)
        .unwrap_or(text.len());
    start..end.max(start)
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

/// Reload installed system fonts and clear cached glyph images.
pub fn refresh_fonts() {
    let mut f = fonts().lock().unwrap_or_else(|e| e.into_inner());
    *f = Fonts {
        system: cosmic_text::FontSystem::new(),
        swash: cosmic_text::SwashCache::new(),
    };
}

fn attrs_for<'a>(style: &'a TextStyle, tag: usize, line_height: f32) -> cosmic_text::Attrs<'a> {
    use cosmic_text::{Attrs, Color, Family, Metrics, Style, Weight};
    let mut attrs = Attrs::new()
        .family(if style.font.trim().is_empty() {
            Family::SansSerif
        } else {
            Family::Name(style.font.trim())
        })
        .metadata(tag)
        .metrics(Metrics::new(style.size, style.size * line_height))
        .color(Color::rgba(
            ((tag >> 16) & 255) as u8,
            ((tag >> 8) & 255) as u8,
            (tag & 255) as u8,
            255,
        ));
    if style.bold {
        attrs = attrs.weight(Weight::BOLD);
    }
    if style.italic {
        attrs = attrs.style(Style::Italic);
    }
    if style.letter_spacing != 0.0 {
        attrs = attrs.letter_spacing(style.letter_spacing);
    }
    attrs
}

fn style_spans(spec: &TextSpec) -> Vec<(std::ops::Range<usize>, TextStyle)> {
    let mut spans = Vec::new();
    let mut cursor = 0;
    for run in &spec.runs {
        if cursor < run.start {
            spans.push((cursor..run.start, spec.base_style()));
        }
        spans.push((run.start..run.end, run.style.clone()));
        cursor = run.end;
    }
    if cursor < spec.text.len() || spans.is_empty() {
        spans.push((cursor..spec.text.len(), spec.base_style()));
    }
    spans
}

/// Identical shaping is used for raster ink and editing geometry.
fn shaped_buffer(
    spec: &TextSpec,
    system: &mut cosmic_text::FontSystem,
) -> (cosmic_text::Buffer, Vec<TextStyle>) {
    use cosmic_text::{Buffer, Metrics, Shaping, Wrap};
    let metrics = Metrics::new(spec.size, spec.size * spec.line_height);
    let mut result = Buffer::new(system, metrics);
    let spans = style_spans(spec);
    let styles: Vec<TextStyle> = spans.iter().map(|(_, style)| style.clone()).collect();
    let path_frame = spec.text_path.as_ref().and_then(|path| {
        (path.mode == crate::text_effects::TextPathMode::Inside)
            .then(|| path.inner_bounds())
            .flatten()
    });
    let width = path_frame.map(|frame| frame.width).or(spec.width);
    let height = path_frame.map(|frame| frame.height).or(spec.height);
    {
        let mut buffer = result.borrow_with(system);
        buffer.set_size(width, height);
        buffer.set_wrap(if width.is_some() {
            Wrap::WordOrGlyph
        } else {
            Wrap::None
        });
        let base = spec.base_style();
        let rich = spans.iter().enumerate().map(|(index, (range, style))| {
            (
                &spec.text[range.clone()],
                attrs_for(style, index + 1, spec.line_height),
            )
        });
        buffer.set_rich_text(
            rich,
            &attrs_for(&base, 0, spec.line_height),
            Shaping::Advanced,
            Some(align_of(spec.align)),
        );
        buffer.shape_until_scroll(true);
    }
    (result, styles)
}

/// Share glyph sampling between measurement and rasterization.
fn draw_glyphs(spec: &TextSpec, mut draw: impl FnMut(i32, i32, u32, u32, [u8; 4], f32)) {
    if spec.text.trim().is_empty() {
        return;
    }
    let mut f = fonts().lock().unwrap_or_else(|e| e.into_inner());
    let Fonts { system, swash } = &mut *f;
    if spec.vertical {
        let mut glyph_spec = spec.clone();
        glyph_spec.width = None;
        glyph_spec.height = None;
        glyph_spec.align = Align::Left;
        glyph_spec.line_height = 1.0;
        glyph_spec.runs.clear();
        glyph_spec.warp = crate::text_effects::TextWarp::default();
        glyph_spec.text_path = None;
        for cell in vertical_cells(spec) {
            glyph_spec.text = spec.text[cell.range.clone()].to_string();
            if glyph_spec.text.trim().is_empty() {
                continue;
            }
            let style = spec.style_at(cell.range.start);
            glyph_spec.font = style.font.clone();
            glyph_spec.size = style.size;
            glyph_spec.color = style.color;
            glyph_spec.bold = style.bold;
            glyph_spec.italic = style.italic;
            glyph_spec.letter_spacing = style.letter_spacing;
            let (mut buffer, _) = shaped_buffer(&glyph_spec, system);
            let advance = buffer
                .layout_runs()
                .map(|r| r.line_w)
                .fold(0.0_f32, f32::max);
            let ox = (cell.rect.x + (cell.rect.width - advance) / 2.0).round() as i32;
            let oy = (cell.rect.y - style.baseline).round() as i32;
            buffer.draw(
                system,
                swash,
                cosmic_text::Color::rgb(0, 0, 0),
                |x, y, w, h, c| draw(x + ox, y + oy, w, h, style.color, c.a() as f32 / 255.0),
            );
        }
    } else {
        let (mut buffer, styles) = shaped_buffer(spec, system);
        let fallback = spec.base_style();
        buffer.draw(
            system,
            swash,
            cosmic_text::Color::rgb(0, 0, 0),
            |x, y, w, h, c| {
                let tag = ((c.r() as usize) << 16) | ((c.g() as usize) << 8) | c.b() as usize;
                let style = tag.checked_sub(1).and_then(|i| styles.get(i));
                let style = style.unwrap_or_else(|| styles.first().unwrap_or(&fallback));
                draw(
                    x,
                    y - style.baseline.round() as i32,
                    w,
                    h,
                    style.color,
                    c.a() as f32 / 255.0,
                );
            },
        );
    }
}

/// A rectangle in unrotated, unscaled text-local coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug)]
struct TextCell {
    range: std::ops::Range<usize>,
    rect: TextRect,
    rtl: bool,
}

/// Editing geometry shares the renderer's font shaping and vertical cells.
/// Indices are UTF-8 byte offsets. Use `TextSpec::transform` for document space.
#[derive(Clone, Debug)]
pub struct TextLayout {
    cells: Vec<TextCell>,
    vertical: bool,
    fallback: TextRect,
}

impl TextLayout {
    pub fn bounds(&self) -> TextRect {
        let Some(first) = self.cells.first() else {
            return self.fallback;
        };
        let mut r = first.rect;
        for cell in &self.cells[1..] {
            let right = (r.x + r.width).max(cell.rect.x + cell.rect.width);
            let bottom = (r.y + r.height).max(cell.rect.y + cell.rect.height);
            r.x = r.x.min(cell.rect.x);
            r.y = r.y.min(cell.rect.y);
            r.width = right - r.x;
            r.height = bottom - r.y;
        }
        r
    }

    pub fn caret(&self, byte: usize) -> TextRect {
        let cell = self
            .cells
            .iter()
            .find(|c| c.range.start == byte)
            .or_else(|| self.cells.iter().find(|c| c.range.contains(&byte)))
            .or_else(|| self.cells.iter().find(|c| c.range.end == byte))
            .or_else(|| self.cells.iter().max_by_key(|c| c.range.end));
        let Some(cell) = cell else {
            return self.caret_in(self.fallback, false, false);
        };
        self.caret_in(
            cell.rect,
            byte >= cell.range.end && !cell.range.is_empty(),
            cell.rtl,
        )
    }

    fn caret_in(&self, r: TextRect, end: bool, rtl: bool) -> TextRect {
        if self.vertical {
            TextRect {
                y: r.y + if end { r.height } else { 0.0 },
                height: 1.0,
                ..r
            }
        } else {
            TextRect {
                x: r.x + if end != rtl { r.width } else { 0.0 },
                width: 1.0,
                ..r
            }
        }
    }

    pub fn hit(&self, x: f32, y: f32) -> usize {
        let cell = self.cells.iter().min_by(|a, b| {
            let distance = |r: TextRect| {
                let dx = (r.x - x).max(0.0).max(x - r.x - r.width);
                let dy = (r.y - y).max(0.0).max(y - r.y - r.height);
                dx * dx + dy * dy
            };
            distance(a.rect).total_cmp(&distance(b.rect))
        });
        let Some(c) = cell else {
            return 0;
        };
        let end = if self.vertical {
            y >= c.rect.y + c.rect.height / 2.0
        } else {
            (x >= c.rect.x + c.rect.width / 2.0) != c.rtl
        };
        if end { c.range.end } else { c.range.start }
    }

    pub fn selection(&self, range: std::ops::Range<usize>) -> Vec<TextRect> {
        if range.is_empty() {
            return Vec::new();
        }
        self.cells
            .iter()
            .filter(|c| c.range.start < range.end && c.range.end > range.start)
            .map(|c| c.rect)
            .collect()
    }
}

fn vertical_cells(spec: &TextSpec) -> Vec<TextCell> {
    let base_advance = (spec.size + spec.letter_spacing).max(1.0);
    let base_column_width = spec.size * spec.line_height;
    // Legacy vertical text stored its inline (column-height) limit in width.
    // New paragraph frames set height, which makes width the physical cross axis.
    let inline_limit = spec.height.or(spec.width);
    let limit = inline_limit.unwrap_or(f32::MAX).max(base_advance);
    let mut cells = Vec::new();
    let (mut x, mut y) = (0.0, 0.0);
    let mut column_width = base_column_width;
    for (start, grapheme) in spec.text.grapheme_indices(true) {
        let style = spec.style_at(start);
        let advance = (style.size + style.letter_spacing).max(1.0);
        let cell_width = style.size * spec.line_height;
        if y > 0.0 && y + advance > limit && grapheme != "\n" && grapheme != "\r\n" {
            x += column_width;
            y = 0.0;
            column_width = base_column_width;
        }
        column_width = column_width.max(cell_width);
        cells.push(TextCell {
            range: start..start + grapheme.len(),
            rect: TextRect {
                x,
                y,
                width: cell_width,
                height: advance,
            },
            rtl: false,
        });
        if grapheme == "\n" || grapheme == "\r\n" {
            x += column_width;
            y = 0.0;
            column_width = base_column_width;
        } else {
            y += advance;
        }
    }
    // Empty final lines need a caret and a hit target even though they have no ink.
    if cells.is_empty() || spec.text.ends_with('\n') {
        cells.push(TextCell {
            range: spec.text.len()..spec.text.len(),
            rect: TextRect {
                x,
                y,
                width: base_column_width,
                height: base_advance,
            },
            rtl: false,
        });
    }
    if let Some(limit) = inline_limit {
        let mut start = 0;
        while start < cells.len() {
            let end = (start + 1..cells.len())
                .find(|i| cells[*i].rect.x != cells[start].rect.x)
                .unwrap_or(cells.len());
            let length = cells[end - 1].rect.y + cells[end - 1].rect.height;
            let offset = match spec.align {
                Align::Center => (limit - length).max(0.0) / 2.0,
                Align::Right => (limit - length).max(0.0),
                _ => 0.0,
            };
            for cell in &mut cells[start..end] {
                cell.rect.y += offset;
            }
            start = end;
        }
    }
    cells
}

fn raw_layout(spec: &TextSpec) -> TextLayout {
    let fallback = TextRect {
        x: 0.0,
        y: 0.0,
        width: spec.size.max(1.0),
        height: spec.size * spec.line_height,
    };
    if spec.vertical {
        let mut cells = vertical_cells(spec);
        let (width, height) = frame_limits(spec);
        clip_layout_frame(&mut cells, width, height);
        return TextLayout {
            cells,
            vertical: true,
            fallback,
        };
    }
    let mut f = fonts().lock().unwrap_or_else(|e| e.into_inner());
    let (buffer, styles) = shaped_buffer(spec, &mut f.system);
    let mut offsets = vec![0];
    for (i, c) in spec.text.char_indices() {
        if c == '\n' {
            offsets.push(i + 1);
        }
    }
    let mut cells = Vec::new();
    for run in buffer.layout_runs() {
        let offset = offsets.get(run.line_i).copied().unwrap_or(spec.text.len());
        if run.glyphs.is_empty() {
            cells.push(TextCell {
                range: offset..offset,
                rect: TextRect {
                    x: 0.0,
                    y: run.line_top,
                    width: 1.0,
                    height: run.line_height,
                },
                rtl: false,
            });
        }
        for glyph in run.glyphs {
            let style = glyph
                .metadata
                .checked_sub(1)
                .and_then(|index| styles.get(index))
                .unwrap_or_else(|| styles.first().expect("text always has a style span"));
            let cluster = &run.text[glyph.start..glyph.end];
            let count = cluster.graphemes(true).count().max(1);
            let advance = glyph.w / count as f32;
            for (i, (start, grapheme)) in cluster.grapheme_indices(true).enumerate() {
                let visual = if glyph.level.is_rtl() {
                    count - 1 - i
                } else {
                    i
                };
                cells.push(TextCell {
                    range: offset + glyph.start + start
                        ..offset + glyph.start + start + grapheme.len(),
                    rect: TextRect {
                        x: glyph.x + visual as f32 * advance,
                        y: run.line_top - style.baseline,
                        width: advance,
                        height: run.line_height,
                    },
                    rtl: glyph.level.is_rtl(),
                });
            }
        }
        let end = offset + run.text.len();
        if run.glyphs.iter().map(|glyph| glyph.end).max().unwrap_or(0) == run.text.len()
            && spec
                .text
                .get(end..)
                .is_some_and(|s| s.starts_with('\n') || s.starts_with("\r\n"))
        {
            let len = if spec.text[end..].starts_with("\r\n") {
                2
            } else {
                1
            };
            cells.push(TextCell {
                range: end..end + len,
                rect: TextRect {
                    x: if run.rtl {
                        run.glyphs
                            .iter()
                            .map(|g| g.x)
                            .reduce(f32::min)
                            .unwrap_or(0.0)
                    } else {
                        run.glyphs
                            .iter()
                            .map(|g| g.x + g.w)
                            .reduce(f32::max)
                            .unwrap_or(0.0)
                    },
                    y: run.line_top,
                    width: (spec.size / 2.0).max(1.0),
                    height: run.line_height,
                },
                rtl: false,
            });
        }
    }
    let (width, height) = frame_limits(spec);
    clip_layout_frame(&mut cells, width, height);
    TextLayout {
        cells,
        vertical: false,
        fallback,
    }
}

/// Measure editable caret and selection geometry, including spaces and empty
/// lines. Warp and path effects alter the returned local geometry so editing,
/// hit testing and rendering continue to agree.
pub fn layout(spec: &TextSpec) -> TextLayout {
    let mut result = raw_layout(spec);
    if spec.warp.is_identity() && spec.text_path.is_none() {
        return result;
    }
    let source = result.bounds();
    let bounds =
        crate::text_effects::EffectRect::new(source.x, source.y, source.width, source.height);
    let map = |rect: TextRect| map_text_rect(rect, bounds, spec);
    for cell in &mut result.cells {
        cell.rect = map(cell.rect);
    }
    result.fallback = map(result.fallback);
    result
}

fn map_text_rect(
    rect: TextRect,
    source: crate::text_effects::EffectRect,
    spec: &TextSpec,
) -> TextRect {
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    // Midpoints keep curved path and warp selection geometry conservative.
    for yi in 0..=2 {
        for xi in 0..=2 {
            let p = (
                rect.x as f64 + rect.width as f64 * xi as f64 / 2.0,
                rect.y as f64 + rect.height as f64 * yi as f64 / 2.0,
            );
            let p = crate::text_effects::map_point(
                p,
                source,
                spec.size,
                spec.warp,
                spec.text_path.as_ref(),
            );
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
    }
    TextRect {
        x: x0 as f32,
        y: y0 as f32,
        width: (x1 - x0).max(1.0) as f32,
        height: (y1 - y0).max(1.0) as f32,
    }
}

fn clip_layout_frame(cells: &mut Vec<TextCell>, width: Option<f32>, height: Option<f32>) {
    cells.retain_mut(|cell| {
        let left = cell.rect.x.max(0.0);
        let right = width.map_or(cell.rect.x + cell.rect.width, |limit| {
            (cell.rect.x + cell.rect.width).min(limit)
        });
        let top = cell.rect.y.max(0.0);
        let bottom = height.map_or(cell.rect.y + cell.rect.height, |limit| {
            (cell.rect.y + cell.rect.height).min(limit)
        });
        cell.rect.x = left;
        cell.rect.width = (right - left).max(0.0);
        cell.rect.y = top;
        cell.rect.height = (bottom - top).max(0.0);
        cell.rect.width > 0.0 && cell.rect.height > 0.0
    });
}

fn frame_limits(spec: &TextSpec) -> (Option<f32>, Option<f32>) {
    if spec.vertical && spec.height.is_none() {
        (None, spec.width)
    } else {
        (spec.width, spec.height)
    }
}

/// Bounds of the entire shaped text object in document coordinates, including
/// off-canvas glyphs. This measures glyph samples without a temporary canvas.
pub fn bounds(spec: &TextSpec) -> IRect {
    let mut local = IRect::default();
    draw_glyphs(spec, |x, y, width, height, color, coverage| {
        if color[3] > 0 && coverage > 0.0 {
            local = local.union(&IRect::new(x, y, width as i32, height as i32));
        }
    });
    if local.is_empty() {
        return local;
    }
    if spec.text_path.is_none() {
        let (width, height) = frame_limits(spec);
        local = local.intersect(&IRect::new(
            0,
            0,
            width.map_or(i32::MAX, |v| v.ceil() as i32),
            height.map_or(i32::MAX, |v| v.ceil() as i32),
        ));
        if local.is_empty() {
            return local;
        }
    }
    let local = if spec.warp.is_identity() && spec.text_path.is_none() {
        crate::text_effects::EffectRect::new(
            local.x as f32,
            local.y as f32,
            local.w as f32,
            local.h as f32,
        )
    } else {
        crate::text_effects::mapped_bounds(
            crate::text_effects::EffectRect::new(
                local.x as f32,
                local.y as f32,
                local.w as f32,
                local.h as f32,
            ),
            spec.size,
            spec.warp,
            spec.text_path.as_ref(),
        )
    };
    let transform = spec.transform();
    let points = [
        glam::dvec2(local.x as f64, local.y as f64),
        glam::dvec2((local.x + local.width) as f64, local.y as f64),
        glam::dvec2(local.x as f64, (local.y + local.height) as f64),
        glam::dvec2(
            (local.x + local.width) as f64,
            (local.y + local.height) as f64,
        ),
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
    let rotated = spec.rotation != 0.0 || spec.scale_x != 1.0 || spec.scale_y != 1.0;
    let origin = glam::dvec2(ox as f64, oy as f64);
    let rotation = spec.transform() * glam::DAffine2::from_translation(-origin);
    let transformed_margin = 2.0 * (spec.scale_x.abs() + spec.scale_y.abs()) as f64 + 2.0;
    let effected = !spec.warp.is_identity() || spec.text_path.is_some();
    let source = raw_layout(spec).bounds();
    let effect_mapper = crate::text_effects::TextEffectMapper::new(
        crate::text_effects::EffectRect::new(source.x, source.y, source.width, source.height),
        spec.size,
        spec.warp,
        spec.text_path.as_ref(),
    );
    let (frame_width, frame_height) = frame_limits(spec);
    // Accumulate coverage per pixel so overlapping glyph edges don't double.
    let mut pixels: std::collections::HashMap<(i32, i32), [f32; 4]> =
        std::collections::HashMap::new();
    draw_glyphs(spec, |x, y, pw, ph, rgba, coverage| {
        let coverage = match spec.anti_alias {
            AntiAliasMode::Smooth => coverage,
            AntiAliasMode::Crisp => ((coverage - 0.2) / 0.6).clamp(0.0, 1.0),
            AntiAliasMode::Strong => coverage.sqrt(),
            AntiAliasMode::None => {
                if coverage >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
        };
        let a = coverage * rgba[3] as f32 / 255.0;
        if a <= 0.0 {
            return;
        }
        let base = color::srgba8_to_premul([rgba[0], rgba[1], rgba[2], 255]);
        for dy in 0..ph as i32 {
            for dx in 0..pw as i32 {
                let local_x = x + dx;
                let local_y = y + dy;
                if spec.text_path.is_none()
                    && (frame_width.is_some_and(|limit| local_x < 0 || local_x as f32 >= limit)
                        || frame_height.is_some_and(|limit| local_y < 0 || local_y as f32 >= limit))
                {
                    continue;
                }
                let targets = if effected {
                    let point =
                        effect_mapper.map((x as f64 + dx as f64 + 0.5, y as f64 + dy as f64 + 0.5));
                    if !effect_mapper.includes(point) {
                        continue;
                    }
                    let sx = ox as f64 + point.0 - 0.5;
                    let sy = oy as f64 + point.1 - 0.5;
                    let (ix, iy) = (sx.floor() as i32, sy.floor() as i32);
                    let (fx, fy) = ((sx - ix as f64) as f32, (sy - iy as f64) as f32);
                    [
                        (ix, iy, (1.0 - fx) * (1.0 - fy)),
                        (ix + 1, iy, fx * (1.0 - fy)),
                        (ix, iy + 1, (1.0 - fx) * fy),
                        (ix + 1, iy + 1, fx * fy),
                    ]
                } else {
                    [
                        (ox + x + dx, oy + y + dy, 1.0),
                        (0, 0, 0.0),
                        (0, 0, 0.0),
                        (0, 0, 0.0),
                    ]
                };
                for (px, py, weight) in targets {
                    if weight <= 0.0 {
                        continue;
                    }
                    if rotated {
                        // Keep glyph samples that can contribute AFTER rotation.
                        // Clipping the unrotated cache first loses letters outside
                        // the canvas that should rotate back into view.
                        let p = rotation
                            .transform_point2(glam::dvec2(px as f64 + 0.5, py as f64 + 0.5));
                        if p.x < -transformed_margin
                            || p.y < -transformed_margin
                            || p.x > w as f64 + transformed_margin
                            || p.y > h as f64 + transformed_margin
                        {
                            continue;
                        }
                    } else if px < 0 || py < 0 || px >= wi || py >= hi {
                        continue;
                    }
                    let e = pixels.entry((px, py)).or_insert([0.0; 4]);
                    // Source-over of the same colour: a + b(1-a).
                    let sample_alpha = a * weight;
                    let keep = 1.0 - sample_alpha;
                    for i in 0..4 {
                        e[i] = base[i] * sample_alpha + e[i] * keep;
                    }
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
    fn character_styles_split_merge_and_roundtrip() {
        let mut spec = TextSpec {
            text: "one two".into(),
            ..Default::default()
        };
        spec.apply_style(0..3, |style| {
            style.bold = true;
            style.color = [255, 0, 0, 255];
        });
        spec.apply_style(4..7, |style| {
            style.italic = true;
            style.size = 30.0;
        });
        assert_eq!(spec.runs.len(), 2);
        assert!(spec.style_at(1).bold);
        assert!(spec.style_at(5).italic);
        assert_eq!(spec.style_at(3), spec.base_style());

        let copy: TextSpec = serde_json::from_str(&serde_json::to_string(&spec).unwrap()).unwrap();
        assert_eq!(copy, spec);

        spec.apply_style(0..3, |style| *style = TextSpec::default().base_style());
        assert_eq!(
            spec.runs.len(),
            1,
            "base formatting is not stored redundantly"
        );
    }

    #[test]
    fn character_ranges_are_grapheme_safe_and_follow_edits() {
        let mut spec = TextSpec {
            text: "Aé👩‍🚀Z".into(),
            ..Default::default()
        };
        let emoji = spec.text.find('👩').unwrap();
        let after_emoji = emoji + "👩‍🚀".len();
        spec.apply_style(emoji + 1..emoji + 2, |style| style.bold = true);
        assert_eq!(spec.runs[0].start..spec.runs[0].end, emoji..after_emoji);

        spec.replace_range(1.."Aé".len(), "hello");
        let shifted = 1 + "hello".len();
        assert!(spec.style_at(shifted).bold);
        assert!(spec.runs.iter().all(
            |run| spec.text.is_char_boundary(run.start) && spec.text.is_char_boundary(run.end)
        ));
    }

    #[test]
    fn mixed_character_styles_render_independent_colors_and_metrics() {
        let mut spec = TextSpec {
            text: "AB".into(),
            size: 24.0,
            color: [255, 0, 0, 255],
            ..Default::default()
        };
        spec.apply_style(1..2, |style| {
            style.color = [0, 0, 255, 255];
            style.size = 54.0;
            style.baseline = 5.0;
        });
        let raster = rasterize(&spec, 200, 100);
        let mut red = false;
        let mut blue = false;
        for y in 0..raster.height() {
            for x in 0..raster.width() {
                let pixel = raster.get(x, y);
                red |= pixel[0] > pixel[2] && pixel[3] > 0;
                blue |= pixel[2] > pixel[0] && pixel[3] > 0;
            }
        }
        assert!(red && blue);
        assert!(layout(&spec).bounds().height > 40.0);
    }

    #[test]
    fn legacy_text_defaults_to_unbounded_smooth_single_style() {
        let legacy: TextSpec = serde_json::from_str(r#"{"text":"Legacy"}"#).unwrap();
        assert!(legacy.runs.is_empty());
        assert_eq!(legacy.height, None);
        assert_eq!(legacy.anti_alias, AntiAliasMode::Smooth);
        assert_eq!(legacy.style_at(2), legacy.base_style());
    }

    #[test]
    fn paragraph_height_clips_ink_and_editing_geometry() {
        let full = TextSpec {
            text: "first line second line third line".into(),
            size: 20.0,
            width: Some(90.0),
            ..Default::default()
        };
        let clipped = TextSpec {
            height: Some(24.0),
            ..full.clone()
        };
        assert!(ink(&rasterize(&clipped, 200, 200)).h <= 24);
        assert!(layout(&clipped).bounds().height <= 24.0);
        assert!(ink(&rasterize(&full, 200, 200)).h > 24);
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
    fn vertical_glyphs_remain_upright_and_geometry_tracks_columns() {
        let spec = TextSpec {
            text: "MM".into(),
            vertical: true,
            size: 24.0,
            x: 10.0,
            y: 10.0,
            ..Default::default()
        };
        let raster = rasterize(&spec, 180, 180);
        assert_eq!(bounds(&spec), ink(&raster));
        let first = rasterize(
            &TextSpec {
                text: "M".into(),
                ..spec.clone()
            },
            180,
            180,
        );
        let mut checked = 0;
        for y in 0..180 {
            for x in 0..180 {
                if first.get(x, y)[3] != 0 {
                    assert_eq!(raster.get(x, y), first.get(x, y));
                    assert_eq!(
                        raster.get(x, y + 24),
                        first.get(x, y),
                        "next glyph stays upright"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 20);
        let wrapped = TextSpec {
            text: "ABCD\nE".into(),
            width: Some(48.0),
            ..spec
        };
        let geometry = layout(&wrapped);
        assert_eq!(geometry.caret(0).y, geometry.caret(2).y);
        assert!(geometry.caret(2).x > geometry.caret(0).x);
        assert!(geometry.caret(5).x > geometry.caret(2).x);
        assert_eq!(geometry.selection(0..2).len(), 2);
        for byte in [0, 1, 2, 3, 5] {
            let caret = geometry.caret(byte);
            assert_eq!(
                geometry.hit(caret.x + caret.width / 2.0, caret.y + 0.1),
                byte
            );
        }
    }

    #[test]
    fn editing_layout_keeps_unicode_spaces_and_empty_lines_addressable() {
        for vertical in [false, true] {
            let spec = TextSpec {
                text: "é e\u{301}\n\nZ\n".into(),
                size: 20.0,
                vertical,
                ..Default::default()
            };
            let geometry = layout(&spec);
            assert!(!geometry.selection(0..spec.text.len()).is_empty());
            let first = geometry.caret(0);
            let after_accent = geometry.caret("é".len());
            if vertical {
                assert!(after_accent.y > first.y);
            } else {
                assert!(after_accent.x > first.x);
            }
            let last = geometry.caret(spec.text.len());
            assert_eq!(geometry.hit(last.x + 0.1, last.y + 0.1), spec.text.len());
            for cell in &geometry.cells {
                assert!(spec.text.is_char_boundary(cell.range.start));
                assert!(spec.text.is_char_boundary(cell.range.end));
            }
            // Combining accent remains a single editable grapheme.
            assert!(
                geometry
                    .cells
                    .iter()
                    .any(|c| &spec.text[c.range.clone()] == "e\u{301}")
            );
        }
    }

    #[test]
    fn editable_scale_and_vertical_mode_roundtrip_with_legacy_defaults() {
        let old: TextSpec = serde_json::from_str(r#"{"text":"Old"}"#).unwrap();
        assert!(!old.vertical);
        assert_eq!((old.scale_x, old.scale_y), (1.0, 1.0));
        let spec = TextSpec {
            text: "Scale".into(),
            size: 20.0,
            x: 150.0,
            y: 30.0,
            scale_x: -1.5,
            scale_y: 2.0,
            vertical: true,
            rotation: 90.0,
            ..Default::default()
        };
        let copy: TextSpec = serde_json::from_str(&serde_json::to_string(&spec).unwrap()).unwrap();
        assert_eq!(copy, spec);
        let raster = rasterize(&spec, 250, 250);
        // Move the mirrored/rotated text inside the canvas for a full ink comparison.
        let b = bounds(&spec);
        assert!(!b.is_empty());
        assert!(!ink(&raster).is_empty());
        let local = glam::dvec2(12.0, 22.0);
        assert!(
            (spec
                .transform()
                .inverse()
                .transform_point2(spec.transform().transform_point2(local))
                - local)
                .length()
                < 1e-8
        );
        let safe = TextSpec {
            scale_x: f32::NAN,
            scale_y: 0.0,
            ..spec
        }
        .sanitized();
        assert_eq!((safe.scale_x, safe.scale_y), (1.0, 1.0));
    }

    #[test]
    fn rtl_end_caret_uses_logical_end_after_visual_reordering() {
        let spec = TextSpec {
            text: "שלום".into(),
            size: 24.0,
            ..Default::default()
        };
        let geometry = layout(&spec);
        let start = geometry.caret(0);
        let end = geometry.caret(spec.text.len());
        assert!(start.x > end.x);
        assert_eq!(
            geometry.hit(end.x + 0.1, end.y + end.height / 2.0),
            spec.text.len()
        );
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

    fn test_path(closed: bool, points: &[(f64, f64)]) -> emulsion_raster::vector::Path {
        emulsion_raster::vector::Path {
            subpaths: vec![emulsion_raster::vector::SubPath {
                anchors: points
                    .iter()
                    .copied()
                    .map(emulsion_raster::vector::Anchor::corner)
                    .collect(),
                closed,
            }],
        }
    }

    #[test]
    fn editable_warp_changes_raster_and_editing_geometry() {
        let plain = TextSpec {
            text: "Warp me".into(),
            size: 30.0,
            x: 20.0,
            y: 40.0,
            ..Default::default()
        };
        let warped = TextSpec {
            warp: crate::text_effects::TextWarp {
                style: crate::text_effects::WarpStyle::Arc,
                bend: 70.0,
                horizontal: 0.0,
                vertical: 0.0,
            },
            ..plain.clone()
        };
        let plain_ink = ink(&rasterize(&plain, 300, 160));
        let warped_ink = ink(&rasterize(&warped, 300, 160));
        assert!(!warped_ink.is_empty());
        assert_ne!(plain_ink, warped_ink);
        assert_ne!(layout(&plain).bounds(), layout(&warped).bounds());

        let encoded = serde_json::to_string(&warped).unwrap();
        assert_eq!(serde_json::from_str::<TextSpec>(&encoded).unwrap(), warped);
        let legacy: TextSpec = serde_json::from_str(r#"{"text":"legacy"}"#).unwrap();
        assert!(legacy.warp.is_identity());
        assert!(legacy.text_path.is_none());
    }

    #[test]
    fn followed_and_inside_path_text_stay_editable_and_visible() {
        let follow = TextSpec {
            text: "Along path".into(),
            size: 22.0,
            text_path: Some(crate::text_effects::TextPath {
                path: test_path(false, &[(20.0, 70.0), (220.0, 70.0)]),
                offset: 15.0,
                ..Default::default()
            }),
            ..Default::default()
        };
        let follow_ink = ink(&rasterize(&follow, 280, 140));
        assert!(!follow_ink.is_empty());
        assert!(follow_ink.x >= 30 && follow_ink.y < 90, "{follow_ink:?}");
        assert!(layout(&follow).bounds().y > 30.0);

        let inside = TextSpec {
            text: "Text wraps inside this closed editable path frame".into(),
            size: 18.0,
            text_path: Some(crate::text_effects::TextPath {
                path: test_path(
                    true,
                    &[(30.0, 20.0), (190.0, 20.0), (190.0, 115.0), (30.0, 115.0)],
                ),
                mode: crate::text_effects::TextPathMode::Inside,
                inset: 5.0,
                ..Default::default()
            }),
            ..Default::default()
        };
        let inside_ink = ink(&rasterize(&inside, 240, 150));
        assert!(!inside_ink.is_empty());
        assert!(
            inside_ink.x >= 34 && inside_ink.right() <= 196,
            "{inside_ink:?}"
        );
        assert!(
            inside_ink.y >= 24 && inside_ink.bottom() <= 121,
            "{inside_ink:?}"
        );
    }
}
