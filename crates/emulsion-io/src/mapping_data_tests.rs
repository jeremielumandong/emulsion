use super::*;
use emulsion_core::{Document, Node, VectorMask};
use emulsion_raster::Raster;
use serde::de::{DeserializeOwned, value};
use std::sync::Arc;

const LEGACY: &str =
    r#"{"x":0.0,"y":0.0,"scale_x":1.0,"scale_y":1.0,"rotation":0.0,"flip_x":false,"flip_y":false}"#;
const PROJECTIVE_IDENTITY: &str = r#"{"projective":[1.0,0.0,0.0,0.0,1.0,0.0,0.0,0.0,1.0]}"#;

fn assert_rejected<T: DeserializeOwned + fmt::Debug>(json: &str) {
    assert!(serde_json::from_str::<T>(json).is_err(), "accepted {json}");
}

fn placement_bits(p: Placement) -> [u64; 5] {
    [p.x, p.y, p.scale_x, p.scale_y, p.rotation].map(f64::to_bits)
}

fn assert_placement_bits(actual: Placement, expected: Placement) {
    assert_eq!(placement_bits(actual), placement_bits(expected));
    assert_eq!(
        (actual.flip_x, actual.flip_y),
        (expected.flip_x, expected.flip_y)
    );
}

fn affine_columns(mapping: Mapping2) -> [f64; 6] {
    match mapping {
        Mapping2::Affine(affine) => affine.to_cols_array(),
        Mapping2::Projective(_) => panic!("legacy array changed representation"),
    }
}

fn document_oracle(placement: Placement, columns: [f64; 6]) -> bool {
    let mut doc = Document::new(1, 1);
    let id = doc.alloc_id();
    let mut node = Node::raster(
        id,
        "legacy oracle",
        Arc::new(Raster::empty(1, 1, [0; 4])),
        placement,
    );
    // No plane: the old predicate applies to dormant metadata too.
    node.mask_transform = Mapping2::Affine(DAffine2::from_cols_array(&columns));
    doc.nodes.push(node);
    doc.validate().is_ok()
}

fn filter_oracle(columns: [f64; 6]) -> bool {
    // Independent pre-v16 numeric predicate, retained as the admission oracle.
    let affine = DAffine2::from_cols_array(&columns);
    let determinant = affine.matrix2.determinant();
    columns.iter().all(|v| v.is_finite())
        && determinant.is_finite()
        && determinant.abs() >= 1e-12
        && affine
            .inverse()
            .to_cols_array()
            .iter()
            .all(|v| v.is_finite())
        && [-1e9, 1e9].into_iter().all(|x| {
            [-1e9, 1e9].into_iter().all(|y| {
                affine.transform_point2(glam::dvec2(x, y)).is_finite()
                    && affine
                        .inverse()
                        .transform_point2(glam::dvec2(x, y))
                        .is_finite()
            })
        })
}

fn vector_oracle(columns: [f64; 6]) -> bool {
    let mask = VectorMask {
        transform: columns,
        ..VectorMask::default()
    };
    mask.valid()
}

