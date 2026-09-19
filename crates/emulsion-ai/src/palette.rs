//! Typed requests → Commands, without a language model.
//!
//! A request is split into clauses ("hide the top two nodes", "rename the
//! third to Sky"). A [`Decide`] names each clause's operation and targets;
//! literals (new names, percentages, blend modes, adjustments) are parsed
//! here. If every clause resolves confidently the plan runs directly as one
//! history step; otherwise the request goes to the assistant.

use crate::decide::{ClauseDecision, Decide, Intent, NodeInfo};
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Node, NodeId, NodeKind};
use emulsion_raster::{Adjustment, BlendMode};

const CONFIDENCE: f32 = 0.5;

/// Verbs that start a new clause after "and" / "then" / ",".
const VERBS: &[&str] = &[
    "hide",
    "show",
    "unhide",
    "rename",
    "call",
    "name",
    "delete",
    "remove",
    "duplicate",
    "copy",
    "group",
    "ungroup",
    "set",
    "make",
    "change",
    "move",
    "bring",
    "send",
    "raise",
    "lower",
    "add",
    "increase",
    "decrease",
    "reduce",
    "brighten",
    "darken",
    "warm",
    "cool",
    "invert",
    "undo",
    "redo",
    "turn",
    "put",
];

#[derive(Debug, Default)]
pub struct Plan {
    pub steps: Vec<Command>,
    /// Human-readable description per step.
    pub summary: Vec<String>,
    /// Clauses that could not be resolved.
    pub unresolved: Vec<String>,
    pub decider: &'static str,
}

impl Plan {
    pub fn is_complete(&self) -> bool {
        self.unresolved.is_empty() && !self.steps.is_empty()
    }
}

/// Split a request into clauses at "and", "then", ",", ";" when a verb follows.
pub fn clauses(request: &str) -> Vec<String> {
    let tokens: Vec<&str> = request.split_whitespace().collect();
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let is_verb = |t: &str| {
        VERBS.contains(
            &t.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
                .as_str(),
        )
    };
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        let low = t.to_lowercase();
        let joiner = matches!(low.as_str(), "and" | "then" | "also");
        if joiner && !cur.is_empty() {
            // Skip "and then".
            let mut j = i + 1;
            while j < tokens.len() && matches!(tokens[j].to_lowercase().as_str(), "then" | "also") {
                j += 1;
            }
            if j < tokens.len() && is_verb(tokens[j]) {
                out.push(cur.join(" "));
                cur.clear();
                i = j;
                continue;
            }
        }
        let trimmed = t.trim_end_matches([',', ';']);
        if trimmed.len() != t.len()
            && i + 1 < tokens.len()
            && (is_verb(tokens[i + 1])
                || matches!(tokens[i + 1].to_lowercase().as_str(), "and" | "then"))
        {
            cur.push(trimmed);
            out.push(cur.join(" "));
            cur.clear();
            i += 1;
            continue;
        }
        cur.push(t);
        i += 1;
    }
    if !cur.is_empty() {
        out.push(cur.join(" "));
    }
    out.into_iter()
        .map(|c| {
            let mut c = c.trim().trim_end_matches(['.', '!']).to_string();
            // A clause that starts after a comma may still begin with a joiner.
            loop {
                let low = c.to_lowercase();
                let Some(rest) = ["and ", "then ", "also "]
                    .iter()
                    .find_map(|j| low.strip_prefix(j).map(|r| r.len()))
                else {
                    break;
                };
                c = c[c.len() - rest..].to_string();
            }
            c
        })
        .filter(|c| !c.is_empty())
        .collect()
}

/// Separate the target phrase from a literal: "rename the third to Sky" →
/// ("rename the third", Some("Sky")).
pub fn split_literal(clause: &str, intent: Intent) -> (String, Option<String>) {
    // Quoted text is always a literal.
    for (open, close) in [('"', '"'), ('“', '”'), ('\'', '\'')] {
        if let Some(a) = clause.find(open) {
            let rest = &clause[a + open.len_utf8()..];
            if let Some(b) = rest.find(close) {
                let lit = rest[..b].to_string();
                let target = format!("{}{}", &clause[..a], &rest[b + close.len_utf8()..]);
                return (target, Some(lit));
            }
        }
    }
    if intent == Intent::Rename {
        let low = clause.to_lowercase();
        for sep in [" to ", " as ", " into "] {
            if let Some(p) = low.rfind(sep) {
                return (
                    clause[..p].to_string(),
                    Some(clause[p + sep.len()..].trim().to_string()),
                );
            }
        }
    }
    if matches!(intent, Intent::Opacity | Intent::Blend) {
        let low = clause.to_lowercase();
        for sep in [" to ", " at "] {
            if let Some(p) = low.rfind(sep) {
                return (
                    clause[..p].to_string(),
                    Some(clause[p + sep.len()..].trim().to_string()),
                );
            }
        }
    }
    (clause.to_string(), None)
}

