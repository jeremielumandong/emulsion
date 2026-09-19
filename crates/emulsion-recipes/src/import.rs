//! Read a recipe from the text people paste: a blog post's settings block
//! or a camera menu typed out. Lines are matched by their label; unknown
//! lines are ignored, and anything not mentioned keeps its default.

use crate::{
    DynamicRange, Grain, GrainSize, Recipe, Strength, WhiteBalance, looks, parse_fraction,
};

/// Parse a pasted block. Returns the recipe and the lines it did not
/// understand, so the panel can show them.
pub fn parse_text(text: &str) -> (Recipe, Vec<String>) {
    let mut r = Recipe::default();
    let mut unknown = Vec::new();
    let mut named = false;
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    for line in &lines {
        let Some((label, value)) = split_label(line) else {
            // A bare first line is the recipe's name.
            if !named && line.chars().count() <= 80 && !line.contains(':') {
                r.name = line
                    .trim_matches(['“', '”', '"', '*', '#'])
                    .trim()
                    .to_string();
                named = true;
            } else {
                unknown.push(line.to_string());
            }
            continue;
        };
        let key = norm(&label);
        let v = value.trim();
        let vl = v.to_lowercase();
        match key.as_str() {
            "name" | "recipe" | "title" => {
                r.name = v.to_string();
                named = true;
            }
            "author" | "by" | "creator" => r.author = v.to_string(),
            "source" | "url" | "link" => r.source_url = v.to_string(),
            "sensor" | "camera" | "cameras" | "compatible" => {
                r.sensor = v
                    .split([',', '/', ';'])
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "filmsimulation" | "simulation" | "film" | "baselook" | "look" => {
                match looks::find(v) {
                    Some(l) => r.film_simulation = l.key.into(),
                    None => {
                        // Keep the name so validation rejects it with the list of looks.
                        r.film_simulation = v.to_string();
                        unknown.push(line.to_string());
                    }
                }
            }
            "dynamicrange" | "dr" => {
                r.dynamic_range = if vl.contains("400") {
                    DynamicRange::Dr400
                } else if vl.contains("200") {
                    DynamicRange::Dr200
                } else {
                    DynamicRange::Dr100
                };
            }
            "grain" | "graineffect" => {
                r.grain = Grain {
                    strength: strength(&vl),
                    size: if vl.contains("large") {
                        GrainSize::Large
                    } else {
                        GrainSize::Small
                    },
                };
            }
            "colorchromeeffect" | "colourchromeeffect" | "colorchrome" | "cce" => {
                r.color_chrome_effect = strength(&vl)
            }
            "colorchromefxblue"
            | "colourchromefxblue"
            | "colorchromeeffectblue"
            | "colorchromeblue"
            | "fxblue"
            | "ccfxblue" => r.color_chrome_fx_blue = strength(&vl),
            "whitebalance" | "wb" => r.white_balance = white_balance(v),
            "highlight" | "highlights" | "highlighttone" => r.highlight = num(v).unwrap_or(0.0),
            "shadow" | "shadows" | "shadowtone" => r.shadow = num(v).unwrap_or(0.0),
            "color" | "colour" | "saturation" => r.color = num(v).unwrap_or(0.0),
            "sharpness" | "sharpening" => r.sharpness = num(v).unwrap_or(0.0),
            "noisereduction" | "nr" | "highisonr" => r.noise_reduction = num(v).unwrap_or(0.0),
            "clarity" => r.clarity = num(v).unwrap_or(0.0),
            "exposurecompensation" | "exposure" | "ev" | "exposurecomp" => {
                r.exposure_compensation = v.to_string()
            }
            "iso" => r.iso = v.to_string(),
            "lut" => r.lut = Some(v.to_string()),
            "notes" | "note" => r.notes = v.to_string(),
            "tags" | "mood" => {
                r.tags = v
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            }
            _ => unknown.push(line.to_string()),
        }
    }
    (r, unknown)
}

fn split_label(line: &str) -> Option<(String, String)> {
    let cleaned = line.trim_start_matches(['-', '*', '•', ' ']);
    // Blog posts use colons; camera-menu transcriptions often use dashes.
    let seps = [":", "=", "–", "—", " - "];
    let (idx, sep_len) = seps
        .iter()
        .filter_map(|sep| cleaned.find(sep).map(|i| (i, sep.len())))
        .min_by_key(|(i, _)| *i)?;
    let (label, value) = cleaned.split_at(idx);
    let label = label.trim().trim_matches('*');
    // Labels are short; a separator deep in a sentence is not one.
    if label.is_empty() || label.split_whitespace().count() > 4 {
        return None;
    }
    Some((label.to_string(), value[sep_len..].trim().to_string()))
}

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn strength(v: &str) -> Strength {
    if v.contains("strong") {
        Strength::Strong
    } else if v.contains("weak") || v.contains("low") {
        Strength::Weak
    } else if v.contains("off") || v.contains("none") || v.trim().is_empty() {
        Strength::Off
    } else {
        Strength::Weak
    }
}

fn num(v: &str) -> Option<f32> {
    let cleaned = v.replace(['–', '−'], "-");
    cleaned.split_whitespace().find_map(parse_fraction)
}

/// "Auto, +2 Red & -4 Blue", "Daylight, R: +3 B: -5", "5200K, -1 Red & +2 Blue", "Auto".
fn white_balance(v: &str) -> WhiteBalance {
    let mut wb = WhiteBalance::default();
    let t = v.replace(['–', '−'], "-");
    let lower = t.to_lowercase();
    // Kelvin.
    if let Some(k) = lower.split(|c: char| !c.is_alphanumeric()).find(|tok| {
        tok.ends_with('k')
            && tok.len() > 1
            && tok[..tok.len() - 1].chars().all(|c| c.is_ascii_digit())
    }) {
        wb.kelvin = k[..k.len() - 1].parse().ok();
        wb.preset = "kelvin".into();
    } else {
        for preset in [
            "auto white priority",
            "auto ambience",
            "auto",
            "daylight",
            "fine",
            "shade",
            "cloudy",
            "fluorescent 3",
            "fluorescent 2",
            "fluorescent 1",
            "fluorescent",
            "incandescent",
            "tungsten",
            "underwater",
        ] {
            if lower.contains(preset) {
                wb.preset = match preset {
                    "auto white priority" | "auto ambience" => "auto".into(),
                    "fine" => "daylight".into(),
                    "tungsten" => "incandescent".into(),
                    p => p.replace(' ', "-"),
                };
                break;
            }
        }
    }
    // Shifts: a signed number followed by red/blue, or "R +2 B -4".
    let toks: Vec<&str> = lower
        .split(|c: char| c.is_whitespace() || c == ',' || c == '&' || c == ':' || c == '/')
        .filter(|s| !s.is_empty())
        .collect();
    for (i, tok) in toks.iter().enumerate() {
        let is_red = tok.starts_with("red") || *tok == "r";
        let is_blue = tok.starts_with("blue") || *tok == "b";
        if !(is_red || is_blue) {
            continue;
        }
        // "+2 red" puts the number first; "R: +3" puts it after.
        let before = i
            .checked_sub(1)
            .and_then(|j| toks.get(j))
            .and_then(|s| parse_shift(s));
        let after = toks.get(i + 1).and_then(|s| parse_shift(s));
        let n = if tok.len() == 1 {
            after.or(before)
        } else {
            before.or(after)
        };
        if let Some(n) = n {
            if is_red {
                wb.red = n.clamp(-9, 9);
            } else {
                wb.blue = n.clamp(-9, 9);
            }
        }
    }
    wb
}

fn parse_shift(s: &str) -> Option<i32> {
    let s = s.trim();
    if s.is_empty() || s.len() > 3 {
        return None;
    }
    let (sign, body) = match s.as_bytes()[0] {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => (1, s),
    };
    body.parse::<i32>().ok().map(|v| sign * v)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOG: &str = "\
Kodachrome-ish Street
Film Simulation: Classic Chrome
Grain Effect: Weak, Small
Color Chrome Effect: Strong
Color Chrome FX Blue: Off
White Balance: Daylight, +2 Red & -4 Blue
Dynamic Range: DR400
Highlight: -1
Shadow: +1
Color: +2
Sharpness: -2
Noise Reduction: -4
Clarity: -3
ISO: Auto, up to ISO 6400
Exposure Compensation: +1/3 to +2/3 (typically)
";

    const MENU: &str = "\
- Film Simulation – Classic Neg.
- Dynamic Range – DR200
- Grain Effect – Strong, Large
- Color Chrome Effect – Weak
- Color Chrome Effect Blue – Strong
- White Balance – Auto, R: +3 B: -5
- Highlight Tone – −2
- Shadow Tone – −1
- Colour – +2
- Sharpness – −1
- High ISO NR – −4
- Clarity – −2
- Exposure – +2/3 to +1 1/3
- Something odd: nobody knows
";

    const KELVIN: &str = "\
Name: Winter Blue
Film: ACROS+R
WB: 5200K, -1 Red & +2 Blue
Grain: Off
Shadow: 0
";

    #[test]
    fn blog_block() {
        let (r, unknown) = parse_text(BLOG);
        assert_eq!(r.name, "Kodachrome-ish Street");
        assert_eq!(r.film_simulation, "classic-chrome");
        assert_eq!(
            r.grain,
            Grain {
                strength: Strength::Weak,
                size: GrainSize::Small
            }
        );
        assert_eq!(r.color_chrome_effect, Strength::Strong);
        assert_eq!(r.color_chrome_fx_blue, Strength::Off);
        assert_eq!(
            (
                r.white_balance.preset.as_str(),
                r.white_balance.red,
                r.white_balance.blue
            ),
            ("daylight", 2, -4)
        );
        assert_eq!(r.dynamic_range, DynamicRange::Dr400);
        assert_eq!(
            (
                r.highlight,
                r.shadow,
                r.color,
                r.sharpness,
                r.noise_reduction,
                r.clarity
            ),
            (-1.0, 1.0, 2.0, -2.0, -4.0, -3.0)
        );
        assert!((r.exposure_ev() - 0.5).abs() < 1e-3);
        assert!(unknown.is_empty(), "{unknown:?}");
        r.validate().unwrap();
    }

    #[test]
    fn menu_block_with_dashes_and_unicode_minus() {
        let (r, unknown) = parse_text(MENU);
        assert_eq!(r.film_simulation, "classic-negative");
        assert_eq!(r.dynamic_range, DynamicRange::Dr200);
        assert_eq!(
            r.grain,
            Grain {
                strength: Strength::Strong,
                size: GrainSize::Large
            }
        );
        assert_eq!(r.color_chrome_fx_blue, Strength::Strong);
        assert_eq!((r.white_balance.red, r.white_balance.blue), (3, -5));
        assert_eq!(
            (r.highlight, r.shadow, r.color, r.noise_reduction),
            (-2.0, -1.0, 2.0, -4.0)
        );
        assert!((r.exposure_ev() - 1.0).abs() < 1e-3);
        assert_eq!(unknown, vec!["- Something odd: nobody knows".to_string()]);
    }

    #[test]
    fn kelvin_and_named_lines() {
        let (r, _) = parse_text(KELVIN);
        assert_eq!(r.name, "Winter Blue");
        assert_eq!(r.film_simulation, "acros-r");
        assert_eq!(r.white_balance.kelvin, Some(5200));
        assert_eq!((r.white_balance.red, r.white_balance.blue), (-1, 2));
        assert_eq!(r.grain.strength, Strength::Off);
    }
}

// ── Files and pages from elsewhere ──────────────────────────────────────

/// Plain text out of an HTML page: tags dropped, block elements become
/// line breaks, entities decoded. Enough for the settings blocks recipe
/// sites publish as paragraphs with `<br>` or as lists.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0;
    let bytes = html.as_bytes();
    let lower = html.to_ascii_lowercase();
    let mut skip_until: Option<&str> = None;
    while i < bytes.len() {
        if let Some(end) = skip_until {
            match lower[i..].find(end) {
                Some(k) => {
                    i += k + end.len();
                    skip_until = None;
                    continue;
                }
                None => break,
            }
        }
        if bytes[i] == b'<' {
            let close = match html[i..].find('>') {
                Some(k) => i + k,
                None => break,
            };
            let tag = lower[i + 1..close].trim_start_matches('/');
            let name: String = tag
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            match name.as_str() {
                "script" if !tag.starts_with('/') => skip_until = Some("</script>"),
                "style" if !tag.starts_with('/') => skip_until = Some("</style>"),
                "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                | "ul" | "ol" | "table" | "section" | "article" | "blockquote" | "pre" => {
                    out.push('\n')
                }
                "td" | "th" => out.push(' '),
                _ => {}
            }
            i = close + 1;
            continue;
        }
        if bytes[i] == b'&'
            && let Some(k) = html[i..].find(';').filter(|k| *k < 10)
        {
            {
                let ent = &html[i + 1..i + k];
                let rep = match ent {
                    "amp" => Some("&"),
                    "lt" => Some("<"),
                    "gt" => Some(">"),
                    "quot" => Some("\""),
                    "apos" | "#39" | "#8217" | "#8216" => Some("'"),
                    "nbsp" | "#160" => Some(" "),
                    "#8211" | "ndash" => Some("–"),
                    "#8212" | "mdash" => Some("—"),
                    "#8722" | "minus" => Some("−"),
                    "#43" => Some("+"),
                    _ => None,
                };
                if let Some(r) = rep {
                    out.push_str(r);
                    i += k + 1;
                    continue;
                }
            }
        }
        let ch = html[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    // Collapse runs of blank lines and trailing spaces.
    let mut lines: Vec<String> = Vec::new();
    for l in out.lines() {
        let t = l.trim();
        if t.is_empty() {
            if lines.last().is_some_and(|p| !p.is_empty()) {
                lines.push(String::new());
            }
        } else {
            lines.push(t.to_string());
        }
    }
    lines.join("\n")
}

/// A recipe from a web page: the page's text is scanned for the settings
/// block and the `<title>` / first heading gives the name. Returns the
/// recipe and the lines it could not read.
pub fn from_html(html: &str, source_url: &str) -> (Recipe, Vec<String>) {
    let title = title_of(html);
    // The <title> and headings name the recipe; keep them out of the
    // settings scan, where "Film Simulation Recipe" would read as a line.
    let lower = html.to_ascii_lowercase();
    let body = match (lower.find("<body"), lower.rfind("</body>")) {
        (Some(a), Some(b)) if b > a => &html[a..b],
        _ => html,
    };
    let text = html_to_text(body);
    // Start at the first settings line so prose before it is not misread.
    let start = text
        .lines()
        .position(|l| {
            let low = l.to_ascii_lowercase();
            low.contains("film simulation")
                || low.starts_with("dynamic range")
                || low.starts_with("grain")
                || low.starts_with("white balance")
                || looks::find(l.trim().trim_matches(['*', ':', ' '])).is_some()
        })
        .unwrap_or(0);
    let block: Vec<&str> = text.lines().skip(start).take(40).collect();
    let (mut r, unknown) = parse_text(&block.join("\n"));
    // Blocks often open with the bare simulation name ("Classic Chrome"),
    // which the text scan takes for a title.
    if let Some(look) = looks::find(&r.name) {
        r.film_simulation = look.key.into();
    }
    // The heading names the recipe on every recipe site; the text scan
    // only has stray lines to go on.
    if let Some(t) = &title {
        r.name = t.clone();
    }
    r.source_url = source_url.to_string();
    if r.author.is_empty() {
        r.author = host_of(source_url);
    }
    (r, unknown)
}

fn title_of(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    for (open, close) in [("<h1", "</h1>"), ("<title>", "</title>")] {
        if let Some(a) = lower.find(open)
            && let Some(gt) = lower[a..].find('>')
            && let Some(b) = lower[a + gt..].find(close)
        {
            let inner = &html[a + gt + 1..a + gt + b];
            let t = html_to_text(inner);
            let t = t
                .split(['|', '–', '—'])
                .next()
                .unwrap_or("")
                .trim()
                .trim_end_matches(|c: char| c == '-' || c.is_whitespace());
            // Drop the usual prefixes and suffixes.
            let mut t = t.to_string();
            for junk in [
                "Fujifilm ",
                "My Fujifilm ",
                "Film Simulation Recipe",
                "Film Simulation",
                "Recipe",
            ] {
                t = t.replace(junk, "");
            }
            let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
            let t = t.trim().trim_matches(':').trim().to_string();
            if t.chars().count() >= 3 {
                return Some(t.chars().take(80).collect());
            }
        }
    }
    None
}

fn host_of(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .split('/')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Links on an index page that look like recipe pages on the same site.
pub fn recipe_links(html: &str, base_url: &str) -> Vec<String> {
    let host = host_of(base_url);
    let lower = html.to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(k) = lower[i..].find("href=") {
        let a = i + k + 5;
        let Some(q) = html[a..].chars().next() else {
            break;
        };
        let (start, end) = if q == '"' || q == '\'' {
            let s = a + 1;
            match html[s..].find(q) {
                Some(e) => (s, s + e),
                None => break,
            }
        } else {
            let e = html[a..]
                .find(['>', ' '])
                .map(|e| a + e)
                .unwrap_or(html.len());
            (a, e)
        };
        let href = &html[start..end];
        i = end.max(a + 1);
        let full = if href.starts_with("http") {
            href.to_string()
        } else if href.starts_with('/') {
            format!("https://{host}{href}")
        } else {
            continue;
        };
        if host_of(&full) != host {
            continue;
        }
        let low = full.to_ascii_lowercase();
        let looks_like_recipe = (low.contains("recipe") || low.contains("film-simulation"))
            && !low.ends_with("/recipes/")
            && !low.contains("-recipes/")
            && !low.contains("/tag/")
            && !low.contains("/category/")
            && !low.contains("#")
            && !low.contains("?");
        if looks_like_recipe && !out.contains(&full) {
            out.push(full);
        }
    }
    out
}

/// Fetch a page.
pub fn fetch(url: &str) -> Result<String, String> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .max_redirects(8)
            .build(),
    );
    let resp = agent
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (X11; Linux) Emulsion recipe import",
        )
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    resp.into_body()
        .read_to_string()
        .map_err(|e| format!("{url}: {e}"))
}

