//! Structured decisions about a typed request.

use emulsion_core::NodeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Intent {
    Hide,
    Show,
    Rename,
    Delete,
    Duplicate,
    Group,
    Ungroup,
    Opacity,
    Blend,
    MoveUp,
    MoveDown,
    MoveTop,
    MoveBottom,
    AddAdjustment,
    Undo,
    Redo,
    Other,
}

impl Intent {
    pub const ALL: [Intent; 17] = [
        Intent::Hide,
        Intent::Show,
        Intent::Rename,
        Intent::Delete,
        Intent::Duplicate,
        Intent::Group,
        Intent::Ungroup,
        Intent::Opacity,
        Intent::Blend,
        Intent::MoveUp,
        Intent::MoveDown,
        Intent::MoveTop,
        Intent::MoveBottom,
        Intent::AddAdjustment,
        Intent::Undo,
        Intent::Redo,
        Intent::Other,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Intent::Hide => "hide",
            Intent::Show => "show",
            Intent::Rename => "rename",
            Intent::Delete => "delete",
            Intent::Duplicate => "duplicate",
            Intent::Group => "group",
            Intent::Ungroup => "ungroup",
            Intent::Opacity => "opacity",
            Intent::Blend => "blend",
            Intent::MoveUp => "move_up",
            Intent::MoveDown => "move_down",
            Intent::MoveTop => "move_top",
            Intent::MoveBottom => "move_bottom",
            Intent::AddAdjustment => "add_adjustment",
            Intent::Undo => "undo",
            Intent::Redo => "redo",
            Intent::Other => "other",
        }
    }

    /// Rubric text for classifiers.
    pub fn describe(self) -> &'static str {
        match self {
            Intent::Hide => "Make nodes invisible (hide, turn off)",
            Intent::Show => "Make nodes visible (show, unhide, turn on)",
            Intent::Rename => "Give a node a new name",
            Intent::Delete => "Delete or remove nodes",
            Intent::Duplicate => "Duplicate or copy nodes",
            Intent::Group => "Put nodes into a new group",
            Intent::Ungroup => "Dissolve a group",
            Intent::Opacity => "Change how transparent or opaque nodes are",
            Intent::Blend => "Change a node's blend mode",
            Intent::MoveUp => "Move a node one step up the stack",
            Intent::MoveDown => "Move a node one step down the stack",
            Intent::MoveTop => "Move a node to the top of the stack",
            Intent::MoveBottom => "Move a node to the bottom of the stack",
            Intent::AddAdjustment => {
                "Change the look: brightness, exposure, contrast, colour, warmth, saturation, levels, invert"
            }
            Intent::Undo => "Undo the last change",
            Intent::Redo => "Redo the last undone change",
            Intent::Other => {
                "Anything else, or needs looking at the picture or several dependent steps"
            }
        }
    }

    pub fn from_key(k: &str) -> Intent {
        Intent::ALL
            .into_iter()
            .find(|i| i.key() == k)
            .unwrap_or(Intent::Other)
    }

    /// Whether the operation acts on specific nodes.
    pub fn needs_targets(self) -> bool {
        !matches!(
            self,
            Intent::AddAdjustment | Intent::Undo | Intent::Redo | Intent::Other
        )
    }
}

/// A node as the decider sees it.
#[derive(Clone, Debug)]
pub struct NodeInfo {
    pub id: NodeId,
    /// 1 = top of the stack.
    pub row: usize,
    pub name: String,
    pub kind: &'static str,
}

/// Decisions about one clause of a request.
#[derive(Clone, Debug, PartialEq)]
pub struct ClauseDecision {
    pub intent: Intent,
    pub confidence: f32,
    /// Nodes the clause refers to, with probability.
    pub targets: Vec<(NodeId, f32)>,
}

/// Something that can answer the palette's questions.
pub trait Decide: Send + Sync {
    fn name(&self) -> &'static str;
    /// Decide every clause at once (Jev answers them in one call).
    fn decide(&self, clauses: &[String], nodes: &[NodeInfo])
    -> anyhow::Result<Vec<ClauseDecision>>;
}

/// Offline keyword rules. Handles the common phrasings; anything else is
/// `Other` and goes to the assistant.
pub struct Keywords;

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '%' || c == '#'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// A blend mode named with a setting verb. Colour words that also describe
/// adjustments (hue, color, saturation, luminosity) need "mode" or "blend".
fn blend_named(clause: &str, w: &[String]) -> bool {
    let Some(m) = crate::palette::blend_in(clause) else {
        return false;
    };
    let has = |k: &str| w.iter().any(|x| x == k);
    let ambiguous = matches!(
        m,
        emulsion_raster::BlendMode::Hue
            | emulsion_raster::BlendMode::Color
            | emulsion_raster::BlendMode::Saturation
            | emulsion_raster::BlendMode::Luminosity
    );
    if has("mode") {
        return true;
    }
    !ambiguous && (has("set") || has("make") || has("change") || has("switch") || has("put"))
}

