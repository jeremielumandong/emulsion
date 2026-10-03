use super::*;
use crate::{
    NodeKind,
    text::{TextSpec, layout},
};
use std::collections::HashSet;

#[test]
fn catalog_has_two_occasions_four_families_and_twelve_unique_layouts() {
    let mut variants = HashSet::new();
    let mut family_ids = HashSet::new();
    for occasion in Occasion::ALL {
        assert_eq!(families(occasion).count(), 2);
        for family in families(occasion) {
            assert!(family_ids.insert(family.id));
            assert!(!family.label.is_empty());
            assert_eq!(family.variants.len(), 3);
            assert_eq!(family.palettes.len(), 3);
            for variant in family.variants {
                assert!(variants.insert(variant.id));
                assert!(!variant.description.is_empty());
            }
            let ids: HashSet<_> = family.palettes.iter().map(|p| p.id).collect();
            assert_eq!(ids.len(), 3);
        }
    }
    assert_eq!(variants.len(), 12);
    assert_eq!(family_ids.len(), 4);
}

#[test]
fn every_variant_palette_builds_a_valid_native_editable_matching_set() {
    for family in &FAMILIES {
        for variant in family.variants {
            for palette in 0..family.palettes.len() {
                let choice = Selection {
                    family: family.id,
                    variant: variant.id,
                    palette,
                };
                let project = choice.create_sized(250, 350).unwrap();
                project.validate().unwrap();
                assert_eq!(project.kind, ProjectKind::Design);
                assert_eq!(project.pages.len(), MATCHING_SET_SIZE);
                assert!(project.pages[0].meta.name.ends_with("Invitation"));
                assert!(project.pages[1].meta.name.ends_with("Details"));
                assert!(project.pages[2].meta.name.ends_with("RSVP"));
                for page in &project.pages {
                    assert_eq!(page.doc.resolution, 300.);
                    assert!(
                        page.doc
                            .nodes
                            .iter()
                            .any(|n| matches!(n.kind, NodeKind::Text { .. }))
                    );
                    assert!(
                        page.doc
                            .nodes
                            .iter()
                            .any(|n| matches!(n.kind, NodeKind::Path { .. }))
                    );
                    assert!(page.doc.nodes.iter().all(|n| matches!(
                        n.kind,
                        NodeKind::Text { .. } | NodeKind::Path { .. } | NodeKind::Fill { .. }
                    )));
                }
            }
        }
    }
}

#[test]
fn chosen_front_is_the_only_front_in_the_matching_set() {
    for family in &FAMILIES {
        let mut fronts = Vec::new();
        for variant in family.variants {
            let choice = Selection {
                family: family.id,
                variant: variant.id,
                palette: 0,
            };
            let front = choice.create_primary(200, 280).unwrap();
            let project = choice.create_sized(200, 280).unwrap();
            assert_eq!(front, project.pages[0].doc);
            assert_ne!(front, project.pages[1].doc);
            assert_ne!(front, project.pages[2].doc);
            for previous in &fronts {
                assert_ne!(&front, previous, "{}", variant.label);
            }
            fronts.push(front);
            for page in &project.pages[1..] {
                let title = page
                    .doc
                    .nodes
                    .iter()
                    .find(|node| node.name == "Card title")
                    .unwrap();
                let NodeKind::Text { spec, .. } = &title.kind else {
                    panic!("editable companion heading")
                };
                assert!(matches!(spec.text.as_str(), "The details" | "Kindly reply"));
            }
        }
    }
}

#[test]
fn palettes_change_the_artwork_without_changing_layout_or_copy() {
    for family in &FAMILIES {
        let first = family.selection().create_primary(200, 280).unwrap();
        for palette in 1..family.palettes.len() {
            let other = Selection {
                palette,
                ..family.selection()
            }
            .create_primary(200, 280)
            .unwrap();
            assert_ne!(first, other);
            assert_eq!(first.nodes.len(), other.nodes.len());
            for (a, b) in first.nodes.iter().zip(&other.nodes) {
                assert_eq!(a.name, b.name);
                match (&a.kind, &b.kind) {
                    (NodeKind::Text { spec: a, .. }, NodeKind::Text { spec: b, .. }) => {
                        assert_eq!(a.text, b.text);
                        assert_eq!(a.x, b.x);
                        assert_eq!(a.y, b.y);
                        assert_eq!(a.size, b.size);
                        assert_eq!(a.font, b.font);
                    }
                    (NodeKind::Path { path: a, .. }, NodeKind::Path { path: b, .. }) => {
                        assert_eq!(a, b)
                    }
                    _ => {}
                }
            }
        }
    }
}