/// A recipe from a Lightroom / Camera Raw develop preset (`.xmp`).
/// Camera-Raw sliders map onto the recipe's tone, colour, grain, vignette
/// and split-toning fields; what has no counterpart is listed.
pub fn from_xmp(xml: &str) -> (Recipe, Vec<String>) {
    let mut r = Recipe::default();
    let mut unknown = Vec::new();
    let get = |key: &str| -> Option<f32> {
        let val = xmp_value(xml, key)?;
        val.trim().trim_start_matches('+').parse::<f32>().ok()
    };
    if let Some(name) = xmp_value(xml, "crs:Name")
        .or_else(|| xmp_value(xml, "rdf:li xml:lang=\"x-default\""))
        .filter(|n| !n.trim().is_empty())
    {
        r.name = name.trim().to_string();
    }
    r.film_simulation = "provia".into();
    if let Some(v) = get("crs:Exposure2012").or_else(|| get("crs:Exposure")) {
        r.exposure_compensation = format!("{v:+.2}");
    }
    if let Some(v) = get("crs:Highlights2012").or_else(|| get("crs:Highlights")) {
        r.highlight = (v / 25.0).clamp(-4.0, 4.0);
    }
    if let Some(v) = get("crs:Shadows2012").or_else(|| get("crs:Shadows")) {
        // Lifting shadows in Lightroom is a negative shadow tone on Fuji.
        r.shadow = (-v / 25.0).clamp(-4.0, 4.0);
    }
    if let Some(v) = get("crs:Blacks2012") {
        r.fade = (v.max(0.0) * 0.8).clamp(0.0, 100.0);
    }
    if let Some(v) = get("crs:Saturation") {
        r.color = (v / 25.0).clamp(-4.0, 4.0);
    }
    if let Some(v) = get("crs:Vibrance") {
        r.color_chrome_effect = if v >= 25.0 {
            Strength::Strong
        } else if v > 5.0 {
            Strength::Weak
        } else {
            Strength::Off
        };
    }
    if let Some(v) = get("crs:Clarity2012").or_else(|| get("crs:Clarity")) {
        r.clarity = (v / 25.0).clamp(-5.0, 5.0);
    }
    if let Some(v) = get("crs:Sharpness") {
        r.sharpness = ((v - 40.0) / 20.0).clamp(-4.0, 4.0);
    }
    if let Some(v) = get("crs:LuminanceSmoothing") {
        r.noise_reduction = (v / 25.0 - 2.0).clamp(-4.0, 4.0);
    }
    if let Some(t) = get("crs:Temperature") {
        if t > 1000.0 {
            r.white_balance.kelvin = Some(t as u32);
        } else {
            // Incremental temperature: warmth relative to as-shot.
            r.white_balance.red = (t / 20.0).round() as i32;
            r.white_balance.blue = (-t / 20.0).round() as i32;
        }
    }
    if let Some(t) = get("crs:Tint") {
        r.white_balance.red += (t / 30.0).round() as i32;
    }
    if let Some(v) = get("crs:GrainAmount") {
        r.grain.strength = if v >= 40.0 {
            Strength::Strong
        } else if v > 0.0 {
            Strength::Weak
        } else {
            Strength::Off
        };
        if get("crs:GrainSize").is_some_and(|s| s > 35.0) {
            r.grain.size = GrainSize::Large;
        }
    }
    if let Some(v) = get("crs:PostCropVignetteAmount") {
        r.vignette = (-v).clamp(-100.0, 100.0);
    }
    // Split toning: hue and saturation per end → RGB shift.
    let tone = |hue: Option<f32>, sat: Option<f32>| -> [f32; 3] {
        let (Some(h), Some(s)) = (hue, sat) else {
            return [0.0; 3];
        };
        if s <= 0.0 {
            return [0.0; 3];
        }
        let (r, g, b) = hue_rgb(h);
        let k = s * 0.6;
        [(r - 0.5) * k, (g - 0.5) * k, (b - 0.5) * k]
    };
    r.split_shadows = tone(
        get("crs:SplitToningShadowHue"),
        get("crs:SplitToningShadowSaturation"),
    );
    r.split_highlights = tone(
        get("crs:SplitToningHighlightHue"),
        get("crs:SplitToningHighlightSaturation"),
    );
    if xmp_value(xml, "crs:ConvertToGrayscale").is_some_and(|v| v.trim() == "True") {
        r.film_simulation = "acros".into();
    }
    for k in [
        "crs:Dehaze",
        "crs:Texture",
        "crs:ToneCurvePV2012",
        "crs:LookTable",
        "crs:Contrast2012",
    ] {
        if xmp_value(xml, k).is_some() {
            unknown.push(k.trim_start_matches("crs:").to_string());
        }
    }
    r.tags.push("lightroom".into());
    (r, unknown)
}

