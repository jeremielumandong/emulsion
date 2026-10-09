//! Independent third-party PSD reference pixels against freshly built native scene trees.
//!
//! No PSD importer, saved-appearance fallback, psd-tools compositor, or expected
//! output image participates in construction of the native input. The manifest
//! contains lossless numeric descriptions of separately decoded raw layer pixels
//! and record-scoped Background role evidence. The profile is selected explicitly.
//! See ../fixtures/psd/blending/README.md for provenance and strict scope limits.

#![allow(dead_code)] // Shared by integration tests and the separate diagnostic example.

use emulsion_raster::{
    CompositeNode, CompositeTree, NodeContent, Placement, Raster, TILE, TileCoord,
    blend::{BlendMode, BlendSpace},
    color,
    composite::{BlendingOptions, Knockout, render_tile_cpu},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::PathBuf, sync::Arc};

#[derive(Deserialize)]
struct Manifest {
    schema: u32,
    upstream_revision: String,
    apply_icc: bool,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    size: [u32; 2],
    source_sha256: String,
    expected_png: String,
    expected_rgba_sha256: String,
    knockout_background_root_index: Option<usize>,
    nodes: Vec<Layer>,
}

#[derive(Deserialize)]
struct Layer {
    name: String,
    kind: String,
    bounds: [i32; 4],
    opacity: u8,
    fill_opacity: u8,
    blend: String,
    knockout: u8,
    stored_clipping: bool,
    photoshop_effective_clipping: bool,
    flags: Flags,
    raw_role: RawRole,
    pixels: Option<Pixels>,
    children: Vec<Layer>,
}

#[derive(Deserialize)]
struct RawRole {
    record_index: usize,
    channel_ids: Vec<i16>,
    flags_byte: u8,
    lspf: Option<u32>,
    lnsr: Option<String>,
    photoshop_background: bool,
}

#[derive(Deserialize)]
struct Flags {
    infx: bool,
    clbl: bool,
    tsly: bool,
    lmgm: bool,
}

#[derive(Deserialize)]
struct Pixels {
    rgba: [u8; 4],
    transparent_rect: Option<[u32; 4]>,
    raw_rgba_sha256: String,
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd/blending")
}