#[test]
fn legacy_placement_bytes_and_bits_use_the_existing_writer() {
    for placement in [
        Placement::default(),
        Placement {
            x: -0.0,
            y: -0.0,
            scale_x: -2.25,
            scale_y: 1e-6,
            rotation: -0.0,
            flip_x: true,
            flip_y: false,
        },
        Placement {
            x: f64::from_bits(0x3fd5_5555_5555_5555),
            y: -1e100,
            scale_x: f64::from_bits(0x3ff0_0000_0000_0001),
            scale_y: -17.123456789012344,
            rotation: 359.99999999999994,
            flip_x: true,
            flip_y: true,
        },
    ] {
        let old_bytes = serde_json::to_vec(&placement).unwrap();
        for adapter in [
            PlacementData::from_raster(placement).unwrap(),
            PlacementData::from_smart(SmartPlacement::Legacy(placement)).unwrap(),
        ] {
            assert_eq!(serde_json::to_vec(&adapter).unwrap(), old_bytes);
            assert_eq!(
                serde_json::to_string_pretty(&adapter).unwrap(),
                serde_json::to_string_pretty(&placement).unwrap()
            );
            let restored: PlacementData = serde_json::from_slice(&old_bytes).unwrap();
            assert_placement_bits(restored.into_raster().unwrap(), placement);
            let SmartPlacement::Legacy(restored) = restored.into_smart() else {
                panic!("legacy placement changed representation");
            };
            assert_placement_bits(restored, placement);
        }
    }
    assert_eq!(
        serde_json::to_string(&PlacementData::from_raster(Placement::default()).unwrap()).unwrap(),
        LEGACY
    );
    let signed: PlacementData = serde_json::from_str(
        r#"{"x":-0,"y":-0.0,"scale_x":1,"scale_y":1,"rotation":-0,"flip_x":false,"flip_y":false}"#,
    )
    .unwrap();
    let p = signed.into_raster().unwrap();
    for number in [p.x, p.y, p.rotation] {
        assert_eq!(number.to_bits(), (-0.0_f64).to_bits());
    }
}

#[test]
fn all_seven_placement_fields_remain_required_and_null_is_not_missing() {
    let original: serde_json::Value = serde_json::from_str(LEGACY).unwrap();
    for field in LEGACY_FIELDS {
        let mut omitted = original.clone();
        omitted.as_object_mut().unwrap().remove(*field);
        let json = omitted.to_string();
        assert_rejected::<Placement>(&json);
        assert_rejected::<PlacementData>(&json);
        let mut null = original.clone();
        null[*field] = serde_json::Value::Null;
        let json = null.to_string();
        assert_rejected::<Placement>(&json);
        assert_rejected::<PlacementData>(&json);
    }
    assert_rejected::<PlacementData>("{}");
    // The native placement contract is an object, never a seven-item tuple.
    assert_rejected::<PlacementData>("[0,0,1,1,0,false,false]");
}

#[test]
fn placement_legacy_numeric_admission_matches_document_oracle() {
    let identity = emulsion_core::node::default_mask_transform();
    let mut cases = vec![Placement::default()];
    for value in [
        0.0,
        -0.0,
        1e-6,
        -1e-6,
        1e-6_f64.next_down(),
        1e300,
        f64::MAX,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        cases.extend([
            Placement {
                x: value,
                ..Placement::default()
            },
            Placement {
                y: value,
                ..Placement::default()
            },
            Placement {
                scale_x: value,
                ..Placement::default()
            },
            Placement {
                scale_y: value,
                ..Placement::default()
            },
            Placement {
                rotation: value,
                ..Placement::default()
            },
        ]);
    }
    for placement in cases {
        let expected = document_oracle(placement, identity);
        assert_eq!(
            PlacementData::from_raster(placement).is_ok(),
            expected,
            "{placement:?}"
        );
        assert_eq!(
            PlacementData::from_smart(SmartPlacement::Legacy(placement)).is_ok(),
            expected,
            "{placement:?}"
        );
        if [
            placement.x,
            placement.y,
            placement.scale_x,
            placement.scale_y,
            placement.rotation,
        ]
        .iter()
        .all(|v| v.is_finite())
        {
            let json = serde_json::to_string(&placement).unwrap();
            assert_eq!(
                serde_json::from_str::<PlacementData>(&json).is_ok(),
                expected,
                "{json}"
            );
        }
    }
}

