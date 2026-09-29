//! Shared owned paragraph runs keep CPU ink, Vello glyphs, caret geometry and exports identical.
use super::*;
use cosmic_text::{Buffer, Color, FontSystem, LayoutGlyph, LayoutRun, SwashCache};
use std::sync::Arc;
pub struct ShapedBuffer {
    native: Option<Buffer>,
    runs: Vec<OwnedRun>,
}
struct OwnedRun {
    line_i: usize,
    text: Arc<str>,
    rtl: bool,
    glyphs: Vec<LayoutGlyph>,
    line_y: f32,
    line_top: f32,
    line_height: f32,
    line_w: f32,
}
impl ShapedBuffer {
    pub(super) fn native(buffer: Buffer) -> Self {
        Self {
            native: Some(buffer),
            runs: Vec::new(),
        }
    }
    pub fn layout_runs(&self) -> Box<dyn Iterator<Item = LayoutRun<'_>> + '_> {
        if let Some(buffer) = &self.native {
            Box::new(buffer.layout_runs())
        } else {
            Box::new(self.runs.iter().map(|r| LayoutRun {
                line_i: r.line_i,
                text: &r.text,
                rtl: r.rtl,
                glyphs: &r.glyphs,
                decorations: &[],
                line_y: r.line_y,
                line_top: r.line_top,
                line_height: r.line_height,
                line_w: r.line_w,
            }))
        }
    }
    pub fn draw(
        &mut self,
        system: &mut FontSystem,
        cache: &mut SwashCache,
        color: Color,
        callback: impl FnMut(i32, i32, u32, u32, Color),
    ) {
        if let Some(buffer) = &mut self.native {
            buffer.draw(system, cache, color, callback);
            return;
        }
        use cosmic_text::Renderer;
        let mut renderer = cosmic_text::LegacyRenderer {
            font_system: system,
            cache,
            callback,
        };
        for run in self.layout_runs() {
            for glyph in run.glyphs {
                renderer.glyph(
                    glyph.physical((0., run.line_y), 1.),
                    glyph.color_opt.unwrap_or(color),
                );
            }
        }
    }
}
fn slice(spec: &TextSpec, start: usize, end: usize) -> TextSpec {
    let mut out = TextSpec {
        text: spec.text[start..end].into(),
        font: spec.font.clone(),
        size: spec.size,
        line_height: spec.line_height,
        color: spec.color,
        bold: spec.bold,
        italic: spec.italic,
        underline: spec.underline,
        strikethrough: spec.strikethrough,
        letter_spacing: spec.letter_spacing,
        align: spec.align,
        width: spec.width,
        ..Default::default()
    };
    let first = spec.runs.partition_point(|r| r.end <= start);
    out.runs = spec.runs[first..]
        .iter()
        .take_while(|r| r.start < end)
        .filter_map(|run| {
            let a = run.start.max(start);
            let b = run.end.min(end);
            (a < b).then(|| TextRun {
                start: a - start,
                end: b - start,
                style: run.style.clone(),
            })
        })
        .collect();
    out
}
fn reindex(glyphs: &mut [LayoutGlyph], offset: usize, styles: &[TextStyle]) {
    for glyph in glyphs {
        let old = glyph
            .metadata
            .saturating_sub(1)
            .min(styles.len().saturating_sub(1));
        glyph.metadata = offset + old + 1;
        let tag = glyph.metadata;
        glyph.color_opt = Some(Color::rgba(
            ((tag >> 16) & 255) as u8,
            ((tag >> 8) & 255) as u8,
            (tag & 255) as u8,
            255,
        ));
    }
}
pub(super) fn shape(spec: &TextSpec, system: &mut FontSystem) -> (ShapedBuffer, Vec<TextStyle>) {
    let mut output = ShapedBuffer {
        native: None,
        runs: Vec::new(),
    };
    let mut styles = Vec::new();
    let mut offset = 0;
    let mut top = 0.;
    let path_frame = spec.text_path.as_ref().and_then(|p| {
        (p.mode == crate::text_effects::TextPathMode::Inside)
            .then(|| p.inner_bounds())
            .flatten()
    });
    let width = path_frame.map(|p| p.width).or(spec.width);
    let height = path_frame.map(|p| p.height).or(spec.height);
    for (line_i, source_line) in spec.text.split('\n').enumerate() {
        let text = source_line.trim_end_matches('\r');
        let text: Arc<str> = Arc::from(text);
        let format = spec
            .paragraphs
            .binary_search_by_key(&offset, |p| p.start)
            .ok()
            .map(|i| spec.paragraphs[i].format)
            .unwrap_or_default();
        top += format.space_before;
        if height.is_some_and(|height| top >= height) {
            break;
        }
        let prefix = if format.list != ParagraphList::None {
            paragraphs::prefix_len(&text)
        } else {
            0
        };
        let mut marker_glyphs = Vec::new();
        let mut marker_baseline = 0.;
        let mut marker_height: f32 = 0.;
        let mut marker_width: f32 = 0.;
        let mut indent = format.indent + f32::from(format.level) * spec.size * 1.5;
        if prefix > 0 {
            let mut marker = slice(spec, offset, offset + prefix);
            marker.width = None;
            marker.align = Align::Left;
            let (buffer, local_styles) = shaped_buffer_raw(&marker, system);
            let style_offset = styles.len();
            for run in buffer.layout_runs() {
                marker_baseline = run.line_y;
                marker_height = run.line_height;
                marker_width = marker_width.max(run.line_w);
                marker_glyphs.extend_from_slice(run.glyphs);
            }
            reindex(&mut marker_glyphs, style_offset, &local_styles);
            styles.extend(local_styles);
            // Long numbers must not collide with text even when the requested hanging gap is too small.
            indent = indent.max(marker_width + spec.size * 0.25);
            let marker_x = (indent - format.hanging.max(marker_width + spec.size * 0.25)).max(0.);
            for glyph in &mut marker_glyphs {
                glyph.x += marker_x;
            }
        }
        let mut body = slice(spec, offset + prefix, offset + text.len());
        body.width = width.map(|w| (w - indent).max(1.));
        body.align = format.align.unwrap_or(spec.align);
        let (buffer, local_styles) = shaped_buffer_raw(&body, system);
        let style_offset = styles.len();
        let mut paragraph_height: f32 = 0.;
        let mut first = true;
        let mut extra_height = 0.;
        for run in buffer.layout_runs() {
            let mut glyphs = run.glyphs.to_vec();
            reindex(&mut glyphs, style_offset, &local_styles);
            for glyph in &mut glyphs {
                glyph.start += prefix;
                glyph.end += prefix;
                glyph.x += indent;
            }
            // A larger rich-text marker shares the body's baseline and expands
            // the first line's ascent/descent; wrapped body lines begin after it.
            let ascent = run.line_y - run.line_top;
            let baseline_shift = if first {
                (marker_baseline - ascent).max(0.)
            } else {
                0.
            };
            let line_height = if first {
                let descent = (run.line_height - ascent).max(marker_height - marker_baseline);
                let height = ascent + baseline_shift + descent;
                extra_height = (height - run.line_height).max(0.);
                height
            } else {
                run.line_height
            };
            let shift = if first { 0. } else { extra_height };
            let line_y = run.line_y + top + shift + baseline_shift;
            let line_top = run.line_top + top + shift;
            if first {
                glyphs.splice(0..0, marker_glyphs.drain(..));
            }
            let line_w = glyphs.iter().map(|g| g.x + g.w).fold(indent, f32::max);
            output.runs.push(OwnedRun {
                line_i,
                text: text.clone(),
                rtl: run.rtl,
                glyphs,
                line_y,
                line_top,
                line_height,
                line_w,
            });
            paragraph_height = paragraph_height.max(run.line_top + shift + line_height);
            first = false;
        }
        if first {
            let line_height = (spec.size * spec.line_height).max(marker_height);
            output.runs.push(OwnedRun {
                line_i,
                text: text.clone(),
                rtl: false,
                glyphs: marker_glyphs,
                line_y: top + marker_baseline,
                line_top: top,
                line_height,
                line_w: indent,
            });
            paragraph_height = line_height;
        }
        styles.extend(local_styles);
        top += paragraph_height + format.space_after;
        offset += source_line.len() + 1;
    }
    if styles.is_empty() {
        styles.push(spec.base_style());
    }
    (output, styles)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rich_list_marker_reserves_height_before_wrapped_body_lines() {
        let mut spec = TextSpec {
            text: "• First words wrap onto more lines with a large marker".into(),
            size: 16.,
            width: Some(180.),
            paragraphs: vec![ParagraphStyle {
                start: 0,
                format: ParagraphFormat {
                    list: ParagraphList::Bullet,
                    indent: 45.,
                    hanging: 35.,
                    ..Default::default()
                },
            }],
            ..Default::default()
        };
        let mut marker = spec.base_style();
        marker.size = 64.;
        spec.runs = vec![TextRun {
            start: 0,
            end: "• ".len(),
            style: marker,
        }];
        let (buffer, _) = shaped_buffer(&spec, &mut font_system());
        let runs: Vec<_> = buffer.layout_runs().collect();
        assert!(runs.len() > 1);
        assert!(runs[0].line_height >= 64.);
        for pair in runs.windows(2) {
            assert!(
                pair[1].line_top + 0.01 >= pair[0].line_top + pair[0].line_height,
                "wrapped line overlaps its larger list marker"
            );
        }
        // All first-line glyphs share a baseline; no artificial glyph offset hides overlap.
        assert!((runs[0].glyphs[0].y - runs[0].glyphs.last().unwrap().y).abs() < 0.01);
    }
}