fn manifest() -> Manifest {
    let manifest: Manifest =
        serde_json::from_str(include_str!("../fixtures/psd/blending/manifest.json")).unwrap();
    assert_eq!(manifest.schema, 2);
    assert_eq!(
        manifest.upstream_revision,
        "d68bf46c7140a1f8c74be9c10b4e21103e820761"
    );
    assert!(!manifest.apply_icc);
    assert_eq!(manifest.cases.len(), 6);
    manifest
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn verified_background(case: &Case) -> Option<usize> {
    fn validate(layers: &[Layer], root: bool, records: &mut HashSet<usize>) {
        for (index, layer) in layers.iter().enumerate() {
            let role = &layer.raw_role;
            assert!(
                records.insert(role.record_index),
                "record reused by two nodes"
            );
            // All properties are evidence from this exact source record. Layer
            // names, color, opacity, and bottom position alone confer no role.
            let background = root
                && index == 0
                && layer.kind == "pixels"
                && !layer.stored_clipping
                && role.channel_ids == [0, 1, 2]
                && role.flags_byte == 0x09
                && role.lspf == Some(0x0d)
                && role.lnsr.as_deref() == Some("bgnd");
            assert_eq!(role.photoshop_background, background, "{}", layer.name);
            validate(&layer.children, false, records);
        }
    }
    validate(&case.nodes, true, &mut HashSet::new());
    let target = case
        .nodes
        .iter()
        .position(|layer| layer.raw_role.photoshop_background);
    assert_eq!(case.knockout_background_root_index, target);
    if case.name.starts_with("knockout-") {
        assert_eq!(target, Some(0));
        let ordinary = &case.nodes[1].raw_role;
        assert_eq!(ordinary.channel_ids, [-1, 0, 1, 2]);
        assert_eq!(ordinary.flags_byte, 0x08);
        assert_eq!(ordinary.lspf, Some(0));
        assert!(ordinary.lnsr.is_none());
        assert!(!ordinary.photoshop_background);
    } else {
        assert!(target.is_none());
    }
    target
}

fn native_tree(case: &Case, space: BlendSpace) -> CompositeTree {
    let target = verified_background(case);
    let nodes = native_nodes(&case.nodes, &mut 1);
    CompositeTree {
        width: case.size[0],
        height: case.size[1],
        space,
        knockout_background: target.map(|index| nodes[index].id),
        nodes,
    }
}

fn native_nodes(layers: &[Layer], next_id: &mut u64) -> Vec<CompositeNode> {
    let mut base = None;
    layers
        .iter()
        .enumerate()
        .map(|(index, layer)| {
            let id = *next_id;
            *next_id += 1;
            let content = match layer.kind.as_str() {
                "pixels" => {
                    assert!(layer.children.is_empty());
                    let [left, top, right, bottom] = layer.bounds;
                    let width = u32::try_from(right - left).unwrap();
                    let height = u32::try_from(bottom - top).unwrap();
                    let pixels = layer.pixels.as_ref().unwrap();
                    let mut bytes = Vec::with_capacity(width as usize * height as usize * 4);
                    for y in 0..height {
                        for x in 0..width {
                            let mut rgba = pixels.rgba;
                            if pixels.transparent_rect.is_some_and(|[x0, y0, x1, y1]| {
                                x >= x0 && x < x1 && y >= y0 && y < y1
                            }) {
                                rgba[3] = 0;
                            }
                            bytes.extend_from_slice(&rgba);
                        }
                    }
                    assert_eq!(sha256(&bytes), pixels.raw_rgba_sha256, "{}", layer.name);
                    NodeContent::Pixels {
                        raster: Arc::new(Raster::from_srgba8(width, height, &bytes)).into(),
                        placement: Placement::at(f64::from(left), f64::from(top)),
                    }
                }
                "group" => {
                    assert!(layer.pixels.is_none());
                    NodeContent::Group(native_nodes(&layer.children, next_id))
                }
                other => panic!("unrecognized controlled input kind: {other}"),
            };
            if layer.stored_clipping != layer.photoshop_effective_clipping {
                // Only the separately exported PSD-compatibility fixture
                // has this exception. Native clipped-group semantics are not
                // changed, and this is not a group-as-base parity assertion.
                assert_eq!(layer.name, "clipping");
                assert_eq!(layer.kind, "group");
                assert!(layer.stored_clipping);
                assert!(!layer.photoshop_effective_clipping);
            }
            let clip_to = if layer.photoshop_effective_clipping {
                Some(base.expect("a clipped member must follow its base"))
            } else {
                base = Some(index);
                None
            };
            CompositeNode {
                id,
                visible: true,
                opacity: f32::from(layer.opacity) / 255.0,
                blend: match layer.blend.as_str() {
                    "norm" => BlendMode::Normal,
                    "pass" => BlendMode::PassThrough,
                    other => panic!("unrecognized controlled blend: {other}"),
                },
                blending: BlendingOptions {
                    fill_opacity: f32::from(layer.fill_opacity) / 255.0,
                    knockout: match layer.knockout {
                        0 => Knockout::None,
                        1 => Knockout::Shallow,
                        2 => Knockout::Deep,
                        other => panic!("unrecognized controlled knockout: {other}"),
                    },
                    blend_interior_effects_as_group: layer.flags.infx,
                    blend_clipped_layers_as_group: layer.flags.clbl,
                    transparency_shapes_layer: layer.flags.tsly,
                    layer_mask_hides_effects: layer.flags.lmgm,
                    ..Default::default()
                },
                mask: None,
                clip_to,
                clip_rect: None,
                content,
            }
        })
        .collect()
}

struct Rendered {
    rgba8: Vec<u8>,
    rgba16: Vec<[u16; 4]>,
}

fn render_cpu(case: &Case, space: BlendSpace) -> Rendered {
    let [width, height] = case.size;
    let tree = native_tree(case, space);
    let mut output = Rendered {
        rgba8: vec![0; width as usize * height as usize * 4],
        rgba16: vec![[0; 4]; width as usize * height as usize],
    };
    for ty in 0..height.div_ceil(TILE) {
        for tx in 0..width.div_ceil(TILE) {
            let tile = render_tile_cpu(&tree, 0, TileCoord::new(tx as i32, ty as i32));
            for y in 0..TILE.min(height - ty * TILE) {
                for x in 0..TILE.min(width - tx * TILE) {
                    // Match production flatten's intermediate premultiplied
                    // RGBA16 storage, while explicitly bypassing acceleration.
                    let pixel = color::f_to_px(tile[(y * TILE + x) as usize]);
                    let index = ((ty * TILE + y) * width + tx * TILE + x) as usize;
                    output.rgba16[index] = pixel;
                    let offset = index * 4;
                    output.rgba8[offset..offset + 4]
                        .copy_from_slice(&color::premul_to_srgba8(color::px_to_f(pixel)));
                }
            }
        }
    }
    output
}

fn comparison(case: &Case, actual: &[u8], space: BlendSpace) -> Option<String> {
    let expected = image::open(fixtures().join(&case.expected_png))
        .unwrap()
        .to_rgba8();
    assert_eq!(expected.dimensions(), (case.size[0], case.size[1]));
    assert_eq!(sha256(expected.as_raw()), case.expected_rgba_sha256);
    let pairs = actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.as_raw().as_chunks::<4>().0.iter());
    let mut count = 0;
    let mut max_error = 0;
    let mut first = None;
    for (index, (a, e)) in pairs.enumerate() {
        if a != e {
            count += 1;
            let error = a.iter().zip(e).map(|(a, e)| a.abs_diff(*e)).max().unwrap();
            max_error = max_error.max(error);
            first.get_or_insert_with(|| {
                format!(
                    "({}, {}): native {a:?}, reference {e:?}",
                    index % case.size[0] as usize,
                    index / case.size[0] as usize
                )
            });
        }
    }
    first.map(|first| {
        format!(
            "{} ({space:?}): {count} differing pixels, max byte error {max_error}; {first}",
            case.name
        )
    })
}

