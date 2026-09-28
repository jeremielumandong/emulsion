//! Per-paragraph list and spacing metadata, with editable source markers.
use super::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParagraphList {
    #[default]
    None,
    Bullet,
    Numbered,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParagraphFormat {
    pub list: ParagraphList,
    /// Nested list level, 0–8. Each level adds 1.5 em to the left indent.
    pub level: u8,
    /// Left indent and hanging marker offset in local document pixels.
    pub indent: f32,
    pub hanging: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub align: Option<Align>,
    /// Explicit restart on this paragraph; None continues the preceding sequence.
    pub restart: Option<u32>,
}
impl ParagraphFormat {
    pub fn validate(self) -> Result<(), String> {
        if self.level > 8
            || [
                self.indent,
                self.hanging,
                self.space_before,
                self.space_after,
            ]
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=10000.).contains(v))
            || self.restart.is_some_and(|n| n == 0 || n > 1_000_000)
        {
            return Err("Paragraph levels must be 0–8, spacing 0–10000 pixels, and numbering starts 1–1000000.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParagraphStyle {
    pub start: usize,
    #[serde(flatten)]
    pub format: ParagraphFormat,
}
fn starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}
pub(super) fn prefix_len(line: &str) -> usize {
    for marker in ["• ", "◦ ", "▪ "] {
        if line.starts_with(marker) {
            return marker.len();
        }
    }
    let length = line
        .bytes()
        .take_while(|b| b.is_ascii_digit() || *b == b'.')
        .count();
    if length > 1
        && line.as_bytes().get(length) == Some(&b' ')
        && line.as_bytes()[length - 1] == b'.'
        && line.as_bytes()[0].is_ascii_digit()
    {
        length + 1
    } else {
        0
    }
}
/// Authored paragraph contents, excluding only metadata-owned list markers.
/// Literal numbered headings and original line separators remain intact.
pub fn paragraph_content(spec: &TextSpec) -> String {
    let mut result = String::with_capacity(spec.text.len());
    let mut start = 0;
    for line in spec.text.split_inclusive('\n') {
        let prefix = if spec
            .paragraphs
            .iter()
            .any(|p| p.start == start && p.format.list != ParagraphList::None)
        {
            prefix_len(line)
        } else {
            0
        };
        result.push_str(&line[prefix..]);
        start += line.len();
    }
    result
}
/// Apply formatting to paragraphs intersecting the byte range (a caret targets its paragraph).
/// Renumber linked nested sequences and preserve rich character runs through prefix replacements.
pub fn apply_paragraphs(
    spec: &TextSpec,
    range: std::ops::Range<usize>,
    format: ParagraphFormat,
) -> Result<TextSpec, String> {
    format.validate()?;
    if range.start > range.end
        || range.end > spec.text.len()
        || !spec.text.is_char_boundary(range.start)
        || !spec.text.is_char_boundary(range.end)
    {
        return Err("Paragraph range must contain valid UTF-8 byte boundaries.".into());
    }
    if spec.vertical {
        return Err("Paragraph lists and hanging indents require horizontal text.".into());
    }
    let offsets = starts(&spec.text);
    if offsets.len() > 1000 {
        return Err("Paragraph formatting supports up to 1000 paragraphs per text layer.".into());
    }
    let mut formats = offsets
        .iter()
        .map(|start| {
            spec.paragraphs
                .iter()
                .find(|p| p.start == *start)
                .map(|p| p.format)
        })
        .collect::<Vec<_>>();
    let previous_formats = formats.clone();
    let mut changed = vec![false; offsets.len()];
    let mut first_selected = true;
    for (i, start) in offsets.iter().copied().enumerate() {
        let end = offsets.get(i + 1).copied().unwrap_or(spec.text.len());
        if (range.is_empty()
            && start <= range.start
            && (range.start < end || i + 1 == offsets.len()))
            || (!range.is_empty() && start < range.end && end > range.start)
        {
            let mut current = format;
            if !first_selected {
                current.restart = None;
            }
            first_selected = false;
            formats[i] = Some(current);
            changed[i] = true;
        }
    }
    let mut counters = [0u32; 9];
    let mut edits = Vec::new();
    for (i, start) in offsets.iter().copied().enumerate() {
        let end = offsets.get(i + 1).copied().unwrap_or(spec.text.len());
        let line = spec.text[start..end].trim_end_matches(['\n', '\r']);
        // Only prefixes belonging to actual list metadata are structural. A
        // typed title such as "2024. Revenue" must survive paragraph formatting.
        let old_prefix = if previous_formats[i].is_some_and(|f| f.list != ParagraphList::None) {
            prefix_len(line)
        } else {
            0
        };
        let Some(style) = formats[i] else {
            counters = [0; 9];
            continue;
        };
        if style.list == ParagraphList::None {
            counters = [0; 9];
            if changed[i] && old_prefix > 0 {
                edits.push((start..start + old_prefix, String::new()));
            }
            continue;
        }
        if line.trim().is_empty() && !changed[i] {
            counters = [0; 9];
            continue;
        }
        let level = usize::from(style.level);
        for counter in &mut counters[level + 1..] {
            *counter = 0;
        }
        if let Some(restart) = style.restart {
            counters[level] = restart;
        } else {
            counters[level] = counters[level].saturating_add(1).max(1);
        }
        for counter in &mut counters[..level] {
            *counter = (*counter).max(1);
        }
        let prefix = match style.list {
            ParagraphList::Bullet => ["• ", "◦ ", "▪ "][level % 3].to_string(),
            ParagraphList::Numbered => format!(
                "{}. ",
                counters[..=level]
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(".")
            ),
            ParagraphList::None => String::new(),
        };
        edits.push((start..start + old_prefix, prefix));
    }
    let length = spec.text.chars().count() as isize
        + edits
            .iter()
            .map(|(r, s)| {
                s.chars().count() as isize - spec.text[r.clone()].chars().count() as isize
            })
            .sum::<isize>();
    if length > MAX_CHARS as isize {
        return Err("The formatted list exceeds the text length limit.".into());
    }
    let mut next = spec.clone();
    for (range, value) in edits.into_iter().rev() {
        next.replace_range(range, &value);
    }
    let new_starts = starts(&next.text);
    next.paragraphs = formats
        .into_iter()
        .enumerate()
        .filter_map(|(i, format)| {
            format.map(|format| ParagraphStyle {
                start: new_starts[i],
                format,
            })
        })
        .collect();
    Ok(next)
}
pub(super) fn normalize(spec: &mut TextSpec) {
    let starts = starts(&spec.text);
    spec.paragraphs
        .retain(|p| p.format.validate().is_ok() && starts.binary_search(&p.start).is_ok());
    spec.paragraphs.sort_by_key(|p| p.start);
    spec.paragraphs.dedup_by_key(|p| p.start);
}
/// Re-anchor surviving paragraphs and inherit formatting for new paragraphs created by typing.
pub(super) fn replacement(
    spec: &TextSpec,
    range: std::ops::Range<usize>,
    replacement: &str,
) -> Vec<ParagraphStyle> {
    let original = starts(&spec.text);
    let active_start = original
        .iter()
        .copied()
        .take_while(|s| *s <= range.start)
        .last()
        .unwrap_or(0);
    let inherited = spec
        .paragraphs
        .iter()
        .find(|p| p.start == active_start)
        .map(|p| p.format);
    let delta = replacement.len() as isize - (range.end - range.start) as isize;
    let mut result = Vec::new();
    for p in &spec.paragraphs {
        if p.start <= range.start {
            result.push(p.clone());
        } else if p.start >= range.end {
            let mut p = p.clone();
            p.start = p.start.saturating_add_signed(delta);
            result.push(p);
        }
    }
    if let Some(mut format) = inherited {
        format.restart = None;
        for (i, _) in replacement.match_indices('\n') {
            result.push(ParagraphStyle {
                start: range.start + i + 1,
                format,
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_numbering_preserves_rich_unicode_and_paragraph_anchors_through_edits() {
        let mut spec = TextSpec {
            text: "Alpha 😀\nBeta\nGamma".into(),
            size: 18.,
            width: Some(180.),
            ..Default::default()
        };
        spec.apply_style(6..10, |s| s.bold = true);
        let format = ParagraphFormat {
            list: ParagraphList::Numbered,
            indent: 36.,
            hanging: 24.,
            ..Default::default()
        };
        let mut next = apply_paragraphs(&spec, 0..spec.text.len(), format).unwrap();
        assert_eq!(next.text, "1. Alpha 😀\n2. Beta\n3. Gamma");
        assert!(next.style_at(next.text.find('😀').unwrap()).bold);
        let start = next.text.find("2. Beta").unwrap();
        next = apply_paragraphs(
            &next,
            start..start + 1,
            ParagraphFormat { level: 1, ..format },
        )
        .unwrap();
        assert_eq!(next.text, "1. Alpha 😀\n1.1. Beta\n2. Gamma");
        assert_eq!(next.paragraphs[1].format.level, 1);
        let encoded = serde_json::to_string(&next).unwrap();
        let decoded: TextSpec = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, next);
        let before_start = next.paragraphs[1].start;
        next.replace_range(0..0, "Préface\n");
        assert_eq!(
            next.paragraphs
                .iter()
                .find(|p| p.format.level == 1)
                .unwrap()
                .start,
            before_start + "Préface\n".len()
        );
        let starts = super::starts(&next.text);
        assert!(next.paragraphs.iter().all(|p| starts.contains(&p.start)));
        assert!(apply_paragraphs(&spec, 7..8, format).is_err());
        assert!(apply_paragraphs(&spec, 0..1, ParagraphFormat { level: 9, ..format }).is_err());
    }
    #[test]
    fn hanging_lists_wrap_at_native_content_edge_and_share_caret_ink_geometry() {
        let spec = TextSpec {
            text:
                "Many words wrap naturally onto several lines and remain editable\nSecond paragraph"
                    .into(),
            size: 18.,
            width: Some(150.),
            underline: true,
            ..Default::default()
        };
        let format = ParagraphFormat {
            list: ParagraphList::Bullet,
            indent: 40.,
            hanging: 28.,
            space_before: 5.,
            space_after: 9.,
            ..Default::default()
        };
        let next = apply_paragraphs(&spec, 0..spec.text.len(), format).unwrap();
        let measured = layout(&next);
        let body = measured
            .cells
            .iter()
            .find(|c| c.range.start == "• ".len())
            .unwrap();
        assert!((body.rect.x - 40.).abs() < 0.1);
        assert!(body.rect.y >= 5.);
        let first_y = body.rect.y;
        let later = measured
            .cells
            .iter()
            .filter(|c| c.rect.y > first_y + 1. && c.range.start < next.text.find('\n').unwrap())
            .collect::<Vec<_>>();
        assert!(!later.is_empty());
        assert!(later.iter().all(|c| c.rect.x >= 39.9));
        assert!(decoration_rects(&next).iter().any(|(r, _)| r[0] >= 40.));
        assert!(!vector_paths(&next).unwrap().is_empty());
        let mut cpu = false;
        draw_glyphs(&next, |x, y, _, _, _, coverage| {
            if x >= 40 && y > 30 && coverage > 0. {
                cpu = true;
            }
        });
        assert!(cpu);
        let first_end = measured
            .cells
            .iter()
            .filter(|c| c.range.start < next.text.find('\n').unwrap())
            .map(|c| c.rect.y + c.rect.height)
            .fold(0., f32::max);
        let second = measured
            .cells
            .iter()
            .find(|c| c.range.start == next.text.find('\n').unwrap() + 1)
            .unwrap();
        assert!(second.rect.y >= first_end + 13.);
    }
    #[test]
    fn paragraph_command_undo_clipboard_and_legacy_defaults_are_lossless() {
        use crate::{Command, Document, Editor, Node, NodeKind, command::Slot};
        let mut e = Editor::new(Document::new(400, 300), None);
        let spec = TextSpec {
            text: "One\nTwo".into(),
            ..Default::default()
        };
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Node::text(0, "List", spec.clone(), 400, 300)),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let before = e.doc.clone();
        let formatted = apply_paragraphs(
            &spec,
            0..spec.text.len(),
            ParagraphFormat {
                list: ParagraphList::Numbered,
                indent: 80.,
                hanging: 60.,
                ..Default::default()
            },
        )
        .unwrap();
        e.execute(Command::SetText {
            id,
            spec: Box::new(formatted.clone()),
        })
        .unwrap();
        let f = crate::fragment::Fragment::capture(&e.doc, &[id]).unwrap();
        let NodeKind::Text { spec, .. } = &f.nodes[0].kind else {
            panic!()
        };
        assert_eq!(spec.paragraphs, formatted.paragraphs);
        e.undo();
        assert_eq!(e.doc, before);
        let legacy: TextSpec = serde_json::from_str("{\"text\":\"Legacy\"}").unwrap();
        assert!(legacy.paragraphs.is_empty());
    }
}

/// Enter in a list creates the next editable marker. Enter on an empty item outdents
/// one level, or exits the list at level zero. Non-list paragraphs return None.
pub fn paragraph_enter(
    spec: &TextSpec,
    range: std::ops::Range<usize>,
) -> Result<Option<(TextSpec, usize)>, String> {
    if range.start > range.end
        || range.end > spec.text.len()
        || !spec.text.is_char_boundary(range.start)
        || !spec.text.is_char_boundary(range.end)
    {
        return Err("Invalid text insertion range.".into());
    }
    let offsets = starts(&spec.text);
    let index = offsets
        .partition_point(|p| *p <= range.start)
        .saturating_sub(1);
    let start = offsets[index];
    let end = offsets.get(index + 1).map_or(spec.text.len(), |n| n - 1);
    let Some(mut format) = spec
        .paragraphs
        .iter()
        .find(|p| p.start == start)
        .map(|p| p.format)
        .filter(|p| p.list != ParagraphList::None)
    else {
        return Ok(None);
    };
    let line = spec.text[start..end].trim_end_matches('\r');
    let prefix = prefix_len(line);
    if range.is_empty() && line[prefix..].trim().is_empty() {
        if format.level > 0 {
            format.level -= 1;
        } else {
            format = ParagraphFormat {
                align: format.align,
                space_before: format.space_before,
                space_after: format.space_after,
                ..Default::default()
            };
        }
        let next = apply_paragraphs(spec, start..end, format)?;
        let start = starts(&next.text)[index];
        let line = next.text[start..].split('\n').next().unwrap_or("");
        let cursor = start + prefix_len(line);
        return Ok(Some((next, cursor)));
    }
    let mut next = spec.clone();
    next.replace_range(range.clone(), "\n");
    let new_start = range.start + 1;
    let next_index = starts(&next.text)
        .binary_search(&new_start)
        .map_err(|_| "Invalid list continuation")?;
    format.restart = None;
    next = apply_paragraphs(&next, new_start..new_start, format)?;
    let start = starts(&next.text)[next_index];
    let line = next.text[start..].split('\n').next().unwrap_or("");
    let cursor = start + prefix_len(line);
    Ok(Some((next, cursor)))
}

#[cfg(test)]
mod continuation_tests {
    use super::*;
    #[test]
    fn paragraph_formatting_preserves_literal_number_and_bullet_prefixes() {
        let spec = TextSpec {
            text: "2024. Revenue\n• Literal bullet".into(),
            ..Default::default()
        };
        let spaced = apply_paragraphs(
            &spec,
            0..spec.text.len(),
            ParagraphFormat {
                space_after: 12.,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(spaced.text, spec.text);
        let listed = apply_paragraphs(
            &spaced,
            0..spaced.text.len(),
            ParagraphFormat {
                list: ParagraphList::Numbered,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(listed.text, "1. 2024. Revenue\n2. • Literal bullet");
        let removed =
            apply_paragraphs(&listed, 0..listed.text.len(), ParagraphFormat::default()).unwrap();
        assert_eq!(removed.text, spec.text);
        assert_eq!(paragraph_content(&listed), spec.text);
        assert_eq!(paragraph_content(&spec), spec.text);
    }
    #[test]
    fn enter_continues_nested_lists_and_exits_empty_item_with_correct_cursor() {
        let spec = TextSpec {
            text: "First".into(),
            size: 18.,
            ..Default::default()
        };
        let format = ParagraphFormat {
            list: ParagraphList::Numbered,
            indent: 36.,
            hanging: 24.,
            ..Default::default()
        };
        let first = apply_paragraphs(&spec, 0..spec.text.len(), format).unwrap();
        let (next, cursor) = paragraph_enter(&first, first.text.len()..first.text.len())
            .unwrap()
            .unwrap();
        assert_eq!(next.text, "1. First\n2. ");
        assert_eq!(cursor, next.text.len());
        let (exited, cursor) = paragraph_enter(&next, cursor..cursor).unwrap().unwrap();
        assert_eq!(exited.text, "1. First\n");
        assert_eq!(cursor, exited.text.len());
        assert_eq!(
            exited.paragraphs.last().unwrap().format.list,
            ParagraphList::None
        );
    }
}
