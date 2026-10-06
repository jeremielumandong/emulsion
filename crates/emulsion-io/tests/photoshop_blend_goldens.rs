//! Independent Photoshop pixel gates built solely from numeric layer inputs.
#[path = "common/photoshop_blend_fixture.rs"]
mod photoshop;

use emulsion_raster::blend::BlendSpace;

#[test]
fn photoshop_sources_and_expected_pixels_retain_pinned_hashes() {
    photoshop::verify_sources();
}

#[test]
fn photoshop_fill_and_opacity_on_transparency_match_native() {
    photoshop::assert_case("opacity-fill", BlendSpace::Srgb);
}

#[test]
fn photoshop_separate_clip_reference_matches_native_pixel_member() {
    photoshop::assert_case("group-clipping", BlendSpace::Srgb);
}

#[test]
fn photoshop_background_identity_comes_from_raw_records_not_names() {
    photoshop::assert_background_identity_ignores_names();
}

#[test]
fn photoshop_srgb_v1_none_inside_isolation_matches_independent_pixels() {
    photoshop::assert_knockout_case(
        "knockout-none-nested",
        BlendSpace::PhotoshopSrgbV1,
        [0, 13909, 14146, 65535],
    );
}

#[test]
fn photoshop_srgb_v1_shallow_inside_isolation_matches_independent_pixels() {
    photoshop::assert_knockout_case(
        "knockout-shallow-nested",
        BlendSpace::PhotoshopSrgbV1,
        [13909, 0, 14146, 65535],
    );
}

#[test]
fn photoshop_srgb_v1_deep_inside_isolation_matches_independent_pixels() {
    photoshop::assert_knockout_case(
        "knockout-deep-nested",
        BlendSpace::PhotoshopSrgbV1,
        [13909, 0, 14146, 65535],
    );
}

#[test]
fn photoshop_srgb_v1_deep_through_passthrough_matches_independent_pixels() {
    photoshop::assert_knockout_case(
        "knockout-deep-nested-pt",
        BlendSpace::PhotoshopSrgbV1,
        [13909, 13909, 65535, 65535],
    );
}
