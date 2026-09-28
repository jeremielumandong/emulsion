//! HTML labels become native styled UTF-8 text runs, with no browser execution.
use super::*;
use emulsion_core::text::{Align, TextRun, TextSpec, TextStyle};
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
fn size(value: &str, base: f32) -> Option<f32> {
    let value = value.trim();
    let factor = if value.ends_with("pt") {
        96. / 72.
    } else if value.ends_with("em") {
        base
    } else if value.ends_with('%') {
        base / 100.
    } else {
        1.
    };
    value
        .trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%')
        .parse::<f32>()
        .ok()
        .map(|v| (v * factor).clamp(1., 1000.))
}
pub(super) fn parse(html: &str, base: &TextSpec, warnings: &mut BTreeSet<String>) -> TextSpec {
    let dom = parse_document(RcDom::default(), Default::default()).one(html);
    let mut spec = base.clone();
    spec.text.clear();
    spec.runs.clear();
    fn newline(spec: &mut TextSpec) {
        if !spec.text.is_empty() && !spec.text.ends_with('\n') {
            spec.text.push('\n');
        }
    }
    fn walk(
        node: &Handle,
        style: TextStyle,
        spec: &mut TextSpec,
        warnings: &mut BTreeSet<String>,
        depth: usize,
        pre: bool,
    ) {
        if depth > 64 || spec.text.chars().count() > emulsion_core::text::MAX_CHARS {
            return;
        }
        match &node.data {
            NodeData::Text { contents } => {
                let text = contents.borrow();
                let mut value = String::new();
                for c in text.chars() {
                    if !pre && c.is_ascii_whitespace() {
                        if (!value.ends_with(' ') && !value.is_empty())
                            || (value.is_empty()
                                && !spec.text.is_empty()
                                && !spec.text.ends_with([' ', '\n', '\t']))
                        {
                            value.push(' ');
                        }
                    } else {
                        value.push(if c == '\u{a0}' { ' ' } else { c });
                    }
                }
                if value.is_empty() {
                    return;
                }
                let start = spec.text.len();
                spec.text.push_str(&value);
                spec.runs.push(TextRun {
                    start,
                    end: spec.text.len(),
                    style,
                });
            }
            NodeData::Element { name, attrs, .. } => {
                let tag = name.local.as_ref();
                if matches!(tag, "script" | "style" | "iframe" | "object") {
                    return;
                }
                if tag == "br" {
                    spec.text.push('\n');
                    return;
                }
                let block = matches!(tag, "div" | "p" | "li" | "tr" | "h1" | "h2" | "h3" | "pre");
                if block {
                    newline(spec);
                }
                if matches!(tag, "td" | "th") && !spec.text.is_empty() && !spec.text.ends_with('\n')
                {
                    spec.text.push('\t');
                }
                let mut style = style;
                style.bold |= matches!(tag, "b" | "strong" | "th" | "h1" | "h2" | "h3");
                style.italic |= matches!(tag, "i" | "em");
                style.underline |= matches!(tag, "u" | "a");
                style.strikethrough |= matches!(tag, "s" | "strike" | "del");
                if tag == "sup" {
                    style.baseline += style.size * 0.35;
                    style.size *= 0.75;
                }
                if tag == "sub" {
                    style.baseline -= style.size * 0.2;
                    style.size *= 0.75;
                }
                if tag == "li" {
                    spec.text.push_str("• ");
                }
                for a in attrs.borrow().iter() {
                    let value = a.value.as_ref();
                    match a.name.local.as_ref() {
                        "color" => {
                            if let Some(Some(c)) = crate::svg::color(value) {
                                style.color = c
                            }
                        }
                        "face" => style.font = value.into(),
                        "style" => {
                            for declaration in value.split(';') {
                                if let Some((key, value)) = declaration.split_once(':') {
                                    let value = value.trim();
                                    match key.trim() {
                                        "font-size" => {
                                            if let Some(v) = size(value, style.size) {
                                                style.size = v
                                            }
                                        }
                                        "font-family" => {
                                            style.font = value.trim_matches(['\'', '"']).into()
                                        }
                                        "font-weight" => {
                                            style.bold = value == "bold"
                                                || value.parse::<u32>().is_ok_and(|v| v >= 600)
                                        }
                                        "font-style" => {
                                            style.italic = value == "italic" || value == "oblique"
                                        }
                                        "color" => {
                                            if let Some(Some(c)) = crate::svg::color(value) {
                                                style.color = c
                                            }
                                        }
                                        "text-decoration" => {
                                            style.underline = value.contains("underline");
                                            style.strikethrough = value.contains("line-through");
                                        }
                                        "letter-spacing" => {
                                            if let Ok(v) =
                                                value.trim_end_matches("px").parse::<f32>()
                                            {
                                                style.letter_spacing = v;
                                            }
                                        }
                                        "vertical-align" => {
                                            if let Ok(v) =
                                                value.trim_end_matches("px").parse::<f32>()
                                            {
                                                style.baseline = v;
                                            }
                                        }
                                        "text-align" => {
                                            if let Some(align) = Align::parse(value) {
                                                spec.align = align
                                            }
                                        }
                                        "line-height" => {
                                            let ratio=if let Some(v)=value.strip_suffix('%') {
                                                v.parse::<f32>().ok().map(|v|v/100.)
                                            } else if value.ends_with("px") || value.ends_with("pt") || value.ends_with("em") {
                                                size(value,style.size).map(|v|v/style.size.max(1.))
                                            } else {value.parse::<f32>().ok()};
                                            if let Some(v)=ratio.filter(|v|v.is_finite()) {spec.line_height=v.clamp(0.5,5.);}

                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if tag == "table" {
                    warnings.insert("HTML table labels retain editable rows, columns and text formatting; browser table sizing is approximated.".into());
                }
                for child in node.children.borrow().iter() {
                    walk(
                        child,
                        style.clone(),
                        spec,
                        warnings,
                        depth + 1,
                        pre || tag == "pre",
                    );
                }
                if block {
                    newline(spec);
                }
            }
            _ => {
                for child in node.children.borrow().iter() {
                    walk(child, style.clone(), spec, warnings, depth + 1, pre);
                }
            }
        }
    }
    walk(
        &dom.document,
        base.base_style(),
        &mut spec,
        warnings,
        0,
        false,
    );
    spec.text = spec.text.trim_end().to_string();
    let len = spec.text.len();
    spec.runs.retain_mut(|r| {
        r.end = r.end.min(len);
        r.start < r.end
    });
    spec.sanitized()
}
pub(super) fn apply(
    doc: &mut Document,
    label: NodeId,
    cell: &Cell,
    style: &BTreeMap<String, String>,
    warnings: &mut BTreeSet<String>,
) {
    if !style.get("html").is_some_and(|v| v == "1") {
        return;
    }
    let (w, h) = (doc.width, doc.height);
    if let Some(Node {
        kind: NodeKind::Text { spec, cache },
        ..
    }) = doc.node_mut(label)
    {
        *spec = Arc::new(parse(
            cell.attrs.get("value").map_or("", String::as_str),
            spec,
            warnings,
        ));
        *cache = emulsion_core::vector_cache::VectorRaster::text(spec.clone(), w, h);
    }
}

pub(super) fn html(spec: &TextSpec) -> String {
    fn span(text: &str, style: &TextStyle) -> String {
        let decoration = match (style.underline, style.strikethrough) {
            (true, true) => "underline line-through",
            (true, false) => "underline",
            (false, true) => "line-through",
            _ => "none",
        };
        let css = format!(
            "font-family:{};font-size:{}px;color:{};font-weight:{};font-style:{};text-decoration:{};letter-spacing:{}px;vertical-align:{}px;",
            style.font.replace([';', '"', '\''], ""),
            style.size,
            hex(Some(style.color)),
            if style.bold { "bold" } else { "normal" },
            if style.italic { "italic" } else { "normal" },
            decoration,
            style.letter_spacing,
            style.baseline
        );
        format!(
            "<span style=\"{}\">{}</span>",
            escape(&css),
            escape(text).replace('\n', "<br/>")
        )
    }
    let mut output = String::new();
    let mut offset = 0;
    for run in &spec.runs {
        if run.start > offset {
            output.push_str(&span(&spec.text[offset..run.start], &spec.base_style()));
        }
        output.push_str(&span(&spec.text[run.start..run.end], &run.style));
        offset = run.end;
    }
    if offset < spec.text.len() {
        output.push_str(&span(&spec.text[offset..], &spec.base_style()));
    }
    format!(
        "<pre style=\"line-height:{}\">{output}</pre>",
        spec.line_height
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn css_line_height_preserves_percentage_and_absolute_spacing() {
        let base=TextSpec{size:20.,..Default::default()};
        for (css,expected) in [("170%",1.7),("30px",1.5),("24pt",1.6),("1.25em",1.25),("1.4",1.4)] {
            let spec=parse(&format!("<p style='line-height:{css}'>One<br>Two</p>"),&base,&mut BTreeSet::new());
            assert!((spec.line_height-expected).abs()<0.001,"{css}: {}",spec.line_height);
        }
    }
    #[test]
    fn styled_unicode_label_roundtrip() {
        let mut warnings = BTreeSet::new();
        let base = TextSpec::default();
        let first = parse(
            "<b>Árbol</b> <i style='color:#ee0011;font-size:21px'>東京</i><br/><u>Link</u>",
            &base,
            &mut warnings,
        );
        let second = parse(&html(&first), &base, &mut warnings);
        assert_eq!(first.text, second.text);
        for (a, b) in first.runs.iter().zip(&second.runs) {
            assert_eq!(a.style, b.style);
        }
    }
}
