//! Brush organization is independent of documents and UI. Mutate a draft catalog,
//! then commit it through the IO store; a failed commit never changes live state.
use emulsion_raster::{
    library::{self, BrushPreset},
    paint::{Brush, DualBlend},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

pub type BrushId = String;
pub type SetId = String;
pub type LibraryId = String;
pub const SCHEMA_VERSION: u32 = 1;
pub const USER_LIBRARY: &str = "library:user";
pub const USER_SET: &str = "set:user";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("{0} was not found")]
    NotFound(String),
    #[error("Names cannot be empty")]
    EmptyName,
    #[error("Built-in libraries and sets cannot be removed or moved")]
    Builtin,
    #[error("Invalid brush catalog: {0}")]
    Invalid(String),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Author {
    pub name: String,
    pub website: String,
    pub copyright: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrushDefinition {
    pub id: BrushId,
    pub set_id: SetId,
    pub name: String,
    pub note: String,
    pub brush: Brush,
    pub baseline: Brush,
    #[serde(default)]
    pub secondary: Option<Brush>,
    #[serde(default)]
    pub combine_mode: DualBlend,
    #[serde(default)]
    pub baseline_secondary: Option<Brush>,
    #[serde(default)]
    pub baseline_combine_mode: DualBlend,
    #[serde(default)]
    pub reset_point_secondary: Option<Brush>,
    #[serde(default)]
    pub reset_point_combine_mode: DualBlend,
    #[serde(default)]
    pub secondary_shape_asset: Option<String>,
    #[serde(default)]
    pub secondary_grain_asset: Option<String>,
    #[serde(default)]
    pub baseline_secondary_shape_asset: Option<String>,
    #[serde(default)]
    pub baseline_secondary_grain_asset: Option<String>,
    #[serde(default)]
    pub reset_point_secondary_shape_asset: Option<String>,
    #[serde(default)]
    pub reset_point_secondary_grain_asset: Option<String>,
    #[serde(default)]
    pub reset_point: Option<Brush>,
    #[serde(default)]
    pub reset_point_shape_asset: Option<String>,
    #[serde(default)]
    pub reset_point_grain_asset: Option<String>,
    #[serde(default)]
    pub author: Author,
    #[serde(default)]
    pub builtin: bool,
    #[serde(default)]
    pub shape_asset: Option<String>,
    #[serde(default)]
    pub grain_asset: Option<String>,
    #[serde(default)]
    pub baseline_shape_asset: Option<String>,
    #[serde(default)]
    pub baseline_grain_asset: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrushSet {
    pub id: SetId,
    pub library_id: LibraryId,
    pub name: String,
    #[serde(default)]
    pub builtin: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrushLibrary {
    pub id: LibraryId,
    pub name: String,
    #[serde(default)]
    pub builtin: bool,
}

/// Runtime values remembered independently for each tool and selected brush.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolMemory {
    pub brush_id: BrushId,
    pub brush: Brush,
    #[serde(default)]
    pub marks: [Option<BrushMark>; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrushMark {
    pub size: f32,
    pub opacity: f32,
}

/// Vector order is display order. IDs, never positions or names, are references.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub schema_version: u32,
    pub revision: u64,
    pub libraries: Vec<BrushLibrary>,
    pub sets: Vec<BrushSet>,
    pub brushes: Vec<BrushDefinition>,
    #[serde(default)]
    pub pinned: Vec<BrushId>,
    #[serde(default)]
    pub recent: Vec<BrushId>,
    #[serde(default)]
    pub tool_memories: BTreeMap<String, ToolMemory>,
}

fn name(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        Err(Error::EmptyName)
    } else {
        Ok(value.into())
    }
}

/// No randomness or platform dependency is required. Persistent creation occurs
/// behind the store's revision check, and validation rejects any ID collision.
pub fn new_id(kind: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{kind}:{time:x}:{:x}:{:x}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

impl Default for Catalog {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Catalog {
    /// Add newly shipped built-ins by identity without changing existing names,
    /// settings, reset points or order. Persistence remains the store's job.
    pub fn backfill_builtins(&mut self) -> usize {
        let shipped = Self::builtin();
        for library in shipped.libraries.into_iter().filter(|l| l.builtin) {
            if !self.libraries.iter().any(|l| l.id == library.id) {
                self.libraries.push(library);
            }
        }
        for set in shipped.sets.into_iter().filter(|s| s.builtin) {
            if !self.sets.iter().any(|s| s.id == set.id) {
                self.sets.push(set);
            }
        }
        let mut added = 0;
        for brush in shipped.brushes {
            if self.brush(&brush.id).is_none() {
                self.brushes.push(brush);
                added += 1;
            }
        }
        added
    }
    pub fn builtin() -> Self {
        let mut catalog = Self {
            schema_version: SCHEMA_VERSION,
            revision: 0,
            libraries: vec![
                BrushLibrary {
                    id: "library:emulsion".into(),
                    name: "Emulsion".into(),
                    builtin: true,
                },
                BrushLibrary {
                    id: USER_LIBRARY.into(),
                    name: "My brushes".into(),
                    builtin: false,
                },
            ],
            sets: vec![BrushSet {
                id: USER_SET.into(),
                library_id: USER_LIBRARY.into(),
                name: "My brushes".into(),
                builtin: false,
            }],
            brushes: vec![],
            pinned: vec![],
            recent: vec![],
            tool_memories: BTreeMap::new(),
        };
        for category in library::CATEGORIES {
            catalog.sets.push(BrushSet {
                id: format!("set:builtin:{category}"),
                library_id: "library:emulsion".into(),
                name: (*category).into(),
                builtin: true,
            });
        }
        for preset in library::library() {
            catalog.brushes.push(BrushDefinition {
                id: format!("brush:builtin:{}:{}", preset.category, preset.name),
                set_id: format!("set:builtin:{}", preset.category),
                name: preset.name,
                note: preset.note,
                brush: preset.brush,
                baseline: preset.brush,
                reset_point: None,
                reset_point_shape_asset: None,
                reset_point_grain_asset: None,
                builtin: true,
                author: Author {
                    name: "Emulsion".into(),
                    ..Author::default()
                },
                secondary: None,
                combine_mode: DualBlend::default(),
                baseline_secondary: None,
                baseline_combine_mode: DualBlend::default(),
                reset_point_secondary: None,
                reset_point_combine_mode: DualBlend::default(),
                secondary_shape_asset: None,
                secondary_grain_asset: None,
                baseline_secondary_shape_asset: None,
                baseline_secondary_grain_asset: None,
                reset_point_secondary_shape_asset: None,
                reset_point_secondary_grain_asset: None,
                shape_asset: None,
                grain_asset: None,
                baseline_shape_asset: None,
                baseline_grain_asset: None,
            });
        }
        catalog
    }

    /// Every legacy row is preserved, including duplicate names and more than
    /// 400 entries. Original array positions provide deterministic migration IDs.
    pub fn migrate_legacy(presets: Vec<BrushPreset>) -> Self {
        let mut catalog = Self::builtin();
        for (index, preset) in presets.into_iter().enumerate() {
            let category = if preset.category.trim().is_empty() {
                "Mine"
            } else {
                &preset.category
            };
            let set_id = format!("set:legacy:{category}");
            if !catalog.sets.iter().any(|s| s.id == set_id) {
                catalog.sets.push(BrushSet {
                    id: set_id.clone(),
                    library_id: USER_LIBRARY.into(),
                    name: category.into(),
                    builtin: false,
                });
            }
            let brush = preset.brush.sanitized();
            catalog.brushes.push(BrushDefinition {
                id: format!("brush:legacy:{index}"),
                set_id,
                name: if preset.name.trim().is_empty() {
                    format!("Imported brush {}", index + 1)
                } else {
                    preset.name
                },
                note: preset.note,
                brush,
                baseline: brush,
                reset_point: None,
                reset_point_shape_asset: None,
                reset_point_grain_asset: None,
                builtin: false,
                author: Author::default(),
                secondary: None,
                combine_mode: DualBlend::default(),
                baseline_secondary: None,
                baseline_combine_mode: DualBlend::default(),
                reset_point_secondary: None,
                reset_point_combine_mode: DualBlend::default(),
                secondary_shape_asset: None,
                secondary_grain_asset: None,
                baseline_secondary_shape_asset: None,
                baseline_secondary_grain_asset: None,
                reset_point_secondary_shape_asset: None,
                reset_point_secondary_grain_asset: None,
                shape_asset: None,
                grain_asset: None,
                baseline_shape_asset: None,
                baseline_grain_asset: None,
            });
        }
        catalog
    }

    pub fn brush(&self, id: &str) -> Option<&BrushDefinition> {
        self.brushes.iter().find(|b| b.id == id)
    }
    pub fn brush_mut(&mut self, id: &str) -> Option<&mut BrushDefinition> {
        self.brushes.iter_mut().find(|b| b.id == id)
    }
    pub fn preset(&self, id: &str) -> Option<BrushPreset> {
        let b = self.brush(id)?;
        Some(BrushPreset {
            name: b.name.clone(),
            category: self.sets.iter().find(|s| s.id == b.set_id)?.name.clone(),
            note: b.note.clone(),
            brush: b.brush,
        })
    }
    pub fn presets(&self) -> Vec<BrushPreset> {
        self.brushes
            .iter()
            .filter_map(|b| self.preset(&b.id))
            .collect()
    }
    pub fn create_library(&mut self, label: &str) -> Result<LibraryId> {
        let id = new_id("library");
        self.libraries.push(BrushLibrary {
            id: id.clone(),
            name: name(label)?,
            builtin: false,
        });
        Ok(id)
    }
    pub fn create_set(&mut self, library_id: &str, label: &str) -> Result<SetId> {
        if !self.libraries.iter().any(|l| l.id == library_id) {
            return Err(Error::NotFound(library_id.into()));
        }
        let id = new_id("set");
        self.sets.push(BrushSet {
            id: id.clone(),
            library_id: library_id.into(),
            name: name(label)?,
            builtin: false,
        });
        Ok(id)
    }
    pub fn add_brush(&mut self, set_id: &str, label: &str, brush: Brush) -> Result<BrushId> {
        if !self.sets.iter().any(|s| s.id == set_id) {
            return Err(Error::NotFound(set_id.into()));
        }
        let id = new_id("brush");
        let brush = brush.sanitized();
        self.brushes.push(BrushDefinition {
            id: id.clone(),
            set_id: set_id.into(),
            name: name(label)?,
            note: String::new(),
            brush,
            baseline: brush,
            reset_point: None,
            reset_point_shape_asset: None,
            reset_point_grain_asset: None,
            secondary: None,
            combine_mode: DualBlend::default(),
            baseline_secondary: None,
            baseline_combine_mode: DualBlend::default(),
            reset_point_secondary: None,
            reset_point_combine_mode: DualBlend::default(),
            secondary_shape_asset: None,
            secondary_grain_asset: None,
            baseline_secondary_shape_asset: None,
            baseline_secondary_grain_asset: None,
            reset_point_secondary_shape_asset: None,
            reset_point_secondary_grain_asset: None,
            author: Author::default(),
            builtin: false,
            shape_asset: None,
            grain_asset: None,
            baseline_shape_asset: None,
            baseline_grain_asset: None,
        });
        Ok(id)
    }
    pub fn rename_brush(&mut self, id: &str, label: &str) -> Result<()> {
        let label = name(label)?;
        self.brush_mut(id)
            .ok_or_else(|| Error::NotFound(id.into()))?
            .name = label;
        Ok(())
    }
    pub fn rename_set(&mut self, id: &str, label: &str) -> Result<()> {
        let label = name(label)?;
        self.sets
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::NotFound(id.into()))?
            .name = label;
        Ok(())
    }
    pub fn rename_library(&mut self, id: &str, label: &str) -> Result<()> {
        let label = name(label)?;
        self.libraries
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::NotFound(id.into()))?
            .name = label;
        Ok(())
    }
    pub fn duplicate_brush(&mut self, id: &str, set_id: &str) -> Result<BrushId> {
        if !self.sets.iter().any(|s| s.id == set_id) {
            return Err(Error::NotFound(set_id.into()));
        }
        let mut brush = self
            .brush(id)
            .ok_or_else(|| Error::NotFound(id.into()))?
            .clone();
        brush.id = new_id("brush");
        brush.set_id = set_id.into();
        brush.name.push_str(" copy");
        brush.builtin = false;
        let id = brush.id.clone();
        self.brushes.push(brush);
        Ok(id)
    }
    pub fn delete_brush(&mut self, id: &str) -> Result<()> {
        let brush = self.brush(id).ok_or_else(|| Error::NotFound(id.into()))?;
        if brush.builtin {
            return Err(Error::Builtin);
        }
        self.brushes.retain(|b| b.id != id);
        self.pinned.retain(|b| b != id);
        self.recent.retain(|b| b != id);
        self.tool_memories.retain(|_, memory| memory.brush_id != id);
        Ok(())
    }
    /// Combining creates a new brush. Source definitions and their identities
    /// remain available, so there is always a lossless path back to either one.
    pub fn combine_brushes(&mut self, primary_id: &str, secondary_id: &str) -> Result<BrushId> {
        let primary = self
            .brush(primary_id)
            .ok_or_else(|| Error::NotFound(primary_id.into()))?
            .clone();
        let secondary = self
            .brush(secondary_id)
            .ok_or_else(|| Error::NotFound(secondary_id.into()))?
            .clone();
        if primary.secondary.is_some() || secondary.secondary.is_some() {
            return Err(Error::Invalid(
                "Uncombine dual brushes before combining them again".into(),
            ));
        }
        if primary_id == secondary_id {
            return Err(Error::Invalid(
                "Select two different brushes to combine".into(),
            ));
        }
        let id = self.add_brush(
            &primary.set_id,
            &format!("{} + {}", primary.name, secondary.name),
            primary.brush,
        )?;
        let b = self.brush_mut(&id).expect("just inserted");
        b.note = format!("Combined from {} and {}", primary.name, secondary.name);
        b.author = primary.author;
        b.shape_asset = primary.shape_asset.clone();
        b.grain_asset = primary.grain_asset.clone();
        b.baseline_shape_asset = primary.shape_asset;
        b.baseline_grain_asset = primary.grain_asset;
        b.secondary = Some(secondary.brush);
        b.baseline_secondary = Some(secondary.brush);
        b.secondary_shape_asset = secondary.shape_asset.clone();
        b.secondary_grain_asset = secondary.grain_asset.clone();
        b.baseline_secondary_shape_asset = secondary.shape_asset;
        b.baseline_secondary_grain_asset = secondary.grain_asset;
        Ok(id)
    }
    pub fn uncombine_brush(&mut self, id: &str) -> Result<(BrushId, BrushId)> {
        let source = self
            .brush(id)
            .ok_or_else(|| Error::NotFound(id.into()))?
            .clone();
        let secondary = source
            .secondary
            .ok_or_else(|| Error::Invalid("This brush has no secondary component".into()))?;
        let primary_id = self.add_brush(
            &source.set_id,
            &format!("{} primary", source.name),
            source.brush,
        )?;
        let secondary_id = self.add_brush(
            &source.set_id,
            &format!("{} secondary", source.name),
            secondary,
        )?;
        for (id, shape, grain) in [
            (&primary_id, &source.shape_asset, &source.grain_asset),
            (
                &secondary_id,
                &source.secondary_shape_asset,
                &source.secondary_grain_asset,
            ),
        ] {
            let b = self.brush_mut(id).expect("just inserted");
            b.author = source.author.clone();
            b.note = format!("Component of {}", source.name);
            b.shape_asset = shape.clone();
            b.grain_asset = grain.clone();
            b.baseline_shape_asset = shape.clone();
            b.baseline_grain_asset = grain.clone();
        }
        Ok((primary_id, secondary_id))
    }
    pub fn delete_set(&mut self, id: &str) -> Result<()> {
        let set = self
            .sets
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::NotFound(id.into()))?;
        if set.builtin || self.brushes.iter().any(|b| b.set_id == id && b.builtin) {
            return Err(Error::Builtin);
        }
        let ids: Vec<_> = self
            .brushes
            .iter()
            .filter(|b| b.set_id == id)
            .map(|b| b.id.clone())
            .collect();
        for id in ids {
            self.delete_brush(&id)?;
        }
        self.sets.retain(|s| s.id != id);
        Ok(())
    }
    pub fn delete_library(&mut self, id: &str) -> Result<()> {
        let lib = self
            .libraries
            .iter()
            .find(|l| l.id == id)
            .ok_or_else(|| Error::NotFound(id.into()))?;
        if lib.builtin || self.sets.iter().any(|s| s.library_id == id && s.builtin) {
            return Err(Error::Builtin);
        }
        let ids: Vec<_> = self
            .sets
            .iter()
            .filter(|s| s.library_id == id)
            .map(|s| s.id.clone())
            .collect();
        for id in ids {
            self.delete_set(&id)?;
        }
        self.libraries.retain(|l| l.id != id);
        Ok(())
    }
    pub fn move_brush(&mut self, id: &str, set_id: &str, index: usize) -> Result<()> {
        if !self.sets.iter().any(|s| s.id == set_id) {
            return Err(Error::NotFound(set_id.into()));
        }
        let source = self
            .brushes
            .iter()
            .position(|b| b.id == id)
            .ok_or_else(|| Error::NotFound(id.into()))?;
        if self.brushes[source].builtin && self.brushes[source].set_id != set_id {
            return Err(Error::Builtin);
        }
        let mut b = self.brushes.remove(source);
        b.set_id = set_id.into();
        let target = self
            .brushes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.set_id == set_id)
            .nth(index)
            .map(|(i, _)| i)
            .unwrap_or_else(|| {
                self.brushes
                    .iter()
                    .rposition(|b| b.set_id == set_id)
                    .map(|i| i + 1)
                    .unwrap_or(self.brushes.len())
            });
        self.brushes.insert(target, b);
        Ok(())
    }
    pub fn move_set(&mut self, id: &str, library_id: &str, index: usize) -> Result<()> {
        if !self.libraries.iter().any(|l| l.id == library_id) {
            return Err(Error::NotFound(library_id.into()));
        }
        let source = self
            .sets
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| Error::NotFound(id.into()))?;
        if self.sets[source].builtin && self.sets[source].library_id != library_id {
            return Err(Error::Builtin);
        }
        let mut set = self.sets.remove(source);
        set.library_id = library_id.into();
        let target = self
            .sets
            .iter()
            .enumerate()
            .filter(|(_, s)| s.library_id == library_id)
            .nth(index)
            .map(|(i, _)| i)
            .unwrap_or_else(|| {
                self.sets
                    .iter()
                    .rposition(|s| s.library_id == library_id)
                    .map(|i| i + 1)
                    .unwrap_or(self.sets.len())
            });
        self.sets.insert(target, set);
        Ok(())
    }
    pub fn reorder_library(&mut self, id: &str, index: usize) -> Result<()> {
        let source = self
            .libraries
            .iter()
            .position(|l| l.id == id)
            .ok_or_else(|| Error::NotFound(id.into()))?;
        let lib = self.libraries.remove(source);
        self.libraries.insert(index.min(self.libraries.len()), lib);
        Ok(())
    }
    pub fn duplicate_set(&mut self, id: &str, library_id: &str) -> Result<SetId> {
        let source = self
            .sets
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::NotFound(id.into()))?
            .clone();
        let ids: Vec<_> = self
            .brushes
            .iter()
            .filter(|b| b.set_id == id)
            .map(|b| b.id.clone())
            .collect();
        let new = self.create_set(library_id, &format!("{} copy", source.name))?;
        for id in ids {
            self.duplicate_brush(&id, &new)?;
        }
        Ok(new)
    }
    pub fn duplicate_library(&mut self, id: &str) -> Result<LibraryId> {
        let source = self
            .libraries
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::NotFound(id.into()))?
            .clone();
        let ids: Vec<_> = self
            .sets
            .iter()
            .filter(|s| s.library_id == id)
            .map(|s| s.id.clone())
            .collect();
        let new = self.create_library(&format!("{} copy", source.name))?;
        for id in ids {
            self.duplicate_set(&id, &new)?;
        }
        Ok(new)
    }
    pub fn pin(&mut self, id: &str, pinned: bool) -> Result<()> {
        if self.brush(id).is_none() {
            return Err(Error::NotFound(id.into()));
        }
        self.pinned.retain(|b| b != id);
        if pinned {
            self.pinned.push(id.into());
        }
        Ok(())
    }
    pub fn record_use(&mut self, id: &str) -> Result<()> {
        if self.brush(id).is_none() {
            return Err(Error::NotFound(id.into()));
        }
        self.recent.retain(|b| b != id);
        self.recent.insert(0, id.into());
        self.recent.truncate(64);
        Ok(())
    }
    pub fn reset_brush(&mut self, id: &str) -> Result<()> {
        let b = self
            .brush_mut(id)
            .ok_or_else(|| Error::NotFound(id.into()))?;
        if let Some(point) = b.reset_point {
            b.secondary = b.reset_point_secondary;
            b.combine_mode = b.reset_point_combine_mode;
            b.secondary_shape_asset = b.reset_point_secondary_shape_asset.clone();
            b.secondary_grain_asset = b.reset_point_secondary_grain_asset.clone();
            b.brush = point;
            b.shape_asset = b.reset_point_shape_asset.clone();
            b.grain_asset = b.reset_point_grain_asset.clone();
            return Ok(());
        }
        b.secondary = b.baseline_secondary;
        b.combine_mode = b.baseline_combine_mode;
        b.secondary_shape_asset = b.baseline_secondary_shape_asset.clone();
        b.secondary_grain_asset = b.baseline_secondary_grain_asset.clone();
        b.brush = b.baseline;
        b.shape_asset = b.baseline_shape_asset.clone();
        b.grain_asset = b.baseline_grain_asset.clone();
        Ok(())
    }
    pub fn create_reset_point(&mut self, id: &str) -> Result<()> {
        let b = self
            .brush_mut(id)
            .ok_or_else(|| Error::NotFound(id.into()))?;
        b.reset_point_secondary = b.secondary;
        b.reset_point_combine_mode = b.combine_mode;
        b.reset_point_secondary_shape_asset = b.secondary_shape_asset.clone();
        b.reset_point_secondary_grain_asset = b.secondary_grain_asset.clone();
        b.reset_point = Some(b.brush);
        b.reset_point_shape_asset = b.shape_asset.clone();
        b.reset_point_grain_asset = b.grain_asset.clone();
        Ok(())
    }
    pub fn restore_original(&mut self, id: &str) -> Result<()> {
        let b = self
            .brush_mut(id)
            .ok_or_else(|| Error::NotFound(id.into()))?;
        b.secondary = b.baseline_secondary;
        b.combine_mode = b.baseline_combine_mode;
        b.secondary_shape_asset = b.baseline_secondary_shape_asset.clone();
        b.secondary_grain_asset = b.baseline_secondary_grain_asset.clone();
        b.brush = b.baseline;
        b.shape_asset = b.baseline_shape_asset.clone();
        b.grain_asset = b.baseline_grain_asset.clone();
        Ok(())
    }
    pub fn set_baseline(&mut self, id: &str) -> Result<()> {
        // Retained as an authoring alias: the original baseline remains intact.
        self.create_reset_point(id)
    }
    pub fn remember_tool(&mut self, tool: &str, id: &str, brush: Brush) -> Result<()> {
        if self.brush(id).is_none() {
            return Err(Error::NotFound(id.into()));
        }
        let marks = self
            .tool_memory(tool, id)
            .map(|m| m.marks)
            .unwrap_or_default();
        self.tool_memories.insert(
            format!("{tool}/{id}"),
            ToolMemory {
                brush_id: id.into(),
                brush: brush.sanitized(),
                marks,
            },
        );
        Ok(())
    }
    pub fn tool_memory(&self, tool: &str, id: &str) -> Option<&ToolMemory> {
        self.tool_memories.get(&format!("{tool}/{id}"))
    }
    pub fn save_mark(&mut self, tool: &str, id: &str, index: usize, brush: Brush) -> Result<()> {
        if index >= 4 {
            return Err(Error::Invalid(
                "Brush mark index must be 0 through 3".into(),
            ));
        }
        let brush = brush.sanitized();
        self.remember_tool(tool, id, brush)?;
        self.tool_memories
            .get_mut(&format!("{tool}/{id}"))
            .expect("remembered tool")
            .marks[index] = Some(BrushMark {
            size: brush.size,
            opacity: brush.opacity,
        });
        Ok(())
    }
    pub fn remove_mark(&mut self, tool: &str, id: &str, index: usize) -> Result<()> {
        if index >= 4 {
            return Err(Error::Invalid(
                "Brush mark index must be 0 through 3".into(),
            ));
        }
        if self.brush(id).is_none() {
            return Err(Error::NotFound(id.into()));
        }
        if let Some(memory) = self.tool_memories.get_mut(&format!("{tool}/{id}")) {
            memory.marks[index] = None;
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(Error::Invalid(format!(
                "unsupported schema {}",
                self.schema_version
            )));
        }
        let mut ids = HashSet::new();
        for (id, label) in self
            .libraries
            .iter()
            .map(|l| (&l.id, &l.name))
            .chain(self.sets.iter().map(|s| (&s.id, &s.name)))
            .chain(self.brushes.iter().map(|b| (&b.id, &b.name)))
        {
            if id.is_empty() || !ids.insert(id) {
                return Err(Error::Invalid(format!("duplicate or empty ID {id}")));
            }
            name(label)?;
        }
        for s in &self.sets {
            if !self.libraries.iter().any(|l| l.id == s.library_id) {
                return Err(Error::Invalid(format!("orphan set {}", s.id)));
            }
        }
        for b in &self.brushes {
            if !self.sets.iter().any(|s| s.id == b.set_id) {
                return Err(Error::Invalid(format!("orphan brush {}", b.id)));
            }
            if b.brush != b.brush.sanitized() || b.baseline != b.baseline.sanitized() {
                return Err(Error::Invalid(format!("invalid settings for {}", b.name)));
            }
            if [
                b.reset_point,
                b.secondary,
                b.baseline_secondary,
                b.reset_point_secondary,
            ]
            .into_iter()
            .flatten()
            .any(|p| p != p.sanitized())
            {
                return Err(Error::Invalid("invalid component or reset point".into()));
            }
            for asset in b.asset_refs() {
                if asset.len() != 64 || !asset.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return Err(Error::Invalid("invalid asset digest".into()));
                }
            }
        }
        for list in [&self.pinned, &self.recent] {
            let mut seen = HashSet::new();
            for id in list {
                if self.brush(id).is_none() || !seen.insert(id) {
                    return Err(Error::Invalid("invalid brush reference".into()));
                }
            }
        }
        for memory in self.tool_memories.values() {
            if memory.marks.iter().flatten().any(|mark| {
                !mark.size.is_finite()
                    || !mark.opacity.is_finite()
                    || !(1.0..=1000.0).contains(&mark.size)
                    || !(0.01..=1.0).contains(&mark.opacity)
            }) {
                return Err(Error::Invalid("invalid brush mark".into()));
            }
            if self.brush(&memory.brush_id).is_none() || memory.brush != memory.brush.sanitized() {
                return Err(Error::Invalid("invalid tool memory".into()));
            }
        }
        Ok(())
    }
}

