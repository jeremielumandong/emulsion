use super::*;
use emulsion_core::text::{
    Align, ParagraphFormat, ParagraphList, ParagraphStyle, TextRun, TextSpec, TextStyle,
};
use geometry::{color, number};
fn style(x: Option<&Xml>, base: &TextStyle, theme: &BTreeMap<String, [u8; 4]>) -> TextStyle {
    let mut s = base.clone();
    let Some(x) = x else {
        return s;
    };
    if !x.attr("sz").is_empty() {
        s.size = (number(x, "sz", 1800.) / 75.).clamp(1., 4000.) as f32;
    }
    if let Some(font) = x.child("latin") {
        s.font = font.attr("typeface").into();
    }
    if s.font.starts_with('+') {
        s.font = "Arial".into();
    }
    for (key, target) in [("b", &mut s.bold), ("i", &mut s.italic)] {
        if !x.attr(key).is_empty() {
            *target = matches!(x.attr(key), "1" | "true");
        }
    }
    if !x.attr("u").is_empty() {
        s.underline = x.attr("u") != "none";
    }
    if !x.attr("strike").is_empty() {
        s.strikethrough = x.attr("strike") != "noStrike";
    }
    if let Some(c) = x.child("solidFill").and_then(|x| color(x, theme)) {
        s.color = c;
    }
    if !x.attr("spc").is_empty() {
        s.letter_spacing = (number(x, "spc", 0.) / 75.).clamp(-50., 500.) as f32;
    }
    if !x.attr("baseline").is_empty() {
        s.baseline = (number(x, "baseline", 0.) / 100000. * f64::from(s.size)) as f32;
    }
    s
}
fn spacing(x: Option<&Xml>) -> f32 {
    x.and_then(|x| x.child("spcPts")).map_or(0., |x| {
        (number(x, "val", 0.) / 75.).clamp(0., 10000.) as f32
    })
}
pub(super) fn decode(
    body: &Xml,
    size: (f64, f64),
    m: DAffine2,
    theme: &BTreeMap<String, [u8; 4]>,
) -> Result<TextSpec> {
    let bp = body.child("bodyPr");
    let inset = |k, default| bp.map_or(default, |b| number(b, k, default * EMU) / EMU);
    let (l, t, r, b) = (
        inset("lIns", 9.6),
        inset("tIns", 4.8),
        inset("rIns", 9.6),
        inset("bIns", 4.8),
    );
    let origin = m.transform_point2(dvec2(l, t));
    let ax = m.matrix2.x_axis;
    let ay = m.matrix2.y_axis;
    if ax.dot(ay).abs() > 1e-6 * ax.length() * ay.length() {
        return Err(error("Skewed text transform cannot remain native text"));
    }
    let base = TextStyle {
        font: "Arial".into(),
        size: 24.,
        ..Default::default()
    };
    let mut spec = TextSpec {
        text: String::new(),
        font: base.font.clone(),
        size: base.size,
        width: Some((size.0 - l - r).max(1.) as f32),
        height: Some((size.1 - t - b).max(1.) as f32),
        x: origin.x as f32,
        y: origin.y as f32,
        scale_x: ax.length() as f32,
        scale_y: (ay.length() * m.matrix2.determinant().signum()) as f32,
        rotation: ax.y.atan2(ax.x).to_degrees() as f32,
        ..Default::default()
    };
    let mut list_formats = Vec::new();
    for (index, p) in body.children("p").enumerate() {
        if index > 0 {
            spec.text.push('\n');
        }
        let start = spec.text.len();
        let pp = p.child("pPr");
        let level = pp.map_or(0, |x| number(x, "lvl", 0.).clamp(0., 8.) as u8);
        let inherited = body
            .child("lstStyle")
            .and_then(|x| x.child(&format!("lvl{}pPr", level + 1)));
        let base = style(inherited.and_then(|x| x.child("defRPr")), &base, theme);
        let base = style(pp.and_then(|x| x.child("defRPr")), &base, theme);
        for run in &p.children {
            match run.name.as_str() {
                "r" | "fld" => {
                    let value = run
                        .children("t")
                        .map(|x| x.text.as_str())
                        .collect::<String>();
                    let a = spec.text.len();
                    spec.text.push_str(&value);
                    let end = spec.text.len();
                    if end > a {
                        spec.runs.push(TextRun {
                            start: a,
                            end,
                            style: style(run.child("rPr"), &base, theme),
                        });
                    }
                }
                "br" => spec.text.push('\n'),
                _ => (),
            }
        }
        let mut format = ParagraphFormat {
            level,
            align: Some(match pp.map_or("", |x| x.attr("algn")) {
                "ctr" => Align::Center,
                "r" => Align::Right,
                "just" | "dist" => Align::Justify,
                _ => Align::Left,
            }),
            ..Default::default()
        };
        if let Some(pp) = pp.or(inherited) {
            format.indent = (number(pp, "marL", 0.) / EMU).clamp(0., 10000.) as f32;
            // Native levels add an em indent; subtract that from the explicit absolute margin.
            format.indent = (format.indent - f32::from(level) * spec.size * 1.5).max(0.);
            format.hanging = (-number(pp, "indent", 0.) / EMU).clamp(0., 10000.) as f32;
            format.space_before = spacing(pp.child("spcBef"));
            format.space_after = spacing(pp.child("spcAft"));
            if pp.child("buChar").is_some() {
                format.list = ParagraphList::Bullet;
            }
            if let Some(n) = pp.child("buAutoNum") {
                format.list = ParagraphList::Numbered;
                format.restart = Some(number(n, "startAt", 1.).clamp(1., 1000000.) as u32);
            }
            if let Some(v) = pp.child("lnSpc").and_then(|x| x.child("spcPct")) {
                spec.line_height = (number(v, "val", 120000.) / 100000.).clamp(0.5, 10.) as f32;
            }
        }
        spec.paragraphs.push(ParagraphStyle {
            start,
            format: ParagraphFormat {
                list: ParagraphList::None,
                ..format
            },
        });
        list_formats.push((start, format));
    }
    if let Some(first) = spec.runs.first() {
        let first = first.style.clone();
        spec.font = first.font;
        spec.size = first.size;
        spec.color = first.color;
        spec.bold = first.bold;
        spec.italic = first.italic;
        spec.underline = first.underline;
        spec.strikethrough = first.strikethrough;
        spec.letter_spacing = first.letter_spacing;
    }
    if spec.text.chars().count() > emulsion_core::text::MAX_CHARS {
        return Err(error("Text object exceeds 20,000 characters"));
    }
    for (start, format) in list_formats.into_iter().rev() {
        if format.list == ParagraphList::None {
            continue;
        }
        spec = emulsion_core::text::apply_paragraphs(&spec, start..start, format).map_err(error)?;
    }
    if let Some(bp) = bp {
        let available = f64::from(spec.height.unwrap_or(0.));
        let used = f64::from(emulsion_core::text::layout(&spec).bounds().height);
        let offset = match bp.attr("anchor") {
            "ctr" => (available - used).max(0.) / 2.,
            "b" => (available - used).max(0.),
            _ => 0.,
        };
        let shift = m.transform_vector2(dvec2(0., offset));
        spec.x += shift.x as f32;
        spec.y += shift.y as f32;
    }
    Ok(spec.sanitized())
}
fn run_xml(value: &str, s: &TextStyle, scale: f32) -> String {
    format!(
        "<a:r><a:rPr lang=\"en-US\" sz=\"{}\" b=\"{}\" i=\"{}\" u=\"{}\" strike=\"{}\" spc=\"{}\" baseline=\"{}\"><a:solidFill>{}</a:solidFill><a:latin typeface=\"{}\"/></a:rPr><a:t xml:space=\"preserve\">{}</a:t></a:r>",
        (s.size * scale * 75.).round().clamp(100., 400000.) as i32,
        u8::from(s.bold),
        u8::from(s.italic),
        if s.underline { "sng" } else { "none" },
        if s.strikethrough {
            "sngStrike"
        } else {
            "noStrike"
        },
        (s.letter_spacing * scale * 75.).round() as i32,
        (s.baseline / s.size.max(1.) * 100000.).round() as i32,
        geometry::color_xml(s.color),
        escaped(if s.font.is_empty() { "Arial" } else { &s.font }),
        escaped(value)
    )
}
pub(super) fn encode(spec: &TextSpec) -> String {
    let mut out = String::from(
        "<p:txBody><a:bodyPr wrap=\"square\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"t\"><a:noAutofit/></a:bodyPr><a:lstStyle/>",
    );
    let mut offset = 0;
    for line in spec.text.split('\n') {
        let f = spec
            .paragraphs
            .iter()
            .find(|p| p.start == offset)
            .map(|p| p.format)
            .unwrap_or_default();
        let align = f.align.unwrap_or(spec.align);
        let align = match align {
            Align::Left => "l",
            Align::Center => "ctr",
            Align::Right => "r",
            Align::Justify => "just",
        };
        // Editable source markers are retained literally, so nested numbering has identical content.
        out.push_str(&format!("<a:p><a:pPr algn=\"{align}\" marL=\"{}\" indent=\"{}\" lvl=\"{}\"><a:lnSpc><a:spcPct val=\"{}\"/></a:lnSpc><a:spcBef><a:spcPts val=\"{}\"/></a:spcBef><a:spcAft><a:spcPts val=\"{}\"/></a:spcAft><a:buNone/></a:pPr>",((f.indent+f32::from(f.level)*spec.size*1.5)*EMU as f32)as i64,-(f.hanging*EMU as f32)as i64,f.level,(spec.line_height*100000.)as i64,(f.space_before*75.)as i64,(f.space_after*75.)as i64));
        let mut boundaries = vec![offset, offset + line.len()];
        for r in &spec.runs {
            if r.start > offset && r.start < offset + line.len() {
                boundaries.push(r.start);
            }
            if r.end > offset && r.end < offset + line.len() {
                boundaries.push(r.end);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        for pair in boundaries.windows(2) {
            out.push_str(&run_xml(
                &spec.text[pair[0]..pair[1]],
                &spec.style_at(pair[0]),
                spec.scale_y.abs(),
            ));
        }
        out.push_str("<a:endParaRPr lang=\"en-US\"/></a:p>");
        offset += line.len() + 1;
    }
    out.push_str("</p:txBody>");
    out
}
