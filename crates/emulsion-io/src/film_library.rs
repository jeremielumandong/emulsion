//! Built-in film and creative preset library: 451 Adobe Camera Raw presets from
//! peva3/Lightroom-Presets (MIT), embedded so they work offline and in the agent.
//! Regenerate the index with `scripts/sync-film-presets.py`.
use crate::{Result, lightroom_presets};
use emulsion_core::raw::DevelopParams;

pub struct FilmPreset {
    /// Upstream folder, e.g. `Color-Negative`, `Black-White`, `Genre`.
    pub category: &'static str,
    pub name: &'static str,
    /// Upstream one-line character note; empty when the docs have none.
    pub description: &'static str,
    pub xmp: &'static str,
}

#[path = "film_library_index.rs"]
mod index;

impl FilmPreset {
    /// Translate onto `base`; settings the preset omits keep `base` values.
    pub fn load(&self, base: DevelopParams) -> Result<lightroom_presets::ImportedPreset> {
        let mut report = lightroom_presets::from_xmp_text(self.xmp, base, self.name)?;
        report.name = self.name.into();
        Ok(report)
    }
}

pub fn all() -> &'static [FilmPreset] {
    index::PRESETS
}

/// Categories with their preset counts, in library order.
pub fn categories() -> Vec<(&'static str, usize)> {
    let mut out: Vec<(&'static str, usize)> = Vec::new();
    for p in all() {
        match out.iter_mut().find(|(c, _)| *c == p.category) {
            Some((_, n)) => *n += 1,
            None => out.push((p.category, 1)),
        }
    }
    out
}

fn normalized(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Find by exact name, ignoring case, spacing and punctuation
/// ("kodak portra 400", "Kodak-Portra-400").
pub fn find(name: &str) -> Option<&'static FilmPreset> {
    let wanted = normalized(name);
    if wanted.is_empty() {
        return None;
    }
    all().iter().find(|p| normalized(p.name) == wanted)
}

/// Rank presets by how many query words appear in their name, category or
/// description; names count most. Optionally limited to one category.
pub fn search(query: &str, category: Option<&str>, limit: usize) -> Vec<&'static FilmPreset> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1)
        .map(str::to_lowercase)
        .collect();
    let category = category.map(normalized);
    let mut scored: Vec<(usize, &'static FilmPreset)> = all()
        .iter()
        .filter(|p| {
            category
                .as_ref()
                .is_none_or(|c| normalized(p.category) == *c)
        })
        .map(|p| {
            let name = p.name.to_lowercase();
            let rest = format!("{} {}", p.category, p.description).to_lowercase();
            let score = words
                .iter()
                .map(|w| {
                    if name.split(|c: char| !c.is_alphanumeric()).any(|n| n == w) {
                        3
                    } else if name.contains(w.as_str()) {
                        2
                    } else if rest.contains(w.as_str()) {
                        1
                    } else {
                        0
                    }
                })
                .sum();
            (score, p)
        })
        .filter(|(score, _)| words.is_empty() || *score > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(b.1.name)));
    scored.into_iter().take(limit).map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_preset_imports_with_its_own_name_and_valid_settings() {
        assert_eq!(all().len(), 451);
        let mut unsupported = std::collections::BTreeMap::<String, usize>::new();
        for p in all() {
            let report = p
                .load(DevelopParams::default())
                .unwrap_or_else(|e| panic!("{}: {e}", p.name));
            assert_eq!(report.name, p.name);
            let prefixed = p.name.as_bytes()[..3.min(p.name.len())]
                .iter()
                .take(2)
                .all(u8::is_ascii_digit)
                && p.name.as_bytes().get(2) == Some(&b' ');
            assert!(!p.name.contains("&amp;") && !prefixed, "{}", p.name);
            report
                .params
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e}", p.name));
            for w in report.warnings.iter().skip(1) {
                *unsupported
                    .entry(w.split(':').next().unwrap_or(w).to_string())
                    .or_default() += 1;
            }
            let has = |key: &str| p.xmp.contains(&format!("crs:{key}=\""));
            let nonzero = |key: &str| {
                p.xmp
                    .split(&format!("crs:{key}=\""))
                    .nth(1)
                    .and_then(|v| v.split('"').next())
                    .and_then(|v| v.parse::<f32>().ok())
                    .is_some_and(|v| v != 0.)
            };
            if nonzero("ColorGradeShadowSat") {
                assert!(
                    report.params.grading[0][1] > 0.,
                    "{} lost shadow grading",
                    p.name
                );
            }
            if nonzero("ColorGradeHighlightSat") {
                assert!(
                    report.params.grading[2][1] > 0.,
                    "{} lost highlight grading",
                    p.name
                );
            }
            if nonzero("GrainAmount") {
                assert!(report.params.grain[0] > 0., "{} lost grain", p.name);
            }
            if p.xmp.contains(r#"crs:Treatment="Monochrome""#)
                || (has("ConvertToGrayscale") && p.xmp.contains(r#"ConvertToGrayscale="True""#))
            {
                assert_eq!(report.params.saturation, -1., "{} is monochrome", p.name);
            }
            if [
                "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
            ]
            .iter()
            .any(|c| nonzero(&format!("GrayMixer{c}")))
            {
                assert_ne!(
                    report.params.gray_mixer, [0.; 8],
                    "{} lost its B&W mix",
                    p.name
                );
            }
        }
        // Only settings Emulsion has no equivalent for may be reported.
        for group in unsupported.keys() {
            assert!(
                [
                    // Vignette shape details and lens-profile toggles.
                    "Other adjustments not applied",
                    "Adobe Vivid profile approximated with extra contrast and saturation",
                ]
                .contains(&group.as_str()),
                "unexpected unsupported group {group}: {unsupported:?}"
            );
        }
    }

    #[test]
    fn find_and_search_match_names_moods_and_categories() {
        assert_eq!(find("kodak portra 400").unwrap().name, "Kodak Portra 400");
        assert!(find("no such film").is_none());
        let portra = search("portra", None, 10);
        assert!(
            portra.iter().all(|p| p.name.contains("Portra")),
            "{:?}",
            portra.iter().map(|p| p.name).collect::<Vec<_>>()
        );
        assert!(!search("wedding", None, 5).is_empty());
        let bw = search("", Some("black-white"), 500);
        assert!(bw.len() > 40 && bw.iter().all(|p| p.category == "Black-White"));
        assert!(
            categories()
                .iter()
                .any(|(c, n)| *c == "Color-Negative" && *n > 50)
        );
    }
}
