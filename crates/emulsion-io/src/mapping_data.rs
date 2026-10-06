//! Strict, IO-only mapping shapes for the coordinated native/history cutover.
//!
//! Native/history v16 share these strict, variant-preserving shapes. Parsing
//! proves representation only; callers must
//! use the appropriate role conversion before resources or candidate publication.

use crate::{IoError, Result};
use emulsion_core::mapping::{Mapping2, SmartPlacement};
use emulsion_raster::{Placement, projective::Projective2};
use glam::DAffine2;
use serde::de::{Error, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

const LEGACY_FIELDS: &[&str] = &[
    "x", "y", "scale_x", "scale_y", "rotation", "flip_x", "flip_y",
];
const PLACEMENT_FIELDS: &[&str] = &[
    "x",
    "y",
    "scale_x",
    "scale_y",
    "rotation",
    "flip_x",
    "flip_y",
    "projective",
];

/// Placement metadata, deliberately without `Default` or an implicit fallback.
/// All seven legacy object fields remain required, including the booleans.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlacementData(SmartPlacement);

impl PlacementData {
    pub(crate) fn is_projective(self) -> bool {
        matches!(self.0, SmartPlacement::Projective(_))
    }

    pub(crate) fn from_smart(placement: SmartPlacement) -> Result<Self> {
        if let SmartPlacement::Legacy(legacy) = placement {
            validate_placement(legacy)?;
        }
        Ok(Self(placement))
    }

    pub(crate) fn from_raster(placement: Placement) -> Result<Self> {
        Self::from_smart(SmartPlacement::Legacy(placement))
    }

    pub(crate) fn into_smart(self) -> SmartPlacement {
        self.0
    }

    /// Even an identity/affine-representable projective marker is forbidden.
    pub(crate) fn into_raster(self) -> Result<Placement> {
        match self.0 {
            SmartPlacement::Legacy(placement) => Ok(placement),
            SmartPlacement::Projective(_) => Err(bad("Raster placement must be legacy")),
        }
    }
}

/// The owner matters even when its raster-mask plane is absent or disabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComponentOwner {
    Smart,
    Other,
}

/// Component metadata. The parser applies raster-mask compatibility admission;
/// filter/vector conversions additionally apply their stronger legacy predicate.
/// This is never a document, sampling-domain or resource certificate.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MappingData(Mapping2);

impl Default for MappingData {
    fn default() -> Self {
        Self(Mapping2::IDENTITY)
    }
}

impl MappingData {
    pub(crate) fn is_projective(self) -> bool {
        matches!(self.0, Mapping2::Projective(_))
    }

    pub(crate) fn from_raster_mask(mapping: Mapping2, owner: ComponentOwner) -> Result<Self> {
        let data = Self(mapping);
        data.into_raster_mask(owner)?;
        Ok(data)
    }

    pub(crate) fn into_raster_mask(self, owner: ComponentOwner) -> Result<Mapping2> {
        match self.0 {
            Mapping2::Affine(affine) => validate_raster_affine(affine)?,
            Mapping2::Projective(_) if owner == ComponentOwner::Other => {
                return Err(bad(
                    "projective raster-mask metadata requires a Smart owner",
                ));
            }
            Mapping2::Projective(_) => {}
        }
        Ok(self.0)
    }

    /// This descriptor belongs only inside a Smart kind; schema-position and
    /// vector-mask coexistence admission remain the enclosing reader's duties.
    pub(crate) fn from_filter_mask(mapping: Mapping2) -> Result<Self> {
        let data = Self(mapping);
        data.into_filter_mask()?;
        Ok(data)
    }

    pub(crate) fn into_filter_mask(self) -> Result<Mapping2> {
        if let Mapping2::Affine(affine) = self.0 {
            validate_filter_vector_affine(affine)?;
        }
        Ok(self.0)
    }

    pub(crate) fn from_vector_mask(columns: [f64; 6]) -> Result<Self> {
        let data = Self(Mapping2::Affine(DAffine2::from_cols_array(&columns)));
        data.into_vector_mask()?;
        Ok(data)
    }

    pub(crate) fn into_vector_mask(self) -> Result<[f64; 6]> {
        match self.0 {
            Mapping2::Affine(affine) => {
                validate_filter_vector_affine(affine)?;
                Ok(affine.to_cols_array())
            }
            Mapping2::Projective(_) => Err(bad("vector-mask transform must be affine")),
        }
    }
}

fn bad(message: &str) -> IoError {
    IoError::Manifest(message.into())
}

