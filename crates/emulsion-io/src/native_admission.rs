//! Bounded schema-position admission before enum decoding can ignore fields.
//! This guards recognized native metadata, not arbitrary future unknown fields.
use crate::{IoError, NativeFailureCode, Result};
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;
use std::io::{Read, Seek};

#[derive(Clone, Debug)]
pub(crate) enum Retention {
    KnownLegacy,
    MustPreserve,
    Indeterminate,
}
#[derive(Clone, Debug)]
pub(crate) struct ArchiveFacts {
    pub retention: Retention,
    pub standalone_strict: bool,
    pub native_version: Option<u32>,
}
impl ArchiveFacts {
    pub fn protected(&self) -> bool {
        !matches!(self.retention, Retention::KnownLegacy)
    }
}
pub(crate) struct Inspection {
    pub facts: ArchiveFacts,
    pub error: Option<IoError>,
}
#[derive(Deserialize)]
struct Header {
    format: String,
    version: u32,
}
#[derive(Deserialize)]
struct Version {
    version: u32,
}

#[derive(Default)]
struct Evidence {
    protected: bool,
    unknown_kind: bool,
    uncertain_shape: bool,
    source: bool,
    original: bool,
    profile: bool,
    invert: bool,
    disabled: bool,
    projective: bool,
    error: Option<IoError>,
}
impl Evidence {
    fn fail(&mut self, code: NativeFailureCode, location: &str, detail: &str) {
        if self.error.is_none() {
            self.error = Some(IoError::NativePreservation {
                code,
                location: location.into(),
                detail: detail.into(),
            });
        }
    }
}
#[derive(Clone, Copy)]
enum Scope {
    Live,
    History,
    Snapshot,
    Working,
    Doc,
    Node,
    Kind,
    Editable,
    FilterMask,
    VectorMask,
    Filter,
    FilterStyle,
}
struct Probe<'a> {
    scope: Scope,
    history: bool,
    location: String,
    evidence: &'a mut Evidence,
}
impl Probe<'_> {
    fn unexpected_shape(&mut self) {
        self.evidence.uncertain_shape = true;
        if matches!(self.scope, Scope::Kind | Scope::Node) {
            self.evidence.unknown_kind = true;
        }
    }
}
impl<'de> DeserializeSeed<'de> for Probe<'_> {
    type Value = (Option<String>, bool, bool);
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Probe<'_> {
    type Value = (Option<String>, bool, bool);
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("native metadata object")
    }
    fn visit_unit<E: serde::de::Error>(mut self) -> std::result::Result<Self::Value, E> {
        if !matches!(
            self.scope,
            Scope::Working | Scope::Editable | Scope::FilterMask | Scope::VectorMask
        ) {
            self.unexpected_shape();
        }
        Ok((None, false, false))
    }
    fn visit_bool<E: serde::de::Error>(mut self, _: bool) -> std::result::Result<Self::Value, E> {
        self.unexpected_shape();
        Ok((None, false, false))
    }
    fn visit_i64<E: serde::de::Error>(mut self, _: i64) -> std::result::Result<Self::Value, E> {
        self.unexpected_shape();
        Ok((None, false, false))
    }
    fn visit_u64<E: serde::de::Error>(mut self, _: u64) -> std::result::Result<Self::Value, E> {
        self.unexpected_shape();
        Ok((None, false, false))
    }
    fn visit_f64<E: serde::de::Error>(mut self, _: f64) -> std::result::Result<Self::Value, E> {
        self.unexpected_shape();
        Ok((None, false, false))
    }
    fn visit_str<E: serde::de::Error>(mut self, _: &str) -> std::result::Result<Self::Value, E> {
        self.unexpected_shape();
        Ok((None, false, false))
    }
    fn visit_seq<A: SeqAccess<'de>>(
        mut self,
        mut seq: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        self.unexpected_shape();
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok((None, false, false))
    }
    fn visit_map<A: MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        let mut kind = None;
        let mut smart_fields = false;
        let mut source_ref = false;
        let mut source_invalid = false;
        let mut editable = false;
        let mut editable_document = false;
        let mut archive_empty = false;
        let mut original = false;
        let mut projective = false;
        let mut vector_mask = false;
        let mut keys = std::collections::HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            // Only known relevant keys are remembered; large unrelated objects
            // and plane/geometry tables are consumed with IgnoredAny.
            let relevant = matches!(
                key.as_str(),
                "type"
                    | "source_document"
                    | "editable"
                    | "original_image"
                    | "filters"
                    | "filter_styles"
                    | "filters_enabled"
                    | "filter_mask"
                    | "source"
                    | "cache"
                    | "offset"
                    | "placement"
                    | "mask_transform"
                    | "transform"
                    | "vector_mask"
                    | "projective"
                    | "blend_space"
                    | "psd_background"
                    | "enabled"
                    | "archive"
                    | "kind"
                    | "nodes"
                    | "commits"
                    | "working"
                    | "doc"
            );
            if relevant && !keys.insert(key.clone()) {
                self.evidence.protected = true;
                self.evidence.fail(
                    NativeFailureCode::ProtectedFieldWrongPosition,
                    &self.location,
                    "duplicate guarded native metadata is ambiguous",
                );
                return Err(serde::de::Error::custom(format!(
                    "duplicate native field {}.{key}",
                    self.location
                )));
            }
            let child_location = if relevant {
                format!("{}.{key}", self.location)
            } else {
                self.location.clone()
            };
            let smart_field = matches!(
                key.as_str(),
                "editable"
                    | "source_document"
                    | "original_image"
                    | "filters"
                    | "filter_styles"
                    | "filters_enabled"
                    | "filter_mask"
            ) || (self.history
                && matches!(key.as_str(), "source" | "cache" | "offset"));
            let protected_name = matches!(
                key.as_str(),
                "source_document" | "original_image" | "filters_enabled" | "projective"
            );
            if protected_name {
                self.evidence.protected = true;
            }
            if smart_field && matches!(self.scope, Scope::Kind) {
                smart_fields = true;
            }
            if smart_field && !matches!(self.scope, Scope::Kind) {
                self.evidence.protected = true;
                self.evidence.fail(
                    NativeFailureCode::ProtectedFieldWrongPosition,
                    &child_location,
                    "editable Smart metadata occurs outside its kind",
                );
            }
            if (matches!(key.as_str(), "psd_background" | "blend_space")
                && !matches!(self.scope, Scope::Live | Scope::Doc))
                || (key == "enabled"
                    && !matches!(
                        self.scope,
                        Scope::FilterStyle | Scope::FilterMask | Scope::VectorMask
                    ))
            {
                self.evidence.protected = true;
                self.evidence.fail(
                    NativeFailureCode::ProtectedFieldWrongPosition,
                    &child_location,
                    "protected metadata occurs outside its defined schema position",
                );
            }
            if key == "projective" {
                self.evidence.projective = true;
                self.evidence.fail(
                    NativeFailureCode::ProtectedFieldWrongPosition,
                    &child_location,
                    "projective metadata must occur in an exclusive mapping descriptor",
                );
            }
            if (key == "placement" && !matches!(self.scope, Scope::Kind))
                || (key == "mask_transform" && !matches!(self.scope, Scope::Node))
                || (key == "vector_mask" && !matches!(self.scope, Scope::Node))
                || (key == "transform"
                    && !matches!(self.scope, Scope::FilterMask | Scope::VectorMask))
            {
                self.evidence.protected = true;
                self.evidence.fail(
                    NativeFailureCode::ProtectedFieldWrongPosition,
                    &child_location,
                    "mapping metadata occurs outside its defined schema position",
                );
            }
            match (self.scope, key.as_str()) {
                (Scope::History, "commits") => {
                    map.next_value_seed(Items {
                        scope: Scope::Snapshot,
                        history: true,
                        location: child_location,
                        evidence: self.evidence,
                    })?;
                }
                (Scope::History, "working") | (Scope::Snapshot | Scope::Working, "doc") => {
                    let scope = if key == "working" {
                        Scope::Working
                    } else {
                        Scope::Doc
                    };
                    map.next_value_seed(Probe {
                        scope,
                        history: true,
                        location: child_location,
                        evidence: self.evidence,
                    })?;
                }
                (Scope::Live | Scope::Doc, "nodes") => {
                    map.next_value_seed(Items {
                        scope: Scope::Node,
                        history: self.history,
                        location: child_location,
                        evidence: self.evidence,
                    })?;
                }
                (Scope::Node, "kind") => {
                    let value = map.next_value_seed(Probe {
                        scope: Scope::Kind,
                        history: self.history,
                        location: child_location,
                        evidence: self.evidence,
                    })?;
                    kind = value.0;
                    projective |= value.2;
                }
                (Scope::Kind | Scope::Editable, "type") => {
                    kind = map.next_value::<Option<String>>()?;
                }
                (Scope::Kind, "source_document") => {
                    source_ref = true;
                    self.evidence.source = true;
                    // SourcePool checks the reference and resource, but null
                    // must not disappear as an absent descriptor here.
                    let reference = map.next_value::<Option<String>>()?;
                    source_invalid = reference.is_none();
                }
                (Scope::Kind, "editable") => {
                    let value = map.next_value_seed(Probe {
                        scope: Scope::Editable,
                        history: self.history,
                        location: child_location,
                        evidence: self.evidence,
                    })?;
                    editable = value.0.is_some();
                    editable_document = value.0.as_deref() == Some("document");
                    archive_empty = value.1;
                }
                (Scope::Editable, "archive") => {
                    archive_empty = map.next_value::<EmptyArchive>()?.0;
                }
                (Scope::Kind, "original_image") => {
                    original = map.next_value::<Option<IgnoredAny>>()?.is_some();
                    self.evidence.original |= original;
                }
                (Scope::Live | Scope::Doc, "blend_space") => {
                    let value = map.next_value::<Option<String>>()?;
                    if value.as_deref() == Some("photoshop-srgb-v1") {
                        self.evidence.profile = true;
                        self.evidence.protected = true;
                    }
                }
                (Scope::Live | Scope::Doc, "psd_background") => {
                    if map.next_value::<Option<IgnoredAny>>()?.is_some() {
                        self.evidence.profile = true;
                        self.evidence.protected = true;
                    }
                }
                (Scope::Kind, "filters_enabled") | (Scope::FilterStyle, "enabled") => {
                    self.evidence.protected = true;
                    if !map.next_value::<bool>()? {
                        self.evidence.disabled = true;
                    }
                }
                (Scope::Kind, "filters") | (Scope::Kind, "filter_styles") => {
                    let scope = if key == "filters" {
                        Scope::Filter
                    } else {
                        Scope::FilterStyle
                    };
                    map.next_value_seed(Items {
                        scope,
                        history: self.history,
                        location: child_location,
                        evidence: self.evidence,
                    })?;
                }
                (Scope::Filter, "kind") => {
                    if map.next_value::<Option<String>>()?.as_deref() == Some("invert") {
                        self.evidence.invert = true;
                        self.evidence.protected = true;
                    }
                }
                (Scope::Kind, "placement") => {
                    let mapping = map.next_value::<crate::mapping_data::PlacementData>()?;
                    projective |= mapping.is_projective();
                    self.evidence.projective |= mapping.is_projective();
                    self.evidence.protected |= mapping.is_projective();
                }
                (Scope::Node, "mask_transform")
                | (Scope::FilterMask | Scope::VectorMask, "transform") => {
                    let mapping = map.next_value::<crate::mapping_data::MappingData>()?;
                    projective |= mapping.is_projective();
                    self.evidence.projective |= mapping.is_projective();
                    self.evidence.protected |= mapping.is_projective();
                    if matches!(self.scope, Scope::VectorMask) && mapping.is_projective() {
                        self.evidence.fail(
                            NativeFailureCode::ProtectedFieldWrongKind,
                            &child_location,
                            "vector masks cannot contain projective mappings",
                        );
                    }
                    match self.scope {
                        Scope::FilterMask => {
                            mapping
                                .into_filter_mask()
                                .map_err(serde::de::Error::custom)?;
                        }
                        Scope::VectorMask => {
                            mapping
                                .into_vector_mask()
                                .map_err(serde::de::Error::custom)?;
                        }
                        _ => {}
                    }
                }
                (Scope::Kind, "filter_mask") | (Scope::Node, "vector_mask") => {
                    let value = map.next_value_seed(Probe {
                        scope: if key == "filter_mask" {
                            Scope::FilterMask
                        } else {
                            Scope::VectorMask
                        },
                        history: self.history,
                        location: child_location,
                        evidence: self.evidence,
                    })?;
                    projective |= value.2;
                    vector_mask |= key == "vector_mask" && value.1;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        if (matches!(self.scope, Scope::Live | Scope::Doc) && !keys.contains("nodes"))
            || (matches!(self.scope, Scope::Snapshot | Scope::Working) && !keys.contains("doc"))
            || (matches!(self.scope, Scope::History) && !keys.contains("commits"))
        {
            self.evidence.uncertain_shape = true;
        }
        if matches!(self.scope, Scope::Node) && !keys.contains("kind") {
            self.evidence.uncertain_shape = true;
            self.evidence.unknown_kind = true;
        }
        if matches!(self.scope, Scope::Editable)
            && !matches!(kind.as_deref(), Some("document" | "svg" | "text" | "path"))
        {
            self.evidence.uncertain_shape = true;
        }
        if matches!(self.scope, Scope::Editable) && kind.as_deref() == Some("document") {
            self.evidence.source = true;
            self.evidence.protected = true;
        }
        if matches!(self.scope, Scope::Kind) {
            match kind.as_deref() {
                Some("smart") => {
                    if source_invalid
                        || source_ref != editable_document
                        || (editable_document && !archive_empty)
                        || (original && editable)
                    {
                        self.evidence.fail(NativeFailureCode::ConflictingSourceDescriptors, &self.location, "Smart source descriptors are incompatible or lack the empty referenced archive placeholder");
                    }
                }
                Some("raster" | "group" | "adjust" | "fill" | "path" | "text" | "strokes") => {
                    if smart_fields {
                        self.evidence.protected = true;
                        self.evidence.fail(
                            NativeFailureCode::ProtectedFieldWrongKind,
                            &self.location,
                            "a recognized non-Smart kind contains Smart-only fields",
                        );
                    }
                }
                _ => self.evidence.unknown_kind = true,
            }
        }
        if projective
            && matches!(self.scope, Scope::Node | Scope::Kind)
            && kind.as_deref() != Some("smart")
        {
            self.evidence.fail(
                NativeFailureCode::ProtectedFieldWrongKind,
                &self.location,
                "projective placement and component mappings require a Smart owner",
            );
        }
        if projective && vector_mask {
            self.evidence.fail(
                NativeFailureCode::ProtectedFieldWrongKind,
                &self.location,
                "vector masks cannot coexist with retained projective Smart metadata",
            );
        }
        Ok((
            kind,
            archive_empty || matches!(self.scope, Scope::FilterMask | Scope::VectorMask),
            projective,
        ))
    }
}
struct Items<'a> {
    scope: Scope,
    history: bool,
    location: String,
    evidence: &'a mut Evidence,
}
impl<'de> DeserializeSeed<'de> for Items<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Items<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("native metadata list")
    }
    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
        self.evidence.uncertain_shape = true;
        Ok(())
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> std::result::Result<(), E> {
        self.evidence.uncertain_shape = true;
        Ok(())
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<(), E> {
        self.evidence.uncertain_shape = true;
        Ok(())
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<(), E> {
        self.evidence.uncertain_shape = true;
        Ok(())
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<(), E> {
        self.evidence.uncertain_shape = true;
        Ok(())
    }
    fn visit_str<E: serde::de::Error>(self, _: &str) -> std::result::Result<(), E> {
        self.evidence.uncertain_shape = true;
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        self.evidence.uncertain_shape = true;
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        let mut index = 0usize;
        while seq
            .next_element_seed(Probe {
                scope: self.scope,
                history: self.history,
                location: format!("{}[{index}]", self.location),
                evidence: self.evidence,
            })?
            .is_some()
        {
            index += 1;
        }
        Ok(())
    }
}
struct EmptyArchive(bool);
impl<'de> Deserialize<'de> for EmptyArchive {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Empty;
        impl<'de> Visitor<'de> for Empty {
            type Value = EmptyArchive;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("empty archive placeholder")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut empty = true;
                while seq.next_element::<IgnoredAny>()?.is_some() {
                    empty = false;
                }
                Ok(EmptyArchive(empty))
            }
        }
        d.deserialize_seq(Empty)
    }
}

