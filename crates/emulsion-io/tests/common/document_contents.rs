//! Exact document assertions for independently decoded import results.
//! Production equality deliberately uses allocation identity and omits some
//! non-history metadata. Verify payloads before aligning only those identities.

use emulsion_core::{Document, NodeKind};
use emulsion_raster::Mask;
use emulsion_raster::image::{Pix, Plane};
use std::fmt::Debug;
use std::sync::Arc;

#[track_caller]
fn assert_plane_contents<P: Pix + Debug>(actual: &Plane<P>, expected: &Plane<P>, context: &str) {
    assert_eq!(
        (actual.width(), actual.height(), actual.fill()),
        (expected.width(), expected.height(), expected.fill()),
        "{context}: source dimensions/fill"
    );
    for y in 0..actual.height() {
        for x in 0..actual.width() {
            assert_eq!(
                actual.get(x, y),
                expected.get(x, y),
                "{context}: source pixel ({x}, {y})"
            );
        }
    }
}

#[track_caller]
fn align_mask(actual: &mut Option<Arc<Mask>>, expected: &Option<Arc<Mask>>, context: &str) {
    match (actual, expected) {
        (Some(actual), Some(expected)) => {
            assert_plane_contents(actual, expected, context);
            *actual = expected.clone();
        }
        (None, None) => {}
        _ => panic!("{context}: mask presence changed"),
    }
}

#[track_caller]
pub fn assert_document_contents(actual: &Document, expected: &Document, context: &str) {
    // These fields are deliberately outside Document::PartialEq/history.
    assert_eq!(
        actual.source_depth, expected.source_depth,
        "{context}: source depth"
    );
    assert_eq!(
        actual.next_id, expected.next_id,
        "{context}: node allocator"
    );
    assert_eq!(actual.info, expected.info, "{context}: image metadata");
    assert_eq!(actual.colors, expected.colors, "{context}: project colors");
    assert_eq!(
        actual.drawing_guides, expected.drawing_guides,
        "{context}: drawing guides"
    );
    assert_eq!(
        actual.nodes.len(),
        expected.nodes.len(),
        "{context}: layer count"
    );

    let mut aligned = actual.clone();
    align_mask(&mut aligned.selection, &expected.selection, context);
    for (index, (actual, expected)) in aligned.nodes.iter_mut().zip(&expected.nodes).enumerate() {
        let context = format!("{context}: layer {index}");
        align_mask(&mut actual.mask, &expected.mask, &context);
        match (&mut actual.kind, &expected.kind) {
            (
                NodeKind::Raster { raster: actual, .. },
                NodeKind::Raster {
                    raster: expected, ..
                },
            ) => {
                assert_plane_contents(actual, expected, &context);
                *actual = expected.clone();
            }
            (
                NodeKind::Smart {
                    source: actual_source,
                    original_image: actual_original,
                    filter_mask: actual_mask,
                    cache: actual_cache,
                    offset: actual_offset,
                    ..
                },
                NodeKind::Smart {
                    source: expected_source,
                    original_image: expected_original,
                    filter_mask: expected_mask,
                    cache: expected_cache,
                    offset: expected_offset,
                    ..
                },
            ) => {
                assert_plane_contents(actual_source, expected_source, &context);
                *actual_source = expected_source.clone();
                match (actual_original, expected_original) {
                    (Some(actual), Some(expected)) => {
                        assert_eq!(
                            actual.bytes(),
                            expected.bytes(),
                            "{context}: original image bytes"
                        );
                        assert_eq!(
                            actual.encoded_sha256(),
                            expected.encoded_sha256(),
                            "{context}: encoded digest"
                        );
                        assert_eq!(
                            actual.source_sha256(),
                            expected.source_sha256(),
                            "{context}: source digest"
                        );
                        *actual = expected.clone();
                    }
                    (None, None) => {}
                    _ => panic!("{context}: original image presence changed"),
                }
                match (actual_mask, expected_mask) {
                    (Some(actual), Some(expected)) => {
                        assert_plane_contents(&actual.pixels, &expected.pixels, &context);
                        actual.pixels = expected.pixels.clone();
                    }
                    (None, None) => {}
                    _ => panic!("{context}: Smart Filter mask presence changed"),
                }
                // Smart equality omits these derived fields.
                assert_plane_contents(actual_cache, expected_cache, &context);
                assert_eq!(
                    &*actual_offset, expected_offset,
                    "{context}: Smart cache offset"
                );
            }
            (
                NodeKind::Path { cache: actual, .. },
                NodeKind::Path {
                    cache: expected, ..
                },
            )
            | (
                NodeKind::Text { cache: actual, .. },
                NodeKind::Text {
                    cache: expected, ..
                },
            )
            | (
                NodeKind::Strokes { cache: actual, .. },
                NodeKind::Strokes {
                    cache: expected, ..
                },
            ) => {
                assert_eq!(
                    actual.size(),
                    expected.size(),
                    "{context}: vector cache size"
                );
                assert_plane_contents(actual.pixels(), expected.pixels(), &context);
            }
            _ => {}
        }
        // Geometry, placement, editable Smart content, filters, all mask
        // controls, hierarchy, IDs, blending, styles and other node metadata.
        assert_eq!(&*actual, expected, "{context}: node contents");
    }
    // All remaining document metadata, including profile, Background identity,
    // dimensions, resolution, design/diagram, ruler guides and RAW provenance.
    assert_eq!(&aligned, expected, "{context}: document contents");
}