pub fn keyword_intent(clause: &str) -> (Intent, f32) {
    let w = words(clause);
    let has = |k: &str| w.iter().any(|x| x == k);
    let phrase = |p: &str| clause.to_lowercase().contains(p);
    let i = if has("undo") {
        Intent::Undo
    } else if has("redo") {
        Intent::Redo
    } else if has("ungroup") {
        Intent::Ungroup
    } else if has("rename") || phrase("call it") || phrase("name it") {
        Intent::Rename
    } else if has("hide") || has("invisible") || phrase("turn off") || phrase("switch off") {
        Intent::Hide
    } else if has("show")
        || has("unhide")
        || has("reveal")
        || phrase("turn on")
        || phrase("make visible")
    {
        Intent::Show
    } else if has("delete") || has("remove") || has("trash") {
        Intent::Delete
    } else if has("duplicate") || has("copy") || has("clone") {
        Intent::Duplicate
    } else if has("group") {
        Intent::Group
    } else if has("opacity") || has("transparent") || has("translucent") || has("opaque") {
        Intent::Opacity
    } else if has("blend") || blend_named(clause, &w) {
        Intent::Blend
    } else if phrase("to the top")
        || phrase("to top")
        || phrase("to the front")
        || phrase("bring to front")
    {
        Intent::MoveTop
    } else if phrase("to the bottom")
        || phrase("to bottom")
        || phrase("to the back")
        || phrase("send to back")
    {
        Intent::MoveBottom
    } else if (has("move") || has("raise") || has("bring"))
        && (has("up") || has("forward") || has("raise"))
    {
        Intent::MoveUp
    } else if (has("move") || has("lower") || has("send"))
        && (has("down") || has("backward") || has("lower"))
    {
        Intent::MoveDown
    } else if crate::palette::adjustment_in(clause).is_some() {
        Intent::AddAdjustment
    } else {
        Intent::Other
    };
    if i == Intent::Other {
        return (i, 0.0);
    }
    // Guard: rules only handle one plain operation per clause. Two
    // operations, conditions, or anything that needs looking at the picture
    // goes to the assistant instead of being guessed at.
    let groups = [
        has("hide") || has("invisible") || has("hidden"),
        has("show") || has("unhide") || has("visible") && !has("invisible"),
        has("rename") || has("called") || has("named") || phrase("call it") || phrase("name it"),
        has("delete") || has("remove"),
        has("duplicate") || has("copy"),
        has("opacity") || has("transparent"),
        has("blend") || has("mode"),
        has("move") || has("raise") || has("lower") || has("bring") || has("send"),
    ];
    let ops = groups.iter().filter(|g| **g).count();
    const VAGUE: &[&str] = &[
        "if",
        "unless",
        "except",
        "but",
        "only",
        "when",
        "whichever",
        "which",
        "whatever",
        "should",
        "look",
        "looks",
        "see",
        "left",
        "right",
        "center",
        "centre",
        "person",
        "people",
        "face",
        "background",
        "foreground",
        "object",
        "sky",
        "better",
        "nicer",
        "fix",
    ];
    let vague = w.iter().any(|x| VAGUE.contains(&x.as_str()));
    // "sky" is only vague outside a rename's new name.
    let vague = vague
        && !(i == Intent::Rename && !w.iter().any(|x| VAGUE.contains(&x.as_str()) && x != "sky"));
    if ops > 1 || vague || clause.contains(';') {
        return (i, 0.3);
    }
    (i, 0.9)
}

const ORDINALS: [(&str, usize); 10] = [
    ("first", 1),
    ("second", 2),
    ("third", 3),
    ("fourth", 4),
    ("fifth", 5),
    ("sixth", 6),
    ("seventh", 7),
    ("eighth", 8),
    ("ninth", 9),
    ("tenth", 10),
];

const COUNTS: [(&str, usize); 9] = [
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
];

fn count(w: &str) -> Option<usize> {
    w.parse()
        .ok()
        .or_else(|| COUNTS.iter().find(|(k, _)| *k == w).map(|(_, n)| *n))
}