fn case(name: &str) -> Case {
    manifest()
        .cases
        .into_iter()
        .find(|case| case.name == name)
        .unwrap()
}

pub fn assert_case(name: &str, space: BlendSpace) {
    let case = case(name);
    let rendered = render_cpu(&case, space);
    if let Some(difference) = comparison(&case, &rendered.rgba8, space) {
        panic!("{difference}");
    }
}

pub fn assert_knockout_case(name: &str, space: BlendSpace, expected_raw: [u16; 4]) {
    assert_eq!(space, BlendSpace::PhotoshopSrgbV1);
    let case = case(name);
    assert!(case.name.starts_with("knockout-"));
    assert_eq!(verified_background(&case), Some(0));
    let rendered = render_cpu(&case, space);
    if let Some(difference) = comparison(&case, &rendered.rgba8, space) {
        panic!("{difference}");
    }
    for (index, actual) in rendered.rgba16.iter().enumerate() {
        assert_eq!(
            *actual,
            expected_raw,
            "{} ({space:?}), raw RGBA16 at ({}, {})",
            case.name,
            index % case.size[0] as usize,
            index / case.size[0] as usize
        );
    }
}

pub fn assert_background_identity_ignores_names() {
    let mut case = case("knockout-deep-nested-pt");
    let original = native_tree(&case, BlendSpace::PhotoshopSrgbV1);
    case.nodes[0].name = "Renamed white layer".into();
    case.nodes[1].name = "Background".into();
    let renamed = native_tree(&case, BlendSpace::PhotoshopSrgbV1);
    assert_eq!(original.knockout_background, Some(original.nodes[0].id));
    assert_eq!(renamed.knockout_background, original.knockout_background);
    assert_ne!(renamed.knockout_background, Some(renamed.nodes[1].id));
}

pub fn verify_sources() {
    for case in manifest().cases {
        let source = std::fs::read(fixtures().join(format!("{}.psd", case.name))).unwrap();
        assert_eq!(sha256(&source), case.source_sha256, "{}", case.name);
        let expected = image::open(fixtures().join(&case.expected_png))
            .unwrap()
            .to_rgba8();
        assert_eq!(
            sha256(expected.as_raw()),
            case.expected_rgba_sha256,
            "{}",
            case.name
        );
        // Also reject accidental changes to raw input bytes and role association.
        let _ = native_tree(&case, BlendSpace::PhotoshopSrgbV1);
    }
}

pub fn knockout_differences(space: BlendSpace) -> Vec<String> {
    manifest()
        .cases
        .iter()
        .filter(|case| case.name.starts_with("knockout-"))
        .filter_map(|case| comparison(case, &render_cpu(case, space).rgba8, space))
        .collect()
}
