//! Low-resolution, intent-aware observations about a drawing in progress.
//!
//! Measurements describe value, detail distribution, edges, colour and balance.
//! They do not establish artistic quality, anatomy, perspective or subject fidelity.
//! Suggested changes are conditional on the brief, medium, style and current stage.
//! Optional Jev ranking prioritizes observations without turning them into defects.

use crate::jev::{Jev, JevError};
use emulsion_core::Document;
use emulsion_raster::color;
use emulsion_raster::composite::{flatten, level_size};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// The artistic brief supplied by the caller; empty fields mean unknown intent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CritiqueContext {
    pub medium: String,
    /// Free-text visual intent, including custom or hybrid styles; not a medium preset.
    pub style: String,
    pub stage: String,
    pub composition_intent: String,
    pub user_constraints: Vec<String>,
}

/// Applies equally to rule-based observations and optional model ranking.
pub const REVIEW_POLICY: &str = "Measurements are observations, not artistic defects or a quality score. Respect the medium, requested style (including custom or hybrid styles), current stage, composition intent and every user constraint. Style is distinct from medium; do not impose naturalism on deliberate abstraction, stylization or flattened space. Unknown intent is not permission to impose a style. Centred composition, symmetry, limited values, hard edges and reserved paper may be intentional. Only suggest a correction if it serves the stated brief at this stage. These roughly 192-pixel measurements cannot establish anatomy, perspective or subject fidelity: inspect full-composition and document-space detail images against the brief to review those qualities.";

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub key: &'static str,
    /// A measured observation, with advice conditional on artistic intent.
    pub text: String,
    /// Review priority, not a measure of artistic quality.
    pub severity: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Critique {
    pub context: CritiqueContext,
    pub issues: Vec<Issue>,
    /// The measurements behind the issues, for Jev and for the curious.
    pub metrics: Map<String, Value>,
    pub ranked_by: &'static str,
}

impl Critique {
    /// The top `n` issues as lines.
    pub fn lines(&self, n: usize) -> Vec<String> {
        self.issues.iter().take(n).map(|i| i.text.clone()).collect()
    }
}

const SIZE: u32 = 192;

/// Analyze without a supplied brief. Unknown intent must remain unknown.
pub fn analyze(doc: &Document) -> Critique {
    analyze_with_context(doc, &CritiqueContext::default())
}

