use super::*;
use crate::{
    NodeKind,
    text::{TextSpec, layout},
};
use std::collections::HashSet;

#[test]
fn catalog_has_five_categories_ten_families_and_thirty_unique_layouts() {
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
    assert_eq!(variants.len(), 30);
    assert_eq!(family_ids.len(), 10);
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
                let (width, height) = family.native_size();
                let project = choice.create_sized(width / 6, height / 6).unwrap();
                project.validate().unwrap();
                assert_eq!(project.kind, ProjectKind::Design);
                assert_eq!(project.pages.len(), family.page_labels().len());
                assert_eq!(project.next_page_id, project.pages.len() as u64 + 1);
                for (page, label) in project.pages.iter().zip(family.page_labels()) {
                    assert!(page.meta.name.ends_with(label));
                    assert_eq!((page.doc.width, page.doc.height), (width / 6, height / 6));
                }
                for page in &project.pages {
                    let expected_resolution = match family.occasion {
                        Category::Social | Category::Presentations => 72.,
                        _ => 300.,
                    };
                    assert_eq!(page.doc.resolution, expected_resolution);
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
    for family in FAMILIES
        .iter()
        .filter(|f| matches!(f.occasion, Category::Wedding | Category::Birthday))
    {
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
fn palettes_change_every_layout_without_changing_its_structure_or_copy() {
    for family in &FAMILIES {
        let (width, height) = family.native_size();
        for variant in family.variants {
            let choice = Selection {
                variant: variant.id,
                ..family.selection()
            };
            let first = choice.create_sized(width / 6, height / 6).unwrap();
            for palette in 1..family.palettes.len() {
                let other = Selection { palette, ..choice }
                    .create_sized(width / 6, height / 6)
                    .unwrap();
                for (first, other) in first.pages.iter().zip(&other.pages) {
                    assert_ne!(first.doc, other.doc);
                    assert_eq!(first.doc.nodes.len(), other.doc.nodes.len());
                    for (a, b) in first.doc.nodes.iter().zip(&other.doc.nodes) {
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
            let (canvas_width, canvas_height) = match family.occasion {
                Category::Social => (500, 500),
                Category::Presentations => (960, 540),
                _ => (500, 700),
            };
            let project = selection.create_sized(canvas_width, canvas_height).unwrap();
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
                        r.0 >= 0.
                            && r.1 >= 0.
                            && r.0 + r.2 <= canvas_width as f32 + 0.5
                            && r.1 + r.3 <= canvas_height as f32 + 0.5,
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

#[test]
fn every_selection_uses_its_category_canvas_and_keeps_paths_in_native_bounds() {
    for family in &FAMILIES {
        for variant in family.variants {
            for palette in 0..3 {
                let choice = Selection {
                    family: family.id,
                    variant: variant.id,
                    palette,
                };
                let project = choice.create().unwrap();
                let (width, height) = family.native_size();
                assert_eq!(project.pages.len(), family.page_labels().len());
                for page in project.pages {
                    assert_eq!((page.doc.width, page.doc.height), (width, height));
                    for node in page.doc.nodes {
                        if let NodeKind::Path { path, .. } = node.kind {
                            let (x, y, w, h) =
                                emulsion_raster::vector_geometry::bounds(&path).unwrap();
                            assert!(
                                x >= -0.5
                                    && y >= -0.5
                                    && x + w <= width as f64 + 0.5
                                    && y + h <= height as f64 + 0.5,
                                "{:?} / {} / {} path outside native canvas: {:?}",
                                variant.id,
                                page.meta.name,
                                node.name,
                                (x, y, w, h)
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn alternative_compositions_are_not_inserted_into_content_sets() {
    for family in &FAMILIES {
        let (w, h) = family.native_size();
        let (w, h) = (w / 6, h / 6);
        let mut primary_pages = Vec::new();
        for variant in family.variants {
            let choice = Selection {
                variant: variant.id,
                ..family.selection()
            };
            let primary = choice.create_primary(w, h).unwrap();
            let project = choice.create_sized(w, h).unwrap();
            assert_eq!(primary, project.pages[0].doc);
            assert_eq!(project.pages.len(), family.page_labels().len());
            for previous in &primary_pages {
                assert_ne!(
                    &primary, previous,
                    "{} alternatives differ structurally",
                    family.label
                );
                assert!(project.pages.iter().all(|page| &page.doc != previous));
            }
            for (index, page) in project.pages.iter().enumerate() {
                assert_eq!(page.meta.id, index as u64 + 1);
                for other in &project.pages[..index] {
                    assert_ne!(
                        page.doc, other.doc,
                        "content pages must have distinct purposes"
                    );
                }
            }
            primary_pages.push(primary);
        }
    }
}

#[test]
fn native_primary_matches_first_content_page_and_category_contracts_are_explicit() {
    for category in Category::ALL {
        let expected = match category {
            Category::Wedding | Category::Birthday => {
                ((1500, 2100), vec!["Invitation", "Details", "RSVP"])
            }
            Category::Social => ((1080, 1080), vec!["Cover", "Story", "Call to action"]),
            Category::Posters => ((1500, 2100), vec!["Flyer / poster"]),
            Category::Presentations => ((1920, 1080), vec!["Title", "Overview", "Next steps"]),
        };
        for family in families(category) {
            assert_eq!(family.native_size(), expected.0);
            assert_eq!(family.page_labels(), expected.1);
            let choice = family.selection();
            assert_eq!(
                choice.primary().unwrap(),
                choice.create().unwrap().pages[0].doc
            );
        }
    }
}

#[test]
fn all_cross_family_variants_and_out_of_range_palettes_are_rejected() {
    for family in &FAMILIES {
        for other_family in &FAMILIES {
            for variant in other_family.variants {
                let choice = Selection {
                    family: family.id,
                    variant: variant.id,
                    palette: 0,
                };
                assert_eq!(choice.validate().is_ok(), family.id == other_family.id);
            }
        }
        for palette in [3, usize::MAX] {
            let choice = Selection {
                palette,
                ..family.selection()
            };
            assert!(choice.validate().is_err());
            assert!(choice.palette().is_err());
            assert!(choice.create_sized(100, 100).is_err());
        }
        assert!(family.selection().create_sized(0, 100).is_err());
        assert!(family.selection().create_sized(100, 0).is_err());
        assert!(family.selection().create_sized(30_001, 100).is_err());
        assert!(family.selection().create_sized(20_001, 20_001).is_err());
    }
}

#[test]
fn original_invitation_selection_identifiers_are_stable() {
    let originals = [
        (
            FamilyId::GardenVows,
            "garden_vows",
            ["botanical_arch", "garden_border", "wildflower_editorial"],
        ),
        (
            FamilyId::ModernVows,
            "modern_vows",
            ["split_type", "monogram", "gallery"],
        ),
        (
            FamilyId::ConfettiClub,
            "confetti_club",
            ["big_number", "party_ticket", "shape_stack"],
        ),
        (
            FamilyId::MidnightToast,
            "midnight_toast",
            ["moonlight", "art_deco", "supper_club"],
        ),
    ];
    for (id, family_name, variants) in originals {
        for (variant, name) in family(id).variants.iter().zip(variants) {
            let json = format!(r#"{{"family":"{family_name}","variant":"{name}","palette":2}}"#);
            let restored: Selection = serde_json::from_str(&json).unwrap();
            assert_eq!(
                restored,
                Selection {
                    family: id,
                    variant: variant.id,
                    palette: 2
                }
            );
            assert_eq!(serde_json::to_string(&restored).unwrap(), json);
        }
    }
    assert_eq!(
        serde_json::to_string(&Occasion::Wedding).unwrap(),
        "\"wedding\""
    );
    assert_eq!(
        serde_json::to_string(&Occasion::Birthday).unwrap(),
        "\"birthday\""
    );
    assert_eq!(
        crate::design::invitations::FamilyId::GardenVows,
        FamilyId::GardenVows
    );
}

#[test]
fn expanded_palettes_keep_readable_ink_and_accent_on_both_paper_colors() {
    fn luminance(color: [u8; 4]) -> f64 {
        color[..3]
            .iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(&component, weight)| {
                let c = component as f64 / 255.;
                let linear = if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                };
                linear * weight
            })
            .sum()
    }
    for family in FAMILIES.iter().filter(|f| {
        matches!(
            f.occasion,
            Category::Social | Category::Posters | Category::Presentations
        )
    }) {
        for palette in family.palettes {
            for color in [
                palette.background,
                palette.ink,
                palette.accent,
                palette.secondary,
                palette.surface,
            ] {
                assert_eq!(color[3], 255);
            }
            for foreground in [palette.ink, palette.accent] {
                for background in [palette.background, palette.surface] {
                    let a = luminance(foreground);
                    let b = luminance(background);
                    let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                    assert!(
                        contrast >= 4.5,
                        "{} / {}: {contrast}",
                        family.label,
                        palette.label
                    );
                }
            }
        }
    }
}
