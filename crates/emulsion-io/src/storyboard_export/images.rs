//! Panel pictures as PNG or JPEG files named by a pattern, one per panel or
//! one per top-level layer of each panel. Files are written with the shared
//! image export.
use super::{Entry, PANEL_TOKENS, Scope, entries, expand, panel_token, select, validate_pattern};
use crate::export::{ExportOptions, export};
use anyhow::{Context, Result, bail};
use emulsion_core::{Document, NodeId, project::Project};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[default]
    Png,
    Jpeg,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub format: Format,
    /// File name without extension, with panel tokens and, per layer,
    /// {layer}. `{index:3}` pads numbers with zeros.
    pub pattern: String,
    /// One image per visible top-level layer of each panel.
    pub per_layer: bool,
    pub jpeg_quality: u8,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            format: Format::Png,
            pattern: "{seq}_{scene}_{panel}".into(),
            per_layer: false,
            jpeg_quality: 92,
        }
    }
}

/// Most files one export writes.
pub const MAX_FILES: usize = 5000;

impl Options {
    pub fn validate(&self) -> Result<()> {
        let mut tokens = PANEL_TOKENS.to_vec();
        if self.per_layer {
            tokens.push("layer");
        }
        if self.pattern.trim().is_empty() {
            bail!("Enter a file name pattern, for example {{seq}}_{{scene}}_{{panel}}")
        }
        validate_pattern(&self.pattern, &tokens)?;
        if !(1..=100).contains(&self.jpeg_quality) {
            bail!("JPEG quality must be 1–100")
        }
        Ok(())
    }
}

/// A file name from expanded text: path separators and characters Windows
/// refuses become `_`.
fn file_name(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = cleaned.trim().trim_end_matches('.').trim();
    name.chars().take(150).collect()
}

/// The visible top-level layers of a page, with the layers clipped to each.
fn layers(doc: &Document) -> Vec<(NodeId, String)> {
    let mut seen = std::collections::HashMap::<String, usize>::new();
    doc.children(None)
        .into_iter()
        .filter_map(|id| doc.node(id))
        .filter(|n| n.visible && n.clip_to.is_none())
        .map(|n| {
            // Two layers with one name get " 2", " 3"… so files stay apart.
            let count = seen.entry(n.name.clone()).or_default();
            *count += 1;
            let name = if *count == 1 {
                n.name.clone()
            } else {
                format!("{} {count}", n.name)
            };
            (n.id, name)
        })
        .collect()
}

/// The page with only `layer` (and layers clipped to it) visible.
fn only(doc: &Document, layer: NodeId) -> Document {
    let mut doc = doc.clone();
    for node in doc.nodes.iter_mut().filter(|n| n.parent.is_none()) {
        node.visible = node.visible && (node.id == layer || node.clip_to == Some(layer));
    }
    doc
}

/// One file to write.
struct Planned<'a> {
    entry: &'a Entry,
    layer: Option<(NodeId, String)>,
    path: PathBuf,
}