pub fn blend_in(text: &str) -> Option<BlendMode> {
    let low = text.to_lowercase();
    let mut modes: Vec<BlendMode> = BlendMode::MENU.iter().flatten().copied().collect();
    modes.push(BlendMode::PassThrough);
    // Longest label first: "linear light" before "light".
    modes.sort_by_key(|m| std::cmp::Reverse(m.label().len()));
    modes.into_iter().find(|m| low.contains(m.label()))
}

fn percent_in(text: &str) -> Option<f32> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter_map(|t| t.parse::<f32>().ok())
        .next()
        .filter(|v| (0.0..=100.0).contains(v))
}

/// Adjustment implied by phrasing, with a moderate default amount.
pub fn adjustment_in(text: &str) -> Option<(Adjustment, &'static str)> {
    let low = text.to_lowercase();
    let has = |k: &str| low.contains(k);
    let more = !(has("less") || has("reduce") || has("decrease") || has("lower"));
    let s = |v: f32| if more { v } else { -v };
    let mut a;
    let label;
    if has("warm") || has("warmer") {
        a = Adjustment::WhiteBalance {
            temperature: 25.0,
            tint: 0.0,
        };
        label = "Warm up";
    } else if has("cool") || has("colder") || has("cooler") {
        a = Adjustment::WhiteBalance {
            temperature: -25.0,
            tint: 0.0,
        };
        label = "Cool down";
    } else if has("invert") || has("negative") {
        a = Adjustment::Invert;
        label = "Invert";
    } else if has("saturat")
        || has("vivid")
        || has("vibran")
        || has("desaturat")
        || has("black and white")
    {
        let v = if has("desaturat") || has("black and white") {
            -100.0f32.max(if has("black and white") {
                -100.0
            } else {
                -40.0
            })
        } else {
            s(25.0)
        };
        a = Adjustment::HueSaturation {
            hue: 0.0,
            saturation: v,
            lightness: 0.0,
        };
        label = "Saturation";
    } else if has("contrast") {
        a = Adjustment::BrightnessContrast {
            brightness: 0.0,
            contrast: s(20.0),
        };
        label = "Contrast";
    } else if has("brighten") || has("brighter") || has("lighter") || (has("exposure") && more) {
        a = Adjustment::Exposure {
            exposure: 0.5,
            offset: 0.0,
            gamma: 1.0,
        };
        label = "Brighten";
    } else if has("darken") || has("darker") || (has("exposure") && !more) {
        a = Adjustment::Exposure {
            exposure: -0.5,
            offset: 0.0,
            gamma: 1.0,
        };
        label = "Darken";
    } else if has("levels") {
        a = Adjustment::Levels {
            in_black: 0.0,
            in_white: 255.0,
            gamma: 1.0,
            out_black: 0.0,
            out_white: 255.0,
        };
        label = "Levels";
    } else {
        return None;
    }
    // "by 1 stop", "+0.8 ev", "by 40"
    if let Some(v) = text
        .split_whitespace()
        .filter_map(|t| {
            t.trim_matches(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
                .parse::<f32>()
                .ok()
        })
        .next()
    {
        match &mut a {
            Adjustment::Exposure { exposure, .. } => {
                *exposure = if *exposure < 0.0 { -v.abs() } else { v.abs() }
            }
            Adjustment::WhiteBalance { temperature, .. } => {
                *temperature = temperature.signum() * v.abs()
            }
            Adjustment::HueSaturation { saturation, .. } => {
                *saturation = saturation.signum() * v.abs()
            }
            Adjustment::BrightnessContrast { contrast, .. } => {
                *contrast = contrast.signum() * v.abs()
            }
            _ => {}
        }
    }
    Some((a, label))
}

/// Every node, top of the stack first, as deciders see it.
pub fn node_infos(doc: &Document) -> Vec<NodeInfo> {
    let mut out = Vec::new();
    fn walk(doc: &Document, parent: Option<NodeId>, out: &mut Vec<NodeInfo>) {
        for id in doc.children(parent).into_iter().rev() {
            let n = doc.node(id).expect("child");
            out.push(NodeInfo {
                id,
                row: out.len() + 1,
                name: n.name.clone(),
                kind: match n.kind {
                    NodeKind::Raster { .. } => "pixels",
                    NodeKind::Group { .. } => "group",
                    NodeKind::Adjust(_) => "adjustment",
                    NodeKind::Fill { .. } => "fill",
                    NodeKind::Path { .. } => "path",
                    NodeKind::Text { .. } => "text",
                    NodeKind::Smart { .. } => "smart",
                },
            });
            walk(doc, Some(id), out);
        }
    }
    walk(doc, None, &mut out);
    out
}

/// Plan a request against `doc`.
pub fn plan(request: &str, doc: &Document, decider: &dyn Decide) -> anyhow::Result<Plan> {
    let clauses = clauses(request);
    let nodes = node_infos(doc);
    let decisions = decider.decide(&clauses, &nodes)?;
    let mut p = Plan {
        decider: decider.name(),
        ..Default::default()
    };
    let mut previous: Vec<NodeId> = Vec::new();
    let name_of = |id: NodeId| {
        doc.node(id)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| format!("#{id}"))
    };
    for (clause, d) in clauses.iter().zip(decisions) {
        let ClauseDecision {
            intent,
            confidence,
            targets,
        } = d;
        if intent == Intent::Other || confidence < CONFIDENCE {
            p.unresolved.push(clause.clone());
            continue;
        }
        let mut ids: Vec<NodeId> = targets
            .into_iter()
            .filter(|(_, pr)| *pr >= CONFIDENCE)
            .map(|(id, _)| id)
            .collect();
        // "it" / "them" refer to the previous clause's nodes.
        let low = clause.to_lowercase();
        if ids.is_empty()
            && [" it", " them", " that", " those", " this"]
                .iter()
                .any(|w| format!(" {low} ").contains(&format!("{w} ")))
        {
            ids = previous.clone();
        }
        if intent.needs_targets() && ids.is_empty() {
            p.unresolved.push(clause.clone());
            continue;
        }
        let (_, literal) = split_literal(clause, intent);
        let before = p.steps.len();
        match intent {
            Intent::Hide | Intent::Show => {
                for id in &ids {
                    p.steps.push(Command::SetVisible {
                        id: *id,
                        visible: intent == Intent::Show,
                    });
                    p.summary.push(format!(
                        "{} {}",
                        if intent == Intent::Show {
                            "Show"
                        } else {
                            "Hide"
                        },
                        name_of(*id)
                    ));
                }
            }
            Intent::Rename => match (ids.as_slice(), literal) {
                ([id], Some(name)) if !name.trim().is_empty() => {
                    p.steps.push(Command::Rename {
                        id: *id,
                        name: name.trim().to_string(),
                    });
                    p.summary
                        .push(format!("Rename {} to {}", name_of(*id), name.trim()));
                }
                _ => p.unresolved.push(clause.clone()),
            },
            Intent::Delete => {
                for id in &ids {
                    p.steps.push(Command::RemoveNode { id: *id });
                    p.summary.push(format!("Delete {}", name_of(*id)));
                }
            }
            Intent::Duplicate => {
                for id in &ids {
                    p.steps.push(Command::DuplicateNode { id: *id });
                    p.summary.push(format!("Duplicate {}", name_of(*id)));
                }
            }
            Intent::Group => {
                p.steps.push(Command::Group {
                    ids: ids.clone(),
                    name: literal.unwrap_or_else(|| "Group".into()),
                });
                p.summary.push(format!(
                    "Group {} node{}",
                    ids.len(),
                    if ids.len() == 1 { "" } else { "s" }
                ));
            }
            Intent::Ungroup => {
                for id in ids
                    .iter()
                    .filter(|id| doc.node(**id).is_some_and(|n| n.is_group()))
                {
                    p.steps.push(Command::Ungroup { id: *id });
                    p.summary.push(format!("Ungroup {}", name_of(*id)));
                }
            }
            Intent::Opacity => match literal
                .as_deref()
                .and_then(percent_in)
                .or_else(|| percent_in(clause))
            {
                Some(v) => {
                    for id in &ids {
                        p.steps.push(Command::SetOpacity {
                            id: *id,
                            opacity: v / 100.0,
                        });
                        p.summary
                            .push(format!("Set {} opacity to {v:.0}%", name_of(*id)));
                    }
                }
                None => p.unresolved.push(clause.clone()),
            },
            Intent::Blend => match literal
                .as_deref()
                .and_then(blend_in)
                .or_else(|| blend_in(clause))
            {
                Some(m) => {
                    for id in &ids {
                        p.steps.push(Command::SetBlend { id: *id, blend: m });
                        p.summary
                            .push(format!("Set {} to {}", name_of(*id), m.label()));
                    }
                }
                None => p.unresolved.push(clause.clone()),
            },
            Intent::MoveUp | Intent::MoveDown | Intent::MoveTop | Intent::MoveBottom => {
                for id in &ids {
                    let Some(n) = doc.node(*id) else { continue };
                    let sib = doc.children(n.parent);
                    let i = sib.iter().position(|s| s == id).unwrap_or(0);
                    let index = match intent {
                        Intent::MoveUp => i + 1,
                        Intent::MoveDown => i.saturating_sub(1),
                        Intent::MoveTop => usize::MAX,
                        _ => 0,
                    };
                    p.steps.push(Command::MoveNode {
                        id: *id,
                        slot: Slot {
                            parent: n.parent,
                            index,
                        },
                    });
                    p.summary.push(format!("Move {}", name_of(*id)));
                }
            }
            Intent::AddAdjustment => match adjustment_in(clause) {
                Some((a, label)) => {
                    let mut node = Node::adjust(0, a);
                    node.name = label.to_string();
                    p.steps.push(Command::AddNode {
                        node: Box::new(node),
                        slot: Slot::TOP,
                    });
                    p.summary.push(format!("Add {label}"));
                }
                None => p.unresolved.push(clause.clone()),
            },
            // Undo and redo act on history, not through Commands.
            Intent::Undo | Intent::Redo | Intent::Other => p.unresolved.push(clause.clone()),
        }
        if p.steps.len() > before {
            previous = ids;
        }
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decide::Keywords;
    use emulsion_core::Editor;
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    fn doc() -> Document {
        let mut d = Document::new(64, 64);
        for name in ["Grass", "Sun", "Clouds"] {
            Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    name,
                    Arc::new(Raster::transparent(64, 64)),
                    Placement::default(),
                )),
                slot: Slot::TOP,
            }
            .apply(&mut d)
            .unwrap();
        }
        d
    }

    #[test]
    fn clause_splitting() {
        assert_eq!(
            clauses("hide the top two nodes and rename the third to Sky"),
            vec!["hide the top two nodes", "rename the third to Sky"]
        );
        assert_eq!(
            clauses("hide sun and clouds"),
            vec!["hide sun and clouds"],
            "'and' inside targets does not split"
        );
        assert_eq!(
            clauses("duplicate card, then set it to multiply"),
            vec!["duplicate card", "set it to multiply"]
        );
    }

    #[test]
    fn deliverable_request_resolves_offline_as_one_step() {
        let d = doc();
        let p = plan(
            "hide the top two nodes and rename the third to Sky",
            &d,
            &Keywords,
        )
        .unwrap();
        assert!(p.is_complete(), "unresolved: {:?}", p.unresolved);
        assert_eq!(
            p.summary,
            vec!["Hide Clouds", "Hide Sun", "Rename Grass to Sky"]
        );
        let mut e = Editor::new(d, None);
        e.begin("Ask");
        for c in p.steps {
            e.execute(c).unwrap();
        }
        e.end();
        assert_eq!(e.history.len(), 1);
        let names: Vec<(&str, bool)> = e
            .doc
            .nodes
            .iter()
            .map(|n| (n.name.as_str(), n.visible))
            .collect();
        assert_eq!(
            names,
            vec![("Sky", true), ("Sun", false), ("Clouds", false)]
        );
    }

    #[test]
    fn literals_and_pronouns() {
        let d = doc();
        let p = plan(
            "set the sun opacity to 40% and set it to screen",
            &d,
            &Keywords,
        )
        .unwrap();
        assert!(p.is_complete(), "{:?}", p.unresolved);
        assert!(
            matches!(p.steps[0], Command::SetOpacity { opacity, .. } if (opacity - 0.4).abs() < 1e-6)
        );
        assert!(matches!(
            p.steps[1],
            Command::SetBlend {
                blend: BlendMode::Screen,
                ..
            }
        ));
        let p = plan("make it warmer", &d, &Keywords).unwrap();
        assert_eq!(p.summary, vec!["Add Warm up"]);
    }

    #[test]
    fn unclear_requests_go_to_the_assistant() {
        let d = doc();
        let p = plan("remove the person on the left", &d, &Keywords).unwrap();
        assert!(!p.is_complete());
        let p = plan("what's in this picture?", &d, &Keywords).unwrap();
        assert_eq!(p.unresolved.len(), 1);
    }
}