fn xmp_value(xml: &str, key: &str) -> Option<String> {
    // Attribute form: key="value"
    if let Some(a) = xml.find(&format!("{key}=\"")) {
        let s = a + key.len() + 2;
        let e = xml[s..].find('"')? + s;
        return Some(xml[s..e].to_string());
    }
    // Element form: <key>value</key>
    let open = format!("<{key}>");
    let a = xml.find(&open)? + open.len();
    let e = xml[a..].find("</")? + a;
    Some(xml[a..e].to_string())
}

/// RGB (0–1) at a hue in degrees, full saturation.
fn hue_rgb(h: f32) -> (f32, f32, f32) {
    let h = (h.rem_euclid(360.0)) / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    }
}

/// A recipe from a Fujifilm X RAW STUDIO / camera profile (`.FP1`, XML).
pub fn from_fp1(xml: &str) -> (Recipe, Vec<String>) {
    let mut r = Recipe::default();
    let mut unknown = Vec::new();
    let tag = |name: &str| -> Option<String> {
        let open = format!("<{name}>");
        let a = xml.find(&open)? + open.len();
        let e = xml[a..].find("</")? + a;
        Some(xml[a..e].trim().to_string())
    };
    let num = |name: &str| -> Option<f32> {
        tag(name)
            .and_then(|v| v.trim_start_matches('+').parse::<f32>().ok())
            .map(|v| if v.abs() > 4.5 { v / 2.0 } else { v })
    };
    if let Some(a) = xml.find("label=\"") {
        let s = a + 7;
        if let Some(e) = xml[s..].find('"') {
            let label = xml[s..s + e].trim();
            if !label.is_empty() {
                r.name = label.to_string();
            }
        }
    }
    if let Some(sim) = tag("FilmSimulation") {
        let key = match sim.to_ascii_lowercase().replace(['_', ' '], "").as_str() {
            "provia" => "provia",
            "velvia" => "velvia",
            "astia" => "astia",
            "classicchrome" => "classic-chrome",
            "classicneg" | "classicnegative" => "classic-negative",
            "proneg" | "pronegstd" | "pronegstandard" => "pro-neg-std",
            "proneghi" => "pro-neg-hi",
            "eterna" => "eterna",
            "eternableachbypass" | "bleachbypass" => "eterna-bleach-bypass",
            "nostalgicneg" | "nostalgicnegative" => "nostalgic-negative",
            "realaace" => "reala-ace",
            "acros" => "acros",
            "acrosye" => "acros-ye",
            "acrosr" => "acros-r",
            "acrosg" => "acros-g",
            "monochrome" | "mono" => "monochrome",
            "monochromeye" => "monochrome-ye",
            "monochromer" => "monochrome-r",
            "monochromeg" => "monochrome-g",
            "sepia" => "sepia",
            other => {
                unknown.push(format!("FilmSimulation {other}"));
                "provia"
            }
        };
        r.film_simulation = key.into();
    }
    if let Some(v) = tag("GrainEffect") {
        r.grain.strength = match v.to_ascii_lowercase().as_str() {
            "strong" => Strength::Strong,
            "weak" => Strength::Weak,
            _ => Strength::Off,
        };
    }
    if let Some(v) = tag("GrainEffectSize") {
        r.grain.size = if v.eq_ignore_ascii_case("large") {
            GrainSize::Large
        } else {
            GrainSize::Small
        };
    }
    let strength = |v: String| match v.to_ascii_lowercase().as_str() {
        "strong" => Strength::Strong,
        "weak" => Strength::Weak,
        _ => Strength::Off,
    };
    if let Some(v) = tag("ChromeEffect").or_else(|| tag("ColorChromeEffect")) {
        r.color_chrome_effect = strength(v);
    }
    if let Some(v) = tag("ColorChromeBlue").or_else(|| tag("ColorChromeEffectBlue")) {
        r.color_chrome_fx_blue = strength(v);
    }
    if let Some(v) = tag("WhiteBalance") {
        r.white_balance.preset = v.to_ascii_lowercase().replace("temperature", "kelvin");
    }
    if let Some(k) = tag("WBColorTemp") {
        r.white_balance.kelvin = k.trim_end_matches('K').parse().ok();
    }
    if let Some(v) = num("WBShiftR") {
        r.white_balance.red = v.round() as i32;
    }
    if let Some(v) = num("WBShiftB") {
        r.white_balance.blue = v.round() as i32;
    }
    if let Some(v) = tag("DynamicRange") {
        r.dynamic_range = match v.trim_start_matches("DR").trim() {
            "200" => DynamicRange::Dr200,
            "400" => DynamicRange::Dr400,
            _ => DynamicRange::Dr100,
        };
    }
    if let Some(v) = num("HighlightTone") {
        r.highlight = v;
    }
    if let Some(v) = num("ShadowTone") {
        r.shadow = v;
    }
    if let Some(v) = num("Color") {
        r.color = v;
    }
    if let Some(v) = num("Sharpness") {
        r.sharpness = v;
    }
    if let Some(v) = num("NoisReduction").or_else(|| num("NoiseReduction")) {
        r.noise_reduction = v;
    }
    if let Some(v) = num("Clarity") {
        r.clarity = v;
    }
    if let Some(v) = tag("ExposureBias") {
        r.exposure_compensation = v;
    }
    r.tags.push("fujifilm".into());
    (r, unknown)
}