fn validate_placement(p: Placement) -> Result<()> {
    // Preserve Document::validate's existing predicate, without promotion or
    // scale sign normalization. It intentionally does not evaluate to_doc.
    if ![p.x, p.y, p.scale_x, p.scale_y, p.rotation]
        .iter()
        .all(|v| v.is_finite())
        || p.scale_x.abs() < 1e-6
        || p.scale_y.abs() < 1e-6
    {
        return Err(bad("invalid legacy placement"));
    }
    Ok(())
}

fn validate_raster_affine(affine: DAffine2) -> Result<()> {
    // Preserve the exact legacy Document predicate. Do not add finite inverse,
    // finite determinant or projective operation checks here: huge finite
    // legacy coefficients can overflow their computed determinant and remain
    // accepted by that predicate. Filter/vector roles have a stronger gate.
    if !affine.to_cols_array().iter().all(|v| v.is_finite())
        || affine.matrix2.determinant().abs() < 1e-12
    {
        return Err(bad("invalid legacy raster-mask affine"));
    }
    Ok(())
}

fn validate_filter_vector_affine(affine: DAffine2) -> Result<()> {
    // Same transform predicate as FilterMaskData::validate and
    // vector_mask::valid_transform, retaining the original arithmetic/order.
    let determinant = affine.matrix2.determinant();
    if !affine.to_cols_array().iter().all(|v| v.is_finite())
        || !determinant.is_finite()
        || determinant.abs() < 1e-12
        || !affine
            .inverse()
            .to_cols_array()
            .iter()
            .all(|v| v.is_finite())
        || ![-1e9, 1e9].into_iter().all(|x| {
            [-1e9, 1e9].into_iter().all(|y| {
                affine.transform_point2(glam::dvec2(x, y)).is_finite()
                    && affine
                        .inverse()
                        .transform_point2(glam::dvec2(x, y))
                        .is_finite()
            })
        })
    {
        return Err(bad("invalid legacy filter/vector-mask affine"));
    }
    Ok(())
}

impl Serialize for PlacementData {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self.0 {
            // Delegate the old writer: order, float formatting and signed zero
            // remain exactly those of Placement, without an enum tag.
            SmartPlacement::Legacy(placement) => placement.serialize(serializer),
            SmartPlacement::Projective(projective) => write_projective(projective, serializer),
        }
    }
}

impl Serialize for MappingData {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self.0 {
            Mapping2::Affine(affine) => affine.to_cols_array().serialize(serializer),
            Mapping2::Projective(projective) => write_projective(projective, serializer),
        }
    }
}

fn write_projective<S: Serializer>(
    projective: Projective2,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(1))?;
    map.serialize_entry("projective", &projective.to_row_major())?;
    map.end()
}

/// Strict numeric scalar: JSON null/string/bool and nonfinite non-JSON values
/// never become a default, zero, or an omitted projective marker.
struct Finite(f64);

impl<'de> Deserialize<'de> for Finite {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Number;
        impl Visitor<'_> for Number {
            type Value = Finite;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a finite number")
            }
            fn visit_f64<E: Error>(self, value: f64) -> std::result::Result<Finite, E> {
                if value.is_finite() {
                    Ok(Finite(value))
                } else {
                    Err(E::custom("mapping coefficient must be finite"))
                }
            }
            fn visit_i64<E: Error>(self, value: i64) -> std::result::Result<Finite, E> {
                self.visit_f64(value as f64)
            }
            fn visit_u64<E: Error>(self, value: u64) -> std::result::Result<Finite, E> {
                self.visit_f64(value as f64)
            }
        }
        deserializer.deserialize_f64(Number)
    }
}

struct Coefficients<const N: usize>([f64; N]);

impl<'de, const N: usize> Deserialize<'de> for Coefficients<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Numbers<const N: usize>;
        impl<'de, const N: usize> Visitor<'de> for Numbers<N> {
            type Value = Coefficients<N>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "exactly {N} finite numbers")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut coefficients = [0.0; N];
                for (i, value) in coefficients.iter_mut().enumerate() {
                    *value = seq
                        .next_element::<Finite>()?
                        .ok_or_else(|| A::Error::invalid_length(i, &self))?
                        .0;
                }
                if seq.next_element::<IgnoredAny>()?.is_some() {
                    return Err(A::Error::invalid_length(N + 1, &self));
                }
                Ok(Coefficients(coefficients))
            }
        }
        deserializer.deserialize_seq(Numbers::<N>)
    }
}