fn entry<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<Option<Vec<u8>>> {
    match zip.by_name(name) {
        Ok(_) => {}
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    crate::ora::read_entry(zip, name, crate::ora::MAX_NATIVE_MANIFEST_BYTES).map(Some)
}

/// Inspect both headers before returning any live semantic error. Failed probes
/// are never affirmative legacy evidence. Reserved resource bytes stay unopened.
pub(crate) fn inspect_archive<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> Inspection {
    let reserved = zip.file_names().any(|name| {
        name.starts_with("sources/")
            || name.starts_with("history/sources/")
            || name.starts_with("original-images/")
    });
    let mut evidence = Evidence::default();
    let mut error = None;
    let mut valid = true;
    let mut versions = [None, None];
    let mut history_exists = false;
    for (index, name) in ["emulsion.json", crate::history::GRAPH]
        .into_iter()
        .enumerate()
    {
        let bytes = match entry(zip, name) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                if index == 0 {
                    valid = false;
                }
                continue;
            }
            Err(cause) => {
                valid = false;
                if error.is_none() {
                    error = Some(cause);
                }
                continue;
            }
        };
        history_exists |= index == 1;
        let header = serde_json::from_slice::<Header>(&bytes);
        // Future schemas need not have today's format or document fields. Probe
        // their version independently, without treating it as valid identity or
        // affirmative legacy evidence. Keep the first TooNew ahead of any
        // earlier mapping/metadata error, matching the full history reader.
        let version = match &header {
            Ok(header) => Some(header.version),
            Err(_) => serde_json::from_slice::<Version>(&bytes)
                .ok()
                .map(|header| header.version),
        };
        let supported = if index == 0 {
            crate::ora::FORMAT_VERSION
        } else {
            crate::history::HISTORY_VERSION
        };
        if let Some(version) = version
            && version > supported
        {
            evidence.protected = true;
            if !matches!(error, Some(IoError::TooNew(_))) {
                error = Some(IoError::TooNew(version));
            }
        }
        match header {
            Ok(header)
                if header.version > 0
                    && header.format
                        == if index == 0 {
                            "emulsion"
                        } else {
                            "emulsion-history"
                        } =>
            {
                versions[index] = Some(header.version);
                evidence.protected |= header.version >= 13;
            }
            Ok(_) => {
                valid = false;
            }
            Err(_) => {
                valid = false;
            }
        }
        let mut de = serde_json::Deserializer::from_slice(&bytes);
        let parsed = Probe {
            scope: if index == 0 {
                Scope::Live
            } else {
                Scope::History
            },
            history: index == 1,
            location: if index == 0 {
                "emulsion.json".into()
            } else {
                crate::history::GRAPH.into()
            },
            evidence: &mut evidence,
        }
        .deserialize(&mut de)
        .and_then(|_| de.end());
        if evidence.uncertain_shape {
            evidence.protected |= crate::native_features::has_feature(&bytes);
        }
        if let Err(cause) = parsed {
            valid = false;
            evidence.protected |= crate::native_features::has_feature(&bytes);
            if (index == 0 || evidence.protected) && error.is_none() {
                error = Some(IoError::Manifest(cause.to_string()));
            }
        }
    }
    let standalone_strict = evidence.protected || reserved;
    let min = versions[0].unwrap_or(0).min(if history_exists {
        versions[1].unwrap_or(0)
    } else {
        versions[0].unwrap_or(0)
    });
    if evidence.error.is_some() && !matches!(error, Some(IoError::TooNew(_))) {
        error = evidence.error;
    }
    if error.is_none() {
        let requirement = if evidence.projective {
            Some((16, "projective Smart mapping state"))
        } else if evidence.disabled {
            Some((15, "disabled Smart Filter state"))
        } else if evidence.profile || evidence.invert {
            Some((14, "Photoshop compositing or Invert state"))
        } else if evidence.original {
            Some((13, "OriginalImage state"))
        } else if evidence.source {
            Some((9, "referenced editable Smart sources"))
        } else {
            None
        };
        if let Some((version, what)) = requirement
            && min < version
        {
            error = Some(IoError::Manifest(format!(
                "{what} requires native and existing history version {version}"
            )));
        }
    }
    if error.is_none() && evidence.projective && !valid {
        error = Some(IoError::Manifest(
            "projective native/history headers or metadata are malformed".into(),
        ));
    }
    let retention = if standalone_strict {
        Retention::MustPreserve
    } else if valid && !evidence.unknown_kind && !evidence.uncertain_shape {
        Retention::KnownLegacy
    } else {
        Retention::Indeterminate
    };
    Inspection {
        facts: ArchiveFacts {
            retention,
            standalone_strict,
            native_version: versions[0],
        },
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn inspect(native: &[u8], history: Option<&[u8]>, reserved: bool) -> Inspection {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in std::iter::once(("emulsion.json", native))
            .chain(history.map(|bytes| (crate::history::GRAPH, bytes)))
            .chain(reserved.then_some((
                "sources/future.ora",
                b"opaque nested future source".as_slice(),
            )))
        {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        inspect_archive(
            &mut zip::ZipArchive::new(Cursor::new(writer.finish().unwrap().into_inner())).unwrap(),
        )
    }
    fn native(kind: serde_json::Value, version: u32) -> Vec<u8> {
        serde_json::to_vec(
            &serde_json::json!({"format":"emulsion", "version":version, "nodes":[{"kind":kind}]}),
        )
        .unwrap()
    }
    fn code(result: Inspection, expected: NativeFailureCode) {
        assert!(
            matches!(result.error, Some(IoError::NativePreservation { code, .. }) if code == expected)
        );
        assert!(result.facts.protected());
    }

    #[test]
    fn headers_require_positive_unambiguous_legacy_identity() {
        let legacy = br#"{"format":"emulsion","version":12,"nodes":[]}"#;
        assert!(matches!(
            inspect(legacy, None, false).facts.retention,
            Retention::KnownLegacy
        ));
        for header in [
            br#"{"format":"emulsion","nodes":[]}"#.as_slice(),
            br#"{"format":"emulsion","version":12,"version":12}"#,
            br#"{"format":"emulsion","version":4294967296}"#,
            br#"{"format":"emulsion","version":12.0}"#,
            br#"{"format":"other","version":12}"#,
            br#"{"format":"emulsion","version":12,"nodes":["#,
        ] {
            assert!(!matches!(
                inspect(header, None, false).facts.retention,
                Retention::KnownLegacy
            ));
        }
        let future = br#"{"format":"emulsion-history","version":17}"#;
        assert!(matches!(
            inspect(legacy, Some(future), false).error,
            Some(IoError::TooNew(17))
        ));
        let broken = br#"{"format":"emulsion-history","version":12,"broken":]}"#;
        assert!(matches!(
            inspect(legacy, Some(broken), false).facts.retention,
            Retention::Indeterminate
        ));
        assert!(matches!(
            inspect(legacy, Some(broken), true).facts.retention,
            Retention::MustPreserve
        ));
    }

    #[test]
    fn future_version_precedes_missing_or_changed_header_schema() {
        let legacy = br#"{"format":"emulsion","version":12,"nodes":[]}"#;
        let future_version = crate::ora::FORMAT_VERSION.max(crate::history::HISTORY_VERSION) + 1;
        for header in [
            serde_json::json!({"version":future_version}),
            serde_json::json!({"format":false,"version":future_version}),
            serde_json::json!({"format":"future-format","version":future_version}),
        ] {
            let header = serde_json::to_vec(&header).unwrap();
            for result in [
                inspect(&header, None, false),
                inspect(legacy, Some(&header), false),
            ] {
                assert!(matches!(result.error, Some(IoError::TooNew(v)) if v == future_version));
                assert!(matches!(result.facts.retention, Retention::MustPreserve));
                assert!(result.facts.standalone_strict);
            }
        }
        for header in [
            br#"{"version":12}"#.as_slice(),
            br#"{"version":16}"#,
            br#"{"version":17,"version":18}"#,
            br#"{"version":4294967296}"#,
            br#"{"version":17.0}"#,
            br#"{"version":"17"}"#,
            br#"{"version":17,"broken":]}"#,
        ] {
            for result in [
                inspect(header, None, false),
                inspect(legacy, Some(header), false),
            ] {
                assert!(!matches!(result.error, Some(IoError::TooNew(_))));
                assert!(!matches!(result.facts.retention, Retention::KnownLegacy));
                assert!(result.facts.protected());
            }
        }
    }

    #[test]
    fn newer_history_header_outweighs_an_earlier_strict_mapping_parse_error() {
        // Source-only regression: the live parser encounters these descriptors
        // before the history header. A newer supported-format identity must
        // still produce the typed TooNew cause, not the mapping parse error.
        for node in [
            serde_json::json!({"kind":{"type":"smart","placement":{"projective":null}}}),
            serde_json::json!({"kind":{"type":"smart"},"mask_transform":{"projective":null}}),
            serde_json::json!({"kind":{"type":"smart","filter_mask":{"transform":{"projective":null}}}}),
        ] {
            let live = serde_json::to_vec(&serde_json::json!({
                "format":"emulsion", "version":16, "nodes":[node]
            }))
            .unwrap();
            for (history, expected_newer) in [
                (br#"{"version":17}"#.as_slice(), true),
                (
                    br#"{"format":"emulsion-history","version":17,"commits":[]}"#.as_slice(),
                    true,
                ),
                (
                    br#"{"format":"emulsion-history","version":16,"commits":[]}"#.as_slice(),
                    false,
                ),
            ] {
                let result = inspect(&live, Some(history), false);
                assert!(matches!(result.facts.retention, Retention::MustPreserve));
                assert!(result.facts.standalone_strict);
                if expected_newer {
                    assert!(matches!(result.error, Some(IoError::TooNew(17))));
                } else {
                    assert!(matches!(result.error, Some(IoError::Manifest(_))));
                }
            }
        }
        let future_live = br#"{"format":"emulsion","version":17,"nodes":[{"kind":{"type":"smart","placement":{"projective":null}}}]}"#;
        let later_history = br#"{"format":"emulsion-history","version":18,"commits":[]}"#;
        assert!(matches!(
            inspect(future_live, Some(later_history), false).error,
            Some(IoError::TooNew(17))
        ));
        assert!(matches!(
            inspect(br#"{"version":17}"#, Some(br#"{"version":18}"#), false).error,
            Some(IoError::TooNew(17))
        ));
    }

    #[test]
    fn known_wrong_kind_is_distinct_from_unknown_kind_legacy_recovery() {
        for value in [serde_json::Value::Null, serde_json::json!([]), true.into()] {
            let bytes = native(
                serde_json::json!({"type":"fill", "rgba":[0,0,0,255], "filters":value}),
                12,
            );
            code(
                inspect(&bytes, None, false),
                NativeFailureCode::ProtectedFieldWrongKind,
            );
        }
        let bytes = native(serde_json::json!({"type":"unknown", "filters":[]}), 12);
        let result = inspect(&bytes, None, false);
        assert!(matches!(result.facts.retention, Retention::Indeterminate));
        assert!(!result.facts.standalone_strict);
    }

    #[test]
    fn protected_names_in_wrong_schema_positions_are_rejected() {
        for field in [
            "source_document",
            "original_image",
            "filters_enabled",
            "psd_background",
            "blend_space",
            "enabled",
        ] {
            for scope in ["document", "node", "mask"] {
                let mut value = serde_json::json!({"format":"emulsion", "version":12,"nodes":[{"kind":{"type":"fill"}}]});
                let target = match scope {
                    "document" => &mut value,
                    "node" => &mut value["nodes"][0],
                    _ => {
                        value["nodes"][0]["vector_mask"] = serde_json::json!({});
                        &mut value["nodes"][0]["vector_mask"]
                    }
                };
                if (field == "enabled" && scope == "mask")
                    || (scope == "document" && matches!(field, "psd_background" | "blend_space"))
                {
                    continue;
                }
                target[field] = serde_json::Value::Null;
                code(
                    inspect(&serde_json::to_vec(&value).unwrap(), None, false),
                    NativeFailureCode::ProtectedFieldWrongPosition,
                );
            }
        }
    }

    #[test]
    fn projective_mapping_shapes_and_owner_roles_admit_only_v16() {
        let identity = serde_json::json!({"projective":[1,0,0,0,1,0,0,0,1]});
        for role in ["placement", "raster-mask", "filter-mask"] {
            let mut node = serde_json::json!({"kind":{"type":"smart"}});
            match role {
                "placement" => node["kind"]["placement"] = identity.clone(),
                "raster-mask" => node["mask_transform"] = identity.clone(),
                _ => {
                    node["kind"]["filter_mask"] =
                        serde_json::json!({"enabled":false,"transform":identity})
                }
            }
            for version in [15, 16] {
                let live = serde_json::json!({"format":"emulsion","version":version,"nodes":[node.clone()]});
                let result = inspect(&serde_json::to_vec(&live).unwrap(), None, false);
                assert_eq!(result.error.is_none(), version == 16, "{role}");
                assert!(result.facts.protected());
            }
            for snapshot in ["commits", "working"] {
                for (live_version, history_version) in [(16, 16), (15, 16), (16, 15)] {
                    let live =
                        serde_json::json!({"format":"emulsion","version":live_version,"nodes":[]});
                    let mut history = serde_json::json!({"format":"emulsion-history","version":history_version,"commits":[]});
                    history[snapshot] = if snapshot == "commits" {
                        serde_json::json!([{"doc":{"nodes":[node.clone()]}}])
                    } else {
                        serde_json::json!({"doc":{"nodes":[node.clone()]}})
                    };
                    let result = inspect(
                        &serde_json::to_vec(&live).unwrap(),
                        Some(&serde_json::to_vec(&history).unwrap()),
                        false,
                    );
                    assert_eq!(
                        result.error.is_none(),
                        live_version == 16 && history_version == 16
                    );
                    assert!(result.facts.protected());
                }
            }
            node["kind"]["type"] = "raster".into();
            let live = serde_json::json!({"format":"emulsion","version":16,"nodes":[node]});
            assert!(
                inspect(&serde_json::to_vec(&live).unwrap(), None, false)
                    .error
                    .is_some()
            );
        }
    }

    #[test]
    fn malformed_conflicting_escaped_and_wrong_position_projective_are_protected() {
        for descriptor in [
            r#"{"projective":null}"#,
            r#"{"projective":[1,0,0,0,1,0,0,0,1],"projective":[1,0,0,0,1,0,0,0,1]}"#,
            r#"{"projec\u0074ive":[1,0,0,0,1,0,0,0,1],"extra":0}"#,
            r#"{"projective":[1,0,0,0,1,0,0,0,1],"x":0}"#,
            r#"{"projective":[1,0,0,0,1,0,0,0]}"#,
        ] {
            let live = format!(
                r#"{{"format":"emulsion","version":16,"nodes":[{{"kind":{{"type":"smart","placement":{descriptor}}}}}]}}"#
            );
            let result = inspect(live.as_bytes(), None, false);
            assert!(result.error.is_some(), "{descriptor}");
            assert!(result.facts.protected());
        }
        let live = br#"{"format":"emulsion","version":16,"nodes":[{"mask_transform":{"projective":[1,0,0,0,1,0,0,0,1]},"vector_mask":{},"kind":{"type":"smart"}}]}"#;
        code(
            inspect(live, None, false),
            NativeFailureCode::ProtectedFieldWrongKind,
        );
        let live = br#"{"format":"emulsion","version":16,"nodes":[{"kind":{"type":"smart"},"vector_mask":{"transform":{"projective":[1,0,0,0,1,0,0,0,1]}}}]}"#;
        code(
            inspect(live, None, false),
            NativeFailureCode::ProtectedFieldWrongKind,
        );
    }

    #[test]
    fn opaque_sources_require_paired_reference_empty_placeholder_and_version_nine() {
        let kind = serde_json::json!({"type":"smart", "source_document":"a", "editable":{"type":"document", "archive":[], "external":null}});
        let result = inspect(&native(kind.clone(), 9), None, true);
        assert!(result.error.is_none());
        assert!(result.facts.standalone_strict);
        assert!(
            inspect(&native(kind.clone(), 8), None, true)
                .error
                .is_some()
        );
        for broken in [
            serde_json::json!({"type":"smart","source_document":"a"}),
            serde_json::json!({"type":"smart","editable":{"type":"document","archive":[]}}),
            serde_json::json!({"type":"smart","source_document":"a","editable":{"type":"svg","xml":"x"}}),
            serde_json::json!({"type":"smart","source_document":"a","editable":{"type":"document","archive":[1]}}),
            serde_json::json!({"type":"smart","original_image":{},"editable":{"type":"svg","xml":"x"}}),
        ] {
            code(
                inspect(&native(broken, 15), None, false),
                NativeFailureCode::ConflictingSourceDescriptors,
            );
        }
    }

    #[test]
    fn escaped_duplicate_and_damaged_protected_fields_remain_evidence() {
        code(inspect(br#"{"format":"emulsion","version":12,"nodes":[{"kind":{"type":"fill","filters_ena\u0062led":true}}]}"#, None, false), NativeFailureCode::ProtectedFieldWrongKind);
        code(inspect(br#"{"format":"emulsion","version":12,"nodes":[{"kind":{"type":"fill","filters":[],"filters":[]}}]}"#, None, false), NativeFailureCode::ProtectedFieldWrongPosition);
        for broken in [
            br#"{"broken":],"source_docu\u006dent":"x"}"#.as_slice(),
            br#"{"broken":],"original_image":null}"#,
            br#"{"broken":],"editable":{"type":"document","archive":[]}}"#,
        ] {
            let result = inspect(
                br#"{"format":"emulsion","version":12,"nodes":[]}"#,
                Some(broken),
                false,
            );
            assert!(matches!(result.facts.retention, Retention::MustPreserve));
            assert!(result.error.is_some());
        }
    }
}