impl BrushDefinition {
    pub fn asset_refs(&self) -> impl Iterator<Item = &String> {
        [
            &self.shape_asset,
            &self.grain_asset,
            &self.baseline_shape_asset,
            &self.baseline_grain_asset,
            &self.reset_point_shape_asset,
            &self.reset_point_grain_asset,
            &self.secondary_shape_asset,
            &self.secondary_grain_asset,
            &self.baseline_secondary_shape_asset,
            &self.baseline_secondary_grain_asset,
            &self.reset_point_secondary_shape_asset,
            &self.reset_point_secondary_grain_asset,
        ]
        .into_iter()
        .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dual_reset_keeps_original_and_marks_survive_tool_updates() {
        let mut c = Catalog::builtin();
        let id = c.add_brush(USER_SET, "Dual", Brush::default()).unwrap();
        let secondary = Brush {
            size: 65.,
            ..Brush::default()
        };
        c.brush_mut(&id).unwrap().secondary = Some(secondary);
        c.brush_mut(&id).unwrap().combine_mode = DualBlend::Screen;
        c.create_reset_point(&id).unwrap();
        c.brush_mut(&id).unwrap().secondary = None;
        c.reset_brush(&id).unwrap();
        assert_eq!(c.brush(&id).unwrap().secondary, Some(secondary));
        assert_eq!(c.brush(&id).unwrap().combine_mode, DualBlend::Screen);
        c.restore_original(&id).unwrap();
        assert!(c.brush(&id).unwrap().secondary.is_none());
        c.save_mark("paint", &id, 2, secondary).unwrap();
        c.remember_tool("paint", &id, Brush::default()).unwrap();
        assert_eq!(
            c.tool_memory("paint", &id).unwrap().marks[2].unwrap().size,
            65.
        );
        assert!(c.tool_memory("erase", &id).is_none());
        assert!(c.save_mark("paint", &id, 4, secondary).is_err());
        c.remove_mark("paint", &id, 2).unwrap();
        assert!(c.tool_memory("paint", &id).unwrap().marks[2].is_none());
        c.validate().unwrap();
    }
    #[test]
    fn combine_uncombine_preserves_sources_and_stable_originals() {
        let mut c = Catalog::builtin();
        let primary = c.add_brush(USER_SET, "Primary", Brush::default()).unwrap();
        let secondary = c
            .add_brush(
                USER_SET,
                "Secondary",
                Brush {
                    size: 27.,
                    ..Brush::default()
                },
            )
            .unwrap();
        c.brush_mut(&secondary).unwrap().shape_asset = Some("a".repeat(64));
        let originals = c.brushes.clone();
        let combined = c.combine_brushes(&primary, &secondary).unwrap();
        assert_eq!(&c.brushes[..originals.len()], originals.as_slice());
        let b = c.brush(&combined).unwrap();
        assert_eq!(b.secondary.unwrap().size, 27.);
        assert_eq!(b.baseline_secondary_shape_asset, Some("a".repeat(64)));
        let (p, s) = c.uncombine_brush(&combined).unwrap();
        assert_eq!(c.brush(&p).unwrap().brush, c.brush(&primary).unwrap().brush);
        assert_eq!(
            c.brush(&s).unwrap().brush,
            c.brush(&secondary).unwrap().brush
        );
        assert_eq!(c.brush(&s).unwrap().shape_asset, Some("a".repeat(64)));
        assert!(c.brush(&combined).is_some());
        assert!(c.combine_brushes(&combined, &primary).is_err());
        c.validate().unwrap();
    }
    #[test]
    fn migration_preserves_duplicates_and_all_rows() {
        let p = BrushPreset {
            name: "same".into(),
            category: "Mine".into(),
            note: "legacy".into(),
            brush: Brush::default(),
        };
        let c = Catalog::migrate_legacy(vec![p; 450]);
        c.validate().unwrap();
        assert_eq!(c.brushes.iter().filter(|b| !b.builtin).count(), 450);
        assert!(c.brush("brush:legacy:449").is_some());
        assert_eq!(
            serde_json::from_str::<Catalog>(&serde_json::to_string(&c).unwrap()).unwrap(),
            c
        );
    }
    #[test]
    fn moves_keep_identity_and_delete_cleans_references() {
        let mut c = Catalog::builtin();
        let a = c.add_brush(USER_SET, "a", Brush::default()).unwrap();
        let b = c.duplicate_brush(&a, USER_SET).unwrap();
        let set = c.create_set(USER_LIBRARY, "Other").unwrap();
        c.pin(&a, true).unwrap();
        c.record_use(&a).unwrap();
        c.remember_tool("erase", &a, Brush::default()).unwrap();
        c.move_brush(&a, &set, 0).unwrap();
        c.rename_brush(&a, "Renamed").unwrap();
        assert_eq!(c.brush(&a).unwrap().set_id, set);
        assert!(c.brush(&b).is_some());
        c.delete_set(&set).unwrap();
        assert!(c.pinned.is_empty());
        assert!(c.recent.is_empty());
        assert!(c.tool_memories.is_empty());
        c.validate().unwrap();
    }
    #[test]
    fn reset_restores_assets_and_tool_memories_are_independent() {
        let mut c = Catalog::builtin();
        let id = c.add_brush(USER_SET, "Custom", Brush::default()).unwrap();
        c.brush_mut(&id).unwrap().shape_asset = Some("a".repeat(64));
        c.set_baseline(&id).unwrap();
        c.brush_mut(&id).unwrap().shape_asset = None;
        c.brush_mut(&id).unwrap().brush.size = 100.;
        c.remember_tool(
            "paint",
            &id,
            Brush {
                size: 22.,
                ..Brush::default()
            },
        )
        .unwrap();
        c.remember_tool(
            "erase",
            &id,
            Brush {
                size: 44.,
                ..Brush::default()
            },
        )
        .unwrap();
        c.reset_brush(&id).unwrap();
        assert_eq!(c.brush(&id).unwrap().shape_asset, Some("a".repeat(64)));
        assert_eq!(c.tool_memory("paint", &id).unwrap().brush.size, 22.);
        assert_eq!(c.tool_memory("erase", &id).unwrap().brush.size, 44.);
    }
}
