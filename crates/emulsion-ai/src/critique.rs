//! Fast critique of a drawing in progress.
//!
//! Instant, rule-based measurements of the composite — value grouping and
//! range, where the detail sits, weight and balance, edge character,
//! colour temperature, symmetry, empty space — turned into the sentences
//! a drawing teacher would say. When a Jev key is available, Jev ranks
//! which issue matters most for the picture; without one the rules rank.
//! Every paint the assistant makes gets the top lines appended to its
//! result, so it corrects course without another look.

use crate::jev::{Jev, JevError};
use emulsion_core::Document;
use emulsion_raster::color;
use emulsion_raster::composite::{flatten, level_size};
use serde_json::{Map, Value, json};

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub key: &'static str,
    /// What is wrong and what to do, in one sentence.
    pub text: String,
    /// 0–1, how much it hurts the picture.
    pub severity: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Critique {
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

/// Measure the composite and list what a teacher would point at.
pub fn analyze(doc: &Document) -> Critique {
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
        return Critique::default();
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
        ranked_by: "rules",
        ..Default::default()
    };
    let empty = 1.0 - painted.len() as f32 / (w * h) as f32;
    c.metrics.insert("empty_fraction".into(), json!(empty));
    if painted.len() < 16 {
        c.issues.push(Issue {
            key: "empty",
            text: "The canvas is still empty; block in the big shapes before anything else.".into(),
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
                "Values are compressed (range {:.0} %): push the darkest darks and lightest lights apart, or the piece reads as grey.",
                range * 100.0
            ),
        );
    }
    if p2 > 0.22 {
        issue(
            "no_darks",
            0.5,
            "There are no real darks; put a full dark accent in the shadow side of the focal form."
                .into(),
        );
    }
    if p98 < 0.72 && p50 < 0.6 {
        issue(
            "no_lights",
            0.45,
            "There are no clean lights; leave or add a light shape where the light hits first."
                .into(),
        );
    }
    if peaks > 6 {
        issue(
            "value_grouping",
            0.5,
            format!(
                "Values scatter into {peaks} groups; squint and merge them into three or four so the design reads from across the room."
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
            issue("dead_centre", 0.55, "The detail sits dead centre; shift the focal point toward a third, or let the negative space breathe on one side.".into());
        }
        if top5 < 0.32 {
            issue("no_focus", 0.5, "Detail is spread evenly, so nothing leads the eye; sharpen and darken one area and soften the rest.".into());
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
            issue("all_hard", 0.4, "Every edge is hard; soften the edges that turn away from the light or sit behind the focal form.".into());
        }
        if hard_frac < 0.08 && edge_frac > 0.05 {
            issue(
                "all_soft",
                0.35,
                "Every edge is soft; give the focal form a few crisp edges so it comes forward."
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
                "The weight leans {}; add a counterweight (a dark accent or a shape) on the other side.",
                if lr > 0.0 { "left" } else { "right" }
            ),
        );
    }
    if top > bottom * 2.2 && top + bottom > 0.0 {
        issue(
            "top_heavy",
            0.3,
            "The picture is top-heavy; ground it with darker values low in the frame.".into(),
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
            issue("one_temperature", 0.3, "Lights and shadows share one colour temperature; make the light warmer and the shadow cooler (or the reverse) for depth.".into());
        }
    }
    if sat > 0.55 {
        issue("oversaturated", 0.35, "Everything is at full saturation; reserve the purest colour for the focal point and grey the rest down.".into());
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
            issue("symmetry", 0.3, "The composition is almost perfectly symmetrical, which reads as static; break it with an asymmetric element.".into());
        }
    }
    if empty > 0.85 {
        issue(
            "mostly_empty",
            0.3,
            format!(
                "{:.0} % of the canvas is empty; decide whether that space is designed or just unfinished.",
                empty * 100.0
            ),
        );
    }

    c.issues.sort_by(|a, b| b.severity.total_cmp(&a.severity));
    c
}

/// Ask Jev which issue matters most for this picture and move it first.
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
            "question": "Given these measurements of a drawing in progress, which single problem should the artist fix first for the biggest improvement?",
            "options": options
        }),
    );
    let state = json!({ "metrics": c.metrics, "issues": c.issues.iter().map(|i| i.key).collect::<Vec<_>>() });
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
}
