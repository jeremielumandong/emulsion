//! Portable, bounded local media assets. Decoding stays in system media runtimes.
use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
pub const MAX_LOCAL_ASSET_BYTES: usize = 32 << 20;
pub const MAX_LOCAL_PAGE_BYTES: usize = 64 << 20;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalMediaKind {
    Video,
    Audio,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalMedia {
    pub boundary: NodeId,
    pub name: String,
    pub kind: LocalMediaKind,
    pub mime: String,
    #[serde(with = "encoded")]
    pub bytes: Arc<Vec<u8>>,
    #[serde(default)]
    pub trim_start_ms: u32,
    #[serde(default)]
    pub trim_end_ms: Option<u32>,
    #[serde(default = "full_volume")]
    pub volume: f32,
    #[serde(default)]
    pub looping: bool,
}
fn full_volume() -> f32 {
    1.
}
impl std::fmt::Debug for LocalMedia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalMedia")
            .field("boundary", &self.boundary)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("mime", &self.mime)
            .field("bytes", &self.bytes.len())
            .field("trim_start_ms", &self.trim_start_ms)
            .field("trim_end_ms", &self.trim_end_ms)
            .field("volume", &self.volume)
            .field("looping", &self.looping)
            .finish()
    }
}
mod encoded {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    pub fn serialize<S: serde::Serializer>(
        value: &Arc<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(value.as_slice()))
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Arc<Vec<u8>>, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.len() > MAX_LOCAL_ASSET_BYTES.div_ceil(3) * 4 {
            return Err(serde::de::Error::custom("Local media exceeds 32 MiB"));
        }
        let bytes = STANDARD.decode(text).map_err(serde::de::Error::custom)?;
        if bytes.len() > MAX_LOCAL_ASSET_BYTES {
            return Err(serde::de::Error::custom("Local media exceeds 32 MiB"));
        }
        Ok(Arc::new(bytes))
    }
}
impl LocalMedia {
    pub fn from_bytes(name: String, bytes: Vec<u8>) -> Result<Self, String> {
        let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let (kind, mime) = match extension.as_str() {
            "mp4" | "m4v" => (LocalMediaKind::Video, "video/mp4"),
            "webm" => (LocalMediaKind::Video, "video/webm"),
            "mov" => (LocalMediaKind::Video, "video/quicktime"),
            "mp3" => (LocalMediaKind::Audio, "audio/mpeg"),
            "m4a" => (LocalMediaKind::Audio, "audio/mp4"),
            "wav" => (LocalMediaKind::Audio, "audio/wav"),
            "ogg" | "oga" | "opus" => (LocalMediaKind::Audio, "audio/ogg"),
            _ => return Err("Choose MP4, WebM, MOV, MP3, M4A, WAV, OGG or Opus media.".into()),
        };
        let value = Self {
            boundary: 0,
            name,
            kind,
            mime: mime.into(),
            bytes: Arc::new(bytes),
            trim_start_ms: 0,
            trim_end_ms: None,
            volume: 1.,
            looping: false,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty()
            || self.name.chars().count() > 255
            || self.name.chars().any(char::is_control)
            || self.bytes.is_empty()
            || self.bytes.len() > MAX_LOCAL_ASSET_BYTES
        {
            return Err("Local media needs a name and a nonempty file of at most 32 MiB.".into());
        }
        if !self.volume.is_finite()
            || !(0. ..=1.).contains(&self.volume)
            || self.trim_start_ms >= 86_400_000
            || self
                .trim_end_ms
                .is_some_and(|end| end <= self.trim_start_ms || end > 86_400_000)
        {
            return Err("Choose volume 0–100% and an end after the start (up to 24 hours).".into());
        }
        let b = self.bytes.as_slice();
        let valid = match (self.kind, self.mime.as_str()) {
            (LocalMediaKind::Video, "video/mp4" | "video/quicktime")
            | (LocalMediaKind::Audio, "audio/mp4") => {
                b.len() >= 12 && (&b[4..8] == b"ftyp" || &b[4..8] == b"moov" || &b[4..8] == b"mdat")
            }
            (LocalMediaKind::Video, "video/webm") => b.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
            (LocalMediaKind::Audio, "audio/mpeg") => {
                b.starts_with(b"ID3") || (b.len() > 1 && b[0] == 0xff && b[1] & 0xe0 == 0xe0)
            }
            (LocalMediaKind::Audio, "audio/wav") => {
                b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WAVE"
            }
            (LocalMediaKind::Audio, "audio/ogg") => b.starts_with(b"OggS"),
            _ => false,
        };
        if !valid {
            return Err(
                "The media file header does not match a supported audio/video container.".into(),
            );
        }
        Ok(())
    }
}
pub fn validate_local(media: &BTreeMap<NodeId, LocalMedia>, doc: &Document) -> Result<(), String> {
    if media.len() > 64
        || media.values().map(|m| m.bytes.len()).sum::<usize>() > MAX_LOCAL_PAGE_BYTES
    {
        return Err(
            "A page supports up to 64 local media objects and 64 MiB of embedded media.".into(),
        );
    }
    for (id, item) in media {
        item.validate()?;
        if !doc.node(*id).is_some_and(|n| n.is_group())
            || doc.node(item.boundary).and_then(|n| n.parent) != Some(*id)
            || rectangle(doc, item.boundary).is_none()
        {
            return Err("Local media needs an axis-aligned rectangular frame. Detach it before rotating or reshaping it.".into());
        }
    }
    Ok(())
}
pub fn insert_local(
    editor: &mut crate::Editor,
    mut media: LocalMedia,
    origin: (f64, f64),
    size: (f64, f64),
) -> Result<NodeId, String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    media.validate()?;
    let mut trial = crate::Editor::try_new(editor.doc.clone(), None).map_err(|e| e.to_string())?;
    let id = insert_youtube(&mut trial, "https://youtu.be/M7lc1UVf-VE", origin, size)?;
    media.boundary = trial
        .doc
        .design
        .media
        .remove(&id)
        .ok_or("Missing media poster")?
        .boundary;
    for child in trial.doc.children(Some(id)) {
        if let Some(NodeKind::Text { spec, .. }) = trial.doc.node(child).map(|n| &n.kind) {
            let mut spec = (**spec).clone();
            spec.text = media.name.chars().take(48).collect();
            trial
                .execute(Command::SetText {
                    id: child,
                    spec: Box::new(spec),
                })
                .map_err(|e| e.to_string())?;
        }
    }
    trial
        .execute(Command::Rename {
            id,
            name: format!(
                "{} · {}",
                if media.kind == LocalMediaKind::Audio {
                    "Audio"
                } else {
                    "Video"
                },
                media.name
            ),
        })
        .map_err(|e| e.to_string())?;
    let mut design = trial.doc.design.clone();
    design.local_media.insert(id, media);
    trial
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|e| e.to_string())?;
    editor.commit_design_document(trial.doc, "Insert local media")?;
    Ok(id)
}
pub fn update_local(
    editor: &mut crate::Editor,
    id: NodeId,
    start: u32,
    end: Option<u32>,
    volume: f32,
    looping: bool,
) -> Result<(), String> {
    editable(&editor.doc, id)?;
    let mut design = editor.doc.design.clone();
    let media = design
        .local_media
        .get_mut(&id)
        .ok_or("Select local video or audio first")?;
    media.trim_start_ms = start;
    media.trim_end_ms = end;
    media.volume = volume;
    media.looping = looping;
    media.validate()?;
    commit(
        editor,
        vec![Command::SetDesign {
            design: Box::new(design),
        }],
    )
}
pub fn detach_local(editor: &mut crate::Editor, id: NodeId) -> Result<(), String> {
    editable(&editor.doc, id)?;
    let mut design = editor.doc.design.clone();
    if design.local_media.remove(&id).is_none() {
        return Err("Select local media first".into());
    }
    commit(
        editor,
        vec![Command::SetDesign {
            design: Box::new(design),
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wave() -> LocalMedia {
        LocalMedia::from_bytes("tone.wav".into(), b"RIFF\0\0\0\0WAVEdata".to_vec()).unwrap()
    }
    #[test]
    fn design_local_media_portable_atomic_and_undoable() {
        let mut editor = crate::Editor::new(Document::new(800, 600), None);
        let id = insert_local(&mut editor, wave(), (20., 30.), (400., 225.)).unwrap();
        assert!(editor.doc.design.media.is_empty());
        assert_eq!(bounds(&editor.doc, id), Some((20., 30., 400., 225.)));
        let original = editor.doc.clone();
        let revision = editor.revision;
        assert!(update_local(&mut editor, id, 2000, Some(1000), 0.5, false).is_err());
        assert_eq!(editor.doc.design, original.design);
        assert_eq!(editor.revision, revision);
        update_local(&mut editor, id, 1000, Some(3000), 0.25, true).unwrap();
        editor.undo();
        assert_eq!(editor.doc.design, original.design);
        editor.redo();
        let data = serde_json::to_value(&editor.doc.design.local_media[&id]).unwrap();
        assert!(data["bytes"].is_string());
        assert!(data.get("path").is_none());
        let loaded: LocalMedia = serde_json::from_value(data).unwrap();
        assert_eq!(loaded, editor.doc.design.local_media[&id]);
        let arc = loaded.bytes.clone();
        assert!(editor.doc.buffers().iter().any(|(_, n)| *n == arc.len()));
        detach_local(&mut editor, id).unwrap();
        assert!(editor.doc.node(id).is_some());
        editor.undo();
        assert!(editor.doc.design.local_media.contains_key(&id));
        editor.undo();
        editor.undo();
        assert!(editor.doc.nodes.is_empty());
    }
    #[test]
    fn design_local_media_rejects_disguised_or_invalid_files() {
        assert!(LocalMedia::from_bytes("evil.mp4".into(), b"<script>".to_vec()).is_err());
        assert!(LocalMedia::from_bytes("tone.exe".into(), b"RIFF\0\0\0\0WAVE".to_vec()).is_err());
        let mut m = wave();
        m.volume = f32::NAN;
        assert!(m.validate().is_err());
        m.volume = 1.;
        m.trim_start_ms = 86_400_000;
        assert!(m.validate().is_err());
    }
}
