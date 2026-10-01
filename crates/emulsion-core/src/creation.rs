//! Validated document creation, shared by native UI and headless callers.
use crate::{Command, Document, Node, NodeKind, command::Slot};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanvasKind {
    #[default]
    Photo,
    Paint,
    Design,
    Diagram,
    Storyboard,
}

impl CanvasKind {
    pub const ALL: [Self; 5] = [
        Self::Photo,
        Self::Paint,
        Self::Design,
        Self::Diagram,
        Self::Storyboard,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Photo => "Photo",
            Self::Paint => "Paint",
            Self::Design => "Design",
            Self::Diagram => "Diagram",
            Self::Storyboard => "Storyboard",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    #[default]
    Pixels,
    Millimeters,
    Inches,
}

impl Unit {
    pub const ALL: [Self; 3] = [Self::Pixels, Self::Millimeters, Self::Inches];
    pub fn label(self) -> &'static str {
        match self {
            Self::Pixels => "px",
            Self::Millimeters => "mm",
            Self::Inches => "in",
        }
    }
    pub fn pixels_per_unit(self, resolution: f64) -> f64 {
        match self {
            Self::Pixels => 1.,
            Self::Millimeters => resolution / 25.4,
            Self::Inches => resolution,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Background {
    #[default]
    White,
    Transparent,
    Black,
    Paper,
}

impl Background {
    pub const ALL: [Self; 4] = [Self::White, Self::Transparent, Self::Black, Self::Paper];
    pub fn label(self) -> &'static str {
        match self {
            Self::White => "White",
            Self::Transparent => "Transparent",
            Self::Black => "Black",
            Self::Paper => "Paper",
        }
    }
    pub fn rgba(self) -> Option<[u8; 4]> {
        match self {
            Self::White => Some([255; 4]),
            Self::Transparent => None,
            Self::Black => Some([0, 0, 0, 255]),
            Self::Paper => Some([244, 241, 234, 255]),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CanvasSpec {
    pub name: String,
    pub kind: CanvasKind,
    pub width: f64,
    pub height: f64,
    pub unit: Unit,
    pub resolution: f64,
    pub depth: u8,
    pub background: Background,
    pub pages: usize,
    pub bleed_mm: f64,
}

impl Default for CanvasSpec {
    fn default() -> Self {
        Self {
            name: "Untitled photo".into(),
            kind: CanvasKind::Photo,
            width: 1920.,
            height: 1080.,
            unit: Unit::Pixels,
            resolution: 72.,
            depth: 16,
            background: Background::White,
            pages: 1,
            bleed_mm: 0.,
        }
    }
}

impl CanvasSpec {
    /// Validate before rounding/casting or allocating a pixel buffer.
    pub fn pixel_size(&self) -> Result<(u32, u32), String> {
        if !self.resolution.is_finite() || !(1. ..=9600.).contains(&self.resolution) {
            return Err("Resolution must be between 1 and 9600 ppi.".into());
        }
        if ![8, 16].contains(&self.depth) {
            return Err("Choose 8-bit or 16-bit color.".into());
        }
        let scale = self.unit.pixels_per_unit(self.resolution);
        let w = self.width * scale;
        let h = self.height * scale;
        if !w.is_finite()
            || !h.is_finite()
            || w < 1.
            || h < 1.
            || w > crate::document::MAX_SIDE as f64
            || h > crate::document::MAX_SIDE as f64
        {
            return Err("Canvas dimensions must be between 1 and 30,000 pixels.".into());
        }
        let (w, h) = (w.round() as u32, h.round() as u32);
        if u64::from(w) * u64::from(h) > crate::document::MAX_PIXELS {
            return Err("Canvas area must not exceed 400 megapixels.".into());
        }
        Ok((w, h))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.chars().count() > 200
            || self.name.chars().any(char::is_control)
        {
            return Err("Enter a document name of 1–200 characters.".into());
        }
        let (w, h) = self.pixel_size()?;
        if self.pages == 0 || self.pages > crate::project::MAX_PAGES {
            return Err(format!("Choose 1–{} pages.", crate::project::MAX_PAGES));
        }
        if !self.is_project() && self.pages != 1 {
            return Err("Multiple pages require a Design, Diagram or Storyboard project.".into());
        }
        if u64::from(w) * u64::from(h) * self.pages as u64 > crate::project::MAX_PROJECT_PIXELS {
            return Err("Project exceeds the total page area limit.".into());
        }
        if !self.bleed_mm.is_finite() || !(0. ..=100.).contains(&self.bleed_mm) {
            return Err("Bleed must be between 0 and 100 mm.".into());
        }
        Ok(())
    }

    /// Whether this kind creates a multi-page project.
    pub fn is_project(&self) -> bool {
        matches!(
            self.kind,
            CanvasKind::Design | CanvasKind::Diagram | CanvasKind::Storyboard
        )
    }

    pub fn create_project(&self) -> Result<crate::project::ProjectEditor, String> {
        self.create_project_with(&crate::storyboard::Preferences::default())
    }

    /// `create_project`, with a storyboard starting from `preferences`.
    pub fn create_project_with(
        &self,
        preferences: &crate::storyboard::Preferences,
    ) -> Result<crate::project::ProjectEditor, String> {
        use crate::project::{PageMeta, Project, ProjectEditor, ProjectKind, ProjectPage};
        use crate::storyboard::{Settings, Storyboard};
        preferences.validate()?;
        let kind = match self.kind {
            CanvasKind::Design => ProjectKind::Design,
            CanvasKind::Diagram => ProjectKind::Diagram,
            CanvasKind::Storyboard => ProjectKind::Storyboard,
            _ => return Err("Choose Design, Diagram or Storyboard for a page project.".into()),
        };
        let doc = self.create()?;
        let graph = crate::Editor::new(doc.clone(), None).graph;
        let page_name = |id: usize| {
            if kind == ProjectKind::Storyboard {
                preferences.naming.panel_name(id)
            } else {
                format!("Page {id}")
            }
        };
        let ids: Vec<u64> = (1..=self.pages as u64).collect();
        let storyboard = (kind == ProjectKind::Storyboard).then(|| {
            Storyboard::with_preferences(Settings::new(doc.width, doc.height), &ids, preferences)
        });
        ProjectEditor::open(
            Project {
                kind,
                storyboard,
                active: 1,
                next_page_id: self.pages as u64 + 1,
                pages: (1..=self.pages)
                    .map(|id| ProjectPage {
                        meta: PageMeta {
                            id: id as u64,
                            name: page_name(id),
                            bleed_mm: self.bleed_mm,
                        },
                        doc: doc.clone(),
                        graph: graph.clone(),
                    })
                    .collect(),
            },
            None,
        )
    }

    /// Change display units without changing the physical canvas dimensions.
    pub fn convert_unit(&mut self, unit: Unit) -> Result<(), String> {
        self.pixel_size()?;
        let ratio =
            self.unit.pixels_per_unit(self.resolution) / unit.pixels_per_unit(self.resolution);
        self.width *= ratio;
        self.height *= ratio;
        self.unit = unit;
        Ok(())
    }

    pub fn create(&self) -> Result<Document, String> {
        self.validate()?;
        let (w, h) = self.pixel_size()?;
        let mut doc = Document::new(w, h);
        doc.resolution = self.resolution as f32;
        doc.source_depth = self.depth;
        let first = match self.background.rgba() {
            Some(rgba) => Node::new(0, "Background", NodeKind::Fill { rgba }),
            None => Node::raster(
                0,
                "Layer 1",
                Arc::new(emulsion_raster::Raster::transparent(w, h)),
                Default::default(),
            ),
        };
        Command::AddNode {
            node: Box::new(first),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .map_err(|e| e.to_string())?;
        doc.validate().map_err(|e| e.to_string())?;
        Ok(doc)
    }

    /// Dense RGBA16 storage used by the engine, irrespective of export depth.
    pub fn layer_bytes(&self) -> Result<u64, String> {
        self.pixel_size()
            .map(|(w, h)| u64::from(w) * u64::from(h) * 8)
    }
}

pub struct CanvasPreset {
    pub category: &'static str,
    pub name: &'static str,
    pub width: f64,
    pub height: f64,
    pub unit: Unit,
    pub resolution: f64,
}

impl CanvasPreset {
    pub fn apply(&self, spec: &mut CanvasSpec) {
        spec.width = self.width;
        spec.height = self.height;
        spec.unit = self.unit;
        spec.resolution = self.resolution;
    }
}

macro_rules! preset {
    ($category:literal, $name:literal, $w:literal, $h:literal, $unit:ident, $ppi:literal) => {
        CanvasPreset {
            category: $category,
            name: $name,
            width: $w,
            height: $h,
            unit: Unit::$unit,
            resolution: $ppi,
        }
    };
}

pub const PHOTO_PRESETS: &[CanvasPreset] = &[
    preset!("Photo", "Photo · 3:2", 6000., 4000., Pixels, 300.),
    preset!("Photo", "Square", 4000., 4000., Pixels, 300.),
    preset!("Photo", "Portrait · 4:5", 3200., 4000., Pixels, 300.),
    preset!("Photo", "Panorama", 9000., 3000., Pixels, 300.),
    preset!("Screen", "Full HD", 1920., 1080., Pixels, 72.),
    preset!("Screen", "4K UHD", 3840., 2160., Pixels, 72.),
    preset!("Screen", "Mobile", 1170., 2532., Pixels, 72.),
    preset!("Print", "A4", 210., 297., Millimeters, 300.),
    preset!("Print", "A3", 297., 420., Millimeters, 300.),
    preset!("Print", "US Letter", 8.5, 11., Inches, 300.),
    preset!("Print", "Postcard", 6., 4., Inches, 300.),
    preset!("Print", "Poster · 18 × 24", 18., 24., Inches, 300.),
    preset!("Film", "DCI 2K", 2048., 1080., Pixels, 72.),
    preset!("Film", "DCI 4K", 4096., 2160., Pixels, 72.),
];

pub const PAINT_PRESETS: &[CanvasPreset] = &[
    preset!("Canvas", "Square canvas", 3000., 3000., Pixels, 300.),
    preset!("Canvas", "Sketchbook", 2480., 3508., Pixels, 300.),
    preset!("Canvas", "Wide canvas", 4000., 2500., Pixels, 300.),
    preset!("Canvas", "Comic page", 2480., 3720., Pixels, 600.),
    preset!("Print", "A4", 210., 297., Millimeters, 300.),
    preset!("Print", "A3", 297., 420., Millimeters, 300.),
    preset!("Print", "US Letter", 8.5, 11., Inches, 300.),
    preset!("Animation", "HD frame", 1920., 1080., Pixels, 72.),
    preset!("Animation", "Square frame", 1080., 1080., Pixels, 72.),
];

pub const DESIGN_PRESETS: &[CanvasPreset] = &[
    preset!("Social", "Square post", 1080., 1080., Pixels, 72.),
    preset!("Social", "Portrait post", 1080., 1350., Pixels, 72.),
    preset!("Social", "Story", 1080., 1920., Pixels, 72.),
    preset!("Presentation", "Widescreen", 1920., 1080., Pixels, 72.),
    preset!("Presentation", "Classic", 1024., 768., Pixels, 72.),
    preset!("Print", "A4 flyer", 210., 297., Millimeters, 300.),
    preset!("Print", "Business card", 3.5, 2., Inches, 300.),
    preset!("Print", "Poster", 18., 24., Inches, 300.),
    preset!("Web", "Banner", 1600., 400., Pixels, 72.),
    preset!("Web", "Video thumbnail", 1280., 720., Pixels, 72.),
];

pub const DIAGRAM_PRESETS: &[CanvasPreset] = &[
    preset!("Diagram", "Flowchart", 1600., 1000., Pixels, 72.),
    preset!("Diagram", "Mind map", 1920., 1080., Pixels, 72.),
    preset!("Diagram", "Architecture", 2400., 1600., Pixels, 72.),
    preset!("Print", "A4 landscape", 297., 210., Millimeters, 150.),
];
pub const STORYBOARD_PRESETS: &[CanvasPreset] = &[
    preset!("Video", "HD · 16:9", 1920., 1080., Pixels, 72.),
    preset!("Video", "4K UHD · 16:9", 3840., 2160., Pixels, 72.),
    preset!("Video", "Classic · 4:3", 1440., 1080., Pixels, 72.),
    preset!("Film", "Flat · 1.85:1", 1998., 1080., Pixels, 72.),
    preset!("Film", "Scope · 2.39:1", 2048., 858., Pixels, 72.),
    preset!("Social", "Vertical · 9:16", 1080., 1920., Pixels, 72.),
    preset!("Social", "Square · 1:1", 1080., 1080., Pixels, 72.),
];

pub fn presets(kind: CanvasKind) -> &'static [CanvasPreset] {
    match kind {
        CanvasKind::Photo => PHOTO_PRESETS,
        CanvasKind::Paint => PAINT_PRESETS,
        CanvasKind::Design => DESIGN_PRESETS,
        CanvasKind::Diagram => DIAGRAM_PRESETS,
        CanvasKind::Storyboard => STORYBOARD_PRESETS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_dimensions_convert_without_drift_and_create_editable_background() {
        let mut spec = CanvasSpec::default();
        PHOTO_PRESETS
            .iter()
            .find(|p| p.name == "A4")
            .unwrap()
            .apply(&mut spec);
        assert_eq!(spec.pixel_size().unwrap(), (2480, 3508));
        for _ in 0..50 {
            spec.convert_unit(Unit::Inches).unwrap();
            spec.convert_unit(Unit::Pixels).unwrap();
            spec.convert_unit(Unit::Millimeters).unwrap();
        }
        assert_eq!(spec.pixel_size().unwrap(), (2480, 3508));
        let doc = spec.create().unwrap();
        assert_eq!(doc.resolution, 300.);
        assert!(matches!(doc.nodes[0].kind, NodeKind::Fill { .. }));
        assert_eq!(spec.layer_bytes().unwrap(), 2480 * 3508 * 8);
    }

    #[test]
    fn invalid_inputs_fail_before_allocation() {
        for value in [f64::NAN, f64::INFINITY, -1., 0., 0.1, 30_001.] {
            assert!(
                CanvasSpec {
                    width: value,
                    ..Default::default()
                }
                .create()
                .is_err()
            );
        }
        assert!(
            CanvasSpec {
                width: 30_000.,
                height: 30_000.,
                ..Default::default()
            }
            .create()
            .is_err()
        );
        assert!(
            CanvasSpec {
                resolution: 0.,
                ..Default::default()
            }
            .create()
            .is_err()
        );
        assert!(
            CanvasSpec {
                name: "  ".into(),
                ..Default::default()
            }
            .create()
            .is_err()
        );
        assert!(
            CanvasSpec {
                depth: 32,
                ..Default::default()
            }
            .create()
            .is_err()
        );
    }

    #[test]
    fn transparent_creation_and_all_builtin_presets_are_valid() {
        let doc = CanvasSpec {
            width: 16.,
            height: 12.,
            background: Background::Transparent,
            ..Default::default()
        }
        .create()
        .unwrap();
        let NodeKind::Raster { raster, .. } = &doc.nodes[0].kind else {
            panic!("editable paint layer")
        };
        assert_eq!(raster.get(8, 6), [0; 4]);
        for kind in CanvasKind::ALL {
            for preset in presets(kind) {
                let mut spec = CanvasSpec::default();
                preset.apply(&mut spec);
                spec.validate().unwrap();
            }
        }
    }

    #[test]
    fn storyboard_specs_create_valid_storyboard_projects() {
        let spec = CanvasSpec {
            name: "Pilot".into(),
            kind: CanvasKind::Storyboard,
            width: 320.,
            height: 180.,
            pages: 3,
            ..Default::default()
        };
        let project = spec.create_project().unwrap();
        let board = project.storyboard().unwrap();
        assert_eq!(board.panels.len(), 3);
        assert_eq!((board.settings.width, board.settings.height), (320, 180));
        assert_eq!(project.page_list()[0].name, "Panel 1");
        project.snapshot().unwrap().validate().unwrap();
    }
}
