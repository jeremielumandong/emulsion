//! Opt-in tracking for legacy instances; new instances track native edits.
use crate::{
    Document, Node, NodeKind,
    design_components::{self, Overrides},
};
/// Infer only from ordinary authoring commands. Native source propagation uses
/// Plan/commit snapshots and does not pass through this hook.
pub(crate) fn infer(before: &Document, after: &mut Document) {
    if !before
        .design
        .component_links
        .values()
        .any(|link| link.auto_overrides)
    {
        return;
    }
    let sources: std::collections::HashSet<_> = design_components::source_roots(&before.design)
        .into_iter()
        .flat_map(|id| before.subtree(id))
        .collect();
    let mut updates = Vec::new();
    for (instance, link) in &before.design.component_links {
        if !link.auto_overrides || sources.contains(instance) {
            continue;
        }
        let Some(current) = after.design.component_links.get(instance) else {
            continue;
        };
        if current.component != link.component
            || current.variant != link.variant
            || current.members != link.members
        {
            continue;
        }
        for (source, target) in &link.members {
            // Nested components own their own property tracking.
            if design_components::owner(before, *target) != Some(*instance) {
                continue;
            }
            let (Some(old), Some(new)) = (before.node(*target), after.node(*target)) else {
                continue;
            };
            let flags = changed(old, new);
            if !flags.is_empty() {
                updates.push((*instance, *source, flags));
            }
        }
    }
    for (instance, source, flags) in updates {
        if let Some(link) = after.design.component_links.get_mut(&instance) {
            let target = link.overrides.entry(source).or_default();
            merge(target, flags);
        }
    }
}
fn merge(to: &mut Overrides, from: Overrides) {
    macro_rules! merge{($($field:ident),*)=>{$(to.$field|=from.$field;)*}}
    merge!(
        content,
        appearance,
        geometry,
        opacity,
        visibility,
        fill,
        stroke,
        stroke_width,
        font_family,
        font_size,
        text_color,
        position,
        size,
        effects
    );
}
fn changed(old: &Node, new: &Node) -> Overrides {
    let mut f = Overrides {
        opacity: old.opacity != new.opacity,
        visibility: old.visible != new.visible,
        effects: old.styles != new.styles
            || old.style_options != new.style_options
            || old.effects_enabled != new.effects_enabled,
        appearance: old.blend != new.blend || old.blending != new.blending,
        ..Default::default()
    };
    match (&old.kind, &new.kind) {
        (NodeKind::Text { spec: a, .. }, NodeKind::Text { spec: b, .. }) => {
            f.content = if a.paragraphs.is_empty() && b.paragraphs.is_empty() {
                a.text != b.text
            } else {
                crate::text::paragraph_content(a) != crate::text::paragraph_content(b)
            };
            f.font_family = a.font != b.font;
            f.font_size = a.size != b.size;
            f.text_color = a.color != b.color;
            f.position = a.x != b.x || a.y != b.y;
            f.size = a.width != b.width
                || a.height != b.height
                || a.scale_x != b.scale_x
                || a.scale_y != b.scale_y;
            f.geometry = a.rotation != b.rotation
                || a.vertical != b.vertical
                || a.warp != b.warp
                || a.text_path != b.text_path;
            f.appearance |= a.bold != b.bold
                || a.italic != b.italic
                || a.underline != b.underline
                || a.strikethrough != b.strikethrough
                || a.line_height != b.line_height
                || a.align != b.align
                || a.anti_alias != b.anti_alias
                || a.letter_spacing != b.letter_spacing
                || a.paragraphs != b.paragraphs;
            if a.text == b.text && a.runs != b.runs {
                f.appearance = true;
            }
        }
        (
            NodeKind::Path {
                path: a, style: x, ..
            },
            NodeKind::Path {
                path: b, style: y, ..
            },
        ) => {
            f.geometry = a != b;
            f.fill = x.fill != y.fill || x.fill_paint != y.fill_paint;
            f.stroke = x.stroke != y.stroke || x.stroke_paint != y.stroke_paint;
            f.stroke_width = x.width != y.width;
            let mut rest = *x;
            rest.fill = y.fill;
            rest.fill_paint = y.fill_paint;
            rest.stroke = y.stroke;
            rest.stroke_paint = y.stroke_paint;
            rest.width = y.width;
            f.appearance |= rest != *y;
        }
        (NodeKind::Fill { rgba: a }, NodeKind::Fill { rgba: b }) => f.fill = a != b,
        (
            NodeKind::Raster {
                raster: a,
                placement: x,
            },
            NodeKind::Raster {
                raster: b,
                placement: y,
            },
        ) => {
            f.content = !std::sync::Arc::ptr_eq(a, b);
            f.geometry = x != y;
        }
        (
            NodeKind::Smart {
                placement: a,
                source: x,
                ..
            },
            NodeKind::Smart {
                placement: b,
                source: y,
                ..
            },
        ) => {
            f.geometry = a != b;
            f.content = !std::sync::Arc::ptr_eq(x, y);
        }
        _ => {}
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, command::Slot, text::TextSpec};
    #[test]
    fn automatic_component_edits_preserve_fine_properties_and_reset_is_explicit() {
        let mut e = Editor::new(Document::new(500, 300), None);
        let text = e
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Title",
                    TextSpec {
                        text: "Original".into(),
                        ..Default::default()
                    },
                    500,
                    300,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let a = design_components::create(&mut e, &[text], "Card").unwrap();
        let b = design_components::insert(&mut e, "Card", "Default", (200., 0.)).unwrap();
        assert!(e.doc.design.component_links[&b].auto_overrides);
        let child = e.doc.children(Some(b))[0];
        let baseline = e.doc.clone();
        e.execute(Command::SetOpacity {
            id: child,
            opacity: 0.3,
        })
        .unwrap();
        let flags = design_components::overrides_for(&e.doc, b, child);
        assert!(flags.opacity);
        assert!(!flags.appearance);
        e.undo();
        assert_eq!(e.doc, baseline);
        e.redo();
        let before_preview = e.doc.clone();
        e.begin("Opacity preview");
        e.preview(Command::SetOpacity {
            id: child,
            opacity: 0.7,
        })
        .unwrap();
        e.cancel();
        assert_eq!(e.doc, before_preview);
        let NodeKind::Text { spec, .. } = &e.doc.node(text).unwrap().kind else {
            panic!()
        };
        let mut spec = (**spec).clone();
        spec.text = "Published".into();
        e.execute(Command::SetText {
            id: text,
            spec: Box::new(spec),
        })
        .unwrap();
        design_components::update(&mut e, a, None).unwrap();
        assert_eq!(e.doc.node(child).unwrap().opacity, 0.3);
        let NodeKind::Text { spec, .. } = &e.doc.node(child).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text, "Published");
        assert!(!design_components::overrides_for(&e.doc, b, child).content);
        design_components::reset(&mut e, b, None).unwrap();
        assert_eq!(e.doc.node(child).unwrap().opacity, 1.);
        assert!(e.doc.design.component_links[&b].overrides.is_empty());
        assert!(e.doc.design.component_links[&b].auto_overrides);
        design_components::set_auto_overrides(&mut e, b, false).unwrap();
        e.execute(Command::SetOpacity {
            id: child,
            opacity: 0.2,
        })
        .unwrap();
        assert!(e.doc.design.component_links[&b].overrides.is_empty());
        let mut wire = serde_json::to_value(&e.doc.design.component_links[&b]).unwrap();
        wire.as_object_mut().unwrap().remove("auto_overrides");
        let legacy: design_components::Instance = serde_json::from_value(wire).unwrap();
        assert!(!legacy.auto_overrides);
    }
    #[test]
    fn automatic_list_formatting_follows_published_words_with_local_paragraph_styles() {
        use crate::text::{ParagraphFormat, ParagraphList, apply_paragraphs};
        let mut e = Editor::new(Document::new(500, 300), None);
        let text = e
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Title",
                    TextSpec {
                        text: "Old first\nOld second".into(),
                        ..Default::default()
                    },
                    500,
                    300,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let a = design_components::create(&mut e, &[text], "List").unwrap();
        let b = design_components::insert(&mut e, "List", "Default", (200., 0.)).unwrap();
        let child = e.doc.children(Some(b))[0];
        let NodeKind::Text { spec, .. } = &e.doc.node(child).unwrap().kind else {
            panic!()
        };
        let spec = apply_paragraphs(
            spec,
            0..spec.text.len(),
            ParagraphFormat {
                list: ParagraphList::Numbered,
                hanging: 14.,
                ..Default::default()
            },
        )
        .unwrap();
        e.execute(Command::SetText {
            id: child,
            spec: Box::new(spec),
        })
        .unwrap();
        let flags = design_components::overrides_for(&e.doc, b, child);
        assert!(flags.appearance);
        assert!(!flags.content);
        let NodeKind::Text { spec, .. } = &e.doc.node(text).unwrap().kind else {
            panic!()
        };
        let mut spec = (**spec).clone();
        spec.text = "Published longer first\nNew second".into();
        e.execute(Command::SetText {
            id: text,
            spec: Box::new(spec),
        })
        .unwrap();
        design_components::update(&mut e, a, None).unwrap();
        let NodeKind::Text { spec, .. } = &e.doc.node(child).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text, "1. Published longer first\n2. New second");
        assert_eq!(spec.paragraphs[1].start, spec.text.find('\n').unwrap() + 1);
        e.doc.validate().unwrap();
    }
}