/// Rows or names mentioned in the target part of a clause.
pub fn keyword_targets(target_text: &str, nodes: &[NodeInfo]) -> Vec<NodeId> {
    let w = words(target_text);
    let n = nodes.len();
    let by_row = |r: usize| nodes.iter().find(|x| x.row == r).map(|x| x.id);
    let mut out: Vec<NodeId> = Vec::new();
    for (i, x) in w.iter().enumerate() {
        let next = w.get(i + 1).map(String::as_str);
        match x.as_str() {
            "all" | "everything" | "every" => return nodes.iter().map(|n| n.id).collect(),
            "top" | "topmost" => match next.and_then(count) {
                Some(k) => out.extend((1..=k.min(n)).filter_map(by_row)),
                None if next
                    .map(|t| ORDINALS.iter().any(|(o, _)| *o == t))
                    .unwrap_or(false) => {}
                None => out.extend(by_row(1)),
            },
            // "second from the bottom" is handled by the ordinal.
            "bottom" if i >= 2 && w[i - 2] == "from" || i >= 1 && w[i - 1] == "from" => {}
            "bottom" | "bottommost" | "last" | "lowest" => match next.and_then(count) {
                Some(k) => out.extend(((n + 1 - k.min(n))..=n).filter_map(by_row)),
                None => out.extend(by_row(n)),
            },
            "row" | "#" => {
                if let Some(r) = next.and_then(|t| t.parse::<usize>().ok()) {
                    out.extend(by_row(r));
                }
            }
            o => {
                if let Some((_, r)) = ORDINALS.iter().find(|(k, _)| *k == o) {
                    // "second from the bottom"
                    let from_bottom = w[i + 1..].iter().take(3).any(|t| t == "bottom");
                    let row = if from_bottom { n + 1 - r.min(&n) } else { *r };
                    out.extend(by_row(row));
                } else if let Some(r) = o.strip_prefix('#').and_then(|d| d.parse::<u64>().ok())
                    && nodes.iter().any(|n| n.id == r)
                {
                    out.push(r);
                }
            }
        }
    }
    // Names: longest node name contained in the text.
    let low = target_text.to_lowercase();
    let mut named: Vec<&NodeInfo> = nodes
        .iter()
        .filter(|n| !n.name.is_empty() && low.contains(&n.name.to_lowercase()))
        .collect();
    named.sort_by_key(|n| std::cmp::Reverse(n.name.len()));
    if let Some(best) = named.first() {
        let len = best.name.len();
        out.extend(named.iter().filter(|n| n.name.len() == len).map(|n| n.id));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|id| seen.insert(*id));
    out
}

impl Decide for Keywords {
    fn name(&self) -> &'static str {
        "keywords"
    }

    fn decide(
        &self,
        clauses: &[String],
        nodes: &[NodeInfo],
    ) -> anyhow::Result<Vec<ClauseDecision>> {
        Ok(clauses
            .iter()
            .map(|c| {
                let (intent, confidence) = keyword_intent(c);
                let (target_text, _) = crate::palette::split_literal(c, intent);
                let targets = keyword_targets(&target_text, nodes)
                    .into_iter()
                    .map(|id| (id, 0.9))
                    .collect();
                ClauseDecision {
                    intent,
                    confidence,
                    targets,
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes() -> Vec<NodeInfo> {
        [
            "White balance",
            "Exposure",
            "Sun group",
            "Card",
            "Sun",
            "Base sky",
        ]
        .iter()
        .enumerate()
        .map(|(i, n)| NodeInfo {
            id: 100 + i as u64,
            row: i + 1,
            name: n.to_string(),
            kind: "pixels",
        })
        .collect()
    }

    #[test]
    fn intents() {
        assert_eq!(keyword_intent("hide the top two nodes").0, Intent::Hide);
        assert_eq!(keyword_intent("rename the third to Sky").0, Intent::Rename);
        assert_eq!(keyword_intent("make it warmer").0, Intent::AddAdjustment);
        assert_eq!(keyword_intent("set card to multiply mode").0, Intent::Blend);
        assert_eq!(
            keyword_intent("bring the sun to the front").0,
            Intent::MoveTop
        );
        assert_eq!(keyword_intent("what is in this picture?").0, Intent::Other);
    }

    #[test]
    fn compound_or_visual_clauses_are_not_trusted() {
        let low = |c: &str| keyword_intent(c).1 < 0.5;
        assert!(low(
            "the top node and the one under it should be invisible; the third should be called Sky"
        ));
        assert!(low("hide whichever node is yellow"));
        assert!(low("remove the person on the left"));
        assert!(low("hide the sun if it is too bright"));
        assert!(!low("hide the top two nodes"));
        assert!(
            !low("rename the third to Sky"),
            "the new name may be any word"
        );
    }

    #[test]
    fn targets_by_row_and_name() {
        let n = nodes();
        assert_eq!(keyword_targets("the top two nodes", &n), vec![100, 101]);
        assert_eq!(keyword_targets("the third", &n), vec![102]);
        assert_eq!(keyword_targets("the bottom layer", &n), vec![105]);
        assert_eq!(keyword_targets("second from the bottom", &n), vec![104]);
        assert_eq!(
            keyword_targets("the sun group", &n),
            vec![102],
            "longest name wins over 'Sun'"
        );
        assert_eq!(keyword_targets("card", &n), vec![103]);
    }
}