#[test]
fn selection_and_editable_primitives_round_trip_without_flattening() {
    for family in &FAMILIES {
        for variant in family.variants {
            let selection = Selection {
                family: family.id,
                variant: variant.id,
                palette: 2,
            };
            let restored: Selection =
                serde_json::from_slice(&serde_json::to_vec(&selection).unwrap()).unwrap();
            assert_eq!(selection, restored);
            let document = restored.create_primary(200, 280).unwrap();
            for node in &document.nodes {
                match &node.kind {
                    NodeKind::Text { spec, .. } => {
                        let restored: TextSpec =
                            serde_json::from_slice(&serde_json::to_vec(spec).unwrap()).unwrap();
                        assert_eq!(spec.as_ref(), &restored);
                    }
                    NodeKind::Path { path, style, .. } => {
                        let restored: emulsion_raster::vector::Path =
                            serde_json::from_slice(&serde_json::to_vec(path).unwrap()).unwrap();
                        assert_eq!(path.as_ref(), &restored);
                        let restored: emulsion_raster::vector::PathStyle =
                            serde_json::from_slice(&serde_json::to_vec(style).unwrap()).unwrap();
                        assert_eq!(style, &restored);
                    }
                    _ => {}
                }
            }
        }
    }
}

#[test]
fn invalid_family_variant_palette_and_canvas_are_rejected() {
    let base = Selection::for_family(FamilyId::GardenVows);
    assert!(
        Selection {
            variant: VariantId::BigNumber,
            ..base
        }
        .create()
        .is_err()
    );
    assert!(Selection { palette: 3, ..base }.create().is_err());
    assert!(base.create_primary(0, 700).is_err());
    assert!(base.create_primary(500, 0).is_err());
    assert!(
        serde_json::from_str::<Selection>(
            r#"{"family":"garden_vows","variant":"botanical_arch","palette":0,"unknown":true}"#
        )
        .is_err()
    );
}

#[test]
fn all_layouts_and_companions_have_unclipped_nonoverlapping_text() {
    for family in &FAMILIES {
        for variant in family.variants {
            let selection = Selection {
                variant: variant.id,
                ..family.selection()
            };
            let project = selection.create_sized(500, 700).unwrap();
            for page in project.pages {
                let mut rectangles = Vec::new();
                for node in page.doc.nodes {
                    let NodeKind::Text { spec, .. } = node.kind else {
                        continue;
                    };
                    let width = spec.width.unwrap();
                    // Measure each authored line without wrapping. Its real glyph
                    // advances must fit, not merely its clipped paragraph box.
                    for line in spec.text.lines() {
                        let unwrapped = TextSpec {
                            text: line.into(),
                            width: None,
                            align: crate::text::Align::Left,
                            ..(*spec).clone()
                        };
                        let measured = layout(&unwrapped).bounds();
                        assert!(
                            measured.width <= width + 0.5,
                            "{:?} / {} / {} line wraps: {} > {} ({line})",
                            variant.id,
                            page.meta.name,
                            node.name,
                            measured.width,
                            width
                        );
                    }
                    let bounds = layout(&spec).bounds();
                    let r = (
                        spec.x + bounds.x,
                        spec.y + bounds.y,
                        bounds.width,
                        bounds.height,
                    );
                    assert!(
                        r.0 >= 0. && r.1 >= 0. && r.0 + r.2 <= 500.5 && r.1 + r.3 <= 700.5,
                        "{:?} / {} / {} is outside canvas: {r:?}",
                        variant.id,
                        page.meta.name,
                        node.name
                    );
                    for (name, other) in &rectangles {
                        let &(x, y, w, h) = other;
                        let overlap_x = (r.0 + r.2).min(x + w) - r.0.max(x);
                        let overlap_y = (r.1 + r.3).min(y + h) - r.1.max(y);
                        assert!(
                            overlap_x <= 0.5 || overlap_y <= 0.5,
                            "{:?} / {} text collision: {} and {} ({overlap_x}, {overlap_y})",
                            variant.id,
                            page.meta.name,
                            node.name,
                            name
                        );
                    }
                    rectangles.push((node.name, r));
                }
            }
        }
    }
}

#[test]
fn native_invitation_set_is_print_sized_at_three_hundred_ppi() {
    let project = Selection::for_family(FamilyId::GardenVows)
        .create()
        .unwrap();
    for page in project.pages {
        assert_eq!((page.doc.width, page.doc.height), NATIVE_SIZE);
        assert_eq!(page.doc.width as f32 / page.doc.resolution, 5.);
        assert_eq!(page.doc.height as f32 / page.doc.resolution, 7.);
    }
}