#[test]
fn component_array_bytes_order_and_signed_zero_match_legacy_writer() {
    for columns in [
        emulsion_core::node::default_mask_transform(),
        [1.0, -0.0, -0.0, 1.0, -0.0, -0.0],
        [2.0, 0.25, -0.375, -3.0, 17.125, -9.25],
        [
            f64::from_bits(0x3ff0_0000_0000_0001),
            0.12345678901234568,
            -0.125,
            0.875,
            0.3333333333333333,
            -1e30,
        ],
    ] {
        let mapping = Mapping2::Affine(DAffine2::from_cols_array(&columns));
        let old_bytes = serde_json::to_vec(&columns).unwrap();
        for adapter in [
            MappingData::from_raster_mask(mapping, ComponentOwner::Smart).unwrap(),
            MappingData::from_raster_mask(mapping, ComponentOwner::Other).unwrap(),
            MappingData::from_filter_mask(mapping).unwrap(),
            MappingData::from_vector_mask(columns).unwrap(),
        ] {
            assert_eq!(serde_json::to_vec(&adapter).unwrap(), old_bytes);
            assert_eq!(
                serde_json::to_string_pretty(&adapter).unwrap(),
                serde_json::to_string_pretty(&columns).unwrap()
            );
            let restored: MappingData = serde_json::from_slice(&old_bytes).unwrap();
            assert_eq!(
                affine_columns(restored.into_raster_mask(ComponentOwner::Other).unwrap())
                    .map(f64::to_bits),
                columns.map(f64::to_bits)
            );
            assert_eq!(
                affine_columns(restored.into_filter_mask().unwrap()).map(f64::to_bits),
                columns.map(f64::to_bits)
            );
            assert_eq!(
                restored.into_vector_mask().unwrap().map(f64::to_bits),
                columns.map(f64::to_bits)
            );
        }
    }
}

