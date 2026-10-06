//! Archive-wide feature gates run before source-plane or original-image allocation.
//! Both native read APIs use this boundary, including the one that skips history.
use crate::{IoError, Result};
use emulsion_core::{NodeId, NodeKind};
use emulsion_filters::{Filter, FilterStyle};
use emulsion_raster::blend::BlendSpace;
use serde::Deserialize;
use std::io::{Read, Seek};

pub(crate) fn enabled_by_default() -> bool {
    true
}
pub(crate) fn is_enabled(value: &bool) -> bool {
    *value
}

pub(crate) fn has_disabled_filters(node: &emulsion_core::Node) -> bool {
    matches!(&node.kind, NodeKind::Smart { filters_enabled, filter_styles, .. }
        if !filters_enabled || filter_styles.iter().any(|style| !style.enabled))
}

pub(crate) fn check_enabled_version(
    version: u32,
    filters: &[Filter],
    styles: &[FilterStyle],
    enabled: bool,
) -> Result<()> {
    if styles.len() > filters.len() {
        return Err(error("Orphan Smart Filter styles"));
    }
    if version < 15 && (!enabled || styles.iter().any(|style| !style.enabled)) {
        return Err(error(
            "Disabled Smart Filters require native/history version 15",
        ));
    }
    Ok(())
}

pub(crate) fn requires_v14(space: BlendSpace, target: Option<NodeId>) -> bool {
    space == BlendSpace::PhotoshopSrgbV1 || target.is_some()
}

/// Presence is semantic even at zero filter opacity or on a hidden layer.
pub(crate) fn has_invert(node: &emulsion_core::Node) -> bool {
    matches!(&node.kind, NodeKind::Smart { filters, .. } if filters.contains(&Filter::Invert))
}

pub(crate) fn check_filters_version(version: u32, filters: &[Filter]) -> Result<()> {
    if version < 14 && filters.contains(&Filter::Invert) {
        return Err(error(
            "Invert Smart Filters require native/history version 14",
        ));
    }
    Ok(())
}

pub(crate) fn check_version(version: u32, space: BlendSpace, target: Option<NodeId>) -> Result<()> {
    if version < 14 && requires_v14(space, target) {
        return Err(error(
            "Photoshop profile and Background metadata require native/history version 14",
        ));
    }
    Ok(())
}

fn error(message: impl Into<String>) -> IoError {
    IoError::Manifest(message.into())
}

#[derive(Default, Deserialize)]
struct Version {
    #[serde(default)]
    version: u32,
}

/// Reduced mapping probe: retain marker presence while skipping unneeded numbers.
/// Strict mapping shape and owner checks belong to native_admission/full decode.
#[derive(Default)]
struct MappingMarker(bool);
impl<'de> Deserialize<'de> for MappingMarker {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Marker;
        impl<'de> serde::de::Visitor<'de> for Marker {
            type Value = MappingMarker;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("mapping metadata")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                Ok(MappingMarker(false))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut found = false;
                while let Some(key) = map.next_key::<String>()? {
                    found |= key == "projective";
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
                Ok(MappingMarker(found))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(MappingMarker(false))
            }
        }
        d.deserialize_any(Marker)
    }
}
#[derive(Deserialize)]
struct MaskMarker {
    #[serde(default)]
    transform: MappingMarker,
}

#[derive(Deserialize)]
struct Kind {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    filters: Vec<Filter>,
    #[serde(default)]
    filter_styles: Vec<FilterStyle>,
    #[serde(default = "enabled_by_default")]
    filters_enabled: bool,
    #[serde(default)]
    placement: MappingMarker,
    #[serde(default)]
    filter_mask: Option<MaskMarker>,
}
#[derive(Deserialize)]
struct Node {
    id: NodeId,
    #[serde(default)]
    parent: Option<NodeId>,
    #[serde(default)]
    clip_to: Option<NodeId>,
    kind: Kind,
    #[serde(default)]
    mask_transform: MappingMarker,
}
#[derive(Deserialize)]
struct Doc {
    blend_space: BlendSpace,
    #[serde(default)]
    psd_background: Option<NodeId>,
    #[serde(default)]
    nodes: Vec<Node>,
}
impl Doc {
    fn has_projective(&self) -> bool {
        self.nodes.iter().any(|n| {
            n.mask_transform.0
                || n.kind.placement.0
                || n.kind.filter_mask.as_ref().is_some_and(|m| m.transform.0)
        })
    }