/// Import any supported file by its extension: `.recipe.toml`/`.toml`,
/// `.xmp`, `.fp1`, or a text block.
pub fn from_file(path: &std::path::Path) -> Result<(Recipe, Vec<String>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let (mut r, unknown) = match ext.as_str() {
        "toml" => (
            Recipe::from_toml(&text).map_err(|e| e.to_string())?,
            Vec::new(),
        ),
        "xmp" => from_xmp(&text),
        "fp1" | "fp2" | "fp3" => from_fp1(&text),
        "html" | "htm" => from_html(&text, ""),
        _ => parse_text(&text),
    };
    if r.name == Recipe::default().name && !stem.is_empty() {
        r.name = stem.trim_end_matches(".recipe").to_string();
    }
    Ok((r, unknown))
}

#[cfg(test)]
mod import_ext_tests {
    use super::*;

    #[test]
    fn html_pages_become_recipes() {
        let html = r#"<html><head><title>My Fujifilm X100V Kodak Portra 400 Film Simulation Recipe – Fuji X Weekly</title>
        <script>var x = "<p>ignored</p>";</script></head><body><h1>Kodak Portra 400</h1>
        <p>Some prose about the film.</p>
        <p><strong>Classic Chrome</strong><br>Dynamic Range: DR-Auto<br>Highlight: -1<br>Shadow: -2<br>Color: +2<br>
        Noise Reduction: -4<br>Sharpening: -2<br>Clarity: +2<br>Grain Effect: Strong, Small<br>Color Chrome Effect: Strong<br>
        Color Chrome Effect Blue: Weak<br>White Balance: Daylight, +3 Red &amp; -5 Blue<br>ISO: Auto, up to ISO 6400<br>
        Exposure Compensation: +1/3 to +1 (typically)</p>
        <a href="/2020/06/10/fujifilm-x100v-film-simulation-kodak-portra-400/">Portra</a>
        <a href="https://fujixweekly.com/fujifilm-x-trans-iv-recipes/">index</a>
        <a href="https://other.site/recipe/x">elsewhere</a>
        <a href='/2020/05/27/my-fujifilm-x100v-kodachrome-64-film-simulation-recipe/'>Kodachrome</a>
        </body></html>"#;
        let text = html_to_text(html);
        assert!(text.contains("Highlight: -1") && !text.contains("ignored"));
        let (r, _) = from_html(html, "https://fujixweekly.com/2020/06/10/x/");
        assert_eq!(r.name, "Kodak Portra 400");
        assert_eq!(r.film_simulation, "classic-chrome");
        assert_eq!((r.highlight, r.shadow, r.color), (-1.0, -2.0, 2.0));
        assert_eq!((r.white_balance.red, r.white_balance.blue), (3, -5));
        assert_eq!(r.grain.strength, Strength::Strong);
        assert_eq!(r.author, "fujixweekly.com");
        r.validate().unwrap();
        let links = recipe_links(html, "https://fujixweekly.com/fujifilm-x-trans-iv-recipes/");
        assert_eq!(links.len(), 2, "{links:?}");
        assert!(links[0].contains("kodak-portra-400") && links[1].contains("kodachrome"));
    }