#[test]
fn absent_component_defaults_only_at_the_enclosing_optional_field() {
    #[derive(Deserialize)]
    struct Old {
        #[serde(default = "emulsion_core::node::default_mask_transform")]
        mask_transform: [f64; 6],
    }
    #[derive(Debug, Deserialize)]
    struct New {
        #[serde(default)]
        mask_transform: MappingData,
    }
    for json in ["{}", r#"{"unrelated_future_field":null}"#] {
        let old: Old = serde_json::from_str(json).unwrap();
        let new: New = serde_json::from_str(json).unwrap();
        assert_eq!(
            affine_columns(
                new.mask_transform
                    .into_raster_mask(ComponentOwner::Other)
                    .unwrap()
            )
            .map(f64::to_bits),
            old.mask_transform.map(f64::to_bits)
        );
    }
    assert_rejected::<New>(r#"{"mask_transform":null}"#);
    assert_rejected::<New>(r#"{"mask_transform":{}}"#);
    assert_rejected::<MappingData>("null");
    assert_rejected::<MappingData>("{}");
}

#[test]
fn distinct_affine_role_predicates_match_existing_runtime_and_io_oracles() {
    let mut cases = vec![
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        [-1.0, -0.0, 0.0, 1.0, -0.0, 0.0],
        [1e-12, 0.0, 0.0, 1.0, 0.0, 0.0],
        [1e-12_f64.next_down(), 0.0, 0.0, 1.0, 0.0, 0.0],
        [1.0, 1.0, 1.0, 1.0, 0.0, 0.0],
        [1e200, 0.0, 0.0, 1e-200, 0.0, 0.0],
        [1e200, 0.0, 0.0, 1e200, 0.0, 0.0],
        [1e200, 1e200, 1e200, 1e200, 0.0, 0.0],
        [1e300, 0.0, 0.0, 1e-290, 0.0, 0.0],
        [1e-6, 0.0, 0.0, 1.0, f64::MAX, 0.0],
    ];
    for index in 0..6 {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut columns = emulsion_core::node::default_mask_transform();
            columns[index] = value;
            cases.push(columns);
        }
    }
    for columns in cases {
        let mapping = Mapping2::Affine(DAffine2::from_cols_array(&columns));
        let raster = document_oracle(Placement::default(), columns);
        let filter = filter_oracle(columns);
        let vector = vector_oracle(columns);
        for owner in [ComponentOwner::Smart, ComponentOwner::Other] {
            assert_eq!(
                MappingData::from_raster_mask(mapping, owner).is_ok(),
                raster,
                "{columns:?}"
            );
        }
        assert_eq!(
            MappingData::from_filter_mask(mapping).is_ok(),
            filter,
            "{columns:?}"
        );
        assert_eq!(
            MappingData::from_vector_mask(columns).is_ok(),
            vector,
            "{columns:?}"
        );
        if columns.iter().all(|v| v.is_finite()) {
            let json = serde_json::to_string(&columns).unwrap();
            let parsed = serde_json::from_str::<MappingData>(&json);
            assert_eq!(parsed.is_ok(), raster, "{json}");
            if let Ok(parsed) = parsed {
                assert_eq!(parsed.into_filter_mask().is_ok(), filter, "{json}");
                assert_eq!(parsed.into_vector_mask().is_ok(), vector, "{json}");
            }
        }
    }
}

#[test]
fn raster_determinant_overflow_and_nan_remain_inherited_compatibility() {
    for columns in [
        [1e200, 0.0, 0.0, 1e200, 0.0, 0.0],
        [1e200, 1e200, 1e200, 1e200, 0.0, 0.0],
    ] {
        let affine = DAffine2::from_cols_array(&columns);
        assert!(!affine.matrix2.determinant().is_finite());
        assert!(document_oracle(Placement::default(), columns));
        let adapter =
            MappingData::from_raster_mask(Mapping2::Affine(affine), ComponentOwner::Other).unwrap();
        assert!(adapter.into_filter_mask().is_err());
        assert!(adapter.into_vector_mask().is_err());
        let json = serde_json::to_string(&adapter).unwrap();
        assert_eq!(json, serde_json::to_string(&columns).unwrap());
        let restored: MappingData = serde_json::from_str(&json).unwrap();
        assert_eq!(
            affine_columns(restored.into_raster_mask(ComponentOwner::Other).unwrap())
                .map(f64::to_bits),
            columns.map(f64::to_bits)
        );
    }
}

#[test]
fn legacy_retention_does_not_invoke_projective_operation_admission() {
    let columns = [1e200, 0.0, 0.0, 1e-200, -0.0, 0.0];
    let mapping = Mapping2::Affine(DAffine2::from_cols_array(&columns));
    assert!(mapping.validate_for_operation().is_err());
    let adapter = MappingData::from_filter_mask(mapping).unwrap();
    let restored: MappingData =
        serde_json::from_slice(&serde_json::to_vec(&adapter).unwrap()).unwrap();
    assert_eq!(
        affine_columns(restored.into_filter_mask().unwrap()).map(f64::to_bits),
        columns.map(f64::to_bits)
    );
}

#[test]
fn unknown_duplicate_and_conflicting_keys_never_fall_back_to_legacy() {
    // The old Placement serde ignores unknown keys. A complete legacy object
    // carrying a projective marker must never enter that permissive fallback.
    for json in [
        format!(r#"{{"projective":[1,0,0,0,1,0,0,0,1],{}"#, &LEGACY[1..]),
        format!(
            r#"{},"projective":[1,0,0,0,1,0,0,0,1]}}"#,
            &LEGACY[..LEGACY.len() - 1]
        ),
        format!(r#"{{"future":true,{}"#, &LEGACY[1..]),
    ] {
        assert!(serde_json::from_str::<Placement>(&json).is_ok());
        assert_rejected::<PlacementData>(&json);
    }
    for field in LEGACY_FIELDS {
        let original: serde_json::Value = serde_json::from_str(LEGACY).unwrap();
        let value = &original[*field];
        let duplicate = format!(r#"{{"{field}":{value},{}"#, &LEGACY[1..]);
        assert_rejected::<PlacementData>(&duplicate);
        for json in [
            format!(r#"{{"{field}":{value},"projective":[1,0,0,0,1,0,0,0,1]}}"#),
            format!(r#"{{"projective":[1,0,0,0,1,0,0,0,1],"{field}":{value}}}"#),
        ] {
            assert_rejected::<PlacementData>(&json);
            assert_rejected::<MappingData>(&json);
        }
    }
    for json in [
        format!(r#"{{"future":null,{}"#, &LEGACY[1..]),
        format!(r#"{{"\u0078":0,{}"#, &LEGACY[1..]),
        r#"{"projective":[1,0,0,0,1,0,0,0,1],"projective":[1,0,0,0,1,0,0,0,1]}"#.into(),
        r#"{"projective":[1,0,0,0,1,0,0,0,1],"\u0070rojective":[1,0,0,0,1,0,0,0,1]}"#.into(),
        r#"{"projective":[1,0,0,0,1,0,0,0,1],"future":null}"#.into(),
        r#"{"future":null,"projective":[1,0,0,0,1,0,0,0,1]}"#.into(),
        r#"{"affine":[1,0,0,1,0,0]}"#.into(),
    ] {
        assert_rejected::<PlacementData>(&json);
        assert_rejected::<MappingData>(&json);
    }
    let escaped = r#"{"\u0070rojective":[1,0,0,0,1,0,0,0,1]}"#;
    let placement: PlacementData = serde_json::from_str(escaped).unwrap();
    assert!(matches!(
        placement.into_smart(),
        SmartPlacement::Projective(_)
    ));
    let mapping: MappingData = serde_json::from_str(escaped).unwrap();
    assert!(matches!(
        mapping.into_filter_mask().unwrap(),
        Mapping2::Projective(_)
    ));
}

#[test]
fn wrong_lengths_types_and_null_are_refused_at_the_wire_boundary() {
    for json in ["null", "true", "false", "0", "1.5", r#""mapping""#, "[]"] {
        assert_rejected::<PlacementData>(json);
        assert_rejected::<MappingData>(json);
    }
    for length in [0, 1, 5, 7, 8, 9, 10] {
        let columns = serde_json::to_string(&vec![1; length]).unwrap();
        assert_rejected::<MappingData>(&columns);
    }
    for length in [0, 1, 5, 6, 8, 10] {
        let columns = serde_json::to_string(&vec![1; length]).unwrap();
        let json = format!(r#"{{"projective":{columns}}}"#);
        assert_rejected::<PlacementData>(&json);
        assert_rejected::<MappingData>(&json);
    }
    for invalid in ["null", "true", r#""1""#, "[]", "{}", "1e400"] {
        let field = format!(r#"{{"projective":{invalid}}}"#);
        assert_rejected::<PlacementData>(&field);
        assert_rejected::<MappingData>(&field);
        for index in 0..9 {
            let mut values = ["1", "0", "0", "0", "1", "0", "0", "0", "1"];
            values[index] = invalid;
            let json = format!(r#"{{"projective":[{}]}}"#, values.join(","));
            assert_rejected::<PlacementData>(&json);
            assert_rejected::<MappingData>(&json);
        }
        for index in 0..6 {
            let mut values = ["1", "0", "0", "1", "0", "0"];
            values[index] = invalid;
            assert_rejected::<MappingData>(&format!("[{}]", values.join(",")));
        }
    }
    for json in ["[0,0,0,0,0,0]", "[1,1,1,1,0,0]", "[1e-13,0,0,1,0,0]"] {
        assert_rejected::<MappingData>(json);
    }
    for (field, invalid) in [
        ("x", "true"),
        ("scale_x", r#""1""#),
        ("flip_x", "0"),
        ("flip_y", "{}"),
        ("rotation", "[]"),
    ] {
        let mut fields: serde_json::Value = serde_json::from_str(LEGACY).unwrap();
        fields[field] = serde_json::from_str(invalid).unwrap();
        assert_rejected::<PlacementData>(&fields.to_string());
    }
}

#[test]
fn nonfinite_values_are_rejected_even_by_non_json_deserializers() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Finite::deserialize(value::F64Deserializer::<value::Error>::new(value)).is_err());
        for index in 0..9 {
            let mut raw = Projective2::IDENTITY.to_row_major();
            raw[index] = value;
            assert!(
                Coefficients::<9>::deserialize(value::SeqDeserializer::<_, value::Error>::new(
                    raw.into_iter()
                ))
                .is_err()
            );
        }
        for index in 0..6 {
            let mut raw = emulsion_core::node::default_mask_transform();
            raw[index] = value;
            assert!(
                MappingData::deserialize(value::SeqDeserializer::<_, value::Error>::new(
                    raw.into_iter()
                ))
                .is_err()
            );
        }
    }
}

#[test]
fn invalid_projective_matrices_match_the_checked_runtime_constructor() {
    for raw in [
        [0.0; 9],
        [1.0, 2.0, 3.0, 1.0, 2.0, 3.0, 0.0, 0.0, 1.0],
        [1e-200, 0.0, 0.0, 0.0, 1e200, 0.0, 0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0, 1.0, 1.0 + f64::EPSILON, 0.0, 0.0, 0.0, 1.0],
    ] {
        assert!(Projective2::from_row_major(raw).is_err());
        let json = format!(
            r#"{{"projective":{}}}"#,
            serde_json::to_string(&raw).unwrap()
        );
        assert_rejected::<PlacementData>(&json);
        assert_rejected::<MappingData>(&json);
    }
}

#[test]
fn marker_presence_and_owner_roles_survive_affine_valued_projective_matrices() {
    for projective in [
        Projective2::IDENTITY,
        Projective2::from_affine(DAffine2::from_translation(glam::dvec2(0.25, -0.5))).unwrap(),
    ] {
        assert!(projective.to_affine().is_ok());
        let placement = PlacementData::from_smart(SmartPlacement::Projective(projective)).unwrap();
        assert!(placement.into_raster().is_err());
        let placement_bytes = serde_json::to_vec(&placement).unwrap();
        let placement: PlacementData = serde_json::from_slice(&placement_bytes).unwrap();
        assert!(matches!(
            placement.into_smart(),
            SmartPlacement::Projective(_)
        ));
        assert!(placement.into_raster().is_err());
        let map = Mapping2::Projective(projective);
        assert!(MappingData::from_raster_mask(map, ComponentOwner::Other).is_err());
        for adapter in [
            MappingData::from_raster_mask(map, ComponentOwner::Smart).unwrap(),
            MappingData::from_filter_mask(map).unwrap(),
        ] {
            assert_eq!(serde_json::to_vec(&adapter).unwrap(), placement_bytes);
            let restored: MappingData = serde_json::from_slice(&placement_bytes).unwrap();
            assert!(matches!(
                restored.into_raster_mask(ComponentOwner::Smart).unwrap(),
                Mapping2::Projective(_)
            ));
            assert!(matches!(
                restored.into_filter_mask().unwrap(),
                Mapping2::Projective(_)
            ));
            assert!(restored.into_raster_mask(ComponentOwner::Other).is_err());
            assert!(restored.into_vector_mask().is_err());
        }
    }
    assert_eq!(
        serde_json::to_string(
            &PlacementData::from_smart(SmartPlacement::Projective(Projective2::IDENTITY)).unwrap()
        )
        .unwrap(),
        PROJECTIVE_IDENTITY
    );
}

fn assert_projective_roundtrips(projective: Projective2) {
    let expected_bits = projective.to_row_major().map(f64::to_bits);
    let expected_bytes = format!(
        r#"{{"projective":{}}}"#,
        serde_json::to_string(&projective.to_row_major()).unwrap()
    )
    .into_bytes();
    let mut placement = PlacementData::from_smart(SmartPlacement::Projective(projective)).unwrap();
    let mut mapping = MappingData::from_filter_mask(Mapping2::Projective(projective)).unwrap();
    for _ in 0..4 {
        assert_eq!(serde_json::to_vec(&placement).unwrap(), expected_bytes);
        assert_eq!(serde_json::to_vec(&mapping).unwrap(), expected_bytes);
        placement = serde_json::from_slice(&expected_bytes).unwrap();
        mapping = serde_json::from_slice(&expected_bytes).unwrap();
        let SmartPlacement::Projective(restored) = placement.into_smart() else {
            panic!("projective placement marker disappeared");
        };
        assert_eq!(restored.to_row_major().map(f64::to_bits), expected_bits);
        let Mapping2::Projective(restored) = mapping.into_filter_mask().unwrap() else {
            panic!("projective component marker disappeared");
        };
        assert_eq!(restored.to_row_major().map(f64::to_bits), expected_bits);
    }
}

#[test]
fn canonical_projective_writer_read_is_bit_idempotent_for_nontrivial_f64s() {
    // The workspace already enables serde_json/float_roundtrip. In particular,
    // these values test decimal parsing and canonicalization, not only integers.
    let raw = [
        f64::from_bits(0x3fe8_0000_0000_0001),
        f64::from_bits(0x3fb9_9999_9999_999a),
        0.3333333333333333,
        -0.14285714285714285,
        f64::from_bits(0x3fe6_6666_6666_6667),
        -0.375,
        f64::from_bits(0x3f50_624d_d2f1_a9fd),
        -0.002718281828459045,
        1.0,
    ];
    let base = Projective2::from_row_major(raw).unwrap();
    for shift in 0..64 {
        let mut varied = raw;
        varied[0] = f64::from_bits(raw[0].to_bits() + shift);
        varied[7] = f64::from_bits(raw[7].to_bits() + shift);
        let projective = Projective2::from_row_major(varied).unwrap();
        assert_projective_roundtrips(projective);
    }
    for projective in [
        base,
        base.inverse().unwrap(),
        base.compose(base).unwrap(),
        Projective2::from_row_major(raw.map(|v| -v * 16.0)).unwrap(),
        Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1e-20, 0.0, 1.0]).unwrap(),
        Projective2::from_row_major([0.0, -0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0]).unwrap(),
        Projective2::from_row_major([-2.0, 0.125, 2.0, 0.0, -2.0, -0.0, 0.015625, 0.0, 1.0])
            .unwrap(),
    ] {
        assert_projective_roundtrips(projective);
    }
}

#[test]
fn raw_projective_input_is_canonicalized_with_existing_api_not_bottom_right() {
    for raw in [
        [2.0, -0.0, 4.0, 0.0, 1.0, -2.0, 0.25, 0.0, 1.0],
        [-2.0, 0.0, -4.0, -0.0, -1.0, 2.0, -0.25, -0.0, -1.0],
        [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0],
        // Valid raw subnormal scale canonicalizes to identity; admission is
        // delegated to Projective2 rather than imposing a new scalar cutoff.
        Projective2::IDENTITY
            .to_row_major()
            .map(|v| v * f64::from_bits(16)),
    ] {
        let expected = Projective2::from_row_major(raw).unwrap();
        let json = format!(
            r#"{{"projective":{}}}"#,
            serde_json::to_string(&raw).unwrap()
        );
        let placement: PlacementData = serde_json::from_str(&json).unwrap();
        let mapping: MappingData = serde_json::from_str(&json).unwrap();
        let SmartPlacement::Projective(actual) = placement.into_smart() else {
            panic!("wrong variant")
        };
        assert_eq!(
            actual.to_row_major().map(f64::to_bits),
            expected.to_row_major().map(f64::to_bits)
        );
        let Mapping2::Projective(actual) = mapping.into_filter_mask().unwrap() else {
            panic!("wrong variant")
        };
        assert_eq!(
            actual.to_row_major().map(f64::to_bits),
            expected.to_row_major().map(f64::to_bits)
        );
        assert_projective_roundtrips(actual);
    }
}