/// Measure the composite while retaining the artistic brief for review and ranking.
pub fn analyze_with_context(doc: &Document, context: &CritiqueContext) -> Critique {
    let tree = doc.composite_tree();
    let mut level = 0;
    while level_size(tree.width, tree.height, level)
        .0
        .max(level_size(tree.width, tree.height, level).1)
        > SIZE
        && level < 12
    {
        level += 1;
    }
    let small = flatten(&tree, level);
    let (w, h) = (small.width() as usize, small.height() as usize);
    let px: Vec<[f32; 4]> = small
        .read_rect(small.bounds())
        .into_iter()
        .map(color::px_to_f)
        .collect();
    if w < 4 || h < 4 {
        return Critique {
            context: context.clone(),
            ranked_by: "rules",
            ..Default::default()
        };
    }
    // Encoded luma per pixel, alpha-aware; transparent counts as empty.
    let mut luma = vec![0.0f32; w * h];
    let mut alpha = vec![0.0f32; w * h];
    let mut warm_dark = (0.0f32, 0usize);
    let mut warm_light = (0.0f32, 0usize);
    let mut sat_sum = 0.0f32;
    for (i, p) in px.iter().enumerate() {
        let a = p[3].clamp(0.0, 1.0);
        alpha[i] = a;
        if a <= 0.02 {
            continue;
        }
        let c = [p[0] / a, p[1] / a, p[2] / a].map(|v| color::linear_to_srgb(v.clamp(0.0, 1.0)));
        let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
        luma[i] = l;
        let mx = c[0].max(c[1]).max(c[2]);
        let mn = c[0].min(c[1]).min(c[2]);
        sat_sum += if mx > 1e-4 { (mx - mn) / mx } else { 0.0 };
        let warmth = c[0] - c[2];
        if l < 0.4 {
            warm_dark = (warm_dark.0 + warmth, warm_dark.1 + 1);
        } else if l > 0.6 {
            warm_light = (warm_light.0 + warmth, warm_light.1 + 1);
        }
    }
    let painted: Vec<usize> = (0..w * h).filter(|i| alpha[*i] > 0.02).collect();
    let mut c = Critique {
        context: context.clone(),
        ranked_by: "rules",
        ..Default::default()
    };
    let empty = 1.0 - painted.len() as f32 / (w * h) as f32;
    c.metrics.insert("empty_fraction".into(), json!(empty));
    if painted.len() < 16 {
        c.issues.push(Issue {
            key: "empty",
            text: "The canvas has very few opaque pixels; establish the first marks for the selected technique while preserving any intended blank paper.".into(),
            severity: 1.0,
        });
        return c;
    }
    let mut issue = |key: &'static str, severity: f32, text: String| {
        if severity > 0.15 {
            c.issues.push(Issue {
                key,
                text,
                severity,
            });
        }
    };

    // ── Values: range and grouping ──
    // A paper or background colour that covers most of the canvas would
    // swamp the statistics. When one value dominates and the marks on it
    // are few (a drawing on paper), the range that matters is the contrast
    // between paper and marks, and grouping is measured over the marks.
    let mut all: Vec<f32> = painted.iter().map(|i| luma[*i]).collect();
    let mut coarse = [0usize; 32];
    for l in &all {
        coarse[((l * 31.999) as usize).min(31)] += 1;
    }
    let (bg_bin, bg_n) = coarse
        .iter()
        .enumerate()
        .max_by_key(|(_, n)| **n)
        .map(|(i, n)| (i, *n))
        .unwrap_or((0, 0));
    let bg_luma = (bg_bin as f32 + 0.5) / 32.0;
    let mut marks: Vec<f32> = all
        .iter()
        .copied()
        .filter(|l| ((l * 31.999) as usize).min(31) != bg_bin)
        .collect();
    let marks_frac = marks.len() as f32 / all.len() as f32;
    c.metrics.insert(
        "background_fraction".into(),
        json!(bg_n as f32 / all.len() as f32),
    );
    let drawing = bg_n as f32 > all.len() as f32 * 0.55 && (0.015..0.4).contains(&marks_frac);
    all.sort_by(|a, b| a.total_cmp(b));
    marks.sort_by(|a, b| a.total_cmp(b));
    let pct_of = |v: &[f32], q: f32| v[((v.len() - 1) as f32 * q) as usize];
    let (p2, p50, p98) = (pct_of(&all, 0.02), pct_of(&all, 0.5), pct_of(&all, 0.98));
    let ls = if drawing { marks } else { all };
    let range = if drawing {
        (bg_luma - pct_of(&ls, 0.5)).abs()
    } else {
        p98 - p2
    };
    c.metrics.insert("drawing_on_paper".into(), json!(drawing));
    c.metrics.insert("value_range".into(), json!(range));
    c.metrics.insert("value_median".into(), json!(p50));
    let mut hist = [0u32; 16];
    for l in &ls {
        hist[((l * 15.999) as usize).min(15)] += 1;
    }
    let peaks = (0..16)
        .filter(|&i| {
            let v = hist[i];
            v > 0
                && v as f32 > ls.len() as f32 * 0.04
                && (i == 0 || hist[i - 1] <= v)
                && (i == 15 || hist[i + 1] <= v)
        })
        .count();
    c.metrics.insert("value_groups".into(), json!(peaks));
    if range < 0.45 {
        issue(
            "flat_values",
            0.6 + (0.45 - range),
            format!(
                "Measured value range is {:.0} %; widen it only if the brief and current stage call for stronger contrast. A limited range may be intentional.",
                range * 100.0
            ),
        );
    }
    if p2 > 0.22 {
        issue(
            "no_darks",
            0.5,
            "The lower value percentile is relatively light; add a dark accent only if needed for the intended value design at this stage."
                .into(),
        );
    }
    if p98 < 0.72 && p50 < 0.6 {
        issue(
            "no_lights",
            0.45,
            "The upper value percentile is relatively dark; reserve or add a light shape only if it serves the brief and medium."
                .into(),
        );
    }
    if peaks > 6 {
        issue(
            "value_grouping",
            0.5,
            format!(
                "The value histogram has {peaks} peaks; simplify groups only if the intended design needs a clearer value hierarchy."
            ),
        );
    }

    // ── Detail, focal point and balance ──
    let mut energy = vec![0.0f32; w * h];
    let (mut ex, mut ey, mut esum) = (0.0f64, 0.0f64, 0.0f64);
    let mut hard = 0usize;
    let mut edge_px = 0usize;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            if alpha[i] <= 0.02 {
                continue;
            }
            let gx = luma[i + 1] - luma[i - 1];
            let gy = luma[i + w] - luma[i - w];
            let g = (gx * gx + gy * gy).sqrt();
            energy[i] = g;
            ex += x as f64 * g as f64;
            ey += y as f64 * g as f64;
            esum += g as f64;
            if g > 0.06 {
                edge_px += 1;
                if g > 0.35 {
                    hard += 1;
                }
            }
        }
    }
    if esum > 1e-6 {
        let (fx, fy) = ((ex / esum) / w as f64, (ey / esum) / h as f64);
        c.metrics.insert("focal_x".into(), json!(fx));
        c.metrics.insert("focal_y".into(), json!(fy));
        let off_centre = ((fx - 0.5).abs().max((fy - 0.5).abs())) as f32;
        // How concentrated the detail is: share of energy in the busiest 20 % of cells.
        let mut cells = [0.0f32; 25];
        for y in 0..h {
            for x in 0..w {
                cells[(y * 5 / h) * 5 + x * 5 / w] += energy[y * w + x];
            }
        }
        cells.sort_by(|a, b| b.total_cmp(a));
        let top5: f32 = cells[..5].iter().sum::<f32>() / (esum as f32).max(1e-6);
        c.metrics.insert("detail_concentration".into(), json!(top5));
        if off_centre < 0.06 && top5 > 0.45 {
            issue("dead_centre", 0.55, "Detail energy is concentrated near the centre. This may support an intentionally centred composition; compare it with the stated composition intent before changing placement.".into());
        }
        if top5 < 0.32 {
            issue("no_focus", 0.5, "Detail energy is distributed broadly; emphasize one area only if the brief calls for a single focal hierarchy.".into());
        }
        let edge_frac = edge_px as f32 / painted.len().max(1) as f32;
        let hard_frac = if edge_px > 0 {
            hard as f32 / edge_px as f32
        } else {
            0.0
        };
        c.metrics
            .insert("hard_edge_fraction".into(), json!(hard_frac));
        if hard_frac > 0.7 && edge_frac > 0.08 {
            issue("all_hard", 0.4, "Most measured edges are hard; retain them for graphic line work, or soften selected edges if the medium and intended form require it.".into());
        }
        if hard_frac < 0.08 && edge_frac > 0.05 {
            issue(
                "all_soft",
                0.35,
                "Few measured edges are hard; add crisp accents only if the intended medium and stage call for them."
                    .into(),
            );
        }
    }
    // Weight: darkness-weighted mass left/right and top/bottom.
    let (mut left, mut right, mut top, mut bottom) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for &i in &painted {
        let wgt = 1.0 - luma[i];
        let (x, y) = (i % w, i / w);
        if x < w / 2 {
            left += wgt
        } else {
            right += wgt
        }
        if y < h / 2 { top += wgt } else { bottom += wgt }
    }
    let lr = (left - right) / (left + right).max(1e-6);
    c.metrics.insert("balance_lr".into(), json!(lr));
    if lr.abs() > 0.45 {
        issue(
            "unbalanced",
            0.4,
            format!(
                "Measured dark weight leans {}; add a counterweight only if this conflicts with the intended balance.",
                if lr > 0.0 { "left" } else { "right" }
            ),
        );
    }
    if top > bottom * 2.2 && top + bottom > 0.0 {
        issue(
            "top_heavy",
            0.3,
            "Measured dark weight is concentrated high in the frame; add lower weight only if the intended composition needs it.".into(),
        );
    }

    // ── Colour ──
    let sat = sat_sum / painted.len() as f32;
    c.metrics.insert("saturation".into(), json!(sat));
    if warm_dark.1 > 20 && warm_light.1 > 20 {
        let wd = warm_dark.0 / warm_dark.1 as f32;
        let wl = warm_light.0 / warm_light.1 as f32;
        c.metrics.insert("temperature_split".into(), json!(wl - wd));
        if (wl - wd).abs() < 0.02 && sat > 0.08 {
            issue("one_temperature", 0.3, "Measured lights and darks have similar colour temperatures; separate them only if the palette and lighting intent call for it.".into());
        }
    }
    if sat > 0.55 {
        issue("oversaturated", 0.35, "Average measured saturation is high; reduce it selectively only if the intended palette needs quieter areas.".into());
    }

    // ── Symmetry ──
    let mut diff = 0.0f32;
    let mut n = 0usize;
    for y in 0..h {
        for x in 0..w / 2 {
            let (a, b) = (y * w + x, y * w + (w - 1 - x));
            if alpha[a] > 0.02 || alpha[b] > 0.02 {
                diff += (luma[a] - luma[b]).abs();
                n += 1;
            }
        }
    }
    if n > 0 {
        let sym = 1.0 - (diff / n as f32) * 4.0;
        c.metrics.insert("symmetry".into(), json!(sym));
        if sym > 0.92 && painted.len() > w * h / 10 {
            issue("symmetry", 0.3, "Measured values are nearly mirror-symmetric. Symmetry may be intentional; preserve it when it supports the brief.".into());
        }
    }
    if empty > 0.85 {
        issue(
            "mostly_empty",
            0.3,
            format!(
                "{:.0} % of the canvas is transparent; this can be reserved paper or intentional negative space. Add marks only if the brief and current stage require them.",
                empty * 100.0
            ),
        );
    }

    // Sketches and first washes are deliberately incomplete. Keep measurements,
    // but lower the priority of observations about value range and finish.
    let preliminary = context.stage.to_lowercase().split_whitespace().any(|word| {
        matches!(
            word,
            "sketch" | "gesture" | "construction" | "thumbnail" | "underpainting" | "wash"
        )
    });
    if preliminary {
        for observation in &mut c.issues {
            if matches!(
                observation.key,
                "flat_values" | "no_darks" | "no_lights" | "all_soft" | "mostly_empty"
            ) {
                observation.severity *= 0.5;
            }
        }
    }
    c.issues.sort_by(|a, b| b.severity.total_cmp(&a.severity));
    c
}

