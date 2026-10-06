//! Portable font resources use private content-derived family names. Loading a
//! document cannot replace an installed font or change another document's text.
use crate::{Command, Document, Editor, NodeId, NodeKind};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex, OnceLock, Weak},
};
pub const MAX_FONT_BYTES: usize = 8 << 20;
pub const MAX_DOCUMENT_FONT_BYTES: usize = 64 << 20;
pub const MAX_FONTS: usize = 32;
const MAX_LIVE_BYTES: usize = 256 << 20;
#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddedFont(Arc<Data>);
#[derive(Debug, PartialEq)]
struct Data {
    alias: String,
    family: String,
    bytes: Arc<Vec<u8>>,
    registered: OnceLock<()>,
}
fn registry() -> &'static Mutex<BTreeMap<String, Weak<Data>>> {
    static VALUE: OnceLock<Mutex<BTreeMap<String, Weak<Data>>>> = OnceLock::new();
    VALUE.get_or_init(Default::default)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    alias: String,
    family: String,
    bytes: String,
}
impl Serialize for EmbeddedFont {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Wire {
            alias: self.alias().into(),
            family: self.family().into(),
            bytes: base64::engine::general_purpose::STANDARD.encode(self.bytes()),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for EmbeddedFont {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(d)?;
        if wire.bytes.len() > MAX_FONT_BYTES.div_ceil(3) * 4 {
            return Err(serde::de::Error::custom("Embedded font exceeds 8 MiB."));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(wire.bytes)
            .map_err(serde::de::Error::custom)?;
        let font = Self::from_bytes(bytes).map_err(serde::de::Error::custom)?;
        if font.alias() != wire.alias || font.family() != wire.family {
            return Err(serde::de::Error::custom(
                "Embedded font identity does not match its content.",
            ));
        }
        Ok(font)
    }
}
impl EmbeddedFont {
    pub fn alias(&self) -> &str {
        &self.0.alias
    }
    pub fn family(&self) -> &str {
        &self.0.family
    }
    pub fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }
    pub fn allocation(&self) -> (usize, usize) {
        (self.0.bytes.as_ptr() as usize, self.0.bytes.len())
    }
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
            return Err("Choose a TTF/OTF font no larger than 8 MiB.".into());
        }
        if !matches!(bytes.get(..4), Some(b"\0\x01\0\0" | b"OTTO" | b"true")) {
            return Err("Choose a single-face TTF or OTF font file.".into());
        }
        // Respect native embedding permissions encoded in the OS/2 table.
        let count = u16::from_be_bytes(
            bytes
                .get(4..6)
                .ok_or("Truncated font header")?
                .try_into()
                .unwrap(),
        ) as usize;
        for index in 0..count {
            let table = bytes
                .get(12 + 16 * index..28 + 16 * index)
                .ok_or("Truncated font table directory")?;
            if &table[..4] == b"OS/2" {
                let offset = u32::from_be_bytes(table[8..12].try_into().unwrap()) as usize;
                let flags = bytes
                    .get(offset.saturating_add(8)..offset.saturating_add(10))
                    .ok_or("Truncated font embedding permissions")?;
                let flags = u16::from_be_bytes(flags.try_into().unwrap());
                if flags & 0x202 != 0 || (flags & 4 != 0 && flags & 8 == 0) {
                    return Err("This font's embedding settings do not permit an editable portable document.".into());
                }
            }
        }
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let alias = format!("EmulsionFont-{hash}");
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_font_data(bytes.clone());
        let family = db
            .faces()
            .next()
            .and_then(|f| f.families.first())
            .map(|v| v.0.clone())
            .ok_or("No usable outline font face was found.")?;
        let data = {
            let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
            registry.retain(|_, font| font.strong_count() > 0);
            if let Some(font) = registry.get(&alias).and_then(Weak::upgrade) {
                drop(registry);
                font.registered.get_or_init(crate::text::refresh_fonts);
                return Ok(Self(font));
            }
            let total: usize = registry
                .values()
                .filter_map(Weak::upgrade)
                .map(|f| f.bytes.len())
                .sum();
            if total.saturating_add(bytes.len()) > MAX_LIVE_BYTES {
                return Err("Open documents already use 256 MiB of embedded fonts. Close unused documents before importing another font.".into());
            }
            let data = Arc::new(Data {
                alias: alias.clone(),
                family,
                bytes: Arc::new(bytes),
                registered: OnceLock::new(),
            });
            registry.insert(alias, Arc::downgrade(&data));
            data
        };
        data.registered.get_or_init(crate::text::refresh_fonts);
        Ok(Self(data))
    }
}
/// Called when CPU, Vello or export constructs its own font database.
pub(crate) fn populate(system: &mut cosmic_text::FontSystem) {
    let fonts: Vec<_> = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .filter_map(Weak::upgrade)
        .collect();
    for font in fonts {
        let db = system.db_mut();
        let ids = db.load_font_source(cosmic_text::fontdb::Source::Binary(font.bytes.clone()));
        for id in ids {
            if let Some(mut info) = db.face(id).cloned() {
                db.remove_face(id);
                for (family, _) in &mut info.families {
                    *family = font.alias.clone();
                }
                db.push_face_info(info);
            }
        }
    }
}
pub fn validate(fonts: &BTreeMap<String, EmbeddedFont>) -> Result<(), String> {
    if fonts.len() > MAX_FONTS
        || fonts.values().map(|f| f.bytes().len()).sum::<usize>() > MAX_DOCUMENT_FONT_BYTES
    {
        return Err("A page supports 32 embedded fonts and 64 MiB of font data.".into());
    }
    if fonts.iter().any(|(alias, font)| alias != font.alias()) {
        return Err("Embedded font resource has an invalid family alias.".into());
    }
    Ok(())
}
pub fn embed(editor: &mut Editor, ids: &[NodeId], font: EmbeddedFont) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit before embedding a font.".into());
    }
    if ids.is_empty() || ids.iter().any(|id| editor.doc.node(*id).is_none()) {
        return Err("Select existing text objects or groups.".into());
    }
    let selected: HashSet<_> = ids.iter().flat_map(|id| editor.doc.subtree(*id)).collect();
    if selected.is_empty() || !selected.iter().all(|id| editor.doc.node(*id).is_some()) {
        return Err("Select existing text objects or groups.".into());
    }
    let mut trial = Editor::try_new(editor.doc.clone(), None).map_err(|e| e.to_string())?;
    let mut design = trial.doc.design.clone();
    design.fonts.insert(font.alias().into(), font.clone());
    validate(&design.fonts)?;
    trial
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|e| e.to_string())?;
    let mut changed = 0;
    for id in selected {
        if let Some(node) = trial.doc.node(id)
            && let NodeKind::Text { spec, .. } = &node.kind
        {
            let mut spec = (**spec).clone();
            spec.font = font.alias().into();
            for run in &mut spec.runs {
                run.style.font = font.alias().into();
            }
            trial
                .execute(Command::SetText {
                    id,
                    spec: Box::new(spec),
                })
                .map_err(|e| e.to_string())?;
            changed += 1;
        }
    }
    if changed == 0 {
        return Err("Select at least one editable text object.".into());
    }
    editor.commit_design_document(trial.doc, "Embed portable font")
}
pub fn used_aliases(doc: &Document) -> HashSet<String> {
    doc.nodes
        .iter()
        .filter_map(|node| {
            if let NodeKind::Text { spec, .. } = &node.kind {
                Some(spec)
            } else {
                None
            }
        })
        .flat_map(|spec| {
            std::iter::once(spec.font.clone())
                .chain(spec.runs.iter().map(|run| run.style.font.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, command::Slot, text::TextSpec};
    fn font() -> EmbeddedFont {
        EmbeddedFont::from_bytes(include_bytes!("../../../assets/fonts/Geist.ttf").to_vec())
            .unwrap()
    }
    #[test]
    fn portable_fonts_keep_native_text_undo_clipboard_and_private_identity() {
        let same = font();
        let font = font();
        assert_eq!(font.alias(), same.alias());
        assert_eq!(font.allocation(), same.allocation());
        let mut editor = Editor::new(Document::new(300, 180), None);
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Title",
                    TextSpec {
                        text: "Portable typography".into(),
                        size: 24.,
                        ..Default::default()
                    },
                    300,
                    180,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let before = editor.doc.clone();
        embed(&mut editor, &[id], font.clone()).unwrap();
        let after = editor.doc.clone();
        let NodeKind::Text { spec, .. } = &editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.font, font.alias());
        assert_eq!(spec.text, "Portable typography");
        assert!(crate::text::bounds(spec).w > 0);
        assert_eq!(editor.doc.design.fonts.len(), 1);
        let mut system = crate::text::font_system();
        assert!(
            system
                .db_mut()
                .faces()
                .any(|face| face.families.iter().any(|(name, _)| name == font.alias()))
        );
        let mut planes = HashSet::new();
        assert!(
            editor
                .doc
                .buffers_once(&mut planes)
                .contains(&font.allocation())
        );
        assert!(editor.doc.clone().buffers_once(&mut planes).is_empty());
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
        assert!(editor.redo());
        assert_eq!(editor.doc, after);
        let fragment = crate::fragment::Fragment::capture(&editor.doc, &[id]).unwrap();
        let mut target = Editor::new(Document::new(300, 180), None);
        let pasted = fragment.paste(&mut target, Slot::TOP, (0., 0.)).unwrap();
        assert_eq!(target.doc.design.fonts, editor.doc.design.fonts);
        assert!(
            matches!(&target.doc.node(pasted[0]).unwrap().kind,NodeKind::Text{spec,..}if spec.font==font.alias())
        );
        editor
            .execute(Command::SetLocked { id, locked: true })
            .unwrap();
        let before = editor.doc.clone();
        assert!(
            embed(
                &mut editor,
                &[id],
                EmbeddedFont::from_bytes(
                    include_bytes!("../../../assets/fonts/GeistMono.ttf").to_vec()
                )
                .unwrap()
            )
            .is_err()
        );
        assert_eq!(editor.doc, before);
    }
    #[test]
    fn portable_font_rejects_malformed_permissions_and_identity() {
        assert!(EmbeddedFont::from_bytes(vec![0; 32]).is_err());
        assert!(EmbeddedFont::from_bytes(vec![0; MAX_FONT_BYTES + 1]).is_err());
        let font = font();
        let mut wire = serde_json::to_value(&font).unwrap();
        wire["alias"] = serde_json::json!("EmulsionFont-spoof");
        assert!(serde_json::from_value::<EmbeddedFont>(wire).is_err());
        let mut bytes = font.bytes().to_vec();
        let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
        for i in 0..count {
            let table = &bytes[12 + 16 * i..28 + 16 * i];
            if &table[..4] == b"OS/2" {
                let offset = u32::from_be_bytes(table[8..12].try_into().unwrap()) as usize;
                bytes[offset + 8..offset + 10].copy_from_slice(&2u16.to_be_bytes());
                break;
            }
        }
        assert!(
            EmbeddedFont::from_bytes(bytes)
                .unwrap_err()
                .contains("embedding")
        );
    }
}
