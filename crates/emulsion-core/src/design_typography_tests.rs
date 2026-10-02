use super::*;
use crate::{Command, Editor, NodeKind, command::Slot};
use std::collections::HashSet;

#[test]
fn typography_catalog_has_stable_distinct_searchable_combinations() {
    let pairs = typography_pairs();
    assert_eq!(pairs.len(), 10);
    assert_eq!(pairs, typography_pairs());
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    let families = crate::text::font_families();
    for (index, pair) in pairs.iter().enumerate() {
        assert_eq!(pair.index, index);
        assert!(ids.insert(pair.id));
        assert!(names.insert(pair.name));
        assert!(!pair.description.is_empty());
        assert!(pair.matches_query(pair.name));
        assert!(pair.matches_query(&pair.heading.font.to_uppercase()));
        assert!(pair.matches_query(&format!("  {}  {} ", pair.heading.font, pair.body.font)));
        assert!(pair.matches_query(" \t\n "));
        assert!(!pair.matches_query("no-such-typeface-987"));
        for (style, sample) in [
            (&pair.heading, pair.heading_sample),
            (&pair.body, pair.body_sample),
        ] {
            assert!(families.contains(&style.font), "{}", style.font);
            let spec = style.text_spec(sample);
            assert_eq!(spec.font, style.font);
            assert_eq!(spec.text, sample);
            assert_eq!(spec.size, style.size);
            assert_eq!(spec.bold, style.bold);
            assert_eq!(spec.italic, style.italic);
            assert_eq!(spec.line_height, style.line_height);
            assert_eq!(spec.letter_spacing, style.letter_spacing);
        }
    }
}

#[test]
fn typography_resolves_only_available_families_or_bundled_fallbacks() {
    let bundled = ["Geist".to_string(), "Geist Mono".to_string()];
    for pair in resolve_catalog(&bundled) {
        for style in [pair.heading, pair.body] {
            assert!(bundled.contains(&style.font));
            assert!(!style.italic, "Bundled faces are upright");
        }
    }
    let available: Vec<String> = ["gEoRgIa", "Arial", "Lato", "Geist", "Geist Mono"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let pairs = resolve_catalog(&available);
    assert_eq!(pairs[3].heading.font, "gEoRgIa");
    assert_eq!(pairs[5].heading.font, "Geist");
    assert_eq!(pairs[5].body.font, "Arial");
    assert_eq!(pairs[6].heading.font, "Lato");
    assert_eq!(pairs[6].body.font, "gEoRgIa");
    assert!(!pairs[5].matches_query("Baskerville"));
}

#[test]
fn typography_pairs_stay_editable_and_paste_as_one_undo() {
    for pair in typography_pairs() {
        let mut editor = Editor::new(Document::new(600, 400), None);
        let before = editor.doc.clone();
        let fragment = typography_pair(&editor.doc, pair.index).unwrap();
        assert_eq!(fragment.nodes.len(), 3);
        assert_eq!(fragment.roots.len(), 1);
        let roots = fragment.paste(&mut editor, Slot::TOP, (0., 0.)).unwrap();
        editor.doc.validate().unwrap();
        assert_eq!(roots.len(), 1);
        assert!(editor.doc.node(roots[0]).unwrap().is_group());
        assert_eq!(editor.doc.children(Some(roots[0])).len(), 2);
        for (name, style, sample) in [
            ("Heading", &pair.heading, pair.heading_sample),
            ("Body", &pair.body, pair.body_sample),
        ] {
            let node = editor.doc.nodes.iter().find(|n| n.name == name).unwrap();
            let NodeKind::Text { spec, cache } = &node.kind else {
                panic!("Pair content must remain native editable text");
            };
            assert!(!cache.is_rendered());
            assert_eq!(spec.font, style.font);
            assert_eq!(spec.bold, style.bold);
            assert_eq!(spec.italic, style.italic);
            assert_eq!(spec.text, sample);
            let bounds = crate::text::bounds(spec);
            assert!(bounds.x >= 0 && bounds.y >= 0);
            assert!(bounds.x + bounds.w <= 600 && bounds.y + bounds.h <= 400);
        }
        let pasted = editor.doc.clone();
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
        assert!(!editor.undo(), "One paste must create only one undo entry");
        assert!(editor.redo());
        assert_eq!(editor.doc, pasted);

        let heading = editor
            .doc
            .nodes
            .iter()
            .find(|n| n.name == "Heading")
            .unwrap();
        let NodeKind::Text { spec, .. } = &heading.kind else {
            unreachable!();
        };
        let mut edited = (**spec).clone();
        edited.text = "My own headline".into();
        editor
            .execute(Command::SetText {
                id: heading.id,
                spec: Box::new(edited),
            })
            .unwrap();
        assert!(editor.undo());
        assert_eq!(editor.doc, pasted);
    }
}

#[test]
fn typography_pairs_fit_tiny_portrait_landscape_and_extreme_canvases() {
    for (width, height) in [
        (1, 1),
        (1, 30_000),
        (30_000, 1),
        (8, 8),
        (120, 24),
        (24, 120),
        (1080, 1080),
        (1080, 1920),
        (1920, 1080),
        (30_000, 10_000),
        (10_000, 30_000),
    ] {
        let doc = Document::new(width, height);
        for pair in typography_pairs() {
            let fragment = fragment(&doc, &pair);
            let mut previous_bottom = 0.;
            for node in fragment.nodes.iter().take(2) {
                let NodeKind::Text { spec, .. } = &node.kind else {
                    panic!("Pair content must remain native text");
                };
                assert_eq!(spec.as_ref(), &spec.as_ref().clone().sanitized());
                let rect = crate::text::layout(spec).bounds();
                let left = spec.x + rect.x * spec.scale_x;
                let top = spec.y + rect.y * spec.scale_y;
                let right = left + rect.width * spec.scale_x;
                let bottom = top + rect.height * spec.scale_y;
                assert!(
                    left >= 0. && top >= previous_bottom,
                    "{} on {width}x{height}",
                    pair.name
                );
                assert!(
                    right <= width as f32 + 0.01,
                    "{} on {width}x{height}: {right}",
                    pair.name
                );
                assert!(
                    bottom <= height as f32 + 0.01,
                    "{} on {width}x{height}: {bottom}",
                    pair.name
                );
                previous_bottom = bottom;
            }
        }
    }
}

#[test]
fn typography_rejects_unknown_indices_and_empty_canvases() {
    assert!(typography_pair(&Document::new(600, 400), PAIRS.len()).is_none());
    assert!(typography_pair(&Document::new(600, 400), usize::MAX).is_none());
    assert!(typography_pair(&Document::new(0, 400), 0).is_none());
    assert!(typography_pair(&Document::new(600, 0), 0).is_none());
}