fn read_field<'de, A: MapAccess<'de>, T: Deserialize<'de>>(
    map: &mut A,
    field: &mut Option<T>,
    name: &'static str,
) -> std::result::Result<(), A::Error> {
    if field.is_some() {
        return Err(A::Error::duplicate_field(name));
    }
    *field = Some(map.next_value()?);
    Ok(())
}

impl<'de> Deserialize<'de> for PlacementData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct PlacementVisitor;
        impl<'de> Visitor<'de> for PlacementVisitor {
            type Value = PlacementData;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a seven-field legacy placement or an exclusive projective object")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<PlacementData, A::Error> {
                let (mut x, mut y, mut scale_x, mut scale_y, mut rotation) = (
                    None::<Finite>,
                    None::<Finite>,
                    None::<Finite>,
                    None::<Finite>,
                    None::<Finite>,
                );
                let (mut flip_x, mut flip_y) = (None::<bool>, None::<bool>);
                let mut projective = None::<Coefficients<9>>;
                let mut legacy = false;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "projective" {
                        if legacy {
                            return Err(A::Error::custom(
                                "projective and legacy placement fields conflict",
                            ));
                        }
                        read_field(&mut map, &mut projective, "projective")?;
                        continue;
                    }
                    if !LEGACY_FIELDS.contains(&key.as_str()) {
                        return Err(A::Error::unknown_field(&key, PLACEMENT_FIELDS));
                    }
                    if projective.is_some() {
                        return Err(A::Error::custom(
                            "projective and legacy placement fields conflict",
                        ));
                    }
                    legacy = true;
                    match key.as_str() {
                        "x" => read_field(&mut map, &mut x, "x")?,
                        "y" => read_field(&mut map, &mut y, "y")?,
                        "scale_x" => read_field(&mut map, &mut scale_x, "scale_x")?,
                        "scale_y" => read_field(&mut map, &mut scale_y, "scale_y")?,
                        "rotation" => read_field(&mut map, &mut rotation, "rotation")?,
                        "flip_x" => read_field(&mut map, &mut flip_x, "flip_x")?,
                        "flip_y" => read_field(&mut map, &mut flip_y, "flip_y")?,
                        _ => unreachable!("legacy field membership checked above"),
                    }
                }
                if let Some(coefficients) = projective {
                    return Projective2::from_row_major(coefficients.0)
                        .map(|p| PlacementData(SmartPlacement::Projective(p)))
                        .map_err(A::Error::custom);
                }
                let placement = Placement {
                    x: x.ok_or_else(|| A::Error::missing_field("x"))?.0,
                    y: y.ok_or_else(|| A::Error::missing_field("y"))?.0,
                    scale_x: scale_x.ok_or_else(|| A::Error::missing_field("scale_x"))?.0,
                    scale_y: scale_y.ok_or_else(|| A::Error::missing_field("scale_y"))?.0,
                    rotation: rotation
                        .ok_or_else(|| A::Error::missing_field("rotation"))?
                        .0,
                    flip_x: flip_x.ok_or_else(|| A::Error::missing_field("flip_x"))?,
                    flip_y: flip_y.ok_or_else(|| A::Error::missing_field("flip_y"))?,
                };
                PlacementData::from_raster(placement).map_err(A::Error::custom)
            }
        }
        deserializer.deserialize_map(PlacementVisitor)
    }
}

impl<'de> Deserialize<'de> for MappingData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct MappingVisitor;
        impl<'de> Visitor<'de> for MappingVisitor {
            type Value = MappingData;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("six finite affine columns or an exclusive projective object")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                seq: A,
            ) -> std::result::Result<MappingData, A::Error> {
                let columns = Coefficients::<6>::deserialize(
                    serde::de::value::SeqAccessDeserializer::new(seq),
                )?
                .0;
                let affine = DAffine2::from_cols_array(&columns);
                validate_raster_affine(affine).map_err(A::Error::custom)?;
                Ok(MappingData(Mapping2::Affine(affine)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<MappingData, A::Error> {
                let mut projective = None::<Coefficients<9>>;
                while let Some(key) = map.next_key::<String>()? {
                    if key != "projective" {
                        return Err(A::Error::unknown_field(&key, &["projective"]));
                    }
                    read_field(&mut map, &mut projective, "projective")?;
                }
                let coefficients =
                    projective.ok_or_else(|| A::Error::missing_field("projective"))?;
                Projective2::from_row_major(coefficients.0)
                    .map(|p| MappingData(Mapping2::Projective(p)))
                    .map_err(A::Error::custom)
            }
        }
        deserializer.deserialize_any(MappingVisitor)
    }
}

#[cfg(test)]
#[path = "mapping_data_tests.rs"]
mod tests;