/// Ask Jev which observation merits review against the brief and move it first.
pub fn rank_with_jev(jev: &Jev, c: &mut Critique) -> Result<(), JevError> {
    if c.issues.len() < 2 {
        return Ok(());
    }
    let options: Vec<Value> = c
        .issues
        .iter()
        .map(|i| json!({ "id": i.key, "description": i.text }))
        .collect();
    let mut q = Map::new();
    q.insert(
        "top_issue".into(),
        json!({
            "type": "Choice",
            "question": "Which observation most warrants image-based review against this brief at the current stage? Follow review_policy. Do not infer anatomy, perspective or subject fidelity from metrics; do not treat intentional composition or medium characteristics as defects.",
            "options": options
        }),
    );
    let state = json!({ "context": c.context, "review_policy": REVIEW_POLICY, "metrics": c.metrics, "observations": c.issues.iter().map(|i| i.key).collect::<Vec<_>>() });
    let answers = jev.evaluate(&state, q)?;
    let pick = answers
        .get("top_issue")
        .and_then(|v| v.get("answer").or(Some(v)))
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.get("id").and_then(Value::as_str).map(str::to_string))
        });
    if let Some(id) = pick
        && let Some(pos) = c.issues.iter().position(|i| i.key == id)
    {
        let top = c.issues.remove(pos);
        c.issues.insert(0, top);
        c.ranked_by = "Jev";
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::Node;
    use emulsion_core::command::{Command, Slot};
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    fn doc_with(r: Raster) -> Document {
        let mut d = Document::new(r.width(), r.height());
        Command::AddNode {
            node: Box::new(Node::raster(0, "a", Arc::new(r), Placement::default())),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        d
    }

    #[test]
    fn empty_flat_and_centred_pictures_get_the_right_notes() {
        let empty = analyze(&doc_with(Raster::transparent(200, 200)));
        assert_eq!(empty.issues[0].key, "empty");
        // A mid-grey field: flat values.
        let flat = analyze(&doc_with(Raster::solid(200, 200, [0.2, 0.2, 0.2, 1.0])));
        assert!(
            flat.issues.iter().any(|i| i.key == "flat_values"),
            "{:?}",
            flat.issues
        );
        // A busy square dead centre on white.
        let centred = Raster::from_fn(200, 200, [65535; 4], |x, y| {
            if (80..120).contains(&x) && (80..120).contains(&y) && (x + y) % 4 < 2 {
                [0, 0, 0, 65535]
            } else {
                [65535; 4]
            }
        });
        let c = analyze(&doc_with(centred));
        assert!(
            c.issues.iter().any(|i| i.key == "dead_centre"),
            "{:?}",
            c.issues
        );
        assert!(c.metrics.contains_key("focal_x"));
        // Off-centre, with darks and lights, is not flagged for either.
        let good = Raster::from_fn(200, 200, [60000, 58000, 52000, 65535], |x, y| {
            if (120..170).contains(&x) && (40..110).contains(&y) {
                [(x * 300) as u16 % 30000, 2000, 1000, 65535]
            } else {
                [60000, 58000, 52000, 65535]
            }
        });
        let g = analyze(&doc_with(good));
        assert!(
            !g.issues
                .iter()
                .any(|i| i.key == "dead_centre" || i.key == "flat_values"),
            "{:?}",
            g.issues
        );
    }

    #[test]
    fn intentional_centred_symmetry_is_observed_without_requiring_change() {
        let doc = doc_with(Raster::from_fn(192, 192, [65535; 4], |x, y| {
            if (76..116).contains(&x) && (76..116).contains(&y) {
                [0, 0, 0, 65535]
            } else {
                [65535; 4]
            }
        }));
        let context = CritiqueContext {
            medium: "manga".into(),
            style: "graphic emblem".into(),
            stage: "inking".into(),
            composition_intent: "centred, mirror-symmetric emblem".into(),
            user_constraints: vec!["Preserve symmetry and the hard black silhouette".into()],
        };
        let c = analyze_with_context(&doc, &context);
        assert_eq!(c.context, context);
        assert!(c.metrics["symmetry"].as_f64().unwrap() > 0.92);
        let centre = c.issues.iter().find(|i| i.key == "dead_centre").unwrap();
        assert!(centre.text.contains("intentionally centred"));
        assert!(!centre.text.contains("shift"));
        let symmetry = c.issues.iter().find(|i| i.key == "symmetry").unwrap();
        assert!(symmetry.text.contains("preserve"));
        assert!(!symmetry.text.contains("break"));
        // Measurements remain identical: intent changes their interpretation.
        assert_eq!(c.metrics, analyze(&doc).metrics);
    }

    #[test]
    fn unknown_intent_defaults_and_partial_context_are_backward_compatible() {
        let context: CritiqueContext =
            serde_json::from_value(json!({"medium": "watercolour"})).unwrap();
        assert!(context.style.is_empty());
        assert!(context.stage.is_empty());
        assert!(context.composition_intent.is_empty());
        assert!(context.user_constraints.is_empty());
        let doc = doc_with(Raster::solid(64, 64, [0.4, 0.4, 0.4, 1.0]));
        assert_eq!(
            analyze(&doc),
            analyze_with_context(&doc, &CritiqueContext::default())
        );
        assert!(
            serde_json::from_value::<CritiqueContext>(json!({"user_constraints": "bad type"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<CritiqueContext>(json!({"compositon_intent": "typo"}))
                .is_err()
        );
        let tiny = analyze_with_context(&Document::new(1, 1), &context);
        assert_eq!(tiny.context, context);
    }

    #[test]
    fn preliminary_wash_keeps_measurements_without_prioritizing_finished_contrast() {
        let doc = doc_with(Raster::solid(64, 64, [0.4, 0.4, 0.4, 1.0]));
        let context = CritiqueContext {
            medium: "watercolour".into(),
            stage: "first wash".into(),
            user_constraints: vec!["Keep the painting high-key".into()],
            ..Default::default()
        };
        let initial = analyze_with_context(&doc, &context);
        let unknown = analyze(&doc);
        assert_eq!(initial.metrics, unknown.metrics);
        for key in ["flat_values", "no_darks"] {
            let provisional = initial.issues.iter().find(|i| i.key == key).unwrap();
            let default = unknown.issues.iter().find(|i| i.key == key).unwrap();
            assert_eq!(provisional.severity, default.severity * 0.5);
            assert!(provisional.text.contains("only if"));
        }
    }
}
