//! Deduplicated bounded font blobs for native current/history page archives.
use crate::{IoError, Result};
use emulsion_core::{
    design_fonts::{EmbeddedFont, MAX_DOCUMENT_FONT_BYTES, MAX_FONT_BYTES, MAX_FONTS},
    design_metadata::Design,
};
use std::{
    collections::BTreeMap,
    io::{Read, Seek},
};
const MAX_POOL_BYTES: usize = 512 << 20;
#[derive(Default)]
pub(crate) struct FontPool(BTreeMap<String, EmbeddedFont>);
impl FontPool {
    pub(crate) fn detach(&mut self, design: &Design) -> (Design, Vec<String>) {
        let mut metadata = design.clone();
        let aliases = metadata.fonts.keys().cloned().collect();
        self.0.extend(std::mem::take(&mut metadata.fonts));
        (metadata, aliases)
    }
    pub(crate) fn entries(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        if self
            .0
            .values()
            .map(|font| font.bytes().len())
            .sum::<usize>()
            > MAX_POOL_BYTES
        {
            return Err(IoError::Manifest(
                "Font history resources exceed 512 MiB.".into(),
            ));
        }
        Ok(self
            .0
            .iter()
            .map(|(alias, font)| (format!("{prefix}/{alias}.font"), font.bytes().to_vec()))
            .collect())
    }
    pub(crate) fn restore<R: Read + Seek>(
        &mut self,
        design: &mut Design,
        aliases: &[String],
        zip: &mut zip::ZipArchive<R>,
        prefix: &str,
    ) -> Result<()> {
        if aliases.len() > MAX_FONTS {
            return Err(IoError::Manifest(
                "Too many font resource references.".into(),
            ));
        }
        for alias in aliases {
            if !alias
                .strip_prefix("EmulsionFont-")
                .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return Err(IoError::Manifest("Invalid font resource alias.".into()));
            }
            let font = if let Some(font) = self.0.get(alias) {
                font.clone()
            } else {
                let mut entry = zip.by_name(&format!("{prefix}/{alias}.font"))?;
                if entry.size() > MAX_FONT_BYTES as u64 {
                    return Err(IoError::Manifest("Font resource exceeds 8 MiB.".into()));
                }
                if self
                    .0
                    .values()
                    .map(|font| font.bytes().len())
                    .sum::<usize>() as u64
                    + entry.size()
                    > MAX_POOL_BYTES as u64
                {
                    return Err(IoError::Manifest(
                        "Font history resources exceed 512 MiB.".into(),
                    ));
                }
                let mut bytes = Vec::new();
                entry
                    .by_ref()
                    .take((MAX_FONT_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)?;
                let font = EmbeddedFont::from_bytes(bytes).map_err(IoError::Manifest)?;
                if font.alias() != alias {
                    return Err(IoError::Manifest("Font resource digest mismatch.".into()));
                }
                self.0.insert(alias.clone(), font.clone());
                font
            };
            design.fonts.insert(alias.clone(), font);
            if design
                .fonts
                .values()
                .map(|f| f.bytes().len())
                .sum::<usize>()
                > MAX_DOCUMENT_FONT_BYTES
            {
                return Err(IoError::Manifest("Document fonts exceed 64 MiB.".into()));
            }
        }
        emulsion_core::design_fonts::validate(&design.fonts).map_err(IoError::Manifest)
    }
}
