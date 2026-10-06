//! Version-13 original PNG resources, shared by the live stack and history.
//! Encoded-byte addressing deliberately distinguishes hidden RGB values even
//! when two sources have the same native premultiplied rendering.
use crate::original_image_png::{self as png, SourceBudget};
use crate::{IoError, Result};
use emulsion_core::node::OriginalImage;
use emulsion_raster::{Raster, TILE, TileCoord};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
    sync::Arc,
};

const MAX_ORIGINALS: usize = 1024;
const PREFIX: &str = "original-images";
fn error(message: impl Into<String>) -> IoError {
    IoError::Manifest(message.into())
}
fn codec(error: png::SourceError) -> IoError {
    IoError::Manifest(error.to_string())
}
fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Stable row-major in-bounds premultiplied RGBA16, big-endian dimensions and
/// channel words. Tile padding, allocation identity and sparse fill layout are
/// intentionally excluded. The domain marker versions this canonical encoding.
pub(crate) fn source_digest(source: &Raster) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"Emulsion OriginalImage native-source v1\0");
    hash.update(source.width().to_be_bytes());
    hash.update(source.height().to_be_bytes());
    let mut row = Vec::with_capacity(source.width() as usize * 8);
    let (columns, _) = source.tiles_at(0);
    for y in 0..source.height() {
        row.clear();
        for tx in 0..columns {
            let tile = source.base_tile(TileCoord::new(tx, (y / TILE) as i32));
            let width = TILE.min(source.width() - tx as u32 * TILE);
            for x in 0..width {
                let pixel =
                    tile.map_or(source.fill(), |tile| tile[((y % TILE) * TILE + x) as usize]);
                for channel in pixel {
                    row.extend_from_slice(&channel.to_be_bytes());
                }
            }
        }
        hash.update(&row);
    }
    hash.finalize().into()
}

/// Call only after strict PNG decoding. Later IO boundaries validate all three
/// pieces again; the core model never treats these claimed digests as authority.
pub(crate) fn capture(bytes: Arc<Vec<u8>>, decoded: &Raster) -> Arc<OriginalImage> {
    Arc::new(OriginalImage::new(
        bytes.clone(),
        Sha256::digest(bytes.as_slice()).into(),
        source_digest(decoded),
    ))
}

