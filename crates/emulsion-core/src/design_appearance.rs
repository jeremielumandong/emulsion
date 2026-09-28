//! Copy object formatting without copying geometry, content, masks or identity.
use crate::{Command, Node, NodeKind};
use emulsion_raster::{blend::BlendMode, vector::PathStyle};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    opacity: f32,
    blend: BlendMode,
    blending: emulsion_raster::composite::BlendingOptions,
    styles: Vec<crate::styles::LayerStyle>,
    options: Vec<crate::style_options::StyleOptions>,
    effects_enabled: bool,
    path: Option<PathStyle>,
    text: Option<(
        crate::text::TextStyle,
        f32,
        crate::text::Align,
        crate::text::AntiAliasMode,
    )>,
    color: Option<[u8; 4]>,
}

impl Appearance {
    pub fn font_families(&self) -> Vec<String> {
        self.text
            .as_ref()
            .map(|(style, ..)| vec![style.font.clone()])
            .unwrap_or_default()
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.opacity.is_finite()
            || !(0. ..=1.).contains(&self.opacity)
            || !self.blending.valid()
            || self.styles.len() > crate::styles::MAX_STYLES
            || self.options.len() > self.styles.len()
            || self.options.iter().any(|option| !option.valid())
            || self
                .styles
                .iter()
                .flat_map(|style| style.params())
                .any(|param| {
                    !param.value.is_finite() || param.value < param.min || param.value > param.max
                })
            || self.path.is_some_and(|style| style != style.sanitized())
        {
            return Err("Saved style contains invalid appearance settings.".into());
        }
        if let Some((style, line_height, _, _)) = &self.text
            && (style.font.chars().count() > 512
                || !style.size.is_finite()
                || !(1. ..=4000.).contains(&style.size)
                || !style.letter_spacing.is_finite()
                || !(-4000. ..=4000.).contains(&style.letter_spacing)
                || !style.baseline.is_finite()
                || !(-4000. ..=4000.).contains(&style.baseline)
                || !line_height.is_finite()
                || !(0.5..=4.).contains(line_height))
        {
            return Err("Saved style contains invalid typography settings.".into());
        }
        Ok(())
    }

    pub fn capture(node: &Node) -> Self {
        let (path, text, color) = match &node.kind {
            NodeKind::Path { style, .. } => (Some(*style), None, style.fill),
            NodeKind::Text { spec, .. } => {
                let style = spec.style_at(0);
                let color = style.color;
                (
                    None,
                    Some((style, spec.line_height, spec.align, spec.anti_alias)),
                    Some(color),
                )
            }
            NodeKind::Fill { rgba } => (None, None, Some(*rgba)),
            _ => (None, None, None),
        };
        Self {
            opacity: node.opacity,
            blend: node.blend,
            blending: node.blending,
            styles: node.styles.clone(),
            options: node.style_options.clone(),
            effects_enabled: node.effects_enabled,
            path,
            text,
            color,
        }
    }