/// Export the panels of `scope` into `dir`, which is created if needed.
/// Names are checked first: a pattern that gives two files the same name
/// writes nothing. Returns the files written, in board order.
pub fn write(
    project: &Project,
    name: &str,
    scope: &Scope,
    options: &Options,
    dir: &Path,
    cancel: &AtomicBool,
) -> Result<Vec<PathBuf>> {
    options.validate()?;
    let board = super::board(project)?;
    let rate = board.settings.frame_rate;
    let chosen = select(entries(project)?, scope)?;
    let doc = |entry: &Entry| -> Result<&Document> {
        Ok(&project
            .pages
            .iter()
            .find(|p| p.meta.id == entry.page)
            .context("Missing panel page")?
            .doc)
    };
    let mut plan = Vec::new();
    let mut names = HashSet::new();
    for entry in &chosen {
        let layers = if options.per_layer {
            layers(doc(entry)?).into_iter().map(Some).collect()
        } else {
            vec![None]
        };
        for layer in layers {
            let pattern = if options.per_layer && !options.pattern.contains("{layer") {
                format!("{}_{{layer}}", options.pattern)
            } else {
                options.pattern.clone()
            };
            let stem = file_name(&expand(&pattern, |token| match token {
                "layer" => layer.as_ref().map(|(_, n)| n.clone()),
                _ => panel_token(entry, name, rate, token),
            })?);
            if stem.is_empty() {
                bail!("The pattern gives panel {} an empty file name", entry.index)
            }
            let file = format!("{stem}.{}", options.format.extension());
            if !names.insert(file.to_lowercase()) {
                bail!(
                    "Two images would both be named “{file}”. Add {{index}} or {{panel}} to the pattern."
                )
            }
            plan.push(Planned {
                entry,
                layer,
                path: dir.join(file),
            });
        }
    }
    if plan.is_empty() {
        bail!("The chosen panels have no visible layers to export")
    }
    if plan.len() > MAX_FILES {
        bail!("Exports are limited to {MAX_FILES} images; choose fewer panels")
    }
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Cannot create the folder {}", dir.display()))?;
    let mut written = Vec::new();
    for item in plan {
        crate::printing::canceled(cancel)?;
        let page = doc(item.entry)?;
        let picture = match &item.layer {
            Some((layer, _)) => only(page, *layer),
            None => page.clone(),
        };
        let mut opts = ExportOptions::for_doc(&picture);
        opts.depth = 8;
        opts.jpeg_quality = options.jpeg_quality;
        export(&picture, &item.path, opts)
            .with_context(|| format!("Cannot write {}", item.path.display()))?;
        written.push(item.path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storyboard_export::tests::project;
    use emulsion_core::{Command, Node, command::Slot};
    use std::sync::Arc;

    #[test]
    fn names_follow_the_pattern_and_collisions_write_nothing() {
        let project = project();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("frames");
        let cancel = AtomicBool::new(false);
        let options = Options {
            pattern: "{scene:3}_{panel:2} {name}".into(),
            ..Default::default()
        };
        let files = write(&project, "Film", &Scope::All, &options, &out, &cancel).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "001_01 Panel 1.png",
                "001_02 Panel 2.png",
                "002_01 Panel 3.png"
            ]
        );
        let image = image::open(&files[0]).unwrap();
        assert_eq!((image.width(), image.height()), (64, 36));

        let collide = dir.path().join("collide");
        let options = Options {
            pattern: "{scene}/{seq}".into(),
            format: Format::Jpeg,
            ..Default::default()
        };
        assert!(write(&project, "Film", &Scope::All, &options, &collide, &cancel).is_err());
        assert!(!collide.exists(), "nothing is written");
        let options = Options {
            pattern: "{nope}".into(),
            ..Default::default()
        };
        assert!(write(&project, "Film", &Scope::All, &options, &collide, &cancel).is_err());
        assert_eq!(file_name(" a/b:c? . "), "a_b_c_");
    }

    #[test]
    fn per_layer_writes_one_image_per_visible_top_level_layer() {
        let mut project = project();
        let page = &mut project.pages[0].doc;
        for (name, color) in [("Hero", [255, 0, 0, 255]), ("Hero", [0, 0, 255, 255])] {
            Command::AddNode {
                node: Box::new(Node::path(
                    0,
                    name,
                    Arc::new(emulsion_raster::vector_geometry::rectangle(
                        0., 0., 10., 10.,
                    )),
                    emulsion_raster::vector::PathStyle {
                        fill: Some(color),
                        stroke: None,
                        ..Default::default()
                    },
                    64,
                    36,
                )),
                slot: Slot::TOP,
            }
            .apply(page)
            .unwrap();
        }
        let hidden = page.nodes.last().unwrap().id;
        let first = project.pages[0].meta.id;
        let dir = tempfile::tempdir().unwrap();
        let options = Options {
            pattern: "{index}".into(),
            per_layer: true,
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);
        let files = write(
            &project,
            "Film",
            &Scope::Panels(vec![first]),
            &options,
            dir.path(),
            &cancel,
        )
        .unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names.contains(&"1_Hero.png".to_string()));
        assert!(names.contains(&"1_Hero 2.png".to_string()));
        let blue = image::open(dir.path().join("1_Hero 2.png"))
            .unwrap()
            .to_rgba8();
        assert_eq!(blue.get_pixel(2, 2).0, [0, 0, 255, 255]);
        assert_eq!(blue.get_pixel(40, 30).0[3], 0, "other layers are hidden");

        project.pages[0].doc.node_mut(hidden).unwrap().visible = false;
        let again = tempfile::tempdir().unwrap();
        let files = write(
            &project,
            "Film",
            &Scope::Panels(vec![first]),
            &options,
            again.path(),
            &cancel,
        )
        .unwrap();
        assert_eq!(files.len(), 1, "hidden layers are skipped");
        assert!(
            write(
                &project,
                "Film",
                &Scope::All,
                &options,
                again.path(),
                &AtomicBool::new(true)
            )
            .is_err()
        );
    }
}
