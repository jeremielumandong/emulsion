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