    fn has_disabled_filters(&self) -> bool {
        // Authored filter state cannot become discardable by corrupting the
        // kind tag. Full native decoding still validates the actual node kind.
        self.nodes.iter().any(|node| {
            !node.kind.filters_enabled || node.kind.filter_styles.iter().any(|style| !style.enabled)
        })
    }

    fn requires_v14(&self) -> bool {
        requires_v14(self.blend_space, self.psd_background) || self.has_invert()
    }

    fn has_invert(&self) -> bool {
        self.nodes
            .iter()
            .any(|node| node.kind.kind == "smart" && node.kind.filters.contains(&Filter::Invert))
    }

    fn validate(&self, native: u32, history: u32) -> Result<()> {
        if self.has_projective() && native.min(history) < 16 {
            return Err(error(
                "Projective Smart mappings require native and existing history version 16",
            ));
        }
        for node in &self.nodes {
            check_enabled_version(
                native.min(history),
                &node.kind.filters,
                &node.kind.filter_styles,
                node.kind.filters_enabled,
            )?;
        }
        if self.has_invert() && (native < 14 || history < 14) {
            return Err(error(
                "Invert Smart Filters require native and history version 14",
            ));
        }
        if requires_v14(self.blend_space, self.psd_background) && (native < 14 || history < 14) {
            return Err(error(
                "Photoshop profile and Background metadata require native and history version 14",
            ));
        }
        if let Some(id) = self.psd_background {
            let valid = self
                .nodes
                .iter()
                .find(|node| node.parent.is_none())
                .is_some_and(|node| {
                    node.id == id && node.clip_to.is_none() && node.kind.kind == "raster"
                })
                && self.nodes.iter().filter(|node| node.id == id).count() == 1;
            if !valid {
                return Err(error(
                    "Invalid Photoshop Background target: expected a unique bottom root raster without a clip link",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Deserialize)]
struct Snapshot {
    doc: Doc,
}
#[derive(Deserialize)]
struct History {
    #[serde(default)]
    commits: Vec<Snapshot>,
    #[serde(default)]
    working: Option<Snapshot>,
}

/// A bounded, nonrecursive lexical fallback for malformed legacy histories.
/// Scan JSON string tokens rather than a second Value tree: an invalid unrelated
/// field or excessive nesting must not prevent discovering later v14 metadata.
/// Escaped keys are decoded too; geometry and plane tables are never retained.
pub(crate) fn has_feature(bytes: &[u8]) -> bool {
    feature_scan(bytes, &mut |_| {})
}

fn feature_scan(bytes: &[u8], work: &mut impl FnMut(usize)) -> bool {
    fn whitespace(bytes: &[u8], mut at: usize) -> usize {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        at
    }
    fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
        let mut at = start + 1;
        while let Some(byte) = bytes.get(at) {
            match byte {
                b'"' => return Some(at + 1),
                b'\\' => at += 2,
                _ => at += 1,
            }
        }
        None
    }
    fn token(bytes: &[u8]) -> Option<String> {
        // Every character of either known key/profile can be Unicode escaped.
        (bytes.len() <= 256)
            .then(|| serde_json::from_slice(bytes).ok())
            .flatten()
    }
    // Track only open editable-object depths while scanning once. Keeping
    // the nesting positions (not object contents) avoids rescanning nested
    // editable suffixes after the normal parser's recursion limit is reached.
    let mut editable_depths = Vec::new();
    let mut pending_editable = None;
    let mut filter_depths = Vec::new();
    let mut pending_filter = None;
    let mut depth = 0usize;
    let mut at = 0;
    while at < bytes.len() {
        work(1);
        if bytes[at] != b'"' {
            match bytes[at] {
                b'{' | b'[' => {
                    depth = depth.saturating_add(1);
                    if pending_editable == Some(at) {
                        editable_depths.push(depth);
                        pending_editable = None;
                    }
                    if let Some((position, styles)) = pending_filter
                        && position == at
                    {
                        filter_depths.push((depth, styles));
                        pending_filter = None;
                    }
                }
                b'}' | b']' => {
                    if editable_depths.last() == Some(&depth) {
                        editable_depths.pop();
                    }
                    if filter_depths
                        .last()
                        .is_some_and(|(start, _)| *start == depth)
                    {
                        filter_depths.pop();
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
            at += 1;
            continue;
        }
        let Some(end) = string_end(bytes, at) else {
            break;
        };
        work(end - at);
        let key = token(&bytes[at..end]);
        at = end;
        let colon = whitespace(bytes, end);
        if bytes.get(colon) != Some(&b':') {
            continue;
        }
        let value = whitespace(bytes, colon + 1);
        match key.as_deref() {
            Some("filters" | "filter_styles") if bytes.get(value) == Some(&b'[') => {
                pending_filter = Some((value, key.as_deref() == Some("filter_styles")));
            }
            Some("kind")
                if filter_depths
                    .last()
                    .is_some_and(|(start, styles)| !styles && depth == start + 1) =>
            {
                if bytes.get(value) == Some(&b'"')
                    && let Some(end) = string_end(bytes, value)
                    && token(&bytes[value..end]).as_deref() == Some("invert")
                {
                    return true;
                }
            }
            Some("enabled")
                if filter_depths
                    .last()
                    .is_some_and(|(start, styles)| *styles && depth == start + 1) =>
            {
                return true;
            }
            Some("editable") if bytes.get(value) == Some(&b'{') => {
                pending_editable = Some(value);
            }
            Some("type") if editable_depths.last() == Some(&depth) => {
                if bytes.get(value) == Some(&b'"')
                    && let Some(end) = string_end(bytes, value)
                    && token(&bytes[value..end]).as_deref() == Some("document")
                {
                    return true;
                }
            }
            Some("filters_enabled" | "source_document" | "original_image" | "projective") => {
                return true;
            }
            Some("psd_background") => {
                let null = bytes.get(value..value + 4) == Some(b"null")
                    && bytes.get(value + 4).is_none_or(|byte| {
                        byte.is_ascii_whitespace() || matches!(byte, b',' | b'}' | b']')
                    });
                if !null {
                    return true;
                }
            }
            Some("blend_space") => {
                if bytes.get(value) != Some(&b'"') {
                    return true;
                }
                let Some(end) = string_end(bytes, value) else {
                    return true;
                };
                if token(&bytes[value..end]).as_deref() == Some("photoshop-srgb-v1") {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Returns whether history decoding must be strict rather than recoverable.
pub(crate) fn preflight_archive<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> Result<bool> {
    let mut live_disabled = false;
    let mut live_projective = false;
    let native = if zip.by_name("emulsion.json").is_ok() {
        let bytes =
            crate::ora::read_entry(zip, "emulsion.json", crate::ora::MAX_NATIVE_MANIFEST_BYTES)?;
        let version: Version = serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
        if version.version > crate::ora::FORMAT_VERSION {
            return Err(IoError::TooNew(version.version));
        }
        let doc: Doc = serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
        doc.validate(version.version, version.version)?;
        live_disabled = doc.has_disabled_filters();
        live_projective = doc.has_projective();
        version.version
    } else {
        0
    };
    let mut strict = native >= 14;
    if zip.by_name(crate::history::GRAPH).is_ok() {
        let bytes =
            crate::ora::read_entry(zip, crate::history::GRAPH, crate::history::MAX_GRAPH_BYTES)?;
        let version = serde_json::from_slice::<Version>(&bytes)
            .ok()
            .map_or(0, |v| v.version);
        if version > crate::history::HISTORY_VERSION {
            return Err(IoError::TooNew(version));
        }
        if live_projective && version < 16 {
            return Err(error(
                "Projective Smart mappings require native and existing history version 16",
            ));
        }
        if live_disabled && version < 15 {
            return Err(error(
                "Disabled Smart Filters require native and history version 15",
            ));
        }
        // The reduced probe intentionally ignores cache/geometry/plane fields.
        // Even if it succeeds, authored enabled metadata must make later full
        // history decoding strict. In particular explicit true flags cannot be
        // discarded because an unrelated ignored field or kind tag is damaged.
        strict |= version >= 14 || has_feature(&bytes);
        match serde_json::from_slice::<History>(&bytes) {
            Ok(history) => {
                for snapshot in history.commits.into_iter().chain(history.working) {
                    snapshot.doc.validate(native, version)?;
                    strict |= snapshot.doc.requires_v14()
                        || snapshot.doc.has_disabled_filters()
                        || snapshot.doc.has_projective();
                }
            }
            Err(e) if strict => {
                return Err(error(format!(
                    "Invalid protected history metadata (including Smart Filters and projective mappings): {e}"
                )));
            }
            Err(_) => {} // Preserve the existing damaged-history policy for legacy documents.
        }
    }
    Ok(strict)
}

#[cfg(test)]
mod tests {
    use super::has_feature;
    #[test]
    fn nested_editable_fallback_scan_has_linear_work_and_preserves_inner_evidence() {
        for count in [32usize, 256, 4096] {
            let mut bytes = br#"{"broken":],"#.to_vec();
            for _ in 0..count {
                bytes.extend_from_slice(br#""editable":{"unrelated":0,"#);
            }
            bytes.extend_from_slice(br#""type":"svg""#);
            bytes.extend(std::iter::repeat_n(b'}', count));
            let mut work = 0usize;
            assert!(!super::feature_scan(&bytes, &mut |n| work += n));
            assert!(
                work <= bytes.len() * 3,
                "{work} steps for {} bytes",
                bytes.len()
            );
            let at = bytes.windows(5).position(|w| w == b"\"svg\"").unwrap();
            bytes.splice(at..at + 5, b"\"document\"".iter().copied());
            let mut work = 0usize;
            assert!(super::feature_scan(&bytes, &mut |n| work += n));
            assert!(work <= bytes.len() * 3);
        }
        assert!(!has_feature(
            br#"{"broken":],"editable":{"other":{"type":"document"}}}"#
        ));
        assert!(has_feature(
            br#"{"broken":],"editable":{"editable":{"type":"svg"},"type":"document"}}"#
        ));
    }

    #[test]
    fn malformed_filter_arrays_do_not_hide_other_protected_markers() {
        for field in [
            "source_document",
            "original_image",
            "projective",
            "filters_enabled",
        ] {
            let bytes =
                format!(r#"{{"broken":],"filters":[{{"kind":"ordinary","{field}":null}}]}}"#);
            assert!(has_feature(bytes.as_bytes()));
        }
        assert!(has_feature(
            br#"{"broken":],"filters":[{"editable":{"type":"document","archive":[]}}]}"#
        ));
        assert!(!has_feature(
            br#"{"broken":],"filters":[{"other":{"kind":"invert","enabled":false}}]}"#
        ));
    }

    #[test]
    fn malformed_history_enabled_scan_is_scoped_and_decodes_escaped_keys() {
        for bytes in [
            br#"{"broken":],"filters_enabled":false}"#.as_slice(),
            br#"{"broken":],"filters_ena\u0062led":true}"#,
            br#"{"broken":],"filter_styles":[{"enabled":false}]}"#,
            br#"{"broken":],"filter_styles":[{"ena\u0062led":null}]}"#,
        ] {
            assert!(has_feature(bytes));
        }
        for bytes in [
            br#"{"broken":],"filter_mask":{"enabled":false}}"#.as_slice(),
            br#"{"broken":],"styles":[{"enabled":false}]}"#,
            br#"{"broken":],"filter_styles":[{"other":{"enabled":false}}]}"#,
            br#"{"broken":],"filter_styles":[[{"enabled":false}]]}"#,
        ] {
            assert!(!has_feature(bytes));
        }
        let mut deep = vec![b'['; 1000];
        deep.extend_from_slice(br#"{"filter_styles":[{"enabled":false}]}"#);
        assert!(has_feature(&deep));
    }

    #[test]
    fn malformed_history_invert_scan_requires_a_filter_descriptor() {
        assert!(has_feature(
            br#"{"broken":],"filters":[{"kind":"invert"}]}"#
        ));
        assert!(has_feature(
            br#"{"broken":],"filt\u0065rs":[{"k\u0069nd":"in\u0076ert"}]}"#
        ));
        assert!(!has_feature(
            br#"{"broken":],"kind":{"type":"adjust","adjustment":{"kind":"invert"}}}"#
        ));
        assert!(!has_feature(
            br#"{"broken":],"name":"invert","filters":[{"kind":"find-edges"}]}"#
        ));
        assert!(!has_feature(
            br#"{"broken":],"filters":["invert",{"extra":{"kind":"invert"}}]}"#
        ));
        assert!(!has_feature(
            br#"{"broken":],"name":"quoted \"filters\":[{\"kind\":\"invert\"}]}"#
        ));
        let mut deep = vec![b'['; 1000];
        deep.extend_from_slice(br#"{"filters":[{"kind":"invert"}]}"#);
        assert!(has_feature(&deep));
    }

    #[test]
    fn malformed_history_presence_scan_handles_escaping_and_deep_unrelated_fields() {
        assert!(has_feature(br#"{"broken":], "\u0070sd_background":1}"#));
        assert!(has_feature(
            br#"{"broken":], "blend_space":"photoshop-srgb-v1"}"#
        ));
        assert!(has_feature(br#"{"blend_space":{}, "psd_background":null}"#));
        assert!(!has_feature(
            br#"{"blend_space":"srgb", "psd_background":null}"#
        ));
        assert!(!has_feature(
            br#"{"name":"quoted \"psd_background\":1", "broken":]}"#
        ));
        let mut deep = vec![b'['; 1000];
        deep.extend_from_slice(br#"{"psd_background":1}"#);
        assert!(has_feature(&deep));
    }
}