    /// Apply the returned commands as one transaction. Normal command locking,
    /// validation and responsive reflow still apply to every target.
    pub fn commands(&self, node: &Node) -> Vec<Command> {
        let id = node.id;
        let blend = if self.blend == BlendMode::PassThrough && !node.is_group() {
            BlendMode::Normal
        } else {
            self.blend
        };
        let mut commands = vec![
            Command::SetOpacity {
                id,
                opacity: self.opacity,
            },
            Command::SetBlend { id, blend },
            Command::SetBlendingOptions {
                id,
                options: self.blending,
            },
            Command::SetLayerEffects {
                id,
                styles: self.styles.clone(),
                options: self.options.clone(),
            },
            Command::SetEffectsEnabled {
                id,
                enabled: self.effects_enabled,
            },
        ];
        match &node.kind {
            NodeKind::Path { path, style, .. } => {
                let mut style = self.path.unwrap_or(*style);
                if self.path.is_none()
                    && let Some(color) = self.color
                {
                    style.fill = Some(color);
                    style.fill_paint = Default::default();
                }
                commands.push(Command::SetPath {
                    id,
                    path: path.clone(),
                    style,
                });
            }
            NodeKind::Text { spec, .. } => {
                let mut spec = (**spec).clone();
                if let Some((style, line_height, align, anti_alias)) = &self.text {
                    spec.font = style.font.clone();
                    spec.size = style.size;
                    spec.bold = style.bold;
                    spec.italic = style.italic;
                    spec.underline = style.underline;
                    spec.strikethrough = style.strikethrough;
                    spec.letter_spacing = style.letter_spacing;
                    spec.line_height = *line_height;
                    spec.align = *align;
                    spec.anti_alias = *anti_alias;
                    spec.runs.clear();
                    // Baseline shift is stored only in rich-text runs; copying
                    // just the base fields would silently drop superscripts.
                    spec.apply_style(0..spec.text.len(), |target| *target = style.clone());
                }
                if let Some(color) = self.color {
                    spec.color = color;
                    for run in &mut spec.runs {
                        run.style.color = color;
                    }
                }
                commands.push(Command::SetText {
                    id,
                    spec: Box::new(spec),
                });
            }
            NodeKind::Fill { .. } => {
                if let Some(rgba) = self.color {
                    commands.push(Command::SetFillColor { id, rgba });
                }
            }
            _ => {}
        }
        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Editor, command::Slot, text::TextSpec};
    #[test]
    fn copying_text_appearance_preserves_content_geometry_and_undo() {
        let mut source = Node::text(
            1,
            "Source",
            TextSpec {
                text: "Heading".into(),
                size: 52.,
                color: [10, 20, 30, 255],
                bold: true,
                ..Default::default()
            },
            400,
            400,
        );
        source.opacity = 0.6;
        let appearance = Appearance::capture(&source);
        let mut editor = Editor::new(Document::new(400, 400), None);
        let spec = TextSpec {
            text: "My own words".into(),
            x: 70.,
            y: 90.,
            width: Some(160.),
            rotation: 20.,
            ..Default::default()
        };
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(0, "Target", spec.clone(), 400, 400)),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let before = editor.doc.clone();
        let commands = appearance.commands(editor.doc.node(id).unwrap());
        editor.begin("Paste style");
        for command in commands {
            editor.execute(command).unwrap();
        }
        editor.end();
        let target = editor.doc.node(id).unwrap();
        assert_eq!(target.opacity, 0.6);
        let NodeKind::Text { spec: actual, .. } = &target.kind else {
            panic!()
        };
        assert_eq!(actual.text, spec.text);
        assert_eq!(
            (actual.x, actual.y, actual.width, actual.rotation),
            (spec.x, spec.y, spec.width, spec.rotation)
        );
        assert_eq!(actual.size, 52.);
        assert!(actual.bold);
        editor.undo();
        assert_eq!(editor.doc, before);
        editor.redo();
        assert_eq!(editor.doc.node(id).unwrap().opacity, 0.6);
    }
    #[test]
    fn copying_superscript_style_keeps_baseline_without_copying_text() {
        let mut source = TextSpec {
            text: "2".into(),
            size: 18.,
            ..Default::default()
        };
        source.apply_style(0..1, |style| {
            style.baseline = 9.;
            style.italic = true;
        });
        let appearance = Appearance::capture(&Node::text(1, "Exponent", source, 300, 200));
        let mut doc = Document::new(300, 200);
        let id = Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Target",
                TextSpec {
                    text: "Own text".into(),
                    x: 50.,
                    y: 60.,
                    ..Default::default()
                },
                300,
                200,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        for command in appearance.commands(doc.node(id).unwrap()) {
            command.apply(&mut doc).unwrap();
        }
        let NodeKind::Text { spec, .. } = &doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text, "Own text");
        assert_eq!((spec.x, spec.y), (50., 60.));
        assert_eq!(spec.style_at(5).baseline, 9.);
        assert!(spec.style_at(5).italic);
    }
}