    #[test]
    fn lightroom_and_fujifilm_files_import() {
        let xmp = r#"<x:xmpmeta><rdf:RDF><rdf:Description crs:Name="Warm Fade" crs:Exposure2012="+0.30"
          crs:Highlights2012="-50" crs:Shadows2012="+25" crs:Blacks2012="+20" crs:Saturation="-25" crs:Vibrance="+30"
          crs:Clarity2012="+15" crs:Temperature="+18" crs:Tint="0" crs:GrainAmount="45" crs:GrainSize="40"
          crs:PostCropVignetteAmount="-30" crs:SplitToningShadowHue="220" crs:SplitToningShadowSaturation="20"
          crs:SplitToningHighlightHue="45" crs:SplitToningHighlightSaturation="15" crs:Dehaze="+10"/></rdf:RDF></x:xmpmeta>"#;
        let (r, unknown) = from_xmp(xmp);
        assert_eq!(r.name, "Warm Fade");
        assert_eq!(r.highlight, -2.0);
        assert_eq!(r.shadow, -1.0, "lifted shadows read as softer shadow tone");
        assert!(r.fade > 10.0 && r.color == -1.0);
        assert_eq!(r.grain.strength, Strength::Strong);
        assert_eq!(r.grain.size, GrainSize::Large);
        assert_eq!(r.vignette, 30.0);
        assert!(
            r.split_shadows[2] > 0.0 && r.split_highlights[0] > 0.0,
            "{:?}",
            r.split_shadows
        );
        assert!(unknown.contains(&"Dehaze".to_string()));
        r.validate().unwrap();

        let fp1 = r#"<?xml version="1.0"?><ConversionProfile application="XRFC" version="1.12.0.0">
          <PropertyGroup device="X-T4" version="1.0" label="Street Chrome"><FilmSimulation>ClassicChrome</FilmSimulation>
          <GrainEffect>Weak</GrainEffect><GrainEffectSize>Small</GrainEffectSize><ChromeEffect>Strong</ChromeEffect>
          <ColorChromeBlue>Weak</ColorChromeBlue><WhiteBalance>Daylight</WhiteBalance><WBShiftR>2</WBShiftR><WBShiftB>-4</WBShiftB>
          <DynamicRange>200</DynamicRange><HighlightTone>-1</HighlightTone><ShadowTone>2</ShadowTone><Color>-2</Color>
          <Sharpness>-1</Sharpness><NoisReduction>-4</NoisReduction><Clarity>0</Clarity><ExposureBias>+1/3</ExposureBias>
          </PropertyGroup></ConversionProfile>"#;
        let (r, unknown) = from_fp1(fp1);
        assert!(unknown.is_empty(), "{unknown:?}");
        assert_eq!(r.name, "Street Chrome");
        assert_eq!(r.film_simulation, "classic-chrome");
        assert_eq!(r.dynamic_range, DynamicRange::Dr200);
        assert_eq!(
            (r.highlight, r.shadow, r.color, r.sharpness),
            (-1.0, 2.0, -2.0, -1.0)
        );
        assert_eq!((r.white_balance.red, r.white_balance.blue), (2, -4));
        assert_eq!(r.color_chrome_effect, Strength::Strong);
        assert_eq!(r.exposure_compensation, "+1/3");
        r.validate().unwrap();
    }
}
