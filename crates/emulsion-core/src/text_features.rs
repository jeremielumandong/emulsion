//! Lists remain editable source text; decorations share native shaping geometry.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStyle {
    None,
    Bullet,
    Numbered,
}
/// Add or replace one list prefix per nonempty line, retaining character formatting.
/// Blank lines remain blank and restart numbered lists. Existing bullet/number
/// prefixes are recognized only at line start, after indentation.
pub fn apply_list(spec: &TextSpec, style: ListStyle) -> Result<TextSpec, String> {
    let mut edits = Vec::new();
    let mut offset = 0;
    let mut number = 1;
    for source_line in spec.text.split_inclusive('\n') {
        let line = source_line.trim_end_matches('\n').trim_end_matches('\r');
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        let content = &line[indent..];
        if content.is_empty() {
            number = 1;
            offset += source_line.len();
            continue;
        }
        let prefix = if content.starts_with("• ") {
            "• ".len()
        } else {
            let digits = content.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && content[digits..].starts_with(". ") {
                digits + 2
            } else {
                0
            }
        };
        let replacement = match style {
            ListStyle::None => String::new(),
            ListStyle::Bullet => "• ".into(),
            ListStyle::Numbered => format!("{number}. "),
        };
        edits.push((offset + indent..offset + indent + prefix, replacement));
        number += 1;
        offset += source_line.len();
    }
    let new_len = spec.text.chars().count() as isize
        + edits
            .iter()
            .map(|(r, s)| {
                s.chars().count() as isize - spec.text[r.clone()].chars().count() as isize
            })
            .sum::<isize>();
    if new_len > MAX_CHARS as isize {
        return Err("The formatted list exceeds the text length limit.".into());
    }
    let mut next = spec.clone();
    for (range, value) in edits.into_iter().rev() {
        next.replace_range(range, &value);
    }
    Ok(next)
}
/// Text-local decoration rectangles and their rich-text colors. Renderers apply
/// the same frame clipping, transforms and warps as glyphs.
pub fn decoration_rects(spec: &TextSpec) -> Vec<([f64; 4], [u8; 4])> {
    if !spec.underline
        && !spec.strikethrough
        && !spec
            .runs
            .iter()
            .any(|r| r.style.underline || r.style.strikethrough)
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    if spec.vertical {
        for cell in vertical_cells(spec) {
            let style = spec.style_at(cell.range.start);
            let r = cell.rect;
            let thickness = (style.size / 16.).max(1.);
            if style.underline {
                out.push((
                    [
                        f64::from(r.x + r.width - thickness),
                        f64::from(r.y),
                        f64::from(thickness),
                        f64::from(r.height),
                    ],
                    style.color,
                ));
            }
            if style.strikethrough {
                out.push((
                    [
                        f64::from(r.x + r.width * 0.5),
                        f64::from(r.y),
                        f64::from(thickness),
                        f64::from(r.height),
                    ],
                    style.color,
                ));
            }
        }
        return out;
    }
    let mut fonts = fonts().lock().unwrap_or_else(|e| e.into_inner());
    let (buffer, styles) = shaped_buffer(spec, &mut fonts.system);
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let Some(style) = styles.get(glyph.metadata.saturating_sub(1)) else {
                continue;
            };
            let thickness = (style.size / 16.).max(1.);
            for (enabled, offset) in [
                (style.underline, style.size * 0.08),
                (style.strikethrough, -style.size * 0.3),
            ] {
                if enabled && glyph.w > 0. {
                    out.push((
                        [
                            glyph.x as f64,
                            f64::from(run.line_y - style.baseline + offset),
                            glyph.w as f64,
                            thickness as f64,
                        ],
                        style.color,
                    ));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_lists_preserve_unicode_rich_content_and_repeated_conversion() {
        let mut text = TextSpec {
            text: "éclair\n  日本語\n\nThird".into(),
            ..Default::default()
        };
        let start = text.text.find("日本語").unwrap();
        text.apply_style(start..start + "日本語".len(), |s| {
            s.underline = true;
            s.color = [200, 20, 60, 255];
        });
        let bullets = apply_list(&text, ListStyle::Bullet).unwrap();
        assert_eq!(bullets.text, "• éclair\n  • 日本語\n\n• Third");
        assert_eq!(apply_list(&bullets, ListStyle::Bullet).unwrap(), bullets);
        let numbered = apply_list(&bullets, ListStyle::Numbered).unwrap();
        assert_eq!(numbered.text, "1. éclair\n  2. 日本語\n\n1. Third");
        let start = numbered.text.find("日本語").unwrap();
        assert!(numbered.style_at(start).underline);
        assert_eq!(numbered.style_at(start).color, [200, 20, 60, 255]);
        assert_eq!(
            apply_list(&numbered, ListStyle::None).unwrap().text,
            text.text
        );
        let crlf = TextSpec {
            text: "One\r\n\r\nTwo".into(),
            ..Default::default()
        };
        assert_eq!(
            apply_list(&crlf, ListStyle::Numbered).unwrap().text,
            "1. One\r\n\r\n1. Two"
        );
        let full = TextSpec {
            text: "x".repeat(MAX_CHARS),
            ..Default::default()
        };
        assert!(apply_list(&full, ListStyle::Bullet).is_err());
    }
    #[test]
    fn native_decorations_share_rich_colors_and_vector_export() {
        let mut text = TextSpec {
            text: "Decorated text".into(),
            size: 32.,
            ..Default::default()
        };
        let plain = vector_paths(&text).unwrap().len();
        text.apply_style(0..9, |s| {
            s.underline = true;
            s.strikethrough = true;
            s.color = [190, 20, 40, 255];
        });
        let rectangles = decoration_rects(&text);
        assert!(!rectangles.is_empty());
        assert!(
            rectangles
                .iter()
                .all(|(r, c)| r[2] > 0. && r[3] > 0. && *c == [190, 20, 40, 255])
        );
        assert_eq!(vector_paths(&text).unwrap().len(), plain + rectangles.len());
        let mut callbacks = Vec::new();
        draw_glyphs(&text, |x, y, w, h, c, a| callbacks.push((x, y, w, h, c, a)));
        let [x, y, w, h] = rectangles[0].0;
        assert!(callbacks.iter().any(|&(px, py, pw, ph, c, a)| pw == 1
            && ph == 1
            && c == [190, 20, 40, 255]
            && a > 0.
            && f64::from(px) >= x.floor()
            && f64::from(px) < x + w
            && f64::from(py) >= y.floor()
            && f64::from(py) < y + h));
        let json = serde_json::to_string(&text).unwrap();
        assert_eq!(serde_json::from_str::<TextSpec>(&json).unwrap(), text);
        let old: TextSpec = serde_json::from_str(r#"{"text":"Old document"}"#).unwrap();
        assert!(!old.underline && !old.strikethrough);
    }
}