/// Validate a retained source before writing bytes to any external/native file.
pub(crate) fn validate(original: &OriginalImage, source: &Raster) -> Result<()> {
    if <[u8; 32]>::from(Sha256::digest(original.bytes().as_slice())) != *original.encoded_sha256() {
        return Err(error("Original PNG encoded digest mismatch"));
    }
    let decoded = png::png_source_with_budget(original.bytes(), &mut SourceBudget::default())
        .map_err(codec)?;
    let digest = source_digest(&decoded);
    if digest != *original.source_sha256() || digest != source_digest(source) {
        return Err(error(
            "Original PNG does not match the current Smart source",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OriginalImageRef {
    pub encoded_sha256: String,
    pub source_sha256: String,
    pub width: u32,
    pub height: u32,
}
impl OriginalImageRef {
    pub fn path(&self) -> String {
        format!("{PREFIX}/{}.png", self.encoded_sha256)
    }
    fn from_original(original: &OriginalImage, source: &Raster) -> Self {
        Self {
            encoded_sha256: hex(original.encoded_sha256()),
            source_sha256: hex(original.source_sha256()),
            width: source.width(),
            height: source.height(),
        }
    }
    fn validate(&self) -> Result<()> {
        if !valid_hash(&self.encoded_sha256) || !valid_hash(&self.source_sha256) {
            return Err(error("Invalid original PNG digest reference"));
        }
        png::native_storage(self.width, self.height).map_err(codec)?;
        Ok(())
    }
}

struct Resource {
    reference: OriginalImageRef,
    bytes: Arc<Vec<u8>>,
    decoded: Option<(Arc<OriginalImage>, Arc<Raster>)>,
}
#[derive(Default)]
pub(crate) struct OriginalImagePool {
    entries: BTreeMap<String, Resource>,
    budget: SourceBudget,
    // Retain each allocation while its identity is cached, preventing pointer reuse.
    source_digests: BTreeMap<usize, (Arc<Raster>, [u8; 32])>,
    source_work: SourceBudget,
    strict_history: bool,
}
impl OriginalImagePool {
    fn digest_for(&mut self, source: &Arc<Raster>) -> Result<[u8; 32]> {
        let identity = Arc::as_ptr(source) as usize;
        if let Some((_, digest)) = self.source_digests.get(&identity) {
            return Ok(*digest);
        }
        // Work is bounded for distinct snapshots too, not just encoded resources.
        self.source_work
            .charge(source.width(), source.height(), 0)
            .map_err(codec)?;
        let digest = source_digest(source);
        self.source_digests
            .insert(identity, (source.clone(), digest));
        Ok(digest)
    }
    pub fn reference(
        &mut self,
        original: &Option<Arc<OriginalImage>>,
        source: &Arc<Raster>,
        editable: bool,
    ) -> Result<Option<OriginalImageRef>> {
        let Some(original) = original else {
            return Ok(None);
        };
        if editable {
            return Err(error(
                "Original PNG is only valid for a raster-backed Smart source",
            ));
        }
        let reference = OriginalImageRef::from_original(original, source);
        reference.validate()?;
        if self.digest_for(source)? != *original.source_sha256() {
            return Err(error(
                "Original PNG does not match the current Smart source",
            ));
        }
        if let Some(existing) = self.entries.get(&reference.encoded_sha256) {
            if existing.reference != reference
                || (!Arc::ptr_eq(&existing.bytes, original.bytes())
                    && existing.bytes.as_slice() != original.bytes().as_slice())
            {
                return Err(error("Conflicting or stale original PNG resource"));
            }
        } else {
            if self.entries.len() >= MAX_ORIGINALS {
                return Err(error("Too many original PNG resources"));
            }
            self.budget
                .charge(reference.width, reference.height, original.bytes().len())
                .map_err(codec)?;
            validate(original, source)?;
            self.entries.insert(
                reference.encoded_sha256.clone(),
                Resource {
                    reference: reference.clone(),
                    bytes: original.bytes().clone(),
                    decoded: Some((original.clone(), source.clone())),
                },
            );
        }
        Ok(Some(reference))
    }
    pub fn requires_valid_history(&self) -> bool {
        self.strict_history || !self.entries.is_empty()
    }
    pub fn entries(&self) -> Vec<(String, Vec<u8>)> {
        self.entries
            .values()
            .map(|entry| (entry.reference.path(), entry.bytes.as_ref().clone()))
            .collect()
    }

    /// Collect and bound every referenced live/history resource before decoding
    /// any PNG. ZIP advertised sizes are also checked before inflating bytes.
    pub fn prepare<R: Read + Seek>(
        &mut self,
        references: impl IntoIterator<Item = OriginalImageRef>,
        zip: &mut zip::ZipArchive<R>,
    ) -> Result<()> {
        let mut pending = BTreeMap::<String, OriginalImageRef>::new();
        for reference in references {
            reference.validate()?;
            let existing = self
                .entries
                .get(&reference.encoded_sha256)
                .map(|e| &e.reference)
                .or_else(|| pending.get(&reference.encoded_sha256));
            if let Some(existing) = existing {
                if existing != &reference {
                    return Err(error("Conflicting original PNG references"));
                }
                continue;
            }
            if self.entries.len() + pending.len() >= MAX_ORIGINALS {
                return Err(error("Too many original PNG resources"));
            }
            let size = zip.by_name(&reference.path())?.size();
            let size = usize::try_from(size)
                .map_err(|_| error("Original PNG size exceeds address space"))?;
            if size == 0 {
                return Err(error("Empty original PNG resource"));
            }
            self.budget
                .charge(reference.width, reference.height, size)
                .map_err(codec)?;
            pending.insert(reference.encoded_sha256.clone(), reference);
        }
        for (key, reference) in pending {
            let expected = zip.by_name(&reference.path())?.size();
            let bytes = crate::ora::read_entry(zip, &reference.path(), expected + 1)?;
            if bytes.len() as u64 != expected {
                return Err(error(
                    "Original PNG ZIP size does not match decoded resource bytes",
                ));
            }
            if hex(&Sha256::digest(&bytes).into()) != key {
                return Err(error("Original PNG encoded digest mismatch"));
            }
            // Metadata must not understate a source's padded allocation budget.
            if bytes.len() < 33
                || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
                || &bytes[8..16] != b"\0\0\0\rIHDR"
                || u32::from_be_bytes(bytes[16..20].try_into().unwrap()) != reference.width
                || u32::from_be_bytes(bytes[20..24].try_into().unwrap()) != reference.height
            {
                return Err(error("Original PNG dimensions differ from its reference"));
            }
            self.entries.insert(
                key,
                Resource {
                    reference,
                    bytes: Arc::new(bytes),
                    decoded: None,
                },
            );
        }
        Ok(())
    }
    pub fn restore(
        &mut self,
        reference: &OriginalImageRef,
        source: Option<&Arc<Raster>>,
        editable: bool,
    ) -> Result<(Arc<OriginalImage>, Arc<Raster>)> {
        if editable {
            return Err(error(
                "Original PNG is only valid for a raster-backed Smart source",
            ));
        }
        let source_digest = source.map(|source| self.digest_for(source)).transpose()?;
        let entry = self
            .entries
            .get_mut(&reference.encoded_sha256)
            .ok_or_else(|| error("Missing original PNG resource"))?;
        if entry.reference != *reference {
            return Err(error("Conflicting original PNG reference"));
        }
        if entry.decoded.is_none() {
            let decoded = png::png_source_with_budget(&entry.bytes, &mut SourceBudget::default())
                .map_err(codec)?;
            let original = capture(entry.bytes.clone(), &decoded);
            if OriginalImageRef::from_original(&original, &decoded) != *reference {
                return Err(error("Original PNG source digest mismatch"));
            }
            entry.decoded = Some((original, decoded));
        }
        let (original, decoded) = entry.decoded.as_ref().unwrap();
        if source_digest.is_some_and(|digest| digest != *original.source_sha256()) {
            return Err(error(
                "Original PNG does not match saved Smart source tiles",
            ));
        }
        Ok((original.clone(), decoded.clone()))
    }
}

// These narrow probes skip all unrelated geometry and history tile tables.
// They enforce v13 even when the original exists only in a saved snapshot.
#[derive(Deserialize)]
struct ProbeKind {
    #[serde(rename = "type")]
    kind_type: String,
    #[serde(default)]
    editable: Option<serde::de::IgnoredAny>,
    #[serde(default)]
    original_image: Option<OriginalImageRef>,
}
impl ProbeKind {
    fn original(self) -> Result<Option<OriginalImageRef>> {
        if self.original_image.is_some() && (self.kind_type != "smart" || self.editable.is_some()) {
            return Err(error(
                "Original PNG metadata requires a raster-backed Smart kind",
            ));
        }
        Ok(self.original_image)
    }
}
#[derive(Deserialize)]
struct ProbeNode {
    kind: ProbeKind,
}
#[derive(Default, Deserialize)]
struct ProbeDoc {
    #[serde(default)]
    nodes: Vec<ProbeNode>,
}
#[derive(Deserialize)]
struct ProbeSnapshot {
    doc: ProbeDoc,
}
#[derive(Deserialize)]
struct ProbeManifest {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    nodes: Vec<ProbeNode>,
}
#[derive(Deserialize)]
struct ProbeHistory {
    version: u32,
    #[serde(default)]
    commits: Vec<ProbeSnapshot>,
    #[serde(default)]
    working: Option<ProbeSnapshot>,
}

#[derive(Deserialize)]
struct HistoryVersionProbe {
    version: u32,
}
#[derive(Deserialize)]
struct PresenceKind {
    #[serde(default)]
    original_image: Option<serde::de::IgnoredAny>,
}
#[derive(Deserialize)]
struct PresenceNode {
    kind: PresenceKind,
}
#[derive(Deserialize)]
struct PresenceDoc {
    #[serde(default)]
    nodes: Vec<PresenceNode>,
}
#[derive(Deserialize)]
struct PresenceSnapshot {
    doc: PresenceDoc,
}
#[derive(Deserialize)]
struct PresenceHistory {
    #[serde(default)]
    commits: Vec<PresenceSnapshot>,
    #[serde(default)]
    working: Option<PresenceSnapshot>,
}

pub(crate) fn preflight_archive<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
) -> Result<OriginalImagePool> {
    let mut pool = OriginalImagePool::default();
    let mut references = Vec::new();
    let manifest_version = if zip.by_name("emulsion.json").is_ok() {
        let bytes =
            crate::ora::read_entry(zip, "emulsion.json", crate::ora::MAX_NATIVE_MANIFEST_BYTES)?;
        let manifest: ProbeManifest =
            serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
        if manifest.version > crate::ora::FORMAT_VERSION {
            return Err(IoError::TooNew(manifest.version));
        }
        references.extend(
            manifest
                .nodes
                .into_iter()
                .map(|node| node.kind.original())
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten(),
        );
        if !references.is_empty() && manifest.version < 13 {
            return Err(error("Original PNG resources require native version 13"));
        }
        Some(manifest.version)
    } else {
        None
    };
    pool.strict_history = manifest_version.is_some_and(|version| version >= 13);
    if zip.by_name(crate::history::GRAPH).is_ok() {
        let bytes =
            crate::ora::read_entry(zip, crate::history::GRAPH, crate::history::MAX_GRAPH_BYTES)?;
        // Probe the version independently: future schemas need not contain
        // any of today's history fields. Malformed v13 source metadata must
        // not quietly enter legacy damaged-history recovery.
        let version = serde_json::from_slice::<HistoryVersionProbe>(&bytes)
            .ok()
            .map(|probe| probe.version);
        if version.is_some_and(|v| v > crate::history::HISTORY_VERSION) {
            return Err(IoError::TooNew(version.unwrap()));
        }
        let parsed = serde_json::from_slice::<ProbeHistory>(&bytes);
        if let Err(parse_error) = &parsed {
            let has_originals =
                serde_json::from_slice::<PresenceHistory>(&bytes).is_ok_and(|history| {
                    history
                        .commits
                        .into_iter()
                        .chain(history.working)
                        .any(|snapshot| {
                            snapshot
                                .doc
                                .nodes
                                .into_iter()
                                .any(|node| node.kind.original_image.is_some())
                        })
                });
            if pool.strict_history || version.is_some_and(|v| v >= 13) || has_originals {
                return Err(error(format!(
                    "Invalid original-bearing history metadata: {parse_error}"
                )));
            }
        }
        if let Ok(history) = parsed {
            let originals: Vec<_> = history
                .commits
                .into_iter()
                .chain(history.working)
                .flat_map(|snapshot| snapshot.doc.nodes)
                .map(|node| node.kind.original())
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();
            if !originals.is_empty()
                && (history.version < 13 || manifest_version.is_none_or(|v| v < 13))
            {
                return Err(error(
                    "Original PNG resources require native and history version 13",
                ));
            }
            references.extend(originals);
        }
    }
    pool.prepare(references, zip)?;
    Ok(pool)
}
