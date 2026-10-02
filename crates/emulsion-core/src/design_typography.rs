//! Curated, editable heading/body combinations with honest local-font previews.
use crate::{Document, Node, fragment::Fragment, text::TextSpec};
use std::sync::{Mutex, OnceLock};

/// A resolved family and its styling. Sizes and spacing use a 640px-wide block;
/// insertion scales that block to fit the destination canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct TypographyStyle {
    pub font: String,
    pub bold: bool,
    pub italic: bool,
    pub size: f32,
    pub line_height: f32,
    pub letter_spacing: f32,
}

impl TypographyStyle {
    /// Shared style for a preview or editable text layer.
    pub fn text_spec(&self, text: &str) -> TextSpec {
        TextSpec {
            text: text.into(),
            font: self.font.clone(),
            size: self.size,
            bold: self.bold,
            italic: self.italic,
            line_height: self.line_height,
            letter_spacing: self.letter_spacing,
            color: [28, 30, 36, 255],
            ..Default::default()
        }
    }
}

/// Preview metadata uses the exact family selected for insertion on this machine.
/// Fonts are never downloaded, and missing optional families use bundled faces.
#[derive(Clone, Debug, PartialEq)]
pub struct TypographyPair {
    pub index: usize,
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub heading: TypographyStyle,
    pub body: TypographyStyle,
    pub heading_sample: &'static str,
    pub body_sample: &'static str,
}

impl TypographyPair {
    /// Every whitespace-separated word must match the name, purpose, resolved
    /// fonts or sample copy. The catalog order stays stable under filtering.
    pub fn matches_query(&self, query: &str) -> bool {
        let haystack = format!(
            "{} {} {} {} {} {}",
            self.name,
            self.description,
            self.heading.font,
            self.body.font,
            self.heading_sample,
            self.body_sample
        )
        .to_lowercase();
        query
            .split_whitespace()
            .all(|word| haystack.contains(&word.to_lowercase()))
    }
}

struct Style {
    families: &'static [&'static str],
    bold: bool,
    size: f32,
    spacing: f32,
}

impl Style {
    const fn new(families: &'static [&'static str], bold: bool, size: f32, spacing: f32) -> Self {
        Self {
            families,
            bold,
            size,
            spacing,
        }
    }

    fn resolve(&self, available: &[String], line_height: f32) -> TypographyStyle {
        let font = self
            .families
            .iter()
            .find_map(|candidate| {
                available
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(candidate))
                    .cloned()
            })
            // The final candidate is embedded in the application on every OS.
            .unwrap_or_else(|| self.families.last().unwrap().to_string());
        TypographyStyle {
            font,
            bold: self.bold,
            italic: false,
            size: self.size,
            line_height,
            letter_spacing: self.spacing,
        }
    }
}

const SANS: &[&str] = &["Geist"];
const MONO: &[&str] = &["Geist Mono"];
const EDITORIAL: &[&str] = &[
    "Georgia",
    "Charter",
    "Liberation Serif",
    "DejaVu Serif",
    "Geist",
];
const CLASSIC: &[&str] = &[
    "Baskerville",
    "Times New Roman",
    "Liberation Serif",
    "DejaVu Serif",
    "Geist",
];
const HUMANIST: &[&str] = &[
    "Avenir Next",
    "Trebuchet MS",
    "Candara",
    "Lato",
    "Carlito",
    "Geist",
];
const NEUTRAL: &[&str] = &[
    "Helvetica Neue",
    "Arial",
    "Liberation Sans",
    "DejaVu Sans",
    "Geist",
];

struct Pair {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    heading: Style,
    body: Style,
    heading_sample: &'static str,
    body_sample: &'static str,
}

