//! Page resize constraints and deterministic, source-preserving motion previews.
use crate::{Command, Document, Editor, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    Start,
    Center,
    End,
    Stretch,
    #[default]
    Scale,
}
impl Anchor {
    pub const ALL: [Self; 5] = [
        Self::Start,
        Self::Center,
        Self::End,
        Self::Stretch,
        Self::Scale,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Center => "Center",
            Self::End => "End",
            Self::Stretch => "Stretch",
            Self::Scale => "Scale",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraint {
    pub horizontal: Anchor,
    pub vertical: Anchor,
    #[serde(default)]
    pub reflow_text: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    #[default]
    None,
    Fade,
    Slide,
    Zoom,
}
impl Effect {
    pub const ALL: [Self; 4] = [Self::None, Self::Fade, Self::Slide, Self::Zoom];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Fade => "Fade",
            Self::Slide => "Slide",
            Self::Zoom => "Zoom",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Motion {
    pub enter: Effect,
    pub exit: Effect,
    pub start_ms: u32,
    pub end_ms: u32,
    pub transition_ms: u32,
    pub offset: (f64, f64),
}
impl Default for Motion {
    fn default() -> Self {
        Self {
            enter: Effect::Fade,
            exit: Effect::None,
            start_ms: 0,
            end_ms: 3000,
            transition_ms: 500,
            offset: (0., 80.),
        }
    }
}
pub type PageTransition = Effect;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Design {
    pub speaker_notes: String,
    pub page_transition: PageTransition,
    pub transition_ms: u32,
    pub saved_styles: BTreeMap<String, crate::design_styles::SavedStyle>,
    pub style_links: BTreeMap<NodeId, String>,
    pub components: BTreeMap<String, crate::design_components::Definition>,
    pub component_links: BTreeMap<NodeId, crate::design_components::Instance>,
    pub media: BTreeMap<NodeId, crate::design::media::YouTube>,
    pub charts: BTreeMap<NodeId, crate::design_charts::Chart>,
    pub frames: BTreeMap<NodeId, crate::design_layout::Frame>,
    pub constraints: BTreeMap<NodeId, Constraint>,
    pub duration_ms: u32,
    pub fps: u32,
    pub motion: BTreeMap<NodeId, Motion>,
}
impl Default for Design {
    fn default() -> Self {
        Self {
            speaker_notes: String::new(),
            page_transition: PageTransition::None,
            transition_ms: 400,
            saved_styles: BTreeMap::new(),
            style_links: BTreeMap::new(),
            components: BTreeMap::new(),
            component_links: BTreeMap::new(),
            media: BTreeMap::new(),
            charts: BTreeMap::new(),
            frames: BTreeMap::new(),
            constraints: BTreeMap::new(),
            duration_ms: 3000,
            fps: 24,
            motion: BTreeMap::new(),
        }
    }
}
impl Design {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
    pub fn validate(&self, doc: &Document) -> Result<(), String> {
        if self.speaker_notes.chars().count() > 20_000
            || !(100..=3000).contains(&self.transition_ms)
        {
            return Err("Speaker notes or page transition duration exceeds its limit.".into());
        }
        crate::design_styles::validate(self, doc)?;
        crate::design_components::validate(self, doc)?;
        crate::design::media::validate(&self.media, doc)?;
        if self.charts.len() > 256 {
            return Err("A page supports up to 256 charts and tables.".into());
        }
        for (id, chart) in &self.charts {
            if !doc.node(*id).is_some_and(|n| n.is_group()) {
                return Err("Chart data needs its own group.".into());
            }
            chart.validate()?;
        }
        crate::design_layout::validate(&self.frames, doc)?;
        if !(100..=60_000).contains(&self.duration_ms) || !(1..=60).contains(&self.fps) {
            return Err("Choose a duration from 0.1–60 seconds and 1–60 fps.".into());
        }
        if self.constraints.len() > crate::document::MAX_NODES
            || self.motion.len() > crate::document::MAX_NODES
        {
            return Err("Too many design object settings.".into());
        }
        for id in self.constraints.keys().chain(self.motion.keys()) {
            if doc.node(*id).is_none() {
                return Err("Design settings refer to a missing object.".into());
            }
        }
        for m in self.motion.values() {
            if m.start_ms >= m.end_ms
                || m.end_ms > self.duration_ms
                || m.transition_ms == 0
                || m.transition_ms > (m.end_ms - m.start_ms) / 2
                || !m.offset.0.is_finite()
                || !m.offset.1.is_finite()
                || m.offset.0.abs() > 1e6
                || m.offset.1.abs() > 1e6
            {
                return Err("Motion timing or offset is outside its limits.".into());
            }
        }
        Ok(())
    }
    pub fn retain_nodes(&mut self, ids: &HashSet<NodeId>) {
        self.style_links.retain(|id, _| ids.contains(id));
        self.components.retain(|_, definition| {
            definition.variants.retain(|_, id| ids.contains(id));
            !definition.variants.is_empty()
        });
        self.component_links.retain(|id, link| {
            ids.contains(id)
                && self
                    .components
                    .get(&link.component)
                    .is_some_and(|d| d.variants.contains_key(&link.variant))
        });
        for link in self.component_links.values_mut() {
            link.members
                .retain(|source, instance| ids.contains(source) && ids.contains(instance));
            link.overrides
                .retain(|source, _| link.members.contains_key(source));
        }
        self.media
            .retain(|id, video| ids.contains(id) && ids.contains(&video.boundary));
        self.charts.retain(|id, _| ids.contains(id));
        self.frames
            .retain(|id, frame| ids.contains(id) && ids.contains(&frame.boundary));
        for frame in self.frames.values_mut() {
            frame.children.retain(|id, _| ids.contains(id));
        }
        self.constraints.retain(|id, _| ids.contains(id));
        self.motion.retain(|id, _| ids.contains(id));
    }
    pub fn fragment(&self, ids: &HashSet<NodeId>) -> Self {
        let mut out = self.clone();
        out.retain_nodes(ids);
        out.saved_styles
            .retain(|name, _| out.style_links.values().any(|link| link == name));
        out
    }
    pub fn remap(&self, map: &HashMap<NodeId, NodeId>) -> Self {
        let id = |id| map.get(&id).copied().unwrap_or(id);
        Self {
            style_links: self
                .style_links
                .iter()
                .map(|(key, value)| (id(*key), value.clone()))
                .collect(),
            components: self
                .components
                .iter()
                .map(|(key, value)| {
                    let mut value = value.clone();
                    value
                        .variants
                        .values_mut()
                        .for_each(|root| *root = id(*root));
                    (key.clone(), value)
                })
                .collect(),
            component_links: self
                .component_links
                .iter()
                .map(|(key, value)| (id(*key), value.remap(map)))
                .collect(),
            media: self
                .media
                .iter()
                .map(|(key, value)| {
                    let mut value = value.clone();
                    value.boundary = id(value.boundary);
                    (id(*key), value)
                })
                .collect(),
            charts: self
                .charts
                .iter()
                .map(|(key, value)| (id(*key), value.clone()))
                .collect(),
            frames: self
                .frames
                .iter()
                .map(|(key, frame)| {
                    let mut frame = frame.clone();
                    frame.boundary = id(frame.boundary);
                    frame.children = frame
                        .children
                        .iter()
                        .map(|(key, value)| (id(*key), *value))
                        .collect();
                    (id(*key), frame)
                })
                .collect(),
            constraints: self.constraints.iter().map(|(k, v)| (id(*k), *v)).collect(),
            motion: self
                .motion
                .iter()
                .map(|(k, v)| (id(*k), v.clone()))
                .collect(),
            ..self.clone()
        }
    }
}
fn axis(anchor: Anchor, position: f64, length: f64, old: f64, new: f64) -> (f64, f64) {
    match anchor {
        Anchor::Start => (position, length),
        Anchor::Center => (position + (new - old) / 2., length),
        Anchor::End => (position + new - old, length),
        Anchor::Stretch => (position, (length + new - old).max(1.)),
        Anchor::Scale => (position * new / old, (length * new / old).max(1.)),
    }
}
pub struct Resized {
    pub doc: Document,
    pub overflow: Vec<NodeId>,
}
/// A separate variant; the source document and raster buffers remain intact.
pub fn resize_variant(source: &Document, width: u32, height: u32) -> Result<Resized, String> {
    Document::new(width, height)
        .validate()
        .map_err(|e| e.to_string())?;
    source.validate().map_err(|e| e.to_string())?;
    let mut editor = Editor::new(source.clone(), None);
    editor
        .execute(Command::Crop {
            rect: emulsion_raster::IRect::new(0, 0, width as i32, height as i32),
            rotation: 0.,
        })
        .map_err(|e| e.to_string())?;
    let links: Vec<_> = editor
        .doc
        .nodes
        .iter()
        .map(|n| (n.id, n.link_group))
        .collect();
    for node in &mut editor.doc.nodes {
        node.link_group = None;
    }
    let mut ids = source.children(None);
    ids.extend(
        source
            .design
            .constraints
            .keys()
            .copied()
            .filter(|id| source.node(*id).is_some_and(|n| n.parent.is_some())),
    );
    ids.sort_by_key(|id| source.depth(*id));
    for id in ids {
        let node = source.node(id).unwrap();
        if matches!(node.kind, NodeKind::Fill { .. } | NodeKind::Adjust(_)) {
            continue;
        }
        if source
            .diagram
            .as_ref()
            .is_some_and(|d| d.edges.contains_key(&id))
        {
            continue;
        }
        let Some(bounds) = crate::geometry::node_bounds(source, id) else {
            continue;
        };
        if bounds.w == 0 || bounds.h == 0 {
            continue;
        }
        let rule = source
            .design
            .constraints
            .get(&id)
            .copied()
            .unwrap_or_default();
        let mut frame = (
            bounds.x as f64,
            bounds.y as f64,
            bounds.w as f64,
            bounds.h as f64,
        );
        if rule.reflow_text
            && let NodeKind::Text { spec, .. } = &node.kind
        {
            frame.0 = spec.x as f64;
            frame.1 = spec.y as f64;
            frame.2 = spec.width.map_or(frame.2, f64::from);
            frame.3 = spec.height.map_or(frame.3, f64::from);
        }
        let (x, w) = axis(
            rule.horizontal,
            frame.0,
            frame.2,
            source.width as f64,
            width as f64,
        );
        let (y, h) = axis(
            rule.vertical,
            frame.1,
            frame.3,
            source.height as f64,
            height as f64,
        );
        if rule.reflow_text
            && let NodeKind::Text { spec, .. } = &node.kind
        {
            let mut spec = (**spec).clone();
            spec.x = x as f32;
            spec.y = y as f32;
            spec.width = Some(w as f32);
            if spec.height.is_some() {
                spec.height = Some(h as f32);
            }
            editor
                .execute(Command::SetText {
                    id,
                    spec: Box::new(spec),
                })
                .map_err(|e| e.to_string())?;
        } else {
            let current = crate::geometry::node_bounds(&editor.doc, id).unwrap_or(bounds);
            let sx = w / current.w.max(1) as f64;
            let sy = h / current.h.max(1) as f64;
            editor
                .execute(Command::TransformNodes {
                    ids: vec![id],
                    transform: [
                        sx,
                        0.,
                        0.,
                        sy,
                        x - current.x as f64 * sx,
                        y - current.y as f64 * sy,
                    ],
                })
                .map_err(|e| e.to_string())?;
        }
    }
    for (id, link) in links {
        if let Some(node) = editor.doc.node_mut(id) {
            node.link_group = link;
        }
    }
    let overflow = editor
        .doc
        .children(None)
        .into_iter()
        .filter(|id| {
            crate::geometry::node_bounds(&editor.doc, *id).is_some_and(|b| {
                b.x < 0 || b.y < 0 || b.right() > width as i32 || b.bottom() > height as i32
            })
        })
        .collect();
    editor.doc.validate().map_err(|e| e.to_string())?;
    Ok(Resized {
        doc: editor.doc,
        overflow,
    })
}
/// Render-only evaluation. Never changes a saved object or its undo history.
pub fn at_time(source: &Document, time_ms: u32) -> Result<Document, String> {
    source.design.validate(source)?;
    let mut doc = source.clone();
    for (id, motion) in &source.design.motion {
        let Some(original) = source.node(*id) else {
            continue;
        };
        let inside = time_ms >= motion.start_ms && time_ms < motion.end_ms;
        if !inside {
            if let Some(node) = doc.node_mut(*id) {
                node.visible = false;
            }
            continue;
        }
        let enter =
            ((time_ms - motion.start_ms) as f64 / motion.transition_ms as f64).clamp(0., 1.);
        let exit = ((motion.end_ms - time_ms) as f64 / motion.transition_ms as f64).clamp(0., 1.);
        let mut alpha = original.opacity;
        let mut translation = (0., 0.);
        let mut scale = 1.;
        for (effect, progress, direction) in [(motion.enter, enter, 1.), (motion.exit, exit, -1.)] {
            let progress = progress * progress * (3. - 2. * progress);
            match effect {
                Effect::None => {}
                Effect::Fade => alpha *= progress as f32,
                Effect::Slide => {
                    translation.0 += motion.offset.0 * (1. - progress) * direction;
                    translation.1 += motion.offset.1 * (1. - progress) * direction;
                }
                Effect::Zoom => scale *= 0.8 + 0.2 * progress,
            }
        }
        if let Some(node) = doc.node_mut(*id) {
            node.opacity = alpha;
        }
        if translation != (0., 0.) || scale != 1. {
            let b = crate::geometry::node_bounds(source, *id).unwrap_or_default();
            let center = (b.x as f64 + b.w as f64 / 2., b.y as f64 + b.h as f64 / 2.);
            // Locks guard edits, not evaluation of already-authored motion.
            let locks: Vec<_> = doc
                .nodes
                .iter()
                .map(|n| (n.id, n.locked, n.locks, n.link_group))
                .collect();
            for node in &mut doc.nodes {
                node.locked = false;
                node.locks = Default::default();
                node.link_group = None;
            }
            crate::transform::transform_nodes(
                &mut doc,
                &[*id],
                [
                    scale,
                    0.,
                    0.,
                    scale,
                    center.0 * (1. - scale) + translation.0,
                    center.1 * (1. - scale) + translation.1,
                ],
            )
            .map_err(|e| e.to_string())?;
            for (id, locked, granular, link) in locks {
                if let Some(node) = doc.node_mut(id) {
                    node.locked = locked;
                    node.locks = granular;
                    node.link_group = link;
                }
            }
        }
    }
    crate::diagram::synchronize(&doc.clone(), &mut doc)?;
    doc.validate().map_err(|e| e.to_string())?;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    #[test]
    fn presentation_metadata_defaults_bounds_and_undo() {
        let defaults: super::Design = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults.transition_ms, 400);
        assert_eq!(defaults.page_transition, super::PageTransition::None);
        let mut editor = crate::Editor::new(crate::Document::new(200, 100), None);
        let mut design = defaults.clone();
        design.speaker_notes = "Presenter notes 日本語".into();
        design.page_transition = super::PageTransition::Slide;
        design.transition_ms = 1200;
        editor
            .execute(crate::Command::SetDesign {
                design: Box::new(design.clone()),
            })
            .unwrap();
        assert_eq!(editor.doc.design, design);
        editor.undo();
        assert_eq!(editor.doc.design, defaults);
        editor.redo();
        let before = editor.doc.clone();
        design.speaker_notes = "x".repeat(20_001);
        assert!(
            editor
                .execute(crate::Command::SetDesign {
                    design: Box::new(design.clone())
                })
                .is_err()
        );
        assert_eq!(editor.doc, before);
        design.speaker_notes.clear();
        design.transition_ms = 99;
        assert!(design.validate(&editor.doc).is_err());
        design.transition_ms = 3001;
        assert!(design.validate(&editor.doc).is_err());
    }

    use super::*;
    use crate::{Node, command::Slot, fragment::Fragment};
    use std::sync::Arc;

    fn fixture() -> (Document, NodeId, NodeId) {
        let mut doc = Document::new(400, 300);
        let image = Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Photo",
                Arc::new(emulsion_raster::Raster::solid(40, 30, [1.; 4])),
                emulsion_raster::Placement::at(30., 40.),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        let text = Command::AddNode {
            node: Box::new(crate::design::TextPreset::Heading.node(
                (400, 300),
                "Geist",
                [0, 0, 0, 255],
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        (doc, image, text)
    }

    #[test]
    fn resize_anchors_reflow_and_shared_pixels_preserve_the_source() {
        let (mut doc, image, text) = fixture();
        doc.design.constraints.insert(
            image,
            Constraint {
                horizontal: Anchor::End,
                vertical: Anchor::Start,
                reflow_text: false,
            },
        );
        doc.design.constraints.insert(
            text,
            Constraint {
                horizontal: Anchor::Stretch,
                vertical: Anchor::Start,
                reflow_text: true,
            },
        );
        let original = doc.clone();
        let resized = resize_variant(&doc, 600, 400).unwrap().doc;
        assert_eq!(doc, original);
        let (
            NodeKind::Raster {
                raster: a,
                placement: pa,
            },
            NodeKind::Raster {
                raster: b,
                placement: pb,
            },
        ) = (
            &doc.node(image).unwrap().kind,
            &resized.node(image).unwrap().kind,
        )
        else {
            panic!("pixels flattened")
        };
        assert!(Arc::ptr_eq(a, b));
        assert_eq!(pb.x, pa.x + 200.);
        assert_eq!(pb.y, pa.y);
        let (NodeKind::Text { spec: a, .. }, NodeKind::Text { spec: b, .. }) = (
            &doc.node(text).unwrap().kind,
            &resized.node(text).unwrap().kind,
        ) else {
            panic!("text flattened")
        };
        assert_eq!(a.size, b.size);
        assert_eq!(b.width, a.width.map(|w| w + 200.));
        assert_eq!(a.text, b.text);
        assert!(resize_variant(&doc, 0, 400).is_err());
    }

    #[test]
    fn motion_evaluation_is_deterministic_and_does_not_move_linked_objects() {
        let (mut doc, image, text) = fixture();
        for id in [image, text] {
            doc.node_mut(id).unwrap().link_group = Some(image);
        }
        doc.node_mut(image).unwrap().locked = true;
        doc.design.motion.insert(
            image,
            Motion {
                enter: Effect::Slide,
                offset: (100., 0.),
                ..Default::default()
            },
        );
        let original = doc.clone();
        let start = at_time(&doc, 0).unwrap();
        assert_eq!(start.node(text), doc.node(text));
        assert_eq!(
            crate::geometry::node_bounds(&start, image).unwrap().x,
            crate::geometry::node_bounds(&doc, image).unwrap().x + 100
        );
        assert!(start.node(image).unwrap().locked);
        assert_eq!(at_time(&doc, 1000).unwrap(), doc);
        assert_eq!(at_time(&doc, 250).unwrap(), at_time(&doc, 250).unwrap());
        assert!(!at_time(&doc, 3000).unwrap().node(image).unwrap().visible);
        assert_eq!(doc, original);
    }

    #[test]
    fn clipboard_remaps_saved_rules_and_invalid_timing_is_atomic() {
        let (mut doc, image, _) = fixture();
        doc.design.motion.insert(image, Motion::default());
        doc.design.constraints.insert(
            image,
            Constraint {
                horizontal: Anchor::End,
                ..Default::default()
            },
        );
        let fragment = Fragment::capture(&doc, &[image]).unwrap();
        let mut editor = Editor::new(doc.clone(), None);
        let pasted = fragment.paste(&mut editor, Slot::TOP, (20., 20.)).unwrap()[0];
        assert_ne!(pasted, image);
        assert_eq!(editor.doc.design.motion[&pasted], doc.design.motion[&image]);
        assert_eq!(
            editor.doc.design.constraints[&pasted],
            doc.design.constraints[&image]
        );
        assert!(editor.undo());
        assert_eq!(editor.doc, doc);
        let mut invalid = doc.design.clone();
        invalid.duration_ms = 200;
        assert!(
            editor
                .execute(Command::SetDesign {
                    design: Box::new(invalid)
                })
                .is_err()
        );
        assert_eq!(editor.doc, doc);
    }
}
