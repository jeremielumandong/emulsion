//! Portable, exact adjustment stages captured from a document.
use crate::{Recipe, RecipeError};
use emulsion_core::{Document, Node, NodeId, NodeKind};
use emulsion_raster::{Adjustment, BlendMode};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSettings {
    pub name: String,
    pub opacity: f32,
    pub blend: BlendMode,
    pub visible: bool,
}
impl NodeSettings {
    fn from_node(n: &Node) -> Self {
        Self {
            name: n.name.clone(),
            opacity: n.opacity,
            blend: n.blend,
            visible: n.visible,
        }
    }
    fn apply(&self, n: &mut Node) {
        n.name = self.name.clone();
        n.opacity = self.opacity;
        n.blend = self.blend;
        n.visible = self.visible;
    }
    fn validate(&self) -> Result<(), RecipeError> {
        if self.name.chars().count() > 512
            || !self.opacity.is_finite()
            || !(0.0..=1.0).contains(&self.opacity)
        {
            return Err(invalid("invalid workflow node name or opacity"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub settings: NodeSettings,
    pub adjustment: Adjustment,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub version: u32,
    pub group: NodeSettings,
    /// Native stack order: bottom to top. LUTs contain their data, not file paths.
    pub stages: Vec<Stage>,
}

fn invalid(message: impl Into<String>) -> RecipeError {
    RecipeError::Invalid(message.into())
}

pub(crate) fn validate_cube(cube: &emulsion_raster::adjust::Cube) -> Result<(), RecipeError> {
    if !(2..=128).contains(&cube.size) || cube.data.len() != (cube.size as usize).pow(3) {
        return Err(invalid(
            "LUT must contain exactly size³ entries, with size 2–128",
        ));
    }
    Ok(())
}

fn validate_adjustment(a: &Adjustment) -> Result<(), RecipeError> {
    for p in a.params() {
        if !p.value.is_finite() || p.value < p.min || p.value > p.max {
            return Err(invalid(format!(
                "{} {} must be within {}–{}",
                a.label(),
                p.key,
                p.min,
                p.max
            )));
        }
    }
    match a {
        Adjustment::Curves {
            master,
            red,
            green,
            blue,
        } => {
            for points in [master, red, green, blue] {
                if !(2..=4096).contains(&points.len())
                    || points
                        .iter()
                        .flatten()
                        .any(|v| !v.is_finite() || !(0.0..=255.0).contains(v))
                    || points.windows(2).any(|p| p[0][0] >= p[1][0])
                {
                    return Err(invalid(
                        "curves require 2–4096 ordered distinct input points within 0–255",
                    ));
                }
            }
        }
        Adjustment::GradientMap { stops, .. } => {
            if !(2..=4096).contains(&stops.len())
                || stops
                    .iter()
                    .any(|s| !s.pos.is_finite() || !(0.0..=1.0).contains(&s.pos))
                || stops.windows(2).any(|s| s[0].pos > s[1].pos)
            {
                return Err(invalid(
                    "gradient map requires 2–4096 ordered stops within 0–1",
                ));
            }
        }
        Adjustment::Lut3D { cube, .. } => validate_cube(cube)?,
        Adjustment::Levels {
            in_black, in_white, ..
        } if in_black >= in_white => {
            return Err(invalid("levels input white must exceed input black"));
        }
        _ => {}
    }
    Ok(())
}

impl Workflow {
    pub fn validate(&self) -> Result<(), RecipeError> {
        if self.version != VERSION {
            return Err(invalid(format!(
                "unsupported workflow version {}",
                self.version
            )));
        }
        if !(1..=256).contains(&self.stages.len()) {
            return Err(invalid("workflow requires 1–256 adjustment stages"));
        }
        self.group.validate()?;
        for s in &self.stages {
            s.settings.validate()?;
            validate_adjustment(&s.adjustment)?;
        }
        Ok(())
    }
    pub(crate) fn compile(&self) -> crate::Compiled {
        let mut group = Node::group(0, &self.group.name);
        self.group.apply(&mut group);
        let children = self
            .stages
            .iter()
            .map(|s| {
                let mut n = Node::adjust(0, s.adjustment.clone());
                s.settings.apply(&mut n);
                n
            })
            .collect();
        (group, children)
    }
}

/// Capture a flat adjustment group or one adjustment. Exclusions are direct stage
/// IDs, never arbitrary document IDs. Unsupported structure is never discarded.
pub fn capture_adjustments(
    doc: &Document,
    id: NodeId,
    name: &str,
    excluded: &[NodeId],
) -> Result<Recipe, RecipeError> {
    let root = doc
        .node(id)
        .ok_or_else(|| invalid("selected node no longer exists"))?;
    let ids = match root.kind {
        NodeKind::Group { .. } => doc.children(Some(id)),
        NodeKind::Adjust(_) => vec![id],
        _ => return Err(invalid("select an adjustment or a flat adjustment group")),
    };
    let omitted: HashSet<_> = excluded.iter().copied().collect();
    if omitted.len() != excluded.len() || omitted.iter().any(|id| !ids.contains(id)) {
        return Err(invalid(
            "excluded stages must be distinct direct adjustment IDs in this capture",
        ));
    }
    let captured: HashSet<_> = ids.iter().copied().chain([id]).collect();
    for n in doc.nodes.iter().filter(|n| captured.contains(&n.id)) {
        if n.mask.is_some() || n.clip_to.is_some() || !n.styles.is_empty() {
            return Err(invalid(format!(
                "{} has a mask, clipping or layer styles; these cannot be saved as an adjustment recipe",
                n.name
            )));
        }
        if (n.id != id || !root.is_group()) && !matches!(n.kind, NodeKind::Adjust(_)) {
            return Err(invalid(format!(
                "{} is not an adjustment; nested groups and pixel content are unsupported",
                n.name
            )));
        }
    }
    if doc
        .nodes
        .iter()
        .any(|n| !captured.contains(&n.id) && n.clip_to.is_some_and(|id| captured.contains(&id)))
    {
        return Err(invalid(
            "another layer clips to this capture; remove that clipping relationship first",
        ));
    }
    let group = if root.is_group() {
        NodeSettings::from_node(root)
    } else {
        NodeSettings {
            name: format!("Recipe · {name}"),
            opacity: 1.0,
            blend: BlendMode::PassThrough,
            visible: true,
        }
    };
    let stages = ids
        .iter()
        .filter(|id| !omitted.contains(id))
        .map(|id| {
            let n = doc.node(*id).expect("direct child exists");
            let NodeKind::Adjust(a) = &n.kind else {
                unreachable!()
            };
            Stage {
                settings: NodeSettings::from_node(n),
                adjustment: a.clone(),
            }
        })
        .collect();
    let r = Recipe {
        name: name.trim().into(),
        workflow: Some(Workflow {
            version: VERSION,
            group,
            stages,
        }),
        ..Recipe::default()
    };
    r.validate()?;
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bundle::Bundle, compile, store};
    use emulsion_core::{Command, Editor, command::Slot};
    use emulsion_raster::{Mask, composite::flatten};
    use std::sync::Arc;

    fn add(ed: &mut Editor, node: Node, parent: Option<NodeId>) -> NodeId {
        ed.execute(Command::AddNode {
            node: Box::new(node),
            slot: Slot {
                parent,
                index: usize::MAX,
            },
        })
        .unwrap()
        .unwrap()
    }
    fn fixture() -> (Editor, NodeId, NodeId) {
        let mut ed = Editor::new(Document::new(8, 6), None);
        add(
            &mut ed,
            Node::new(
                0,
                "Photo",
                NodeKind::Fill {
                    rgba: [80, 110, 140, 255],
                },
            ),
            None,
        );
        let mut group = Node::group(0, "My exact grade");
        group.blend = BlendMode::PassThrough;
        group.opacity = 0.71;
        let gid = add(&mut ed, group, None);
        let mut exposure = Node::adjust(
            0,
            Adjustment::Exposure {
                exposure: 0.6,
                offset: 0.02,
                gamma: 1.15,
            },
        );
        exposure.name = "Base exposure".into();
        exposure.opacity = 0.83;
        let eid = add(&mut ed, exposure, Some(gid));
        let mut color = Node::adjust(
            0,
            Adjustment::HueSaturation {
                hue: 12.0,
                saturation: 18.0,
                lightness: 0.0,
            },
        );
        color.blend = BlendMode::Color;
        add(&mut ed, color, Some(gid));
        let mut hidden = Node::adjust(0, Adjustment::Invert);
        hidden.visible = false;
        add(&mut ed, hidden, Some(gid));
        (ed, gid, eid)
    }

    #[test]
    fn exact_capture_toml_bundle_apply_preserves_pixels_structure_and_undo() {
        let (source, gid, _) = fixture();
        let recipe = capture_adjustments(&source.doc, gid, "Reusable grade", &[]).unwrap();
        let text = Bundle::new("Looks", vec![recipe.clone()]).to_toml();
        let decoded = Bundle::from_toml(&text).unwrap().recipes.remove(0);
        assert_eq!(recipe, decoded);
        let mut target = Editor::new(Document::new(8, 6), None);
        add(&mut target, source.doc.nodes[0].clone(), None);
        let before = flatten(&target.doc.composite_tree(), 0).to_srgba8();
        let new_group = store::add_to(&mut target, compile(&decoded).unwrap(), Slot::TOP).unwrap();
        assert_eq!(
            flatten(&source.doc.composite_tree(), 0).to_srgba8(),
            flatten(&target.doc.composite_tree(), 0).to_srgba8()
        );
        assert_eq!(target.doc.node(new_group).unwrap().opacity, 0.71);
        let children = target.doc.children(Some(new_group));
        assert_eq!(children.len(), 3);
        assert!(!target.doc.node(children[2]).unwrap().visible);
        assert!(target.undo());
        assert_eq!(target.doc.nodes.len(), 1);
        assert_eq!(flatten(&target.doc.composite_tree(), 0).to_srgba8(), before);
    }

    #[test]
    fn subsets_are_explicit_and_unsupported_content_is_rejected() {
        let (mut ed, gid, eid) = fixture();
        let r = capture_adjustments(&ed.doc, gid, "Subset", &[eid]).unwrap();
        assert_eq!(r.workflow.unwrap().stages.len(), 2);
        assert!(capture_adjustments(&ed.doc, gid, "Bad", &[999]).is_err());
        assert!(capture_adjustments(&ed.doc, gid, "Bad", &[eid, eid]).is_err());
        assert!(capture_adjustments(&ed.doc, eid, "Empty", &[eid]).is_err());
        let single = capture_adjustments(&ed.doc, eid, "Exposure", &[]).unwrap();
        assert_eq!(single.workflow.unwrap().stages.len(), 1);
        ed.doc.node_mut(eid).unwrap().mask = Some(Arc::new(Mask::white(8, 6)));
        ed.doc.node_mut(eid).unwrap().mask_enabled = false;
        assert!(capture_adjustments(&ed.doc, gid, "Masked", &[]).is_err());
        ed.doc.node_mut(eid).unwrap().mask = None;
        let external = add(&mut ed, Node::adjust(0, Adjustment::Invert), None);
        ed.doc.node_mut(external).unwrap().clip_to = Some(gid);
        assert!(capture_adjustments(&ed.doc, gid, "External clip", &[]).is_err());
        ed.doc.node_mut(external).unwrap().clip_to = None;
        add(&mut ed, Node::group(0, "Nested"), Some(gid));
        assert!(capture_adjustments(&ed.doc, gid, "Nested", &[]).is_err());
    }

    #[test]
    fn corrupt_workflows_fail_before_compile() {
        let (ed, gid, _) = fixture();
        let r = capture_adjustments(&ed.doc, gid, "Good", &[]).unwrap();
        let mut bad = r.clone();
        bad.workflow.as_mut().unwrap().version = 99;
        assert!(Recipe::from_toml(&bad.to_toml()).is_err());
        let mut bad = r.clone();
        bad.workflow.as_mut().unwrap().group.opacity = f32::NAN;
        assert!(compile(&bad).is_err());
        let mut bad = r.clone();
        bad.workflow.as_mut().unwrap().stages[0].adjustment = Adjustment::Exposure {
            exposure: f32::INFINITY,
            offset: 0.0,
            gamma: 1.0,
        };
        assert!(compile(&bad).is_err());
        let mut bad = r.clone();
        bad.workflow.as_mut().unwrap().stages[0].adjustment = Adjustment::Lut3D {
            cube: emulsion_raster::adjust::Cube {
                name: "Corrupt".into(),
                size: 128,
                data: Arc::new(vec![[0; 3]]),
            },
            strength: 100.0,
        };
        assert!(compile(&bad).is_err());
        let mut bad = r.clone();
        bad.workflow.as_mut().unwrap().stages[0].adjustment = Adjustment::Curves {
            master: vec![[255.0, 0.0], [0.0, 255.0]],
            red: vec![],
            green: vec![],
            blue: vec![],
        };
        assert!(compile(&bad).is_err());
        let mut bundle = Bundle::new("Future", vec![r]);
        bundle.version += 1;
        assert!(Bundle::from_toml(&bundle.to_toml()).is_err());
    }

    #[test]
    fn saved_names_cannot_overwrite_and_luts_travel_without_original_file() {
        let dir =
            std::env::temp_dir().join(format!("emulsion-workflow-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lut_path = dir.join("source.cube");
        std::fs::write(
            &lut_path,
            "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n",
        )
        .unwrap();
        let r = Recipe {
            name: "A/B".into(),
            lut: Some(lut_path.to_string_lossy().into()),
            ..Recipe::default()
        };
        let path = store::save_new(&dir, &r).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(store::save_new(&dir, &r).is_err());
        let colliding = Recipe {
            name: "A?B".into(),
            ..Recipe::default()
        };
        assert!(store::save_new(&dir, &colliding).is_err());
        assert!(
            store::save(&dir, &colliding).is_err(),
            "legacy imports must not replace a different sanitized name"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let mut loaded = Recipe::from_toml(std::str::from_utf8(&original).unwrap()).unwrap();
        assert!(loaded.lut.is_none());
        assert!(loaded.embedded_lut.is_some());
        std::fs::remove_file(&lut_path).unwrap();
        assert!(compile(&loaded).is_ok());
        loaded.notes = "Explicit update".into();
        assert_eq!(store::update(&dir, "A/B", &loaded).unwrap(), path);
        assert_eq!(store::find(&dir, "A/B").unwrap().notes, "Explicit update");
        assert!(store::update(&dir, "Missing", &loaded).is_err());
        loaded.notes = "Reimport same logical name".into();
        store::save(&dir, &loaded).unwrap();
        assert_eq!(store::find(&dir, "A/B").unwrap().notes, loaded.notes);
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(
            files,
            vec![path],
            "private staging files are cleaned up after success and collision"
        );
        let future = dir.join("future.recipe.toml");
        std::fs::write(&future, "name = \"future\"\n[workflow]\nversion = 999").unwrap();
        let new_recipe = Recipe {
            name: "future".into(),
            ..Recipe::default()
        };
        assert!(
            store::save(&dir, &new_recipe).is_err(),
            "unreadable existing recipe must not be overwritten"
        );
        assert!(std::fs::read_to_string(future).unwrap().contains("999"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
