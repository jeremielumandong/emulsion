//! Versioned, nondestructive development settings owned by the document.
use crate::NodeId;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DevelopParams {
    pub exposure: f32,
    pub temperature: f32,
    pub tint: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub black_point: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    /// Output ordinates at gamma-2.2 input positions 0, .25, .5, .75, 1.
    pub tone_curve: [f32; 5],
    /// Camera-channel gains, normalized to green; None uses the as-shot gains.
    pub wb_override: Option<[f32; 4]>,
}

impl Default for DevelopParams {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            temperature: 0.0,
            tint: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            black_point: 0.0,
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            tone_curve: Self::LINEAR_CURVE,
            wb_override: None,
        }
    }
}

impl DevelopParams {
    pub const LINEAR_CURVE: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
    pub const MEDIUM_CONTRAST_CURVE: [f32; 5] = [0.0, 0.20, 0.5, 0.80, 1.0];
    pub const STRONG_CONTRAST_CURVE: [f32; 5] = [0.0, 0.15, 0.5, 0.85, 1.0];

    pub fn validate(&self) -> Result<(), &'static str> {
        for (value, low, high) in [
            (self.exposure, -3.0, 3.0),
            (self.temperature, -1.0, 1.0),
            (self.tint, -1.0, 1.0),
            (self.highlights, 0.0, 1.0),
            (self.shadows, -1.0, 1.0),
            (self.black_point, 0.0, 0.25),
            (self.brightness, -1.0, 1.0),
            (self.contrast, -1.0, 1.0),
            (self.saturation, -1.0, 1.0),
        ] {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err("RAW development parameter outside its supported range");
            }
        }
        if self
            .tone_curve
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || self.tone_curve.windows(2).any(|pair| pair[0] > pair[1])
        {
            return Err("RAW tone curve must be finite, bounded, and monotonic");
        }
        if self.wb_override.is_some_and(|wb| {
            wb.iter()
                .any(|v| !v.is_finite() || !(0.01..=100.0).contains(v))
        }) {
            return Err("RAW white balance gains must be finite and between 0.01 and 100");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RawMetadata {
    pub make: String,
    pub model: String,
    pub format: String,
    pub compression: String,
    pub bits_per_sample: u32,
    pub sensor: String,
    pub decoder: String,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<String>,
}

impl Default for RawMetadata {
    fn default() -> Self {
        Self {
            make: String::new(),
            model: String::new(),
            format: "unknown".into(),
            compression: "unknown".into(),
            bits_per_sample: 0,
            sensor: "unknown".into(),
            decoder: "unknown".into(),
            width: 0,
            height: 0,
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RawDocument {
    pub schema_version: u32,
    pub node_id: NodeId,
    /// Linked original. Never overwritten by saving the document.
    pub source: PathBuf,
    pub source_sha256: String,
    pub params: DevelopParams,
    pub metadata: RawMetadata,
}

impl RawDocument {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 {
            return Err("unsupported RAW recipe version");
        }
        if self.source.as_os_str().is_empty() {
            return Err("missing RAW source path");
        }
        if self.source_sha256.len() != 64
            || !self.source_sha256.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("invalid RAW source fingerprint");
        }
        self.params.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Document, Editor, Node, NodeKind};
    use emulsion_raster::Raster;
    use std::sync::Arc;

    fn document() -> Document {
        let mut doc = Document::new(2, 2);
        doc.nodes.push(Node::raster(
            1,
            "RAW",
            Arc::new(Raster::solid(2, 2, [0.2, 0.3, 0.4, 1.0])),
            Default::default(),
        ));
        doc.next_id = 2;
        doc.raw_originals = vec!["photo.dng".into()];
        doc.raw = Some(RawDocument {
            schema_version: 1,
            node_id: 1,
            source: "photo.dng".into(),
            source_sha256: "a".repeat(64),
            params: DevelopParams::default(),
            metadata: RawMetadata::default(),
        });
        doc
    }

    #[test]
    fn legacy_recipe_defaults_and_new_controls_roundtrip() {
        let legacy: DevelopParams =
            serde_json::from_str(r#"{"exposure":1.0,"temperature":0.2}"#).unwrap();
        assert_eq!(legacy.tone_curve, DevelopParams::LINEAR_CURVE);
        assert_eq!(legacy.wb_override, None);
        legacy.validate().unwrap();
        let params = DevelopParams {
            tone_curve: DevelopParams::MEDIUM_CONTRAST_CURVE,
            wb_override: Some([2.0, 1.0, 1.5, 1.0]),
            black_point: 0.02,
            brightness: 0.2,
            contrast: 0.3,
            saturation: -0.1,
            ..legacy
        };
        let json = serde_json::to_string(&params).unwrap();
        assert_eq!(
            serde_json::from_str::<DevelopParams>(&json).unwrap(),
            params
        );
        let mut invalid = params;
        invalid.tone_curve[2] = 0.0;
        assert!(invalid.validate().is_err());
        invalid = params;
        invalid.wb_override = Some([f32::NAN; 4]);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn development_and_recipe_undo_together() {
        let before = document();
        let mut editor = Editor::new(before.clone(), None);
        let params = DevelopParams {
            exposure: 1.0,
            ..Default::default()
        };
        let pixels = Arc::new(Raster::solid(2, 2, [0.4, 0.6, 0.8, 1.0]));
        editor
            .execute(Command::DevelopRaw {
                id: 1,
                raster: pixels.clone(),
                params,
            })
            .unwrap();
        assert_eq!(editor.doc.raw.as_ref().unwrap().params, params);
        assert!(
            matches!(&editor.doc.nodes[0].kind, NodeKind::Raster { raster, .. } if Arc::ptr_eq(raster, &pixels))
        );
        editor.undo();
        assert_eq!(editor.doc, before);
        editor.redo();
        assert_eq!(editor.doc.raw.as_ref().unwrap().params, params);
    }

    #[test]
    fn painting_detaches_recipe_but_undo_restores_it() {
        let mut editor = Editor::new(document(), None);
        editor
            .execute(Command::ReplacePixels {
                id: 1,
                raster: Arc::new(Raster::solid(2, 2, [1.0; 4])),
                dirty: emulsion_raster::IRect::new(0, 0, 2, 2),
                label: "Paint".into(),
            })
            .unwrap();
        assert!(editor.doc.raw.is_none());
        editor.undo();
        assert!(editor.doc.raw.is_some());
        editor.execute(Command::RemoveNode { id: 1 }).unwrap();
        assert!(editor.doc.raw.is_none());
    }

    #[test]
    fn raw_validation_and_locks_are_atomic() {
        let mut doc = document();
        let before = doc.clone();
        let mut command = Command::DevelopRaw {
            id: 1,
            raster: Arc::new(Raster::solid(2, 2, [1.0; 4])),
            params: DevelopParams {
                exposure: f32::NAN,
                ..Default::default()
            },
        };
        assert!(command.apply(&mut doc).is_err());
        assert_eq!(doc, before);
        if let Command::DevelopRaw { params, .. } = &mut command {
            params.exposure = 1.0;
        }
        doc.nodes[0].locked = true;
        let locked = doc.clone();
        assert!(command.apply(&mut doc).is_err());
        assert_eq!(doc, locked);
        doc.nodes[0].locked = false;
        doc.raw.as_mut().unwrap().schema_version = 2;
        assert!(doc.validate().is_err());
    }

    #[test]
    fn smart_conversion_preserves_raw_and_development_updates_source() {
        let mut doc = document();
        Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
        assert!(doc.raw.is_some());
        let pixels = Arc::new(Raster::solid(2, 2, [0.5; 4]));
        Command::DevelopRaw {
            id: 1,
            raster: pixels.clone(),
            params: DevelopParams::default(),
        }
        .apply(&mut doc)
        .unwrap();
        assert!(
            matches!(&doc.nodes[0].kind, NodeKind::Smart { source, .. } if Arc::ptr_eq(source, &pixels))
        );
    }

    #[test]
    fn merge_keeps_recipe_with_chosen_pixels_and_relink_is_undoable() {
        use crate::graph::{ConflictKey, MergeOutcome, Side, merge};
        use std::collections::HashMap;
        let base = document();
        let mut ours = base.clone();
        let mut theirs = base.clone();
        for (doc, exposure, level) in [(&mut ours, 1.0, 0.7), (&mut theirs, -1.0, 0.1)] {
            Command::DevelopRaw {
                id: 1,
                raster: Arc::new(Raster::solid(2, 2, [level; 4])),
                params: DevelopParams {
                    exposure,
                    ..Default::default()
                },
            }
            .apply(doc)
            .unwrap();
        }
        assert!(matches!(
            merge(&base, &ours, &theirs, &HashMap::new()).unwrap(),
            MergeOutcome::Conflicts(_)
        ));
        let choices = HashMap::from([(ConflictKey::Node(1), Side::Theirs)]);
        let MergeOutcome::Merged(merged) = merge(&base, &ours, &theirs, &choices).unwrap() else {
            panic!("resolved merge")
        };
        assert_eq!(merged.raw, theirs.raw);
        let MergeOutcome::Merged(merged) = merge(&base, &base, &theirs, &HashMap::new()).unwrap()
        else {
            panic!("clean merge")
        };
        assert_eq!(merged.raw, theirs.raw);
        let mut editor = Editor::new(base.clone(), None);
        editor
            .execute(Command::RelinkRaw {
                source: "moved/photo.dng".into(),
            })
            .unwrap();
        assert_eq!(
            editor.doc.raw.as_ref().unwrap().source,
            PathBuf::from("moved/photo.dng")
        );
        editor.undo();
        assert_eq!(editor.doc.raw, base.raw);
        assert!(
            editor
                .doc
                .raw_originals
                .contains(&PathBuf::from("moved/photo.dng"))
        );
        editor.redo();
        assert!(
            editor
                .doc
                .raw_originals
                .contains(&PathBuf::from("photo.dng"))
        );
    }
}