const PAIRS: [Pair; 10] = [
    Pair {
        id: "modern-clarity",
        name: "Modern clarity",
        description: "A confident hierarchy for presentations and reports.",
        heading: Style::new(SANS, true, 60., -1.2),
        body: Style::new(SANS, false, 22., 0.),
        heading_sample: "Make room for ideas",
        body_sample: "A clear point of view. A story worth sharing.",
    },
    Pair {
        id: "technical-journal",
        name: "Technical journal",
        description: "Product updates, research and precise supporting details.",
        heading: Style::new(SANS, true, 56., -0.8),
        body: Style::new(MONO, false, 20., 0.),
        heading_sample: "Built with purpose",
        body_sample: "Ideas, experiments, and the details that matter.",
    },
    Pair {
        id: "studio-notes",
        name: "Studio notes",
        description: "A distinctive introduction for portfolios and projects.",
        heading: Style::new(MONO, true, 48., -0.8),
        body: Style::new(SANS, false, 22., 0.),
        heading_sample: "Work in progress",
        body_sample: "A closer look at the thinking behind the work.",
    },
    Pair {
        id: "editorial-feature",
        name: "Editorial feature",
        description: "Inviting headlines for magazines, essays and interviews.",
        heading: Style::new(EDITORIAL, true, 60., -0.5),
        body: Style::new(SANS, false, 21., 0.),
        heading_sample: "A fresh perspective",
        body_sample: "Stories and conversations that stay with you.",
    },
    Pair {
        id: "quiet-chapter",
        name: "Quiet chapter",
        description: "A considered pace for reading, reflections and quotations.",
        heading: Style::new(CLASSIC, false, 60., 0.),
        body: Style::new(EDITORIAL, false, 24., 0.),
        heading_sample: "The next chapter",
        body_sample: "Take a moment. There is more to the story.",
    },
    Pair {
        id: "classic-report",
        name: "Classic report",
        description: "A familiar hierarchy for proposals and professional reports.",
        heading: Style::new(CLASSIC, true, 56., 0.),
        body: Style::new(NEUTRAL, false, 22., 0.),
        heading_sample: "A lasting impression",
        body_sample: "Thoughtful ideas, clearly presented.",
    },
    Pair {
        id: "warm-welcome",
        name: "Warm welcome",
        description: "An approachable voice for invitations and community stories.",
        heading: Style::new(HUMANIST, true, 54., -0.5),
        body: Style::new(EDITORIAL, false, 23., 0.),
        heading_sample: "Better together",
        body_sample: "Good people. Shared ideas. Something worth making.",
    },
    Pair {
        id: "precision-brief",
        name: "Precision brief",
        description: "Ordered typography for specifications, process and data.",
        heading: Style::new(MONO, false, 44., 1.4),
        body: Style::new(NEUTRAL, false, 21., 0.),
        heading_sample: "THE BIG PICTURE",
        body_sample: "A simple framework for what comes next.",
    },
    Pair {
        id: "gallery-label",
        name: "Gallery label",
        description: "Restrained titles and compact captions for art and photography.",
        heading: Style::new(SANS, false, 62., -1.4),
        body: Style::new(MONO, false, 18., 0.2),
        heading_sample: "Space to explore",
        body_sample: "A collection of moments, carefully observed.",
    },
    Pair {
        id: "daily-dispatch",
        name: "Daily dispatch",
        description: "Direct headlines and readable copy for newsletters and news.",
        heading: Style::new(NEUTRAL, true, 58., -1.),
        body: Style::new(CLASSIC, false, 24., 0.),
        heading_sample: "Here is what matters",
        body_sample: "New ideas and useful stories, brought into focus.",
    },
];

fn resolve_catalog(available: &[String]) -> Vec<TypographyPair> {
    PAIRS
        .iter()
        .enumerate()
        .map(|(index, pair)| TypographyPair {
            index,
            id: pair.id,
            name: pair.name,
            description: pair.description,
            heading: pair.heading.resolve(available, 1.15),
            body: pair.body.resolve(available, 1.4),
            heading_sample: pair.heading_sample,
            body_sample: pair.body_sample,
        })
        .collect()
}

/// Ordered, searchable combinations. Enumeration happens once per font refresh,
/// not on every drawer render. Each index also addresses `typography_pair`.
pub fn typography_pairs() -> Vec<TypographyPair> {
    struct CatalogCache {
        generation: u64,
        pairs: Vec<TypographyPair>,
    }
    static CACHE: OnceLock<Mutex<Option<CatalogCache>>> = OnceLock::new();
    let generation = crate::text::font_generation();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(cached) = &*cache
        && cached.generation == generation
    {
        return cached.pairs.clone();
    }
    let pairs = resolve_catalog(&crate::text::font_families());
    *cache = Some(CatalogCache {
        generation,
        pairs: pairs.clone(),
    });
    pairs
}

/// Two independently editable text layers in one group. `Fragment::paste`
/// inserts the whole combination as a single undoable operation.
pub fn typography_pair(doc: &Document, variant: usize) -> Option<Fragment> {
    if doc.width == 0 || doc.height == 0 || variant >= PAIRS.len() {
        return None;
    }
    let pair = typography_pairs().into_iter().nth(variant)?;
    Some(fragment(doc, &pair))
}

fn fragment(doc: &Document, pair: &TypographyPair) -> Fragment {
    const WIDTH: f32 = 640.;
    const GAP: f32 = 24.;
    let mut heading = pair.heading.text_spec(pair.heading_sample);
    let mut body = pair.body.text_spec(pair.body_sample);
    heading.width = Some(WIDTH);
    body.width = Some(WIDTH);
    let heading_height = crate::text::layout(&heading).bounds().height;
    let body_height = crate::text::layout(&body).bounds().height;
    let block_height = heading_height + GAP + body_height;
    let scale = (doc.width as f32 * 0.8 / WIDTH).min(doc.height as f32 * 0.6 / block_height);
    // Bake ordinary scaling into editable font sizes. For tiny canvases, keep
    // the 1px minimum font size and use the existing local text transform.
    let baked_scale = scale.max(1. / heading.size.min(body.size));
    let local_scale = scale / baked_scale;
    let x = (doc.width as f32 - WIDTH * scale) / 2.;
    let y = (doc.height as f32 - block_height * scale) / 2.;
    let mut nodes = Vec::with_capacity(3);
    for (id, name, mut spec, offset) in [
        (1, "Heading", heading, 0.),
        (2, "Body", body, heading_height + GAP),
    ] {
        spec.size *= baked_scale;
        spec.letter_spacing *= baked_scale;
        spec.width = Some(WIDTH * baked_scale);
        spec.scale_x = local_scale;
        spec.scale_y = local_scale;
        spec.x = x;
        spec.y = y + offset * scale;
        let mut node = Node::text(id, name, spec, doc.width, doc.height);
        node.parent = Some(3);
        nodes.push(node);
    }
    nodes.push(Node::group(3, format!("{} font combination", pair.name)));
    Fragment {
        nodes,
        roots: vec![3],
        design: Default::default(),
        diagram: None,
        raw_originals: Vec::new(),
    }
}

#[cfg(test)]
#[path = "design_typography_tests.rs"]
mod tests;
